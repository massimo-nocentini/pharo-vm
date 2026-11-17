/* enginetest.c -- the checks of the engine (tsEngine.h), run by
 * run-c-tests.sh in the lanes engine (clang -O2), engine-asan (gcc, ASan and
 * UBSan) and engine-wasm64 (emcc -m64, under node).
 *
 * The references are written here, independently of timsort.c: a stable
 * bottom-up merge sort (the order of equal elements is the input's, so a
 * stable sort has exactly one result, which the engine's must equal word
 * for word), the order of each kind decoded from its bits, and an exact
 * order of numbers that scales every number to an integer (a double is
 * k * 2^-1074 exactly) and compares big integers.  The checks:
 *
 *   E1  ts_sort_words: sizes 0-70 and around the merge buffer's 256 and 512,
 *       up to 1,000,000; patterns random, few unique, sorted, reversed,
 *       equal, nearly sorted, sawtooth, organ pipe, runs; ascending and
 *       descending against the stable reference (records whose key ties);
 *       n - 1 comparisons on sorted and reversed input; no comparison for
 *       n < 2; an abort at every k-th comparison (TS_ABORTED at once, no
 *       further call, a permutation left); arguments outside the contract;
 *       a sort run from inside a comparison
 *   E2  ts_sort_buffer: the eight kinds, values drawn around their edges
 *       (NaNs of either sign and any payload, infinities, -0.0, subnormals,
 *       the extremes of each integer type), ascending and descending, sizes
 *       0-40 and up to 1,000,000: the bits equal the reference's; NaN last
 *       (first when descending); arguments outside the contract (kinds 0,
 *       9, -1, n < 0, NULL, misaligned)
 *   E3  ts_argsort_buffer: every kind, words of 0-based indices (shift 0)
 *       and of SmallInteger oops of 1-based ones (shift 3, origin 1), sub-
 *       ranges and repeated indices, up to 1,000,000 keys (descending: 70,000
 *       without --slow), against the reference; a word outside
 *       the keys (perm untouched, whatever n); arguments outside the
 *       contract
 *   E4  ts_number_compare: every pair of a set of edges (critic.md §4.2:
 *       +-(2^53 +- 1), 2^60 - 1, 2^60, 2^63, 2^64, 2^1023, 2^1024, +-inf, the
 *       smallest subnormal, -0.0 against 0 and 0.0, and more) as integers,
 *       LargeIntegers (also with high zero bytes, and zero) and doubles
 *       (with their neighbours), and random pairs, against the exact
 *       reference; antisymmetry; sorts of numbers against the reference
 *   E5  ts_string_compare: byte and wide strings, NULs, units 0x80, 0xFF,
 *       0x80000000, 0xFFFFFFFF, prefixes, empty strings with NULL units,
 *       against a reference unit by unit; sorts of strings
 *   E6  no memory (glibc only, without AddressSanitizer: malloc is wrapped
 *       here): the merge buffer or the widened copy refused gives
 *       TS_NO_MEMORY, the narrow buffers untouched, the others permuted
 *
 * Usage: enginetest [--only E1,E2,...] [--seed N] [--slow]
 * (--slow: sizes up to 4,000,000 and more random pairs.)  Prints the
 * timings of the sorts of 1,000,000 random elements, one line per failed
 * check (the first 50), and "enginetest ok: N checks, 0 failures"; exits 1
 * when a check failed.
 */

#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "tsEngine.h"

#ifndef __has_feature
# define __has_feature(x) 0
#endif

static long checks, failures;
static const char *section = "";

static void
fail(const char *format, ...)
{
	va_list args;

	if (++failures <= 50) {
		va_start(args, format);
		printf("%s FAILED: ", section);
		vprintf(format, args);
		printf("\n");
		va_end(args);
	}
}

#define CHECK(condition, ...) do { checks++; if (!(condition)) fail(__VA_ARGS__); } while (0)

/* xorshift64*, seeded */
static uint64_t seed = 0x54696d536f727431ull;

static uint64_t
next(void)
{
	seed ^= seed >> 12;
	seed ^= seed << 25;
	seed ^= seed >> 27;
	return seed * 0x2545F4914F6CDD1Dull;
}

static uint64_t
below(uint64_t n)
{
	return n ? next() % n : 0;
}

static int slow;

static double
seconds(void)
{
	return (double)clock() / CLOCKS_PER_SEC;
}

static void *
xmalloc(size_t size)
{
	void *p = malloc(size ? size : 1);

	if (!p) {
		fprintf(stderr, "enginetest: out of memory\n");
		exit(2);
	}
	return p;
}


/*** Refused allocations: a wrapper of glibc's malloc ***/

#if defined(__GLIBC__) && !defined(__SANITIZE_ADDRESS__) && !__has_feature(address_sanitizer) \
	&& !defined(__EMSCRIPTEN__)
# define MALLOC_HOOK 1
extern void *__libc_malloc(size_t size);
static long mallocsBeforeFailure = -1;	/* the mallocs that succeed before one fails, -1: none fails */
static long mallocs;			/* while armed */

void *
malloc(size_t size)
{
	if (mallocsBeforeFailure >= 0) {
		mallocs++;
		if (mallocsBeforeFailure-- == 0) {
			return NULL;	/* once: then disarmed (-1) */
		}
	}
	return __libc_malloc(size);
}

static void
refuseMallocAfter(long count)
{
	mallocs = 0;
	mallocsBeforeFailure = count;
}
#else
# define MALLOC_HOOK 0
#endif


/*** The reference: a stable bottom-up merge sort ***/

/* Sorts n words by less (strict) stably; descending: by less of the
 * swapped pair, equal words keeping their order */
static void
referenceSort(int64_t *words, size_t n, int descending, ts_less_fn less, void *arg)
{
	int64_t *from = words, *to = xmalloc(n * sizeof *to), *swap;
	size_t width, i;

	for (width = 1; width < n; width *= 2) {
		for (i = 0; i < n; i += 2 * width) {
			size_t mid = i + width < n ? i + width : n, end = i + 2 * width < n ? i + 2 * width : n;
			size_t l = i, r = mid, k = i;
			while (l < mid && r < end) {
				int rightFirst = descending ? less(from[l], from[r], arg) > 0 : less(from[r], from[l], arg) > 0;
				to[k++] = rightFirst ? from[r++] : from[l++];
			}
			while (l < mid) {
				to[k++] = from[l++];
			}
			while (r < end) {
				to[k++] = from[r++];
			}
		}
		swap = from;
		from = to;
		to = swap;
	}
	if (from != words) {
		memcpy(words, from, n * sizeof *words);
		free(from);
	} else {
		free(to);
	}
}

static int
compareWords(const void *a, const void *b)
{
	int64_t x = *(const int64_t *)a, y = *(const int64_t *)b;

	return (x > y) - (x < y);
}

/* Whether the n words of a are a permutation of those of b */
static int
isPermutation(const int64_t *a, const int64_t *b, size_t n)
{
	int64_t *x = xmalloc(n * sizeof *x), *y = xmalloc(n * sizeof *y);
	int same;

	memcpy(x, a, n * sizeof *x);
	memcpy(y, b, n * sizeof *y);
	qsort(x, n, sizeof *x, compareWords);
	qsort(y, n, sizeof *y, compareWords);
	same = n == 0 || memcmp(x, y, n * sizeof *x) == 0;
	free(x);
	free(y);
	return same;
}


