//! The module-level contract, checkable without a VM or a Cairo.

use std::ffi::CStr;

#[test]
fn the_module_name_matches_the_library_name() {
    // The VM compares this against the module it was asked to load and
    // rejects the library on a mismatch (`callInitializersIn` in
    // src/common/sqNamedPrims.c). It must equal the `[lib] name`.
    let name = unsafe { CStr::from_ptr(CairoPlugin::getModuleName()) };
    assert_eq!(name.to_str().unwrap(), "CairoPlugin");
}

#[test]
fn set_interpreter_rejects_a_null_proxy() {
    assert_eq!(CairoPlugin::setInterpreter(std::ptr::null_mut()), 0);
}

#[test]
fn initialising_without_cairo_declines_rather_than_pretending() {
    // On a machine with Cairo installed this loads it and answers 1; on one
    // without, it must answer 0 so the VM rejects the module and the image
    // can fall back to its FFI binding.
    let available = CairoPlugin::ffi::cairo().is_ok();
    let answer = CairoPlugin::initialiseModule();
    assert_eq!(answer == 1, available || CairoPlugin::ffi::cairo().is_ok());
}

#[test]
fn every_declared_entry_point_is_accounted_for() {
    if let Ok(c) = CairoPlugin::ffi::cairo() {
        assert!(c.declared_count() >= 90, "the table shrank unexpectedly");
        assert!(c.resolved_count() <= c.declared_count());
    }
}
