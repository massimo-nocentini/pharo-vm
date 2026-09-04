//! `pango_parse_markup`, `PangoAttrList` and `PangoColor`.
//!
//! **v1 exposes no attribute construction at all.** Pango's markup language
//! already expresses every attribute the eighteen `pango_attr_*_new`
//! constructors offer, so an image that can build a string can build any
//! attribute set -- at the cost of one primitive instead of eighteen. More to
//! the point, `pango_attr_list_insert`, `_insert_before` and `_change` all
//! take their `PangoAttribute *` `transfer=full`: the list swallows it, the
//! caller must never touch it again, and a registry handle on an attribute
//! plus an insert primitive is a double free with eighteen chances to write
//! it. Markup has no attribute handles, so it cannot get this wrong. The
//! constructors stay declared in `ffi` from day one, so the v2 that wants
//! programmatic ranges -- syntax highlighting, search hits -- is
//! primitives-only work with no new FFI risk.
//!
//! # Units and indices
//!
//! Nothing in this file speaks Pango units or pixels: an attribute list
//! carries no geometry. What it does carry is **byte** offsets. A
//! `PangoAttribute`'s `start_index`/`end_index` and
//! [`primitiveAttrListSplice`]'s `pos` and `len` are all counted in UTF-8
//! bytes, not characters (pango-attributes.h:295-297 says "(in bytes)", and
//! parsing `"aé<b>Bold</b>"` was measured to give start 3 where the character
//! offset is 2). Pharo code thinks in characters and the two agree for ASCII,
//! so a confusion here passes every English-language test and then corrupts
//! the first time someone types an accent.
//!
//! # Colour
//!
//! Channels cross this boundary as Pango's own **16-bit** values, 0..=65535,
//! and the image converts. The conversion is `v * 257` up and
//! `(v + 128) / 257` down, never `v << 8` and `v >> 8`: `0xFF << 8` is 65280,
//! so white would not round-trip. That `* 257` is Pango's own rule and not an
//! invention here -- `pango_color_parse("#3366cc")` answers red 13107, which
//! is exactly `0x33 * 257`.

use core::ffi::{c_char, c_int};
use std::sync::{Mutex, PoisonError};

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{self, gunichar, pango, pg, GStr, PangoAttrList, PangoColor};
use crate::resources::{
    as_c_int_positive, destroy_attr_list, from_gboolean, int_array, register_attr_list,
    take_gerror, utf8_cstring, utf8_text, with_attr_list,
};

// ---- the last markup error ----------------------------------------------

/// The code reported when a parse failed and glib set no `GError`.
///
/// Every `GMarkupError` enumerator is non-negative (gmarkup.h:51-63 starts at
/// `G_MARKUP_ERROR_BAD_UTF8` = 0), so a negative code cannot collide with a
/// real one and the image can test for it.
const NO_ERROR_CODE: c_int = -1;

/// What [`primitiveParseMarkup`] copies out of a `GError` before freeing it.
struct MarkupError {
    code: c_int,
    message: String,
}

/// The last markup failure, for the two primitives that report it.
///
/// A `PrimErr` is one small integer and the whole value of this primitive is
/// the sentence behind it -- "Error on line 1 char 24: Element “markup” was
/// closed, but the currently open element is “b”" tells the image author what
/// to fix, and `BadArgument` does not. This is the shape sdl3-plugin's
/// `primitiveGetError` already uses, for the same reason.
static LAST_MARKUP_ERROR: Mutex<Option<MarkupError>> = Mutex::new(None);

/// The stored failure, with a poisoned lock treated as an ordinary one.
///
/// A panic inside a primitive is caught and turned into a failure, so a
/// poisoned mutex is not a reason to stop answering: the value behind it is a
/// message and a code, and neither can be left half-written.
fn last_markup_error() -> std::sync::MutexGuard<'static, Option<MarkupError>> {
    LAST_MARKUP_ERROR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Copies a `GError` into [`LAST_MARKUP_ERROR`] and frees it.
