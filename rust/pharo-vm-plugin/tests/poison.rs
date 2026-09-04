//! A panic that tore shared state disables the module instead of continuing.
//!
//! Its own file, and therefore its own process: the poison flag is per-cdylib
//! and never cleared, so it must not leak into the tests that assume a healthy
//! module. One `#[test]`, for the same reason -- the harness runs tests in
//! parallel threads within one binary.
//!
//! The hazard being pinned is the one the unwind switch created. `Registry`
//! holds its mutex across the caller's closure, so a panic in there leaves a
//! slot half-written; the old `lock()` swallowed the resulting `PoisonError`
//! with `PoisonError::into_inner`, justifying it with the very promise that
//! `panic = "abort"` made vacuous. Under unwind that swallow would turn "die
//! on a broken invariant" into "carry on with one".

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};
use std::sync::Mutex;

use pharo_vm_plugin::{
    pharo_primitive, poison, sqInt, Interp, PrimErr, PrimResult, Registry, VirtualMachine,
};

static FAILED_WITH: AtomicIsize = AtomicIsize::new(-1);
static BODY_RAN: AtomicBool = AtomicBool::new(false);
/// Counts what the registry released, to prove a poisoned `drain` frees nothing.
static DROPPED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn primitiveFailFor(code: sqInt) -> sqInt {
    FAILED_WITH.store(code, Ordering::SeqCst);
    code
}

/// Stands in for the raw pointer a real plugin registers: something whose
/// release is observable.
struct Tracked(u32);

impl Drop for Tracked {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

// One invocation per library, exactly as a plugin declares its kinds.
pharo_vm_plugin::resource_tags! { Tracked = 1 }

static REGISTRY: Registry<Tracked> = Registry::new();
/// Shared state the SDK does *not* own, reached through [`poison::lock`].
static UNGUARDED: Mutex<u32> = Mutex::new(0);

#[pharo_primitive(accessor_depth = -1)]
fn primitiveTouchesNothing(vm: &Interp) -> PrimResult<()> {
    let _ = vm;
    BODY_RAN.store(true, Ordering::SeqCst);
    Ok(())
}

fn install_fake_proxy() {
    // SAFETY: every field is an `Option<fn>`, whose all-zero bit pattern is
    // `None`; only the entry filled in below is called through.
    let mut vt: VirtualMachine = unsafe { core::mem::zeroed() };
    vt.primitiveFailFor = Some(primitiveFailFor);
    let vt: &'static mut VirtualMachine = Box::leak(Box::new(vt));
    // Installs the panic hook, which is what the whole file depends on.
    assert_eq!(
        pharo_vm_plugin::__private::set_interpreter(vt, "PoisonTestPlugin"),
        1
    );
}

#[test]
fn a_panic_that_tears_the_registry_disables_the_module() {
    install_fake_proxy();

    let handle = REGISTRY.insert(Tracked(1)).expect("a fresh registry");
    assert_eq!(REGISTRY.with(handle, |r| r.0), Ok(1));

    // --- a panic that tears nothing -------------------------------------
    //
    // Outside any critical section, so it is an ordinary primitive failure
    // and the module carries on. Anything else would let one bad argument
    // deep in a computation disable a plugin for the rest of the session.
    let outside = std::panic::catch_unwind(|| panic!("nothing is mid-mutation"));
    assert!(outside.is_err());
    assert!(!poison::is_poisoned(), "a panic over nothing is not poison");
    assert_eq!(REGISTRY.with(handle, |r| r.0), Ok(1));

    // --- a panic mid-mutation -------------------------------------------
    //
    // `with_mut` holds the lock across the closure, so this is the real
    // shape of the hazard: the slot is left half-written.
    let inside = std::panic::catch_unwind(|| {
        REGISTRY.with_mut(handle, |r| {
            r.0 = 99;
            panic!("halfway through mutating the slot");
        })
    });
    assert!(inside.is_err());

    assert!(
        poison::is_poisoned(),
        "the hook must see the guard still held and poison the module"
    );

    // --- every registry operation refuses, and says why -----------------
    assert_eq!(REGISTRY.with(handle, |r| r.0), Err(PrimErr::Unsupported));
    assert_eq!(
        REGISTRY.with_mut(handle, |r| r.0),
        Err(PrimErr::Unsupported)
    );
    assert!(matches!(
        REGISTRY.insert(Tracked(2)),
        Err(PrimErr::Unsupported)
    ));
    assert!(matches!(REGISTRY.remove(handle), Err(PrimErr::Unsupported)));
    assert!(
        !REGISTRY.is_live(handle.raw()),
        "nothing in it is trustworthy"
    );
    assert_eq!(REGISTRY.len(), 0);
    assert!(REGISTRY.is_empty());
    assert!(REGISTRY.remove_where(|_| true).is_empty());

    // Insert dropped the `Tracked(2)` it could not store; nothing else has
    // been released, and `drain` must not release anything either.
    let released_by_the_failed_insert = DROPPED.load(Ordering::SeqCst);
    assert!(REGISTRY.drain().is_empty(), "a torn table frees nothing");
    assert_eq!(
        DROPPED.load(Ordering::SeqCst),
        released_by_the_failed_insert,
        "draining a poisoned registry must leak rather than free garbage"
    );

    // --- and primitives fail fast, without running their bodies ---------
    BODY_RAN.store(false, Ordering::SeqCst);
    assert_eq!(primitiveTouchesNothing(), 0);
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

    // --- the same fail-fast reaches a plugin's own mutex ----------------
    //
    // The flag is already set, so this only shows the seam exists: a plugin
    // routes its own `static Mutex` through `poison::lock`, which opens the
    // same `Section` the registry does. `tests/plugin_mutex_poison.rs` is
    // where that mechanism is pinned properly, in a process of its own.
    let hand_rolled = std::panic::catch_unwind(|| {
        let mut guarded = poison::lock(&UNGUARDED).expect("not yet poisoned");
        *guarded = 1;
        panic!("mid-mutation of state no Registry owns");
    });
    assert!(hand_rolled.is_err());
    assert!(poison::is_poisoned());
    assert_eq!(
        poison::lock(&UNGUARDED).map(|g| *g),
        Err(PrimErr::Unsupported),
        "and the mutex it tore refuses afterwards"
    );
}
