/* tsCoro.h -- the resumable sort: timsort.c's sort of a permutation runs on
 * a C stack of its own, and every comparison suspends it and hands the pair
 * of indices to the caller, who evaluates it (in the image: a sortBlock, in
 * the caller's Process) and answers on the next call, which resumes the
 * sort where it stopped.  No callback into the VM: the caller only ever
 * makes plain C calls that return on its own stack.  Used by the plugin's
 * primitiveBlockSort* (TimSortPlugin.c) and by libTimSortFFI's ts_ffi_coro_*
 * (tsFFI.c), each linking its own copy of src/coro (the tables, the stack
 * cache and the counters below are per copy).  The contract is
 * docs/TimSort.md, sections "Primitive contract" and "Library contract".
 *
 * TS_CORO_SUPPORTED tells, at compile time, whether the target has a stack
 * switch: hand-written for x86-64 System V (Linux, macOS) and AArch64
 * (Linux, with a `bti c` landing pad where the code needs one; macOS),
 * Windows Fibers on Windows x64 and arm64.  Elsewhere (Emscripten, whose
 * WebAssembly cannot switch its call stack without ASYNCIFY or JSPI, and
 * any other target) it is 0: the functions below are not declared,
 * src/coro is not compiled into the Emscripten VM (cmake/plugins.cmake),
 * and the glue's primitives answer PrimErrUnsupported.
 *
 * A sort: ts_coro_open(n, descending) makes the permutation 0 .. n-1 and a
 * stack, and answers an id; each ts_coro_next(h, answer) answers the next
 * request (i << 32) | j, "is element i before element j?", until it answers
 * TS_DONE; ts_coro_permutation(h) is then the stable sorted order (element
 * permutation[k] goes to position k); ts_coro_close(id) frees it.  See
 * ts_coro_next for the encoding and the answers.
 *
 * Threads: none of this is thread-safe.  One thread drives every sort of a
 * copy (the VM's interpreter thread, or the thread of the FFI callouts);
 * Windows: the first sort converts that thread to a fiber
 * (ConvertThreadToFiber) unless it is one already (IsThreadAFiber, then
 * GetCurrentFiber), once, and never converts it back.  A sort may be
 * resumed from any depth of the caller's C stack (the caller's context is
 * saved afresh at every resume), and any number of sorts may be live and
 * interleaved (each has its stack); nothing of a sort may be called from
 * inside its own comparison (no comparison here calls anything: the sort
 * side only suspends).
 *
 * Signals and the VM: the sort side runs only inside ts_coro_next, between
 * two requests, and calls nothing but timsort.c and the switch; signal
 * handlers that run there use the sort's stack (TS_CORO_STACK_SIZE, ample:
 * the sort itself needs about 4 KB). */