/*** E1: words ***/

/* Records: the key in the high half, the index of the input in the low
 * half, compared by key only */
typedef struct {
	long calls;
	long abortAt;		/* the call that answers -1, 0: none */
	long callsAfterAbort;
	int aborted;
	int nested;		/* sort a few words from inside every 97th call */
} Counter;

static int
keyLess(int64_t a, int64_t b, void *arg)
{
	Counter *c = arg;

	if (c) {
		c->calls++;
		if (c->aborted) {
			c->callsAfterAbort++;
		}
		if (c->abortAt && c->calls == c->abortAt) {
			c->aborted = 1;
			return -1;
		}
		if (c->nested && c->calls % 97 == 0) {
			int64_t inner[40];
			int k, rc;
			for (k = 0; k < 40; k++) {
				inner[k] = (int64_t)((uint64_t)(next() % 7) << 32) | k;
			}
			rc = ts_sort_words(inner, 40, (int)(c->calls & 1), keyLess, NULL);
			for (k = 1; k < 40; k++) {
				int64_t x = inner[k - 1] >> 32, y = inner[k] >> 32;
				if (rc != TS_OK || (c->calls & 1 ? x < y : x > y)
				 || (x == y && (inner[k - 1] & 0xFFFFFFFF) > (inner[k] & 0xFFFFFFFF))) {
					c->nested = -1;	/* the inner sort was wrong */
				}
			}
		}
	}
	return (a >> 32) < (b >> 32);
}

enum { RANDOM, FEW, SORTED, REVERSED, EQUAL, NEARLY, SAWTOOTH, ORGAN, RUNS, PATTERNS };
static const char *patternNames[] = { "random", "few unique", "sorted", "reversed", "equal",
	"nearly sorted", "sawtooth", "organ pipe", "runs" };

static int64_t
keyOf(int pattern, size_t i, size_t n)
{
	switch (pattern) {
	case RANDOM: return (int64_t)below(n + 1);
	case FEW: return (int64_t)below(4);
	case SORTED: return (int64_t)i;
	case REVERSED: return (int64_t)(n - i);
	case EQUAL: return 7;
	case NEARLY: return (int64_t)i;	/* then swaps */
	case SAWTOOTH: return (int64_t)(i % 37);
	case ORGAN: return (int64_t)(i < n / 2 ? i : n - i);
	default: return (int64_t)((i / 100) % 2 ? i : n - i);	/* runs up and down */
	}
}

static void
makeRecords(int64_t *words, size_t n, int pattern)
{
	size_t i;

	for (i = 0; i < n; i++) {
		words[i] = (int64_t)((uint64_t)keyOf(pattern, i, n) << 32) | (int64_t)i;
	}
	if (pattern == NEARLY) {
		for (i = 0; i < n / 50 + 1 && n > 1; i++) {
			size_t a = below(n), b = below(n);
			int64_t ka = words[a] >> 32, kb = words[b] >> 32;
			words[a] = (int64_t)((uint64_t)kb << 32) | (int64_t)a;
			words[b] = (int64_t)((uint64_t)ka << 32) | (int64_t)b;
		}
	}
}

static void
checkWordsOnce(size_t n, int pattern, int descending)
{
	int64_t *words = xmalloc(n * sizeof *words), *expected = xmalloc(n * sizeof *expected);
	Counter c;
	int rc;

	makeRecords(words, n, pattern);
	memcpy(expected, words, n * sizeof *words);
	referenceSort(expected, n, descending, keyLess, NULL);
	memset(&c, 0, sizeof c);
	rc = ts_sort_words(words, (int64_t)n, descending, keyLess, &c);
	CHECK(rc == TS_OK, "words %zu %s%s: rc %d", n, patternNames[pattern], descending ? " descending" : "", rc);
	CHECK(n == 0 || memcmp(words, expected, n * sizeof *words) == 0, "words %zu %s%s: not the stable order",
		n, patternNames[pattern], descending ? " descending" : "");
	if (n < 2) {
		CHECK(c.calls == 0, "words %zu: %ld comparisons", n, c.calls);
	} else if (!descending && (pattern == SORTED || pattern == REVERSED || pattern == EQUAL)) {
		CHECK(c.calls == (long)n - 1, "words %zu %s: %ld comparisons, not n - 1", n, patternNames[pattern], c.calls);
	}
	free(words);
	free(expected);
}

static void
checkWords(void)
{
	static const size_t sizes[] = { 255, 256, 257, 511, 512, 513, 1000, 1025, 4096, 10007, 65536, 100000 };
	size_t i, n, k;
	int pattern, descending;
	int64_t words[4] = { 3, 1, 2, 0 };
	Counter c;
	double t;

	section = "E1";
	for (n = 0; n <= 70; n++) {
		for (pattern = 0; pattern < PATTERNS; pattern++) {
			for (descending = 0; descending < 2; descending++) {
				checkWordsOnce(n, pattern, descending);
			}
		}
	}
	for (i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
		for (pattern = 0; pattern < PATTERNS; pattern++) {
			for (descending = 0; descending < 2; descending++) {
				checkWordsOnce(sizes[i], pattern, descending);
			}
		}
	}
	t = seconds();
	for (pattern = RANDOM; pattern <= REVERSED; pattern++) {
		checkWordsOnce(1000000, pattern, 0);
		checkWordsOnce(1000000, pattern, 1);
	}
	if (slow) {
		for (pattern = 0; pattern < PATTERNS; pattern++) {
			checkWordsOnce(4000000, pattern, 0);
			checkWordsOnce(4000000, pattern, 1);
		}
	}
	printf("E1 words: 1,000,000 records of 4 patterns, both directions, with the reference: %.2f s\n",
		seconds() - t);

	/* aborts: at every k-th comparison of a few sorts */
	for (k = 1; k <= 3000; k += (k < 40 ? 1 : 97)) {
		for (descending = 0; descending < 2; descending++) {
			size_t m = 600;
			int64_t *a = xmalloc(m * sizeof *a), *original = xmalloc(m * sizeof *a);
			int rc;
			makeRecords(a, m, RANDOM);
			memcpy(original, a, m * sizeof *a);
			memset(&c, 0, sizeof c);
			c.abortAt = (long)k;
			rc = ts_sort_words(a, (int64_t)m, descending, keyLess, &c);
			if (c.aborted) {
				CHECK(rc == TS_ABORTED, "abort at %zu: rc %d", k, rc);
				CHECK(c.callsAfterAbort == 0, "abort at %zu: %ld calls after it", k, c.callsAfterAbort);
			} else {
				CHECK(rc == TS_OK, "no abort (%ld calls < %zu): rc %d", c.calls, k, rc);
			}
			CHECK(isPermutation(a, original, m), "abort at %zu: not a permutation", k);
			free(a);
			free(original);
		}
	}
	/* outside the contract */
	CHECK(ts_sort_words(words, -1, 0, keyLess, NULL) == TS_BAD_ARGUMENT, "n < 0");
	CHECK(ts_sort_words(NULL, 2, 0, keyLess, NULL) == TS_BAD_ARGUMENT, "words NULL");
	CHECK(ts_sort_words(words, 2, 0, NULL, NULL) == TS_BAD_ARGUMENT, "less NULL");
	CHECK(ts_sort_words(NULL, 0, 0, NULL, NULL) == TS_OK, "nothing to sort");
	CHECK(ts_sort_words(words, 1, 0, NULL, NULL) == TS_OK, "one word, no less");
	CHECK(words[0] == 3 && words[1] == 1, "the words of the refused calls changed");
	/* a sort from inside the comparisons */
	{
		size_t m = 5000;
		int64_t *a = xmalloc(m * sizeof *a), *expected = xmalloc(m * sizeof *a);
		makeRecords(a, m, RANDOM);
		memcpy(expected, a, m * sizeof *a);
		referenceSort(expected, m, 0, keyLess, NULL);
		memset(&c, 0, sizeof c);
		c.nested = 1;
		CHECK(ts_sort_words(a, (int64_t)m, 0, keyLess, &c) == TS_OK, "nested: rc");
		CHECK(c.nested == 1, "nested: an inner sort was wrong");
		CHECK(memcmp(a, expected, m * sizeof *a) == 0, "nested: the outer sort is wrong");
		free(a);
		free(expected);
	}
}


