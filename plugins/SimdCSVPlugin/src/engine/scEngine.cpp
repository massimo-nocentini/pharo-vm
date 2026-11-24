/* scEngine.cpp -- the engine of the SimdCSVPlugin: the choice of the kernel,
 * the automaton, the loop around the kernels, the cursor, the text of a field
 * and its numbers.  scEngine.h is the interface.
 *
 * The part of src/main.cpp of simdcsv a9494a8 (Apache-2.0) from 'struct
 * simd_input' to flatten_bits is included byte for byte (SHA256 checked).  On
 * x86_64 __AVX2__ is defined for the include; on arm64 its vmull_p64 is
 * replaced by a macro (a shift prefix-xor, no PMULL) and neonmovemask_bulk,
 * which upstream does not define, is ours.  The loop, the tail, the checks,
 * the other kernels, the builders and the errors are ours.  WebAssembly builds
 * contain no simdcsv code.
 *
 * C++17 without the C++ library: no C++ header, no exception, no RTTI, no
 * static constructor, no magic static, no heap.
 *
 * Over-read and over-write.  No unit outside [from, to) is read: the last
 * partial block is copied into a zeroed array of 64 bytes, and every mask is
 * cut to its real bytes.  No slot outside dest[0, capacity) is written; slots
 * beyond `stored' may hold garbage. */
#include "scEngine.h"
#include <float.h>
#include <stdlib.h>
#include <string.h>
#if defined(__x86_64__)
# include <immintrin.h>         /* before any target pragma */
#elif defined(__aarch64__)
# include <arm_neon.h>
#endif

#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ != __ORDER_LITTLE_ENDIAN__
# error "SimdCSVPlugin: little-endian only"
#endif
static_assert(sizeof(size_t) == 8 && sizeof(void *) == 8, "64-bit only");
static_assert(FLT_EVAL_METHOD == 0, "double arithmetic must be done in double");

#define sc_inline static inline __attribute__((always_inline, unused))

namespace {

enum { LF = 10, CR = 13 };

/* What the units of a dialect are.  A separator is never a line end. */
struct sc_classes {
    uint32_t separator, quote;
    bool lf_only, no_quote;

