/* ffitest.c -- C-level tests of libTimSortFFI (tsFFI.h), run by
 * run-c-tests.sh in the lanes ffi and ffi-asan, linked with tsFFI.c, the
 * engine (tsEngine.c), src/coro and timsort.c.  Given --lib PATH, the same
 * checks run through a built libTimSortFFI instead, which the test opens
 * with dlopen and whose exports it looks up by name with dlsym, as the FFI
 * of the image does (the library's own copy of the engine and of the table
 * of resumable sorts then answers).
 *
 * The references are written here, independently of timsort.c: a stable
 * merge sort of indices with the order of tsFFI.h (integers signed or
 * unsigned as the type, floats with every NaN after every number and the
 * NaNs equal, -0.0 equal to 0.0; descending: the stable sort by the
 * reversed order, equal elements in their order of the input).  Results
 * are compared bit by bit, so that a NaN payload or the sign of a zero that
 * moved to the wrong place, or equal elements out of their order, fail.
 * The checks:
 *
 *   F1  identity: ts_ffi_protocol is 1, the versions are the engine's
 *       ("TimSortPlugin 1.0", "fd70381"), ts_ffi_coro_impl agrees with
 *       ts_ffi_coro_supported
 *   F2  the eight buffer sorts, ascending and descending, against the
 *       reference: sizes 0-70, 100, 1000, 10,000, 100,000 (and 1,000,000
 *       with --slow), shapes random, sorted, reversed, few unique, nearly
 *       sorted, all equal, organ pipe, sawtooth; floats with NaNs of both
 *       signs and several payloads, signed zeros, infinities, subnormals;
 *       integers at their extremes; descending is any non-zero int64 (1, 2,
 *       2^32, INT64_MIN), as the image may pass it
 *   F3  the failures of the buffer sorts: n < 0, NULL with n > 0, a
 *       misaligned base (TS_FFI_BAD_ARGUMENT, the buffer untouched); NULL
 *       with n = 0 and n < 2 are TS_FFI_OK and touch nothing
 *   F4  the argsorts over i64, u64 and f64 keys, as F2: the permutation is
 *       the reference's, idx is filled by the library (its input ignored),
 *       the keys are never written
 *   F5  the failures of the argsorts: n < 0, NULL keys or idx, misaligned
 *       keys or idx, idx overlapping the keys (TS_FFI_BAD_ARGUMENT, idx
 *       untouched); n = 0 touches nothing
 *   F6  the resumable sort, its contract: every request is (i << 32) | j
 *       with i != j in [0, n); driven by a C comparison the permutation is
 *       the reference's, ascending and descending; Pharo's answer for a
 *       <= block, (c[j] <= c[i]) not; n = 0 and 1 answer TS_FFI_DONE at
 *       once, and a finished sort keeps its answer; answers > 1 count as 1;
 *       abort (TS_FFI_ABORTED, kept; no permutation); free of a pending, a
 *       finished and an unstarted sort; stale ids (TS_FFI_BAD_ARGUMENT for
 *       next, permutation, free, size); ts_ffi_coro_size of a live sort;
 *       ts_ffi_coro_permutation before the end,
 *       with a wrong n, into NULL; n outside [0, 2^31 - 1], and (Linux,
 *       without AddressSanitizer) n = 2^31 - 1 refused for memory, not as
 *       an argument; ids count up from a base in [2^32, 2^58) and are never
 *       reused; ts_ffi_coro_live counts.  Without a stack
 *       switch: every ts_ffi_coro_* answers TS_FFI_UNSUPPORTED
 *   F7  the resumable sort under load: 64 live sorts and the 65th refused
 *       (TS_FFI_LIMIT) until one is freed; interleaved sorts stepped in
 *       turn; a whole sort run between two steps of another (nested, as a
 *       sortBlock that sorts); 100,000 random elements; random aborts and
 *       frees; the live count back to 0
 *   F8  (--lib only) the library exports the 22 functions of tsFFI.h and
 *       none of the engine or of src/coro (dlsym finds no ts_sort_words,
 *       ts_coro_open, list_sort_impl)
 *
 * With --slow (TS_TEST_ARGS=--slow in run-c-tests.sh), F2 and F4 also sort
 * 1,000,000 elements of each shape.  F1 prints the time of a sort of
 * 1,000,000 random doubles and the cost of a step of the resumable sort.
 *
 * Built with -DFFITEST_DLOPEN_ONLY, the test links no source of the
 * library and needs --lib (cc ffitest.c -ldl).
 *
 * Usage: ffitest [--lib PATH] [--only F1,F2,...] [--slow]
 * Prints one line per failed check and a summary; exits 1 when a check
 * failed, 2 on a usage error or when PATH cannot be opened. */

#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#if defined(_WIN32)
# include <windows.h>
#else
# include <dlfcn.h>
#endif

/* RLIMIT_AS, which AddressSanitizer cannot run under: Linux, without it */
#if defined(__SANITIZE_ADDRESS__)
# define FFITEST_ASAN 1
#elif defined(__has_feature)
# if __has_feature(address_sanitizer)
#  define FFITEST_ASAN 1
# endif
#endif
#if defined(__linux__) && !defined(FFITEST_ASAN)
# include <sys/resource.h>
# include <unistd.h>
# define FFITEST_RLIMIT 1
#else
# define FFITEST_RLIMIT 0
#endif

#include "tsFFI.h"

static int checks, failures, slow;
static const char *currentTest = "";

/* Counts a check, prints it when it failed; answers ok */
static int
check(int ok, int line, const char *format, ...)
{
	va_list args;

	checks++;
	if (ok)
		return 1;
	failures++;
	if (failures > 200)
		return 0;
	printf("FAIL %s ffitest.c:%d: ", currentTest, line);
	va_start(args, format);
	vprintf(format, args);
	va_end(args);
	printf("\n");
	return 0;
}

#define CHECK(condition) check((condition) != 0, __LINE__, "%s", #condition)
#define CHECKF(condition, ...) check((condition) != 0, __LINE__, __VA_ARGS__)

static double
nowMillis(void)
{
	struct timespec t;

	clock_gettime(CLOCK_MONOTONIC, &t);
	return (double)t.tv_sec * 1e3 + (double)t.tv_nsec / 1e6;
}


/*** The library: its exports, linked or looked up by name ***/

static struct {
	int64_t (*protocol)(void);
	const char *(*version)(void);
	const char *(*timsort_version)(void);
	int64_t (*sort_f64)(double *, int64_t, int64_t);
	int64_t (*sort_f32)(float *, int64_t, int64_t);
	int64_t (*sort_i64)(int64_t *, int64_t, int64_t);
	int64_t (*sort_u64)(uint64_t *, int64_t, int64_t);
	int64_t (*sort_i32)(int32_t *, int64_t, int64_t);
	int64_t (*sort_u32)(uint32_t *, int64_t, int64_t);
	int64_t (*sort_u16)(uint16_t *, int64_t, int64_t);
	int64_t (*sort_u8)(uint8_t *, int64_t, int64_t);
	int64_t (*argsort_i64)(const int64_t *, int64_t *, int64_t, int64_t);
	int64_t (*argsort_u64)(const uint64_t *, int64_t *, int64_t, int64_t);
	int64_t (*argsort_f64)(const double *, int64_t *, int64_t, int64_t);
	int64_t (*coro_supported)(void);
	const char *(*coro_impl)(void);
	int64_t (*coro_new)(int64_t, int64_t);
	int64_t (*coro_next)(int64_t, int64_t);
	int64_t (*coro_permutation)(int64_t, int64_t *, int64_t);
	int64_t (*coro_free)(int64_t);
	int64_t (*coro_live)(void);
	int64_t (*coro_size)(int64_t);
} lib;

