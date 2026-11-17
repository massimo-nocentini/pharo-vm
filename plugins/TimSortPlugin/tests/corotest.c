/* corotest.c -- the checks of the resumable sort (tsCoro.h, src/coro), run
 * by run-c-tests.sh in the lanes coro (clang -O2) and coro-asan (gcc -O1,
 * AddressSanitizer and UBSan), on native targets with TS_CORO_SUPPORTED
 * only; linked with the engine (tsEngine.c) and timsort.c.  The checks:
 *
 *   setup      the switch of this target, available, nothing live
 *   arguments  ts_coro_open outside [0, TS_CORO_MAX_SIZE], ts_coro_next of
 *              NULL, lookups and closes of ids never opened; the ids of
 *              this load begin at a base in [2^32, 2^58), not at 1
 *   guard      (Linux) the stack of a sort lies right above a PROT_NONE
 *              page, and is unmapped by ts_coro_close_all
 *   small      n = 0 and 1 (done at the first call, no stack), every order
 *              of 2 and 3 elements, a finished sort answering its end again
 *   equal      sorts stepped to their end, of many sizes (0 .. 65,539;
 *              1,000,000 with --slow) and patterns (random, few distinct,
 *              sorted, reversed, equal, sawtooth, organ pipe, nearly sorted),
 *              ascending and descending, answering through the keys: every
 *              request names two distinct elements; the permutation equals
 *              ts_sort_words' with the same comparison, request for
 *              comparison, and a stable sort by (key, index) of the test's
 *              own (qsort); answers > 1 count as 1
 *   abort      an answer < 0 at a random request: TS_ABORTED then and at
 *              every later call, the permutation a permutation of 0 .. n-1
 *   close      ts_coro_close of a sort never started, pending at a random
 *              request, finished, and from inside one of its own requests;
 *              a second close and later lookups of the id fail
 *   nested     while a sort waits for an answer, whole sorts run to their
 *              end, stepped (three deep) and through ts_sort_words
 *   depth      every request answered from a different depth of the
 *              caller's stack (the caller's context is saved afresh at
 *              every resume)
 *   table      TS_CORO_TABLE_SIZE sorts live and stepped interleaved at
 *              random, the next refused (TS_LIMIT); 1,000 sorts through the
 *              table, opened, stepped and closed at random; ids count up,
 *              stale ids are never found again; ts_coro_close_all
 *   memory     (Linux, not under AddressSanitizer) with RLIMIT_AS just above
 *              the process: the permutation, the stack and timsort.c's merge
 *              buffer cannot be allocated: TS_NO_MEMORY from ts_coro_open
 *              and from ts_coro_next, the permutation intact; n =
 *              TS_CORO_MAX_SIZE fails for memory, not as an argument
 *   stack      the high-water mark of the sort's stack (from the lowest
 *              word ever written) after sorts of 200,000 elements
 *              (1,000,000 with --slow) of every pattern, below a quarter of
 *              TS_CORO_STACK_SIZE
 *   speed      the cost of a request: a sort of 100,000 random keys
 *              (1,000,000 with --slow) stepped, against ts_sort_words with
 *              the same comparison, in ns per request; ts_coro_open + two
 *              calls + ts_coro_close of 2 elements, with the stack kept and
 *              mapped afresh
 *
 * Usage: corotest [--slow] [--seed N].  Prints one line per failed check
 * (the first 50), the measures, and "corotest ok: N checks, 0 failures";
 * exits 1 when a check failed. */

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "tsCoro.h"
#include "tsEngine.h"

#if TS_CORO_SUPPORTED

#include "../src/coro/tsCoroSwitch.h"

#if defined(__linux__) && !TS_CORO_ASAN
# include <sys/resource.h>
# include <unistd.h>
# define TEST_MEMORY 1
#else
# define TEST_MEMORY 0
#endif
#if defined(__linux__)
# include <unistd.h>
#endif

static long checks, failures;
static int slow;

static void
fail(const char *format, ...)
{
	va_list args;

	if (++failures <= 50) {
		va_start(args, format);
		printf("corotest FAILED: ");
		vprintf(format, args);
		printf("\n");
		va_end(args);
	}
}

#define CHECK(condition, ...) do { checks++; if (!(condition)) fail(__VA_ARGS__); } while (0)

/* xorshift64*, seeded */
static uint64_t seed = 0x436f726f54657374ull;

static uint64_t
next(void)
{
	seed ^= seed >> 12;
	seed ^= seed << 25;
	seed ^= seed >> 27;
	return seed * 0x2545F4914F6CDD1Dull;
}

static int64_t
below(int64_t n)
{
	return n > 0 ? (int64_t)(next() % (uint64_t)n) : 0;
}

static double
now(void)
{
	struct timespec t;

	clock_gettime(CLOCK_MONOTONIC, &t);
	return t.tv_sec + t.tv_nsec * 1e-9;
}


/*** Keys, answers and references ***/

/* The patterns of keys */
enum { RANDOM, FEW, SORTED, REVERSED, EQUAL, SAWTOOTH, ORGAN, NEARLY, PATTERNS };
static const char *patternNames[] = { "random", "few", "sorted", "reversed", "equal", "sawtooth",
	"organ", "nearly" };