    explicit sc_classes(const sc_dialect *d)
        : separator(d->separator), quote(d->quote), lf_only((d->flags & SC_F_LF_ONLY) != 0),
          no_quote((d->flags & SC_F_NO_QUOTE) != 0) {}
    bool is_quote(uint32_t c) const { return c == quote && !no_quote; }
    bool is_separator(uint32_t c) const { return c == separator; }
    bool is_line_end(uint32_t c) const { return c != separator && (c == LF || (c == CR && !lf_only)); }
    /* Where the record after the line end at e starts */
    template <class U> uint64_t after(const U *u, uint64_t e, uint64_t to) const {
        return e + 1 + (u[e] == CR && e + 1 < to && u[e + 1] == LF && separator != LF);
    }
};

/* The needles of a kernel, XORed as its registers are so that the quote is
 * '"', and the masks of the flags */
struct sc_needles {
    uint8_t flip, separator, lf, cr;
    uint64_t quoting, cr_ends, skip_blank;      /* all ones or 0 */
};

/* How a scan stopped */
enum { SC_AT_END, SC_NO_SLOT, SC_IRREGULAR };

/* One call of sc_index (dest) or sc_measure (no dest, fields are counted) */
struct sc_job {
    const void *units;
    uint64_t limit;             /* where the scan ends: to, or to - 1 */
    uint64_t base;
    uint32_t *dest;
    uint64_t capacity;
    sc_needles needles;
    uint32_t flags;
    /* progress */
    uint64_t pos;               /* in: the start of a record */
    uint64_t fields;            /* entries stored, or fields counted */
    /* the complete records: always right after the automaton; after a kernel
     * only sc_measure's (sc_index finds them in dest) */
    uint64_t done;              /* where the last one ends: the open record's start */
    uint64_t done_fields, records;
    /* how it stopped */
    int event;
    uint64_t at;                /* SC_IRREGULAR: the unit */
    bool inside;                /* SC_AT_END: in QUOTED */
    uint64_t opening;           /* the automaton: the last opening quote */
};

#include "scKernels.inc"

/* ------------------------------------------------------------ the kernels */

struct sc_kernel_entry {
    const char *name;
    void (*index)(sc_job *);
    void (*measure)(sc_job *);
    uint64_t (*lines)(const uint8_t *u, uint64_t from, uint64_t to, bool lf_only);
};

/* Best first, the last one is always supported */
constexpr sc_kernel_entry sc_kernels[] = {
#if defined(__x86_64__)
    { "avx2", sc_avx2::scan_index, sc_avx2::scan_measure, sc_avx2::count_lines },
    { "sse2", sc_sse2::scan_index, sc_sse2::scan_measure, sc_sse2::count_lines },
#elif defined(__aarch64__)
    { "neon", sc_neon::scan_index, sc_neon::scan_measure, sc_neon::count_lines },
#endif
    { "fallback", sc_fallback::scan_index, sc_fallback::scan_measure, sc_fallback::count_lines },
};
constexpr int sc_compiled = (int)(sizeof sc_kernels / sizeof sc_kernels[0]);

/* The only writable state */
const sc_kernel_entry *sc_chosen = &sc_kernels[sc_compiled - 1];
char sc_note[64];

/* How many kernels of the table, from its start, this machine lacks */
int sc_unsupported() {
#if defined(__x86_64__)
    __builtin_cpu_init();
    if (!(__builtin_cpu_supports("avx2") && __builtin_cpu_supports("bmi") && __builtin_cpu_supports("popcnt")
          && __builtin_cpu_supports("pclmul")))
        return 1;
#endif
    return 0;
}

bool sc_same(const char *a, const char *b) {
    while (*a && *a == *b) {
        a++;
        b++;
    }
    return *a == *b;
}

/* ----------------------------------------------------------- the automaton */

#ifdef SC_TESTING
uint64_t sc_resume_count;
#endif

/* The definition of the language (2.4 of the specification; scEngine.h).
 * From the record start j->pos to the first record start at or after `stop',
 * or to an event.  STRICT: an irregular unit is an event. */
template <class U> void sc_automaton(sc_job *j, const sc_classes &k, uint64_t stop, bool strict) {
    enum { START, UNQUOTED, QUOTED, AFTER } state = START;
    const U *u = (const U *)j->units;
    const uint64_t limit = j->limit;
    const bool skip_blank = (j->flags & SC_F_SKIP_BLANK) != 0;
    uint64_t i = j->pos, fields = j->fields;

    j->done = i;
    j->done_fields = fields;
    j->event = SC_AT_END;
    while (i < limit) {
        const uint32_t c = u[i];
        switch (state) {
        case QUOTED:
            if (k.is_quote(c))
                state = AFTER;
            i++;
            continue;
        case START:
            if (k.is_quote(c)) {
                j->opening = i;
                state = QUOTED;
                i++;
                continue;
            }
            break;
        case AFTER:
            if (k.is_quote(c)) {
                state = QUOTED;
                i++;
                continue;
            }
            break;
        case UNQUOTED:
            break;
        }
        const bool ends = k.is_line_end(c);
        if (!ends && !k.is_separator(c)) {
            if (state == AFTER || (state == UNQUOTED && k.is_quote(c))) {
                if (strict) {
                    j->event = SC_IRREGULAR;
                    j->at = i;
                    break;
                }
            }
            state = UNQUOTED;
            i++;
            continue;
        }
        if (!(ends && skip_blank && i == j->done)) {
            if (fields == j->capacity) {
                j->event = SC_NO_SLOT;
                break;
            }
            if (j->dest)
                j->dest[fields] = (uint32_t)(i - j->base);
            fields++;
            state = START;
            if (!ends) {
                i++;
                continue;
            }
            j->records++;
            j->done_fields = fields;
        }
        i = j->done = k.after(u, i, limit);
        if (i >= stop)
            break;
    }
    j->pos = i;
    j->fields = fields;
    j->inside = state == QUOTED;
}

/* After a kernel's scan from the record start `start', when `fields' entries
 * were stored: where the open record starts */
void sc_find_done(sc_job *j, const sc_classes &k, uint64_t start, uint64_t fields) {
    const uint8_t *u = (const uint8_t *)j->units;
    uint64_t n = j->fields;

    while (n > fields && k.is_separator(u[j->base + j->dest[n - 1]]))
        n--;
    j->done_fields = n;
    j->done = n == fields ? start : k.after(u, j->base + j->dest[n - 1], j->limit);
}

/* The window of the automaton around an irregular unit, in units */
enum : uint64_t { SC_WINDOW_MIN = 1024, SC_WINDOW_MAX = 65536 };

/* The whole scan by the chosen kernel: SIMD where it is proved equal to the
 * automaton, the automaton around every irregular unit */
void sc_scan_bytes(sc_job *j, const sc_classes &k) {
    const bool strict = (j->flags & SC_F_STRICT) != 0;
    uint64_t window = 0;

    for (;;) {
        const uint64_t start = j->pos, fields = j->fields;
        if (j->dest) {
            sc_chosen->index(j);
            sc_find_done(j, k, start, fields);
        } else
            sc_chosen->measure(j);
        if (j->event != SC_IRREGULAR || strict)
            return;
        if (window == 0 || j->at - start >= window)
            window = SC_WINDOW_MIN;
        else if (window < SC_WINDOW_MAX)
            window *= 2;
        j->pos = j->done;
        j->fields = j->done_fields;
        sc_automaton<uint8_t>(j, k, j->done + window, false);
        /* at the end inside or after the window, or no slot: that is the answer */
        if (j->event != SC_AT_END || j->pos >= j->limit)
            return;
#ifdef SC_TESTING
        sc_resume_count++;
#endif
    }
}

template <class U> uint64_t sc_count_lines(const U *u, uint64_t from, uint64_t to, bool lf_only) {
    uint64_t n = 0;

    for (uint64_t i = from; i < to; i++)
        n += (u[i] == LF && !(i > from && !lf_only && u[i - 1] == CR)) || (u[i] == CR && !lf_only);
    return n;
}

/* records, min_fields and max_fields: one walk over the stored entries */
template <class U> void sc_count_records(const sc_job *j, const sc_classes &k, uint64_t stored, uint64_t to, sc_scan *out) {
    const U *u = (const U *)j->units;
    uint64_t records = 0, least = UINT64_MAX, most = 0, first = 0;

    for (uint64_t i = 0; i < stored; i++) {
        const uint64_t e = j->base + j->dest[i];
        if (e < to && k.is_separator(u[e]))
            continue;
        const uint64_t width = i + 1 - first;
        first = i + 1;
        records++;
        least = width < least ? width : least;
        most = width > most ? width : most;
    }
    out->records = records;
    out->min_fields = records ? least : 0;
    out->max_fields = most;
}

template <class U> void sc_run(const sc_input *in, const sc_dialect *d, sc_job *j, sc_scan *out) {
    const U *u = (const U *)in->units;
    const sc_classes k(d);
    const uint64_t to = in->to;
    const bool final = (d->flags & SC_F_FINAL) != 0, strict = (d->flags & SC_F_STRICT) != 0;
    const bool skip_blank = (d->flags & SC_F_SKIP_BLANK) != 0;
    /* a CR in the last position of a chunk: the LF of its pair may follow, so
     * the scan ends before it */
    const bool held = !final && !k.lf_only && to > in->from && u[to - 1] == CR && !k.is_separator(CR);
    /* the kernels need a unit for the separator and distinct needles */
    const bool simd = sizeof(U) == 1 && k.separator <= 255 && k.separator != LF && k.separator != CR
        && (k.no_quote || (k.quote != k.separator && k.quote != LF && k.quote != CR));

    j->units = u;
    j->limit = to - held;
    j->flags = d->flags;
    j->pos = j->done = in->from;
    j->fields = j->done_fields = j->records = 0;
    j->at = j->opening = 0;
    j->inside = false;
    if (simd) {
        const uint8_t flip = (uint8_t)(d->quote ^ '"');
        const uint64_t ones = ~(uint64_t)0;
        j->needles.flip = flip;
        j->needles.separator = (uint8_t)(d->separator ^ flip);
        j->needles.lf = (uint8_t)(LF ^ flip);
        j->needles.cr = (uint8_t)(CR ^ flip);
        j->needles.quoting = k.no_quote ? 0 : ones;
        j->needles.cr_ends = k.lf_only ? 0 : ones;
        j->needles.skip_blank = skip_blank ? ones : 0;
        sc_scan_bytes(j, k);
    } else
        sc_automaton<U>(j, k, UINT64_MAX, strict);

    /* the blank lines before the open record */
    if (skip_blank)
        while (j->done < j->limit && k.is_line_end(u[j->done]))
            j->done++;
    if (j->event == SC_AT_END) {
        if (final && j->done < to) {
            if (j->inside && strict) {
                /* where the quote opened: the automaton on the open record */
                sc_job open = *j;
                open.dest = NULL;
                open.capacity = UINT64_MAX;
                open.pos = j->done;
                sc_automaton<U>(&open, k, UINT64_MAX, true);
                out->code = SC_E_UNTERMINATED_QUOTE;
                j->event = SC_IRREGULAR;
                j->at = open.opening;
            } else if (j->fields == j->capacity)
                j->event = SC_NO_SLOT;
            else {
                if (j->dest)
                    j->dest[j->fields] = (uint32_t)(to - j->base);
                j->fields++;
                j->records++;
                j->done_fields = j->fields;
                j->done = to;
            }
        } else if (held && !j->inside && !(skip_blank && j->done == j->limit) && j->fields == j->capacity)
            j->event = SC_NO_SLOT;      /* the CR ends a field */
    }
    switch (j->event) {
    case SC_NO_SLOT:
        out->status = SC_MORE;
        break;
    case SC_IRREGULAR:
        out->status = SC_ERROR;
        out->position = j->at;
        if (out->code == SC_E_NONE)
            out->code = k.is_quote(u[j->at]) ? SC_E_UNEXPECTED_QUOTE : SC_E_TEXT_AFTER_QUOTE;
        break;
    default:
        out->status = SC_OK;
        break;
    }
    out->stored = j->done_fields;
    out->next = j->done;
    if (j->dest)
        sc_count_records<U>(j, k, out->stored, to, out);
    else
        out->records = j->records;
    if (d->flags & SC_F_LINES)
        out->lines = sizeof(U) == 1
            ? sc_chosen->lines((const uint8_t *)in->units, in->from, out->next, k.lf_only)
            : sc_count_lines<U>(u, in->from, out->next, k.lf_only);
}

void sc_run(const sc_input *in, const sc_dialect *d, sc_job *j, sc_scan *out) {
    if (in->wide)
        sc_run<uint32_t>(in, d, j, out);
    else
        sc_run<uint8_t>(in, d, j, out);
}

/* ------------------------------------------------------------------ fields */

sc_inline uint32_t sc_unit(const sc_input *in, uint64_t i) {
    return in->wide ? ((const uint32_t *)in->units)[i] : ((const uint8_t *)in->units)[i];
}

sc_inline uint64_t sc_load8(const uint8_t *p) {
    uint64_t w;
    memcpy(&w, p, 8);
    return w;
}

const uint64_t SC_HIGH = UINT64_C(0x8080808080808080), SC_LOW7 = UINT64_C(0x7f7f7f7f7f7f7f7f);

/* The first q at or after i, or end */
sc_inline uint64_t sc_find(const uint8_t *u, uint64_t i, uint64_t end, uint32_t q) {
    const uint64_t needle = UINT64_C(0x0101010101010101) * (q & 255);

    for (; end - i >= 8; i += 8) {
        const uint64_t x = sc_load8(u + i) ^ needle, hit = ~(((x & SC_LOW7) + SC_LOW7) | x | SC_LOW7);
        if (hit)
            return i + ((uint64_t)__builtin_ctzll(hit) >> 3);
    }
    while (i < end && u[i] != q)
        i++;
    return i;
}
sc_inline uint64_t sc_find(const uint32_t *u, uint64_t i, uint64_t end, uint32_t q) {
    while (i < end && u[i] != q)
        i++;
    return i;
}

/* The text of a quoted field is made of segments of the source: `qq' is q, the
 * next lone q closes, the rest is literal.  From p (after the opening quote,
 * or after a segment) to end; answers the end of the segment, *p is where the
 * next one starts, *more false after the last. */
template <class U> sc_inline uint64_t sc_segment(const U *u, uint64_t *p, uint64_t end, uint32_t q, bool *closed, bool *more) {
    if (*closed) {
        *more = false;
        return *p = end;
    }
    const uint64_t i = sc_find(u, *p, end, q);
    if (i == end) {             /* unterminated */
        *more = false;
        return *p = end;
    }
    if (i + 1 < end && u[i + 1] == q) {
        *p = i + 2;
        return i + 1;
    }
    *closed = true;
    *p = i + 1;
    return i;
}

/* The length of the UTF-8 sequence at p (p < e), 0 when it is not valid */
sc_inline unsigned sc_decode(const uint8_t *p, const uint8_t *e, uint32_t *point) {
    const uint32_t a = p[0];
    const uint64_t n = (uint64_t)(e - p);

    if (a < 0x80) {
        *point = a;
        return 1;
    }
    if (a < 0xC2)
        return 0;
    if (a < 0xE0) {
        if (n < 2 || (p[1] & 0xC0) != 0x80)
            return 0;
        *point = (a & 0x1F) << 6 | (p[1] & 0x3Fu);
        return 2;
    }
    if (a < 0xF0) {
        if (n < 3 || (p[1] & 0xC0) != 0x80 || (p[2] & 0xC0) != 0x80)
            return 0;
        *point = (a & 0x0F) << 12 | (p[1] & 0x3Fu) << 6 | (p[2] & 0x3Fu);
        return *point < 0x800 || (*point >= 0xD800 && *point <= 0xDFFF) ? 0 : 3;
    }
    if (a < 0xF5) {
        if (n < 4 || (p[1] & 0xC0) != 0x80 || (p[2] & 0xC0) != 0x80 || (p[3] & 0xC0) != 0x80)
            return 0;
        *point = (a & 0x07) << 18 | (p[1] & 0x3Fu) << 12 | (p[2] & 0x3Fu) << 6 | (p[3] & 0x3Fu);
        return *point < 0x10000 || *point > 0x10FFFF ? 0 : 4;
    }
    return 0;
}

/* What the units of a segment are: adds its characters to t->units and ORs
 * them into *any; 0 or SC_E_INVALID_UTF8 and t->position */
sc_inline int sc_measure_segment(const uint8_t *u, uint64_t a, uint64_t b, bool latin1, sc_text *t, uint32_t *any) {
    if (latin1) {
        t->units += b - a;
        return SC_E_NONE;
    }
    while (a < b) {
        uint32_t point;
        while (b - a >= 8 && !(sc_load8(u + a) & SC_HIGH)) {
            a += 8;
            t->units += 8;
        }
        if (a == b)
            break;
        const unsigned n = sc_decode(u + a, u + b, &point);
        if (n == 0) {
            t->position = a;
            return SC_E_INVALID_UTF8;
        }
        *any |= point;
        a += n;
        t->units++;
    }
    return SC_E_NONE;
}
sc_inline int sc_measure_segment(const uint32_t *u, uint64_t a, uint64_t b, bool, sc_text *t, uint32_t *any) {
    uint32_t all = 0;

    for (uint64_t i = a; i < b; i++)
        all |= u[i];
    *any |= all;
    t->units += b - a;
    return SC_E_NONE;
}

template <class D> sc_inline D *sc_copy_segment(const uint8_t *u, uint64_t a, uint64_t b, bool latin1, D *dest) {
    while (a < b) {
        uint32_t point;
        if (sizeof(D) == 1) {
            if (latin1) {
                memcpy(dest, u + a, b - a);
                return dest + (b - a);
            }
            while (b - a >= 8 && !(sc_load8(u + a) & SC_HIGH)) {
                memcpy(dest, u + a, 8);
                dest += 8;
                a += 8;
            }
            if (a == b)
                break;
        }
        const unsigned n = latin1 ? 0 : sc_decode(u + a, u + b, &point);
        if (n == 0) {           /* LATIN1 (or not what was measured) */
            *dest++ = (D)u[a++];
            continue;
        }
        *dest++ = (D)point;
        a += n;
    }
    return dest;
}
template <class D> sc_inline D *sc_copy_segment(const uint32_t *u, uint64_t a, uint64_t b, bool, D *dest) {
    while (a < b)
        *dest++ = (D)u[a++];
    return dest;
}

template <class U> void sc_measure_text(const sc_input *in, const sc_dialect *d, const sc_field *f, sc_text *t) {
    const U *u = (const U *)in->units;
    const sc_classes k(d);
    const bool latin1 = (d->flags & SC_F_LATIN1) != 0;
    uint32_t any = 0;

    t->kind = SC_EMPTY;
    t->code = SC_E_NONE;
    t->units = t->position = 0;
    t->start = f->start;
    t->end = f->end;
    t->quoted = 0;
    t->plain = 1;
    if (f->start >= f->end)
        return;
    if (!k.is_quote(u[f->start]))
        t->code = sc_measure_segment(u, t->start, t->end, latin1, t, &any);
    else {
        uint64_t p = f->start + 1, a, b;
        bool closed = false, more = true;

        t->quoted = 1;
        t->start = p;
        do {
            a = p;
            b = sc_segment(u, &p, f->end, k.quote, &closed, &more);
            /* plain: one segment, closed by the last unit or not at all */
            if (a == t->start && p == f->end && b == f->end - closed) {
                t->end = b;
                more = false;
            } else
                t->plain = 0;
            t->code = sc_measure_segment(u, a, b, latin1, t, &any);
        } while (more && t->code == SC_E_NONE);
    }
    if (t->code != SC_E_NONE) {
        t->units = 0;
        t->kind = SC_BYTES;
    } else if (t->units == 0)
        t->kind = SC_EMPTY_QUOTED;
    else
        t->kind = any > 0xFF ? SC_WIDE : SC_BYTES;
}

template <class U, class D> void sc_copy_text(const sc_input *in, const sc_dialect *d, const sc_text *t, D *dest) {
    const U *u = (const U *)in->units;
    const bool latin1 = (d->flags & SC_F_LATIN1) != 0;

    if (t->plain) {
        if (sizeof(U) == 1 && sizeof(D) == 1 && t->units == t->end - t->start)
            memcpy(dest, u + t->start, t->units);       /* LATIN1 or ASCII */
        else
            sc_copy_segment(u, t->start, t->end, latin1, dest);
        return;
    }
    uint64_t p = t->start;
    bool closed = false, more = true;
    do {
        const uint64_t a = p, b = sc_segment(u, &p, t->end, d->quote, &closed, &more);
        dest = sc_copy_segment(u, a, b, latin1, dest);
    } while (more);
}

/* ----------------------------------------------------------------- numbers */

/* The text of a number: the units of a field without its quotes, 0 when they
 * are not plain; blanks around it cut */
template <class U> sc_inline bool sc_number_text(const U *u, const sc_classes &k, const sc_field *f, uint64_t *a, uint64_t *b) {
    uint64_t s = f->start, e = f->end;

    if (s < e && k.is_quote(u[s])) {
        s++;
        const uint64_t i = sc_find(u, s, e, k.quote);
        if (i < e) {
            if (i != e - 1)
                return false;
            e--;
        }
    }
    while (s < e && (u[s] == ' ' || u[s] == '\t'))
        s++;
    while (s < e && (u[e - 1] == ' ' || u[e - 1] == '\t'))
        e--;
    *a = s;
    *b = e;
    return s < e;
}

template <class U> sc_inline bool sc_is_digit(U c) { return (uint32_t)c - '0' < 10; }

template <class U> int sc_integer(const sc_input *in, const sc_dialect *d, const sc_field *f, int64_t *value) {
    const U *u = (const U *)in->units;
    uint64_t a, b;
    int64_t n = 0;
    bool negative = false;

    if (!sc_number_text(u, sc_classes(d), f, &a, &b))
        return 0;
    if (u[a] == '+' || u[a] == '-')
        negative = u[a++] == '-';
    if (a == b)
        return 0;
    while (a < b && u[a] == '0')
        a++;
    if (b - a > 18)
        return 0;
    for (; a < b; a++) {
        if (!sc_is_digit(u[a]))
            return 0;
        n = n * 10 + (int64_t)((uint32_t)u[a] - '0');
    }
    *value = negative ? -n : n;
    return 1;
}

/* The exact powers of ten */
const double SC_TENS[23] = { 1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
                             1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22 };
const uint64_t SC_EXACT = (uint64_t)1 << 53;
enum { SC_EXPONENT_LIMIT = 99999 };

template <class U> int sc_float(const sc_input *in, const sc_dialect *d, const sc_field *f, int comma, double *value) {
    const U *u = (const U *)in->units;
    const U point = (U)(comma ? ',' : '.');
    uint64_t a, b, mantissa = 0;
    int64_t exponent = 0;       /* of ten: minus the digits after the point */
    unsigned significant = 0;
    bool negative = false, digits = false, fraction = false;

    if (!sc_number_text(u, sc_classes(d), f, &a, &b))
        return 0;
    if (u[a] == '+' || u[a] == '-')
        negative = u[a++] == '-';
    for (; a < b; a++) {
        if (!fraction && u[a] == point) {
            fraction = true;
            continue;
        }
        if (!sc_is_digit(u[a]))
            break;
        digits = true;
        exponent -= fraction;
        if (significant == 0 && u[a] == '0')
            continue;
        if (++significant > 19)
            return 0;
        mantissa = mantissa * 10 + ((uint32_t)u[a] - '0');
    }
    if (!digits)
        return 0;
    if (a < b) {
        int32_t explicit_exponent = 0;
        bool down = false;

        if (u[a] != 'e' && u[a] != 'E')
            return 0;
        a++;
        if (a < b && (u[a] == '+' || u[a] == '-'))
            down = u[a++] == '-';
        if (a == b)
            return 0;
        for (; a < b; a++) {
            if (!sc_is_digit(u[a]))
                return 0;
            explicit_exponent = explicit_exponent * 10 + (int32_t)((uint32_t)u[a] - '0');
            if (explicit_exponent >= SC_EXPONENT_LIMIT)
                return 0;       /* saturated */
        }
        exponent += down ? -explicit_exponent : explicit_exponent;
    }
    /* Clinger: an exact mantissa and an exact power, one rounding */
    if (mantissa > SC_EXACT || exponent < -22 || exponent > 37)
        return 0;
    for (; exponent > 22; exponent--) {
        mantissa *= 10;         /* no overflow: it was at most 2^53 */
        if (mantissa > SC_EXACT)
            return 0;
    }
    double x = (double)mantissa;
    x = exponent < 0 ? x / SC_TENS[-exponent] : x * SC_TENS[exponent];
    *value = negative ? -x : x;
    return 1;
}

}       /* namespace */