/* The 22 exports of tsFFI.h, in the order of lib */
static const char *const exportNames[] = {
	"ts_ffi_protocol", "ts_ffi_version", "ts_ffi_timsort_version",
	"ts_ffi_sort_f64", "ts_ffi_sort_f32", "ts_ffi_sort_i64", "ts_ffi_sort_u64",
	"ts_ffi_sort_i32", "ts_ffi_sort_u32", "ts_ffi_sort_u16", "ts_ffi_sort_u8",
	"ts_ffi_argsort_i64", "ts_ffi_argsort_u64", "ts_ffi_argsort_f64",
	"ts_ffi_coro_supported", "ts_ffi_coro_impl", "ts_ffi_coro_new", "ts_ffi_coro_next",
	"ts_ffi_coro_permutation", "ts_ffi_coro_free", "ts_ffi_coro_live", "ts_ffi_coro_size"
};
#define EXPORTS ((int)(sizeof exportNames / sizeof exportNames[0]))
_Static_assert(sizeof lib == sizeof exportNames, "ffitest: one name per export");

static void *library;

/* The functions linked into the test (not compiled with
   -DFFITEST_DLOPEN_ONLY, a build of the test alone, which needs --lib) */
static int
linkDirectly(void)
{
#ifdef FFITEST_DLOPEN_ONLY
	printf("ffitest: built with FFITEST_DLOPEN_ONLY: --lib PATH is required\n");
	return 0;
#else
	lib.protocol = ts_ffi_protocol;
	lib.version = ts_ffi_version;
	lib.timsort_version = ts_ffi_timsort_version;
	lib.sort_f64 = ts_ffi_sort_f64;
	lib.sort_f32 = ts_ffi_sort_f32;
	lib.sort_i64 = ts_ffi_sort_i64;
	lib.sort_u64 = ts_ffi_sort_u64;
	lib.sort_i32 = ts_ffi_sort_i32;
	lib.sort_u32 = ts_ffi_sort_u32;
	lib.sort_u16 = ts_ffi_sort_u16;
	lib.sort_u8 = ts_ffi_sort_u8;
	lib.argsort_i64 = ts_ffi_argsort_i64;
	lib.argsort_u64 = ts_ffi_argsort_u64;
	lib.argsort_f64 = ts_ffi_argsort_f64;
	lib.coro_supported = ts_ffi_coro_supported;
	lib.coro_impl = ts_ffi_coro_impl;
	lib.coro_new = ts_ffi_coro_new;
	lib.coro_next = ts_ffi_coro_next;
	lib.coro_permutation = ts_ffi_coro_permutation;
	lib.coro_free = ts_ffi_coro_free;
	lib.coro_live = ts_ffi_coro_live;
	lib.coro_size = ts_ffi_coro_size;
	return 1;
#endif
}

static void *
lookUp(const char *name)
{
#if defined(_WIN32)
	return (void *)GetProcAddress((HMODULE)library, name);
#else
	return dlsym(library, name);
#endif
}

/* Opens path and fills lib by name; 0 when the library or an export is
   missing */
static int
linkLibrary(const char *path)
{
	void *slots[EXPORTS];
	int i;

#if defined(_WIN32)
	library = (void *)LoadLibraryA(path);
#else
	library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
#endif
	if (!library) {
#if defined(_WIN32)
		printf("ffitest: cannot open %s\n", path);
#else
		printf("ffitest: cannot open %s: %s\n", path, dlerror());
#endif
		return 0;
	}
	for (i = 0; i < EXPORTS; i++) {
		if (!(slots[i] = lookUp(exportNames[i]))) {
			printf("ffitest: %s has no %s\n", path, exportNames[i]);
			return 0;
		}
	}
	/* (POSIX: a data pointer and a function pointer have the same
	   representation; the slots of lib are function pointers in order) */
	memcpy(&lib, slots, sizeof lib);
	return 1;
}


/*** Random ***/

static uint64_t seed = 0x7473666669746573ull;

static uint64_t
next64(void)
{
	uint64_t z = (seed += 0x9E3779B97F4A7C15ull);

	z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
	z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
	return z ^ (z >> 31);
}

static uint64_t
below(uint64_t n)
{
	return n ? next64() % n : 0;
}


/*** The types and their order (the reference) ***/

enum { F64, F32, I64, U64, I32, U32, U16, U8, TYPES };

static const char *const typeNames[TYPES] = { "f64", "f32", "i64", "u64", "i32", "u32", "u16", "u8" };
static const size_t typeSizes[TYPES] = { 8, 4, 8, 8, 4, 4, 2, 1 };

/* lt(a, b): element a strictly before element b, ascending */
static int
less(int type, const void *base, int64_t a, int64_t b)
{
	switch (type) {
	case F64: {
		double x = ((const double *)base)[a], y = ((const double *)base)[b];
		return !isnan(x) && (isnan(y) || x < y);
	}
	case F32: {
		float x = ((const float *)base)[a], y = ((const float *)base)[b];
		return !isnan(x) && (isnan(y) || x < y);
	}
	case I64: return ((const int64_t *)base)[a] < ((const int64_t *)base)[b];
	case U64: return ((const uint64_t *)base)[a] < ((const uint64_t *)base)[b];
	case I32: return ((const int32_t *)base)[a] < ((const int32_t *)base)[b];
	case U32: return ((const uint32_t *)base)[a] < ((const uint32_t *)base)[b];
	case U16: return ((const uint16_t *)base)[a] < ((const uint16_t *)base)[b];
	default: return ((const uint8_t *)base)[a] < ((const uint8_t *)base)[b];
	}
}

/* The order of the reference sort: lt, or lt reversed when descending */
static int
before(int type, const void *base, int descending, int64_t a, int64_t b)
{
	return descending ? less(type, base, b, a) : less(type, base, a, b);
}

/* A stable merge sort of the n indices at perm by the elements of base they
   stand for: an element moves before an earlier one only when it is
   strictly before it */
static void
referenceMerge(int type, const void *base, int descending, int64_t *perm, int64_t *tmp, int64_t n)
{
	int64_t half = n / 2, i = 0, j = half, k = 0;

	if (n < 2)
		return;
	referenceMerge(type, base, descending, perm, tmp, half);
	referenceMerge(type, base, descending, perm + half, tmp, n - half);
	while (i < half && j < n)
		tmp[k++] = before(type, base, descending, perm[j], perm[i]) ? perm[j++] : perm[i++];
	while (i < half)
		tmp[k++] = perm[i++];
	while (j < n)
		tmp[k++] = perm[j++];
	memcpy(perm, tmp, (size_t)n * sizeof(int64_t));
}

/* The stable sorted order of the n elements at base: element perm[k] goes
   to position k */
static void
referenceArgSort(int type, const void *base, int descending, int64_t *perm, int64_t n)
{
	int64_t *tmp = malloc((size_t)(n ? n : 1) * sizeof(int64_t)), i;

	if (!tmp) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	for (i = 0; i < n; i++)
		perm[i] = i;
	referenceMerge(type, base, descending, perm, tmp, n);
	free(tmp);
}

