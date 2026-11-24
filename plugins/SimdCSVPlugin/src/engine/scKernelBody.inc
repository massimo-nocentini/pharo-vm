/* scKernelBody.inc -- the block and the loops of a kernel, included by
 * scKernels.inc once per namespace: ours, over the five names of the slice and
 * flip_input.
 *
 * A block is 64 bytes.  Q are its quotes, M the prefix xor of the quotes (from
 * an opening quote to the unit before the closing one), O = Q & M the opening
 * and C = Q & ~M the closing quotes, T the ends outside of quotes.  Where every
 * opening quote stands at the start of a field or right after a closing one
 * (an escaped quote), and every closing quote is followed by an end or by a
 * quote, the ends T are those of the automaton; otherwise the first bit of
 * `bad' is the first irregular unit, and the ends before it are. */

/* From one block to the next; at the start of a record */
struct carries {
    uint64_t inside = 0;        /* all ones: in a quoted field (the slice's) */
    uint64_t end = 1;           /* the last unit was an end, */
    uint64_t closing = 0;       /* a closing quote, */
    uint64_t cr = 0;            /* a CR that ended a record, */
    uint64_t line = 1;          /* a line end */
};

struct masks {
    uint64_t fields, lines;     /* the field ends; those that end a record */
    uint64_t bad, quoted;
};

sc_inline masks block(const uint8_t *p, const sc_needles &n, uint64_t keep, carries &c) {
    const simd_input in = flip_input(fill_input(p), n.flip);
    const uint64_t Q = cmp_mask_against_input(in, '"') & keep & n.quoting;
    const uint64_t M = find_quote_mask(in, c.inside) & n.quoting;
    const uint64_t outside = ~M & keep;
    const uint64_t lf = cmp_mask_against_input(in, n.lf) & outside;
    const uint64_t cr = cmp_mask_against_input(in, n.cr) & outside & n.cr_ends;
    const uint64_t line = lf | cr;
    const uint64_t T = (cmp_mask_against_input(in, n.separator) & outside) | line;
    const uint64_t O = Q & M, C = Q & ~M;
    const uint64_t after_closing = C << 1 | c.closing;
    masks m;

    m.quoted = M;
    m.bad = ((O & ~(T << 1 | c.end | after_closing)) | (after_closing & ~(T | O))) & keep;
    /* the LF of a CRLF is no end of its own; with SKIP_BLANK neither is a line
     * end right after a line end */
    m.lines = line & ~(lf & (cr << 1 | c.cr));
    m.lines &= ~((line << 1 | c.line) & n.skip_blank);
    m.fields = (T & ~line) | m.lines;
    c.end = T >> 63;
    c.closing = C >> 63;
    c.cr = cr >> 63;
    c.line = line >> 63;
    return m;
}

/* The records that end in a block; `fields' are the fields before it */
sc_inline void tally_lines(sc_tally &t, const masks &m, uint64_t p, uint64_t fields, const bool widths) {
    if (m.lines == 0)
        return;
    const unsigned last = 63 - (unsigned)__builtin_clzll(m.lines);

    t.done = p + last + 1;
    if (!widths) {
        t.records += (uint64_t)hamming(m.lines);
        t.fields = fields + (uint64_t)hamming(m.fields & ~(uint64_t)0 >> (63 - last));
        return;
    }
    if (m.lines == m.fields) {          /* all but the first are records of one field */
        const uint64_t n = (uint64_t)hamming(m.lines);
        t.ended(fields + 1);
        if (n > 1)
            t.ended(fields + 2);
        t.fields = fields + n;
        t.records += n - 1 - (n > 1);
        return;
    }
    for (uint64_t lines = m.lines; lines; lines &= lines - 1)
        t.ended(fields + (uint64_t)hamming(m.fields & (lines ^ (lines - 1))));
}

