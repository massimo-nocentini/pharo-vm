//! `JPEGReaderPlugin`, in Rust.
//!
//! The accelerator behind the image's *pure-Smalltalk* JPEG decoder
//! (`JPEGReadStream` / `JPEGDecompressStream`): four primitives that take
//! over the hot loops — huffman-decoding one MCU block, the integer inverse
//! DCT, and Y'CbCr-to-ARGB conversion — while every piece of decoder state
//! stays in Smalltalk objects handed to each call. This is **not**
//! `JPEGReadWriter2Plugin` (the libjpeg-based codec, ported separately as
//! `rust/plugins/jpeg-plugin`); nothing here parses JPEG files, so no JPEG
//! crate could substitute — the port is a faithful mechanical translation
//! of the generated C, `plugins/JPEGReaderPlugin/src/common/JPEGReaderPlugin.c`.
//!
//! # What changes underneath
//!
//! * **No cached raw pointers into image memory.** The C held `int*`s to up
//!   to 3x128 block WordArrays in globals and indexed them without bounds
//!   checks; a cursor past the loaded blocks dereferenced a stale pointer.
//!   Here every input is copied out, computed on, and written back once —
//!   an out-of-range cursor is a clean primitive failure.
//! * **No partial writes on failure.** See the README for this and the
//!   other removed undefined behaviours.

#![allow(non_snake_case)] // primitive names are fixed by the image

mod color;
mod huffman;
mod idct;
mod stream;

use core::slice;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use color::{
    Component, BLOCK_WIDTH_INDEX, CURRENT_X_INDEX, CURRENT_Y_INDEX, H_SCALE_INDEX,
    MAX_MCU_BLOCKS, MCU_BLOCK_INDEX, MCU_WIDTH_INDEX, MIN_COMPONENT_SIZE, PRIOR_DC_VALUE_INDEX,
    V_SCALE_INDEX,
};
use stream::JpegStream;

/// `usqInt` in the generated C: the unsigned register-width integer.
pub(crate) type UsqInt = usize;

pub(crate) const DCT_SIZE: usize = 8;
pub(crate) const DCT_SIZE2: usize = 64;
pub(crate) const MAX_SAMPLE: sqInt = 255;
pub(crate) const SAMPLE_OFFSET: sqInt = 127;

// The C's getModuleName answers "JPEGReaderPlugin VMMaker.oscog-eem.2480
// (e)"; the VM compares only the module-name prefix, and the jpeg-plugin
// port established the convention of answering the bare name.
pharo_plugin!("JPEGReaderPlugin");

// ---------------------------------------------------------------------------
// Proxy calls the safe API does not cover
// ---------------------------------------------------------------------------

/// The proxy's `isWords`: word-indexable and *not* byte-indexable — stricter
/// than the safe API's `is_words_or_bytes`, and the check the C used on
/// every WordArray argument.
fn is_words(vm: &Interp, oop: Oop) -> PrimResult<bool> {
    // SAFETY: the pointer came from the VM's own proxy table; the signature
    // is the one declared in the SDK's proxy.rs.
    unsafe {
        let f = (*vm.as_raw()).isWords.ok_or(PrimErr::Unsupported)?;
        Ok(f(oop.0) != 0)
    }
}

