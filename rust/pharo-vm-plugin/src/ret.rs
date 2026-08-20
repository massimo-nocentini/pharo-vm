//! What a primitive can answer.

use crate::error::PrimResult;
use crate::interp::{Interp, Oop};
use crate::proxy::sqInt;

/// Implemented by every type a `#[pharo_primitive]` body may return.
///
/// This is what lets a primitive be written as `-> PrimResult<f64>` instead of
/// hand-rolling a `methodReturnFloat` call.
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
