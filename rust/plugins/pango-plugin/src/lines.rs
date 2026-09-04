//! Line measurement, by `(layout, lineIndex)`.
//!
//! No `PangoLayoutLine *` ever reaches the image, and that is the single most
//! important structural decision in this crate. A line is invalidated by *any*
//! change to its layout (pango-layout.h:131-133), and a registry handle cannot
//! express that: the handle would stay live, its generation would still match,
//! and the plugin would dereference memory Pango had freed -- inside the VM,
//! with no Smalltalk fallback. A `Registry` guards stale *handles*, not stale
//! *pointees*. So every primitive here is addressed `(layout, lineIndex)`,
//! resolves its line through [`crate::resources::with_line`], and lets the
//! pointer die with the call. The same argument retires `PangoLayoutIter` from
//! v1.
//!
//! `with_line` is also where the two safety rules of this module live: it
//! range-checks the index against `pango_layout_get_line_count` before every
//! call -- Pango's own accessor answers NULL rather than failing, and a NULL
//! deref is what an unchecked index costs -- and it uses
//! `pango_layout_get_line_readonly`, never `pango_layout_get_line`. The
//! non-readonly form marks the layout dirty, so merely *measuring* a line
//! would force the next `get_extents` to lay the text out again.
//!
//! Units, since every integer here is one of three things: byte offsets from
//! `get_start_index`/`get_length`, **pixels** from `LineGetPixelExtents` alone,
//! and **Pango units** from everything else.

use core::ffi::c_int;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{self, PangoRectangle};
use crate::resources::{
    as_c_int, as_c_int_positive, as_gboolean, from_gboolean, int_array, oop_array,
    rect_pair_array, with_line,
};

// ---- position within the layout's text ----------------------------------

/// `pango_layout_line_get_start_index`: where this line begins, as a **byte**
/// offset into the UTF-8 the plugin handed Pango.
///
/// Bytes, not characters and not Pango units. The image cannot index its own
/// ByteString with this -- a Pharo String is Latin-1 and Pango holds a UTF-8
/// copy -- and must index against `primitiveLayoutGetText`.
///
/// Pango 1.50+; older installations answer `Unsupported`.
#[pharo_primitive]
fn primitiveLineStartIndex(_vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    with_line(layout, line, |l| {
        Ok(ffi::pg!(p, pango_layout_line_get_start_index(l)) as sqInt)
    })
}

/// `pango_layout_line_get_length`: the line's length in **bytes** of UTF-8.
///
/// Pango 1.50+.
#[pharo_primitive]
fn primitiveLineLength(_vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    with_line(layout, line, |l| {
        Ok(ffi::pg!(p, pango_layout_line_get_length(l)) as sqInt)
    })
}

