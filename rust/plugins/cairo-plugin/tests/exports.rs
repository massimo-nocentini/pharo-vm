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

// ---- the cross-plugin bridge ----------------------------------------------
//
// These five symbols are what PangoPlugin resolves out of this library through
// the VM's `ioLoadFunctionFrom`, so they are ABI: a rename is a breaking change
// for a consumer that this build cannot catch, because the lookup is a `dlsym`
// by literal name and fails at run time, in one primitive, on someone else's
// machine.

#[test]
fn the_bridge_abi_version_is_the_one_consumers_were_written_against() {
    assert_eq!(CairoPlugin::bridge::cairoPluginBridgeAbiVersion(), 1);
    assert_eq!(CairoPlugin::bridge::BRIDGE_ABI, 1);
}

#[test]
fn the_bridge_struct_is_the_size_the_wire_format_promises() {
    // A consumer declares this layout a second time in its own crate; nothing
    // links the two declarations, so the size is checked on both sides.
    assert_eq!(
        core::mem::size_of::<CairoPlugin::bridge::CairoBridgeContextV1>(),
        32
    );
    assert_eq!(
        core::mem::align_of::<CairoPlugin::bridge::CairoBridgeContextV1>(),
        8
    );
}

#[test]
fn borrowing_a_bogus_handle_answers_zero_without_touching_the_struct() {
    use core::mem::MaybeUninit;
    let mut out = MaybeUninit::<CairoPlugin::bridge::CairoBridgeContextV1>::uninit();
    let size = core::mem::size_of::<CairoPlugin::bridge::CairoBridgeContextV1>() as u32;
    // Handle 0 is never valid: `handles.rs` starts generations at 1.
    // SAFETY: `out` is writable and aligned, and `size` is its real size.
    let answer =
        unsafe { CairoPlugin::bridge::cairoPluginBorrowContext_v1(0, out.as_mut_ptr(), size) };
    assert_eq!(answer, 0, "a dead handle must not be lent out");
    // `out` is still uninitialised, which is the contract: written whole or not
    // at all. Nothing reads it.
}

#[test]
fn a_wrong_struct_size_is_refused_rather_than_written_through() {
    use core::mem::MaybeUninit;
    let mut out = MaybeUninit::<CairoPlugin::bridge::CairoBridgeContextV1>::uninit();
    // SAFETY: `out` is writable and aligned; the wrong `out_size` is the point
    // of the test, and the callee must refuse rather than write.
    assert_eq!(
        unsafe { CairoPlugin::bridge::cairoPluginBorrowContext_v1(0, out.as_mut_ptr(), 8) },
        0
    );
}

#[test]
fn a_null_out_pointer_is_refused() {
    // SAFETY: a null `out` is the case under test; the callee checks it first
    // and never dereferences it.
    assert_eq!(
        unsafe { CairoPlugin::bridge::cairoPluginBorrowContext_v1(1, core::ptr::null_mut(), 32) },
        0
    );
}

#[test]
fn the_status_of_a_dead_handle_is_minus_one() {
    // Distinguishable from every real `cairo_status_t`, all of which are
    // non-negative.
    assert_eq!(CairoPlugin::bridge::cairoPluginContextStatus_v1(0), -1);
}

#[test]
fn the_cairo_identity_is_published_whenever_cairo_loaded() {
    // The token PangoPlugin compares against the `cairo_create` its own
    // pangocairo will really call. It must be a real address when Cairo is
    // here, or the handshake cannot distinguish "same Cairo" from "no Cairo".
    let identity = CairoPlugin::bridge::cairoPluginCairoIdentity_v1();
    let path = CairoPlugin::bridge::cairoPluginCairoPath_v1();
    if CairoPlugin::ffi::cairo().is_ok() {
        assert!(!identity.is_null(), "cairo loaded but published no identity");
        assert!(!path.is_null(), "cairo loaded but published no path");
    } else {
        assert!(identity.is_null());
        assert!(path.is_null());
    }
}
