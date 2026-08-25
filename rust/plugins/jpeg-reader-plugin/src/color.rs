//! MCU colour conversion: `JPEGReaderPlugin>>#colorConvertMCU` and
//! `#colorConvertGrayscaleMCU`, with the per-component sample cursor of
//! `#nextSampleFrom:`.
//!
//! A `JPEGColorComponent` walks its decoded 8x8 blocks pixel by pixel,
//! upsampling by the component's h/v scale; the conversion folds Y'CbCr to
//! ARGB in 16.16 fixed point, with an error-diffusion residual per channel
//! controlled by `ditherMask`.

use pharo_vm_plugin::sqInt;

use crate::{UsqInt, DCT_SIZE2, MAX_SAMPLE, SAMPLE_OFFSET};

/// A `JPEGColorComponent`'s scalar fields, as `colorComponent:from:` copied
/// them into the C's `int[11]` globals. Only the seven indexed slots below
/// are ever loaded or read; the others stay zero.
pub type ColorComponent = [i32; MIN_COMPONENT_SIZE];

pub const CURRENT_X_INDEX: usize = 0;
pub const CURRENT_Y_INDEX: usize = 1;
pub const H_SCALE_INDEX: usize = 2;
pub const V_SCALE_INDEX: usize = 3;
/// Instance-variable index of the component's Array of MCU blocks.
pub const MCU_BLOCK_INDEX: usize = 4;
pub const BLOCK_WIDTH_INDEX: usize = 5;
pub const MCU_WIDTH_INDEX: usize = 8;
pub const PRIOR_DC_VALUE_INDEX: usize = 10;
/// Fewest instance variables a JPEGColorComponent may have.
pub const MIN_COMPONENT_SIZE: usize = 11;
/// Most blocks the C's fixed pointer tables could hold.
pub const MAX_MCU_BLOCKS: usize = 128;

pub const RED_INDEX: usize = 0;
pub const GREEN_INDEX: usize = 1;
pub const BLUE_INDEX: usize = 2;

// 16.16 fixed-point Y'CbCr -> RGB factors, as in the C header.
const FIX_0_34414: sqInt = 22554;
const FIX_0_71414: sqInt = 46802;
const FIX_1_40200: sqInt = 91881;
const FIX_1_77200: sqInt = 116130;

/// A colour component with its decoded blocks: the pair of C globals
/// (`yComponent`/`yBlocks`, ...) that `yColorComponentFrom:` and friends
/// loaded together.
pub struct Component {
    pub fields: ColorComponent,
    pub blocks: Vec<[i32; DCT_SIZE2]>,
}

/// `JPEGReaderPlugin>>#nextSampleFrom:` — the sample under the cursor,
/// advancing it. `None` where the C was undefined:
///
/// * exactly one of hScale/vScale zero divided by zero (the C only skips
///   the division when *both* are zero);
/// * a cursor outside the loaded blocks read a stale entry of the C's
///   128-pointer table.
pub fn next_sample(component: &mut Component) -> Option<i32> {
    let comp = &mut component.fields;
    let cur_x = comp[CURRENT_X_INDEX];
    let mut dx = cur_x;
    let mut dy = comp[CURRENT_Y_INDEX];
    let sx = comp[H_SCALE_INDEX];
    let sy = comp[V_SCALE_INDEX];
    if !(sx == 0 && sy == 0) {
        if sx == 0 || sy == 0 {
            return None;
        }
        // wrapping_div: INT_MIN / -1 trapped in the C build; here it wraps.
        dx = dx.wrapping_div(sx);
        dy = dy.wrapping_div(sy);
    }
    // The C widens dx/dy through usqInt, so a negative coordinate becomes a
    // huge block index — caught by the bounds check below.
    let block_index = ((dy as sqInt as UsqInt) >> 3)
        .wrapping_mul(comp[BLOCK_WIDTH_INDEX] as sqInt as UsqInt)
        .wrapping_add((dx as sqInt as UsqInt) >> 3);
    let sample_index = (((dy & 7) as usize) << 3) + (dx & 7) as usize;
    let sample = *component.blocks.get(block_index)?.get(sample_index)?;

    let comp = &mut component.fields;
    let cur_x = cur_x.wrapping_add(1);
    if cur_x < comp[MCU_WIDTH_INDEX].wrapping_mul(8) {
        comp[CURRENT_X_INDEX] = cur_x;
    } else {
        comp[CURRENT_X_INDEX] = 0;
        comp[CURRENT_Y_INDEX] = comp[CURRENT_Y_INDEX].wrapping_add(1);
    }
    Some(sample)
}

