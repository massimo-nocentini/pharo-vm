//! `pango_cairo_*`: drawing a layout onto one of CairoPlugin's contexts.
//!
//! Every primitive here that takes a Cairo context borrows it from CairoPlugin
//! for the duration of exactly one primitive, through
//! [`crate::cairo_bridge::with_cairo_context`]. Drawing with the state the
//! image's own context already carries -- its current point, source, transform
//! and clip -- is the entire reason for going through the bridge rather than
//! making a context of our own. A plugin that opened its own Cairo and took a
//! *surface* handle could not compose with any of that, which is the only
//! reason to use Pango and Cairo together at all.
//!
//! Consequently: **the image sets the current point first**, through
//! CairoPlugin, and these draw there. Nothing in this module moves the pen, sets
//! a source, or saves and restores the context.
//!
//! Units. The two coordinate systems meet here and they are not the same one.
//! Everything Pango answers elsewhere in this plugin is in Pango units
//! (`PANGO_SCALE` of them to the device unit); everything crossing into Cairo
//! here -- the underline rectangles, the resolution -- is in Cairo's own
//! user-space units, as doubles, because these are Cairo drawing calls and the
//! coordinates are the context's. Each primitive below says which it speaks.
//!
//! Two of the twelve, the resolution accessors, touch no Cairo context at all:
//! they configure a `PangoContext` and so need no bridge, no borrow and no
//! CairoPlugin.

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, PrimErr, PrimResult};

use crate::cairo_bridge::with_cairo_context;
use crate::ffi::{self, cairo_t, pango};
use pharo_vm_plugin::handles::Handle;

use crate::resources::{
    register_context, register_layout, with_context, with_layout, with_line, Context, Layout,
};

// ---- shared helpers ------------------------------------------------------

/// Checks the rectangle both error-underline entry points are given.
///
/// Both `pango_cairo_show_error_underline` and
/// `pango_cairo_error_underline_path` open with
/// `g_return_if_fail ((width >= 0) && (height >= 0))` -- read out of the
/// shipped `libpangocairo`, and measured. A negative width draws nothing,
/// answers nothing, and writes one line to stderr; under
/// `G_DEBUG=fatal-criticals` the same assertion calls `abort()` and the image
/// goes down with it. A NaN takes the same branch, because a comparison
/// against NaN is false either way round.
///
/// So the two are checked here rather than each on its own: they are one
/// rectangle contract, and a check written twice is a check that drifts. `x`
/// and `y` are unconstrained and are not checked.
fn checked_underline_extent(width: f64, height: f64) -> PrimResult<()> {
    if !width.is_finite() || !height.is_finite() || width < 0.0 || height < 0.0 {
        return Err(PrimErr::BadArgument);
    }
    Ok(())
}

// ---- making Pango objects from a Cairo context ---------------------------

/// `pango_cairo_create_layout`. Answers a new layout handle.
///
/// The layout is set up to match the context's *current* transformation and
/// target surface, and it carries a `PangoContext` of its own -- one per
/// layout, which is slightly wasteful for an image laying out a lot of text and
/// is why `primitiveContextCreateLayout` on a shared context exists beside it.
/// Change the transform or the target surface afterwards and the layout must be
/// told, with [`primitiveCairoUpdateLayout`].
///
/// Transfer full: the handle owns a reference, and
/// `primitiveLayoutDestroy` releases it.
#[pharo_primitive]
fn primitiveCairoCreateLayout(vm: &Interp, context: sqInt) -> PrimResult<Handle<Layout>> {
    let p = pango()?;
    with_cairo_context(vm, context, |cr| {
        let layout = ffi::pg!(
            p,
            pango_cairo_create_layout(cr.as_ptr().cast::<cairo_t>())
        );
        register_layout(layout)
    })
}

/// `pango_cairo_create_context`. Answers a new context handle.
///
/// The context to make several layouts from, so the per-layout `PangoContext`
/// of [`primitiveCairoCreateLayout`] is paid for once. Like a layout it
/// snapshots the Cairo context's transform and target surface, so it too has to
/// be updated with [`primitiveCairoUpdateContext`] when either changes.
///
/// Transfer full: the handle owns a reference. Note that destroying a context
/// under live layouts is the image's business, not the plugin's -- Pango's own
/// reference from each layout keeps the object alive.
#[pharo_primitive]
fn primitiveCairoCreateContext(vm: &Interp, context: sqInt) -> PrimResult<Handle<Context>> {
    let p = pango()?;
    with_cairo_context(vm, context, |cr| {
        let ctx = ffi::pg!(
            p,
            pango_cairo_create_context(cr.as_ptr().cast::<cairo_t>())
        );
        register_context(ctx)
    })
}

