/* sjFFI.h -- the C interface of libSimdJSONFFI, the library that the
 * package SimdJSON-FFI (the submodule plugins/SimdJSONPlugin/simdjson.st)
 * calls through the FFI of the image when the VM has no SimdJSONPlugin.
 *
 * It runs the engine of the plugin (sjEngine.cpp, sjText.c) unchanged,
 * with a builder (sjFFI.c) that writes the value as a tape of 8-byte slots
 * in pre-order, which the image decodes (SimdJSONTapeDecoder); and it
 * writes JSON from a tape that the image writes (SimdJSONTapeEncoder), as
 * primitiveWrite writes it (see Writing below).  No VM header, no object
 * pointer, no callback: every argument is a number or the address of the
 * bytes or words of a String, a ByteArray or a WordArray.
 *
 * The tape, native byte order, every slot 8 bytes:
 *
 *   header = payload << 8 | tag, as an int64 (payload signed for SMALLINT)
 *
 *   SJ_TAPE_SMALLINT    payload: the integer, |v| < 2^52
 *   SJ_TAPE_BYTESTRING  payload: characters n; n Latin-1 bytes follow, padded to 8
 *   SJ_TAPE_FLOAT       one slot follows: the double
 *   SJ_TAPE_ARRAY       payload: elements n; n values follow
 *   SJ_TAPE_OBJECT      payload: pairs n; n (key, value) follow, the key a string
 *   SJ_TAPE_NULL, SJ_TAPE_TRUE, SJ_TAPE_FALSE
 *   SJ_TAPE_INT64       one slot follows: the int64, as two native uint32, the high first
 *   SJ_TAPE_UINT64      one slot follows: the uint64 (above INT64_MAX), the same way
 *   SJ_TAPE_WIDESTRING  payload: characters n; its n uint32 code points are the next n
 *                       of the words, a second area that sj_ffi_take copies into a
 *                       WordArray (the image copies a WordArray into a WideString with
 *                       one primitive, a ByteArray only a character at a time)
 *   SJ_TAPE_NUMBERTEXT  payload: bytes n; n ASCII bytes follow, padded to 8: the text of
 *                       a number the image converts (an exponent with SJ_F_EXACT, or an
 *                       integer beyond 64 bits)
 *
 * sj_ffi_read writes its answer in the first SJ_FFI_HEADER bytes of buffer,
 * as int64 slots (SJ_FFI_A_*), and the tape after them when it fits in
 * capacity; otherwise, and for the words, sj_ffi_take copies them into
 * buffers of their size.
 */

#ifndef SJ_FFI_H
#define SJ_FFI_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

#if defined(_WIN32)
# define SJ_FFI_EXPORT __declspec(dllexport)
#else
# define SJ_FFI_EXPORT __attribute__((visibility("default")))
#endif

#define SJ_FFI_PROTOCOL 1	/* sj_ffi_protocol(): the version of this contract */

enum {
	SJ_TAPE_SMALLINT = 1, SJ_TAPE_BYTESTRING = 2, SJ_TAPE_FLOAT = 3,
	SJ_TAPE_ARRAY = 4, SJ_TAPE_OBJECT = 5, SJ_TAPE_NULL = 6, SJ_TAPE_TRUE = 7,
	SJ_TAPE_FALSE = 8, SJ_TAPE_INT64 = 9, SJ_TAPE_UINT64 = 10,
	SJ_TAPE_WIDESTRING = 11, SJ_TAPE_NUMBERTEXT = 12
};

