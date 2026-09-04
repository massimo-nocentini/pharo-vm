//! `PangoFontDescription`: a request for a font, and the 33 primitives that
//! build one.
//!
//! A description is not a font. It is a *pattern* -- family, style, weight,
//! size and the rest, each field either set or unset -- that a font map later
//! matches against what is installed. That is why every getter here has a
//! companion bit in [`primitiveFontDescriptionGetSetFields`]: "weight 400"
//! and "weight unspecified" are different requests, and `get_weight` answers
//! 400 for both.
//!
//! Three things this module exists to get right.
//!
//! **Weight and width are continuous ranges, not ordinals.** Pango's header
//! calls weight "a numeric value ranging from 100 to 1000" and the twelve
//! named constants merely "some common, predefined values". A variable font's
//! weight axis legitimately produces 450, so membership of the named set is
//! the wrong check; so is `0..=11`, which would reject every legal value and
//! accept near-zero garbage. `PangoWidth` (1.58) repeats the shape at
//! 500..=2000, and although its names line up with `PangoStretch`'s the two
//! are not interchangeable.
//!
//! **`set_size` and `set_absolute_size` do not measure the same thing.**
//! `set_size` takes points x PANGO_SCALE; `set_absolute_size` takes device
//! units x PANGO_SCALE and takes a `double` only to buy sub-unit precision.
//! Passing 10.0 to the second gives a font about a hundredth of a device unit
//! tall, not a ten-pixel one. There is no `get_absolute_size`: one `gint`
//! getter serves both, and it is uninterpretable without
//! [`primitiveFontDescriptionGetSizeIsAbsolute`].
//!
//! **No primitive here reaches a `_static` setter.** `set_family_static` and
//! its two siblings store the caller's `char *` by pointer, on the promise
//! that it outlives the description. Every string a primitive receives is a
//! `CString` dropped when the primitive returns, so each of them would be a
//! use-after-free. They are declared in `ffi.rs` for an honest symbol census
//! and are reachable from nothing.

use core::ffi::c_char;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    borrowed_str, pango, pg, GStr, PANGO_FONT_MASK_ALL, PANGO_GRAVITY_MAX, PANGO_STRETCH_MAX,
    PANGO_STYLE_MAX, PANGO_VARIANT_MAX, PANGO_VARIANT_MAX_PRE_1_50, PANGO_VERSION_1_50,
    PANGO_VERSION_1_58, PANGO_WEIGHT_MAX, PANGO_WEIGHT_MIN, PANGO_WIDTH_MAX, PANGO_WIDTH_MIN,
};
use pharo_vm_plugin::handles::Handle;

use crate::resources::{
    as_c_int_positive, as_gboolean, destroy_font_desc, enum_in, enum_in_or_since, enum_in_since,
    from_gboolean, register_font_desc, runtime_version, utf8_cstring, with_font_desc, FontDesc,
};

/// A `const char *` getter's answer as a Pharo String, or nil for NULL.
///
/// The three borrowed getters in this module -- family, variations, features
/// -- all answer NULL when the field was never set [measured on a fresh
/// description], and nil is the only answer that keeps them agreeing with
/// `get_set_fields`. An empty String would be indistinguishable from a family
/// the image had genuinely set to the empty string, and the image would have
/// to consult the mask to tell them apart -- which is exactly the step that
/// gets skipped.
fn borrowed_or_nil(vm: &Interp, ptr: *const c_char) -> PrimResult<Oop> {
    // SAFETY: `ptr` came from a transfer-none getter on a description that
    // stays alive for the whole primitive, so it is null or NUL-terminated
    // and valid until the copy inside `borrowed_str` finishes.
    match unsafe { borrowed_str(ptr) } {
        Some(s) => vm.string(&s),
        None => vm.nil(),
    }
}

// ---- lifecycle and identity ---------------------------------------------

/// `pango_font_description_new`: an empty description, every field unset.
///
/// Answers a handle the image must eventually pass to
/// [`primitiveFontDescriptionDestroy`]. A description is not reference
/// counted -- `pango_font_description_free` is the only verb -- so nothing
/// else in this plugin can keep it alive on the image's behalf.
#[pharo_primitive]
fn primitiveFontDescriptionNew(vm: &Interp) -> PrimResult<Handle<FontDesc>> {
    vm.expect_argument_count(0)?;
    let p = pango()?;
    register_font_desc(pg!(p, pango_font_description_new()))
}

