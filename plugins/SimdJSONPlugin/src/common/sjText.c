/* sjText.c -- pure C99 helpers of the SimdJSONPlugin (see sjText.h)
 *
 * Sources are Smalltalk objects read in place: a ByteArray of UTF-8, a
 * ByteString of Latin-1 characters, or a WideString of code points.  A
 * "unit" is a byte of the first two and a 32-bit word of the third, and
 * every index here is a unit index.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "sjBridge.h"
#include "sjText.h"

#define isWhite(c) ((c) == ' ' || (c) == '\t' || (c) == '\n' || (c) == '\r' || (c) == '\f')
#define isDigit(c) ((c) >= '0' && (c) <= '9')

static inline uint32_t
unitAt(const void *src, int width, size_t i)
{
	return width == 1 ? ((const uint8_t *)src)[i] : ((const uint32_t *)src)[i];
}

/* The functions below that loop over units are written once for both
 * widths and called with a constant width, so that each call site gets a
 * loop of its own. */

static inline size_t
skipWhite(const void *src, int width, size_t from, size_t stop)
{
	while (from < stop && isWhite(unitAt(src, width, from)))
		from++;
	return from;
}

size_t
sj_skip_ws(const void *src, int width, size_t from, size_t stop)
{
	return width == 1 ? skipWhite(src, 1, from, stop) : skipWhite(src, 4, from, stop);
}

static inline size_t
trimWhite(const void *src, int width, size_t from, size_t stop)
{
	while (stop > from && isWhite(unitAt(src, width, stop - 1)))
		stop--;
	return stop;
}

size_t
sj_trim_ws(const void *src, int width, size_t from, size_t stop)
{
	return width == 1 ? trimWhite(src, 1, from, stop) : trimWhite(src, 4, from, stop);
}

/* The index after the string whose opening quote is at from, or stop when
 * it is not closed.  A backslash escapes the unit after it. */
static inline size_t
stringEnd(const void *src, int width, size_t from, size_t stop)
{
	size_t i = from + 1;
	uint32_t c;

	while (i < stop) {
		c = unitAt(src, width, i);
		if (c == '"')
			return i + 1;
		i += c == '\\' ? 2 : 1;
	}
	return stop;
}

/* The end of the value that starts at from: after the bracket that closes
 * the container, after the closing quote of a string, or before the first
 * white space or structural unit that ends a scalar.  It never decides
 * whether the value is valid: simdjson parses exactly [from, end), and a
 * valid value is never cut to a shorter valid one, because a scalar ends
 * only at a delimiter and a container only at depth 0. */
static inline size_t
valueEnd(const void *src, int width, size_t from, size_t stop)
{
	uint32_t c = unitAt(src, width, from);
	size_t i, depth;

	if (c == '{' || c == '[') {
		depth = 0;
		i = from;
		while (i < stop) {
			c = unitAt(src, width, i);
			if (c == '"') {
				i = stringEnd(src, width, i, stop);
				continue;
			}
			if (c == '{' || c == '[')
				depth++;
			else if ((c == '}' || c == ']') && --depth == 0)
				return i + 1;
			i++;
		}
		return stop;
	}
	if (c == '"')
		return stringEnd(src, width, from, stop);
	for (i = from + 1; i < stop; i++) {
		c = unitAt(src, width, i);
		if (isWhite(c) || c == ',' || c == ':' || c == ']' || c == '}'
		 || c == '[' || c == '{' || c == '"')
			break;
	}
	return i;
}

size_t
sj_value_end(const void *src, int width, size_t from, size_t stop)
{
	return width == 1 ? valueEnd(src, 1, from, stop) : valueEnd(src, 4, from, stop);
}

/* Only a UTF-8 or UTF-32 source can hold U+FEFF: in a Latin-1 source the
 * bytes EF BB BF are three characters, which simdjson rejects anyway. */
int
sj_is_bom(const void *src, int encoding, size_t at, size_t stop)
{
	const uint8_t *s = src;

	switch (encoding) {
	case SJ_ENC_UTF8:
		return at + 3 <= stop && s[at] == 0xEF && s[at + 1] == 0xBB && s[at + 2] == 0xBF;
	case SJ_ENC_UTF32:
		return at < stop && ((const uint32_t *)src)[at] == 0xFEFF;
	default:
		return 0;
	}
}