/// The proxy's `storeIntegerofObjectwithValue`, for
/// `storeJPEGStreamOn:`-style write-back of SmallInteger instance
/// variables. Like the C, the result is not checked (see README).
fn store_integer(vm: &Interp, index: sqInt, oop: Oop, value: sqInt) -> PrimResult<()> {
    // SAFETY: as in `is_words`; `oop` is a pointers object with at least
    // `index + 1` slots, validated by the caller's load pass.
    unsafe {
        let f = (*vm.as_raw())
            .storeIntegerofObjectwithValue
            .ok_or(PrimErr::Unsupported)?;
        f(index, oop.0, value);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Marshalling image objects
// ---------------------------------------------------------------------------

/// A WordArray's slots as the signed 32-bit values the C read through its
/// `int*`, where they lie, after the same `isWords` check.
///
/// A huffman table is hundreds of words and is consulted once per symbol;
/// the C indexed the image object directly and never copied one, and neither
/// does this. The reinterpretation is free: `i32` and `u32` have the same
/// size and alignment and every bit pattern is a valid `i32`, so this is the
/// same read the C's `int*` performed.
fn ints_of(vm: &Interp, oop: Oop) -> PrimResult<&[i32]> {
    if !is_words(vm, oop)? {
        return Err(PrimErr::GenericFailure);
    }
    let words = vm.words_of(oop)?;
    // SAFETY: same layout, same length, and no `i32` bit pattern is invalid.
    Ok(unsafe { slice::from_raw_parts(words.as_ptr().cast::<i32>(), words.len()) })
}

/// One 64-slot WordArray as a coefficient block, where it lies.
fn block_of(vm: &Interp, oop: Oop) -> PrimResult<&[i32; DCT_SIZE2]> {
    ints_of(vm, oop)?
        .try_into()
        .map_err(|_| PrimErr::GenericFailure)
}

/// The mutable twin of [`ints_of`], over words already borrowed in place.
fn ints_mut(words: &mut [u32]) -> &mut [i32] {
    // SAFETY: as in `ints_of`; the caller's `&mut` is the only live borrow.
    unsafe { slice::from_raw_parts_mut(words.as_mut_ptr().cast::<i32>(), words.len()) }
}

/// A block's bit pattern for `write_words`.
fn block_bits(block: &[i32; DCT_SIZE2]) -> [u32; DCT_SIZE2] {
    let mut words = [0u32; DCT_SIZE2];
    for (dst, &v) in words.iter_mut().zip(block) {
        *dst = v as u32;
    }
    words
}

/// `JPEGReaderPlugin>>#colorComponent:from:` — the scalar fields of a
/// `JPEGColorComponent`, truncated to 32 bits as the C's `int[]` was.
fn color_component_from(vm: &Interp, oop: Oop) -> PrimResult<color::ColorComponent> {
    if !vm.is_pointers(oop)? || vm.slot_size_of(oop)? < MIN_COMPONENT_SIZE as sqInt {
        return Err(PrimErr::GenericFailure);
    }
    let mut fields: color::ColorComponent = [0; MIN_COMPONENT_SIZE];
    for idx in [
        CURRENT_X_INDEX,
        CURRENT_Y_INDEX,
        H_SCALE_INDEX,
        V_SCALE_INDEX,
        BLOCK_WIDTH_INDEX,
        MCU_WIDTH_INDEX,
        PRIOR_DC_VALUE_INDEX,
    ] {
        // fetch_integer fails (as the C's fetchIntegerofObject: did) when
        // the slot is not a SmallInteger.
        fields[idx] = vm.fetch_integer(idx as sqInt, oop)? as i32;
    }
    Ok(fields)
}

/// `JPEGReaderPlugin>>#colorComponentBlocks:from:` — the component's MCU
/// blocks, each a 64-slot WordArray, copied out.
fn color_component_blocks_from<'a>(vm: &'a Interp, oop: Oop) -> PrimResult<color::Blocks<'a>> {
    if !vm.is_pointers(oop)? || vm.slot_size_of(oop)? < MIN_COMPONENT_SIZE as sqInt {
        return Err(PrimErr::GenericFailure);
    }
    let array_oop = vm.fetch_pointer(MCU_BLOCK_INDEX as sqInt, oop)?;
    if !vm.is_pointers(array_oop)? {
        return Err(PrimErr::GenericFailure);
    }
    let max = vm.slot_size_of(array_oop)?;
    if max > MAX_MCU_BLOCKS as sqInt {
        return Err(PrimErr::GenericFailure);
    }
    let mut blocks = color::Blocks::new();
    for i in 0..max {
        let block_oop = vm.fetch_pointer(i, array_oop)?;
        if !is_words(vm, block_oop)? || vm.slot_size_of(block_oop)? != DCT_SIZE2 as sqInt {
            return Err(PrimErr::GenericFailure);
        }
        // The `max` check above already fits the table.
        if !blocks.push(block_of(vm, block_oop)?) {
            return Err(PrimErr::GenericFailure);
        }
    }
    Ok(blocks)
}