/// `pango_font_description_from_string`: parses "Sans Bold Italic 12".
///
/// The trailing number is a size **in points**, which Pango multiplies by
/// PANGO_SCALE for the size field; a trailing "12px" is an absolute size in
/// device units instead. Pango never fails this parse -- an unrecognised word
/// becomes part of the family name -- so a typo answers a description that
/// simply will not match, not an error.
#[pharo_primitive]
fn primitiveFontDescriptionFromString(vm: &Interp, s: Oop) -> PrimResult<Handle<FontDesc>> {
    let p = pango()?;
    let text = utf8_cstring(vm, s)?;
    register_font_desc(pg!(p, pango_font_description_from_string(text.as_ptr())))
}

/// `pango_font_description_copy`: a deep copy, answering a fresh handle.
///
/// `copy`, never `copy_static`. The static variant aliases the source's
/// family, variations and features strings rather than duplicating them, and
/// is safe only while the source outlives the copy -- a lifetime the image
/// has no way to express, and one this plugin could not enforce for it.
#[pharo_primitive]
fn primitiveFontDescriptionCopy(_vm: &Interp, d: sqInt) -> PrimResult<Handle<FontDesc>> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        register_font_desc(pg!(p, pango_font_description_copy(desc)))
    })
}

/// `pango_font_description_free`. Destroying twice fails the second time.
#[pharo_primitive]
fn primitiveFontDescriptionDestroy(_vm: &Interp, d: sqInt) -> PrimResult<()> {
    destroy_font_desc(d)
}

/// `pango_font_description_to_string`: the round trip back to
/// [`primitiveFontDescriptionFromString`]'s input.
///
/// Only the fields that are set appear, so this is also the cheapest way to
/// see a description's mask from the image. The size is printed in points.
///
/// The `char *` it answers is glib's to free, not a static string, and it is
/// released here -- this and `to_filename` are the two likeliest leaks in the
/// whole plugin, which is why the owned/borrowed distinction is carried in the
/// return type rather than in a comment.
#[pharo_primitive]
fn primitiveFontDescriptionToString(_vm: &Interp, d: sqInt) -> PrimResult<String> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        let raw = pg!(p, pango_font_description_to_string(desc));
        // SAFETY: the header answers `char *`, which in Pango means
        // transfer-full without exception, so this is a glib allocation
        // nothing else frees; `GStr` g_frees it exactly once, on drop.
        let owned = unsafe { GStr::from_owned(raw) };
        Ok(owned.to_string_lossy_owned())
    })
}

/// `pango_font_description_to_filename`: the same text lowercased and stripped
/// of everything a filename cannot carry.
///
/// Nullable, and an empty String is what NULL crosses as -- there is no
/// filename for a description with no family. The image should treat empty as
/// "no answer" rather than as a name.
#[pharo_primitive]
fn primitiveFontDescriptionToFilename(_vm: &Interp, d: sqInt) -> PrimResult<String> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        let raw = pg!(p, pango_font_description_to_filename(desc));
        // SAFETY: `char *` again, transfer-full and nullable; `GStr` handles
        // NULL by freeing nothing and answering an empty String.
        let owned = unsafe { GStr::from_owned(raw) };
        Ok(owned.to_string_lossy_owned())
    })
}

/// `pango_font_description_equal`: field-by-field equality.
///
/// **Equality compares values, not masks.** Two descriptions can be equal and
/// still disagree about which fields were explicitly set, because an unset
/// field compares as its default. So a Smalltalk `=`/`hash` pair built on this
/// and [`primitiveFontDescriptionHash`] is coherent with itself, but
/// `a = b` does not imply `a getSetFields = b getSetFields`.
#[pharo_primitive]
fn primitiveFontDescriptionEqual(_vm: &Interp, a: sqInt, b: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    // The inner lookup takes the registry lock again, which is safe because
    // `with_font_desc` releases it before running the closure: it resolves the
    // handle to a pointer and hands that out.
    with_font_desc(a, |first| {
        with_font_desc(b, |second| {
            Ok(from_gboolean(pg!(
                p,
                pango_font_description_equal(first, second)
            )))
        })
    })
}

