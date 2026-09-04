//! Exercises the binding against a real Pango, when there is one.
//!
//! This file is the only thing in the tree that can catch a **mistranscribed
//! signature**: `ffi.rs` declares 264 Pango entry points by hand, and a
//! `c_double` where the header says `float`, or a Rust `bool` where it says
//! `gboolean`, compiles, links, resolves, and then answers garbage. The
//! compiler cannot help, `missing_entry_points()` cannot help, and the
//! symptom surfaces arbitrarily far from the mistake. A round trip through
//! the real library can.
//!
//! It also catches a mistranscribed **name**, which is the same failure
//! wearing a different hat: `pango_font_face_get_name` does not exist, the
//! real symbol is `pango_font_face_get_face_name`, and a typo leaves the
//! `Option` permanently `None` and every primitive over it answering
//! `Unsupported` for the life of the plugin -- indistinguishable, from every
//! other angle, from "this machine's Pango is too old".
//!
//! **Skip discipline.** These tests skip when Pango cannot be loaded, which
//! is an ordinary configuration for this plugin: nothing in `cmake/`
//! downloads Pango, so a bare build machine has none. Set
//! `PANGO_TESTS_REQUIRED=1` to turn every skip into a failure -- that is what
//! a CI job which has just installed `libpango-1.0-0` and `libglib2.0-0`
//! wants, because otherwise the suite would silently stop exercising the
//! transcribed signatures the day the runner image dropped the package, and
//! nothing would go red.

use core::ffi::{c_char, c_int, c_void};
use std::sync::{Mutex, MutexGuard};

use PangoPlugin::ffi::{self, PangoColor, PangoRectangle};
use PangoPlugin::resources;

/// Whether a skipped test should instead fail.
///
/// Read from the environment rather than from a cargo feature so that CI can
/// demand the coverage without a separate build of the crate.
fn skips_are_failures() -> bool {
    std::env::var_os("PANGO_TESTS_REQUIRED").is_some_and(|v| !v.is_empty() && v != "0")
}

macro_rules! skip_without_pango {
    () => {
        if !ffi::load() {
            assert!(
                !skips_are_failures(),
                "PANGO_TESTS_REQUIRED is set and no Pango could be loaded; \
                 the plugin looked at every candidate in ffi::library_names"
            );
            eprintln!("skipping: no Pango could be loaded");
            return;
        }
    };
}

/// The six registries are process-wide statics, and `cargo test` runs the
/// tests in this binary on parallel threads. Every test that asserts on
/// [`resources::live_counts`] or calls [`resources::release_all`] takes this
/// first, so one test's bookkeeping is never another's failure.
static REGISTRY: Mutex<()> = Mutex::new(());

fn registry_lock() -> MutexGuard<'static, ()> {
    // A poisoned lock only means some earlier test panicked while holding it;
    // the registries themselves are internally locked, so carrying on is
    // right and reporting a second, derived failure is not.
    REGISTRY.lock().unwrap_or_else(|e| e.into_inner())
}

/// A context off the default pangocairo font map: the cheapest real
/// `PangoContext` there is, and the one every layout test needs.
///
/// # Safety
///
/// The answer is transfer=full and the caller must `g_object_unref` it. The
/// font map it came from is *not* owned -- `pango_cairo_font_map_get_default`
/// is transfer=none and unref'ing it would destroy text rendering for the
/// whole process.
unsafe fn fresh_context() -> *mut ffi::PangoContext {
    let p = ffi::pango().expect("pango loaded");
    // SAFETY: both entries resolved (asserted below), and the font map is
    // borrowed for the duration of the call only.
    unsafe {
        let map = (p.pango_cairo_font_map_get_default.expect("get_default"))();
        assert!(!map.is_null(), "the default font map is never NULL");
        (p.pango_font_map_create_context.expect("create_context"))(map)
    }
}

/// Releases a pointer glib owns. Written out rather than inlined because
/// forgetting it in one test out of a dozen is exactly the leak this suite
/// exists to notice in the plugin.
///
/// # Safety
///
/// `ptr` must be a live GObject this caller owns a reference to.
unsafe fn unref(ptr: *mut c_void) {
    let g = ffi::glib().expect("glib loaded");
    // SAFETY: the caller's contract.
    unsafe { (g.g_object_unref.expect("g_object_unref"))(ptr) }
}

// ---- names ----------------------------------------------------------------

#[test]
fn every_entry_point_this_plugin_declares_really_exists() {
    skip_without_pango!();
    let p = ffi::pango().unwrap();
    let g = ffi::glib().unwrap();

    // The gate is mandatory, not decoration. Pango is a system library, so
    // `missing_entry_points()` is legitimately non-empty on an older install
    // -- `add_font_file` is 1.56, `set_width` is 1.58, `tab_array_to_string`
    // is 1.50 -- and an unconditional emptiness assertion would fail on every
    // machine that is not the one this was written on.
    // SAFETY: no pointers cross here; `pango_version` takes nothing and
    // answers a plain int.
    let version = unsafe { (p.pango_version.expect("pango_version"))() };
    let excused = ffi::entries_introduced_after(version);
    let missing = p.missing_entry_points();
    let unexpected: Vec<&&str> = missing.iter().filter(|n| !excused.contains(n)).collect();
    assert!(
        unexpected.is_empty(),
        "misspelt or absent from Pango {version}: {unexpected:?}"
    );

    // glib's eight entries have been in glib since 2.x and none is gated, so
    // any absence here is a typo outright.
    assert!(
        g.missing_entry_points().is_empty(),
        "misspelt in the glib table: {:?}",
        g.missing_entry_points()
    );
}

