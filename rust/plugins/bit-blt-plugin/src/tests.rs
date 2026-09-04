//! Unit tests for the blitter core.
//!
//! There is no VM in this environment, so these tests drive the engine
//! directly on Rust-owned buffers, exactly the state `loadBitBltFrom:warping:`
//! would have produced. Expected values are computed independently inside the
//! tests: either hand-derived word vectors, or a slow per-pixel reference
//! blitter built from the *definitions* of the rules and of the Form pixel
//! layout (big-endian bit addressing within each 32-bit word for positive
//! depths), never by re-running the engine's own arithmetic.

use pharo_vm_plugin::sqInt;

use crate::rules;
use crate::state::{BitBlt, CmTable, DITHER8_LOOKUP, MASK_TABLE};
use crate::warp::deltaFromtonSteps;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// An in-memory Form: the bitmap plus its geometry.
#[derive(Clone)]
struct Form {
    bits: Vec<u32>,
    width: i32,
    height: i32,
    depth: i32,
    msb: bool,
}

impl Form {
    fn new(width: i32, height: i32, depth: i32, msb: bool) -> Self {
        let ppw = 32 / depth;
        let wpr = (width + ppw - 1) / ppw;
        Form {
            bits: vec![0; (wpr * height) as usize],
            width,
            height,
            depth,
            msb,
        }
    }

    fn ppw(&self) -> i32 {
        32 / self.depth
    }

    fn words_per_row(&self) -> i32 {
        (self.width + self.ppw() - 1) / self.ppw()
    }

    /// Bytes per row.
    fn pitch(&self) -> i32 {
        self.words_per_row() * 4
    }

    /// Width including the padding pixels of the last word.
    fn padded_width(&self) -> i32 {
        self.words_per_row() * self.ppw()
    }

    fn pixel_mask(&self) -> u32 {
        if self.depth == 32 {
            0xFFFF_FFFF
        } else {
            (1u32 << self.depth) - 1
        }
    }

    /// Independent pixel extraction straight from the Form layout spec:
    /// pixels pack left-to-right within each word, from the most significant
    /// bits for positive (big-endian) depths and from the least significant
    /// for negative ones.
    fn pixel(&self, x: i32, y: i32) -> u32 {
        let ppw = self.ppw();
        let word = self.bits[(y * self.words_per_row() + x / ppw) as usize];
        let index_in_word = x % ppw;
        let shift = if self.msb {
            32 - self.depth * (index_in_word + 1)
        } else {
            self.depth * index_in_word
        };
        (word >> shift) & self.pixel_mask()
    }

    fn set_pixel(&mut self, x: i32, y: i32, value: u32) {
        let ppw = self.ppw();
        let index = (y * self.words_per_row() + x / ppw) as usize;
        let index_in_word = x % ppw;
        let shift = if self.msb {
            32 - self.depth * (index_in_word + 1)
        } else {
            self.depth * index_in_word
        };
        let mask = self.pixel_mask() << shift;
        self.bits[index] = (self.bits[index] & !mask) | ((value & self.pixel_mask()) << shift);
    }

    /// Deterministic pseudorandom fill (LCG), so failures are reproducible.
    fn fill_random(&mut self, mut seed: u32) {
        for w in self.bits.iter_mut() {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *w = seed;
        }
    }
}

/// Distinct dummy oops for the two forms, so `sourceForm == destForm` only
/// when a test wants overlap handling.
const DEST_FORM_OOP: sqInt = 0x1000;
const SOURCE_FORM_OOP: sqInt = 0x2000;

/// Point the state at `dest` (and `source`, if any), as
/// `loadBitBltFrom:warping:` would after validation, with a full-form clip
/// rectangle and no halftone.
fn setup(bb: &mut BitBlt, dest: &mut Form, source: Option<&Form>, rule: sqInt) {
    *bb = BitBlt::new();
    bb.combinationRule = rule;
    bb.destForm = DEST_FORM_OOP;
    bb.destBits = dest.bits.as_mut_ptr() as usize;
    bb.destWidth = dest.width;
    bb.destHeight = dest.height;
    bb.destDepth = dest.depth;
    bb.destMSB = dest.msb as i32;
    bb.destPPW = (32 / dest.depth) as sqInt;
    bb.destPitch = dest.pitch();
    bb.endOfDestination = bb.destBits + dest.bits.len() * 4;
    bb.clipX = 0;
    bb.clipY = 0;
    bb.clipWidth = dest.width as sqInt;
    bb.clipHeight = dest.height as sqInt;
    bb.noHalftone = true;
    match source {
        Some(s) => {
            bb.noSource = false;
            bb.sourceForm = SOURCE_FORM_OOP;
            bb.sourceBits = s.bits.as_ptr() as usize;
            bb.sourceWidth = s.width;
            bb.sourceHeight = s.height;
            bb.sourceDepth = s.depth;
            bb.sourceMSB = s.msb as i32;
            bb.sourcePPW = (32 / s.depth) as sqInt;
            bb.sourcePitch = s.pitch();
            bb.endOfSource = bb.sourceBits + s.bits.len() * 4;
        }
        None => bb.noSource = true,
    }
}

/// Clip and run the dispatch (`copyBits` minus locking, which needs a VM).
fn run(bb: &mut BitBlt) {
    bb.clipRange();
    if bb.bbW > 0 && bb.bbH > 0 {
        // SAFETY: setup() established valid geometry over live Vec buffers.
        unsafe { bb.copyBitsDispatch() };
    } else {
        bb.affectedL = 0;
        bb.affectedR = 0;
        bb.affectedT = 0;
        bb.affectedB = 0;
    }
}

/// Slow reference for per-pixel rules: clip exactly as `clipRange` defines
/// (already-clipped coordinates are passed in), apply `rule` pixel by pixel
/// from the *before* images, leave everything else untouched.
#[allow(clippy::too_many_arguments)]
fn ref_blit_per_pixel(
    dest_before: &Form,
    src: &Form,
    rule: &dyn Fn(u32, u32) -> u32,
    sx: i32,
    sy: i32,
    dx: i32,
    dy: i32,
    w: i32,
    h: i32,
) -> Form {
    let mut out = dest_before.clone();
    let mask = dest_before.pixel_mask();
    for j in 0..h {
        for i in 0..w {
            let s = src.pixel(sx + i, sy + j);
            let d = dest_before.pixel(dx + i, dy + j);
            out.set_pixel(dx + i, dy + j, rule(s, d) & mask);
        }
    }
    out
}