/* UTF-8 bytes of a code point.  Surrogates are encoded as three bytes, and
 * anything above U+10FFFF (a WideString holds 32-bit values) as the byte
 * FF: both are invalid UTF-8, so simdjson answers UTF8_ERROR for them. */
static inline size_t
utf8Size(uint32_t cp)
{
	return cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : cp <= 0x10FFFF ? 4 : 1;
}

static inline char *
utf8Put(char *d, uint32_t cp)
{
	if (cp < 0x80)
		*d++ = (char)cp;
	else if (cp < 0x800) {
		*d++ = (char)(0xC0 | (cp >> 6));
		*d++ = (char)(0x80 | (cp & 0x3F));
	} else if (cp < 0x10000) {
		*d++ = (char)(0xE0 | (cp >> 12));
		*d++ = (char)(0x80 | ((cp >> 6) & 0x3F));
		*d++ = (char)(0x80 | (cp & 0x3F));
	} else if (cp <= 0x10FFFF) {
		*d++ = (char)(0xF0 | (cp >> 18));
		*d++ = (char)(0x80 | ((cp >> 12) & 0x3F));
		*d++ = (char)(0x80 | ((cp >> 6) & 0x3F));
		*d++ = (char)(0x80 | (cp & 0x3F));
	} else
		*d++ = (char)0xFF;
	return d;
}

/* Whether the 8 code points at w[i] are all ASCII (and all before to) */
static inline int
ascii8(const uint32_t *w, size_t i, size_t to)
{
	return i + 8 <= to
		&& (w[i] | w[i + 1] | w[i + 2] | w[i + 3] | w[i + 4] | w[i + 5] | w[i + 6] | w[i + 7]) < 0x80;
}

size_t
sj_utf8_length(const void *src, int encoding, size_t from, size_t to)
{
	const uint8_t *s = src;
	const uint32_t *w = src;
	size_t i, n = to - from;

	switch (encoding) {
	case SJ_ENC_LATIN1:	/* one more byte per character >= 0x80: their high bits, 8 at a time */
		for (i = from; i + 8 <= to; i += 8) {
			uint64_t x;

			memcpy(&x, s + i, 8);
			n += (size_t)__builtin_popcountll(x & 0x8080808080808080ULL);
		}
		for (; i < to; i++)
			n += s[i] >> 7;
		return n;
	case SJ_ENC_UTF32:
		for (n = 0, i = from; i < to; ) {
			if (ascii8(w, i, to)) {
				n += 8;
				i += 8;
			} else
				n += utf8Size(w[i++]);
		}
		return n;
	default:
		return n;
	}
}

void
sj_transcode(const void *src, int encoding, size_t from, size_t to, char *dst)
{
	const uint8_t *s = src;
	const uint32_t *w = src;
	size_t i;

	switch (encoding) {
	case SJ_ENC_LATIN1:	/* runs of 16 ASCII characters are copied */
		for (i = from; i < to; ) {
			uint64_t a, b;

			if (i + 16 <= to) {
				memcpy(&a, s + i, 8);
				memcpy(&b, s + i + 8, 8);
				if (!((a | b) & 0x8080808080808080ULL)) {
					memcpy(dst, s + i, 16);
					dst += 16;
					i += 16;
					continue;
				}
			}
			dst = utf8Put(dst, s[i++]);
		}
		break;
	case SJ_ENC_UTF32:	/* runs of 8 ASCII code points are narrowed */
		for (i = from; i < to; ) {
			if (ascii8(w, i, to)) {
				int k;

				for (k = 0; k < 8; k++)
					dst[k] = (char)w[i + k];
				dst += 8;
				i += 8;
			} else
				dst = utf8Put(dst, w[i++]);
		}
		break;
	default:
		memcpy(dst, s + from, to - from);
	}
}

/* The code point of the sequence at s[*i], which simdjson has validated;
 * *i moves past it.  A sequence cut by the end of the string gives 0. */
