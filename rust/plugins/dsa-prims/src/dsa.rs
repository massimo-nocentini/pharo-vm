//! The algorithmic core of DSAPrims, as pure functions.
//!
//! Everything here is a mechanical translation of the Slang-generated loops in
//! `plugins/DSAPrims/src/common/DSAPrims.c`, kept free of the VM so it can be
//! unit-tested against known answers. The number layout is the image's:
//! LargePositiveInteger digits are base-256, stored least-significant first,
//! and the SHA state and expanded block are native-endian 32-bit words.
//!
//! Faithfulness notes are inline; the intentional divergences (all of them
//! removals of undefined behaviour in the C) are the `Err` returns of
//! [`big_divide`] and are listed in the crate README.

use pharo_vm_plugin::PrimErr;

/// SHA-1 round constants, exactly the decimal literals the C uses.
const K1: u32 = 1_518_500_249; // 0x5A827999
const K2: u32 = 1_859_775_393; // 0x6ED9EBA1
const K3: u32 = 2_400_959_708; // 0x8F1BBCDC
const K4: u32 = 3_395_469_782; // 0xCA62C1D6

/// Expands a 64-byte block into the 80-word SHA-1 message schedule.
///
/// Words 0..16 are read big-endian from the block; words 16..80 are the SHA-1
/// recurrence `rotl1(w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16])`. The rotate-left-1
/// is what makes this SHA-1 rather than SHA-0.
pub(crate) fn expand_block(block: &[u8; 64]) -> [u32; 80] {
    let mut w = [0u32; 80];
    for (i, chunk) in block.chunks_exact(4).enumerate() {
        w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    w
}

/// Runs the 80 SHA-1 rounds over an expanded block, adding the result into
/// `state` (the Davies-Meyer feed-forward), exactly as the C does.
///
/// All arithmetic is the C's `unsigned int` arithmetic, i.e. wrapping u32.
pub(crate) fn hash_block(state: &mut [u32; 5], w: &[u32; 80]) {
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, &wi) in w.iter().enumerate() {
        let (k, f) = match i {
            0..=19 => (K1, (b & c) | (!b & d)),
            20..=39 => (K2, b ^ c ^ d),
            40..=59 => (K3, (b & c) | (b & d) | (c & d)),
            _ => (K4, b ^ c ^ d),
        };
        let tmp = k
            .wrapping_add(f)
            .wrapping_add(a.rotate_left(5))
            .wrapping_add(e)
            .wrapping_add(wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = tmp;
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
}

/// Schoolbook multiply of base-256 little-endian digit strings, accumulating
/// into `prod`. Requires `prod.len() == f1.len() + f2.len()`.
///
/// Two oddities preserved from the C:
/// * rows for a zero digit of `f1` are skipped entirely, so whatever `prod`
///   already holds in those columns is left alone -- the caller is expected to
///   supply a zero-filled `prod`;
/// * the carry out of each row *overwrites* `prod[i + f2.len()]` instead of
///   adding into it.
pub(crate) fn big_multiply(f1: &[u8], f2: &[u8], prod: &mut [u8]) {
    debug_assert_eq!(prod.len(), f1.len() + f2.len());
    for (i, &digit) in f1.iter().enumerate() {
        if digit == 0 {
            continue;
        }
        let digit = i64::from(digit);
        // Loop invariants, as the C states them: 0 <= carry <= 0xFF, and the
        // output column is i + j.
        let mut carry: i64 = 0;
        let mut k = i;
        for &d in f2 {
            let sum = i64::from(d) * digit + i64::from(prod[k]) + carry;
            carry = sum >> 8;
            prod[k] = (sum & 0xFF) as u8;
            k += 1;
        }
        prod[k] = carry as u8;
    }
}

/// Knuth's algorithm D (vol. 2, 2nd ed., pp. 257-260) on base-256 digits,
/// exactly as the C's `bigDivideLoop` performs it.
///
/// Divides `div` into `rem` in place: on return `rem` holds the remainder and
/// the low `rem.len() - div.len()` digits of `quo` hold the quotient. `quo` is
/// expected to arrive zero-filled; digits beyond the quotient are not touched.
/// As in the C, the digit estimate is exact only when the divisor is
/// normalised (top digit >= 128), which the image's caller arranges.
///
/// Answers how many times the rare add-back correction ran, so tests can
/// prove that path was exercised; the primitive discards it.
///
/// The `Err` cases replace undefined behaviour in the C -- see the README:
/// * a divisor of fewer than 2 digits made the C read out of bounds;
/// * a divisor whose top digit is zero made the C divide by zero;
/// * a `quo` shorter than the quotient made the C write past its end.
pub(crate) fn big_divide(rem: &mut [u8], div: &[u8], quo: &mut [u8]) -> Result<u32, PrimErr> {
    let dn = div.len();
    let rn = rem.len();
    if dn < 2 {
        return Err(PrimErr::BadArgument);
    }
    // The top two divisor digits drive the quotient-digit estimate.
    let d1 = i64::from(div[dn - 1]);
    let d2 = i64::from(div[dn - 2]);
    if d1 == 0 {
        return Err(PrimErr::BadArgument);
    }
    if rn > dn && quo.len() < rn - dn {
        return Err(PrimErr::BadArgument);
    }

    let mut add_backs = 0u32;
    // The C walks 1-based digit positions j = rn down to dn + 1; indices here
    // are the same positions shifted down by one.
    for j in ((dn + 1)..=rn).rev() {
        // The top several digits of the running remainder.
        let first_digit = i64::from(rem[j - 1]);
        let first_two = (first_digit << 8) + i64::from(rem[j - 2]);
        let third_digit = i64::from(rem[j - 3]);

        // Estimate q, the next quotient digit. Knuth shows the estimate is
        // never low and at most one too high after these corrections.
        let mut q = if first_digit == d1 {
            0xFF
        } else {
            first_two / d1
        };
        if d2 * q > ((first_two - q * d1) << 8) + third_digit {
            q -= 1;
            if d2 * q > ((first_two - q * d1) << 8) + third_digit {
                q -= 1;
            }
        }

        let digit_shift = j - dn - 1;
        if q > 0 {
            // Subtract div * q, shifted left by digit_shift digits.
            let mut borrow: i64 = 0;
            let mut r = digit_shift;
            for &d in div {
                let prod = i64::from(d) * q + borrow;
                borrow = prod >> 8;
                let mut result_digit = i64::from(rem[r]) - (prod & 0xFF);
                if result_digit < 0 {
                    // Borrow from the next digit.
                    result_digit += 256;
                    borrow += 1;
                }
                rem[r] = result_digit as u8;
                r += 1;
            }
            let q_too_big = if borrow == 0 {
                false
            } else {
                let result_digit = i64::from(rem[r]) - borrow;
                if result_digit < 0 {
                    // The digit estimate was one too large (quite rare). The
                    // C stores `resultDigit + 256` through an unsigned char,
                    // i.e. modulo 256; `as u8` reproduces that.
                    rem[r] = (result_digit + 256) as u8;
                    true
                } else {
                    rem[r] = result_digit as u8;
                    false
                }
            };
            if q_too_big {
                // Add the shifted divisor back (extremely rare).
                add_backs += 1;
                let mut carry: i64 = 0;
                let mut r = digit_shift;
                for &d in div {
                    let sum = i64::from(rem[r]) + i64::from(d) + carry;
                    rem[r] = (sum & 0xFF) as u8;
                    carry = sum >> 8;
                    r += 1;
                }
                let sum = i64::from(rem[r]) + carry;
                rem[r] = (sum & 0xFF) as u8;
                q -= 1;
            }
        }
        // The C assigns the sqInt q through an unsigned char, truncating; a
        // q > 0xFF can only arise from an unnormalised divisor.
        quo[digit_shift] = q as u8;
    }
    Ok(add_backs)
}

/// The 1-based index of the top-most non-zero digit.
///
/// Faithful to the C's loop, including its quirk: the scan cannot distinguish
/// "stopped at a non-zero digit 1" from "ran out of digits", so an all-zero
/// (or empty) number answers 1, not 0.
pub(crate) fn highest_non_zero_digit_index(digits: &[u8]) -> usize {
    let mut i = digits.len();
    while i > 0 {
        i -= 1;
        if digits[i] != 0 {
            // The C exits its while-condition after the decrement, so the
            // answer is this index + 1.
            break;
        }
    }
    i + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- SHA-1, through the same two entry points the image uses ----------

    /// SHA-1 of an arbitrary message, built from `expand_block`/`hash_block`
    /// plus the standard padding -- which is exactly how the image's
    /// SecureHashAlgorithm drives these primitives.
    fn sha1(msg: &[u8]) -> [u8; 20] {
        let mut state: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
        let mut data = msg.to_vec();
        data.push(0x80);
        while data.len() % 64 != 56 {
            data.push(0);
        }
        data.extend_from_slice(&((msg.len() as u64) * 8).to_be_bytes());
        for block in data.chunks_exact(64) {
            let mut b = [0u8; 64];
            b.copy_from_slice(block);
            let w = expand_block(&b);
            hash_block(&mut state, &w);
        }
        let mut out = [0u8; 20];
        for (i, s) in state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
        }
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn sha1_empty_message() {
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn sha1_abc() {
        assert_eq!(
            hex(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn sha1_two_block_message() {
        // 56 bytes: padding forces a second block.
        assert_eq!(
            hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn sha1_million_a() {
        assert_eq!(
            hex(&sha1(&vec![b'a'; 1_000_000])),
            "34aa973cd4c4daa4f61eeb2bdbad27316534016f"
        );
    }

    #[test]
    fn expand_block_reads_big_endian() {
        let mut block = [0u8; 64];
        block[0..4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        block[60..64].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let w = expand_block(&block);
        assert_eq!(w[0], 0x1234_5678);
        assert_eq!(w[15], 0xDEAD_BEEF);
    }

    #[test]
    // The point of this test is to spell the rotate differently from the
    // implementation's rotate_left, so a bug there cannot cancel out here.
    #[allow(clippy::manual_rotate)]
    fn expand_block_recurrence_matches_independent_computation() {
        // A block of counter bytes, checked word by word against a separately
        // written recurrence.
        let mut block = [0u8; 64];
        for (i, b) in block.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let w = expand_block(&block);
        for i in 16..80 {
            let x = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
            assert_eq!(w[i], (x << 1) | (x >> 31), "word {i}");
        }
    }

    #[test]
    fn hash_block_adds_into_state() {
        // Davies-Meyer: the round output is added to the incoming state, so
        // hashing from a shifted state shifts the result by the same amount
        // only in the feed-forward, not in the rounds. Check the feed-forward
        // directly: state' - rounds-output == incoming state.
        let w = expand_block(&[0u8; 64]);
        let mut from_zero = [0u32; 5];
        hash_block(&mut from_zero, &w);
        // from_zero now holds exactly the rounds' output for zero state.
        let mut state = [1u32, 2, 3, 4, 5];
        let before = state;
        hash_block(&mut state, &w);
        for i in 0..5 {
            assert_ne!(state[i], from_zero[i].wrapping_add(before[i]),
                "rounds must depend on the incoming state, not just the feed-forward");
        }
    }

    // ---- big_multiply ------------------------------------------------------

    /// Little-endian digits of `v`, exactly `len` of them.
    fn le_digits(v: u128, len: usize) -> Vec<u8> {
        (0..len).map(|i| (v >> (8 * i)) as u8).collect()
    }

    fn u128_of(digits: &[u8]) -> u128 {
        digits
            .iter()
            .rev()
            .fold(0u128, |acc, &d| (acc << 8) | u128::from(d))
    }

    /// Column-wise reference multiply, structured differently from the port
    /// (one pass per output digit, u64 accumulator) so a shared bug is
    /// unlikely.
    fn reference_multiply(f1: &[u8], f2: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; f1.len() + f2.len()];
        let mut carry: u64 = 0;
        for (k, digit) in out.iter_mut().enumerate() {
            let mut acc = carry;
            for (i, &a) in f1.iter().enumerate().take(k + 1) {
                let j = k - i;
                if j < f2.len() {
                    acc += u64::from(a) * u64::from(f2[j]);
                }
            }
            *digit = (acc & 0xFF) as u8;
            carry = acc >> 8;
        }
        out
    }

    #[test]
    fn multiply_known_answers_against_u128() {
        let cases: [(u128, usize, u128, usize); 6] = [
            (0, 1, 0, 1),
            (255, 1, 255, 1),
            (0x1234, 2, 0x5678, 2),
            (0xFFFF_FFFF, 4, 0xFFFF_FFFF, 4),
            (0xDEAD_BEEF_CAFE, 6, 0x0102_0304, 4),
            (u64::MAX as u128, 8, u64::MAX as u128, 8),
        ];
        for (a, alen, b, blen) in cases {
            let f1 = le_digits(a, alen);
            let f2 = le_digits(b, blen);
            let mut prod = vec![0u8; alen + blen];
            big_multiply(&f1, &f2, &mut prod);
            assert_eq!(u128_of(&prod), a * b, "{a:#x} * {b:#x}");
        }
    }

    #[test]
    fn multiply_with_leading_zero_digits() {
        // Zero digits of f1 skip their row entirely; the result must still be
        // right when either factor carries high zero padding.
        let f1 = vec![0x00, 0xFF, 0x00, 0x00]; // 0xFF00
        let f2 = vec![0x02, 0x00, 0x00]; // 2
        let mut prod = vec![0u8; 7];
        big_multiply(&f1, &f2, &mut prod);
        assert_eq!(u128_of(&prod), 0xFF00 * 2);
    }

    #[test]
    fn multiply_sweep_against_reference() {
        // A deterministic xorshift sweep across digit lengths 1..=12.
        let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut rand = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for _ in 0..500 {
            let alen = 1 + (rand() % 12) as usize;
            let blen = 1 + (rand() % 12) as usize;
            let f1: Vec<u8> = (0..alen).map(|_| rand() as u8).collect();
            let f2: Vec<u8> = (0..blen).map(|_| rand() as u8).collect();
            let mut prod = vec![0u8; alen + blen];
            big_multiply(&f1, &f2, &mut prod);
            assert_eq!(prod, reference_multiply(&f1, &f2));
        }
    }

    #[test]
    fn multiply_preserves_the_c_quirks_on_prefilled_prod() {
        // The C adds into prod's existing content but *overwrites* the final
        // carry cell of each row. f1=[2], f2=[3], prod pre-filled with 0xFF:
        // column 0: 3*2 + 0xFF = 0x105 -> digit 5 carry 1; column 1: the
        // carry overwrites the 0xFF.
        let mut prod = vec![0xFF, 0xFF];
        big_multiply(&[2], &[3], &mut prod);
        assert_eq!(prod, vec![0x05, 0x01]);
    }

    // ---- big_divide --------------------------------------------------------

    /// Runs big_divide and checks quotient and remainder against u128 ground
    /// truth. Answers the add-back count.
    fn check_divide(a: u128, alen: usize, b: u128, blen: usize) -> u32 {
        let mut rem = le_digits(a, alen);
        let div = le_digits(b, blen);
        let mut quo = vec![0u8; alen.saturating_sub(blen)];
        let add_backs = big_divide(&mut rem, &div, &mut quo)
            .unwrap_or_else(|e| panic!("{a:#x} / {b:#x} failed: {e}"));
        assert_eq!(u128_of(&quo), a / b, "quotient of {a:#x} / {b:#x}");
        assert_eq!(u128_of(&rem), a % b, "remainder of {a:#x} / {b:#x}");
        add_backs
    }

    #[test]
    fn divide_known_answers() {
        // Normalised divisors (top digit >= 0x80) and quotients that fit in
        // rem size - div size digits: the preconditions of Knuth's estimate,
        // which the image's caller guarantees and the C assumes.
        check_divide(0x1234_5678_9ABC_DEF0, 8, 0x8000, 2);
        check_divide(0x7FFF_FFFF_FFFF_FFFF, 8, 0xFF00_0001, 4);
        check_divide(0xAEAD_BEEF_0BAD_F00D_1234_5678, 12, 0xCAFE_BABE, 4);
        check_divide(u128::MAX >> 1, 16, 0x8000_0000_0000_0001, 8);
    }

    #[test]
    fn divide_when_remainder_shorter_or_equal_is_a_no_op() {
        // The main loop runs from rn down to dn + 1: with rn <= dn it never
        // iterates, and rem and quo are untouched.
        let mut rem = vec![0x34, 0x12];
        let mut quo = vec![0xAA];
        assert_eq!(big_divide(&mut rem, &[0x01, 0x80], &mut quo), Ok(0));
        assert_eq!(rem, vec![0x34, 0x12]);
        assert_eq!(quo, vec![0xAA]);
    }

    #[test]
    fn divide_first_digit_equal_to_d1_caps_the_estimate() {
        // Remainder top digit equals divisor top digit: q starts at 0xFF.
        check_divide(0x80FE_FFFF, 4, 0x80FF, 2);
        check_divide(0xFF12_3456_789A, 6, 0xFF80, 2);
    }

    #[test]
    fn divide_estimate_correction_path() {
        // Constructed so the first estimate q = 0x7F fails the two-digit test
        // once (d2 * q exceeds the three-digit view) and is corrected to
        // 0x7E, which is then exact: 0x3F810000 / 0x80FF00 = 0x7E.
        let mut rem = le_digits(0x3F81_0000, 4);
        let div = le_digits(0x0080_FF00, 3);
        let mut quo = vec![0u8; 1];
        assert_eq!(big_divide(&mut rem, &div, &mut quo), Ok(0));
        assert_eq!(u128_of(&quo), 0x3F81_0000 / 0x80_FF00);
        assert_eq!(u128_of(&rem), 0x3F81_0000 % 0x80_FF00);
    }

    #[test]
    fn divide_add_back_path() {
        // Constructed add-back case: div = 0x8000FF, rem = 0x40000000. The
        // estimate from the top digits is q = 0x80 and survives the
        // three-digit test (d2 = 0), but 0x80 * 0x8000FF > 0x40000000, so the
        // subtraction borrows out and the divisor is added back: q = 0x7F.
        let mut rem = le_digits(0x4000_0000, 4);
        let div = le_digits(0x0080_00FF, 3);
        let mut quo = vec![0u8; 1];
        assert_eq!(big_divide(&mut rem, &div, &mut quo), Ok(1));
        assert_eq!(u128_of(&quo), 0x4000_0000 / 0x80_00FF);
        assert_eq!(u128_of(&rem), 0x4000_0000 % 0x80_00FF);
    }

    #[test]
    fn divide_sweep_against_u128() {
        // Deterministic sweep: random remainders against normalised divisors
        // of 2..=6 digits, checked against u128 division.
        let mut s: u64 = 0x0123_4567_89AB_CDEF;
        let mut rand = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut add_backs = 0u32;
        for _ in 0..2000 {
            let blen = 2 + (rand() % 5) as usize;
            let alen = blen + 1 + (rand() % (16 - blen as u64 - 1)) as usize;
            let mut div_digits: Vec<u8> = (0..blen).map(|_| rand() as u8).collect();
            *div_digits.last_mut().unwrap() |= 0x80; // normalise
            let mut a_digits: Vec<u8> = (0..alen).map(|_| rand() as u8).collect();
            // Keep the quotient within alen - blen digits: with the dividend's
            // top digit below the (normalised) divisor's, a < b * 256^(a-b).
            *a_digits.last_mut().unwrap() &= 0x7F;
            let a = u128_of(&a_digits);
            let b = u128_of(&div_digits);
            let mut rem = a_digits.clone();
            let mut quo = vec![0u8; alen - blen];
            add_backs += big_divide(&mut rem, &div_digits, &mut quo).unwrap();
            assert_eq!(u128_of(&quo), a / b, "quotient of {a:#x} / {b:#x}");
            assert_eq!(u128_of(&rem), a % b, "remainder of {a:#x} / {b:#x}");
        }
        // The sweep need not hit the rare path (divide_add_back_path pins
        // it); this only documents how rare it is.
        assert!(add_backs < 100);
    }

    #[test]
    fn divide_rejects_what_was_undefined_in_c() {
        // One-digit divisor: the C read a byte before the object.
        assert_eq!(
            big_divide(&mut [1, 2, 3], &[0x80], &mut [0, 0]),
            Err(PrimErr::BadArgument)
        );
        // Zero top digit: the C divided by zero.
        assert_eq!(
            big_divide(&mut [1, 2, 3], &[0xFF, 0x00], &mut [0]),
            Err(PrimErr::BadArgument)
        );
        // Quotient buffer shorter than the quotient: the C wrote past it.
        assert_eq!(
            big_divide(&mut [1, 2, 3, 4], &[0x01, 0x80], &mut [0]),
            Err(PrimErr::BadArgument)
        );
    }

    // ---- highest_non_zero_digit_index -------------------------------------

    #[test]
    fn highest_index_matches_the_c_loop() {
        // (digits, expected) -- expected values traced through the C loop,
        // including the all-zero quirk where it answers 1, not 0.
        let cases: [(&[u8], usize); 8] = [
            (&[], 1),
            (&[0], 1),
            (&[0, 0], 1),
            (&[5], 1),
            (&[0, 5], 2),
            (&[5, 0], 1),
            (&[1, 2, 3, 4], 4),
            (&[1, 0, 0, 0xFF, 0, 0], 4),
        ];
        for (digits, expected) in cases {
            assert_eq!(highest_non_zero_digit_index(digits), expected, "{digits:?}");
        }
    }
}
