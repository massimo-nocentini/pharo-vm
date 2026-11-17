/* tsEngine.c -- the engine of the TimSortPlugin and libTimSortFFI
 * (tsEngine.h): the only file that includes timsort.h, and it includes no VM
 * header.
 *
 * Every sort is one call of timsort.c's list_sort_impl (renamed
 * pharo_timsort_list_sort_impl by cmake/timsort.cmake), which sorts 8-byte
 * words and calls a comparison with the two words (not their addresses).
 * Its answer is 0, or 1 for any failure; the comparisons of
 * ts_sort_words record whether the caller aborted, so that a failure is
 * told apart: TS_ABORTED, else TS_NO_MEMORY (merge_getmem's malloc, the
 * only other failure).  The typed comparisons never abort.
 *
 * The buffers: the kinds of 8 bytes are sorted where they are, with a
 * comparison of their kind (signed, unsigned, binary64 with NaN last).  The
 * narrower kinds are widened into a malloc'ed buffer of 8-byte words that
 * keeps the bits of each element in its low half, so that writing back is
 * a truncation: the integers as their value (sign-extended for i32, so
 * that one signed comparison serves u8, u16, u32 and i32 alike), a
 * binary32 with a 32-bit key in its high half that orders as the kind's
 * order (every NaN the largest key, -0.0 the key of 0.0), compared as an
 * unsigned integer.  The argsort reads its keys where they are, through
 * the index each word stands for.
 *
 * The order of objects (ts_number_compare, ts_string_compare) is exact and
 * allocates nothing: an integer against a double by truncation and the
 * sign of the remainder, a LargeInteger against a double by bit length,
 * then the top 53 bits, then whether any lower bit is set.
 */

#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "timsort.h"
#include "tsEngine.h"

#ifndef TS_TIMSORT_VERSION
# define TS_TIMSORT_VERSION "unknown"
#endif

_Static_assert(sizeof(timsort_object_t) == sizeof(int64_t), "tsEngine: timsort.c must sort 8-byte words");
_Static_assert(sizeof(double) == 8 && sizeof(float) == 4, "tsEngine: IEEE binary64 and binary32");

const char *
ts_timsort_version(void)
{
	return TS_TIMSORT_VERSION;
}

/* The word a comparison of list_sort_impl gets */
#define WORD(p) ((int64_t)(intptr_t)(p))

/* list_sort_impl over the n words at words: CPython's defaults (binary
 * insertion, the plain branch); 0 or 1 as list_sort_impl answers */
static int
timsort(int64_t *words, int64_t n, int descending, threeways_comparefunc_t compare, void *arg)
{
	timsort_list_t list;

	list.ob_size = (timsort_ssize_t)n;
	list.ob_item = (timsort_object_t **)words;
	return list_sort_impl(&list, descending != 0, 0, 1, compare, arg);
}


/*** Words, with the caller's comparison ***/

/* The comparison of list_sort_impl over the caller's: the words, and
 * whether the caller aborted (list_sort_impl answers 1 for any failure) */
typedef struct {
	ts_less_fn less;
	void *arg;
	int aborted;
} Words;

static int
wordsLess(timsort_object_t *a, timsort_object_t *b, void *arg)
{
	Words *w = arg;
	int answer = w->less(WORD(a), WORD(b), w->arg);

	if (answer < 0) {
		w->aborted = 1;
		return -1;
	}
	return answer > 0;
}

int
ts_sort_words(int64_t *words, int64_t n, int descending, ts_less_fn less, void *arg)
{
	Words w;

	if (n < 0 || (n > 0 && !words) || (n > 1 && !less)) {
		return TS_BAD_ARGUMENT;
	}
	if (n < 2) {
		return TS_OK;
	}
	w.less = less;
	w.arg = arg;
	w.aborted = 0;
	if (timsort(words, n, descending, wordsLess, &w) == 0) {
		return TS_OK;
	}
	return w.aborted ? TS_ABORTED : TS_NO_MEMORY;
}


/*** Typed comparisons ***/

static double
doubleOf(int64_t word)
{
	double d;

	memcpy(&d, &word, sizeof d);
	return d;
}

/* x before y in the order of the float kinds: by value, every NaN after
 * every number, NaNs equal, -0.0 = 0.0 */
static inline int
realLess(double x, double y)
{
	return x < y || (y != y && x == x);
}

static int
lessI64(timsort_object_t *a, timsort_object_t *b, void *arg)
{
	(void)arg;
	return WORD(a) < WORD(b);
}

