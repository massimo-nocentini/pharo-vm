/* tsEngine.h -- the C interface between the TimSortPlugin glue
 * (TimSortPlugin.c, the only file that includes VM headers), libTimSortFFI
 * (tsFFI.c), the resumable sort (tsCoro.h) and the engine (tsEngine.c, the
 * only file that includes timsort.h; timsort.c itself is the static library
 * pharo_timsort, cmake/timsort.cmake).  No VM type and no object pointer:
 * the engine sorts 8-byte words with a comparison given by its caller,
 * buffers of typed numbers, and permutations of indices by typed keys, and
 * it gives the exact order of numbers and of strings that the glue's
 * comparisons of objects use.  The contract is docs/TimSort.md, sections
 * "Primitive contract" and "Library contract".
 *
 * Every sort is timsort.c's list_sort_impl, with CPython's defaults (binary
 * insertion, plain branch): stable, and adaptive (one comparison per
 * element on sorted or reversed input).  descending (non-zero) is
 * timsort's reverse flag, the stable descending sort: elements that compare
 * equal keep their order of the input, as Pharo's sort: [:a :b | a >= b]
 * keeps them; it is not an ascending sort reversed.
 *
 * Memory: timsort.c's merge buffer (at most n/2 words, malloc'ed beyond 256
 * of them), and the 8-byte copy of a widened buffer (ts_sort_buffer); both
 * freed before returning.  No global state: every function is reentrant
 * (a comparison may run another sort) and may run on several threads at once
 * on different data.  Nothing here calls back into a VM.
 *
 * Answers: TS_OK, or a negative status of the enum below.  A failure never
 * loses an element: the data is then a permutation of the input (or the
 * input itself where a function says so). */
#ifndef TS_ENGINE_H
#define TS_ENGINE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

#define TS_PROTOCOL 1				/* primitiveInfo slot 1, ts_ffi_protocol */
#define TS_PLUGIN_VERSION "TimSortPlugin 1.0"	/* primitiveInfo slot 2 */

/* The engine and the resumable sort are linked into the plugin (whose C
 * entry points must keep the default visibility) and into libTimSortFFI:
 * their functions are hidden, so that neither exports them */
#if defined(__GNUC__) && !defined(_WIN32)
# define TS_INTERNAL __attribute__((visibility("hidden")))
#else
# define TS_INTERNAL
#endif

/* Statuses: the answers of the engine, of the resumable sort (tsCoro.h)
 * and, with the same values, of libTimSortFFI (TS_FFI_*, tsFFI.h) */
enum {
	TS_OK = 0,
	TS_DONE = -1,		/* the resumable sort finished: the permutation is sorted */
	TS_NO_MEMORY = -2,	/* malloc failed (or mmap, for a stack of the resumable sort) */
	TS_ABORTED = -3,	/* the caller's comparison answered < 0 */
	TS_BAD_ARGUMENT = -4,	/* outside the contract: NULL, n < 0, an unknown kind, a misaligned
				   base, an index out of range, an unknown handle */
	TS_UNSUPPORTED = -5,	/* the resumable sort on a target without a stack switch */
	TS_LIMIT = -6		/* the table of live resumable sorts is full (TS_CORO_TABLE_SIZE) */
};

/* Kinds of numeric elements: the codes the image passes (TimSort kindFloat64
 * ... in the package TimSort), the plugin checks against the format of the
 * object, and libTimSortFFI's typed functions stand for.  Order: by value,
 * integers signed or unsigned as the kind says; floats with every NaN after
 * every number (NaNs equal to each other, whatever their sign and payload),
 * -0.0 equal to 0.0, infinities at their places.
 *
 *   kind		bytes	element			Pharo class (Spur format)
 *   TS_KIND_F64  1	8	IEEE binary64		Float64Array (9)
 *   TS_KIND_F32  2	4	IEEE binary32		Float32Array (10)
 *   TS_KIND_I64  3	8	signed 64-bit		none in Pharo 12-15 (format 9; FFI keys)
 *   TS_KIND_U64  4	8	unsigned 64-bit		DoubleWordArray (9)
 *   TS_KIND_I32  5	4	signed 32-bit		IntegerArray (10)
 *   TS_KIND_U32  6	4	unsigned 32-bit		WordArray, Bitmap (10)
 *   TS_KIND_U16  7	2	unsigned 16-bit		DoubleByteArray (12)
 *   TS_KIND_U8   8	1	unsigned 8-bit		ByteArray (16)
 *
 * Codes 0 and 9 and above are unknown (TS_BAD_ARGUMENT); new kinds get new
 * codes. */
enum {
	TS_KIND_F64 = 1, TS_KIND_F32 = 2, TS_KIND_I64 = 3, TS_KIND_U64 = 4,
	TS_KIND_I32 = 5, TS_KIND_U32 = 6, TS_KIND_U16 = 7, TS_KIND_U8 = 8
};
#define TS_KIND_FIRST TS_KIND_F64
#define TS_KIND_LAST TS_KIND_U8

/* The bytes of an element of kind (a size_t), 0 for an unknown kind */
#define TS_KIND_SIZE(kind) ((size_t)( \
	(kind) == TS_KIND_F64 || (kind) == TS_KIND_I64 || (kind) == TS_KIND_U64 ? 8 : \
	(kind) == TS_KIND_F32 || (kind) == TS_KIND_I32 || (kind) == TS_KIND_U32 ? 4 : \
	(kind) == TS_KIND_U16 ? 2 : \
	(kind) == TS_KIND_U8 ? 1 : 0))