#[test]
fn the_face_name_getter_is_the_symbol_pango_really_exports() {
    skip_without_pango!();
    // The concrete half of the test above, for the one name the plan singles
    // out: `pango_font_face_get_name` does not exist. Resolving the symbol is
    // not enough -- this walks a real family and reads a real face name, so a
    // declaration that resolved to something else would answer nonsense
    // rather than "Regular".
    let p = ffi::pango().unwrap();
    // SAFETY: `list_families` is transfer=container -- the array is g_freed
    // below and not one element is touched after the walk; every family and
    // face pointer is borrowed from the font map and outlives this block.
    unsafe {
        let map = (p.pango_cairo_font_map_get_default.unwrap())();
        let mut families: *mut *mut ffi::PangoFontFamily = core::ptr::null_mut();
        let mut n: c_int = 0;
        (p.pango_font_map_list_families.unwrap())(map, &mut families, &mut n);
        assert!(n > 0, "a machine with Pango has at least one font family");
        assert!(!families.is_null());

        let first = *families;
        let name = ffi::borrowed_str((p.pango_font_family_get_name.unwrap())(first));
        assert!(name.is_some_and(|s| !s.is_empty()), "family name is empty");

        let mut faces: *mut *mut ffi::PangoFontFace = core::ptr::null_mut();
        let mut m: c_int = 0;
        (p.pango_font_family_list_faces.unwrap())(first, &mut faces, &mut m);
        assert!(m > 0 && !faces.is_null(), "a family has at least one face");
        let face_name = ffi::borrowed_str((p.pango_font_face_get_face_name.unwrap())(*faces));
        assert!(
            face_name.is_some_and(|s| !s.is_empty()),
            "face name is empty -- the wrong symbol would be the reason"
        );

        (ffi::glib().unwrap().g_free.unwrap())(faces.cast());
        (ffi::glib().unwrap().g_free.unwrap())(families.cast());
    }
}

// ---- transcribed types ----------------------------------------------------

#[test]
fn a_pango_unit_is_a_thousand_and_twenty_fourth_of_a_device_unit() {
    skip_without_pango!();
    let p = ffi::pango().unwrap();
    // SAFETY: both entries take and answer scalars; no pointer is involved.
    let (from, to) = unsafe {
        (
            (p.pango_units_from_double.unwrap())(1.0),
            (p.pango_units_to_double.unwrap())(1024),
        )
    };
    assert_eq!(from, 1024, "PANGO_SCALE, asked of Pango rather than assumed");
    assert_eq!(to, 1.0);
    // And the crate's own const helper agrees with the library's rounding.
    assert_eq!(ffi::pango_pixels(1024), 1);
    assert_eq!(ffi::pango_pixels(from), 1);
}

#[test]
fn line_spacing_round_trips_through_a_float() {
    skip_without_pango!();
    // `pango_layout_set_line_spacing` takes a `float`, not a `double`
    // (pango-layout.h). A `c_double` declaration passes the value in `d0`
    // while the callee reads `s0` on AArch64, so it answers garbage -- but
    // only for a value a `float` cannot hold exactly, which is why 0.1 is
    // here beside 1.5. 1.5 alone would pass under either declaration.
    let p = ffi::pango().unwrap();
    // SAFETY: every pointer below is one this test made and destroys again;
    // the layout outlives each call.
    unsafe {
        let ctx = fresh_context();
        let layout = (p.pango_layout_new.unwrap())(ctx);

        (p.pango_layout_set_line_spacing.unwrap())(layout, 1.5);
        assert_eq!((p.pango_layout_get_line_spacing.unwrap())(layout), 1.5f32);

        (p.pango_layout_set_line_spacing.unwrap())(layout, 0.1);
        let got = (p.pango_layout_get_line_spacing.unwrap())(layout);
        assert_eq!(got, 0.1f32, "not a double: 0.1f32 != 0.1f64");
        assert_ne!(f64::from(got), 0.1f64, "and the widening is lossy, as it must be");

        unref(layout.cast());
        unref(ctx.cast());
    }
}

#[test]
fn a_gboolean_crosses_as_a_whole_int_in_both_directions() {
    skip_without_pango!();
    // Every `gboolean` in the table is `c_int`, four bytes. Declared as a
    // Rust `bool` it would be one byte with a validity invariant -- and a C
    // function that answered any non-zero other than 1 would then be
    // *undefined behaviour* on the Rust side, not merely a wrong answer, so
    // this is the one transcription mistake that cannot be argued as benign.
    //
    // The 1-and-0 round trip is what a live Pango can actually show. It does
    // not by itself distinguish four bytes from one, because Pango keeps
    // `justify` in a one-bit field and so never answers anything but 0 or 1;
    // what it does show is that the value survives the boundary at all, which
    // a wrongly-sized argument slot on a register-starved call would break.
    let p = ffi::pango().unwrap();
    // SAFETY: as above.
    unsafe {
        let ctx = fresh_context();
        let layout = (p.pango_layout_new.unwrap())(ctx);

        (p.pango_layout_set_justify.unwrap())(layout, resources::as_gboolean(true));
        let on = (p.pango_layout_get_justify.unwrap())(layout);
        assert_eq!(on, 1, "TRUE, and exactly TRUE");
        assert!(resources::from_gboolean(on));

        (p.pango_layout_set_justify.unwrap())(layout, resources::as_gboolean(false));
        let off = (p.pango_layout_get_justify.unwrap())(layout);
        assert_eq!(off, 0);
        assert!(!resources::from_gboolean(off));

        unref(layout.cast());
        unref(ctx.cast());
    }

    // A computed gboolean rather than a stored one, so the answer comes back
    // through the return register instead of out of a struct field.
    let bold = c"Sans Bold 12";
    let plain = c"Sans 12";
    // SAFETY: three descriptions made here, all freed here; `equal` borrows.
    unsafe {
        let a = (p.pango_font_description_from_string.unwrap())(bold.as_ptr());
        let b = (p.pango_font_description_from_string.unwrap())(bold.as_ptr());
        let c = (p.pango_font_description_from_string.unwrap())(plain.as_ptr());
        assert_eq!((p.pango_font_description_equal.unwrap())(a, b), 1);
        assert_eq!((p.pango_font_description_equal.unwrap())(a, c), 0);
        (p.pango_font_description_free.unwrap())(a);
        (p.pango_font_description_free.unwrap())(b);
        (p.pango_font_description_free.unwrap())(c);
    }
}