/* The buffer sort of the library for type */
static int64_t
librarySort(int type, void *base, int64_t n, int64_t descending)
{
	switch (type) {
	case F64: return lib.sort_f64(base, n, descending);
	case F32: return lib.sort_f32(base, n, descending);
	case I64: return lib.sort_i64(base, n, descending);
	case U64: return lib.sort_u64(base, n, descending);
	case I32: return lib.sort_i32(base, n, descending);
	case U32: return lib.sort_u32(base, n, descending);
	case U16: return lib.sort_u16(base, n, descending);
	default: return lib.sort_u8(base, n, descending);
	}
}


/* The descending argument of a call: 0, or a non-zero value that changes
   with n (1, 2, 2^32, INT64_MIN: any non-zero means descending) */
static int64_t
descendingArgument(int descending, int64_t n)
{
	static const int64_t values[] = { 1, 2, (int64_t)1 << 32, INT64_MIN };

	return descending ? values[n % 4] : 0;
}


/*** Data ***/

enum { RANDOM, SORTED, REVERSED, FEW_UNIQUE, NEARLY_SORTED, ALL_EQUAL, ORGAN_PIPE, SAWTOOTH, SHAPES };

static const char *const shapeNames[SHAPES] = {
	"random", "sorted", "reversed", "few-unique", "nearly-sorted", "all-equal", "organ-pipe", "sawtooth"
};

/* Values that matter for each type, mixed into random data */
static const double specialDoubles[] = {
	0.0, -0.0, 1.0, -1.0, INFINITY, -INFINITY, 4.9406564584124654e-324, -4.9406564584124654e-324,
	2.2250738585072014e-308, 1.7976931348623157e308, -1.7976931348623157e308, 9007199254740993.0,
	0.5, 0.1
};

static double
nanWithBits(uint64_t bits)
{
	double d;

	memcpy(&d, &bits, 8);
	return d;
}

static float
nanWithBits32(uint32_t bits)
{
	float f;

	memcpy(&f, &bits, 4);
	return f;
}

/* A random element of type into slot i of base: a special value now and
   then, else a value that comes back often when few is set */
static void
randomElement(int type, void *base, int64_t i, int few)
{
	uint64_t r = few ? below(10) * 0x0101010101010101ull : next64();
	int special = below(16) == 0;

	switch (type) {
	case F64: {
		double d;
		if (special) {
			uint64_t pick = below(18);
			static const uint64_t nans[] = {
				0x7FF8000000000000ull, 0xFFF8000000000000ull, 0x7FF0000000000001ull, 0x7FF4000000000BADull
			};
			d = pick < 14 ? specialDoubles[pick] : nanWithBits(nans[pick - 14]);
		} else {
			d = few ? (double)(r & 7) - 3.5 : ((double)(int64_t)r) / (double)(1 + below(1000000));
		}
		((double *)base)[i] = d;
		break;
	}
	case F32: {
		float f;
		if (special) {
			uint64_t pick = below(12);
			static const uint32_t nans[] = { 0x7FC00000u, 0xFFC00000u, 0x7F800001u, 0x7FA00BADu };
			static const float specials[] = { 0.0f, -0.0f, 1.0f, -1.0f, INFINITY, -INFINITY, 1.4e-45f, 3.4028235e38f };
			f = pick < 8 ? specials[pick] : nanWithBits32(nans[pick - 8]);
		} else {
			f = few ? (float)(r & 7) - 3.5f : (float)((double)(int32_t)r / (double)(1 + below(1000)));
		}
		((float *)base)[i] = f;
		break;
	}
	case I64: {
		static const int64_t specials[] = { INT64_MIN, INT64_MAX, 0, -1, 1, INT64_MIN + 1 };
		((int64_t *)base)[i] = special ? specials[below(6)] : (int64_t)r;
		break;
	}
	case U64: {
		static const uint64_t specials[] = { 0, UINT64_MAX, (uint64_t)INT64_MAX, (uint64_t)INT64_MIN, 1 };
		((uint64_t *)base)[i] = special ? specials[below(5)] : r;
		break;
	}
	case I32: {
		static const int32_t specials[] = { INT32_MIN, INT32_MAX, 0, -1, 1 };
		((int32_t *)base)[i] = special ? specials[below(5)] : (int32_t)r;
		break;
	}
	case U32: {
		static const uint32_t specials[] = { 0, UINT32_MAX, 0x80000000u, 1 };
		((uint32_t *)base)[i] = special ? specials[below(4)] : (uint32_t)r;
		break;
	}
	case U16:
		((uint16_t *)base)[i] = special ? (below(2) ? 0xFFFF : 0) : (uint16_t)r;
		break;
	default:
		((uint8_t *)base)[i] = special ? (below(2) ? 0xFF : 0) : (uint8_t)r;
		break;
	}
}

/* Fills the n elements of type at base with shape (sorted, reversed and
   the others are made from random elements ordered by the reference, so
   that they hold the special values too, and repeated ones) */
static void
fill(int type, void *base, int64_t n, int shape)
{
	size_t size = typeSizes[type];
	int64_t i;

	for (i = 0; i < n; i++)
		randomElement(type, base, i, shape == FEW_UNIQUE);
	if (shape == ALL_EQUAL) {
		for (i = 1; i < n; i++)
			memcpy((char *)base + i * size, base, size);
		return;
	}
	if (shape == SORTED || shape == REVERSED || shape == NEARLY_SORTED || shape == ORGAN_PIPE || shape == SAWTOOTH) {
		int64_t *perm = malloc((size_t)(n ? n : 1) * sizeof(int64_t)), offsets[4] = { 0 };
		char *copy = malloc(n ? (size_t)n * size : 1);

		if (!perm || !copy) {
			printf("ffitest: out of memory\n");
			exit(2);
		}
		/* sawtooth: four ascending runs, run r of the sorted elements r,
		   r + 4, r + 8, ... */
		for (i = 1; i < 4; i++)
			offsets[i] = offsets[i - 1] + (n - (i - 1) + 3) / 4;
		memcpy(copy, base, (size_t)n * size);
		referenceArgSort(type, copy, shape == REVERSED, perm, n);
		for (i = 0; i < n; i++) {
			int64_t to = i;

			if (shape == ORGAN_PIPE)
				to = i % 2 ? n - 1 - i / 2 : i / 2;
			else if (shape == SAWTOOTH)
				to = offsets[i % 4] + i / 4;
			memcpy((char *)base + to * size, copy + perm[i] * size, size);
		}
		if (shape == NEARLY_SORTED) {
			for (i = 0; i < n / 20 + 1 && n > 1; i++) {
				int64_t a = (int64_t)below((uint64_t)n), b = (int64_t)below((uint64_t)n);
				char t[8];

				memcpy(t, (char *)base + a * size, size);
				memcpy((char *)base + a * size, (char *)base + b * size, size);
				memcpy((char *)base + b * size, t, size);
			}
		}
		free(perm);
		free(copy);
	}
}


/*** F1 identity ***/