/* The int64 slots of the answer, at the start of buffer */
enum {
	SJ_FFI_A_STATUS = 0,	/* SJ_OK, SJ_ERROR or SJ_TOO_LARGE of sjBridge.h */
	SJ_FFI_A_CODE = 1,	/* SJ_ERROR: the error code; SJ_TOO_LARGE: units of the value */
	SJ_FFI_A_NEXT = 2,	/* SJ_OK: 0-based unit after the value and its white space */
	SJ_FFI_A_SIZE = 3,	/* SJ_OK: bytes of the tape */
	SJ_FFI_A_TICKET = 4,	/* SJ_OK: what sj_ffi_take wants, when it has something to copy */
	SJ_FFI_A_INLINE = 5,	/* SJ_OK: 1 when the tape follows the answer in buffer */
	SJ_FFI_A_WORDS = 6,	/* SJ_OK: the uint32 of the words */
	SJ_FFI_A_SLOTS = 8
};
#define SJ_FFI_HEADER (8 * SJ_FFI_A_SLOTS)

/* Read the JSON value at start0 of source (units of encoding, SJ_ENC_* of
 * sjBridge.h), up to stop0 (exclusive), with flags SJ_F_NEXT and
 * SJ_F_EXACT (the others are ignored), and maxUnits (-1: no cap), as
 * primitiveRead does.  Answer 0, with the answer in buffer, -1 for a bad
 * argument (capacity below SJ_FFI_HEADER, a negative unit, an unknown
 * encoding), or -2 when C memory ran out. */
SJ_FFI_EXPORT int64_t sj_ffi_read(const void *source, int64_t encoding, int64_t units,
		int64_t start0, int64_t stop0, int64_t flags, int64_t maxUnits,
		void *buffer, int64_t capacity);

/* Copy what the last sj_ffi_read kept: the tape into dst, of size bytes (0
 * when it was inline), and the words into wordsDst, of count uint32; forget
 * them.  Answer 0, or -1 when ticket, size or count is not the last read's
 * (another read came in between: read again). */
SJ_FFI_EXPORT int64_t sj_ffi_take(void *dst, int64_t size, void *wordsDst, int64_t count,
		int64_t ticket);

/* simdjson's message of an error code, or "" */
SJ_FFI_EXPORT const char *sj_ffi_error_message(int64_t code);

SJ_FFI_EXPORT int64_t sj_ffi_protocol(void);
SJ_FFI_EXPORT const char *sj_ffi_simdjson_version(void);
/* The simdjson kernel: haswell, westmere, arm64, fallback... */
SJ_FFI_EXPORT const char *sj_ffi_implementation(void);

/* Free the tape kept, and with freeParser the memory of the parser */
SJ_FFI_EXPORT void sj_ffi_release(int64_t freeParser);


/*** Writing: the image writes a tape of the value, the library its JSON ***
 *
 * The image (SimdJSONTapeEncoder) walks the value as primitiveWrite walks
 * it and writes a tape of 8-byte slots in pre-order, header payload << 8 |
 * tag as an int64, the code points of the WideStrings in a second area of
 * uint32 (the words).  The objects it cannot write, and those that may be
 * the culprit of an error, are numbered from 1 in the order it meets them;
 * the library answers those numbers, and the image the objects.
 *
 *   SJ_WTAPE_SMALLINT    payload: the integer, |v| < 2^52
 *   SJ_WTAPE_INT64       one slot follows: an integer of the SmallInteger range
 *   SJ_WTAPE_BYTESTRING  payload: characters n; n Latin-1 bytes follow, padded to 8
 *   SJ_WTAPE_WIDESTRING  payload: characters n; one slot follows: its number; its
 *                        code points are the next n words
 *   SJ_WTAPE_FLOAT       payload: 0, or the number of a NaN or an infinity; one slot
 *                        follows: the double
 *   SJ_WTAPE_ARRAY       payload: elements n; n values follow
 *   SJ_WTAPE_OBJECT      payload: pairs n; n (key, value) follow, the key a string or
 *                        SJ_WTAPE_BADKEY
 *   SJ_WTAPE_NULL, SJ_WTAPE_TRUE, SJ_WTAPE_FALSE
 *   SJ_WTAPE_POSITIVE, SJ_WTAPE_NEGATIVE
 *                        payload: bytes n; the n bytes of a LargeInteger's magnitude
 *                        follow, least significant first, padded to 8
 *   SJ_WTAPE_HOLE        payload: its number: an object the image writes
 *   SJ_WTAPE_BADKEY      payload: its number: a key that is not a string (error 1)
 *   SJ_WTAPE_DEEP        payload: its number: a value deeper than maxDepth (error 3)
 *
 * sj_ffi_write writes the JSON as primitiveWrite does, from level, with
 * flags SJW_* of SimdJSONPlugin.c (1 PRETTY, 2 ASCII_ONLY, 4 BYTES, 8
 * NON_FINITE_NULL; 16 FLOAT_HOLES is the image's) and maxBytes (-1 none),
 * and answers in the first SJ_FFI_W_HEADER bytes of buffer (SJ_FFI_W_*):
 * the status (0 OK, 2 ERROR, 3 TOO_LARGE), the error code and the number of
 * the culprit; the output as UTF-8 (BYTES), else as the narrowest of Latin-1
 * and UTF-32; and the holes, (position in units of the output, number,
 * level) as three int64 each.  When the output is not UTF-32 and the holes
 * and it fit, they follow the answer in buffer, holes first; otherwise
 * sj_ffi_take_output copies them. */

