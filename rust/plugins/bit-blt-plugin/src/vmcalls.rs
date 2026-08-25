//! The interpreter-proxy calls the C plugin imports in `setInterpreter`.
//!
//! The safe [`Interp`] API deliberately reports failures as `Result`s, but
//! this plugin must mirror the C's control flow, which *continues* after a
//! failed accessor and consults `failed()` at the same points the generated
//! code does (a fetch that fails half-way through `loadBitBltFrom:warping:`
//! must not shortcut the load, or the failure code observed by the image
//! changes). So the engine talks to the proxy through these thin wrappers,
//! one per C import, each calling the raw table entry.
//!
//! Every wrapper treats a missing table entry the way the C treats a null
//! function pointer it never checks: the entries used here all predate proxy
//! 1.8 and are always present; if one ever were missing we answer the
//! neutral value (0 / false) rather than crash. `statNumGCs` and
//! `isPositiveMachineIntegerObject` answer 0 when absent by explicit design
//! in the C (`#define statNumGCs() 0` on old proxies).

use core::ffi::c_char;

use pharo_vm_plugin::{sqInt, Interp, PrimErr};

/// Calls a proxy entry, answering `$default` if the VM did not supply it.
macro_rules! vmcall {
    ($vm:expr, $field:ident ( $($arg:expr),* ), $default:expr) => {
        // SAFETY: the pointer came from the VM's own proxy table and the
        // signature is the published one; arguments are the caller's
        // responsibility exactly as in the C.
        match unsafe { (*$vm.as_raw()).$field } {
            Some(f) => unsafe { f($($arg),*) },
            None => $default,
        }
    };
}

pub fn methodArgumentCount(vm: &Interp) -> sqInt {
    vmcall!(vm, methodArgumentCount(), 0)
}

pub fn stackValue(vm: &Interp, offset: sqInt) -> sqInt {
    vmcall!(vm, stackValue(offset), 0)
}

pub fn stackIntegerValue(vm: &Interp, offset: sqInt) -> sqInt {
    vmcall!(vm, stackIntegerValue(offset), 0)
}

pub fn stackObjectValue(vm: &Interp, offset: sqInt) -> sqInt {
    vmcall!(vm, stackObjectValue(offset), 0)
}

pub fn failed(vm: &Interp) -> bool {
    vmcall!(vm, failed(), 0) != 0
}

pub fn primitiveFail(vm: &Interp) -> sqInt {
    vmcall!(vm, primitiveFail(), 0)
}

pub fn primitiveFailFor(vm: &Interp, reason: sqInt) -> sqInt {
    vmcall!(vm, primitiveFailFor(reason), 0)
}

/// The current failure code as a [`PrimErr`], for re-raising the C's exact
/// code through the SDK's `Err` path (`primitiveFailFor` with the same code
/// is idempotent).
pub fn currentFailure(vm: &Interp) -> PrimErr {
    let code = vmcall!(vm, primitiveFailureCode(), 1);
    match code {
        2 => PrimErr::BadReceiver,
        3 => PrimErr::BadArgument,
        4 => PrimErr::BadIndex,
        5 => PrimErr::BadNumArgs,
        6 => PrimErr::Inappropriate,
        7 => PrimErr::Unsupported,
        8 => PrimErr::NoModification,
        9 => PrimErr::NoMemory,
        10 => PrimErr::NoCMemory,
        11 => PrimErr::NotFound,
        12 => PrimErr::BadMethod,
        13 => PrimErr::NamedInternal,
        14 => PrimErr::ObjectMayMove,
        15 => PrimErr::LimitExceeded,
        16 => PrimErr::ObjectIsPinned,
        17 => PrimErr::WritePastObject,
        18 => PrimErr::ObjectMoved,
        19 => PrimErr::ObjectNotPinned,
        20 => PrimErr::CallbackError,
        21 => PrimErr::OSError,
        22 => PrimErr::FFIException,
        23 => PrimErr::NeedCompaction,
        24 => PrimErr::OperationFailed,
        _ => PrimErr::GenericFailure,
    }
}