static void
fillKeys(int64_t *keys, int64_t n, int pattern)
{
	int64_t k;

	for (k = 0; k < n; k++) {
		switch (pattern) {
		case RANDOM: keys[k] = (int64_t)next(); break;
		case FEW: keys[k] = below(4); break;
		case SORTED: keys[k] = k; break;
		case REVERSED: keys[k] = n - k; break;
		case EQUAL: keys[k] = 7; break;
		case SAWTOOTH: keys[k] = k % 37; break;
		case ORGAN: keys[k] = k < n / 2 ? k : n - k; break;
		default: keys[k] = k; break;
		}
	}
	if (pattern == NEARLY) {
		for (k = 0; k < n / 100 + 1 && n > 1; k++) {
			int64_t a = below(n), b = below(n), t = keys[a];

			keys[a] = keys[b];
			keys[b] = t;
		}
	}
}

/* The answer to a request r about keys: lt(i, j) = keys[i] < keys[j]; a
 * true answer is sometimes 7 (any answer > 1 counts as 1) */
static int64_t
answerOf(const int64_t *keys, int64_t r)
{
	int64_t i = r >> 32, j = r & 0xffffffff;

	if (keys[i] < keys[j]) {
		return (next() & 7) == 0 ? 7 : 1;
	}
	return 0;
}

/* Whether r is a request about two distinct elements of n */
static int
validRequest(int64_t r, int64_t n)
{
	int64_t i = r >> 32, j = r & 0xffffffff;

	return r >= 0 && i < n && j < n && i != j;
}

/* Steps the sort h to its end, answering through keys: its end, the
 * requests counted in *requests; stops at the first invalid request */
static int64_t
drive(ts_coro *h, const int64_t *keys, int64_t n, long *requests)
{
	int64_t r = ts_coro_next(h, 0);

	*requests = 0;
	while (r >= 0) {
		if (!validRequest(r, n)) {
			fail("request %lld of %lld elements", (long long)r, (long long)n);
			return r;
		}
		++*requests;
		r = ts_coro_next(h, answerOf(keys, r));
	}
	return r;
}

/* The comparison of ts_sort_words over keys, counting */
typedef struct {
	const int64_t *keys;
	long compares;
} Direct;

static int
directLess(int64_t a, int64_t b, void *arg)
{
	Direct *d = arg;

	d->compares++;
	return d->keys[a] < d->keys[b];
}

/* The stable reference: indices sorted by (key, index), the key reversed
 * when descending */
static const int64_t *referenceKeys;
static int referenceDescending;

static int
referenceOrder(const void *pa, const void *pb)
{
	int64_t a = *(const int64_t *)pa, b = *(const int64_t *)pb;
	int64_t ka = referenceKeys[a], kb = referenceKeys[b];

	if (ka != kb) {
		return (ka < kb) != referenceDescending ? -1 : 1;
	}
	return a < b ? -1 : 1;
}

static int
isPermutation(const int64_t *perm, int64_t n)
{
	unsigned char *seen = calloc((size_t)(n > 0 ? n : 1), 1);
	int64_t k;
	int ok = seen != NULL;

	for (k = 0; ok && k < n; k++) {
		ok = perm[k] >= 0 && perm[k] < n && !seen[perm[k]];
		if (ok) {
			seen[perm[k]] = 1;
		}
	}
	free(seen);
	return ok;
}

static ts_coro *
openSort(int64_t n, int descending)
{
	int status = 1;
	int64_t id = ts_coro_open(n, descending, &status);

	CHECK(id >= 1 && status == TS_OK, "ts_coro_open(%lld): id %lld, status %d", (long long)n, (long long)id, status);
	return ts_coro_lookup(id);
}

/* The first id of this load, which checkArguments learns (ids count up
   from a base the copy draws at its first sort) */
static int64_t firstId = 1;

/* The id of a live sort (the table is the only place that knows it) */
static int64_t
idOf(ts_coro *h)
{
	int64_t id;

	for (id = firstId; id <= firstId + 1000000; id++) {
		if (ts_coro_lookup(id) == h) {
			return id;
		}
	}
	return 0;
}


/*** The checks ***/

static void
checkSetup(void)
{
	const char *impl = ts_coro_impl();
#if defined(_WIN32)
	const char *want = "fibers";
#elif defined(__x86_64__)
	const char *want = "asm-x86_64-sysv";
#else
	const char *want = "asm-aarch64";
#endif

	CHECK(strcmp(impl, want) == 0, "ts_coro_impl %s, expected %s", impl, want);
	CHECK(ts_coro_available() == 1, "ts_coro_available %d (a shadow stack?)", ts_coro_available());
	CHECK(ts_coro_live() == 0, "live %lld at the start", (long long)ts_coro_live());
	CHECK(ts_coro_stack_high_water() == 0, "a stack before any sort");
	printf("switch %s, stack %d KB + a guard page, %d live sorts at most, %d stack kept\n",
		impl, TS_CORO_STACK_SIZE / 1024, TS_CORO_TABLE_SIZE, TS_CORO_STACK_CACHE);
}

#if defined(__linux__)
/* The stacks with their guard page in /proc/self/maps: a page "---p" right
 * below TS_CORO_STACK_SIZE bytes "rw-p" */
