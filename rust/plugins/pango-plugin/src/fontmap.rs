//! Font maps, contexts, font enumeration, metrics and languages.
//!
//! Three ownership rules shape everything in this file, and each of them is a
//! crash rather than a warning when broken.
//!
//! **A font map is owned or borrowed, and the two constructors sit one line
//! apart in the same header.** `pango_cairo_font_map_get_default` is
//! transfer-none -- the gir says it "is owned by Pango and must not be freed"
//! -- while `pango_cairo_font_map_new` beside it is transfer-full. Unref'ing
//! the default destroys text rendering for the whole image at a point
//! arbitrarily far from the bug, so the distinction is carried in
//! [`resources::FontMap`]'s `owned` flag rather than in the caller's memory.
//!
//! **Enumeration snapshots; no family or face handle ever exists.** Both
//! `list_families` and `list_faces` are `transfer=container`: the array is the
//! caller's to `g_free` and not one element is. A listed family sits at
//! refcount 1, held by the font map alone, so unref'ing one frees an object
//! the font map still points at. Worse, the elements have no lifetime the
//! plugin can observe -- `pango_font_map_changed()` or a fontconfig reload
//! invalidates them silently, and `Registry`'s generation counter guards
//! against a stale *handle*, not against a live handle to freed memory. So the
//! whole walk happens inside one primitive and only Strings and Booleans come
//! out.
//!
//! **`pango_context_get_font_description` answers a borrow and
//! `pango_font_face_describe` answers an owned value**, and both are
//! `PangoFontDescription *`. The first is copied before it is registered,
//! because registering the borrowed pointer means `pango_font_description_free`
//! on memory the context still owns; the second is turned into a String and
//! freed here, because it is the one call in the enumeration path that hands
//! the plugin something to release.
//!
//! Every integer that crosses from here is in **Pango units** (metrics, face
//! sizes) except the resolution, which is in dots per inch, and the serials,
//! which are opaque counters.

use core::ffi::c_int;
use core::ptr;

