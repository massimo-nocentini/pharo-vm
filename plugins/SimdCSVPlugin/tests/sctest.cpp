/* sctest.cpp -- the engine of the SimdCSVPlugin (src/engine) against a second
 * automaton, written here from its definition only (scEngine.h), on every
 * kernel of the machine and on 32-bit units.
 *
 *   sctest [--small] [--seed N] [SECTION...]    the sections below, default all but 4gib
 *   sctest --sections                           their names
 *   sctest --forced                             prints the kernel and the note of sc_init
 *
 * Compiled with -DSC_TESTING, as the engine.  No C++ library (the MSan lane).
 * The first difference is printed, and the program exits 1. */
#include "scEngine.h"
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#if defined(__unix__) || defined(__APPLE__)
# if !defined(__EMSCRIPTEN__)
#  include <sys/mman.h>
#  include <unistd.h>
#  define HAVE_MMAP 1
# endif
#endif

extern "C" SC_API uint64_t sc_test_resumes(void);

namespace {

bool small;
uint64_t seed = UINT64_C(88172645463325252);
uint64_t checks;

uint32_t rnd() {
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    return (uint32_t)(seed >> 20);
}
uint32_t rnd(uint32_t n) { return rnd() % n; }

/* what was being checked, for the report */
const uint8_t *now_bytes;
uint64_t now_count;
sc_dialect now_dialect;
uint64_t now_from, now_to, now_base, now_capacity;
const char *now_kernel = "";

[[noreturn]] void fail(const char *format, ...) {
    va_list args;
    va_start(args, format);
    printf("FAILED (%s): ", now_kernel);
    vprintf(format, args);
    va_end(args);
    printf("\n  separator 0x%x quote 0x%x flags %u from %llu to %llu base %llu capacity %llu\n  units:", now_dialect.separator,
           now_dialect.quote, now_dialect.flags, (unsigned long long)now_from, (unsigned long long)now_to,
           (unsigned long long)now_base, (unsigned long long)now_capacity);
    for (uint64_t i = 0; now_bytes && i < now_count && i < 2000; i++) {
        const uint8_t c = now_bytes[i];
        if (c == '\n')
            printf("\\n");
        else if (c == '\r')
            printf("\\r");
        else if (c >= 32 && c < 127)
            printf("%c", c);
        else
            printf("\\x%02x", c);
    }
    printf("\n");
    exit(1);
}

/* -------------------------------------------------------------- reference */

enum { MAX_UNITS = 70000 };

struct Reference {
    sc_scan scan;
    /* per field end, the ones of incomplete records too */
    uint64_t entries[MAX_UNITS + 2], starts[MAX_UNITS + 2];
    bool last[MAX_UNITS + 2];
};
Reference ref, ref2;

struct Units {
    const uint8_t *bytes;
    const uint32_t *words;
    uint32_t at(uint64_t i) const { return bytes ? bytes[i] : words[i]; }
};

bool is_separator(const sc_dialect &d, uint32_t c) { return c == d.separator; }
bool is_line(const sc_dialect &d, uint32_t c) {
    if (c == d.separator)
        return false;
    return c == '\n' || (c == '\r' && !(d.flags & SC_F_LF_ONLY));
}
bool is_quote(const sc_dialect &d, uint32_t c) { return !(d.flags & SC_F_NO_QUOTE) && c == d.quote; }
bool is_end(const sc_dialect &d, uint32_t c) { return is_separator(d, c) || is_line(d, c); }

/* One field from i: its end (a position with an end unit, or `to'), its first
 * irregular unit (or UINT64_MAX), and whether it is an open quoted field */
struct FieldEnd {
    uint64_t end, irregular;
    int code;
    bool open;
};
FieldEnd reference_field(const Units &u, uint64_t i, uint64_t to, const sc_dialect &d) {
    FieldEnd f = { 0, UINT64_MAX, 0, false };
    const uint64_t start = i;

    if (i < to && is_quote(d, u.at(i))) {
        bool closed = false;
        i++;
        while (i < to && !closed) {
            if (!is_quote(d, u.at(i)))
                i++;
            else if (i + 1 < to && is_quote(d, u.at(i + 1)))
                i += 2;
            else {
                closed = true;
                i++;
            }
        }
        if (!closed) {
            f.open = true;
            f.end = to;
            return f;
        }
        if (i < to && !is_end(d, u.at(i))) {
            f.irregular = i;
            f.code = SC_E_TEXT_AFTER_QUOTE;
        }
    }
    while (i < to && !is_end(d, u.at(i))) {
        if (is_quote(d, u.at(i)) && i > start && f.irregular == UINT64_MAX) {
            f.irregular = i;
            f.code = SC_E_UNEXPECTED_QUOTE;
        }
        i++;
    }
    f.end = i;
    return f;
}

uint64_t reference_lines(const Units &u, uint64_t from, uint64_t to, bool lf_only) {
    uint64_t lines = 0;
    for (uint64_t i = from; i < to; i++) {
        if (u.at(i) == '\r' && !lf_only)
            lines++;
        if (u.at(i) == '\n' && !(i > from && u.at(i - 1) == '\r' && !lf_only))
            lines++;
    }
    return lines;
}

/* The whole scan, record after record */
void reference(Reference *r, const Units &u, uint64_t from, uint64_t to, const sc_dialect &d, uint64_t base, uint64_t capacity) {
    const bool final = d.flags & SC_F_FINAL, strict = d.flags & SC_F_STRICT, skip = d.flags & SC_F_SKIP_BLANK;
    const bool lf_only = d.flags & SC_F_LF_ONLY;
    sc_scan *s = &r->scan;
    uint64_t pos = from, n = 0;

    memset(s, 0, sizeof *s);
    s->min_fields = UINT64_MAX;
    for (;;) {
        /* a CR that may be the half of a CRLF */
        const uint64_t passable = !final && !lf_only && to > from && u.at(to - 1) == '\r' && !is_separator(d, '\r') ? to - 1 : to;
        while (skip && pos < passable && is_line(d, u.at(pos)))
            pos++;
        s->next = pos;
        s->stored = n;
        if (pos == to || (skip && is_line(d, u.at(pos))))
            break;
        uint64_t width = 0;
        bool complete = false;
        while (!complete) {
            const FieldEnd f = reference_field(u, pos, to, d);
            if (strict && f.irregular != UINT64_MAX) {
                s->status = SC_ERROR;
                s->code = f.code;
                s->position = f.irregular;
                goto stop;
            }
            if (f.end == to && !final)
                goto stop;
            if (f.end == to && f.open && strict) {
                s->status = SC_ERROR;
                s->code = SC_E_UNTERMINATED_QUOTE;
                s->position = pos;
                goto stop;
            }
            if (n + width == capacity) {
                s->status = SC_MORE;
                goto stop;
            }
            r->starts[n + width] = pos;
            r->entries[n + width] = f.end - base;
            r->last[n + width] = f.end == to || !is_separator(d, u.at(f.end));
            width++;
            if (f.end == to) {
                pos = to;
                complete = true;
            } else if (is_separator(d, u.at(f.end)))
                pos = f.end + 1;
            else {
                if (u.at(f.end) == '\r' && f.end == to - 1 && !final)
                    goto stop;
                pos = f.end + 1;
                if (u.at(f.end) == '\r' && pos < to && u.at(pos) == '\n' && is_line(d, '\n'))
                    pos++;
                complete = true;
            }
        }
        n += width;
        s->records++;
        if (width < s->min_fields)
            s->min_fields = width;
        if (width > s->max_fields)
            s->max_fields = width;
    }
stop:
    if (s->records == 0)
        s->min_fields = 0;
    if (d.flags & SC_F_LINES)
        s->lines = reference_lines(u, from, s->next, lf_only);
}

/* The text of the field [a, b), as positions of the source, by the automaton */
uint64_t text_positions[MAX_UNITS + 2];
uint64_t reference_text(const Units &u, uint64_t a, uint64_t b, const sc_dialect &d, bool *quoted, bool *plain) {
    uint64_t n = 0;

    *quoted = a < b && is_quote(d, u.at(a));
    *plain = true;
    if (!*quoted) {
        for (uint64_t i = a; i < b; i++)
            text_positions[n++] = i;
        return n;
    }
    enum { IN, AFTER, OUT } state = IN;
    for (uint64_t i = a + 1; i < b; i++) {
        const bool q = is_quote(d, u.at(i));
        if (state == IN && q)
            state = AFTER;
        else if (state == AFTER && q) {
            state = IN;
            *plain = false;
            text_positions[n++] = i;
        } else {
            if (state == AFTER) {
                state = OUT;
                *plain = false;
            }
            text_positions[n++] = i;
        }
    }
    return n;
}

/* The UTF-8 sequence of b[0, n): its length, 0 when it is not valid */
int reference_utf8(const uint8_t *b, uint64_t n, uint32_t *point) {
    for (int size = 1; size <= 4; size++) {
        static const uint32_t least[] = { 0, 0, 0x80, 0x800, 0x10000 };
        static const int lead_bits[] = { 0, 7, 5, 4, 3 };
        const uint32_t lead = size == 1 ? 0 : (0xF00u >> size) & 0xFF;
        if ((uint32_t)b[0] >> lead_bits[size] != lead >> lead_bits[size])
            continue;
        if (n < (uint64_t)size)
            return 0;
        uint32_t v = b[0] & ((1u << lead_bits[size]) - 1);
        for (int i = 1; i < size; i++) {
            if (b[i] >> 6 != 2)
                return 0;
            v = v * 64 + (b[i] & 63);
        }
        if (v < least[size] || v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF))
            return 0;
        *point = v;
        return size;
    }
    return 0;
}

/* sc_text_measure and sc_text_copy on one field */
void check_text(const sc_input &in, const sc_dialect &d, const sc_field &f) {
    static uint32_t points[MAX_UNITS + 2];
    const Units u = { in.wide ? NULL : (const uint8_t *)in.units, in.wide ? (const uint32_t *)in.units : NULL };
    bool quoted, plain;
    const uint64_t n = reference_text(u, f.start, f.end, d, &quoted, &plain);
    uint64_t count = 0, bad = UINT64_MAX;
    uint32_t most = 0;

    for (uint64_t i = 0; i < n && bad == UINT64_MAX;) {
        uint32_t point = u.at(text_positions[i]);
        uint64_t size = 1;
        if (!in.wide && !(d.flags & SC_F_LATIN1)) {
            uint8_t b[4];
            uint64_t k = 0;             /* adjacent units of the source only */
            while (k < 4 && i + k < n && text_positions[i + k] == text_positions[i] + k) {
                b[k] = (uint8_t)u.at(text_positions[i + k]);
                k++;
            }
            size = (uint64_t)reference_utf8(b, k, &point);
            if (size == 0) {
                bad = text_positions[i];
                break;
            }
        }
        points[count++] = point;
        most = point > most ? point : most;
        i += size;
    }
    sc_text t;
    memset(&t, 0xAA, sizeof t);
    sc_text_measure(&in, &d, &f, &t);
    if (t.quoted != (int)quoted || t.plain != (int)plain)
        fail("text [%llu,%llu) quoted %d plain %d", (unsigned long long)f.start, (unsigned long long)f.end, t.quoted, t.plain);
    if (bad != UINT64_MAX) {
        if (t.code != SC_E_INVALID_UTF8 || t.position != bad || t.units != 0)
            fail("text [%llu,%llu) code %d position %llu, expected invalid UTF-8 at %llu", (unsigned long long)f.start,
                 (unsigned long long)f.end, t.code, (unsigned long long)t.position, (unsigned long long)bad);
        return;
    }
    const int kind = count == 0 ? (quoted ? SC_EMPTY_QUOTED : SC_EMPTY) : most > 0xFF ? SC_WIDE : SC_BYTES;
    if (t.code != 0 || t.kind != kind || t.units != count)
        fail("text [%llu,%llu) code %d kind %d units %llu, expected kind %d units %llu", (unsigned long long)f.start,
             (unsigned long long)f.end, t.code, t.kind, (unsigned long long)t.units, kind, (unsigned long long)count);
    if (plain && n && (t.start != text_positions[0] || t.end != text_positions[n - 1] + 1))
        fail("text [%llu,%llu) is said to be [%llu,%llu)", (unsigned long long)f.start, (unsigned long long)f.end,
             (unsigned long long)t.start, (unsigned long long)t.end);
    const uint64_t unit = kind == SC_WIDE ? 4 : 1;
    uint8_t *dest = (uint8_t *)malloc(count * unit + 8);        /* 4 guard bytes before and after */
    memset(dest, 0xC3, count * unit + 8);
    sc_text_copy(&in, &d, &t, dest + 4);
    for (uint64_t i = 0; i < 4; i++)
        if (dest[i] != 0xC3 || dest[4 + count * unit + i] != 0xC3)
            fail("sc_text_copy wrote outside of its %llu units", (unsigned long long)count);
    for (uint64_t i = 0; i < count; i++) {
        uint32_t got;
        if (kind == SC_WIDE)
            memcpy(&got, dest + 4 + 4 * i, 4);
        else
            got = dest[4 + i];
        if (got != points[i])
            fail("text [%llu,%llu) character %llu is 0x%x, expected 0x%x", (unsigned long long)f.start, (unsigned long long)f.end,
                 (unsigned long long)i, got, points[i]);
    }
    free(dest);
    checks++;
}