static inline uint32_t
utf8Next(const uint8_t *s, size_t n, size_t *i)
{
	uint32_t c = s[*i];
	size_t k = c < 0x80 ? 1 : c < 0xE0 ? 2 : c < 0xF0 ? 3 : 4;

	if (*i + k > n) {
		*i = n;
		return 0;
	}
	switch (k) {
	case 1:
		break;
	case 2:
		c = ((c & 0x1F) << 6) | (s[*i + 1] & 0x3F);
		break;
	case 3:
		c = ((c & 0x0F) << 12) | ((s[*i + 1] & 0x3F) << 6) | (s[*i + 2] & 0x3F);
		break;
	default:
		c = ((c & 0x07) << 18) | ((s[*i + 1] & 0x3F) << 12) | ((s[*i + 2] & 0x3F) << 6) | (s[*i + 3] & 0x3F);
	}
	*i += k;
	return c;
}

void
sj_utf8_scan(const char *s, size_t n, size_t *chars, uint32_t *maxcp)
{
	const uint8_t *u = (const uint8_t *)s;
	size_t i = 0, count = 0;
	uint32_t c, max = 0;

	while (i < n) {
		c = u[i] < 0x80 ? u[i++] : utf8Next(u, n, &i);
		if (c > max)
			max = c;
		count++;
	}
	*chars = count;
	*maxcp = max;
}

void
sj_utf8_to_latin1(const char *s, size_t n, uint8_t *dst)
{
	const uint8_t *u = (const uint8_t *)s;
	size_t i = 0;

	while (i < n)
		*dst++ = (uint8_t)(u[i] < 0x80 ? u[i++] : utf8Next(u, n, &i));
}

void
sj_utf8_to_utf32(const char *s, size_t n, uint32_t *dst)
{
	const uint8_t *u = (const uint8_t *)s;
	size_t i = 0;

	while (i < n)
		*dst++ = u[i] < 0x80 ? u[i++] : utf8Next(u, n, &i);
}

/* Every exponent token passes, because a JSON mantissa ends in a digit;
 * an 'e' in a string such as "5e3a" is only a false positive, which costs
 * one enumeration. */
int
sj_exponent_gate(const char *json, size_t n)
{
	static const char letters[2] = { 'e', 'E' };
	const char *p, *q, *end = json + n;
	size_t i;
	int k;

	for (k = 0; k < 2; k++)
		for (p = json; p < end && (q = memchr(p, letters[k], (size_t)(end - p))); p = q + 1) {
			i = (size_t)(q - json);
			if (i > 0 && isDigit(json[i - 1]) && i + 1 < n
			 && (isDigit(json[i + 1]) || json[i + 1] == '+' || json[i + 1] == '-'))
				return 1;
		}
	return 0;
}

/* Number tokens of a valid JSON text, outside strings: from '-' or a
 * digit, over [0-9.eE+-].  Each gets an ordinal in document order, which
 * is the order of the DOM's pre-order walk, duplicate keys included.
 *
 * The text is valid JSON, so outside strings it is ASCII, an 'e' or 'E'
 * after a digit is an exponent (true and false have it after a letter),
 * '+' only follows one, and the bytes '-' to '9' (0x2D-0x39) only occur
 * in numbers, '/' never.  Away from exponents, a token is therefore a run
 * of such bytes, and the runs are counted 16 or 8 bytes at a time, as
 * long as the bytes hold no quote and no 'e' or 'E'; the others are read
 * one by one. */
#define ONES  0x0101010101010101ULL
#define HIGHS 0x8080808080808080ULL

/* 0x80 in each byte of w that is c; w is ASCII up to its first c, and no
   carry crosses a byte below it */
static inline uint64_t
bytesEqual(uint64_t w, uint8_t c)
{
	uint64_t x = w ^ (ONES * c);

	return ~((x + ~HIGHS) | x) & HIGHS;
}

/* 0x80 in each byte of w that is in '-'..'9'; w is ASCII */
static inline uint64_t
numberBytes(uint64_t w)
{
	return (w + ONES * (0x80 - '-')) & ~(w + ONES * (0x80 - '9' - 1)) & HIGHS;
}

static inline unsigned
inNumber(uint8_t c)
{
	return (uint8_t)(c - '-') <= '9' - '-' || c == '+' || (c | 0x20) == 'e';
}

