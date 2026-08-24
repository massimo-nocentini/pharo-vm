//! Packing decoded scanlines into a Squeak `Form` bitmap.
//!
//! This is a faithful port of the pixel loop in
//! `plugins/JPEGReadWriter2Plugin/src/common/sqJPEGReadWriter2Plugin.c`. The
//! odd bits of that loop -- the 4x4 ordered dither lifted from
//! `Form>>orderedDither32To16`, the component-offset table, the two word
//! orders -- are reproduced exactly rather than tidied up.
//!
//! Bit-identical output is not the goal, and is not achievable: the decoder
//! feeding this stage is no longer libjpeg, and two conforming JPEG decoders
//! disagree in the last bit or two of the IDCT. What must match is the
//! *packing*, so that a given decoded pixel lands in the same place with the
//! same bit layout. Measured against the C plugin over a 90-case corpus, the
//! worst visible channel difference is 3/255.
//!
//! Where this code deliberately differs, it says so.

use pharo_vm_plugin::PrimErr;

/// A `Form` depth this plugin can pack.
///
/// The image reports depth as a signed integer: the magnitude is bits per
/// pixel, and a negative value means the two (or four) pixels packed into a
/// word are in the opposite order. That raw integer exists only at the
/// [`TryFrom`] boundary; everything past it works in terms of the three
/// supported layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormDepth {
    /// 32 bits per pixel: one ARGB pixel per word.
    Argb32 {
        /// Pixel order within a word is reversed (a negative depth).
        reversed: bool,
    },
    /// 16 bits per pixel: two RGB555 pixels per word.
    Rgb555 {
        /// Pixel order within a word is reversed (a negative depth).
        reversed: bool,
    },
    /// 8 bits per pixel: four grayscale pixels per word.
    Gray8 {
        /// Pixel order within a word is reversed (a negative depth).
        reversed: bool,
    },
}

impl TryFrom<isize> for FormDepth {
    type Error = PrimErr;

    /// Fails with `BadArgument` for any depth this plugin cannot pack, the
    /// code the callers' explicit depth checks used to produce.
    fn try_from(depth: isize) -> Result<Self, PrimErr> {
        Ok(match depth {
            32 => Self::Argb32 { reversed: false },
            -32 => Self::Argb32 { reversed: true },
            16 => Self::Rgb555 { reversed: false },
            -16 => Self::Rgb555 { reversed: true },
            8 => Self::Gray8 { reversed: false },
            -8 => Self::Gray8 { reversed: true },
            _ => return Err(PrimErr::BadArgument),
        })
    }
}

impl FormDepth {
    /// Pixels packed into one 32-bit bitmap word.
    #[must_use]
    pub const fn pixels_per_word(self) -> usize {
        match self {
            Self::Argb32 { .. } => 1,
            Self::Rgb555 { .. } => 2,
            Self::Gray8 { .. } => 4,
        }
    }

    /// Components the encoder receives per pixel.
    ///
    /// Grayscale for [`FormDepth::Gray8`], RGB for everything else -- exactly
    /// the choice the C made when setting `in_color_space`.
    #[must_use]
    pub const fn components(self) -> usize {
        match self {
            Self::Gray8 { .. } => 1,
            Self::Argb32 { .. } | Self::Rgb555 { .. } => 3,
        }
    }
}

/// Everything the row packer needs that does not change between rows.
#[derive(Debug, Clone, Copy)]
pub struct RowConfig {
    /// Destination Form depth.
    pub depth: FormDepth,
    /// Words in one row of the destination bitmap.
    pub words_per_row: usize,
    /// Components the decoder emits per pixel: 3 for RGB, 1 for grayscale.
    pub out_components: usize,
    /// Apply the 4x4 ordered dither when reducing to 16 bits.
    pub dither: bool,
}

/// The dither matrix from `Form>>orderedDither32To16`, as the C plugin
/// transcribed it.
const DITHER_1: [u32; 8] = [2, 0, 14, 12, 1, 3, 13, 15];
const DITHER_2: [u32; 8] = [10, 8, 6, 4, 9, 11, 5, 7];

/// Component offsets within a pixel pair, chosen by component count.
///
/// For RGB the second pixel starts three bytes on; for grayscale both pixels
/// are a single byte each and every channel reads the same one.
const fn offsets(out_components: usize) -> ([usize; 3], [usize; 3]) {
    if out_components == 3 {
        ([0, 1, 2], [3, 4, 5])
    } else {
        ([0, 0, 0], [1, 1, 1])
    }
}

