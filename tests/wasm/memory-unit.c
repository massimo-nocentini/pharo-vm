/* memory-unit.c -- unit tests of the Spur heap allocation for WebAssembly
 *
 * Includes src/emscripten/memoryEmscripten.c (its "pharovm/pharo.h" is the
 * stub tests/wasm/lanes/56-memory-unit.sh writes) and drives it as Spur
 * does: stack pages and new space at fixed addresses, the first old-space
 * segment at the 64-bit hint, and every later segment at the end of the
 * last one or at the first gap that holds it, with malloc taking memory at
 * the break in between.  It checks that
 *
 *  - the fixed spaces and the first segment go exactly where asked;
 *  - a segment freed at the top grows with the break: segments of 32, 64,
 *    128 and 256 MiB, each freed before the next, all go at one address;
 *  - a reused segment is zeros, both the part that was dirty and the part
 *    the break gave;
 *  - a free range below memory that malloc took at the break is not grown
 *    into it;
 *  - old space grows up to the end of its window [base, 2 * base) and never
 *    past it, and the hint at that end and the perm-space hint get nothing.
 *
 * With the argument 'big', it checks instead that a first segment larger
 * than the window gets nothing (the statics of the allocator allow one
 * first segment per run).
 *
 * Prints one line per failed check and a summary; exits 1 when a check
 * failed.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "../../src/emscripten/memoryEmscripten.c"

#define MiB ((usqInt)1 << 20)
#define FIRST_SEGMENT ((usqInt)70516736)	/* the stock image's */

static int checks, failures;

#define CHECK(condition) check((condition), #condition, __LINE__)

static void
check(int ok, const char *what, int line)
{
	checks += 1;
	if (!ok) {
		failures += 1;
		printf("FAIL memory-unit.c:%d: %s\n", line, what);
	}
}

/* Spur's segments, by address */
typedef struct { usqInt start, size; } Segment;
static Segment segments[256];
static int numSegments;

/* SpurSegmentManager>>firstGapOfSizeAtLeast: the end of the first segment
 * followed by a gap that holds size, or of the last one */
static usqInt
firstGapOfSizeAtLeast(usqInt size)
{
	for (int i = 0; i + 1 < numSegments; i++)
		if (segments[i + 1].start - (segments[i].start + segments[i].size) >= size)
			return segments[i].start + segments[i].size;
	return segments[numSegments - 1].start + segments[numSegments - 1].size;
}

static usqInt
addSegment(usqInt size)
{
	usqInt start = sqAllocateMemory(size, size, firstGapOfSizeAtLeast(size));
	int i = numSegments;

	if (!start)
		return 0;
	while (i > 0 && segments[i - 1].start > start) {
		segments[i] = segments[i - 1];
		i--;
	}
	segments[i] = (Segment){start, size};
	numSegments++;
	return start;
}

static void
removeSegment(usqInt start)
{
	for (int i = 0; i < numSegments; i++)
		if (segments[i].start == start) {
			sqDeallocateMemorySegmentAtOfSize((void *)start, segments[i].size);
			memmove(&segments[i], &segments[i + 1], (numSegments - i - 1) * sizeof(Segment));
			numSegments--;
			return;
		}
}

/* Whether every page of [start, start + size) starts and ends with zeros */
static int
isZeros(usqInt start, usqInt size)
{
	for (usqInt p = start; p < start + size; p += 4096)
		if (*(unsigned char *)p || *(unsigned char *)(p + 4095))
			return 0;
	return 1;
}

static void
initialSpaces(void)
{
	usqInt newSpace = sqAllocateMemory(0, 22872064, 0x8000000);
	usqInt oldSpace = sqAllocateMemory(0, FIRST_SEGMENT, OLD_SPACE_HINT);

	CHECK(newSpace == 0x8000000);
	CHECK(oldSpace == 0x20000000);
	CHECK(oldSpaceBase == 0x20000000);
	segments[numSegments++] = (Segment){oldSpace, FIRST_SEGMENT};
}

static void
regrowAndZeros(void)
{
	usqInt first = 0, start;
	void *malloced;

	/* malloc at the break, below the next segment, which leaves it unaligned */
	CHECK(malloc(100000) != NULL);
	for (usqInt size = 32 * MiB; size <= 256 * MiB; size *= 2) {
		start = addSegment(size);
		CHECK(start != 0);
		if (!start)
			return;
		CHECK(start % WASM_PAGE_SIZE == 0);
		if (!first)
			first = start;
		CHECK(start == first);
		CHECK(isZeros(start, size));
		memset((void *)start, 0xAB, size);
		removeSegment(start);
	}
	/* the dirty 256 MiB and 64 MiB more from the break */
	start = addSegment(320 * MiB);
	CHECK(start == first);
	CHECK(start && isZeros(start, 320 * MiB));
	if (start) {
		memset((void *)start, 0xAB, 320 * MiB);
		removeSegment(start);
	}
	/* malloc takes the break above the free range: the next segment does not
	 * grow the range into it */
	malloced = malloc(32 * MiB);
	CHECK(malloced != NULL);
	start = addSegment(32 * MiB);	/* in the free range */
	CHECK(start == first);
	if (start)
		removeSegment(start);
	if (malloced && (usqInt)malloced > first && (usqInt)malloced < 2 * oldSpaceBase) {
		start = addSegment(((usqInt)malloced - first) + 16 * MiB);
		CHECK(start == 0 || start + ((usqInt)malloced - first) + 16 * MiB <= (usqInt)malloced
			|| start >= (usqInt)malloced + 32 * MiB);
		if (start)
			removeSegment(start);
	}
	free(malloced);
}

static void
window(void)
{
	usqInt start, end = 0;
	int grown = 0;

	while (grown < 100 && (start = addSegment(16 * MiB))) {
		CHECK(start >= oldSpaceBase && start + 16 * MiB <= 2 * oldSpaceBase);
		if (start + 16 * MiB > end)
			end = start + 16 * MiB;
		grown++;
	}
	CHECK(grown < 100);
	/* it filled the window, less what malloc took and a segment */
	CHECK(end > 2 * oldSpaceBase - 64 * MiB);
	CHECK(sqAllocateMemory(0, 16 * MiB, 2 * oldSpaceBase) == 0);
	CHECK(sqAllocateMemory(0, 16 * MiB, 3 * oldSpaceBase) == 0);
	CHECK(sqAllocateMemory(0, 10 * MiB, PERM_SPACE_HINT) == 0);
	printf("memory-unit: %d segments of 16 MiB, old space up to %p of [%p, %p)\n",
		grown, (void *)end, (void *)oldSpaceBase, (void *)(2 * oldSpaceBase));
}

int
main(int argc, char **argv)
{
	if (argc > 1 && strcmp(argv[1], "big") == 0) {
		sqAllocateMemory(0, 22872064, 0x8000000);
		CHECK(sqAllocateMemory(0, 0x20000000 + WASM_PAGE_SIZE, OLD_SPACE_HINT) == 0);
		CHECK(oldSpaceBase == 0x20000000);
	}
	else {
		initialSpaces();
		regrowAndZeros();
		window();
	}
	printf("memory-unit%s: %d checks, %d failed\n", argc > 1 ? " big" : "", checks, failures);
	return failures != 0;
}