static int
addToken(sj_token **list, size_t *count, size_t *cap, uint64_t ordinal, size_t start, size_t end)
{
	size_t newCap;
	sj_token *grown;

	if (*count == *cap) {
		newCap = *cap ? 2 * *cap : 64;
		if (!(grown = realloc(*list, newCap * sizeof(sj_token))))
			return 0;
		*list = grown;
		*cap = newCap;
	}
	(*list)[*count].ordinal = (uint32_t)ordinal;
	(*list)[*count].offset = (uint32_t)start;
	(*list)[*count].length = (uint32_t)(end - start);
	*count += 1;
	return 1;
}

int64_t
sj_number_tokens(const char *json, size_t n, sj_token **list, size_t *count, size_t *cap)
{
	const uint8_t *s = (const uint8_t *)json;
	const char *q;
	size_t i = 0, start, b;
	uint64_t ordinal = 0, w, w2, x, y, x2, y2, run, run2;
	uint64_t before = 0;	/* 0x80 when s[i - 1] is in a run */
	uint8_t c;

	*count = 0;
	while (i < n) {
#if !defined(__BYTE_ORDER__) || __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__
		while (i + 16 <= n) {
			memcpy(&w, s + i, 8);
			memcpy(&w2, s + i + 8, 8);
			/* a zero byte in x, y, x2 or y2 (the test is never wrong
			   when there is one) */
			x = w ^ (ONES * '"');
			y = (w | ONES * 0x20) ^ (ONES * 'e');
			x2 = w2 ^ (ONES * '"');
			y2 = (w2 | ONES * 0x20) ^ (ONES * 'e');
			if ((((x - ONES) & ~x) | ((y - ONES) & ~y) | ((x2 - ONES) & ~x2) | ((y2 - ONES) & ~y2)) & HIGHS)
				break;
			/* the first byte of each run, then their sum (at most 16) */
			run = numberBytes(w);
			run2 = numberBytes(w2);
			ordinal += ((((run & ~(run << 8 | before)) >> 7) + ((run2 & ~(run2 << 8 | run >> 56)) >> 7)) * ONES) >> 56;
			before = run2 >> 56;
			i += 16;
		}
		if (i + 8 <= n) {
			memcpy(&w, s + i, 8);
			if (!(bytesEqual(w, '"') | bytesEqual(w | ONES * 0x20, 'e'))) {
				run = numberBytes(w);
				ordinal += (((run & ~(run << 8 | before)) >> 7) * ONES) >> 56;
				before = run >> 56;
				i += 8;
				continue;
			}
		}
#endif
		c = s[i];
		if (c == '"') {
			/* up to a quote after an even run of backslashes */
			do {
				if (!(q = memchr(json + i + 1, '"', n - i - 1)))
					return (int64_t)ordinal;	/* not valid JSON */
				i = (size_t)(q - json);
				for (b = 0; s[i - 1 - b] == '\\'; b++)
					;
			} while (b & 1);
			i++;
			before = 0;
		} else if ((c | 0x20) == 'e' && i > 0 && isDigit(s[i - 1])) {
			/* an exponent: its token was counted at its first byte */
			for (start = i; start > 0 && inNumber(s[start - 1]); start--)
				;
			for (i++; i < n && inNumber(s[i]); i++)
				;
			if (!addToken(list, count, cap, ordinal - 1, start, i))
				return -1;
			before = 0x80;
		} else {
			run = (uint8_t)(c - '-') <= '9' - '-' ? 0x80 : 0;
			ordinal += run && !before;
			before = run;
			i++;
		}
	}
	return (int64_t)ordinal;
}

/* Spur64BitMemoryManager>>isSmallFloatValue: */
int
sj_is_small_float(double d)
{
	uint64_t raw, exponent;

	memcpy(&raw, &d, sizeof(raw));
	exponent = (raw >> 52) & 0x7FF;
	return exponent > 896
		? exponent <= 0xFF + 896
		: (raw & ((1ULL << 52) - 1)) == 0 ? exponent == 0 : exponent == 896;
}

uint64_t
sj_spur_pointers(uint64_t slots)
{
	return 8 + (slots >= 255 ? 8 : 0) + 8 * (slots ? slots : 1);
}

uint64_t
sj_spur_bytes(uint64_t nbytes)
{
	return sj_spur_pointers((nbytes + 7) / 8);
}