static int
guardedStacks(void)
{
	FILE *f = fopen("/proc/self/maps", "r");
	unsigned long page = (unsigned long)sysconf(_SC_PAGESIZE), start, end, guardEnd = 0;
	char line[512], perms[8];
	int count = 0;

	if (!f) {
		return -1;
	}
	while (fgets(line, sizeof line, f)) {
		if (sscanf(line, "%lx-%lx %7s", &start, &end, perms) != 3) {
			continue;
		}
		if (strcmp(perms, "rw-p") == 0 && start == guardEnd && end - start == TS_CORO_STACK_SIZE) {
			count++;
		}
		guardEnd = strcmp(perms, "---p") == 0 && end - start == page ? end : 0;
	}
	fclose(f);
	return count;
}

static void
checkGuard(void)
{
	int before = guardedStacks(), during;
	int64_t id = ts_coro_open(10, 0, NULL);

	during = guardedStacks();
	CHECK(before >= 0 && during == before + 1, "the guarded stacks: %d, then %d with a sort", before, during);
	ts_coro_close(id);
	ts_coro_close_all();
	CHECK(guardedStacks() == before, "the guarded stacks after close_all: %d", guardedStacks());
	printf("guard: the stack of a sort lies right above a PROT_NONE page (/proc/self/maps)\n");
}
#endif

static void
checkArguments(void)
{
	static const int64_t bad[] = { -1, -2, INT64_MIN, TS_CORO_MAX_SIZE + 1, INT64_MAX };
	static const int64_t ids[] = { 0, -1, 1, 2, INT64_MIN, 1000000000, INT64_MAX };
	int64_t id, id2;
	int status;
	size_t k;

	for (k = 0; k < sizeof bad / sizeof bad[0]; k++) {
		status = 1;
		CHECK(ts_coro_open(bad[k], 0, &status) == 0 && status == TS_BAD_ARGUMENT,
			"ts_coro_open(%lld): status %d", (long long)bad[k], status);
		CHECK(ts_coro_open(bad[k], 1, NULL) == 0, "ts_coro_open(%lld, NULL status)", (long long)bad[k]);
	}
	CHECK(ts_coro_next(NULL, 0) == TS_BAD_ARGUMENT, "ts_coro_next(NULL)");
	CHECK(ts_coro_result(NULL) == 0 && ts_coro_size(NULL) == 0 && ts_coro_permutation(NULL) == NULL,
		"result, size, permutation of NULL");
	for (k = 0; k < sizeof ids / sizeof ids[0]; k++) {
		CHECK(ts_coro_lookup(ids[k]) == NULL, "lookup(%lld) of nothing", (long long)ids[k]);
		CHECK(ts_coro_close(ids[k]) == TS_BAD_ARGUMENT, "close(%lld) of nothing", (long long)ids[k]);
	}
	CHECK(ts_coro_live() == 0, "live after bad arguments");
	/* the ids of this load count up from a base drawn at its first sort, in
	   [2^32, 2^58): not from 1, which the first sort of every load (every
	   session of the image) would get again */
	id = ts_coro_open(2, 0, NULL);
	id2 = ts_coro_open(2, 0, NULL);
	CHECK(id >= (int64_t)1 << 32 && id < (int64_t)1 << 59 && id2 == id + 1,
		"ids of this load: %lld then %lld, not from a base in [2^32, 2^58)", (long long)id, (long long)id2);
	CHECK(ts_coro_close(id) == TS_OK && ts_coro_close(id2) == TS_OK && ts_coro_live() == 0, "ids: closed");
	firstId = id;
	ts_coro_close_all();	/* no stack kept */
}

static void
checkSmall(void)
{
	static const int64_t orders[][3] = { {0, 1, 2}, {0, 2, 1}, {1, 0, 2}, {1, 2, 0}, {2, 0, 1}, {2, 1, 0},
		{0, 0, 1}, {1, 0, 0}, {0, 1, 0}, {1, 1, 1} };
	int64_t n, r, id, keys[3];
	int64_t want[3];
	size_t o;
	int descending, k;
	long requests;
	ts_coro *h;

	for (n = 0; n < 2; n++) {
		id = ts_coro_open(n, 0, NULL);
		h = ts_coro_lookup(id);
		CHECK(h != NULL && ts_coro_size(h) == n && ts_coro_result(h) == 0, "n = %lld opened", (long long)n);
		if (!h) {
			continue;
		}
		CHECK(ts_coro_stack_high_water() == 0 || n > 1, "a stack for n < 2");
		CHECK(ts_coro_next(h, 5) == TS_DONE, "n = %lld: done at once", (long long)n);
		CHECK(ts_coro_next(h, -1) == TS_DONE && ts_coro_result(h) == TS_DONE, "n = %lld: done again", (long long)n);
		CHECK(n == 0 || ts_coro_permutation(h)[0] == 0, "n = 1: permutation");
		CHECK(ts_coro_close(id) == TS_OK && ts_coro_close(id) == TS_BAD_ARGUMENT, "n = %lld: close", (long long)n);
	}
	for (n = 2; n <= 3; n++) {
		for (o = 0; o < sizeof orders / sizeof orders[0]; o++) {
			for (descending = 0; descending < 2; descending++) {
				for (k = 0; k < n; k++) {
					keys[k] = orders[o][k];
					want[k] = k;
				}
				referenceKeys = keys;
				referenceDescending = descending;
				qsort(want, (size_t)n, sizeof want[0], referenceOrder);
				h = openSort(n, descending);
				if (!h) {
					continue;
				}
				CHECK(ts_coro_result(h) == 0, "result before the end");
				r = drive(h, keys, n, &requests);
				CHECK(r == TS_DONE && ts_coro_result(h) == TS_DONE, "n = %lld: %lld", (long long)n, (long long)r);
				CHECK(memcmp(ts_coro_permutation(h), want, (size_t)n * sizeof want[0]) == 0,
					"n = %lld, order %zu, descending %d: permutation", (long long)n, o, descending);
				CHECK(ts_coro_next(h, 1) == TS_DONE, "done again");
				CHECK(ts_coro_close(idOf(h)) == TS_OK, "close");
			}
		}
	}
	CHECK(ts_coro_live() == 0, "live after the small sorts");
}

