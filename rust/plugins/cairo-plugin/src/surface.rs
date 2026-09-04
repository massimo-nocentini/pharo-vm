//! Surfaces: creating them, measuring them, and moving pixels in and out.
//!
//! Two ways to make an image surface, with different trades:
//!
//! * `primitiveImageSurfaceCreate` -- Cairo allocates the pixels. Safe, and
//!   the pixels have to be copied to reach a Form.
//! * `primitiveImageSurfaceCreateForBitmap` -- Cairo draws straight into an
//!   image object, which is pinned for as long as Cairo can reach it. No copy,
//!   at the cost of one immovable object; this is what Athens needs to keep
//!   its present redraw cost.

use core::ffi::c_uchar;

use pharo_vm_plugin::handles::Handle;
use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{self, cairo, cc};
use crate::resources::{
    as_c_int, as_c_int_positive, destroy_surface, with_surface, Surface, SURFACES,
};

/// `cairo_image_surface_create`. Answers a surface handle.
///
/// `format` is a `cairo_format_t`; out-of-range values are rejected rather
/// than turned into a surface that silently ignores every later call.
#[pharo_primitive]
fn primitiveImageSurfaceCreate(
    _vm: &Interp,
    format: sqInt,
    width: sqInt,
    height: sqInt,
) -> PrimResult<Handle<Surface>> {
    let c = cairo()?;
    let format = as_c_int(format)?;
    if !ffi::is_valid_format(format) {
        return Err(PrimErr::BadArgument);
    }
    let ptr = cc!(
        c,
        cairo_image_surface_create(format, as_c_int_positive(width)?, as_c_int_positive(height)?)
    );
    SURFACES.insert(Surface::adopt(ptr, None)?)
}

/// `cairo_image_surface_create_for_data` over an image object.
///
/// `bits` must be a Bitmap, ByteArray or other word- or byte-indexable object
/// big enough for `stride * height` bytes. It is pinned here and stays pinned
/// until the surface is destroyed with no other Cairo reference outstanding --
/// see `primitiveRetainedPinCount`.
///
/// Cairo requires `stride` to be one its own arithmetic produces; the image
/// should get it from `primitiveFormatStrideForWidth` rather than computing
/// `width * 4` and hoping.
#[pharo_primitive]
fn primitiveImageSurfaceCreateForBitmap(
    vm: &Interp,
    bits: Oop,
    format: sqInt,
    width: sqInt,
    height: sqInt,
    stride: sqInt,
) -> PrimResult<Handle<Surface>> {
    let c = cairo()?;
    let format = as_c_int(format)?;
    if !ffi::is_valid_format(format) {
        return Err(PrimErr::BadArgument);
    }
    let (width, height, stride) = (
        as_c_int_positive(width)?,
        as_c_int_positive(height)?,
        as_c_int_positive(stride)?,
    );

    // Cairo will not check any of this, and getting it wrong means Cairo
    // writing past the end of an image object.
    let minimum_stride = cc!(c, cairo_format_stride_for_width(format, width));
    if minimum_stride < 0 || stride < minimum_stride {
        return Err(PrimErr::BadArgument);
    }
    let needed = usize::try_from(stride)
        .ok()
        .and_then(|s| s.checked_mul(usize::try_from(height).ok()?))
        .ok_or(PrimErr::BadArgument)?;

    let (_, available) = vm.indexable_bytes_ptr(bits)?;
    if available < needed {
        return Err(PrimErr::BadIndex);
    }
    // Pin before taking the address: pinning can move the object into old
    // space, so the address is only meaningful afterwards.
    let pinned = vm.pin_object(bits)?;
    let (ptr, available) = vm.indexable_bytes_ptr(pinned)?;
    if available < needed {
        vm.unpin_object(pinned)?;
        return Err(PrimErr::BadIndex);
    }

    let surface = cc!(
        c,
        cairo_image_surface_create_for_data(ptr.cast::<c_uchar>(), format, width, height, stride)
    );
    match Surface::adopt(surface, Some(pinned)) {
        Ok(s) => SURFACES.insert(s),
        Err(e) => {
            vm.unpin_object(pinned)?;
            Err(e)
        }
    }
}

/// `cairo_image_surface_create_from_png`.
#[pharo_primitive]
fn primitiveImageSurfaceCreateFromPng(vm: &Interp, filename: Oop) -> PrimResult<Handle<Surface>> {
    let c = cairo()?;
    let path = vm.c_string_value(filename)?;
    let ptr = cc!(c, cairo_image_surface_create_from_png(path.as_ptr()));
    SURFACES.insert(Surface::adopt(ptr, None)?)
}

