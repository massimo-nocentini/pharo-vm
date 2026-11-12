/* sjFFI.c -- libSimdJSONFFI: the engine of the SimdJSONPlugin for the FFI
 * of the image (sjFFI.h has the contract and the layouts of the tapes):
 * reading, here, and writing, at the end of the file.
 *
 * The engine (sjEngine.cpp) walks the parsed value in pre-order and calls a
 * builder for each value: a container before its elements, a key before its
 * value.  The builder here appends each value to the tape as it is made, so
 * the tape is in pre-order too.  null, true and false are not made: the
 * engine answers the builder's nilOop, trueOop and falseOop, sentinels that
 * array_put and object_put (and sj_ffi_read, for a value that is only one of
 * them) append when they store them, which is where they belong in the walk.
 *
 * This file is not in src/common, which the plugin's CMake globs: the
 * plugin does not export these functions.  The library is built with hidden
 * visibility: only the SJ_FFI_EXPORT functions are seen, so that the engine
 * of a SimdJSONPlugin loaded in the same process never gets in between.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "sjBridge.h"
#include "sjText.h"
#include "sjFFI.h"

#ifndef SJ_SIMDJSON_VERSION
# define SJ_SIMDJSON_VERSION "5.0.5"	/* the release cmake/simdjson.cmake pins */
#endif

enum { NIL = 1, TRUE_ = 2, FALSE_ = 3, MADE = 4 };	/* the oops of the builder */

#define SMALL_LIMIT ((int64_t)1 << 52)	/* SMALLINT payloads: a header stays a SmallInteger */
#define KEEP_LIMIT (1024 * 1024)		/* a tape buffer above it is freed after each read */

static sj_out tape;
static sj_out words;		/* the code points of the WideStrings, in tape order */
static int failed;		/* C memory ran out while building */
static int64_t ticket;		/* of what is kept for sj_ffi_take */
static int kept;		/* the tape, the words, or both, wait for sj_ffi_take */
static int tapeKept;		/* the tape did not fit the buffer of the read */

static inline size_t
pad8(size_t n)
{
	return (n + 7) & ~(size_t)7;
}

static void
slot(uint64_t w)
{
	if (!failed && sj_out_bytes(&tape, (const char *)&w, 8))
		failed = 1;
}

/* A 64-bit integer as two native uint32, the high one first: the image reads
   them with uint32AtOffset:, since int64AtOffset: answers wrong values beyond
   the SmallIntegers (the interpreter below -2^60, the JIT at 2^60) */
static void
halves(uint64_t v)
{
	uint32_t h[2] = { (uint32_t)(v >> 32), (uint32_t)v };

	if (!failed && sj_out_bytes(&tape, (const char *)h, 8))
		failed = 1;
}

static void
header(int tag, uint64_t payload)
{
	slot(payload << 8 | (uint64_t)tag);
}

/* Room for n bytes after a header, zeroed up to the next slot; answer where
   they go, or NULL */
static char *
room(size_t n)
{
	size_t padded = pad8(n);
	char *at;

	if (failed || sj_out_reserve(&tape, padded)) {
		failed = 1;
		return NULL;
	}
	at = tape.p + tape.n;
	if (padded > n)
		memset(at + n, 0, padded - n);
	tape.n += padded;
	return at;
}

static sj_oop
made(void)
{
	return failed ? 0 : MADE;
}

static void
sentinel(sj_oop v)
{
	switch (v) {
	case NIL: header(SJ_TAPE_NULL, 0); break;
	case TRUE_: header(SJ_TAPE_TRUE, 0); break;
	case FALSE_: header(SJ_TAPE_FALSE, 0); break;
	default: break;
	}
}


/*** The builder ***/

static sj_oop
bInt64(int64_t v)
{
	if (v > -SMALL_LIMIT && v < SMALL_LIMIT)
		slot((uint64_t)v << 8 | SJ_TAPE_SMALLINT);
	else {
		header(SJ_TAPE_INT64, 0);
		halves((uint64_t)v);
	}
	return made();
}