uint64_t
sj_spur_words32(uint64_t nwords)
{
	return sj_spur_pointers((nwords + 1) / 2);
}

size_t
sj_dict_capacity(size_t npairs)
{
	size_t n = (npairs + 1) * 4 / 3, d;

	if (n <= 5)
		return 5;
	for (n |= 1;; n += 2) {
		for (d = 3; d * d <= n && n % d; d += 2)
			;
		if (d * d > n)
			return n;
	}
}

uint64_t
sj_bound(uint64_t utf8Bytes)
{
	return 32 * utf8Bytes + 4096;
}

/* The key set: keys are copied into one byte area, entries are numbered
 * from 1 in first-seen order, and the table holds entry numbers (0 for an
 * empty slot), probed linearly; it doubles when half full. */
typedef struct {
	uint64_t hash;
	size_t offset, length;
} sj_key;

struct sj_keyset {
	uint32_t *table;
	size_t tableSize, count;
	sj_key *keys;
	size_t keysCap;
	char *bytes;
	size_t bytesUsed, bytesCap;
};

static uint64_t
keyHash(const char *s, size_t n)
{
	uint64_t h = 0xCBF29CE484222325ULL;	/* FNV-1a */
	size_t i;

	for (i = 0; i < n; i++)
		h = (h ^ (uint8_t)s[i]) * 0x100000001B3ULL;
	return h;
}

sj_keyset *
sj_keyset_new(size_t hint)
{
	sj_keyset *k = calloc(1, sizeof(sj_keyset));
	size_t size = 16;

	if (!k)
		return NULL;
	while (size < 2 * hint)
		size *= 2;
	if (!(k->table = calloc(size, sizeof(uint32_t)))) {
		free(k);
		return NULL;
	}
	k->tableSize = size;
	return k;
}

void
sj_keyset_free(sj_keyset *k)
{
	if (!k)
		return;
	free(k->table);
	free(k->keys);
	free(k->bytes);
	free(k);
}

static int
keysetGrow(sj_keyset *k)
{
	size_t size = 2 * k->tableSize, i, j;
	uint32_t *table = calloc(size, sizeof(uint32_t));

	if (!table)
		return 0;
	for (i = 0; i < k->count; i++) {
		for (j = k->keys[i].hash & (size - 1); table[j]; j = (j + 1) & (size - 1))
			;
		table[j] = (uint32_t)(i + 1);
	}
	free(k->table);
	k->table = table;
	k->tableSize = size;
	return 1;
}

int64_t
sj_keyset_index(sj_keyset *k, const char *utf8, size_t n, int *isNew)
{
	uint64_t h = keyHash(utf8, n);
	size_t i, cap;
	sj_key *key;
	void *grown;

	for (i = h & (k->tableSize - 1); k->table[i]; i = (i + 1) & (k->tableSize - 1)) {
		key = &k->keys[k->table[i] - 1];
		if (key->hash == h && key->length == n && (!n || !memcmp(k->bytes + key->offset, utf8, n))) {
			*isNew = 0;
			return k->table[i];
		}
	}
	if (k->count == k->keysCap) {
		cap = k->keysCap ? 2 * k->keysCap : 16;
		if (!(grown = realloc(k->keys, cap * sizeof(sj_key))))
			return -1;
		k->keys = grown;
		k->keysCap = cap;
	}
	if (k->bytesUsed + n > k->bytesCap) {
		for (cap = k->bytesCap ? 2 * k->bytesCap : 256; cap < k->bytesUsed + n; cap *= 2)
			;
		if (!(grown = realloc(k->bytes, cap)))
			return -1;
		k->bytes = grown;
		k->bytesCap = cap;
	}
	if (2 * (k->count + 1) > k->tableSize) {
		if (!keysetGrow(k))
			return -1;
		for (i = h & (k->tableSize - 1); k->table[i]; i = (i + 1) & (k->tableSize - 1))
			;
	}
	if (n)
		memcpy(k->bytes + k->bytesUsed, utf8, n);
	key = &k->keys[k->count];
	key->hash = h;
	key->offset = k->bytesUsed;
	key->length = n;
	k->bytesUsed += n;
	k->table[i] = (uint32_t)++k->count;
	*isNew = 1;
	return (int64_t)k->count;
}