#[test]
fn a_colour_channel_is_sixteen_bits_and_pango_expands_by_two_hundred_and_fifty_seven() {
    skip_without_pango!();
    // `PangoColor` is three `guint16` and no alpha: six bytes, alignment 2.
    // Declared as three `c_int` the struct would be twelve bytes and every
    // channel would read from the wrong offset -- and #3366cc, whose bytes
    // ascend, is the input that makes that visible.
    let p = ffi::pango().unwrap();
    let mut colour = PangoColor { red: 0, green: 0, blue: 0 };
    let spec = c"#3366cc";
    // SAFETY: `colour` is a live stack slot of exactly the layout Pango
    // expects (asserted in ffi's own unit tests), and `spec` is a literal
    // NUL-terminated string.
    let ok = unsafe { (p.pango_color_parse.unwrap())(&mut colour, spec.as_ptr()) };
    assert_eq!(ok, 1);
    assert_eq!(colour.red, 0x3333, "0x33 * 257");
    assert_eq!(colour.green, 0x6666);
    assert_eq!(colour.blue, 0xcccc);
    assert_eq!(colour.red, 13107, "the value the plan names");

    // The 1.46 entry point beside it fills an alpha the six-byte struct has
    // no room for, which is why it is a fourth out parameter.
    if let Some(with_alpha) = p.pango_color_parse_with_alpha {
        let mut alpha: u16 = 0;
        let mut c = PangoColor { red: 0, green: 0, blue: 0 };
        let spec = c"#3366cc80";
        // SAFETY: as above, plus `alpha`, a live `u16` slot.
        let ok = unsafe { with_alpha(&mut c, &mut alpha, spec.as_ptr()) };
        assert_eq!(ok, 1);
        assert_eq!(c.red, 13107, "the colour is unchanged by the alpha suffix");
        assert_eq!(alpha, 0x8080);
    }
}

// ---- markup ---------------------------------------------------------------

#[test]
fn markup_yields_plain_text_and_the_accelerator_behind_the_marker() {
    skip_without_pango!();
    let p = ffi::pango().unwrap();
    let markup = "_File";
    let mut attrs = core::ptr::null_mut();
    let mut text: *mut c_char = core::ptr::null_mut();
    let mut accel: ffi::gunichar = 0;
    let mut error = core::ptr::null_mut();
    // SAFETY: every out parameter is a live stack slot, the text is passed
    // with its own byte length rather than relying on a NUL, and both owned
    // answers are released below.
    let ok = unsafe {
        (p.pango_parse_markup.unwrap())(
            markup.as_ptr().cast(),
            markup.len() as c_int,
            u32::from('_'),
            &mut attrs,
            &mut text,
            &mut accel,
            &mut error,
        )
    };
    assert_eq!(ok, 1, "a valid markup string parses");
    assert!(error.is_null());
    // SAFETY: transfer=full `char *` out of glib's allocator; `GStr` g_frees.
    let plain = unsafe { ffi::GStr::from_owned(text) };
    assert_eq!(plain.to_string_lossy_owned(), "File");
    assert_eq!(accel, u32::from('F'), "the character after the marker");
    assert!(!attrs.is_null(), "the underline is an attribute");
    // SAFETY: transfer=full attribute list; the only reference is this one.
    unsafe { (p.pango_attr_list_unref.unwrap())(attrs) };
}

#[test]
fn a_failed_parse_leaves_every_out_parameter_untouched() {
    skip_without_pango!();
    // The reason `set_markup` has to be validated with `parse_markup` first:
    // on failure nothing is written, so a primitive that trusted the out
    // params would read the uninitialised stack, and `pango_layout_set_markup`
    // itself is `void` and reports the failure only as a g_warning.
    let p = ffi::pango().unwrap();
    let markup = "<b>unclosed";
    let sentinel = 0xDEAD_BEEFu32;
    let mut attrs = core::ptr::null_mut();
    let mut text: *mut c_char = core::ptr::null_mut();
    let mut accel: ffi::gunichar = sentinel;
    let mut error = core::ptr::null_mut();
    // SAFETY: as above.
    let ok = unsafe {
        (p.pango_parse_markup.unwrap())(
            markup.as_ptr().cast(),
            markup.len() as c_int,
            0,
            &mut attrs,
            &mut text,
            &mut accel,
            &mut error,
        )
    };
    assert_eq!(ok, 0);
    assert!(attrs.is_null() && text.is_null());
    assert_eq!(accel, sentinel, "the out param was not written");
    assert!(!error.is_null(), "and a GError was set");
    // SAFETY: transfer=full `GError *`; `take_gerror` copies the message and
    // g_error_frees, which is the one correct release verb for it.
    let taken = unsafe { resources::take_gerror(error) };
    let (code, message) = taken.expect("a non-NULL GError yields its contents");
    assert!(code >= 0, "every GMarkupError enumerator is non-negative");
    assert!(!message.is_empty(), "glib always sets a message here");
}