static sj_oop
bUInt64(uint64_t v)
{
	if (v <= (uint64_t)INT64_MAX)
		return bInt64((int64_t)v);
	header(SJ_TAPE_UINT64, 0);
	halves(v);
	return made();
}

static sj_oop
bFloat64(double v)
{
	uint64_t bits;

	memcpy(&bits, &v, 8);
	header(SJ_TAPE_FLOAT, 0);
	slot(bits);
	return made();
}

static sj_oop
bNumberText(const char *ascii, size_t n)
{
	char *at;

	sj_note_number_text();
	header(SJ_TAPE_NUMBERTEXT, n);
	if ((at = room(n)))
		memcpy(at, ascii, n);
	return made();
}

static sj_oop
bString(const char *utf8, size_t n)
{
	size_t chars;
	uint32_t maxcp;
	char *at;

	sj_utf8_scan(utf8, n, &chars, &maxcp);
	if (maxcp < 256) {
		header(SJ_TAPE_BYTESTRING, chars);
		if ((at = room(chars)))
			sj_utf8_to_latin1(utf8, n, (uint8_t *)at);
	} else {
		/* in the words, which the image copies into a WordArray, and from it into
		   WideStrings with one primitive each (none copies from a ByteArray) */
		header(SJ_TAPE_WIDESTRING, chars);
		if (!failed && sj_out_reserve(&words, 4 * chars))
			failed = 1;
		if (!failed) {
			sj_utf8_to_utf32(utf8, n, (uint32_t *)(words.p + words.n));	/* 4-aligned: malloc, 4 * chars */
			words.n += 4 * chars;
		}
	}
	return made();
}

static sj_oop
bArray(size_t n)
{
	header(SJ_TAPE_ARRAY, n);
	return made();
}

static int
bArrayPut(sj_oop array, size_t i0, sj_oop v)
{
	(void)array; (void)i0;
	sentinel(v);
	return failed ? -1 : 0;
}

static sj_oop
bObject(size_t npairs)
{
	header(SJ_TAPE_OBJECT, npairs);
	return made();
}

static int
bObjectPut(sj_oop obj, size_t pair0, sj_oop key, sj_oop v)
{
	(void)obj; (void)pair0; (void)key;
	sentinel(v);
	return failed ? -1 : 0;
}

static const sj_builder builder = {
	NIL, TRUE_, FALSE_,
	bInt64, bUInt64, bFloat64, bNumberText, bString, bString,
	bArray, bArrayPut, bObject, bObjectPut
};


/*** The exports ***/

static void
forget(void)
{
	kept = tapeKept = 0;
	tape.n = words.n = 0;
	if (tape.cap > KEEP_LIMIT)
		sj_out_free(&tape);
	if (words.cap > KEEP_LIMIT)
		sj_out_free(&words);
}

SJ_FFI_EXPORT int64_t
sj_ffi_read(const void *source, int64_t encoding, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags, int64_t maxUnits,
		void *buffer, int64_t capacity)
{
	int64_t answer[SJ_FFI_A_SLOTS] = { 0 };
	sj_request rq;
	sj_outcome out;
	int rc;

	if (capacity < SJ_FFI_HEADER || !buffer || units < 0 || start0 < 0 || stop0 < 0
	 || (encoding != SJ_ENC_UTF8 && encoding != SJ_ENC_LATIN1 && encoding != SJ_ENC_UTF32))
		return -1;	/* the engine checks start0 <= stop0 <= units */
	forget();
	failed = 0;
	rq.in.base = source;
	rq.in.units = (size_t)units;
	rq.in.encoding = (int)encoding;
	rq.in.start0 = (size_t)start0;
	rq.in.stop0 = (size_t)stop0;
	rq.flags = (unsigned)flags & (SJ_F_NEXT | SJ_F_EXACT);
	rq.budget = -1;
	rq.maxUnits = maxUnits;
	rc = sj_read(&rq, &builder, &out);
	if (rc == SJ_RC_DONE && out.status == SJ_OK)
		sentinel(out.value);	/* a value that is only null, true or false */
	if (rc == SJ_RC_NO_C_MEMORY || rc == SJ_RC_BUILD_FAILED || failed) {
		forget();
		failed = 0;
		return -2;	/* the builder fails only for C memory */
	}
	answer[SJ_FFI_A_STATUS] = out.status;
	answer[SJ_FFI_A_CODE] = out.status == SJ_ERROR ? out.code : out.need;
	answer[SJ_FFI_A_NEXT] = (int64_t)out.next0;
	if (out.status == SJ_OK) {
		answer[SJ_FFI_A_SIZE] = (int64_t)tape.n;
		answer[SJ_FFI_A_WORDS] = (int64_t)(words.n / 4);
		if ((int64_t)tape.n <= capacity - SJ_FFI_HEADER) {
			answer[SJ_FFI_A_INLINE] = 1;
			memcpy((char *)buffer + SJ_FFI_HEADER, tape.p, tape.n);
			tape.n = 0;
		} else
			tapeKept = 1;
		if (tapeKept || words.n) {
			answer[SJ_FFI_A_TICKET] = ++ticket;
			kept = 1;
		} else
			forget();
	} else
		forget();
	memcpy(buffer, answer, sizeof answer);
	return 0;
}