/*** Writing ***/

void
sj_out_free(sj_out *o)
{
	free(o->p);
	o->p = NULL;
	o->n = o->cap = 0;
}

int
sj_out_reserve(sj_out *o, size_t more)
{
	size_t cap;
	char *grown;

	if (more <= o->cap - o->n)
		return 0;
	if (more > SIZE_MAX / 2 - o->n)
		return -1;
	cap = o->cap ? o->cap : 4096;
	while (cap - o->n < more)
		cap *= 2;
	if (!(grown = realloc(o->p, cap)))
		return -1;
	o->p = grown;
	o->cap = cap;
	return 0;
}

int
sj_out_bytes(sj_out *o, const char *s, size_t n)
{
	if (sj_out_reserve(o, n))
		return -1;
	memcpy(o->p + o->n, s, n);
	o->n += n;
	return 0;
}

int
sj_out_char(sj_out *o, char c)
{
	if (o->n == o->cap && sj_out_reserve(o, 1))
		return -1;
	o->p[o->n++] = c;
	return 0;
}

static int
outUInt64(sj_out *o, uint64_t v, int negative)
{
	char digits[21];
	int i = 21;

	do {
		digits[--i] = (char)('0' + v % 10);
		v /= 10;
	} while (v);
	if (negative)
		digits[--i] = '-';
	return sj_out_bytes(o, digits + i, (size_t)(21 - i));
}

int
sj_out_int64(sj_out *o, int64_t v)
{
	return v < 0 ? outUInt64(o, 0 - (uint64_t)v, 1) : outUInt64(o, (uint64_t)v, 0);
}

/* The decimal digits of a magnitude of any size (LargePositiveInteger and
 * LargeNegativeInteger hold it in little-endian bytes): 32-bit limbs,
 * divided by 10^9 until nothing is left, quadratic but only on numbers of
 * more than 64 bits. */
int
sj_out_magnitude(sj_out *o, const uint8_t *le, size_t n, int negative)
{
	uint32_t *limbs;
	uint32_t *chunks;
	size_t nlimbs, nchunks = 0, i;
	uint64_t small = 0;
	char nine[10];
	int k, rc = 0;

	while (n > 0 && le[n - 1] == 0)
		n--;
	if (n <= 8) {
		for (i = n; i > 0; i--)
			small = (small << 8) | le[i - 1];
		return outUInt64(o, small, negative && small != 0);
	}
	nlimbs = (n + 3) / 4;
	limbs = calloc(nlimbs, sizeof *limbs);
	chunks = malloc((nlimbs * 10 / 9 + 2) * sizeof *chunks);	/* 2^32 < 10^9 * 4.3 */
	if (!limbs || !chunks) {
		free(limbs);
		free(chunks);
		return -1;
	}
	for (i = 0; i < n; i++)
		limbs[i / 4] |= (uint32_t)le[i] << (8 * (i % 4));
	while (nlimbs > 0) {
		uint64_t rem = 0;

		for (i = nlimbs; i > 0; i--) {
			uint64_t cur = (rem << 32) | limbs[i - 1];

			limbs[i - 1] = (uint32_t)(cur / 1000000000u);
			rem = cur % 1000000000u;
		}
		chunks[nchunks++] = (uint32_t)rem;
		while (nlimbs > 0 && limbs[nlimbs - 1] == 0)
			nlimbs--;
	}
	if (negative)
		rc = sj_out_char(o, '-');
	if (!rc)
		rc = outUInt64(o, chunks[nchunks - 1], 0);
	for (i = nchunks - 1; !rc && i > 0; i--) {
		for (k = 8; k >= 0; k--) {
			nine[k] = (char)('0' + chunks[i - 1] % 10);
			chunks[i - 1] /= 10;
		}
		rc = sj_out_bytes(o, nine, 9);
	}
	free(limbs);
	free(chunks);
	return rc;
}