/* One sort of n keys of a pattern, stepped, against the references */
static void
equalOne(int64_t n, int pattern, int descending)
{
	int64_t *keys = malloc((size_t)(n > 0 ? n : 1) * sizeof(int64_t));
	int64_t *direct = malloc((size_t)(n > 0 ? n : 1) * sizeof(int64_t));
	int64_t *reference = malloc((size_t)(n > 0 ? n : 1) * sizeof(int64_t));
	Direct d;
	int64_t k, r;
	long requests;
	int status;
	ts_coro *h;

	if (!keys || !direct || !reference) {
		fail("no memory for %lld keys", (long long)n);
		goto done;
	}
	fillKeys(keys, n, pattern);
	for (k = 0; k < n; k++) {
		direct[k] = reference[k] = k;
	}
	d.keys = keys;
	d.compares = 0;
	status = ts_sort_words(direct, n, descending, directLess, &d);
	CHECK(status == TS_OK, "ts_sort_words: %d", status);
	referenceKeys = keys;
	referenceDescending = descending;
	qsort(reference, (size_t)n, sizeof reference[0], referenceOrder);
	h = openSort(n, descending);
	if (!h) {
		goto done;
	}
	r = drive(h, keys, n, &requests);
	CHECK(r == TS_DONE, "n = %lld %s: end %lld", (long long)n, patternNames[pattern], (long long)r);
	CHECK(requests == d.compares, "n = %lld %s descending %d: %ld requests, %ld comparisons",
		(long long)n, patternNames[pattern], descending, requests, d.compares);
	CHECK(n == 0 || memcmp(ts_coro_permutation(h), direct, (size_t)n * sizeof direct[0]) == 0,
		"n = %lld %s descending %d: not ts_sort_words' permutation", (long long)n, patternNames[pattern], descending);
	CHECK(n == 0 || memcmp(ts_coro_permutation(h), reference, (size_t)n * sizeof reference[0]) == 0,
		"n = %lld %s descending %d: not the stable order", (long long)n, patternNames[pattern], descending);
	CHECK(ts_coro_close(idOf(h)) == TS_OK, "close");
done:
	free(keys);
	free(direct);
	free(reference);
}

static void
checkEqual(void)
{
	static const int64_t sizes[] = { 63, 64, 65, 127, 128, 129, 255, 256, 257, 1000, 1023, 1024, 4097,
		10007, 65539 };
	int64_t n;
	size_t s;
	int pattern, descending, runs = 0;

	for (pattern = 0; pattern < PATTERNS; pattern++) {
		for (descending = 0; descending < 2; descending++) {
			for (n = 0; n <= 40; n++, runs++) {
				equalOne(n, pattern, descending);
			}
			for (s = 0; s < sizeof sizes / sizeof sizes[0]; s++, runs++) {
				equalOne(sizes[s], pattern, descending);
			}
			if (slow) {
				equalOne(1000000, pattern, descending);
				runs++;
			}
		}
	}
	CHECK(ts_coro_live() == 0, "live after the equal sorts");
	printf("equal: %d sorts stepped to their end\n", runs);
}

static void
checkAbort(void)
{
	int round;

	for (round = 0; round < 200; round++) {
		int64_t n = 2 + below(round < 100 ? 50 : 20000);
		int64_t *keys = malloc((size_t)n * sizeof(int64_t));
		int64_t r, at;
		long requests = 0;
		ts_coro *h;

		if (!keys) {
			fail("no memory");
			return;
		}
		fillKeys(keys, n, (int)below(PATTERNS));
		h = openSort(n, round & 1);
		if (!h) {
			free(keys);
			continue;
		}
		at = below(n * 2);	/* sometimes beyond the end: then no abort */
		r = ts_coro_next(h, 0);
		while (r >= 0 && requests < at) {
			requests++;
			r = ts_coro_next(h, answerOf(keys, r));
		}
		if (r >= 0) {
			r = ts_coro_next(h, round % 3 == 0 ? INT64_MIN : -1 - below(5));
			CHECK(r == TS_ABORTED, "abort at %lld of %lld: %lld", (long long)at, (long long)n, (long long)r);
			CHECK(ts_coro_result(h) == TS_ABORTED, "result after the abort");
			CHECK(ts_coro_next(h, 1) == TS_ABORTED && ts_coro_next(h, 0) == TS_ABORTED, "aborted again");
		} else {
			CHECK(r == TS_DONE, "end %lld", (long long)r);
		}
		CHECK(isPermutation(ts_coro_permutation(h), n), "abort at %lld of %lld: not a permutation",
			(long long)at, (long long)n);
		CHECK(ts_coro_close(idOf(h)) == TS_OK, "close after the abort");
		free(keys);
	}
	CHECK(ts_coro_live() == 0, "live after the aborts");
}

