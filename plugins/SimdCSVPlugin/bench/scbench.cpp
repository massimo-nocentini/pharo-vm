/* scbench.cpp -- the speed of the engine of the SimdCSVPlugin (sc_index and
 * sc_measure) and, with -DSC_UPSTREAM on x86_64, of simdcsv's own find_indexes
 * on the same text
 *
 *   scbench [--mb N] [--repeat N] FILE...     default 64 and 9
 *   scbench --kernels                         the kernels of this machine
 *
 * Each FILE (SCBench dump:size: writes them) is repeated in memory, without
 * its header after the first time, to N MB of 1,000,000 bytes.  The kernel is
 * the one sc_init chooses: SIMDCSV_FORCE_IMPLEMENTATION selects another.
 * Lines, tab-separated (the best and the median of the repetitions, in GB/s
 * of 10^9 bytes):
 *
 *   C  data  upstream   find_indexes  best median  fields=N bytes=N
 *   C  data  <kernel>   index         ...          one call, SC_F_FINAL
 *   C  data  <kernel>   index+lines   ...          the same with SC_F_LINES
 *   C  data  <kernel>   batches       ...          calls of 1 MiB, SC_F_LINES: the reader's
 *   C  data  <kernel>   measure       ...          sc_measure, with SC_F_LINES: count:
 *
 *   C  data  ratio      index/find_indexes  the best and the median, in percent, of the
 *                                     repetitions: simdcsv and the index are timed in
 *                                     turn, so that what disturbs a moment is in both
 *
 * On regular text (no quote inside a field) the index must be upstream's; on
 * another the line of upstream says "different".  run-bench.sh builds it with
 * the flags of the VM build (clang++ -O3) and the engine without SC_TESTING
 * and, as there, without any -m flag; upstream's code (upstream.cpp) is in a
 * file of its own, with -mavx2 -mbmi -mpopcnt -mpclmul. */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include "scEngine.h"

#ifdef SC_UPSTREAM
/* upstream.cpp: */
extern uint64_t upstream_find_indexes(const uint8_t *buf, uint64_t len, uint32_t *indexes);
#endif

static double
now(void)
{
    struct timespec t;

    clock_gettime(CLOCK_MONOTONIC, &t);
    return (double)t.tv_sec + (double)t.tv_nsec * 1e-9;
}

static int
ascending(const void *a, const void *b)
{
    double x = *(const double *)a, y = *(const double *)b;

    return x < y ? -1 : x > y;
}

static int repeat = 9;

template <typename F> static double
timed(F f)
{
    double t = now();

    f();
    return now() - t;
}

/* Prints the line of the repetitions */
static void
report(const char *data, const char *who, const char *what, uint64_t bytes, uint64_t fields, const char *note, double *times)
{
    qsort(times, (size_t)repeat, sizeof times[0], ascending);
    printf("C\t%s\t%s\t%s\t%.2f\t%.2f\tfields=%llu\tbytes=%llu%s\n", data, who, what,
        (double)bytes / times[0] / 1e9, (double)bytes / times[repeat / 2] / 1e9,
        (unsigned long long)fields, (unsigned long long)bytes, note);
    fflush(stdout);
}

/* Calls f repeat times after one untimed call, and prints a line */
template <typename F> static void
series(const char *data, const char *who, const char *what, uint64_t bytes, uint64_t fields, const char *note, F f)
{
    double times[101];

    f();
    for (int i = 0; i < repeat; i++) {
        times[i] = timed(f);
    }
    report(data, who, what, bytes, fields, note, times);
}

