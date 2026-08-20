//! Proves the checked-in proxy struct still matches the real headers.
//!
//! `pharo-vm-plugin` deliberately checks in its `VirtualMachine` definition so
//! that plugin authors need no Pharo checkout (see `proxy.rs`). The price of
//! that choice is drift: if `virtualMachine.h` gains, loses or reorders a
//! field, the checked-in copy silently disagrees with the VM, and every plugin
//! built against it calls through the wrong slot.
//!
//! This test buys the safety back. It compares our struct against
//! `pharo-vm-sys`, which generates its view from the actual header at build
//! time, field by field.
//!
//! It needs the VM's headers, so it sits behind the `verify-abi` feature and
//! runs only inside a configured build tree:
//!
//! ```sh
//! source build/rust-env.sh
//! cargo test -p pharo-vm-plugin --features verify-abi
//! ```
#![cfg(feature = "verify-abi")]

use core::mem::{align_of, offset_of, size_of};

use pharo_vm_plugin::proxy as ours;
use pharo_vm_sys as theirs;

fn check(field: &str, ours: usize, theirs: usize, mismatches: &mut Vec<String>) {
    if ours != theirs {
        mismatches.push(format!(
            "  {field}: checked-in offset {ours}, header says {theirs}"
        ));
    }
}

#[test]
fn proxy_struct_matches_the_real_header() {
    assert_eq!(
        size_of::<ours::VirtualMachine>(),
        size_of::<theirs::VirtualMachine>(),
        "struct VirtualMachine changed size; regenerate pharo-vm-plugin/src/proxy.rs"
    );
    assert_eq!(
        align_of::<ours::VirtualMachine>(),
        align_of::<theirs::VirtualMachine>(),
        "struct VirtualMachine changed alignment"
    );

    let mut mismatches = Vec::new();
    check(
        "minorVersion",
        offset_of!(ours::VirtualMachine, minorVersion),
        offset_of!(theirs::VirtualMachine, minorVersion),
        &mut mismatches,
    );
    check(
        "majorVersion",
        offset_of!(ours::VirtualMachine, majorVersion),
        offset_of!(theirs::VirtualMachine, majorVersion),
        &mut mismatches,
    );
    check(
        "pop",
        offset_of!(ours::VirtualMachine, pop),
        offset_of!(theirs::VirtualMachine, pop),
        &mut mismatches,
    );
    check(
        "popthenPush",
        offset_of!(ours::VirtualMachine, popthenPush),
        offset_of!(theirs::VirtualMachine, popthenPush),
        &mut mismatches,
    );
    check(
        "push",
        offset_of!(ours::VirtualMachine, push),
        offset_of!(theirs::VirtualMachine, push),
        &mut mismatches,
    );
    check(
        "pushBool",
        offset_of!(ours::VirtualMachine, pushBool),
        offset_of!(theirs::VirtualMachine, pushBool),
        &mut mismatches,
    );
    check(
        "pushFloat",
        offset_of!(ours::VirtualMachine, pushFloat),
        offset_of!(theirs::VirtualMachine, pushFloat),
        &mut mismatches,
    );
    check(
        "pushInteger",
        offset_of!(ours::VirtualMachine, pushInteger),
        offset_of!(theirs::VirtualMachine, pushInteger),
        &mut mismatches,
    );
    check(
        "stackFloatValue",
        offset_of!(ours::VirtualMachine, stackFloatValue),
        offset_of!(theirs::VirtualMachine, stackFloatValue),
        &mut mismatches,
    );
    check(
        "stackIntegerValue",
        offset_of!(ours::VirtualMachine, stackIntegerValue),
        offset_of!(theirs::VirtualMachine, stackIntegerValue),
        &mut mismatches,
    );
    check(
        "stackObjectValue",
        offset_of!(ours::VirtualMachine, stackObjectValue),
        offset_of!(theirs::VirtualMachine, stackObjectValue),
        &mut mismatches,
    );
    check(
        "stackValue",
        offset_of!(ours::VirtualMachine, stackValue),
        offset_of!(theirs::VirtualMachine, stackValue),
        &mut mismatches,
    );
    check(
        "argumentCountOf",
        offset_of!(ours::VirtualMachine, argumentCountOf),
        offset_of!(theirs::VirtualMachine, argumentCountOf),
        &mut mismatches,
    );
    check(
        "arrayValueOf",
        offset_of!(ours::VirtualMachine, arrayValueOf),
        offset_of!(theirs::VirtualMachine, arrayValueOf),
        &mut mismatches,
    );
    check(
        "byteSizeOf",
        offset_of!(ours::VirtualMachine, byteSizeOf),
        offset_of!(theirs::VirtualMachine, byteSizeOf),
        &mut mismatches,
    );
    check(
        "fetchArrayofObject",
        offset_of!(ours::VirtualMachine, fetchArrayofObject),
        offset_of!(theirs::VirtualMachine, fetchArrayofObject),
        &mut mismatches,
    );
    check(
        "fetchClassOf",
        offset_of!(ours::VirtualMachine, fetchClassOf),
        offset_of!(theirs::VirtualMachine, fetchClassOf),
        &mut mismatches,
    );
    check(
        "fetchFloatofObject",
        offset_of!(ours::VirtualMachine, fetchFloatofObject),
        offset_of!(theirs::VirtualMachine, fetchFloatofObject),
        &mut mismatches,
    );
    check(
        "fetchIntegerofObject",
        offset_of!(ours::VirtualMachine, fetchIntegerofObject),
        offset_of!(theirs::VirtualMachine, fetchIntegerofObject),
        &mut mismatches,
    );
    check(
        "fetchPointerofObject",
        offset_of!(ours::VirtualMachine, fetchPointerofObject),
        offset_of!(theirs::VirtualMachine, fetchPointerofObject),
        &mut mismatches,
    );
    check(
        "obsoleteDontUseThisFetchWordofObject",
        offset_of!(ours::VirtualMachine, obsoleteDontUseThisFetchWordofObject),
        offset_of!(theirs::VirtualMachine, obsoleteDontUseThisFetchWordofObject),
        &mut mismatches,
    );
    check(
        "firstFixedField",
        offset_of!(ours::VirtualMachine, firstFixedField),
        offset_of!(theirs::VirtualMachine, firstFixedField),
        &mut mismatches,
    );
    check(
        "firstIndexableField",
        offset_of!(ours::VirtualMachine, firstIndexableField),
        offset_of!(theirs::VirtualMachine, firstIndexableField),
        &mut mismatches,
    );
    check(
        "literalofMethod",
        offset_of!(ours::VirtualMachine, literalofMethod),
        offset_of!(theirs::VirtualMachine, literalofMethod),
        &mut mismatches,
    );
    check(
        "literalCountOf",
        offset_of!(ours::VirtualMachine, literalCountOf),
        offset_of!(theirs::VirtualMachine, literalCountOf),
        &mut mismatches,
    );
    check(
        "methodArgumentCount",
        offset_of!(ours::VirtualMachine, methodArgumentCount),
        offset_of!(theirs::VirtualMachine, methodArgumentCount),
        &mut mismatches,
    );
    check(
        "methodPrimitiveIndex",
        offset_of!(ours::VirtualMachine, methodPrimitiveIndex),
        offset_of!(theirs::VirtualMachine, methodPrimitiveIndex),
        &mut mismatches,
    );
    check(
        "primitiveIndexOf",
        offset_of!(ours::VirtualMachine, primitiveIndexOf),
        offset_of!(theirs::VirtualMachine, primitiveIndexOf),
        &mut mismatches,
    );
    check(
        "sizeOfSTArrayFromCPrimitive",
        offset_of!(ours::VirtualMachine, sizeOfSTArrayFromCPrimitive),
        offset_of!(theirs::VirtualMachine, sizeOfSTArrayFromCPrimitive),
        &mut mismatches,
    );
    check(
        "slotSizeOf",
        offset_of!(ours::VirtualMachine, slotSizeOf),
        offset_of!(theirs::VirtualMachine, slotSizeOf),
        &mut mismatches,
    );
    check(
        "stObjectat",
        offset_of!(ours::VirtualMachine, stObjectat),
        offset_of!(theirs::VirtualMachine, stObjectat),
        &mut mismatches,
    );
    check(
        "stObjectatput",
        offset_of!(ours::VirtualMachine, stObjectatput),
        offset_of!(theirs::VirtualMachine, stObjectatput),
        &mut mismatches,
    );
    check(
        "stSizeOf",
        offset_of!(ours::VirtualMachine, stSizeOf),
        offset_of!(theirs::VirtualMachine, stSizeOf),
        &mut mismatches,
    );
    check(
        "storeIntegerofObjectwithValue",
        offset_of!(ours::VirtualMachine, storeIntegerofObjectwithValue),
        offset_of!(theirs::VirtualMachine, storeIntegerofObjectwithValue),
        &mut mismatches,
    );
    check(
        "storePointerofObjectwithValue",
        offset_of!(ours::VirtualMachine, storePointerofObjectwithValue),
        offset_of!(theirs::VirtualMachine, storePointerofObjectwithValue),
        &mut mismatches,
    );
    check(
        "isKindOf",
        offset_of!(ours::VirtualMachine, isKindOf),
        offset_of!(theirs::VirtualMachine, isKindOf),
        &mut mismatches,
    );
    check(
        "isMemberOf",
        offset_of!(ours::VirtualMachine, isMemberOf),
        offset_of!(theirs::VirtualMachine, isMemberOf),
        &mut mismatches,
    );
    check(
        "isBytes",
        offset_of!(ours::VirtualMachine, isBytes),
        offset_of!(theirs::VirtualMachine, isBytes),
        &mut mismatches,
    );
    check(
        "isFloatObject",
        offset_of!(ours::VirtualMachine, isFloatObject),
        offset_of!(theirs::VirtualMachine, isFloatObject),
        &mut mismatches,
    );
    check(
        "isIndexable",
        offset_of!(ours::VirtualMachine, isIndexable),
        offset_of!(theirs::VirtualMachine, isIndexable),
        &mut mismatches,
    );
    check(
        "isIntegerObject",
        offset_of!(ours::VirtualMachine, isIntegerObject),
        offset_of!(theirs::VirtualMachine, isIntegerObject),
        &mut mismatches,
    );
    check(
        "isIntegerValue",
        offset_of!(ours::VirtualMachine, isIntegerValue),
        offset_of!(theirs::VirtualMachine, isIntegerValue),
        &mut mismatches,
    );
    check(
        "isPointers",
        offset_of!(ours::VirtualMachine, isPointers),
        offset_of!(theirs::VirtualMachine, isPointers),
        &mut mismatches,
    );
    check(
        "isWeak",
        offset_of!(ours::VirtualMachine, isWeak),
        offset_of!(theirs::VirtualMachine, isWeak),
        &mut mismatches,
    );
    check(
        "isWords",
        offset_of!(ours::VirtualMachine, isWords),
        offset_of!(theirs::VirtualMachine, isWords),
        &mut mismatches,
    );
    check(
        "isWordsOrBytes",
        offset_of!(ours::VirtualMachine, isWordsOrBytes),
        offset_of!(theirs::VirtualMachine, isWordsOrBytes),
        &mut mismatches,
    );
    check(
        "booleanValueOf",
        offset_of!(ours::VirtualMachine, booleanValueOf),
        offset_of!(theirs::VirtualMachine, booleanValueOf),
        &mut mismatches,
    );
    check(
        "checkedIntegerValueOf",
        offset_of!(ours::VirtualMachine, checkedIntegerValueOf),
        offset_of!(theirs::VirtualMachine, checkedIntegerValueOf),
        &mut mismatches,
    );
    check(
        "floatObjectOf",
        offset_of!(ours::VirtualMachine, floatObjectOf),
        offset_of!(theirs::VirtualMachine, floatObjectOf),
        &mut mismatches,
    );
    check(
        "floatValueOf",
        offset_of!(ours::VirtualMachine, floatValueOf),
        offset_of!(theirs::VirtualMachine, floatValueOf),
        &mut mismatches,
    );
    check(
        "integerObjectOf",
        offset_of!(ours::VirtualMachine, integerObjectOf),
        offset_of!(theirs::VirtualMachine, integerObjectOf),
        &mut mismatches,
    );
    check(
        "integerValueOf",
        offset_of!(ours::VirtualMachine, integerValueOf),
        offset_of!(theirs::VirtualMachine, integerValueOf),
        &mut mismatches,
    );
    check(
        "positive32BitIntegerFor",
        offset_of!(ours::VirtualMachine, positive32BitIntegerFor),
        offset_of!(theirs::VirtualMachine, positive32BitIntegerFor),
        &mut mismatches,
    );
    check(
        "positive32BitValueOf",
        offset_of!(ours::VirtualMachine, positive32BitValueOf),
        offset_of!(theirs::VirtualMachine, positive32BitValueOf),
        &mut mismatches,
    );
    check(
        "falseObject",
        offset_of!(ours::VirtualMachine, falseObject),
        offset_of!(theirs::VirtualMachine, falseObject),
        &mut mismatches,
    );
    check(
        "nilObject",
        offset_of!(ours::VirtualMachine, nilObject),
        offset_of!(theirs::VirtualMachine, nilObject),
        &mut mismatches,
    );
    check(
        "trueObject",
        offset_of!(ours::VirtualMachine, trueObject),
        offset_of!(theirs::VirtualMachine, trueObject),
        &mut mismatches,
    );
    check(
        "classArray",
        offset_of!(ours::VirtualMachine, classArray),
        offset_of!(theirs::VirtualMachine, classArray),
        &mut mismatches,
    );
    check(
        "classBitmap",
        offset_of!(ours::VirtualMachine, classBitmap),
        offset_of!(theirs::VirtualMachine, classBitmap),
        &mut mismatches,
    );
    check(
        "classByteArray",
        offset_of!(ours::VirtualMachine, classByteArray),
        offset_of!(theirs::VirtualMachine, classByteArray),
        &mut mismatches,
    );
    check(
        "classCharacter",
        offset_of!(ours::VirtualMachine, classCharacter),
        offset_of!(theirs::VirtualMachine, classCharacter),
        &mut mismatches,
    );
    check(
        "classFloat",
        offset_of!(ours::VirtualMachine, classFloat),
        offset_of!(theirs::VirtualMachine, classFloat),
        &mut mismatches,
    );
    check(
        "classLargePositiveInteger",
        offset_of!(ours::VirtualMachine, classLargePositiveInteger),
        offset_of!(theirs::VirtualMachine, classLargePositiveInteger),
        &mut mismatches,
    );
    check(
        "classPoint",
        offset_of!(ours::VirtualMachine, classPoint),
        offset_of!(theirs::VirtualMachine, classPoint),
        &mut mismatches,
    );
    check(
        "classSemaphore",
        offset_of!(ours::VirtualMachine, classSemaphore),
        offset_of!(theirs::VirtualMachine, classSemaphore),
        &mut mismatches,
    );
    check(
        "classSmallInteger",
        offset_of!(ours::VirtualMachine, classSmallInteger),
        offset_of!(theirs::VirtualMachine, classSmallInteger),
        &mut mismatches,
    );
    check(
        "classString",
        offset_of!(ours::VirtualMachine, classString),
        offset_of!(theirs::VirtualMachine, classString),
        &mut mismatches,
    );
    check(
        "clone",
        offset_of!(ours::VirtualMachine, clone),
        offset_of!(theirs::VirtualMachine, clone),
        &mut mismatches,
    );
    check(
        "instantiateClassindexableSize",
        offset_of!(ours::VirtualMachine, instantiateClassindexableSize),
        offset_of!(theirs::VirtualMachine, instantiateClassindexableSize),
        &mut mismatches,
    );
    check(
        "makePointwithxValueyValue",
        offset_of!(ours::VirtualMachine, makePointwithxValueyValue),
        offset_of!(theirs::VirtualMachine, makePointwithxValueyValue),
        &mut mismatches,
    );
    check(
        "popRemappableOop",
        offset_of!(ours::VirtualMachine, popRemappableOop),
        offset_of!(theirs::VirtualMachine, popRemappableOop),
        &mut mismatches,
    );
    check(
        "pushRemappableOop",
        offset_of!(ours::VirtualMachine, pushRemappableOop),
        offset_of!(theirs::VirtualMachine, pushRemappableOop),
        &mut mismatches,
    );
    check(
        "becomewith",
        offset_of!(ours::VirtualMachine, becomewith),
        offset_of!(theirs::VirtualMachine, becomewith),
        &mut mismatches,
    );
    check(
        "byteSwapped",
        offset_of!(ours::VirtualMachine, byteSwapped),
        offset_of!(theirs::VirtualMachine, byteSwapped),
        &mut mismatches,
    );
    check(
        "failed",
        offset_of!(ours::VirtualMachine, failed),
        offset_of!(theirs::VirtualMachine, failed),
        &mut mismatches,
    );
    check(
        "fullGC",
        offset_of!(ours::VirtualMachine, fullGC),
        offset_of!(theirs::VirtualMachine, fullGC),
        &mut mismatches,
    );
    check(
        "primitiveFail",
        offset_of!(ours::VirtualMachine, primitiveFail),
        offset_of!(theirs::VirtualMachine, primitiveFail),
        &mut mismatches,
    );
    check(
        "showDisplayBitsLeftTopRightBottom",
        offset_of!(ours::VirtualMachine, showDisplayBitsLeftTopRightBottom),
        offset_of!(theirs::VirtualMachine, showDisplayBitsLeftTopRightBottom),
        &mut mismatches,
    );
    check(
        "signalSemaphoreWithIndex",
        offset_of!(ours::VirtualMachine, signalSemaphoreWithIndex),
        offset_of!(theirs::VirtualMachine, signalSemaphoreWithIndex),
        &mut mismatches,
    );
    check(
        "success",
        offset_of!(ours::VirtualMachine, success),
        offset_of!(theirs::VirtualMachine, success),
        &mut mismatches,
    );
    check(
        "superclassOf",
        offset_of!(ours::VirtualMachine, superclassOf),
        offset_of!(theirs::VirtualMachine, superclassOf),
        &mut mismatches,
    );
    check(
        "statNumGCs",
        offset_of!(ours::VirtualMachine, statNumGCs),
        offset_of!(theirs::VirtualMachine, statNumGCs),
        &mut mismatches,
    );
    check(
        "stringForCString",
        offset_of!(ours::VirtualMachine, stringForCString),
        offset_of!(theirs::VirtualMachine, stringForCString),
        &mut mismatches,
    );
    check(
        "loadBitBltFrom",
        offset_of!(ours::VirtualMachine, loadBitBltFrom),
        offset_of!(theirs::VirtualMachine, loadBitBltFrom),
        &mut mismatches,
    );
    check(
        "copyBits",
        offset_of!(ours::VirtualMachine, copyBits),
        offset_of!(theirs::VirtualMachine, copyBits),
        &mut mismatches,
    );
    check(
        "copyBitsFromtoat",
        offset_of!(ours::VirtualMachine, copyBitsFromtoat),
        offset_of!(theirs::VirtualMachine, copyBitsFromtoat),
        &mut mismatches,
    );
    check(
        "classLargeNegativeInteger",
        offset_of!(ours::VirtualMachine, classLargeNegativeInteger),
        offset_of!(theirs::VirtualMachine, classLargeNegativeInteger),
        &mut mismatches,
    );
    check(
        "signed32BitIntegerFor",
        offset_of!(ours::VirtualMachine, signed32BitIntegerFor),
        offset_of!(theirs::VirtualMachine, signed32BitIntegerFor),
        &mut mismatches,
    );
    check(
        "signed32BitValueOf",
        offset_of!(ours::VirtualMachine, signed32BitValueOf),
        offset_of!(theirs::VirtualMachine, signed32BitValueOf),
        &mut mismatches,
    );
    check(
        "includesBehaviorThatOf",
        offset_of!(ours::VirtualMachine, includesBehaviorThatOf),
        offset_of!(theirs::VirtualMachine, includesBehaviorThatOf),
        &mut mismatches,
    );
    check(
        "primitiveMethod",
        offset_of!(ours::VirtualMachine, primitiveMethod),
        offset_of!(theirs::VirtualMachine, primitiveMethod),
        &mut mismatches,
    );
    check(
        "classExternalAddress",
        offset_of!(ours::VirtualMachine, classExternalAddress),
        offset_of!(theirs::VirtualMachine, classExternalAddress),
        &mut mismatches,
    );
    check(
        "ioLoadModuleOfLength",
        offset_of!(ours::VirtualMachine, ioLoadModuleOfLength),
        offset_of!(theirs::VirtualMachine, ioLoadModuleOfLength),
        &mut mismatches,
    );
    check(
        "ioLoadSymbolOfLengthFromModule",
        offset_of!(ours::VirtualMachine, ioLoadSymbolOfLengthFromModule),
        offset_of!(theirs::VirtualMachine, ioLoadSymbolOfLengthFromModule),
        &mut mismatches,
    );
    check(
        "isInMemory",
        offset_of!(ours::VirtualMachine, isInMemory),
        offset_of!(theirs::VirtualMachine, isInMemory),
        &mut mismatches,
    );
    check(
        "ioLoadFunctionFrom",
        offset_of!(ours::VirtualMachine, ioLoadFunctionFrom),
        offset_of!(theirs::VirtualMachine, ioLoadFunctionFrom),
        &mut mismatches,
    );
    check(
        "ioMicroMSecs",
        offset_of!(ours::VirtualMachine, ioMicroMSecs),
        offset_of!(theirs::VirtualMachine, ioMicroMSecs),
        &mut mismatches,
    );
    check(
        "positive64BitIntegerFor",
        offset_of!(ours::VirtualMachine, positive64BitIntegerFor),
        offset_of!(theirs::VirtualMachine, positive64BitIntegerFor),
        &mut mismatches,
    );
    check(
        "positive64BitValueOf",
        offset_of!(ours::VirtualMachine, positive64BitValueOf),
        offset_of!(theirs::VirtualMachine, positive64BitValueOf),
        &mut mismatches,
    );
    check(
        "signed64BitIntegerFor",
        offset_of!(ours::VirtualMachine, signed64BitIntegerFor),
        offset_of!(theirs::VirtualMachine, signed64BitIntegerFor),
        &mut mismatches,
    );
    check(
        "signed64BitValueOf",
        offset_of!(ours::VirtualMachine, signed64BitValueOf),
        offset_of!(theirs::VirtualMachine, signed64BitValueOf),
        &mut mismatches,
    );
    check(
        "isArray",
        offset_of!(ours::VirtualMachine, isArray),
        offset_of!(theirs::VirtualMachine, isArray),
        &mut mismatches,
    );
    check(
        "forceInterruptCheck",
        offset_of!(ours::VirtualMachine, forceInterruptCheck),
        offset_of!(theirs::VirtualMachine, forceInterruptCheck),
        &mut mismatches,
    );
    check(
        "fetchLong32ofObject",
        offset_of!(ours::VirtualMachine, fetchLong32ofObject),
        offset_of!(theirs::VirtualMachine, fetchLong32ofObject),
        &mut mismatches,
    );
    check(
        "getThisSessionID",
        offset_of!(ours::VirtualMachine, getThisSessionID),
        offset_of!(theirs::VirtualMachine, getThisSessionID),
        &mut mismatches,
    );
    check(
        "ioFilenamefromStringofLengthresolveAliases",
        offset_of!(
            ours::VirtualMachine,
            ioFilenamefromStringofLengthresolveAliases
        ),
        offset_of!(
            theirs::VirtualMachine,
            ioFilenamefromStringofLengthresolveAliases
        ),
        &mut mismatches,
    );
    check(
        "vmEndianness",
        offset_of!(ours::VirtualMachine, vmEndianness),
        offset_of!(theirs::VirtualMachine, vmEndianness),
        &mut mismatches,
    );
    check(
        "addGCRoot",
        offset_of!(ours::VirtualMachine, addGCRoot),
        offset_of!(theirs::VirtualMachine, addGCRoot),
        &mut mismatches,
    );
    check(
        "removeGCRoot",
        offset_of!(ours::VirtualMachine, removeGCRoot),
        offset_of!(theirs::VirtualMachine, removeGCRoot),
        &mut mismatches,
    );
    check(
        "primitiveFailFor",
        offset_of!(ours::VirtualMachine, primitiveFailFor),
        offset_of!(theirs::VirtualMachine, primitiveFailFor),
        &mut mismatches,
    );
    check(
        "sendInvokeCallbackStackRegistersJmpbuf",
        offset_of!(ours::VirtualMachine, sendInvokeCallbackStackRegistersJmpbuf),
        offset_of!(
            theirs::VirtualMachine,
            sendInvokeCallbackStackRegistersJmpbuf
        ),
        &mut mismatches,
    );
    check(
        "reestablishContextPriorToCallback",
        offset_of!(ours::VirtualMachine, reestablishContextPriorToCallback),
        offset_of!(theirs::VirtualMachine, reestablishContextPriorToCallback),
        &mut mismatches,
    );
    check(
        "isOopImmutable",
        offset_of!(ours::VirtualMachine, isOopImmutable),
        offset_of!(theirs::VirtualMachine, isOopImmutable),
        &mut mismatches,
    );
    check(
        "isOopMutable",
        offset_of!(ours::VirtualMachine, isOopMutable),
        offset_of!(theirs::VirtualMachine, isOopMutable),
        &mut mismatches,
    );
    check(
        "methodReturnBool",
        offset_of!(ours::VirtualMachine, methodReturnBool),
        offset_of!(theirs::VirtualMachine, methodReturnBool),
        &mut mismatches,
    );
    check(
        "methodReturnFloat",
        offset_of!(ours::VirtualMachine, methodReturnFloat),
        offset_of!(theirs::VirtualMachine, methodReturnFloat),
        &mut mismatches,
    );
    check(
        "methodReturnInteger",
        offset_of!(ours::VirtualMachine, methodReturnInteger),
        offset_of!(theirs::VirtualMachine, methodReturnInteger),
        &mut mismatches,
    );
    check(
        "methodReturnString",
        offset_of!(ours::VirtualMachine, methodReturnString),
        offset_of!(theirs::VirtualMachine, methodReturnString),
        &mut mismatches,
    );
    check(
        "methodReturnValue",
        offset_of!(ours::VirtualMachine, methodReturnValue),
        offset_of!(theirs::VirtualMachine, methodReturnValue),
        &mut mismatches,
    );
    check(
        "topRemappableOop",
        offset_of!(ours::VirtualMachine, topRemappableOop),
        offset_of!(theirs::VirtualMachine, topRemappableOop),
        &mut mismatches,
    );
    check(
        "addHighPriorityTickee",
        offset_of!(ours::VirtualMachine, addHighPriorityTickee),
        offset_of!(theirs::VirtualMachine, addHighPriorityTickee),
        &mut mismatches,
    );
    check(
        "addSynchronousTickee",
        offset_of!(ours::VirtualMachine, addSynchronousTickee),
        offset_of!(theirs::VirtualMachine, addSynchronousTickee),
        &mut mismatches,
    );
    check(
        "utcMicroseconds",
        offset_of!(ours::VirtualMachine, utcMicroseconds),
        offset_of!(theirs::VirtualMachine, utcMicroseconds),
        &mut mismatches,
    );
    check(
        "tenuringIncrementalGC",
        offset_of!(ours::VirtualMachine, tenuringIncrementalGC),
        offset_of!(theirs::VirtualMachine, tenuringIncrementalGC),
        &mut mismatches,
    );
    check(
        "isYoung",
        offset_of!(ours::VirtualMachine, isYoung),
        offset_of!(theirs::VirtualMachine, isYoung),
        &mut mismatches,
    );
    check(
        "isKindOfClass",
        offset_of!(ours::VirtualMachine, isKindOfClass),
        offset_of!(theirs::VirtualMachine, isKindOfClass),
        &mut mismatches,
    );
    check(
        "primitiveErrorTable",
        offset_of!(ours::VirtualMachine, primitiveErrorTable),
        offset_of!(theirs::VirtualMachine, primitiveErrorTable),
        &mut mismatches,
    );
    check(
        "primitiveFailureCode",
        offset_of!(ours::VirtualMachine, primitiveFailureCode),
        offset_of!(theirs::VirtualMachine, primitiveFailureCode),
        &mut mismatches,
    );
    check(
        "instanceSizeOf",
        offset_of!(ours::VirtualMachine, instanceSizeOf),
        offset_of!(theirs::VirtualMachine, instanceSizeOf),
        &mut mismatches,
    );
    check(
        "signedMachineIntegerValueOf",
        offset_of!(ours::VirtualMachine, signedMachineIntegerValueOf),
        offset_of!(theirs::VirtualMachine, signedMachineIntegerValueOf),
        &mut mismatches,
    );
    check(
        "stackSignedMachineIntegerValue",
        offset_of!(ours::VirtualMachine, stackSignedMachineIntegerValue),
        offset_of!(theirs::VirtualMachine, stackSignedMachineIntegerValue),
        &mut mismatches,
    );
    check(
        "positiveMachineIntegerValueOf",
        offset_of!(ours::VirtualMachine, positiveMachineIntegerValueOf),
        offset_of!(theirs::VirtualMachine, positiveMachineIntegerValueOf),
        &mut mismatches,
    );
    check(
        "stackPositiveMachineIntegerValue",
        offset_of!(ours::VirtualMachine, stackPositiveMachineIntegerValue),
        offset_of!(theirs::VirtualMachine, stackPositiveMachineIntegerValue),
        &mut mismatches,
    );
    check(
        "cStringOrNullFor",
        offset_of!(ours::VirtualMachine, cStringOrNullFor),
        offset_of!(theirs::VirtualMachine, cStringOrNullFor),
        &mut mismatches,
    );
    check(
        "signalNoResume",
        offset_of!(ours::VirtualMachine, signalNoResume),
        offset_of!(theirs::VirtualMachine, signalNoResume),
        &mut mismatches,
    );
    check(
        "isImmediate",
        offset_of!(ours::VirtualMachine, isImmediate),
        offset_of!(theirs::VirtualMachine, isImmediate),
        &mut mismatches,
    );
    check(
        "characterObjectOf",
        offset_of!(ours::VirtualMachine, characterObjectOf),
        offset_of!(theirs::VirtualMachine, characterObjectOf),
        &mut mismatches,
    );
    check(
        "characterValueOf",
        offset_of!(ours::VirtualMachine, characterValueOf),
        offset_of!(theirs::VirtualMachine, characterValueOf),
        &mut mismatches,
    );
    check(
        "isCharacterObject",
        offset_of!(ours::VirtualMachine, isCharacterObject),
        offset_of!(theirs::VirtualMachine, isCharacterObject),
        &mut mismatches,
    );
    check(
        "isCharacterValue",
        offset_of!(ours::VirtualMachine, isCharacterValue),
        offset_of!(theirs::VirtualMachine, isCharacterValue),
        &mut mismatches,
    );
    check(
        "isPinned",
        offset_of!(ours::VirtualMachine, isPinned),
        offset_of!(theirs::VirtualMachine, isPinned),
        &mut mismatches,
    );
    check(
        "pinObject",
        offset_of!(ours::VirtualMachine, pinObject),
        offset_of!(theirs::VirtualMachine, pinObject),
        &mut mismatches,
    );
    check(
        "unpinObject",
        offset_of!(ours::VirtualMachine, unpinObject),
        offset_of!(theirs::VirtualMachine, unpinObject),
        &mut mismatches,
    );
    check(
        "primitiveFailForOSError",
        offset_of!(ours::VirtualMachine, primitiveFailForOSError),
        offset_of!(theirs::VirtualMachine, primitiveFailForOSError),
        &mut mismatches,
    );
    check(
        "methodReturnReceiver",
        offset_of!(ours::VirtualMachine, methodReturnReceiver),
        offset_of!(theirs::VirtualMachine, methodReturnReceiver),
        &mut mismatches,
    );
    check(
        "isBooleanObject",
        offset_of!(ours::VirtualMachine, isBooleanObject),
        offset_of!(theirs::VirtualMachine, isBooleanObject),
        &mut mismatches,
    );
    check(
        "isPositiveMachineIntegerObject",
        offset_of!(ours::VirtualMachine, isPositiveMachineIntegerObject),
        offset_of!(theirs::VirtualMachine, isPositiveMachineIntegerObject),
        &mut mismatches,
    );
    check(
        "ptEnterInterpreterFromCallback",
        offset_of!(ours::VirtualMachine, ptEnterInterpreterFromCallback),
        offset_of!(theirs::VirtualMachine, ptEnterInterpreterFromCallback),
        &mut mismatches,
    );
    check(
        "ptExitInterpreterToCallback",
        offset_of!(ours::VirtualMachine, ptExitInterpreterToCallback),
        offset_of!(theirs::VirtualMachine, ptExitInterpreterToCallback),
        &mut mismatches,
    );
    check(
        "isNonImmediate",
        offset_of!(ours::VirtualMachine, isNonImmediate),
        offset_of!(theirs::VirtualMachine, isNonImmediate),
        &mut mismatches,
    );
    check(
        "platformSemaphoreNew",
        offset_of!(ours::VirtualMachine, platformSemaphoreNew),
        offset_of!(theirs::VirtualMachine, platformSemaphoreNew),
        &mut mismatches,
    );
    check(
        "scheduleInMainThread",
        offset_of!(ours::VirtualMachine, scheduleInMainThread),
        offset_of!(theirs::VirtualMachine, scheduleInMainThread),
        &mut mismatches,
    );
    check(
        "waitOnExternalSemaphoreIndex",
        offset_of!(ours::VirtualMachine, waitOnExternalSemaphoreIndex),
        offset_of!(theirs::VirtualMachine, waitOnExternalSemaphoreIndex),
        &mut mismatches,
    );

    assert!(
        mismatches.is_empty(),
        "the checked-in interpreter proxy has drifted from \
         include/pharovm/common/virtualMachine.h:\n{}\n\n\
         Regenerate pharo-vm-plugin/src/proxy.rs from the header, and bump \
         VM_PROXY_MINOR if the published ABI really changed.",
        mismatches.join("\n")
    );
}

/// The proxy version this crate claims must match what the headers declare.
#[test]
fn proxy_version_is_the_one_the_struct_came_from() {
    // VM_PROXY_MAJOR/MINOR are #defines, so bindgen does not surface them
    // here; this pins the version proxy.rs was extracted at.
    assert_eq!(ours::VM_PROXY_MAJOR, 1);
    assert_eq!(ours::VM_PROXY_MINOR, 15);
}
