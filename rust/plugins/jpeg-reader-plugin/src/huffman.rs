//! Huffman decoding of one 8x8 coefficient block.
//!
//! Mirrors `JPEGReaderPlugin>>#jpegDecodeValueFrom:size:` and
//! `#decodeBlockInto:component:`. The tables are the image-built
//! `JPEGHuffmanTable` WordArrays: slot 0 carries the initial code length in
//! its top byte, the first real lookup starts at slot 2, and each entry is
//! either a leaf (bits 24..30 clear) or a chain — extra code length in the
//! top byte, next table offset in the low 16 bits.

use pharo_vm_plugin::sqInt;

use crate::stream::JpegStream;
use crate::{UsqInt, DCT_SIZE2};

/// Longest huffman code the tables may ask for.
const MAX_BITS: sqInt = 16;

/// Zig-zag to row-major order: coefficient `k` of the entropy-coded stream
/// lands at `NATURAL_ORDER[k]` in the block.
const NATURAL_ORDER: [usize; DCT_SIZE2] = [
    0, 1, 8, 16, 9, 2, 3, 10, //
    17, 24, 32, 25, 18, 11, 4, 5, //
    12, 19, 26, 33, 40, 48, 41, 34, //
    27, 20, 13, 6, 7, 14, 21, 28, //
    35, 42, 49, 56, 57, 50, 43, 36, //
    29, 22, 15, 23, 30, 37, 44, 51, //
    58, 59, 52, 45, 38, 31, 39, 46, //
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// `JPEGReaderPlugin>>#jpegDecodeValueFrom:size:` — the next symbol, or -1
/// on any malformed table or starved stream.
///
/// The C read `table[0]` unconditionally and never checked the index against
/// the table's *lower* bound (a chain entry with offset 0 can drive it to
/// -1); both out-of-bounds reads are decode failures here.
pub fn decode_value(stream: &mut JpegStream, table: &[i32]) -> sqInt {
    let Some(&first) = table.first() else {
        return -1;
    };
    // Initial bits needed. The C widens through usqInt, so a negative slot 0
    // becomes a huge count and fails the MaxBits check.
    let mut bits_needed = ((first as sqInt as UsqInt) >> 24) as sqInt;
    if bits_needed > MAX_BITS {
        return -1;
    }
    // First real table.
    let mut table_index: sqInt = 2;
    loop {
        let bits = stream.get_bits(bits_needed);
        if bits < 0 {
            return -1;
        }
        let index = (table_index + bits) - 1;
        if index >= table.len() as sqInt {
            return -1;
        }
        let Ok(i) = usize::try_from(index) else {
            return -1;
        };
        // Lookup entry in table.
        let value = table[i];
        if (value & 0x3F00_0000) == 0 {
            return value as sqInt;
        }
        // Table offset in low 16 bit.
        table_index = (value & 0xFFFF) as sqInt;
        // Additional bits in high 8 bit.
        bits_needed = (((value as sqInt as UsqInt) >> 24) & 0xFF) as sqInt;
        if bits_needed > MAX_BITS {
            return -1;
        }
    }
}

/// `JPEGReaderPlugin>>#scaleAndSignExtend:inFieldWidth:` — JPEG's EXTEND: a
/// `field_width`-bit magnitude whose high bit clear means a negative value.
///
/// The C builds both thresholds with a 32-bit `1U`; the wrapping shifts
/// reproduce its behaviour when a corrupt table hands over an oversized
/// field width (shift-count UB in C, count-masked on the targets it ships
/// on). `bits` may be -1 from a starved read — the C feeds it through
/// unchecked, and so does this.
fn scale_and_sign_extend(bits: sqInt, field_width: sqInt) -> sqInt {
    let half = 1u32.wrapping_shl(field_width.wrapping_sub(1) as u32) as sqInt;
    if bits < half {
        (bits - 1u32.wrapping_shl(field_width as u32) as sqInt) + 1
    } else {
        bits
    }
}

/// `JPEGReaderPlugin>>#decodeBlockInto:component:` — one DC delta plus the
/// run-length coded AC coefficients, dequantised into natural order.
///
/// Answers the block exactly as the C wrote it into the image's WordArray,
/// and advances `prior_dc` (the component's `PriorDCValue`, a C `int`, so it
/// wraps at 32 bits). `Err` where the C called `primitiveFail` — with the
/// difference that the C had by then already scribbled a partial block into
/// the image array, while this port leaves it untouched (see README).
pub fn decode_block(
    stream: &mut JpegStream,
    dc_table: &[i32],
    ac_table: &[i32],
    prior_dc: &mut i32,
) -> Result<[i32; DCT_SIZE2], ()> {
    let mut coeffs = [0i32; DCT_SIZE2];

    let mut byte = decode_value(stream, dc_table);
    if byte < 0 {
        return Err(());
    }
    if byte != 0 {
        let bits = stream.get_bits(byte);
        byte = scale_and_sign_extend(bits, byte);
    }
    let dc = (*prior_dc as sqInt).wrapping_add(byte) as i32;
    *prior_dc = dc;
    coeffs[0] = dc;

    let mut index: sqInt = 1;
    while index < DCT_SIZE2 as sqInt {
        let mut byte = decode_value(stream, ac_table);
        if byte < 0 {
            return Err(());
        }
        let zero_count = ((byte as UsqInt) >> 4) as sqInt;
        byte &= 15;
        if byte != 0 {
            index += zero_count;
            let bits = stream.get_bits(byte);
            byte = scale_and_sign_extend(bits, byte);
            if !(0..DCT_SIZE2 as sqInt).contains(&index) {
                return Err(());
            }
            coeffs[NATURAL_ORDER[index as usize]] = byte as i32;
        } else if zero_count == 15 {
            // ZRL: sixteen zeroes (15 here, one more below).
            index += zero_count;
        } else {
            // End of block.
            break;
        }
        index += 1;
    }
    Ok(coeffs)
}

#[cfg(test)]
mod tests {
    // Underscores in the bit-stream literals mark huffman code boundaries,
    // not byte nibbles.
    #![allow(clippy::unusual_byte_groupings)]

    use super::*;

    fn stream(bytes: &[u8]) -> JpegStream<'_> {
        JpegStream::new(bytes, 0, bytes.len() as sqInt, 0, 0).expect("valid stream")
    }

    /// A single-level table for 2-bit codes: code `b` answers `leaves[b]`.
    /// Entries live at `2 + bits - 1`, i.e. slots 1..=4.
    fn two_bit_table(leaves: [i32; 4]) -> Vec<i32> {
        vec![0x0200_0000, leaves[0], leaves[1], leaves[2], leaves[3]]
    }

    #[test]
    fn decodes_leaves_of_a_flat_table() {
        let table = two_bit_table([10, 20, 30, 40]);
        let mut s = stream(&[0b01_10_0000]);
        assert_eq!(decode_value(&mut s, &table), 20);
        assert_eq!(decode_value(&mut s, &table), 30);
    }

    #[test]
    fn follows_a_chain_entry_into_a_second_table() {
        // 1 initial bit: 0 -> leaf 0x0A, 1 -> chain to offset 5 for 1 more
        // bit, whose leaves sit at 5 + bits - 1 = slots 4 and 5.
        let table = vec![0x0100_0000, 0x0A, 0x0100_0005, 0, 0x0B, 0x0C];
        let mut s = stream(&[0b0_10_11_000]);
        assert_eq!(decode_value(&mut s, &table), 0x0A);
        assert_eq!(decode_value(&mut s, &table), 0x0B);
        assert_eq!(decode_value(&mut s, &table), 0x0C);
    }

    #[test]
    fn rejects_index_past_the_table() {
        // 2-bit codes but only slots 1 and 2 exist: code 10 indexes slot 3.
        let table = vec![0x0200_0000, 7, 7];
        let mut s = stream(&[0b10_000000]);
        assert_eq!(decode_value(&mut s, &table), -1);
    }

    #[test]
    fn rejects_oversized_code_length_and_empty_table() {
        let mut s = stream(&[0xAB, 0xCD, 0xEF]);
        assert_eq!(decode_value(&mut s, &[17 << 24]), -1);
        assert_eq!(decode_value(&mut s, &[]), -1);
    }

    #[test]
    fn rejects_a_starved_stream() {
        let table = two_bit_table([1, 2, 3, 4]);
        // One byte of input: drain it, then ask for a 2-bit code.
        let mut s = stream(&[0x00]);
        assert_eq!(s.get_bits(8), 0);
        assert_eq!(decode_value(&mut s, &table), -1);
    }

    #[test]
    fn extend_produces_jpeg_signed_magnitudes() {
        // Field width 3: raw 0..3 map to -7..-4, raw 4..7 stay positive.
        for (raw, expected) in [(0, -7), (3, -4), (4, 4), (7, 7)] {
            assert_eq!(scale_and_sign_extend(raw, 3), expected);
        }
        // Width 1: 0 -> -1, 1 -> 1.
        assert_eq!(scale_and_sign_extend(0, 1), -1);
        assert_eq!(scale_and_sign_extend(1, 1), 1);
    }

    // Block-level fixtures: DC codes 00..11 mean "size 0..3"; AC codes are
    // 00 -> EOB, 01 -> run 0/size 1, 10 -> ZRL or run 15/size 1, 11 -> run
    // 1/size 1, depending on the test's table.
    fn dc_table() -> Vec<i32> {
        two_bit_table([0, 1, 2, 3])
    }

    #[test]
    fn dc_delta_zero_and_immediate_eob() {
        // DC size 0, then EOB.
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 5;
        let mut s = stream(&[0b00_00_0000]);
        let block = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(prior, 5);
        assert_eq!(block[0], 5);
        assert!(block[1..].iter().all(|&c| c == 0));
    }

    #[test]
    fn dc_delta_accumulates_across_blocks() {
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 0;
        // Twice: DC size 2, magnitude 11b (=3), EOB.
        let mut s = stream(&[0b10_11_00_10, 0b11_00_0000]);
        let first = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(first[0], 3);
        let second = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(second[0], 6);
        assert_eq!(prior, 6);
    }

    #[test]
    fn dc_low_magnitude_is_negative() {
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 0;
        // DC size 2, magnitude 00b -> extend to -3; EOB.
        let mut s = stream(&[0b10_00_00_00]);
        let block = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(block[0], -3);
        assert_eq!(prior, -3);
    }

    #[test]
    fn ac_coefficients_land_in_natural_order() {
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 0;
        // DC 0; AC run1/size1 bit 1 -> +1 at zigzag 2 (natural 8);
        // AC run1/size1 bit 0 -> -1 at zigzag 4 (natural 9); EOB.
        let mut s = stream(&[0b00_11_1_11_0, 0b00_000000]);
        let block = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(block[8], 1);
        assert_eq!(block[9], -1);
        let touched: Vec<usize> = (0..64).filter(|&i| block[i] != 0).collect();
        assert_eq!(touched, vec![8, 9]);
    }

    #[test]
    fn zrl_skips_sixteen_zeroes() {
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 0;
        // DC 0; ZRL; run0/size1 bit 1 -> +1 at zigzag 17 (natural 19); EOB.
        let mut s = stream(&[0b00_10_01_1_0, 0b0_0000000]);
        let block = decode_block(&mut s, &dc_table(), &ac, &mut prior).unwrap();
        assert_eq!(block[19], 1);
        assert_eq!(block.iter().filter(|&&c| c != 0).count(), 1);
    }

    #[test]
    fn coefficient_index_past_the_block_fails() {
        // AC code 10 means run 15/size 1 here: four of them push the index
        // to 64 and trip the bounds check the C also had.
        let ac = two_bit_table([0x00, 0x01, 0xF1, 0x11]);
        let mut prior = 0;
        let mut s = stream(&[0b00_10_1_10_1, 0b10_1_10_1_00]);
        assert!(decode_block(&mut s, &dc_table(), &ac, &mut prior).is_err());
    }

    #[test]
    fn starved_ac_stream_fails_the_block() {
        let ac = two_bit_table([0x00, 0x01, 0xF0, 0x11]);
        let mut prior = 0;
        // DC 0; AC run0/size1 bit 1; then one stray bit — the next AC code
        // cannot be read.
        let mut s = stream(&[0b00_01_1_01_1]);
        assert!(decode_block(&mut s, &dc_table(), &ac, &mut prior).is_err());
    }
}
