/* fuzz.cpp -- random inputs of the engine of the SimdCSVPlugin, against the
 * second automaton of sctest.cpp, on every kernel: as a libFuzzer target, or,
 * compiled with -DSC_FUZZ_MAIN, as a program of its own (fuzz --seconds N
 * [--seed N]), which makes its inputs itself. */
#define SCTEST_NO_MAIN
#include "sctest.cpp"
#include <time.h>

/* The first 12 bytes choose the dialect and the range, the others are data */
static void fuzz_one(const uint8_t *data, size_t size) {
    static const uint8_t separators[] = { ',', ';', '\t', 0, '|', 0xFF, ' ', 'x' };
    static const uint8_t quotes[] = { '"', '"', '\'', 0, 0xFE, 'a', '"', ';' ^ 1 };
    const uint8_t *b = data + 12;
    uint64_t n;

    if (size < 12 || size > 4096)
        return;
    n = size - 12;
    uint32_t flags = data[0] | (uint32_t)(data[1] & 1) << 8;
    sc_dialect d = dialect(separators[data[2] & 7], quotes[data[3] & 7], flags);
    if (d.separator == d.quote)
        return;
    if (d.separator > 127 || d.quote > 127)
        d.flags |= SC_F_LATIN1;
    const uint64_t base = data[4] & 1 && n ? (data[5] | (uint64_t)data[6] << 8) % (n + 1) : 0;
    const uint64_t from = data[4] & 2 ? base + data[7] % (n - base + 1) : base;
    const uint64_t capacity = data[4] & 4 ? (data[8] | (uint64_t)data[9] << 8) % (n + 2) : n + 1;

    check(b, from, n, d, base, capacity, DEEP | EXACT);
    const uint64_t split = (data[10] | (uint64_t)data[11] << 8) % (n + 1);
    for (int k = 0; k < sc_kernel_count(); k++) {
        sc_use_kernel(k);
        check_split(b, n, split, d);
    }
    check_text_bytes(b, n, d.flags & (SC_F_LATIN1 | SC_F_NO_QUOTE));
    /* the data as a number, as an index of itself */
    static char text[4097];
    memcpy(text, b, n);
    text[n] = 0;
    check_number(text);
    uint32_t *index = (uint32_t *)malloc(n / 4 * 4 + 4);
    memcpy(index, b, n / 4 * 4);
    for (uint64_t i = 0; i < n / 4; i++)
        if (data[4] & 8)
            index[i] %= n + 1;
    const sc_input in = { b, 0, base, n };
    sc_cursor c;
    sc_field f;
    for (uint64_t first = 0; first <= n / 4; first += 1 + n / 16)
        if (sc_cursor_open(&c, &in, &d, index, first, n / 4) == 0)
            while (sc_cursor_next(&c, &f) == 0)
                if (f.start < base || f.start > f.end || f.end > n)
                    fail("field [%llu,%llu)", (unsigned long long)f.start, (unsigned long long)f.end);
    free(index);
}

#ifdef SC_FUZZ_MAIN
int main(int argc, char **argv) {
    static uint8_t data[4096];
    int seconds = 60;

    for (int i = 1; i + 1 < argc; i++) {
        if (strcmp(argv[i], "--seconds") == 0)
            seconds = atoi(argv[i + 1]);
        if (strcmp(argv[i], "--seed") == 0)
            seed += strtoull(argv[i + 1], NULL, 10) * 2;
    }
    sc_init();
    const time_t end = time(NULL) + seconds;
    uint64_t runs = 0;
    while (time(NULL) < end)
        for (int i = 0; i < 1000; i++, runs++) {
            const sc_dialect d = dialect(',', '"', 0);
            const size_t size = 12 + rnd(rnd(8) ? 200 : 3000);
            const int style = (int)rnd(7);
            for (size_t k = 0; k < size; k++)
                data[k] = k < 12 ? (uint8_t)rnd() : random_unit(style, d);
            if (rnd(2))
                data[2] = data[3] = 0;
            fuzz_one(data, size);
        }
    printf("fuzz: %llu inputs, %llu checks ok\n", (unsigned long long)runs, (unsigned long long)checks);
    return 0;
}
#else
extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);
extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
    fuzz_one(data, size);
    return 0;
}
#endif