static void
checkClose(void)
{
	int64_t keys[5000], id, r, at;
	int64_t live0 = ts_coro_live();
	int round;
	ts_coro *h;

	fillKeys(keys, 5000, RANDOM);
	/* never started, pending, finished */
	for (round = 0; round < 300; round++) {
		int64_t n = 2 + below(5000 - 2);

		id = ts_coro_open(n, round & 1, NULL);
		h = ts_coro_lookup(id);
		CHECK(h != NULL && ts_coro_live() == live0 + 1, "open for close");
		if (!h) {
			continue;
		}
		at = round % 3 == 0 ? 0 : below(n * 12);
		r = 0;
		if (at > 0) {
			r = ts_coro_next(h, 0);
			while (r >= 0 && --at > 0) {
				r = ts_coro_next(h, answerOf(keys, r));
			}
		}
		CHECK(ts_coro_close(id) == TS_OK, "close at a request");
		CHECK(ts_coro_lookup(id) == NULL && ts_coro_close(id) == TS_BAD_ARGUMENT, "closed id found again");
		CHECK(ts_coro_live() == live0, "live after a close");
	}
	/* from inside one of its own requests: the caller then finds no sort */
	id = ts_coro_open(1000, 0, NULL);
	h = ts_coro_lookup(id);
	r = h ? ts_coro_next(h, 0) : -1;
	for (at = 0; r >= 0 && at < 100; at++) {
		r = ts_coro_next(h, answerOf(keys, r));
	}
	CHECK(r >= 0, "a pending request");
	CHECK(ts_coro_close(id) == TS_OK && ts_coro_lookup(id) == NULL, "close inside a request");
	CHECK(ts_coro_live() == live0, "live after closing inside a request");
}

/* A sort stepped to its end, which at random requests runs further whole
 * sorts (stepped, depth levels more, and through ts_sort_words) before it
 * answers */
static void
nestedSort(int depth, long *sorts)
{
	int64_t n = 2 + below(depth == 0 ? 3000 : 300);
	int64_t *keys = malloc((size_t)n * sizeof(int64_t));
	int64_t *reference = malloc((size_t)n * sizeof(int64_t));
	int64_t r, k;
	int descending = (int)below(2);
	ts_coro *h;

	if (!keys || !reference) {
		fail("no memory");
		free(keys);
		free(reference);
		return;
	}
	fillKeys(keys, n, (int)below(PATTERNS));
	for (k = 0; k < n; k++) {
		reference[k] = k;
	}
	referenceKeys = keys;
	referenceDescending = descending;
	qsort(reference, (size_t)n, sizeof reference[0], referenceOrder);
	h = openSort(n, descending);
	if (!h) {
		free(keys);
		free(reference);
		return;
	}
	r = ts_coro_next(h, 0);
	while (r >= 0) {
		if (depth < 3 && below(depth == 0 ? 40 : 200) == 0) {
			nestedSort(depth + 1, sorts);
		}
		if (below(500) == 0) {
			int64_t m = 1 + below(200), words[200];
			Direct d;

			for (k = 0; k < m; k++) {
				words[k] = k;
			}
			d.keys = keys;
			d.compares = 0;
			CHECK(ts_sort_words(words, m < n ? m : n, 0, directLess, &d) == TS_OK, "ts_sort_words inside");
		}
		r = ts_coro_next(h, answerOf(keys, r));
	}
	CHECK(r == TS_DONE, "nested at depth %d: %lld", depth, (long long)r);
	CHECK(memcmp(ts_coro_permutation(h), reference, (size_t)n * sizeof reference[0]) == 0,
		"nested at depth %d: not the stable order", depth);
	CHECK(ts_coro_close(idOf(h)) == TS_OK, "close nested");
	++*sorts;
	free(keys);
	free(reference);
}

static void
checkNested(void)
{
	long sorts = 0;
	int round;

	for (round = 0; round < 20; round++) {
		nestedSort(0, &sorts);
	}
	CHECK(ts_coro_live() == 0, "live after the nested sorts");
	printf("nested: %ld sorts, up to 4 suspended at once\n", sorts);
}

/* Answers the request r of h from depth more frames down the stack */
static int64_t
answerDeep(ts_coro *h, const int64_t *keys, int64_t r, int depth)
{
	volatile char pad[200];

	pad[0] = (char)depth;
	if (depth > 0) {
		return answerDeep(h, keys, r, depth - 1) + pad[0] - (char)depth;
	}
	return ts_coro_next(h, answerOf(keys, r));
}