#ifndef TS_CORO_H
#define TS_CORO_H
#include <stddef.h>
#include <stdint.h>
#include "tsEngine.h"
#ifdef __cplusplus
extern "C" {
#endif

#if defined(__EMSCRIPTEN__)
# define TS_CORO_SUPPORTED 0
#elif defined(_WIN32) && (defined(_M_X64) || defined(__x86_64__) || defined(_M_ARM64) || defined(__aarch64__))
# define TS_CORO_SUPPORTED 1	/* Windows Fibers */
#elif !defined(_WIN32) && (defined(__x86_64__) || defined(__aarch64__)) && (defined(__linux__) || defined(__APPLE__))
# define TS_CORO_SUPPORTED 1	/* the hand-written switch, ELF or Mach-O */
#else
# define TS_CORO_SUPPORTED 0
#endif

/* The usable stack of a sort, below which a guard page (PROT_NONE, or the
 * guard of a fiber's stack) stops an overflow */
#define TS_CORO_STACK_SIZE (256 * 1024)
/* The elements of a sort at most: requests pack two indices in 32 bits each
 * (the plugin allows fewer: its requests are SmallIntegers, 2^28 at most) */
#define TS_CORO_MAX_SIZE ((int64_t)INT32_MAX)
/* Live sorts at most, per copy (the plugin, libTimSortFFI) */
#define TS_CORO_TABLE_SIZE 64
/* Stacks kept for the next sorts when one is freed (mmap and munmap of a
 * stack cost microseconds), per copy */
#define TS_CORO_STACK_CACHE 1

#if TS_CORO_SUPPORTED

typedef struct ts_coro ts_coro;

/* 1 when this build can switch stacks, 0 when not (a stub of src/coro, or a
 * switch that failed to set up: ConvertThreadToFiber failed).  primitiveInfo
 * slot 5 and ts_ffi_coro_supported answer it. */
TS_INTERNAL int ts_coro_available(void);

/* The switch: "asm-x86_64-sysv", "asm-aarch64", "fibers", or "stub" (static
 * storage) */
TS_INTERNAL const char *ts_coro_impl(void);

/* A new sort of the permutation 0 .. n-1 (int64_t indices, the handle's
 * own), stable, descending when descending != 0.  Answers the id of the
 * sort, >= 2^32 and below 2^59: ids count up, in each load of a copy,
 * from a base drawn at its first sort, and are never reused within the
 * load, so a stale id of this load (a second close) is never live again;
 * an id of an earlier load (a Process of the image saved in the middle of
 * a sort) names a live sort of this one only by a 2^-58 chance, which the
 * image avoids besides by checking the session.  Answers 0 when it failed,
 * and sets
 * *status (status may be NULL): TS_BAD_ARGUMENT for n outside [0,
 * TS_CORO_MAX_SIZE]; TS_UNSUPPORTED when ts_coro_available() is 0;
 * TS_LIMIT when TS_CORO_TABLE_SIZE sorts are live; TS_NO_MEMORY when the
 * handle, the permutation or the stack could not be allocated.  *status is
 * TS_OK on success.  The sort does not start before the first
 * ts_coro_next. */
TS_INTERNAL int64_t ts_coro_open(int64_t n, int descending, int *status);

/* The live sort of id, or NULL when id is not live (never opened, or
 * closed) */
TS_INTERNAL ts_coro *ts_coro_lookup(int64_t id);

/* Frees the sort of id, in any state: when a request is pending, the sort
 * is resumed with "abort" answers until timsort.c returns (it frees its
 * merge buffer), then its stack goes back to the cache (or is unmapped)
 * and the handle and the permutation are freed.  Answers TS_OK, or
 * TS_BAD_ARGUMENT when id is not live (nothing happens). */
TS_INTERNAL int ts_coro_close(int64_t id);

/* Closes every live sort and frees the cached stacks: shutdownModule of the
 * plugin, and the end of a test.  Ids keep counting. */
TS_INTERNAL void ts_coro_close_all(void);

/* The live sorts of this copy: primitiveInfo slot 6, ts_ffi_coro_live */
TS_INTERNAL int64_t ts_coro_live(void);

/* Runs the sort h to its next comparison or to its end, and answers:
 *
 *   >= 0       a request: (i << 32) | j, i and j in [0, n), two elements
 *              (indices into the collection, 0-based, the values of the
 *              permutation, not positions in it).  The next call must
 *              answer lt(i, j), "element i must come before element j"
 *              (strictly): 1 for true, 0 for false, any negative value to
 *              abort the sort (an answer > 1 is taken as 1).  The image's
 *              lt for a sortBlock aBlock is
 *                  (aBlock value: (c at: j + 1) value: (c at: i + 1)) not
 *              which for a <=-style block is Pharo's sort: exactly.
 *   TS_DONE    (-1) the sort is finished: ts_coro_permutation is sorted
 *   TS_NO_MEMORY (-2) timsort.c's merge buffer could not be allocated
 *   TS_ABORTED (-3) the caller answered < 0
 *   TS_BAD_ARGUMENT (-4) h is NULL
 *
 * The answer of the first call is ignored (pass 0).  A sort of n < 2
 * answers TS_DONE at the first call.  Once finished (TS_DONE, TS_NO_MEMORY,
 * TS_ABORTED) the sort stays so: later calls answer the same value at once.
 * After TS_NO_MEMORY or TS_ABORTED the permutation is a permutation of
 * 0 .. n-1 in no useful order.  The handle is freed only by ts_coro_close:
 * finishing frees nothing but the stack, which goes back to the cache. */
TS_INTERNAL int64_t ts_coro_next(ts_coro *h, int64_t answer);

/* 0 while the sort h has not finished (not started, or a request pending),
 * else its final answer: TS_DONE, TS_NO_MEMORY or TS_ABORTED */
TS_INTERNAL int ts_coro_result(const ts_coro *h);

/* The elements of the sort h: its n */
TS_INTERNAL int64_t ts_coro_size(const ts_coro *h);

/* The permutation of the sort h: n int64_t, 0-based indices, the handle's
 * own (valid until ts_coro_close); sorted once ts_coro_result(h) is
 * TS_DONE.  It may be read at any time (the sort side is suspended whenever
 * the caller runs), never written. */
TS_INTERNAL const int64_t *ts_coro_permutation(const ts_coro *h);

#endif /* TS_CORO_SUPPORTED */

#ifdef __cplusplus
}
#endif
#endif