/// `cairo_surface_write_to_png`. Answers the `cairo_status_t`.
#[pharo_primitive]
fn primitiveSurfaceWriteToPng(vm: &Interp, surface: sqInt, filename: Oop) -> PrimResult<i32> {
    let c = cairo()?;
    let path = vm.c_string_value(filename)?;
    with_surface(surface, |s| {
        Ok(cc!(c, cairo_surface_write_to_png(s, path.as_ptr())))
    })
}

/// `cairo_surface_destroy`. Destroying a handle twice fails the second time
/// rather than freeing twice.
#[pharo_primitive]
fn primitiveSurfaceDestroy(vm: &Interp, surface: sqInt) -> PrimResult<()> {
    destroy_surface(vm, surface)
}

/// Is this still a live surface handle?
#[pharo_primitive]
fn primitiveSurfaceIsLive(_vm: &Interp, surface: sqInt) -> PrimResult<bool> {
    Ok(SURFACES.is_live(surface))
}

/// `cairo_surface_status`.
#[pharo_primitive]
fn primitiveSurfaceStatus(_vm: &Interp, surface: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_surface(surface, |s| Ok(cc!(c, cairo_surface_status(s))))
}

/// `cairo_surface_flush`. Call before reading the pixels by any other route.
#[pharo_primitive]
fn primitiveSurfaceFlush(_vm: &Interp, surface: sqInt) -> PrimResult<()> {
    let c = cairo()?;
    with_surface(surface, |s| {
        cc!(c, cairo_surface_flush(s));
        Ok(())
    })
}

/// `cairo_surface_mark_dirty`. Call after writing the pixels by any other
/// route, or Cairo will draw over a cached view of them.
#[pharo_primitive]
fn primitiveSurfaceMarkDirty(_vm: &Interp, surface: sqInt) -> PrimResult<()> {
    let c = cairo()?;
    with_surface(surface, |s| {
        cc!(c, cairo_surface_mark_dirty(s));
        Ok(())
    })
}

/// `cairo_surface_mark_dirty_rectangle`.
#[pharo_primitive]
fn primitiveSurfaceMarkDirtyRectangle(
    _vm: &Interp,
    surface: sqInt,
    x: sqInt,
    y: sqInt,
    width: sqInt,
    height: sqInt,
) -> PrimResult<()> {
    let c = cairo()?;
    let (x, y, w, h) = (
        as_c_int(x)?,
        as_c_int(y)?,
        as_c_int_positive(width)?,
        as_c_int_positive(height)?,
    );
    with_surface(surface, |s| {
        cc!(c, cairo_surface_mark_dirty_rectangle(s, x, y, w, h));
        Ok(())
    })
}

/// `cairo_surface_set_device_offset`.
#[pharo_primitive]
fn primitiveSurfaceSetDeviceOffset(
    _vm: &Interp,
    surface: sqInt,
    x: f64,
    y: f64,
) -> PrimResult<()> {
    let c = cairo()?;
    with_surface(surface, |s| {
        cc!(c, cairo_surface_set_device_offset(s, x, y));
        Ok(())
    })
}

/// `cairo_image_surface_get_width`.
#[pharo_primitive]
fn primitiveImageSurfaceWidth(_vm: &Interp, surface: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_surface(surface, |s| Ok(cc!(c, cairo_image_surface_get_width(s))))
}

/// `cairo_image_surface_get_height`.
#[pharo_primitive]
fn primitiveImageSurfaceHeight(_vm: &Interp, surface: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_surface(surface, |s| Ok(cc!(c, cairo_image_surface_get_height(s))))
}

/// `cairo_image_surface_get_stride`.
#[pharo_primitive]
fn primitiveImageSurfaceStride(_vm: &Interp, surface: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_surface(surface, |s| Ok(cc!(c, cairo_image_surface_get_stride(s))))
}

/// `cairo_image_surface_get_format`.
#[pharo_primitive]
fn primitiveImageSurfaceFormat(_vm: &Interp, surface: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_surface(surface, |s| Ok(cc!(c, cairo_image_surface_get_format(s))))
}

/// `cairo_format_stride_for_width`. The only correct way to size a bitmap for
/// `primitiveImageSurfaceCreateForBitmap`.
#[pharo_primitive]
fn primitiveFormatStrideForWidth(_vm: &Interp, format: sqInt, width: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    let format = as_c_int(format)?;
    if !ffi::is_valid_format(format) {
        return Err(PrimErr::BadArgument);
    }
    Ok(cc!(
        c,
        cairo_format_stride_for_width(format, as_c_int_positive(width)?)
    ))
}