/// Compare two forms pixel by pixel over the padded width (so mask overruns
/// into the padding are caught too).
fn assert_forms_equal(actual: &Form, expected: &Form, context: &str) {
    for y in 0..actual.height {
        for x in 0..actual.padded_width() {
            assert_eq!(
                actual.pixel(x, y),
                expected.pixel(x, y),
                "{context}: pixel ({x},{y})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Combination rules, word level
// ---------------------------------------------------------------------------

mod merge_rules {
    use super::*;

    fn bb_at_depth(depth: i32) -> BitBlt {
        let mut bb = BitBlt::new();
        bb.destDepth = depth;
        bb.destPPW = (32 / depth) as sqInt;
        bb.destMask = 0xFFFF_FFFF;
        bb
    }

    fn merge(bb: &mut BitBlt, rule: sqInt, s: u32, d: u32) -> u32 {
        bb.combinationRule = rule;
        // SAFETY: the rules exercised here touch no image memory unless the
        // test set up cmLookupTable, which points at a live Vec.
        unsafe { bb.mergeFnwith(s, d) }
    }

    /// Rules 0..17 are plain boolean functions of the two words; assert each
    /// against its definition.
    #[test]
    fn bitwise_rules_match_their_definitions() {
        let s: u32 = 0xF0F0_3C3C;
        let d: u32 = 0xFF00_00FF;
        let mut bb = bb_at_depth(32);
        let expected: [(sqInt, u32); 18] = [
            (0, 0),
            (1, s & d),
            (2, s & !d),
            (3, s),
            (4, !s & d),
            (5, d),
            (6, s ^ d),
            (7, s | d),
            (8, !s & !d),
            (9, !s ^ d),
            (10, !d),
            (11, s | !d),
            (12, !s),
            (13, !s | d),
            (14, !s | !d),
            (15, d),
            (16, d),
            (17, d),
        ];
        for (rule, want) in expected {
            assert_eq!(merge(&mut bb, rule, s, d), want, "rule {rule}");
        }
    }

    #[test]
    fn add_and_sub_word_wrap() {
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 18, 0xFFFF_FFFF, 1), 0); // wraps
        assert_eq!(merge(&mut bb, 18, 3, 4), 7);
        assert_eq!(merge(&mut bb, 19, 0, 1), 0xFFFF_FFFF); // s - d wraps
        assert_eq!(merge(&mut bb, 19, 9, 4), 5);
    }

    /// Reference: per-field saturating add over the given field offsets.
    fn sat_add(w1: u32, w2: u32, offsets: &[u32], bits: u32) -> u32 {
        let mask = (1u32 << bits) - 1;
        let mut out = 0;
        for &o in offsets {
            let sum = ((w1 >> o) & mask) + ((w2 >> o) & mask);
            out |= sum.min(mask) << o;
        }
        out
    }

    #[test]
    fn rgbAdd_saturates_per_component() {
        // 32bpp: four 8-bit fields.
        let mut bb = bb_at_depth(32);
        let s = 0x01FF_0080;
        let d = 0x0201_0080;
        assert_eq!(
            merge(&mut bb, 20, s, d),
            sat_add(s, d, &[0, 8, 16, 24], 8)
        );
        // 16bpp: three 5-bit fields per half-word; bit 15/31 are dropped.
        let mut bb = bb_at_depth(16);
        let s = 0xFFFF_7FFF;
        let d = 0x0001_0421;
        assert_eq!(
            merge(&mut bb, 21 - 1, s, d), // rule 20
            sat_add(s & 0x7FFF_7FFF, d & 0x7FFF_7FFF, &[0, 5, 10, 16, 21, 26], 5)
        );
        // 8bpp: rule 20 saturates whole 8-bit pixels.
        let mut bb = bb_at_depth(8);
        let s = 0x0080_FF01;
        let d = 0x0090_0102;
        assert_eq!(merge(&mut bb, 20, s, d), sat_add(s, d, &[0, 8, 16, 24], 8));
        // 1bpp: saturating single-bit add is bitwise OR.
        let mut bb = bb_at_depth(1);
        assert_eq!(merge(&mut bb, 20, 0b1100, 0b1010), 0b1110);
    }

    /// Reference: per-field absolute difference.
    fn abs_diff(w1: u32, w2: u32, offsets: &[u32], bits: u32) -> u32 {
        let mask = (1u32 << bits) - 1;
        let mut out = 0;
        for &o in offsets {
            let a = (w1 >> o) & mask;
            let b = (w2 >> o) & mask;
            out |= a.abs_diff(b) << o;
        }
        out
    }

    #[test]
    fn rgbSub_is_absolute_difference() {
        let mut bb = bb_at_depth(32);
        let s = 0x1020_30FF;
        let d = 0x2010_4001;
        assert_eq!(merge(&mut bb, 21, s, d), abs_diff(s, d, &[0, 8, 16, 24], 8));
        let mut bb = bb_at_depth(16);
        let s = 0x7C1F_001F;
        let d = 0x0001_7C00;
        assert_eq!(
            merge(&mut bb, 21, s, d),
            abs_diff(s, d, &[0, 5, 10], 5) | abs_diff(s >> 16, d >> 16, &[0, 5, 10], 5) << 16
        );
    }

    #[test]
    fn rgbMax_and_min_per_component() {
        let mut bb = bb_at_depth(32);
        let s = 0x10FF_0080;
        let d = 0x2000_FF7F;
        let max = |a: u32, b: u32, o: u32| (((a >> o) & 0xFF).max((b >> o) & 0xFF)) << o;
        let want: u32 = [0u32, 8, 16, 24].iter().map(|&o| max(s, d, o)).sum();
        assert_eq!(merge(&mut bb, 27, s, d), want);
        let min = |a: u32, b: u32, o: u32| (((a >> o) & 0xFF).min((b >> o) & 0xFF)) << o;
        let want: u32 = [0u32, 8, 16, 24].iter().map(|&o| min(s, d, o)).sum();
        assert_eq!(merge(&mut bb, 28, s, d), want);
        // 29 is min with the source inverted first.
        let want: u32 = [0u32, 8, 16, 24].iter().map(|&o| min(!s, d, o)).sum();
        assert_eq!(merge(&mut bb, 29, s, d), want);
    }

    #[test]
    fn rgbMul_multiplies_fractionally() {
        // Per the C comment, each field computes ((a+1)*(b+1)-1) >> bits:
        // multiplying by 0xFF is identity, by 0 is (almost) zero.
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 37, 0xFFFF_FFFF, 0x1234_5678), 0x1234_5678);
        assert_eq!(merge(&mut bb, 37, 0x00FF_00FF, 0xFF80_FF80), 0x0080_0080);
        // 0x80 * 0x80: (0x81 * 0x81 - 1) >> 8 = 0x41.
        assert_eq!(merge(&mut bb, 37, 0x0000_0080, 0x0000_0080), 0x0000_0041);
    }

    #[test]
    fn alphaBlend_uses_source_alpha_byte() {
        let mut bb = bb_at_depth(32);
        let d = 0x0011_2233;
        // alpha 0 answers dest, alpha 255 answers source.
        assert_eq!(merge(&mut bb, 24, 0x00FF_FFFF, d), d);
        assert_eq!(merge(&mut bb, 24, 0xFFAB_CDEF, d), 0xFFAB_CDEF);
        // alpha 128 with black dest: each channel is
        // ceil((s * 128 + d * 127) / 255); the alpha channel blends the
        // source's alpha against 255 (the C ORs 0xFF0000 into the AG term).
        let s = 0x80FF_00FF;
        let got = merge(&mut bb, 24, s, 0x0000_0000);
        let ch = |sc: u32, dc: u32| (sc * 128 + dc * 127).div_ceil(255);
        // The source's own alpha byte is replaced by 0xFF in the blend (the
        // C ORs 0xFF0000 into the alpha/green term), so the answered alpha
        // is ch(0xFF, destAlpha).
        let want = (ch(0xFF, 0) << 24) | (ch(0xFF, 0) << 16) | (ch(0, 0) << 8) | ch(0xFF, 0);
        assert_eq!(got, want);
    }

    #[test]
    fn alphaBlendScaled_adds_premultiplied_source() {
        // Reference from the definition: dst' = src + floor(dst*(255-a)/256),
        // saturated per channel.
        let ch = |s: u32, d: u32, un: u32| (d * un / 256 + s).min(255);
        let s = 0x8040_2010;
        let d = 0xFFFF_FFFF;
        let un = 255 - 0x80;
        let want = (ch(0x40, 0xFF, un) << 16) | (ch(0x20, 0xFF, un) << 8) | ch(0x10, 0xFF, un)
            | (ch(0x80, 0xFF, un) << 24);
        assert_eq!(rules::alphaBlendScaledwith(s, d), want);
        // Fully opaque source replaces the destination.
        assert_eq!(rules::alphaBlendScaledwith(0xFF12_3456, 0x0055_66AA), 0xFF12_3456);
    }

    #[test]
    fn alphaBlendConst_blends_by_sourceAlpha() {
        let mut bb = bb_at_depth(32);
        let s = 0x1122_3344;
        let d = 0xFFEE_DDCC;
        bb.sourceAlpha = 0;
        // ceil-division: alpha 0 answers dest, alpha 255 answers source.
        assert_eq!(merge(&mut bb, 30, s, d), d);
        bb.sourceAlpha = 255;
        assert_eq!(merge(&mut bb, 30, s, d), s);
        // Paint mode: a zero source pixel leaves dest.
        bb.sourceAlpha = 255;
        assert_eq!(merge(&mut bb, 31, 0, d), d);
        // 16bpp: per 5-bit channel ceil((s*a + d*(255-a))/255), both pixels.
        let mut bb = bb_at_depth(16);
        bb.destMask = 0xFFFF_FFFF;
        bb.sourceAlpha = 128;
        let s = 0x7C00_001F;
        let d = 0x001F_7C00;
        let ch = |sc: u32, dc: u32| (sc * 128 + dc * 127).div_ceil(255) & 0x1F;
        let pix = |sp: u32, dp: u32| {
            ch(sp & 31, dp & 31)
                | (ch((sp >> 5) & 31, (dp >> 5) & 31) << 5)
                | (ch((sp >> 10) & 31, (dp >> 10) & 31) << 10)
        };
        let want = pix(s & 0xFFFF, d & 0xFFFF) | (pix(s >> 16, d >> 16) << 16);
        assert_eq!(merge(&mut bb, 30, s, d), want);
        // destMask gating: with only the low pixel selected, the high pixel
        // of the answer keeps the *destination* bits (result starts as d).
        bb.destMask = 0x0000_FFFF;
        let want_masked = pix(s & 0xFFFF, d & 0xFFFF) | (d & 0xFFFF_0000);
        assert_eq!(merge(&mut bb, 30, s, d), want_masked);
    }

    #[test]
    fn pixPaint_and_pixMask() {
        let mut bb = bb_at_depth(8);
        let s = 0x00FF_0000;
        let d = 0x1122_3344;
        // paint: source-zero fields keep dest, others take source.
        assert_eq!(merge(&mut bb, 25, s, d), 0x11FF_3344);
        assert_eq!(merge(&mut bb, 25, 0, d), d);
        // mask: source-nonzero fields clear dest.
        assert_eq!(merge(&mut bb, 26, s, d), 0x1100_3344);
    }

    /// On an LP64 host, `partitionedAND` with nBits = 32 compares a 32-bit
    /// field against the sign-extended -1 mask and never matches: the C
    /// answers 0 where a 32-bit host would keep the destination. Mirrored.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn pixMask_at_depth32_reproduces_the_lp64_quirk() {
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 26, 0, 0x1234_5678), 0);
    }

    #[test]
    fn pixSwap_reverses_pixel_order() {
        let mut bb = bb_at_depth(8);
        assert_eq!(merge(&mut bb, 38, 0, 0x1122_3344), 0x4433_2211);
        let mut bb = bb_at_depth(16);
        assert_eq!(merge(&mut bb, 38, 0, 0x1234_5678), 0x5678_1234);
        let mut bb = bb_at_depth(4);
        assert_eq!(merge(&mut bb, 38, 0, 0x1234_5678), 0x8765_4321);
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 38, 0, 0xDEAD_BEEF), 0xDEAD_BEEF);
    }

    #[test]
    fn pixClear_zeroes_matching_pixels() {
        let mut bb = bb_at_depth(8);
        assert_eq!(merge(&mut bb, 39, 0x1122_3344, 0x1123_3345), 0x0023_0045);
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 39, 0xAA, 0xAA), 0);
        assert_eq!(merge(&mut bb, 39, 0xAA, 0xAB), 0xAB);
    }

    #[test]
    fn fixAlpha_fills_zero_alpha_from_source() {
        let mut bb = bb_at_depth(32);
        assert_eq!(merge(&mut bb, 40, 0xFF00_0000, 0), 0);
        assert_eq!(merge(&mut bb, 40, 0xFF00_0000, 0x8811_2233), 0x8811_2233);
        assert_eq!(merge(&mut bb, 40, 0xCC00_0000, 0x0011_2233), 0xCC11_2233);
        // Not 32bpp: untouched.
        let mut bb = bb_at_depth(16);
        assert_eq!(merge(&mut bb, 40, 0xFFFF_FFFF, 0x1234), 0x1234);
    }

    #[test]
    fn OLDrgbDiff_tallies_into_bitCount() {
        // depth 8: counts differing pixels, answer is the untouched dest.
        let mut bb = bb_at_depth(8);
        assert_eq!(merge(&mut bb, 22, 0x1122_3344, 0x1122_3344), 0x1122_3344);
        assert_eq!(bb.bitCount, 0);
        assert_eq!(merge(&mut bb, 22, 0x1122_3344, 0x1123_3345), 0x1123_3345);
        assert_eq!(bb.bitCount, 2);
        // depth 32: sums the per-channel absolute differences (RGB only).
        let mut bb = bb_at_depth(32);
        merge(&mut bb, 22, 0x0010_2030, 0x0020_1030);
        assert_eq!(bb.bitCount, 0x10 + 0x10);
    }

    #[test]
    fn rgbDiff_respects_the_destination_mask() {
        // depth 8: pixel-equality count, only under destMask.
        let mut bb = bb_at_depth(8);
        bb.destMask = 0x0000_FFFF; // low two pixels only
        merge(&mut bb, 32, 0x1122_3344, 0x9922_3345);
        assert_eq!(bb.bitCount, 1); // only the 0x44/0x45 mismatch counts
        // depth 16: channel-difference sums.
        let mut bb = bb_at_depth(16);
        bb.destMask = 0xFFFF_FFFF;
        merge(&mut bb, 32, 0x0000_001F, 0x0000_0000);
        assert_eq!(bb.bitCount, 31);
    }

    #[test]
    fn tally_rules_count_into_the_color_map() {
        use crate::state::{COLOR_MAP_INDEXED_PART, COLOR_MAP_PRESENT};
        // Rule 33 tallies destination pixels through rgbMap into the map.
        let mut counts = vec![0u32; 256];
        let mut bb = bb_at_depth(8);
        bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
        bb.cmMask = 255;
        bb.cmLookupTable = counts.as_mut_ptr() as usize;
        bb.destMask = 0xFFFF_FFFF;
        merge(&mut bb, 33, 0, 0x0102_0201);
        assert_eq!(counts[1], 2);
        assert_eq!(counts[2], 2);
        // Masked pixels are not tallied.
        counts.iter_mut().for_each(|c| *c = 0);
        bb.destMask = 0xFF;
        merge(&mut bb, 33, 0, 0x0102_0201);
        assert_eq!(counts[1], 1);
        assert_eq!(counts[2], 0);
        // Rule 23 (OLD) ignores the mask.
        counts.iter_mut().for_each(|c| *c = 0);
        bb.destMask = 0xFF;
        merge(&mut bb, 23, 0, 0x0102_0201);
        assert_eq!(counts[1], 2);
        assert_eq!(counts[2], 2);
        // Without an indexed map the rule is a no-op.
        bb.cmFlags = 0;
        assert_eq!(merge(&mut bb, 33, 0, 0xAB), 0xAB);
    }

    #[test]
    fn rgbComponentAlpha32_matches_the_documented_formula() {
        let mut bb = bb_at_depth(32);
        bb.componentAlphaModeAlpha = 0xFF;
        bb.componentAlphaModeColor = 0xFFFFFF;
        // A zero mask leaves the destination alone.
        // SAFETY: no gamma tables installed.
        assert_eq!(unsafe { bb.rgbComponentAlpha32with(0, 0x1234_5678) }, 0x1234_5678);
        // Full mask, white color, black opaque dest: each channel becomes
        // (d*(255-255))>>8 + (255*255)>>8 = 254; the alpha byte of the mask
        // is 0, so dst.A = (255*255)>>8 + 0 = 254.
        assert_eq!(
            unsafe { bb.rgbComponentAlpha32with(0x00FF_FFFF, 0xFF00_0000) },
            0xFEFE_FEFE
        );
        // Per-component mask: only the masked component moves toward the
        // color; b = (0xFF*0xFF)>>8 = 254 for the blue-only mask on black.
        assert_eq!(
            unsafe { bb.rgbComponentAlpha32with(0x0000_00FF, 0xFF00_0000) },
            0xFE00_00FE
        );
    }

    #[test]
    fn rule41_at_depth16_expands_blends_and_repacks() {
        let mut bb = bb_at_depth(16);
        bb.componentAlphaModeAlpha = 0xFF;
        bb.componentAlphaModeColor = 0xFFFFFF;
        // Mask all-ones on a black pixel: the expanded 32-bit blend gives
        // a = 255 (capped) and r = g = b = 254 -> 0xFFF7F7F7, and the C's
        // 32 -> 16 repack (rgbMap:from:to: with whole-pixel widths) simply
        // truncates to the top 16 bits: 0xFFF7 per pixel, alpha bit included.
        // SAFETY: no gamma tables installed.
        let got = unsafe { bb.rgbComponentAlphawith(0xFFFF_FFFF, 0x0000_0000) };
        assert_eq!(got, 0xFFF7_FFF7);
        // Zero word answers dest.
        assert_eq!(unsafe { bb.rgbComponentAlphawith(0, 0x1234_5678) }, 0x1234_5678);
    }
}

// ---------------------------------------------------------------------------
// Helpers: masks, maps, dither, warp deltas
// ---------------------------------------------------------------------------

mod helpers {
    use super::*;

    #[test]
    fn mask_table_matches_the_c_array() {
        assert_eq!(MASK_TABLE[1], 1);
        assert_eq!(MASK_TABLE[2], 3);
        assert_eq!(MASK_TABLE[4], 15);
        assert_eq!(MASK_TABLE[8], 255);
        assert_eq!(MASK_TABLE[16], 65535);
        assert_eq!(MASK_TABLE[32], 0xFFFF_FFFF);
    }

    #[test]
    fn rgbMapfromto_expands_and_truncates() {
        // 15-bit -> 24-bit: each 5-bit channel goes to the top of its byte.
        assert_eq!(rules::rgbMapfromto(0x7FFF, 5, 8), 0xF8F8F8);
        assert_eq!(rules::rgbMapfromto(0x0001, 5, 8), 0x000008);
        // 24-bit -> 15-bit: truncation to the top 5 bits of each byte.
        assert_eq!(rules::rgbMapfromto(0xFFFFFF, 8, 5), 0x7FFF);
        assert_eq!(rules::rgbMapfromto(0x080808, 8, 5), 0x0421);
        // A non-zero pixel that truncates to zero answers 1 (transparency).
        assert_eq!(rules::rgbMapfromto(0x070707, 8, 5), 1);
        // Equal widths: the 15/24-bit clamps.
        assert_eq!(rules::rgbMapfromto(0xFFFF, 5, 5), 0x7FFF);
        assert_eq!(rules::rgbMapfromto(-1, 8, 8), 0xFFFFFF);
    }

    #[test]
    fn dither_lookup_spot_values() {
        // 255 always dithers to full 5-bit intensity.
        for t in 0..16 {
            assert_eq!(DITHER8_LOOKUP[t * 256 + 255], 31);
        }
        assert_eq!(DITHER8_LOOKUP[0], 0);
        // 128: threshold 0, value ditherValues16[16] = 15, for any t.
        assert_eq!(DITHER8_LOOKUP[128], 15);
        assert_eq!(DITHER8_LOOKUP[7 * 256 + 128], 15);
        // 0xFF per channel -> 0x7FFF.
        assert_eq!(rules::dither32To16threshold(0x00FF_FFFF, 0), 0x7FFF);
        assert_eq!(rules::dither32To16threshold(0, 0), 0);
    }

    #[test]
    fn deltaFromtonSteps_vectors() {
        // Ascending: ((x2-x1) + 2^14) / (n+1) + 1.
        assert_eq!(deltaFromtonSteps(0, 3 << 14, 3), (1 << 14) + 1);
        assert_eq!(deltaFromtonSteps(0, 1 << 14, 3), (1 << 13) + 1);
        assert_eq!(deltaFromtonSteps(5, 5, 7), 0);
        // Descending is the negated ascending delta.
        assert_eq!(deltaFromtonSteps(3 << 14, 0, 3), -((1 << 14) + 1));
    }

    #[test]
    fn setupColorMasksFromto_builds_the_c_tables() {
        let mut bb = BitBlt::new();
        // Expanding 5 -> 8.
        bb.setupColorMasksFromto(5, 8);
        assert_eq!(bb.cmMaskTable, CmTable::Local);
        assert_eq!(bb.cmLocalMasks, [0x7C00, 0x3E0, 0x1F, 0]);
        assert_eq!(bb.cmLocalShifts, [9, 6, 3, 0]);
        // Compressing 8 -> 5.
        let mut bb = BitBlt::new();
        bb.setupColorMasksFromto(8, 5);
        assert_eq!(bb.cmLocalMasks, [0xF80000, 0xF800, 0xF8, 0]);
        assert_eq!(bb.cmLocalShifts, [-9, -6, -3, 0]);
    }

    #[test]
    fn destMaskAndPointerInit_masks_at_word_boundaries() {
        // 1bpp MSB, dx = 5, w = 13: startBits = 27, endBits = 18 % 32 -> the
        // first word keeps bits 5..31 (27 low bits of the MSB mask), and,
        // since 13 < 27, one word with the intersection.
        let mut bb = BitBlt::new();
        bb.destDepth = 1;
        bb.destPPW = 32;
        bb.destMSB = 1;
        bb.destPitch = 8;
        bb.dx = 5;
        bb.bbW = 13;
        bb.destMaskAndPointerInit();
        // mask1 selects pixels 5.. (27 low bits), mask2 pixels ..17.
        // bbW (13) < startBits (27): single word, intersection.
        assert_eq!(bb.nWords, 1);
        assert_eq!(bb.mask1, (0xFFFF_FFFFu32 >> 5) & (0xFFFF_FFFFu32 << (32 - 18)));
        assert_eq!(bb.mask2, 0);
        // Crossing a word: dx = 28, w = 8 at 1bpp -> 2 words.
        bb.dx = 28;
        bb.bbW = 8;
        bb.destMaskAndPointerInit();
        assert_eq!(bb.nWords, 2);
        assert_eq!(bb.mask1, 0x0000_000F); // last 4 pixels of word 0
        assert_eq!(bb.mask2, 0xF000_0000); // first 4 pixels of word 1
    }
}

// ---------------------------------------------------------------------------
// clipRange
// ---------------------------------------------------------------------------

mod clipping {
    use super::*;

    fn clip_bb() -> BitBlt {
        let mut bb = BitBlt::new();
        bb.clipX = 10;
        bb.clipY = 20;
        bb.clipWidth = 100;
        bb.clipHeight = 50;
        bb.sourceWidth = 300;
        bb.sourceHeight = 300;
        bb
    }

    #[test]
    fn unclipped_passes_through() {
        let mut bb = clip_bb();
        bb.destX = 30;
        bb.destY = 30;
        bb.sourceX = 1;
        bb.sourceY = 2;
        bb.width = 20;
        bb.height = 10;
        bb.clipRange();
        assert_eq!(
            (bb.dx, bb.dy, bb.sx, bb.sy, bb.bbW, bb.bbH),
            (30, 30, 1, 2, 20, 10)
        );
    }

