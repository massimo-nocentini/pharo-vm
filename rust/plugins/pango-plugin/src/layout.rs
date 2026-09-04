//! `PangoLayout`: a paragraph of text, its properties, its measurements and
//! its caret.
//!
//! Every integer that crosses here is in **Pango units** unless the
//! primitive's name says `Pixel`, and nothing in this file multiplies or
//! divides by `PANGO_SCALE`. Three entry points in the whole layout API speak
//! device units -- `get_pixel_size`, `get_pixel_extents` and the line variant
//! that lives in `lines.rs` -- and `get_pixel_extents` rounds *outwards*, so
//! it is not `PANGO_PIXELS(get_extents)` and neither may be computed from the
//! other. The image converts; the plugin passes through.
//!
//! The hazards this module carries, each named again where it bites:
//!
//! * `set_line_spacing`/`get_line_spacing` are the only floating-point values
//!   in the whole layout API and they are C `float`. The table declares
//!   `c_float`; the cast from the image's Float happens in the primitive body.
//! * `set_width(-1)`, a negative `set_height`, a negative `set_indent` and
//!   `move_cursor_visually`'s `-1`/`G_MAXINT` answers are **sentinels**, not
//!   lengths. Nothing here clamps, scales or rejects them for being negative.
//! * `xy_to_index` answers a `gboolean` and delivers the index through an out
//!   parameter, and its FALSE means "clamped", not "failed".
//! * `set_markup` returns void and reports a parse error only as a
//!   `g_warning` -- which under `G_DEBUG=fatal-warnings` aborts the VM -- so
//!   the primitive validates with `pango_parse_markup` before touching the
//!   layout.
//! * `get_attributes` and `get_font_description` are transfer **none** while
//!   `get_tabs`, three lines below them in the same header block, is transfer
//!   **full**. One leak and one double free live in that difference.
//! * Every index here, `get_direction`'s included, is a **byte** offset into
//!   the layout's UTF-8 text -- the plugin exposes no character-indexed entry
//!   point at all, and `primitiveLayoutGetCharacterCount` is the one primitive
//!   in the crate that so much as counts characters.
//! * Four of those indices are asserted by Pango against the text's length,
//!   not clamped, so this file checks them itself: see [`checked_byte_index`].

use core::ffi::{c_int, c_uint};
use core::{ptr, slice};
use std::ffi::{CStr, CString};

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    self, gl, glib, pango, pg, PangoRectangle, PANGO_ALIGN_MAX, PANGO_ELLIPSIZE_MAX,
    PANGO_VERSION_1_56, PANGO_WRAP_MAX, PANGO_WRAP_NONE,
};
use crate::resources::{
    as_c_int, as_c_int_positive, as_gboolean, destroy_layout, enum_in, enum_in_or_since,
    from_gboolean, int_array, oop_array, rect_array, rect_pair_array, register_attr_list,
    register_context, register_font_desc, register_layout, register_tab_array, runtime_version,
    utf8_cstring, with_context, with_layout, with_optional_attr_list,
    with_optional_font_desc, with_optional_tab_array,
};

// ---- helpers shared by the text entries ----------------------------------

/// The byte count for Pango's explicit-length text entries.
///
/// **Never -1.** A Smalltalk String is not NUL-terminated, and the `-1`
/// convention asks Pango to `strlen` the pointer -- which would read past the
/// end of a buffer this plugin built from image bytes. The `CString` behind
/// it exists for the interior-NUL rejection, not for the terminator: Pango
/// truncates its text at an embedded NUL even when `length` is positive, so a
/// string carrying one cannot be represented and `utf8_cstring` fails it as
/// `BadArgument` rather than letting the layout silently lose its tail.
fn text_length(text: &CString) -> PrimResult<c_int> {
    c_int::try_from(text.as_bytes().len()).map_err(|_| PrimErr::LimitExceeded)
}

/// The byte length of a layout's text, which is the upper bound on every byte
/// index this file hands to Pango.
///
/// `pango_layout_get_text` is transfer-none and NUL-terminated, and its length
/// is exactly the `layout->length` Pango asserts against: a string carrying an
/// interior NUL never reaches a layout because [`utf8_cstring`] refuses it, so
/// `strlen` cannot come up short here.
fn layout_byte_length(l: *mut ffi::PangoLayout) -> PrimResult<c_int> {
    let p = pango()?;
    let text = pg!(p, pango_layout_get_text(l));
    if text.is_null() {
        // A layout that has never been given text. Pango's own `length` is 0.
        return Ok(0);
    }
    // SAFETY: transfer-none `const char *`, NUL-terminated, owned by the
    // layout and alive for the rest of this primitive. Only measured, never
    // kept.
    let length = unsafe { CStr::from_ptr(text) }.to_bytes().len();
    c_int::try_from(length).map_err(|_| PrimErr::LimitExceeded)
}

/// The bounds check the index-taking entry points need, and Pango will not do
/// for them.
///
/// `get_cursor_pos`, `get_caret_pos`, `index_to_line_x` and
/// `move_cursor_visually` all open with a `g_return_if_fail` on
/// `index <= layout->length` -- measured, and the assertion strings are in the
/// shipped `libpango`. Past the end Pango writes **neither** out parameter, so
/// the primitive would answer whatever its Rust locals were initialised to --
/// a fabricated `{0. 0}` that the image cannot tell from the real answer for
/// the start of the layout -- and under `G_DEBUG=fatal-criticals` the same
/// assertion aborts the VM outright. So an index past the end is `BadIndex`
/// and Pango is never reached with one. The way an image gets there is a byte
/// index cached across a `setText:` that shortened the layout.
///
/// `length` itself is in range: it is the position after the last byte, where
/// a caret legitimately sits. `index_to_pos` needs none of this -- measured,
/// it clamps rather than asserting -- and neither does `get_direction`.
fn checked_byte_index(l: *mut ffi::PangoLayout, index: c_int) -> PrimResult<c_int> {
    if index > layout_byte_length(l)? {
        return Err(PrimErr::BadIndex);
    }
    Ok(index)
}