static int
lessU64(timsort_object_t *a, timsort_object_t *b, void *arg)
{
	(void)arg;
	return (uint64_t)WORD(a) < (uint64_t)WORD(b);
}

static int
lessF64(timsort_object_t *a, timsort_object_t *b, void *arg)
{
	(void)arg;
	return realLess(doubleOf(WORD(a)), doubleOf(WORD(b)));
}

/* A widened binary32: its key in the high half */
static int
lessHigh32(timsort_object_t *a, timsort_object_t *b, void *arg)
{
	(void)arg;
	return ((uint64_t)WORD(a) >> 32) < ((uint64_t)WORD(b) >> 32);
}

/* The key of a binary32 of the given bits, as an unsigned integer that
 * orders as the float kinds order: the negative numbers below 0x80000000
 * (complemented, so that a larger magnitude is a smaller key), zero (either
 * sign) at 0x80000000, the positive numbers above it, +inf at 0xFF800000,
 * every NaN at 0xFFFFFFFF. */
static uint32_t
keyF32(uint32_t bits)
{
	if ((bits & 0x7FFFFFFFu) > 0x7F800000u) {
		return 0xFFFFFFFFu;
	}
	if (bits == 0x80000000u) {
		bits = 0;
	}
	return bits & 0x80000000u ? ~bits : bits | 0x80000000u;
}

static int
isKind(int kind)
{
	return kind >= TS_KIND_FIRST && kind <= TS_KIND_LAST;
}


/*** Buffers ***/

/* The word of the element of kind at p, widened as the file's comment says */
static int64_t
widen(const void *p, int kind)
{
	uint32_t u32;
	int32_t i32;
	uint16_t u16;

	switch (kind) {
	case TS_KIND_F32:
		memcpy(&u32, p, 4);
		return (int64_t)(((uint64_t)keyF32(u32) << 32) | u32);
	case TS_KIND_I32:
		memcpy(&i32, p, 4);
		return i32;
	case TS_KIND_U32:
		memcpy(&u32, p, 4);
		return u32;
	case TS_KIND_U16:
		memcpy(&u16, p, 2);
		return u16;
	default:	/* TS_KIND_U8 */
		return *(const uint8_t *)p;
	}
}

/* Writes the low bits of word back as the element of kind at p */
static void
narrow(void *p, int kind, int64_t word)
{
	uint32_t u32 = (uint32_t)(uint64_t)word;
	uint16_t u16 = (uint16_t)(uint64_t)word;

	switch (kind) {
	case TS_KIND_F32: case TS_KIND_I32: case TS_KIND_U32:
		memcpy(p, &u32, 4);
		break;
	case TS_KIND_U16:
		memcpy(p, &u16, 2);
		break;
	default:
		*(uint8_t *)p = (uint8_t)word;
	}
}

int
ts_sort_buffer(void *base, int64_t n, int kind, int descending)
{
	size_t size = TS_KIND_SIZE(kind), i;
	int64_t *words;
	char *p = base;
	int rc;

	if (!isKind(kind) || n < 0 || (n > 0 && !base) || (base && (uintptr_t)base % size != 0)) {
		return TS_BAD_ARGUMENT;
	}
	if (n < 2) {
		return TS_OK;
	}
	switch (kind) {
	case TS_KIND_F64:
		return timsort(base, n, descending, lessF64, NULL) ? TS_NO_MEMORY : TS_OK;
	case TS_KIND_I64:
		return timsort(base, n, descending, lessI64, NULL) ? TS_NO_MEMORY : TS_OK;
	case TS_KIND_U64:
		return timsort(base, n, descending, lessU64, NULL) ? TS_NO_MEMORY : TS_OK;
	}
	if ((uint64_t)n > SIZE_MAX / sizeof(int64_t) || !(words = malloc((size_t)n * sizeof(int64_t)))) {
		return TS_NO_MEMORY;
	}
	for (i = 0; i < (size_t)n; i++) {
		words[i] = widen(p + i * size, kind);
	}
	rc = timsort(words, n, descending, kind == TS_KIND_F32 ? lessHigh32 : lessI64, NULL);
	if (rc == 0) {
		for (i = 0; i < (size_t)n; i++) {
			narrow(p + i * size, kind, words[i]);
		}
	}
	free(words);
	return rc ? TS_NO_MEMORY : TS_OK;
}


/*** Argsort ***/

typedef struct {
	const void *keys;
	int shift;
	int64_t origin;
} Keys;

/* The index of the key the word w stands for (checked by the pre-pass) */
#define INDEX(k, w) ((size_t)(((w) >> (k)->shift) - (k)->origin))

