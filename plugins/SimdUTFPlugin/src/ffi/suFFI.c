/* suFFI.c -- libSimdUTFFFI: the engine of the SimdUTFPlugin (suEngine.cpp)
 * for the FFI of the image (suFFI.h has the contract).  Each su_ffi_*
 * function answers what the primitive of the same name answers, as a status
 * word of docs/SimdUTF.md.
 *
 * The library sees addresses, not objects: the image checks what only it can
 * see (the kinds of its objects, a read-only destination) before calling, and
 * the checks here are those the primitive makes on numbers, in its order:
 * NULL addresses first, then the source unit, the flags and options with the
 * kind of the source, the source range and its UTF-16 alignment, and last the
 * destination (its kind, the source itself, the index, the UTF-16 alignment).
 * The engine checks them again.  Every call is into a destination the image
 * allocated after a sizing call, so OVER_BUDGET is never answered.
 *
 * No scratch memory and no state between calls but su_init's choice and the
 * three names, which are filled once: the functions are reentrant.
 *
 * This file is not in src/common, which the plugin's CMake globs: the plugin
 * does not export these functions.  The library is built with hidden
 * visibility: only the SU_FFI_EXPORT functions are seen, so that the engine
 * of a SimdUTFPlugin loaded in the same process never gets in between.
 */

#include <stdint.h>
#include <string.h>

#include "suEngine.h"
#include "suFFI.h"

/*** The names, which outlive the call: the image reads them as Strings ***/

static su_info info;
static int infoState;		/* 0: empty, 1: being filled, 2: filled */