SJ_FFI_EXPORT int64_t
sj_ffi_take(void *dst, int64_t size, void *wordsDst, int64_t count, int64_t aTicket)
{
	if (!kept || aTicket != ticket
	 || size != (tapeKept ? (int64_t)tape.n : 0) || count != (int64_t)(words.n / 4)
	 || (size && !dst) || (count && !wordsDst))
		return -1;
	if (size)
		memcpy(dst, tape.p, tape.n);
	if (count)
		memcpy(wordsDst, words.p, words.n);
	forget();
	return 0;
}

SJ_FFI_EXPORT const char *
sj_ffi_error_message(int64_t code)
{
	const char *message = code >= 0 && code <= INT32_MAX ? sj_error_message((int)code) : NULL;

	return message ? message : "";
}

SJ_FFI_EXPORT int64_t
sj_ffi_protocol(void)
{
	return SJ_FFI_PROTOCOL;
}

SJ_FFI_EXPORT const char *
sj_ffi_simdjson_version(void)
{
	return SJ_SIMDJSON_VERSION;
}

SJ_FFI_EXPORT const char *
sj_ffi_implementation(void)
{
	static sj_info info;

	sj_get_info(&info);
	info.implementation[sizeof info.implementation - 1] = 0;
	return info.implementation;
}

static void wForget(void);

SJ_FFI_EXPORT void
sj_ffi_release(int64_t freeParser)
{
	kept = tapeKept = 0;
	sj_out_free(&tape);
	sj_out_free(&words);
	wForget();
	sj_release(freeParser != 0);
}


/*** Writing ***
 *
 * The walk of primitiveWrite (SimdJSONPlugin.c), on the tape of the image:
 * the same appenders of sjText.c and sj_print_double, the same newlines,
 * indentation, separators and checks, in the same order, so the same bytes.
 */

enum { W_PRETTY = 1, W_ASCII_ONLY = 2, W_BYTES = 4, W_NON_FINITE_NULL = 8 };
enum { W_C_OOM = -1, W_FAILED = -2, W_OVER = -3, W_MALFORMED = -4 };

static struct {
	const uint8_t *tape;
	size_t size, at;
	const uint32_t *words;
	size_t wordCount, wordAt;
	int flags;
	const char *newLine;
	size_t newLineSize;
	int64_t maxBytes;
	int64_t code, culprit;
	int64_t *holes;		/* (output byte, number, level) */
	size_t holeCount, holeCapacity;
} w;

static sj_out output;		/* UTF-8 */
static sj_out narrowed;		/* Latin-1 or UTF-32, when the output is a String */
static int64_t wTicket;
static int wKept;

static void
wForget(void)
{
	wKept = 0;
	output.n = narrowed.n = 0;
	w.holeCount = 0;
	if (output.cap > KEEP_LIMIT)
		sj_out_free(&output);
	if (narrowed.cap > KEEP_LIMIT)
		sj_out_free(&narrowed);
	if (w.holeCapacity > KEEP_LIMIT / 24) {
		free(w.holes);
		w.holes = NULL;
		w.holeCapacity = 0;
	}
}

