//! Exercises the binding against a real Cairo, when there is one.
//!
//! These are the tests that would catch a mistranscribed signature, which is
//! the failure mode this crate is most exposed to: `ffi.rs` declares Cairo's
//! entry points by hand rather than generating them from `cairo.h`.
//!
//! They **skip** when Cairo cannot be loaded, which is the case on a bare
//! build machine -- the VM's Cairo arrives as a downloaded binary next to the
//! executable, and a `cargo test` run has no executable to sit beside. A CI
//! job that wants them to mean something must install Cairo or run them from
//! a built VM's directory.

use core::ffi::c_int;

use CairoPlugin::ffi::{self, cairo, CAIRO_STATUS_SUCCESS};

/// ARGB32.
const FORMAT: c_int = 0;

macro_rules! skip_without_cairo {
    () => {
        if !ffi::load() {
            eprintln!("skipping: no Cairo could be loaded");
            return;
        }
    };
}

#[test]
fn a_surface_reports_the_geometry_it_was_asked_for() {
    skip_without_cairo!();
    let c = cairo().unwrap();
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 7, 11);
        assert_eq!((c.cairo_surface_status.unwrap())(s), CAIRO_STATUS_SUCCESS);
        assert_eq!((c.cairo_image_surface_get_width.unwrap())(s), 7);
        assert_eq!((c.cairo_image_surface_get_height.unwrap())(s), 11);
        assert_eq!((c.cairo_image_surface_get_format.unwrap())(s), FORMAT);
        // Cairo pads rows; the stride is never narrower than a packed row.
        assert!((c.cairo_image_surface_get_stride.unwrap())(s) >= 7 * 4);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn painting_opaque_red_fills_every_pixel() {
    skip_without_cairo!();
    let c = cairo().unwrap();
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 4, 4);
        let cr = (c.cairo_create.unwrap())(s);
        assert_eq!((c.cairo_status.unwrap())(cr), CAIRO_STATUS_SUCCESS);

        (c.cairo_set_source_rgba.unwrap())(cr, 1.0, 0.0, 0.0, 1.0);
        (c.cairo_paint.unwrap())(cr);
        assert_eq!((c.cairo_status.unwrap())(cr), CAIRO_STATUS_SUCCESS);

        (c.cairo_surface_flush.unwrap())(s);
        let data = (c.cairo_image_surface_get_data.unwrap())(s);
        let stride = (c.cairo_image_surface_get_stride.unwrap())(s) as usize;
        assert!(!data.is_null());
        for y in 0..4usize {
            for x in 0..4usize {
                let pixel = data.add(y * stride + x * 4).cast::<u32>().read_unaligned();
                // ARGB32 is a native-endian 32-bit word, alpha in the top byte.
                assert_eq!(pixel, 0xFFFF_0000, "pixel ({x},{y})");
            }
        }

        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn a_filled_rectangle_lands_where_it_was_asked_to() {
    skip_without_cairo!();
    let c = cairo().unwrap();
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 4, 4);
        let cr = (c.cairo_create.unwrap())(s);
        (c.cairo_set_source_rgba.unwrap())(cr, 0.0, 0.0, 1.0, 1.0);
        (c.cairo_rectangle.unwrap())(cr, 0.0, 0.0, 2.0, 2.0);
        (c.cairo_fill.unwrap())(cr);
        (c.cairo_surface_flush.unwrap())(s);

        let data = (c.cairo_image_surface_get_data.unwrap())(s);
        let stride = (c.cairo_image_surface_get_stride.unwrap())(s) as usize;
        let at = |x: usize, y: usize| data.add(y * stride + x * 4).cast::<u32>().read_unaligned();
        assert_eq!(at(0, 0), 0xFF00_00FF, "inside the rectangle");
        assert_eq!(at(1, 1), 0xFF00_00FF, "inside the rectangle");
        assert_eq!(at(3, 3), 0x0000_0000, "outside it, still transparent");

        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn the_matrix_accessors_agree_on_field_order() {
    skip_without_cairo!();
    // The one struct this crate lays out itself. If `cairo_matrix_t`'s fields
    // were in the wrong order, a scale would come back as a shear.
    let c = cairo().unwrap();
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 2, 2);
        let cr = (c.cairo_create.unwrap())(s);
        (c.cairo_scale.unwrap())(cr, 2.0, 3.0);
        (c.cairo_translate.unwrap())(cr, 5.0, 7.0);

        let mut m = ffi::cairo_matrix_t::default();
        (c.cairo_get_matrix.unwrap())(cr, &mut m);
        assert_eq!(m.xx, 2.0);
        assert_eq!(m.yy, 3.0);
        assert_eq!(m.xy, 0.0);
        assert_eq!(m.yx, 0.0);
        assert_eq!(m.x0, 10.0); // 5 * 2
        assert_eq!(m.y0, 21.0); // 7 * 3

        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn user_to_device_maps_through_the_current_matrix() {
    skip_without_cairo!();
    let c = cairo().unwrap();
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 2, 2);
        let cr = (c.cairo_create.unwrap())(s);
        (c.cairo_translate.unwrap())(cr, 10.0, 20.0);
        let (mut x, mut y) = (1.0, 2.0);
        (c.cairo_user_to_device.unwrap())(cr, &mut x, &mut y);
        assert_eq!((x, y), (11.0, 22.0));

        // The distance form ignores translation, which is the distinction the
        // two primitives exist to preserve.
        let (mut dx, mut dy) = (1.0, 2.0);
        (c.cairo_user_to_device_distance.unwrap())(cr, &mut dx, &mut dy);
        assert_eq!((dx, dy), (1.0, 2.0));

        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn text_extents_are_non_zero_for_real_text() {
    skip_without_cairo!();
    let c = cairo().unwrap();
    let Some(text_extents) = c.cairo_text_extents else {
        eprintln!("skipping: no toy text API in this Cairo");
        return;
    };
    unsafe {
        let s = (c.cairo_image_surface_create.unwrap())(FORMAT, 64, 64);
        let cr = (c.cairo_create.unwrap())(s);
        (c.cairo_set_font_size.unwrap())(cr, 20.0);
        let mut e = ffi::cairo_text_extents_t::default();
        text_extents(cr, c"Pharo".as_ptr(), &mut e);
        // Without any font at all Cairo answers zeroes rather than failing, so
        // this asserts the call shape, not the font stack.
        assert!(e.width >= 0.0 && e.height >= 0.0);
        assert!(e.x_advance >= 0.0);

        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);
    }
}