/// `yColorComponentFrom:` / `cbColorComponentFrom:` / `crColorComponentFrom:`
/// — fields, then blocks, short-circuiting like the C's `&&`.
fn full_component<'a>(vm: &'a Interp, oop: Oop) -> PrimResult<Component<'a>> {
    let fields = color_component_from(vm, oop)?;
    let blocks = color_component_blocks_from(vm, oop)?;
    Ok(Component { fields, blocks })
}

/// `JPEGReaderPlugin>>#loadJPEGStreamFrom:` — the stream's collection and
/// its position/limit/bit state, with the C's validity checks.
///
/// The returned stream borrows the collection's bytes; nothing in these
/// primitives allocates through the VM, so the borrow cannot be moved under
/// us.
fn load_jpeg_stream<'a>(vm: &'a Interp, stream_oop: Oop) -> PrimResult<JpegStream<'a>> {
    if !vm.is_pointers(stream_oop)? || vm.slot_size_of(stream_oop)? < 5 {
        return Err(PrimErr::GenericFailure);
    }
    let collection = vm.fetch_pointer(0, stream_oop)?;
    if !vm.is_bytes(collection)? {
        return Err(PrimErr::GenericFailure);
    }
    let bytes = vm.bytes_of(collection)?;
    let position = vm.fetch_integer(1, stream_oop)?;
    let read_limit = vm.fetch_integer(2, stream_oop)?;
    let bit_buffer = vm.fetch_integer(3, stream_oop)?;
    let bit_count = vm.fetch_integer(4, stream_oop)?;
    JpegStream::new(bytes, position, read_limit, bit_buffer, bit_count)
        .ok_or(PrimErr::GenericFailure)
}

/// The shared head of both colour-convert primitives: ditherMask, the
/// 3-slot residuals WordArray and the destination bits WordArray, validated
/// in the C's order.
fn convert_common(vm: &Interp) -> PrimResult<(sqInt, Oop, [i32; 3], Oop)> {
    vm.expect_argument_count(4)?;
    let dither_mask = vm.stack_integer(0)?;
    let residuals_oop = vm.stack_value(1)?;
    if !is_words(vm, residuals_oop)? || vm.slot_size_of(residuals_oop)? != 3 {
        return Err(PrimErr::GenericFailure);
    }
    let w = vm.words_of(residuals_oop)?;
    let residuals = [w[0] as i32, w[1] as i32, w[2] as i32];
    let bits_oop = vm.stack_value(2)?;
    if !is_words(vm, bits_oop)? {
        return Err(PrimErr::GenericFailure);
    }
    Ok((dither_mask, residuals_oop, residuals, bits_oop))
}

/// Writes the carried residuals back.
fn finish_convert(vm: &Interp, residuals_oop: Oop, residuals: [i32; 3]) -> PrimResult<()> {
    let r = [
        residuals[0] as u32,
        residuals[1] as u32,
        residuals[2] as u32,
    ];
    vm.write_words(residuals_oop, 0, &r)?;
    // Answering () answers the receiver and pops the arguments — what the
    // C's pop(4) left the stack as.
    Ok(())
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// Converts one grayscale MCU into 32-bit gray pixels.
///
/// Arguments: `component bits residuals ditherMask`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveColorConvertGrayscaleMCU(vm: &Interp) -> PrimResult<()> {
    let (dither_mask, residuals_oop, mut residuals, bits_oop) = convert_common(vm)?;
    let component_oop = vm.stack_value(3)?;

    // The destination is filled where it lies -- the C's `unsigned int
    // *bits` -- rather than through a staging buffer the size of the whole
    // MCU. Its view is taken before the component's blocks are borrowed, so
    // an image that passes the bitmap as one of them fails cleanly instead
    // of converting out of the array it is writing.
    vm.with_words_mut(bits_oop, |bits| {
        let mut y = full_component(vm, component_oop)?;
        color::color_convert_grayscale_mcu(&mut y, &mut residuals, dither_mask, bits)
            .map_err(|()| PrimErr::GenericFailure)
    })??;
    finish_convert(vm, residuals_oop, residuals)
}