    #[test]
    fn clipped_left_and_top_shifts_source() {
        let mut bb = clip_bb();
        bb.destX = 4; // 6 left of clipX
        bb.destY = 15; // 5 above clipY
        bb.sourceX = 0;
        bb.sourceY = 0;
        bb.width = 50;
        bb.height = 30;
        bb.clipRange();
        assert_eq!((bb.dx, bb.dy), (10, 20));
        assert_eq!((bb.sx, bb.sy), (6, 5));
        assert_eq!((bb.bbW, bb.bbH), (44, 25));
    }

    #[test]
    fn clipped_right_and_bottom_shrinks() {
        let mut bb = clip_bb();
        bb.destX = 100;
        bb.destY = 60;
        bb.width = 50;
        bb.height = 50;
        bb.clipRange();
        assert_eq!(bb.bbW, 10); // clip right edge at 110
        assert_eq!(bb.bbH, 10); // clip bottom edge at 70
    }

    #[test]
    fn negative_source_origin_moves_dest() {
        let mut bb = clip_bb();
        bb.destX = 30;
        bb.destY = 30;
        bb.sourceX = -5;
        bb.sourceY = -7;
        bb.width = 20;
        bb.height = 20;
        bb.clipRange();
        assert_eq!((bb.sx, bb.sy), (0, 0));
        assert_eq!((bb.dx, bb.dy), (35, 37));
        assert_eq!((bb.bbW, bb.bbH), (15, 13));
    }

    #[test]
    fn source_extent_limits_the_region() {
        let mut bb = clip_bb();
        bb.sourceWidth = 25;
        bb.sourceHeight = 25;
        bb.destX = 30;
        bb.destY = 30;
        bb.sourceX = 20;
        bb.sourceY = 24;
        bb.width = 20;
        bb.height = 20;
        bb.clipRange();
        assert_eq!(bb.bbW, 5);
        assert_eq!(bb.bbH, 1);
    }

    #[test]
    fn fully_clipped_yields_empty_region() {
        let mut bb = clip_bb();
        bb.destX = 500;
        bb.destY = 30;
        bb.width = 20;
        bb.height = 20;
        bb.clipRange();
        assert!(bb.bbW <= 0);
    }

    #[test]
    fn no_source_skips_source_clipping() {
        let mut bb = clip_bb();
        bb.noSource = true;
        bb.sourceX = -100; // would shift dest if source clipping ran
        bb.destX = 30;
        bb.destY = 30;
        bb.width = 20;
        bb.height = 10;
        bb.clipRange();
        assert_eq!((bb.dx, bb.bbW), (30, 20));
    }
}

// ---------------------------------------------------------------------------
// The copy loops, against a per-pixel reference
// ---------------------------------------------------------------------------

mod copy_loops {
    use super::*;

    /// Word-exact: 1bpp MSB copy with a skew. Copy 8 pixels from x=4 to x=9.
    /// Source bits 4..11 of 0xABCD1234 are 0xBC; placed at dest bits 9..16
    /// that is 0xBC << 15 = 0x005E0000.
    #[test]
    fn copyLoop_word_exact_skewed_1bpp() {
        let mut src = Form::new(32, 1, 1, true);
        let mut dst = Form::new(32, 1, 1, true);
        src.bits[0] = 0xABCD_1234;
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.destX = 9;
        bb.sourceX = 4;
        bb.width = 8;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(dst.bits[0], 0x005E_0000);
    }

    /// Word-exact: 1bpp copy crossing a word boundary. Bits 0..7 of
    /// 0xDEADBEEF (0xDE) land at dest bits 28..35: word0 gets the top nibble
    /// 0xD in its low bits, word1 the nibble 0xE in its high bits.
    #[test]
    fn copyLoop_word_exact_across_word_boundary() {
        let mut src = Form::new(64, 1, 1, true);
        let mut dst = Form::new(64, 1, 1, true);
        src.bits[0] = 0xDEAD_BEEF;
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.destX = 28;
        bb.sourceX = 0;
        bb.width = 8;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(dst.bits[0], 0x0000_000D);
        assert_eq!(dst.bits[1], 0xE000_0000);
    }