/// Parses markup and fails the primitive if it does not parse, touching no
/// layout either way.
///
/// This exists because `pango_layout_set_markup` and
/// `pango_layout_set_markup_with_accel` both return `void`: on a parse failure
/// they emit a `g_warning` to stderr, leave the layout holding its previous
/// text, and tell the caller nothing. The image would then render stale text
/// with no way to know why. Worse, a VM started with `G_DEBUG=fatal-warnings`
/// turns that warning into an `abort()` of the whole process.
///
/// `pango_parse_markup` is the same parser with a `GError **`, and all three
/// of its other out parameters are documented `optional` -- measured NULL-safe
/// on 1.58.2 -- so the validation costs one extra parse of a string that is,
/// in every real use, one label long.
fn validate_markup(markup: &CString, length: c_int, accel_marker: c_uint) -> PrimResult<()> {
    let p = pango()?;
    let mut error: *mut ffi::GError = ptr::null_mut();
    let ok = pg!(
        p,
        pango_parse_markup(
            markup.as_ptr(),
            length,
            accel_marker,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut error,
        )
    );
    if from_gboolean(ok) {
        return Ok(());
    }
    // The message is kept, not dropped: `primitiveLastMarkupError` is the
    // crate's one diagnostic channel and the image is documented to read it
    // after a markup failure. Discarding it here would leave the previous
    // `primitiveParseMarkup`'s sentence standing, which the image would then
    // attribute to *this* failure -- a wrong explanation being worse than
    // none, and a parse error being precisely what `set_markup` gives no other
    // way to see.
    //
    // SAFETY: the call answered FALSE, so by glib's convention `error` is
    // either null or a GError it allocated and handed over transfer=full,
    // which is `record_markup_error`'s contract; it frees it.
    unsafe { crate::markup::record_markup_error(error) };
    Err(PrimErr::OperationFailed)
}

// ---- construction and identity -------------------------------------------

/// `pango_layout_new`. Answers a handle on a new layout drawn with `context`.
///
/// The layout takes its own reference on the context (measured: rc 1 -> 2
/// across this call), so the image may destroy its context handle immediately
/// afterwards and the layout stays valid.
#[pharo_primitive]
fn primitiveLayoutNew(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |ctx| {
        let layout = pg!(p, pango_layout_new(ctx));
        register_layout(layout)
    })
}

/// `pango_layout_copy`. Answers a handle on a deep copy: text, attribute list
/// and tab array are all copied by value, so the two layouts share nothing the
/// image can change.
#[pharo_primitive]
fn primitiveLayoutCopy(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        let copy = pg!(p, pango_layout_copy(l));
        register_layout(copy)
    })
}

/// `g_object_unref` on the layout, and forgets the handle.
///
/// Destroying twice fails the second time with `NotFound` rather than
/// double-unref'ing, which is the whole reason handles are integers here.
#[pharo_primitive]
fn primitiveLayoutDestroy(_vm: &Interp, layout: sqInt) -> PrimResult<()> {
    destroy_layout(layout)
}

/// `pango_layout_get_context`. Answers a handle on the layout's context.
///
/// The getter is transfer **none** -- the layout owns that reference -- so the
/// plugin takes its own `g_object_ref` before registering. Without it the
/// image's context handle would go stale the moment the layout was destroyed,
/// and destroying the handle would unref a reference this plugin never owned.
#[pharo_primitive]
fn primitiveLayoutGetContext(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    let g = glib()?;
    with_layout(layout, |l| {
        let ctx = pg!(p, pango_layout_get_context(l));
        if ctx.is_null() {
            // Documented not-nullable; a null here means the layout is not one.
            return Err(PrimErr::BadArgument);
        }
        gl!(g, g_object_ref(ctx.cast()));
        register_context(ctx)
    })
}

/// `pango_layout_context_changed`. Tells the layout that its context was
/// modified after the layout was made, discarding what it had computed.
///
/// Also the documented way to force the serial to advance.
#[pharo_primitive]
fn primitiveLayoutContextChanged(_vm: &Interp, layout: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_context_changed(l));
        Ok(())
    })
}

/// `pango_layout_get_serial`. A number that changes whenever the layout does.
///
/// It starts small and non-zero, never becomes 0, and **wraps**: compare it
/// with `~=`, never with `<`. This is what an image-side cache of measurements
/// should key on.
#[pharo_primitive]
fn primitiveLayoutSerial(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_serial(l)) as sqInt))
}

// ---- text and markup -----------------------------------------------------

/// `pango_layout_set_text`, with an explicit byte length.
///
/// The text goes in as UTF-8: `utf8_cstring` decodes a Pharo ByteString from
/// Latin-1 when that is what it holds, because handing Pango raw Latin-1 bytes
/// renders every accented character as a placeholder box.
///
/// **The consequence the image must know:** every byte index this module
/// speaks -- `xy_to_index`, `index_to_pos`, `get_cursor_pos` -- is an offset
/// into *that* UTF-8 text, not into the image's String. Index against
/// `primitiveLayoutGetText`.
///
/// This does **not** clear attributes left over from a previous
/// `setMarkup:`; Pango's own doc says so. Follow it with
/// `primitiveLayoutSetAttributes` and handle 0 if the layout has ever held
/// markup.
#[pharo_primitive]
fn primitiveLayoutSetText(vm: &Interp, layout: sqInt, text: Oop) -> PrimResult<()> {
    let p = pango()?;
    let text = utf8_cstring(vm, text)?;
    let length = text_length(&text)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_text(l, text.as_ptr(), length));
        Ok(())
    })
}