use pharo_vm_plugin::{pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use crate::ffi::{
    self, gl, glib, pango, pg, GError, PangoFontFace, PangoFontFamily, PangoLanguage,
    PANGO_DIRECTION_MAX, PANGO_GRAVITY_HINT_MAX, PANGO_GRAVITY_MAX,
};
use crate::resources::{
    self, as_gboolean, destroy_context, destroy_font_map, enum_in, font_map_is_borrowed,
    from_gboolean, register_context, register_font_desc, register_font_map_borrowed,
    register_font_map_owned, with_context, with_font_desc, with_font_map,
    with_optional_font_desc,
};

// ---- shared helpers ------------------------------------------------------

/// Answers the image's Boolean object for a Rust `bool`.
///
/// The enumeration primitives build Arrays whose elements are a mix of Strings
/// and Booleans, so they need the objects rather than
/// `Interp::return_bool`'s side effect.
fn boolean(vm: &Interp, flag: bool) -> PrimResult<Oop> {
    if flag {
        vm.true_object()
    } else {
        vm.false_object()
    }
}

/// Interns the image's RFC-3066 tag, answering NULL for nil.
///
/// A `PangoLanguage *` is a pointer into a process-wide intern table, never
/// allocated and never freed, so the `CString` this builds may die at the end
/// of the call while the answer stays valid forever. That is also why no
/// registry holds languages: the image keeps the String and re-interns, which
/// is a hash lookup.
///
/// NULL is meaningful and different at each use -- "the context's own
/// language" for `get_metrics`, "reset to the default" for `set_language`,
/// "the default" for `get_sample_string`, and "matches nothing but `*`" for
/// `matches` -- so each caller documents what it passes nil for.
fn language_from(vm: &Interp, p: &ffi::Pango, tag: Oop) -> PrimResult<*mut PangoLanguage> {
    if vm.is_nil(tag)? {
        return Ok(ptr::null_mut());
    }
    let tag = resources::utf8_cstring(vm, tag)?;
    Ok(pg!(p, pango_language_from_string(tag.as_ptr())))
}

/// Reads a `const char *` a Pango getter answered, as Rust text.
///
/// Every borrowed getter in this file goes through here so that the `unsafe`
/// and its justification live in one place rather than nine.
fn borrowed(text: *const core::ffi::c_char) -> String {
    // SAFETY: Pango's `const char *` getters are all transfer-none and answer
    // either NULL or a NUL-terminated string the library keeps alive past the
    // call; `borrowed_str` copies it and frees nothing.
    unsafe { ffi::borrowed_str(text) }.unwrap_or_default()
}

/// Turns a `transfer=container` array of families into plain Rust values.
///
/// Separate from its caller so that the `g_free` of the container happens on
/// every path out of the walk, including the `Unsupported` an old Pango
/// missing `pango_font_family_is_variable` (1.44) would raise here.
fn describe_families(
    p: &ffi::Pango,
    families: &[*mut PangoFontFamily],
) -> PrimResult<Vec<(String, bool, bool)>> {
    let mut rows = Vec::with_capacity(families.len());
    for &family in families {
        let name = borrowed(pg!(p, pango_font_family_get_name(family)));
        let monospace = from_gboolean(pg!(p, pango_font_family_is_monospace(family)));
        let variable = from_gboolean(pg!(p, pango_font_family_is_variable(family)));
        rows.push((name, monospace, variable));
    }
    Ok(rows)
}

/// The same one level down: face name, description string, synthesized flag.
///
/// `pango_font_face_describe` is the one transfer-full call in the whole
/// enumeration path. Its description is turned into a String and freed inside
/// the loop, so an image that enumerates a thousand faces on every font-picker
/// open does not accumulate a thousand descriptions.
fn describe_faces(
    p: &ffi::Pango,
    faces: &[*mut PangoFontFace],
) -> PrimResult<Vec<(String, String, bool)>> {
    let mut rows = Vec::with_capacity(faces.len());
    for &face in faces {
        // `pango_font_face_get_face_name`, not `_get_name`: the latter does
        // not exist, and a missing entry point is indistinguishable from an
        // old Pango from every angle except the live test.
        let name = borrowed(pg!(p, pango_font_face_get_face_name(face)));
        let description = pg!(p, pango_font_face_describe(face));
        let text = if description.is_null() {
            String::new()
        } else {
            let owned = pg!(p, pango_font_description_to_string(description));
            // SAFETY: `pango_font_description_to_string` is transfer-full --
            // `char *`, not `const char *` -- so `GStr` g_frees it on drop.
            // Its borrowed neighbour `pango_language_to_string` is the pair
            // most likely to be got backwards; the pointer's constness is the
            // only thing that tells them apart.
            let text = unsafe { ffi::GStr::from_owned(owned) }.to_string_lossy_owned();
            pg!(p, pango_font_description_free(description));
            text
        };
        let synthesized = from_gboolean(pg!(p, pango_font_face_is_synthesized(face)));
        rows.push((name, text, synthesized));
    }
    Ok(rows)
}

// ---- font maps -----------------------------------------------------------

/// `pango_cairo_font_map_get_default`, as a **borrowed** handle.
///
/// The per-thread font map Pango owns, at refcount 1 for the life of the
/// process. Destroying this handle drops nothing, which is what makes it safe
/// for the image to treat it like any other: the `owned` flag inside the
/// registry, not the image's discipline, is what keeps it alive.
#[pharo_primitive]
fn primitiveDefaultFontMap(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let p = pango()?;
    register_font_map_borrowed(pg!(p, pango_cairo_font_map_get_default()))
}

/// `pango_cairo_font_map_new`, as an **owned** handle the image must destroy.
///
/// A private font map, for the image that wants a resolution or an added font
/// file that does not leak into every other layout in the process.
#[pharo_primitive]
fn primitiveFontMapNew(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let p = pango()?;
    register_font_map_owned(pg!(p, pango_cairo_font_map_new()))
}

/// Releases a font map handle, unref'ing the object only if this plugin owned
/// a reference on it.
#[pharo_primitive]
fn primitiveFontMapDestroy(_vm: &Interp, map: sqInt) -> PrimResult<()> {
    destroy_font_map(map)
}

/// Does this handle name a font map Pango owns rather than one this plugin
/// does?
///
/// Diagnostic, not a permission check -- destroying either handle is legal.
/// It exists so the image can tell "I made this" from "Pango lent me this"
/// when it is deciding whether setting the resolution is a local change or a
/// process-wide one.
#[pharo_primitive]
fn primitiveFontMapIsBorrowed(_vm: &Interp, map: sqInt) -> PrimResult<bool> {
    font_map_is_borrowed(map)
}

/// `pango_font_map_get_serial` (1.32+): an opaque counter that changes
/// whenever the font map does.
///
/// The only sound way to notice that a font list the image cached has gone
/// stale -- fontconfig can reload underneath a running process, and nothing
/// else reports it.
#[pharo_primitive]
fn primitiveFontMapSerial(_vm: &Interp, map: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_map(map, |m| Ok(pg!(p, pango_font_map_get_serial(m)) as sqInt))
}

/// `pango_cairo_font_map_set_resolution`, in **dots per inch**.
///
/// The one number that converts a font size in points into device units, so
/// getting it wrong scales every glyph in the image. Note that on the default
/// font map this is a process-wide change: everything already laid out against
/// it re-renders at the new size.
#[pharo_primitive]
fn primitiveFontMapSetResolution(_vm: &Interp, map: sqInt, dpi: f64) -> PrimResult<()> {
    let p = pango()?;
    with_font_map(map, |m| {
        // The cast is the interface downcast C would spell
        // `PANGO_CAIRO_FONT_MAP (map)`. Pango `g_return_if_fail`s a font map
        // that does not implement the interface, so a non-Cairo one is a
        // no-op and a console warning rather than a crash -- and every font
        // map this plugin hands out comes from pangocairo.
        pg!(p, pango_cairo_font_map_set_resolution(m.cast(), dpi));
        Ok(())
    })
}

/// `pango_cairo_font_map_get_resolution`, in **dots per inch**.
#[pharo_primitive]
fn primitiveFontMapGetResolution(_vm: &Interp, map: sqInt) -> PrimResult<f64> {
    let p = pango()?;
    with_font_map(map, |m| {
        Ok(pg!(p, pango_cairo_font_map_get_resolution(m.cast())))
    })
}

/// `pango_font_map_create_context` (1.22+), as an owned context handle.
///
/// Deliberately **not** `pango_cairo_font_map_create_context`, which is
/// deprecated since 1.22 and sits behind `#ifndef PANGO_DISABLE_DEPRECATED`:
/// it is present in some builds and absent in others, and the difference does
/// not show up until the plugin runs on the build that lacks it.
///
/// This is also the only context constructor exposed. A bare
/// `pango_context_new` has no font map, so `get_metrics` and every layout made
/// from it answer nothing useful; Pango's own documentation says to use this
/// instead.
#[pharo_primitive]
fn primitiveFontMapCreateContext(_vm: &Interp, map: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_font_map(map, |m| {
        register_context(pg!(p, pango_font_map_create_context(m)))
    })
}

/// `pango_font_map_list_families`, snapshotted into an Array of 3-element
/// Arrays `{familyName. isMonospace. isVariable}`.
///
/// Nothing Pango owns crosses the boundary. The array of `PangoFontFamily *`
/// is `transfer=container`, which is a third state and not a synonym for full
/// or none: the container is `g_free`d here and not one element is unref'd.
/// The families themselves are never handed out, because they are invalidated
/// by a font map change that the plugin has no way to observe.
///
/// The families come back in no particular order; sorting is the image's.
#[pharo_primitive]
fn primitiveFontMapListFamilies(vm: &Interp, map: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let g = glib()?;
    let rows = with_font_map(map, |m| {
        let mut families: *mut *mut PangoFontFamily = ptr::null_mut();
        let mut count: c_int = 0;
        pg!(
            p,
            pango_font_map_list_families(m, &mut families, &mut count)
        );
        // SAFETY: on return Pango has written `count` family pointers at
        // `families`, or left both untouched. `slice::from_raw_parts` is
        // undefined behaviour on a null pointer even at length zero, so the
        // empty case takes a slice that never dereferences anything.
        let listed: &[*mut PangoFontFamily] = if families.is_null() || count <= 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(families, count as usize) }
        };
        let walked = describe_families(p, listed);
        // Freed before the `?`, so an entry point missing halfway through the
        // walk costs the primitive rather than the container. `g_free`, never
        // Rust's allocator: glib allocated it, and a cross-heap free is a hard
        // crash on Windows.
        gl!(g, g_free(families.cast()));
        walked
    })?;

    let mut entries = Vec::with_capacity(rows.len());
    for (name, monospace, variable) in &rows {
        let row = vm.instantiate(vm.class_array()?, 3)?;
        let name = vm.string(name)?;
        vm.store_pointer(0, row, name)?;
        let monospace = boolean(vm, *monospace)?;
        vm.store_pointer(1, row, monospace)?;
        let variable = boolean(vm, *variable)?;
        vm.store_pointer(2, row, variable)?;
        entries.push(row);
    }
    resources::oop_array(vm, &entries)
}