    /// Word-exact fill: rule 3, no source, no halftone fills with all-ones
    /// under the pixel masks. 4bpp MSB, dx=1, w=3 -> word 0x0FFF0000.
    #[test]
    fn fill_word_exact_4bpp() {
        let mut dst = Form::new(8, 1, 4, true);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, None, 3);
        bb.destX = 1;
        bb.width = 3;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(dst.bits[0], 0x0FFF_0000);
    }

    /// Sweep source/dest alignments, widths and depths for the same-depth
    /// copyLoop (rules 3 and 6), comparing against the per-pixel reference.
    #[test]
    fn copyLoop_matches_reference_across_alignments() {
        for &depth in &[1, 2, 4, 8, 16, 32] {
            for &msb in &[true, false] {
                for &(sx, sy) in &[(0, 0), (1, 1), (7, 2)] {
                    for &(dx, dy) in &[(0, 0), (3, 1), (5, 3)] {
                        for &w in &[1, 5, 29] {
                            for &rule in &[3, 6] {
                                let mut src = Form::new(64, 8, depth, msb);
                                let mut dst = Form::new(64, 8, depth, msb);
                                src.fill_random(0xBEEF ^ (depth as u32) << 8);
                                dst.fill_random(0x1234 ^ w as u32);
                                let before = dst.clone();
                                let mut bb = BitBlt::new();
                                setup(&mut bb, &mut dst, Some(&src), rule);
                                bb.destX = dx;
                                bb.destY = dy;
                                bb.sourceX = sx;
                                bb.sourceY = sy;
                                bb.width = w;
                                bb.height = 3;
                                run(&mut bb);
                                let f: &dyn Fn(u32, u32) -> u32 = if rule == 3 {
                                    &|s, _| s
                                } else {
                                    &|s, d| s ^ d
                                };
                                let expected = ref_blit_per_pixel(
                                    &before, &src, f, sx as i32, sy as i32, dx as i32, dy as i32,
                                    w as i32, 3,
                                );
                                assert_forms_equal(
                                    &dst,
                                    &expected,
                                    &format!(
                                        "depth {depth} msb {msb} s({sx},{sy}) d({dx},{dy}) w{w} rule {rule}"
                                    ),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Overlapping copies inside one form: for the directions the C
    /// handles exactly (vertical, leftward, word-aligned or phase-aligned
    /// rightward), the result is memmove semantics; compare against a
    /// snapshot-based reference. (Skewed rightward same-row copies are NOT
    /// memmove-exact in the C -- see overlap_matches_the_c_plugin_exactly.)
    #[test]
    fn overlapping_copy_uses_the_right_direction() {
        for &depth in &[1, 8, 32] {
            for &(sx, sy, dx, dy) in &[
                (3, 0, 0, 0),   // left, same row: forward copy is safe
                (0, 0, 0, 2),   // down: vDir = -1
                (0, 2, 0, 0),   // up: forward
                (1, 1, 4, 3),   // down-right: rows are disjoint
                (0, 0, 32, 0),  // right, word-aligned at every depth: skew 0
                (1, 0, 5, 0),   // right, same in-word phase at 8/32 bpp
            ] {
                if depth == 1 && (sx, dx) == (1, 5) {
                    // 1bpp phase differs (5-1 is not a multiple of 32).
                    continue;
                }
                let mut form = Form::new(64, 8, depth, true);
                form.fill_random(0xCAFE + depth as u32);
                let before = form.clone();
                let (w, h) = (24, 4);
                let mut bb = BitBlt::new();
                // Same buffer and same form oop on both sides.
                setup(&mut bb, &mut form, None, 3);
                bb.noSource = false;
                bb.sourceForm = DEST_FORM_OOP;
                bb.sourceBits = bb.destBits;
                bb.sourceWidth = bb.destWidth;
                bb.sourceHeight = bb.destHeight;
                bb.sourceDepth = bb.destDepth;
                bb.sourceMSB = bb.destMSB;
                bb.sourcePPW = bb.destPPW;
                bb.sourcePitch = bb.destPitch;
                bb.endOfSource = bb.endOfDestination;
                bb.destX = dx;
                bb.destY = dy;
                bb.sourceX = sx;
                bb.sourceY = sy;
                bb.width = w;
                bb.height = h;
                run(&mut bb);
                let expected = ref_blit_per_pixel(
                    &before, &before, &|s, _| s, sx as i32, sy as i32, dx as i32, dy as i32,
                    w as i32, h as i32,
                );
                assert_forms_equal(
                    &form,
                    &expected,
                    &format!("overlap depth {depth} s({sx},{sy}) d({dx},{dy})"),
                );
            }
        }
    }

    /// Golden vectors captured from the real `BitBltPlugin.c` (compiled
    /// standalone and driven through its own `performCopyLoop`): overlapping
    /// same-form copies, including the skewed rightward cases where the C's
    /// reversed loop is *not* memmove-exact. The port must match the C word
    /// for word, quirks included.
    #[test]
    fn overlap_matches_the_c_plugin_exactly() {
        #[allow(clippy::type_complexity)]
        let cases: Vec<(i32, i32, i32, i32, i32, i32, i32, Vec<u32>)> = vec![
            (8, 0, 0, 3, 0, 40, 4, vec![0xF39A94F4, 0x9A945ED5, 0x93F025F3, 0x3DE340AF, 0x5EFD9F8C, 0xD62E7235, 0xF2BB298F, 0x55CA7469, 0xC873439B, 0x147FC613, 0xB8546DE8, 0x952F1127, 0x3D775C5A, 0xEF147FF1, 0x09FC789C, 0x1D8B3B4B, 0x0073D809, 0x73D82E1D, 0xF141B500, 0x8A679086, 0xC995AFA1, 0x664742B2, 0x76DDB940, 0xE0EBC437, 0xEF045310, 0x663D96BB, 0xCFD7FD38, 0xC07CAB37, 0x675A8F2A, 0x8C2CF481, 0x24BBC3EC, 0x0C13EE5B, 0x114D4F24, 0x4D4FFE0C, 0x09374511, 0xA93FE0BB, 0x5A71BFCD, 0xFFD4125A, 0xADE449F7, 0x59A11478, 0x1E1963F1, 0xBEAF66EF, 0xF67F8D88, 0xBEB90947, 0x8D75B5FA, 0x1754CD11, 0x3C1B233C, 0x8AB5A56B, 0xCF03FB3C, 0x03FBCE8A, 0x14D0D5CF, 0x7F6C30ED, 0xB291CFF6, 0xCFD4E216, 0xE0CED9C6, 0x74EA64ED, 0x86B27316, 0x9AD536D9, 0x854B1DD8, 0xCEA52B57, 0xC595D0CA, 0x59F509A1, 0x606F968C, 0x4BC1607B, 0x8DB4DB9E, 0xF08D0E65, 0x0F31EC80, 0xFEB2F5DF, 0x7C4349B2, 0x8D989D69, 0x9B27C7B4, 0x7799CF83, 0xDBB7AF06, 0xAC153AAD, 0x29C8DC28, 0xE3421167, 0xCCC7DF9A, 0x1BB6AA31, 0x0B4E1DDC, 0xD9C81F8B, 0x1CBCEF6E, 0x5B2AEFF5, 0x6825C0D0, 0x657C9DEF, 0x77073282, 0x149E4FF9, 0x2BA73904, 0xA6087093, 0x06123CD6, 0xC07F4E3D, 0x60073A78, 0xB3D0BB77, 0xD758E26A, 0xB282AEC1, 0x6F8BB92C, 0xA79AE29B, 0x96B9373E, 0x14E77585, 0xFEFFE920, 0xEA7089FF, 0x75088F52, 0x95FAE689, 0x55683E54, 0x31C395A3, 0x4DE77EA6, 0xCBDC85CD, 0x84F66CC8, 0xCBD22987, 0x60D5D93A, 0x92821751, 0xC93D687C, 0x204AA9AB, 0x3186B30E, 0xE7FB9F15, 0xCEA56570, 0x892FBA0F, 0x63746022, 0x0AF76119, 0xD71FD7A4, 0xE7FC3EB3, 0xA4B47476, 0xDF85E15D, 0xF01B7318, 0x9B075B97, 0xFC0BC40A, 0xDE1DE3E1, 0xADB82BCC, 0xB72874BB]),
            (1, 3, 0, 8, 0, 29, 4, vec![0x74251C04, 0xC4C2AA37, 0xA1A128E0, 0x56064B81, 0x0C0D0947, 0x67D25D5B, 0xE960684A, 0xF3A47E45, 0x82D5DAE0, 0x766050BF, 0x82262712, 0xDCB21B49, 0x127A6C14, 0x81AA6863, 0xE958B266, 0xD8FAA68D]),
            (8, 0, 0, 4, 0, 40, 4, vec![0x92BC8CA9, 0x92BC8CA9, 0x8C8E6DF4, 0xDC4DC0C3, 0x3A3A6F46, 0x57927DED, 0x5930C668, 0x931E76A7, 0x6E9083DA, 0x4E988171, 0xA1C98C1C, 0xADD7B7AE, 0xF3909B35, 0x60CAF310, 0xC025AB2F, 0xB8245EC2, 0x54FB0F39, 0x54FB0F39, 0x11D06F44, 0x0DFDB1D3, 0x64C60D16, 0x8ADA617D, 0xD192B4B8, 0xFADB70B7, 0x6A5B96AA, 0xD7B75601, 0xA31BB76C, 0xBE270F7E, 0xF7E4F0C5, 0x8E3AAB60, 0x9211E73F, 0x5EA1CB92, 0x630475C9, 0x630475C9, 0x8B380494, 0x128E26E3, 0x9D505EE6, 0x83C9690D, 0x6FC97708, 0xCEDF2EC7, 0x3BD69D7A, 0x29FD8E91, 0x5145F6BC, 0xF04B9B4E, 0xBAC4EA55, 0x1019B7B0, 0x3D1D674F, 0x24CDAC62, 0xE321C059, 0xE321C059, 0x007A2DE4, 0x6C301FF3, 0x065664B6, 0xD0B8949D, 0xE45A0D58, 0x03EAB0D7, 0x36CE984A, 0x34D42B21, 0xAA9D4A0C, 0xB6625B1E, 0x95A987E5, 0xA78D1800, 0x4E292B5F, 0xD8150132, 0xB1DBEEE9, 0x3A8BEB34, 0xED549D03, 0x55951E86, 0xFA40E42D, 0x150977A8, 0xC2FEF6E7, 0xD650871A, 0x65E42BB1, 0x96B6B15C, 0x005C9D0B, 0x7DC84EEE, 0x244BC975, 0xF2F9CC50, 0x0E56336F, 0x5524CA02, 0xF1FC0179, 0x13A23C84, 0xC8AC9E13, 0xA4098C56, 0x133B57BD, 0x6CDCB5F8, 0x795D00F7, 0x8CA969EA, 0x39169041, 0x36672CAC, 0x7844C01B, 0x7F1A76BE, 0xD4A4AF05, 0xBE04D4A0, 0x93057F7F, 0x57EA06D2, 0x9C8AF809, 0xC73221D4, 0xA1292323, 0x3DF0AE26, 0x48C0EF4D, 0x2C18C848, 0xE885CF07, 0x936640BA, 0xC89458D1, 0x33C3BBFC, 0x35E1E72B, 0x8E35D28E, 0x76ED3895, 0x519330F0, 0xCDD80F8F, 0x4B91B7A2, 0x10D1D299, 0x41F09B24, 0x99FB2C33, 0x72C783F6, 0x722AAADD, 0xB842AE98, 0x363A6117, 0xBB540B8A, 0x5CC68561, 0x12215F4C, 0xC285123B, 0xEA37625E, 0xCD9E6625, 0xC3C9E140, 0x9CAEE39F, 0x1A88DC72]),
            (8, 1, 0, 5, 0, 17, 3, vec![0x7BADB960, 0xD6ADB960, 0xD67D9D3F, 0x889E0992, 0x13879BC9, 0x79043CE3, 0x4F217CE6, 0xCB5BEF0D, 0x899E4508, 0x06C1A4C7, 0xD35A9B7A, 0x40E57491, 0x56B0A4BC, 0x1B6638EB, 0xEA40794E, 0xAAA83055, 0xA28845B0, 0x978845B0, 0x974E9D4F, 0x9AD16A62, 0xED066659, 0xD13AB5F3, 0x0CE702B6, 0x33049A9D, 0xC29A5B58, 0x8A42A6D7, 0x1B4A164A, 0x514D9121, 0x716B780C, 0x89E4D3FB, 0x1D06B91E, 0x77764DE5, 0x0BD72600, 0x45D72600, 0x457FE15F, 0xBA003F32, 0xF38214E9, 0x791FB303, 0xDEC53C86, 0x0BA66A2D, 0x809545A8, 0x0B2C6CE7, 0x23A3851A, 0x324F11B1, 0x05C85F5C, 0xAD96730B, 0x3AFC2CEE, 0x18620F75, 0x0DFF5A50, 0x8232696F, 0x3AD78802, 0x61C3A779, 0x82E9AA84, 0xA3213413, 0xD5B92A56, 0x001A5DBD, 0xE69403F8, 0x4EBFF6F7, 0xD6B3E7EA, 0x77D2F641, 0x6C9C5AAC, 0xE74C161B, 0x74BDD4BE, 0x93647505, 0x2CA5E2A0, 0xBAC7357F, 0x514444D2, 0x48D41E09, 0x620D0FD4, 0xF71B3923, 0x35FFCC26, 0xD579754D, 0xECDB9648, 0x6E7E4507, 0xE6083EBA, 0x54023ED1, 0x87FC69FC, 0x1016BD2B, 0x9628B08E, 0x50B67E95, 0x68AFBEF0, 0x38DF458F, 0xE07375A2, 0x1FFC7899, 0x3B3F0924, 0x40CAC233, 0x471621F6, 0xFB1CB0DD, 0xB0F0FC98, 0xE8285717, 0x9A6D898A, 0x2745EB61, 0x133D8D4C, 0x8947683B, 0xD659C05E, 0xAAD12C25, 0x9041EF40, 0x325B999F, 0x4AD21A72, 0x54C5B729, 0x34749674, 0x0BA0CF43, 0x23B92BC6, 0x1A9D106D, 0xC59936E8, 0xADBF2D27, 0xA3F0C85A, 0x1046FBF1, 0xF2F4C49C, 0x4C6F174B, 0xA7AE042E, 0x7E6D7DB5, 0x8EC17390, 0xD95D31AF, 0x420D3342, 0xDAF8D9B9, 0xC4E2B7C4, 0x834E6053, 0x89E5E996, 0xA7D393FD, 0x82D94538, 0x3583C737, 0xE9DEFB2A, 0x7BEE7081, 0x84F70FEC, 0xFB5ECA5B, 0x87C27BFE, 0xBA847345]),
            (16, 2, 0, 5, 0, 22, 4, vec![0x03EB5A04, 0xFFA41D93, 0x7EE8CC63, 0x1D9303EB, 0x05D6FFA4, 0x833D7EE8, 0xEB78B270, 0xB877F164, 0xBB6AB318, 0xB3C131CE, 0xFA2C6FF4, 0x2F9BF953, 0x203EC9AC, 0x4A8526FF, 0x3F398852, 0xAEAD8B89, 0xE1389F54, 0xCAFE82A3, 0xB55387A6, 0xF8EEFACD, 0x96845DC8, 0xC70D6687, 0xB225F23A, 0x0E895C51, 0xB819E97C, 0xB5E336AB, 0x5773DC0E, 0x545CB415, 0x0AF17670, 0x90D2970F, 0x3FC79922, 0xF4E74619, 0xAA8C78A4, 0xA5A66BB3, 0x7946F4E7, 0x6BB3AA8C, 0xBD76A5A6, 0x965D7946, 0xA4181509, 0xD897CEE9, 0x1D0ADC05, 0x68E1E3C6, 0xECCC930A, 0x41BB17B8, 0xCBDE3F18, 0xC1A54B1F, 0xAEBD1DF2, 0x6299E4A9, 0xE3DBE5F4, 0xE10CD8C3, 0xF37EA746, 0x665955ED, 0x6B59BE68, 0x0A030EA7, 0x64BC3BDA, 0x3220D971, 0x1AC6041C, 0xE4DC50CB, 0x87A2EFAE, 0x73327335, 0x7012EB10, 0x6B9D432F, 0xEBC716C2, 0xC18E6739, 0xE25BE744, 0xEEE2C9D3, 0x6FF8C18E, 0xC9D3E25B, 0x4516EEE2, 0x397D6FF8, 0xACB896B7, 0x08B781D9, 0x4EAA6D46, 0xAE018A55, 0x2F6CEEB5, 0x63DBBD16, 0x477E4F00, 0xC8C57F3F, 0xF5D28392, 0x71CDCDC9, 0x1C817C94, 0x71193EE3, 0x2DF096E6, 0x2A3C410D, 0xBDAE6F08, 0x364FC6C7, 0x311E557A, 0x29F1E691, 0x43BE6EBC, 0x5D957AEB, 0x8CF2D34E, 0xC792C255, 0xD29DAFB0, 0xEAA0FF4F, 0x3B0C6462, 0xF9A11859, 0x7A01A5E4, 0x49E137F3, 0x2FE4F9A1, 0x37F37A01, 0x9CB649E1, 0x6C9D2FE4, 0x05580F41, 0x48D7AF5D, 0x504AB9E1, 0x83218CE4, 0xC20C333E, 0x95FB8D13, 0x931E38EC, 0x5FE5C35F, 0x68E1B932, 0x959146E9, 0xA3D16334, 0xABABB503, 0x0B915686, 0x2E5FBC2D, 0x1CAA6FA8, 0x80FB8EE7, 0xF8B43F1A, 0xD84483B1, 0x60AB295C, 0x6196B50B, 0xEB4B86EE, 0x7445A175, 0xA6B9C450, 0xE3E5CB6F, 0x3BFF8202, 0xC8675979, 0x5425B484, 0x2929B613, 0xB9F3C456, 0xFA702FBD, 0x509BADF8, 0x58DF98F7, 0xC6DB21EA, 0xF4ECE841, 0xBF59A4AC, 0xC064D81B, 0x4A4BAEBE, 0x3A748705, 0xD1A2CCA0, 0xBADB177F, 0x5052BED2, 0xEB2C5009, 0xA67399D4, 0xC54C3B23, 0x6748E626, 0x008BC74D, 0x6B75C048, 0x630E6707, 0x10E5F8BA, 0x0360B0D1, 0x333433FC, 0xB667FF2B, 0xDA950A8E, 0xC0131095, 0x348F28F0, 0xF173A78F, 0xF1086FA2, 0xBD292A99, 0x67701324, 0x03444433, 0x430DBBF6, 0x780B82DD, 0xB2BDA698, 0x2548F917, 0x87A1C38A, 0xAC08DD61, 0x1F8FD74C, 0x2CF12A3B, 0xBB449A5E, 0x279A3E25, 0xC5A3D940, 0xC5907B9F, 0xE88D9472, 0xF3E6E929, 0x65102074, 0xF682D143, 0x4FFF45C6, 0xD288626D, 0xE13860E8, 0x99904F27, 0x431B825A, 0x558E6DF1, 0x11018E9C, 0xA591594B, 0x46B75E2E, 0x15C30FB5, 0x9845DD90, 0x715293AF, 0x508F2D42, 0xCB2E8BB9, 0xBE88C1C4, 0x52B8E253, 0x341A8396, 0x4BDB65FD, 0x76EAEF38, 0x3E256937, 0x92A0352A, 0xB4DA6281, 0x0D5E59EC, 0x4A198C5B, 0xE28A55FE, 0x41868545, 0x2D1A35E0, 0x3B1AEFBF, 0x61FA3A12, 0x95091249, 0x344EF714, 0x7BD77763, 0x089C7566, 0x7A1D8D8D, 0x091A5188, 0x25894747, 0xCCBCDBFA, 0x5D15BB11, 0xE3BB393C, 0xFC9AC36B, 0xCF9A81CE, 0x041D9ED5, 0xC205E230, 0x858A8FCF, 0x44FBBAE2, 0x49BF7CD9, 0x7817C064, 0x960F9073, 0x2A021B36, 0xDDA7D91D, 0x924B87D8, 0x067CE957, 0x1F3E76CA, 0x4FA977A1, 0x7C6D2C8C, 0x6765FE7B, 0xFA04E19E, 0xE9015C65, 0xA22DE280, 0xDF8273DF, 0xE100AFB2, 0x17DACB69, 0x7CD81DB4, 0x95D22D83, 0x08087506, 0x711348AD, 0xC2439228, 0x4C014F67, 0x5F32059A, 0x8C3E9831, 0x290933DC, 0x0D0C3D8B, 0xC926756E, 0x3DEABDF5, 0x75F736D0, 0x14239BEF, 0xACB61882, 0xF423FDF9]),
            (8, 3, 0, 0, 0, 40, 4, vec![0xB69767DE, 0x9D0CBCCF, 0x585C3A8A, 0xD7CDB8AA, 0x4AE3E315, 0x21599B2C, 0x0CEA08F7, 0xFBE9348D, 0x1ECC4211, 0xE5AE481A, 0xAE481A00, 0x5A74455F, 0xEE415332, 0x7CC818E9, 0xBB460D34, 0xF2545703, 0x86DDEAAE, 0x2D0524B9, 0xA842AD50, 0xE7940719, 0x1A085595, 0xB160B513, 0x5C2F5397, 0x0BB1EF00, 0xEE972ED3, 0x75387D4E, 0x387D4E50, 0xADCFCD6F, 0xEBAD9C02, 0x9C9AAB79, 0x8F6CDE84, 0xA082D813, 0x563C7FA1, 0xBDF4D077, 0xF87209DA, 0xF7564C7B, 0xEA318A7A, 0x41DD860E, 0xAC54E23A, 0x1B5BB5A8, 0xBEC07239, 0x051670D6, 0x1670D6A0, 0x4B4D997F, 0xF7EF58D2, 0xDD7C2209, 0x442D43D4, 0x5B75DD23, 0x26883FB9, 0x4DA4050A, 0x4893D129, 0x071615D2, 0xBAA1AAC2, 0xD182231D, 0xFCC3C5E1, 0x2B026584, 0x8EC04542, 0x951907B2, 0x1907B2F0, 0x0C8EA98F, 0x463389A2, 0xC6B57C99, 0xCE3C3D24, 0xAE5E6633, 0xAEED75F6, 0x4083F4DD, 0x00477098, 0xB5C43B17, 0x6C301D8A, 0xC91F6F61, 0xD9E1414C, 0x6D4F8C3B, 0x2D1B945E, 0x0120F025, 0xDE66E340, 0xB773FD9F, 0x88E72E72, 0xD5CFBB29, 0x238ECA74, 0xB4AD7343, 0xC6B57FC6, 0x1EE5546D, 0x6C5CAAE8, 0x59E41127, 0x58A85C5A, 0xD6917FF1, 0x9955789C, 0xDB103B4B, 0x9E34D82E, 0x6FBE41B5, 0x21F36790, 0x0E1E95AF, 0xC1B74742, 0x0E93DDB9, 0x8B59EBC4, 0x2A140453, 0xC5473D96, 0xA73CD7FD, 0x1049B938, 0x8671AB37, 0x12CB8F2A, 0x46E9F481, 0xEE54C3EC, 0x3ED8EE5B, 0x234E4FFE, 0x0B163745, 0x0C523FE0, 0xDEEF71BF, 0x1190D412, 0x8B0AE449, 0xEE12A114, 0x7A831963, 0x2BDFAF66, 0xB7A37F8D, 0x29539B88, 0xD5EE0947, 0xD926B5FA, 0x7551CD11, 0xCFF4233C, 0x82BAA56B, 0x6544FBCE, 0x7461D0D5, 0x83686C30, 0x148791CF, 0x88A0D4E2, 0x0B7DCED9, 0x256DEA64, 0xD22BB273]),
            (8, 0, 0, 0, 2, 40, 4, vec![0xC32F6AB5, 0x76087C90, 0xC68EA6AF, 0x6F2E2442, 0xCEAB16B9, 0x1E3DD0C4, 0xC9E4A553, 0xBC056A96, 0x59D320FD, 0x24016E38, 0x65E3DC37, 0xB41E0C2A, 0x8AE84D81, 0x6AB548EC, 0x01BDAF5B, 0x06D21CFE, 0x2175A045, 0x740094E0, 0xE5A7C2BF, 0xC232F112, 0xEFD45D49, 0x2383C614, 0xE2FFFA63, 0x34DD1C66, 0xCAF0088D, 0x03CC9088, 0x0CB07A47, 0x230C72FA, 0x344A6611, 0x8089E83C, 0x63D3A66B, 0xE2F008CE, 0xC32F6AB5, 0x76087C90, 0xC68EA6AF, 0x6F2E2442, 0xCEAB16B9, 0x1E3DD0C4, 0xC9E4A553, 0xBC056A96, 0x59D320FD, 0x24016E38, 0xAA3CDC57, 0xA5CFCDCA, 0x8CB4E2A1, 0x95A39B8C, 0x3663A17B, 0x76D8289E, 0x2175A045, 0x740094E0, 0xE5A7C2BF, 0xC232F112, 0xEFD45D49, 0x2383C614, 0xE2FFFA63, 0x34DD1C66, 0xCAF0088D, 0x03CC9088, 0xB58A0267, 0xAD751C9A, 0x7FD0C331, 0xF79762DC, 0xC7FEA08B, 0x85E77C6E, 0x423F79D5, 0xCA000130, 0x9C9822CF, 0x343E31E2, 0x8A8987D9, 0x55BC4F64, 0x5F64D373, 0xE7088236, 0x1305141D, 0x308986D8, 0x69D8EC77, 0x22495F6A, 0x878707C1, 0xAD3A3E2C, 0x4F75A39B, 0x1EBB043E, 0x5D05F765, 0x7F2BC180, 0x0640C6DF, 0xC8BCE6B2, 0x39539669, 0x23DC6CB4, 0x7F843083, 0x1E449C06, 0x58AB43AD, 0x95FD5128, 0xD6AA9A87, 0x33D9963A, 0x3C00B051, 0xC6A12D7C, 0xFBD9AAAB, 0x6B2FC00E, 0x75BF4815, 0x77EAFA70, 0x68164B0F, 0xBBEFBD22, 0xC5491A19, 0xEDFC3CA4, 0xBE6B5FB3, 0x087F2176, 0x96BEAA5D, 0x7F13A818, 0xEFC00C97, 0x28F2C10A, 0xE3A6BCE1, 0xAD2130CC, 0x047BB5BB, 0x8062AFDE, 0x403255A5, 0xEA3A0AC0, 0x4E69FF1F, 0x5D7E41F2, 0xBAC0B8A9, 0x8A4CA9F4, 0x66DECCC3, 0x3A600B46, 0x91A369ED, 0x1614C268, 0x9D1A42A7, 0x2FA1DFDA, 0x03222D71, 0x734F481C, 0xB8ECC4CB, 0x2EB0D3AE]),
            (8, 0, 2, 0, 0, 40, 4, vec![0x423F79D5, 0xCA000130, 0x9C9822CF, 0x343E31E2, 0x8A8987D9, 0x55BC4F64, 0x5F64D373, 0xE7088236, 0x1305141D, 0x308986D8, 0x65E3DC37, 0xB41E0C2A, 0x8AE84D81, 0x6AB548EC, 0x01BDAF5B, 0x06D21CFE, 0x5D05F765, 0x7F2BC180, 0x0640C6DF, 0xC8BCE6B2, 0x39539669, 0x23DC6CB4, 0x7F843083, 0x1E449C06, 0x58AB43AD, 0x95FD5128, 0x0CB07A47, 0x230C72FA, 0x344A6611, 0x8089E83C, 0x63D3A66B, 0xE2F008CE, 0x6B8218F5, 0xF7E8D5D0, 0x79C2AEEF, 0x125C0F82, 0x5CFB88F9, 0x8E191E04, 0x640F1193, 0x098E69D6, 0xCCBB973D, 0x252CEF78, 0xAA3CDC57, 0xA5CFCDCA, 0x8CB4E2A1, 0x95A39B8C, 0x3663A17B, 0x76D8289E, 0xB9ACDE85, 0x45DC3E20, 0x9A7EDAFF, 0x0308AC52, 0xAC8A5F89, 0x75E76354, 0x1DF676A3, 0x8B22EBA6, 0x3A4F0ECD, 0x245D61C8, 0xB58A0267, 0xAD751C9A, 0x7FD0C331, 0xF79762DC, 0xC7FEA08B, 0x85E77C6E, 0x6B8218F5, 0xF7E8D5D0, 0x79C2AEEF, 0x125C0F82, 0x5CFB88F9, 0x8E191E04, 0x640F1193, 0x098E69D6, 0xCCBB973D, 0x252CEF78, 0x69D8EC77, 0x22495F6A, 0x878707C1, 0xAD3A3E2C, 0x4F75A39B, 0x1EBB043E, 0xB9ACDE85, 0x45DC3E20, 0x9A7EDAFF, 0x0308AC52, 0xAC8A5F89, 0x75E76354, 0x1DF676A3, 0x8B22EBA6, 0x3A4F0ECD, 0x245D61C8, 0xD6AA9A87, 0x33D9963A, 0x3C00B051, 0xC6A12D7C, 0xFBD9AAAB, 0x6B2FC00E, 0x75BF4815, 0x77EAFA70, 0x68164B0F, 0xBBEFBD22, 0xC5491A19, 0xEDFC3CA4, 0xBE6B5FB3, 0x087F2176, 0x96BEAA5D, 0x7F13A818, 0xEFC00C97, 0x28F2C10A, 0xE3A6BCE1, 0xAD2130CC, 0x047BB5BB, 0x8062AFDE, 0x403255A5, 0xEA3A0AC0, 0x4E69FF1F, 0x5D7E41F2, 0xBAC0B8A9, 0x8A4CA9F4, 0x66DECCC3, 0x3A600B46, 0x91A369ED, 0x1614C268, 0x9D1A42A7, 0x2FA1DFDA, 0x03222D71, 0x734F481C, 0xB8ECC4CB, 0x2EB0D3AE]),
            (8, 1, 1, 4, 3, 40, 4, vec![0x21B83AF0, 0x1BB5918F, 0x339D51A2, 0x82C4A499, 0x4CF14524, 0x06BFCE33, 0x234FBDF6, 0x5A319CDD, 0xE708F898, 0xD1C82317, 0x0ED2E58A, 0x6F139761, 0x7737494C, 0x79DDF43B, 0xB5C6DC5E, 0x38839825, 0x65596B40, 0x7BF4E59F, 0x87E2F672, 0xA248E329, 0xB3A5D274, 0xFB08DB43, 0xECC9C7C6, 0x2A9CFC6D, 0x99A032E8, 0xD681F927, 0x3F1D245A, 0x9A2FA7F1, 0x744D809C, 0x94D8A34B, 0x0ED2202E, 0xDA6AE9B5, 0xBFA7EF90, 0x46797DAF, 0x7EC50F42, 0xADF705B9, 0x0D52F3C4, 0xC4E96C53, 0xD18D8596, 0x4F7E7FFD, 0x2C8F4138, 0xF2299337, 0x7992572A, 0x3AB21C81, 0xF76ECBEC, 0xDC5B565B, 0x405D97FE, 0xA38CDF45, 0x7948C7E0, 0x596B407B, 0xF4E59F87, 0xE2F672A2, 0x48E329B3, 0xA5D274FB, 0x08DB43EC, 0xC9C7C62A, 0x9CFC6D99, 0xA032E8D6, 0x81F9273F, 0x4BC3F511, 0x17B02B3C, 0xDA770D6B, 0x134643CE, 0xD52278D5, 0x9820F430, 0xA7EF9046, 0x797DAF7E, 0xC50F42AD, 0xF705B90D, 0x52F3C4C4, 0xE96C53D1, 0x8D85964F, 0x7E7FFD2C, 0x8F4138F2, 0x29933779, 0x36CE31A1, 0x05669E8C, 0xE17CC87B, 0xFBA9239E, 0xE2A4B665, 0x2F557480, 0x48C7E0E9, 0xA459BF59, 0x309C125F, 0xD80C4962, 0x6DA91470, 0x52816372, 0xD7F76646, 0xEF278DFD, 0x1B23885F, 0x3FF1471C, 0x6379D231, 0x5A2725DC, 0x1BFD878B, 0xE8E3376E, 0x01CC97F5, 0xAF4B48D0, 0x20F430F0, 0x1679CF47, 0x529CE218, 0x34F6D9AC, 0xAAF264C9, 0x751A73B5, 0x261D3679, 0x47F31DCD, 0xC8D9D8FC, 0x8613575E, 0xC7AFD6C1, 0x68C6C12C, 0x9CCA4A9B, 0x15917F3E, 0xBA931D85, 0x35A77120, 0xB92271FF, 0x7E815752, 0xB8CD0E89, 0xBE044654, 0x74DFFDA3, 0x2FC8C6A6, 0x0EFD2DCD, 0x588EF4C8, 0x98C11187, 0xA167A13A, 0x77993F51, 0x8D5A707C, 0x6EF411AB, 0xD790FB0E, 0x77314715]),
            (4, 2, 0, 7, 0, 21, 3, vec![0x4C4884DC, 0x884DD4EB, 0x2C0984C4, 0x74B1ED8A, 0xE164FF61, 0xDCC9914C, 0xF2FB9C3B, 0x1538645E, 0x9577802F, 0x78025153, 0xC3340957, 0x90D9FE72, 0x06C74B29, 0x53811A74, 0x7FDB8343, 0x026C4FC6, 0x4E0DE46D, 0xDE46D026, 0xBFAE84E0, 0xB2122C5A, 0x367B0FF1, 0x9791C89C, 0x9C004B4B, 0xA0C5A82E, 0x93F8D1B5, 0x5E4CB790, 0x8487A5AF, 0x316D1742, 0x7BAF6DB9, 0x21203BC4, 0x99061453, 0x69F20D96, 0x62C967FD, 0xA7CD0938, 0x59FCBB37, 0xC93B5F2A, 0x67778481, 0x0CE513EC, 0x1C0CFE5B, 0xAD531FFE, 0x4834C745, 0x643F8FE0, 0xB3DC81BF, 0xE5FAA412, 0xCD4A7449, 0xAEACF114, 0x8E392963, 0x467E7F66, 0xA8940F8D, 0x4EEAEB88, 0xD87D1947, 0x8ACA85FA, 0x0F835D11, 0x73D8733C, 0x9D32B56B, 0x23BDCBCE, 0x936460D5, 0x2BE9BC30, 0xF8F8A1CF, 0xBEBEA4E2, 0xFBE15ED9, 0x15DC3A64, 0xCBA5C273, 0x9C8EA536]),
        ];
        // 16 guard words on each side, filled from the same LCG stream as
        // the C driver: a reversed blit's preload can read the word before
        // the first row, and the value read must match the C run's.
        const GUARD: usize = 16;
        for (depth, sx, sy, dx, dy, w, h, expected) in cases {
            let wpr = ((64 * depth) / 32) as usize;
            let mut arena = vec![0u32; GUARD + wpr * 8 + GUARD];
            // Same LCG and seed as the C driver.
            let mut seed: u32 = 0xC0FFEE
                ^ (depth as u32 * 977 + sx as u32 * 31 + dx as u32 * 7 + w as u32);
            for word in arena.iter_mut() {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *word = seed;
            }
            let mut bb = BitBlt::new();
            bb.combinationRule = 3;
            bb.noSource = false;
            bb.noHalftone = true;
            bb.sourceForm = DEST_FORM_OOP;
            bb.destForm = DEST_FORM_OOP;
            bb.destDepth = depth;
            bb.sourceDepth = depth;
            bb.destMSB = 1;
            bb.sourceMSB = 1;
            bb.destPPW = (32 / depth) as sqInt;
            bb.sourcePPW = bb.destPPW;
            bb.destPitch = (wpr * 4) as i32;
            bb.sourcePitch = (wpr * 4) as i32;
            bb.destBits = arena[GUARD..].as_mut_ptr() as usize;
            bb.sourceBits = bb.destBits;
            bb.endOfDestination = bb.destBits + wpr * 8 * 4;
            bb.endOfSource = bb.endOfDestination;
            bb.sx = sx;
            bb.sy = sy;
            bb.dx = dx;
            bb.dy = dy;
            bb.bbW = w;
            bb.bbH = h;
            // SAFETY: harness-owned buffer (with guards), geometry as set up.
            unsafe { bb.performCopyLoop() };
            assert_eq!(
                &arena[GUARD..GUARD + wpr * 8],
                &expected[..],
                "golden mismatch: depth {depth} s({sx},{sy}) d({dx},{dy}) w{w} h{h}"
            );
        }
    }

    /// Fill through a halftone: the pattern word for row y is
    /// `halftone[(dy + row) % height]`, and pixels take their bits from
    /// their own position in that word.
    #[test]
    fn fill_uses_the_halftone_pattern_per_row() {
        let halftone: Vec<u32> = vec![0xAAAA_AAAA, 0x5555_5555];
        let mut dst = Form::new(64, 6, 1, true);
        dst.fill_random(77);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, None, 3);
        bb.noHalftone = false;
        bb.halftoneBase = halftone.as_ptr() as usize;
        bb.halftoneHeight = 2;
        bb.destX = 3;
        bb.destY = 1;
        bb.width = 50;
        bb.height = 4;
        run(&mut bb);
        // Reference: independent halftone form addressing.
        let ht = |x: i32, y: i32| -> u32 {
            let word = halftone[((1 + y) % 2) as usize];
            (word >> (31 - (x % 32))) & 1
        };
        for y in 0..dst.height {
            for x in 0..dst.padded_width() {
                let inside = (3..53).contains(&x) && (1..5).contains(&y);
                let want = if inside { ht(x, y - 1) } else { before.pixel(x, y) };
                assert_eq!(dst.pixel(x, y), want, "pixel ({x},{y})");
            }
        }
    }

    /// copyLoop with halftone AND rule: source AND pattern AND merge.
    #[test]
    fn copyLoop_applies_the_halftone_before_merging() {
        let halftone: Vec<u32> = vec![0xF0F0_F0F0];
        let mut src = Form::new(32, 2, 1, true);
        let mut dst = Form::new(32, 2, 1, true);
        src.bits = vec![0xFFFF_FFFF, 0xFFFF_FFFF];
        dst.bits = vec![0, 0];
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 7); // OR
        bb.noHalftone = false;
        bb.halftoneBase = halftone.as_ptr() as usize;
        bb.halftoneHeight = 1;
        bb.width = 32;
        bb.height = 2;
        run(&mut bb);
        // (source & halftone) | dest = halftone pattern.
        assert_eq!(dst.bits, vec![0xF0F0_F0F0, 0xF0F0_F0F0]);
    }

    /// Rule 32 through the whole dispatch: bitCount counts only pixels
    /// inside the (unaligned) destination rectangle.
    #[test]
    fn rgbDiff_counts_only_pixels_inside_the_rect() {
        let mut src = Form::new(16, 2, 8, true);
        let mut dst = Form::new(16, 2, 8, true);
        for x in 0..16 {
            for y in 0..2 {
                src.set_pixel(x, y, (x + 1) as u32);
                dst.set_pixel(x, y, if x % 3 == 0 { (x + 1) as u32 } else { 0 });
            }
        }
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 32);
        bb.destX = 1;
        bb.destY = 0;
        bb.sourceX = 1;
        bb.sourceY = 0;
        bb.width = 13;
        bb.height = 2;
        run(&mut bb);
        // Expected: differing pixels at x in 1..14 (those with x % 3 != 0),
        // rows 0 and 1.
        let per_row = (1..14).filter(|x| x % 3 != 0).count() as sqInt;
        assert_eq!(bb.bitCount, 2 * per_row);
        // The destination itself is untouched by the diff rule.
        assert_eq!(dst.pixel(2, 0), 0);
    }
}

// ---------------------------------------------------------------------------
// copyLoopPixMap: depth conversion, color maps, endianness
// ---------------------------------------------------------------------------

mod pix_map {
    use super::*;
    use crate::state::{COLOR_MAP_INDEXED_PART, COLOR_MAP_PRESENT};

    /// 8bpp -> 32bpp through an indexed color map.
    #[test]
    fn indexed_map_8_to_32() {
        let lookup: Vec<u32> = (0..256).map(|i| 0xFF00_0000 | (i * 0x010307) as u32).collect();
        let mut src = Form::new(11, 3, 8, true);
        src.fill_random(9);
        let mut dst = Form::new(13, 4, 32, true);
        dst.fill_random(10);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
        bb.cmMask = 255;
        bb.cmLookupTable = lookup.as_ptr() as usize;
        bb.destX = 1;
        bb.destY = 1;
        bb.sourceX = 2;
        bb.sourceY = 0;
        bb.width = 9;
        bb.height = 3;
        run(&mut bb);
        let expected = ref_blit_per_pixel(
            &before,
            &src,
            &|s, _| lookup[s as usize],
            2,
            0,
            1,
            1,
            9,
            3,
        );
        assert_forms_equal(&dst, &expected, "8->32 indexed");
    }

    /// 4bpp -> 8bpp through a 16-entry map, MSB.
    #[test]
    fn indexed_map_4_to_8() {
        let lookup: Vec<u32> = (0..16).map(|i| (0xF0 - i) as u32).collect();
        let mut src = Form::new(21, 3, 4, true);
        src.fill_random(21);
        let mut dst = Form::new(23, 3, 8, true);
        dst.fill_random(22);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
        bb.cmMask = 15;
        bb.cmLookupTable = lookup.as_ptr() as usize;
        bb.destX = 3;
        bb.sourceX = 1;
        bb.width = 17;
        bb.height = 3;
        run(&mut bb);
        let expected =
            ref_blit_per_pixel(&before, &src, &|s, _| lookup[s as usize], 1, 0, 3, 0, 17, 3);
        assert_forms_equal(&dst, &expected, "4->8 indexed");
    }

    /// 16bpp -> 32bpp through the fixed shift/mask part (the implicit RGB
    /// conversion the loader installs for old-style maps): each 5-bit
    /// channel lands in the top of its byte; a nonzero pixel never maps to 0.
    #[test]
    fn fixed_map_16_to_32() {
        let mut src = Form::new(9, 2, 16, true);
        src.fill_random(5);
        let mut dst = Form::new(9, 2, 32, true);
        dst.fill_random(6);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.setupColorMasksFromto(5, 8);
        bb.width = 9;
        bb.height = 2;
        run(&mut bb);
        let map = |p: u32| {
            let v = ((p & 0x7C00) << 9) | ((p & 0x3E0) << 6) | ((p & 0x1F) << 3);
            if v == 0 && p != 0 {
                1
            } else {
                v
            }
        };
        let expected = ref_blit_per_pixel(&before, &src, &|s, _| map(s), 0, 0, 0, 0, 9, 2);
        assert_forms_equal(&dst, &expected, "16->32 fixed");
    }

    /// 32bpp -> 16bpp truncation through the fixed part.
    #[test]
    fn fixed_map_32_to_16() {
        let mut src = Form::new(7, 2, 32, true);
        src.fill_random(41);
        let mut dst = Form::new(7, 2, 16, true);
        dst.fill_random(42);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.setupColorMasksFromto(8, 5);
        bb.width = 7;
        bb.height = 2;
        run(&mut bb);
        let map = |p: u32| {
            let v = (((p >> 19) & 31) << 10) | (((p >> 11) & 31) << 5) | ((p >> 3) & 31);
            if v == 0 && p != 0 {
                1
            } else {
                v
            }
        };
        let expected = ref_blit_per_pixel(&before, &src, &|s, _| map(s), 0, 0, 0, 0, 7, 2);
        assert_forms_equal(&dst, &expected, "32->16 fixed");
    }

    /// Same depth but different bit order forces the pixmap loop; pixels
    /// must survive the endianness swap intact.
    #[test]
    fn msb_to_lsb_same_depth() {
        for &depth in &[1, 4, 8, 16] {
            let mut src = Form::new(37, 3, depth, true);
            src.fill_random(depth as u32 * 3 + 1);
            let mut dst = Form::new(41, 3, depth, false);
            dst.fill_random(depth as u32 * 5 + 7);
            let before = dst.clone();
            let mut bb = BitBlt::new();
            setup(&mut bb, &mut dst, Some(&src), 3);
            bb.destX = 2;
            bb.sourceX = 3;
            bb.width = 31;
            bb.height = 3;
            run(&mut bb);
            let expected = ref_blit_per_pixel(&before, &src, &|s, _| s, 3, 0, 2, 0, 31, 3);
            assert_forms_equal(&dst, &expected, &format!("msb->lsb depth {depth}"));
        }
    }

    /// Depth conversion with a merge rule other than store: 8 -> 32 with OR.
    #[test]
    fn pixmap_merges_with_the_destination() {
        let lookup: Vec<u32> = (0..256).map(|i| i as u32 * 0x0101).collect();
        let mut src = Form::new(6, 2, 8, true);
        src.fill_random(11);
        let mut dst = Form::new(6, 2, 32, true);
        dst.fill_random(12);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 7); // bitOr
        bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
        bb.cmMask = 255;
        bb.cmLookupTable = lookup.as_ptr() as usize;
        bb.width = 6;
        bb.height = 2;
        run(&mut bb);
        let expected =
            ref_blit_per_pixel(&before, &src, &|s, d| lookup[s as usize] | d, 0, 0, 0, 0, 6, 2);
        assert_forms_equal(&dst, &expected, "8->32 or-merge");
    }
}

// ---------------------------------------------------------------------------
// Rule 34 / rule 41 fast paths
// ---------------------------------------------------------------------------

mod alpha_paths {
    use super::*;

    /// Reference for alphaBlendScaled (see merge_rules).
    fn blend_scaled(s: u32, d: u32) -> u32 {
        let un = 255 - (s >> 24);
        let ch = |o: u32| ((((d >> o) & 0xFF) * un / 256) + ((s >> o) & 0xFF)).min(255) << o;
        ch(0) | ch(8) | ch(16) | ch(24)
    }

    #[test]
    fn rule34_32bpp_blends_by_source_alpha() {
        let mut src = Form::new(4, 1, 32, true);
        let mut dst = Form::new(4, 1, 32, true);
        src.bits = vec![0xFF11_2233, 0x0044_5566, 0x8060_5040, 0x0000_0000];
        dst.bits = vec![0x0101_0101, 0x2222_2222, 0xFFFF_FFFF, 0x4040_4040];
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 34);
        bb.width = 4;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(dst.bits[0], 0xFF11_2233); // alpha FF: copied
        assert_eq!(dst.bits[1], before.bits[1]); // alpha 00: untouched
        assert_eq!(dst.bits[2], blend_scaled(src.bits[2], before.bits[2]));
        assert_eq!(dst.bits[3], before.bits[3]); // alpha 00: untouched
    }

    /// Rule 34 into 8bpp requires a color map; the run consults the default
    /// 8->32 table for the destination and maps the result back. We assert
    /// the two extreme alphas: transparent leaves the pixel, opaque maps the
    /// source through the color map.
    #[test]
    fn rule34_8bpp_uses_the_color_map() {
        use crate::state::{COLOR_MAP_INDEXED_PART, COLOR_MAP_PRESENT};
        // Map every 12-bit RGB to a recognizable index.
        let lookup: Vec<u32> = (0..4096).map(|i| (i & 0xFF) as u32).collect();
        let mut src = Form::new(4, 1, 32, true);
        let mut dst = Form::new(4, 1, 8, true);
        src.bits = vec![0xFF10_2030, 0x0011_2233, 0xFFFF_FFFF, 0x00FF_FFFF];
        dst.bits = vec![0x0501_0203];
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 34);
        bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
        bb.cmMask = 4095;
        bb.cmBitsPerColor = 4;
        bb.cmLookupTable = lookup.as_ptr() as usize;
        // Fixed part 8 -> 4 bits per color, as the loader would set up.
        bb.setupColorMasksFromto(8, 4);
        bb.width = 4;
        bb.height = 1;
        run(&mut bb);
        // Pixel 0: opaque 0x102030 -> 12-bit 0x123 -> lookup -> 0x23.
        assert_eq!(dst.pixel(0, 0), 0x23);
        // Pixel 1: alpha 0 (below the 0x1F threshold): untouched.
        assert_eq!(dst.pixel(1, 0), 0x01);
        // Pixel 2: opaque white -> 0xFFF -> 0xFF.
        assert_eq!(dst.pixel(2, 0), 0xFF);
        // Pixel 3: alpha 0: untouched.
        assert_eq!(dst.pixel(3, 0), 0x03);
    }

    #[test]
    fn rule41_32bpp_component_alpha() {
        let mut src = Form::new(3, 1, 32, true);
        let mut dst = Form::new(3, 1, 32, true);
        src.bits = vec![0x0000_0000, 0x00FF_FFFF, 0x0000_00FF];
        dst.bits = vec![0x1234_5678, 0xFF00_0000, 0xFF00_0000];
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 41);
        bb.componentAlphaModeAlpha = 0xFF;
        bb.componentAlphaModeColor = 0xFFFFFF;
        bb.width = 3;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(dst.bits[0], 0x1234_5678); // zero mask: skipped
        assert_eq!(dst.bits[1], 0xFEFE_FEFE); // see merge_rules
        assert_eq!(dst.bits[2], 0xFE00_00FE); // blue-only mask
    }

    /// The quick paths only run for 32bpp sources on a *different* form;
    /// same-form rule 34 falls back to the general loop (alphaBlendScaled
    /// word-wise).
    #[test]
    fn rule34_same_form_uses_the_general_loop() {
        let mut form = Form::new(2, 1, 32, true);
        form.bits = vec![0x8040_2010, 0x0011_2233];
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut form, None, 34);
        bb.noSource = false;
        bb.sourceForm = DEST_FORM_OOP; // same form
        bb.sourceBits = bb.destBits;
        bb.sourceWidth = bb.destWidth;
        bb.sourceHeight = bb.destHeight;
        bb.sourceDepth = 32;
        bb.sourceMSB = 1;
        bb.sourcePPW = 1;
        bb.sourcePitch = bb.destPitch;
        bb.endOfSource = bb.endOfDestination;
        bb.destX = 1;
        bb.sourceX = 0;
        bb.width = 1;
        bb.height = 1;
        run(&mut bb);
        assert_eq!(form.bits[1], blend_scaled(0x8040_2010, 0x0011_2233));
    }
}

