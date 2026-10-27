/* sjBridge.h -- the C interface between the SimdJSONPlugin glue
 * (SimdJSONPlugin.c, C, the only file that includes VM headers) and its
 * engine (sjEngine.cpp, C++17, the only file that includes simdjson.h).
 *
 * The engine parses one JSON value of a Smalltalk source object and walks
 * it, calling back the builder of the glue for every object to allocate;
 * for the writer it only formats doubles.
 * The primitive contract is in docs/SimdJSON.md, section "Primitive contract".
 */

#ifndef SJ_BRIDGE_H
#define SJ_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef int64_t sj_oop;                 /* sqInt on 64-bit images, i64 on wasm64 too; 0 = failed */

enum { SJ_ENC_UTF8 = 0, SJ_ENC_LATIN1 = 1, SJ_ENC_UTF32 = 2 };
enum { SJ_F_NEXT = 1, SJ_F_EXACT = 2, SJ_F_DICT = 4, SJ_F_SYMKEYS = 8 };
enum { SJ_OK = 0, SJ_OVER_BUDGET = 1, SJ_ERROR = 2, SJ_TOO_LARGE = 3 };
enum { SJ_ERR_BOM = 1001, SJ_ERR_NUMBERS = 1002 };
enum { SJ_RC_DONE = 0, SJ_RC_BUILD_FAILED = -1, SJ_RC_NO_C_MEMORY = -2 };
enum { SJ_NUM_STATS = 12 };            /* the counters of sj_info, primitiveInfo's stats Array */

typedef struct sj_input {
	const void *base;       /* firstIndexableField of the source */
	size_t      units;      /* units in the object */
	int         encoding;   /* SJ_ENC_* */
	size_t      start0;     /* 0-based first unit */
	size_t      stop0;      /* 0-based exclusive end, <= units */
} sj_input;

/* Implemented by SimdJSONPlugin.c.  Every call answers 0 (or -1 for the int
   ones) once an allocation failed; the VM failure code is then already set. */
typedef struct sj_builder {
	sj_oop nilOop, trueOop, falseOop;
	sj_oop (*int64)(int64_t v);                        /* SmallInteger, or LargeInteger (8 bytes) */
	sj_oop (*uint64)(uint64_t v);
	sj_oop (*float64)(double v);                       /* immediate, or BoxedFloat64 */
	sj_oop (*number_text)(const char *ascii, size_t n);/* ByteString; becomes a fixup when stored */
	sj_oop (*string)(const char *utf8, size_t n);      /* ByteString or wideClass, narrowest */
	sj_oop (*key)(const char *utf8, size_t n);         /* String, or SmallInteger index with SYMKEYS */
	sj_oop (*array)(size_t n);                         /* Array of nils */
	int    (*array_put)(sj_oop array, size_t i0, sj_oop v);
	sj_oop (*object)(size_t npairs);                   /* FlatMap of 2n slots, or empty Dictionary */
	int    (*object_put)(sj_oop obj, size_t pair0, sj_oop key, sj_oop v);
} sj_builder;

typedef struct sj_request {
	sj_input in;
	unsigned flags;         /* SJ_F_* */
	int64_t  budget;        /* bytes the build may allocate, -1 unchecked */
	int64_t  maxUnits;      /* the most units simdjson is given, -1 none */
} sj_request;

typedef struct sj_outcome {
	int     status;         /* SJ_OK, SJ_OVER_BUDGET, SJ_ERROR, SJ_TOO_LARGE */
	int     code;           /* SJ_ERROR: error code */
	int64_t need;           /* SJ_OVER_BUDGET: bytes; SJ_TOO_LARGE: units of the value */
	sj_oop  value;          /* SJ_OK: root */
	size_t  next0;          /* SJ_OK: 0-based unit after the value and its white space */
} sj_outcome;

typedef struct sj_info {
	char    implementation[32];
	int     emscripten;
	int64_t capacity, cachedBytes;
	int64_t stats[SJ_NUM_STATS];   /* order of primitiveInfo's stats Array */
} sj_info;

/* SJ_RC_DONE: *out is meaningful.  SJ_RC_BUILD_FAILED: a builder call
   failed (leave the stack alone).  SJ_RC_NO_C_MEMORY: fail PrimErrNoCMemory. */
int         sj_read(const sj_request *rq, const sj_builder *b, sj_outcome *out);
void        sj_release(int free_parser);
const char *sj_error_message(int code);   /* static string, or NULL */
void        sj_get_info(sj_info *info);
void        sj_note_number_text(void);     /* counter, called by the builder */
/* The text of a finite double as Float>>printOn: (and NeoJSONWriter) spells
   it, from simdjson's shortest round-trip digits, into dst of at least
   SJ_DOUBLE_CHARS bytes; answers its length, or 0 on C OOM. */
enum { SJ_DOUBLE_CHARS = 64 };
size_t      sj_print_double(double v, char *dst);
#ifdef __cplusplus
}
#endif
#endif
