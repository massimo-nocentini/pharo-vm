//! Unpacking a Squeak `Form` bitmap back into pixels for the encoder.
//!
//! The mirror image of [`crate::pixels`], ported from the compression loop in
//! `sqJPEGReadWriter2Plugin.c`. The masks are `& 248` rather than `& 255`
//! because a 16-bit Form only carries five bits per channel: the C recovers
//! them into the top five bits of a byte and leaves the bottom three at zero,
//! and so does this.

use crate::pixels::NativeDepth;

/// Components the encoder receives per pixel for a given Form depth.
///
/// Grayscale for depth 8, RGB for everything else -- exactly the choice the C
/// made when setting `in_color_space`.
#[must_use]
pub const fn input_components(depth: NativeDepth) -> usize {
    if depth.bits() == 8 {
        1
    } else {
        3
    }
}

/// Unpacks one row of bitmap words into interleaved component bytes.
///
/// `out` is the row buffer, `words_per_row * pixels_per_word * components`
/// long; the C sized it the same way, so rows of a Form whose width is not a
/// multiple of `pixels_per_word` carry padding pixels that the encoder ignores
/// because it is told the true width.
pub fn unpack_row(words: &[u32], depth: NativeDepth, pixels_per_word: usize, out: &mut [u8]) {
    let components = input_components(depth);
    let step = components * pixels_per_word;

    for (j, &word) in words.iter().enumerate() {
        let i = j * step;
        if i + step > out.len() {
            break;
        }
        match depth.0 {
            32 | -32 => {
                // Alpha is dropped, as in the C: JPEG has no alpha channel.
                out[i] = ((word >> 16) & 255) as u8;
                out[i + 1] = ((word >> 8) & 255) as u8;
                out[i + 2] = (word & 255) as u8;
            }
            16 => {
                out[i] = ((word >> 23) & 248) as u8;
                out[i + 1] = ((word >> 18) & 248) as u8;
                out[i + 2] = ((word >> 13) & 248) as u8;
                out[i + 3] = ((word >> 7) & 248) as u8;
                out[i + 4] = ((word >> 2) & 248) as u8;
                out[i + 5] = ((word << 3) & 248) as u8;
            }
            -16 => {
                out[i] = ((word >> 7) & 248) as u8;
                out[i + 1] = ((word >> 2) & 248) as u8;
                out[i + 2] = ((word << 3) & 248) as u8;
                out[i + 3] = ((word >> 23) & 248) as u8;
                out[i + 4] = ((word >> 18) & 248) as u8;
                out[i + 5] = ((word >> 13) & 248) as u8;
            }
            8 => {
                out[i] = ((word >> 24) & 255) as u8;
                out[i + 1] = ((word >> 16) & 255) as u8;
                out[i + 2] = ((word >> 8) & 255) as u8;
                out[i + 3] = (word & 255) as u8;
            }
            -8 => {
                out[i] = (word & 255) as u8;
                out[i + 1] = ((word >> 8) & 255) as u8;
                out[i + 2] = ((word >> 16) & 255) as u8;
                out[i + 3] = ((word >> 24) & 255) as u8;
            }
            // The C's switch had no default and left the buffer as it was.
            // Callers reject unsupported depths first.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_32_drops_alpha_and_keeps_rgb() {
        let mut out = [0u8; 3];
        unpack_row(&[0xFF12_3456], NativeDepth(32), 1, &mut out);
        assert_eq!(out, [0x12, 0x34, 0x56]);
    }

    #[test]
    fn negative_depth_32_is_treated_the_same() {
        let mut a = [0u8; 3];
        let mut b = [0u8; 3];
        unpack_row(&[0xFF12_3456], NativeDepth(32), 1, &mut a);
        unpack_row(&[0xFF12_3456], NativeDepth(-32), 1, &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn depth_16_expands_five_bits_into_the_top_of_each_byte() {
        // High half: r=31 g=0 b=0. Low half: r=0 g=31 b=0.
        let word = ((32768u32 | (31 << 10)) << 16) | (32768 | (31 << 5));
        let mut out = [0u8; 6];
        unpack_row(&[word], NativeDepth(16), 2, &mut out);
        assert_eq!(out, [248, 0, 0, 0, 248, 0]);
    }

    #[test]
    fn negative_depth_16_swaps_the_pixel_pair() {
        let word = ((32768u32 | (31 << 10)) << 16) | (32768 | (31 << 5));
        let mut fwd = [0u8; 6];
        let mut rev = [0u8; 6];
        unpack_row(&[word], NativeDepth(16), 2, &mut fwd);
        unpack_row(&[word], NativeDepth(-16), 2, &mut rev);
        assert_eq!(&fwd[0..3], &rev[3..6]);
        assert_eq!(&fwd[3..6], &rev[0..3]);
    }

    #[test]
    fn depth_8_unpacks_four_grays() {
        let mut fwd = [0u8; 4];
        let mut rev = [0u8; 4];
        unpack_row(&[0x1122_3344], NativeDepth(8), 4, &mut fwd);
        unpack_row(&[0x1122_3344], NativeDepth(-8), 4, &mut rev);
        assert_eq!(fwd, [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(rev, [0x44, 0x33, 0x22, 0x11]);
    }

    #[test]
    fn component_count_follows_depth() {
        assert_eq!(input_components(NativeDepth(8)), 1);
        assert_eq!(input_components(NativeDepth(-8)), 1);
        assert_eq!(input_components(NativeDepth(16)), 3);
        assert_eq!(input_components(NativeDepth(32)), 3);
    }

    /// A short output buffer must stop the loop, not panic or write past it.
    #[test]
    fn a_short_row_buffer_is_not_overrun() {
        let mut out = [0u8; 4]; // room for one pixel, not two
        unpack_row(&[0xFF11_2233, 0xFF44_5566], NativeDepth(32), 1, &mut out);
        assert_eq!(&out[0..3], &[0x11, 0x22, 0x33]);
    }
}
