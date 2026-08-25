//! The integer inverse DCT, `JPEGReaderPlugin>>#idctBlockInt:qt:`.
//!
//! A Loeffler-Ligtenberg-Moshovitz 8x8 IDCT in 13-bit fixed point, fused
//! with dequantisation. The transcription keeps the C's exact temporary
//! widths: `t2`, `t3`, `z1`..`z3` and the `ws` scratch block are `int`
//! (32-bit), while `t0`, `t1`, `t10`..`t13`, `z4`, `z5` are `sqInt`
//! (register-width) — so several intermediate sums truncate to 32 bits, and
//! that truncation is part of the observable behaviour being ported. All
//! arithmetic wraps, as the C build's does.

use pharo_vm_plugin::sqInt;

use crate::{DCT_SIZE, DCT_SIZE2, MAX_SAMPLE, SAMPLE_OFFSET};

const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;
/// End of pass 1: descale by 2^11 (ConstBits - Pass1Bits), by *division*, so
/// it rounds toward zero rather than toward negative infinity.
const PASS1_DIV: sqInt = 0x800;
/// End of pass 2: descale by 2^18 (ConstBits + Pass1Bits + 3).
const PASS2_DIV: sqInt = 0x40000;

const FIX_0_298631336: i32 = 2446;
const FIX_0_390180644: i32 = 0xC7C;
const FIX_0_541196100: i32 = 4433;
const FIX_0_765366865: i32 = 6270;
const FIX_0_899976223: i32 = 7373;
const FIX_1_175875602: i32 = 9633;
const FIX_1_501321110: i32 = 12299;
const FIX_1_847759065: i32 = 15137;
const FIX_1_961570560: i32 = 16069;
const FIX_2_053119869: i32 = 16819;
const FIX_2_562915447: i32 = 20995;
const FIX_3_072711026: i32 = 25172;

/// `v + SampleOffset`, clamped high then low, exactly in the C's order.
fn clamp_sample(v: sqInt) -> i32 {
    let v = v.wrapping_add(SAMPLE_OFFSET);
    let v = if v < MAX_SAMPLE { v } else { MAX_SAMPLE };
    let v = if v < 0 { 0 } else { v };
    v as i32
}