/* "fd70381": the commit of timsort.c the engine was built with (the
 * definition TS_TIMSORT_VERSION of cmake/plugins.cmake, from
 * PHARO_TIMSORT_VERSION of cmake/timsort.cmake); static storage */
TS_INTERNAL const char *ts_timsort_version(void);


/*** Sorts ***/

/* The comparison of ts_sort_words: a strict "a comes before b" of two of
 * the words sorted (the words themselves, not their addresses), with the
 * arg given to ts_sort_words.  Answers > 0 when a must come before b, 0 when
 * not (b before a, or the two are equal), < 0 to abort the sort.  It must be
 * a strict weak order for the result to be sorted and stable: a non-strict
 * one (a <= b) still sorts, but leaves equal elements in no particular
 * order.  It may run anything but a sort of the same words. */
typedef int (*ts_less_fn)(int64_t a, int64_t b, void *arg);

/* Sorts the n words at words in place by less (stable).  less is never
 * called when n < 2.  Answers TS_OK; TS_ABORTED when less answered < 0
 * (at once: no further call); TS_NO_MEMORY when the merge buffer could
 * not be allocated; TS_BAD_ARGUMENT when n < 0, words is NULL and n > 0, or
 * less is NULL and n > 1.  After TS_ABORTED or TS_NO_MEMORY the words are a
 * permutation of the input, in no useful order. */
TS_INTERNAL int ts_sort_words(int64_t *words, int64_t n, int descending, ts_less_fn less, void *arg);

/* Sorts the n elements of kind at base in place by value (the order of the
 * kinds above; stable).  The kinds of 8 bytes are sorted where they are;
 * the narrower ones are copied (their bits, unchanged) into a malloc'ed
 * buffer of n 8-byte words, sorted there and written back.  Elements are
 * moved, never converted: the bits of every element, NaN payloads and the
 * sign of zeros included, are kept.  base must be aligned to the size of
 * an element.  Answers TS_OK; TS_NO_MEMORY (a narrower kind: the elements
 * are untouched; an 8-byte kind: a permutation of them); TS_BAD_ARGUMENT for
 * an unknown kind, n < 0, base NULL with n > 0, or a misaligned base.  n < 2
 * is TS_OK at once. */
TS_INTERNAL int ts_sort_buffer(void *base, int64_t n, int kind, int descending);

/* Sorts the n words at perm in place by the keys they stand for (stable):
 * the key of a word w is the element (w >> shift) - origin of the nkeys
 * elements of kind at keys (>> arithmetic, on int64_t), compared as
 * ts_sort_buffer compares them.  The words are moved, never changed: the
 * glue passes SmallInteger oops of 1-based indices (shift 3, origin 1, keys
 * the first element of the buffer), libTimSortFFI 0-based indices (shift 0,
 * origin 0).  Every word must stand for an element: a pre-pass checks them
 * all before the first comparison, and answers TS_BAD_ARGUMENT, perm
 * untouched, when one is outside [0, nkeys).  keys must be aligned to the
 * size of an element.  Answers TS_OK; TS_NO_MEMORY (perm is a permutation of
 * its input); TS_BAD_ARGUMENT for an unknown kind, nkeys < 0, n < 0, shift
 * outside [0, 63], keys NULL with nkeys > 0, perm NULL with n > 0, or a
 * misaligned keys.  The pre-pass runs whatever n: with n < 2 it is all the
 * function does. */
TS_INTERNAL int ts_argsort_buffer(const void *keys, int64_t nkeys, int kind,
		int64_t *perm, int64_t n, int shift, int64_t origin, int descending);


/*** The order of objects: numbers and strings ***/

/* A number of the image, as the glue reads it from an object; exactly one
 * of the three representations, by type */
enum { TS_NUMBER_INTEGER = 0, TS_NUMBER_FLOAT = 1, TS_NUMBER_LARGE = 2 };
typedef struct ts_number {
	int type;		/* TS_NUMBER_* */
	int negative;		/* TS_NUMBER_LARGE: 1 for a LargeNegativeInteger */
	int64_t integer;	/* TS_NUMBER_INTEGER: a SmallInteger's value */
	double real;		/* TS_NUMBER_FLOAT: a Float's value, never a NaN */
	const uint8_t *digits;	/* TS_NUMBER_LARGE: the magnitude, little-endian bytes */
	size_t size;		/* TS_NUMBER_LARGE: the bytes of digits; high zero bytes are allowed */
} ts_number;

/* The exact order of two numbers, as Pharo's image code orders them
 * (Integer against Float exactly, as asExactFraction would; never the
 * rounding of the JIT beyond 2^53): -1, 0 or 1 for a < b, a = b, a > b.
 * -0.0 = 0.0 = 0 (and a LARGE of zero magnitude); the infinities are below
 * and above every integer.  A NaN is outside the contract (the glue
 * declines it): the answer is then unspecified, but defined. */
TS_INTERNAL int ts_number_compare(const ts_number *a, const ts_number *b);

/* A string of the image: size code units from units, bytes (ByteString,
 * ByteSymbol) or 32-bit words (WideString, WideSymbol, wide != 0) */
typedef struct ts_string {
	const void *units;	/* may be NULL when size is 0 */
	size_t size;
	int wide;
} ts_string;

/* The order of String>>#< : code point by code point (bytes and 32-bit
 * words compared as unsigned values, a byte string against a wide one
 * included), case-sensitive, a proper prefix before the longer string:
 * -1, 0 or 1. */
TS_INTERNAL int ts_string_compare(const ts_string *a, const ts_string *b);

#ifdef __cplusplus
}
#endif
#endif