// ---- ownership ------------------------------------------------------------

#[test]
fn the_default_font_map_survives_being_destroyed_by_the_image() {
    skip_without_pango!();
    let _guard = registry_lock();
    // `pango_cairo_font_map_get_default` is transfer=NONE and its neighbour
    // `pango_cairo_font_map_new` is transfer=FULL. The default map sits at
    // refcount 1 for the whole process, so one stray unref destroys text
    // rendering everywhere -- and the crash lands in whatever code touches a
    // font next, not here. Registering it borrowed and then destroying the
    // handle is the exact sequence that would do it.
    let p = ffi::pango().unwrap();
    // SAFETY: borrowed for the call only; nothing here takes a reference.
    let map = unsafe { (p.pango_cairo_font_map_get_default.unwrap())() };
    let handle = resources::register_font_map_borrowed(map).expect("registered");
    assert!(resources::font_map_is_borrowed(handle).unwrap());
    resources::destroy_font_map(handle).expect("destroyed");

    // If the borrow had been mishandled the map is freed and this is a
    // use-after-free, which on a real Pango is a crash -- exactly the signal
    // wanted from a test.
    // SAFETY: the map is still alive, which is what is being asserted.
    let serial = unsafe {
        let again = (p.pango_cairo_font_map_get_default.unwrap())();
        assert_eq!(again, map, "the default map is a process-wide singleton");
        (p.pango_font_map_get_serial.unwrap())(again)
    };
    assert!(serial > 0, "a live font map has a non-zero serial");
}

#[test]
fn a_thousand_layouts_created_and_destroyed_leave_no_registry_entries() {
    skip_without_pango!();
    let _guard = registry_lock();
    let p = ffi::pango().unwrap();
    let before = resources::live_counts();

    for _ in 0..1000 {
        // SAFETY: a fresh context and a fresh layout, both registered so the
        // registry owns the reference from here on.
        let (ctx, layout) = unsafe {
            let ctx = fresh_context();
            (ctx, (p.pango_layout_new.unwrap())(ctx))
        };
        let layout_handle = resources::register_layout(layout).expect("registered");
        let ctx_handle = resources::register_context(ctx).expect("registered");
        resources::destroy_layout(layout_handle).expect("destroyed");
        resources::destroy_context(ctx_handle).expect("destroyed");
    }

    assert_eq!(
        resources::live_counts(),
        before,
        "a create/destroy pair must be exactly neutral"
    );
}

#[test]
fn an_owned_string_and_a_borrowed_one_are_released_by_opposite_rules() {
    skip_without_pango!();
    // `pango_font_description_to_string` answers `char *` -- owned, g_free --
    // while `pango_version_string` answers `const char *`, which is static
    // for the process and freeing it would corrupt the heap. The types are
    // the only difference, and there is no exception to that rule anywhere in
    // Pango, which is why the crate keys `GStr` and `borrowed_str` off them.
    let p = ffi::pango().unwrap();
    let spec = c"Sans Bold 12";
    // SAFETY: `from_string` is transfer=full and is freed below; `to_string`
    // is transfer=full and `GStr` g_frees it; `version_string` is borrowed
    // and `borrowed_str` copies without freeing.
    unsafe {
        let desc = (p.pango_font_description_from_string.unwrap())(spec.as_ptr());
        assert!(!desc.is_null());
        let owned = ffi::GStr::from_owned((p.pango_font_description_to_string.unwrap())(desc));
        assert_eq!(owned.to_string_lossy_owned(), "Sans Bold 12");
        (p.pango_font_description_free.unwrap())(desc);

        let borrowed = ffi::borrowed_str((p.pango_version_string.unwrap())());
        let borrowed = borrowed.expect("Pango always names its version");
        assert!(borrowed.starts_with('1'), "unexpected version {borrowed}");
        // Reading it twice would be a use-after-free if the first read had
        // freed it.
        let again = ffi::borrowed_str((p.pango_version_string.unwrap())()).unwrap();
        assert_eq!(again, borrowed);
    }
}

// ---- geometry -------------------------------------------------------------