/// `pango_font_description_hash`, consistent with
/// [`primitiveFontDescriptionEqual`] and documented as independent of the
/// mask.
///
/// The `guint` crosses whole rather than truncated. It routinely exceeds
/// `i32::MAX` -- a fresh description hashes to 3147890688 here [measured] --
/// so on a 64-bit image it is widened and boxed as a LargeInteger if need be,
/// and on a 32-bit one, where `sqInt` cannot hold it at all, the primitive
/// declines. Casting instead would answer a negative number that two different
/// descriptions could share, which is the one thing a hash may not do.
#[pharo_primitive]
fn primitiveFontDescriptionHash(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        let hash = pg!(p, pango_font_description_hash(desc));
        sqInt::try_from(hash).map_err(|_| PrimErr::LimitExceeded)
    })
}

/// `pango_font_description_merge`: copies `from`'s set fields into `into`,
/// overwriting fields already set there only when `replace` is true.
///
/// `merge`, never `merge_static`; see [`primitiveFontDescriptionCopy`] for
/// why. Pango treats a NULL source as a no-op, but a handle of 0 is refused
/// with `NotFound` here instead: the image asking to merge something it has
/// already destroyed is a bug, and silently doing nothing hides it.
#[pharo_primitive]
fn primitiveFontDescriptionMerge(
    _vm: &Interp,
    into: sqInt,
    from: sqInt,
    replace: bool,
) -> PrimResult<()> {
    let p = pango()?;
    let replace = as_gboolean(replace);
    with_font_desc(into, |target| {
        with_font_desc(from, |source| {
            pg!(p, pango_font_description_merge(target, source, replace));
            Ok(())
        })
    })
}

// ---- family -------------------------------------------------------------

/// `pango_font_description_set_family`, which **copies** the string.
///
/// The copying setter is the only one bound. `set_family_static` would keep
/// the pointer, and the `CString` behind it dies when this primitive returns.
///
/// A comma-separated list is legal and means "the first of these that exists":
/// Pango's own generic families are `Serif`, `Sans`, `Monospace`, `Cursive`,
/// `Fantasy` and `System-ui`.
#[pharo_primitive]
fn primitiveFontDescriptionSetFamily(vm: &Interp, d: sqInt, family: Oop) -> PrimResult<()> {
    let p = pango()?;
    let family = utf8_cstring(vm, family)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_family(desc, family.as_ptr()));
        Ok(())
    })
}

/// `pango_font_description_get_family`: the family name, or nil when unset.
///
/// Borrowed -- the header says `const char *`, and the string lives and dies
/// with the description -- so nothing is freed here.
#[pharo_primitive]
fn primitiveFontDescriptionGetFamily(vm: &Interp, d: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        borrowed_or_nil(vm, pg!(p, pango_font_description_get_family(desc)))
    })
}

// ---- style, variant, weight, stretch, width ------------------------------

/// `pango_font_description_set_style`. `PangoStyle`: 0 normal, 1 oblique,
/// 2 italic.
///
/// Range-checked here because Pango does not check: `set_style(desc, 99)`
/// stores 99 [measured], and the matcher then falls through every arm of its
/// switch and answers a font nobody asked for.
#[pharo_primitive]
fn primitiveFontDescriptionSetStyle(_vm: &Interp, d: sqInt, v: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let v = enum_in(v, 0, PANGO_STYLE_MAX)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_style(desc, v));
        Ok(())
    })
}

/// `pango_font_description_get_style`, 0..=2. Answers 0 when unset.
#[pharo_primitive]
fn primitiveFontDescriptionGetStyle(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_style(desc)) as sqInt)
    })
}

/// `pango_font_description_set_variant`. `PangoVariant`: 0 normal,
/// 1 small caps, and 2..=6 (all-small-caps, petite caps, all-petite-caps,
/// unicase, title caps) which arrived in Pango 1.50.
///
/// The four late values are refused with `Unsupported` on an older Pango
/// rather than stored. Pango would accept them silently and then ignore them,
/// and "the small caps I asked for never appeared" is a much harder thing to
/// diagnose from the image than a primitive that says so.
#[pharo_primitive]
fn primitiveFontDescriptionSetVariant(_vm: &Interp, d: sqInt, v: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let v = enum_in_or_since(
        v,
        0,
        PANGO_VARIANT_MAX_PRE_1_50,
        PANGO_VARIANT_MAX,
        PANGO_VERSION_1_50,
        runtime_version(),
    )?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_variant(desc, v));
        Ok(())
    })
}