/* ------------------------------------------------------------------ checks */

const uint32_t CANARY = 0xC5C5C5C5;
enum { MARGIN = 20 };
uint32_t slots[MAX_UNITS + 2 * MARGIN + 2], *const dest = slots + MARGIN;
uint32_t wide[MAX_UNITS + 2];

void compare(const char *what, const sc_scan &got, const sc_scan &want) {
    if (got.status != want.status || got.code != want.code || got.stored != want.stored || got.records != want.records
        || got.next != want.next || got.lines != want.lines || got.min_fields != want.min_fields
        || got.max_fields != want.max_fields || got.position != want.position)
        fail("%s: status %d code %d stored %llu records %llu next %llu lines %llu min %llu max %llu position %llu,\n"
             "  expected status %d code %d stored %llu records %llu next %llu lines %llu min %llu max %llu position %llu", what,
             got.status, got.code, (unsigned long long)got.stored, (unsigned long long)got.records, (unsigned long long)got.next,
             (unsigned long long)got.lines, (unsigned long long)got.min_fields, (unsigned long long)got.max_fields,
             (unsigned long long)got.position, want.status, want.code, (unsigned long long)want.stored,
             (unsigned long long)want.records, (unsigned long long)want.next, (unsigned long long)want.lines,
             (unsigned long long)want.min_fields, (unsigned long long)want.max_fields, (unsigned long long)want.position);
}

void check_index(const sc_input &in, const sc_dialect &d, uint64_t base, uint64_t capacity, const Reference &want) {
    sc_scan s;

    for (int i = 0; i < MARGIN; i++)
        slots[i] = dest[capacity + (uint64_t)i] = CANARY;
    memset(&s, 0xAA, sizeof s);
    sc_index(&in, &d, base, dest, capacity, &s);
    compare("sc_index", s, want.scan);
    for (uint64_t i = 0; i < s.stored; i++)
        if (dest[i] != want.entries[i])
            fail("entry %llu is %u, expected %llu", (unsigned long long)i, dest[i], (unsigned long long)want.entries[i]);
    for (int i = 0; i < MARGIN; i++)
        if (slots[i] != CANARY || dest[capacity + (uint64_t)i] != CANARY)
            fail("sc_index wrote outside of its slots");
    checks++;
}

void check_measure(const sc_input &in, const sc_dialect &d, const Reference &want) {
    sc_scan s, w = want.scan;

    w.min_fields = w.max_fields = 0;
    memset(&s, 0xAA, sizeof s);
    sc_measure(&in, &d, &s);
    compare("sc_measure", s, w);
    checks++;
}

void check_cursor(const sc_input &from, const sc_dialect &d, const uint32_t *index, const Reference &want, bool texts) {
    sc_cursor c;
    sc_field f;
    const uint64_t stored = want.scan.stored;

    if (sc_cursor_open(&c, &from, &d, index, 0, stored) != 0)
        fail("sc_cursor_open(0)");
    for (uint64_t i = 0; i < stored; i++) {
        if (sc_cursor_next(&c, &f) != 0)
            fail("sc_cursor_next refuses slot %llu", (unsigned long long)i);
        if (f.start != want.starts[i] || f.end != from.from + want.entries[i] || f.last != (int)want.last[i])
            fail("field %llu is [%llu,%llu) last %d", (unsigned long long)i, (unsigned long long)f.start, (unsigned long long)f.end,
                 f.last);
        if (texts)
            check_text(from, d, f);
    }
    if (sc_cursor_next(&c, &f) != SC_E_BAD_INDEX)
        fail("sc_cursor_next goes past the last slot");
    for (uint64_t first = 1; first <= stored; first++) {        /* open after every slot */
        const int rc = sc_cursor_open(&c, &from, &d, index, first, stored);
        const uint64_t e = from.from + want.entries[first - 1];
        const int expected = e < from.to && want.last[first - 1] ? 0 : SC_E_BAD_INDEX;
        if (rc != expected)
            fail("sc_cursor_open(%llu) answers %d", (unsigned long long)first, rc);
        if (rc == 0 && first < stored && (sc_cursor_next(&c, &f) != 0 || f.start != want.starts[first]))
            fail("sc_cursor_next after sc_cursor_open(%llu)", (unsigned long long)first);
    }
    checks++;
}

enum { DEEP = 1, NO_WIDE = 2, EXACT = 4 };

/* bytes[from, to) on every kernel and on words, against the reference */
void check(const uint8_t *bytes, uint64_t from, uint64_t to, const sc_dialect &d, uint64_t base, uint64_t capacity, int how) {
    const Units u = { bytes, NULL };

    now_bytes = bytes;
    now_count = to;
    now_dialect = d;
    now_from = from;
    now_to = to;
    now_base = base;
    now_capacity = capacity;
    now_kernel = "reference";
    if (to > MAX_UNITS || capacity > MAX_UNITS + 1)
        fail("the test's own limits");
    reference(&ref, u, from, to, d, base, capacity);
    reference(&ref2, u, from, to, d, base, UINT64_MAX);
    const int kernels = sc_kernel_count();
    sc_input in = { bytes, 0, from, to };
    for (int k = 0; k < kernels; k++) {
        if (!sc_use_kernel(k))
            fail("sc_use_kernel(%d)", k);
        now_kernel = sc_kernel();
        check_index(in, d, base, capacity, ref);
        check_measure(in, d, ref2);
    }
    if (how & EXACT) {          /* nothing else around it: for ASan */
        uint8_t *copy = (uint8_t *)malloc(to - from ? to - from : 1);
        memcpy(copy, bytes + from, to - from);
        sc_input alone = { copy, 0, 0, to - from };
        uint32_t *their = (uint32_t *)malloc(capacity ? 4 * capacity : 1);
        sc_scan s, m;
        for (int k = 0; k < kernels; k++) {
            sc_use_kernel(k);
            now_kernel = sc_kernel();
            sc_index(&alone, &d, 0, their, capacity, &s);
            sc_measure(&alone, &d, &m);
            if (s.stored != ref.scan.stored || s.next + from != ref.scan.next || m.stored != ref2.scan.stored)
                fail("alone: stored %llu", (unsigned long long)s.stored);
        }
        free(their);
        free(copy);
    }
    if (how & DEEP) {
        now_kernel = "cursor";
        sc_input all = { bytes, 0, base, to };
        if (base == from)
            check_cursor(all, d, dest, ref, true);
    }
    if (!(how & NO_WIDE)) {
        now_kernel = "wide";
        for (uint64_t i = 0; i < to; i++)
            wide[i] = bytes[i];
        in.units = wide;
        in.wide = 1;
        check_index(in, d, base, capacity, ref);
        check_measure(in, d, ref2);
        if ((how & DEEP) && base == from) {
            now_kernel = "wide cursor";
            sc_input all = { wide, 1, base, to };
            check_cursor(all, d, dest, ref, true);
        }
    }
}

void check_all(const uint8_t *bytes, uint64_t n, const sc_dialect &d, int how) { check(bytes, 0, n, d, 0, n + 1, how); }

sc_dialect dialect(uint32_t separator, int quote, uint32_t flags) {
    sc_dialect d = { separator, (uint8_t)quote, flags };
    return d;
}

/* --------------------------------------------------------------- sections */

/* {a , " LF CR}^0..size at the offsets 0, 60, 125 (the last in a quoted field) */
void exhaustive(int size, const uint8_t alphabet[5], const sc_dialect *dialects, int count, int how) {
    static const int offsets[] = { 0, 60, 125 };
    uint8_t b[200];

    for (int o = 0; o < 3; o++)
        for (int length = 0; length <= size; length++) {
            const int off = offsets[o];
            long combinations = 1;
            for (int i = 0; i < length; i++)
                combinations *= 5;
            memset(b, 'x', sizeof b);
            if (off) {
                b[off - 2] = alphabet[1];
                b[off - 1] = o == 1 ? alphabet[1] : alphabet[2];
            }
            for (long v = 0; v < combinations; v++) {
                long t = v;
                for (int i = 0; i < length; i++, t /= 5)
                    b[off + i] = alphabet[t % 5];
                for (int i = 0; i < count; i++)
                    check_all(b, (uint64_t)(off + length), dialects[i], how);
            }
        }
}

const uint8_t ALPHABET[5] = { 'a', ',', '"', '\n', '\r' };

void exhaustive_lenient() {
    const sc_dialect d[] = { dialect(',', '"', SC_F_FINAL), dialect(',', '"', SC_F_LINES) };
    exhaustive(small ? 6 : 8, ALPHABET, d, 2, 0);
}
void exhaustive_strict() {
    const sc_dialect d[] = { dialect(',', '"', SC_F_STRICT | SC_F_FINAL), dialect(',', '"', SC_F_STRICT) };
    exhaustive(small ? 5 : 8, ALPHABET, d, 2, NO_WIDE);
}
void exhaustive_lf_only() {
    const sc_dialect d[] = { dialect(',', '"', SC_F_LF_ONLY | SC_F_FINAL | SC_F_LINES), dialect(',', '"', SC_F_LF_ONLY | SC_F_STRICT) };
    exhaustive(small ? 5 : 8, ALPHABET, d, 2, NO_WIDE);
}
void exhaustive_skip() {
    const sc_dialect d[] = { dialect(',', '"', SC_F_SKIP_BLANK | SC_F_FINAL | SC_F_LINES), dialect(',', '"', SC_F_SKIP_BLANK | SC_F_LINES),
                             dialect(',', '"', SC_F_SKIP_BLANK | SC_F_STRICT | SC_F_FINAL) };
    exhaustive(small ? 5 : 8, ALPHABET, d, 3, NO_WIDE);
}

/* every combination of the flags, and other units, ^0..6 with the cursor and
 * the texts */