static void
checkDepth(void)
{
	enum { N = 20000 };
	static int64_t keys[N], reference[N];
	int64_t k, r;
	ts_coro *h;

	fillKeys(keys, N, RANDOM);
	for (k = 0; k < N; k++) {
		reference[k] = k;
	}
	referenceKeys = keys;
	referenceDescending = 0;
	qsort(reference, N, sizeof reference[0], referenceOrder);
	h = openSort(N, 0);
	if (!h) {
		return;
	}
	r = ts_coro_next(h, 0);
	while (r >= 0) {
		r = answerDeep(h, keys, r, (int)below(64));
	}
	CHECK(r == TS_DONE && memcmp(ts_coro_permutation(h), reference, sizeof reference) == 0,
		"answers from many depths");
	CHECK(ts_coro_close(idOf(h)) == TS_OK, "close");
}

/* A sort of the table check */
typedef struct {
	int64_t id;
	int64_t n;
	int64_t *keys;
	int64_t request;	/* the pending request, or the end */
	int descending;
} Live;

static void
liveCheck(Live *s)
{
	ts_coro *h = ts_coro_lookup(s->id);
	int64_t *reference = malloc((size_t)s->n * sizeof(int64_t));
	int64_t k;

	if (!reference || !h) {
		fail("live check: %s", h ? "no memory" : "lost sort");
		free(reference);
		return;
	}
	for (k = 0; k < s->n; k++) {
		reference[k] = k;
	}
	referenceKeys = s->keys;
	referenceDescending = s->descending;
	qsort(reference, (size_t)s->n, sizeof reference[0], referenceOrder);
	CHECK(s->request == TS_DONE && memcmp(ts_coro_permutation(h), reference, (size_t)s->n * sizeof reference[0]) == 0,
		"interleaved sort %lld: not the stable order", (long long)s->id);
	free(reference);
}

static void
checkTable(void)
{
	Live sorts[TS_CORO_TABLE_SIZE];
	int64_t lastId = 0, stale[64];
	int count = 0, k, opened = 0, finished = 0, closedEarly = 0, staleCount = 0, status;

	/* TS_CORO_TABLE_SIZE live, stepped interleaved */
	for (k = 0; k < TS_CORO_TABLE_SIZE; k++) {
		Live *s = &sorts[k];

		s->n = 2 + below(2000);
		s->keys = malloc((size_t)s->n * sizeof(int64_t));
		fillKeys(s->keys, s->n, (int)below(PATTERNS));
		s->descending = (int)below(2);
		s->id = ts_coro_open(s->n, s->descending, &status);
		CHECK(s->id > lastId && status == TS_OK, "open %d of the table: id %lld after %lld", k,
			(long long)s->id, (long long)lastId);
		lastId = s->id;
		s->request = ts_coro_next(ts_coro_lookup(s->id), 0);
	}
	CHECK(ts_coro_live() == TS_CORO_TABLE_SIZE, "live %lld", (long long)ts_coro_live());
	status = 0;
	CHECK(ts_coro_open(10, 0, &status) == 0 && status == TS_LIMIT, "open beyond the table: %d", status);
	CHECK(ts_coro_open(0, 0, &status) == 0 && status == TS_LIMIT, "open of 0 beyond the table: %d", status);
	count = TS_CORO_TABLE_SIZE;
	while (count > 0) {
		k = (int)below(count);
		if (sorts[k].request >= 0) {
			sorts[k].request = ts_coro_next(ts_coro_lookup(sorts[k].id),
				answerOf(sorts[k].keys, sorts[k].request));
			continue;
		}
		liveCheck(&sorts[k]);
		CHECK(ts_coro_close(sorts[k].id) == TS_OK, "close %lld", (long long)sorts[k].id);
		free(sorts[k].keys);
		sorts[k] = sorts[--count];
	}
	CHECK(ts_coro_live() == 0, "live after the table");

	/* 1,000 sorts through the table, at random */
	while (opened < 1000 || count > 0) {
		uint64_t choice = below(100);

		if (opened < 1000 && count < TS_CORO_TABLE_SIZE && (choice < 10 || count == 0)) {
			Live *s = &sorts[count];

			s->n = below(300);
			s->keys = malloc((size_t)(s->n > 0 ? s->n : 1) * sizeof(int64_t));
			fillKeys(s->keys, s->n, (int)below(PATTERNS));
			s->descending = (int)below(2);
			s->id = ts_coro_open(s->n, s->descending, &status);
			CHECK(s->id > lastId && status == TS_OK, "open %d: id %lld after %lld", opened,
				(long long)s->id, (long long)lastId);
			lastId = s->id;
			s->request = ts_coro_next(ts_coro_lookup(s->id), 0);
			count++;
			opened++;
		} else if (count > 0) {
			Live *s = &sorts[k = (int)below(count)];

			if (s->request >= 0 && (choice != 99 || below(10) != 0)) {
				s->request = ts_coro_next(ts_coro_lookup(s->id), answerOf(s->keys, s->request));
				continue;
			}
			if (s->request < 0) {
				liveCheck(s);
				finished++;
			} else {
				closedEarly++;
			}
			CHECK(ts_coro_close(s->id) == TS_OK, "close %lld", (long long)s->id);
			stale[staleCount++ % 64] = s->id;
			free(s->keys);
			*s = sorts[--count];
		}
		CHECK(ts_coro_live() == count, "live %lld, %d open", (long long)ts_coro_live(), count);
	}
	for (k = 0; k < 64 && k < staleCount; k++) {
		CHECK(ts_coro_lookup(stale[k]) == NULL && ts_coro_close(stale[k]) == TS_BAD_ARGUMENT,
			"stale id %lld found", (long long)stale[k]);
	}
	CHECK(ts_coro_lookup(lastId + 1) == NULL, "an id not given yet found");

	/* ts_coro_close_all: fresh, pending and finished sorts, the stacks kept */
	{
		int64_t a = ts_coro_open(100, 0, NULL), b = ts_coro_open(100, 1, NULL), c = ts_coro_open(1, 0, NULL);
		int64_t d;

		ts_coro_next(ts_coro_lookup(b), 0);
		ts_coro_next(ts_coro_lookup(c), 0);
		CHECK(ts_coro_live() == 3, "live before close_all");
		ts_coro_close_all();
		CHECK(ts_coro_live() == 0 && ts_coro_lookup(a) == NULL && ts_coro_lookup(b) == NULL
			&& ts_coro_lookup(c) == NULL, "close_all");
		CHECK(ts_coro_stack_high_water() == 0, "a stack kept after close_all");
		d = ts_coro_open(5, 0, NULL);
		CHECK(d > c, "ids after close_all: %lld after %lld", (long long)d, (long long)c);
		ts_coro_close(d);
	}
	printf("table: %d live interleaved, then %d sorts (%d finished, %d closed pending), last id %lld\n",
		TS_CORO_TABLE_SIZE, opened, finished, closedEarly, (long long)lastId);
}