/// `pango_font_description_get_variant`, 0..=6. Answers 0 when unset.
#[pharo_primitive]
fn primitiveFontDescriptionGetVariant(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_variant(desc)) as sqInt)
    })
}

/// `pango_font_description_set_weight`. **A continuous 100..=1000**, not an
/// ordinal.
///
/// The named constants (thin 100, normal 400, bold 700, ultraheavy 1000) are
/// waypoints in that range, not the whole of it: a variable font's weight axis
/// produces 450 as readily as 400, and Pango stores whatever it is given
/// [measured: `set_weight(450)` reads back as 450]. Checking membership of the
/// named set would reject that; the range check exists only to turn a
/// Smalltalk typo into a failure at the point of the mistake rather than into
/// a font that quietly fails to match.
#[pharo_primitive]
fn primitiveFontDescriptionSetWeight(_vm: &Interp, d: sqInt, v: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let v = enum_in(v, PANGO_WEIGHT_MIN, PANGO_WEIGHT_MAX)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_weight(desc, v));
        Ok(())
    })
}

/// `pango_font_description_get_weight`, 100..=1000. Answers 400 when unset.
#[pharo_primitive]
fn primitiveFontDescriptionGetWeight(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_weight(desc)) as sqInt)
    })
}

/// `pango_font_description_set_stretch`. `PangoStretch`: 0 ultra-condensed
/// through 4 normal to 8 ultra-expanded.
///
/// Nine ordinal values, and genuinely an ordinal -- unlike its 1.58
/// counterpart [`primitiveFontDescriptionSetWidth`], which spells the same
/// nine names as numbers 500..=2000 and shares this field's mask bit.
#[pharo_primitive]
fn primitiveFontDescriptionSetStretch(_vm: &Interp, d: sqInt, v: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let v = enum_in(v, 0, PANGO_STRETCH_MAX)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_stretch(desc, v));
        Ok(())
    })
}

/// `pango_font_description_get_stretch`, 0..=8. Answers 4 (normal) when unset.
#[pharo_primitive]
fn primitiveFontDescriptionGetStretch(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_stretch(desc)) as sqInt)
    })
}

/// `pango_font_description_set_width`, Pango 1.58 and later. **A continuous
/// 500..=2000**, the same shape as weight.
///
/// `PangoWidth` is `PangoStretch` re-expressed so that intermediate values fit
/// between the named ones -- 500 ultra-condensed, 1000 normal, 2000
/// ultra-expanded -- and the two are **the same field**: they share mask bit
/// `1 << 4`, so setting one sets the other. They are not interchangeable as
/// numbers, though; 4 means normal to `set_stretch` and is out of range here.
///
/// Fails with `Unsupported` on a Pango older than 1.58, which does not export
/// the entry point at all.
#[pharo_primitive]
fn primitiveFontDescriptionSetWidth(_vm: &Interp, d: sqInt, v: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let v = enum_in_since(
        v,
        PANGO_WIDTH_MIN,
        PANGO_WIDTH_MAX,
        PANGO_VERSION_1_58,
        runtime_version(),
    )?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_width(desc, v));
        Ok(())
    })
}

/// `pango_font_description_get_width`, 500..=2000, Pango 1.58 and later.
/// Answers 1000 (normal) when unset, and `Unsupported` on an older Pango.
#[pharo_primitive]
fn primitiveFontDescriptionGetWidth(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_width(desc)) as sqInt)
    })
}

// ---- size ---------------------------------------------------------------