/// Copies a surface's pixels into an image object, packed.
///
/// The destination receives `height` rows of `width * bytesPerPixel` bytes with
/// no padding -- the shape a Form's bits are in -- whatever stride Cairo chose
/// internally. Flushes first, so a drawing that is still buffered is included.
///
/// For the copy-free alternative see `primitiveImageSurfaceCreateForBitmap`.
#[pharo_primitive]
fn primitiveImageSurfaceReadInto(vm: &Interp, surface: sqInt, bits: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let (src, stride, width, height, bpp) = with_surface(surface, |s| {
        cc!(c, cairo_surface_flush(s));
        let format = cc!(c, cairo_image_surface_get_format(s));
        let bpp = ffi::format_bytes_per_pixel(format).ok_or(PrimErr::Unsupported)?;
        let data = cc!(c, cairo_image_surface_get_data(s));
        if data.is_null() {
            return Err(PrimErr::Inappropriate); // not an image surface
        }
        Ok((
            data,
            usize::try_from(cc!(c, cairo_image_surface_get_stride(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            usize::try_from(cc!(c, cairo_image_surface_get_width(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            usize::try_from(cc!(c, cairo_image_surface_get_height(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            bpp,
        ))
    })?;

    let row_bytes = width.checked_mul(bpp).ok_or(PrimErr::LimitExceeded)?;
    let needed = row_bytes.checked_mul(height).ok_or(PrimErr::LimitExceeded)?;
    let (_, available) = vm.indexable_bytes_ptr(bits)?;
    if available < needed {
        return Err(PrimErr::BadIndex);
    }

    // Copied row by row through an owned buffer rather than written in place:
    // `write_bytes` is the SDK's only sanctioned way into an image object, and
    // it re-checks immutability and bounds for every row.
    let mut row = vec![0u8; row_bytes];
    for y in 0..height {
        // SAFETY: `src` points at `stride * height` bytes Cairo owns, and this
        // row lies inside them.
        let start = unsafe { src.add(y * stride) };
        // SAFETY: as above; `row_bytes <= stride` because Cairo's stride is
        // never narrower than a packed row.
        unsafe { core::ptr::copy_nonoverlapping(start, row.as_mut_ptr(), row_bytes) };
        vm.write_bytes(bits, y * row_bytes, &row)?;
    }
    Ok(())
}

/// Copies packed pixels from an image object into a surface.
///
/// The inverse of `primitiveImageSurfaceReadInto`, with the same packing, and
/// it marks the surface dirty afterwards so Cairo does not draw over a stale
/// cached view.
#[pharo_primitive]
fn primitiveImageSurfaceWriteFrom(vm: &Interp, surface: sqInt, bits: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let (dst, stride, width, height, bpp) = with_surface(surface, |s| {
        cc!(c, cairo_surface_flush(s));
        let format = cc!(c, cairo_image_surface_get_format(s));
        let bpp = ffi::format_bytes_per_pixel(format).ok_or(PrimErr::Unsupported)?;
        let data = cc!(c, cairo_image_surface_get_data(s));
        if data.is_null() {
            return Err(PrimErr::Inappropriate);
        }
        Ok((
            data,
            usize::try_from(cc!(c, cairo_image_surface_get_stride(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            usize::try_from(cc!(c, cairo_image_surface_get_width(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            usize::try_from(cc!(c, cairo_image_surface_get_height(s)))
                .map_err(|_| PrimErr::OperationFailed)?,
            bpp,
        ))
    })?;

    let row_bytes = width.checked_mul(bpp).ok_or(PrimErr::LimitExceeded)?;
    let needed = row_bytes.checked_mul(height).ok_or(PrimErr::LimitExceeded)?;
    let source = vm.bytes_of(bits).or_else(|_| {
        // A Bitmap is words, not bytes, and `bytes_of` rejects it. Reading it
        // as raw bytes is exactly what the pixel copy wants.
        let (ptr, len) = vm.indexable_bytes_ptr(bits)?;
        // SAFETY: `ptr` addresses `len` readable bytes of the object, and
        // nothing allocates before the borrow ends.
        Ok::<&[u8], PrimErr>(unsafe { core::slice::from_raw_parts(ptr, len) })
    })?;
    if source.len() < needed {
        return Err(PrimErr::BadIndex);
    }

    for y in 0..height {
        // SAFETY: the destination row lies inside the `stride * height` bytes
        // Cairo owns, and the source row inside the image object checked above.
        unsafe {
            core::ptr::copy_nonoverlapping(
                source.as_ptr().add(y * row_bytes),
                dst.add(y * stride),
                row_bytes,
            );
        }
    }

    with_surface(surface, |s| {
        cc!(c, cairo_surface_mark_dirty(s));
        Ok(())
    })
}
