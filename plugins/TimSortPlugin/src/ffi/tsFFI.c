/* tsFFI.c -- libTimSortFFI: the engine (tsEngine.h) and the resumable sort
 * (tsCoro.h) of the TimSortPlugin for the FFI of the image (tsFFI.h has the
 * contract; docs/TimSort.md, "Library contract").  Each ts_ffi_* function
 * answers TS_FFI_OK or a status of tsFFI.h, the engine's statuses with the
 * same values (checked below).
 *
 * The library sees addresses and numbers, never objects: the image checks
 * what only it can see (the class of a buffer, a read-only receiver, the
 * range of a collection) before calling, and passes the first element of
 * the range.  The checks here are those on numbers and addresses: n < 0,
 * NULL with n > 0, the alignment of an element; the engine makes them
 * again.  An argsort also refuses an index buffer that overlaps its keys
 * (writing the indices would change the keys under the sort).
 *
 * The resumable sorts are known to the image by their ids, numbers of the
 * table of tsCoro.h, never by a C pointer: a stale id (a second free)
 * finds no sort and fails with TS_FFI_BAD_ARGUMENT instead of touching
 * freed memory.  An id of an earlier session (a Process saved in the
 * middle of a sort) is one of an earlier load of the library, whose ids
 * count up from another random base (tsCoro.h): it names a live sort of
 * this load only by a 2^-58 chance, and the image checks the session before
 * it uses an id (TimSort class>>blockSort:from:to:with:).  The table and the cached stack are
 * this library's own copy (the plugin, when loaded in the same process,
 * has its own).
 *
 * No VM header, no callback, no state between calls but the table of
 * tsCoro.h: the sorts of buffers are reentrant and may run on any thread;
 * the resumable sorts are driven from one thread (tsCoro.h).
 *
 * This file is not in src/common, which the plugin's CMake globs: the plugin
 * does not export these functions.  The library is built with hidden
 * visibility (the engine and src/coro are TS_INTERNAL besides): only the
 * TS_FFI_EXPORT functions are seen, so that the engine of a TimSortPlugin
 * loaded in the same process never gets in between. */

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "tsEngine.h"
#include "tsCoro.h"
#include "tsFFI.h"

_Static_assert(TS_FFI_PROTOCOL == TS_PROTOCOL, "tsFFI.h: the protocol is the engine's");
_Static_assert(TS_FFI_OK == TS_OK && TS_FFI_DONE == TS_DONE && TS_FFI_NO_MEMORY == TS_NO_MEMORY
	&& TS_FFI_ABORTED == TS_ABORTED && TS_FFI_BAD_ARGUMENT == TS_BAD_ARGUMENT
	&& TS_FFI_UNSUPPORTED == TS_UNSUPPORTED && TS_FFI_LIMIT == TS_LIMIT,
	"tsFFI.h: the statuses are the engine's");
_Static_assert(sizeof(double) == 8 && sizeof(float) == 4, "tsFFI.h: IEEE binary64 and binary32");

TS_FFI_EXPORT int64_t
ts_ffi_protocol(void)
{
	return TS_FFI_PROTOCOL;
}

TS_FFI_EXPORT const char *
ts_ffi_version(void)
{
	return TS_PLUGIN_VERSION;
}

TS_FFI_EXPORT const char *
ts_ffi_timsort_version(void)
{
	return ts_timsort_version();
}


/*** Buffers, sorted in place ***/

/* The engine checks n, NULL and the alignment, in that order, and answers
   as tsFFI.h says (a widened kind leaves the elements untouched on
   TS_NO_MEMORY) */