sc_inline void scan(sc_job *j, const bool store) {
    const uint8_t *u = (const uint8_t *)j->units;
    const uint64_t limit = j->limit, base = j->base, capacity = j->capacity;
    const uint64_t start = j->pos;
    const sc_needles n = j->needles;
    uint32_t *dest = j->dest;
    uint64_t p = start, fields = j->tally.fields;
    sc_tally tally = j->tally;
    carries c;

    j->event = SC_AT_END;
    j->inside = false;
    if (store)
        while (limit - p >= 256 && capacity - fields >= 272) {
            const carries saved = c;
            masks m[4];

            /* (hints: an address beyond the input or the output is none) */
            __builtin_prefetch((const void *)((uintptr_t)(dest + fields) + 2048), 1);
            for (int i = 0; i < 4; i++) {
                __builtin_prefetch((const void *)((uintptr_t)u + p + 64 * (unsigned)i + 768));
                m[i] = block(u + p + 64 * i, n, ~(uint64_t)0, c);
            }
            if (__builtin_expect((m[0].bad | m[1].bad | m[2].bad | m[3].bad) != 0, 0)) {
                c = saved;      /* the loop below stops there */
                break;
            }
            for (int i = 0; i < 4; i++) {
                const uint64_t at = p + 64 * (uint64_t)i;
                uint32_t count = 0;
                tally_lines(tally, m[i], at, fields, true);
                flatten_bits(dest + fields, count, (uint32_t)(at - base), m[i].fields);
                fields += count;
            }
            p += 256;
            j->inside = (m[3].quoted >> 63) != 0;
        }
    while (p < limit) {
        const uint64_t rest = limit - p, room = capacity - fields;
        const unsigned size = rest < 64 ? (unsigned)rest : 64;
        const uint64_t keep = ~(uint64_t)0 >> (64 - size);
        uint8_t tail[64];
        const uint8_t *q = u + p;

        if (size < 64) {
            memset(tail, 0, sizeof tail);
            memcpy(tail, q, size);
            q = tail;
        }
        masks m = block(q, n, keep, c);
        if (m.bad) {            /* what is before its first bit */
            j->event = SC_IRREGULAR;
            j->at = p + (uint64_t)trailingzeroes(m.bad);
            m.fields &= (m.bad & (0 - m.bad)) - 1;
            m.lines &= m.fields;
        }
        if (store && room < 64 && (uint64_t)hamming(m.fields) > room) {
            uint64_t beyond = m.fields;         /* what is before the first field without a slot */
            for (uint64_t i = 0; i < room; i++)
                beyond &= beyond - 1;
            j->event = SC_NO_SLOT;
            m.fields ^= beyond;
            m.lines &= m.fields;
        }
        tally_lines(tally, m, p, fields, store);
        if (!store)
            fields += (uint64_t)hamming(m.fields);
        else if (room >= 80) {
            uint32_t count = 0;
            flatten_bits(dest + fields, count, (uint32_t)(p - base), m.fields);
            fields += count;
        } else
            for (uint64_t bits = m.fields; bits; bits &= bits - 1)
                dest[fields++] = (uint32_t)(p + (uint64_t)trailingzeroes(bits) - base);
        if (j->event != SC_AT_END)
            break;
        p += size;
        /* the state after the last real byte: with the quote NUL the zeros
         * after it are quotes */
        j->inside = (m.quoted >> (size - 1) & 1) != 0;
    }
    if (tally.done > start && tally.done < limit && u[tally.done - 1] == CR && u[tally.done] == LF)
        tally.done++;           /* the LF of the last record's CRLF */
    j->pos = p;
    j->fields = fields;
    j->tally = tally;
}

static void scan_index(sc_job *j) { scan(j, true); }
static void scan_measure(sc_job *j) { scan(j, false); }

/* The line ends of [from, to), quoted ones too, a CRLF once */
static uint64_t count_lines(const uint8_t *u, uint64_t from, uint64_t to, bool lf_only) {
    const uint64_t cr_ends = lf_only ? 0 : ~(uint64_t)0;
    uint64_t carry = 0, lines = 0;

    for (uint64_t p = from; p < to;) {
        const uint64_t rest = to - p;
        const unsigned size = rest < 64 ? (unsigned)rest : 64;
        const uint64_t keep = ~(uint64_t)0 >> (64 - size);
        uint8_t tail[64];
        const uint8_t *q = u + p;

        if (size < 64) {
            memset(tail, 0, sizeof tail);
            memcpy(tail, q, size);
            q = tail;
        }
        const simd_input in = fill_input(q);
        const uint64_t lf = cmp_mask_against_input(in, LF) & keep;
        const uint64_t cr = cmp_mask_against_input(in, CR) & keep & cr_ends;
        lines += (uint64_t)hamming((lf | cr) & ~(lf & (cr << 1 | carry)));
        carry = cr >> 63;
        p += size;
    }
    return lines;
}