/* ---------------------------------------------------------------- interface */

extern "C" {

void sc_init(void) {
    const char *forced = getenv("SIMDCSV_FORCE_IMPLEMENTATION");
    const int first = sc_unsupported();

    sc_chosen = &sc_kernels[first];
    sc_note[0] = 0;
    if (forced == NULL || forced[0] == 0)
        return;
    for (int i = first; i < sc_compiled; i++)
        if (sc_same(forced, sc_kernels[i].name)) {
            sc_chosen = &sc_kernels[i];
            memcpy(sc_note, forced, strlen(forced) + 1);
            return;
        }
    memcpy(sc_note, "refused ", 8);
    size_t n = 0;
    for (; n < 55 && forced[n]; n++)
        sc_note[8 + n] = forced[n];
    sc_note[8 + n] = 0;
}

const char *sc_kernel(void) { return sc_chosen->name; }
const char *sc_forced_note(void) { return sc_note; }
int sc_kernel_count(void) { return sc_compiled - sc_unsupported(); }
const char *sc_kernel_name(int i) {
    const int first = sc_unsupported();
    return i < 0 || i >= sc_compiled - first ? NULL : sc_kernels[first + i].name;
}

#ifdef SC_TESTING
int sc_use_kernel(int i) {
    const int first = sc_unsupported();
    if (i < 0 || i >= sc_compiled - first)
        return 0;
    sc_chosen = &sc_kernels[first + i];
    return 1;
}
SC_API uint64_t sc_test_resumes(void);
uint64_t sc_test_resumes(void) { return sc_resume_count; }
#endif

void sc_measure(const sc_input *in, const sc_dialect *d, sc_scan *out) {
    sc_job j;

    memset(out, 0, sizeof *out);
    memset(&j, 0, sizeof j);
    j.capacity = UINT64_MAX;
    sc_run(in, d, &j, out);
}

void sc_index(const sc_input *in, const sc_dialect *d, uint64_t base, uint32_t *dest, uint64_t capacity, sc_scan *out) {
    sc_job j;
    uint32_t none;

    memset(out, 0, sizeof *out);
    if (in->to - base > SC_MAX_SPAN) {
        out->status = SC_TOO_LARGE;
        out->next = in->from;
        return;
    }
    memset(&j, 0, sizeof j);
    j.base = base;
    j.dest = capacity ? dest : &none;   /* never NULL, which is sc_measure */
    j.capacity = capacity;
    sc_run(in, d, &j, out);
}

int sc_cursor_open(sc_cursor *c, const sc_input *in, const sc_dialect *d, const uint32_t *index, uint64_t first, uint64_t end) {
    c->in = *in;
    c->d = *d;
    c->index = index;
    c->slot = first;
    c->end = end;
    c->pos = in->from;
    c->at_start = 1;
    if (first > end || in->from > in->to)
        return SC_E_BAD_INDEX;
    if (first > 0) {
        const sc_classes k(d);
        const uint64_t e = in->from + index[first - 1];
        if (e >= in->to || !k.is_line_end(sc_unit(in, e)))
            return SC_E_BAD_INDEX;
        c->pos = in->wide ? k.after((const uint32_t *)in->units, e, in->to) : k.after((const uint8_t *)in->units, e, in->to);
    }
    return SC_E_NONE;
}

int sc_cursor_next(sc_cursor *c, sc_field *f) {
    const sc_classes k(&c->d);
    const uint64_t to = c->in.to;

    if (c->at_start) {
        if (c->d.flags & SC_F_SKIP_BLANK)
            while (c->pos < to && k.is_line_end(sc_unit(&c->in, c->pos)))
                c->pos++;
        if (c->pos >= to)
            return SC_E_BAD_INDEX;
    }
    if (c->slot >= c->end)
        return SC_E_BAD_INDEX;
    const uint64_t e = c->in.from + c->index[c->slot];
    if (e < c->pos || e > to)
        return SC_E_BAD_INDEX;
    uint64_t next = to;
    int last = 1;
    if (e < to) {
        const uint32_t unit = sc_unit(&c->in, e);
        if (k.is_separator(unit)) {
            last = 0;
            next = e + 1;
        } else if (k.is_line_end(unit))
            next = c->in.wide ? k.after((const uint32_t *)c->in.units, e, to) : k.after((const uint8_t *)c->in.units, e, to);
        else
            return SC_E_BAD_INDEX;
    }
    f->start = c->pos;
    f->end = e;
    f->last = last;
    c->pos = next;
    c->at_start = last;
    c->slot++;
    return SC_E_NONE;
}

void sc_text_measure(const sc_input *in, const sc_dialect *d, const sc_field *f, sc_text *t) {
    if (in->wide)
        sc_measure_text<uint32_t>(in, d, f, t);
    else
        sc_measure_text<uint8_t>(in, d, f, t);
}

void sc_text_copy(const sc_input *in, const sc_dialect *d, const sc_text *t, void *dest) {
    if (t->units == 0)
        return;
    if (in->wide) {
        if (t->kind == SC_WIDE)
            sc_copy_text<uint32_t>(in, d, t, (uint32_t *)dest);
        else
            sc_copy_text<uint32_t>(in, d, t, (uint8_t *)dest);
    } else if (t->kind == SC_WIDE)
        sc_copy_text<uint8_t>(in, d, t, (uint32_t *)dest);
    else
        sc_copy_text<uint8_t>(in, d, t, (uint8_t *)dest);
}

int sc_to_integer(const sc_input *in, const sc_dialect *d, const sc_field *f, int64_t *value) {
    return in->wide ? sc_integer<uint32_t>(in, d, f, value) : sc_integer<uint8_t>(in, d, f, value);
}

int sc_to_float(const sc_input *in, const sc_dialect *d, const sc_field *f, int comma, double *value) {
    return in->wide ? sc_float<uint32_t>(in, d, f, comma, value) : sc_float<uint8_t>(in, d, f, comma, value);
}

}       /* extern "C" */