void exhaustive_dialects() {
    for (uint32_t f = 0; f < 32; f++) {
        const uint32_t flags = (f & 1 ? (uint32_t)(SC_F_STRICT) : 0u) | (f & 2 ? (uint32_t)(SC_F_LF_ONLY) : 0u) | (f & 4 ? (uint32_t)(SC_F_NO_QUOTE) : 0u)
            | (f & 8 ? (uint32_t)(SC_F_SKIP_BLANK) : 0u) | (f & 16 ? (uint32_t)(SC_F_FINAL) : 0u) | SC_F_LINES;
        const sc_dialect d = dialect(',', '"', flags);
        exhaustive(small ? 4 : 6, ALPHABET, &d, 1, DEEP);
    }
    static const struct { uint8_t separator, quote; } units[] = { { ';', '\'' }, { '\t', '"' }, { 0, '"' }, { 0xFF, 0 }, { 'x', 0xFF },
        { '"', '\'' }, { '#', '"' ^ 1 }, { 0xFE, 0xFF } };
    for (unsigned i = 0; i < sizeof units / sizeof units[0]; i++) {
        const uint8_t alphabet[5] = { 'a', units[i].separator, units[i].quote, '\n', '\r' };
        static const uint32_t flags[] = { SC_F_FINAL | SC_F_LATIN1, SC_F_STRICT | SC_F_FINAL | SC_F_LATIN1, SC_F_SKIP_BLANK | SC_F_LATIN1,
                                          SC_F_LF_ONLY | SC_F_LATIN1 };
        for (int j = 0; j < 4; j++) {
            const sc_dialect d = dialect(units[i].separator, units[i].quote, flags[j]);
            exhaustive(small ? 4 : 6, alphabet, &d, 1, DEEP);
        }
    }
}

uint32_t random_flags();

/* Dialects that the glue refuses (a separator or a quote that is a line end or
 * the other one, a separator over 255 for bytes): what they mean is nobody's
 * business, but no kernel may be given them, so the answers are the same
 * everywhere, inside of the bounds */
void degenerate() {
    static const struct { uint32_t separator; uint8_t quote; } units[] = { { '"', '"' }, { '\n', '"' }, { '\r', '"' }, { ',', '\n' },
        { ',', '\r' }, { 0x12C, '"' }, { '\n', '\n' }, { '\r', '\r' }, { '\r', '\n' } };
    static uint8_t b[300];
    static uint32_t first[400], words[300];
    const int rounds = small ? 10000 : 300000;

    for (int round = 0; round < rounds; round++) {
        const int which = (int)rnd(9);
        const sc_dialect d = dialect(units[which].separator, units[which].quote, random_flags());
        const uint64_t n = rnd(rnd(4) ? 20 : 300), capacity = rnd(2) ? n + 1 : rnd((uint32_t)n + 1);
        for (uint64_t i = 0; i < n; i++)
            words[i] = b[i] = (uint8_t) "a,\"\n\r"[rnd(5)];
        sc_scan s[4], m[4];
        uint64_t stored = 0;
        now_bytes = b;
        now_count = n;
        now_dialect = d;
        now_capacity = capacity;
        for (int k = 0; k <= sc_kernel_count(); k++) {
            const bool wide_units = k == sc_kernel_count();
            const sc_input in = { wide_units ? (const void *)words : b, wide_units, 0, n };
            if (!wide_units)
                sc_use_kernel(k);
            now_kernel = wide_units ? "wide" : sc_kernel();
            dest[capacity] = CANARY;
            sc_index(&in, &d, 0, dest, capacity, &s[k]);
            sc_measure(&in, &d, &m[k]);
            if (dest[capacity] != CANARY || s[k].stored > capacity || s[k].next > n || m[k].next > n || s[k].status > SC_ERROR)
                fail("stored %llu next %llu", (unsigned long long)s[k].stored, (unsigned long long)s[k].next);
            if (k == 0) {
                stored = s[0].stored;
                memcpy(first, dest, 4 * stored);
                continue;
            }
            if (wide_units && which == 5) {     /* the separator is a byte's nowhere */
                sc_text t;
                sc_field f;
                sc_cursor c;
                sc_cursor_open(&c, &in, &d, dest, 0, s[k].stored);
                while (sc_cursor_next(&c, &f) == 0)
                    sc_text_measure(&in, &d, &f, &t);
            }
            compare("sc_index", s[k], s[0]);
            compare("sc_measure", m[k], m[0]);
            if (memcmp(first, dest, 4 * stored) != 0)
                fail("other entries");
            checks++;
        }
    }
}

/* The quote NUL with every tail: the zero padding is not data */
void nul_quote() {
    uint8_t b[200];
    for (int size = 1; size <= 130; size++)
        for (int at = 0; at < size; at++)
            for (int style = 0; style < 4; style++) {
                memset(b, 'x', sizeof b);
                if (at > 0)
                    b[at - 1] = ',';
                b[at] = 0;
                if (style == 1 && at + 1 < size)
                    b[size - 1] = 0;
                if (style == 2 && at + 2 < size)
                    b[at + 2] = '\n';
                if (style == 3 && at + 2 < size)
                    b[size - 2] = 0, b[size - 1] = 0;
                for (uint32_t f = 0; f < 4; f++)
                    check_all(b, (uint64_t)size,
                              dialect(',', 0, (f & 1 ? (uint32_t)(SC_F_STRICT) : 0u) | (f & 2 ? (uint32_t)(SC_F_FINAL) : 0u) | SC_F_LATIN1), DEEP);
            }
}

/* Every pair of bytes at the offsets 0..7, for every needle */
void pairs() {
    static const struct { uint8_t separator, quote; } needles[] = { { ',', '"' }, { '\t', '\'' }, { 0, 0xFF }, { 0xFF, 0x80 }, { 0x80, 0 },
        { 0x7F, '\v' } };
    uint8_t b[24];

    for (unsigned k = 0; k < (small ? 2u : 6u); k++)
        for (int offset = 0; offset < 8; offset++)
            for (int x = 0; x < 256; x++)
                for (int y = 0; y < 256; y++) {
                    memset(b, 'x', sizeof b);
                    b[offset] = (uint8_t)x;
                    b[offset + 1] = (uint8_t)y;
                    check_all(b, 16, dialect(needles[k].separator, needles[k].quote, SC_F_FINAL | SC_F_LATIN1 | (offset & 1 ? (uint32_t)(SC_F_STRICT) : 0u)),
                              NO_WIDE);
                }
    /* a needle followed by needle^1 all along a block, at every place */
    static uint8_t long_one[300];
    for (int needle = 0; needle < 256; needle++)
        for (int place = 0; place < 140; place++)
            for (int other = -1; other < 8; other++) {
                memset(long_one, needle ^ 0x55, sizeof long_one);
                long_one[place] = (uint8_t)needle;
                long_one[place + 1] = (uint8_t)(needle ^ (other < 0 ? 0 : 1 << other));
                if (needle != '\n' && needle != '\r')
                    check_all(long_one, 150, dialect((uint32_t)needle, needle == '"' ? '\'' : '"', SC_F_FINAL | SC_F_LATIN1), NO_WIDE);
                if (needle != ',' && needle != '\n' && needle != '\r')
                    check_all(long_one, 150, dialect(',', needle, SC_F_LATIN1), NO_WIDE);
            }
}

/* a random unit of a style */
uint8_t random_unit(int style, const sc_dialect &d) {
    const uint32_t r = rnd();
    const uint8_t special[6] = { (uint8_t)d.separator, d.quote, '\n', '\r', 'a', 0xC3 };
    switch (style) {
    case 0: return special[r % 5];
    case 1: return r % 30 ? 'y' : special[(r >> 8) % 5];
    case 2: return (uint8_t)r;
    case 3: return r % 3 ? 'z' : (r >> 4) % 6 ? (uint8_t)d.separator : (r >> 8) % 8 ? (uint8_t)'\n' : special[(r >> 11) % 5];
    case 4: return r % 400 ? (r >> 9) % 5 ? 'w' : (r >> 12) % 10 ? (uint8_t)d.separator : (uint8_t)'\n' : d.quote;
    case 5: return r % 2 ? special[(r >> 2) % 6] : (uint8_t)(r >> 8);
    default: return r % 7 ? (uint8_t)d.separator : special[(r >> 3) % 5];
    }
}

/* Regular units: quoted and unquoted fields, records of 1 to 8 fields */
uint64_t random_regular(uint8_t *b, uint64_t size, const sc_dialect &d, uint32_t irregular) {
    uint64_t n = 0;
    while (n + 64 < size) {
        const uint32_t fields = 1 + rnd(8);
        for (uint32_t f = 0; f < fields && n + 64 < size; f++) {
            if (f)
                b[n++] = (uint8_t)d.separator;
            const uint32_t length = rnd(12);
            if (rnd(4) == 0) {
                b[n++] = d.quote;
                for (uint32_t i = 0; i < length; i++)
                    switch (rnd(8)) {
                    case 0: b[n++] = d.quote; b[n++] = d.quote; break;
                    case 1: b[n++] = (uint8_t)d.separator; break;
                    case 2: b[n++] = '\n'; break;
                    case 3: b[n++] = '\r'; break;
                    default: b[n++] = 'q';
                    }
                b[n++] = d.quote;
            } else
                for (uint32_t i = 0; i < length; i++)
                    b[n++] = irregular && rnd(irregular) == 0 ? d.quote : (uint8_t)('a' + i);
        }
        const uint32_t r = rnd(6);
        if (r != 1)
            b[n++] = r < 2 ? '\r' : '\n';
        else {
            b[n++] = '\r';
            b[n++] = '\n';
        }
    }
    return n;
}

uint32_t random_flags() {
    uint32_t f = 0;
    if (rnd(3) == 0) f |= SC_F_STRICT;
    if (rnd(3) == 0) f |= SC_F_LF_ONLY;
    if (rnd(8) == 0) f |= SC_F_NO_QUOTE;
    if (rnd(3) == 0) f |= SC_F_SKIP_BLANK;
    if (rnd(2) == 0) f |= SC_F_FINAL;
    if (rnd(2) == 0) f |= SC_F_LINES;
    if (rnd(4) == 0) f |= SC_F_LATIN1;
    return f;
}

sc_dialect random_dialect() {
    static const uint8_t separators[] = { ',', ',', ',', ';', '\t', '|', 0, 0xFF, ' ' };
    static const uint8_t quotes[] = { '"', '"', '"', '\'', 0, 0xFE };
    sc_dialect d;
    do
        d = dialect(separators[rnd(9)], quotes[rnd(6)], random_flags());
    while (d.separator == d.quote);
    if (d.separator > 127 || d.quote > 127)
        d.flags |= SC_F_LATIN1;         /* the glue's rule for bytes */
    return d;
}

void random_ranges() {
    static uint8_t b[6000];
    const int rounds = small ? 20000 : 300000;

    for (int round = 0; round < rounds; round++) {
        const sc_dialect d = random_dialect();
        const int style = (int)rnd(8);
        const uint64_t size = rnd(20) ? rnd(700) : rnd(5000);
        uint64_t n = size;
        if (style == 7)
            n = random_regular(b, size, d, rnd(2) ? 0 : 1 + rnd(200));
        else
            for (uint64_t i = 0; i < size; i++)
                b[i] = random_unit(style, d);
        const uint64_t base = rnd(4) ? 0 : rnd((uint32_t)n + 1), from = rnd(2) ? base : base + rnd((uint32_t)(n - base) + 1);
        const uint64_t capacity = rnd(3) ? n + 1 : rnd((uint32_t)n + 2);
        check(b, from, n, d, base, capacity, EXACT | (n < 800 ? DEEP : NO_WIDE));
    }
}