/// `pango_layout_get_text`. The layout's text as UTF-8.
///
/// Borrowed: the buffer belongs to the layout and is invalidated by the next
/// `setText:`/`setMarkup:`, so it is copied into a Smalltalk String here and
/// never freed.
#[pharo_primitive]
fn primitiveLayoutGetText(_vm: &Interp, layout: sqInt) -> PrimResult<String> {
    let p = pango()?;
    with_layout(layout, |l| {
        let text = pg!(p, pango_layout_get_text(l));
        // SAFETY: transfer-none `const char *`, NUL-terminated, owned by the
        // layout and alive for the rest of this primitive. Copied, not freed.
        Ok(unsafe { ffi::borrowed_str(text) }.unwrap_or_default())
    })
}

/// `pango_layout_get_character_count`. **Unicode characters**, not bytes.
///
/// The one count in this module that is not a byte offset. Every `index`
/// elsewhere is a byte into the UTF-8 text and the two differ for any text
/// outside ASCII.
#[pharo_primitive]
fn primitiveLayoutGetCharacterCount(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_character_count(l)) as sqInt)
    })
}

/// `pango_parse_markup` to validate, then `pango_layout_set_markup`.
///
/// Two parses, deliberately: see [`validate_markup`]. A malformed string fails
/// with `OperationFailed` and the layout keeps the text it had.
#[pharo_primitive]
fn primitiveLayoutSetMarkup(vm: &Interp, layout: sqInt, markup: Oop) -> PrimResult<()> {
    let p = pango()?;
    let markup = utf8_cstring(vm, markup)?;
    let length = text_length(&markup)?;
    validate_markup(&markup, length, 0)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_markup(l, markup.as_ptr(), length));
        Ok(())
    })
}

/// `pango_layout_set_markup_with_accel`. Answers the accelerator character as
/// its Unicode code point, or 0 when the markup carried none.
///
/// `accelMarker` is a code point too -- `gunichar`, not a byte and not a
/// Character index -- conventionally `$_` (95). The first character after an
/// unescaped marker is underlined and reported here.
///
/// Validated first for the same reason as [`primitiveLayoutSetMarkup`]: this
/// entry point is `void` as well, and a parse error would otherwise be a
/// warning on stderr and stale text in the layout.
#[pharo_primitive]
fn primitiveLayoutSetMarkupWithAccel(
    vm: &Interp,
    layout: sqInt,
    markup: Oop,
    accel_marker: sqInt,
) -> PrimResult<sqInt> {
    let p = pango()?;
    let markup = utf8_cstring(vm, markup)?;
    let length = text_length(&markup)?;
    let accel_marker = c_uint::try_from(accel_marker).map_err(|_| PrimErr::BadArgument)?;
    validate_markup(&markup, length, accel_marker)?;
    with_layout(layout, |l| {
        let mut accel_char: ffi::gunichar = 0;
        pg!(
            p,
            pango_layout_set_markup_with_accel(
                l,
                markup.as_ptr(),
                length,
                accel_marker,
                &mut accel_char,
            )
        );
        Ok(accel_char as sqInt)
    })
}

// ---- attributes, font description, tabs ----------------------------------

/// `pango_layout_set_attributes`. Handle 0 clears the list.
///
/// Pango references the list rather than taking it, so the image keeps
/// ownership of its handle and may destroy it independently.
#[pharo_primitive]
fn primitiveLayoutSetAttributes(_vm: &Interp, layout: sqInt, attrs: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_optional_attr_list(attrs, |list| {
        with_layout(layout, |l| {
            pg!(p, pango_layout_set_attributes(l, list));
            Ok(())
        })
    })
}

/// `pango_layout_get_attributes`. A new handle on the layout's attribute list,
/// or nil when it has none.
///
/// Transfer **none**: the answer is the layout's own list, so this takes a
/// `pango_attr_list_ref` before registering it. Registering the borrow
/// directly would have the image's `destroy` unref a reference the plugin
/// never owned -- a double free, arriving whenever the layout happened to be
/// released first.
#[pharo_primitive]
fn primitiveLayoutGetAttributes(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let handle = with_layout(layout, |l| {
        let list = pg!(p, pango_layout_get_attributes(l));
        if list.is_null() {
            return Ok(None);
        }
        let list = pg!(p, pango_attr_list_ref(list));
        register_attr_list(list).map(Some)
    })?;
    match handle {
        Some(handle) => vm.integer_checked(handle),
        None => vm.nil(),
    }
}

/// `pango_layout_set_font_description`. Handle 0 unsets it, and the layout
/// falls back to its context's description.
///
/// The description is copied by Pango, so the image's handle stays its own.
#[pharo_primitive]
fn primitiveLayoutSetFontDescription(_vm: &Interp, layout: sqInt, desc: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_optional_font_desc(desc, |d| {
        with_layout(layout, |l| {
            pg!(p, pango_layout_set_font_description(l, d));
            Ok(())
        })
    })
}

/// `pango_layout_get_font_description`. A handle on a **copy** of the layout's
/// description, or nil when it has none.
///
/// Transfer none, and `const` besides: the answer points into the layout.
/// `pango_font_description_free` on it would be a double free, and holding it
/// past the next `setFontDescription:` would dangle. Copying is the only way
/// to give the image something with a lifetime of its own.
#[pharo_primitive]
fn primitiveLayoutGetFontDescription(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let handle = with_layout(layout, |l| {
        let desc = pg!(p, pango_layout_get_font_description(l));
        if desc.is_null() {
            return Ok(None);
        }
        let copy = pg!(p, pango_font_description_copy(desc));
        register_font_desc(copy).map(Some)
    })?;
    match handle {
        Some(handle) => vm.integer_checked(handle),
        None => vm.nil(),
    }
}