/// Reads `pixels[i]`, or 0 past the end.
///
/// **This is a deliberate divergence from the C.** There, the packing loop
/// steps `out_components * pixels_per_word` bytes at a time until it passes
/// `row_stride`, so a width that is not a multiple of `pixels_per_word` makes
/// the final iteration read past the scanline buffer. libjpeg allocates that
/// buffer at exactly `row_stride` bytes, so for an odd-width image at depth 16
/// the C plugin reads up to three bytes out of bounds -- undefined behaviour,
/// and whatever it produces for those trailing pixels is not reproducible.
/// Substituting zero keeps the output deterministic and the read in bounds.
#[inline]
fn at(pixels: &[u8], i: usize) -> u8 {
    pixels.get(i).copied().unwrap_or(0)
}

/// Packs one decoded scanline into `out`, which must be `words_per_row` long.
///
/// `scanline` is the row's index in the image, needed because the dither
/// pattern depends on it.
pub fn pack_row(row: &[u8], scanline: u32, cfg: &RowConfig, out: &mut [u32]) {
    debug_assert_eq!(out.len(), cfg.words_per_row);

    let step = cfg.out_components * cfg.depth.pixels_per_word();
    if step == 0 {
        return; // `chunks(0)` would panic
    }
    let (off1, off2) = offsets(cfg.out_components);

    // The last chunk may be short when the width is not a multiple of
    // pixels_per_word; `at` zero-fills the missing tail (see above).
    for (j, (pixels, word)) in row.chunks(step).zip(out.iter_mut()).enumerate() {
        *word = match cfg.depth {
            FormDepth::Argb32 { .. } => {
                let r = u32::from(at(pixels, off1[0]));
                let g = u32::from(at(pixels, off1[1]));
                let b = u32::from(at(pixels, off1[2]));
                (255 << 24) | (r << 16) | (g << 8) | b
            }
            FormDepth::Rgb555 { reversed } => {
                let (mut r1, mut g1, mut b1) = (
                    at(pixels, off1[0]),
                    at(pixels, off1[1]),
                    at(pixels, off1[2]),
                );
                let (mut r2, mut g2, mut b2) = (
                    at(pixels, off2[0]),
                    at(pixels, off2[1]),
                    at(pixels, off2[2]),
                );

                let (p1, p2);
                if cfg.dither {
                    let slot = ((scanline & 3) << 1) | (j as u32 & 1);
                    let dmv1 = DITHER_1[slot as usize];
                    let dmv2 = DITHER_2[slot as usize];
                    p1 = (
                        dither_channel(r1, dmv1),
                        dither_channel(g1, dmv1),
                        dither_channel(b1, dmv1),
                    );
                    p2 = (
                        dither_channel(r2, dmv2),
                        dither_channel(g2, dmv2),
                        dither_channel(b2, dmv2),
                    );
                } else {
                    r1 >>= 3;
                    g1 >>= 3;
                    b1 >>= 3;
                    r2 >>= 3;
                    g2 >>= 3;
                    b2 >>= 3;
                    p1 = (u32::from(r1), u32::from(g1), u32::from(b1));
                    p2 = (u32::from(r2), u32::from(g2), u32::from(b2));
                }

                let w1 = 32768 | (p1.0 << 10) | (p1.1 << 5) | p1.2;
                let w2 = 32768 | (p2.0 << 10) | (p2.1 << 5) | p2.2;
                if reversed {
                    (w2 << 16) | w1
                } else {
                    (w1 << 16) | w2
                }
            }
            FormDepth::Gray8 { reversed } => {
                let g1 = u32::from(at(pixels, 0));
                let g2 = u32::from(at(pixels, 1));
                let g3 = u32::from(at(pixels, 2));
                let g4 = u32::from(at(pixels, 3));
                if reversed {
                    (g4 << 24) | (g3 << 16) | (g2 << 8) | g1
                } else {
                    (g1 << 24) | (g2 << 16) | (g3 << 8) | g4
                }
            }
        };
    }
}