#define KEY_LESS(name, type, less) \
	static int \
	name(timsort_object_t *a, timsort_object_t *b, void *arg) \
	{ \
		const Keys *k = arg; \
		const type *keys = k->keys; \
		type x = keys[INDEX(k, WORD(a))], y = keys[INDEX(k, WORD(b))]; \
		return less; \
	}

KEY_LESS(keyLessF64, double, realLess(x, y))
KEY_LESS(keyLessF32, float, realLess(x, y))
KEY_LESS(keyLessI64, int64_t, x < y)
KEY_LESS(keyLessU64, uint64_t, x < y)
KEY_LESS(keyLessI32, int32_t, x < y)
KEY_LESS(keyLessU32, uint32_t, x < y)
KEY_LESS(keyLessU16, uint16_t, x < y)
KEY_LESS(keyLessU8, uint8_t, x < y)

int
ts_argsort_buffer(const void *keys, int64_t nkeys, int kind,
		int64_t *perm, int64_t n, int shift, int64_t origin, int descending)
{
	static const threeways_comparefunc_t lesses[] = { NULL,
		keyLessF64, keyLessF32, keyLessI64, keyLessU64, keyLessI32, keyLessU32, keyLessU16, keyLessU8 };
	size_t size = TS_KIND_SIZE(kind);
	Keys k;
	int64_t i;

	if (!isKind(kind) || nkeys < 0 || n < 0 || shift < 0 || shift > 63
	 || (nkeys > 0 && !keys) || (n > 0 && !perm) || (keys && (uintptr_t)keys % size != 0)) {
		return TS_BAD_ARGUMENT;
	}
	/* every word stands for a key, before the first comparison */
	for (i = 0; i < n; i++) {
		int64_t v = perm[i] >> shift;
		if (v < origin || (uint64_t)v - (uint64_t)origin >= (uint64_t)nkeys) {
			return TS_BAD_ARGUMENT;
		}
	}
	if (n < 2) {
		return TS_OK;
	}
	k.keys = keys;
	k.shift = shift;
	k.origin = origin;
	return timsort(perm, n, descending, lesses[kind], &k) ? TS_NO_MEMORY : TS_OK;
}


/*** Numbers ***/

/* -1, 0 or 1 as x < y, x = y, x > y */
#define ORDER(x, y) (((x) > (y)) - ((x) < (y)))

/* The bytes of a magnitude without its high zero bytes */
static size_t
trimmed(const uint8_t *digits, size_t size)
{
	while (size > 0 && digits[size - 1] == 0) {
		size--;
	}
	return size;
}

/* An integer as a sign and a magnitude: an INTEGER's bytes go to buffer */
typedef struct {
	int sign;		/* -1, 0, 1 */
	const uint8_t *digits;
	size_t size;		/* trimmed */
} Magnitude;

static void
magnitudeOf(const ts_number *x, uint8_t buffer[8], Magnitude *m)
{
	if (x->type == TS_NUMBER_INTEGER) {
		uint64_t u = x->integer < 0 ? 0 - (uint64_t)x->integer : (uint64_t)x->integer;
		int i;
		for (i = 0; i < 8; i++) {
			buffer[i] = (uint8_t)(u >> (8 * i));
		}
		m->digits = buffer;
		m->size = trimmed(buffer, 8);
		m->sign = x->integer < 0 ? -1 : 1;
	} else {
		m->digits = x->digits;
		m->size = x->digits ? trimmed(x->digits, x->size) : 0;
		m->sign = x->negative ? -1 : 1;
	}
	if (m->size == 0) {
		m->sign = 0;
	}
}

/* Two integers, either of them a LARGE */
static int
compareIntegers(const ts_number *a, const ts_number *b)
{
	uint8_t ba[8], bb[8];
	Magnitude x, y;
	size_t i;
	int order = 0;

	magnitudeOf(a, ba, &x);
	magnitudeOf(b, bb, &y);
	if (x.sign != y.sign) {
		return ORDER(x.sign, y.sign);
	}
	if (x.size != y.size) {
		order = ORDER(x.size, y.size);
	} else {
		for (i = x.size; i-- > 0;) {
			if (x.digits[i] != y.digits[i]) {
				order = ORDER(x.digits[i], y.digits[i]);
				break;
			}
		}
	}
	return x.sign < 0 ? -order : order;
}