/// `pango_font_map_get_family` (1.46+) then `pango_font_family_list_faces`,
/// snapshotted into an Array of 3-element Arrays
/// `{faceName. descriptionString. isSynthesized}`.
///
/// The description is a String in `pango_font_description_to_string`'s format,
/// which `primitiveFontDescriptionFromString` round-trips, so the image can
/// name a face and get a description back without a face handle ever existing.
///
/// `NotFound` when the font map has no family of that name -- `get_family` is
/// nullable, and a nil there is "no such family", not a failure to look.
#[pharo_primitive]
fn primitiveFontMapListFacesOfFamily(vm: &Interp, map: sqInt, family: Oop) -> PrimResult<Oop> {
    let p = pango()?;
    let g = glib()?;
    let name = resources::utf8_cstring(vm, family)?;
    let rows = with_font_map(map, |m| {
        let family = pg!(p, pango_font_map_get_family(m, name.as_ptr()));
        if family.is_null() {
            return Err(PrimErr::NotFound);
        }
        let mut faces: *mut *mut PangoFontFace = ptr::null_mut();
        let mut count: c_int = 0;
        pg!(p, pango_font_family_list_faces(family, &mut faces, &mut count));
        // SAFETY: as in `primitiveFontMapListFamilies` -- `count` pointers at
        // `faces` on return, and the null case must not reach
        // `from_raw_parts` at all.
        let listed: &[*mut PangoFontFace] = if faces.is_null() || count <= 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(faces, count as usize) }
        };
        let walked = describe_faces(p, listed);
        gl!(g, g_free(faces.cast()));
        walked
    })?;

    let mut entries = Vec::with_capacity(rows.len());
    for (face, description, synthesized) in &rows {
        let row = vm.instantiate(vm.class_array()?, 3)?;
        let face = vm.string(face)?;
        vm.store_pointer(0, row, face)?;
        let description = vm.string(description)?;
        vm.store_pointer(1, row, description)?;
        let synthesized = boolean(vm, *synthesized)?;
        vm.store_pointer(2, row, synthesized)?;
        entries.push(row);
    }
    resources::oop_array(vm, &entries)
}

