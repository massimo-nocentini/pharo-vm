//! Cairo's "toy" text API: enough to draw a string, measure it, and turn it
//! into a path.
//!
//! Deliberately only the toy API. The scaled-font and glyph interfaces are
//! where a real text stack lives, and binding them means binding
//! `cairo_glyph_t` arrays and font options as well -- worth doing, but it is
//! its own piece of work and the image's font handling would have to move with
//! it. See the README's "Not covered" section.

use std::ffi::CString;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{cairo, cairo_font_extents_t, cairo_matrix_t, cairo_text_extents_t, cc};
use crate::resources::{as_c_int, with_context};

/// Turns an image string into one C can read, rejecting an interior NUL.
///
/// An interior NUL would silently truncate the text, which is the kind of
/// difference that shows up as a rendering bug months later.
fn c_text(vm: &Interp, oop: Oop) -> PrimResult<CString> {
    CString::new(vm.string_value(oop)?).map_err(|_| PrimErr::BadArgument)
}

/// `cairo_select_font_face`. `slant` is 0..2, `weight` 0..1.
#[pharo_primitive]
fn primitiveSelectFontFace(
    vm: &Interp,
    context: sqInt,
    family: Oop,
    slant: sqInt,
    weight: sqInt,
) -> PrimResult<()> {
    let c = cairo()?;
    let family = c_text(vm, family)?;
    let slant = as_c_int(slant)?;
    let weight = as_c_int(weight)?;
    if !(0..=2).contains(&slant) || !(0..=1).contains(&weight) {
        return Err(PrimErr::BadArgument);
    }
    with_context(context, |cr| {
        cc!(c, cairo_select_font_face(cr, family.as_ptr(), slant, weight));
        Ok(())
    })
}

/// `cairo_set_font_size`.
#[pharo_primitive]
fn primitiveSetFontSize(_vm: &Interp, context: sqInt, size: f64) -> PrimResult<()> {
    let c = cairo()?;
    with_context(context, |cr| {
        cc!(c, cairo_set_font_size(cr, size));
        Ok(())
    })
}

/// `cairo_set_font_matrix`, from a 48-byte ByteArray of six doubles.
#[pharo_primitive]
fn primitiveSetFontMatrix(vm: &Interp, context: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let values = vm.read_f64s(matrix, 6)?;
    let m = cairo_matrix_t::from_slice(&values).ok_or(PrimErr::BadArgument)?;
    with_context(context, |cr| {
        cc!(c, cairo_set_font_matrix(cr, &m));
        Ok(())
    })
}

/// `cairo_show_text`, which also advances the current point.
#[pharo_primitive]
fn primitiveShowText(vm: &Interp, context: sqInt, text: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let text = c_text(vm, text)?;
    with_context(context, |cr| {
        cc!(c, cairo_show_text(cr, text.as_ptr()));
        Ok(())
    })
}

/// `cairo_text_path`, adding the text's outline to the current path instead of
/// painting it.
#[pharo_primitive]
fn primitiveTextPath(vm: &Interp, context: sqInt, text: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let text = c_text(vm, text)?;
    with_context(context, |cr| {
        cc!(c, cairo_text_path(cr, text.as_ptr()));
        Ok(())
    })
}

/// `cairo_text_extents`, into a 48-byte ByteArray of six doubles:
/// x_bearing, y_bearing, width, height, x_advance, y_advance.
#[pharo_primitive]
fn primitiveTextExtents(
    vm: &Interp,
    context: sqInt,
    text: Oop,
    extents: Oop,
) -> PrimResult<()> {
    let c = cairo()?;
    if usize::try_from(vm.byte_size_of(extents)?)? != 48 {
        return Err(PrimErr::BadArgument);
    }
    let text = c_text(vm, text)?;
    let mut e = cairo_text_extents_t::default();
    with_context(context, |cr| {
        cc!(c, cairo_text_extents(cr, text.as_ptr(), &mut e));
        Ok(())
    })?;
    vm.write_f64s(
        extents,
        &[
            e.x_bearing,
            e.y_bearing,
            e.width,
            e.height,
            e.x_advance,
            e.y_advance,
        ],
    )
}

/// `cairo_font_extents`, into a 40-byte ByteArray of five doubles:
/// ascent, descent, height, max_x_advance, max_y_advance.
#[pharo_primitive]
fn primitiveFontExtents(vm: &Interp, context: sqInt, extents: Oop) -> PrimResult<()> {
    let c = cairo()?;
    if usize::try_from(vm.byte_size_of(extents)?)? != 40 {
        return Err(PrimErr::BadArgument);
    }
    let mut e = cairo_font_extents_t::default();
    with_context(context, |cr| {
        cc!(c, cairo_font_extents(cr, &mut e));
        Ok(())
    })?;
    vm.write_f64s(
        extents,
        &[
            e.ascent,
            e.descent,
            e.height,
            e.max_x_advance,
            e.max_y_advance,
        ],
    )
}
