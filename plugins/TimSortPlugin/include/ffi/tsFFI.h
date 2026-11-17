/* tsFFI.h -- the C interface of libTimSortFFI, which the package TimSort-FFI
 * (the submodule plugins/TimSortPlugin/timsort.st) calls through the FFI of
 * the image when the VM has no TimSortPlugin.  It runs the engine and the
 * resumable sort of the plugin (tsEngine.c, src/coro) unchanged.  No VM
 * header, no object pointer, no callback: every argument is a number or the
 * address of the first element of a buffer that the image pinned for the
 * call (or allocated outside the heap), and every answer is a number (or a
 * string of static storage).  The library keeps no state between calls but
 * the table of its live resumable sorts and their cached stack.
 *
 * Every int64 answer of a sort is TS_FFI_OK or a negative status below, the
 * statuses of tsEngine.h with the same values (tsFFI.c checks it).  The
 * order of the elements is the one of the plugin (docs/TimSort.md,
 * "Semantics"): by value; integers signed or unsigned as the function's
 * type; floats with every NaN after every number (the NaNs equal to each
 * other), -0.0 equal to 0.0; stable; descending (non-zero) is the stable
 * descending sort (equal elements keep their order, NaNs first).
 *
 * Threads: the sorts of buffers may run on any thread; the resumable sorts
 * (ts_ffi_coro_*) must all be driven from one thread (the image's callouts
 * of the same-thread FFI), see tsCoro.h. */
#ifndef TS_FFI_H
#define TS_FFI_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#if defined(_WIN32)
# define TS_FFI_EXPORT __declspec(dllexport)
#else
# define TS_FFI_EXPORT __attribute__((visibility("default")))
#endif

#define TS_FFI_PROTOCOL 1
#define TS_FFI_OK 0
#define TS_FFI_DONE (-1)		/* ts_ffi_coro_next: the sort is finished */
#define TS_FFI_NO_MEMORY (-2)		/* malloc (or the stack of a resumable sort) failed */
#define TS_FFI_ABORTED (-3)		/* ts_ffi_coro_next: the image answered < 0 */
#define TS_FFI_BAD_ARGUMENT (-4)	/* NULL, n < 0, a misaligned buffer, an unknown id, ... */
#define TS_FFI_UNSUPPORTED (-5)		/* no resumable sort on this target */
#define TS_FFI_LIMIT (-6)		/* TS_CORO_TABLE_SIZE (64) resumable sorts are live */

TS_FFI_EXPORT int64_t ts_ffi_protocol(void);		/* TS_FFI_PROTOCOL */
TS_FFI_EXPORT const char *ts_ffi_version(void);		/* "TimSortPlugin 1.0": the engine's, as primitiveInfo */
TS_FFI_EXPORT const char *ts_ffi_timsort_version(void);	/* "fd70381": the commit of timsort.c */


/*** Buffers, sorted in place ***/

/* Each sorts the n elements at base in place, stable, and answers
 * TS_FFI_OK; TS_FFI_NO_MEMORY (f64, i64, u64: the elements are a
 * permutation of the input; the narrower types, which are sorted in a
 * malloc'ed 8-byte copy: the elements are untouched); TS_FFI_BAD_ARGUMENT
 * when n < 0, base is NULL with n > 0, or base is not aligned to the size of
 * an element.  n < 2 is TS_FFI_OK at once.  Elements are moved, never
 * converted (NaN payloads and signed zeros are kept). */
TS_FFI_EXPORT int64_t ts_ffi_sort_f64(double *base, int64_t n, int64_t descending);	/* Float64Array */
TS_FFI_EXPORT int64_t ts_ffi_sort_f32(float *base, int64_t n, int64_t descending);	/* Float32Array */
TS_FFI_EXPORT int64_t ts_ffi_sort_i64(int64_t *base, int64_t n, int64_t descending);	/* SmallIntegers extracted by the image */
TS_FFI_EXPORT int64_t ts_ffi_sort_u64(uint64_t *base, int64_t n, int64_t descending);	/* DoubleWordArray */
TS_FFI_EXPORT int64_t ts_ffi_sort_i32(int32_t *base, int64_t n, int64_t descending);	/* IntegerArray */
TS_FFI_EXPORT int64_t ts_ffi_sort_u32(uint32_t *base, int64_t n, int64_t descending);	/* WordArray, Bitmap */
TS_FFI_EXPORT int64_t ts_ffi_sort_u16(uint16_t *base, int64_t n, int64_t descending);	/* DoubleByteArray */
TS_FFI_EXPORT int64_t ts_ffi_sort_u8(uint8_t *base, int64_t n, int64_t descending);	/* ByteArray */