/// Dequantises and inverse-transforms one block in place: coefficients in,
/// samples offset to 0..=255 out.
pub fn idct_block_int(an_array: &mut [i32; DCT_SIZE2], qt: &[i32; DCT_SIZE2]) {
    let mut ws = [0i32; DCT_SIZE2];

    // Pass 1: columns, into ws.
    for i in 0..DCT_SIZE {
        let mut ac_term: sqInt = -1;
        for row in 1..DCT_SIZE {
            if ac_term == -1 && an_array[row * DCT_SIZE + i] != 0 {
                ac_term = row as sqInt;
            }
        }
        if ac_term == -1 {
            // DC-only column. The C dequantises with qt[0] here, not qt[i]
            // — odd, but faithful.
            let dcval = an_array[i].wrapping_mul(qt[0]).wrapping_shl(PASS1_BITS);
            for j in 0..DCT_SIZE {
                ws[j * DCT_SIZE + i] = dcval;
            }
            continue;
        }

        // Even part.
        let mut z2: i32 = an_array[DCT_SIZE * 2 + i].wrapping_mul(qt[DCT_SIZE * 2 + i]);
        let mut z3: i32 = an_array[DCT_SIZE * 6 + i].wrapping_mul(qt[DCT_SIZE * 6 + i]);
        let mut z1: i32 = z2.wrapping_add(z3).wrapping_mul(FIX_0_541196100);
        let mut t2: i32 = z1.wrapping_add(z3.wrapping_mul(-FIX_1_847759065));
        let mut t3: i32 = z1.wrapping_add(z2.wrapping_mul(FIX_0_765366865));
        z2 = an_array[i].wrapping_mul(qt[i]);
        z3 = an_array[DCT_SIZE * 4 + i].wrapping_mul(qt[DCT_SIZE * 4 + i]);
        let mut t0: sqInt = (z2.wrapping_add(z3) as sqInt) << CONST_BITS;
        let mut t1: sqInt = (z2.wrapping_sub(z3) as sqInt) << CONST_BITS;
        let t10 = t0.wrapping_add(t3 as sqInt);
        let t13 = t0.wrapping_sub(t3 as sqInt);
        let t11 = t1.wrapping_add(t2 as sqInt);
        let t12 = t1.wrapping_sub(t2 as sqInt);

        // Odd part. The products are 32-bit in C even when assigned to the
        // sqInt t0/t1.
        t0 = an_array[DCT_SIZE * 7 + i].wrapping_mul(qt[DCT_SIZE * 7 + i]) as sqInt;
        t1 = an_array[DCT_SIZE * 5 + i].wrapping_mul(qt[DCT_SIZE * 5 + i]) as sqInt;
        t2 = an_array[DCT_SIZE * 3 + i].wrapping_mul(qt[DCT_SIZE * 3 + i]);
        t3 = an_array[DCT_SIZE + i].wrapping_mul(qt[DCT_SIZE + i]);
        z1 = t0.wrapping_add(t3 as sqInt) as i32;
        z2 = t1.wrapping_add(t2 as sqInt) as i32;
        z3 = t0.wrapping_add(t2 as sqInt) as i32;
        let mut z4: sqInt = t1.wrapping_add(t3 as sqInt);
        let z5: sqInt = (z3 as sqInt)
            .wrapping_add(z4)
            .wrapping_mul(FIX_1_175875602 as sqInt);
        t0 = t0.wrapping_mul(FIX_0_298631336 as sqInt);
        t1 = t1.wrapping_mul(FIX_2_053119869 as sqInt);
        t2 = t2.wrapping_mul(FIX_3_072711026);
        t3 = t3.wrapping_mul(FIX_1_501321110);
        z1 = z1.wrapping_mul(-FIX_0_899976223);
        z2 = z2.wrapping_mul(-FIX_2_562915447);
        z3 = z3.wrapping_mul(-FIX_1_961570560);
        z4 = z4.wrapping_mul((-FIX_0_390180644) as sqInt);
        z3 = (z3 as sqInt).wrapping_add(z5) as i32;
        z4 = z4.wrapping_add(z5);
        t0 = t0.wrapping_add(z1 as sqInt).wrapping_add(z3 as sqInt);
        t1 = t1.wrapping_add(z2 as sqInt).wrapping_add(z4);
        t2 = t2.wrapping_add(z2).wrapping_add(z3);
        t3 = (t3.wrapping_add(z1) as sqInt).wrapping_add(z4) as i32;

        ws[i] = (t10.wrapping_add(t3 as sqInt) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 7 + i] = (t10.wrapping_sub(t3 as sqInt) / PASS1_DIV) as i32;
        ws[DCT_SIZE + i] = (t11.wrapping_add(t2 as sqInt) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 6 + i] = (t11.wrapping_sub(t2 as sqInt) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 2 + i] = (t12.wrapping_add(t1) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 5 + i] = (t12.wrapping_sub(t1) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 3 + i] = (t13.wrapping_add(t0) / PASS1_DIV) as i32;
        ws[DCT_SIZE * 4 + i] = (t13.wrapping_sub(t0) / PASS1_DIV) as i32;
    }

    // Pass 2: rows, out of ws into the block, offset and clamped.
    for i in (0..DCT_SIZE2).step_by(DCT_SIZE) {
        // Even part.
        let mut z2: i32 = ws[i + 2];
        let mut z3: i32 = ws[i + 6];
        let mut z1: i32 = z2.wrapping_add(z3).wrapping_mul(FIX_0_541196100);
        let mut t2: i32 = z1.wrapping_add(z3.wrapping_mul(-FIX_1_847759065));
        let mut t3: i32 = z1.wrapping_add(z2.wrapping_mul(FIX_0_765366865));
        let mut t0: sqInt = (ws[i].wrapping_add(ws[i + 4]) as sqInt) << CONST_BITS;
        let mut t1: sqInt = (ws[i].wrapping_sub(ws[i + 4]) as sqInt) << CONST_BITS;
        let t10 = t0.wrapping_add(t3 as sqInt);
        let t13 = t0.wrapping_sub(t3 as sqInt);
        let t11 = t1.wrapping_add(t2 as sqInt);
        let t12 = t1.wrapping_sub(t2 as sqInt);

        // Odd part.
        t0 = ws[i + 7] as sqInt;
        t1 = ws[i + 5] as sqInt;
        t2 = ws[i + 3];
        t3 = ws[i + 1];
        z1 = t0.wrapping_add(t3 as sqInt) as i32;
        z2 = t1.wrapping_add(t2 as sqInt) as i32;
        z3 = t0.wrapping_add(t2 as sqInt) as i32;
        let mut z4: sqInt = t1.wrapping_add(t3 as sqInt);
        let z5: sqInt = (z3 as sqInt)
            .wrapping_add(z4)
            .wrapping_mul(FIX_1_175875602 as sqInt);
        t0 = t0.wrapping_mul(FIX_0_298631336 as sqInt);
        t1 = t1.wrapping_mul(FIX_2_053119869 as sqInt);
        t2 = t2.wrapping_mul(FIX_3_072711026);
        t3 = t3.wrapping_mul(FIX_1_501321110);
        z1 = z1.wrapping_mul(-FIX_0_899976223);
        z2 = z2.wrapping_mul(-FIX_2_562915447);
        z3 = z3.wrapping_mul(-FIX_1_961570560);
        z4 = z4.wrapping_mul((-FIX_0_390180644) as sqInt);
        z3 = (z3 as sqInt).wrapping_add(z5) as i32;
        z4 = z4.wrapping_add(z5);
        t0 = t0.wrapping_add(z1 as sqInt).wrapping_add(z3 as sqInt);
        t1 = t1.wrapping_add(z2 as sqInt).wrapping_add(z4);
        t2 = t2.wrapping_add(z2).wrapping_add(z3);
        t3 = (t3.wrapping_add(z1) as sqInt).wrapping_add(z4) as i32;

        an_array[i] = clamp_sample(t10.wrapping_add(t3 as sqInt) / PASS2_DIV);
        an_array[i + 7] = clamp_sample(t10.wrapping_sub(t3 as sqInt) / PASS2_DIV);
        an_array[i + 1] = clamp_sample(t11.wrapping_add(t2 as sqInt) / PASS2_DIV);
        an_array[i + 6] = clamp_sample(t11.wrapping_sub(t2 as sqInt) / PASS2_DIV);
        an_array[i + 2] = clamp_sample(t12.wrapping_add(t1) / PASS2_DIV);
        an_array[i + 5] = clamp_sample(t12.wrapping_sub(t1) / PASS2_DIV);
        an_array[i + 3] = clamp_sample(t13.wrapping_add(t0) / PASS2_DIV);
        an_array[i + 4] = clamp_sample(t13.wrapping_sub(t0) / PASS2_DIV);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// The textbook float IDCT with dequantisation and the +127 offset —
    /// slow but obviously right, for tolerance comparisons.
    fn reference_idct(block: &[i32; 64], qt: &[i32; 64]) -> [f64; 64] {
        let mut out = [0f64; 64];
        for y in 0..8 {
            for x in 0..8 {
                let mut sum = 0.0;
                for v in 0..8 {
                    for u in 0..8 {
                        let cu = if u == 0 { 1.0 / 2f64.sqrt() } else { 1.0 };
                        let cv = if v == 0 { 1.0 / 2f64.sqrt() } else { 1.0 };
                        let f = (block[v * 8 + u] as f64) * (qt[v * 8 + u] as f64);
                        sum += cu
                            * cv
                            * f
                            * (((2 * x + 1) as f64) * (u as f64) * PI / 16.0).cos()
                            * (((2 * y + 1) as f64) * (v as f64) * PI / 16.0).cos();
                    }
                }
                out[y * 8 + x] = sum / 4.0 + 127.0;
            }
        }
        out
    }

    #[test]
    fn all_zero_coefficients_give_a_flat_offset_block() {
        let mut block = [0i32; 64];
        let qt = [1i32; 64];
        idct_block_int(&mut block, &qt);
        assert!(block.iter().all(|&v| v == 127), "{block:?}");
    }

    #[test]
    fn dc_only_block_is_flat_at_the_scaled_dc() {
        let mut block = [0i32; 64];
        block[0] = 8;
        let mut qt = [1i32; 64];
        qt[0] = 4;
        idct_block_int(&mut block, &qt);
        // Dequantised DC 32 -> 32/8 + 127 = 131, exactly, everywhere.
        assert!(block.iter().all(|&v| v == 131), "{block:?}");
    }

    #[test]
    fn matches_the_float_reference_on_mixed_coefficients() {
        // Uniform qt sidesteps the C's qt[0] quirk on DC-only columns, so
        // the reference stays honest.
        let qt = [16i32; 64];
        let mut block = [0i32; 64];
        // A deterministic scatter of small coefficients.
        for (k, coeff) in block.iter_mut().enumerate() {
            let k = k as i32;
            *coeff = ((k * 7) % 13) - 6; // -6..=6, every column non-flat
        }
        block[0] = 40;
        let expected = reference_idct(&block, &qt);
        let mut actual = block;
        idct_block_int(&mut actual, &qt);
        for k in 0..64 {
            let want = expected[k].clamp(0.0, 255.0);
            let got = actual[k] as f64;
            assert!(
                (got - want).abs() <= 2.0,
                "sample {k}: got {got}, reference {want}"
            );
        }
    }

    #[test]
    fn single_ac_coefficient_matches_the_reference() {
        let qt = [8i32; 64];
        let mut block = [0i32; 64];
        block[1] = 20; // horizontal cosine
        block[8] = -10; // vertical cosine
        let expected = reference_idct(&block, &qt);
        let mut actual = block;
        idct_block_int(&mut actual, &qt);
        for k in 0..64 {
            let want = expected[k].clamp(0.0, 255.0);
            assert!(
                (actual[k] as f64 - want).abs() <= 2.0,
                "sample {k}: got {}, reference {want}",
                actual[k]
            );
        }
    }

    #[test]
    fn saturates_high_and_low() {
        let qt = [16i32; 64];
        let mut high = [0i32; 64];
        high[0] = 10_000;
        idct_block_int(&mut high, &qt);
        assert!(high.iter().all(|&v| v == 255), "{high:?}");

        let mut low = [0i32; 64];
        low[0] = -10_000;
        idct_block_int(&mut low, &qt);
        assert!(low.iter().all(|&v| v == 0), "{low:?}");
    }

    #[test]
    fn descaling_divides_toward_zero() {
        // A DC just below zero after the offset: division (not shift)
        // means -1/Pass2Div rounds to 0, so the result sits at the offset
        // rather than one below it. dequantised DC of -8 gives exactly
        // -1 per sample before descaling in pass 2.
        let qt = [1i32; 64];
        let mut block = [0i32; 64];
        block[0] = -8;
        idct_block_int(&mut block, &qt);
        // -8/8 = -1 -> 127 - 1 = 126 everywhere.
        assert!(block.iter().all(|&v| v == 126), "{block:?}");
    }
}