/// `pango_layout_set_tabs`. Handle 0 reinstates the default stop every eight
/// spaces.
///
/// The tab array is copied into the layout; the image keeps its own.
/// Pango's own warning is worth passing on: tabs conflict with justification
/// and with any alignment other than left.
#[pharo_primitive]
fn primitiveLayoutSetTabs(_vm: &Interp, layout: sqInt, tabs: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_optional_tab_array(tabs, |t| {
        with_layout(layout, |l| {
            pg!(p, pango_layout_set_tabs(l, t));
            Ok(())
        })
    })
}

/// `pango_layout_get_tabs`. A handle on a tab array the image now owns, or nil
/// when the layout uses the default stops.
///
/// Transfer **full**, unlike the two getters immediately above it in the same
/// header block: this one hands ownership over and leaks unless the array is
/// registered and eventually freed with `pango_tab_array_free`. No `ref` and
/// no copy here -- that would be the leak the other two's rules prevent.
#[pharo_primitive]
fn primitiveLayoutGetTabs(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let handle = with_layout(layout, |l| {
        let tabs = pg!(p, pango_layout_get_tabs(l));
        if tabs.is_null() {
            return Ok(None);
        }
        register_tab_array(tabs).map(Some)
    })?;
    match handle {
        Some(handle) => vm.integer_checked(handle),
        None => vm.nil(),
    }
}

// ---- geometry, wrapping, alignment ---------------------------------------

/// `pango_layout_set_width`, in Pango units.
///
/// **-1 is a sentinel meaning "no wrapping and no ellipsization"**, not a
/// width of minus one unit. The image must send -1 unscaled: a -1 that was
/// multiplied by `PANGO_SCALE` on the way in is a width of -1024 units and
/// means nothing at all. Nothing here scales or clamps it.
#[pharo_primitive]
fn primitiveLayoutSetWidth(_vm: &Interp, layout: sqInt, units: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let width = as_c_int(units)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_width(l, width));
        Ok(())
    })
}

/// `pango_layout_get_width`, in Pango units. -1 when no width is set.
#[pharo_primitive]
fn primitiveLayoutGetWidth(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_width(l)) as sqInt))
}

/// `pango_layout_set_height`. **The sign changes the unit.**
///
/// Positive is a maximum height in Pango units; negative is the negated
/// maximum number of lines per paragraph, which is a *count* and not a length.
/// The default, -1, means "ellipsize after the first line of each paragraph".
/// Any scheme that converted the image's value to Pango units would be wrong
/// for half the domain of this one function, so the plugin converts nothing.
///
/// It only has an effect when a positive width is set and ellipsization is not
/// `PANGO_ELLIPSIZE_NONE`; Pango calls the other combination undefined.
#[pharo_primitive]
fn primitiveLayoutSetHeight(_vm: &Interp, layout: sqInt, units_or_lines: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let height = as_c_int(units_or_lines)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_height(l, height));
        Ok(())
    })
}

/// `pango_layout_get_height`. Pango units when positive, a negated line count
/// when negative -- see [`primitiveLayoutSetHeight`].
#[pharo_primitive]
fn primitiveLayoutGetHeight(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_height(l)) as sqInt))
}

/// `pango_layout_set_wrap`. `PangoWrapMode`: 0 word, 1 char, 2 word-then-char,
/// 3 none.
///
/// 3 exists only from Pango 1.56 and answers `Unsupported` on anything older.
/// This is the one range check in the module that must be made rather than
/// trusted: `pango_layout_set_wrap` guards only that its argument is a layout,
/// so an out-of-range mode is *stored* and then falls through every switch in
/// the line breaker -- a subtly wrong layout instead of a failure anyone can
/// see.
///
/// Wrapping has no effect at all unless a width is set.
#[pharo_primitive]
fn primitiveLayoutSetWrap(_vm: &Interp, layout: sqInt, mode: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let mode = enum_in_or_since(
        mode,
        0,
        PANGO_WRAP_MAX,
        PANGO_WRAP_NONE,
        PANGO_VERSION_1_56,
        runtime_version(),
    )?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_wrap(l, mode));
        Ok(())
    })
}

/// `pango_layout_get_wrap`. The `PangoWrapMode` as an integer.
#[pharo_primitive]
fn primitiveLayoutGetWrap(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_wrap(l)) as sqInt))
}

/// `pango_layout_is_wrapped`. Whether any paragraph actually *had* to wrap --
/// not whether a wrap mode is set.
#[pharo_primitive]
fn primitiveLayoutIsWrapped(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(p, pango_layout_is_wrapped(l))))
    })
}

/// `pango_layout_set_indent`, in Pango units.
///
/// A **negative** value is a hanging indent, not an error. Ignored entirely
/// when the alignment is centred.
#[pharo_primitive]
fn primitiveLayoutSetIndent(_vm: &Interp, layout: sqInt, units: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let indent = as_c_int(units)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_indent(l, indent));
        Ok(())
    })
}

/// `pango_layout_get_indent`, in Pango units. Negative means hanging.
#[pharo_primitive]
fn primitiveLayoutGetIndent(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_indent(l)) as sqInt))
}

/// `pango_layout_set_spacing`, in Pango units: the gap between one line's
/// bottom and the next line's top.
///
/// Since 1.44 this is **ignored** whenever a non-zero line-spacing factor has
/// been set with [`primitiveLayoutSetLineSpacing`], which places lines by the
/// font's own line height instead. Set one or the other, not both.
#[pharo_primitive]
fn primitiveLayoutSetSpacing(_vm: &Interp, layout: sqInt, units: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let spacing = as_c_int(units)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_spacing(l, spacing));
        Ok(())
    })
}