// ---- keeping them in step with the context -------------------------------

/// `pango_cairo_update_layout`. Re-reads the Cairo context's transform and
/// target surface into the layout's private `PangoContext`.
///
/// Needed after the image changes either through CairoPlugin. Skipping it does
/// not fail: it lays the text out for the transform that was current when the
/// layout was made, and hinting and metrics come out subtly wrong, which is
/// harder to notice than an error would be.
///
/// Only for a layout that came from [`primitiveCairoCreateLayout`]; a layout
/// made on a shared context has no private context to update, and
/// [`primitiveCairoUpdateContext`] is the call for that one.
#[pharo_primitive]
fn primitiveCairoUpdateLayout(vm: &Interp, context: sqInt, layout: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_layout(layout, |l| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_update_layout(cr.as_ptr().cast::<cairo_t>(), l)
            );
            Ok(())
        })
    })
}

/// `pango_cairo_update_context`. Re-reads the Cairo context's transform,
/// target surface and font options into a `PangoContext`.
///
/// The shared-context counterpart of [`primitiveCairoUpdateLayout`]. Layouts
/// made on this context see the change the next time they are laid out.
#[pharo_primitive]
fn primitiveCairoUpdateContext(vm: &Interp, context: sqInt, pango_context: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_context(pango_context, |ctx| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_update_context(cr.as_ptr().cast::<cairo_t>(), ctx)
            );
            Ok(())
        })
    })
}

// ---- drawing -------------------------------------------------------------

/// `pango_cairo_show_layout`. Draws the layout at the context's current point.
///
/// The top-left corner of the layout goes at the current point, and the text is
/// painted with the context's current source. The image sets the point, source,
/// transform and clip through CairoPlugin first.
///
/// Fails `Inappropriate` when the context has already latched a Cairo error --
/// drawing on one of those silently paints nothing -- and `OperationFailed`
/// when this call is what latched it.
#[pharo_primitive]
fn primitiveCairoShowLayout(vm: &Interp, context: sqInt, layout: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_layout(layout, |l| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_show_layout(cr.as_ptr().cast::<cairo_t>(), l)
            );
            Ok(())
        })
    })
}

/// `pango_cairo_show_layout_line`. Draws one line of a layout.
///
/// `index` is a zero-based line number in `layout`, range-checked against the
/// layout's line count; out of range is `BadIndex`. The **left edge of the
/// line's baseline** goes at the current point -- not its top-left corner, which
/// is where [`primitiveCairoShowLayout`] puts a whole layout. An image drawing
/// line by line has to advance the pen by the line's own metrics rather than by
/// its logical height.
///
/// The line is borrowed from the layout for the duration of this primitive and
/// is never freed here.
#[pharo_primitive]
fn primitiveCairoShowLayoutLine(
    vm: &Interp,
    context: sqInt,
    layout: sqInt,
    index: sqInt,
) -> PrimResult<()> {
    let p = pango()?;
    with_line(layout, index, |line| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_show_layout_line(cr.as_ptr().cast::<cairo_t>(), line)
            );
            Ok(())
        })
    })
}

/// `pango_cairo_layout_path`. Adds the layout's glyph outlines to the current
/// path instead of painting them.
///
/// For stroking text, clipping to it, or filling it with a gradient: the image
/// follows this with `cairo_fill`, `cairo_stroke` or `cairo_clip` through
/// CairoPlugin. Nothing is drawn by this primitive itself, so a context whose
/// source or line width is not yet set is fine here.
#[pharo_primitive]
fn primitiveCairoLayoutPath(vm: &Interp, context: sqInt, layout: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_layout(layout, |l| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_layout_path(cr.as_ptr().cast::<cairo_t>(), l)
            );
            Ok(())
        })
    })
}