/// `pango_font_map_add_font_file` (1.56+): loads a font file into this font
/// map, where its fonts take precedence over pre-existing ones of the same
/// name.
///
/// `Unsupported` on a Pango older than 1.56, `OperationFailed` when Pango
/// declined the file. The `GError`'s message is read and freed rather than
/// leaked, but it does not reach the image: a `PrimErr` carries no payload,
/// and inventing a channel for one string is not worth the interface.
#[pharo_primitive]
fn primitiveFontMapAddFontFile(vm: &Interp, map: sqInt, path: Oop) -> PrimResult<()> {
    let p = pango()?;
    let path = resources::utf8_cstring(vm, path)?;
    with_font_map(map, |m| {
        let mut error: *mut GError = ptr::null_mut();
        let ok = pg!(p, pango_font_map_add_font_file(m, path.as_ptr(), &mut error));
        // SAFETY: `error` is either still null or a `GError *` Pango handed
        // over transfer-full, which is exactly `take_gerror`'s contract; it
        // copies the message out and frees the error.
        let _ = unsafe { resources::take_gerror(error) };
        if from_gboolean(ok) {
            Ok(())
        } else {
            Err(PrimErr::OperationFailed)
        }
    })
}

// ---- contexts ------------------------------------------------------------

/// Releases a context handle.
///
/// Safe in any order relative to the layouts made from it: a layout takes its
/// own reference on its context, so destroying the context handle first leaves
/// every layout working and a later `primitiveLayoutGetContext` answers a
/// fresh handle on the same live object.
#[pharo_primitive]
fn primitiveContextDestroy(_vm: &Interp, context: sqInt) -> PrimResult<()> {
    destroy_context(context)
}

