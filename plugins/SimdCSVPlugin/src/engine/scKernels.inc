/* scKernels.inc -- the kernels, one namespace each, included by scEngine.cpp
 * (inside its anonymous namespace): sc_avx2 and sc_neon hold the slice of
 * simdcsv, sc_sse2 and sc_fallback define its five names themselves.  Each
 * namespace then includes scKernelBody.inc, the block and the loops.
 *
 * No compiler flag selects an instruction set: sc_avx2 is compiled under a
 * target pragma with __AVX2__ defined by hand, and runs only where
 * __builtin_cpu_supports answers so. */

/* bit i of the answer: the XOR of the bits 0 to i of x */
sc_inline uint64_t sc_prefix_xor(uint64_t x) {
    x ^= x << 1;
    x ^= x << 2;
    x ^= x << 4;
    x ^= x << 8;
    x ^= x << 16;
    return x ^ x << 32;
}

/* What the slice needs, per namespace (a function is inlined into callers of
 * its own target only), and our flatten_bits, which writes the bits' slots
 * only (the slice's writes up to 15 more) */
#define really_inline sc_inline
#define SC_BIT_FUNCTIONS \
    sc_inline int trailingzeroes(uint64_t x) { return x ? __builtin_ctzll(x) : 64; } \
    sc_inline int hamming(uint64_t x) { return __builtin_popcountll(x); }
/* (x86_64 without popcnt: __builtin_popcountll would call libgcc) */
#define SC_BIT_FUNCTIONS_SHIFTS \
    sc_inline int trailingzeroes(uint64_t x) { return x ? __builtin_ctzll(x) : 64; } \
    sc_inline int hamming(uint64_t x) { \
        x -= x >> 1 & UINT64_C(0x5555555555555555); \
        x = (x & UINT64_C(0x3333333333333333)) + (x >> 2 & UINT64_C(0x3333333333333333)); \
        x = (x + (x >> 4)) & UINT64_C(0x0f0f0f0f0f0f0f0f); \
        return (int)(x * UINT64_C(0x0101010101010101) >> 56); \
    }
#define SC_FLATTEN_BITS \
    sc_inline void flatten_bits(uint32_t *to, uint32_t &base, uint32_t at, uint64_t bits) { \
        uint32_t *p = to + base; \
        while (bits) { \
            *p++ = at + (uint32_t)__builtin_ctzll(bits); \
            bits &= bits - 1; \
        } \
        base = (uint32_t)(p - to); \
    }

#if defined(__x86_64__)

/* ------------------------------------------------------------------- avx2 */
#if defined(__clang__)
# pragma clang attribute push(__attribute__((target("avx2,bmi,popcnt,pclmul"))), apply_to = function)
#else
# pragma GCC push_options
# pragma GCC target("avx2,bmi,popcnt,pclmul")
#endif
#ifndef __AVX2__
# define __AVX2__ 1
# define SC_DEFINED_AVX2
#endif
namespace sc_avx2 {
SC_BIT_FUNCTIONS
#include "simdcsv_slice.inc"
sc_inline simd_input flip_input(simd_input in, uint8_t flip) {
    const __m256i x = _mm256_set1_epi8((char)flip);
    in.lo = _mm256_xor_si256(in.lo, x);
    in.hi = _mm256_xor_si256(in.hi, x);
    return in;
}
#include "scKernelBody.inc"
}
#ifdef SC_DEFINED_AVX2
# undef __AVX2__
# undef SC_DEFINED_AVX2
#endif
#if defined(__clang__)
# pragma clang attribute pop
#else
# pragma GCC pop_options
#endif