/// `pango_layout_get_spacing`, in Pango units.
#[pharo_primitive]
fn primitiveLayoutGetSpacing(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| Ok(pg!(p, pango_layout_get_spacing(l)) as sqInt))
}

/// `pango_layout_set_line_spacing`. A **unitless factor**: 0 (the default,
/// meaning "use `spacing` instead"), 1, 1.5, 2. `baseline2 = baseline1 +
/// factor * height2`.
///
/// The image sends a Float, i.e. an `f64`, and the cast to `f32` happens here
/// because the C parameter is `float` (pango-layout.h:229-230). Declaring it
/// `double` in the table would compile, link and resolve, and then produce
/// garbage: on AArch64 the caller writes `d0` and the callee reads `s0`. This
/// pair is the only floating-point value in the entire `PangoLayout` API, and
/// only a live round-trip test would ever notice it being wrong.
#[pharo_primitive]
fn primitiveLayoutSetLineSpacing(_vm: &Interp, layout: sqInt, factor: f64) -> PrimResult<()> {
    let p = pango()?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_line_spacing(l, factor as f32));
        Ok(())
    })
}

/// `pango_layout_get_line_spacing`. The unitless factor, widened from the C
/// `float` -- so a value the image sent as 0.1 comes back as
/// `f64::from(0.1f32)` and not as 0.1. That is the round trip through 32-bit
/// storage, not a plugin rounding error.
#[pharo_primitive]
fn primitiveLayoutGetLineSpacing(_vm: &Interp, layout: sqInt) -> PrimResult<f64> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(f64::from(pg!(p, pango_layout_get_line_spacing(l))))
    })
}

/// `pango_layout_set_justify`. Stretches lines to fill the layout width.
#[pharo_primitive]
fn primitiveLayoutSetJustify(_vm: &Interp, layout: sqInt, on: bool) -> PrimResult<()> {
    let p = pango()?;
    let on = as_gboolean(on);
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_justify(l, on));
        Ok(())
    })
}

/// `pango_layout_get_justify`.
#[pharo_primitive]
fn primitiveLayoutGetJustify(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(p, pango_layout_get_justify(l))))
    })
}

/// `pango_layout_set_justify_last_line`. Pango 1.50 and later; answers
/// `Unsupported` when the entry point did not resolve.
///
/// Has an effect only when justification is on.
#[pharo_primitive]
fn primitiveLayoutSetJustifyLastLine(_vm: &Interp, layout: sqInt, on: bool) -> PrimResult<()> {
    let p = pango()?;
    let on = as_gboolean(on);
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_justify_last_line(l, on));
        Ok(())
    })
}

/// `pango_layout_get_justify_last_line`. Pango 1.50 and later.
#[pharo_primitive]
fn primitiveLayoutGetJustifyLastLine(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(p, pango_layout_get_justify_last_line(l))))
    })
}

/// `pango_layout_set_auto_dir`. Default true.
///
/// When on and a paragraph's computed direction differs from the context's
/// base direction, the meanings of left and right alignment **swap**. That is
/// a feature for bidirectional text and a surprise for anyone laying out a
/// mixed document, so it is worth knowing before blaming the alignment.
#[pharo_primitive]
fn primitiveLayoutSetAutoDir(_vm: &Interp, layout: sqInt, on: bool) -> PrimResult<()> {
    let p = pango()?;
    let on = as_gboolean(on);
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_auto_dir(l, on));
        Ok(())
    })
}

/// `pango_layout_get_auto_dir`.
#[pharo_primitive]
fn primitiveLayoutGetAutoDir(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(p, pango_layout_get_auto_dir(l))))
    })
}

/// `pango_layout_set_alignment`. `PangoAlignment`: 0 left, 1 centre, 2 right.
///
/// Range-checked, because Pango does not check and an out-of-range value is
/// stored rather than rejected. With justification on this affects only the
/// partial lines; with auto-dir on, 0 and 2 may swap.
#[pharo_primitive]
fn primitiveLayoutSetAlignment(_vm: &Interp, layout: sqInt, alignment: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let alignment = enum_in(alignment, 0, PANGO_ALIGN_MAX)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_alignment(l, alignment));
        Ok(())
    })
}

/// `pango_layout_get_alignment`. The `PangoAlignment` as an integer.
#[pharo_primitive]
fn primitiveLayoutGetAlignment(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_alignment(l)) as sqInt)
    })
}

/// `pango_layout_set_single_paragraph_mode`. When on, newlines stop being
/// paragraph separators and are drawn as glyphs instead -- what a single-line
/// text field wants.
#[pharo_primitive]
fn primitiveLayoutSetSingleParagraphMode(
    _vm: &Interp,
    layout: sqInt,
    on: bool,
) -> PrimResult<()> {
    let p = pango()?;
    let on = as_gboolean(on);
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_single_paragraph_mode(l, on));
        Ok(())
    })
}

/// `pango_layout_get_single_paragraph_mode`.
#[pharo_primitive]
fn primitiveLayoutGetSingleParagraphMode(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(
            p,
            pango_layout_get_single_paragraph_mode(l)
        )))
    })
}

/// `pango_layout_set_ellipsize`. `PangoEllipsizeMode`: 0 none, 1 start,
/// 2 middle, 3 end. Range-checked for the same reason as the wrap mode.
#[pharo_primitive]
fn primitiveLayoutSetEllipsize(_vm: &Interp, layout: sqInt, mode: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let mode = enum_in(mode, 0, PANGO_ELLIPSIZE_MAX)?;
    with_layout(layout, |l| {
        pg!(p, pango_layout_set_ellipsize(l, mode));
        Ok(())
    })
}

/// `pango_layout_get_ellipsize`. The `PangoEllipsizeMode` as an integer.
#[pharo_primitive]
fn primitiveLayoutGetEllipsize(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_ellipsize(l)) as sqInt)
    })
}