#if defined(_MSC_VER) && !defined(__clang__)
# include <intrin.h>
# define LOAD_ACQUIRE(p) _InterlockedOr((volatile long *)(p), 0)
# define STORE_RELEASE(p, v) _InterlockedExchange((volatile long *)(p), (v))
# define CLAIM(p) (_InterlockedCompareExchange((volatile long *)(p), 1, 0) == 0)
#else
# define LOAD_ACQUIRE(p) __atomic_load_n((p), __ATOMIC_ACQUIRE)
# define STORE_RELEASE(p, v) __atomic_store_n((p), (v), __ATOMIC_RELEASE)
# define CLAIM(p) claim(p)
static int
claim(int *state)
{
	int expected = 0;

	return __atomic_compare_exchange_n(state, &expected, 1, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
}
#endif

/* The first caller fills info; a caller that comes while it does waits for
   the few hundred bytes to be copied */
static const su_info *
theInfo(void)
{
	if (LOAD_ACQUIRE(&infoState) != 2) {
		if (CLAIM(&infoState)) {
			su_get_info(&info);
			STORE_RELEASE(&infoState, 2);
		} else {
			while (LOAD_ACQUIRE(&infoState) != 2) {
			}
		}
	}
	return &info;
}

SU_FFI_EXPORT int64_t
su_ffi_protocol(void)
{
	return su_init() ? SU_FFI_PROTOCOL : 0;
}

SU_FFI_EXPORT const char *
su_ffi_simdutf_version(void)
{
	return theInfo()->simdutf;
}

SU_FFI_EXPORT const char *
su_ffi_implementation(void)
{
	return theInfo()->implementation;
}

SU_FFI_EXPORT const char *
su_ffi_forced(void)
{
	return theInfo()->forced;
}

/*** Answers ***/

/* -(status + 4*code + 4096*payload); payload < 2^48, so it fits */
static int64_t
word(int status, int code, uint64_t payload)
{
	return -(int64_t)((uint64_t)status + 4 * (uint64_t)code + 4096 * payload);
}

/* The answer of an engine call; DECODED measures pack the characters and the
   kind as chars*2 + wide */
static int64_t
answer(int rc, const su_outcome *out, int packed)
{
	switch (rc) {
	case SU_RC_DONE:
		if (out->status == SU_ERROR) {
			return word(SU_ERROR, out->code, (uint64_t)out->position0 + 1);
		}
		if (packed) {
			return (int64_t)out->units * 2 + (out->kind == SU_KIND_UTF32);
		}
		return (int64_t)out->units;
	case SU_RC_NO_ROOM:
		return word(SU_TOO_LARGE, 0, out->units);
	case SU_RC_BAD_INDEX:
		return SU_FFI_BAD_INDEX;
	default:
		return SU_FFI_BAD_ARGUMENT;
	}
}

/*** Checks, in the order of the primitives ***/

static int
isUnicode(int64_t encoding)
{
	return encoding == SU_ENC_UTF8 || encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

static int
isUtf16(int64_t encoding)
{
	return encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

/* 0 when 0 <= start0 <= stop0 <= units (and start0 is even for a UTF-16
   source), else SU_FFI_BAD_INDEX */
static int64_t
checkRange(int64_t units, int64_t start0, int64_t stop0, int utf16)
{
	if (units < 0 || start0 < 0 || start0 > stop0 || stop0 > units || (utf16 && (start0 & 1))) {
		return SU_FFI_BAD_INDEX;
	}
	return 0;
}

/* 0 when the destination is not the source and 0 <= at0 <= dstUnits (at0
   even for a UTF-16 target), else the failure */
static int64_t
checkDestination(const void *source, void *dst, int64_t dstUnits, int64_t at0, int utf16)
{
	if (dst == source) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if (dstUnits < 0 || at0 < 0 || at0 > dstUnits || (utf16 && (at0 & 1))) {
		return SU_FFI_BAD_INDEX;
	}
	return 0;
}

static su_input
input(const void *source, int64_t words, int64_t units, int64_t start0, int64_t stop0)
{
	su_input in;

	in.base = source;
	in.units = (size_t)units;
	in.words = (int)words;
	in.start0 = (size_t)start0;
	in.stop0 = (size_t)stop0;
	return in;
}

/* The room of the destination from at0 on, of unitBytes-byte units */
static su_sink
into(void *dst, int kind, size_t unitBytes, int64_t dstUnits, int64_t at0)
{
	su_sink sink;

	sink.base = (char *)dst + (size_t)at0 * unitBytes;
	sink.capacity = (size_t)(dstUnits - at0);
	sink.kind = kind;
	sink.ctx = NULL;
	sink.allocate = NULL;
	return sink;
}

/*** The conversions ***/

SU_FFI_EXPORT int64_t
su_ffi_encode(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags,
		void *dst, int64_t dstUnits, int64_t at0)
{
	int64_t target = flags & SU_F_ENCODING, failure;
	su_input in;
	su_sink sink;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || !dst || (words != 0 && words != 1)) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((flags & ~(int64_t)(SU_F_ENCODING | SU_F_BOM)) || !isUnicode(target)) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, 0))
		|| (failure = checkDestination(source, dst, dstUnits, at0, isUtf16(target)))) {
		return failure;
	}
	in = input(source, words, units, start0, stop0);
	sink = into(dst, SU_KIND_BYTES, 1, dstUnits, at0);
	return answer(su_encode(&in, (unsigned)flags, &sink, &out), &out, 0);
}

SU_FFI_EXPORT int64_t
su_ffi_decode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags,
		void *dst, int64_t dstWords, int64_t dstUnits, int64_t at0)
{
	int64_t encoding = flags & SU_F_ENCODING, failure;
	su_input in;
	su_sink sink;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || !dst) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((flags & ~(int64_t)(SU_F_ENCODING | SU_F_BOM | SU_F_WIDE | SU_F_LATIN1)) || !isUnicode(encoding)
		|| ((flags & SU_F_WIDE) && (flags & SU_F_LATIN1))) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, isUtf16(encoding)))) {
		return failure;
	}
	if ((dstWords != 0 && dstWords != 1) || (dstWords && (flags & SU_F_LATIN1))
		|| (!dstWords && (flags & SU_F_WIDE))) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkDestination(source, dst, dstUnits, at0, 0))) {
		return failure;
	}
	in = input(source, 0, units, start0, stop0);
	sink = dstWords ? into(dst, SU_KIND_UTF32, 4, dstUnits, at0) : into(dst, SU_KIND_LATIN1, 1, dstUnits, at0);
	return answer(su_decode(&in, (unsigned)flags, &sink, &out), &out, 0);
}