/// `pango_layout_line_is_paragraph_start`: is this line the first of its
/// paragraph, rather than a continuation produced by wrapping?
///
/// The answer is a `gboolean` -- four bytes, never a Rust `bool` -- and is
/// converted here so the image only ever sees a Boolean.
///
/// Pango 1.50+.
#[pharo_primitive]
fn primitiveLineIsParagraphStart(_vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<bool> {
    let p = ffi::pango()?;
    with_line(layout, line, |l| {
        Ok(from_gboolean(ffi::pg!(
            p,
            pango_layout_line_is_paragraph_start(l)
        )))
    })
}

/// `pango_layout_line_get_resolved_direction`: the `PangoDirection` the
/// bidirectional algorithm settled on for this line.
///
/// An output, so it needs no range check on the way in; it is one of the
/// direction constants the image already knows from
/// `primitiveLayoutDirectionAtIndex`.
///
/// Pango 1.50+.
#[pharo_primitive]
fn primitiveLineResolvedDirection(_vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    with_line(layout, line, |l| {
        Ok(ffi::pg!(p, pango_layout_line_get_resolved_direction(l)) as sqInt)
    })
}

// ---- extents ------------------------------------------------------------

/// `pango_layout_line_get_extents`: ink and logical rectangles as one
/// 8-element Array, `{inkX. inkY. inkW. inkH. logX. logY. logW. logH}`, in
/// **Pango units**.
///
/// Both rectangles in one call, and one primitive rather than two, so the
/// image cannot pair an ink rectangle with a logical one measured after the
/// layout changed underneath it. Both out pointers are passed non-NULL even
/// though the gir marks them optional: those annotations are newer than the
/// function, and a NULL an older Pango does not guard for is a crash where a
/// stack rectangle costs nothing.
#[pharo_primitive]
fn primitiveLineGetExtents(vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<Oop> {
    let p = ffi::pango()?;
    let mut ink = PangoRectangle::default();
    let mut logical = PangoRectangle::default();
    with_line(layout, line, |l| {
        ffi::pg!(p, pango_layout_line_get_extents(l, &mut ink, &mut logical));
        Ok(())
    })?;
    rect_pair_array(vm, &ink, &logical)
}

/// `pango_layout_line_get_pixel_extents`: the same pair, in **device pixels**.
///
/// One of exactly three functions in this plugin that speaks pixels. It is
/// *not* `PANGO_PIXELS` applied to [`primitiveLineGetExtents`]: Pango rounds
/// this pair **outwards** so the rectangle still covers every pixel the line
/// touches, whereas `PANGO_PIXELS` rounds to nearest. Implementing either from
/// the other loses a pixel of ink at the edges.
#[pharo_primitive]
fn primitiveLineGetPixelExtents(vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<Oop> {
    let p = ffi::pango()?;
    let mut ink = PangoRectangle::default();
    let mut logical = PangoRectangle::default();
    with_line(layout, line, |l| {
        ffi::pg!(
            p,
            pango_layout_line_get_pixel_extents(l, &mut ink, &mut logical)
        );
        Ok(())
    })?;
    rect_pair_array(vm, &ink, &logical)
}

/// `pango_layout_line_get_height`: the height this line occupies, in **Pango
/// units**.
///
/// Answers through an out parameter, not a return value. Pango 1.44+; older
/// installations answer `Unsupported`, and the image's fallback is the height
/// of the logical rectangle from [`primitiveLineGetExtents`].
#[pharo_primitive]
fn primitiveLineGetHeight(_vm: &Interp, layout: sqInt, line: sqInt) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    let mut height: c_int = 0;
    with_line(layout, line, |l| {
        ffi::pg!(p, pango_layout_line_get_height(l, &mut height));
        Ok(())
    })?;
    Ok(height as sqInt)
}

// ---- hit-testing --------------------------------------------------------

/// `pango_layout_line_x_to_index`: the 3-element Array
/// `{hit. byteIndex. trailing}` for an X position in **Pango units**, measured
/// from the left edge of the line.
///
/// The C function answers a `gboolean`, *not* the index -- misreading the
/// return value as the index "works", because 0 and 1 are both plausible byte
/// offsets, which is what makes the mistake survive review. The index arrives
/// through `int *index_`.
///
/// A `FALSE` answer means "the X was outside the line, so the result was
/// clamped to its start or end", **not** "failed": both out parameters are
/// valid either way. Failing the primitive on `FALSE` would break clicking
/// past the end of a line, which is the commonest editor gesture there is, so
/// `hit` is reported as the first element and the primitive never fails on it.
///
/// `trailing` is a **character count** into the grapheme that was hit -- zero
/// for its leading edge -- not a Boolean, despite the name it shares with
/// [`primitiveLineIndexToX`]'s Boolean input.
#[pharo_primitive]
fn primitiveLineXToIndex(vm: &Interp, layout: sqInt, line: sqInt, x: sqInt) -> PrimResult<Oop> {
    let p = ffi::pango()?;
    // Not `as_c_int_positive`: an X to the left of the line is negative and is
    // exactly the case whose clamping this primitive reports.
    let x = as_c_int(x)?;
    let mut index: c_int = 0;
    let mut trailing: c_int = 0;
    let hit = with_line(layout, line, |l| {
        Ok(from_gboolean(ffi::pg!(
            p,
            pango_layout_line_x_to_index(l, x, &mut index, &mut trailing)
        )))
    })?;
    let hit = if hit {
        vm.true_object()?
    } else {
        vm.false_object()?
    };
    let index = vm.integer_checked(index as sqInt)?;
    let trailing = vm.integer_checked(trailing as sqInt)?;
    oop_array(vm, &[hit, index, trailing])
}

/// `pango_layout_line_index_to_x`: the X position, in **Pango units** from the
/// left edge of the line, of a **byte** index within it.
///
/// `trailing` is a genuine `gboolean` here -- the trailing edge of the
/// grapheme when true, its leading edge when false -- and so the asymmetry
/// with [`primitiveLineXToIndex`], where `trailing` comes back as a character
/// count, is Pango's and not this plugin's.
///
/// The index is rejected when negative. Pango does not validate it and walks
/// its run list looking for a match, so a negative index is a bug in the image
/// that would otherwise answer a plausible-looking coordinate.
#[pharo_primitive]
fn primitiveLineIndexToX(
    _vm: &Interp,
    layout: sqInt,
    line: sqInt,
    byte_index: sqInt,
    trailing: bool,
) -> PrimResult<sqInt> {
    let p = ffi::pango()?;
    let byte_index = as_c_int_positive(byte_index)?;
    let trailing = as_gboolean(trailing);
    let mut x: c_int = 0;
    with_line(layout, line, |l| {
        ffi::pg!(
            p,
            pango_layout_line_index_to_x(l, byte_index, trailing, &mut x)
        );
        Ok(())
    })?;
    Ok(x as sqInt)
}

/// `pango_layout_line_get_x_ranges`: the X ranges a byte range covers, as a
/// `2 * n` Array of **Pango units** relative to the layout.
///
/// Range `n` starts at element `2n` and ends at element `2n + 1`; its width is
/// the difference. More than one range appears when the selected bytes are
/// split across directions, which is why this cannot be a single rectangle.
///
/// The array Pango fills in is `g_malloc`'d and transfer-full: it is the only
/// allocation in this whole module the plugin owns. It is copied out and
/// `g_free`d inside this same primitive, before anything image-side runs --
/// handing the pointer to the image would mean the image owned a glib
/// allocation, and there is no primitive in this plugin that could correctly
/// free one later.
///
/// `start` and `end` are **byte** offsets, `end` being one past the last byte
/// wanted. Both are rejected when negative, and `start > end` is rejected as
/// well: Pango neither checks nor documents that case, and an inverted range
/// answering an empty selection silently is worse for the image than a failed
/// primitive.
#[pharo_primitive]
fn primitiveLineGetXRanges(
    vm: &Interp,
    layout: sqInt,
    line: sqInt,
    start: sqInt,
    end: sqInt,
) -> PrimResult<Oop> {
    let p = ffi::pango()?;
    let g = ffi::glib()?;
    let start = as_c_int_positive(start)?;
    let end = as_c_int_positive(end)?;
    if start > end {
        return Err(PrimErr::BadArgument);
    }
    let values = with_line(layout, line, |l| {
        let mut ranges: *mut c_int = core::ptr::null_mut();
        let mut count: c_int = 0;
        ffi::pg!(
            p,
            pango_layout_line_get_x_ranges(l, start, end, &mut ranges, &mut count)
        );
        // An empty answer may leave `ranges` NULL or leave it a zero-length
        // allocation; both are copied out as an empty Vec and only a non-NULL
        // pointer is freed, so neither path can dereference NULL.
        let mut values: Vec<c_int> = Vec::new();
        if !ranges.is_null() {
            if count > 0 {
                // Two ints per range, as documented: `(*ranges)[2n]` and
                // `(*ranges)[2n + 1]`.
                let len = (count as usize).saturating_mul(2);
                // SAFETY: `ranges` is the g_malloc'd array Pango just filled
                // in, `count` is the range count it reported alongside it, and
                // the layout is still alive for the whole of this closure, so
                // `2 * count` ints are initialised and readable here.
                values.extend_from_slice(unsafe { core::slice::from_raw_parts(ranges, len) });
            }
            ffi::gl!(g, g_free(ranges.cast()));
        }
        Ok(values)
    })?;
    int_array(vm, &values)
}