/// `pango_layout_is_ellipsized`. Whether text actually *was* ellipsized -- a
/// mode plus a width plus a paragraph too long for it -- not whether a mode is
/// set.
#[pharo_primitive]
fn primitiveLayoutIsEllipsized(_vm: &Interp, layout: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(from_gboolean(pg!(p, pango_layout_is_ellipsized(l))))
    })
}

/// `pango_layout_get_direction`, at a **byte** offset into the layout's UTF-8
/// text. Pango 1.46 and later.
///
/// The C parameter is called `index` and its own documentation reads "the byte
/// index of the char", which is easy to read the other way round: the function
/// is *about* a character, but it is *addressed* in bytes, like `xy_to_index`,
/// `index_to_pos`, `get_cursor_pos` and every line accessor. Measured on
/// "a<U+05E9>b" -- three characters, four bytes -- it answers LTR, RTL, RTL,
/// LTR for indices 0 to 3: it partitions on the Hebrew letter's *two bytes*.
/// So the primitive is named `AtIndex`, like its neighbours, and an image that
/// walks characters must convert first.
///
/// Answers a `PangoDirection`: 0 LTR, 1 RTL, 2 and 3 the deprecated
/// top-to-bottom pair, 4 weak LTR, 5 weak RTL, 6 neutral.
///
/// Not bounds-checked, unlike the four indices below: `get_direction` reads
/// past the end without an assertion (it answers the layout's own direction
/// there), so there is no crash and no glib critical to head off.
#[pharo_primitive]
fn primitiveLayoutDirectionAtIndex(
    _vm: &Interp,
    layout: sqInt,
    byte_index: sqInt,
) -> PrimResult<sqInt> {
    let p = pango()?;
    // An offset from the start of the text; negative is never meaningful here,
    // unlike the dimensions above.
    let index = as_c_int_positive(byte_index)?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_direction(l, index)) as sqInt)
    })
}

// ---- measurement ---------------------------------------------------------

/// `pango_layout_get_size`, as a Point in **Pango units**.
///
/// The logical width and height. Both out parameters are passed non-NULL even
/// though the gir marks them optional: that annotation post-dates the
/// function, an older Pango that does not guard a NULL would crash, and a
/// stack `int` costs nothing.
#[pharo_primitive]
fn primitiveLayoutGetSize(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let (width, height) = with_layout(layout, |l| {
        let mut width: c_int = 0;
        let mut height: c_int = 0;
        pg!(p, pango_layout_get_size(l, &mut width, &mut height));
        Ok((width, height))
    })?;
    vm.point(width as sqInt, height as sqInt)
}

/// `pango_layout_get_pixel_size`, as a Point in **device units (pixels)**.
///
/// Not `PANGO_PIXELS` of [`primitiveLayoutGetSize`]: this rounds through
/// `get_pixel_extents`, which rounds outwards, so the two can differ by a
/// pixel. Neither is implemented from the other.
#[pharo_primitive]
fn primitiveLayoutGetPixelSize(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let (width, height) = with_layout(layout, |l| {
        let mut width: c_int = 0;
        let mut height: c_int = 0;
        pg!(p, pango_layout_get_pixel_size(l, &mut width, &mut height));
        Ok((width, height))
    })?;
    vm.point(width as sqInt, height as sqInt)
}

/// `pango_layout_get_extents`, as an 8-element Array in **Pango units**:
/// `{inkX. inkY. inkW. inkH. logicalX. logicalY. logicalW. logicalH}`.
///
/// Both rectangles at once, from one call, so the image cannot pair an ink
/// rectangle with a logical one measured after a change.
///
/// **Both may have non-zero -- and negative -- x and y**, and offsetting the
/// drawing by them is the step whose omission shows up as right-to-left text
/// sitting in the wrong place inside a layout with a set width.
#[pharo_primitive]
fn primitiveLayoutGetExtents(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let (ink, logical) = with_layout(layout, |l| {
        let mut ink = PangoRectangle::default();
        let mut logical = PangoRectangle::default();
        pg!(p, pango_layout_get_extents(l, &mut ink, &mut logical));
        Ok((ink, logical))
    })?;
    rect_pair_array(vm, &ink, &logical)
}

/// `pango_layout_get_pixel_extents`, as an 8-element Array in **device units
/// (pixels)**, ink then logical.
///
/// Rounds each rectangle *outwards* so that the rounded one fully contains the
/// unrounded one -- which is why it is not `PANGO_PIXELS` applied to
/// [`primitiveLayoutGetExtents`].
#[pharo_primitive]
fn primitiveLayoutGetPixelExtents(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let (ink, logical) = with_layout(layout, |l| {
        let mut ink = PangoRectangle::default();
        let mut logical = PangoRectangle::default();
        pg!(p, pango_layout_get_pixel_extents(l, &mut ink, &mut logical));
        Ok((ink, logical))
    })?;
    rect_pair_array(vm, &ink, &logical)
}

/// `pango_layout_get_baseline`: the y of the first line's baseline measured
/// from the top of the layout, in **Pango units**.
#[pharo_primitive]
fn primitiveLayoutGetBaseline(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_baseline(l)) as sqInt)
    })
}

/// `pango_layout_get_line_count`. Line indices run `0 ..= count - 1`; the
/// line primitives in `lines.rs` range-check against this.
#[pharo_primitive]
fn primitiveLayoutGetLineCount(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_line_count(l)) as sqInt)
    })
}

/// `pango_layout_get_unknown_glyphs_count`. How many characters no available
/// font could render -- the count of tofu boxes, and the cheapest way for the
/// image to ask "is this font actually able to draw this string?".
#[pharo_primitive]
fn primitiveLayoutGetUnknownGlyphsCount(_vm: &Interp, layout: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_layout(layout, |l| {
        Ok(pg!(p, pango_layout_get_unknown_glyphs_count(l)) as sqInt)
    })
}