/* NeoJSONWriter>>escapeChar: and escapeUnicode4:, with its uppercase hex */
static int
outEscape(sj_out *o, uint32_t c)
{
	static const char hex[] = "0123456789ABCDEF";
	char u[6];

	switch (c) {
	case '"': return sj_out_bytes(o, "\\\"", 2);
	case '\\': return sj_out_bytes(o, "\\\\", 2);
	case 8: return sj_out_bytes(o, "\\b", 2);
	case 12: return sj_out_bytes(o, "\\f", 2);
	case 10: return sj_out_bytes(o, "\\n", 2);
	case 13: return sj_out_bytes(o, "\\r", 2);
	case 9: return sj_out_bytes(o, "\\t", 2);
	}
	u[0] = '\\';
	u[1] = 'u';
	u[2] = hex[(c >> 12) & 15];
	u[3] = hex[(c >> 8) & 15];
	u[4] = hex[(c >> 4) & 15];
	u[5] = hex[c & 15];
	return sj_out_bytes(o, u, 6);
}

static int
outUtf8(sj_out *o, uint32_t c)
{
	char b[4];

	if (c < 0x80)
		return sj_out_char(o, (char)c);
	if (c < 0x800) {
		b[0] = (char)(0xC0 | (c >> 6));
		b[1] = (char)(0x80 | (c & 0x3F));
		return sj_out_bytes(o, b, 2);
	}
	if (c < 0x10000) {	/* surrogates too: NeoJSON writes the character as it is */
		b[0] = (char)(0xE0 | (c >> 12));
		b[1] = (char)(0x80 | ((c >> 6) & 0x3F));
		b[2] = (char)(0x80 | (c & 0x3F));
		return sj_out_bytes(o, b, 3);
	}
	b[0] = (char)(0xF0 | (c >> 18));
	b[1] = (char)(0x80 | ((c >> 12) & 0x3F));
	b[2] = (char)(0x80 | ((c >> 6) & 0x3F));
	b[3] = (char)(0x80 | (c & 0x3F));
	return sj_out_bytes(o, b, 4);
}

/* NeoJSONWriter>>doesCodePointNeedEscaping: is c < 32, " and \, and with
 * asciiOnly c > 126; the other characters go out as UTF-8.  Runs of
 * characters that need nothing are copied at once. */
#define plainByte(c, asciiOnly) ((c) >= 32 && (c) != '"' && (c) != '\\' && (c) < ((asciiOnly) ? 127 : 128))

int
sj_out_string(sj_out *o, const void *src, int encoding, size_t n, int asciiOnly)
{
	size_t i = 0, run;
	uint32_t c;

	if (sj_out_reserve(o, n + 2) || sj_out_char(o, '"'))
		return -1;
	while (i < n) {
		if (encoding == SJ_ENC_LATIN1) {
			const uint8_t *s = (const uint8_t *)src;

			for (run = i; run < n && plainByte(s[run], asciiOnly); run++) {
			}
			if (run > i && sj_out_bytes(o, (const char *)s + i, run - i))
				return -1;
			if ((i = run) == n)
				break;
			c = s[i++];
		} else {
			c = ((const uint32_t *)src)[i++];
		}
		if (c < 32 || c == '"' || c == '\\' || (asciiOnly && c > 126 && c <= 0xFFFF)) {
			if (outEscape(o, c))
				return -1;
		} else if (c > 0x10FFFF) {
			return -2;
		} else if (asciiOnly && c > 0xFFFF) {
			c -= 0x10000;
			if (outEscape(o, 0xD800 + (c >> 10)) || outEscape(o, 0xDC00 + (c & 0x3FF)))
				return -1;
		} else if (outUtf8(o, c)) {
			return -1;
		}
	}
	return sj_out_char(o, '"');
}

/* Whether |v| is exactly odd x 10^t, odd being 10 D + 5: the midpoint of
 * the digits D and D + 1.  With |v| = m 2^q, m odd, and odd x 5^t x 2^t
 * (t >= 0) or odd / 5^-t / 2^-t (t < 0), the powers of 2 and the odd parts
 * must match; integers of 64 bits suffice, as m < 2^53 and odd < 10^18. */
