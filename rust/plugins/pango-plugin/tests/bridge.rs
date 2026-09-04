//! The consumer half of the CairoPlugin bridge, on a machine where there is no
//! CairoPlugin.
//!
//! That is the configuration these tests care about, and it is the ordinary one
//! for `cargo test`: there is no VM, so there is no plugin directory, so
//! `ioLoadFunctionFrom` can never find anything. The bridge has to answer a
//! *named* reason and a clean `Unsupported` there -- not panic, not block, and
//! not hand out a pointer it could not verify.
//!
//! Note what is deliberately **not** asserted: that the identity handshake
//! passes. On a homebrew macOS machine with a bundled Cairo it is expected to
//! fail, because two different Cairos are mapped, and refusing is the correct
//! outcome rather than a defect. So what is tested is the refusal path.
//!
//! The VM is faked. Every field of the proxy table is an `Option<fn>`, so a
//! zeroed table is a VM that supports nothing, and one field filled in is a VM
//! that supports exactly that -- enough to walk the bridge's state machine
//! through each of its branches without a Pharo image anywhere.

use core::ffi::{c_char, c_void};
use std::cell::Cell;
use std::sync::Mutex;

use pharo_vm_plugin::{Interp, PrimErr, VirtualMachine};
use PangoPlugin::cairo_bridge;

/// The cached bridge state is one `static` for the whole test binary, and
/// cargo runs tests on several threads. Every test that resolves or forgets it
/// takes this first, so one test's cached negative is never another's starting
/// point.
static SERIAL: Mutex<()> = Mutex::new(());

/// A proxy table that supports nothing at all.
///
/// Sound because every field of `VirtualMachine` is an `Option<fn>`, whose
/// all-zero bit pattern is `None`: this is a VM whose every entry point is
/// missing, which is exactly what a plugin sees from a VM too old to have one.
fn a_vm_that_supports_nothing() -> VirtualMachine {
    // SAFETY: as above -- all-zero is a valid `VirtualMachine`, being `None` in
    // every field.
    unsafe { core::mem::zeroed() }
}

/// `ioLoadFunctionFrom` as the VM implements it for a module that is present
/// but exports none of the names asked for.
///
/// A null function name answers the constant 1 -- "the module is there" -- and
/// every real name answers null. `sqNamedPrims.c:329-332` tests the *pointer*,
/// which is what makes those two answers distinguishable at all.
unsafe extern "C" fn a_module_with_no_bridge(
    function: *mut c_char,
    _module: *mut c_char,
) -> *mut c_void {
    if function.is_null() {
        // Not an address, and never dereferenced: `ioLoadFunctionFrom`
        // answers this literal constant, and `module_is_loadable` exists
        // exactly because it is the only way to reach it.
        return 1 as *mut c_void;
    }
    core::ptr::null_mut()
}

#[test]
fn without_cairo_plugin_the_bridge_names_a_reason_instead_of_panicking() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cairo_bridge::forget();
    let mut vt = a_vm_that_supports_nothing();
    // SAFETY: `vt` is a valid proxy table and outlives every use of `vm`.
    let vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut vt)) };

    let reason = cairo_bridge::unavailable_because(&vm)
        .expect("a VM that cannot load a module cannot have a bridge");
    // The empty string is the image's signal that the bridge *works*, so the
    // unavailable path must never produce one.
    assert!(!reason.is_empty(), "an unnamed reason is not a diagnostic");
    assert!(
        reason.contains("Cairo") || reason.contains("cairo") || reason.contains("pango"),
        "the reason names neither library: {reason}"
    );
}

#[test]
fn borrowing_a_context_without_cairo_plugin_is_unsupported_and_never_runs_the_body() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cairo_bridge::forget();
    let mut vt = a_vm_that_supports_nothing();
    // SAFETY: as above.
    let vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut vt)) };

    let ran = Cell::new(false);
    let answer = cairo_bridge::with_cairo_context(&vm, 1, |_cr| {
        ran.set(true);
        Ok(())
    });
    // `Unsupported`, not `NotFound`: the handle was never even looked at,
    // because there is nothing to look it up in. The image distinguishes "this
    // configuration cannot draw Pango text" from "that context is gone" on
    // exactly this difference.
    assert_eq!(answer, Err(PrimErr::Unsupported));
    assert!(
        !ran.get(),
        "the body ran, which means it was handed a cairo_t that was never verified"
    );
}

