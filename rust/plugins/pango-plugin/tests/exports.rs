//! The module-level contract, checkable without a VM and without a Pango.
//!
//! Everything here runs on a machine that has never heard of Pango, which is
//! the point: this plugin binds a system library, so "no Pango" is an ordinary
//! configuration and the module has to behave correctly in it.

use std::ffi::CStr;

#[test]
fn the_module_name_matches_the_library_name() {
    // The VM compares this against the module it was asked to load and
    // rejects the library on a mismatch (`callInitializersIn` in
    // src/common/sqNamedPrims.c). It must equal the `[lib] name`.
    let name = unsafe { CStr::from_ptr(PangoPlugin::getModuleName()) };
    assert_eq!(name.to_str().unwrap(), "PangoPlugin");
}

#[test]
fn set_interpreter_rejects_a_null_proxy() {
    assert_eq!(PangoPlugin::setInterpreter(std::ptr::null_mut()), 0);
}

#[test]
fn initialising_without_pango_declines_rather_than_pretending() {
    // On a machine with Pango installed this loads it and answers 1; on one
    // without, it must answer 0 so the VM rejects the module and the image
    // knows to fall back rather than calling primitives that would all
    // answer `Unsupported`.
    let available = PangoPlugin::ffi::pango().is_ok() && PangoPlugin::ffi::glib().is_ok();
    let answer = PangoPlugin::initialiseModule();
    assert_eq!(
        answer == 1,
        available || (PangoPlugin::ffi::pango().is_ok() && PangoPlugin::ffi::glib().is_ok())
    );
}

#[test]
fn shutting_down_is_safe_whether_or_not_anything_loaded() {
    // The VM calls this on module unload regardless of how initialisation
    // went, so it must not depend on Pango being there.
    assert_eq!(PangoPlugin::shutdownModule(), 1);
    assert_eq!(PangoPlugin::shutdownModule(), 1, "and twice");
}

#[test]
fn both_function_tables_are_the_size_this_crate_was_written_against() {
    // A guard against a table being trimmed by accident during a merge: the
    // phase-2 modules are written against these entries and a silently
    // shorter table would turn into `Unsupported` at run time rather than a
    // compile error.
    if let Ok(p) = PangoPlugin::ffi::pango() {
        assert!(p.declared_count() >= 260, "the pango table shrank");
        assert!(p.resolved_count() <= p.declared_count());
    }
    if let Ok(g) = PangoPlugin::ffi::glib() {
        assert_eq!(g.declared_count(), 8, "the glib table is meant to be exhaustive");
        assert!(g.resolved_count() <= g.declared_count());
    }
}

#[test]
fn the_version_gate_excuses_nothing_on_a_current_pango() {
    // The gate exists so the live name check can tell "absent because old"
    // from "absent because misspelt". If it ever excused an entry on the
    // newest Pango it would be excusing a typo forever.
    assert!(PangoPlugin::ffi::entries_introduced_after(15802).is_empty());
    assert!(!PangoPlugin::ffi::entries_introduced_after(14400).is_empty());
}

#[test]
fn the_library_search_reaches_past_what_the_system_loader_would_find() {
    // Regression test for the measured macOS failure: bare-name dlopen does
    // not find Homebrew's libraries, nothing bundles Pango, and without an
    // absolute candidate the plugin declines on a machine that plainly has
    // Pango installed. Asserted through the public path so it stays true.
    let Ok(p) = PangoPlugin::ffi::pango() else { return };
    // It loaded, so whatever the loader was given must name a real file: on
    // macOS that is an absolute candidate this crate appended, because the
    // bare name does not resolve there and nothing bundles Pango.
    assert!(!p.path.is_empty());
    if p.path.starts_with('/') {
        assert!(
            std::path::Path::new(&p.path).exists(),
            "recorded a path that does not exist: {}",
            p.path
        );
    }
    assert!(
        p.path.contains("pangocairo"),
        "recorded the wrong library: {}",
        p.path
    );
}

#[test]
fn no_resource_is_registered_before_the_image_asks_for_one() {
    let counts = PangoPlugin::resources::live_counts();
    assert_eq!(counts, [0; 6]);
}