#if TEST_MEMORY
static size_t
processBytes(void)
{
	FILE *f = fopen("/proc/self/statm", "r");
	unsigned long pages = 0;

	if (f) {
		if (fscanf(f, "%lu", &pages) != 1) {
			pages = 0;
		}
		fclose(f);
	}
	return pages * (size_t)sysconf(_SC_PAGESIZE);
}

static void
checkMemory(void)
{
	enum { N = 4000000 };
	struct rlimit saved, limit;
	int64_t *keys = malloc(N * sizeof(int64_t));
	int64_t id, r;
	int status;
	ts_coro *h;

	if (!keys || getrlimit(RLIMIT_AS, &saved) != 0) {
		fail("memory: setup");
		free(keys);
		return;
	}
	fillKeys(keys, N, RANDOM);
	ts_coro_close_all();	/* no stack kept */
	id = ts_coro_open(N, 0, NULL);	/* its permutation before the limit */
	h = ts_coro_lookup(id);
	limit = saved;
	limit.rlim_cur = processBytes() + 128 * 1024;
	if (!h || setrlimit(RLIMIT_AS, &limit) != 0) {
		fail("memory: setrlimit");
		free(keys);
		return;
	}
	/* the permutation; the stack (the handle and the permutation of 2 come
	 * from the heap the process has) */
	status = 0;
	CHECK(ts_coro_open((int64_t)1 << 30, 0, &status) == 0 && status == TS_NO_MEMORY,
		"no memory for the permutation: %d", status);
	/* the largest n passes the check of the arguments: its permutation
	   (16 GiB) is refused for memory, not for n */
	status = 0;
	CHECK(ts_coro_open(TS_CORO_MAX_SIZE, 1, &status) == 0 && status == TS_NO_MEMORY,
		"n = TS_CORO_MAX_SIZE: %d, not TS_NO_MEMORY", status);
	status = 0;
	CHECK(ts_coro_open(2, 0, &status) == 0 && status == TS_NO_MEMORY, "no memory for the stack: %d", status);
	CHECK(ts_coro_live() == 1, "live after the failed opens");
	/* the merge buffer: the stack is mapped already */
	r = ts_coro_next(h, 0);
	while (r >= 0) {
		r = ts_coro_next(h, answerOf(keys, r));
	}
	setrlimit(RLIMIT_AS, &saved);
	CHECK(r == TS_NO_MEMORY && ts_coro_result(h) == TS_NO_MEMORY, "no memory for the merge: %lld", (long long)r);
	CHECK(ts_coro_next(h, 1) == TS_NO_MEMORY, "no memory again");
	CHECK(isPermutation(ts_coro_permutation(h), N), "no memory: not a permutation");
	CHECK(ts_coro_close(id) == TS_OK && ts_coro_live() == 0, "close after no memory");
	printf("memory: TS_NO_MEMORY for the permutation and the stack (ts_coro_open) and for the merge buffer "
		"(ts_coro_next, %d elements)\n", N);
	free(keys);
}
#endif