/// Converts one Y'CbCr MCU into 32-bit ARGB pixels.
///
/// Arguments: `(Array of: 3 components) bits residuals ditherMask`.
#[pharo_primitive(accessor_depth = 3)]
fn primitiveColorConvertMCU(vm: &Interp) -> PrimResult<()> {
    let (dither_mask, residuals_oop, mut residuals, bits_oop) = convert_common(vm)?;
    let components_oop = vm.stack_value(3)?;
    if !vm.is_pointers(components_oop)? || vm.slot_size_of(components_oop)? != 3 {
        return Err(PrimErr::GenericFailure);
    }

    // In place, blocks borrowed inside the view: see the grayscale case.
    vm.with_words_mut(bits_oop, |bits| {
        let mut y = full_component(vm, vm.fetch_pointer(0, components_oop)?)?;
        let mut cb = full_component(vm, vm.fetch_pointer(1, components_oop)?)?;
        let mut cr = full_component(vm, vm.fetch_pointer(2, components_oop)?)?;
        color::color_convert_mcu(&mut y, &mut cb, &mut cr, &mut residuals, dither_mask, bits)
            .map_err(|()| PrimErr::GenericFailure)
    })??;
    finish_convert(vm, residuals_oop, residuals)
}

/// Huffman-decodes the next 8x8 coefficient block from the stream.
///
/// Arguments: `anArray component dcTable acTable stream`; on success the
/// stream's position/bitBuffer/bitCount and the component's priorDCValue
/// are stored back and the block is filled in natural order.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveDecodeMCU(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(5)?;
    let stream_oop = vm.stack_value(0)?;
    let mut stream = load_jpeg_stream(vm, stream_oop)?;
    let ac_table = ints_of(vm, vm.stack_value(1)?)?;
    let dc_table = ints_of(vm, vm.stack_value(2)?)?;
    let component_oop = vm.stack_value(3)?;
    let fields = color_component_from(vm, component_oop)?;
    let array_oop = vm.stack_value(4)?;
    if !is_words(vm, array_oop)? || vm.slot_size_of(array_oop)? != DCT_SIZE2 as sqInt {
        return Err(PrimErr::GenericFailure);
    }

    let mut prior_dc = fields[PRIOR_DC_VALUE_INDEX];
    let coeffs = huffman::decode_block(&mut stream, dc_table, ac_table, &mut prior_dc)
        .map_err(|()| PrimErr::GenericFailure)?;
    let (position, bit_buffer, bit_count) = (stream.position, stream.bit_buffer, stream.bit_count);

    // storeJPEGStreamOn:, plus the block itself — all writes on the success
    // path only, where the C interleaved them with the decode.
    vm.write_words(array_oop, 0, &block_bits(&coeffs))?;
    store_integer(vm, 1, stream_oop, position)?;
    store_integer(vm, 3, stream_oop, bit_buffer)?;
    store_integer(vm, 4, stream_oop, bit_count)?;
    store_integer(
        vm,
        PRIOR_DC_VALUE_INDEX as sqInt,
        component_oop,
        prior_dc as sqInt,
    )?;
    // Answering () pops the five arguments, as the C's pop(5) did.
    Ok(())
}

/// Dequantises and inverse-transforms one block in place.
///
/// Arguments: `anArray qt`, both 64-slot word arrays of signed integers.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveIdctInt(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(2)?;
    let qt_oop = vm.stack_value(0)?;
    if !is_words(vm, qt_oop)? || vm.slot_size_of(qt_oop)? != DCT_SIZE2 as sqInt {
        return Err(PrimErr::GenericFailure);
    }
    let array_oop = vm.stack_value(1)?;
    if !is_words(vm, array_oop)? || vm.slot_size_of(array_oop)? != DCT_SIZE2 as sqInt {
        return Err(PrimErr::GenericFailure);
    }
    // Dequantised and transformed where it lies -- the C's `int *array`.
    // The mutable view is taken first, so an image that passes one object as
    // both arguments fails here instead of transforming the quantisation
    // table into itself.
    vm.with_words_mut(array_oop, |words| {
        let block: &mut [i32; DCT_SIZE2] = ints_mut(words)
            .try_into()
            .map_err(|_| PrimErr::GenericFailure)?;
        idct::idct_block_int(block, block_of(vm, qt_oop)?);
        Ok(())
    })?
    // Answering () pops the two arguments, as the C's pop(2) did.
}
