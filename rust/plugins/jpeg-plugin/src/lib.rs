//! `JPEGReadWriter2Plugin`, in Rust.
//!
//! This replaces a vendored copy of IJG libjpeg `6b, 27-Mar-1998` --  24,627
//! lines of C, carried in-tree since 1998 and therefore never picking up a
//! distro security update -- with the maintained pure-Rust `jpeg-decoder` and
//! `jpeg-encoder` crates behind the same eleven primitives.
//!
//! The image cannot tell the difference: same module name, same primitive
//! names, same argument order, same answers.
//!
//! # What changes underneath
//!
//! * **No `setjmp`/`longjmp`.** libjpeg reports errors by longjmp-ing out of a
//!   callback; the C plugin `malloc`s a `jmp_buf` per call to catch it. Here a
//!   decode failure is a `Result`.
//! * **No pointers in image memory.** See [`state`] for why the old blob was
//!   fragile across a garbage collection, and what this one holds instead.
//! * **No out-of-bounds read on odd widths.** See [`pixels`].

#![allow(non_snake_case)] // primitive names are fixed by the image

mod encode;
mod pixels;
mod state;

use jpeg_decoder::{Decoder, PixelFormat};
use jpeg_encoder::{ColorType, Encoder};
use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, Oop, PrimErr, PrimResult};

use pixels::{NativeDepth, RowConfig};
use state::{Decompress, ErrorMgr};

pharo_plugin!("JPEGReadWriter2Plugin");

/// Instance variable indices in a `Form`: bits, width, height, depth.
///
/// Fixed by the image's class definition, and read the same way by the C
/// plugin's generated shim.
mod form {
    pub const BITS: isize = 0;
    pub const WIDTH: isize = 1;
    pub const HEIGHT: isize = 2;
    pub const DEPTH: isize = 3;
}

// ---------------------------------------------------------------------------
// Capability and sizing primitives
// ---------------------------------------------------------------------------

/// Answers true, so the image knows a real plugin is installed.
#[pharo_primitive(accessor_depth = 0)]
fn primJPEGPluginIsPresent(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(true)
}

/// Answers whether 8-bit grayscale JPEGs can be decoded straight to a depth-8
/// Form. `jpeg-decoder` emits `L8` for grayscale sources, so yes.
#[pharo_primitive(accessor_depth = 0)]
fn primSupports8BitGrayscaleJPEGs(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(true)
}

/// Bytes the image should allocate for a decompression blob.
#[pharo_primitive(accessor_depth = 0)]
fn primJPEGDecompressStructSize(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(Decompress::blob_size() as isize)
}

/// Bytes the image should allocate for a compression blob.
///
/// The encoder is stateless between calls, so this is the same POD record.
#[pharo_primitive(accessor_depth = 0)]
fn primJPEGCompressStructSize(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(Decompress::blob_size() as isize)
}

/// Bytes the image should allocate for an error record.
#[pharo_primitive(accessor_depth = 0)]
fn primJPEGErrorMgr2StructSize(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(ErrorMgr::blob_size() as isize)
}

// ---------------------------------------------------------------------------
// Header inspection
// ---------------------------------------------------------------------------

/// Reads the decompression blob the image passes as the only argument.
///
/// Answers a zeroed state rather than failing when the blob is not ours: the
/// C plugin answered whatever happened to be in the struct, and the image
/// detects "no header" by seeing a zero width.
fn decompress_arg(vm: &Interp) -> PrimResult<Decompress> {
    // One argument: the blob. With one argument, stack offset 0 is it.
    vm.expect_argument_count(1)?;
    let blob = vm.stack_value(0)?;
    let bytes = vm.bytes_of(blob)?;
    Ok(Decompress::from_bytes(bytes).unwrap_or_else(Decompress::empty))
}

/// Width of the image whose header was read into this blob.
#[pharo_primitive]
fn primImageWidth(vm: &Interp) -> PrimResult<isize> {
    Ok(decompress_arg(vm)?.width as isize)
}

/// Height of the image whose header was read into this blob.
#[pharo_primitive]
fn primImageHeight(vm: &Interp) -> PrimResult<isize> {
    Ok(decompress_arg(vm)?.height as isize)
}