// ---- hit-testing and the caret -------------------------------------------

/// `pango_layout_xy_to_index`. `x` and `y` in **Pango units** from the layout's
/// top-left. Answers a 3-element Array `{hit. byteIndex. trailing}`.
///
/// **The C function's return value is a `gboolean`, not the index** -- the
/// index comes back through an out parameter, and reading the return as the
/// index is the mistake that "works", because 0 and 1 are both plausible byte
/// offsets.
///
/// `hit` false means the point was outside the layout and the answer was
/// **clamped to the nearest position**, not that anything failed: both out
/// parameters are valid either way, so this primitive never fails on a false.
/// Failing there would break clicking past the end of a line, which is the
/// commonest gesture an editor has.
///
/// `trailing` is a *character count* within the grapheme, not a boolean: 0 is
/// the leading edge, otherwise it is the number of characters in the grapheme.
#[pharo_primitive]
fn primitiveLayoutXyToIndex(vm: &Interp, layout: sqInt, x: sqInt, y: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    // Both may be negative: a point above or to the left of the layout is a
    // legitimate query that comes back clamped.
    let x = as_c_int(x)?;
    let y = as_c_int(y)?;
    let (hit, index, trailing) = with_layout(layout, |l| {
        let mut index: c_int = 0;
        let mut trailing: c_int = 0;
        let hit = pg!(
            p,
            pango_layout_xy_to_index(l, x, y, &mut index, &mut trailing)
        );
        Ok((from_gboolean(hit), index, trailing))
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

/// `pango_layout_index_to_pos`, as a 4-element Array `{x. y. width. height}`
/// in **Pango units**.
///
/// `x` is always the *leading* edge of the grapheme and `x + width` the
/// trailing edge, so **`width` is negative for a right-to-left grapheme**.
/// That sign is the concrete reason rectangles cross as Arrays of
/// SmallIntegers rather than as unsigned machine words.
#[pharo_primitive]
fn primitiveLayoutIndexToPos(vm: &Interp, layout: sqInt, byte_index: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let index = as_c_int_positive(byte_index)?;
    let pos = with_layout(layout, |l| {
        let mut pos = PangoRectangle::default();
        pg!(p, pango_layout_index_to_pos(l, index, &mut pos));
        Ok(pos)
    })?;
    rect_array(vm, &pos)
}

/// `pango_layout_index_to_line_x`, as a 2-element Array `{lineIndex. xPos}`.
///
/// `byteIndex` is a byte offset into the layout's UTF-8 text; `trailing`
/// chooses the grapheme's trailing edge rather than its leading one. The
/// answered `lineIndex` is in `0 ..= lineCount - 1` and `xPos` is in **Pango
/// units** from the left edge of *that line*, not of the layout.
///
/// Past the end of the text is `BadIndex`; see [`checked_byte_index`].
#[pharo_primitive]
fn primitiveLayoutIndexToLineX(
    vm: &Interp,
    layout: sqInt,
    byte_index: sqInt,
    trailing: bool,
) -> PrimResult<Oop> {
    let p = pango()?;
    let index = as_c_int_positive(byte_index)?;
    let trailing = as_gboolean(trailing);
    let (line, x_pos) = with_layout(layout, |l| {
        let index = checked_byte_index(l, index)?;
        let mut line: c_int = 0;
        let mut x_pos: c_int = 0;
        pg!(
            p,
            pango_layout_index_to_line_x(l, index, trailing, &mut line, &mut x_pos)
        );
        Ok((line, x_pos))
    })?;
    int_array(vm, &[line, x_pos])
}

/// `pango_layout_get_cursor_pos`, as an 8-element Array in **Pango units**:
/// the strong cursor's rectangle then the weak one's.
///
/// Each is a zero-width rectangle with the height of the run, so what the
/// image draws is the line from `y` to `y + height` at `x`. The two differ
/// only in bidirectional text, where the strong cursor follows the keyboard's
/// direction and the weak one the other.
///
/// Past the end of the text is `BadIndex`; see [`checked_byte_index`].
#[pharo_primitive]
fn primitiveLayoutGetCursorPos(vm: &Interp, layout: sqInt, byte_index: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let index = as_c_int_positive(byte_index)?;
    let (strong, weak) = with_layout(layout, |l| {
        let index = checked_byte_index(l, index)?;
        let mut strong = PangoRectangle::default();
        let mut weak = PangoRectangle::default();
        pg!(
            p,
            pango_layout_get_cursor_pos(l, index, &mut strong, &mut weak)
        );
        Ok((strong, weak))
    })?;
    rect_pair_array(vm, &strong, &weak)
}

/// `pango_layout_get_caret_pos`, the same 8-element Array in **Pango units**.
/// Pango 1.50 and later; answers `Unsupported` on an older library.
///
/// The difference from [`primitiveLayoutGetCursorPos`] is that this one
/// applies the font's caret slope and offset, which is what italic text wants:
/// a slanted caret rather than a vertical one. It shares
/// `get_cursor_pos`'s bounds check, and its assertion: past the end of the
/// text is `BadIndex`.
#[pharo_primitive]
fn primitiveLayoutGetCaretPos(vm: &Interp, layout: sqInt, byte_index: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let index = as_c_int_positive(byte_index)?;
    let (strong, weak) = with_layout(layout, |l| {
        let index = checked_byte_index(l, index)?;
        let mut strong = PangoRectangle::default();
        let mut weak = PangoRectangle::default();
        pg!(
            p,
            pango_layout_get_caret_pos(l, index, &mut strong, &mut weak)
        );
        Ok((strong, weak))
    })?;
    rect_pair_array(vm, &strong, &weak)
}

/// `pango_layout_move_cursor_visually`, as a 2-element Array
/// `{newByteIndex. newTrailing}`.
///
/// Visual motion, which in bidirectional text is not the same as motion
/// through the byte sequence -- that is the whole point of the function.
/// `strong` says which of the two cursors is moving; `oldTrailing` is 0 for
/// the leading edge of the grapheme at `oldByteIndex`; `direction` is negative
/// to move left and positive to move right.
///
/// **Two sentinels come back in `newByteIndex` and must reach the image
/// intact: -1 means the cursor moved off the beginning of the layout, and
/// `G_MAXINT` (2147483647) that it moved off the end.** Neither is an error
/// and neither is clamped here; an image walking a document uses them to step
/// into the previous or next layout.
///
/// `oldByteIndex` past the end of the text is `BadIndex`, and so is the end of
/// the text itself with a non-zero `oldTrailing` -- Pango asserts on both, and
/// answers a fabricated `{0. 0}` rather than failing. A stale index cached
/// across a `setText:` is the way an image reaches either.
#[pharo_primitive]
fn primitiveLayoutMoveCursorVisually(
    vm: &Interp,
    layout: sqInt,
    strong: bool,
    old_index: sqInt,
    old_trailing: sqInt,
    direction: sqInt,
) -> PrimResult<Oop> {
    let p = pango()?;
    let strong = as_gboolean(strong);
    let old_index = as_c_int_positive(old_index)?;
    let old_trailing = as_c_int_positive(old_trailing)?;
    // The one argument here that is meaningfully negative: it is a direction,
    // not a position.
    let direction = as_c_int(direction)?;
    let (new_index, new_trailing) = with_layout(layout, |l| {
        let old_index = checked_byte_index(l, old_index)?;
        // The second of this function's two assertions:
        // `old_index < layout->length || old_trailing == 0`. There is no
        // grapheme after the last byte for a trailing edge to be on, so the
        // one index `checked_byte_index` allows through needs this extra word.
        if old_trailing != 0 && old_index == layout_byte_length(l)? {
            return Err(PrimErr::BadIndex);
        }
        let mut new_index: c_int = 0;
        let mut new_trailing: c_int = 0;
        pg!(
            p,
            pango_layout_move_cursor_visually(
                l,
                strong,
                old_index,
                old_trailing,
                direction,
                &mut new_index,
                &mut new_trailing,
            )
        );
        Ok((new_index, new_trailing))
    })?;
    int_array(vm, &[new_index, new_trailing])
}

/// `pango_layout_get_log_attrs_readonly`, as a Bitmap of raw 32-bit words --
/// one per position, **one more than the character count**, because there is a
/// position before the first character and one after the last.
///
/// Each word is a `PangoLogAttr`, fifteen one-bit flags packed into a `guint`
/// by the C compiler: bit 0 `isLineBreak`, 1 `isMandatoryBreak`, 2
/// `isCharBreak`, 3 `isWhite`, 4 `isCursorPosition`, 5 `isWordStart`, 6
/// `isWordEnd`, 7 `isSentenceBoundary`, 8 `isSentenceStart`, 9
/// `isSentenceEnd`, 10 `backspaceDeletesCharacter`, 11 `isExpandableSpace`, 12
/// `isWordBoundary`, 13 `breakInsertsHyphen`, 14 `breakRemovesPreceding`.
/// The image masks; reproducing a C bitfield in Rust would mean asserting the
/// compiler's bit order for no gain.
///
/// The readonly form is preferred because it allocates nothing -- the array
/// belongs to the layout and dies with the next change to it, so the words are
/// copied out here and the pointer never leaves this primitive. Where only the
/// allocating form resolved, its `g_malloc`'d array is copied and `g_free`d in
/// the same call.
#[pharo_primitive]
fn primitiveLayoutGetLogAttrs(vm: &Interp, layout: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let attrs: Vec<ffi::PangoLogAttr> = with_layout(layout, |l| {
        if p.pango_layout_get_log_attrs_readonly.is_some() {
            let mut count: c_int = 0;
            let base = pg!(p, pango_layout_get_log_attrs_readonly(l, &mut count));
            if base.is_null() || count <= 0 {
                return Ok(Vec::new());
            }
            let count = usize::try_from(count).map_err(|_| PrimErr::LimitExceeded)?;
            // SAFETY: Pango just wrote `count` attrs at `base`, they belong to
            // the layout, and nothing between here and the copy can modify the
            // layout and invalidate them.
            return Ok(unsafe { slice::from_raw_parts(base, count) }.to_vec());
        }
        // The 1.30-and-later readonly form is missing, so pay for the copy
        // Pango makes and free it before returning.
        let g = glib()?;
        let mut base: *mut ffi::PangoLogAttr = ptr::null_mut();
        let mut count: c_int = 0;
        pg!(p, pango_layout_get_log_attrs(l, &mut base, &mut count));
        if base.is_null() {
            return Ok(Vec::new());
        }
        let owned = if count > 0 {
            let count = usize::try_from(count).map_err(|_| PrimErr::LimitExceeded)?;
            // SAFETY: `base` is a g_malloc'd array of `count` attrs this call
            // now owns, freed immediately below.
            unsafe { slice::from_raw_parts(base, count) }.to_vec()
        } else {
            Vec::new()
        };
        gl!(g, g_free(base.cast()));
        Ok(owned)
    })?;
    let size = sqInt::try_from(attrs.len()).map_err(|_| PrimErr::LimitExceeded)?;
    let bitmap = vm.instantiate(vm.class_bitmap()?, size)?;
    vm.write_words(bitmap, 0, &attrs)?;
    Ok(bitmap)
}