SU_FFI_EXPORT int64_t
su_ffi_measure(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags)
{
	int64_t op = flags & SU_F_OP, encoding = flags & SU_F_ENCODING, failure;
	int utf16 = 0;
	su_input in;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || (words != 0 && words != 1)) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if (op == SU_OP_ENCODED) {
		if ((flags & ~(int64_t)(SU_F_ENCODING | SU_F_BOM)) || !isUnicode(encoding)) {
			return SU_FFI_BAD_ARGUMENT;
		}
	} else if (op == SU_OP_DECODED) {
		if (words || (flags & ~(int64_t)(SU_F_OP | SU_F_ENCODING | SU_F_BOM)) || !isUnicode(encoding)) {
			return SU_FFI_BAD_ARGUMENT;
		}
		utf16 = isUtf16(encoding);
	} else if (words || flags != SU_OP_BASE64) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, utf16))) {
		return failure;
	}
	in = input(source, words, units, start0, stop0);
	return answer(su_measure(&in, (unsigned)flags, &out), &out, op == SU_OP_DECODED);
}

SU_FFI_EXPORT int64_t
su_ffi_validate(const void *source, int64_t words, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags)
{
	int64_t failure;
	su_input in;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || (words != 0 && words != 1)) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if (flags != SU_OP_ASCII && (words ? flags != SU_ENC_UTF32 : !isUnicode(flags))) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, flags != SU_OP_ASCII && isUtf16(flags)))) {
		return failure;
	}
	in = input(source, words, units, start0, stop0);
	return answer(su_validate(&in, (unsigned)flags, &out), &out, 0);
}

static int
goodLineLength(int64_t lineLength)
{
	return lineLength == 0 || (lineLength > 0 && lineLength % 4 == 0 && (uint64_t)lineLength < SU_MAX_LINE_LENGTH);
}

SU_FFI_EXPORT int64_t
su_ffi_base64_encode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t options, int64_t lineLength, int64_t lineEnd,
		void *dst, int64_t dstUnits, int64_t at0)
{
	int64_t failure;
	su_input in;
	su_sink sink;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || !dst) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if (options < 0 || options > SU_B64_URL_PADDED || !goodLineLength(lineLength)
		|| lineEnd < 0 || lineEnd > SU_EOL_CRLF) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, 0))
		|| (failure = checkDestination(source, dst, dstUnits, at0, 0))) {
		return failure;
	}
	in = input(source, 0, units, start0, stop0);
	sink = into(dst, SU_KIND_LATIN1, 1, dstUnits, at0);
	return answer(su_base64_encode(&in, (unsigned)options, (size_t)lineLength, (unsigned)lineEnd, &sink, &out),
		&out, 0);
}

SU_FFI_EXPORT int64_t
su_ffi_base64_decode(const void *source, int64_t units,
		int64_t start0, int64_t stop0, int64_t options, int64_t lastChunk,
		void *dst, int64_t dstUnits, int64_t at0)
{
	int64_t failure;
	su_input in;
	su_sink sink;
	su_outcome out;

	su_init();
	if ((!source && units > 0) || !dst) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((options != SU_B64_STANDARD && options != SU_B64_URL && options != SU_B64_ANY)
		|| (lastChunk != SU_CHUNK_LOOSE && lastChunk != SU_CHUNK_STRICT)) {
		return SU_FFI_BAD_ARGUMENT;
	}
	if ((failure = checkRange(units, start0, stop0, 0))
		|| (failure = checkDestination(source, dst, dstUnits, at0, 0))) {
		return failure;
	}
	in = input(source, 0, units, start0, stop0);
	sink = into(dst, SU_KIND_BYTES, 1, dstUnits, at0);
	return answer(su_base64_decode(&in, (unsigned)options, (unsigned)lastChunk, &sink, &out), &out, 0);
}

SU_FFI_EXPORT int64_t
su_ffi_base64_encoded_size(int64_t n, int64_t options, int64_t lineLength, int64_t lineEnd)
{
	size_t size;

	su_init();
	if (n < 0 || options < 0 || options > SU_B64_URL_PADDED || !goodLineLength(lineLength)
		|| lineEnd < 0 || lineEnd > SU_EOL_CRLF) {
		return SU_FFI_BAD_ARGUMENT;
	}
	size = su_base64_encoded_size((size_t)n, (unsigned)options, (size_t)lineLength, (unsigned)lineEnd);
	if (size == SIZE_MAX || size > (size_t)INT64_MAX) {
		return SU_FFI_BAD_ARGUMENT;
	}
	return (int64_t)size;
}