/// Component count of the image whose header was read into this blob.
#[pharo_primitive]
fn primImageNumComponents(vm: &Interp) -> PrimResult<isize> {
    Ok(decompress_arg(vm)?.num_components as isize)
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Components `jpeg-decoder` will emit per pixel for a given source format.
const fn out_components(format: PixelFormat) -> u32 {
    match format {
        PixelFormat::L8 | PixelFormat::L16 => 1,
        PixelFormat::RGB24 => 3,
        PixelFormat::CMYK32 => 4,
    }
}

/// Parses just the header, recording the image's dimensions in the blob.
///
/// Arguments, innermost first: the decompression blob, the JPEG bytes, the
/// error record.
///
/// A malformed JPEG is **not** a primitive failure. The C plugin swallowed
/// libjpeg's longjmp and left the struct alone, and the image checks for a
/// zero width; failing the primitive here would change that contract.
#[pharo_primitive(name = "primJPEGReadHeaderfromByteArrayerrorMgr")]
fn read_header(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(3)?;
    let blob = vm.stack_value(2)?;
    let source = vm.stack_value(1)?;
    let error = vm.stack_value(0)?;

    let source_bytes = vm.bytes_of(source)?;
    if source_bytes.is_empty() {
        return finish_header(vm, blob, error, Decompress::empty());
    }

    let mut decoder = Decoder::new(source_bytes);
    let state = match decoder.read_info() {
        Ok(()) => match decoder.info() {
            Some(info) => Decompress::new(
                u32::from(info.width),
                u32::from(info.height),
                out_components(info.pixel_format),
                out_components(info.pixel_format),
            ),
            None => Decompress::empty(),
        },
        Err(_) => Decompress::empty(),
    };
    finish_header(vm, blob, error, state)
}

/// Writes the header outcome back into the image's blobs.
fn finish_header(vm: &Interp, blob: Oop, error: Oop, state: Decompress) -> PrimResult<()> {
    let failed = state.width == 0;
    vm.write_bytes(blob, 0, &state.to_bytes())?;
    vm.write_bytes(error, 0, &ErrorMgr::new(failed).to_bytes())?;
    // No explicit pop: answering `()` answers the receiver, and the VM's
    // methodReturnReceiver already pops argumentCount items -- exactly what
    // the C shim's `pop(3)` did. Popping here too would corrupt the stack.
    Ok(())
}

/// Decodes the image into a `Form`'s bitmap.
///
/// Arguments, innermost first: the decompression blob, the JPEG bytes, the
/// `Form`, the dither flag, the error record. Mirrors the argument order and
/// the validation the C plugin's generated shim performed.
#[pharo_primitive(name = "primJPEGReadImagefromByteArrayonFormdoDitheringerrorMgr")]
fn read_image(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(5)?;
    let blob = vm.stack_value(4)?;
    let source = vm.stack_value(3)?;
    let form = vm.stack_value(2)?;
    let dither = vm.boolean_value(vm.stack_value(1)?)?;
    let error = vm.stack_value(0)?;

    if !vm.is_kind_of_named(form, "Form")? {
        return Err(PrimErr::BadArgument);
    }
    if vm.bytes_of(blob)?.len() < Decompress::blob_size() {
        return Err(PrimErr::BadArgument);
    }

    let bitmap = vm.fetch_pointer(form::BITS, form)?;
    let native_depth = NativeDepth(vm.fetch_integer(form::DEPTH, form)? as i32);
    let form_width = vm.fetch_integer(form::WIDTH, form)?;
    let form_height = vm.fetch_integer(form::HEIGHT, form)?;

    if !native_depth.is_supported() || form_width <= 0 || form_height <= 0 {
        return Err(PrimErr::BadArgument);
    }
    if !vm.is_words_or_bytes(bitmap)? {
        return Err(PrimErr::BadArgument);
    }

    // Geometry, computed exactly as the C shim did.
    let form_depth = native_depth.bits();
    let form_components = if form_depth == 8 { 1 } else { 4 };
    let component_bits = if form_depth == 16 { 4 } else { 8 };
    let pixels_per_word = (32 / (form_components * component_bits)) as usize;
    let form_width = form_width as usize;
    let form_height = form_height as usize;
    let words_per_row = form_width.div_ceil(pixels_per_word);

    let bitmap_bytes =
        usize::try_from(vm.byte_size_of(bitmap)?).map_err(|_| PrimErr::BadArgument)?;
    if bitmap_bytes < words_per_row * 4 * form_height {
        return Err(PrimErr::BadArgument);
    }

    let source_bytes = vm.bytes_of(source)?;
    if source_bytes.is_empty() {
        return Err(PrimErr::BadArgument);
    }

    // Decode. A broken JPEG leaves the Form untouched and reports through the
    // error record, as the C plugin's longjmp path did.
    let mut decoder = Decoder::new(source_bytes);
    let Ok(pixels) = decoder.decode() else {
        return finish_image(vm, error, true);
    };
    let Some(info) = decoder.info() else {
        return finish_image(vm, error, true);
    };

    let occ = out_components(info.pixel_format) as usize;
    let row_stride = usize::from(info.width) * occ;
    let cfg = RowConfig {
        depth: native_depth,
        pixels_per_word,
        words_per_row,
        out_components: occ,
        dither,
    };

    // Pack row by row, writing each into the bitmap as it is produced. Rows
    // beyond the Form's height are dropped rather than overrunning it.
    let rows = usize::from(info.height).min(form_height);
    let mut words = vec![0u32; words_per_row];
    for row in 0..rows {
        let start = row * row_stride;
        let end = (start + row_stride).min(pixels.len());
        if start >= pixels.len() {
            break;
        }
        words.fill(0);
        pixels::pack_row(&pixels[start..end], row as u32, &cfg, &mut words);
        vm.write_words(bitmap, row * words_per_row, &words)?;
    }

    finish_image(vm, error, false)
}

/// Records the decode outcome and pops the arguments.
fn finish_image(vm: &Interp, error: Oop, failed: bool) -> PrimResult<()> {
    vm.write_bytes(error, 0, &ErrorMgr::new(failed).to_bytes())?;
    // As in finish_header: answering the receiver does the popping.
    Ok(())
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// Encodes a `Form` as JPEG into the destination byte array.
///
/// Arguments, innermost first: the compression blob, the destination
/// `ByteArray`, the `Form`, the quality (0-100), the progressive flag, the
/// error record. Answers the number of bytes written, or 0 if the encoded
/// image did not fit -- the same signal the C gave when its fixed-capacity
/// destination filled up.
#[pharo_primitive(name = "primJPEGWriteImageonByteArrayformqualityprogressiveJPEGerrorMgr")]
fn write_image(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(6)?;
    let blob = vm.stack_value(5)?;
    let destination = vm.stack_value(4)?;
    let form = vm.stack_value(3)?;
    let quality = vm.stack_integer(2)?;
    let progressive = vm.boolean_value(vm.stack_value(1)?)?;
    let error = vm.stack_value(0)?;

    if !vm.is_kind_of_named(form, "Form")? {
        return Err(PrimErr::BadArgument);
    }
    if vm.bytes_of(blob)?.len() < Decompress::blob_size() {
        return Err(PrimErr::BadArgument);
    }

    let bitmap = vm.fetch_pointer(form::BITS, form)?;
    let form_width = vm.fetch_integer(form::WIDTH, form)?;
    let form_height = vm.fetch_integer(form::HEIGHT, form)?;
    let native_depth = NativeDepth(vm.fetch_integer(form::DEPTH, form)? as i32);

    if !native_depth.is_supported() || form_width <= 0 || form_height <= 0 {
        return Err(PrimErr::BadArgument);
    }
    if !vm.is_words_or_bytes(bitmap)? {
        return Err(PrimErr::BadArgument);
    }

    let form_depth = native_depth.bits();
    let form_components = if form_depth == 8 { 1 } else { 4 };
    let component_bits = if form_depth == 16 { 4 } else { 8 };
    let pixels_per_word = (32 / (form_components * component_bits)) as usize;
    let width = form_width as usize;
    let height = form_height as usize;
    let words_per_row = width.div_ceil(pixels_per_word);

    let bitmap_bytes =
        usize::try_from(vm.byte_size_of(bitmap)?).map_err(|_| PrimErr::BadArgument)?;
    if bitmap_bytes < words_per_row * 4 * height {
        return Err(PrimErr::BadArgument);
    }

    // JPEG dimensions are 16-bit, and the encoder takes u16.
    let (Ok(w16), Ok(h16)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(PrimErr::LimitExceeded);
    };

    let components = encode::input_components(native_depth);
    let padded_row = words_per_row * pixels_per_word * components;
    let words = vm.words_of(bitmap)?;

    // Unpack the whole image, then encode in one go.
    let mut samples = vec![0u8; padded_row * height];
    for row in 0..height {
        let start = row * words_per_row;
        let Some(row_words) = words.get(start..start + words_per_row) else {
            return Err(PrimErr::BadIndex);
        };
        let out = &mut samples[row * padded_row..(row + 1) * padded_row];
        encode::unpack_row(row_words, native_depth, pixels_per_word, out);
    }

    // The encoder wants tightly packed rows; drop the padding pixels that the
    // Form carries when its width is not a multiple of pixels_per_word.
    let tight_row = width * components;
    if tight_row != padded_row {
        for row in 0..height {
            samples.copy_within(
                row * padded_row..row * padded_row + tight_row,
                row * tight_row,
            );
        }
    }
    samples.truncate(tight_row * height);

    let color = if components == 1 {
        ColorType::Luma
    } else {
        ColorType::Rgb
    };
    let quality = quality.clamp(0, 100) as u8;

    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, quality);
    encoder.set_progressive(progressive);
    if encoder.encode(&samples, w16, h16, color).is_err() {
        vm.write_bytes(error, 0, &ErrorMgr::new(true).to_bytes())?;
        return Ok(0);
    }

    let capacity =
        usize::try_from(vm.byte_size_of(destination)?).map_err(|_| PrimErr::BadArgument)?;
    if out.len() > capacity {
        // Would not fit. Report nothing written rather than truncating to a
        // corrupt JPEG.
        vm.write_bytes(error, 0, &ErrorMgr::new(true).to_bytes())?;
        return Ok(0);
    }

    vm.write_bytes(destination, 0, &out)?;
    vm.write_bytes(error, 0, &ErrorMgr::new(false).to_bytes())?;
    Ok(out.len() as isize)
}