// ---------------------------------------------------------------------------
// WarpBlt
// ---------------------------------------------------------------------------

mod warping {
    use super::*;

    /// Compute the warp parameters the interpreter half would derive from
    /// the quad `[p1, p2, p3, p4]` (fixed-point 14 coordinates), mirroring
    /// `warpLoop`'s fetch sequence.
    #[allow(clippy::type_complexity)]
    fn warp_params(
        quad: [(sqInt, sqInt); 4],
        height: sqInt,
    ) -> (sqInt, sqInt, sqInt, sqInt, sqInt, sqInt, sqInt, sqInt) {
        let mut nSteps = height - 1;
        if nSteps <= 0 {
            nSteps = 1;
        }
        let (p1, p2, p3, p4) = (quad[0], quad[1], quad[2], quad[3]);
        let mut pAx = p1.0;
        let deltaP12x = deltaFromtonSteps(pAx, p2.0, nSteps);
        if deltaP12x < 0 {
            pAx = p2.0 - nSteps * deltaP12x;
        }
        let mut pAy = p1.1;
        let deltaP12y = deltaFromtonSteps(pAy, p2.1, nSteps);
        if deltaP12y < 0 {
            pAy = p2.1 - nSteps * deltaP12y;
        }
        let mut pBx = p4.0;
        let deltaP43x = deltaFromtonSteps(pBx, p3.0, nSteps);
        if deltaP43x < 0 {
            pBx = p3.0 - nSteps * deltaP43x;
        }
        let mut pBy = p4.1;
        let deltaP43y = deltaFromtonSteps(pBy, p3.1, nSteps);
        if deltaP43y < 0 {
            pBy = p3.1 - nSteps * deltaP43y;
        }
        (pAx, pAy, pBx, pBy, deltaP12x, deltaP12y, deltaP43x, deltaP43y)
    }