/// `pango_context_changed` (1.32+): tells the context its font map's contents
/// changed underneath it, so it drops what it cached.
#[pharo_primitive]
fn primitiveContextChanged(_vm: &Interp, context: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_context(context, |c| {
        pg!(p, pango_context_changed(c));
        Ok(())
    })
}

/// `pango_context_get_serial` (1.32+): an opaque counter that changes whenever
/// the context does.
///
/// What a layout compares against to know its measurements are stale. The
/// image can use it the same way for anything it caches per context.
#[pharo_primitive]
fn primitiveContextSerial(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |c| {
        Ok(pg!(p, pango_context_get_serial(c)) as sqInt)
    })
}

/// `pango_context_get_font_map` (1.6+), as an **owned** font map handle.
///
/// The getter is transfer-none, so this takes its own `g_object_ref` before
/// registering. The alternative -- registering it borrowed -- would be a
/// handle whose object could be freed while the handle stayed valid, because
/// the context that holds the last reference is itself destroyable from the
/// image. One extra reference makes the destroy path honest.
///
/// `NotFound` when the context has no font map, which is only reachable
/// through a bare `pango_context_new` -- and this plugin exposes no such
/// constructor precisely because a context without a font map measures
/// nothing.
#[pharo_primitive]
fn primitiveContextGetFontMap(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    let g = glib()?;
    with_context(context, |c| {
        let map = pg!(p, pango_context_get_font_map(c));
        if map.is_null() {
            return Err(PrimErr::NotFound);
        }
        gl!(g, g_object_ref(map.cast()));
        // The reference taken above is balanced on both paths: on success the
        // registry holds it until the image destroys the handle, and on the
        // one failure `Registry::insert` has -- `LimitExceeded` -- the value
        // is already in its slot and `release_all` unrefs it at shutdown.
        register_font_map_owned(map)
    })
}

/// `pango_context_set_font_description`. Pango copies the description, so the
/// image keeps its handle and may destroy it immediately after.
///
/// **Handle 0 is not accepted here**, unlike its layout counterpart. The gir
/// marks `pango_layout_set_font_description`'s argument nullable -- NULL there
/// means "unset" -- and does not mark this one, and the difference is real:
/// measured, `pango_context_set_font_description(ctx, NULL)` raises
/// `assertion 'desc != NULL' failed`, leaves the context's description exactly
/// as it was, and under `G_DEBUG=fatal-criticals` aborts the process with the
/// image in it. So 0 fails with `NotFound` like any other dead handle rather
/// than being forwarded as a NULL Pango will refuse anyway.
#[pharo_primitive]
fn primitiveContextSetFontDescription(_vm: &Interp, context: sqInt, desc: sqInt) -> PrimResult<()> {
    let p = pango()?;
    with_context(context, |c| {
        with_font_desc(desc, |d| {
            pg!(p, pango_context_set_font_description(c, d));
            Ok(())
        })
    })
}

/// `pango_context_get_font_description`, **copied**, as a new description
/// handle; nil when the context has none.
///
/// The getter is transfer-none: the context still owns what it answers.
/// Registering that pointer and later `pango_font_description_free`ing it is a
/// double free, and the second free lands wherever the context is next used.
/// `pango_font_description_copy` first is the whole fix.
#[pharo_primitive]
fn primitiveContextGetFontDescription(vm: &Interp, context: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let handle = with_context(context, |c| {
        let borrowed = pg!(p, pango_context_get_font_description(c));
        if borrowed.is_null() {
            return Ok(None);
        }
        register_font_desc(pg!(p, pango_font_description_copy(borrowed))).map(Some)
    })?;
    match handle {
        Some(handle) => vm.integer_checked(handle),
        None => vm.nil(),
    }
}

