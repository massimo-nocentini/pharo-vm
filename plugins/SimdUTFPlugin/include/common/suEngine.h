/* suEngine.h -- the C interface between the SimdUTFPlugin glue (SimdUTFPlugin.c,
 * C, the only file that includes VM headers), libSimdUTFFFI (suFFI.c, C) and the
 * engine (suEngine.cpp, C++17, the only file that includes simdutf.h).  The
 * engine never allocates (no malloc, no new): results go into a caller's sink.
 * The contract is docs/SimdUTF.md, sections "Primitive contract" and "Library
 * contract". */
#ifndef SU_ENGINE_H
#define SU_ENGINE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

#define SU_PROTOCOL 1
#define SU_PLUGIN_VERSION "SimdUTFPlugin 1.0"

/* encodings (flags bits 0-2) */
enum { SU_ENC_UTF8 = 0, SU_ENC_LATIN1 = 1, SU_ENC_UTF32 = 2, SU_ENC_UTF16LE = 3, SU_ENC_UTF16BE = 4 };
/* flags of su_encode, su_decode, su_measure, su_validate */
enum { SU_F_ENCODING = 7, SU_F_BOM = 8, SU_F_WIDE = 16, SU_F_LATIN1 = 32, SU_F_OP = 192 };
enum { SU_OP_ENCODED = 0, SU_OP_DECODED = 64, SU_OP_BASE64 = 128 };	/* su_measure */
enum { SU_OP_VALID = 0, SU_OP_ASCII = 64 };				/* su_validate */
/* base64 */
enum { SU_B64_STANDARD = 0, SU_B64_URL = 1, SU_B64_STANDARD_NO_PADDING = 2,
       SU_B64_URL_PADDED = 3, SU_B64_ANY = 8 };		/* encode: 0-3; decode: 0, 1, 8 */
enum { SU_CHUNK_LOOSE = 0, SU_CHUNK_STRICT = 1 };
enum { SU_EOL_LF = 0, SU_EOL_CR = 1, SU_EOL_CRLF = 2 };
#define SU_MAX_LINE_LENGTH ((size_t)1 << 30)	/* exclusive */

/* statuses and error codes of an outcome */
enum { SU_OK = 0, SU_OVER_BUDGET = 1, SU_ERROR = 2, SU_TOO_LARGE = 3 };
enum { SU_E_HEADER_BITS = 1, SU_E_TOO_SHORT = 2, SU_E_TOO_LONG = 3, SU_E_OVERLONG = 4,
       SU_E_TOO_LARGE = 5, SU_E_SURROGATE = 6, SU_E_INVALID_BASE64_CHARACTER = 7,
       SU_E_BASE64_INPUT_REMAINDER = 8, SU_E_BASE64_EXTRA_BITS = 9,
       SU_E_OTHER = 11, SU_E_ODD_UTF16_LENGTH = 1001, SU_E_NOT_LATIN1 = 1002 };
/* what a result holds: a ByteArray, a ByteString (Latin-1 bytes), UTF-32 words */
enum { SU_KIND_BYTES = 0, SU_KIND_LATIN1 = 1, SU_KIND_UTF32 = 2 };
/* answers of the functions below */
enum { SU_RC_DONE = 0, SU_RC_NO_ROOM = -1, SU_RC_BAD_ARGUMENT = -2, SU_RC_BAD_INDEX = -3 };

typedef struct su_input {
	const void *base;	/* the first unit of the source OBJECT (not of the range) */
	size_t units;		/* units in the object: bytes, or 32-bit words when words != 0 */
	int words;		/* 0: bytes (Latin-1 for encode, else as flags say); 1: UTF-32 words */
	size_t start0;		/* 0-based first unit of the range */
	size_t stop0;		/* 0-based exclusive end of the range, start0 <= stop0 <= units */
} su_input;

/* Where a result goes.
 * Into mode (base != NULL): capacity units of the given kind from base (the
 * destination's room from its index on); a result that needs more answers
 * SU_RC_NO_ROOM, out->units = the units needed, nothing written.  For su_decode,
 * kind says what the destination takes: SU_KIND_LATIN1 (a byte object) or
 * SU_KIND_UTF32 (a word object).
 * Allocate mode (base == NULL): allocate is called at most once, after every
 * validation that can fail (su_base64_decode: before decoding, at the exact
 * size), with the exact units and their kind; it answers the address of the
 * first unit (any non-NULL address for 0 units) or NULL: SU_RC_NO_ROOM,
 * out->units and out->kind = the request.
 * A sink with base == NULL and allocate == NULL is SU_RC_BAD_ARGUMENT (never a
 * call through a null pointer); so is an su_input with base == NULL and
 * units > 0. */
typedef struct su_sink {
	void *base;
	size_t capacity;
	int kind;
	void *ctx;
	void *(*allocate)(void *ctx, int kind, size_t units);
} su_sink;

typedef struct su_outcome {
	int status;		/* SU_OK or SU_ERROR when SU_RC_DONE */
	int code;		/* SU_ERROR: SU_E_* */
	int kind;		/* SU_OK: SU_KIND_* of the result (su_measure DECODED: the narrowest) */
	size_t position0;	/* SU_ERROR: absolute 0-based unit of the source object */
	size_t units;		/* SU_OK: units written or measured (DECODED: characters);
				   SU_RC_NO_ROOM: units needed */
} su_outcome;

typedef struct su_info {
	int ready;			/* the active kernel is not "unsupported" */
	int emscripten;
	char simdutf[16];		/* SIMDUTF_VERSION */
	char implementation[32];	/* the active kernel after su_init */
	char forced[64];		/* "", the honoured SIMDUTF_FORCE_IMPLEMENTATION, or "refused <value>", cut to 63 bytes */
	int count;			/* names in implementations */
	char implementations[8][32];	/* compiled in, runtime-supported, not "unsupported" */
} su_info;

/* Idempotent and thread-safe; every function below calls it first.  Chooses the
 * kernel (the SIMDUTF_FORCE_IMPLEMENTATION guard).  Answers info.ready. */
int su_init(void);
void su_get_info(su_info *info);

/* Each answers SU_RC_DONE (then *out holds SU_OK or SU_ERROR), SU_RC_NO_ROOM, or
 * SU_RC_BAD_ARGUMENT / SU_RC_BAD_INDEX for a request outside the contract (the
 * glue checks first and maps them to PrimErrBadArgument / PrimErrBadIndex; the
 * FFI relies on them).  Flag, option and alignment rules: docs/SimdUTF.md. */
int su_encode(const su_input *in, unsigned flags, const su_sink *sink, su_outcome *out);
int su_decode(const su_input *in, unsigned flags, const su_sink *sink, su_outcome *out);
int su_measure(const su_input *in, unsigned flags, su_outcome *out);
int su_validate(const su_input *in, unsigned flags, su_outcome *out);
int su_base64_encode(const su_input *in, unsigned options, size_t lineLength, unsigned lineEnd,
		const su_sink *sink, su_outcome *out);
int su_base64_decode(const su_input *in, unsigned options, unsigned lastChunk,
		const su_sink *sink, su_outcome *out);
/* the characters of base64 for n bytes (docs/SimdUTF.md, primitiveBase64Encode),
   or SIZE_MAX for bad options or when the result does not fit in size_t
   (checked arithmetic) */
size_t su_base64_encoded_size(size_t n, unsigned options, size_t lineLength, unsigned lineEnd);

#ifdef __cplusplus
}
#endif
#endif