    /// warpBits minus surface locking.
    fn run_warp(bb: &mut BitBlt, quad: [(sqInt, sqInt); 4], smoothing: sqInt, source_map: usize) {
        let ns = bb.noSource;
        bb.noSource = true;
        bb.clipRange();
        bb.noSource = ns;
        assert!(!bb.noSource && bb.bbW > 0 && bb.bbH > 0, "warp region empty");
        bb.destMaskAndPointerInit();
        let (pAx, pAy, pBx, pBy, d12x, d12y, d43x, d43y) = warp_params(quad, bb.height);
        // SAFETY: harness-owned buffers, geometry as set up.
        unsafe {
            bb.warpLoopBody(pAx, pAy, pBx, pBy, d12x, d12y, d43x, d43y, smoothing, source_map)
        };
    }

    const FP: sqInt = 1 << 14;

    #[test]
    fn identity_warp_reproduces_the_source() {
        let mut src = Form::new(4, 4, 32, true);
        for y in 0..4 {
            for x in 0..4 {
                src.set_pixel(x, y, 0xFF00_0000 | (x as u32) << 8 | y as u32);
            }
        }
        let mut dst = Form::new(4, 4, 32, true);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.width = 4;
        bb.height = 4;
        run_warp(
            &mut bb,
            [(0, 0), (0, 3 * FP), (3 * FP, 3 * FP), (3 * FP, 0)],
            1,
            0,
        );
        assert_forms_equal(&dst, &src, "identity warp");
    }