/*** E2, E3: buffers and argsort ***/

static const char *kindNames[] = { "?", "f64", "f32", "i64", "u64", "i32", "u32", "u16", "u8" };

/* The order of the kinds, from the bits of two elements of the buffer at
 * arg (the reference's: decoded with the C types) */
typedef struct {
	const unsigned char *base;
	int kind;
	int shift;		/* words: (w >> shift) - origin is the element */
	int64_t origin;
} Elements;

static int
elementLess(int64_t a, int64_t b, void *arg)
{
	const Elements *e = arg;
	size_t size = TS_KIND_SIZE(e->kind);
	const unsigned char *p = e->base + (size_t)((a >> e->shift) - e->origin) * size;
	const unsigned char *q = e->base + (size_t)((b >> e->shift) - e->origin) * size;

	switch (e->kind) {
	case TS_KIND_F64: {
		double x, y;
		memcpy(&x, p, 8);
		memcpy(&y, q, 8);
		return isnan(x) ? 0 : isnan(y) ? 1 : x < y;
	}
	case TS_KIND_F32: {
		float x, y;
		memcpy(&x, p, 4);
		memcpy(&y, q, 4);
		return isnan(x) ? 0 : isnan(y) ? 1 : x < y;
	}
	case TS_KIND_I64: { int64_t x, y; memcpy(&x, p, 8); memcpy(&y, q, 8); return x < y; }
	case TS_KIND_U64: { uint64_t x, y; memcpy(&x, p, 8); memcpy(&y, q, 8); return x < y; }
	case TS_KIND_I32: { int32_t x, y; memcpy(&x, p, 4); memcpy(&y, q, 4); return x < y; }
	case TS_KIND_U32: { uint32_t x, y; memcpy(&x, p, 4); memcpy(&y, q, 4); return x < y; }
	case TS_KIND_U16: { uint16_t x, y; memcpy(&x, p, 2); memcpy(&y, q, 2); return x < y; }
	default: return *p < *q;
	}
}

/* An element of kind, drawn around the edges of its type: in a small range
 * (ties) a third of the time, else from the edges or at random */
static void
drawElement(unsigned char *p, int kind)
{
	uint64_t r = next(), choice = below(16);

	switch (kind) {
	case TS_KIND_F64: {
		static const uint64_t edges[] = { 0, 0x8000000000000000ull, 0x7FF0000000000000ull, 0xFFF0000000000000ull,
			0x7FF8000000000000ull, 0xFFF8000000000000ull, 0x7FF0000000000001ull, 0xFFFFFFFFFFFFFFFFull,
			0x0000000000000001ull, 0x8000000000000001ull, 0x0010000000000000ull, 0x7FEFFFFFFFFFFFFFull,
			0xFFEFFFFFFFFFFFFFull, 0x000FFFFFFFFFFFFFull };
		double d;
		if (choice < 5) {
			d = (double)((int)below(9) - 4) / (choice < 3 ? 1.0 : 2.0);
			memcpy(p, &d, 8);
		} else if (choice < 10) {
			memcpy(p, &edges[below(sizeof edges / sizeof edges[0])], 8);
		} else {
			memcpy(p, &r, 8);
		}
		break;
	}
	case TS_KIND_F32: {
		static const uint32_t edges[] = { 0, 0x80000000u, 0x7F800000u, 0xFF800000u, 0x7FC00000u, 0xFFC00000u,
			0x7F800001u, 0xFFFFFFFFu, 1, 0x80000001u, 0x00800000u, 0x7F7FFFFFu, 0xFF7FFFFFu };
		uint32_t bits = (uint32_t)r;
		float f;
		if (choice < 5) {
			f = (float)((int)below(9) - 4) / (choice < 3 ? 1.0f : 2.0f);
			memcpy(p, &f, 4);
			break;
		}
		if (choice < 10) {
			bits = edges[below(sizeof edges / sizeof edges[0])];
		}
		memcpy(p, &bits, 4);
		break;
	}
	case TS_KIND_I64: case TS_KIND_U64: {
		static const uint64_t edges[] = { 0, 1, 0x7FFFFFFFFFFFFFFFull, 0x8000000000000000ull,
			0xFFFFFFFFFFFFFFFFull, 0x00000000FFFFFFFFull, 0x0000000100000000ull };
		uint64_t v = choice < 5 ? below(7) - 3 : choice < 10 ? edges[below(7)] : r;
		memcpy(p, &v, 8);
		break;
	}
	case TS_KIND_I32: case TS_KIND_U32: {
		static const uint32_t edges[] = { 0, 1, 0x7FFFFFFFu, 0x80000000u, 0xFFFFFFFFu, 0xFFFFu, 0x10000u };
		uint32_t v = choice < 5 ? (uint32_t)below(7) - 3 : choice < 10 ? edges[below(7)] : (uint32_t)r;
		memcpy(p, &v, 4);
		break;
	}
	case TS_KIND_U16: {
		uint16_t v = choice < 5 ? (uint16_t)below(5) : choice < 10 ? (uint16_t)(below(2) ? 0xFFFF : 0x8000) : (uint16_t)r;
		memcpy(p, &v, 2);
		break;
	}
	default:
		*p = choice < 5 ? (unsigned char)below(4) : (unsigned char)r;
	}
}

static int
isNaNAt(const unsigned char *p, int kind)
{
	if (kind == TS_KIND_F64) {
		double d;
		memcpy(&d, p, 8);
		return isnan(d);
	}
	if (kind == TS_KIND_F32) {
		float f;
		memcpy(&f, p, 4);
		return isnan(f);
	}
	return 0;
}

/* Sorts n elements of kind with the engine and with the reference and
 * compares their bytes */