static int64_t
sortBuffer(void *base, int64_t n, int kind, int64_t descending)
{
	return ts_sort_buffer(base, n, kind, descending != 0);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_f64(double *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_F64, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_f32(float *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_F32, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_i64(int64_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_I64, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_u64(uint64_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_U64, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_i32(int32_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_I32, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_u32(uint32_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_U32, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_u16(uint16_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_U16, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_sort_u8(uint8_t *base, int64_t n, int64_t descending)
{
	return sortBuffer(base, n, TS_KIND_U8, descending);
}


/*** Permutations by keys (argsort) ***/

/* Checks the arguments (idx untouched when they are wrong), fills idx with
   0 .. n-1 and sorts it by the keys: the engine's argsort with 0-based
   indices (shift 0, origin 0), whose pre-pass then finds every index in
   range */
static int64_t
argSort(const void *keys, int64_t *idx, int64_t n, int kind, int64_t descending)
{
	uintptr_t k = (uintptr_t)keys, p = (uintptr_t)idx;
	int64_t i;

	if (n < 0 || (n > 0 && (!keys || !idx)) || (k & 7) || (p & 7)) {
		return TS_FFI_BAD_ARGUMENT;
	}
	if (n == 0) {
		return TS_FFI_OK;
	}
	/* 8-byte elements, both: they overlap when each starts before the
	   other ends (n <= INT64_MAX / 8, or neither buffer could exist) */
	if ((uint64_t)n > (uint64_t)(INT64_MAX / 8) || (k < p + (uint64_t)n * 8 && p < k + (uint64_t)n * 8)) {
		return TS_FFI_BAD_ARGUMENT;
	}
	for (i = 0; i < n; i++) {
		idx[i] = i;
	}
	return ts_argsort_buffer(keys, n, kind, idx, n, 0, 0, descending != 0);
}

TS_FFI_EXPORT int64_t
ts_ffi_argsort_i64(const int64_t *keys, int64_t *idx, int64_t n, int64_t descending)
{
	return argSort(keys, idx, n, TS_KIND_I64, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_argsort_u64(const uint64_t *keys, int64_t *idx, int64_t n, int64_t descending)
{
	return argSort(keys, idx, n, TS_KIND_U64, descending);
}

TS_FFI_EXPORT int64_t
ts_ffi_argsort_f64(const double *keys, int64_t *idx, int64_t n, int64_t descending)
{
	return argSort(keys, idx, n, TS_KIND_F64, descending);
}


/*** The resumable sort: any sortBlock (tsCoro.h) ***/

/* Every ts_ffi_coro_* function first asks this: without a stack switch for
   the target (TS_CORO_SUPPORTED 0, src/coro compiled to nothing) or with
   one that failed to set up, each answers TS_FFI_UNSUPPORTED, whatever its
   arguments */

TS_FFI_EXPORT int64_t
ts_ffi_coro_supported(void)
{
#if TS_CORO_SUPPORTED
	return ts_coro_available() != 0;
#else
	return 0;
#endif
}

TS_FFI_EXPORT const char *
ts_ffi_coro_impl(void)
{
#if TS_CORO_SUPPORTED
	return ts_coro_impl();
#else
	return "none";
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_new(int64_t n, int64_t descending)
{
#if TS_CORO_SUPPORTED
	int status = TS_OK;
	int64_t id;

	if (!ts_coro_available()) {
		return TS_FFI_UNSUPPORTED;
	}
	id = ts_coro_open(n, descending != 0, &status);
	return id > 0 ? id : status != TS_OK ? status : TS_FFI_NO_MEMORY;
#else
	(void)n; (void)descending;
	return TS_FFI_UNSUPPORTED;
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_next(int64_t id, int64_t answer)
{
#if TS_CORO_SUPPORTED
	ts_coro *h;

	if (!ts_coro_available()) {
		return TS_FFI_UNSUPPORTED;
	}
	if (!(h = ts_coro_lookup(id))) {
		return TS_FFI_BAD_ARGUMENT;
	}
	return ts_coro_next(h, answer);
#else
	(void)id; (void)answer;
	return TS_FFI_UNSUPPORTED;
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_permutation(int64_t id, int64_t *dst, int64_t n)
{
#if TS_CORO_SUPPORTED
	const ts_coro *h;

	if (!ts_coro_available()) {
		return TS_FFI_UNSUPPORTED;
	}
	if (!(h = ts_coro_lookup(id)) || ts_coro_result(h) != TS_DONE || n != ts_coro_size(h)
		|| (n > 0 && !dst)) {
		return TS_FFI_BAD_ARGUMENT;
	}
	if (n > 0) {
		/* (memcpy: dst need not be aligned) */
		memcpy(dst, ts_coro_permutation(h), (size_t)n * sizeof(int64_t));
	}
	return TS_FFI_OK;
#else
	(void)id; (void)dst; (void)n;
	return TS_FFI_UNSUPPORTED;
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_free(int64_t id)
{
#if TS_CORO_SUPPORTED
	if (!ts_coro_available()) {
		return TS_FFI_UNSUPPORTED;
	}
	return ts_coro_close(id) == TS_OK ? TS_FFI_OK : TS_FFI_BAD_ARGUMENT;
#else
	(void)id;
	return TS_FFI_UNSUPPORTED;
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_live(void)
{
#if TS_CORO_SUPPORTED
	return ts_coro_available() ? ts_coro_live() : 0;
#else
	return 0;
#endif
}

TS_FFI_EXPORT int64_t
ts_ffi_coro_size(int64_t id)
{
#if TS_CORO_SUPPORTED
	const ts_coro *h;

	if (!ts_coro_available()) {
		return TS_FFI_UNSUPPORTED;
	}
	return (h = ts_coro_lookup(id)) ? ts_coro_size(h) : TS_FFI_BAD_ARGUMENT;
#else
	(void)id;
	return TS_FFI_UNSUPPORTED;
#endif
}