/// Reduces an 8-bit channel to 5 bits through the ordered dither.
#[inline]
fn dither_channel(value: u8, threshold: u32) -> u32 {
    let di = (u32::from(value) * 496) >> 8;
    let dmi = di & 15;
    let dmo = di >> 4;
    if threshold < dmi {
        dmo + 1
    } else {
        dmo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depth(raw: isize) -> FormDepth {
        FormDepth::try_from(raw).expect("a supported depth")
    }

    fn cfg(raw_depth: isize, wpr: usize, occ: usize, dither: bool) -> RowConfig {
        RowConfig {
            depth: depth(raw_depth),
            words_per_row: wpr,
            out_components: occ,
            dither,
        }
    }

    #[test]
    fn only_the_six_form_depths_convert() {
        for raw in [32, -32, 16, -16, 8, -8] {
            assert!(FormDepth::try_from(raw).is_ok(), "depth {raw}");
        }
        for raw in [0, 1, -1, 2, 4, 24, -24, 64] {
            assert_eq!(
                FormDepth::try_from(raw),
                Err(PrimErr::BadArgument),
                "depth {raw}"
            );
        }
    }

    #[test]
    fn geometry_follows_the_depth() {
        assert_eq!(depth(32).pixels_per_word(), 1);
        assert_eq!(depth(-16).pixels_per_word(), 2);
        assert_eq!(depth(8).pixels_per_word(), 4);
        assert_eq!(depth(8).components(), 1);
        assert_eq!(depth(-8).components(), 1);
        assert_eq!(depth(16).components(), 3);
        assert_eq!(depth(32).components(), 3);
    }

    #[test]
    fn depth_32_packs_argb_with_opaque_alpha() {
        let row = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let mut out = [0u32; 2];
        pack_row(&row, 0, &cfg(32, 2, 3, false), &mut out);
        assert_eq!(out, [0xFF12_3456, 0xFF78_9ABC]);
    }

    #[test]
    fn depth_32_grayscale_replicates_the_single_channel() {
        let row = [0x40, 0x80];
        let mut out = [0u32; 2];
        pack_row(&row, 0, &cfg(32, 2, 1, false), &mut out);
        assert_eq!(out, [0xFF40_4040, 0xFF80_8080]);
    }

    #[test]
    fn depth_16_undithered_truncates_to_five_bits() {
        // Two RGB pixels: (0xFF,0x00,0x00) and (0x00,0xFF,0x00).
        let row = [0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00];
        let mut out = [0u32; 1];
        pack_row(&row, 0, &cfg(16, 1, 3, false), &mut out);
        let red = 32768u32 | (31 << 10);
        let green = 32768u32 | (31 << 5);
        assert_eq!(out[0], (red << 16) | green);
    }

    #[test]
    fn negative_depth_16_swaps_the_pixel_pair() {
        let row = [0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00];
        let mut fwd = [0u32; 1];
        let mut rev = [0u32; 1];
        pack_row(&row, 0, &cfg(16, 1, 3, false), &mut fwd);
        pack_row(&row, 0, &cfg(-16, 1, 3, false), &mut rev);
        assert_eq!(fwd[0] >> 16, rev[0] & 0xFFFF);
        assert_eq!(fwd[0] & 0xFFFF, rev[0] >> 16);
    }

    #[test]
    fn depth_8_packs_four_grays_per_word() {
        let row = [0x11, 0x22, 0x33, 0x44];
        let mut fwd = [0u32; 1];
        let mut rev = [0u32; 1];
        pack_row(&row, 0, &cfg(8, 1, 1, false), &mut fwd);
        pack_row(&row, 0, &cfg(-8, 1, 1, false), &mut rev);
        assert_eq!(fwd[0], 0x1122_3344);
        assert_eq!(rev[0], 0x4433_2211);
    }

    #[test]
    fn dithering_stays_within_five_bits() {
        // Every channel must land in 0..=31 so it cannot corrupt the
        // neighbouring field once shifted.
        for v in 0u8..=255 {
            for t in 0u32..16 {
                assert!(dither_channel(v, t) <= 31, "value {v}, threshold {t}");
            }
        }
    }

    #[test]
    fn dither_varies_with_scanline_and_column() {
        let row = [0x88; 12];
        let mut a = [0u32; 2];
        let mut b = [0u32; 2];
        pack_row(&row, 0, &cfg(16, 2, 3, true), &mut a);
        pack_row(&row, 1, &cfg(16, 2, 3, true), &mut b);
        // A flat input still produces a pattern; otherwise dithering is a no-op.
        assert!(a != b || a[0] != a[1]);
    }

    /// The case the C plugin reads out of bounds on: an odd width at depth 16
    /// makes the last word want six bytes when only three remain.
    #[test]
    fn odd_width_at_depth_16_stays_in_bounds() {
        let row = [0xFF, 0x80, 0x40, 0x20, 0x10, 0x08, 0x04, 0x02, 0x01]; // 3 px
        let mut out = [0u32; 2]; // words_per_row for width 3 = 2
        pack_row(&row, 0, &cfg(16, 2, 3, false), &mut out);
        // Second word's high half is the real third pixel; the low half is the
        // zero-filled phantom pixel, which the C read from past the buffer.
        assert_eq!(out[1] & 0xFFFF, 32768);
    }
}