static int
wSlot(int64_t *v)
{
	if (w.at + 8 > w.size)
		return W_MALFORMED;
	memcpy(v, w.tape + w.at, 8);
	w.at += 8;
	return 0;
}

static int
wFail(int64_t code, int64_t number)
{
	w.code = code;
	w.culprit = number;
	return W_FAILED;
}

static int
wEmit(const char *s, size_t n)
{
	return sj_out_bytes(&output, s, n) ? W_C_OOM : 0;
}

static int
wChar(char c)
{
	return sj_out_char(&output, c) ? W_C_OOM : 0;
}

static int
wNewlineIndent(int64_t level)
{
	int64_t i;

	if (!(w.flags & W_PRETTY))
		return 0;
	if (wEmit(w.newLine, w.newLineSize))
		return W_C_OOM;
	for (i = 0; i < level; i++)
		if (wChar('\t'))
			return W_C_OOM;
	return 0;
}

static int
wHole(int64_t number, int64_t level)
{
	if (w.holeCount == w.holeCapacity) {
		size_t capacity = w.holeCapacity ? 2 * w.holeCapacity : 16;
		int64_t *grown = realloc(w.holes, capacity * 3 * sizeof(int64_t));

		if (!grown)
			return W_C_OOM;
		w.holes = grown;
		w.holeCapacity = capacity;
	}
	w.holes[3 * w.holeCount] = (int64_t)output.n;
	w.holes[3 * w.holeCount + 1] = number;
	w.holes[3 * w.holeCount + 2] = level;
	w.holeCount++;
	return 0;
}

/* A string of the tape whose header (tag, payload) was read */
static int
wString(int tag, int64_t n)
{
	int rc;

	if (n < 0)
		return W_MALFORMED;
	if (tag == SJ_WTAPE_BYTESTRING) {
		size_t padded = pad8((size_t)n);

		if (padded > w.size - w.at)
			return W_MALFORMED;
		rc = sj_out_string(&output, w.tape + w.at, SJ_ENC_LATIN1, (size_t)n, w.flags & W_ASCII_ONLY);
		w.at += padded;
		return rc ? W_C_OOM : 0;
	}
	if (tag == SJ_WTAPE_WIDESTRING) {
		int64_t number;

		if ((rc = wSlot(&number)))
			return rc;
		if ((size_t)n > w.wordCount - w.wordAt)
			return W_MALFORMED;
		rc = sj_out_string(&output, w.words + w.wordAt, SJ_ENC_UTF32, (size_t)n, w.flags & W_ASCII_ONLY);
		w.wordAt += (size_t)n;
		return rc == -2 ? wFail(4, number) : rc ? W_C_OOM : 0;
	}
	return W_MALFORMED;
}

static int
wCloseContainer(int64_t count, int64_t level, char close)
{
	int rc;

	if (count) {
		if ((rc = wNewlineIndent(level)))
			return rc;
	} else if ((w.flags & W_PRETTY) && wChar(' '))
		return W_C_OOM;
	return wChar(close);
}

static int
wSeparate(int64_t index, int64_t level)
{
	if (index && wChar(','))
		return W_C_OOM;
	return wNewlineIndent(level + 1);
}