#[test]
fn a_layouts_size_agrees_with_its_extents_and_its_pixel_size_rounds_outward() {
    skip_without_pango!();
    // `get_size` is Pango units, `get_pixel_size` is the same measurement
    // rounded *outward* through the ink/logical extents rather than through
    // PANGO_PIXELS -- so the two can differ by a pixel, and a caller that
    // assumed otherwise would clip text. Asserting the relation rather than
    // an exact figure keeps this independent of which fonts are installed.
    let p = ffi::pango().unwrap();
    let text = "The quick brown fox jumps over the lazy dog";
    // SAFETY: the layout outlives every call, the text is passed with an
    // explicit length, and both objects are released.
    unsafe {
        let ctx = fresh_context();
        let layout = (p.pango_layout_new.unwrap())(ctx);
        (p.pango_layout_set_text.unwrap())(layout, text.as_ptr().cast(), text.len() as c_int);

        let mut w = 0;
        let mut h = 0;
        (p.pango_layout_get_size.unwrap())(layout, &mut w, &mut h);
        assert!(w > 0 && h > 0, "laid-out text has a size");

        let mut pw = 0;
        let mut ph = 0;
        (p.pango_layout_get_pixel_size.unwrap())(layout, &mut pw, &mut ph);
        assert!(
            pw >= ffi::pango_pixels(w) - 1 && pw <= ffi::pango_pixels_ceil(w),
            "pixel width {pw} is not a rounding of {w}"
        );
        assert!(ph >= ffi::pango_pixels(h) - 1 && ph <= ffi::pango_pixels_ceil(h));

        // The logical rectangle is the same measurement a third way, and it
        // is the one that proves `PangoRectangle`'s field order: a struct
        // whose width and height were transposed would still be plausible.
        let mut ink = PangoRectangle::default();
        let mut logical = PangoRectangle::default();
        (p.pango_layout_get_extents.unwrap())(layout, &mut ink, &mut logical);
        assert_eq!(logical.width, w, "logical width is get_size's width");
        assert_eq!(logical.height, h);
        assert!(
            ink.width <= logical.width,
            "ink {} exceeds logical {}",
            ink.width,
            logical.width
        );
        assert_eq!((p.pango_layout_get_line_count.unwrap())(layout), 1);
        assert_eq!(
            (p.pango_layout_get_character_count.unwrap())(layout),
            text.chars().count() as c_int
        );

        unref(layout.cast());
        unref(ctx.cast());
    }
}

#[test]
fn a_tab_array_round_trips_through_its_string_form() {
    skip_without_pango!();
    // `PangoTabArray` is freed with `pango_tab_array_free`, not unref'd, and
    // its out params are two `*mut c_int` -- the alignment is an enum in an
    // int-sized slot, not a byte. Both to_string and from_string are 1.50+,
    // so this skips rather than fails on an older Pango: the plugin's own
    // answer there is `Unsupported`, which is correct behaviour and not a
    // defect to assert against.
    let p = ffi::pango().unwrap();
    let (Some(to_string), Some(from_string)) =
        (p.pango_tab_array_to_string, p.pango_tab_array_from_string)
    else {
        eprintln!("skipping: this Pango predates pango_tab_array_to_string (1.50)");
        return;
    };
    // SAFETY: one array made here, one parsed from its own text, both freed.
    unsafe {
        let tabs = (p.pango_tab_array_new.unwrap())(2, 0);
        assert!(!tabs.is_null());
        (p.pango_tab_array_set_tab.unwrap())(tabs, 0, 0, 1024);
        (p.pango_tab_array_set_tab.unwrap())(tabs, 1, 0, 4096);
        assert_eq!((p.pango_tab_array_get_size.unwrap())(tabs), 2);

        let mut align: c_int = -1;
        let mut location: c_int = -1;
        (p.pango_tab_array_get_tab.unwrap())(tabs, 1, &mut align, &mut location);
        assert_eq!((align, location), (0, 4096));

        let text = ffi::GStr::from_owned(to_string(tabs));
        let text = text.to_string_lossy_owned();
        assert!(!text.is_empty());

        let spec = std::ffi::CString::new(text.clone()).unwrap();
        let parsed = from_string(spec.as_ptr());
        assert!(!parsed.is_null(), "Pango could not re-read {text:?}");
        assert_eq!((p.pango_tab_array_get_size.unwrap())(parsed), 2);
        let mut align2: c_int = -1;
        let mut location2: c_int = -1;
        (p.pango_tab_array_get_tab.unwrap())(parsed, 1, &mut align2, &mut location2);
        assert_eq!((align2, location2), (align, location));

        (p.pango_tab_array_free.unwrap())(parsed);
        (p.pango_tab_array_free.unwrap())(tabs);
    }
}

#[test]
fn font_metrics_are_read_and_then_unreffed_in_one_breath() {
    skip_without_pango!();
    // `pango_context_get_metrics` is transfer=full out of a cache: read the
    // nine accessors and unref before returning, or the entry leaks once per
    // call. Both `desc` and `language` are legitimately NULL, which is what
    // makes "the context's own font, the context's own language" expressible.
    let p = ffi::pango().unwrap();
    // SAFETY: the metrics are owned here and unref'd below; the context
    // outlives them.
    unsafe {
        let ctx = fresh_context();
        let metrics = (p.pango_context_get_metrics.unwrap())(
            ctx,
            core::ptr::null(),
            core::ptr::null_mut(),
        );
        assert!(!metrics.is_null(), "a context can always measure something");
        let ascent = (p.pango_font_metrics_get_ascent.unwrap())(metrics);
        let descent = (p.pango_font_metrics_get_descent.unwrap())(metrics);
        assert!(ascent > 0 && descent > 0, "in Pango units, both positive");
        assert!(
            ascent > 1024,
            "an ascent of {ascent} would be under a pixel -- pixels for units"
        );
        (p.pango_font_metrics_unref.unwrap())(metrics);
        unref(ctx.cast());
    }
}

// ---- indices --------------------------------------------------------------

