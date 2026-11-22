/* cmTrack.c -- the tracking allocator of the CMarkPlugin's engine, its C
 * limits and the guarded landing (docs/CMark.md, "Memory and limits").
 *
 * cmark-gfm aborts the process when its allocator answers NULL and when a
 * buffer would grow past 1 GiB (buffer.c, cmark_strbuf_grow).  So every
 * parse runs with this file's cmark_mem (cmark_parser_new_with_mem), and
 * every block of cmark and of the engine carries a 32-byte header {prev,
 * next, size, list} that links it into one of two lists of live blocks: the
 * blocks of cmark (nodes, buffers, extension data, a renderer's output) and
 * the engine's own (T, its position tables, the tape, the strings, the
 * parent stack).  The header keeps the 16-byte alignment of malloc.  A
 * request that would take the call's live bytes (both lists, headers
 * included, plus the kept result) over its C budget, a block of cmark over
 * CM_MAX_BLOCK_BYTES, or a malloc that answers NULL, longjmps back to
 * cm_guarded, the only setjmp of the plugin (rule R9): its caller restores
 * cmark's static tables and sweeps both lists, without calling cmark (the
 * tree may be half-built).  cmark's frames between hold no resources.
 *
 * With blocks of cmark of at most 256 MiB, a strbuf target is at most one
 * block plus one addition (another block or one input line), so at most 512
 * MiB: the abort of cmark_strbuf_grow is unreachable.
 *
 * One static context, active only between cm_track_begin and cm_track_end:
 * the VM calls primitives from one thread, and a call during a call is a bug
 * (asserted in the test builds).
 */

#include <setjmp.h>
#include <stdlib.h>
#include <string.h>

#include "cmark-gfm.h"
#include "cmPrivate.h"

typedef struct cm_block {
	struct cm_block *prev;
	struct cm_block *next;
	size_t size;		/* the bytes asked for */
	size_t list;		/* CMARK_LIST, OWN_LIST, or NO_LIST once kept */
} cm_block;

/* 16-byte alignment of the bytes after the header (C99: no _Static_assert) */
typedef char cm_block_header_keeps_alignment[sizeof(cm_block) % 16 == 0 ? 1 : -1];

enum { NO_LIST = 0, CMARK_LIST = 1, OWN_LIST = 2 };

static cm_block lists[3];	/* the sentinels of the circular lists (NO_LIST unused) */
static size_t counts[3];
static uint64_t charged;	/* the bytes of both lists, headers included */
static cm_call *call;		/* the active call, or NULL */
static size_t maxBlock = CM_MAX_BLOCK_BYTES;
static jmp_buf landing;

#define HEADER(block) ((cm_block *)(block) - 1)
#define BYTES(header) ((void *)((cm_block *)(header) + 1))
#define COST(size) ((uint64_t)sizeof(cm_block) + (uint64_t)(size))

static void
initLists(void)
{
	int i;

	for (i = 0; i < 3; i++) {
		if (!lists[i].next) {
			lists[i].prev = lists[i].next = &lists[i];
		}
	}
}

static void
linkInto(cm_block *b, size_t list)
{
	cm_block *head = &lists[list];

	b->list = list;
	b->prev = head->prev;
	b->next = head;
	head->prev->next = b;
	head->prev = b;
	counts[list]++;
	charged += COST(b->size);
}

static void
unlinkFrom(cm_block *b)
{
	b->prev->next = b->next;
	b->next->prev = b->prev;
	counts[b->list]--;
	charged -= COST(b->size);
	b->prev = b->next = NULL;
	b->list = NO_LIST;
}

void
cm_track_fail(int code)
{
	CM_ASSERT(call != NULL);
	call->limit = code;
	longjmp(landing, 1);
}

/* Checks that the call may hold size more bytes after freeing old */
static void
charge(uint64_t oldCost, uint64_t newCost)
{
	uint64_t total = charged - oldCost + call->kept;

	if (newCost > call->budget || total > call->budget - newCost) {
		cm_track_fail(CM_CLIM_BUDGET);
	}
}

static void *
allocate(size_t size, size_t list, int zero)
{
	cm_block *b;

	CM_ASSERT(call != NULL);
	charge(0, COST(size));
	b = zero ? calloc(1, sizeof(cm_block) + size) : malloc(sizeof(cm_block) + size);
	if (!b) {
		cm_track_fail(CM_CLIM_MALLOC);
	}
	b->size = size;
	linkInto(b, list);
	return BYTES(b);
}