static int
wValue(int64_t level)
{
	char text[SJ_DOUBLE_CHARS];
	int64_t header, payload, v, i;
	double d;
	size_t n;
	int tag, rc;

	if (w.maxBytes >= 0 && output.n > (uint64_t)w.maxBytes)
		return W_OVER;
	if ((rc = wSlot(&header)))
		return rc;
	tag = (int)(header & 255);
	payload = header >> 8;	/* arithmetic */
	switch (tag) {
	case SJ_WTAPE_NULL: return wEmit("null", 4);
	case SJ_WTAPE_TRUE: return wEmit("true", 4);
	case SJ_WTAPE_FALSE: return wEmit("false", 5);
	case SJ_WTAPE_SMALLINT:
		return sj_out_int64(&output, payload) ? W_C_OOM : 0;
	case SJ_WTAPE_INT64:
		if ((rc = wSlot(&v)))
			return rc;
		return sj_out_int64(&output, v) ? W_C_OOM : 0;
	case SJ_WTAPE_FLOAT:
		if ((rc = wSlot(&v)))
			return rc;
		memcpy(&d, &v, 8);
		if (d - d != 0)	/* NaN or an infinity */
			return w.flags & W_NON_FINITE_NULL ? wEmit("null", 4) : wFail(2, payload);
		if (!(n = sj_print_double(d, text)))
			return W_C_OOM;
		return wEmit(text, n);
	case SJ_WTAPE_HOLE:
		return wHole(payload, level);
	case SJ_WTAPE_DEEP:
		return wFail(3, payload);
	case SJ_WTAPE_BYTESTRING: case SJ_WTAPE_WIDESTRING:
		return wString(tag, payload);
	case SJ_WTAPE_POSITIVE: case SJ_WTAPE_NEGATIVE: {
		size_t padded;

		if (payload < 0 || (padded = pad8((size_t)payload)) > w.size - w.at)
			return W_MALFORMED;
		rc = sj_out_magnitude(&output, w.tape + w.at, (size_t)payload, tag == SJ_WTAPE_NEGATIVE);
		w.at += padded;
		return rc ? W_C_OOM : 0;
	}
	case SJ_WTAPE_ARRAY:
		if (payload < 0 || wChar('['))
			return payload < 0 ? W_MALFORMED : W_C_OOM;
		for (i = 0; i < payload; i++)
			if ((rc = wSeparate(i, level)) || (rc = wValue(level + 1)))
				return rc;
		return wCloseContainer(payload, level, ']');
	case SJ_WTAPE_OBJECT:
		if (payload < 0 || wChar('{'))
			return payload < 0 ? W_MALFORMED : W_C_OOM;
		for (i = 0; i < payload; i++) {
			int64_t keyHeader;

			if ((rc = wSlot(&keyHeader)))
				return rc;
			if ((keyHeader & 255) == SJ_WTAPE_BADKEY)
				return wFail(1, keyHeader >> 8);
			if ((rc = wSeparate(i, level))
			 || (rc = wString((int)(keyHeader & 255), keyHeader >> 8)))
				return rc;
			if (((w.flags & W_PRETTY) && wChar(' ')) || wChar(':') || ((w.flags & W_PRETTY) && wChar(' ')))
				return W_C_OOM;
			if ((rc = wValue(level + 1)))
				return rc;
		}
		return wCloseContainer(payload, level, '}');
	default:
		return W_MALFORMED;
	}
}

/* A String output: Latin-1 when every character fits, else UTF-32, and the
   positions of the holes in characters */
static int
wNarrow(int64_t *kind, size_t *units)
{
	size_t i, chars, from = 0, count = 0;
	uint32_t maxcp;

	for (i = 0; i < output.n && (unsigned char)output.p[i] < 0x80; i++) {
	}
	if (i == output.n) {	/* ASCII: the same bytes */
		*kind = 1;
		*units = output.n;
		return 0;
	}
	sj_utf8_scan(output.p, output.n, &chars, &maxcp);
	*units = chars;
	*kind = maxcp <= 0xFF ? 1 : 2;
	if (sj_out_reserve(&narrowed, *kind == 1 ? chars : 4 * chars))
		return W_C_OOM;
	if (*kind == 1)
		sj_utf8_to_latin1(output.p, output.n, (uint8_t *)narrowed.p);
	else
		sj_utf8_to_utf32(output.p, output.n, (uint32_t *)narrowed.p);
	narrowed.n = *kind == 1 ? chars : 4 * chars;
	for (i = 0; i < w.holeCount; i++) {
		size_t at = (size_t)w.holes[3 * i];

		count += sj_utf8_chars(output.p + from, at - from);
		from = at;
		w.holes[3 * i] = (int64_t)count;
	}
	return 0;
}