pub fn fetchPointerofObject(vm: &Interp, index: sqInt, oop: sqInt) -> sqInt {
    vmcall!(vm, fetchPointerofObject(index, oop), 0)
}

pub fn fetchIntegerofObject(vm: &Interp, index: sqInt, oop: sqInt) -> sqInt {
    vmcall!(vm, fetchIntegerofObject(index, oop), 0)
}

pub fn fetchLong32ofObject(vm: &Interp, index: sqInt, oop: sqInt) -> sqInt {
    vmcall!(vm, fetchLong32ofObject(index, oop), 0)
}

/// `oopForPointer(firstIndexableField(oop))` in one step: the C stores the
/// pointer in an integer variable immediately.
pub fn firstIndexableFieldAddr(vm: &Interp, oop: sqInt) -> usize {
    vmcall!(vm, firstIndexableField(oop), core::ptr::null_mut()) as usize
}

pub fn floatValueOf(vm: &Interp, oop: sqInt) -> f64 {
    vmcall!(vm, floatValueOf(oop), 0.0)
}

pub fn integerObjectOf(vm: &Interp, value: sqInt) -> sqInt {
    vmcall!(vm, integerObjectOf(value), 0)
}

pub fn integerValueOf(vm: &Interp, oop: sqInt) -> sqInt {
    vmcall!(vm, integerValueOf(oop), 0)
}

pub fn isIntegerObject(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isIntegerObject(oop), 0) != 0
}

pub fn isArray(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isArray(oop), 0) != 0
}

pub fn isBytes(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isBytes(oop), 0) != 0
}

pub fn isPointers(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isPointers(oop), 0) != 0
}

pub fn isWords(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isWords(oop), 0) != 0
}

pub fn isWordsOrBytes(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isWordsOrBytes(oop), 0) != 0
}

pub fn isPositiveMachineIntegerObject(vm: &Interp, oop: sqInt) -> bool {
    vmcall!(vm, isPositiveMachineIntegerObject(oop), 0) != 0
}

pub fn byteSizeOf(vm: &Interp, oop: sqInt) -> sqInt {
    vmcall!(vm, byteSizeOf(oop), 0)
}

pub fn slotSizeOf(vm: &Interp, oop: sqInt) -> sqInt {
    vmcall!(vm, slotSizeOf(oop), 0)
}

pub fn nilObject(vm: &Interp) -> sqInt {
    vmcall!(vm, nilObject(), 0)
}

pub fn positive32BitIntegerFor(vm: &Interp, value: u32) -> sqInt {
    vmcall!(vm, positive32BitIntegerFor(value), 0)
}

pub fn positive32BitValueOf(vm: &Interp, oop: sqInt) -> u32 {
    vmcall!(vm, positive32BitValueOf(oop), 0)
}

// The cast is a no-op on LP64 but needed where c_ulong is 32-bit.
#[allow(clippy::unnecessary_cast)]
pub fn positive64BitValueOf(vm: &Interp, oop: sqInt) -> u64 {
    vmcall!(vm, positive64BitValueOf(oop), 0) as u64
}

pub fn statNumGCs(vm: &Interp) -> sqInt {
    vmcall!(vm, statNumGCs(), 0)
}

pub fn storeIntegerofObjectwithValue(vm: &Interp, index: sqInt, oop: sqInt, value: sqInt) -> sqInt {
    vmcall!(vm, storeIntegerofObjectwithValue(index, oop, value), 0)
}

/// `ioLoadFunctionFrom` -- resolve an entry point exported by another
/// module, answered as an address (0 when unresolved).
pub fn ioLoadFunctionFrom(vm: &Interp, fn_name: &'static [u8], module_name: &'static [u8]) -> usize {
    debug_assert!(fn_name.ends_with(b"\0") && module_name.ends_with(b"\0"));
    vmcall!(
        vm,
        ioLoadFunctionFrom(
            fn_name.as_ptr() as *mut c_char,
            module_name.as_ptr() as *mut c_char
        ),
        core::ptr::null_mut()
    ) as usize
}