/// One colour channel's clamp-and-dither tail: clamp to 0..=MaxSample, fold
/// the masked low bits into the channel's residual, and floor at 1 — the
/// exact sequence the C performs for red, green and blue.
fn dither_channel(value: sqInt, residual: &mut i32, dither_mask: sqInt) -> sqInt {
    let mut v = if value < MAX_SAMPLE { value } else { MAX_SAMPLE };
    v = if v < 0 { 0 } else { v };
    *residual = (v & dither_mask) as i32;
    v &= MAX_SAMPLE.wrapping_sub(dither_mask);
    if v < 1 {
        1
    } else {
        v
    }
}

/// `JPEGReaderPlugin>>#colorConvertMCU` — fills `bits` with one ARGB pixel
/// per slot. `Err` where the C's sample fetch was undefined behaviour; the
/// caller turns that into a primitive failure.
pub fn color_convert_mcu(
    y: &mut Component,
    cb: &mut Component,
    cr: &mut Component,
    residuals: &mut [i32; 3],
    dither_mask: sqInt,
    bits: &mut [u32],
) -> Result<(), ()> {
    y.fields[CURRENT_X_INDEX] = 0;
    y.fields[CURRENT_Y_INDEX] = 0;
    cb.fields[CURRENT_X_INDEX] = 0;
    cb.fields[CURRENT_Y_INDEX] = 0;
    cr.fields[CURRENT_X_INDEX] = 0;
    cr.fields[CURRENT_Y_INDEX] = 0;
    for pixel in bits.iter_mut() {
        let yv = next_sample(y).ok_or(())? as sqInt;
        let cbv = (next_sample(cb).ok_or(())? as sqInt).wrapping_sub(SAMPLE_OFFSET);
        let crv = (next_sample(cr).ok_or(())? as sqInt).wrapping_sub(SAMPLE_OFFSET);

        let red = yv
            .wrapping_add(FIX_1_40200.wrapping_mul(crv) / 65536)
            .wrapping_add(residuals[RED_INDEX] as sqInt);
        let red = dither_channel(red, &mut residuals[RED_INDEX], dither_mask);
        let green = yv
            .wrapping_sub(FIX_0_34414.wrapping_mul(cbv) / 65536)
            .wrapping_sub(FIX_0_71414.wrapping_mul(crv) / 65536)
            .wrapping_add(residuals[GREEN_INDEX] as sqInt);
        let green = dither_channel(green, &mut residuals[GREEN_INDEX], dither_mask);
        let blue = yv
            .wrapping_add(FIX_1_77200.wrapping_mul(cbv) / 65536)
            .wrapping_add(residuals[BLUE_INDEX] as sqInt);
        let blue = dither_channel(blue, &mut residuals[BLUE_INDEX], dither_mask);

        *pixel = 0xFF00_0000u32
            .wrapping_add((red as u32) << 16)
            .wrapping_add((green as u32) << 8)
            .wrapping_add(blue as u32);
    }
    Ok(())
}