static void *
reallocate(void *block, size_t size, size_t list)
{
	cm_block *b, *nb;

	if (!block) {
		return allocate(size, list, 0);
	}
	CM_ASSERT(call != NULL);
	b = HEADER(block);
	CM_ASSERT(b->list == list);
	charge(COST(b->size), COST(size));
	/* realloc copies prev and next; the neighbours are fixed after, so a
	   NULL answer leaves the list as it was (b still in it) */
	nb = realloc(b, sizeof(cm_block) + size);
	if (!nb) {
		cm_track_fail(CM_CLIM_MALLOC);
	}
	nb->prev->next = nb;
	nb->next->prev = nb;
	charged = charged - COST(nb->size) + COST(size);
	nb->size = size;
	return BYTES(nb);
}

static void
release(void *block)
{
	cm_block *b;

	if (!block) {
		return;
	}
	b = HEADER(block);
	CM_ASSERT(b->list == CMARK_LIST || b->list == OWN_LIST);
	unlinkFrom(b);
	free(b);
}


/*** cmark's allocator ***/

static void *
trackCalloc(size_t count, size_t size)
{
	if (size && count > SIZE_MAX / size) {
		cm_track_fail(CM_CLIM_BLOCK);
	}
	if (count * size > maxBlock) {
		cm_track_fail(CM_CLIM_BLOCK);
	}
	return allocate(count * size, CMARK_LIST, 1);
}

static void *
trackRealloc(void *block, size_t size)
{
	if (size > maxBlock) {
		cm_track_fail(CM_CLIM_BLOCK);
	}
	return reallocate(block, size, CMARK_LIST);
}

static void
trackFree(void *block)
{
	release(block);
}

static cmark_mem trackMem = { trackCalloc, trackRealloc, trackFree };

void *
cm_track_allocator(void)
{
	return &trackMem;
}


/*** The engine's own blocks ***/

void *
cm_own_alloc(size_t size)
{
	return allocate(size, OWN_LIST, 0);
}

void *
cm_own_realloc(void *block, size_t size)
{
	return reallocate(block, size, OWN_LIST);
}

void
cm_own_free(void *block)
{
	release(block);
}

void
cm_track_detach(void *block)
{
	cm_block *b = HEADER(block);

	CM_ASSERT(b->list == CMARK_LIST);
	unlinkFrom(b);
	linkInto(b, OWN_LIST);
}

void
cm_own_keep(void *block)
{
	cm_block *b;

	if (!block) {
		return;
	}
	b = HEADER(block);
	CM_ASSERT(b->list == OWN_LIST);
	unlinkFrom(b);
}

void
cm_block_free(void *block)
{
	cm_block *b;

	if (!block) {
		return;
	}
	b = HEADER(block);
	CM_ASSERT(b->list == NO_LIST);
	free(b);
}

size_t
cm_block_bytes(const void *block)
{
	return block ? (size_t)COST(((const cm_block *)block - 1)->size) : 0;
}


/*** The call ***/

void
cm_track_begin(cm_call *ctx)
{
	initLists();
	CM_ASSERT(call == NULL);
	CM_ASSERT(counts[CMARK_LIST] == 0 && counts[OWN_LIST] == 0 && charged == 0);
	call = ctx;
	call->limit = 0;
}

void
cm_track_end(void)
{
	CM_ASSERT(call != NULL);
	CM_ASSERT(counts[CMARK_LIST] == 0 && counts[OWN_LIST] == 0 && charged == 0);
	call = NULL;
}

void
cm_track_set_kept(uint64_t keptBytes)
{
	CM_ASSERT(call != NULL);
	call->kept = keptBytes;
}

void
cm_track_set_max_block(size_t size)
{
	maxBlock = size ? size : CM_MAX_BLOCK_BYTES;
}

static size_t
sweep(size_t list)
{
	cm_block *head = &lists[list];
	size_t n = 0;

	initLists();
	while (head->next != head) {
		cm_block *b = head->next;
		unlinkFrom(b);
		free(b);
		n++;
	}
	return n;
}

size_t
cm_track_sweep(void)
{
	return sweep(CMARK_LIST);
}

size_t
cm_own_sweep(void)
{
	return sweep(OWN_LIST);
}

size_t
cm_track_blocks(void)
{
	return counts[CMARK_LIST] + counts[OWN_LIST];
}

int
cm_guarded(int (*work)(cm_call *), cm_call *ctx)
{
	if (setjmp(landing) != 0) {
		return CM_LANDED;
	}
	return work(ctx);
}