///
/// `pub(crate)` because `layout::validate_markup` parses markup too, and an
/// image told to read [`primitiveLastMarkupError`] after a failure must not be
/// handed the message from some earlier, unrelated parse. All three markup
/// entry points write the store, so all three are truthful afterwards.
///
/// # Safety
///
/// `error` must be null, or a `GError *` glib allocated and handed over
/// `transfer=full`, as every `GError **` out-parameter in Pango is. It must
/// not be read again afterwards: this frees it.
pub(crate) unsafe fn record_markup_error(error: *mut ffi::GError) {
    // SAFETY: forwarded from this function's own contract. `take_gerror`
    // copies the message into a Rust String *before* `g_error_free` runs,
    // which is the only order that works -- the message dies with the GError.
    let taken = unsafe { take_gerror(error) };
    *last_markup_error() = Some(match taken {
        Some((code, message)) => MarkupError { code, message },
        // glib permits a FALSE return with no GError set. Recording that as a
        // state of its own beats leaving the previous failure's message in
        // place, where the image would read it as this one's.
        None => MarkupError {
            code: NO_ERROR_CODE,
            message: String::new(),
        },
    });
}

// ---- owning a list across a window where Pharo can still fail -------------

/// A `PangoAttrList` this plugin owns and has not yet handed to the registry.
///
/// The window between a successful `pango_parse_markup` and the point where
/// the list lands in the registry is not safe: `instantiate`, `store_pointer`
/// and `string` all answer `NoMemory`, and a bare `?` there would leak a
/// `PangoAttrList` reference with nothing left holding a pointer to it. This
/// guard closes the window, exactly as `GStr` closes the matching one around
/// the `char *` the same call produces.
struct OwnedAttrList(*mut PangoAttrList);

impl OwnedAttrList {
    /// Hands the pointer on, leaving nothing for [`Drop`] to release.
    fn take(&mut self) -> *mut PangoAttrList {
        core::mem::replace(&mut self.0, core::ptr::null_mut())
    }
}

impl Drop for OwnedAttrList {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // The one place in this file that reads a raw entry rather than going
        // through `pg!`: a `Drop` has no way to report `Unsupported`, and if
        // `pango_attr_list_unref` were missing there would be no Pango that
        // could have produced this pointer either, so silence loses nothing.
        let Ok(p) = pango() else { return };
        let Some(unref) = p.pango_attr_list_unref else {
            return;
        };
        // SAFETY: `self.0` is non-null by the check above and is a list this
        // value owns one reference on, released exactly once because `take`
        // nulls the field whenever the reference goes elsewhere.
        unsafe { unref(self.0) };
    }
}

/// Resolves two attribute-list handles, one at a time.
///
/// Nesting one `with_attr_list` inside another would also work: the closure
/// `resources::with_attr_list` hands to `Registry::with` is only
/// `AttrList::as_ptr`, so the `MutexGuard` is dropped before the caller's own
/// closure runs -- which is why `primitiveFontDescriptionEqual` may nest two
/// `with_font_desc` calls, and why nothing in this crate ever holds a registry
/// lock across a call into Pango. That last part is the rule worth keeping:
/// hold one across a foreign call and the interpreter thread deadlocks with no
/// primitive failure, no timeout and nothing in the image to catch. Resolving
/// both handles up front makes the rule visible instead of merely observed.
/// The pointers stay good in between because a primitive runs to completion
/// before the image can destroy anything.
fn two_lists(a: sqInt, b: sqInt) -> PrimResult<(*mut PangoAttrList, *mut PangoAttrList)> {
    let first = with_attr_list(a, Ok)?;
    let second = with_attr_list(b, Ok)?;
    Ok((first, second))
}

/// Narrows an image integer to one of Pango's 16-bit colour channels.
fn channel(value: sqInt) -> PrimResult<u16> {
    u16::try_from(value).map_err(|_| PrimErr::BadArgument)
}

/// Validates an accelerator marker as a Unicode scalar value.
///
/// 0 means "no accelerator" and is also a perfectly good scalar value, so one
/// check covers both. A surrogate, or anything past U+10FFFF, is not a
/// `gunichar` Pango could ever match -- it would quietly disable the
/// accelerator rather than fail, and the image would see markup that parsed
/// but underlined nothing.
fn accel_marker_of(value: sqInt) -> PrimResult<gunichar> {
    let raw = u32::try_from(value).map_err(|_| PrimErr::BadArgument)?;
    if char::from_u32(raw).is_none() {
        return Err(PrimErr::BadArgument);
    }
    Ok(raw)
}

