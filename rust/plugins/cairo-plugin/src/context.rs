//! Contexts: state, paths, painting, transformations and measuring.
//!
//! One primitive per `cairo_*` entry point, taking a context handle where the
//! C takes a `cairo_t *`. The mapping is mechanical on purpose -- an image-side
//! backend ported from the FFI binding should read the same way it did.

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{cairo, cairo_matrix_t, cc};
use crate::resources::{
    as_c_int, destroy_context, with_context, with_pattern, with_surface, Context, CONTEXTS,
};

/// Declares a primitive that takes a context handle and some `f64`s and
/// answers the receiver.
///
/// These are the bulk of Cairo's API and they differ only in name and arity,
/// so writing them out longhand would be sixty near-identical bodies for a
/// reader to check one by one.
macro_rules! context_primitive {
    ($(#[$meta:meta])* $prim:ident => $entry:ident ( $($arg:ident),* )) => {
        $(#[$meta])*
        // `cairo_curve_to` and friends genuinely take six coordinates; the
        // arity is Cairo's, not a design choice available here.
        #[allow(clippy::too_many_arguments)]
        #[pharo_primitive]
        fn $prim(_vm: &Interp, context: sqInt $(, $arg: f64)*) -> PrimResult<()> {
            let c = cairo()?;
            with_context(context, |cr| {
                cc!(c, $entry(cr $(, $arg)*));
                Ok(())
            })
        }
    };
}

// ---- lifecycle -------------------------------------------------------------

/// `cairo_create`. Answers a context handle.
///
/// The context takes its own reference on the surface, so the image may
/// destroy its surface handle first -- though it should not: the surface's
/// backing store then stays pinned, which `primitiveRetainedPinCount` counts.
#[pharo_primitive]
fn primitiveContextCreate(_vm: &Interp, surface: sqInt) -> PrimResult<sqInt> {
    let c = cairo()?;
    let ptr = with_surface(surface, |s| Ok(cc!(c, cairo_create(s))))?;
    CONTEXTS.insert(Context::adopt(ptr)?)
}

/// `cairo_destroy`.
#[pharo_primitive]
fn primitiveContextDestroy(_vm: &Interp, context: sqInt) -> PrimResult<()> {
    destroy_context(context)
}

/// Is this still a live context handle?
#[pharo_primitive]
fn primitiveContextIsLive(_vm: &Interp, context: sqInt) -> PrimResult<bool> {
    Ok(CONTEXTS.is_live(context))
}

/// `cairo_status`. Zero is success; anything else latches and every later call
/// on this context does nothing.
#[pharo_primitive]
fn primitiveContextStatus(_vm: &Interp, context: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_context(context, |cr| Ok(cc!(c, cairo_status(cr))))
}

// ---- state -----------------------------------------------------------------

/// Declares a primitive that takes only a context handle.
macro_rules! context_nullary {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        context_primitive!($(#[$meta])* $prim => $entry());
    };
}

context_nullary!(
    /// `cairo_save`.
    primitiveSave => cairo_save
);
context_nullary!(
    /// `cairo_restore`.
    primitiveRestore => cairo_restore
);
context_nullary!(
    /// `cairo_push_group`.
    primitivePushGroup => cairo_push_group
);
context_nullary!(
    /// `cairo_pop_group_to_source`.
    primitivePopGroupToSource => cairo_pop_group_to_source
);

// ---- painting --------------------------------------------------------------

context_nullary!(
    /// `cairo_paint`.
    primitivePaint => cairo_paint
);
context_primitive!(
    /// `cairo_paint_with_alpha`.
    primitivePaintWithAlpha => cairo_paint_with_alpha(alpha)
);
context_nullary!(
    /// `cairo_fill`, which clears the path.
    primitiveFill => cairo_fill
);
context_nullary!(
    /// `cairo_fill_preserve`, which keeps it.
    primitiveFillPreserve => cairo_fill_preserve
);
context_nullary!(
    /// `cairo_stroke`.
    primitiveStroke => cairo_stroke
);
context_nullary!(
    /// `cairo_stroke_preserve`.
    primitiveStrokePreserve => cairo_stroke_preserve
);
context_nullary!(
    /// `cairo_clip`.
    primitiveClip => cairo_clip
);
context_nullary!(
    /// `cairo_clip_preserve`.
    primitiveClipPreserve => cairo_clip_preserve
);
context_nullary!(
    /// `cairo_reset_clip`.
    primitiveResetClip => cairo_reset_clip
);

/// `cairo_mask`, with a pattern as the mask.
#[pharo_primitive]
fn primitiveMask(_vm: &Interp, context: sqInt, pattern: sqInt) -> PrimResult<()> {
    let c = cairo()?;
    let p = with_pattern(pattern, Ok)?;
    with_context(context, |cr| {
        cc!(c, cairo_mask(cr, p));
        Ok(())
    })
}

/// `cairo_mask_surface`.
#[pharo_primitive]
fn primitiveMaskSurface(
    _vm: &Interp,
    context: sqInt,
    surface: sqInt,
    x: f64,
    y: f64,
) -> PrimResult<()> {
    let c = cairo()?;
    let s = with_surface(surface, Ok)?;
    with_context(context, |cr| {
        cc!(c, cairo_mask_surface(cr, s, x, y));
        Ok(())
    })
}

// ---- paths -----------------------------------------------------------------

context_nullary!(
    /// `cairo_new_path`.
    primitiveNewPath => cairo_new_path
);
context_nullary!(
    /// `cairo_new_sub_path`.
    primitiveNewSubPath => cairo_new_sub_path
);
context_nullary!(
    /// `cairo_close_path`.
    primitiveClosePath => cairo_close_path
);
context_primitive!(
    /// `cairo_move_to`.
    primitiveMoveTo => cairo_move_to(x, y)
);
context_primitive!(
    /// `cairo_line_to`.
    primitiveLineTo => cairo_line_to(x, y)
);
context_primitive!(
    /// `cairo_rel_move_to`.
    primitiveRelMoveTo => cairo_rel_move_to(dx, dy)
);
context_primitive!(
    /// `cairo_rel_line_to`.
    primitiveRelLineTo => cairo_rel_line_to(dx, dy)
);
context_primitive!(
    /// `cairo_curve_to`.
    primitiveCurveTo => cairo_curve_to(x1, y1, x2, y2, x3, y3)
);
context_primitive!(
    /// `cairo_rel_curve_to`.
    primitiveRelCurveTo => cairo_rel_curve_to(dx1, dy1, dx2, dy2, dx3, dy3)
);
context_primitive!(
    /// `cairo_rectangle`.
    primitiveRectangle => cairo_rectangle(x, y, width, height)
);
context_primitive!(
    /// `cairo_arc`, sweeping in the direction of increasing angle.
    primitiveArc => cairo_arc(xc, yc, radius, angle1, angle2)
);
context_primitive!(
    /// `cairo_arc_negative`.
    primitiveArcNegative => cairo_arc_negative(xc, yc, radius, angle1, angle2)
);

// ---- sources ---------------------------------------------------------------

context_primitive!(
    /// `cairo_set_source_rgb`.
    primitiveSetSourceRgb => cairo_set_source_rgb(red, green, blue)
);
context_primitive!(
    /// `cairo_set_source_rgba`.
    primitiveSetSourceRgba => cairo_set_source_rgba(red, green, blue, alpha)
);

/// `cairo_set_source`, with a pattern.
#[pharo_primitive]
fn primitiveSetSource(_vm: &Interp, context: sqInt, pattern: sqInt) -> PrimResult<()> {
    let c = cairo()?;
    let p = with_pattern(pattern, Ok)?;
    with_context(context, |cr| {
        cc!(c, cairo_set_source(cr, p));
        Ok(())
    })
}

/// `cairo_set_source_surface`.
#[pharo_primitive]
fn primitiveSetSourceSurface(
    _vm: &Interp,
    context: sqInt,
    surface: sqInt,
    x: f64,
    y: f64,
) -> PrimResult<()> {
    let c = cairo()?;
    let s = with_surface(surface, Ok)?;
    with_context(context, |cr| {
        cc!(c, cairo_set_source_surface(cr, s, x, y));
        Ok(())
    })
}

// ---- graphics state --------------------------------------------------------

context_primitive!(
    /// `cairo_set_line_width`.
    primitiveSetLineWidth => cairo_set_line_width(width)
);
context_primitive!(
    /// `cairo_set_miter_limit`.
    primitiveSetMiterLimit => cairo_set_miter_limit(limit)
);
context_primitive!(
    /// `cairo_set_tolerance`.
    primitiveSetTolerance => cairo_set_tolerance(tolerance)
);

/// `cairo_get_line_width`.
#[pharo_primitive]
fn primitiveGetLineWidth(_vm: &Interp, context: sqInt) -> PrimResult<f64> {
    let c = cairo()?;
    with_context(context, |cr| Ok(cc!(c, cairo_get_line_width(cr))))
}

/// Declares a primitive that sets one of Cairo's enumerated state values.
///
/// The value is range-checked before it reaches Cairo: Cairo takes these as a
/// plain `int` and puts the context into a permanent error state when handed
/// one it does not recognise, which the image would only notice as a drawing
/// that stopped appearing.
macro_rules! context_enum {
    ($(#[$meta:meta])* $prim:ident => $entry:ident, max = $max:literal) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(_vm: &Interp, context: sqInt, value: sqInt) -> PrimResult<()> {
            let c = cairo()?;
            let value = as_c_int(value)?;
            if !(0..=$max).contains(&value) {
                return Err(PrimErr::BadArgument);
            }
            with_context(context, |cr| {
                cc!(c, $entry(cr, value));
                Ok(())
            })
        }
    };
}

context_enum!(
    /// `cairo_set_line_cap`: butt, round, square.
    primitiveSetLineCap => cairo_set_line_cap, max = 2
);
context_enum!(
    /// `cairo_set_line_join`: miter, round, bevel.
    primitiveSetLineJoin => cairo_set_line_join, max = 2
);
context_enum!(
    /// `cairo_set_fill_rule`: winding, even-odd.
    primitiveSetFillRule => cairo_set_fill_rule, max = 1
);
context_enum!(
    /// `cairo_set_antialias`: default, none, gray, subpixel, fast, good, best.
    primitiveSetAntialias => cairo_set_antialias, max = 6
);
context_enum!(
    /// `cairo_set_operator`, from `CAIRO_OPERATOR_CLEAR` to
    /// `CAIRO_OPERATOR_HSL_LUMINOSITY`.
    primitiveSetOperator => cairo_set_operator, max = 28
);

/// `cairo_set_dash`. `dashes` is a ByteArray of native-endian doubles, one per
/// dash length; an empty one turns dashing off.
#[pharo_primitive]
fn primitiveSetDash(vm: &Interp, context: sqInt, dashes: Oop, offset: f64) -> PrimResult<()> {
    let c = cairo()?;
    let byte_size = usize::try_from(vm.byte_size_of(dashes)?)?;
    if byte_size % 8 != 0 {
        return Err(PrimErr::BadArgument);
    }
    let lengths = vm.read_f64s(dashes, byte_size / 8)?;
    // Cairo rejects a negative dash length by latching an error; say so here.
    if lengths.iter().any(|d| *d < 0.0 || !d.is_finite()) {
        return Err(PrimErr::BadArgument);
    }
    let count = as_c_int(sqInt::try_from(lengths.len()).map_err(|_| PrimErr::LimitExceeded)?)?;
    with_context(context, |cr| {
        cc!(c, cairo_set_dash(cr, lengths.as_ptr(), count, offset));
        Ok(())
    })
}

// ---- transformations -------------------------------------------------------

context_primitive!(
    /// `cairo_translate`.
    primitiveTranslate => cairo_translate(tx, ty)
);
context_primitive!(
    /// `cairo_scale`.
    primitiveScale => cairo_scale(sx, sy)
);
context_primitive!(
    /// `cairo_rotate`, in radians.
    primitiveRotate => cairo_rotate(angle)
);
context_nullary!(
    /// `cairo_identity_matrix`.
    primitiveIdentityMatrix => cairo_identity_matrix
);

/// Reads a `cairo_matrix_t` from a 48-byte ByteArray of native-endian doubles,
/// in the header's field order: xx, yx, xy, yy, x0, y0.
fn matrix_from(vm: &Interp, oop: Oop) -> PrimResult<cairo_matrix_t> {
    cairo_matrix_t::from_slice(&vm.read_f64_array::<6>(oop)?).ok_or(PrimErr::BadArgument)
}

/// `cairo_transform`, composing with the current matrix.
#[pharo_primitive]
fn primitiveTransform(vm: &Interp, context: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let m = matrix_from(vm, matrix)?;
    with_context(context, |cr| {
        cc!(c, cairo_transform(cr, &m));
        Ok(())
    })
}

/// `cairo_set_matrix`, replacing the current matrix.
#[pharo_primitive]
fn primitiveSetMatrix(vm: &Interp, context: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let m = matrix_from(vm, matrix)?;
    with_context(context, |cr| {
        cc!(c, cairo_set_matrix(cr, &m));
        Ok(())
    })
}

/// `cairo_get_matrix`, writing six doubles into the 48-byte ByteArray given.
#[pharo_primitive]
fn primitiveGetMatrix(vm: &Interp, context: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    // Checked before the call so a wrongly sized buffer fails without having
    // moved the context at all.
    if usize::try_from(vm.byte_size_of(matrix)?)? != 48 {
        return Err(PrimErr::BadArgument);
    }
    let mut m = cairo_matrix_t::default();
    with_context(context, |cr| {
        cc!(c, cairo_get_matrix(cr, &mut m));
        Ok(())
    })?;
    vm.write_f64s(matrix, &m.to_array())
}

/// Declares a primitive that maps a point or a distance through the context's
/// matrix, in place, in a 16-byte ByteArray of two doubles.
macro_rules! context_map_point {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(vm: &Interp, context: sqInt, point: Oop) -> PrimResult<()> {
            let c = cairo()?;
            let [mut x, mut y] = vm.read_f64_array::<2>(point)?;
            with_context(context, |cr| {
                cc!(c, $entry(cr, &mut x, &mut y));
                Ok(())
            })?;
            vm.write_f64s(point, &[x, y])
        }
    };
}

context_map_point!(
    /// `cairo_user_to_device`.
    primitiveUserToDevice => cairo_user_to_device
);
context_map_point!(
    /// `cairo_device_to_user`.
    primitiveDeviceToUser => cairo_device_to_user
);
context_map_point!(
    /// `cairo_user_to_device_distance`, which ignores translation.
    primitiveUserToDeviceDistance => cairo_user_to_device_distance
);
context_map_point!(
    /// `cairo_device_to_user_distance`.
    primitiveDeviceToUserDistance => cairo_device_to_user_distance
);

// ---- measuring -------------------------------------------------------------

/// Declares a primitive answering four doubles -- x1, y1, x2, y2 -- into a
/// 32-byte ByteArray.
macro_rules! context_extents {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(vm: &Interp, context: sqInt, extents: Oop) -> PrimResult<()> {
            let c = cairo()?;
            if usize::try_from(vm.byte_size_of(extents)?)? != 32 {
                return Err(PrimErr::BadArgument);
            }
            let (mut x1, mut y1, mut x2, mut y2) = (0.0, 0.0, 0.0, 0.0);
            with_context(context, |cr| {
                cc!(
                    c,
                    $entry(cr, &mut x1, &mut y1, &mut x2, &mut y2)
                );
                Ok(())
            })?;
            vm.write_f64s(extents, &[x1, y1, x2, y2])
        }
    };
}

context_extents!(
    /// `cairo_path_extents`.
    primitivePathExtents => cairo_path_extents
);
context_extents!(
    /// `cairo_fill_extents`.
    primitiveFillExtents => cairo_fill_extents
);
context_extents!(
    /// `cairo_stroke_extents`.
    primitiveStrokeExtents => cairo_stroke_extents
);
context_extents!(
    /// `cairo_clip_extents`.
    primitiveClipExtents => cairo_clip_extents
);

/// Declares a hit test: context, x, y, answering a boolean.
macro_rules! context_hit_test {
    ($(#[$meta:meta])* $prim:ident => $entry:ident) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(_vm: &Interp, context: sqInt, x: f64, y: f64) -> PrimResult<bool> {
            let c = cairo()?;
            with_context(context, |cr| Ok(cc!(c, $entry(cr, x, y)) != 0))
        }
    };
}

context_hit_test!(
    /// `cairo_in_fill`.
    primitiveInFill => cairo_in_fill
);
context_hit_test!(
    /// `cairo_in_stroke`.
    primitiveInStroke => cairo_in_stroke
);
context_hit_test!(
    /// `cairo_in_clip`.
    primitiveInClip => cairo_in_clip
);