/* ------------------------------------------------------------------- sse2 */
namespace sc_sse2 {
SC_BIT_FUNCTIONS_SHIFTS
struct simd_input {
    __m128i v[4];
};
sc_inline simd_input fill_input(const uint8_t *p) {
    simd_input in;
    for (int i = 0; i < 4; i++)
        in.v[i] = _mm_loadu_si128((const __m128i *)(const void *)(p + 16 * i));
    return in;
}
sc_inline simd_input flip_input(simd_input in, uint8_t flip) {
    const __m128i x = _mm_set1_epi8((char)flip);
    for (int i = 0; i < 4; i++)
        in.v[i] = _mm_xor_si128(in.v[i], x);
    return in;
}
sc_inline uint64_t cmp_mask_against_input(simd_input in, uint8_t m) {
    const __m128i x = _mm_set1_epi8((char)m);
    uint64_t mask = 0;
    for (int i = 0; i < 4; i++)
        mask |= (uint64_t)(uint32_t)_mm_movemask_epi8(_mm_cmpeq_epi8(in.v[i], x)) << (16 * i);
    return mask;
}
sc_inline uint64_t find_quote_mask(simd_input in, uint64_t &inside) {
    const uint64_t mask = sc_prefix_xor(cmp_mask_against_input(in, '"')) ^ inside;
    inside = 0 - (mask >> 63);
    return mask;
}
SC_FLATTEN_BITS
#include "scKernelBody.inc"
}

#elif defined(__aarch64__)

/* ------------------------------------------------------------------- neon */
namespace sc_neon {
SC_BIT_FUNCTIONS
sc_inline uint64_t neonmovemask_bulk(uint8x16_t a, uint8x16_t b, uint8x16_t c, uint8x16_t d) {
    const uint8x16_t bits = { 1, 2, 4, 8, 16, 32, 64, 128, 1, 2, 4, 8, 16, 32, 64, 128 };
    uint8x16_t x = vpaddq_u8(vandq_u8(a, bits), vandq_u8(b, bits));
    const uint8x16_t y = vpaddq_u8(vandq_u8(c, bits), vandq_u8(d, bits));
    x = vpaddq_u8(x, y);
    x = vpaddq_u8(x, x);
    return vgetq_lane_u64(vreinterpretq_u64_u8(x), 0);
}
#undef vmull_p64
#define vmull_p64(a, b) sc_prefix_xor(b)
#include "simdcsv_slice.inc"
#undef vmull_p64
sc_inline simd_input flip_input(simd_input in, uint8_t flip) {
    const uint8x16_t x = vdupq_n_u8(flip);
    in.i0 = veorq_u8(in.i0, x);
    in.i1 = veorq_u8(in.i1, x);
    in.i2 = veorq_u8(in.i2, x);
    in.i3 = veorq_u8(in.i3, x);
    return in;
}
#include "scKernelBody.inc"
}

#endif

/* --------------------------------------------------------------- fallback */
/* 64 bits at a time (little-endian), everywhere; the only one on wasm */
namespace sc_fallback {
#if defined(__x86_64__)
SC_BIT_FUNCTIONS_SHIFTS
#else
SC_BIT_FUNCTIONS
#endif
struct simd_input {
    uint64_t w[8];
};
sc_inline simd_input fill_input(const uint8_t *p) {
    simd_input in;
    memcpy(in.w, p, 64);
    return in;
}
sc_inline simd_input flip_input(simd_input in, uint8_t flip) {
    const uint64_t x = UINT64_C(0x0101010101010101) * flip;
    for (int i = 0; i < 8; i++)
        in.w[i] ^= x;
    return in;
}
sc_inline uint64_t cmp_mask_against_input(simd_input in, uint8_t m) {
    const uint64_t l = UINT64_C(0x7f7f7f7f7f7f7f7f), needle = UINT64_C(0x0101010101010101) * m;
    uint64_t mask = 0;
    for (int i = 0; i < 8; i++) {
        const uint64_t x = in.w[i] ^ needle, t = (x & l) + l;
        /* the exact test: 0x80 in the bytes that are 0, and nothing else
         * (the usual shorter one is wrong on a needle followed by needle^1) */
        const uint64_t hit = ~(t | x | l);
        mask |= hit * UINT64_C(0x0002040810204081) >> 56 << (8 * i);
    }
    return mask;
}
sc_inline uint64_t find_quote_mask(simd_input in, uint64_t &inside) {
    const uint64_t mask = sc_prefix_xor(cmp_mask_against_input(in, '"')) ^ inside;
    inside = 0 - (mask >> 63);
    return mask;
}
SC_FLATTEN_BITS
#include "scKernelBody.inc"
}

#undef SC_BIT_FUNCTIONS
#undef SC_BIT_FUNCTIONS_SHIFTS
#undef SC_FLATTEN_BITS
#undef really_inline
