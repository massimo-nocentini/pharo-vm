/* memoryEmscripten.c -- Spur heap allocation for Emscripten (WebAssembly)
 *
 * Replaces src/unix/memoryUnix.c.  Emscripten (6.0.10,
 * system/lib/libc/emscripten_mmap.c):
 *  - mmap() returns EINVAL for any non-NULL address hint, MAP_FIXED or not;
 *  - munmap() only accepts a whole mapping (same address and length);
 *  - linear memory is the single range [0, heap size), grown only at the top
 *    by sbrk().
 * The Spur memory map wants stack pages and new space at fixed addresses
 * (64MiB and 128MiB when targeting Emscripten, checked by VMMemoryMap), old
 * space at a power of two above new space (so that calculateMaskToUse can tell
 * young from old by address bits), and old-space segments above the first one
 * and inside the old window [oldSpaceBase, 2 * oldSpaceBase).  So the VM's
 * address space is managed here on top of sbrk():
 *  - an exact request at or above the break moves the break; the range it
 *    skips becomes a hole the VM owns (never touched, so it costs nothing
 *    until it is used);
 *  - a request that falls inside a hole is carved out of it, and a hole that
 *    ends at the break, such as a freed segment at the top, grows with the
 *    break when the request runs past it;
 *  - freed segments become (dirty) holes and are zeroed again when reused.
 * Old space never leaves its window: the masks tell old objects by their
 * address, so the write barrier would not see an object past it.  A request
 * that does not fit fails, and Spur reports an OutOfMemory.
 * dlmalloc (HAVE_MMAP 0, MORECORE_CANNOT_TRIM 1) copes with these foreign
 * sbrk()s: its next sbrk() result simply becomes a new malloc segment.
 *
 * The 64-bit memory map asks for old space at 2^40 (a hint); it goes at
 * PHARO_WASM_OLD_SPACE_BASE instead, 512MiB unless the build or the
 * environment variable of the same name says otherwise.  Everything stays
 * below 4GiB, so the same layout works with -sMEMORY64=1 and =2.
 */

#include "pharovm/pharo.h"
#include <unistd.h>
#include <string.h>
#include <emscripten/heap.h>

#define alignUpToPage(v) (((usqInt)(v) + WASM_PAGE_SIZE - 1) & ~((usqInt)WASM_PAGE_SIZE - 1))

#ifndef PHARO_WASM_OLD_SPACE_BASE
# define PHARO_WASM_OLD_SPACE_BASE 0x20000000	/* 512MiB: old space in [512MiB, 1GiB) */
#endif

/* The addresses the 64-bit memory map asks old space and perm space for;
 * the perm-space one (2^41) is beyond any wasm memory and is refused.
 */
#define OLD_SPACE_HINT ((usqInt)0x10000000000ULL)
#define PERM_SPACE_HINT ((usqInt)0x20000000000ULL)

typedef struct { usqInt start, end; int dirty; } Range;
#define MAX_RANGES 1024
static Range holes[MAX_RANGES];
static int numHoles = 0;
static Range blocks[MAX_RANGES];
static int numBlocks = 0;

static usqInt oldSpaceBase = 0;
static usqInt fixedSpaceEnd = 0;	/* the end of stack pages and new space */

static void
removeAt(Range *ranges, int *count, int i)
{
	memmove(&ranges[i], &ranges[i + 1], (*count - i - 1) * sizeof(Range));
	(*count)--;
}

static void
addHole(usqInt start, usqInt end, int dirty)
{
	int i = 0;

	if (start >= end || numHoles == MAX_RANGES)
		return;	/* leak rather than fail */
	while (i < numHoles && holes[i].start < start)
		i++;
	memmove(&holes[i + 1], &holes[i], (numHoles - i) * sizeof(Range));
	holes[i] = (Range){start, end, dirty};
	numHoles++;
	if (i + 1 < numHoles && holes[i].end == holes[i + 1].start) {
		holes[i].end = holes[i + 1].end;
		holes[i].dirty |= holes[i + 1].dirty;
		removeAt(holes, &numHoles, i + 1);
	}
	if (i > 0 && holes[i - 1].end == holes[i].start) {
		holes[i - 1].end = holes[i].end;
		holes[i - 1].dirty |= holes[i].dirty;
		removeAt(holes, &numHoles, i);
	}
}

/* Whether hole i holds [start, end), or would once the break moved up to
 * end: a hole that ends at the break grows with it.
 */
static int
holds(int i, usqInt start, usqInt end)
{
	if (holes[i].start > start)
		return 0;
	if (end <= holes[i].end)
		return 1;
	return start < holes[i].end
		&& holes[i].end == (usqInt)sbrk(0)
		&& end <= (usqInt)emscripten_get_heap_max();
}

/* [start, start + size) out of hole i, which holds it, or 0. */
static usqInt
carve(int i, usqInt start, usqInt size)
{
	Range hole = holes[i];
	usqInt end = start + size;

	/* past the end of a hole that ends at the break: move the break */
	if (end > hole.end && sbrk((intptr_t)(end - hole.end)) == (void *)-1)
		return 0;
	removeAt(holes, &numHoles, i);
	addHole(hole.start, start, hole.dirty);
	addHole(end, hole.end, hole.dirty);
	if (hole.dirty)	/* keep mmap's zero-fill semantics (the new break is zeros) */
		memset((void *)start, 0, (end < hole.end ? end : hole.end) - start);
	return start;
}