SJ_FFI_EXPORT int64_t
sj_ffi_write(const void *tape, int64_t tapeSize, const void *words, int64_t wordCount,
		int64_t flags, const void *newLine, int64_t newLineSize, int64_t level, int64_t maxBytes,
		void *buffer, int64_t capacity)
{
	int64_t answer[SJ_FFI_W_SLOTS] = { 0 };
	int64_t kind = 0;
	size_t units, bytes, holeBytes;
	const char *out;
	int rc;

	if (capacity < SJ_FFI_W_HEADER || !buffer || !tape || tapeSize < 8 || wordCount < 0
	 || (wordCount && !words) || newLineSize < 0 || (newLineSize && !newLine) || level < 0 || maxBytes < -1
	 || flags < 0 || flags > 31)
		return -1;
	wForget();
	w.tape = tape;
	w.size = (size_t)tapeSize;
	w.at = 0;
	w.words = words;
	w.wordCount = (size_t)wordCount;
	w.wordAt = 0;
	w.flags = (int)flags;
	w.newLine = newLine;
	w.newLineSize = (size_t)newLineSize;
	w.maxBytes = maxBytes;
	w.code = w.culprit = 0;
	rc = wValue(level);
	if (rc == 0 && maxBytes >= 0 && output.n > (uint64_t)maxBytes)
		rc = W_OVER;
	if (rc == 0 && w.at != w.size)
		rc = W_MALFORMED;
	if (rc == 0 && !(flags & W_BYTES) && wNarrow(&kind, &units))
		rc = W_C_OOM;
	if (rc == W_C_OOM || rc == W_MALFORMED) {
		wForget();
		return rc == W_C_OOM ? -2 : -1;
	}
	if (rc == W_FAILED) {
		answer[SJ_FFI_W_STATUS] = 2;
		answer[SJ_FFI_W_CODE] = w.code;
		answer[SJ_FFI_W_CULPRIT] = w.culprit;
		wForget();
	} else if (rc == W_OVER) {
		answer[SJ_FFI_W_STATUS] = 3;
		wForget();
	} else {
		if (flags & W_BYTES)
			units = output.n;
		out = kind == 0 || narrowed.n == 0 ? output.p : narrowed.p;
		bytes = kind == 0 ? output.n : kind == 1 ? units : 4 * units;
		holeBytes = 24 * w.holeCount;
		answer[SJ_FFI_W_KIND] = kind;
		answer[SJ_FFI_W_UNITS] = (int64_t)units;
		answer[SJ_FFI_W_BYTES] = (int64_t)bytes;
		answer[SJ_FFI_W_HOLES] = (int64_t)w.holeCount;
		if (kind != 2 && holeBytes + bytes <= (size_t)capacity - SJ_FFI_W_HEADER) {
			answer[SJ_FFI_W_INLINE] = 1;
			if (holeBytes)
				memcpy((char *)buffer + SJ_FFI_W_HEADER, w.holes, holeBytes);
			if (bytes)
				memcpy((char *)buffer + SJ_FFI_W_HEADER + holeBytes, out, bytes);
			wForget();
		} else {
			answer[SJ_FFI_W_TICKET] = ++wTicket;
			wKept = 1;
		}
	}
	memcpy(buffer, answer, sizeof answer);
	return 0;
}

SJ_FFI_EXPORT int64_t
sj_ffi_take_output(void *holes, int64_t holeCount, void *dst, int64_t outputBytes, int64_t aTicket)
{
	int ascii = !(w.flags & W_BYTES) && narrowed.n == 0;
	size_t bytes = (w.flags & W_BYTES) || ascii ? output.n : narrowed.n;
	const char *out = (w.flags & W_BYTES) || ascii ? output.p : narrowed.p;

	if (!wKept || aTicket != wTicket || holeCount != (int64_t)w.holeCount || outputBytes != (int64_t)bytes
	 || (holeCount && !holes) || (bytes && !dst))
		return -1;
	if (holeCount)
		memcpy(holes, w.holes, 24 * w.holeCount);
	if (bytes)
		memcpy(dst, out, bytes);
	wForget();
	return 0;
}