/// Builds the three-slot answer, once the list is registered.
///
/// Split out because every line of it can fail with `NoMemory`, and the caller
/// has to retire the handle when one does -- see [`primitiveParseMarkup`].
fn parse_result(vm: &Interp, handle: sqInt, text: &str, accel: gunichar) -> PrimResult<Oop> {
    let array = vm.instantiate(vm.class_array()?, 3)?;
    let handle_oop = vm.integer_checked(handle)?;
    vm.store_pointer(0, array, handle_oop)?;
    let text_oop = vm.string(text)?;
    vm.store_pointer(1, array, text_oop)?;
    // Pango leaves `accel_char` at 0 when the markup carried no accelerator,
    // and 0 is a legal code point, so nil is what distinguishes "none" from
    // "U+0000" -- which markup cannot contain anyway, being invalid UTF-8 XML.
    let accel_oop = if accel == 0 {
        vm.nil()?
    } else {
        vm.integer_checked(sqInt::try_from(accel).map_err(|_| PrimErr::LimitExceeded)?)?
    };
    vm.store_pointer(2, array, accel_oop)?;
    Ok(array)
}

// ---- parsing -------------------------------------------------------------

/// `pango_parse_markup`. Answers `{attrListHandle. plainText. accelCharOrNil}`.
///
/// The markup is passed with its explicit **byte** length rather than as a
/// NUL-terminated string, so a Smalltalk String holding a NUL is parsed as
/// written instead of being truncated at it.
///
/// `accelMarker` is the code point that precedes an accelerator -- `$_` is the
/// usual one -- or 0 for none. Every character it marks also gains a
/// `PANGO_UNDERLINE_LOW` attribute in the answered list, and the marker itself
/// is stripped from `plainText`.
///
/// The handle is on a list the image now owns and must eventually pass to
/// [`primitiveAttrListDestroy`]; `plainText` is a fresh Smalltalk String.
///
/// On a parse failure this answers `BadArgument` and stores the reason, which
/// [`primitiveLastMarkupError`] and [`primitiveLastMarkupErrorCode`] report.
/// That pairing is the whole reason to prefer this over
/// `pango_layout_set_markup`, which takes no `GError **` and reports a bad
/// markup string only as a warning on stderr.
#[pharo_primitive]
fn primitiveParseMarkup(vm: &Interp, markup: Oop, accel_marker: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let markup = utf8_text(vm, markup)?;
    let length = c_int::try_from(markup.len()).map_err(|_| PrimErr::LimitExceeded)?;
    let marker = accel_marker_of(accel_marker)?;

    let mut list: *mut PangoAttrList = core::ptr::null_mut();
    let mut text: *mut c_char = core::ptr::null_mut();
    let mut accel: gunichar = 0;
    let mut error: *mut ffi::GError = core::ptr::null_mut();

    let ok = pg!(
        p,
        pango_parse_markup(
            markup.as_ptr().cast::<c_char>(),
            length,
            marker,
            &mut list,
            &mut text,
            &mut accel,
            &mut error,
        )
    );

    if !from_gboolean(ok) {
        // Nothing but the GError needs releasing here. Pango documents, and it
        // was measured with `0xdeadbeef` sentinels that survived, that on a
        // failure "none of the output arguments are touched except for
        // @error" -- so a cleanup over `list` and `text` would be freeing
        // whatever this function put there, which is null.
        //
        // SAFETY: a FALSE return means `error` is either null or a GError this
        // call now owns, which is exactly `record_markup_error`'s contract.
        unsafe { record_markup_error(error) };
        return Err(PrimErr::BadArgument);
    }

    // From here both outputs are owned, and both guards release them on every
    // early return below.
    let mut list = OwnedAttrList(list);
    // SAFETY: `text` is the `g_malloc`'d NUL-terminated UTF-8 buffer the call
    // handed over `transfer=full`; `GStr` `g_free`s it, which is the only
    // correct verb -- `libc::free` across the glib heap is a crash on Windows.
    let text = unsafe { GStr::from_owned(text) };
    if list.0.is_null() || text.is_null() {
        return Err(PrimErr::NoCMemory);
    }
    let stripped = text.to_string_lossy_owned();

    let handle = register_attr_list(list.take())?;
    let built = parse_result(vm, handle, &stripped, accel);
    if built.is_err() {
        // The image never saw this handle, so nothing else would ever retire
        // it. Registering and then failing is rare -- it needs the image to be
        // out of memory -- but a leak that only happens under memory pressure
        // is the worst kind to diagnose.
        let _ = destroy_attr_list(handle);
    }
    built
}