/// `pango_font_description_set_size`: **points x PANGO_SCALE**, as an integer.
///
/// A 12 point font is `12 * 1024 = 12288`. How many device units that becomes
/// depends on the context's resolution -- at the usual 96 dpi a 10 point font
/// is 13.3 pixels -- which is the whole difference between this and
/// [`primitiveFontDescriptionSetAbsoluteSize`], and the reason both exist.
/// Setting either clears the other.
///
/// Negative sizes are refused before the call. Pango answers one with
/// `g_return_if_fail (size >= 0)` [measured: a Pango-CRITICAL and the field
/// left untouched], and under `G_DEBUG=fatal-criticals` a glib critical aborts
/// the process -- taking the image with it, from a primitive that had a
/// perfectly good way to fail.
#[pharo_primitive]
fn primitiveFontDescriptionSetSize(_vm: &Interp, d: sqInt, points_x_scale: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let size = as_c_int_positive(points_x_scale)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_size(desc, size));
        Ok(())
    })
}

/// `pango_font_description_get_size`: the size field, **uninterpretable on its
/// own**.
///
/// Points x PANGO_SCALE or device units x PANGO_SCALE, depending on which
/// setter last ran -- there is no `get_absolute_size`, one getter serves both
/// -- so the image must pair every reading with
/// [`primitiveFontDescriptionGetSizeIsAbsolute`]. Answers 0 when the field was
/// never set *and* when it was explicitly set to 0; only
/// [`primitiveFontDescriptionGetSetFields`] tells those apart.
#[pharo_primitive]
fn primitiveFontDescriptionGetSize(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_size(desc)) as sqInt)
    })
}

/// `pango_font_description_set_absolute_size`: **device units x PANGO_SCALE**,
/// as a double.
///
/// The classic Pango mistake lives here. The `double` buys sub-unit precision;
/// it does **not** change the scale. A ten-pixel font on a pixel backend is
/// `10 * 1024 = 10240.0`, and passing `10.0` asks for a font roughly one
/// hundredth of a pixel tall -- which renders as nothing at all, with no error
/// anywhere. Use the plugin's `primitiveUnitsFromDouble` to build the argument
/// rather than multiplying by a hard-coded 1024.
///
/// Negative and non-finite sizes are refused for the reason given on
/// [`primitiveFontDescriptionSetSize`]: Pango's own check is a
/// `g_return_if_fail`, which is a process abort under
/// `G_DEBUG=fatal-criticals`.
#[pharo_primitive]
fn primitiveFontDescriptionSetAbsoluteSize(
    _vm: &Interp,
    d: sqInt,
    device_x_scale: f64,
) -> PrimResult<()> {
    let p = pango()?;
    if !device_x_scale.is_finite() || device_x_scale < 0.0 {
        return Err(PrimErr::BadArgument);
    }
    with_font_desc(d, |desc| {
        pg!(
            p,
            pango_font_description_set_absolute_size(desc, device_x_scale)
        );
        Ok(())
    })
}

/// `pango_font_description_get_size_is_absolute`: how to read
/// [`primitiveFontDescriptionGetSize`].
///
/// True means the size is in device units, false that it is in points. The
/// underlying `gboolean` is a four-byte `int`, not a C99 `_Bool`, and is
/// converted rather than reinterpreted -- Pango is entitled to answer any
/// non-zero value for true.
#[pharo_primitive]
fn primitiveFontDescriptionGetSizeIsAbsolute(_vm: &Interp, d: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(from_gboolean(pg!(
            p,
            pango_font_description_get_size_is_absolute(desc)
        )))
    })
}

// ---- gravity ------------------------------------------------------------

/// `pango_font_description_set_gravity`. `PangoGravity`: 0 south, 1 east,
/// 2 north, 3 west, 4 auto.
///
/// **4 unsets the field rather than setting it to auto.** After
/// `set_gravity(desc, 4)` the gravity bit of
/// [`primitiveFontDescriptionGetSetFields`] reads clear [measured], and the
/// description no longer constrains gravity at all. The same constant means
/// what it says on a context's base gravity, one call away in this same
/// plugin. This is Pango's behaviour, not a workaround, and it is documented
/// here rather than corrected.
#[pharo_primitive]
fn primitiveFontDescriptionSetGravity(_vm: &Interp, d: sqInt, g: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let g = enum_in(g, 0, PANGO_GRAVITY_MAX)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_set_gravity(desc, g));
        Ok(())
    })
}

/// `pango_font_description_get_gravity`, 0..=4. Answers 0 (south) when unset,
/// which is why the mask matters here too.
#[pharo_primitive]
fn primitiveFontDescriptionGetGravity(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_gravity(desc)) as sqInt)
    })
}