static void
checkBufferOnce(int kind, size_t n, int descending, int sortedInput, double *elapsed)
{
	size_t size = TS_KIND_SIZE(kind), i;
	unsigned char *buffer = xmalloc(n * size), *original = xmalloc(n * size), *expected = xmalloc(n * size);
	int64_t *order = xmalloc(n * sizeof *order);
	Elements e = { NULL, kind, 0, 0 };
	double t;
	int rc;

	for (i = 0; i < n; i++) {
		drawElement(buffer + i * size, kind);
	}
	e.base = buffer;
	if (sortedInput) {
		for (i = 0; i < n; i++) {
			order[i] = (int64_t)i;
		}
		referenceSort(order, n, 0, elementLess, &e);
		for (i = 0; i < n; i++) {
			memcpy(original + i * size, buffer + (size_t)order[i] * size, size);
		}
		memcpy(buffer, original, n * size);
	}
	memcpy(original, buffer, n * size);
	e.base = original;
	for (i = 0; i < n; i++) {
		order[i] = (int64_t)i;
	}
	referenceSort(order, n, descending, elementLess, &e);
	for (i = 0; i < n; i++) {
		memcpy(expected + i * size, original + (size_t)order[i] * size, size);
	}
	t = seconds();
	rc = ts_sort_buffer(buffer, (int64_t)n, kind, descending);
	if (elapsed) {
		*elapsed = seconds() - t;
	}
	CHECK(rc == TS_OK, "%s %zu%s: rc %d", kindNames[kind], n, descending ? " descending" : "", rc);
	CHECK(n == 0 || memcmp(buffer, expected, n * size) == 0, "%s %zu%s%s: not the reference's order",
		kindNames[kind], n, sortedInput ? " sorted" : "", descending ? " descending" : "");
	/* NaN last ascending, first descending */
	if (n > 1 && (kind == TS_KIND_F64 || kind == TS_KIND_F32)) {
		int seenNaN = 0, seenNumber = 0, misplaced = 0;
		for (i = 0; i < n; i++) {
			int nan = isNaNAt(buffer + i * size, kind);
			misplaced |= descending ? (nan && seenNumber) : (!nan && seenNaN);
			seenNaN |= nan;
			seenNumber |= !nan;
		}
		CHECK(!misplaced, "%s %zu%s: a NaN out of place", kindNames[kind], n, descending ? " descending" : "");
	}
	free(buffer);
	free(original);
	free(expected);
	free(order);
}

static void
checkBuffers(void)
{
	static const size_t sizes[] = { 100, 255, 256, 257, 513, 1000, 4097, 65537 };
	unsigned char *bytes = xmalloc(64);
	int kind, descending;
	size_t n, i;

	section = "E2";
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		double elapsed[2];
		for (descending = 0; descending < 2; descending++) {
			for (n = 0; n <= 40; n++) {
				for (i = 0; i < 4; i++) {
					checkBufferOnce(kind, n, descending, 0, NULL);
				}
			}
			for (i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
				checkBufferOnce(kind, sizes[i], descending, 0, NULL);
				checkBufferOnce(kind, sizes[i], descending, 1, NULL);
			}
			checkBufferOnce(kind, 1000000, descending, 0, &elapsed[descending]);
			if (slow) {
				checkBufferOnce(kind, 4000000, descending, 0, NULL);
			}
		}
		printf("E2 %s: 1,000,000 random elements in %.3f s ascending, %.3f s descending\n",
			kindNames[kind], elapsed[0], elapsed[1]);
	}
	/* outside the contract */
	memset(bytes, 0, 64);
	CHECK(ts_sort_buffer(bytes, 2, 0, 0) == TS_BAD_ARGUMENT, "kind 0");
	CHECK(ts_sort_buffer(bytes, 2, 9, 0) == TS_BAD_ARGUMENT, "kind 9");
	CHECK(ts_sort_buffer(bytes, 2, -1, 0) == TS_BAD_ARGUMENT, "kind -1");
	CHECK(ts_sort_buffer(bytes, -1, TS_KIND_U8, 0) == TS_BAD_ARGUMENT, "n < 0");
	CHECK(ts_sort_buffer(NULL, 1, TS_KIND_U8, 0) == TS_BAD_ARGUMENT, "NULL");
	CHECK(ts_sort_buffer(NULL, 0, TS_KIND_F64, 0) == TS_OK, "NULL, nothing to sort");
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		if (TS_KIND_SIZE(kind) > 1) {
			CHECK(ts_sort_buffer(bytes + 1, 2, kind, 0) == TS_BAD_ARGUMENT, "%s misaligned", kindNames[kind]);
		}
	}
	free(bytes);
}

/* Argsorts the words perm (n of them, standing for the keys by shift and
 * origin) with the engine and the reference */
static void
checkArgsortOnce(int kind, size_t nkeys, size_t n, int shift, int64_t origin, int descending, int sub)
{
	size_t size = TS_KIND_SIZE(kind), i;
	unsigned char *keys = xmalloc(nkeys * size);
	int64_t *perm = xmalloc(n * sizeof *perm), *expected = xmalloc(n * sizeof *perm);
	Elements e;
	int rc;

	for (i = 0; i < nkeys; i++) {
		drawElement(keys + i * size, kind);
	}
	for (i = 0; i < n; i++) {
		/* the identity, a sub-range, or random indices (repeated) */
		uint64_t index = sub == 0 ? i : sub == 1 ? nkeys - n + i : below(nkeys);
		perm[i] = (int64_t)(((uint64_t)index + (uint64_t)origin) << shift) | (shift ? 1 : 0);
	}
	memcpy(expected, perm, n * sizeof *perm);
	e.base = keys;
	e.kind = kind;
	e.shift = shift;
	e.origin = origin;
	referenceSort(expected, n, descending, elementLess, &e);
	rc = ts_argsort_buffer(keys, (int64_t)nkeys, kind, perm, (int64_t)n, shift, origin, descending);
	CHECK(rc == TS_OK, "argsort %s %zu/%zu: rc %d", kindNames[kind], n, nkeys, rc);
	CHECK(n == 0 || memcmp(perm, expected, n * sizeof *perm) == 0, "argsort %s %zu of %zu keys, shift %d%s (%d): "
		"not the reference's", kindNames[kind], n, nkeys, shift, descending ? " descending" : "", sub);
	free(keys);
	free(perm);
	free(expected);
}