/// The message behind the last markup failure, or an empty String.
///
/// Read it immediately after a markup primitive failed -- `BadArgument` from
/// [`primitiveParseMarkup`], `OperationFailed` from `primitiveLayoutSetMarkup`
/// and its accel variant, all three of which write this store. It is a *last*
/// error in the `SDL_GetError` sense and a success does not clear it.
///
/// Pango wraps the image's markup in a synthetic `<markup>` element before
/// parsing, so the character offset in the message counts into that wrapper
/// and not into the image's own string. The text is a diagnostic, not an
/// index.
#[pharo_primitive]
fn primitiveLastMarkupError(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    Ok(last_markup_error()
        .as_ref()
        .map_or_else(String::new, |e| e.message.clone()))
}

/// The `GError` code behind the last markup failure, or -1 for none.
///
/// The codes are glib's `GMarkupError` (gmarkup.h:51-63): 0 bad UTF-8,
/// 1 empty, 2 parse, 3 unknown element, 4 unknown attribute, 5 invalid
/// content, 6 missing attribute. Markup errors arrive in glib's own
/// `g-markup-error-quark` domain -- there is no Pango error domain at all --
/// so the image should read these as glib's, and -1 both when nothing has
/// failed yet and when a failure carried no `GError`.
#[pharo_primitive]
fn primitiveLastMarkupErrorCode(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(last_markup_error()
        .as_ref()
        .map_or(NO_ERROR_CODE, |e| e.code) as sqInt)
}

// ---- attribute lists -----------------------------------------------------

/// `pango_attr_list_new`. Answers a handle on an empty attribute list.
///
/// Empty and, in v1, un-fillable: there is no primitive that adds an
/// attribute. What it is for is `pango_attr_list_splice` and
/// `pango_layout_set_attributes` -- an empty list is how the image clears a
/// layout's attributes while keeping a handle to pass around.
#[pharo_primitive]
fn primitiveAttrListNew(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let p = pango()?;
    let list = pg!(p, pango_attr_list_new());
    if list.is_null() {
        return Err(PrimErr::NoCMemory);
    }
    register_attr_list(list)
}

/// `pango_attr_list_copy`. Answers a handle on an independent copy, or nil.
///
/// Copying is how the image gets a list it may change freely. A list reached
/// from a layout is the layout's own -- `PangoAttrList` is refcounted, so a
/// handle on one shares it rather than duplicating it -- and modifying that in
/// place would change the layout's text with it.
///
/// nil is Pango's documented answer to a NULL source, which a live handle can
/// never be; the branch is here because the gir marks the return nullable and
/// answering nil costs less than pretending it cannot happen.
#[pharo_primitive]
fn primitiveAttrListCopy(vm: &Interp, list: sqInt) -> PrimResult<Oop> {
    let source = with_attr_list(list, Ok)?;
    let p = pango()?;
    let copy = pg!(p, pango_attr_list_copy(source));
    if copy.is_null() {
        return vm.nil();
    }
    let handle = register_attr_list(copy)?;
    vm.integer_checked(handle)
}

/// `pango_attr_list_unref`. Releases one reference and retires the handle.
///
/// **Unref, not free** -- a `PangoAttrList` is refcounted and its neighbour in
/// every layout call, `PangoTabArray`, is not. A repeat call answers
/// `NotFound` rather than dropping a reference twice, because the registry's
/// generation counter has already retired the handle, so a Smalltalk finaliser
/// that runs twice is harmless.
#[pharo_primitive]
fn primitiveAttrListDestroy(_vm: &Interp, list: sqInt) -> PrimResult<()> {
    destroy_attr_list(list)
}

/// `pango_attr_list_equal`. Do two lists hold the same attributes?
///
/// Set equality over the attributes, not pointer identity and not order, so
/// two lists parsed from equivalent markup compare equal. The entry point
/// arrived in Pango 1.46, so an older install answers `Unsupported` -- which
/// the image can tell from `BadArgument` and act on, where a guessed `false`
/// would be indistinguishable from a real inequality.
#[pharo_primitive]
fn primitiveAttrListEqual(_vm: &Interp, list: sqInt, other: sqInt) -> PrimResult<bool> {
    let (first, second) = two_lists(list, other)?;
    let p = pango()?;
    Ok(from_gboolean(pg!(p, pango_attr_list_equal(first, second))))
}