#ifdef HAVE_MMAP
/* pages of their own, between two that cannot be read */
struct Guarded {
    uint8_t *first, *end;
    explicit Guarded(size_t size) {
        const size_t page = (size_t)sysconf(_SC_PAGESIZE);
        size = (size + page - 1) / page * page;
        uint8_t *m = (uint8_t *)mmap(NULL, size + 2 * page, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (m == MAP_FAILED || mprotect(m, page, PROT_NONE) || mprotect(m + page + size, page, PROT_NONE))
            fail("mmap");
        first = m + page;
        end = first + size;
    }
};

/* Lengths 0 to 520 at the start and at the end of the pages, every alignment;
 * the output's end at the end of its pages */
void guard_pages() {
    Guarded in(4096), out(16384);
    const int seeds = small ? 2 : 12;

    for (int s = 0; s < seeds; s++)
        for (uint64_t length = 0; length <= 520; length++) {
            for (int side = 0; side < 2; side++)
                for (uint64_t shift = 0; shift < (side ? 64u : 1u); shift += side && length > 80 ? 7 : 1) {
                    uint8_t *p = side ? in.first + shift : in.end - length;
                    sc_dialect d = random_dialect();
                    if (s < 2)
                        d.flags |= SC_F_LINES;
                    for (uint64_t i = 0; i < length; i++)
                        p[i] = random_unit(s % 7, d);
                    now_bytes = p;
                    now_count = length;
                    now_dialect = d;
                    const Units u = { p, NULL };
                    const uint64_t capacity = rnd(2) ? length + 1 : rnd((uint32_t)length + 1);
                    reference(&ref, u, 0, length, d, 0, capacity);
                    reference(&ref2, u, 0, length, d, 0, UINT64_MAX);
                    const sc_input input = { p, 0, 0, length };
                    for (int k = 0; k < sc_kernel_count(); k++) {
                        sc_use_kernel(k);
                        now_kernel = sc_kernel();
                        check_measure(input, d, ref2);
                        for (int place = 0; place < 2; place++) {
                            uint32_t *slot = place ? (uint32_t *)(void *)out.first : (uint32_t *)(void *)out.end - capacity;
                            sc_scan got;
                            sc_index(&input, &d, 0, slot, capacity, &got);
                            compare("sc_index", got, ref.scan);
                            for (uint64_t i = 0; i < got.stored; i++)
                                if (slot[i] != ref.entries[i])
                                    fail("entry %llu", (unsigned long long)i);
                        }
                    }
                    if (d.flags & SC_F_LATIN1 || s % 7 < 2) {
                        sc_cursor c;
                        sc_field f;
                        sc_index(&input, &d, 0, dest, length + 1, &ref2.scan);
                        sc_cursor_open(&c, &input, &d, dest, 0, ref2.scan.stored);
                        while (sc_cursor_next(&c, &f) == 0) {
                            sc_text t;
                            int64_t integer;
                            double number;
                            sc_text_measure(&input, &d, &f, &t);
                            uint8_t *copy = out.end - t.units * (t.kind == SC_WIDE ? 4 : 1);
                            if (t.units)
                                sc_text_copy(&input, &d, &t, copy);
                            sc_to_integer(&input, &d, &f, &integer);
                            sc_to_float(&input, &d, &f, 0, &number);
                        }
                    }
                }
        }
}
#endif

/* Every capacity from 0 to one more than the fields, in front of canaries */
void capacities() {
    static uint8_t b[2000];
    const int rounds = small ? 4 : 60;

    for (int round = 0; round < rounds; round++) {
        sc_dialect d = random_dialect();
        if (round % 3 == 0)
            d = dialect(',', '"', round % 2 ? (uint32_t)(SC_F_FINAL) : 0u);
        uint64_t n = 300 + rnd(900);
        if (round % 4 == 3)
            n = random_regular(b, n, d, 0);
        else
            for (uint64_t i = 0; i < n; i++)
                b[i] = random_unit(round % 4 ? 6 : 3, d);
        reference(&ref, Units{ b, NULL }, 0, n, d, 0, UINT64_MAX);
        const uint64_t fields = ref.scan.stored;
        for (uint64_t capacity = 0; capacity <= fields + 1; capacity++)
            check(b, 0, n, d, 0, capacity, NO_WIDE);
        for (uint64_t capacity = fields; capacity <= fields + 300; capacity++)
            check(b, 0, n, d, 0, capacity, NO_WIDE);
    }
}

/* The order of the events: an irregular unit and the capacity limit around
 * each other, everywhere in a block; a blank line ended by a CR at the end */
void event_order() {
    uint8_t b[700];

    for (int size = 300; size <= 600; size += 300)
        for (int at = 40; at < size - 8; at += small ? 13 : 1)
            for (int delta = -2; delta <= 2; delta++)
                for (int kind = 0; kind < 3; kind++) {
                    memset(b, ',', sizeof b);
                    for (int i = 7; i < size; i += 16)
                        b[i] = '\n';
                    const int irregular = at + delta;
                    if (kind == 0) {            /* a quote in an unquoted field */
                        b[irregular - 1] = 'a';
                        b[irregular] = '"';
                    } else if (kind == 1) {     /* text after a closing quote */
                        b[irregular - 3] = ',';
                        b[irregular - 2] = b[irregular - 1] = '"';
                        b[irregular] = 'a';
                    } else {                    /* unterminated */
                        b[irregular - 1] = ',';
                        b[irregular] = '"';
                    }
                    /* the entries up to `at' in the source without the change: at most at + 1 */
                    for (int capacity = at - 6; capacity <= at + 3; capacity++)
                        for (uint32_t f = 0; f < 4; f++)
                            check(b, 0, (uint64_t)size,
                                  dialect(',', '"', (f & 1 ? (uint32_t)(SC_F_STRICT) : 0u) | (f & 2 ? (uint32_t)(SC_F_FINAL) : 0u)), 0, (uint64_t)capacity, NO_WIDE);
                }
    /* line ends at the end of a chunk */
    static const char *const ends[] = { "\r", "\n\r", "\r\r", "\r\n", "\r\n\r", "\n\n", "\r\r\n", "\"\r", "\"\r\r", ",\r", "a\r", "\"\"\r", "a\r\r", "a\r\n\r" };
    for (int prefix = 0; prefix < 140; prefix++)
        for (unsigned e = 0; e < sizeof ends / sizeof ends[0]; e++)
            for (uint32_t f = 0; f < 32; f++) {
                const size_t length = strlen(ends[e]);
                memset(b, 'x', sizeof b);
                for (int i = 4; i < prefix; i += 5)
                    b[i] = i % 10 == 9 ? '\n' : ',';
                memcpy(b + prefix, ends[e], length);
                const uint32_t flags = (f & 1 ? (uint32_t)(SC_F_STRICT) : 0u) | (f & 2 ? (uint32_t)(SC_F_LF_ONLY) : 0u) | (f & 4 ? (uint32_t)(SC_F_SKIP_BLANK) : 0u)
                    | (f & 8 ? (uint32_t)(SC_F_FINAL) : 0u) | (f & 16 ? (uint32_t)(SC_F_LINES) : 0u);
                const uint64_t n = (uint64_t)prefix + length;
                for (uint64_t capacity = 0; capacity < (prefix < 30 ? n + 1 : 3); capacity++)
                    check(b, 0, n, dialect(',', '"', flags), 0, capacity, NO_WIDE);
                check_all(b, n, dialect(',', '"', flags), DEEP);
            }
}

/* Two calls, the second one from `next': as one call */
void check_split(const uint8_t *b, uint64_t n, uint64_t split, const sc_dialect &d) {
    static uint32_t second[MAX_UNITS + 2], whole[MAX_UNITS + 2];
    sc_dialect final = d, open = d;
    sc_scan w, s1, s2, m1, m2;

    final.flags |= SC_F_FINAL | SC_F_LINES;
    open.flags |= SC_F_LINES;
    open.flags &= ~(uint32_t)SC_F_FINAL;
    now_bytes = b;
    now_count = n;
    now_dialect = d;
    now_from = 0;
    now_to = n;
    now_base = split;
    now_kernel = sc_kernel();
    const sc_input all = { b, 0, 0, n }, left = { b, 0, 0, split };
    sc_index(&all, &final, 0, whole, n + 1, &w);
    sc_index(&left, &open, 0, dest, n + 1, &s1);
    sc_measure(&left, &open, &m1);
    if (m1.stored != s1.stored || m1.next != s1.next)
        fail("split %llu: measure", (unsigned long long)split);
    if (s1.status == SC_ERROR) {
        if (w.status != SC_ERROR || w.position != s1.position || w.next != s1.next || w.lines != s1.lines || w.stored != s1.stored)
            fail("split %llu: the error of the first call", (unsigned long long)split);
        return;
    }
    if (s1.status != SC_OK || s1.next > split)
        fail("split %llu: status %d", (unsigned long long)split, s1.status);
    const sc_input right = { b, 0, s1.next, n };
    sc_index(&right, &final, s1.next, second, n + 1, &s2);
    sc_measure(&right, &final, &m2);
    if (m2.stored != s2.stored || m2.next != s2.next || m2.status != s2.status)
        fail("split %llu: measure", (unsigned long long)split);
    if (s1.stored + s2.stored != w.stored || s1.records + s2.records != w.records || s2.next != w.next || s2.status != w.status
        || s2.code != w.code || s2.position != w.position || s1.lines + s2.lines != w.lines)
        fail("split %llu: stored %llu + %llu of %llu records %llu + %llu of %llu next %llu of %llu lines %llu + %llu of %llu",
             (unsigned long long)split, (unsigned long long)s1.stored, (unsigned long long)s2.stored, (unsigned long long)w.stored,
             (unsigned long long)s1.records, (unsigned long long)s2.records, (unsigned long long)w.records,
             (unsigned long long)s2.next, (unsigned long long)w.next, (unsigned long long)s1.lines, (unsigned long long)s2.lines,
             (unsigned long long)w.lines);
    for (uint64_t i = 0; i < w.stored; i++)
        if (whole[i] != (i < s1.stored ? dest[i] : s1.next + second[i - s1.stored]))
            fail("split %llu: entry %llu", (unsigned long long)split, (unsigned long long)i);
    checks++;
}

void splits() {
    static uint8_t b[800];
    const int rounds = small ? 40 : 1500;

    for (int round = 0; round < rounds; round++) {
        sc_dialect d = random_dialect();
        const int style = round % 9;
        uint64_t n = rnd(round % 5 ? 120 : 700);
        if (style == 7 || style == 8)
            n = random_regular(b, 100 + n, d, style == 7 ? 0 : 40);
        else
            for (uint64_t i = 0; i < n; i++)
                b[i] = random_unit(style, d);
        for (int k = 0; k < sc_kernel_count(); k++) {
            sc_use_kernel(k);
            for (uint64_t split = 0; split <= n; split++)
                check_split(b, n, split, d);
        }
    }
}

/* Irregular quotes every k bytes, and the SIMD taking over again after them */
void irregular() {
    static uint8_t b[60000];
    const int step = small ? 331 : 1;
    const uint64_t n = small ? 12000 : 20000;

    for (int k = 1; k <= 5000; k += k < 200 && !small ? 1 : k < 50 ? 1 : step * 7) {
        for (uint64_t i = 0; i < n; i++)
            b[i] = i % 41 == 40 ? '\n' : i % 8 == 7 ? ',' : 'a';
        for (uint64_t i = 3; i < n; i += (uint64_t)k)
            b[i] = '"';
        for (uint32_t f = 0; f < 4; f++)
            check_all(b, n, dialect(',', '"', (f & 1 ? (uint32_t)(SC_F_FINAL) : 0u) | (f & 2 ? (uint32_t)(SC_F_SKIP_BLANK) : 0u)), NO_WIDE);
    }
    /* blank lines (no entries) longer than the largest window before an irregular unit */
    for (uint32_t f = 0; f < 4; f++) {
        static uint8_t blank[MAX_UNITS];
        memset(blank, f & 2 ? '\r' : '\n', sizeof blank);
        memcpy(blank + sizeof blank - 20, "a\"b,c\nd,e\nf\"\"\ng\n\n\nh", 20);
        check_all(blank, sizeof blank, dialect(',', '"', SC_F_SKIP_BLANK | SC_F_LINES | (f & 1 ? (uint32_t)(SC_F_FINAL) : 0u)), NO_WIDE);
        memcpy(blank, "a\n\n", 3);
        check_all(blank, sizeof blank, dialect(',', '"', SC_F_SKIP_BLANK | SC_F_LINES | (f & 1 ? (uint32_t)(SC_F_FINAL) : 0u)), NO_WIDE);
    }
    /* one irregular unit in 50,000: one resume; with one in every 8 KB: one each */
    for (int k = 0; k < sc_kernel_count(); k++) {
        sc_use_kernel(k);
        now_kernel = sc_kernel();
        for (int each = 1; each <= 6; each++) {
            for (uint64_t i = 0; i < 50000; i++)
                b[i] = i % 41 == 40 ? '\n' : i % 8 == 7 ? ',' : 'a';
            for (int i = 0; i < each; i++)
                b[1000 + 8000 * i + 1] = '"';
            const sc_input in = { b, 0, 0, 50000 };
            const sc_dialect d = dialect(',', '"', 0);
            sc_scan s;
            uint64_t before = sc_test_resumes();
            sc_index(&in, &d, 0, dest, 50000, &s);
            if (sc_test_resumes() - before != (uint64_t)each)
                fail("%d irregular units, %llu resumes (sc_index)", each, (unsigned long long)(sc_test_resumes() - before));
            before = sc_test_resumes();
            sc_measure(&in, &d, &s);
            if (sc_test_resumes() - before != (uint64_t)each)
                fail("%d irregular units, %llu resumes (sc_measure)", each, (unsigned long long)(sc_test_resumes() - before));
        }
        /* the windows: 1 KiB for the first hit (0 to 1025), 2 KiB for one 165
         * units later (1189 to 3239), then for one 2,051 units later 1 KiB
         * again (5289 to 6314, and 6355 is another hit), for one 2,010 units
         * later 4 KiB (5248 to 9348) */
        for (int late = 0; late < 2; late++) {
            const int places[] = { 1, 1190, late ? 5290 : 5249, 6356 };
            for (int i = 0; i < 50000; i++)
                b[i] = i % 41 == 40 ? '\n' : i % 8 == 7 ? ',' : 'a';
            for (int i = 0; i < 4; i++) {
                b[places[i] - 1] = 'a';
                b[places[i]] = '"';
            }
            const sc_input windows = { b, 0, 0, 50000 };
            const sc_dialect lenient = dialect(',', '"', 0);
            sc_scan got;
            const uint64_t resumes = sc_test_resumes();
            sc_index(&windows, &lenient, 0, dest, 50000, &got);
            if (sc_test_resumes() - resumes != 3u + (unsigned)late)
                fail("the window after a stretch: %llu resumes", (unsigned long long)(sc_test_resumes() - resumes));
        }
        /* an irregular unit in every record of 41: the windows 1, 2, 4, ... 64 KiB */
        for (uint64_t i = 1; i < 50000; i += 41) {
            b[i - 1] = 'a';
            b[i] = '"';
        }
        const sc_input in = { b, 0, 0, 50000 };
        const sc_dialect d = dialect(',', '"', 0);
        sc_scan s;
        const uint64_t before = sc_test_resumes();
        sc_index(&in, &d, 0, dest, 50000, &s);
        if (sc_test_resumes() - before != 5)
            fail("irregular everywhere: %llu resumes", (unsigned long long)(sc_test_resumes() - before));
    }
}

/* TOO_LARGE at the exact bound (nothing is read nor written) */
void too_large() {
    const sc_dialect d = dialect(',', '"', SC_F_FINAL);
    sc_scan s;
    uint32_t slot = CANARY;
    for (uint64_t base = 0; base < 3; base++) {
        const uint64_t to = base + SC_MAX_SPAN + 1;
        const sc_input in = { &slot, 0, to - 1, to };
        sc_index(&in, &d, base, &slot, 1, &s);
        if (s.status != SC_TOO_LARGE || s.stored || s.records || s.next != to - 1 || s.lines || s.code || s.position || slot != CANARY)
            fail("base %llu: status %d", (unsigned long long)base, s.status);
    }
    /* the largest span, from its end: nothing to read */
    const sc_input in = { &slot, 0, SC_MAX_SPAN, SC_MAX_SPAN };
    sc_index(&in, &d, 0, &slot, 1, &s);
    if (s.status != SC_OK || s.stored || s.next != SC_MAX_SPAN || slot != CANARY)
        fail("the largest span: status %d", s.status);
    checks += 4;
}

/* Positions over 4 GiB, in a sparse map (opt-in: 4 GiB are read) */
#ifdef HAVE_MMAP
void four_gib() {
    const uint64_t total = (uint64_t)5 << 30;
    uint8_t *m = (uint8_t *)mmap(NULL, total, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (m == MAP_FAILED)
        fail("mmap");
    const uint64_t far = ((uint64_t)9 << 29) + 13;      /* 4.5 GiB */
    memcpy(m + far, "a,\"b\nc\",d\ne\"f,g\n", 16);
    const sc_dialect d = dialect(',', '"', SC_F_LINES);
    for (int k = 0; k < sc_kernel_count(); k++) {
        sc_use_kernel(k);
        now_kernel = sc_kernel();
        /* base and from beyond 4 GiB */
        sc_input in = { m, 0, far, far + 16 };
        sc_scan s;
        sc_index(&in, &d, far, dest, 10, &s);
        if (s.status != SC_OK || s.stored != 5 || s.records != 2 || s.next != far + 16 || s.lines != 3 || dest[0] != 1 || dest[1] != 7
            || dest[2] != 9 || dest[3] != 13 || dest[4] != 15)
            fail("beyond 4 GiB: stored %llu next %llu", (unsigned long long)s.stored, (unsigned long long)s.next);
        /* a span of 2^32 - 2, the last entries near 2^32 */
        const uint64_t end = far + 16, base = end - SC_MAX_SPAN;
        in.from = base;
        in.to = end;
        sc_index(&in, &d, base, dest, 10, &s);
        if (s.status != SC_OK || s.stored != 5 || s.records != 2 || s.next != end || dest[4] != SC_MAX_SPAN - 1 || dest[0] != SC_MAX_SPAN - 15)
            fail("a span of 2^32 - 2: stored %llu", (unsigned long long)s.stored);
        in.from = 0;
        sc_measure(&in, &d, &s);
        if (s.status != SC_OK || s.stored != 5 || s.records != 2 || s.next != end || s.lines != 3)
            fail("sc_measure over 4 GiB: stored %llu", (unsigned long long)s.stored);
        sc_dialect strict = d;
        strict.flags |= SC_F_STRICT;
        sc_measure(&in, &strict, &s);
        if (s.status != SC_ERROR || s.position != far + 11 || s.code != SC_E_UNEXPECTED_QUOTE)
            fail("strict over 4 GiB: position %llu", (unsigned long long)s.position);
        in.from = base;
        sc_cursor c;
        sc_field f;
        sc_index(&in, &d, base, dest, 10, &s);
        if (sc_cursor_open(&c, &in, &d, dest, 3, 5) || sc_cursor_next(&c, &f) || f.start != far + 10 || f.end != far + 13)
            fail("the cursor over 4 GiB");
        checks += 5;
    }
    /* as many entries as an index can have: 2^32 - 2 separators, 16 GiB of slots */
    const uint64_t most = SC_MAX_SPAN, bytes = 4 * (most + 4);
    uint32_t *many = (uint32_t *)mmap(NULL, bytes, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (many == MAP_FAILED)
        fail("mmap");
    memset(m, ',', most);
    m[most] = '\n';
    for (int k = 0; k < sc_kernel_count(); k++) {
        sc_use_kernel(k);
        now_kernel = sc_kernel();
        sc_input in = { m, 0, 0, most };
        sc_dialect end = d;
        end.flags = SC_F_FINAL;
        sc_scan s;
        many[most + 1] = many[most + 2] = CANARY;
        sc_index(&in, &end, 0, many, most + 1, &s);
        if (s.status != SC_OK || s.stored != most + 1 || s.records != 1 || s.next != most || s.min_fields != most + 1
            || s.max_fields != most + 1 || many[most + 1] != CANARY || many[most + 2] != CANARY)
            fail("2^32 - 2 separators: stored %llu", (unsigned long long)s.stored);
        for (uint64_t i = 0; i <= most; i++)
            if (many[i] != i)
                fail("2^32 - 2 separators: entry %llu", (unsigned long long)i);
        many[most] = CANARY;            /* one slot less */
        sc_index(&in, &end, 0, many, most, &s);
        if (s.status != SC_MORE || s.stored != 0 || s.next != 0 || many[most] != CANARY)
            fail("2^32 - 2 slots: status %d", s.status);
        checks += 2;
    }
    munmap(many, bytes);
    munmap(m, total);
}
#endif

/* Indexes that are not those of the source: never a read outside (ASan) */
void cursor_fuzz() {
    const int rounds = small ? 20000 : 300000;

    for (int round = 0; round < rounds; round++) {
        const sc_dialect d = random_dialect();
        const uint64_t n = rnd(120), slack = rnd(6), base = rnd(3) ? 0 : rnd((uint32_t)n + 1);
        const bool words = rnd(3) == 0;
        uint8_t *b = (uint8_t *)malloc(n ? n : 1);
        uint32_t *w = (uint32_t *)malloc(n ? 4 * n : 4);
        uint32_t *index = (uint32_t *)malloc(4 * (n + 1 + slack));
        sc_scan s;
        for (uint64_t i = 0; i < n; i++)
            w[i] = b[i] = random_unit((int)rnd(7), d);
        const sc_input in = { words ? (const void *)w : b, words, base, n };
        sc_index(&in, &d, base, index, n + 1, &s);
        uint64_t count = s.stored;
        switch (rnd(7)) {
        case 0: break;
        case 1: for (uint64_t i = 0; i < count; i++) index[i] = rnd() << rnd(12); break;
        case 2: for (uint64_t i = 0; i + 1 < count; i++) index[i] = index[count - 1 - i]; break;
        case 3: if (count) count -= rnd((uint32_t)count) + 1; break;
        case 4: for (uint64_t i = 0; i < count; i++) index[i] += rnd(3) - 1; break;
        case 5: if (count) index[rnd((uint32_t)count)] = rnd((uint32_t)n + 3); break;
        case 6: for (; count < n + 1 + slack; count++) index[count] = rnd(2) ? (uint32_t)(n - base) : rnd((uint32_t)n + 2); break;
        }
        /* exactly as many slots as it is told */
        uint32_t *exact = (uint32_t *)malloc(count ? 4 * count : 1);
        memcpy(exact, index, 4 * count);
        const uint64_t first = rnd(2) ? 0 : rnd((uint32_t)count + 3);
        now_bytes = b;
        now_count = n;
        now_dialect = d;
        now_kernel = "cursor";
        sc_cursor c;
        if (sc_cursor_open(&c, &in, &d, exact, first, count) == 0) {
            sc_field f;
            uint64_t before = base, records = 0;
            while (sc_cursor_next(&c, &f) == 0) {
                if (f.start < before || f.end < f.start || f.end > n)
                    fail("field [%llu,%llu)", (unsigned long long)f.start, (unsigned long long)f.end);
                if (f.end < n && !is_end(d, b[f.end]))
                    fail("a field ends at %llu", (unsigned long long)f.end);
                before = f.end;
                records += (uint64_t)f.last;
                sc_text t;
                int64_t integer;
                double number;
                sc_text_measure(&in, &d, &f, &t);
                uint8_t *copy = (uint8_t *)malloc(t.units * 4 + 1);
                sc_text_copy(&in, &d, &t, copy);
                free(copy);
                sc_to_integer(&in, &d, &f, &integer);
                sc_to_float(&in, &d, &f, 1, &number);
            }
            if (c.slot > count)
                fail("the cursor is at slot %llu", (unsigned long long)c.slot);
            checks += records;
        } else if (first > 0 && first <= count) {
            const uint64_t e = base + exact[first - 1];
            if (e < n && is_line(d, b[e]))
                fail("sc_cursor_open(%llu) refuses a line end", (unsigned long long)first);
        }
        free(exact);
        free(index);
        free(w);
        free(b);
    }
    /* the rules, one by one */
    const char *text = "a,b\r\nc,d\n\n\ne,";
    const sc_input in = { text, 0, 0, strlen(text) };
    const sc_dialect d = dialect(',', '"', 0), skip = dialect(',', '"', SC_F_SKIP_BLANK), lf = dialect(',', '"', SC_F_LF_ONLY);
    uint32_t index[16] = { 1, 3, 6, 8, 9, 10, 12, 13 };
    sc_cursor c;
    sc_field f;
#define EXPECT(e) \
    do { \
        checks++; \
        if (!(e)) \
            fail("%s", #e); \
    } while (0)
    now_bytes = (const uint8_t *)text;
    now_count = in.to;
    now_kernel = "cursor rules";
    EXPECT(sc_cursor_open(&c, &in, &d, index, 0, 8) == 0 && c.pos == 0 && c.slot == 0 && c.end == 8);
    for (int i = 0; i < 8; i++)
        EXPECT(sc_cursor_next(&c, &f) == 0);
    EXPECT(f.start == 13 && f.end == 13 && f.last);
    EXPECT(sc_cursor_open(&c, &in, &d, index, 2, 8) == 0 && c.pos == 5);        /* after CRLF */
    EXPECT(sc_cursor_open(&c, &in, &lf, index, 2, 8) == SC_E_BAD_INDEX);        /* a CR is data */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 1, 8) == SC_E_BAD_INDEX);         /* a separator */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 8, 8) == SC_E_BAD_INDEX);         /* `to' */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 9, 8) == SC_E_BAD_INDEX);         /* beyond end */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 8, 9) == SC_E_BAD_INDEX);
    EXPECT(sc_cursor_open(&c, &in, &skip, index, 4, 8) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX); /* 9 < pos */
    index[4] = 12;
    index[5] = 13;
    EXPECT(sc_cursor_open(&c, &in, &skip, index, 4, 6) == 0 && sc_cursor_next(&c, &f) == 0 && f.start == 11 && f.end == 12 && !f.last);
    EXPECT(sc_cursor_next(&c, &f) == 0 && f.start == 13 && f.end == 13 && f.last);
    EXPECT(sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    EXPECT(sc_cursor_open(&c, &in, &skip, index, 4, 5) == 0 && sc_cursor_next(&c, &f) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    index[0] = 14;      /* past `to' */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 0, 1) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    index[0] = 0;       /* a unit that ends nothing */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 0, 1) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    index[0] = 13;      /* the quoted content or any unit may be inside: only the ends are looked at */
    EXPECT(sc_cursor_open(&c, &in, &d, index, 0, 1) == 0 && sc_cursor_next(&c, &f) == 0 && f.last && f.end == 13);
    EXPECT(sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    const sc_input empty = { text, 0, 3, 3 };
    index[0] = 0;
    EXPECT(sc_cursor_open(&c, &empty, &d, index, 0, 1) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    /* base + index is done in 64 bits */
    index[0] = UINT32_MAX;
    EXPECT(sc_cursor_open(&c, &in, &d, index, 0, 1) == 0 && sc_cursor_next(&c, &f) == SC_E_BAD_INDEX);
    EXPECT(sc_cursor_open(&c, &in, &d, index, 1, 1) == SC_E_BAD_INDEX);
    const sc_input backwards = { text, 0, 5, 3 };
    EXPECT(sc_cursor_open(&c, &backwards, &d, index, 0, 1) == SC_E_BAD_INDEX);
#undef EXPECT
}

/* ----------------------------------------------------------------- numbers */

/* The rules of the numbers, over a text of char, from their definition */
int reference_integer(const char *s, size_t n, int64_t *value) {
    size_t i = 0, e = n;
    while (i < e && (s[i] == ' ' || s[i] == '\t'))
        i++;
    while (e > i && (s[e - 1] == ' ' || s[e - 1] == '\t'))
        e--;
    bool minus = false;
    if (i < e && (s[i] == '-' || s[i] == '+'))
        minus = s[i++] == '-';
    if (i == e)
        return 0;
    for (size_t k = i; k < e; k++)
        if (s[k] < '0' || s[k] > '9')
            return 0;
    while (i < e && s[i] == '0')
        i++;
    if (e - i > 18)
        return 0;
    char digits[32];
    memset(digits, 0, sizeof digits);
    memcpy(digits, s + i, e - i);
    *value = e == i ? 0 : strtoll(digits, NULL, 10);
    if (minus)
        *value = -*value;
    return 1;
}

int reference_float(const char *s, size_t n, char point, double *value) {
    static char text[MAX_UNITS + 8];
    size_t i = 0, e = n;
    while (i < e && (s[i] == ' ' || s[i] == '\t'))
        i++;
    while (e > i && (s[e - 1] == ' ' || s[e - 1] == '\t'))
        e--;
    const size_t start = i;
    if (i < e && (s[i] == '-' || s[i] == '+'))
        i++;
    char significant[24];
    unsigned kept = 0, digits = 0;
    long long fraction = 0;
    bool seen = false;
    for (; i < e; i++) {
        if (s[i] == point && !seen)
            seen = true;
        else if (s[i] >= '0' && s[i] <= '9') {
            digits++;
            fraction += seen;
            if (kept || s[i] != '0') {
                if (kept < 20)
                    significant[kept] = s[i];
                kept++;
            }
        } else
            break;
    }
    if (!digits)
        return 0;
    long long explicit_exponent = 0;
    if (i < e) {
        if (s[i] != 'e' && s[i] != 'E')
            return 0;
        i++;
        bool minus = false;
        if (i < e && (s[i] == '-' || s[i] == '+'))
            minus = s[i++] == '-';
        if (i == e)
            return 0;
        for (size_t k = i; k < e; k++)
            if (s[k] < '0' || s[k] > '9')
                return 0;
        while (i < e && s[i] == '0')
            i++;
        if (e - i > 5)
            return 0;
        for (; i < e; i++)
            explicit_exponent = explicit_exponent * 10 + (s[i] - '0');
        if (explicit_exponent >= 99999)
            return 0;
        if (minus)
            explicit_exponent = -explicit_exponent;
    }
    if (kept > 19)
        return 0;
    const long long exponent = explicit_exponent - fraction;
    uint64_t mantissa = 0;
    for (unsigned k = 0; k < kept; k++)
        mantissa = mantissa * 10 + (uint64_t)(significant[k] - '0');
    if (mantissa > UINT64_C(9007199254740992) || exponent < -22 || exponent > 37)
        return 0;
    if (exponent > 22) {
        uint64_t limit = UINT64_C(9007199254740992);
        for (long long k = 22; k < exponent; k++)
            limit /= 10;
        if (mantissa > limit)
            return 0;
    }
    memcpy(text, s + start, e - start);
    text[e - start] = 0;
    for (char *p = text; *p; p++)
        if (*p == point)
            *p = '.';
    *value = strtod(text, NULL);
    return 1;
}

uint32_t number_words[MAX_UNITS + 2];
uint64_t converted_floats, converted_integers;

void check_number_in(const sc_input &in, const sc_dialect &d, const char *s, size_t n, bool odd) {
    const sc_field f = { in.from, in.to, 1 };
    int64_t got = -77, want = -77;
    double gotf = -77, wantf = -77;

    now_bytes = (const uint8_t *)s;
    now_count = n;
    now_dialect = d;
    now_kernel = in.wide ? "wide numbers" : "numbers";
    const int i = sc_to_integer(&in, &d, &f, &got), wi = odd ? 0 : reference_integer(s, n, &want);
    if (i != wi || got != want)
        fail("sc_to_integer %d %lld, expected %d %lld", i, (long long)got, wi, (long long)want);
    converted_integers += (uint64_t)i;
    for (int comma = 0; comma < 2; comma++) {
        gotf = wantf = -77;
        const int x = sc_to_float(&in, &d, &f, comma, &gotf), wx = odd ? 0 : reference_float(s, n, comma ? ',' : '.', &wantf);
        if (x != wx || memcmp(&gotf, &wantf, sizeof gotf) != 0)
            fail("sc_to_float(comma %d) %d %.17g, expected %d %.17g", comma, x, gotf, wx, wantf);
        converted_floats += (uint64_t)x;
    }
    checks++;
}

void check_number(const char *s) {
    static char quoted[MAX_UNITS + 4];
    const size_t n = strlen(s);
    if (*s == '"')
        return;
    const sc_dialect d = dialect(';', '"', 0), none = dialect(';', '"', SC_F_NO_QUOTE);

    for (size_t i = 0; i < n; i++)
        number_words[i] = (uint8_t)s[i];
    const sc_input in = { s, 0, 0, n }, words = { number_words, 1, 0, n };
    check_number_in(in, d, s, n, false);
    check_number_in(words, d, s, n, false);
    /* in quotes; without its closing quote; with text after it; with a doubled quote */
    if (memchr(s, '"', n))
        return;
    quoted[0] = '"';
    memcpy(quoted + 1, s, n);
    quoted[n + 1] = '"';
    quoted[n + 2] = '"';
    const sc_input q = { quoted, 0, 0, n + 2 }, unclosed = { quoted, 0, 0, n + 1 }, doubled = { quoted, 0, 0, n + 3 };
    check_number_in(q, d, s, n, false);
    check_number_in(unclosed, d, s, n, false);
    check_number_in(doubled, d, s, n, true);
    check_number_in(q, none, s, n, true);
    for (size_t i = 0; i < n + 2; i++)
        number_words[i] = (uint8_t)quoted[i];
    const sc_input qw = { number_words, 1, 0, n + 2 };
    check_number_in(qw, d, s, n, false);
    quoted[n + 2] = '1';
    check_number_in(doubled, d, s, n, true);
}

void numbers() {
    static const char *const edges[] = { "", " ", "+", "-", "+-1", "0", "-0", "+0", "00", "007", "-007", "1", "-1", "+1", " 1", "1 ", "\t1\t", " 1 2 ",
        "1 .", "- 1", "12a", "a12", "0x10", "1_0", "123456789012345678", "-123456789012345678", "999999999999999999", "1000000000000000000",
        "0000000000000000000000000123456789012345678", "00000000000000000000001234567890123456789", "9223372036854775807",
        "1.", ".1", ".", "-.", "1.0", "-1.5", "0.1", "0,1", ",5", "5,", "1,5,5", "1.5.5", "1.,5", "1e", "1e+", "1e-", "e5", ".e5", "1e5", "1E5",
        "1e+5", "1e-5", "1.e5", ".5e1", "1e5.", "1e5e5", "1e 5", "1 e5", "1e22", "1e23", "1e-22", "1e-23", "1e37", "1e38", "123e37", "9e37", "9.1e38",
        "9007199254740992", "9007199254740993", "9007199254740992e22", "9007199254740992e-22", "9007199254740992e23", "900719925474099e23",
        "900719925474100e23", "1e400", "-1e400", "1e18446744073709551617", "1e-18446744073709551617", "1e99998", "1e99999", "1e100000", "0e0", "0e5",
        "0e22", "0e37", "0e38", "0e-22", "0e-23", "-0.0", "-0e5", "0.0000000000000000000000", "0.00000000000000000000000", "000.5",
        "1234567890123456789", "12345678901234567890", "0.1234567890123456789", "1.0000000000000000", "1.00000000000000000000", "nan", "inf",
        "-inf", "NaN", "Infinity", "1f", "1.5f", "0x1p3", "1\v", "\n1", "1\r", "4.35", "0.000001", "123456.789", "8.41e21", "8.41E-21",
        "1e00000000000000000000000000005", "1e-00000000000000000000000000005", "1e000000", "00000000000000000000000000000000.5",
        "0.00000000000000000001e20", "0.000000000000000000001e40", "/", ":", "1/", "1:", "/1", ":1", "e", "E", "1e/", "1e:", "\xB1", "1\xB0" };
    for (unsigned i = 0; i < sizeof edges / sizeof edges[0]; i++)
        check_number(edges[i]);
    /* the exponent bounds, with every sign */
    char text[MAX_UNITS + 1];
    for (int e = -60; e <= 60; e++)
        for (int m = 0; m < 6; m++) {
            static const char *const mantissas[] = { "1", "-9", "9007199254740992", "9.5", "0.001", "4503599627370497" };
            snprintf(text, sizeof text, "%se%d", mantissas[m], e);
            check_number(text);
        }
    /* the exact bound of every power above 22 */
    uint64_t limit = UINT64_C(9007199254740992);
    for (int e = 22; e <= 38; e++, limit /= 10)
        for (int delta = -1; delta <= 1; delta++) {
            snprintf(text, sizeof text, "%llue%d", (unsigned long long)(limit + (uint64_t)delta), e);
            check_number(text);
        }
    /* 400 digits after the point, and a fraction as long as the exponent saturates */
    for (int zeros = 0; zeros < 420; zeros++) {
        size_t n = 0;
        text[n++] = '0';
        text[n++] = '.';
        memset(text + n, '0', (size_t)zeros);
        n += (size_t)zeros;
        snprintf(text + n, sizeof text - n, "125");
        check_number(text);
        snprintf(text + n, sizeof text - n, "125e%d", zeros + 3);
        check_number(text);
    }
    memset(text, '0', sizeof text);
    text[0] = '1';
    memcpy(text + 60000, "e-59999", 8);
    check_number(text);                 /* 1 and 59,999 zeros: too many digits */
    memset(text, '0', sizeof text);
    text[1] = '.';
    memcpy(text + 60001, "1e60000", 8);
    check_number(text);                 /* exponent 0: 1.0 */
    memcpy(text + 60001, "1e99999", 8);
    check_number(text);                 /* saturated */
    memcpy(text + 60001, "1e60023", 8);
    check_number(text);
    memset(text, '7', sizeof text);
    text[sizeof text - 1] = 0;
    text[1] = '.';
    check_number(text);
    memset(text, '0', sizeof text);
    text[sizeof text - 1] = 0;
    check_number(text);
    text[sizeof text - 2] = '9';
    check_number(text);
    const int rounds = small ? 50000 : 1000000;
    for (int round = 0; round < rounds; round++) {
        char *p = text;
        switch (rnd(8)) {
        case 0:                 /* units of numbers, at random */
            for (uint32_t n = rnd(10); n; n--)
                *p++ = "0123456789+-.,eE \tx\""[rnd(20)];
            *p = 0;
            break;
        case 1: snprintf(text, sizeof text, "%.17g", (rnd() - 2147483648.0) * pow(10, (int)rnd(50) - 25)); break;
        case 2: snprintf(text, sizeof text, "%.*f", (int)rnd(12), (rnd() - 2147483648.0) / (1 + rnd(1000))); break;
        case 3: snprintf(text, sizeof text, "%d", (int32_t)rnd()); break;
        case 4: snprintf(text, sizeof text, "%llu", ((unsigned long long)rnd() << 32 | rnd()) >> rnd(64)); break;
        case 5: snprintf(text, sizeof text, "%u.%ue%d", rnd(), rnd(100000), (int)rnd(90) - 45); break;
        case 6: snprintf(text, sizeof text, "%s%.*g%s", rnd(2) ? " " : "", (int)rnd(17), rnd() * 1e-7, rnd(2) ? "\t" : ""); break;
        default:
            if (rnd(2))
                *p++ = "+-"[rnd(2)];
            for (uint32_t n = rnd(12); n; n--)
                *p++ = (char)('0' + rnd(10));
            if (rnd(2)) {
                *p++ = ".,"[rnd(2)];
                for (uint32_t n = rnd(12); n; n--)
                    *p++ = (char)('0' + rnd(10));
            }
            if (rnd(2)) {
                *p++ = "eE"[rnd(2)];
                if (rnd(2))
                    *p++ = "+-"[rnd(2)];
                for (uint32_t n = rnd(3); n; n--)
                    *p++ = (char)('0' + rnd(10));
            }
            *p = 0;
        }
        check_number(text);
    }
    if (converted_floats < (uint64_t)rounds || converted_integers < (uint64_t)rounds / 2)
        fail("only %llu floats and %llu integers were converted", (unsigned long long)converted_floats,
             (unsigned long long)converted_integers);
    /* a quote that is a unit of numbers: a lone one inside is odd all the same */
    static const struct { const char *text; char quote; int odd; } quotes[] = { { "5125", '5', 0 }, { "51256", '5', 1 }, { "5 5 5", '5', 1 },
        { " 12 3", ' ', 1 }, { " 12 ", ' ', 0 }, { " 12  ", ' ', 1 }, { " 12", ' ', 0 }, { "-12-", '-', 0 }, { "-12-3", '-', 1 },
        { "51255", '5', 1 }, { "512", '5', 0 }, { "5555", '5', 1 } };
    for (unsigned i = 0; i < sizeof quotes / sizeof quotes[0]; i++) {
        const sc_input in = { quotes[i].text, 0, 0, strlen(quotes[i].text) };
        const sc_dialect d = dialect(',', quotes[i].quote, 0);
        const sc_field f = { 0, in.to, 1 };
        int64_t integer = 0;
        double number = 0;
        const int a = sc_to_integer(&in, &d, &f, &integer), b = sc_to_float(&in, &d, &f, 0, &number);
        if (a != !quotes[i].odd || b != !quotes[i].odd || (a && (integer != 12 || number != 12)))
            fail("the quoted number %s: %d %d", quotes[i].text, a, b);
        checks++;
    }
    /* the exponent saturates at 99999, whatever the digits after the point */
    static char far[100100];
    for (int e = 99990; e <= 100000; e++) {
        memset(far, '0', sizeof far);
        far[1] = '.';
        const int length = snprintf(far + 2 + e - 1, 32, "1e%d", e);
        const sc_input in = { far, 0, 0, (uint64_t)(2 + e - 1 + length) };
        const sc_dialect d = dialect(',', '"', 0);
        const sc_field f = { 0, in.to, 1 };
        double number = 0;
        if (sc_to_float(&in, &d, &f, 0, &number) != (e < 99999) || (e < 99999 && number != 1.0))
            fail("the exponent %d after %d digits", e, e);
        checks++;
    }
    /* words whose low bytes are digits are not digits */
    for (int bit = 8; bit < 32; bit++) {
        uint32_t w[3] = { '1', '2', '3' };
        for (int i = 0; i < 3; i++) {
            w[i] ^= 1u << bit;
            const sc_input in = { w, 1, 0, 3 };
            const sc_dialect d = dialect(',', '"', 0);
            const sc_field f = { 0, 3, 1 };
            int64_t integer;
            double number;
            if (sc_to_integer(&in, &d, &f, &integer) || sc_to_float(&in, &d, &f, 0, &number))
                fail("the word 0x%x is a digit", w[i]);
            w[i] ^= 1u << bit;
            checks++;
        }
    }
    /* the range of the field, not more: units around it are not looked at */
    const char *around = "91234e59";
    const sc_input in = { around, 0, 0, 8 };
    const sc_dialect d = dialect(',', '"', 0);
    const sc_field f = { 1, 5, 1 };
    int64_t integer;
    double number;
    if (!sc_to_integer(&in, &d, &f, &integer) || integer != 1234 || !sc_to_float(&in, &d, &f, 0, &number) || number != 1234)
        fail("a field inside of its input");
}

/* ------------------------------------------------------------------- UTF-8 */

void check_text_bytes(const uint8_t *b, uint64_t n, uint32_t flags) {
    const sc_input in = { b, 0, 0, n };
    const sc_field f = { 0, n, 1 };
    const sc_dialect d = dialect(',', '"', flags);
    now_bytes = b;
    now_count = n;
    now_dialect = d;
    now_kernel = "text";
    check_text(in, d, f);
}

void utf8() {
    uint8_t b[40];

    /* every sequence of two bytes, at the start, after 8 and 15 ASCII ones, before ASCII */
    for (int x = 0; x < 256; x++)
        for (int y = 0; y < 256; y++)
            for (int before = 0; before < 3; before++) {
                static const int at[] = { 0, 8, 15 };
                memset(b, 'a', sizeof b);
                b[at[before]] = (uint8_t)x;
                b[at[before] + 1] = (uint8_t)y;
                check_text_bytes(b, (uint64_t)at[before] + 2, 0);
                check_text_bytes(b, (uint64_t)at[before] + 5, 0);
                if (before == 0) {
                    check_text_bytes(b, 2, SC_F_LATIN1);
                    check_text_bytes(b, 2, SC_F_NO_QUOTE);
                }
            }
    /* every sequence of three bytes that starts one, and all the others sampled */
    for (int x = 0; x < 256; x++)
        for (int y = 0; y < 256; y++)
            for (int z = 0; z < 256; z++) {
                if (small && rnd(16))
                    continue;
                b[0] = (uint8_t)x;
                b[1] = (uint8_t)y;
                b[2] = (uint8_t)z;
                b[3] = 'a';
                check_text_bytes(b, 3 + (uint64_t)(z & 1), 0);
            }
    /* the bounds of four, then samples */
    static const uint8_t leads[] = { 0xEF, 0xF0, 0xF1, 0xF3, 0xF4, 0xF5, 0xF7, 0xF8, 0xFC, 0xFF };
    static const uint8_t seconds[] = { 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0 };
    for (unsigned i = 0; i < sizeof leads; i++)
        for (unsigned j = 0; j < sizeof seconds; j++)
            for (unsigned k = 0; k < sizeof seconds; k++)
                for (unsigned l = 0; l < sizeof seconds; l++)
                    for (uint64_t n = 1; n <= 5; n++) {
                        b[0] = leads[i];
                        b[1] = seconds[j];
                        b[2] = seconds[k];
                        b[3] = seconds[l];
                        b[4] = 0x80;
                        check_text_bytes(b, n, 0);
                    }
    for (uint32_t point = 0; point < 0x110000; point += small ? 131 : 1) {
        uint64_t n = 0;
        if (point < 0x80)
            b[n++] = (uint8_t)point;
        else if (point < 0x800) {
            b[n++] = (uint8_t)(0xC0 | point >> 6);
            b[n++] = (uint8_t)(0x80 | (point & 63));
        } else if (point < 0x10000) {
            b[n++] = (uint8_t)(0xE0 | point >> 12);
            b[n++] = (uint8_t)(0x80 | (point >> 6 & 63));
            b[n++] = (uint8_t)(0x80 | (point & 63));
        } else {
            b[n++] = (uint8_t)(0xF0 | point >> 18);
            b[n++] = (uint8_t)(0x80 | (point >> 12 & 63));
            b[n++] = (uint8_t)(0x80 | (point >> 6 & 63));
            b[n++] = (uint8_t)(0x80 | (point & 63));
        }
        check_text_bytes(b, n, 0);
    }
    /* quoted fields: the quotes cut the sequences; long and mixed texts */
    static uint8_t text[300];
    const int rounds = small ? 30000 : 1000000;
    for (int round = 0; round < rounds; round++) {
        uint64_t n = 0;
        const uint32_t size = rnd(rnd(4) ? 24 : 200), style = rnd(5);
        if (rnd(2))
            text[n++] = '"';
        while (n < size)
            switch (style == 0 ? rnd(4) : rnd(style * 12)) {
            case 0: text[n++] = 0xC3; text[n++] = 0xA9; break;
            case 1: text[n++] = 0xE2; text[n++] = 0x82; text[n++] = 0xAC; break;
            case 2: text[n++] = '"'; text[n++] = '"'; break;
            case 3: text[n++] = rnd(20) ? 'b' : (uint8_t)rnd(); break;
            case 4: if (rnd(4) == 0) text[n++] = '"'; break;
            case 5: text[n++] = 0xF0; text[n++] = 0x9F; text[n++] = 0x98; text[n++] = 0x80; break;
            default: text[n++] = 'a';
            }
        if (rnd(2))
            text[n++] = '"';
        check_text_bytes(text, n, rnd(5) ? 0u : rnd(2) ? (uint32_t)SC_F_LATIN1 : (uint32_t)SC_F_NO_QUOTE);
        if (rnd(8) == 0) {
            static uint32_t words[300];
            for (uint64_t i = 0; i < n; i++)
                words[i] = rnd(40) ? text[i] : rnd() >> rnd(32);
            const sc_input in = { words, 1, 0, n };
            const sc_field f = { 0, n, 1 };
            const sc_dialect d = dialect(',', '"', 0);
            now_kernel = "wide text";
            check_text(in, d, f);
        }
    }
}

/* Random 32-bit units: a separator and bytes beyond 255 */
void words() {
    static uint32_t w[400];
    const int rounds = small ? 20000 : 200000;

    for (int round = 0; round < rounds; round++) {
        const uint64_t n = rnd(120);
        const sc_dialect d = dialect(rnd(2) ? 0x20AC : ',', rnd(3) ? '"' : 0xFF, random_flags() & ~(uint32_t)SC_F_LATIN1);
        const uint32_t units[8] = { d.separator, d.quote, '\n', '\r', 'a', 0x122, 0x10A, (uint32_t)d.quote | 0x100 };
        for (uint64_t i = 0; i < n; i++)
            w[i] = rnd(30) ? units[rnd(8)] : rnd();
        const Units u = { NULL, w };
        now_bytes = NULL;
        now_dialect = d;
        now_kernel = "words";
        const uint64_t base = rnd(3) ? 0 : rnd((uint32_t)n + 1), capacity = rnd(2) ? n + 1 : rnd((uint32_t)n + 1);
        reference(&ref, u, base, n, d, base, capacity);
        reference(&ref2, u, base, n, d, base, UINT64_MAX);
        const sc_input in = { w, 1, base, n };
        check_index(in, d, base, capacity, ref);
        check_measure(in, d, ref2);
        check_cursor(in, d, dest, ref, true);
    }
}

/* The kernels' table, and the notes of sc_init */
void interface() {
    const int n = sc_kernel_count();
    if (n < 1 || strcmp(sc_kernel_name(n - 1), "fallback") != 0)
        fail("the last kernel is not fallback");
    if (sc_kernel_name(n) || sc_kernel_name(-1) || sc_use_kernel(n) || sc_use_kernel(-1))
        fail("a kernel that is not there");
    for (int i = 0; i < n; i++)
        if (!sc_use_kernel(i) || strcmp(sc_kernel(), sc_kernel_name(i)) != 0)
            fail("kernel %d", i);
    sc_init();
    if (getenv("SIMDCSV_FORCE_IMPLEMENTATION") == NULL && (strcmp(sc_kernel(), sc_kernel_name(0)) != 0 || *sc_forced_note()))
        fail("sc_init chose %s (%s)", sc_kernel(), sc_forced_note());
#if defined(__unix__) || defined(__APPLE__)
    const char *was = getenv("SIMDCSV_FORCE_IMPLEMENTATION");
    char kept[200] = "";
    if (was)
        strncpy(kept, was, sizeof kept - 1);
    setenv("SIMDCSV_FORCE_IMPLEMENTATION", "bogus", 1);
    sc_init();
    if (strcmp(sc_forced_note(), "refused bogus") != 0 || strcmp(sc_kernel(), sc_kernel_name(0)) != 0)
        fail("bogus: %s (%s)", sc_kernel(), sc_forced_note());
    setenv("SIMDCSV_FORCE_IMPLEMENTATION", "fallback", 1);
    sc_init();
    if (strcmp(sc_forced_note(), "fallback") != 0 || strcmp(sc_kernel(), "fallback") != 0)
        fail("fallback: %s (%s)", sc_kernel(), sc_forced_note());
    unsetenv("SIMDCSV_FORCE_IMPLEMENTATION");
    sc_init();
    if (*sc_forced_note() || strcmp(sc_kernel(), sc_kernel_name(0)) != 0)
        fail("unset: %s (%s)", sc_kernel(), sc_forced_note());
    if (was)
        setenv("SIMDCSV_FORCE_IMPLEMENTATION", kept, 1);
    sc_init();
#endif
    checks += 7;
}

#ifdef SC_UPSTREAM
/* simdcsv's own find_indexes (src/main.cpp, cut by run-c-tests.sh), with 64
 * zero bytes after the input: what sc_index answers with LF_ONLY on regular
 * input with a final LF */
}       /* namespace */
#include <x86intrin.h>
#include <cstdint>
namespace upstream {
#define really_inline static inline __attribute__((always_inline, unused))
really_inline int trailingzeroes(uint64_t x) { return __builtin_ctzll(x | (x == 0)) + 64 * (x == 0); }
really_inline int hamming(uint64_t x) { return __builtin_popcountll(x); }
#include "simdcsv_upstream.inc"
}
namespace {
void upstream_indexes() {
    const uint64_t size = 60000;
    static uint8_t b[60000 + 256];
    static uint32_t theirs[60000 + 256];
    const sc_dialect d = dialect(',', '"', SC_F_LF_ONLY | SC_F_FINAL);

    for (int round = 0; round < (small ? 200 : 4000); round++) {
        uint64_t n = random_regular(b, 64 + rnd(round % 20 ? 1500 : (uint32_t)size - 64), d, 0);
        for (uint64_t i = 0; i < n; i++)        /* no CR outside: LF_ONLY */
            if (b[i] == '\r')
                b[i] = 'r';
        if (n == 0)
            continue;
        if (rnd(2))             /* cut at any record end: the last record ends with LF */
            b[n++] = '\n';
        const Units u = { b, NULL };
        reference(&ref2, u, 0, n, d, 0, UINT64_MAX);
        if (n && b[n - 1] != '\n')
            b[n++] = '\n';
        sc_dialect strict = d;
        strict.flags |= SC_F_STRICT;
        reference(&ref, u, 0, n, strict, 0, UINT64_MAX);
        if (ref.scan.status != SC_OK || ref.scan.next != n)
            continue;           /* a CR became the r after a quote */
        memset(b + n, 0, 256);
        upstream::ParsedCSV p;
        p.indexes = theirs;
        upstream::find_indexes(b, n + 64, p);
        uint64_t count = p.n_indexes;
        while (count && theirs[count - 1] >= n)
            fail("upstream finds an end in the padding");
        now_bytes = b;
        now_count = n;
        const sc_input in = { b, 0, 0, n };
        for (int k = 0; k < sc_kernel_count(); k++) {
            sc_scan s;
            sc_use_kernel(k);
            now_kernel = sc_kernel();
            sc_index(&in, &d, 0, dest, n + 1, &s);
            if (s.stored != count || memcmp(dest, theirs, 4 * count) != 0)
                fail("upstream: %llu, ours %llu", (unsigned long long)count, (unsigned long long)s.stored);
            checks++;
        }
    }
    if (checks < 100)
        fail("upstream: only %llu comparisons", (unsigned long long)checks);
}
#endif

[[maybe_unused]] const struct Section {
    const char *name;
    void (*run)();
    bool asked;         /* only when it is named */
} sections[] = {
    { "interface", interface, false },
    { "lenient", exhaustive_lenient, false },
    { "strict", exhaustive_strict, false },
    { "lf-only", exhaustive_lf_only, false },
    { "skip", exhaustive_skip, false },
    { "dialects", exhaustive_dialects, false },
    { "degenerate", degenerate, false },
    { "nul-quote", nul_quote, false },
    { "pairs", pairs, false },
    { "random", random_ranges, false },
#ifdef HAVE_MMAP
    { "guard-pages", guard_pages, false },
    { "4gib", four_gib, true },
#endif
    { "capacities", capacities, false },
    { "events", event_order, false },
    { "splits", splits, false },
    { "irregular", irregular, false },
    { "too-large", too_large, false },
    { "cursor", cursor_fuzz, false },
    { "numbers", numbers, false },
    { "utf8", utf8, false },
    { "words", words, false },
#ifdef SC_UPSTREAM
    { "upstream", upstream_indexes, true },
#endif
};

}       /* namespace */

