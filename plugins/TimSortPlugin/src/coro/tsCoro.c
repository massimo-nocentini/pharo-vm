/* tsCoro.c -- the resumable sort (tsCoro.h): the table of live sorts, their
 * handles and stacks, and the sort side, which runs timsort.c's sort of a
 * permutation through the engine (ts_sort_words of tsEngine.h: this file
 * does not include timsort.h) on a stack of its own (tsCoroSwitch.h).  No
 * VM header.  Compiled natively only (cmake/plugins.cmake); everything here
 * is inside #if TS_CORO_SUPPORTED, so the file compiles to nothing
 * elsewhere.
 *
 * A sort (struct ts_coro) owns its permutation (n int64_t, 0 .. n-1 at the
 * start) and, from ts_coro_open until it finishes, a stack.  On that stack
 * sortMain calls ts_sort_words over the permutation with suspendLess as
 * the comparison, which stores the pair it is asked about, yields to the
 * caller, and answers what the caller answered at the next ts_coro_next.
 * The words the engine sorts are the indices themselves, so a request
 * names two elements, never positions.  The caller side only ever makes
 * plain C calls that return on its own stack, and nothing of the sort runs
 * outside ts_coro_next (and ts_coro_close, which resumes a pending sort
 * with "abort" answers until the engine returns: it frees its merge
 * buffer).
 *
 * The table holds the live sorts densely (live[0 .. liveCount)), so a
 * lookup scans the live ones only (usually one).  Ids count up from a
 * base that each load of the copy draws at its first sort (firstId), and
 * are never reused within that load.  A finished sort gives its stack back at once: kept
 * for the next sort (TS_CORO_STACK_CACHE of them: mapping and unmapping a
 * stack costs microseconds) or freed.  Nothing here is thread-safe: one
 * thread drives every sort of a copy (tsCoro.h). */

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <time.h>

#include "tsCoroSwitch.h"

#if TS_CORO_SUPPORTED

/* The states of a sort */
enum {
	FRESH = 0,	/* opened, not started */
	RUNNING,	/* its stack runs (inside ts_coro_next) */
	PENDING,	/* suspended in a comparison: a request is out */
	FINISHED	/* result says how */
};

struct ts_coro {
	int64_t id;
	int64_t n;
	int64_t *perm;		/* n indices (at least one word, n = 0 included) */
	ts_stack *stack;	/* until the sort finishes (none when n < 2) */
	int descending;
	int state;
	int result;		/* 0 until FINISHED, then TS_DONE, TS_NO_MEMORY or TS_ABORTED */
	int answer;		/* to the pending request: 1, 0, or -1 to abort */
	int64_t i, j;		/* the pending request: lt(i, j)? */
};

static ts_coro *live[TS_CORO_TABLE_SIZE];
static int liveCount;
static int64_t lastId;	/* 0 before the first sort of this load */
static ts_stack *cache[TS_CORO_STACK_CACHE];
static int cached;


/*** The sort side (on the sort's stack) ***/

/* The comparison of ts_sort_words: hands lt(a, b) to the caller and
 * answers its answer */
static int
suspendLess(int64_t a, int64_t b, void *arg)
{
	ts_coro *h = arg;

	h->i = a;
	h->j = b;
	h->state = PENDING;
	ts_stack_yield(h->stack);
	return h->answer;
}

/* One sort: the main of every stack (its loop calls it for each sort the
 * stack is given) */
static void
sortMain(ts_stack *s)
{
	ts_coro *h = s->job;
	int status = ts_sort_words(h->perm, h->n, h->descending, suspendLess, h);

	h->result = status == TS_OK ? TS_DONE : status;
	h->state = FINISHED;
	s->job = NULL;
}


/*** Stacks ***/

static ts_stack *
takeStack(void)
{
	if (cached > 0) {
		return cache[--cached];
	}
	return ts_stack_new(sortMain);
}

/* A stack between two sorts (parked) or never resumed: kept or freed */
static void
giveStack(ts_stack *s)
{
	if (cached < TS_CORO_STACK_CACHE) {
		cache[cached++] = s;
	} else {
		ts_stack_delete(s);
	}
}

size_t
ts_coro_stack_high_water(void)
{
	size_t most = 0, used;
	int k;

	for (k = 0; k < cached; k++) {
		used = ts_stack_used(cache[k]);
		most = used > most ? used : most;
	}
	for (k = 0; k < liveCount; k++) {
		if (live[k]->stack) {
			used = ts_stack_used(live[k]->stack);
			most = used > most ? used : most;
		}
	}
	return most;
}


/*** The caller side ***/

/* The id before the first one of this load: in [2^32, 2^58), drawn from
 * the clocks and from addresses, which differ from one load to the next
 * (a new process, address space layout randomization).  The image keeps
 * ids across a snapshot: a Process saved while it waited in a sort comes
 * back, in the next session, with an id of the earlier load, and with ids
 * counting up from 1 in every load that id would name the first sort of
 * the new one.  With a base of 58 random bits it almost never names a live
 * sort (the image checks the session besides, TimSort class>>blockSort:...),
 * and ids stay below 2^59, SmallIntegers of the image. */