#[test]
fn get_direction_partitions_a_layouts_text_by_bytes_not_by_characters() {
    skip_without_pango!();
    // `pango_layout_get_direction`'s parameter is called `index` and its
    // documentation reads "the byte index of the char", which is a sentence
    // that can be read either way round -- and reading it the wrong way is
    // invisible in every ASCII test, because there a byte offset and a
    // character position are the same number. So this test uses text where
    // they differ, and asserts the *shape* of the answer rather than the
    // constants: what proves the semantics is that the two bytes of one
    // Hebrew letter answer alike and differently from the Latin either side
    // of it. Were the argument a character position, index 2 would be the
    // trailing 'b' and would agree with index 0 instead.
    let p = ffi::pango().unwrap();
    let Some(get_direction) = p.pango_layout_get_direction else {
        eprintln!("skipping: this Pango predates pango_layout_get_direction (1.46)");
        return;
    };
    let text = "a\u{05E9}b";
    assert_eq!(text.len(), 4, "one Latin byte, two Hebrew, one Latin");
    assert_eq!(text.chars().count(), 3);
    // SAFETY: the layout outlives every call, the text is passed with an
    // explicit length, and both objects are released.
    unsafe {
        let ctx = fresh_context();
        let layout = (p.pango_layout_new.unwrap())(ctx);
        (p.pango_layout_set_text.unwrap())(layout, text.as_ptr().cast(), text.len() as c_int);
        assert_eq!(
            (p.pango_layout_get_character_count.unwrap())(layout),
            3,
            "the layout counts characters even though this function does not"
        );

        let dir: Vec<c_int> = (0..4).map(|i| get_direction(layout, i)).collect();
        assert_ne!(dir[1], dir[0], "the Hebrew letter runs the other way");
        assert_eq!(
            dir[2], dir[1],
            "index 2 is the Hebrew letter's second byte, not the 'b'"
        );
        assert_eq!(dir[3], dir[0], "index 3 is the 'b'");

        unref(layout.cast());
        unref(ctx.cast());
    }
}

#[test]
fn the_byte_length_a_layout_asserts_against_is_the_length_of_the_text_it_answers() {
    skip_without_pango!();
    // `layout::checked_byte_index` bounds every index it forwards by the
    // `strlen` of `pango_layout_get_text`, because Pango's own bound is the
    // private `layout->length` and there is no accessor for it. The two must
    // be the same number: too small and a legal caret position at the very end
    // of the text is refused, too large and the `g_return_if_fail` this exists
    // to head off is reachable again after all. So: the end position is
    // exercised here, and it is *not* the character count -- a bound taken
    // from `get_character_count` would cut this text short by one.
    let p = ffi::pango().unwrap();
    let text = "a\u{05E9}b";
    // SAFETY: as above; the rectangles are plain out parameters on the stack.
    unsafe {
        let ctx = fresh_context();
        let layout = (p.pango_layout_new.unwrap())(ctx);
        (p.pango_layout_set_text.unwrap())(layout, text.as_ptr().cast(), text.len() as c_int);

        let answered = ffi::borrowed_str((p.pango_layout_get_text.unwrap())(layout))
            .expect("a layout that has been given text answers it");
        assert_eq!(answered.len(), text.len(), "4 bytes in, 4 bytes out");
        assert!(
            answered.len() > (p.pango_layout_get_character_count.unwrap())(layout) as usize,
            "the bound is bytes, and for this text there are more of them"
        );

        // The position after the last byte: where a caret sits at the end of a
        // paragraph, and the one index the assertion admits past the text.
        let mut strong = PangoRectangle::default();
        let mut weak = PangoRectangle::default();
        (p.pango_layout_get_cursor_pos.unwrap())(
            layout,
            answered.len() as c_int,
            &mut strong,
            &mut weak,
        );
        assert!(strong.height > 0, "the end of the text has a caret on it");

        unref(layout.cast());
        unref(ctx.cast());
    }
}

// ---- the two-Cairo hazard -------------------------------------------------

#[cfg(unix)]
extern "C" {
    /// `dladdr`, from libSystem/libdl. Declared here rather than pulling in a
    /// `libc` dependency for one call in one test.
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
}

/// `Dl_info`, identically laid out on macOS and glibc.
#[cfg(unix)]
#[repr(C)]
struct DlInfo {
    fname: *const c_char,
    fbase: *mut c_void,
    sname: *const c_char,
    saddr: *mut c_void,
}

#[test]
#[cfg(unix)]
fn the_cairo_reached_through_pangocairo_is_the_one_pangocairo_is_linked_against() {
    skip_without_pango!();
    // The bridge's whole safety argument rests on an assumption nothing else
    // checks: that `dlsym` on the libpangocairo handle answers the Cairo that
    // libpangocairo itself calls. It is true for dyld and for glibc, but if
    // it ever stopped being true the identity handshake would *pass* when it
    // should refuse -- and passing wrongly is the corruption case, while
    // refusing wrongly is only a declined primitive.
    let p = ffi::pango().unwrap();
    let Some(create) = p.cairo_create else {
        eprintln!("skipping: this pangocairo does not re-export cairo_create");
        return;
    };
    let addr = create as *const c_void;
    let mut info = DlInfo {
        fname: core::ptr::null(),
        fbase: core::ptr::null_mut(),
        sname: core::ptr::null(),
        saddr: core::ptr::null_mut(),
    };
    // SAFETY: `addr` is the address of a resolved function in a library this
    // process has loaded, and `info` is a live stack slot of the right shape.
    let ok = unsafe { dladdr(addr, &mut info) };
    assert_ne!(ok, 0, "dladdr could not place cairo_create");
    // SAFETY: dladdr filled `fname` with a NUL-terminated path owned by the
    // loader; borrowed, never freed.
    let file = unsafe { ffi::borrowed_str(info.fname) }.expect("dladdr named a file");
    assert!(
        file.contains("cairo"),
        "cairo_create resolved out of {file}, which is not a Cairo -- the \
         bridge's identity comparison would then be comparing the wrong thing"
    );
    // And the version accessor beside it comes from the same file, so the
    // string the bridge puts in its refusal names a real library.
    if let Some(version) = p.cairo_version {
        // SAFETY: `cairo_version` takes nothing and answers an int.
        let v = unsafe { version() };
        assert!(v >= 10000, "implausible cairo_version {v}");
    }
}