#ifndef SCTEST_NO_MAIN
int main(int argc, char **argv) {
    bool named = false;

    setvbuf(stdout, NULL, _IOLBF, 0);
    sc_init();
    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--small") == 0)
            small = true;
        else if (strcmp(argv[i], "--seed") == 0 && i + 1 < argc)
            seed = strtoull(argv[++i], NULL, 10) * 2 + 1;
        else if (strcmp(argv[i], "--sections") == 0) {
            for (const Section &s : sections)
                printf("%s%s\n", s.name, s.asked ? " (asked)" : "");
            return 0;
        } else if (strcmp(argv[i], "--forced") == 0) {
            printf("%s|%s|", sc_kernel(), sc_forced_note());
            for (int k = 0; k < sc_kernel_count(); k++)
                printf(" %s", sc_kernel_name(k));
            printf("\n");
            return 0;
        } else
            named = true;
    }
    for (const Section &s : sections) {
        bool run = !named && !s.asked;
        for (int i = 1; i < argc; i++)
            run = run || strcmp(argv[i], s.name) == 0;
        if (!run)
            continue;
        const uint64_t before = checks;
        s.run();
        printf("%s: %llu checks ok\n", s.name, (unsigned long long)(checks - before));
    }
    if (checks == 0) {
        printf("no section was run\n");
        return 1;
    }
    return 0;
}
#endif