enum {
	SJ_WTAPE_SMALLINT = 1, SJ_WTAPE_BYTESTRING = 2, SJ_WTAPE_FLOAT = 3, SJ_WTAPE_ARRAY = 4,
	SJ_WTAPE_OBJECT = 5, SJ_WTAPE_NULL = 6, SJ_WTAPE_TRUE = 7, SJ_WTAPE_FALSE = 8,
	SJ_WTAPE_INT64 = 9, SJ_WTAPE_WIDESTRING = 11, SJ_WTAPE_HOLE = 13, SJ_WTAPE_BADKEY = 14,
	SJ_WTAPE_DEEP = 15, SJ_WTAPE_POSITIVE = 16, SJ_WTAPE_NEGATIVE = 17
};

enum {
	SJ_FFI_W_STATUS = 0,	/* 0 OK, 2 ERROR, 3 TOO_LARGE */
	SJ_FFI_W_CODE = 1,	/* ERROR: 1 a key, 2 a NaN or an infinity, 3 too deep, 4 a code point */
	SJ_FFI_W_CULPRIT = 2,	/* ERROR: the number of the culprit */
	SJ_FFI_W_KIND = 3,	/* OK: 0 UTF-8, 1 Latin-1, 2 UTF-32 */
	SJ_FFI_W_UNITS = 4,	/* OK: the units of the output (bytes, or characters) */
	SJ_FFI_W_BYTES = 5,	/* OK: its bytes */
	SJ_FFI_W_HOLES = 6,	/* OK: the holes */
	SJ_FFI_W_INLINE = 7,	/* OK: 1 when the holes and the output follow the answer */
	SJ_FFI_W_TICKET = 8,	/* OK: what sj_ffi_take_output wants otherwise */
	SJ_FFI_W_SLOTS = 16
};
#define SJ_FFI_W_HEADER (8 * SJ_FFI_W_SLOTS)

/* Answer 0, with the answer in buffer, -1 for a bad argument or a malformed
 * tape, or -2 when C memory ran out */
SJ_FFI_EXPORT int64_t sj_ffi_write(const void *tape, int64_t tapeSize, const void *words, int64_t wordCount,
		int64_t flags, const void *newLine, int64_t newLineSize, int64_t level, int64_t maxBytes,
		void *buffer, int64_t capacity);

/* Copy the holes (3 int64 each) and the output that the last sj_ffi_write
 * kept, and forget them; answer 0, or -1 when a count or the ticket is not
 * that write's (another write came in between: write again) */
SJ_FFI_EXPORT int64_t sj_ffi_take_output(void *holes, int64_t holeCount, void *output, int64_t outputBytes,
		int64_t ticket);

#ifdef __cplusplus
}
#endif
#endif