static void
checkStack(void)
{
	int64_t N = slow ? 1000000 : 200000;
	int64_t *keys = malloc((size_t)N * sizeof(int64_t));
	size_t used, most = 0;
	int pattern;
	long requests;

	if (!keys) {
		fail("no memory");
		return;
	}
	ts_coro_close_all();	/* a stack mapped afresh: zero */
	for (pattern = 0; pattern < PATTERNS; pattern++) {
		int descending;

		for (descending = 0; descending < 2; descending++) {
			ts_coro *h;

			fillKeys(keys, N, pattern);
			h = openSort(N, descending);
			if (!h) {
				continue;
			}
			drive(h, keys, N, &requests);
			ts_coro_close(idOf(h));
			used = ts_coro_stack_high_water();
			most = used > most ? used : most;
		}
	}
	CHECK(most > 0 && most < TS_CORO_STACK_SIZE / 4, "stack high water %zu bytes", most);
	printf("stack: %zu bytes at most of %d KB (sorts of %lld elements, every pattern%s)\n", most,
		TS_CORO_STACK_SIZE / 1024, (long long)N, TS_CORO_ASAN ? ", AddressSanitizer" : "");
	free(keys);
}

static void
checkSpeed(void)
{
	int64_t n = slow ? 1000000 : 100000;
	int64_t *keys = malloc((size_t)n * sizeof(int64_t));
	int64_t *words = malloc((size_t)n * sizeof(int64_t));
	double best = 1e9, bestDirect = 1e9, t, cycleWarm = 1e9, cycleCold = 1e9;
	int rep, reps = slow ? 7 : 3, k, cycles = 20000;
	long requests = 0;
	Direct d;

	if (!keys || !words) {
		fail("no memory");
		free(keys);
		free(words);
		return;
	}
	fillKeys(keys, n, RANDOM);
	for (rep = 0; rep < reps; rep++) {
		int64_t r, id = ts_coro_open(n, 0, NULL);
		ts_coro *h = ts_coro_lookup(id);

		requests = 0;
		t = now();
		r = ts_coro_next(h, 0);
		while (r >= 0) {
			int64_t i = r >> 32, j = r & 0xffffffff;

			requests++;
			r = ts_coro_next(h, keys[i] < keys[j]);
		}
		t = now() - t;
		best = t < best ? t : best;
		ts_coro_close(id);

		for (k = 0; k < n; k++) {
			words[k] = k;
		}
		d.keys = keys;
		d.compares = 0;
		t = now();
		ts_sort_words(words, n, 0, directLess, &d);
		t = now() - t;
		bestDirect = t < bestDirect ? t : bestDirect;
	}
	CHECK(requests == d.compares, "speed: %ld requests, %ld comparisons", requests, d.compares);
	for (rep = 0; rep < 3; rep++) {
		t = now();
		for (k = 0; k < cycles; k++) {
			int64_t id = ts_coro_open(2, 0, NULL);
			ts_coro *h = ts_coro_lookup(id);

			ts_coro_next(h, ts_coro_next(h, 0) >= 0);
			ts_coro_close(id);
		}
		t = (now() - t) / cycles;
		cycleWarm = t < cycleWarm ? t : cycleWarm;
		t = now();
		for (k = 0; k < cycles / 10; k++) {
			int64_t id = ts_coro_open(2, 0, NULL);
			ts_coro *h = ts_coro_lookup(id);

			ts_coro_next(h, ts_coro_next(h, 0) >= 0);
			ts_coro_close(id);
			ts_coro_close_all();
		}
		t = (now() - t) / (cycles / 10);
		cycleCold = t < cycleCold ? t : cycleCold;
	}
	printf("speed: %lld random keys, %ld requests: stepped %.1f ns, ts_sort_words %.1f ns per comparison: "
		"%.1f ns per request for the switch (best of %d%s)\n",
		(long long)n, requests, best * 1e9 / requests, bestDirect * 1e9 / requests,
		(best - bestDirect) * 1e9 / requests, reps, TS_CORO_ASAN ? ", AddressSanitizer" : "");
	printf("speed: open + 2 requests + close of 2 elements: %.0f ns with the stack kept, %.0f ns mapped afresh\n",
		cycleWarm * 1e9, cycleCold * 1e9);
	free(keys);
	free(words);
}

int
main(int argc, char **argv)
{
	int k;

	for (k = 1; k < argc; k++) {
		if (strcmp(argv[k], "--slow") == 0) {
			slow = 1;
		} else if (strcmp(argv[k], "--seed") == 0 && k + 1 < argc) {
			seed = strtoull(argv[++k], NULL, 0) | 1;
		} else {
			fprintf(stderr, "usage: corotest [--slow] [--seed N]\n");
			return 2;
		}
	}
	setvbuf(stdout, NULL, _IOLBF, 0);
	checkSetup();
	if (ts_coro_available()) {
		checkArguments();
#if defined(__linux__)
		checkGuard();
#endif
		checkSmall();
		checkEqual();
		checkAbort();
		checkClose();
		checkNested();
		checkDepth();
		checkTable();
#if TEST_MEMORY
		checkMemory();
#else
		printf("memory: not checked here (%s)\n", TS_CORO_ASAN ? "AddressSanitizer" : "not Linux");
#endif
		checkStack();
		checkSpeed();
	}
	ts_coro_close_all();
	CHECK(ts_coro_live() == 0, "live at the end");
	if (failures) {
		printf("corotest FAILED: %ld of %ld checks\n", failures, checks);
		return 1;
	}
	printf("corotest ok: %ld checks, 0 failures\n", checks);
	return 0;
}

#else

int
main(void)
{
	printf("corotest: no resumable sort on this target (TS_CORO_SUPPORTED is 0)\n");
	return 0;
}

#endif /* TS_CORO_SUPPORTED */