static void
checkArgsort(void)
{
	static const size_t sizes[] = { 0, 1, 2, 3, 7, 31, 64, 300, 1000, 5000, 70000 };
	int kind, descending, sub;
	size_t i;
	double t = seconds();

	section = "E3";
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		for (descending = 0; descending < 2; descending++) {
			for (i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
				size_t n = sizes[i];
				for (sub = 0; sub < 3; sub++) {
					checkArgsortOnce(kind, n + (sub ? 13 : 0), n, 0, 0, descending, sub);
					checkArgsortOnce(kind, n + (sub ? 13 : 0), n, 3, 1, descending, sub);
				}
			}
			if (!descending || slow) {
				checkArgsortOnce(kind, 1000000, 1000000, 3, 1, descending, 0);
			}
		}
	}
	printf("E3 argsort: the 8 kinds, up to 1,000,000 keys (descending: 70,000), with the reference: %.2f s\n", seconds() - t);
	/* words outside the keys: perm untouched, whatever n */
	{
		double keys[4] = { 3, 1, 2, 0 };
		int64_t perm[4], copy[4];
		static const int64_t bad[] = { 4, -1, INT64_MIN, INT64_MAX };
		for (i = 0; i < 4; i++) {
			size_t n;
			for (n = 1; n <= 4; n++) {
				perm[0] = 0; perm[1] = 1; perm[2] = 2; perm[3] = 3;
				perm[n - 1] = bad[i];
				memcpy(copy, perm, sizeof perm);
				CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, (int64_t)n, 0, 0, 0) == TS_BAD_ARGUMENT,
					"word %lld of %zu", (long long)bad[i], n);
				CHECK(memcmp(perm, copy, sizeof perm) == 0, "word %lld of %zu: perm changed", (long long)bad[i], n);
			}
		}
		/* tagged: 1-based, so the oop of 0 and of 5 are out */
		perm[0] = (0 << 3) | 1;
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, 3, 1, 0) == TS_BAD_ARGUMENT, "the oop of 0");
		perm[0] = (5 << 3) | 1;
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, 3, 1, 0) == TS_BAD_ARGUMENT, "the oop of 5");
		perm[0] = (4 << 3) | 1;
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, 3, 1, 0) == TS_OK, "the oop of 4");
		/* origin far below: no overflow */
		perm[0] = INT64_MAX;
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, 0, INT64_MIN, 0) == TS_BAD_ARGUMENT, "origin INT64_MIN");
		/* outside the contract */
		perm[0] = 0;
		CHECK(ts_argsort_buffer(keys, 4, 0, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "kind 0");
		CHECK(ts_argsort_buffer(keys, 4, 9, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "kind 9");
		CHECK(ts_argsort_buffer(keys, -1, TS_KIND_F64, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "nkeys < 0");
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, -1, 0, 0, 0) == TS_BAD_ARGUMENT, "n < 0");
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, -1, 0, 0) == TS_BAD_ARGUMENT, "shift -1");
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, perm, 1, 64, 0, 0) == TS_BAD_ARGUMENT, "shift 64");
		CHECK(ts_argsort_buffer(NULL, 4, TS_KIND_F64, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "keys NULL");
		CHECK(ts_argsort_buffer(keys, 4, TS_KIND_F64, NULL, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "perm NULL");
		CHECK(ts_argsort_buffer((char *)keys + 4, 3, TS_KIND_F64, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "misaligned");
		CHECK(ts_argsort_buffer(NULL, 0, TS_KIND_F64, NULL, 0, 0, 0, 0) == TS_OK, "nothing");
		CHECK(ts_argsort_buffer(keys, 0, TS_KIND_F64, perm, 1, 0, 0, 0) == TS_BAD_ARGUMENT, "no keys, one word");
	}
}


/*** E4: numbers ***/

/* The exact reference: x * 2^1074, a big integer (every double is an
 * integer multiple of 2^-1074), or an infinity */
#define LIMBS 96
typedef struct {
	int infinity;		/* -1, 0, 1 */
	int sign;		/* -1, 0, 1 */
	uint32_t limb[LIMBS];	/* little-endian */
} Exact;

/* limb += value << bit */
static void
addShifted(Exact *x, uint64_t value, int bit)
{
	int k = bit / 32, s = bit % 32;
	unsigned __int128 v = (unsigned __int128)value << s;

	while (v && k < LIMBS) {
		v += x->limb[k];
		x->limb[k++] = (uint32_t)v;
		v >>= 32;
	}
	if (v) {
		fprintf(stderr, "enginetest: an exact number is too large\n");
		exit(2);
	}
}

static int
isZero(const Exact *x)
{
	int k;

	for (k = 0; k < LIMBS; k++) {
		if (x->limb[k]) {
			return 0;
		}
	}
	return 1;
}

static void
exactOf(const ts_number *a, Exact *x)
{
	memset(x, 0, sizeof *x);
	if (a->type == TS_NUMBER_INTEGER) {
		uint64_t u = a->integer < 0 ? 0 - (uint64_t)a->integer : (uint64_t)a->integer;
		addShifted(x, u, 1074);
		x->sign = a->integer < 0 ? -1 : 1;
	} else if (a->type == TS_NUMBER_LARGE) {
		size_t i;
		for (i = 0; i < a->size; i++) {
			addShifted(x, a->digits[i], 1074 + 8 * (int)i);
		}
		x->sign = a->negative ? -1 : 1;
	} else {
		uint64_t bits, fraction;
		int exponent;
		memcpy(&bits, &a->real, 8);
		exponent = (int)((bits >> 52) & 0x7FF);
		fraction = bits & 0xFFFFFFFFFFFFFull;
		x->sign = bits >> 63 ? -1 : 1;
		if (exponent == 0x7FF) {
			x->infinity = x->sign;
			return;
		}
		if (exponent == 0) {
			addShifted(x, fraction, 0);
		} else {
			addShifted(x, fraction | 0x10000000000000ull, exponent - 1);
		}
	}
	if (isZero(x)) {
		x->sign = 0;
	}
}

static int
exactCompare(const Exact *x, const Exact *y)
{
	int k, order = 0;

	if (x->infinity || y->infinity) {
		return (x->infinity > y->infinity) - (x->infinity < y->infinity);
	}
	if (x->sign != y->sign) {
		return (x->sign > y->sign) - (x->sign < y->sign);
	}
	for (k = LIMBS; k-- > 0;) {
		if (x->limb[k] != y->limb[k]) {
			order = x->limb[k] > y->limb[k] ? 1 : -1;
			break;
		}
	}
	return x->sign < 0 ? -order : order;
}

/* A pool of numbers and of their exact values */
typedef struct {
	ts_number *n;
	Exact *exact;
	uint8_t **digits;
	size_t count, capacity;
} Numbers;

static void
addNumber(Numbers *p, ts_number x)
{
	if (p->count == p->capacity) {
		p->capacity = p->capacity ? 2 * p->capacity : 1024;
		p->n = realloc(p->n, p->capacity * sizeof *p->n);
		p->exact = realloc(p->exact, p->capacity * sizeof *p->exact);
		p->digits = realloc(p->digits, p->capacity * sizeof *p->digits);
		if (!p->n || !p->exact || !p->digits) {
			fprintf(stderr, "enginetest: out of memory\n");
			exit(2);
		}
	}
	p->digits[p->count] = NULL;
	if (x.type == TS_NUMBER_LARGE) {
		uint8_t *copy = xmalloc(x.size);
		if (x.size) {
			memcpy(copy, x.digits, x.size);
		}
		x.digits = copy;
		p->digits[p->count] = copy;
	}
	p->n[p->count] = x;
	exactOf(&x, &p->exact[p->count]);
	p->count++;
}

static void
addInteger(Numbers *p, int64_t v)
{
	ts_number x;

	memset(&x, 0, sizeof x);
	x.type = TS_NUMBER_INTEGER;
	x.integer = v;
	addNumber(p, x);
}

static void
addReal(Numbers *p, double d)
{
	ts_number x;

	if (isnan(d)) {
		return;
	}
	memset(&x, 0, sizeof x);
	x.type = TS_NUMBER_FLOAT;
	x.real = d;
	addNumber(p, x);
}

static void
addLarge(Numbers *p, const uint8_t *digits, size_t size, int negative)
{
	ts_number x;

	memset(&x, 0, sizeof x);
	x.type = TS_NUMBER_LARGE;
	x.digits = digits;
	x.size = size;
	x.negative = negative;
	addNumber(p, x);
}

/* 2^power + offset (offset in [-2, 2], power >= 2), as LARGE digits; with
 * extra high zero bytes */
static size_t
powerDigits(uint8_t *digits, int power, int offset, size_t extra)
{
	size_t size = (size_t)power / 8 + 1, i;

	memset(digits, 0, size + extra);
	digits[power / 8] = (uint8_t)(1u << (power % 8));
	if (offset > 0) {
		digits[0] = (uint8_t)(digits[0] + offset);	/* power >= 8 or no carry for our offsets */
	} else if (offset < 0) {
		/* subtract: borrow through the zero bytes */
		int borrow = -offset;
		for (i = 0; i < size && borrow; i++) {
			int v = digits[i] - borrow;
			borrow = v < 0 ? 1 : 0;
			digits[i] = (uint8_t)(v + (v < 0 ? 256 : 0));
		}
	}
	return size + extra;
}

static void
freeNumbers(Numbers *p)
{
	size_t i;

	for (i = 0; i < p->count; i++) {
		free(p->digits[i]);
	}
	free(p->n);
	free(p->exact);
	free(p->digits);
	memset(p, 0, sizeof *p);
}

/* The neighbour of d toward +inf (up) or -inf, from its bits (no libm) */
static double
neighbour(double d, int up)
{
	uint64_t bits;

	if (isnan(d) || isinf(d)) {
		return d;
	}
	if (d == 0) {
		return up ? ldexp(1.0, -1074) : -ldexp(1.0, -1074);
	}
	memcpy(&bits, &d, 8);
	bits = (d > 0) == (up != 0) ? bits + 1 : bits - 1;
	memcpy(&d, &bits, 8);
	return d;
}

static void
edgeNumbers(Numbers *p)
{
	static const int powers[] = { 2, 3, 7, 8, 15, 16, 31, 32, 52, 53, 54, 59, 60, 61, 62, 63, 64, 65, 100,
		127, 128, 500, 1022, 1023, 1024, 1025, 1030 };
	static const double reals[] = { 0.0, 0.5, 1.0 / 3, 0.1, 1.5, 2.5, 1e-300, 1e300, 1e308, 4.9e-324,
		2.2250738585072014e-308, 1.7976931348623157e308 };
	uint8_t digits[200];
	size_t i, size;
	int offset, sign;

	for (i = 0; i < sizeof powers / sizeof powers[0]; i++) {
		int power = powers[i];
		for (offset = -2; offset <= 2; offset++) {
			for (sign = -1; sign <= 1; sign += 2) {
				double d;
				size = powerDigits(digits, power, offset, 0);
				addLarge(p, digits, size, sign < 0);
				size = powerDigits(digits, power, offset, 3);
				addLarge(p, digits, size, sign < 0);	/* high zero bytes */
				if (power < 63 || (power == 63 && sign < 0 && offset <= 0)) {
					int64_t v = (int64_t)(((uint64_t)1 << power) + (uint64_t)(int64_t)offset);
					addInteger(p, sign < 0 ? (int64_t)(0 - (uint64_t)v) : v);
				}
				d = ldexp(1.0, power);
				if (!isinf(d)) {
					d = sign * (d + offset);
					addReal(p, d);
					addReal(p, neighbour(d, 1));
					addReal(p, neighbour(d, 0));
				}
			}
		}
	}
	for (i = 0; i < sizeof reals / sizeof reals[0]; i++) {
		addReal(p, reals[i]);
		addReal(p, -reals[i]);
		addReal(p, neighbour(reals[i], 1));
		addReal(p, neighbour(-reals[i], 0));
	}
	addReal(p, INFINITY);
	addReal(p, -INFINITY);
	addReal(p, ldexp(1.0, -1074));		/* the smallest subnormal */
	addReal(p, -ldexp(1.0, -1074));
	addReal(p, (double)INT64_MAX);		/* 2^63 */
	addReal(p, (double)INT64_MIN);
	addInteger(p, 0);
	addInteger(p, 1);
	addInteger(p, -1);
	addInteger(p, INT64_MAX);
	addInteger(p, INT64_MIN);
	addInteger(p, INT64_MAX - 1);
	addInteger(p, INT64_MIN + 1);
	addInteger(p, ((int64_t)1 << 60) - 1);	/* SmallInteger maxVal */
	addInteger(p, -((int64_t)1 << 60));	/* SmallInteger minVal */
	/* a LARGE of zero, non-normalized ones holding small values */
	memset(digits, 0, sizeof digits);
	addLarge(p, digits, 0, 0);
	addLarge(p, digits, 8, 0);
	addLarge(p, digits, 8, 1);
	digits[0] = 5;
	addLarge(p, digits, 8, 0);
	addLarge(p, digits, 8, 1);
	addLarge(p, digits, 1, 0);
	/* 2^1024 - 2^971 = DBL_MAX, as a LARGE, and its neighbours */
	memset(digits, 0, sizeof digits);
	for (i = 971; i < 1024; i++) {
		digits[i / 8] |= (uint8_t)(1u << (i % 8));
	}
	addLarge(p, digits, 128, 0);
	addLarge(p, digits, 128, 1);
	digits[0] = 1;
	addLarge(p, digits, 128, 0);
	addLarge(p, digits, 128, 1);
}

static void
randomNumbers(Numbers *p, size_t count)
{
	uint8_t digits[150];
	size_t i, k;

	for (i = 0; i < count; i++) {
		switch (below(6)) {
		case 0:
			addInteger(p, (int64_t)next());
			break;
		case 1:
			addInteger(p, (int64_t)next() >> below(64));
			break;
		case 2: {
			uint64_t bits = next();
			double d;
			memcpy(&d, &bits, 8);
			addReal(p, d);
			break;
		}
		case 3:
			addReal(p, ldexp((double)(int64_t)next(), (int)below(200) - 100));
			break;
		case 4: {
			size_t size = 1 + below(140);
			for (k = 0; k < size; k++) {
				digits[k] = (uint8_t)next();
			}
			if (below(4) == 0) {
				digits[size - 1] = 0;	/* a high zero byte */
			}
			addLarge(p, digits, size, (int)below(2));
			break;
		}
		default: {
			/* an integer and a double that are close */
			int64_t v = (int64_t)next() >> below(12);
			addInteger(p, v);
			addReal(p, (double)v);
		}
		}
	}
}

static int
numberLess(int64_t a, int64_t b, void *arg)
{
	const Numbers *p = arg;

	return ts_number_compare(&p->n[a], &p->n[b]) < 0;
}

static int
exactLess(int64_t a, int64_t b, void *arg)
{
	const Numbers *p = arg;

	return exactCompare(&p->exact[a], &p->exact[b]) < 0;
}

static void
checkNumberPairs(const Numbers *p, const char *what)
{
	size_t i, j;

	for (i = 0; i < p->count; i++) {
		for (j = 0; j < p->count; j++) {
			int got = ts_number_compare(&p->n[i], &p->n[j]), want = exactCompare(&p->exact[i], &p->exact[j]);
			checks++;
			if (got != want) {
				const ts_number *x = &p->n[i], *y = &p->n[j];
				fail("%s %zu (type %d %lld %.17g size %zu) vs %zu (type %d %lld %.17g size %zu): %d, not %d", what,
					i, x->type, (long long)x->integer, x->real, x->size,
					j, y->type, (long long)y->integer, y->real, y->size, got, want);
			}
		}
	}
}

static void
checkNumberSort(const Numbers *p, int descending)
{
	size_t n = p->count, i;
	int64_t *got = xmalloc(n * sizeof *got), *want = xmalloc(n * sizeof *want);

	for (i = 0; i < n; i++) {
		got[i] = want[i] = (int64_t)i;
	}
	CHECK(ts_sort_words(got, (int64_t)n, descending, numberLess, (void *)p) == TS_OK, "sort of numbers: rc");
	referenceSort(want, n, descending, exactLess, (void *)p);
	CHECK(memcmp(got, want, n * sizeof *got) == 0, "sort of %zu numbers%s: not the exact order", n,
		descending ? " descending" : "");
	free(got);
	free(want);
}

static void
checkNumbers(void)
{
	Numbers p;
	size_t edges;
	long before;
	int round;

	section = "E4";
	memset(&p, 0, sizeof p);
	edgeNumbers(&p);
	edges = p.count;
	before = checks;
	checkNumberPairs(&p, "edge");
	checkNumberSort(&p, 0);
	checkNumberSort(&p, 1);
	printf("E4 numbers: %zu edges, %ld pairs\n", edges, checks - before);
	freeNumbers(&p);
	for (round = 0; round < (slow ? 40 : 6); round++) {
		memset(&p, 0, sizeof p);
		edgeNumbers(&p);
		randomNumbers(&p, 1200);
		checkNumberPairs(&p, "random");
		checkNumberSort(&p, round & 1);
		freeNumbers(&p);
	}
	/* a few by hand, for the reader */
	{
		ts_number big, f, i;
		uint8_t two60[8] = { 0, 0, 0, 0, 0, 0, 0, 0x10 }, two60plus1[8] = { 1, 0, 0, 0, 0, 0, 0, 0x10 };
		memset(&big, 0, sizeof big);
		memset(&f, 0, sizeof f);
		memset(&i, 0, sizeof i);
		big.type = TS_NUMBER_LARGE;
		big.digits = two60plus1;
		big.size = 8;
		f.type = TS_NUMBER_FLOAT;
		f.real = ldexp(1.0, 60);
		i.type = TS_NUMBER_INTEGER;
		i.integer = ((int64_t)1 << 60) - 1;
		CHECK(ts_number_compare(&big, &f) == 1, "2^60+1 > 2^60 asFloat");
		CHECK(ts_number_compare(&i, &f) == -1, "2^60-1 < 2^60 asFloat (the JIT says otherwise)");
		big.digits = two60;
		CHECK(ts_number_compare(&big, &f) == 0, "2^60 = 2^60 asFloat");
		f.real = -0.0;
		i.integer = 0;
		CHECK(ts_number_compare(&i, &f) == 0, "0 = -0.0");
		f.real = INFINITY;
		big.digits = two60;
		CHECK(ts_number_compare(&big, &f) == -1, "2^60 < inf");
	}
}


/*** E5: strings ***/

typedef struct {
	ts_string s;
	void *units;
} Text;

static int
referenceStringCompare(const ts_string *a, const ts_string *b)
{
	size_t i;

	for (i = 0; i < a->size && i < b->size; i++) {
		uint32_t x = a->wide ? ((const uint32_t *)a->units)[i] : ((const uint8_t *)a->units)[i];
		uint32_t y = b->wide ? ((const uint32_t *)b->units)[i] : ((const uint8_t *)b->units)[i];
		if (x != y) {
			return x < y ? -1 : 1;
		}
	}
	return a->size < b->size ? -1 : a->size > b->size ? 1 : 0;
}

static void
makeText(Text *t, size_t size, int wide, const Text *prefixOf)
{
	static const uint32_t byteUnits[] = { 0, 1, 'A', 'a', 'b', 'z', 0x7F, 0x80, 0xE9, 0xFF };
	static const uint32_t wideUnits[] = { 0, 1, 'A', 'a', 'b', 0xFF, 0x100, 0x3B1, 0xFFFF, 0x10000,
		0x7FFFFFFF, 0x80000000u, 0xFFFFFFFFu };
	size_t i;

	t->s.size = size;
	t->s.wide = wide;
	t->units = size ? xmalloc(size * (wide ? 4 : 1)) : NULL;
	t->s.units = t->units;
	for (i = 0; i < size; i++) {
		uint32_t u;
		if (prefixOf && i < prefixOf->s.size) {
			u = prefixOf->s.wide ? ((uint32_t *)prefixOf->units)[i] : ((uint8_t *)prefixOf->units)[i];
			if (!wide && u > 0xFF) {
				u &= 0xFF;
			}
		} else if (wide) {
			u = wideUnits[below(sizeof wideUnits / sizeof wideUnits[0])];
		} else {
			u = byteUnits[below(sizeof byteUnits / sizeof byteUnits[0])];
		}
		if (wide) {
			((uint32_t *)t->units)[i] = u;
		} else {
			((uint8_t *)t->units)[i] = (uint8_t)u;
		}
	}
}

static int
textLess(int64_t a, int64_t b, void *arg)
{
	const Text *t = arg;

	return ts_string_compare(&t[a].s, &t[b].s) < 0;
}

static int
referenceTextLess(int64_t a, int64_t b, void *arg)
{
	const Text *t = arg;

	return referenceStringCompare(&t[a].s, &t[b].s) < 0;
}

static void
checkStrings(void)
{
	size_t count = 600, i, j;
	Text *texts = xmalloc(count * sizeof *texts);
	int64_t *got = xmalloc(count * sizeof *got), *want = xmalloc(count * sizeof *want);
	long before = checks;
	int round, descending;

	section = "E5";
	for (round = 0; round < (slow ? 20 : 4); round++) {
		for (i = 0; i < count; i++) {
			size_t size = below(4) == 0 ? 0 : below(9);
			const Text *prefix = i > 0 && below(3) == 0 ? &texts[below(i)] : NULL;
			makeText(&texts[i], prefix && below(2) ? prefix->s.size + below(3) : size, (int)below(2), prefix);
		}
		for (i = 0; i < count; i++) {
			for (j = 0; j < count; j++) {
				int got1 = ts_string_compare(&texts[i].s, &texts[j].s);
				int want1 = referenceStringCompare(&texts[i].s, &texts[j].s);
				checks++;
				if (got1 != want1) {
					fail("strings %zu (%zu units, wide %d) vs %zu (%zu, %d): %d, not %d",
						i, texts[i].s.size, texts[i].s.wide, j, texts[j].s.size, texts[j].s.wide, got1, want1);
				}
			}
		}
		for (descending = 0; descending < 2; descending++) {
			for (i = 0; i < count; i++) {
				got[i] = want[i] = (int64_t)i;
			}
			CHECK(ts_sort_words(got, (int64_t)count, descending, textLess, texts) == TS_OK, "sort of strings: rc");
			referenceSort(want, count, descending, referenceTextLess, texts);
			CHECK(memcmp(got, want, count * sizeof *got) == 0, "sort of strings%s: not the reference's",
				descending ? " descending" : "");
		}
		for (i = 0; i < count; i++) {
			free(texts[i].units);
		}
	}
	/* by hand */
	{
		uint8_t a[] = { 'a' }, aNul[] = { 'a', 0 }, ab[] = { 'a', 'b' }, B[] = { 'B' }, e9[] = { 0xE9 };
		uint32_t wa[] = { 'a' }, wHigh[] = { 0x80000000u }, wLow[] = { 0x7FFFFFFF }, wMax[] = { 0xFFFFFFFFu };
		ts_string sa = { a, 1, 0 }, saNul = { aNul, 2, 0 }, sab = { ab, 2, 0 }, sB = { B, 1, 0 }, se9 = { e9, 1, 0 };
		ts_string swa = { wa, 1, 1 }, swHigh = { wHigh, 1, 1 }, swLow = { wLow, 1, 1 }, swMax = { wMax, 1, 1 };
		ts_string empty = { NULL, 0, 0 }, wideEmpty = { NULL, 0, 1 };
		CHECK(ts_string_compare(&sa, &saNul) == -1, "'a' < 'a\\0'");
		CHECK(ts_string_compare(&sa, &sab) == -1, "'a' < 'ab'");
		CHECK(ts_string_compare(&sB, &sa) == -1, "'B' < 'a' (case-sensitive)");
		CHECK(ts_string_compare(&sa, &se9) == -1, "'a' < 'é'");
		CHECK(ts_string_compare(&sa, &swa) == 0, "'a' = 'a' asWideString");
		CHECK(ts_string_compare(&swLow, &swHigh) == -1, "16r7FFFFFFF < 16r80000000 (unsigned)");
		CHECK(ts_string_compare(&swMax, &se9) == 1, "16rFFFFFFFF > 'é'");
		CHECK(ts_string_compare(&empty, &wideEmpty) == 0, "'' = '' asWideString");
		CHECK(ts_string_compare(&empty, &sa) == -1, "'' < 'a'");
	}
	printf("E5 strings: %ld comparisons and sorts\n", checks - before);
	free(texts);
	free(got);
	free(want);
}


/*** E6: no memory ***/

static void
checkNoMemory(void)
{
#if MALLOC_HOOK
	size_t n = 20000, i;
	int64_t *words = xmalloc(n * sizeof *words), *original = xmalloc(n * sizeof *words);
	unsigned char *buffer = xmalloc(n * 4), *copy = xmalloc(n * 4);
	int kind, rc;
	long refusal;

	section = "E6";
	/* the merge buffer of ts_sort_words */
	makeRecords(words, n, RANDOM);
	memcpy(original, words, n * sizeof *words);
	refuseMallocAfter(0);
	rc = ts_sort_words(words, (int64_t)n, 0, keyLess, NULL);
	refuseMallocAfter(-1);
	CHECK(rc == TS_NO_MEMORY, "words: rc %d", rc);
	CHECK(isPermutation(words, original, n), "words: not a permutation");
	/* no malloc at all below 512 (MERGESTATE_TEMP_SIZE 256) */
	refuseMallocAfter(0);
	rc = ts_sort_words(words, 300, 0, keyLess, NULL);
	refuseMallocAfter(-1);
	CHECK(rc == TS_OK && mallocs == 0, "300 words: rc %d, %ld mallocs", rc, mallocs);
	/* buffers: the widened copy (first malloc) or the merge buffer (second) */
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		size_t size = TS_KIND_SIZE(kind);
		for (refusal = 0; refusal < (size < 8 ? 2 : 1); refusal++) {
			unsigned char *b = size == 8 ? (unsigned char *)words : buffer;
			for (i = 0; i < n; i++) {
				drawElement(b + i * size, kind);
			}
			memcpy(size == 8 ? (unsigned char *)original : copy, b, n * size);
			refuseMallocAfter(refusal);
			rc = ts_sort_buffer(b, (int64_t)n, kind, (int)refusal);
			refuseMallocAfter(-1);
			CHECK(rc == TS_NO_MEMORY, "%s, malloc %ld refused: rc %d", kindNames[kind], refusal + 1, rc);
			if (size == 8) {
				CHECK(isPermutation(words, original, n), "%s: not a permutation", kindNames[kind]);
			} else {
				CHECK(memcmp(buffer, copy, n * size) == 0, "%s, malloc %ld refused: the buffer changed",
					kindNames[kind], refusal + 1);
			}
		}
	}
	/* argsort */
	for (i = 0; i < n; i++) {
		drawElement(buffer + 4 * i, TS_KIND_U32);
		words[i] = (int64_t)i;
	}
	memcpy(original, words, n * sizeof *words);
	refuseMallocAfter(0);
	rc = ts_argsort_buffer(buffer, (int64_t)n, TS_KIND_U32, words, (int64_t)n, 0, 0, 0);
	refuseMallocAfter(-1);
	CHECK(rc == TS_NO_MEMORY, "argsort: rc %d", rc);
	CHECK(isPermutation(words, original, n), "argsort: not a permutation");
	printf("E6 no memory: the merge buffer and the widened copy refused\n");
	free(words);
	free(original);
	free(buffer);
	free(copy);
#else
	printf("E6 no memory: skipped (malloc is wrapped only with glibc, without AddressSanitizer)\n");
#endif
}

int
main(int argc, char **argv)
{
	const char *only = NULL;
	int i;

	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--slow") == 0) {
			slow = 1;
		} else if (strcmp(argv[i], "--only") == 0 && i + 1 < argc) {
			only = argv[++i];
		} else if (strcmp(argv[i], "--seed") == 0 && i + 1 < argc) {
			seed = strtoull(argv[++i], NULL, 0) | 1;
		} else {
			fprintf(stderr, "usage: enginetest [--only E1,E2,...] [--seed N] [--slow]\n");
			return 2;
		}
	}
#define RUN(name, fn) if (!only || strstr(only, name)) fn()
	printf("enginetest: timsort.c %s%s\n", ts_timsort_version(), slow ? ", slow" : "");
	RUN("E1", checkWords);
	RUN("E2", checkBuffers);
	RUN("E3", checkArgsort);
	RUN("E4", checkNumbers);
	RUN("E5", checkStrings);
	RUN("E6", checkNoMemory);
	if (failures) {
		printf("enginetest FAILED: %ld checks, %ld failures\n", checks, failures);
		return 1;
	}
	printf("enginetest ok: %ld checks, 0 failures\n", checks);
	return 0;
}