/* An integer of int64_t against a double (not a NaN), exactly */
static int
compareIntegerReal(int64_t i, double d)
{
	int64_t t;
	double fraction;

	if (d >= 9223372036854775808.0) {	/* 2^63, +inf */
		return -1;
	}
	if (d < -9223372036854775808.0) {	/* below -2^63, -inf */
		return 1;
	}
	t = (int64_t)d;			/* toward zero, exact */
	if (i != t) {
		return ORDER(i, t);
	}
	fraction = d - (double)t;	/* exact: |d| < 2^52 when d has a fraction */
	return fraction > 0 ? -1 : fraction < 0 ? 1 : 0;
}

/* The magnitude m (sign 1, size > 0) against a positive finite double x,
 * from the bits of x (no libm): x = mantissa * 2^(e - 53), where mantissa
 * has 53 bits and the integer part of x has e bits */
static int
compareMagnitudeReal(const Magnitude *m, double x)
{
	int e, length, k;
	size_t shift, i;
	uint8_t top = m->digits[m->size - 1];
	uint64_t raw, mantissa, bits = 0;

	memcpy(&raw, &x, sizeof raw);
	if ((raw >> 52) == 0) {
		return 1;		/* a subnormal: x < 1 <= m */
	}
	e = (int)(raw >> 52) - 1022;
	mantissa = (raw & 0xFFFFFFFFFFFFFull) | ((uint64_t)1 << 52);
	if (e <= 0) {
		return 1;		/* x < 1 <= m */
	}
	length = (int)(m->size - 1) * 8;
	while (top) {
		length++;
		top >>= 1;
	}
	if (length != e) {
		return ORDER(length, e);
	}
	if (e <= 53) {			/* m < 2^53: exact as a double */
		uint64_t v = 0;
		for (i = m->size; i-- > 0;) {
			v = (v << 8) | m->digits[i];
		}
		return ORDER((double)v, x);
	}
	/* x is the integer mantissa * 2^(e - 53): the 53 bits of m from
	   e - 53 up, then whether a lower bit is set */
	shift = (size_t)(e - 53);
	for (k = 0; k < 53; k++) {
		size_t bit = shift + (size_t)k;
		if ((m->digits[bit / 8] >> (bit % 8)) & 1) {
			bits |= (uint64_t)1 << k;
		}
	}
	if (bits != mantissa) {
		return ORDER(bits, mantissa);
	}
	for (i = 0; i < shift / 8; i++) {
		if (m->digits[i]) {
			return 1;
		}
	}
	return (m->digits[shift / 8] & ((1u << (shift % 8)) - 1)) ? 1 : 0;
}

/* A LARGE against a double (not a NaN), exactly */
static int
compareLargeReal(const ts_number *a, double d)
{
	uint8_t buffer[8];
	Magnitude m;
	int sign, order;

	magnitudeOf(a, buffer, &m);
	if (d - d != 0) {
		return d > 0 ? -1 : 1;	/* an infinity */
	}
	sign = ORDER(d, 0.0);
	if (m.sign != sign) {
		return ORDER(m.sign, sign);
	}
	if (sign == 0) {
		return 0;
	}
	order = compareMagnitudeReal(&m, sign > 0 ? d : -d);
	return sign > 0 ? order : -order;
}

int
ts_number_compare(const ts_number *a, const ts_number *b)
{
	if (a->type == TS_NUMBER_FLOAT) {
		if (b->type == TS_NUMBER_FLOAT) {
			return ORDER(a->real, b->real);
		}
		return b->type == TS_NUMBER_INTEGER ? -compareIntegerReal(b->integer, a->real)
			: -compareLargeReal(b, a->real);
	}
	if (b->type == TS_NUMBER_FLOAT) {
		return a->type == TS_NUMBER_INTEGER ? compareIntegerReal(a->integer, b->real)
			: compareLargeReal(a, b->real);
	}
	if (a->type == TS_NUMBER_INTEGER && b->type == TS_NUMBER_INTEGER) {
		return ORDER(a->integer, b->integer);
	}
	return compareIntegers(a, b);
}


/*** Strings ***/

static uint32_t
unitAt(const ts_string *s, size_t i)
{
	return s->wide ? ((const uint32_t *)s->units)[i] : ((const uint8_t *)s->units)[i];
}

int
ts_string_compare(const ts_string *a, const ts_string *b)
{
	size_t n = a->size < b->size ? a->size : b->size, i;

	if (!a->wide && !b->wide) {
		int order = n ? memcmp(a->units, b->units, n) : 0;
		if (order) {
			return order < 0 ? -1 : 1;
		}
	} else {
		for (i = 0; i < n; i++) {
			uint32_t x = unitAt(a, i), y = unitAt(b, i);
			if (x != y) {
				return x < y ? -1 : 1;
			}
		}
	}
	return ORDER(a->size, b->size);
}
