//! The interpreter proxy: the ABI every Pharo VM plugin talks to.
//!
//! This module is deliberately **header-free**. A plugin author should be able
//! to write a plugin with nothing but `cargo add pharo-vm-plugin` -- no Pharo
//! checkout, no CMake, no bindgen, no libclang. So the `VirtualMachine` struct
//! below is checked in rather than generated.
//!
//! That is the opposite of the choice `pharo-vm-sys` makes, and deliberately
//! so. `pharo-vm-sys` binds headers whose types vary with the local build
//! configuration, so generating them per build is the only safe option. Here
//! we are pinning a *published ABI* -- proxy version 1.15 -- which is exactly
//! the thing a separately-compiled plugin must agree with. Generating it from
//! whatever headers happen to be lying around would be less safe, not more.
//!
//! Drift is caught rather than assumed: `layout.rs` checks this struct
//! field-for-field against bindgen's view of the real header, and runs
//! whenever the tests are run inside a configured VM build tree.
//!
//! Provenance: extracted from `include/pharovm/common/virtualMachine.h`
//! (VM_PROXY_MAJOR 1, VM_PROXY_MINOR 15) via bindgen 0.69.

// The proxy mirrors a C struct, so its 153 fields keep their C names: they are
// the ABI, and renaming them would only obscure which entry is which when
// reading alongside virtualMachine.h. Documenting each one individually would
// add nothing over the header.
#![allow(non_snake_case)]
#![allow(missing_docs)]

use core::ffi;

/// The VM's object-pointer-sized signed integer -- an oop, or an immediate.
///
/// `sqInt` is `int`, `long` or `long long` in C depending on the build, but in
/// every configuration this VM ships it is the same width as a pointer, which
/// is what `isize` gives us on every target.
///
/// The one C configuration where that is *not* true is a 32-bit image on a
/// 64-bit host (`SQ_HOST64 && SQ_IMAGE32`, which routes object access through
/// `sqMemoryBase`). This build system does not produce it -- the image word
/// size follows the host pointer size -- and this crate does not support it.
#[allow(non_camel_case_types)]
pub type sqInt = isize;

/// Signed integer wide enough to hold a machine pointer.
#[allow(non_camel_case_types)]
pub type sqIntptr_t = isize;

/// Unsigned integer wide enough to hold a machine pointer.
#[allow(non_camel_case_types)]
pub type usqIntptr_t = usize;

/// Opaque platform semaphore. Plugins only ever hold pointers to these.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Semaphore {
    _private: [u8; 0],
}

/// Tag bits Spur spends on immediates, which is what narrows a SmallInteger
/// below the width of `sqInt`.
///
/// Three in a 64-bit image (SmallInteger, Character and SmallFloat all get a
/// tag pattern), one in a 32-bit image.
pub const NUM_SMALL_INTEGER_TAG_BITS: u32 = if core::mem::size_of::<sqInt>() == 8 {
    3
} else {
    1
};

/// Number of value bits in a SmallInteger, sign included.
const SMALL_INTEGER_BITS: u32 =
    (core::mem::size_of::<sqInt>() as u32) * 8 - NUM_SMALL_INTEGER_TAG_BITS;

/// Largest integer representable as a SmallInteger rather than a LargeInteger.
///
/// `1152921504606846975` in a 64-bit image; matches `MaxSmallInteger` in the
/// generated `interp.h`.
pub const MAX_SMALL_INTEGER: sqInt = (1 << (SMALL_INTEGER_BITS - 1)) - 1;

/// Smallest integer representable as a SmallInteger.
pub const MIN_SMALL_INTEGER: sqInt = -(1 << (SMALL_INTEGER_BITS - 1));

/// Proxy major version this crate is built against.
pub const VM_PROXY_MAJOR: sqInt = 1;
/// Proxy minor version this crate is built against.
pub const VM_PROXY_MINOR: sqInt = 15;