// ---- a real render --------------------------------------------------------

/// ARGB32, and the only `cairo_format_t` this file uses. A fresh ARGB32 image
/// surface is zero-filled, which is what makes "count the pixels that are not
/// zero" a sound measure of ink.
const CAIRO_FORMAT_ARGB32: c_int = 0;

/// The handful of Cairo entry points a render needs and [`ffi`] deliberately
/// does not declare.
///
/// `ffi.rs` declares exactly four Cairo functions, and only so the bridge can
/// compare `cairo_create`'s *address*: in the VM it is `CairoPlugin` that owns
/// every surface and context, and a second plugin allocating its own would be
/// the two-Cairo hazard rather than a convenience. Inside a `cargo test`
/// binary there is no `CairoPlugin` and no VM bundle, so a render has to make
/// its own surface -- and it must make it out of the Cairo that libpangocairo
/// itself calls, or the test would prove nothing about the code that ships.
///
/// Re-opening the path `ffi` already resolved gets exactly that: the loader
/// refcounts an image it has mapped rather than mapping a second copy, so
/// these pointers and `ffi`'s `cairo_create` come from one library.
struct CairoForTests {
    _lib: pharo_vm_plugin::dylib::Library,
    image_surface_create: unsafe extern "C" fn(c_int, c_int, c_int) -> *mut c_void,
    surface_status: unsafe extern "C" fn(*mut c_void) -> c_int,
    surface_flush: unsafe extern "C" fn(*mut c_void),
    surface_destroy: unsafe extern "C" fn(*mut c_void),
    image_surface_get_data: unsafe extern "C" fn(*mut c_void) -> *mut u8,
    image_surface_get_stride: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_source_rgb: unsafe extern "C" fn(*mut c_void, f64, f64, f64),
    move_to: unsafe extern "C" fn(*mut c_void, f64, f64),
}

/// Answers `None`, loudly, when the Cairo behind pangocairo cannot be reached
/// -- a static Cairo folded into libpangocairo would hide these symbols, and a
/// render is then simply not available rather than broken.
fn cairo_for_tests() -> Option<CairoForTests> {
    let path = ffi::pango().ok()?.path.clone();
    // SAFETY: the path is one this process has already dlopened successfully
    // through `ffi::load`, so opening it again maps nothing new; and every
    // signature below is transcribed from cairo.h, where these six have been
    // stable since 1.0.
    let lib = unsafe { pharo_vm_plugin::dylib::Library::new(&path) }.ok()?;
    macro_rules! sym {
        ($name:literal) => {
            // SAFETY: as above.
            match unsafe { pharo_vm_plugin::dylib::symbol(&lib, $name) } {
                Some(f) => f,
                None => {
                    eprintln!("skipping: {path} does not re-export {}", $name);
                    return None;
                }
            }
        };
    }
    let out = CairoForTests {
        image_surface_create: sym!("cairo_image_surface_create"),
        surface_status: sym!("cairo_surface_status"),
        surface_flush: sym!("cairo_surface_flush"),
        surface_destroy: sym!("cairo_surface_destroy"),
        image_surface_get_data: sym!("cairo_image_surface_get_data"),
        image_surface_get_stride: sym!("cairo_image_surface_get_stride"),
        set_source_rgb: sym!("cairo_set_source_rgb"),
        move_to: sym!("cairo_move_to"),
        _lib: lib,
    };
    Some(out)
}

/// The bounding box of every pixel that is not transparent black, and how many
/// there are.
///
/// Answers `None` for an untouched surface. Counting rather than sampling
/// because a single sampled pixel is a coin toss against antialiased glyph
/// edges, and the count is what distinguishes "some ink" from "the whole
/// surface got painted", which is the way this test could pass for the wrong
/// reason.
///
/// # Safety
///
/// `data` must address `height` rows of `stride` bytes.
unsafe fn ink(
    data: *const u8,
    stride: usize,
    width: usize,
    height: usize,
) -> Option<(usize, [usize; 4])> {
    let mut count = 0usize;
    let mut box_ = [usize::MAX, usize::MAX, 0usize, 0usize];
    for y in 0..height {
        for x in 0..width {
            // SAFETY: the caller's contract, and x < width <= stride / 4.
            let pixel = unsafe { data.add(y * stride + x * 4).cast::<u32>().read_unaligned() };
            if pixel != 0 {
                count += 1;
                box_[0] = box_[0].min(x);
                box_[1] = box_[1].min(y);
                box_[2] = box_[2].max(x);
                box_[3] = box_[3].max(y);
            }
        }
    }
    (count > 0).then_some((count, box_))
}