/*** Permutations by keys (argsort) ***/

/* Each writes into idx the n indices 0 .. n-1 of the n keys at keys,
 * ordered by their keys (stable): key idx[0] is the smallest (descending:
 * the largest).  The keys are read, never written.  The image applies the
 * permutation (the elements of a collection, kept by identity: element
 * idx[k] + 1 goes to position k + 1).  Answers TS_FFI_OK; TS_FFI_NO_MEMORY
 * (idx is a permutation of 0 .. n-1); TS_FFI_BAD_ARGUMENT when n < 0, keys
 * or idx is NULL with n > 0, or either is not aligned to 8 bytes. */
TS_FFI_EXPORT int64_t ts_ffi_argsort_i64(const int64_t *keys, int64_t *idx, int64_t n, int64_t descending);
TS_FFI_EXPORT int64_t ts_ffi_argsort_u64(const uint64_t *keys, int64_t *idx, int64_t n, int64_t descending);
TS_FFI_EXPORT int64_t ts_ffi_argsort_f64(const double *keys, int64_t *idx, int64_t n, int64_t descending);


/*** The resumable sort: any sortBlock (tsCoro.h) ***/

/* 1 when this library can run resumable sorts (a stack switch for this
 * target, set up), else 0: then every other ts_ffi_coro_* function answers
 * TS_FFI_UNSUPPORTED (ts_ffi_coro_live 0) */
TS_FFI_EXPORT int64_t ts_ffi_coro_supported(void);
/* The switch: "asm-x86_64-sysv", "asm-aarch64", "fibers", "stub" or "none" */
TS_FFI_EXPORT const char *ts_ffi_coro_impl(void);

/* A new sort of the permutation 0 .. n-1, descending when descending != 0.
 * Answers its id, >= 2^32 and below 2^59 (ids count up from a random base
 * of each load of the library and are never reused while it is loaded), or
 * TS_FFI_BAD_ARGUMENT (n outside [0, 2^31 - 1]),
 * TS_FFI_UNSUPPORTED, TS_FFI_LIMIT (64 sorts live), TS_FFI_NO_MEMORY. */
TS_FFI_EXPORT int64_t ts_ffi_coro_new(int64_t n, int64_t descending);

/* Runs the sort id to its next comparison or its end: a request
 * (i << 32) | j >= 0, i and j 0-based indices of the elements whose order
 * the next call answers (1: element i must come before element j, 0: not,
 * negative: abort; the image's answer for aBlock is
 * (aBlock value: (c at: j + 1) value: (c at: i + 1)) ifTrue: [0] ifFalse: [1]);
 * or TS_FFI_DONE, TS_FFI_NO_MEMORY, TS_FFI_ABORTED, which it keeps answering
 * once finished; TS_FFI_BAD_ARGUMENT when id is not live;
 * TS_FFI_UNSUPPORTED.  The answer of the first call is ignored (pass 0). */
TS_FFI_EXPORT int64_t ts_ffi_coro_next(int64_t id, int64_t answer);

/* Copies the sorted permutation of the sort id into dst: n int64_t, 0-based
 * indices (element dst[k] + 1 goes to position k + 1).  Answers TS_FFI_OK;
 * TS_FFI_BAD_ARGUMENT when id is not live, the sort has not answered
 * TS_FFI_DONE, n is not its size, or dst is NULL with n > 0 (dst untouched);
 * TS_FFI_UNSUPPORTED. */
TS_FFI_EXPORT int64_t ts_ffi_coro_permutation(int64_t id, int64_t *dst, int64_t n);

/* Frees the sort id, in any state (a pending sort is aborted first).
 * Answers TS_FFI_OK, or TS_FFI_BAD_ARGUMENT when id is not live (already
 * freed: the image's ensure: blocks may free twice), or TS_FFI_UNSUPPORTED. */
TS_FFI_EXPORT int64_t ts_ffi_coro_free(int64_t id);

/* The live resumable sorts of the library */
TS_FFI_EXPORT int64_t ts_ffi_coro_live(void);

/* The number of elements of the live sort id (n of its ts_ffi_coro_new),
 * or TS_FFI_BAD_ARGUMENT when id is not live, TS_FFI_UNSUPPORTED.  The
 * image asks it to tell an id that is not live from a wrong size, which
 * ts_ffi_coro_permutation answers alike, and so keeps no table of its own
 * (whose update between ts_ffi_coro_new and the caller could be cut short
 * by a terminated Process). */
TS_FFI_EXPORT int64_t ts_ffi_coro_size(int64_t id);

#ifdef __cplusplus
}
#endif
#endif