/// `pango_context_set_language`, from an RFC-3066 tag; **nil resets the
/// context to the default language**, which is what NULL means here.
#[pharo_primitive]
fn primitiveContextSetLanguage(vm: &Interp, context: sqInt, tag: Oop) -> PrimResult<()> {
    let p = pango()?;
    let language = language_from(vm, p, tag)?;
    with_context(context, |c| {
        pg!(p, pango_context_set_language(c, language));
        Ok(())
    })
}

/// `pango_context_get_language` then `pango_language_to_string`, as the
/// canonicalised RFC-3066 tag.
///
/// The tag is borrowed and must not be freed -- contrast
/// `pango_font_description_to_string`, whose answer must be. The two are told
/// apart by `const char *` against `char *` and by nothing else.
#[pharo_primitive]
fn primitiveContextGetLanguage(_vm: &Interp, context: sqInt) -> PrimResult<String> {
    let p = pango()?;
    with_context(context, |c| {
        let language = pg!(p, pango_context_get_language(c));
        Ok(borrowed(pg!(p, pango_language_to_string(language))))
    })
}

/// `pango_context_set_base_dir`, a `PangoDirection` in 0..=6.
///
/// Range-checked here because Pango does not check: an out-of-range direction
/// falls through the switch in the bidi resolver and the paragraph comes out
/// in an order nothing explains.
#[pharo_primitive]
fn primitiveContextSetBaseDirection(
    _vm: &Interp,
    context: sqInt,
    direction: sqInt,
) -> PrimResult<()> {
    let p = pango()?;
    let direction = enum_in(direction, 0, PANGO_DIRECTION_MAX)?;
    with_context(context, |c| {
        pg!(p, pango_context_set_base_dir(c, direction));
        Ok(())
    })
}

/// `pango_context_get_base_dir`, a `PangoDirection` in 0..=6.
#[pharo_primitive]
fn primitiveContextGetBaseDirection(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |c| {
        Ok(pg!(p, pango_context_get_base_dir(c)) as sqInt)
    })
}

/// `pango_context_set_base_gravity` (1.16+), a `PangoGravity` in 0..=4.
#[pharo_primitive]
fn primitiveContextSetBaseGravity(_vm: &Interp, context: sqInt, gravity: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let gravity = enum_in(gravity, 0, PANGO_GRAVITY_MAX)?;
    with_context(context, |c| {
        pg!(p, pango_context_set_base_gravity(c, gravity));
        Ok(())
    })
}

/// `pango_context_get_base_gravity` (1.16+): the gravity that was **asked
/// for**, which may be `PANGO_GRAVITY_AUTO`.
#[pharo_primitive]
fn primitiveContextGetBaseGravity(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |c| {
        Ok(pg!(p, pango_context_get_base_gravity(c)) as sqInt)
    })
}

/// `pango_context_get_gravity` (1.16+): the gravity that will be **used**,
/// with `AUTO` already resolved against the matrix.
///
/// Reported separately from the base gravity because the two differ exactly
/// when the context has a rotation, and that is the case the image most needs
/// to see.
#[pharo_primitive]
fn primitiveContextGetGravity(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |c| Ok(pg!(p, pango_context_get_gravity(c)) as sqInt))
}

/// `pango_context_set_gravity_hint` (1.16+), a `PangoGravityHint` in 0..=2.
#[pharo_primitive]
fn primitiveContextSetGravityHint(_vm: &Interp, context: sqInt, hint: sqInt) -> PrimResult<()> {
    let p = pango()?;
    let hint = enum_in(hint, 0, PANGO_GRAVITY_HINT_MAX)?;
    with_context(context, |c| {
        pg!(p, pango_context_set_gravity_hint(c, hint));
        Ok(())
    })
}

/// `pango_context_get_gravity_hint` (1.16+), a `PangoGravityHint` in 0..=2.
#[pharo_primitive]
fn primitiveContextGetGravityHint(_vm: &Interp, context: sqInt) -> PrimResult<sqInt> {
    let p = pango()?;
    with_context(context, |c| {
        Ok(pg!(p, pango_context_get_gravity_hint(c)) as sqInt)
    })
}