static void
testIdentity(void)
{
	int64_t supported = lib.coro_supported();
	const char *impl = lib.coro_impl();
	double start, ms;
	int64_t n = 1000000, i;
	double *data;

	CHECK(lib.protocol() == TS_FFI_PROTOCOL);
	CHECK(lib.protocol() == 1);
	CHECKF(lib.version() && strcmp(lib.version(), "TimSortPlugin 1.0") == 0, "version %s", lib.version());
	CHECKF(lib.timsort_version() && strcmp(lib.timsort_version(), "fd70381") == 0,
		"timsort.c %s", lib.timsort_version());
	CHECK(supported == 0 || supported == 1);
	CHECK(impl != NULL);
	if (impl) {
		CHECKF(supported ? strcmp(impl, "asm-x86_64-sysv") == 0 || strcmp(impl, "asm-aarch64") == 0
				|| strcmp(impl, "fibers") == 0
			: strcmp(impl, "stub") == 0 || strcmp(impl, "none") == 0,
			"coroutines %d, impl %s", (int)supported, impl);
	}
	CHECK(lib.coro_live() == 0);
	printf("libTimSortFFI: protocol %d, %s, timsort.c %s, coroutines %d (%s)\n",
		(int)lib.protocol(), lib.version(), lib.timsort_version(), (int)supported, impl ? impl : "?");

	/* the time of one sort of 1,000,000 random doubles */
	if (!(data = malloc((size_t)n * sizeof(double)))) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	for (i = 0; i < n; i++)
		data[i] = (double)(int64_t)next64() / 1e9;
	start = nowMillis();
	CHECK(lib.sort_f64(data, n, 0) == TS_FFI_OK);
	ms = nowMillis() - start;
	for (i = 1; i < n; i++) {
		if (data[i - 1] > data[i]) {
			CHECKF(0, "not sorted at %lld", (long long)i);
			break;
		}
	}
	printf("  ts_ffi_sort_f64: 1,000,000 random doubles in %.1f ms\n", ms);
	free(data);
}


/*** F2 buffers ***/

/* Sorts a copy of the n elements at data through the library and compares
   it bit by bit with the reference's order */
static void
checkSort(int type, const void *data, int64_t n, int descending, const char *shape)
{
	size_t size = typeSizes[type];
	char *got = malloc(n ? (size_t)n * size : 1), *want = malloc(n ? (size_t)n * size : 1);
	int64_t *perm = malloc((size_t)(n ? n : 1) * sizeof(int64_t)), i, rc;

	if (!got || !want || !perm) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	memcpy(got, data, (size_t)n * size);
	referenceArgSort(type, data, descending, perm, n);
	for (i = 0; i < n; i++)
		memcpy(want + i * size, (const char *)data + perm[i] * size, size);
	rc = librarySort(type, got, n, descendingArgument(descending, n));
	if (CHECKF(rc == TS_FFI_OK, "%s %s n=%lld descending=%d: status %lld",
			typeNames[type], shape, (long long)n, descending, (long long)rc)) {
		for (i = 0; i < n; i++) {
			if (memcmp(got + i * size, want + i * size, size) != 0) {
				CHECKF(0, "%s %s n=%lld descending=%d: differs from the reference at %lld",
					typeNames[type], shape, (long long)n, descending, (long long)i);
				break;
			}
		}
		if (i == n)
			checks++;
	}
	free(got);
	free(want);
	free(perm);
}

static void
testBuffers(void)
{
	static const int64_t sizes[] = { 100, 1000, 10000, 100000, 1000000 };
	int type, shape, descending, s;
	int64_t n;

	for (type = 0; type < TYPES; type++) {
		void *data = malloc(1000000 * typeSizes[type]);

		if (!data) {
			printf("ffitest: out of memory\n");
			exit(2);
		}
		for (n = 0; n <= 70; n++) {
			for (shape = 0; shape < SHAPES; shape++) {
				fill(type, data, n, shape);
				for (descending = 0; descending < 2; descending++)
					checkSort(type, data, n, descending, shapeNames[shape]);
			}
		}
		for (s = 0; s < (int)(sizeof sizes / sizeof sizes[0]); s++) {
			if (sizes[s] == 1000000 && !slow)
				continue;
			for (shape = 0; shape < SHAPES; shape++) {
				fill(type, data, sizes[s], shape);
				for (descending = 0; descending < 2; descending++)
					checkSort(type, data, sizes[s], descending, shapeNames[shape]);
			}
		}
		/* the order of NaNs and zeros by their bits: every NaN last
		   (first when descending), in their order of the input; -0.0 and
		   0.0 equal */
		if (type == F64) {
			double d[8] = { nanWithBits(0xFFF8000000000001ull), 0.0, nanWithBits(0x7FF8000000000002ull),
				-0.0, -INFINITY, 0.0, nanWithBits(0x7FF0000000000003ull), -0.0 };
			uint64_t want[8] = { 0xFFF0000000000000ull, 0, 0x8000000000000000ull, 0, 0x8000000000000000ull,
				0xFFF8000000000001ull, 0x7FF8000000000002ull, 0x7FF0000000000003ull };
			uint64_t wantDown[8] = { 0xFFF8000000000001ull, 0x7FF8000000000002ull, 0x7FF0000000000003ull,
				0, 0x8000000000000000ull, 0, 0x8000000000000000ull, 0xFFF0000000000000ull };
			double e[8];

			memcpy(e, d, sizeof d);
			CHECK(lib.sort_f64(e, 8, 0) == TS_FFI_OK);
			CHECK(memcmp(e, want, sizeof e) == 0);
			memcpy(e, d, sizeof d);
			CHECK(lib.sort_f64(e, 8, 7) == TS_FFI_OK);	/* any non-zero is descending */
			CHECK(memcmp(e, wantDown, sizeof e) == 0);
		}
		free(data);
	}
}


/*** F3 failures of the buffer sorts ***/

static void
testBufferFailures(void)
{
	uint64_t words[4] = { 4, 3, 2, 1 }, copy[4];
	char *bytes = (char *)words;
	int type;

	for (type = 0; type < TYPES; type++) {
		const char *name = typeNames[type];

		memcpy(copy, words, sizeof words);
		CHECKF(librarySort(type, words, -1, 0) == TS_FFI_BAD_ARGUMENT, "%s n -1", name);
		CHECKF(librarySort(type, words, INT64_MIN, 1) == TS_FFI_BAD_ARGUMENT, "%s n INT64_MIN", name);
		CHECKF(librarySort(type, NULL, 1, 0) == TS_FFI_BAD_ARGUMENT, "%s NULL, 1", name);
		CHECKF(librarySort(type, NULL, INT64_MAX, 0) == TS_FFI_BAD_ARGUMENT, "%s NULL, INT64_MAX", name);
		CHECKF(librarySort(type, NULL, 0, 0) == TS_FFI_OK, "%s NULL, 0", name);
		CHECKF(librarySort(type, words, 0, 0) == TS_FFI_OK, "%s n 0", name);
		CHECKF(librarySort(type, words, 1, 1) == TS_FFI_OK, "%s n 1", name);
		if (typeSizes[type] > 1) {
			CHECKF(librarySort(type, bytes + 1, 2, 0) == TS_FFI_BAD_ARGUMENT, "%s misaligned by 1", name);
			CHECKF(librarySort(type, bytes + typeSizes[type] / 2, 2, 0) == TS_FFI_BAD_ARGUMENT,
				"%s misaligned by half an element", name);
		}
		CHECKF(memcmp(copy, words, sizeof words) == 0, "%s: a refused sort touched the buffer", name);
		if (typeSizes[type] == 1) {
			bytes[1] = 9;
			CHECKF(librarySort(type, bytes + 1, 2, 0) == TS_FFI_OK, "%s at an odd address", name);
			CHECKF(bytes[1] == 0 && bytes[2] == 9, "%s at an odd address: sorted", name);
			memcpy(words, copy, sizeof words);
		}
	}
	/* a sort of 8-byte elements in place, not a copy (the buffer itself) */
	{
		int64_t v[3] = { 3, -1, 2 };
		int64_t w[3] = { -1, 2, 3 };

		CHECK(lib.sort_i64(v, 3, 0) == TS_FFI_OK);
		CHECK(memcmp(v, w, sizeof v) == 0);
	}
}