#[test]
fn a_surface_over_borrowed_memory_writes_into_that_memory() {
    skip_without_cairo!();
    // The shape `primitiveImageSurfaceCreateForBitmap` uses: Cairo draws
    // straight into memory it does not own. Here the buffer is a Vec; in the
    // plugin it is a pinned image object.
    let c = cairo().unwrap();
    unsafe {
        let stride = (c.cairo_format_stride_for_width.unwrap())(FORMAT, 4);
        assert!(stride >= 16);
        let mut buffer = vec![0u8; stride as usize * 4];
        let s = (c.cairo_image_surface_create_for_data.unwrap())(
            buffer.as_mut_ptr(),
            FORMAT,
            4,
            4,
            stride,
        );
        assert_eq!((c.cairo_surface_status.unwrap())(s), CAIRO_STATUS_SUCCESS);
        let cr = (c.cairo_create.unwrap())(s);
        (c.cairo_set_source_rgba.unwrap())(cr, 0.0, 1.0, 0.0, 1.0);
        (c.cairo_paint.unwrap())(cr);
        (c.cairo_surface_flush.unwrap())(s);
        (c.cairo_destroy.unwrap())(cr);
        (c.cairo_surface_destroy.unwrap())(s);

        let first = buffer.as_ptr().cast::<u32>().read_unaligned();
        assert_eq!(first, 0xFF00_FF00);
    }
}

#[test]
fn a_bad_format_is_rejected_before_it_reaches_cairo() {
    // No Cairo needed: this is the plugin's own guard, and the reason for it
    // is that Cairo answers a "nil surface" instead of failing.
    assert!(!ffi::is_valid_format(-1));
    assert!(!ffi::is_valid_format(99));
}

#[test]
fn every_entry_point_this_plugin_declares_really_exists() {
    skip_without_cairo!();
    // The check that catches a misspelled symbol name, which would otherwise
    // be indistinguishable from an older Cairo lacking the entry point.
    //
    // `cairo_debug_reset_static_data` is genuinely optional: it is a debugging
    // aid some builds strip, and no primitive depends on it.
    let missing: Vec<&str> = cairo()
        .unwrap()
        .missing_entry_points()
        .into_iter()
        .filter(|n| *n != "cairo_debug_reset_static_data")
        .collect();
    assert!(missing.is_empty(), "not exported by this Cairo: {missing:?}");
}
