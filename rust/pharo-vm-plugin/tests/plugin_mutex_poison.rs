//! A plugin's *own* `static Mutex` gets the same fail-fast as `Registry`.
//!
//! Its own file, and therefore its own process, for the reason
//! `tests/poison.rs` gives: the poison flag is per-cdylib and never cleared,
//! and a poisoned mutex is never unpoisoned either, so neither may leak into a
//! test that assumes a healthy module. One `#[test]` for the same reason.
//!
//! What is pinned here is the half of the wave that the SDK's `Registry` fixed
//! and the plugins did not. Twelve process-global mutexes across the plugin
//! tree recovered their own poison with `PoisonError::into_inner`, two of them
//! citing "a panic inside a primitive is caught and turned into a failure" as
//! the reason -- a promise that was vacuous while the whole tree was
//! `panic = "abort"`, and that under `panic = "unwind"` says only that
//! continuing is *possible*, never that it is *safe*. This file is shaped like
//! those sites: a two-field global that is only meaningful as a pair, torn by a
//! panic between the two writes.

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Mutex, PoisonError};

use pharo_vm_plugin::{
    pharo_primitive, poison, sqInt, Interp, PrimErr, PrimResult, VirtualMachine,
};

static FAILED_WITH: AtomicIsize = AtomicIsize::new(-1);
static BODY_RAN: AtomicBool = AtomicBool::new(false);
/// What the primitive body last read out of the cache.
static SLOT_SEEN: AtomicIsize = AtomicIsize::new(-1);

unsafe extern "C" fn primitiveFailFor(code: sqInt) -> sqInt {
    FAILED_WITH.store(code, Ordering::SeqCst);
    code
}

/// The success path: `IntoReturn for ()` answers the receiver, so the table
/// needs this entry for a healthy primitive to be distinguishable from a
/// failing one.
unsafe extern "C" fn methodReturnReceiver() -> sqInt {
    0
}

/// A plugin global in the shape all of them have: fields that mean something
/// only together. `generation` names which resource `slot` refers to, exactly
/// as `file-plugin`'s `last_path` names which directory its `open_dir` walks.
struct Cache {
    generation: u8,
    slot: u8,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    generation: 1,
    slot: 10,
});

/// The accessor every migrated site now has.
fn cache() -> PrimResult<poison::Guarded<'static, Cache>> {
    poison::lock(&CACHE)
}

/// Answers nothing through the proxy -- the fake table below has no return
/// entries -- and records what it saw in a static instead, so the assertions
/// can tell "ran and read the torn pair" from "never ran".
#[pharo_primitive(accessor_depth = -1)]
fn primitiveReadsTheCache(vm: &Interp) -> PrimResult<()> {
    let _ = vm;
    BODY_RAN.store(true, Ordering::SeqCst);
    SLOT_SEEN.store(sqInt::from(cache()?.slot), Ordering::SeqCst);
    Ok(())
}

fn install_fake_proxy() {
    // SAFETY: every field is an `Option<fn>`, whose all-zero bit pattern is
    // `None`; only the entry filled in below is called through.
    let mut vt: VirtualMachine = unsafe { core::mem::zeroed() };
    vt.primitiveFailFor = Some(primitiveFailFor);
    vt.methodReturnReceiver = Some(methodReturnReceiver);
    let vt: &'static mut VirtualMachine = Box::leak(Box::new(vt));
    // Installs the panic hook, which is what the whole file depends on.
    assert_eq!(
        pharo_vm_plugin::__private::set_interpreter(vt, "MutexPoisonTestPlugin"),
        1
    );
}

#[test]
fn a_panic_under_a_plugins_own_mutex_disables_the_module() {
    install_fake_proxy();

    // --- healthy ---------------------------------------------------------
    assert_eq!(cache().map(|c| c.slot), Ok(10));
    BODY_RAN.store(false, Ordering::SeqCst);
    assert_eq!(primitiveReadsTheCache(), 1, "a healthy primitive succeeds");
    assert!(BODY_RAN.load(Ordering::SeqCst));
    assert_eq!(SLOT_SEEN.load(Ordering::SeqCst), 10);

    // --- a panic that tears nothing --------------------------------------
    //
    // No lock held, so no `Section` is open: an ordinary primitive failure,
    // and the module carries on. Anything else would let one bad argument
    // deep in a computation disable a plugin for the rest of the session.
    let outside = std::panic::catch_unwind(|| panic!("holding nothing"));
    assert!(outside.is_err());
    assert!(!poison::is_poisoned(), "a panic over nothing is not poison");
    assert_eq!(cache().map(|c| c.slot), Ok(10));

    // --- a panic between the two writes ----------------------------------
    let inside = std::panic::catch_unwind(|| {
        let mut c = cache().expect("still healthy");
        c.generation = 2;
        // The pair now disagrees: generation 2 with slot 10.
        panic!("halfway through re-pointing the cache");
    });
    assert!(inside.is_err());

    // The state really is torn, which is what makes the assertions below
    // mean something. Reached the way the old code reached it -- and this is
    // the only place in the tree that may still do so.
    {
        let recovered = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        assert_eq!(
            (recovered.generation, recovered.slot),
            (2, 10),
            "a swallowed poison hands back exactly this inconsistent pair"
        );
    }

    // --- the two halves of the fix ---------------------------------------
    //
    // std's poison, honoured rather than recovered: this lock refuses.
    assert_eq!(
        cache().map(|c| c.slot),
        Err(PrimErr::Unsupported),
        "the torn pair must never be handed to a caller"
    );

    // And the `Section` the lock opened, which is what tells the hook this
    // panic was tearing something rather than merely failing.
    assert!(
        poison::is_poisoned(),
        "the hook must see the guard still held and poison the module"
    );

    // --- so the next primitive fails fast, without running its body ------
    BODY_RAN.store(false, Ordering::SeqCst);
    FAILED_WITH.store(-1, Ordering::SeqCst);
    SLOT_SEEN.store(-1, Ordering::SeqCst);
    assert_eq!(primitiveReadsTheCache(), 0);
    assert!(
        !BODY_RAN.load(Ordering::SeqCst),
        "the gate is before the body, not after it"
    );
    assert_eq!(
        SLOT_SEEN.load(Ordering::SeqCst),
        -1,
        "nothing read the torn state"
    );
    assert_eq!(
        FAILED_WITH.load(Ordering::SeqCst),
        PrimErr::Unsupported.code(),
        "Unsupported, never NoMemory: the image retries NoMemory after a \
         scavenge and then a full GC, on every single call"
    );
}