/// `pango_context_set_matrix` (1.6+), from a 48-byte ByteArray of six doubles
/// in **Pango's order `xx xy yx yy x0 y0`**; nil unsets it.
///
/// That order is *not* Cairo's. The middle two elements are transposed between
/// the two libraries, and a pure scale or a pure translation looks identical
/// in both -- so a transposed matrix passes every test that does not rotate or
/// skew, and then shears the text on the one that does.
///
/// Pango copies the matrix during the call, so the stack copy here is enough.
#[pharo_primitive]
fn primitiveContextSetMatrix(vm: &Interp, context: sqInt, matrix: Oop) -> PrimResult<()> {
    let p = pango()?;
    if vm.is_nil(matrix)? {
        return with_context(context, |c| {
            pg!(p, pango_context_set_matrix(c, ptr::null()));
            Ok(())
        });
    }
    let m = ffi::PangoMatrix::from_pango_slice(&vm.read_f64_array::<6>(matrix)?)
        .ok_or(PrimErr::BadArgument)?;
    with_context(context, |c| {
        pg!(p, pango_context_set_matrix(c, &m));
        Ok(())
    })
}

/// `pango_context_get_matrix` (1.6+), as a 48-byte ByteArray of six doubles in
/// **Pango's order**; nil when no matrix is set.
///
/// Answering nil rather than fabricating the identity keeps the two states
/// distinguishable. They behave the same -- Pango documents "no matrix set" as
/// the identity -- but an image that wants to know whether it has ever set one
/// cannot recover that from a six-tuple.
///
/// The matrix is borrowed and copied out here; nothing is freed.
#[pharo_primitive]
fn primitiveContextGetMatrix(vm: &Interp, context: sqInt) -> PrimResult<Oop> {
    let p = pango()?;
    let matrix = with_context(context, |c| {
        let m = pg!(p, pango_context_get_matrix(c));
        if m.is_null() {
            return Ok(None);
        }
        // SAFETY: non-null by the check above, and `get_matrix` is
        // transfer-none: the context owns a live `PangoMatrix` at `m` for at
        // least the rest of this call. `PangoMatrix` is `Copy`, so the read
        // ends the borrow here.
        Ok(Some(unsafe { *m }))
    })?;
    match matrix {
        Some(m) => {
            let bytes = vm.instantiate(vm.class_byte_array()?, 48)?;
            vm.write_f64s(bytes, &m.to_pango_array())?;
            Ok(bytes)
        }
        None => vm.nil(),
    }
}

/// `pango_context_set_round_glyph_positions` (1.44+).
///
/// The Boolean crosses as a Boolean; inside the plugin it is a `gboolean`,
/// which is a four-byte `int` and never a Rust `bool`.
#[pharo_primitive]
fn primitiveContextSetRoundGlyphPositions(
    _vm: &Interp,
    context: sqInt,
    round: bool,
) -> PrimResult<()> {
    let p = pango()?;
    let round = as_gboolean(round);
    with_context(context, |c| {
        pg!(p, pango_context_set_round_glyph_positions(c, round));
        Ok(())
    })
}

/// `pango_context_get_round_glyph_positions` (1.44+).
#[pharo_primitive]
fn primitiveContextGetRoundGlyphPositions(_vm: &Interp, context: sqInt) -> PrimResult<bool> {
    let p = pango()?;
    with_context(context, |c| {
        Ok(from_gboolean(pg!(
            p,
            pango_context_get_round_glyph_positions(c)
        )))
    })
}