/// `pango_attr_list_to_string`. The list in Pango's own textual form.
///
/// Round-trips through [`primitiveAttrListFromString`], and is the readable
/// half of debugging a layout whose attributes are not what the image thinks.
/// It is **not** markup: it is one line per attribute, `start end type value`.
/// Pango 1.50 and newer; `Unsupported` below that.
#[pharo_primitive]
fn primitiveAttrListToString(_vm: &Interp, list: sqInt) -> PrimResult<String> {
    let list = with_attr_list(list, Ok)?;
    let p = pango()?;
    let text = pg!(p, pango_attr_list_to_string(list));
    // SAFETY: transfer=full `char *` -- the header's non-const return is the
    // rule, with no exception anywhere in Pango -- so `GStr` owns it and
    // `g_free`s it on drop.
    Ok(unsafe { GStr::from_owned(text) }.to_string_lossy_owned())
}

/// `pango_attr_list_from_string`. Parses [`primitiveAttrListToString`]'s form.
///
/// Answers a handle the image owns, or nil when the text does not parse --
/// this entry point has no `GError **`, so nil is the whole diagnosis
/// available. Pango 1.50 and newer; `Unsupported` below that.
///
/// The text crosses as a C string, so an interior NUL is `BadArgument`: the
/// entry takes no length and would stop at it.
#[pharo_primitive]
fn primitiveAttrListFromString(vm: &Interp, text: Oop) -> PrimResult<Oop> {
    let text = utf8_cstring(vm, text)?;
    let p = pango()?;
    let list = pg!(p, pango_attr_list_from_string(text.as_ptr()));
    if list.is_null() {
        return vm.nil();
    }
    let handle = register_attr_list(list)?;
    vm.integer_checked(handle)
}

/// `pango_attr_list_splice`. Copies `other`'s attributes into `list` at `pos`,
/// having first shifted everything at or after `pos` along by `len` bytes.
///
/// `pos` and `len` are **byte** offsets into the UTF-8 text the list describes
/// -- see this module's header. Nothing here can check them: an attribute list
/// carries no text, so the plugin has no string to test `pos` against for a
/// character boundary. That check belongs to the image, which has the string.
///
/// `other` is `transfer=none`: its attributes are copied and the image still
/// owns it and must still destroy it.
///
/// `len == 0` is rejected. Pango reads it not as "splice nothing" but as "do
/// not limit `other`'s attributes at all", overlaying every one of them on the
/// whole of `list` -- so a caller who passes 0 meaning an empty splice gets a
/// merge of everything, which is as far from what they asked for as the API
/// allows. Splicing a list into itself is rejected for the same class of
/// reason: Pango would walk `other` while inserting into it.
#[pharo_primitive]
fn primitiveAttrListSplice(
    _vm: &Interp,
    list: sqInt,
    other: sqInt,
    pos: sqInt,
    len: sqInt,
) -> PrimResult<()> {
    let pos = as_c_int_positive(pos)?;
    let len = as_c_int_positive(len)?;
    if len == 0 {
        return Err(PrimErr::BadArgument);
    }
    let (target, source) = two_lists(list, other)?;
    if core::ptr::eq(target, source) {
        return Err(PrimErr::BadArgument);
    }
    let p = pango()?;
    pg!(p, pango_attr_list_splice(target, source, pos, len));
    Ok(())
}

// ---- colour --------------------------------------------------------------

/// `pango_color_parse_with_alpha`. Answers `{red. green. blue. alpha}`.
///
/// All four channels are Pango's **16-bit** values, 0..=65535; the image
/// converts to its own 8 bits with `(v + 128) / 257`, never `v >> 8`. Alpha is
/// 65535 when the spec carried none.
///
/// The spec is either a CSS colour name or `#rgb`, `#rrggbb`, `#rrrgggbbb` or
/// `#rrrrggggbbbb`, optionally with an alpha component. An unparseable one is
/// `BadArgument`, and the struct is left untouched rather than half-written --
/// measured, so this never reads a partially parsed colour.
///
/// Falls back to `pango_color_parse` on a Pango older than 1.46, where
/// `_with_alpha` does not exist; alpha is then always opaque, which is what
/// that Pango would have meant anyway.
#[pharo_primitive]
fn primitiveColorParse(vm: &Interp, spec: Oop) -> PrimResult<Oop> {
    let spec = utf8_cstring(vm, spec)?;
    let p = pango()?;
    let mut color = PangoColor::default();
    let mut alpha: u16 = u16::MAX;
    let ok = if p.pango_color_parse_with_alpha.is_some() {
        pg!(
            p,
            pango_color_parse_with_alpha(&mut color, &mut alpha, spec.as_ptr())
        )
    } else {
        pg!(p, pango_color_parse(&mut color, spec.as_ptr()))
    };
    if !from_gboolean(ok) {
        return Err(PrimErr::BadArgument);
    }
    int_array(
        vm,
        &[
            c_int::from(color.red),
            c_int::from(color.green),
            c_int::from(color.blue),
            c_int::from(alpha),
        ],
    )
}