// ---- variations and features --------------------------------------------

/// `pango_font_description_set_variations`, Pango 1.42 and later: OpenType
/// variation axes, as `"wght=450,wdth=87.5"`.
///
/// Copies the string; the `_static` sibling is unreachable, as everywhere in
/// this module. Pango does not validate the syntax -- an unparseable axis is
/// dropped when the font is loaded, not here. `Unsupported` on an older Pango.
#[pharo_primitive]
fn primitiveFontDescriptionSetVariations(vm: &Interp, d: sqInt, s: Oop) -> PrimResult<()> {
    let p = pango()?;
    let variations = utf8_cstring(vm, s)?;
    with_font_desc(d, |desc| {
        pg!(
            p,
            pango_font_description_set_variations(desc, variations.as_ptr())
        );
        Ok(())
    })
}

/// `pango_font_description_get_variations`: the variation string, or nil when
/// unset. Borrowed; nothing is freed.
#[pharo_primitive]
fn primitiveFontDescriptionGetVariations(vm: &Interp, d: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        borrowed_or_nil(vm, pg!(p, pango_font_description_get_variations(desc)))
    })
}

/// `pango_font_description_set_features`, Pango 1.56 and later: OpenType
/// feature tags, as `"liga=0,dlig=1"`.
///
/// The setter is 1.56; the getter is older, so a Pango between 1.42 and 1.56
/// can read the field but not write it, and this primitive answers
/// `Unsupported` there. Copies the string.
#[pharo_primitive]
fn primitiveFontDescriptionSetFeatures(vm: &Interp, d: sqInt, s: Oop) -> PrimResult<()> {
    let p = pango()?;
    let features = utf8_cstring(vm, s)?;
    with_font_desc(d, |desc| {
        pg!(
            p,
            pango_font_description_set_features(desc, features.as_ptr())
        );
        Ok(())
    })
}

/// `pango_font_description_get_features`: the feature string, or nil when
/// unset. Borrowed; nothing is freed.
#[pharo_primitive]
fn primitiveFontDescriptionGetFeatures(vm: &Interp, d: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        borrowed_or_nil(vm, pg!(p, pango_font_description_get_features(desc)))
    })
}

// ---- the mask -----------------------------------------------------------

/// `pango_font_description_get_set_fields`: which fields were explicitly set,
/// as a `PangoFontMask` bit set.
///
/// family 1, style 2, variant 4, weight 8, **width and stretch both 16**,
/// size 32, gravity 64, variations 128, features 256, colour 512.
///
/// The shared bit is not a transcription slip: the header calls
/// `PANGO_FONT_MASK_WIDTH` "an alias for STRETCH", because 1.58's `PangoWidth`
/// re-expresses the same field. An image-side mask class must not offer
/// `widthIsSet` and `stretchIsSet` as predicates that could ever disagree.
///
/// This is the companion every getter in this module needs: each of them
/// answers a plausible default for a field that was never set.
#[pharo_primitive]
fn primitiveFontDescriptionGetSetFields(_vm: &Interp, d: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_desc(d, |desc| {
        Ok(pg!(p, pango_font_description_get_set_fields(desc)) as sqInt)
    })
}

/// `pango_font_description_unset_fields`: clears the fields named by a
/// `PangoFontMask`, returning them to "unspecified".
///
/// Not the same as setting them to a default. An unset field is one the font
/// map is free to fill from elsewhere -- from the context's description, or
/// from a merge -- and 0 here is a legal no-op.
///
/// Bits above 512 are refused, because they mean nothing to any Pango and the
/// image asking for one has miscomputed its mask. Bits Pango's own version
/// predates are *not* refused: on a Pango older than 1.56 the features and
/// colour bits name fields that do not exist, and unsetting them does nothing,
/// which is harmless and lets one image-side mask serve every runtime.
#[pharo_primitive]
fn primitiveFontDescriptionUnsetFields(_vm: &Interp, d: sqInt, mask: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let mask = enum_in(mask, 0, PANGO_FONT_MASK_ALL)?;
    with_font_desc(d, |desc| {
        pg!(p, pango_font_description_unset_fields(desc, mask));
        Ok(())
    })
}