/// `pango_cairo_layout_line_path`. The one-line counterpart of
/// [`primitiveCairoLayoutPath`], positioned on the baseline as
/// [`primitiveCairoShowLayoutLine`] is.
///
/// `index` is a zero-based line number, range-checked; out of range is
/// `BadIndex`.
#[pharo_primitive]
fn primitiveCairoLayoutLinePath(
    vm: &Interp,
    context: sqInt,
    layout: sqInt,
    index: sqInt,
) -> PrimResult<()> {
    let p = pango()?;
    with_line(layout, index, |line| {
        with_cairo_context(vm, context, |cr| {
            ffi::pg!(
                p,
                pango_cairo_layout_line_path(cr.as_ptr().cast::<cairo_t>(), line)
            );
            Ok(())
        })
    })
}

/// `pango_cairo_show_error_underline`. Paints the squiggly line that marks a
/// spelling error, covering the given rectangle.
///
/// `x`, `y`, `width` and `height` are **Cairo user-space units**, doubles, not
/// Pango units: this is a Cairo drawing call and the rectangle is in the
/// context's own coordinates. An image that has a rectangle from
/// `primitiveLayoutIndexToPos` must divide by `primitiveScale` first.
///
/// Pango rounds the width to a whole number of up/down segments and centres the
/// result in the rectangle asked for, so the squiggle is not exactly the
/// rectangle given. `width` and `height` must be non-negative and finite:
/// Pango asserts on the first and mishandles the second, so both are
/// `BadArgument` here and the Cairo context is never even borrowed. See
/// [`checked_underline_extent`].
#[pharo_primitive]
fn primitiveCairoShowErrorUnderline(
    vm: &Interp,
    context: sqInt,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> PrimResult<()> {
    let p = pango()?;
    checked_underline_extent(width, height)?;
    with_cairo_context(vm, context, |cr| {
        ffi::pg!(
            p,
            pango_cairo_show_error_underline(cr.as_ptr().cast::<cairo_t>(), x, y, width, height)
        );
        Ok(())
    })
}

/// `pango_cairo_error_underline_path`. The same squiggle, added to the current
/// path instead of painted.
///
/// Units as in [`primitiveCairoShowErrorUnderline`]: Cairo user-space doubles,
/// with the same non-negative, finite `width` and `height`.
#[pharo_primitive]
fn primitiveCairoErrorUnderlinePath(
    vm: &Interp,
    context: sqInt,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> PrimResult<()> {
    let p = pango()?;
    checked_underline_extent(width, height)?;
    with_cairo_context(vm, context, |cr| {
        ffi::pg!(
            p,
            pango_cairo_error_underline_path(cr.as_ptr().cast::<cairo_t>(), x, y, width, height)
        );
        Ok(())
    })
}

// ---- resolution: a PangoContext setting, no Cairo context involved -------

/// `pango_cairo_context_set_resolution`. Sets the context's resolution in dots
/// per inch.
///
/// The scale between the *points* a `PangoFontDescription` names and Cairo's
/// user-space units: at the default 96, a 10-point font comes out 13.3 units
/// high (10 * 96 / 72). Zero or negative means "use the font map's resolution",
/// which is how the image asks for the default back.
///
/// Takes no Cairo context, so it needs neither CairoPlugin nor the bridge --
/// only a `PangoContext` that came from a pangocairo font map, which every
/// context this plugin can make does.
#[pharo_primitive]
fn primitiveCairoContextSetResolution(_vm: &Interp, context: sqInt, dpi: f64) -> PrimResult<()> {
    let p = pango()?;
    with_context(context, |ctx| {
        ffi::pg!(p, pango_cairo_context_set_resolution(ctx, dpi));
        Ok(())
    })
}

/// `pango_cairo_context_get_resolution`. The context's resolution in dots per
/// inch.
///
/// **Negative when no resolution has been set on this context**, which is not
/// an error and is Pango's way of saying "the font map's". The image must test
/// for that rather than treating the answer as a DPI.
#[pharo_primitive]
fn primitiveCairoContextGetResolution(_vm: &Interp, context: sqInt) -> PrimResult<f64> {
    let p = pango()?;
    with_context(context, |ctx| {
        Ok(ffi::pg!(p, pango_cairo_context_get_resolution(ctx)))
    })
}
