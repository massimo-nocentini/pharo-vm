//! The SDK's headline promise, exercised rather than asserted in prose.
//!
//! `run_primitive` wraps every primitive body in [`std::panic::catch_unwind`],
//! and the crate documentation has always said a panic therefore becomes a
//! clean primitive failure. That was false for every plugin that shipped:
//! cargo reads profiles only from a workspace root, and the one this crate
//! belongs to sets `panic = "abort"`, so the catch was dead code and an
//! `unwrap()` on image input killed the user's image. The plugin cdylibs now
//! build from `rust/plugins/Cargo.toml`, which sets `panic = "unwind"`, and
//! this test pins the behaviour that setting buys.
//!
//! It drives the real export -- the `extern "C"` function the VM would call --
//! against a proxy table with two entries filled in, which is all a failing
//! primitive touches. What it cannot check is the panic strategy of the
//! *shipped* cdylib: cargo ignores `panic` for test binaries, so a test always
//! runs under unwind. That half is a build-configuration fact, checked by
//! reading the flags cargo passes rustc (`cargo build --release -p <plugin>
//! -v | grep -- '-C panic='`), and stated in `rust/README.md`.

#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use pharo_vm_plugin::{
    pharo_primitive, poison, sqInt, Interp, PrimErr, PrimResult, VirtualMachine,
};

/// The code the last `primitiveFailFor` was given, or -1 for "not called".
static FAILED_WITH: AtomicIsize = AtomicIsize::new(-1);
/// The value the last `methodReturnInteger` was given.
static RETURNED: AtomicIsize = AtomicIsize::new(-1);
/// Set by the panicking body, to prove it really ran.
static BODY_RAN: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn primitiveFailFor(code: sqInt) -> sqInt {
    FAILED_WITH.store(code, Ordering::SeqCst);
    code
}

unsafe extern "C" fn methodReturnInteger(value: sqInt) -> sqInt {
    RETURNED.store(value, Ordering::SeqCst);
    value
}

/// Hands the SDK a proxy table with the two entries these primitives use.
///
/// Leaked because `setInterpreter`'s contract is that the table lives as long
/// as the process; the VM's own table does, and a `Box` on a test stack would
/// not.
fn install_fake_proxy() {
    // SAFETY: every field of `VirtualMachine` is an `Option<fn>`, whose
    // all-zero bit pattern is the guaranteed niche for `None`. Only the two
    // entries filled in below are ever called through.
    let mut vt: VirtualMachine = unsafe { core::mem::zeroed() };
    vt.primitiveFailFor = Some(primitiveFailFor);
    vt.methodReturnInteger = Some(methodReturnInteger);
    let vt: &'static mut VirtualMachine = Box::leak(Box::new(vt));
    assert_eq!(
        pharo_vm_plugin::__private::set_interpreter(vt, "PanicTestPlugin"),
        1
    );
}

/// Stands in for the lookup a plugin author is sure cannot miss.
///
/// A function rather than a literal `None` at the `expect` site, so the shape
/// is the real one -- a value that came from somewhere -- rather than
/// something the optimiser and clippy both see straight through.
fn cached_value_for(_key: sqInt) -> Option<sqInt> {
    None
}

/// A primitive that does what a plugin author does by accident.
///
/// The `expect` is the point of the fixture, not an oversight: this is the
/// shape of mistake -- an `Option` that "cannot" be `None` -- that used to
/// take the image down with it.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveThatPanics(vm: &Interp) -> PrimResult<sqInt> {
    let _ = vm;
    BODY_RAN.store(true, Ordering::SeqCst);
    Ok(cached_value_for(1).expect("the kind of mistake a plugin author makes"))
}

/// A well-behaved neighbour, to show the module still works afterwards.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAnswersSeven(vm: &Interp) -> PrimResult<sqInt> {
    let _ = vm;
    Ok(7)
}

#[test]
fn a_panicking_primitive_fails_instead_of_killing_the_process() {
    install_fake_proxy();

    // If the catch were dead code -- which is what `panic = "abort"` made it --
    // this call would not return at all: the test binary would die on SIGABRT
    // and the harness would report the process, not a failed assertion.
    let answer = primitiveThatPanics();

    assert!(BODY_RAN.load(Ordering::SeqCst), "the body has to have run");
    assert_eq!(answer, 0, "a failed primitive answers 0");
    assert_eq!(
        FAILED_WITH.load(Ordering::SeqCst),
        PrimErr::GenericFailure.code(),
        "a panic is reported to the image as an ordinary primitive failure"
    );

    // A panic over nothing but local computation is not a torn invariant, so
    // the module stays usable and the next primitive runs normally.
    assert!(
        !poison::is_poisoned(),
        "a panic with no shared state open must not disable the module"
    );
    assert_eq!(primitiveAnswersSeven(), 1);
    assert_eq!(RETURNED.load(Ordering::SeqCst), 7);
}
