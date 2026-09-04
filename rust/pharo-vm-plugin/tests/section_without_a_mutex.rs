//! Fail-fast for shared state that has no mutex to poison.
//!
//! Its own file, and therefore its own process, for the reason
//! `tests/poison.rs` gives: the flag is per-cdylib and never cleared. One
//! `#[test]`, because the harness runs tests in parallel threads within one
//! binary and this one has to observe the flag going from clear to set.
//!
//! The other two mechanisms lean on `std`'s mutex poison and only *add* the
//! module flag. `b2d-plugin`'s `with_globals` has no mutex at all -- its
//! globals live in an `UnsafeCell` asserted `Sync` on the
//! single-interpreter-thread contract, exactly as the C left them in file
//! scope -- so for that shape the [`poison::Section`] is not an addition, it is
//! the whole of the story. This file is that shape.

#![allow(non_snake_case)]

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use pharo_vm_plugin::{
    pharo_primitive, poison, sqInt, Interp, PrimErr, PrimResult, VirtualMachine,
};

static FAILED_WITH: AtomicIsize = AtomicIsize::new(-1);
static BODY_RAN: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn primitiveFailFor(code: sqInt) -> sqInt {
    FAILED_WITH.store(code, Ordering::SeqCst);
    code
}

unsafe extern "C" fn methodReturnReceiver() -> sqInt {
    0
}

/// `b2d-plugin`'s `GlobalsCell` in miniature: a pair of function pointers
/// written together by `initialiseModule` and nulled together by
/// `moduleUnloaded`, so that one written without the other is a live pointer
/// into a `dlclose`d library.
struct GlobalsCell(UnsafeCell<(usize, usize)>);

// SAFETY: the VM calls every plugin entry point on the interpreter thread --
// the same contract `b2d-plugin` asserts, and the same one this test keeps.
unsafe impl Sync for GlobalsCell {}

static GLOBALS: GlobalsCell = GlobalsCell(UnsafeCell::new((0, 0)));

/// The two-line wrap the `poison` module documents, and the whole of the fix
/// at a site with no mutex.
fn with_globals<R>(f: impl FnOnce(&mut (usize, usize)) -> R) -> R {
    let _section = poison::Section::enter();
    // SAFETY: single thread, as asserted above.
    unsafe { f(&mut *GLOBALS.0.get()) }
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveUsesTheGlobals(vm: &Interp) -> PrimResult<()> {
    let _ = vm;
    BODY_RAN.store(true, Ordering::SeqCst);
    with_globals(|g| {
        // The call a plugin would make through the pair it just read.
        assert_eq!(g.0, g.1, "the two are only ever written together");
    });
    Ok(())
}

fn install_fake_proxy() {
    // SAFETY: every field is an `Option<fn>`, whose all-zero bit pattern is
    // `None`; only the entries filled in below are called through.
    let mut vt: VirtualMachine = unsafe { core::mem::zeroed() };
    vt.primitiveFailFor = Some(primitiveFailFor);
    vt.methodReturnReceiver = Some(methodReturnReceiver);
    let vt: &'static mut VirtualMachine = Box::leak(Box::new(vt));
    // Installs the panic hook, which is what the whole file depends on.
    assert_eq!(
        pharo_vm_plugin::__private::set_interpreter(vt, "SectionTestPlugin"),
        1
    );
}

#[test]
fn a_panic_inside_a_section_disables_the_module_with_no_mutex_in_sight() {
    install_fake_proxy();

    // --- healthy ---------------------------------------------------------
    with_globals(|g| *g = (1, 1));
    BODY_RAN.store(false, Ordering::SeqCst);
    assert_eq!(primitiveUsesTheGlobals(), 1, "a healthy primitive succeeds");
    assert!(BODY_RAN.load(Ordering::SeqCst));

    // --- a panic outside every section -----------------------------------
    let outside = std::panic::catch_unwind(|| panic!("nothing is mid-mutation"));
    assert!(outside.is_err());
    assert!(!poison::is_poisoned(), "a panic over nothing is not poison");

    // --- a panic between the two writes ----------------------------------
    let inside = std::panic::catch_unwind(|| {
        with_globals(|g| {
            g.0 = 2;
            // g.1 is still 1: the pair now disagrees, and there is no mutex
            // anywhere that records the fact.
            panic!("halfway through re-pointing the globals");
        });
    });
    assert!(inside.is_err());

    assert!(
        poison::is_poisoned(),
        "with no mutex to poison, the Section is the only thing that can \
         tell the hook this panic tore something"
    );

    // The state really is torn, which is what makes the gate matter: run the
    // body now and its own assertion would fire.
    with_globals(|g| assert_eq!(*g, (2, 1), "torn, as the panic left it"));

    // --- so the next primitive fails fast, without running its body ------
    BODY_RAN.store(false, Ordering::SeqCst);
    FAILED_WITH.store(-1, Ordering::SeqCst);
    assert_eq!(primitiveUsesTheGlobals(), 0);
    assert!(
        !BODY_RAN.load(Ordering::SeqCst),
        "the gate is before the body, not after it"
    );
    assert_eq!(
        FAILED_WITH.load(Ordering::SeqCst),
        PrimErr::Unsupported.code(),
        "Unsupported, never NoMemory: the image retries NoMemory after a \
         scavenge and then a full GC, on every single call"
    );
}