#[test]
fn the_failed_lookup_is_cached_and_forgetting_it_really_looks_again() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Load Pango *before* the first answer is taken, not after the last one.
    // `unavailable_because` reports "libpangocairo did not load" until
    // something has called `ffi::load()`, and that call is a process-wide
    // `OnceLock` shared with every other test in this binary -- so asking
    // afterwards makes this test's assertion depend on whether some other
    // test happened to be scheduled first, which is exactly the flake this
    // hoisting removes.
    let pango_loaded = PangoPlugin::ffi::load();
    cairo_bridge::forget();

    let mut absent = a_vm_that_supports_nothing();
    // SAFETY: valid tables, both outliving their `Interp`s.
    let absent_vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut absent)) };
    let mut present = a_vm_that_supports_nothing();
    present.ioLoadFunctionFrom = Some(a_module_with_no_bridge);
    // SAFETY: as above.
    let present_vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut present)) };

    let first = cairo_bridge::unavailable_because(&absent_vm).expect("no module, no bridge");
    // Asked again with a VM that answers differently, and *without* forgetting:
    // the cached negative must win, because the whole reason it is cached is
    // that re-resolving costs a walk of every plugin path on every miss.
    let cached = cairo_bridge::unavailable_because(&present_vm).expect("still no bridge");
    assert_eq!(cached, first, "the negative answer was not cached");

    cairo_bridge::forget();
    let refreshed = cairo_bridge::unavailable_because(&present_vm).expect("still no bridge");
    if pango_loaded {
        // With pango loaded the two VMs reach different branches -- "no such
        // module" against "the module is there and exports no v1 bridge" -- so
        // a changed answer is proof the cache was really dropped rather than
        // re-read.
        assert_ne!(
            refreshed, first,
            "forget() did not drop the cached negative result"
        );
    }
}

#[test]
fn a_module_that_is_present_but_has_no_bridge_is_told_apart_from_a_missing_one() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !PangoPlugin::ffi::load() {
        eprintln!("skipping: no pango, so every reason collapses to that one");
        return;
    }
    cairo_bridge::forget();

    let mut vt = a_vm_that_supports_nothing();
    vt.ioLoadFunctionFrom = Some(a_module_with_no_bridge);
    // SAFETY: valid table, outliving `vm`.
    let vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut vt)) };

    let reason = cairo_bridge::unavailable_because(&vm).expect("the module exports no bridge");
    // `load_function_from` cannot tell "no module" from "no symbol" -- both are
    // NotFound -- which is why `resolve` asks `module_is_loadable` first. If
    // that ever stopped happening this reason would silently become the wrong
    // one, and the person reading it would go looking for a plugin that is
    // installed.
    assert!(
        reason.contains("bridge"),
        "a present CairoPlugin was reported as absent: {reason}"
    );
}

#[test]
fn the_library_path_of_a_cairo_plugin_that_is_not_there_is_empty_rather_than_a_guess() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut vt = a_vm_that_supports_nothing();
    // SAFETY: valid table, outliving `vm`.
    let vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut vt)) };
    assert_eq!(cairo_bridge::cairo_plugin_library_path(&vm), "");
}

#[test]
fn module_unloaded_survives_every_name_the_vm_can_pass() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // The VM ignores the answer, so what is being checked is that the call
    // returns at all: this runs with the C stack of `ioUnloadModule` under it,
    // and a panic escaping here would be undefined behaviour rather than a
    // failed test.
    //
    // SAFETY: the VM passes `ModuleEntry::name`, and each of these is either
    // null or a NUL-terminated string, which is that contract.
    unsafe {
        assert_eq!(PangoPlugin::moduleUnloaded(core::ptr::null_mut()), 1);
        let mut other = *b"B2DPlugin\0";
        assert_eq!(
            PangoPlugin::moduleUnloaded(other.as_mut_ptr().cast::<c_char>()),
            1
        );
        let mut cairo = *b"CairoPlugin\0";
        assert_eq!(
            PangoPlugin::moduleUnloaded(cairo.as_mut_ptr().cast::<c_char>()),
            1
        );
        // Not UTF-8, and so not CairoPlugin either. It must be compared as
        // bytes rather than decoded, or this is a panic inside an `extern "C"`.
        let mut invalid = [0xffu8, 0xfe, 0x00];
        assert_eq!(
            PangoPlugin::moduleUnloaded(invalid.as_mut_ptr().cast::<c_char>()),
            1
        );
    }
}

#[test]
fn unloading_cairo_plugin_drops_whatever_the_bridge_had_cached() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !PangoPlugin::ffi::load() {
        eprintln!("skipping: no pango, so every reason collapses to that one");
        return;
    }
    cairo_bridge::forget();

    let mut absent = a_vm_that_supports_nothing();
    // SAFETY: valid tables, both outliving their `Interp`s.
    let absent_vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut absent)) };
    let mut present = a_vm_that_supports_nothing();
    present.ioLoadFunctionFrom = Some(a_module_with_no_bridge);
    // SAFETY: as above.
    let present_vm = unsafe { Interp::from_raw(core::ptr::from_mut(&mut present)) };

    let cached = cairo_bridge::unavailable_because(&absent_vm).expect("no module, no bridge");
    // The VM `dlclose`s CairoPlugin right after this call, so anything resolved
    // out of it dangles from here. Nothing cached may survive the notification.
    // SAFETY: a NUL-terminated name, as the VM passes.
    unsafe {
        let mut name = *b"CairoPlugin\0";
        PangoPlugin::moduleUnloaded(name.as_mut_ptr().cast::<c_char>());
    }
    let after = cairo_bridge::unavailable_because(&present_vm).expect("still no bridge");
    assert_ne!(
        after, cached,
        "moduleUnloaded left a stale answer behind, so it would leave stale pointers too"
    );
}