static int
isMidpoint(double v, uint64_t odd, int t)
{
	uint64_t bits, m, x;
	int q, i;

	memcpy(&bits, &v, sizeof bits);
	m = bits & 0xFFFFFFFFFFFFFull;
	q = (int)((bits >> 52) & 0x7FF);
	if (q) {
		m |= 1ull << 52;
		q -= 1075;
	} else {
		q = -1074;
	}
	if (!m)
		return 0;
	while (!(m & 1)) {
		m >>= 1;
		q++;
	}
	if (q != t)
		return 0;
	x = t >= 0 ? odd : m;
	for (i = 0; i < (t >= 0 ? t : -t); i++) {
		if (x > UINT64_MAX / 5)
			return 0;
		x *= 5;
	}
	return t >= 0 ? x == m : x == odd;
}

/* Float>>absPrintExactlyOn:base: lays the shortest digits d1..dn of a
 * value 0.d1..dn x 10^k out in fixed notation when -3 <= k <= 6 ("0.001",
 * "123.0", "1000000.0" is "1.0e6"), else as "d1.d2..dn" (".0" when n is 1),
 * "e" and k - 1.  simdjson's text (Dragonbox) gives the same shortest
 * digits, in printf's %g layout ("1e+15", "1.5e-05", "0.0001", "100.0"),
 * but for a tie between two shortest digit strings: Dragonbox takes the
 * even one, Pharo the upper one (r * 2 >= s). */
size_t
sj_float_to_pharo(double v, const char *s, size_t n, char *dst)
{
	char digits[32];
	size_t i = 0, k = 0, nd = 0, intDigits = 0, zerosAfterPoint = 0;
	int exp10, sawPoint = 0, sawDigit = 0, e = 0, eNegative = 0;

	if (i < n && s[i] == '-')
		dst[k++] = s[i++];
	for (; i < n && s[i] != 'e' && s[i] != 'E'; i++) {
		if (s[i] == '.') {
			sawPoint = 1;
		} else if (s[i] == '0' && !sawDigit) {
			if (sawPoint)
				zerosAfterPoint++;
		} else if (nd < sizeof digits) {
			sawDigit = 1;
			digits[nd++] = s[i];
			if (!sawPoint)
				intDigits++;
		}
	}
	if (i < n) {
		for (i++; i < n && (s[i] == '+' || s[i] == '-'); i++)
			eNegative = s[i] == '-';
		for (; i < n; i++)
			e = 10 * e + (s[i] - '0');
	}
	while (nd > 1 && digits[nd - 1] == '0') {	/* zeros of the layout, as in "100.0" */
		nd--;
	}
	if (nd == 0 || (nd == 1 && digits[0] == '0')) {	/* zero, which Float>>printOn: spells itself */
		memcpy(dst + k, "0.0", 3);
		return k + 3;
	}
	exp10 = (eNegative ? -e : e) + (intDigits ? (int)intDigits : -(int)zerosAfterPoint);
	if (nd <= 17 && (digits[nd - 1] - '0') % 2 == 0) {
		uint64_t d = 0;

		for (i = 0; i < nd; i++)
			d = 10 * d + (uint64_t)(digits[i] - '0');
		if (isMidpoint(v, 10 * d + 5, exp10 - (int)nd - 1))
			digits[nd - 1]++;
	}
	if (exp10 >= -3 && exp10 <= 6) {
		if (exp10 <= 0) {
			dst[k++] = '0';
			dst[k++] = '.';
			for (i = 0; i < (size_t)-exp10; i++)
				dst[k++] = '0';
			memcpy(dst + k, digits, nd);
			return k + nd;
		}
		for (i = 0; i < nd || i < (size_t)exp10; i++) {
			if (i == (size_t)exp10)
				dst[k++] = '.';
			dst[k++] = i < nd ? digits[i] : '0';
		}
		if (nd <= (size_t)exp10) {
			dst[k++] = '.';
			dst[k++] = '0';
		}
		return k;
	}
	dst[k++] = digits[0];
	dst[k++] = '.';
	if (nd == 1) {
		dst[k++] = '0';
	} else {
		memcpy(dst + k, digits + 1, nd - 1);
		k += nd - 1;
	}
	return k + (size_t)sprintf(dst + k, "e%d", exp10 - 1);
}

size_t
sj_utf8_chars(const char *s, size_t n)
{
	size_t i, count = 0;

	for (i = 0; i < n; i++)
		count += ((uint8_t)s[i] & 0xC0) != 0x80;
	return count;
}