    #[test]
    fn two_x_scale_duplicates_pixels() {
        let mut src = Form::new(2, 2, 32, true);
        src.set_pixel(0, 0, 0xFF00_0001);
        src.set_pixel(1, 0, 0xFF00_0002);
        src.set_pixel(0, 1, 0xFF00_0003);
        src.set_pixel(1, 1, 0xFF00_0004);
        let mut dst = Form::new(4, 4, 32, true);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.width = 4;
        bb.height = 4;
        run_warp(&mut bb, [(0, 0), (0, FP), (FP, FP), (FP, 0)], 1, 0);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    dst.pixel(x, y),
                    src.pixel(x / 2, y / 2),
                    "pixel ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn warp_off_source_pixels_read_as_zero() {
        // Warp a quad hanging off the right/bottom edge of a 2x2 source:
        // out-of-range picks answer 0.
        let mut src = Form::new(2, 2, 32, true);
        src.set_pixel(0, 0, 0xAAAA_AAAA);
        src.set_pixel(1, 0, 0xBBBB_BBBB);
        src.set_pixel(0, 1, 0xCCCC_CCCC);
        src.set_pixel(1, 1, 0xDDDD_DDDD);
        let mut dst = Form::new(4, 4, 32, true);
        dst.fill_random(3);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.width = 4;
        bb.height = 4;
        // Identity spacing but starting at (1,1): the lower-right 3x3 of the
        // dest samples outside the source.
        run_warp(
            &mut bb,
            [(FP, FP), (FP, 4 * FP), (4 * FP, 4 * FP), (4 * FP, FP)],
            1,
            0,
        );
        assert_eq!(dst.pixel(0, 0), 0xDDDD_DDDD);
        for y in 0..4 {
            for x in 0..4 {
                if x > 0 || y > 0 {
                    assert_eq!(dst.pixel(x, y), 0, "pixel ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn warp_with_indexed_color_map() {
        let lookup: Vec<u32> = (0..256).map(|i| 0xFF00_0000 | (i * 7) as u32).collect();
        let mut src = Form::new(4, 4, 8, true);
        src.fill_random(15);
        let mut dst = Form::new(4, 4, 32, true);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.cmFlags =
            crate::state::COLOR_MAP_PRESENT | crate::state::COLOR_MAP_INDEXED_PART;
        bb.cmMask = 255;
        bb.cmLookupTable = lookup.as_ptr() as usize;
        bb.width = 4;
        bb.height = 4;
        run_warp(
            &mut bb,
            [(0, 0), (0, 3 * FP), (3 * FP, 3 * FP), (3 * FP, 0)],
            1,
            0,
        );
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    dst.pixel(x, y),
                    lookup[src.pixel(x, y) as usize],
                    "pixel ({x},{y})"
                );
            }
        }
    }

    /// Smoothing 2 with a 2x downscale is an exact 2x2 box filter here: the
    /// sample offsets land exactly on the four source pixels.
    #[test]
    fn smoothing_two_averages_two_by_two_blocks() {
        let mut src = Form::new(4, 4, 32, true);
        for y in 0..4 {
            for x in 0..4 {
                // Channel values divisible by 4 so the average is exact.
                let v = ((x * 4 + y * 16) * 4) as u32;
                src.set_pixel(x, y, 0xFF00_0000 | v << 8 | v);
            }
        }
        let mut dst = Form::new(2, 2, 32, true);
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.width = 2;
        bb.height = 2;
        run_warp(
            &mut bb,
            [(0, 0), (0, 3 * FP), (3 * FP, 3 * FP), (3 * FP, 0)],
            2,
            0,
        );
        for y in 0..2 {
            for x in 0..2 {
                let avg = |o: u32| {
                    let mut sum = 0;
                    for j in 0..2 {
                        for i in 0..2 {
                            sum += (src.pixel(x * 2 + i, y * 2 + j) >> o) & 0xFF;
                        }
                    }
                    sum / 4
                };
                let want = (avg(24) << 24) | (avg(16) << 16) | (avg(8) << 8) | avg(0);
                assert_eq!(dst.pixel(x, y), want, "pixel ({x},{y})");
            }
        }
    }

    /// Pixel extraction at every depth, both bit orders, through the warp
    /// pickers' shift table.
    #[test]
    fn pickWarpPixel_extracts_at_all_depths() {
        for &(depth, msb) in &[
            (1, true),
            (2, true),
            (4, true),
            (8, true),
            (16, true),
            (32, true),
            (4, false),
            (8, false),
        ] {
            let mut src = Form::new(32 / depth, 1, depth, msb);
            src.bits = vec![0x1234_5678];
            let mut dst = Form::new(4, 4, depth, msb);
            let mut bb = BitBlt::new();
            setup(&mut bb, &mut dst, Some(&src), 3);
            bb.warpLoopSetup();
            for x in 0..(32 / depth) {
                // SAFETY: source buffer is live.
                let got = unsafe { bb.pickWarpPixelAtXy((x as sqInt) << 14, 0) };
                assert_eq!(got, src.pixel(x, 0), "depth {depth} msb {msb} x {x}");
            }
            // Out of range answers 0.
            assert_eq!(unsafe { bb.pickWarpPixelAtXy(-1, 0) }, 0);
            assert_eq!(unsafe { bb.pickWarpPixelAtXy(0, 99 << 14) }, 0);
        }
    }

    /// Warp into an unaligned destination strip at 8bpp: exercises mask1 /
    /// mask2 and the per-word pixel packing of warpLoopBody.
    #[test]
    fn warp_into_unaligned_8bpp_destination() {
        let mut src = Form::new(8, 2, 8, true);
        src.fill_random(88);
        let mut dst = Form::new(16, 3, 8, true);
        dst.fill_random(99);
        let before = dst.clone();
        let mut bb = BitBlt::new();
        setup(&mut bb, &mut dst, Some(&src), 3);
        bb.destX = 3;
        bb.destY = 1;
        bb.width = 8;
        bb.height = 2;
        run_warp(
            &mut bb,
            [(0, 0), (0, FP), (7 * FP, FP), (7 * FP, 0)],
            1,
            0,
        );
        for y in 0..dst.height {
            for x in 0..dst.padded_width() {
                let inside = (3..11).contains(&x) && (1..3).contains(&y);
                if inside {
                    assert_eq!(dst.pixel(x, y), src.pixel(x - 3, y - 1), "pixel ({x},{y})");
                } else {
                    assert_eq!(dst.pixel(x, y), before.pixel(x, y), "pixel ({x},{y})");
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Randomized differential sweep against the real C plugin
// ---------------------------------------------------------------------------

/// 400 generated blit scenarios -- all 35 loadable rules, every depth pair,
/// MSB and LSB, with and without an indexed color map, a halftone, and a
/// source -- were run through the *actual* `BitBltPlugin.c` (compiled
/// standalone, driven through its own `tryCopyingBitsQuickly` /
/// `performCopyLoop`), and an FNV-1a hash of each resulting destination
/// bitmap (plus `bitCount`) recorded. The port must reproduce every hash.
mod c_differential {
    use super::*;
    use crate::state::{COLOR_MAP_INDEXED_PART, COLOR_MAP_PRESENT};

    /// Hashes captured from the C run (see the crate README for the driver).
    const C_HASHES: [u32; 400] = [
        0xBE238495, 0x1372E985, 0xA938652A, 0x3B9F831F, 0x77F71B97, 0xCDBE2FA2, 0xD894C632, 0x5F0A8B5F,
        0x48339C69, 0xC717565F, 0x3EFACC90, 0x7988F99F, 0xE3BF49EE, 0x61FC581F, 0x09FA1E1F, 0x517082C7,
        0xD3DB3AB8, 0x1264D00F, 0x86F1C59F, 0xF7355EDF, 0x17E32C1F, 0x6380C6F7, 0x9D9E4C63, 0x8EE7FD4F,
        0xB8DB611F, 0xF18B950F, 0x5B22A9EF, 0xFE80BA9F, 0xB85DB916, 0xEC078319, 0x19AF61A0, 0x449B96FF,
        0x06C25B04, 0xF58535AF, 0x7ED0E2E8, 0xFFC8A657, 0xD2F4D21F, 0xD9338ADB, 0x8B21C083, 0x015DA81E,
        0xD5F335E7, 0x2527402F, 0x7EBF27CC, 0x8AD490F7, 0xDA7AD01F, 0x9762636F, 0x84E5C65F, 0x37F598BF,
        0x096387EC, 0xF52D8564, 0xCF9DA6D3, 0x02695E1F, 0xEABC3F40, 0x8B2EBDE7, 0xCD6ABDDE, 0x9B0C945F,
        0x989F3B8E, 0x08064079, 0x18C405EC, 0xB060F19F, 0xC8CD66B3, 0x9B9D101F, 0x72D2AF8B, 0xA03D46A3,
        0x9500A0F4, 0x7153DE00, 0x37B17F9F, 0x0E2F189F, 0xED99CFFB, 0x61FAD2D8, 0xE154DA69, 0xADBFD598,
        0xACDD3A60, 0x79131AAF, 0xEFFCC316, 0xEC66089F, 0x32D984EB, 0x5615206F, 0x062FAEB0, 0x20E7570F,
        0xE03502A9, 0x043B96DF, 0x06435C86, 0xA35344CF, 0xE18C9DDF, 0xE45D2D6F, 0xC13FE1A4, 0x2D455DD9,
        0x6CA40A78, 0xAB625DC7, 0x3CB360DF, 0xF598AA97, 0x1F78ACA7, 0x039B15EF, 0xDFC5C640, 0x521083DC,
        0x1B6A6B8F, 0x6DE89B1E, 0x976C6B9F, 0x7E42E75F, 0xAA17362A, 0x192E4C0A, 0x7D6B51CE, 0xE17708DE,
        0x76112BB3, 0x69E1F49D, 0x18E1897A, 0x206219BF, 0x89C4EE1F, 0x0281246C, 0x2EAC7E13, 0xEBD1C1A9,
        0xE68281D0, 0x03EBF08F, 0xDCB08D88, 0xDA09CF1F, 0x956593EB, 0x56F2FE1F, 0x206EAB1F, 0x406ECBA9,
        0xF547A11F, 0x93A3BCD3, 0x58FABC2F, 0xB4914B1F, 0xA6E02909, 0xDBE7259F, 0x00229C1B, 0xBF2C83B3,
        0x23377F67, 0x90A17FEF, 0x49B2BD1F, 0xAFEE9A9F, 0x2B35E245, 0xDE2F2597, 0x9BEA9CA8, 0x219690DF,
        0x2C67917F, 0x1AC09D2F, 0xC102B131, 0x0CC43B1F, 0xF458551F, 0x0D193233, 0x5EF45DF3, 0x15C42257,
        0xF99F3000, 0x84EEB2BD, 0x336B65F3, 0x62D498DF, 0x093C03D1, 0x8B9BA46B, 0xCFA55E1F, 0x40BA703F,
        0xF0D7A9DF, 0x5D02CE1F, 0xFCC5F44F, 0xCEA4A4FF, 0x23192834, 0x47C47053, 0x81BE3874, 0xCA92308F,
        0x710EB71F, 0x0B384F6F, 0x20EB0B6B, 0xEC6B9517, 0x77E04EB6, 0x5770A02F, 0xB0153B1F, 0x1B50F578,
        0x42C51198, 0x8DA9EA9F, 0x031F246A, 0xACE2331F, 0x7593C6DE, 0x720AE0AF, 0x0F35FFF0, 0xBFE5261F,
        0xFBAFB19F, 0x8F8EF10F, 0xC950E6A9, 0x5524535F, 0x38266D6F, 0x0888CC8F, 0x9DA4D21F, 0x78369F00,
        0x9A75991F, 0xA1969E8F, 0xF4E21A16, 0x44DF6F1F, 0x2384FEC2, 0x57CF6462, 0xA4AD02E3, 0x6BCDD47F,
        0x17C3BA06, 0xC36CBD56, 0x43BA882D, 0x05686A77, 0xBCAF3ABF, 0xC3665193, 0x894267B0, 0xD619FDDF,
        0xEB500452, 0x4E05B21F, 0x26F92DBA, 0x9EB4A597, 0x53FF7D1F, 0xE5BB861C, 0xE2F8374B, 0x0BD022B5,
        0xBF60B73F, 0x2A284629, 0xB4177EDD, 0x78090DEB, 0xF1EA335F, 0xB8D65F69, 0xD4359784, 0xB4221B1F,
        0x62D10656, 0x9D21F75F, 0x4033297C, 0x3EC47B1F, 0x0165452A, 0x4285075F, 0x7645679E, 0xD73BF4DF,
        0x6CEA5B35, 0x7F9A162F, 0xFC9170FF, 0x7F11493F, 0x8E0D756F, 0x807BCAB0, 0x158D8C1F, 0xD1D72BFF,
        0x63D62F6C, 0xF1CD95E6, 0x8CF28847, 0x0573BC87, 0xC89AB21F, 0x0A29EE22, 0x51921C0F, 0x93484829,
        0xB8DD10BA, 0x0208C249, 0x3C370ED8, 0xF9FF479F, 0x1F22FE81, 0xE3D9E7DA, 0xD46FFFBE, 0x949F727E,
        0xB2F8FC7B, 0xC32E579F, 0xDC03559F, 0xB294FA7F, 0x3C37F53B, 0x7A637652, 0x1A416C1F, 0xC6BCB760,
        0xC7B7221F, 0x3FAEBD99, 0x556321E8, 0xE2F2A577, 0x89E48F60, 0xBAF0312F, 0x3693D7DD, 0x001F390F,
        0x875B889F, 0xAA7A661F, 0x0FAE7C3A, 0xFCB97E97, 0x35E05232, 0xB158FE34, 0x87EA4535, 0xA121ED1F,
        0x55D7D4A2, 0xBAAC1113, 0x92DB7482, 0xABC79BDF, 0x6B387005, 0x30D3310D, 0x4D665A3B, 0xB067B387,
        0xBCFDAB46, 0x703C848F, 0x4A1A5834, 0x0C7CC58F, 0x1BECDC28, 0xB5F9624E, 0x7B1912EF, 0x4D99D80F,
        0x486FA15F, 0x5EF34ECB, 0xE2E26C0A, 0x182514FF, 0x4B4B5488, 0xB1CB9AC4, 0x4C50EC4C, 0x4E12E23A,
        0xB527F968, 0xCA5E0CF2, 0xDB804C5F, 0xE1C863BF, 0x1F2BF3E3, 0x48C1929F, 0x5CDD211F, 0x22B9E337,
        0xB915B51F, 0x3BC19C0F, 0xACCB101F, 0xBED27FDF, 0xE88CF03A, 0xB7207ADC, 0x288CEECB, 0xCDD593BF,
        0x2F5D341F, 0xEA70E15F, 0x748554FF, 0xFEA6831F, 0x1E734C21, 0x3E9EF439, 0x92D35AB0, 0xE52C47F7,
        0xFAADE546, 0x059CF7DF, 0x70A23401, 0xFB8236DF, 0xA27F5CC6, 0x3F8E2E21, 0xA225E9ED, 0xD29A2B7F,
        0x067CE003, 0x68C5841F, 0x07B8981F, 0xE1EDE99F, 0xA9474AD2, 0xC10558CF, 0xBB342EA7, 0x8406E357,
        0x553BCC1F, 0xB993798D, 0x66EC169E, 0xFA0D2FA7, 0x4119491F, 0x0D8FD02F, 0x88B3BC2F, 0x977E8F1F,
        0xB7BAA91F, 0x28A1CD5D, 0xAE795734, 0xC40B3497, 0x57F4FD1F, 0x18C0D8AF, 0xC12F0501, 0x9054B91F,
        0x6BE44FDF, 0x3514C1E3, 0x1D3B5A4A, 0x6B324C9F, 0xEC6DF278, 0xBE0823AC, 0xC1D28286, 0x451EED1F,
        0xEE81541F, 0x25867CAB, 0x6A8FC7DB, 0x117FE43F, 0x1BAE7C13, 0xF727CC77, 0x70D43E20, 0x0DCC0D63,
        0x86957634, 0x5D191B07, 0xF68806CA, 0xB0EEE8AF, 0x3722003D, 0x7517C18F, 0x0E50E51F, 0x9CD2900F,
        0xB9E7457C, 0x94B209F5, 0x3132B2F9, 0x0821B09F, 0xEE45A21F, 0x1644BCB8, 0x7E3F6D1F, 0x0930AFBF,
        0x70814CCF, 0x3497CA4B, 0xA5D0D11F, 0x169C1A8F, 0x7D33C4AE, 0x80E551A5, 0x2A1F4A69, 0xB8EE6181,
        0x93890DAB, 0x4185501A, 0x5855112A, 0x44F43B37, 0xBE1F7BA5, 0x25873CDF, 0x29F3FD1F, 0x6216DCCC,
    ];

    const GUARD: usize = 16;
    const DEPTHS: [i32; 6] = [1, 2, 4, 8, 16, 32];
    const RULES: [sqInt; 35] = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, //
        18, 19, 20, 21, 24, 25, 26, 27, 28, 29, 30, 31, 32, 34, 37, 38, 39, 40, 41,
    ];

    struct Lcg(u32);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            self.0
        }
    }

    #[test]
    fn four_hundred_scenarios_match_the_c_plugin() {
        for (i, &want) in C_HASHES.iter().enumerate() {
            let mut r = Lcg(0x9E37_79B9 ^ (i as u32).wrapping_mul(2_654_435_761));
            let srcD = DEPTHS[(r.next() % 6) as usize];
            let dstD = DEPTHS[(r.next() % 6) as usize];
            let srcM = (r.next() % 4) != 0;
            let dstM = (r.next() % 4) != 0;
            let rule = RULES[(r.next() % 35) as usize];
            let sx0 = (r.next() % 24) as i32;
            let sy0 = (r.next() % 4) as i32;
            let dx0 = (r.next() % 24) as i32;
            let dy0 = (r.next() % 4) as i32;
            let mut w0 = 1 + (r.next() % 40) as i32;
            let mut h0 = 1 + (r.next() % 4) as i32;
            let useMap = (r.next() % 3) == 0;
            let useHt = (r.next() % 4) == 0;
            let noSrc = (r.next() % 8) == 0;
            let sourceAlpha = (r.next() % 256) as sqInt;
            let caAlpha = (r.next() % 256) as sqInt;
            let caColor = (r.next() & 0xFF_FFFF) as sqInt;
            w0 = w0.min(64 - sx0.max(dx0));
            h0 = h0.min(8 - sy0.max(dy0));
            let wprS = ((64 * srcD) / 32) as usize;
            let wprD = ((64 * dstD) / 32) as usize;
            let mut srcArena: Vec<u32> =
                (0..GUARD + wprS * 8 + GUARD).map(|_| r.next()).collect();
            let mut dstArena: Vec<u32> =
                (0..GUARD + wprD * 8 + GUARD).map(|_| r.next()).collect();
            let mut lookup: Vec<u32> = (0..512).map(|_| r.next()).collect();
            let halftone: Vec<u32> = (0..4).map(|_| r.next()).collect();

            let mut bb = BitBlt::new();
            bb.combinationRule = rule;
            bb.noSource = noSrc;
            bb.sourceForm = SOURCE_FORM_OOP;
            bb.destForm = DEST_FORM_OOP;
            bb.sourceDepth = srcD;
            bb.destDepth = dstD;
            bb.sourceMSB = srcM as i32;
            bb.destMSB = dstM as i32;
            bb.sourcePPW = (32 / srcD) as sqInt;
            bb.destPPW = (32 / dstD) as sqInt;
            bb.sourcePitch = (wprS * 4) as i32;
            bb.destPitch = (wprD * 4) as i32;
            bb.sourceBits = srcArena[GUARD..].as_mut_ptr() as usize;
            bb.destBits = dstArena[GUARD..].as_mut_ptr() as usize;
            bb.endOfSource = bb.sourceBits + wprS * 8 * 4;
            bb.endOfDestination = bb.destBits + wprD * 8 * 4;
            if useMap {
                bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
                bb.cmMask = 511;
                bb.cmBitsPerColor = 3;
                bb.cmLookupTable = lookup.as_mut_ptr() as usize;
            }
            if useHt {
                bb.noHalftone = false;
                bb.halftoneBase = halftone.as_ptr() as usize;
                bb.halftoneHeight = 1 + (i as sqInt % 4);
            } else {
                bb.noHalftone = true;
            }
            bb.sourceAlpha = sourceAlpha;
            bb.componentAlphaModeAlpha = caAlpha;
            bb.componentAlphaModeColor = caColor;
            bb.sx = sx0;
            bb.sy = sy0;
            bb.dx = dx0;
            bb.dy = dy0;
            bb.bbW = w0;
            bb.bbH = h0;
            // As the C driver: copyBitsDispatch minus the affected-rect
            // bookkeeping.
            // SAFETY: harness-owned buffers, geometry as set up.
            unsafe {
                if !bb.tryCopyingBitsQuickly() {
                    bb.bitCount = 0;
                    bb.performCopyLoop();
                }
            }

            let mut hash: u32 = 2_166_136_261;
            for &wv in &dstArena[GUARD..GUARD + wprD * 8] {
                hash ^= wv;
                hash = hash.wrapping_mul(16_777_619);
            }
            hash ^= bb.bitCount as u32;
            hash = hash.wrapping_mul(16_777_619);
            assert_eq!(
                hash, want,
                "case {i}: rule {rule} {srcD}->{dstD} msb {srcM}/{dstM} \
                 s({sx0},{sy0}) d({dx0},{dy0}) w{w0} h{h0} map {useMap} ht {useHt} noSrc {noSrc}"
            );
        }
    }
}

/// 150 generated WarpBlt scenarios run through the real C plugin's
/// `warpLoop` (compiled standalone with a faked interpreter serving the quad
/// points and smoothing arguments), hashed like the copy sweep. Covers both
/// smoothing modes, all depth pairs, LSB destinations, indexed maps, and
/// off-source sampling.
mod warp_differential {
    use super::*;
    use crate::state::{COLOR_MAP_INDEXED_PART, COLOR_MAP_PRESENT};

    const C_WARP_HASHES: [u32; 150] = [
        0x65B27A5B, 0x549A2EDF, 0x7AAFCDEF, 0x56D4451C, 0x6B2EEE0F, 0x81C4579F, 0x79FB4BDF, 0xD4E1675F,
        0x5B119973, 0xD703C11F, 0xA92FC8A9, 0x7F31901F, 0xA4DF2EEE, 0xBCB94C1F, 0x373395B4, 0xBCFCBFA7,
        0x49252136, 0x71649F1F, 0xC8E45C57, 0x291093F6, 0x02138FE7, 0x5593D31F, 0x08DAFC3F, 0xA1E46EDF,
        0xCBE44E7F, 0xE099E59F, 0xF15A799F, 0x2B0379E6, 0x1D289040, 0x8818D39B, 0x7AADA3FA, 0x31EEB77C,
        0x3D52FFDF, 0x03CF551F, 0x6785D105, 0x0A5704BB, 0x1D8559B3, 0x73FE2B2E, 0x53C0258F, 0x93D61DD9,
        0xDCF31005, 0xFFE1091F, 0xC4A5A4BF, 0xB3293409, 0xA5E7E6BF, 0x8A567F41, 0x2A57A2CF, 0xF1D4F8FB,
        0xE61A12EF, 0xE9AFE79F, 0x3520996F, 0x365A771F, 0xCDEDF7DF, 0xD10E1AB9, 0x853937A4, 0xC2C813E0,
        0x4573D0B7, 0x96D444E3, 0x4FF2B48F, 0xDF6C4C5F, 0x292AFE6F, 0xF4763A9F, 0xFB7C0A5F, 0x4ED37E4E,
        0x3F83138F, 0x5A0A028A, 0x2D9E803F, 0x905CBFA5, 0xA3A3993F, 0x1468C11F, 0x5179CD3F, 0xC8FF853A,
        0x55B68FCF, 0x0CC5191F, 0x8FF9031F, 0x8AE150DF, 0xA80ED6B5, 0xA23A62A3, 0xA169EE3B, 0x82DCC91F,
        0x72B9C633, 0xB64D961F, 0x61152B77, 0xAD72411F, 0xB4D2F85F, 0x9EF7A7C8, 0x876EEDFF, 0x5FD10D1F,
        0xD87A38F7, 0xC118B51F, 0x6E47FC9F, 0xE8DE331F, 0xDD43D733, 0x177B091F, 0x5CFE175D, 0x186DB3DF,
        0xB0DBDDC7, 0x828C2F1F, 0x97E3983F, 0x44D1AF1F, 0x2639226F, 0xCAB9511F, 0xA942E837, 0x6B6AACF6,
        0x75CF3B8F, 0x3EA9DCDF, 0x3D1251E7, 0x5F1655B3, 0x32B9382F, 0xFD11191F, 0xCCD8BFBD, 0xE8F68BDF,
        0x5007CCB0, 0x5FE2A51F, 0xCF26FB1F, 0x58048BDF, 0x613D3F2F, 0x5E86391F, 0x32B56E0F, 0x2F37C01F,
        0x76578044, 0x4733121F, 0x02E3C54A, 0x3734551F, 0x3E2094F0, 0x77F8A11F, 0x988FD4F3, 0x75C2E713,
        0xAAFDC13F, 0x240A50DF, 0xEDBF135F, 0xB063E11F, 0x3FC402F8, 0x9C6E851F, 0xFB6491BF, 0x9E10DD1F,
        0x7BA97BCF, 0x442A0B79, 0xC2EA231F, 0xA51CE741, 0x68770D8D, 0xBA7B3A1F, 0xA8B2AEAB, 0xAEB3E41F,
        0xFAAC3C1F, 0x91297338, 0xA50EEDDF, 0x4FD7CD9F, 0x6A425C3F, 0xB0BEFBF3,
    ];

    const GUARD: usize = 16;
    const DEPTHS: [i32; 6] = [1, 2, 4, 8, 16, 32];

    struct Lcg(u32);
    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
            self.0
        }
    }

    #[test]
    fn warp_scenarios_match_the_c_plugin() {
        for (i, &want) in C_WARP_HASHES.iter().enumerate() {
            let mut r = Lcg(0xABCD_1234 ^ (i as u32).wrapping_mul(2_654_435_761));
            let srcD = DEPTHS[(r.next() % 6) as usize];
            let dstD = DEPTHS[(r.next() % 6) as usize];
            let srcM = (r.next() % 4) != 0;
            let dstM = (r.next() % 4) != 0;
            let rule: sqInt = if r.next() % 2 != 0 { 3 } else { 25 };
            let useMap = (r.next() % 3) == 0;
            let smoothing = 1 + (r.next() % 2) as sqInt;
            let destX = (r.next() % 8) as sqInt;
            let destY = (r.next() % 4) as sqInt;
            let width = 1 + (r.next() % 40) as sqInt;
            let height = 1 + (r.next() % 6) as sqInt;
            let clipX = (r.next() % 6) as sqInt;
            let clipY = (r.next() % 3) as sqInt;
            let mut q = [0 as sqInt; 8];
            for slot in q.iter_mut() {
                *slot = (r.next() % (67 << 14)) as sqInt - (1 << 14);
            }
            let wprS = ((64 * srcD) / 32) as usize;
            let wprD = ((64 * dstD) / 32) as usize;
            let srcArena: Vec<u32> = (0..GUARD + wprS * 8 + GUARD).map(|_| r.next()).collect();
            let mut dstArena: Vec<u32> =
                (0..GUARD + wprD * 8 + GUARD).map(|_| r.next()).collect();
            let lookup: Vec<u32> = (0..512).map(|_| r.next()).collect();
            let mapWords: Vec<u32> = (0..65536).map(|_| r.next()).collect();

            let mut bb = BitBlt::new();
            bb.combinationRule = rule;
            bb.noSource = false;
            bb.sourceForm = SOURCE_FORM_OOP;
            bb.destForm = DEST_FORM_OOP;
            bb.sourceDepth = srcD;
            bb.destDepth = dstD;
            bb.sourceMSB = srcM as i32;
            bb.destMSB = dstM as i32;
            bb.sourcePPW = (32 / srcD) as sqInt;
            bb.destPPW = (32 / dstD) as sqInt;
            bb.sourcePitch = (wprS * 4) as i32;
            bb.destPitch = (wprD * 4) as i32;
            bb.sourceBits = srcArena[GUARD..].as_ptr() as usize;
            bb.destBits = dstArena[GUARD..].as_mut_ptr() as usize;
            bb.sourceWidth = 64;
            bb.sourceHeight = 8;
            bb.destWidth = 64;
            bb.destHeight = 8;
            bb.endOfSource = bb.sourceBits + wprS * 8 * 4;
            bb.endOfDestination = bb.destBits + wprD * 8 * 4;
            if useMap {
                bb.cmFlags = COLOR_MAP_PRESENT | COLOR_MAP_INDEXED_PART;
                bb.cmMask = 511;
                bb.cmBitsPerColor = 3;
                bb.cmLookupTable = lookup.as_ptr() as usize;
            }
            bb.noHalftone = true;
            bb.destX = destX;
            bb.destY = destY;
            bb.width = width;
            bb.height = height;
            bb.clipX = clipX;
            bb.clipY = clipY;
            bb.clipWidth = 64 - clipX;
            bb.clipHeight = 8 - clipY;
            bb.isWarping = true;

            // warpBits minus surface locking.
            let ns = bb.noSource;
            bb.noSource = true;
            bb.clipRange();
            bb.noSource = ns;
            if bb.bbW > 0 && bb.bbH > 0 {
                bb.destMaskAndPointerInit();
                // The interpreter half of warpLoop, on the faked object:
                // slotSize passes, every warp field is a SmallInteger, and
                // the source map is always long enough.
                let mut nSteps = bb.height - 1;
                if nSteps <= 0 {
                    nSteps = 1;
                }
                let mut pAx = q[0];
                let deltaP12x = deltaFromtonSteps(pAx, q[2], nSteps);
                if deltaP12x < 0 {
                    pAx = q[2] - nSteps * deltaP12x;
                }
                let mut pAy = q[1];
                let deltaP12y = deltaFromtonSteps(pAy, q[3], nSteps);
                if deltaP12y < 0 {
                    pAy = q[3] - nSteps * deltaP12y;
                }
                let mut pBx = q[6];
                let deltaP43x = deltaFromtonSteps(pBx, q[4], nSteps);
                if deltaP43x < 0 {
                    pBx = q[4] - nSteps * deltaP43x;
                }
                let mut pBy = q[7];
                let deltaP43y = deltaFromtonSteps(pBy, q[5], nSteps);
                if deltaP43y < 0 {
                    pBy = q[5] - nSteps * deltaP43y;
                }
                // SAFETY: harness-owned buffers, geometry as set up.
                unsafe {
                    bb.warpLoopBody(
                        pAx,
                        pAy,
                        pBx,
                        pBy,
                        deltaP12x,
                        deltaP12y,
                        deltaP43x,
                        deltaP43y,
                        smoothing,
                        mapWords.as_ptr() as usize,
                    )
                };
            }

            let mut hash: u32 = 2_166_136_261;
            for &wv in &dstArena[GUARD..GUARD + wprD * 8] {
                hash ^= wv;
                hash = hash.wrapping_mul(16_777_619);
            }
            // The C driver folds its (always clear) failure flag in.
            hash = hash.wrapping_mul(16_777_619);
            assert_eq!(
                hash, want,
                "warp case {i}: rule {rule} {srcD}->{dstD} msb {srcM}/{dstM} smoothing {smoothing} map {useMap}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Fail-fast after a panic mid-load
// ---------------------------------------------------------------------------

/// A panic while [`crate::state`] is held refuses every later lock.
///
/// The SDK proves the mechanism in
/// `pharo-vm-plugin/tests/plugin_mutex_poison.rs`; this proves the *wiring* at
/// the site with the widest window in the tree. `state()` is held across the
/// whole of `loadBitBltFrom:warping:`, so the span in which the ~40 fields
/// describing source, destination and clip disagree is not two instructions --
/// it is every accessor call the load makes.
///
/// It shares this binary with the blitter tests above, which is safe in one
/// direction only and deliberately: nothing else in this file calls `state()`,
/// and a poisoned `Mutex` never unpoisons, so this test must stay the only one
/// that takes that lock. The module-wide flag is not touched here at all --
/// that needs `setInterpreter` to have installed the panic hook, which no unit
/// test does.
///
/// The panic is raised by this test rather than injected through the proxy on
/// purpose: every plugin-to-VM call crosses `extern "C"`, whose abort-on-unwind
/// shim would turn an injected panic into `SIGABRT` instead of the unwind the
/// hazard is made of.
#[test]
fn a_panic_while_the_blitter_state_is_loaded_refuses_every_later_lock() {
    use pharo_vm_plugin::PrimErr;

    assert!(crate::state().is_ok(), "a fresh module hands out the state");

    let torn = std::panic::catch_unwind(|| {
        let mut bb = crate::state().expect("still healthy");
        // Half of what `loadBitBltFrom:warping:` writes: the rule now says
        // one thing and every geometry field still says another.
        bb.combinationRule = 3;
        panic!("an accessor raised an error mid-load");
    });
    assert!(torn.is_err());

    assert_eq!(
        crate::state().err(),
        Some(PrimErr::Unsupported),
        "the half-loaded BitBlt must never be handed to a caller -- not to a \
         primitive, and not to the `copyBits` the Balloon engine calls outside \
         the primitive fence"
    );
}
