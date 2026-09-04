//! Patterns: solid colours, gradients, and surfaces used as paint.

use pharo_vm_plugin::handles::Handle;
use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{cairo, cairo_matrix_t, cc};
use crate::resources::{as_c_int, destroy_pattern, with_pattern, with_surface, Pattern, PATTERNS};

/// Declares a pattern constructor taking only `f64`s.
macro_rules! pattern_constructor {
    ($(#[$meta:meta])* $prim:ident => $entry:ident ( $($arg:ident),* )) => {
        $(#[$meta])*
        #[allow(clippy::too_many_arguments)]
        #[pharo_primitive]
        fn $prim(_vm: &Interp $(, $arg: f64)*) -> PrimResult<Handle<Pattern>> {
            let c = cairo()?;
            let ptr = cc!(c, $entry($($arg),*));
            PATTERNS.insert(Pattern::adopt(ptr)?)
        }
    };
}

pattern_constructor!(
    /// `cairo_pattern_create_rgb`.
    primitivePatternCreateRgb => cairo_pattern_create_rgb(red, green, blue)
);
pattern_constructor!(
    /// `cairo_pattern_create_rgba`.
    primitivePatternCreateRgba => cairo_pattern_create_rgba(red, green, blue, alpha)
);
pattern_constructor!(
    /// `cairo_pattern_create_linear`, from one point to another.
    primitivePatternCreateLinear => cairo_pattern_create_linear(x0, y0, x1, y1)
);
pattern_constructor!(
    /// `cairo_pattern_create_radial`, between two circles.
    primitivePatternCreateRadial =>
        cairo_pattern_create_radial(cx0, cy0, radius0, cx1, cy1, radius1)
);

/// `cairo_pattern_create_for_surface`.
///
/// The pattern takes its own reference on the surface, so the surface outlives
/// the image's handle on it -- which is why destroying a surface still
/// referenced this way keeps its backing store pinned.
#[pharo_primitive]
fn primitivePatternCreateForSurface(_vm: &Interp, surface: sqInt) -> PrimResult<Handle<Pattern>> {
    let c = cairo()?;
    let ptr = with_surface(surface, |s| Ok(cc!(c, cairo_pattern_create_for_surface(s))))?;
    PATTERNS.insert(Pattern::adopt(ptr)?)
}

/// `cairo_pattern_destroy`.
#[pharo_primitive]
fn primitivePatternDestroy(_vm: &Interp, pattern: sqInt) -> PrimResult<()> {
    destroy_pattern(pattern)
}

/// Is this still a live pattern handle?
#[pharo_primitive]
fn primitivePatternIsLive(_vm: &Interp, pattern: sqInt) -> PrimResult<bool> {
    Ok(PATTERNS.is_live(pattern))
}

/// `cairo_pattern_status`.
#[pharo_primitive]
fn primitivePatternStatus(_vm: &Interp, pattern: sqInt) -> PrimResult<i32> {
    let c = cairo()?;
    with_pattern(pattern, |p| Ok(cc!(c, cairo_pattern_status(p))))
}

/// `cairo_pattern_add_color_stop_rgba`. `offset` runs from 0.0 to 1.0.
#[pharo_primitive]
#[allow(clippy::too_many_arguments)]
fn primitivePatternAddColorStopRgba(
    _vm: &Interp,
    pattern: sqInt,
    offset: f64,
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
) -> PrimResult<()> {
    let c = cairo()?;
    with_pattern(pattern, |p| {
        cc!(
            c,
            cairo_pattern_add_color_stop_rgba(p, offset, red, green, blue, alpha)
        );
        Ok(())
    })
}

/// Declares a primitive setting one of a pattern's enumerated properties,
/// range-checked before Cairo can latch an error on it.
macro_rules! pattern_enum {
    ($(#[$meta:meta])* $prim:ident => $entry:ident, max = $max:literal) => {
        $(#[$meta])*
        #[pharo_primitive]
        fn $prim(_vm: &Interp, pattern: sqInt, value: sqInt) -> PrimResult<()> {
            let c = cairo()?;
            let value = as_c_int(value)?;
            if !(0..=$max).contains(&value) {
                return Err(PrimErr::BadArgument);
            }
            with_pattern(pattern, |p| {
                cc!(c, $entry(p, value));
                Ok(())
            })
        }
    };
}

pattern_enum!(
    /// `cairo_pattern_set_extend`: none, repeat, reflect, pad.
    primitivePatternSetExtend => cairo_pattern_set_extend, max = 3
);
pattern_enum!(
    /// `cairo_pattern_set_filter`: fast, good, best, nearest, bilinear,
    /// gaussian.
    primitivePatternSetFilter => cairo_pattern_set_filter, max = 5
);

/// `cairo_pattern_set_matrix`, from a 48-byte ByteArray of six doubles.
///
/// Note Cairo's direction: this is the pattern-to-user matrix *inverted*, so
/// scaling it by 2 halves the pattern rather than doubling it. The image-side
/// backend has to keep doing whatever it did before.
#[pharo_primitive]
fn primitivePatternSetMatrix(vm: &Interp, pattern: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    let m =
        cairo_matrix_t::from_slice(&vm.read_f64_array::<6>(matrix)?).ok_or(PrimErr::BadArgument)?;
    with_pattern(pattern, |p| {
        cc!(c, cairo_pattern_set_matrix(p, &m));
        Ok(())
    })
}

/// `cairo_pattern_get_matrix`, into a 48-byte ByteArray.
#[pharo_primitive]
fn primitivePatternGetMatrix(vm: &Interp, pattern: sqInt, matrix: Oop) -> PrimResult<()> {
    let c = cairo()?;
    if usize::try_from(vm.byte_size_of(matrix)?)? != 48 {
        return Err(PrimErr::BadArgument);
    }
    let mut m = cairo_matrix_t::default();
    with_pattern(pattern, |p| {
        cc!(c, cairo_pattern_get_matrix(p, &mut m));
        Ok(())
    })?;
    vm.write_f64s(matrix, &m.to_array())
}
