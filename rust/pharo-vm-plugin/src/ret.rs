//! What a primitive can answer.

use crate::error::PrimResult;
use crate::handles::{Handle, Resource};
use crate::interp::{Interp, Oop};
use crate::proxy::sqInt;

/// Implemented by every type a `#[pharo_primitive]` body may return.
///
/// This is what lets a primitive be written as `-> PrimResult<f64>` instead of
/// hand-rolling a `methodReturnFloat` call.
///
/// Every impl runs *after* the primitive body has returned, and the ones that
/// allocate can fail with [`PrimErr::NoMemory`](crate::PrimErr::NoMemory) --
/// which makes the VM re-run the whole primitive. See the `Vec<u8>` impl below
/// for what that costs a body with side effects. The impls that can answer it:
///
/// - `sqInt` and `i32`, through [`Interp::return_integer`]: a value outside
///   `MIN_SMALL_INTEGER..=MAX_SMALL_INTEGER` is boxed as a LargeInteger, and
///   `signed64BitIntegerFor` answering 0 is `NoMemory`. Easy to overlook,
///   because the common case -- a small count, a small index -- is an
///   immediate and allocates nothing.
/// - `Vec<u8>`, through [`Interp::instantiate`].
/// - `&str` and `String`, through [`Interp::string`].
/// - `Option<T>`, exactly when `T`'s own impl can; `None` allocates nothing.
///
/// `Handle<R>` goes through `return_integer` too, but cannot reach the boxing
/// path: `MAX_HANDLE <= MAX_SMALL_INTEGER` is a compile-time assertion
/// (`handles.rs`), so every handle is an immediate.
///
/// `f64` is the one allocating impl that *cannot* report the failure:
/// `methodReturnFloat` boxes a Float in the image but answers 0 whatever
/// happens (`InterpreterProxy>>#methodReturnFloat:`,
/// `smalltalksrc/VMMaker/InterpreterProxy.class.st:665-670`), so there is
/// nothing here to check.
pub trait IntoReturn {
    /// Hands the value back to the VM as the primitive's answer.
    fn into_return(self, vm: &Interp) -> PrimResult<()>;
}

/// Answering nothing answers the receiver, matching Smalltalk's default.
impl IntoReturn for () {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_receiver()
    }
}

impl IntoReturn for Oop {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_value(self)
    }
}

impl IntoReturn for bool {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_bool(self)
    }
}

impl IntoReturn for f64 {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_float(self)
    }
}

impl IntoReturn for sqInt {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_integer(self)
    }
}

/// A resource handle answers the integer the image holds it by.
///
/// So a constructor primitive can be written `-> PrimResult<Handle<Surface>>`
/// and say in its signature what kind of handle it mints, instead of answering
/// an anonymous `sqInt`.
impl<R: Resource> IntoReturn for Handle<R> {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_integer(self.raw())
    }
}

/// Convenience for the common `-> PrimResult<i32>` case.
///
/// `isize: From<i32>` does not exist, because `isize` may be 16 bits. The VM
/// has no 16-bit target -- `sqInt` is pointer-sized, so 32 or 64 -- which
/// makes the widening lossless here.
impl IntoReturn for i32 {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        vm.return_integer(self as sqInt)
    }
}

impl IntoReturn for &str {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        let oop = vm.string(self)?;
        vm.return_value(oop)
    }
}

impl IntoReturn for String {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        self.as_str().into_return(vm)
    }
}

/// Answers a ByteArray holding these bytes.
///
/// The shape a plugin hands back a buffer in: a hash, a serialised struct, a
/// blob a foreign library filled in. An empty `Vec` answers an empty
/// ByteArray, not nil.
///
/// # Allocating an answer can make the primitive run twice
///
/// This allocates, so it can fail with
/// [`PrimErr::NoMemory`](crate::PrimErr::NoMemory) -- and that failure is not
/// the end of the call. When a primitive fails with `PrimErrNoMemory` the
/// interpreter scavenges and, for a named external call, **runs the primitive
/// again** from the top: `StackInterpreter>>#retryPrimitiveOnFailure`
/// (`smalltalksrc/VMMaker/StackInterpreter.class.st:13146-13182`) counts
/// `gcDone`, checks `isExternalPrimitiveCall:`, does a scavenge on the first
/// failure and a full GC on the second, and re-dispatches the primitive
/// function pointer while `gcDone <= 2`.
///
/// `IntoReturn` runs *after* the primitive body has returned. So a body that
/// changed Rust-side state -- advanced a cursor, closed a handle, drained a
/// queue, freed a registry entry -- and then failed to allocate its answer is
/// re-entered against the state it already changed, and does the whole thing
/// a second time. Nothing in the C tells it apart from a first call.
///
/// Hence the rule for every primitive answering an allocated object:
/// **allocate first, mutate Rust-side state last, and never answer `NoMemory`
/// after a side effect.** Where the order cannot be arranged -- the size of
/// the answer is only known once the work is done -- either make the body
/// idempotent, or have the image pass a destination object in and fill it
/// with [`Interp::write_bytes`], which allocates nothing.
impl IntoReturn for Vec<u8> {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        let size = sqInt::try_from(self.len())?;
        let oop = vm.instantiate(vm.class_byte_array()?, size)?;
        vm.write_bytes(oop, 0, &self)?;
        vm.return_value(oop)
    }
}

/// `Some(x)` answers whatever `x` answers; `None` answers nil.
///
/// How a primitive says the library had nothing to give -- no matrix on this
/// context, no match for that name -- without a sentinel the image has to be
/// taught to recognise.
///
/// `None` allocates nothing. `Some` inherits everything `T`'s own impl does,
/// including the re-run hazard spelled out on the `Vec<u8>` impl above: the
/// allocation still happens after the body has finished, so the same rule
/// applies -- allocate first, mutate Rust-side state last.
impl<T: IntoReturn> IntoReturn for Option<T> {
    fn into_return(self, vm: &Interp) -> PrimResult<()> {
        match self {
            Some(value) => value.into_return(vm),
            None => {
                let nil = vm.nil()?;
                vm.return_value(nil)
            }
        }
    }
}