static void
run(const char *path, uint64_t size)
{
    FILE *file = fopen(path, "rb");
    char name[64];
    const char *slash = strrchr(path, '/');

    if (!file) {
        fprintf(stderr, "scbench: cannot open %s\n", path);
        exit(1);
    }
    snprintf(name, sizeof name, "%s", slash ? slash + 1 : path);
    if (strrchr(name, '.')) {
        *strrchr(name, '.') = 0;
    }
    fseek(file, 0, SEEK_END);
    uint64_t length = (uint64_t)ftell(file);
    rewind(file);
    uint8_t *b = (uint8_t *)malloc(size + length + 4096);
    uint32_t *dest = (uint32_t *)malloc(4 * (size + length) / 2 + 4096), *theirs;
    if (!b || !dest || fread(b, 1, length, file) != length) {
        fprintf(stderr, "scbench: %s\n", path);
        exit(1);
    }
    fclose(file);
    const uint8_t *newline = (const uint8_t *)memchr(b, '\n', length);
    uint64_t header = newline ? (uint64_t)(newline - b) + 1 : 0, n = length;
    while (n < size) {
        memcpy(b + n, b + header, length - header);
        n += length - header;
    }
    memset(b + n, 0, 256);

    const sc_input in = { b, 0, 0, n };
    sc_dialect d = { ',', '"', SC_F_FINAL };
    sc_scan s;
    uint64_t capacity = (4 * (size + length) / 2 + 4096) / 4;
    sc_index(&in, &d, 0, dest, capacity, &s);	/* touches the pages of dest too */
    if (s.status != SC_OK) {
        fprintf(stderr, "scbench: %s: status %d\n", path, s.status);
        exit(1);
    }
    uint64_t fields = s.stored;
#ifdef SC_UPSTREAM
    if (getenv("SIMDCSV_FORCE_IMPLEMENTATION") == NULL) {	/* once, not for each kernel */
        theirs = (uint32_t *)malloc(4 * capacity);
        uint64_t count = upstream_find_indexes(b, n + 64, theirs);
        while (count && theirs[count - 1] >= n) {	/* phantom separators of the padding */
            count--;
        }
        int same = count == fields && memcmp(theirs, dest, 4 * count) == 0;
        double upstream[101], index[101], ratio[101];
        sc_index(&in, &d, 0, dest, capacity, &s);
        for (int i = 0; i < repeat; i++) {
            upstream[i] = timed([&] { upstream_find_indexes(b, n + 64, theirs); });
            index[i] = timed([&] { sc_index(&in, &d, 0, dest, capacity, &s); });
            ratio[i] = upstream[i] / index[i];
        }
        report(name, "upstream", "find_indexes", n, count, same ? "\tsame" : "\tdifferent", upstream);
        report(name, sc_kernel(), "index", n, fields, "", index);
        qsort(ratio, (size_t)repeat, sizeof ratio[0], ascending);
        printf("C\t%s\tratio\tindex/find_indexes\t%.1f\t%.1f\tleast=%.1f\n", name, ratio[repeat - 1] * 100,
            ratio[repeat / 2] * 100, ratio[0] * 100);
        free(theirs);
    } else
#else
    (void)theirs;
#endif
    series(name, sc_kernel(), "index", n, fields, "", [&] { sc_index(&in, &d, 0, dest, capacity, &s); });
    d.flags |= SC_F_LINES;
    series(name, sc_kernel(), "index+lines", n, fields, "", [&] { sc_index(&in, &d, 0, dest, capacity, &s); });
    series(name, sc_kernel(), "batches", n, fields, "", [&] {
        for (uint64_t from = 0; from < n; from = s.next) {
            sc_dialect batch = d;
            sc_input part = { b, 0, from, n - from > 1048576 ? from + 1048576 : n };
            if (part.to < n) {
                batch.flags &= ~(uint32_t)SC_F_FINAL;
            }
            sc_index(&part, &batch, from, dest, capacity, &s);
            if (s.next == from) {
                abort();
            }
        }
    });
    series(name, sc_kernel(), "measure", n, fields, "", [&] { sc_measure(&in, &d, &s); });
    free(b);
    free(dest);
}

int
main(int argc, char **argv)
{
    uint64_t size = 64000000;
    int i = 1;

    sc_init();
    for (; i < argc && argv[i][0] == '-'; i++) {
        if (!strcmp(argv[i], "--kernels")) {
            for (int k = 0; k < sc_kernel_count(); k++) {
                puts(sc_kernel_name(k));
            }
            return 0;
        }
        if (i + 1 < argc && !strcmp(argv[i], "--mb")) {
            size = strtoull(argv[++i], NULL, 10) * 1000000;
        } else if (i + 1 < argc && !strcmp(argv[i], "--repeat") && atoi(argv[i + 1]) > 0 && atoi(argv[i + 1]) <= 101) {
            repeat = atoi(argv[++i]);
        } else {
            fprintf(stderr, "usage: scbench [--mb N] [--repeat N] FILE... | --kernels\n");
            return 2;
        }
    }
    for (; i < argc; i++) {
        run(argv[i], size);
    }
    return 0;
}
