//! Unpacking a Squeak `Form` bitmap back into pixels for the encoder.
//!
//! The mirror image of [`crate::pixels`], ported from the compression loop in
//! `sqJPEGReadWriter2Plugin.c`. The masks are `& 248` rather than `& 255`
//! because a 16-bit Form only carries five bits per channel: the C recovers
//! them into the top five bits of a byte and leaves the bottom three at zero,
//! and so does this.

use crate::pixels::FormDepth;

/// Unpacks one row of bitmap words into interleaved component bytes.
///
/// `out` is the row buffer, `words_per_row * pixels_per_word * components`
/// long; the C sized it the same way, so rows of a Form whose width is not a
/// multiple of `pixels_per_word` carry padding pixels that the encoder ignores
/// because it is told the true width.
pub fn unpack_row(words: &[u32], depth: FormDepth, out: &mut [u8]) {
    let step = depth.components() * depth.pixels_per_word();

    for (pixels, &word) in out.chunks_exact_mut(step).zip(words) {
        match depth {
            // Alpha is dropped, as in the C: JPEG has no alpha channel. The C
            // also ignored the word order at this depth (one pixel per word).
            FormDepth::Argb32 { .. } => {
                pixels[0] = ((word >> 16) & 255) as u8;
                pixels[1] = ((word >> 8) & 255) as u8;
                pixels[2] = (word & 255) as u8;
            }
            FormDepth::Rgb555 { reversed } => {
                // One pixel from each halfword; `high` is where the high
                // halfword's pixel lands.
                let (high, low) = if reversed { (3, 0) } else { (0, 3) };
                pixels[high] = ((word >> 23) & 248) as u8;
                pixels[high + 1] = ((word >> 18) & 248) as u8;
                pixels[high + 2] = ((word >> 13) & 248) as u8;
                pixels[low] = ((word >> 7) & 248) as u8;
                pixels[low + 1] = ((word >> 2) & 248) as u8;
                pixels[low + 2] = ((word << 3) & 248) as u8;
            }
            FormDepth::Gray8 { reversed } => {
                for (k, gray) in pixels.iter_mut().enumerate() {
                    let shift = if reversed { 8 * k } else { 24 - 8 * k };
                    *gray = ((word >> shift) & 255) as u8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(raw: isize) -> FormDepth {
        FormDepth::try_from(raw).expect("a supported depth")
    }

    #[test]
    fn depth_32_drops_alpha_and_keeps_rgb() {
        let mut out = [0u8; 3];
        unpack_row(&[0xFF12_3456], depth(32), &mut out);
        assert_eq!(out, [0x12, 0x34, 0x56]);
    }

    #[test]
    fn negative_depth_32_is_treated_the_same() {
        let mut a = [0u8; 3];
        let mut b = [0u8; 3];
        unpack_row(&[0xFF12_3456], depth(32), &mut a);
        unpack_row(&[0xFF12_3456], depth(-32), &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn depth_16_expands_five_bits_into_the_top_of_each_byte() {
        // High half: r=31 g=0 b=0. Low half: r=0 g=31 b=0.
        let word = ((32768u32 | (31 << 10)) << 16) | (32768 | (31 << 5));
        let mut out = [0u8; 6];
        unpack_row(&[word], depth(16), &mut out);
        assert_eq!(out, [248, 0, 0, 0, 248, 0]);
    }

    #[test]
    fn negative_depth_16_swaps_the_pixel_pair() {
        let word = ((32768u32 | (31 << 10)) << 16) | (32768 | (31 << 5));
        let mut fwd = [0u8; 6];
        let mut rev = [0u8; 6];
        unpack_row(&[word], depth(16), &mut fwd);
        unpack_row(&[word], depth(-16), &mut rev);
        assert_eq!(&fwd[0..3], &rev[3..6]);
        assert_eq!(&fwd[3..6], &rev[0..3]);
    }

    #[test]
    fn depth_8_unpacks_four_grays() {
        let mut fwd = [0u8; 4];
        let mut rev = [0u8; 4];
        unpack_row(&[0x1122_3344], depth(8), &mut fwd);
        unpack_row(&[0x1122_3344], depth(-8), &mut rev);
        assert_eq!(fwd, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(rev, [0x44, 0x33, 0x22, 0x11]);
    }

    /// A short output buffer must stop the loop, not panic or write past it.
    #[test]
    fn a_short_row_buffer_is_not_overrun() {
        let mut out = [0u8; 4]; // room for one pixel, not two
        unpack_row(&[0xFF11_2233, 0xFF44_5566], depth(32), &mut out);
        assert_eq!(&out[0..3], &[0x11, 0x22, 0x33]);
    }
}
