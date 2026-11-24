/* upstream.cpp -- simdcsv's own find_indexes for scbench.cpp, in a file of its
 * own, which run-bench.sh compiles with -mavx2 -mbmi -mpopcnt -mpclmul, all that
 * the code needs.  simdcsv_upstream.inc is the part of src/main.cpp from
 * "struct ParsedCSV" to main, cut by run-bench.sh (the slice of the engine,
 * and upstream's loop) */
#include <x86intrin.h>
#include <stdint.h>
#include <stddef.h>
#define really_inline static inline __attribute__((always_inline, unused))
really_inline int trailingzeroes(uint64_t x) { return __builtin_ctzll(x); }
really_inline int hamming(uint64_t x) { return __builtin_popcountll(x); }
#include "simdcsv_upstream.inc"

uint64_t
upstream_find_indexes(const uint8_t *buf, uint64_t len, uint32_t *indexes)
{
    ParsedCSV p;

    p.indexes = indexes;
    find_indexes(buf, len, p);
    return p.n_indexes;
}