/// `pango_color_to_string`. The colour as `#rrrrggggbbbb`.
///
/// The channels are Pango's **16-bit** values, 0..=65535, so an image with
/// 8-bit channels multiplies each by 257 first -- `0xFF * 257` is 65535 while
/// `0xFF << 8` is 65280, and only the first round-trips white.
///
/// The answer is always the widest form: `red` comes back as the thirteen
/// characters `#ffff00000000`, which is **not** a web colour and must not be
/// handed to an image-side `#rrggbb` parser as if it were.
#[pharo_primitive]
fn primitiveColorToString(
    _vm: &Interp,
    red: sqInt,
    green: sqInt,
    blue: sqInt,
) -> PrimResult<String> {
    let color = PangoColor {
        red: channel(red)?,
        green: channel(green)?,
        blue: channel(blue)?,
    };
    let p = pango()?;
    let text = pg!(p, pango_color_to_string(&color));
    // SAFETY: transfer=full `char *`, documented as "a newly-allocated text
    // string that must be freed with g_free()"; `GStr` is that free.
    Ok(unsafe { GStr::from_owned(text) }.to_string_lossy_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_accel_marker_that_is_not_a_scalar_value_is_refused() {
        assert_eq!(accel_marker_of(0), Ok(0));
        assert_eq!(accel_marker_of(sqInt::from(b'_')), Ok(u32::from(b'_')));
        assert_eq!(accel_marker_of(0x10_FFFF), Ok(0x10_FFFF));
        // A lone surrogate and anything past the last plane are `gunichar`
        // values Pango can hold but never match, so they would disable the
        // accelerator silently instead of failing.
        assert_eq!(accel_marker_of(0xD800), Err(PrimErr::BadArgument));
        assert_eq!(accel_marker_of(0x11_0000), Err(PrimErr::BadArgument));
        assert_eq!(accel_marker_of(-1), Err(PrimErr::BadArgument));
    }

    #[test]
    fn a_colour_channel_is_sixteen_bits_and_wider_values_are_refused() {
        assert_eq!(channel(0), Ok(0));
        assert_eq!(channel(65535), Ok(u16::MAX));
        assert_eq!(channel(65536), Err(PrimErr::BadArgument));
        assert_eq!(channel(-1), Err(PrimErr::BadArgument));
    }

    #[test]
    fn eight_bit_channels_widen_by_two_hundred_and_fifty_seven_not_by_a_shift() {
        // The rule the two colour primitives document, checked against the
        // value Pango itself was measured to produce for "#3366cc": a shift
        // would give 0x3300 == 13056 and white would not round-trip.
        assert_eq!(u16::from(0x33_u8) * 257, 13107);
        assert_eq!(u16::from(0xFF_u8) * 257, u16::MAX);
        assert_ne!(u16::from(0xFF_u8) << 8, u16::MAX);
        assert_eq!((u32::from(13107_u16) + 128) / 257, 0x33);
        assert_eq!((u32::from(u16::MAX) + 128) / 257, 0xFF);
    }

    #[test]
    fn a_recorded_failure_is_read_back_and_a_missing_one_reads_as_minus_one() {
        *last_markup_error() = None;
        assert!(last_markup_error().is_none());
        *last_markup_error() = Some(MarkupError {
            code: 2,
            message: "Error on line 1 char 24".to_owned(),
        });
        let stored = last_markup_error();
        let stored = stored.as_ref().expect("just stored");
        assert_eq!(stored.code, 2);
        assert_eq!(stored.message, "Error on line 1 char 24");
    }

    #[test]
    fn a_null_list_guard_releases_nothing() {
        // The guard's whole job is that `take` disarms it: a pointer handed to
        // the registry must not also be unref'd here.
        let mut guard = OwnedAttrList(core::ptr::null_mut());
        assert!(guard.take().is_null());
        assert!(guard.0.is_null());
        drop(guard);
    }
}
