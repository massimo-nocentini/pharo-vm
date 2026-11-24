/* scEngine.h -- the C interface between the SimdCSVPlugin glue
 * (src/common/SimdCSVPlugin.c, C99, the only file that includes VM headers)
 * and its engine (src/engine/scEngine.cpp, C++17 without the C++ library).
 *
 * Total functions: no abort, no I/O, no allocation, no writable global but
 * the chosen kernel.  Memory is the caller's.  A unit is a byte (wide = 0) or
 * a 32-bit word (wide = 1); an input is units[from, to), and no unit outside
 * of it is read.  Positions are 0-based and half-open here, and every position
 * of sc_scan and sc_text is an offset into units (not relative to from); the
 * values of an index are offsets from its base.
 *
 * The automaton.  END is the separator, LF or CR (with SC_F_LF_ONLY: not CR);
 * the LF after a CR that ended a record belongs to that end.
 *   START     quote -> QUOTED; END -> a field ends; other -> UNQUOTED
 *   UNQUOTED  END -> a field ends; quote -> irregular (SC_E_UNEXPECTED_QUOTE),
 *             literal; other -> stay
 *   QUOTED    quote -> AFTER; other -> stay
 *   AFTER     quote -> QUOTED (an escaped quote); END -> a field ends; other
 *             -> irregular (SC_E_TEXT_AFTER_QUOTE), literal, UNQUOTED
 * At the end of the data (SC_F_FINAL) an open record that is not empty has an
 * entry at `to'; in QUOTED it is unterminated: lenient keeps the rest as the
 * field, SC_F_STRICT answers SC_E_UNTERMINATED_QUOTE at the opening quote.
 * With SC_F_SKIP_BLANK a line end at the start of a record makes no entry and
 * no record.
 *
 * An index holds one uint32_t per field: the offset, from base, of the unit
 * that ends it (or of `to').  docs/SimdCSV.md is the whole contract. */
#ifndef SC_ENGINE_H
#define SC_ENGINE_H
#include <stddef.h>
#include <stdint.h>
#if defined(__GNUC__) || defined(__clang__)
# define SC_API __attribute__((visibility("hidden")))	/* never exported from libSimdCSVPlugin.so */
#else
# define SC_API
#endif
#ifdef __cplusplus
extern "C" {
#endif

#define SC_PROTOCOL 1
#define SC_MAX_SPAN 4294967294u			/* to - base */
#define SC_MAX_FIELD_COUNT 16777216

enum { SC_OK, SC_MORE, SC_ERROR, SC_TOO_LARGE, SC_NO_ROOM };	/* status */
enum { SC_E_NONE, SC_E_UNEXPECTED_QUOTE, SC_E_TEXT_AFTER_QUOTE, SC_E_UNTERMINATED_QUOTE,
       SC_E_INVALID_UTF8, SC_E_FIELD_COUNT, SC_E_BAD_INDEX };	/* code */
enum { SC_F_STRICT=1, SC_F_LF_ONLY=2, SC_F_NO_QUOTE=4, SC_F_SKIP_BLANK=8, SC_F_LATIN1=16, SC_F_FINAL=32, SC_F_LINES=64,
       SC_F_EXACT_COUNT=128, SC_F_KEEP_QUOTED_EMPTY=256, SC_F_ALL=511 };

typedef struct sc_input { const void *units; int wide; uint64_t from, to; } sc_input;	/* units[from, to), 1 or 4 bytes each */
typedef struct sc_dialect { uint32_t separator; uint8_t quote; uint32_t flags; } sc_dialect;
typedef struct sc_scan { int status, code; uint64_t stored, records, next, lines, min_fields, max_fields, position; } sc_scan;

SC_API void sc_init(void);			/* chooses the kernel; the only getenv (SIMDCSV_FORCE_IMPLEMENTATION) */
SC_API const char *sc_kernel(void);
SC_API const char *sc_forced_note(void);	/* "", the forced kernel, or "refused <value>" */
SC_API int sc_kernel_count(void);
SC_API const char *sc_kernel_name(int i);	/* compiled AND supported here, best first */
SC_API int sc_use_kernel(int i);		/* only with -DSC_TESTING */

/* Both: in [from, to), which starts at a record boundary.  Only complete
 * records are stored, next is the start of the first one that is not.  Events
 * in input order: no slot left (SC_MORE), an irregular unit with SC_F_STRICT
 * (SC_ERROR), the end of the range (SC_OK). */
SC_API void sc_measure(const sc_input *, const sc_dialect *, sc_scan *);
SC_API void sc_index(const sc_input *, const sc_dialect *, uint64_t base, uint32_t *dest, uint64_t capacity, sc_scan *);

/* The fields of the records of an index, which is not trusted */
typedef struct sc_cursor { sc_input in; sc_dialect d; const uint32_t *index; uint64_t slot, end, pos; int at_start; } sc_cursor;
typedef struct sc_field { uint64_t start, end; int last; } sc_field;	/* [start,end), quotes included; last of its record */
SC_API int sc_cursor_open(sc_cursor *, const sc_input *, const sc_dialect *, const uint32_t *index, uint64_t first, uint64_t end);
SC_API int sc_cursor_next(sc_cursor *, sc_field *);	/* both: 0 or SC_E_BAD_INDEX; in.from is base */

/* The text of a field */
enum { SC_EMPTY, SC_EMPTY_QUOTED, SC_BYTES, SC_WIDE };
typedef struct sc_text { int kind, code; uint64_t units, position; uint64_t start, end; int quoted, plain; } sc_text;
SC_API void sc_text_measure(const sc_input *, const sc_dialect *, const sc_field *, sc_text *);	/* code 0 or SC_E_INVALID_UTF8 + position */
SC_API void sc_text_copy(const sc_input *, const sc_dialect *, const sc_text *, void *dest);

/* The number of a field: 1 converted, 0 odd */
SC_API int sc_to_integer(const sc_input *, const sc_dialect *, const sc_field *, int64_t *);
SC_API int sc_to_float(const sc_input *, const sc_dialect *, const sc_field *, int comma, double *);

#ifdef __cplusplus
}
#endif
#endif