static int64_t
firstId(void)
{
	uint64_t z = (uint64_t)time(NULL) * 0x9E3779B97F4A7C15ull;
	int local = 0;

	z ^= (uint64_t)clock() << 17;
#if defined(CLOCK_REALTIME) && !defined(_WIN32)
	{
		struct timespec now;

		if (clock_gettime(CLOCK_REALTIME, &now) == 0) {
			z ^= (uint64_t)now.tv_nsec * 0xBF58476D1CE4E5B9ull;
		}
	}
#endif
	z ^= (uint64_t)(uintptr_t)&lastId;
	z ^= (uint64_t)(uintptr_t)&local << 11;
	/* splitmix64's finalizer */
	z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
	z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
	z ^= z >> 31;
	return (int64_t)((z & (((uint64_t)1 << 58) - 1)) | ((uint64_t)1 << 32));
}

int
ts_coro_available(void)
{
	return ts_stack_setup(0);
}

const char *
ts_coro_impl(void)
{
	return ts_stack_impl();
}

int64_t
ts_coro_open(int64_t n, int descending, int *status)
{
	ts_coro *h;
	int64_t k;
	int failure;

	if (n < 0 || n > TS_CORO_MAX_SIZE) {
		failure = TS_BAD_ARGUMENT;
		goto failed;
	}
	if (!ts_stack_setup(1)) {
		failure = TS_UNSUPPORTED;
		goto failed;
	}
	if (liveCount >= TS_CORO_TABLE_SIZE) {
		failure = TS_LIMIT;
		goto failed;
	}
	failure = TS_NO_MEMORY;
	h = calloc(1, sizeof *h);
	if (!h) {
		goto failed;
	}
	h->perm = malloc((size_t)(n > 0 ? n : 1) * sizeof(int64_t));
	if (!h->perm) {
		free(h);
		goto failed;
	}
	if (n >= 2 && !(h->stack = takeStack())) {
		free(h->perm);
		free(h);
		goto failed;
	}
	for (k = 0; k < n; k++) {
		h->perm[k] = k;
	}
	h->n = n;
	h->descending = descending != 0;
	h->state = FRESH;
	if (lastId == 0) {
		lastId = firstId();
	}
	h->id = ++lastId;
	live[liveCount++] = h;
	if (status) {
		*status = TS_OK;
	}
	return h->id;

failed:
	if (status) {
		*status = failure;
	}
	return 0;
}

ts_coro *
ts_coro_lookup(int64_t id)
{
	int k;

	if (id < 1) {
		return NULL;
	}
	for (k = liveCount - 1; k >= 0; k--) {
		if (live[k]->id == id) {
			return live[k];
		}
	}
	return NULL;
}

int64_t
ts_coro_next(ts_coro *h, int64_t answer)
{
	if (!h) {
		return TS_BAD_ARGUMENT;
	}
	switch (h->state) {
	case FINISHED:
		return h->result;
	case PENDING:
		h->answer = answer < 0 ? -1 : answer > 0;
		break;
	case FRESH:
		if (h->n < 2) {
			h->result = TS_DONE;
			h->state = FINISHED;
			return TS_DONE;
		}
		h->stack->job = h;
		break;
	default:	/* RUNNING: called from the sort's own comparison, which the
			   sort side never does */
		return TS_BAD_ARGUMENT;
	}
	h->state = RUNNING;
	ts_stack_resume(h->stack);
	if (h->state == PENDING) {
		return (h->i << 32) | h->j;
	}
	giveStack(h->stack);
	h->stack = NULL;
	return h->result;
}

int
ts_coro_result(const ts_coro *h)
{
	return h && h->state == FINISHED ? h->result : 0;
}

int64_t
ts_coro_size(const ts_coro *h)
{
	return h ? h->n : 0;
}

const int64_t *
ts_coro_permutation(const ts_coro *h)
{
	return h ? h->perm : NULL;
}

int64_t
ts_coro_live(void)
{
	return liveCount;
}

int
ts_coro_close(int64_t id)
{
	ts_coro *h = NULL;
	int k;

	if (id >= 1) {
		for (k = 0; k < liveCount; k++) {
			if (live[k]->id == id) {
				h = live[k];
				break;
			}
		}
	}
	if (!h) {
		return TS_BAD_ARGUMENT;
	}
	/* out of the table first, so that nothing finds it while it ends */
	live[k] = live[--liveCount];
	live[liveCount] = NULL;
	while (h->state == PENDING) {
		ts_coro_next(h, -1);
	}
	if (h->stack) {		/* FRESH: never resumed */
		giveStack(h->stack);
	}
	free(h->perm);
	free(h);
	return TS_OK;
}

void
ts_coro_close_all(void)
{
	while (liveCount > 0) {
		ts_coro_close(live[liveCount - 1]->id);
	}
	while (cached > 0) {
		ts_stack_delete(cache[--cached]);
	}
}

#else

typedef int ts_coro_unsupported;	/* (ISO C wants a declaration in every file) */

#endif /* TS_CORO_SUPPORTED */