#[test]
fn a_laid_out_string_really_puts_ink_on_a_cairo_surface() {
    skip_without_pango!();
    let p = ffi::pango().unwrap();
    let Some(c) = cairo_for_tests() else { return };

    const W: usize = 240;
    const H: usize = 64;

    // SAFETY: every entry used below is asserted resolved as it is taken, the
    // surface and context are created here and destroyed at the end, and the
    // layout is transfer=full from `pango_cairo_create_layout`.
    unsafe {
        let surface = (c.image_surface_create)(CAIRO_FORMAT_ARGB32, W as c_int, H as c_int);
        assert_eq!((c.surface_status)(surface), 0, "the surface was not created");
        let cr = (p.cairo_create.expect("cairo_create"))(surface);
        assert_eq!((p.cairo_status.expect("cairo_status"))(cr), 0);

        let stride = (c.image_surface_get_stride)(surface) as usize;
        let data = (c.image_surface_get_data)(surface);
        assert!(!data.is_null(), "an image surface always has pixels");
        assert!(
            ink(data, stride, W, H).is_none(),
            "a fresh ARGB32 surface must be zero-filled, or the diff below \
             measures nothing"
        );

        // pangocairo's own idea of a `cairo_t *`. `ffi` gives it a named
        // opaque type while Cairo's four entries there speak `c_void`,
        // because those exist only to have their addresses compared.
        let crp = cr.cast::<ffi::cairo_t>();

        let layout = (p.pango_cairo_create_layout.expect("create_layout"))(crp);
        assert!(!layout.is_null());

        // A named family would make the test depend on this machine's fonts.
        // "Sans" is fontconfig's own alias and resolves wherever fontconfig
        // has any font at all; the size is given in points because the
        // string form is what the image will use.
        let desc_name = std::ffi::CString::new("Sans 16").unwrap();
        let desc = (p
            .pango_font_description_from_string
            .expect("description_from_string"))(desc_name.as_ptr());
        assert!(!desc.is_null());
        (p.pango_layout_set_font_description.expect("set_font_description"))(layout, desc);

        // The control for the experiment. An empty layout draws nothing, so
        // if the surface is dirty after this, the ink counted further down
        // would not be the text either.
        (p.pango_layout_set_text.expect("set_text"))(layout, c"".as_ptr(), 0);
        (c.set_source_rgb)(cr, 1.0, 1.0, 1.0);
        (c.move_to)(cr, 2.0, 2.0);
        (p.pango_cairo_update_layout.expect("update_layout"))(crp, layout);
        (p.pango_cairo_show_layout.expect("show_layout"))(crp, layout);
        (c.surface_flush)(surface);
        assert!(
            ink(data, stride, W, H).is_none(),
            "an empty layout drew something"
        );

        let text = "Hello, Pango";
        (p.pango_layout_set_text.expect("set_text"))(
            layout,
            text.as_ptr().cast::<c_char>(),
            text.len() as c_int,
        );

        // The measurement, before the drawing, so the two are independent
        // answers about the same layout rather than one derived from the
        // other.
        let (mut pw, mut ph) = (0, 0);
        (p.pango_layout_get_pixel_size.expect("get_pixel_size"))(layout, &mut pw, &mut ph);
        assert!(
            pw > 20 && (pw as usize) < W,
            "twelve characters at 16pt measured {pw}px wide, which is not plausible"
        );
        assert!(
            ph > 8 && (ph as usize) < H,
            "one line at 16pt measured {ph}px tall, which is not plausible"
        );
        // And the unit answer must agree with the pixel one to within the
        // rounding PANGO_PIXELS does, which is the cheapest check there is
        // that `get_size` and `get_pixel_size` were not transcribed onto each
        // other's signatures.
        let (mut uw, mut uh) = (0, 0);
        (p.pango_layout_get_size.expect("get_size"))(layout, &mut uw, &mut uh);
        assert_eq!(ffi::pango_pixels(uw), pw);
        assert_eq!(ffi::pango_pixels(uh), ph);

        (c.move_to)(cr, 2.0, 2.0);
        (p.pango_cairo_update_layout.expect("update_layout"))(crp, layout);
        (p.pango_cairo_show_layout.expect("show_layout"))(crp, layout);
        // Cairo latches an error and then ignores every later call in
        // silence, so this is the only place a failed draw becomes visible.
        assert_eq!(
            (p.cairo_status.expect("cairo_status"))(cr),
            0,
            "cairo latched an error while showing the layout"
        );
        (c.surface_flush)(surface);

        let (count, [x0, y0, x1, y1]) =
            ink(data, stride, W, H).expect("the layout drew no pixels at all");
        assert!(
            count > 40,
            "only {count} pixels changed, which is too few to be twelve glyphs"
        );
        assert!(
            count < W * H / 2,
            "{count} pixels changed out of {}, so something painted the \
             surface rather than drawing text",
            W * H
        );
        // The ink must sit inside the box the measurement promised, moved to
        // where the pen was. Three pixels of slack for antialiasing and for
        // the overshoot a glyph is allowed past its logical extents; a
        // transposed width and height, or units mistaken for pixels, misses
        // by very much more than that.
        assert!(x0 >= 2, "ink at x={x0} is left of the pen");
        assert!(y0 >= 2, "ink at y={y0} is above the pen");
        assert!(
            x1 <= 2 + pw as usize + 3,
            "ink reaches x={x1} but the layout measured {pw}px wide"
        );
        assert!(
            y1 <= 2 + ph as usize + 3,
            "ink reaches y={y1} but the layout measured {ph}px tall"
        );
        // A whitespace-only difference would satisfy everything above; this
        // says the glyphs really span the measured width.
        assert!(
            x1 - x0 > pw as usize / 2,
            "the ink spans {} of a measured {pw}px, which is not a line of text",
            x1 - x0
        );

        (p.pango_font_description_free.expect("description_free"))(desc);
        unref(layout.cast());
        (p.cairo_destroy.expect("cairo_destroy"))(cr);
        (c.surface_destroy)(surface);
    }
}