/*** F4 argsorts ***/

static int64_t
libraryArgSort(int type, const void *keys, int64_t *idx, int64_t n, int64_t descending)
{
	switch (type) {
	case I64: return lib.argsort_i64(keys, idx, n, descending);
	case U64: return lib.argsort_u64(keys, idx, n, descending);
	default: return lib.argsort_f64(keys, idx, n, descending);
	}
}

static void
checkArgSort(int type, const void *keys, int64_t n, int descending, const char *shape)
{
	int64_t *got = malloc((size_t)(n ? n : 1) * sizeof(int64_t)), *want = malloc((size_t)(n ? n : 1) * sizeof(int64_t));
	char *before = malloc(n ? (size_t)n * 8 : 1);
	int64_t rc, i;

	if (!got || !want || !before) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	memcpy(before, keys, (size_t)n * 8);
	for (i = 0; i < n; i++)
		got[i] = -7 - i;	/* the library fills idx itself */
	referenceArgSort(type, keys, descending, want, n);
	rc = libraryArgSort(type, keys, got, n, descendingArgument(descending, n));
	if (CHECKF(rc == TS_FFI_OK, "argsort %s %s n=%lld descending=%d: status %lld",
			typeNames[type], shape, (long long)n, descending, (long long)rc)) {
		CHECKF(memcmp(got, want, (size_t)n * sizeof(int64_t)) == 0,
			"argsort %s %s n=%lld descending=%d: differs from the reference",
			typeNames[type], shape, (long long)n, descending);
	}
	CHECKF(memcmp(before, keys, (size_t)n * 8) == 0, "argsort %s %s: the keys changed", typeNames[type], shape);
	free(got);
	free(want);
	free(before);
}

static void
testArgSorts(void)
{
	static const int types[] = { I64, U64, F64 };
	static const int64_t sizes[] = { 100, 1000, 10000, 100000, 1000000 };
	int t, shape, descending, s;
	int64_t n;
	void *keys = malloc(1000000 * 8);

	if (!keys) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	for (t = 0; t < 3; t++) {
		for (n = 0; n <= 70; n++) {
			for (shape = 0; shape < SHAPES; shape++) {
				fill(types[t], keys, n, shape);
				for (descending = 0; descending < 2; descending++)
					checkArgSort(types[t], keys, n, descending, shapeNames[shape]);
			}
		}
		for (s = 0; s < (int)(sizeof sizes / sizeof sizes[0]); s++) {
			if (sizes[s] == 1000000 && !slow)
				continue;
			for (shape = 0; shape < SHAPES; shape++) {
				fill(types[t], keys, sizes[s], shape);
				for (descending = 0; descending < 2; descending++)
					checkArgSort(types[t], keys, sizes[s], descending, shapeNames[shape]);
			}
		}
	}
	/* keys of f64 with NaNs and zeros: the permutation keeps their order */
	{
		double k[6] = { nanWithBits(0x7FF8000000000001ull), -0.0, 1.0, nanWithBits(0xFFF8000000000000ull), 0.0, -1.0 };
		int64_t idx[6], want[6] = { 5, 1, 4, 2, 0, 3 }, wantDown[6] = { 0, 3, 2, 1, 4, 5 };

		CHECK(lib.argsort_f64(k, idx, 6, 0) == TS_FFI_OK);
		CHECK(memcmp(idx, want, sizeof idx) == 0);
		CHECK(lib.argsort_f64(k, idx, 6, -1) == TS_FFI_OK);
		CHECK(memcmp(idx, wantDown, sizeof idx) == 0);
	}
	/* i64 and u64 differ on the sign bit */
	{
		int64_t k[3] = { -1, 1, 0 }, idx[3], wantSigned[3] = { 0, 2, 1 }, wantUnsigned[3] = { 2, 1, 0 };

		CHECK(lib.argsort_i64(k, idx, 3, 0) == TS_FFI_OK);
		CHECK(memcmp(idx, wantSigned, sizeof idx) == 0);
		CHECK(lib.argsort_u64((const uint64_t *)k, idx, 3, 0) == TS_FFI_OK);
		CHECK(memcmp(idx, wantUnsigned, sizeof idx) == 0);
	}
	free(keys);
}


/*** F5 failures of the argsorts ***/