/* Exactly [start, start + size), or 0. */
static usqInt
allocateExactly(usqInt start, usqInt size)
{
	usqInt brk;

	for (int i = 0; i < numHoles; i++)
		if (holds(i, start, start + size))
			return carve(i, start, size);
	brk = (usqInt)sbrk(0);
	if (start < brk || start + size > (usqInt)emscripten_get_heap_max())
		return 0;
	if (sbrk((intptr_t)(start + size - brk)) == (void *)-1)
		return 0;
	addHole(brk, start, 0);
	return start;
}

/* The lowest [start, start + size) with low <= start and start + size <= high, or 0. */
static usqInt
allocateAbove(usqInt low, usqInt high, usqInt size)
{
	usqInt start;

	for (int i = 0; i < numHoles; i++) {
		/* (a hole may start at a break that malloc left unaligned) */
		start = alignUpToPage(holes[i].start > low ? holes[i].start : low);
		if (start + size <= high && holds(i, start, start + size))
			return carve(i, start, size);
	}
	start = alignUpToPage(sbrk(0));
	if (start < low)
		start = low;
	return start + size <= high ? allocateExactly(start, size) : 0;
}

/* Where old space goes: PHARO_WASM_OLD_SPACE_BASE from the environment, or
 * the build's default.  Old space needs a power of two above new space, and
 * its window [base, 2 * base) must fit in the maximum memory.
 */
static usqInt
computeOldSpaceBase(void)
{
	char *setting = getenv("PHARO_WASM_OLD_SPACE_BASE");
	char *end = NULL;
	usqInt base = (usqInt)PHARO_WASM_OLD_SPACE_BASE;

	if (setting && *setting)
		base = (usqInt)strtoull(setting, &end, 0);
	if ((end && *end)
	 || base == 0
	 || (base & (base - 1)) != 0
	 || base <= fixedSpaceEnd
	 || base > (usqInt)emscripten_get_heap_max() / 2) {
		logError("Invalid PHARO_WASM_OLD_SPACE_BASE %s: old space needs a power of two above the end of new space (%p) and at most half the maximum memory (%p)",
			setting && *setting ? setting : "(build default)",
			(void *)fixedSpaceEnd, (void *)((usqInt)emscripten_get_heap_max() / 2));
		exit(1);
	}
	return base;
}

usqInt
sqAllocateMemory(usqInt minHeapSize, usqInt desiredHeapSize, usqInt desiredBaseAddress)
{
	usqInt size = alignUpToPage(desiredHeapSize ? desiredHeapSize : 1);
	usqInt base = alignUpToPage(desiredBaseAddress);
	usqInt result;
	int oldSpace = 0;

	if (base == OLD_SPACE_HINT) {
		/* the first old-space segment: exactly at the power of two, or the
		 * masks computed from its address would be wrong */
		if (!oldSpaceBase)
			oldSpaceBase = computeOldSpaceBase();
		result = size <= oldSpaceBase ? allocateExactly(oldSpaceBase, size) : 0;
		oldSpace = 1;
	}
	else if (base >= PERM_SPACE_HINT)
		result = 0;	/* perm space: out of reach */
	else if (oldSpaceBase && base >= oldSpaceBase) {
		/* more old space: at the hint or above it, inside the window (a hint
		 * at or past its end, when old space fills it, gets nothing) */
		result = allocateAbove(base, 2 * oldSpaceBase, size);
		oldSpace = 1;
	}
	else if (base) {
		/* stack pages and new space: exactly where the memory map wants them */
		result = allocateExactly(base, size);
		if (result && result + size > fixedSpaceEnd)
			fixedSpaceEnd = result + size;
	}
	else
		result = allocateAbove(0, (usqInt)emscripten_get_heap_max(), size);

	if (result && numBlocks < MAX_RANGES)
		blocks[numBlocks++] = (Range){result, result + size, 0};
	if (!result && oldSpace)
		logError("Cannot allocate %lu bytes of old space at %p (break %p, maximum memory %lu): old space must fit in [%p, %p) (PHARO_WASM_OLD_SPACE_BASE moves it)",
			(unsigned long)size, (void *)desiredBaseAddress, sbrk(0),
			(unsigned long)emscripten_get_heap_max(),
			(void *)oldSpaceBase, (void *)(2 * oldSpaceBase));
	else if (!result && base >= PERM_SPACE_HINT)
		logError("Cannot allocate %lu bytes of perm space at %p: WebAssembly memory cannot reach it (perm space is not supported)",
			(unsigned long)size, (void *)desiredBaseAddress);
	else if (!result)
		logError("Cannot allocate %lu bytes at %p (break %p, maximum memory %lu)",
			(unsigned long)size, (void *)desiredBaseAddress, sbrk(0),
			(unsigned long)emscripten_get_heap_max());
	logDebug("Allocated %lu bytes at %p for %p (break %p, heap %lu)",
		(unsigned long)size, (void *)result, (void *)desiredBaseAddress, sbrk(0),
		(unsigned long)emscripten_get_heap_size());
	return result;
}

/* Free what sqAllocateMemory recorded at addr.  The recorded size wins over
 * sz: savedFirstFieldsSpace is released with limit - start, which may be less
 * than what was allocated (SpurPlanningCompactor).
 */
void
sqDeallocateMemorySegmentAtOfSize(void *addr, sqInt sz)
{
	for (int i = 0; i < numBlocks; i++)
		if (blocks[i].start == (usqInt)addr) {
			addHole(blocks[i].start, blocks[i].end, 1);
			blocks[i] = blocks[--numBlocks];
			return;
		}
	logError("Cannot free the unknown segment %p (%ld bytes)", addr, (long)sz);
}

/* There is no JIT in WebAssembly, and the StackVM never asks for code memory
 * (its code zone size is 0).
 */
void *
allocateJITMemory(usqInt desiredSize, usqInt desiredPosition)
{
	logError("There is no JIT in WebAssembly");
	return 0;
}