/// `pango_context_get_metrics`, as an Array of nine SmallIntegers **in Pango
/// units**: ascent, descent, height, approximate char width, approximate digit
/// width, underline position, underline thickness, strikethrough position,
/// strikethrough thickness.
///
/// Handle 0 for `desc` means the context's own font description, and a nil
/// `language` means the context's own language; both are Pango's documented
/// NULL behaviour rather than an invention here.
///
/// This is the reason there is no metrics registry. `pango_context_get_metrics`
/// is transfer-full, a `PangoFontMetrics` has no mutable state, and there is
/// nothing to do with a live one but read these nine ints -- so reading them
/// and unref'ing in the same primitive removes a whole handle type and its
/// `shutdown` obligations. The unref runs even when an accessor is missing
/// (`get_height` is 1.44+), which is why the reads happen in a closure.
///
/// `get_height` answers 0 when the line height is unavailable, which is a real
/// state on some backends: do not divide by it.
#[pharo_primitive]
fn primitiveContextGetMetrics(
    vm: &Interp,
    context: sqInt,
    desc: sqInt,
    language: Oop,
) -> PrimResult<Oop> {
    let p = pango()?;
    let language = language_from(vm, p, language)?;
    let values = with_context(context, |c| {
        with_optional_font_desc(desc, |d| {
            let metrics = pg!(p, pango_context_get_metrics(c, d, language));
            if metrics.is_null() {
                // Pango found no font at all to measure; nothing to unref.
                return Err(PrimErr::OperationFailed);
            }
            let read = || -> PrimResult<[c_int; 9]> {
                Ok([
                    pg!(p, pango_font_metrics_get_ascent(metrics)),
                    pg!(p, pango_font_metrics_get_descent(metrics)),
                    pg!(p, pango_font_metrics_get_height(metrics)),
                    pg!(p, pango_font_metrics_get_approximate_char_width(metrics)),
                    pg!(p, pango_font_metrics_get_approximate_digit_width(metrics)),
                    pg!(p, pango_font_metrics_get_underline_position(metrics)),
                    pg!(p, pango_font_metrics_get_underline_thickness(metrics)),
                    pg!(p, pango_font_metrics_get_strikethrough_position(metrics)),
                    pg!(p, pango_font_metrics_get_strikethrough_thickness(metrics)),
                ])
            };
            let values = read();
            pg!(p, pango_font_metrics_unref(metrics));
            values
        })
    })?;
    resources::int_array(vm, &values)
}

// ---- languages -----------------------------------------------------------

/// `pango_language_get_default` then `pango_language_to_string`: the language
/// Pango derived from the process locale, as an RFC-3066 tag.
///
/// A String, not a handle. A `PangoLanguage *` is an interned, immortal
/// pointer with no free function, so a registry would buy nothing and cost a
/// destroy path; the image holds the tag and this plugin re-interns on each
/// use, which is a hash lookup rather than an allocation.
#[pharo_primitive]
fn primitiveLanguageDefault(vm: &Interp) -> PrimResult<String> {
    vm.expect_argument_count(0)?;
    let p = pango()?;
    let language = pg!(p, pango_language_get_default());
    Ok(borrowed(pg!(p, pango_language_to_string(language))))
}

/// `pango_language_get_sample_string`: text representative of the characters
/// this language needs, for a font preview.
///
/// **nil means the default language.** Pango answers the classic "The quick
/// brown fox" for a language it has no sample for, which is indistinguishable
/// from a real English sample except by comparing against the sample for the
/// non-existent tag `xx` -- so the image should treat this as a preview, not
/// as evidence the language is known.
#[pharo_primitive]
fn primitiveLanguageSampleString(vm: &Interp, tag: Oop) -> PrimResult<String> {
    let p = pango()?;
    let language = language_from(vm, p, tag)?;
    Ok(borrowed(pg!(p, pango_language_get_sample_string(language))))
}

/// `pango_language_matches`: does this tag match one of the ranges in a
/// semicolon-separated list?
///
/// A range matches when it is `*`, when it is exactly the tag, or when it is a
/// prefix of the tag followed by `-`. **nil for the tag matches nothing but
/// `*`**, which is Pango's own documented behaviour for NULL and not a
/// substitution for the default language -- unlike every other nil in this
/// file.
#[pharo_primitive]
fn primitiveLanguageMatches(vm: &Interp, tag: Oop, ranges: Oop) -> PrimResult<bool> {
    let p = pango()?;
    let language = language_from(vm, p, tag)?;
    let ranges = resources::utf8_cstring(vm, ranges)?;
    Ok(from_gboolean(pg!(
        p,
        pango_language_matches(language, ranges.as_ptr())
    )))
}