/// `JPEGReaderPlugin>>#colorConvertGrayscaleMCU` — one gray ARGB pixel per
/// slot, dithering through the *green* residual only.
pub fn color_convert_grayscale_mcu(
    y: &mut Component,
    residuals: &mut [i32; 3],
    dither_mask: sqInt,
    bits: &mut [u32],
) -> Result<(), ()> {
    y.fields[CURRENT_X_INDEX] = 0;
    y.fields[CURRENT_Y_INDEX] = 0;
    for pixel in bits.iter_mut() {
        let sample = next_sample(y).ok_or(())?;
        let mut yv = (sample as sqInt).wrapping_add(residuals[GREEN_INDEX] as sqInt);
        yv = if yv < MAX_SAMPLE { yv } else { MAX_SAMPLE };
        // No clamp at zero: the C's grayscale path lacks the `< 0` check its
        // colour path has, so a negative sample masks to a high value below.
        // Kept faithfully.
        residuals[GREEN_INDEX] = (yv & dither_mask) as i32;
        yv &= MAX_SAMPLE.wrapping_sub(dither_mask);
        yv = if yv < 1 { 1 } else { yv };
        *pixel = 0xFF00_0000u32
            .wrapping_add((yv as u32) << 16)
            .wrapping_add((yv as u32) << 8)
            .wrapping_add(yv as u32);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component whose fields are (curX, curY, hScale, vScale, blockWidth,
    /// mcuWidth) with the given blocks.
    fn component(sx: i32, sy: i32, block_width: i32, mcu_width: i32, blocks: Vec<[i32; 64]>) -> Component {
        let mut fields: ColorComponent = [0; MIN_COMPONENT_SIZE];
        fields[H_SCALE_INDEX] = sx;
        fields[V_SCALE_INDEX] = sy;
        fields[BLOCK_WIDTH_INDEX] = block_width;
        fields[MCU_WIDTH_INDEX] = mcu_width;
        Component { fields, blocks }
    }

    fn flat_block(v: i32) -> [i32; 64] {
        [v; 64]
    }

    #[test]
    fn cursor_walks_blocks_left_to_right_then_wraps() {
        // Two blocks side by side, no scaling: 16 samples per row, rows of
        // 8 from block 0 then 8 from block 1.
        let mut c = component(0, 0, 2, 2, vec![flat_block(0), flat_block(1)]);
        let row: Vec<i32> = (0..32).map(|_| next_sample(&mut c).unwrap()).collect();
        let expected: Vec<i32> = (0..32).map(|i| (i % 16) / 8).collect();
        assert_eq!(row, expected);
        // Two full rows consumed: cursor sits at the start of row 2.
        assert_eq!(c.fields[CURRENT_X_INDEX], 0);
        assert_eq!(c.fields[CURRENT_Y_INDEX], 2);
    }

    #[test]
    fn scaling_replicates_samples() {
        // 2x2 upsampling of one block over a 16-wide MCU: each source
        // sample covers a 2x2 pixel square.
        let mut block = [0i32; 64];
        for (i, v) in block.iter_mut().enumerate() {
            *v = i as i32;
        }
        let mut c = component(2, 2, 1, 2, vec![block]);
        for dy in 0..4 {
            for dx in 0..16 {
                let sample = next_sample(&mut c).unwrap();
                assert_eq!(sample, (dy / 2) * 8 + (dx / 2), "at ({dx},{dy})");
            }
        }
    }

    #[test]
    fn out_of_range_cursor_is_a_clean_failure() {
        // No blocks at all: the C would have read a stale pointer.
        let mut c = component(0, 0, 1, 1, vec![]);
        assert_eq!(next_sample(&mut c), None);
        // Block width pushing the index past the vector.
        let mut c = component(0, 0, 7, 1, vec![flat_block(0)]);
        c.fields[CURRENT_Y_INDEX] = 8; // dy >> 3 = 1 -> block 7
        assert_eq!(next_sample(&mut c), None);
    }

    #[test]
    fn one_sided_zero_scale_is_a_clean_failure() {
        // The C skips the division only when BOTH scales are zero; sx=2,
        // sy=0 divided by zero there.
        let mut c = component(2, 0, 1, 1, vec![flat_block(9)]);
        assert_eq!(next_sample(&mut c), None);
        // Both zero means "no scaling" and works.
        let mut c = component(0, 0, 1, 1, vec![flat_block(9)]);
        assert_eq!(next_sample(&mut c), Some(9));
    }

    #[test]
    fn neutral_chroma_yields_gray_pixels() {
        let mut y = component(0, 0, 1, 1, vec![flat_block(200)]);
        let mut cb = component(0, 0, 1, 1, vec![flat_block(127)]);
        let mut cr = component(0, 0, 1, 1, vec![flat_block(127)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 64];
        color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).unwrap();
        assert!(bits.iter().all(|&p| p == 0xFFC8_C8C8), "{bits:0x?}");
        assert_eq!(residuals, [0, 0, 0]);
    }

    #[test]
    fn chroma_math_matches_hand_computed_fixed_point() {
        // y=100, cb'=64, cr'=32:
        //   red   = 100 + 91881*32/65536          = 100 + 44 = 144
        //   green = 100 - 22554*64/65536 - 46802*32/65536 = 100-22-22 = 56
        //   blue  = 100 + 116130*64/65536         = 100 + 113 = 213
        let mut y = component(0, 0, 1, 1, vec![flat_block(100)]);
        let mut cb = component(0, 0, 1, 1, vec![flat_block(127 + 64)]);
        let mut cr = component(0, 0, 1, 1, vec![flat_block(127 + 32)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 4];
        color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).unwrap();
        assert!(bits.iter().all(|&p| p == 0xFF90_38D5), "{bits:0x?}");
    }

    #[test]
    fn channels_clamp_high_then_floor_at_one() {
        // y=5, cr'=100: red = 5+140 = 145; green = 5-71 = -66 -> 0 -> 1;
        // blue = 5. And with y=250 the red channel saturates at 255.
        let mut y = component(0, 0, 1, 1, vec![flat_block(5)]);
        let mut cb = component(0, 0, 1, 1, vec![flat_block(127)]);
        let mut cr = component(0, 0, 1, 1, vec![flat_block(227)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 1];
        color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).unwrap();
        assert_eq!(bits[0], 0xFF91_0105);

        let mut y = component(0, 0, 1, 1, vec![flat_block(250)]);
        let mut cb = component(0, 0, 1, 1, vec![flat_block(127)]);
        let mut cr = component(0, 0, 1, 1, vec![flat_block(227)]);
        let mut residuals = [0i32; 3];
        color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).unwrap();
        assert_eq!((bits[0] >> 16) & 0xFF, 255);
    }

    #[test]
    fn dither_folds_masked_bits_into_the_residual() {
        // Gray value 6, mask 3: pixel 0 keeps 4 and carries 2; pixel 1 sees
        // 6+2=8, keeps 8, carries 0; and so on, alternating.
        let mut y = component(0, 0, 1, 1, vec![flat_block(6)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 4];
        color_convert_grayscale_mcu(&mut y, &mut residuals, 3, &mut bits).unwrap();
        let grays: Vec<u32> = bits.iter().map(|&p| p & 0xFF).collect();
        assert_eq!(grays, vec![4, 8, 4, 8]);
        assert_eq!(residuals[GREEN_INDEX], 0);
    }

    #[test]
    fn grayscale_keeps_the_missing_zero_clamp() {
        // A negative sample is NOT clamped at zero in the C's grayscale
        // path: -5 & 255 = 251.
        let mut y = component(0, 0, 1, 1, vec![flat_block(-5)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 1];
        color_convert_grayscale_mcu(&mut y, &mut residuals, 0, &mut bits).unwrap();
        assert_eq!(bits[0], 0xFFFB_FBFB);
    }

    #[test]
    fn conversion_reports_bad_cursors_as_errors() {
        let mut y = component(0, 0, 1, 1, vec![]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 1];
        assert!(color_convert_grayscale_mcu(&mut y, &mut residuals, 0, &mut bits).is_err());

        let mut y = component(0, 0, 1, 1, vec![flat_block(1)]);
        let mut cb = component(2, 0, 1, 1, vec![flat_block(1)]);
        let mut cr = component(0, 0, 1, 1, vec![flat_block(1)]);
        assert!(
            color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).is_err()
        );
    }

    #[test]
    fn subsampled_chroma_walks_at_its_own_pace() {
        // Y at full resolution over two blocks, chroma half resolution from
        // one block whose left half is neutral and right half is +64 blue.
        let mut cb_block = flat_block(127);
        for row in 0..8 {
            for col in 4..8 {
                cb_block[row * 8 + col] = 127 + 64;
            }
        }
        let mut y = component(0, 0, 2, 2, vec![flat_block(100), flat_block(100)]);
        let mut cb = component(2, 2, 1, 2, vec![cb_block]);
        let mut cr = component(2, 2, 1, 2, vec![flat_block(127)]);
        let mut residuals = [0i32; 3];
        let mut bits = vec![0u32; 16]; // one 16-pixel row
        color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, 0, &mut bits).unwrap();
        // Left 8 pixels neutral (100,100,100); right 8 shifted by cb'=64:
        // green 100-22=78, blue 100+113=213.
        assert!(bits[..8].iter().all(|&p| p == 0xFF64_6464), "{bits:0x?}");
        assert!(bits[8..].iter().all(|&p| p == 0xFF64_4ED5), "{bits:0x?}");
    }
}