static void
testArgSortFailures(void)
{
	static const int types[] = { I64, U64, F64 };
	int64_t storage[12], idx[5], copy[5];
	int64_t *keys = storage, i;
	int t;

	for (i = 0; i < 12; i++)
		storage[i] = 12 - i;
	for (t = 0; t < 3; t++) {
		const char *name = typeNames[types[t]];

		for (i = 0; i < 5; i++)
			idx[i] = 100 + i;
		memcpy(copy, idx, sizeof idx);
		CHECKF(libraryArgSort(types[t], keys, idx, -1, 0) == TS_FFI_BAD_ARGUMENT, "%s n -1", name);
		CHECKF(libraryArgSort(types[t], NULL, idx, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s NULL keys", name);
		CHECKF(libraryArgSort(types[t], keys, NULL, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s NULL idx", name);
		CHECKF(libraryArgSort(types[t], (char *)keys + 4, idx, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s keys misaligned", name);
		CHECKF(libraryArgSort(types[t], keys, (int64_t *)((char *)storage + 8 * 6 + 1), 3, 0) == TS_FFI_BAD_ARGUMENT,
			"%s idx misaligned", name);
		CHECKF(libraryArgSort(types[t], keys, keys, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s idx = keys", name);
		CHECKF(libraryArgSort(types[t], keys, keys + 2, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s idx overlaps keys' end", name);
		CHECKF(libraryArgSort(types[t], keys + 2, keys, 3, 0) == TS_FFI_BAD_ARGUMENT, "%s keys overlap idx's end", name);
		CHECKF(libraryArgSort(types[t], keys, NULL, INT64_MAX, 0) == TS_FFI_BAD_ARGUMENT, "%s INT64_MAX", name);
		CHECKF(memcmp(copy, idx, sizeof idx) == 0, "%s: a refused argsort touched idx", name);
		for (i = 0; i < 12; i++)
			CHECKF(storage[i] == 12 - i, "%s: a refused argsort touched the keys", name);
		CHECKF(libraryArgSort(types[t], NULL, NULL, 0, 0) == TS_FFI_OK, "%s NULL, NULL, 0", name);
		CHECKF(libraryArgSort(types[t], keys, idx, 0, 0) == TS_FFI_OK, "%s n 0", name);
		CHECKF(memcmp(copy, idx, sizeof idx) == 0, "%s: n 0 touched idx", name);
		/* adjacent buffers do not overlap */
		CHECKF(libraryArgSort(types[t], keys, keys + 3, 3, 0) == TS_FFI_OK, "%s idx right after keys", name);
		CHECKF(keys[3] == 2 && keys[4] == 1 && keys[5] == 0, "%s: the permutation of 12, 11, 10", name);
		CHECKF(libraryArgSort(types[t], keys + 6, keys + 3, 3, 1) == TS_FFI_OK, "%s idx right before keys", name);
		CHECKF(keys[3] == 0 && keys[4] == 1 && keys[5] == 2, "%s: the permutation of 6, 5, 4 descending", name);
		for (i = 0; i < 12; i++)
			storage[i] = 12 - i;
		CHECKF(libraryArgSort(types[t], keys, idx, 1, 0) == TS_FFI_OK && idx[0] == 0, "%s n 1", name);
	}
}


/*** F6 the resumable sort: its contract ***/

static int64_t
requestI(int64_t r)
{
	return r >> 32;
}

static int64_t
requestJ(int64_t r)
{
	return r & 0xFFFFFFFF;
}

/* Runs the sort id over the int64 keys from its request r (or from its
   start: drive) to its end, answering lt(i, j); answers its final status,
   and the number of steps in *steps.  When
   pharo is set, answers as the image does for the block [:a :b | a <= b]:
   (c[j] <= c[i]) not.  Every request is checked. */
static int64_t
driveFrom(int64_t id, int64_t r, const int64_t *keys, int64_t n, int pharo, int64_t *steps)
{
	int64_t count = 0;
	int bad = 0;

	while (r >= 0) {
		int64_t i = requestI(r), j = requestJ(r), answer;

		if (i < 0 || i >= n || j < 0 || j >= n || i == j) {
			if (!bad++)
				CHECKF(0, "request %lld: i %lld, j %lld, n %lld", (long long)r, (long long)i, (long long)j, (long long)n);
			return lib.coro_next(id, -1);
		}
		answer = pharo ? !(keys[j] <= keys[i]) : keys[i] < keys[j];
		r = lib.coro_next(id, answer);
		count++;
	}
	if (steps)
		*steps = count;
	return r;
}

static int64_t
drive(int64_t id, const int64_t *keys, int64_t n, int pharo, int64_t *steps)
{
	return driveFrom(id, lib.coro_next(id, 0), keys, n, pharo, steps);
}

/* A whole resumable sort of the n keys, compared with the reference */
static void
checkResumable(const int64_t *keys, int64_t n, int descending, int pharo, const char *what)
{
	int64_t *got = malloc((size_t)(n ? n : 1) * sizeof(int64_t)), *want = malloc((size_t)(n ? n : 1) * sizeof(int64_t));
	int64_t id, r, steps = 0;

	if (!got || !want) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	referenceArgSort(I64, keys, descending, want, n);
	id = lib.coro_new(n, descendingArgument(descending, n));
	if (!CHECKF(id >= 1, "%s n=%lld: new %lld", what, (long long)n, (long long)id))
		goto done;
	r = drive(id, keys, n, pharo, &steps);
	CHECKF(r == TS_FFI_DONE, "%s n=%lld descending=%d: %lld", what, (long long)n, descending, (long long)r);
	CHECK(lib.coro_next(id, 0) == TS_FFI_DONE);
	CHECK(lib.coro_permutation(id, got, n) == TS_FFI_OK);
	CHECKF(memcmp(got, want, (size_t)n * sizeof(int64_t)) == 0,
		"%s n=%lld descending=%d: differs from the reference", what, (long long)n, descending);
	CHECKF(n < 2 ? steps == 0 : steps >= n - 1, "%s n=%lld: %lld steps", what, (long long)n, (long long)steps);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
done:
	free(got);
	free(want);
}

static void
fillKeys(int64_t *keys, int64_t n, int shape)
{
	fill(I64, keys, n, shape);
	if (shape == FEW_UNIQUE) {
		int64_t i;

		for (i = 0; i < n; i++)
			keys[i] = (int64_t)below(5);
	}
}

static void
testUnsupported(void)
{
	int64_t perm[2];

	CHECK(lib.coro_supported() == 0);
	CHECK(lib.coro_new(10, 0) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_new(-1, 0) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_next(1, 0) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_permutation(1, perm, 2) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_free(1) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_size(1) == TS_FFI_UNSUPPORTED);
	CHECK(lib.coro_live() == 0);
	printf("  the resumable sort is not supported here: every ts_ffi_coro_* answers TS_FFI_UNSUPPORTED\n");
}

/* n = 2^31 - 1, the largest: the check of the arguments passes it, and its
   permutation (16 GiB) is refused for memory under an RLIMIT_AS just above
   the process (Linux, without AddressSanitizer) */
static void
testLargestResumable(void)
{
#if FFITEST_RLIMIT
	struct rlimit saved, limit;
	unsigned long pages = 0;
	FILE *f = fopen("/proc/self/statm", "r");
	int64_t id;

	if (!f || fscanf(f, "%lu", &pages) != 1 || getrlimit(RLIMIT_AS, &saved) != 0) {
		CHECKF(0, "largest n: no /proc/self/statm or RLIMIT_AS");
		if (f)
			fclose(f);
		return;
	}
	fclose(f);
	limit = saved;
	limit.rlim_cur = pages * (unsigned long)sysconf(_SC_PAGESIZE) + 64 * 1024 * 1024;
	if (setrlimit(RLIMIT_AS, &limit) != 0) {
		CHECKF(0, "largest n: setrlimit");
		return;
	}
	id = lib.coro_new(INT32_MAX, 0);
	setrlimit(RLIMIT_AS, &saved);
	CHECKF(id == TS_FFI_NO_MEMORY, "new of 2^31 - 1 elements: %lld, not TS_FFI_NO_MEMORY", (long long)id);
	if (id > 0)
		lib.coro_free(id);
	CHECK(lib.coro_live() == 0);
#endif
}

static void
testResumable(void)
{
	int64_t keys[2000], perm[2000], n, id, id2, r, last;
	int shape, descending;

	if (!lib.coro_supported()) {
		testUnsupported();
		return;
	}
	/* sizes 0-70 and 1000, every shape, both directions, both answers */
	for (n = 0; n <= 70; n++) {
		for (shape = 0; shape < SHAPES; shape++) {
			fillKeys(keys, n, shape);
			for (descending = 0; descending < 2; descending++) {
				checkResumable(keys, n, descending, 0, shapeNames[shape]);
				if (!descending)
					checkResumable(keys, n, 0, 1, shapeNames[shape]);
			}
		}
	}
	for (shape = 0; shape < SHAPES; shape++) {
		fillKeys(keys, 1000, shape);
		checkResumable(keys, 1000, 0, 0, shapeNames[shape]);
		checkResumable(keys, 1000, 1, 0, shapeNames[shape]);
		checkResumable(keys, 1000, 0, 1, shapeNames[shape]);
	}
	CHECK(lib.coro_live() == 0);

	/* n 0 and 1: done at the first call, and kept */
	for (n = 0; n < 2; n++) {
		id = lib.coro_new(n, 0);
		CHECK(id >= 1);
		CHECK(lib.coro_permutation(id, perm, n) == TS_FFI_BAD_ARGUMENT);	/* not started */
		CHECK(lib.coro_next(id, 0) == TS_FFI_DONE);
		CHECK(lib.coro_next(id, 1) == TS_FFI_DONE);
		perm[0] = 99;
		CHECK(lib.coro_permutation(id, perm, n) == TS_FFI_OK);
		CHECK(n == 0 ? perm[0] == 99 : perm[0] == 0);
		CHECK(lib.coro_permutation(id, NULL, 0) == (n == 0 ? TS_FFI_OK : TS_FFI_BAD_ARGUMENT));
		CHECK(lib.coro_free(id) == TS_FFI_OK);
	}

	/* the arguments of new */
	CHECK(lib.coro_new(-1, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_new(INT64_MIN, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_new((int64_t)INT32_MAX + 1, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_new(INT64_MAX, 1) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_live() == 0);
	testLargestResumable();

	/* ids count up, never reused; stale ids fail */
	id = lib.coro_new(3, 0);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
	id2 = lib.coro_new(3, 0);
	CHECKF(id2 > id, "ids %lld then %lld", (long long)id, (long long)id2);
	/* from a base of this load in [2^32, 2^58), not from 1, which the first
	   sort of every load (of every session of the image) would get again */
	CHECKF(id >= (int64_t)1 << 32 && id2 < (int64_t)1 << 59, "ids %lld and %lld: not from a base in [2^32, 2^58)",
		(long long)id, (long long)id2);
	CHECK(lib.coro_next(id, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_permutation(id, perm, 3) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_free(id) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_size(id) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_size(id2) == 3);
	CHECK(lib.coro_free(id2) == TS_FFI_OK);
	CHECK(lib.coro_size(id2) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_free(id2) == TS_FFI_BAD_ARGUMENT);	/* a second free */
	CHECK(lib.coro_next(0, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_next(-1, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_next(id2 + 1000, 0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_free(0) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_free(INT64_MIN) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_live() == 0);

	/* the permutation: before the end, a wrong n, NULL */
	fillKeys(keys, 50, RANDOM);
	id = lib.coro_new(50, 0);
	CHECK(lib.coro_live() == 1);
	r = lib.coro_next(id, 0);
	CHECK(r >= 0);
	CHECK(lib.coro_permutation(id, perm, 50) == TS_FFI_BAD_ARGUMENT);	/* pending */
	r = driveFrom(id, r, keys, 50, 0, NULL);
	CHECK(r == TS_FFI_DONE);
	CHECK(lib.coro_permutation(id, perm, 49) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_permutation(id, perm, 51) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_permutation(id, NULL, 50) == TS_FFI_BAD_ARGUMENT);
	CHECK(lib.coro_permutation(id, perm, 50) == TS_FFI_OK);
	{
		int64_t want[50];

		referenceArgSort(I64, keys, 0, want, 50);
		CHECK(memcmp(perm, want, sizeof want) == 0);
	}
	{
		/* misaligned dst: copied all the same */
		int64_t space[51];
		int64_t *odd = (int64_t *)((char *)space + 4);
		int64_t copy[50];

		CHECK(lib.coro_permutation(id, odd, 50) == TS_FFI_OK);
		memcpy(copy, odd, sizeof copy);
		CHECK(memcmp(copy, perm, sizeof copy) == 0);
	}
	CHECK(lib.coro_free(id) == TS_FFI_OK);

	/* answers above 1 count as 1 */
	fillKeys(keys, 300, RANDOM);
	{
		int64_t want[300], got[300];

		referenceArgSort(I64, keys, 0, want, 300);
		id = lib.coro_new(300, 0);
		r = lib.coro_next(id, 12345);
		while (r >= 0)
			r = lib.coro_next(id, keys[requestI(r)] < keys[requestJ(r)] ? 1 + (int64_t)below(1000000) : 0);
		CHECK(r == TS_FFI_DONE);
		CHECK(lib.coro_permutation(id, got, 300) == TS_FFI_OK);
		CHECK(memcmp(got, want, sizeof got) == 0);
		CHECK(lib.coro_free(id) == TS_FFI_OK);
	}

	/* abort: any negative answer; kept; no permutation; freed */
	{
		static const int64_t aborts[] = { -1, -2, -3, INT64_MIN };
		int a;

		for (a = 0; a < 4; a++) {
			id = lib.coro_new(300, a & 1);
			r = lib.coro_next(id, 0);
			for (n = 0; n < 37 && r >= 0; n++)
				r = lib.coro_next(id, keys[requestI(r)] < keys[requestJ(r)]);
			CHECK(r >= 0);
			last = lib.coro_next(id, aborts[a]);
			CHECKF(last == TS_FFI_ABORTED, "abort with %lld: %lld", (long long)aborts[a], (long long)last);
			CHECK(lib.coro_next(id, 1) == TS_FFI_ABORTED);
			CHECK(lib.coro_permutation(id, perm, 300) == TS_FFI_BAD_ARGUMENT);
			CHECK(lib.coro_live() == 1);
			CHECK(lib.coro_free(id) == TS_FFI_OK);
			CHECK(lib.coro_live() == 0);
		}
	}

	/* free of an unstarted, a pending and a finished sort */
	id = lib.coro_new(100, 0);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
	id = lib.coro_new(100, 1);
	CHECK(lib.coro_next(id, 0) >= 0);
	CHECK(lib.coro_next(id, 1) >= 0);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
	CHECK(lib.coro_next(id, 1) == TS_FFI_BAD_ARGUMENT);
	id = lib.coro_new(2, 0);
	CHECK(lib.coro_next(id, 0) >= 0);
	CHECK(lib.coro_next(id, 0) == TS_FFI_DONE);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
	CHECK(lib.coro_live() == 0);

	/* the request of two elements: (1 << 32) | 0 or (0 << 32) | 1; the
	   answer decides their order */
	id = lib.coro_new(2, 0);
	r = lib.coro_next(id, 0);
	CHECKF(r == ((int64_t)1 << 32) || r == 1, "request %llx", (unsigned long long)r);
	CHECK(lib.coro_next(id, 1) == TS_FFI_DONE);
	CHECK(lib.coro_permutation(id, perm, 2) == TS_FFI_OK);
	CHECK(perm[0] == requestI(r) && perm[1] == requestJ(r));
	CHECK(lib.coro_free(id) == TS_FFI_OK);
}


/*** F7 the resumable sort under load ***/

static void
testResumableLoad(void)
{
	int64_t ids[64], states[64], keysOf[64][40];
	int64_t keys[100000], *want, *got, id, extra, r, steps = 0, n;
	int k, live, round;
	double start, ms;

	if (!lib.coro_supported()) {
		printf("  (skipped: the resumable sort is not supported here)\n");
		return;
	}
	CHECK(lib.coro_live() == 0);

	/* 64 live sorts, interleaved: each one step in turn */
	for (k = 0; k < 64; k++) {
		fillKeys(keysOf[k], 40, k % SHAPES);
		ids[k] = lib.coro_new(40, k & 1);
		CHECKF(ids[k] >= 1, "sort %d: %lld", k, (long long)ids[k]);
		states[k] = lib.coro_next(ids[k], 0);
	}
	CHECK(lib.coro_live() == 64);
	CHECK(lib.coro_new(5, 0) == TS_FFI_LIMIT);
	CHECK(lib.coro_new(-1, 0) == TS_FFI_BAD_ARGUMENT);	/* the argument first */
	CHECK(lib.coro_free(ids[63]) == TS_FFI_OK);
	extra = lib.coro_new(5, 0);
	CHECK(extra > ids[63]);
	CHECK(lib.coro_new(5, 0) == TS_FFI_LIMIT);
	CHECK(lib.coro_free(extra) == TS_FFI_OK);
	ids[63] = lib.coro_new(40, 1);
	states[63] = lib.coro_next(ids[63], 0);
	do {
		live = 0;
		for (k = 0; k < 64; k++) {
			int64_t s = states[k];

			if (s >= 0) {
				int64_t i = requestI(s), j = requestJ(s);

				states[k] = lib.coro_next(ids[k], keysOf[k][i] < keysOf[k][j]);
				live++;
			}
		}
	} while (live);
	for (k = 0; k < 64; k++) {
		int64_t want40[40], got40[40];

		CHECKF(states[k] == TS_FFI_DONE, "sort %d: %lld", k, (long long)states[k]);
		referenceArgSort(I64, keysOf[k], k & 1, want40, 40);
		CHECK(lib.coro_permutation(ids[k], got40, 40) == TS_FFI_OK);
		CHECKF(memcmp(got40, want40, sizeof got40) == 0, "interleaved sort %d", k);
		CHECK(lib.coro_free(ids[k]) == TS_FFI_OK);
	}
	CHECK(lib.coro_live() == 0);

	/* nested: a whole sort between every two steps of another, the outer
	   one 3 levels deep */
	{
		int64_t outerKeys[200], outerWant[200], outerGot[200], innerKeys[30];
		int64_t outer, middle;

		fillKeys(outerKeys, 200, RANDOM);
		fillKeys(innerKeys, 30, RANDOM);
		referenceArgSort(I64, outerKeys, 0, outerWant, 200);
		outer = lib.coro_new(200, 0);
		middle = lib.coro_new(30, 1);
		r = lib.coro_next(outer, 0);
		CHECK(lib.coro_next(middle, 0) >= 0);
		round = 0;
		while (r >= 0) {
			if (round++ % 50 == 0)
				checkResumable(innerKeys, 30, round & 1, 1, "nested");
			r = lib.coro_next(outer, outerKeys[requestI(r)] < outerKeys[requestJ(r)]);
		}
		CHECK(r == TS_FFI_DONE);
		CHECK(lib.coro_permutation(outer, outerGot, 200) == TS_FFI_OK);
		CHECK(memcmp(outerGot, outerWant, sizeof outerGot) == 0);
		CHECK(lib.coro_live() == 2);
		CHECK(lib.coro_free(middle) == TS_FFI_OK);	/* pending since its first request */
		CHECK(lib.coro_free(outer) == TS_FFI_OK);
		CHECK(lib.coro_live() == 0);
	}

	/* random aborts and frees, mid-sort */
	for (round = 0; round < 200; round++) {
		n = 2 + (int64_t)below(500);
		fillKeys(keys, n, (int)below(SHAPES));
		id = lib.coro_new(n, (int64_t)below(2));
		r = lib.coro_next(id, 0);
		{
			int64_t stop = (int64_t)below((uint64_t)n * 4);

			for (steps = 0; steps < stop && r >= 0; steps++)
				r = lib.coro_next(id, keys[requestI(r)] < keys[requestJ(r)]);
		}
		if (r >= 0 && below(2))
			CHECK(lib.coro_next(id, -1) == TS_FFI_ABORTED);
		CHECK(lib.coro_free(id) == TS_FFI_OK);
	}
	CHECK(lib.coro_live() == 0);

	/* 100,000 random elements, and the cost of a step */
	n = 100000;
	want = malloc((size_t)n * sizeof(int64_t));
	got = malloc((size_t)n * sizeof(int64_t));
	if (!want || !got) {
		printf("ffitest: out of memory\n");
		exit(2);
	}
	fillKeys(keys, n, RANDOM);
	referenceArgSort(I64, keys, 0, want, n);
	id = lib.coro_new(n, 0);
	start = nowMillis();
	r = drive(id, keys, n, 0, &steps);
	ms = nowMillis() - start;
	CHECK(r == TS_FFI_DONE);
	CHECK(lib.coro_permutation(id, got, n) == TS_FFI_OK);
	CHECK(memcmp(got, want, (size_t)n * sizeof(int64_t)) == 0);
	CHECK(lib.coro_free(id) == TS_FFI_OK);
	CHECK(lib.coro_live() == 0);
	printf("  resumable sort: 100,000 random int64 in %lld steps, %.1f ms, %.1f ns per step\n",
		(long long)steps, ms, ms * 1e6 / (double)(steps ? steps : 1));
	free(want);
	free(got);
}


/*** F8 the exports of the library ***/

static void
testExports(void)
{
	static const char *const hidden[] = {
		"ts_sort_words", "ts_sort_buffer", "ts_argsort_buffer", "ts_number_compare", "ts_string_compare",
		"ts_timsort_version", "ts_coro_open", "ts_coro_next", "ts_coro_close", "ts_coro_close_all",
		"ts_coro_lookup", "ts_coro_available", "ts_coro_impl", "list_sort_impl",
		"pharo_timsort_list_sort_impl", "listsort", "timsort"
	};
	int i;

	if (!library) {
		printf("  (skipped: no --lib)\n");
		return;
	}
	for (i = 0; i < EXPORTS; i++)
		CHECKF(lookUp(exportNames[i]) != NULL, "%s is not exported", exportNames[i]);
	for (i = 0; i < (int)(sizeof hidden / sizeof hidden[0]); i++)
		CHECKF(lookUp(hidden[i]) == NULL, "%s is exported", hidden[i]);
}


/*** Main ***/

static const struct {
	const char *name;
	void (*run)(void);
} tests[] = {
	{ "F1", testIdentity },
	{ "F2", testBuffers },
	{ "F3", testBufferFailures },
	{ "F4", testArgSorts },
	{ "F5", testArgSortFailures },
	{ "F6", testResumable },
	{ "F7", testResumableLoad },
	{ "F8", testExports }
};

int
main(int argc, char **argv)
{
	const char *only = NULL, *path = NULL;
	double start = nowMillis();
	int i, t;

	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--slow") == 0) {
			slow = 1;
		} else if (strcmp(argv[i], "--only") == 0 && i + 1 < argc) {
			only = argv[++i];
		} else if (strcmp(argv[i], "--lib") == 0 && i + 1 < argc) {
			path = argv[++i];
		} else {
			printf("usage: ffitest [--lib PATH] [--only F1,F2,...] [--slow]\n");
			return 2;
		}
	}
	if (path) {
		if (!linkLibrary(path))
			return 2;
	} else if (!linkDirectly()) {
		return 2;
	}
	for (t = 0; t < (int)(sizeof tests / sizeof tests[0]); t++) {
		int before = failures;
		double began = nowMillis();

		if (only) {
			const char *p = strstr(only, tests[t].name);
			size_t len = strlen(tests[t].name);

			if (!p || (p != only && p[-1] != ',') || (p[len] != '\0' && p[len] != ','))
				continue;
		}
		currentTest = tests[t].name;
		printf("%s\n", tests[t].name);
		tests[t].run();
		printf("%s: %s (%.0f ms)\n", tests[t].name, failures == before ? "ok" : "FAILED", nowMillis() - began);
	}
	printf("ffitest%s%s: %d checks, %d failures, %.1f s\n", path ? " " : "", path ? path : "",
		checks, failures, (nowMillis() - start) / 1e3);
	return failures ? 1 : 0;
}