/// The function-pointer table the VM hands a plugin in `setInterpreter`.
///
/// Every field is `Option<fn>` because a null entry is representable: older or
/// differently-configured VMs may leave slots unset. Prefer the checked
/// accessors on [`crate::Interp`] over reaching in here.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct VirtualMachine {
    pub minorVersion: Option<unsafe extern "C" fn() -> sqInt>,
    pub majorVersion: Option<unsafe extern "C" fn() -> sqInt>,
    pub pop: Option<unsafe extern "C" fn(nItems: sqInt) -> sqInt>,
    pub popthenPush: Option<unsafe extern "C" fn(nItems: sqInt, oop: sqInt)>,
    pub push: Option<unsafe extern "C" fn(object: sqInt)>,
    pub pushBool: Option<unsafe extern "C" fn(trueOrFalse: sqInt) -> sqInt>,
    pub pushFloat: Option<unsafe extern "C" fn(f: f64)>,
    pub pushInteger: Option<unsafe extern "C" fn(integerValue: sqInt) -> sqInt>,
    pub stackFloatValue: Option<unsafe extern "C" fn(offset: sqInt) -> f64>,
    pub stackIntegerValue: Option<unsafe extern "C" fn(offset: sqInt) -> sqInt>,
    pub stackObjectValue: Option<unsafe extern "C" fn(offset: sqInt) -> sqInt>,
    pub stackValue: Option<unsafe extern "C" fn(offset: sqInt) -> sqInt>,
    pub argumentCountOf: Option<unsafe extern "C" fn(methodPointer: sqInt) -> sqInt>,
    pub arrayValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> *mut ffi::c_void>,
    pub byteSizeOf: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub fetchArrayofObject:
        Option<unsafe extern "C" fn(fieldIndex: sqInt, objectPointer: sqInt) -> *mut ffi::c_void>,
    pub fetchClassOf: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub fetchFloatofObject:
        Option<unsafe extern "C" fn(fieldIndex: sqInt, objectPointer: sqInt) -> f64>,
    pub fetchIntegerofObject:
        Option<unsafe extern "C" fn(fieldIndex: sqInt, objectPointer: sqInt) -> sqInt>,
    pub fetchPointerofObject: Option<unsafe extern "C" fn(fieldIndex: sqInt, oop: sqInt) -> sqInt>,
    pub obsoleteDontUseThisFetchWordofObject:
        Option<unsafe extern "C" fn(fieldFieldIndex: sqInt, oop: sqInt) -> sqInt>,
    pub firstFixedField: Option<unsafe extern "C" fn(oop: sqInt) -> *mut ffi::c_void>,
    pub firstIndexableField: Option<unsafe extern "C" fn(oop: sqInt) -> *mut ffi::c_void>,
    pub literalofMethod: Option<unsafe extern "C" fn(offset: sqInt, methodPointer: sqInt) -> sqInt>,
    pub literalCountOf: Option<unsafe extern "C" fn(methodPointer: sqInt) -> sqInt>,
    pub methodArgumentCount: Option<unsafe extern "C" fn() -> sqInt>,
    pub methodPrimitiveIndex: Option<unsafe extern "C" fn() -> sqInt>,
    pub primitiveIndexOf: Option<unsafe extern "C" fn(methodPointer: sqInt) -> sqInt>,
    pub sizeOfSTArrayFromCPrimitive: Option<unsafe extern "C" fn(cPtr: *mut ffi::c_void) -> sqInt>,
    pub slotSizeOf: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub stObjectat: Option<unsafe extern "C" fn(array: sqInt, fieldIndex: sqInt) -> sqInt>,
    pub stObjectatput:
        Option<unsafe extern "C" fn(array: sqInt, fieldIndex: sqInt, value: sqInt) -> sqInt>,
    pub stSizeOf: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub storeIntegerofObjectwithValue:
        Option<unsafe extern "C" fn(fieldIndex: sqInt, oop: sqInt, integer: sqInt) -> sqInt>,
    pub storePointerofObjectwithValue:
        Option<unsafe extern "C" fn(fieldIndex: sqInt, oop: sqInt, valuePointer: sqInt) -> sqInt>,
    pub isKindOf: Option<unsafe extern "C" fn(oop: sqInt, aString: *mut ffi::c_char) -> sqInt>,
    pub isMemberOf: Option<unsafe extern "C" fn(oop: sqInt, aString: *mut ffi::c_char) -> sqInt>,
    pub isBytes: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isFloatObject: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isIndexable: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isIntegerObject: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isIntegerValue: Option<unsafe extern "C" fn(intValue: sqInt) -> sqInt>,
    pub isPointers: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isWeak: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isWords: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isWordsOrBytes: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub booleanValueOf: Option<unsafe extern "C" fn(obj: sqInt) -> sqInt>,
    pub checkedIntegerValueOf: Option<unsafe extern "C" fn(intOop: sqInt) -> sqInt>,
    pub floatObjectOf: Option<unsafe extern "C" fn(aFloat: f64) -> sqInt>,
    pub floatValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> f64>,
    pub integerObjectOf: Option<unsafe extern "C" fn(value: sqInt) -> sqInt>,
    pub integerValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub positive32BitIntegerFor: Option<unsafe extern "C" fn(integerValue: ffi::c_uint) -> sqInt>,
    pub positive32BitValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> ffi::c_uint>,
    pub falseObject: Option<unsafe extern "C" fn() -> sqInt>,
    pub nilObject: Option<unsafe extern "C" fn() -> sqInt>,
    pub trueObject: Option<unsafe extern "C" fn() -> sqInt>,
    pub classArray: Option<unsafe extern "C" fn() -> sqInt>,
    pub classBitmap: Option<unsafe extern "C" fn() -> sqInt>,
    pub classByteArray: Option<unsafe extern "C" fn() -> sqInt>,
    pub classCharacter: Option<unsafe extern "C" fn() -> sqInt>,
    pub classFloat: Option<unsafe extern "C" fn() -> sqInt>,
    pub classLargePositiveInteger: Option<unsafe extern "C" fn() -> sqInt>,
    pub classPoint: Option<unsafe extern "C" fn() -> sqInt>,
    pub classSemaphore: Option<unsafe extern "C" fn() -> sqInt>,
    pub classSmallInteger: Option<unsafe extern "C" fn() -> sqInt>,
    pub classString: Option<unsafe extern "C" fn() -> sqInt>,
    pub clone: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub instantiateClassindexableSize:
        Option<unsafe extern "C" fn(classPointer: sqInt, size: sqInt) -> sqInt>,
    pub makePointwithxValueyValue:
        Option<unsafe extern "C" fn(xValue: sqInt, yValue: sqInt) -> sqInt>,
    pub popRemappableOop: Option<unsafe extern "C" fn() -> sqInt>,
    pub pushRemappableOop: Option<unsafe extern "C" fn(oop: sqInt)>,
    pub becomewith: Option<unsafe extern "C" fn(array1: sqInt, array2: sqInt) -> sqInt>,
    pub byteSwapped: Option<unsafe extern "C" fn(w: sqInt) -> sqInt>,
    pub failed: Option<unsafe extern "C" fn() -> sqInt>,
    pub fullGC: Option<unsafe extern "C" fn()>,
    pub primitiveFail: Option<unsafe extern "C" fn() -> sqInt>,
    pub showDisplayBitsLeftTopRightBottom:
        Option<unsafe extern "C" fn(aForm: sqInt, l: sqInt, t: sqInt, r: sqInt, b: sqInt) -> sqInt>,
    pub signalSemaphoreWithIndex: Option<unsafe extern "C" fn(semaIndex: sqInt) -> sqInt>,
    pub success: Option<unsafe extern "C" fn(aBoolean: sqInt) -> sqInt>,
    pub superclassOf: Option<unsafe extern "C" fn(classPointer: sqInt) -> sqInt>,
    pub statNumGCs: Option<unsafe extern "C" fn() -> sqInt>,
    pub stringForCString:
        Option<unsafe extern "C" fn(nullTerminatedCString: *const ffi::c_char) -> sqInt>,
    pub loadBitBltFrom: Option<unsafe extern "C" fn(bbOop: sqInt) -> sqInt>,
    pub copyBits: Option<unsafe extern "C" fn() -> sqInt>,
    pub copyBitsFromtoat:
        Option<unsafe extern "C" fn(leftX: sqInt, rightX: sqInt, yValue: sqInt) -> sqInt>,
    pub classLargeNegativeInteger: Option<unsafe extern "C" fn() -> sqInt>,
    pub signed32BitIntegerFor: Option<unsafe extern "C" fn(integerValue: sqInt) -> sqInt>,
    pub signed32BitValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> ffi::c_int>,
    pub includesBehaviorThatOf:
        Option<unsafe extern "C" fn(aClass: sqInt, aSuperClass: sqInt) -> sqInt>,
    pub primitiveMethod: Option<unsafe extern "C" fn() -> sqInt>,
    pub classExternalAddress: Option<unsafe extern "C" fn() -> sqInt>,
    pub ioLoadModuleOfLength:
        Option<unsafe extern "C" fn(modIndex: sqInt, modLength: sqInt) -> *mut ffi::c_void>,
    pub ioLoadSymbolOfLengthFromModule: Option<
        unsafe extern "C" fn(
            fnIndex: sqInt,
            fnLength: sqInt,
            handle: *mut ffi::c_void,
        ) -> *mut ffi::c_void,
    >,
    pub isInMemory: Option<unsafe extern "C" fn(address: sqInt) -> sqInt>,
    pub ioLoadFunctionFrom: Option<
        unsafe extern "C" fn(
            fnName: *mut ffi::c_char,
            modName: *mut ffi::c_char,
        ) -> *mut ffi::c_void,
    >,
    pub ioMicroMSecs: Option<unsafe extern "C" fn() -> sqInt>,
    pub positive64BitIntegerFor: Option<unsafe extern "C" fn(integerValue: ffi::c_ulong) -> sqInt>,
    pub positive64BitValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> ffi::c_ulong>,
    pub signed64BitIntegerFor: Option<unsafe extern "C" fn(integerValue: ffi::c_long) -> sqInt>,
    pub signed64BitValueOf: Option<unsafe extern "C" fn(oop: sqInt) -> ffi::c_long>,
    pub isArray: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub forceInterruptCheck: Option<unsafe extern "C" fn() -> sqInt>,
    pub fetchLong32ofObject:
        Option<unsafe extern "C" fn(fieldFieldIndex: sqInt, oop: sqInt) -> sqInt>,
    pub getThisSessionID: Option<unsafe extern "C" fn() -> sqInt>,
    pub ioFilenamefromStringofLengthresolveAliases: Option<
        unsafe extern "C" fn(
            aCharBuffer: *mut ffi::c_char,
            filenameIndex: *mut ffi::c_char,
            filenameLength: sqInt,
            resolveFlag: sqInt,
        ) -> sqInt,
    >,
    pub vmEndianness: Option<unsafe extern "C" fn() -> sqInt>,
    pub addGCRoot: Option<unsafe extern "C" fn(varLoc: *mut sqInt) -> sqInt>,
    pub removeGCRoot: Option<unsafe extern "C" fn(varLoc: *mut sqInt) -> sqInt>,
    pub primitiveFailFor: Option<unsafe extern "C" fn(code: sqInt) -> sqInt>,
    pub sendInvokeCallbackStackRegistersJmpbuf: Option<
        unsafe extern "C" fn(
            thunkPtrAsInt: sqInt,
            stackPtrAsInt: sqInt,
            regsPtrAsInt: sqInt,
            jmpBufPtrAsInt: sqInt,
        ) -> sqInt,
    >,
    pub reestablishContextPriorToCallback:
        Option<unsafe extern "C" fn(callbackContext: sqInt) -> sqInt>,
    pub isOopImmutable: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isOopMutable: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub methodReturnBool: Option<unsafe extern "C" fn(arg1: sqInt) -> sqInt>,
    pub methodReturnFloat: Option<unsafe extern "C" fn(arg1: f64) -> sqInt>,
    pub methodReturnInteger: Option<unsafe extern "C" fn(arg1: sqInt) -> sqInt>,
    pub methodReturnString: Option<unsafe extern "C" fn(arg1: *mut ffi::c_char) -> sqInt>,
    pub methodReturnValue: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub topRemappableOop: Option<unsafe extern "C" fn() -> sqInt>,
    pub addHighPriorityTickee:
        Option<unsafe extern "C" fn(ticker: Option<unsafe extern "C" fn()>, periodms: ffi::c_uint)>,
    pub addSynchronousTickee: Option<
        unsafe extern "C" fn(
            ticker: Option<unsafe extern "C" fn()>,
            periodms: ffi::c_uint,
            roundms: ffi::c_uint,
        ),
    >,
    pub utcMicroseconds: Option<unsafe extern "C" fn() -> ffi::c_ulonglong>,
    pub tenuringIncrementalGC: Option<unsafe extern "C" fn()>,
    pub isYoung: Option<unsafe extern "C" fn(anOop: sqInt) -> sqInt>,
    pub isKindOfClass: Option<unsafe extern "C" fn(oop: sqInt, aClass: sqInt) -> sqInt>,
    pub primitiveErrorTable: Option<unsafe extern "C" fn() -> sqInt>,
    pub primitiveFailureCode: Option<unsafe extern "C" fn() -> sqInt>,
    pub instanceSizeOf: Option<unsafe extern "C" fn(aClass: sqInt) -> sqInt>,
    pub signedMachineIntegerValueOf: Option<unsafe extern "C" fn(arg1: sqInt) -> sqIntptr_t>,
    pub stackSignedMachineIntegerValue: Option<unsafe extern "C" fn(arg1: sqInt) -> sqIntptr_t>,
    pub positiveMachineIntegerValueOf: Option<unsafe extern "C" fn(arg1: sqInt) -> usqIntptr_t>,
    pub stackPositiveMachineIntegerValue: Option<unsafe extern "C" fn(arg1: sqInt) -> usqIntptr_t>,
    pub cStringOrNullFor: Option<unsafe extern "C" fn(arg1: sqInt) -> *mut ffi::c_char>,
    pub signalNoResume: Option<unsafe extern "C" fn(arg1: sqInt) -> sqInt>,
    pub isImmediate: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub characterObjectOf: Option<unsafe extern "C" fn(charCode: sqInt) -> sqInt>,
    pub characterValueOf: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub isCharacterObject: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub isCharacterValue: Option<unsafe extern "C" fn(charCode: ffi::c_int) -> sqInt>,
    pub isPinned: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub pinObject: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub unpinObject: Option<unsafe extern "C" fn(objOop: sqInt) -> sqInt>,
    pub primitiveFailForOSError: Option<unsafe extern "C" fn(osErrorCode: ffi::c_long) -> sqInt>,
    pub methodReturnReceiver: Option<unsafe extern "C" fn() -> sqInt>,
    pub isBooleanObject: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub isPositiveMachineIntegerObject: Option<unsafe extern "C" fn(arg1: sqInt) -> sqInt>,
    pub ptEnterInterpreterFromCallback:
        Option<unsafe extern "C" fn(arg1: *mut ffi::c_void) -> sqInt>,
    pub ptExitInterpreterToCallback: Option<unsafe extern "C" fn(arg1: *mut ffi::c_void) -> sqInt>,
    pub isNonImmediate: Option<unsafe extern "C" fn(oop: sqInt) -> sqInt>,
    pub platformSemaphoreNew:
        Option<unsafe extern "C" fn(initialValue: ffi::c_int) -> *mut Semaphore>,
    pub scheduleInMainThread:
        Option<unsafe extern "C" fn(closure: Option<unsafe extern "C" fn() -> sqInt>) -> sqInt>,
    pub waitOnExternalSemaphoreIndex: Option<unsafe extern "C" fn(semaphoreIndex: sqInt)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pinned against the generated `interp.h`, which declares
    /// `MinSmallInteger -1152921504606846976`,
    /// `MaxSmallInteger 1152921504606846975` and
    /// `NumSmallIntegerTagBits 3` for a 64-bit image.
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn small_integer_bounds_match_interp_h() {
        assert_eq!(NUM_SMALL_INTEGER_TAG_BITS, 3);
        assert_eq!(MAX_SMALL_INTEGER, 1_152_921_504_606_846_975);
        assert_eq!(MIN_SMALL_INTEGER, -1_152_921_504_606_846_976);
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn small_integer_bounds_are_31_bit() {
        assert_eq!(NUM_SMALL_INTEGER_TAG_BITS, 1);
        assert_eq!(MAX_SMALL_INTEGER, 1_073_741_823);
        assert_eq!(MIN_SMALL_INTEGER, -1_073_741_824);
    }

    /// `sqInt` must be pointer-sized; the whole object representation depends
    /// on it. See the type's own documentation for the one C configuration
    /// this crate does not support.
    #[test]
    fn sqint_is_pointer_sized() {
        assert_eq!(
            core::mem::size_of::<sqInt>(),
            core::mem::size_of::<*const ffi::c_void>()
        );
    }
}
