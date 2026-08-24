//! Replaces `src/common/sqVirtualMachine.c` on Unix.
//!
//! Builds the interpreter proxy: the `VirtualMachine` struct of function
//! pointers that every plugin is handed by `setInterpreter`, and through which
//! every external primitive reaches the interpreter. There is almost no logic
//! here -- it is one table, filled in once and cached.
//!
//! # Why the declarations moved to a header
//!
//! About half of these functions were declared nowhere but at the top of
//! `sqVirtualMachine.c`, so porting the file would have deleted the only
//! statement of their signatures and left the Rust restating them from
//! memory. Getting one wrong is undefined behaviour that neither the linker
//! nor `abi-check.sh` can see, because C linkage carries no types.
//!
//! So the block moved to `include/pharovm/common/interpreterProxyFunctions.h`
//! -- unchanged, same order, same conditionals -- and both sides read it:
//! `sqVirtualMachine.c` includes it, and `pharo-vm-sys` binds the whole file.
//! Every assignment below is therefore checked by rustc against a field type
//! and a function type that both came from the C headers. A wrong pairing does
//! not compile.
//!
//! # Pinned to proxy version 1.15
//!
//! The C selects entries with `#if VM_PROXY_MINOR > N` for N from 1 to 14, and
//! `VM_PROXY_MINOR` is 15, so every one of those is taken and the single
//! `<= 13` alternative is not. Rather than carry seventeen dead branches, this
//! file is written for 15 and asserts it at compile time. Raising the proxy
//! version fails the build here, which is the right place to notice.
//!
//! # Faithful oddities
//!
//! * `fetchIntegerofObject` is not the interpreter's function but a wrapper
//!   that answers a Character's code point when field 0 of one is asked for.
//!   Plugins written before immediate Characters existed read them that way.
//! * The slot that used to be `fetchWordofObject` is named
//!   `obsoleteDontUseThisFetchWordofObject` in both the struct and the
//!   function. It keeps its position so plugins built before the 64-bit-clean
//!   rework still find something there; `fetchLong32ofObject` is the
//!   replacement.
//! * Four of the 153 slots end up null. `scheduleInMainThread` is assigned
//!   `NULL` outright; `showDisplayBitsLeftTopRightBottom`,
//!   `sendInvokeCallbackStackRegistersJmpbuf` and
//!   `reestablishContextPriorToCallback` are declared by
//!   `virtualMachine.h` and never mentioned by the C at all. A plugin that
//!   called one would call through a null pointer. Left exactly as they were
//!   -- filling them in is a decision about the plugin ABI, not a port -- but
//!   there is a test that names all four, so a fifth cannot appear unnoticed.
//! * The proxy is built once and never freed (the C `calloc`ed it; here the
//!   table is a leaked `Box`, same lifetime), and `sqGetInterpreterProxy` is
//!   not synchronised -- two threads racing the first call would each build a
//!   table and one would leak. Only the VM thread calls it.

use core::ffi::c_int;

use pharo_vm_sys::{sqInt, VirtualMachine};

/// `usqLong` from `memoryAccess.h`, the VM's unsigned at-least-64-bit integer.
///
/// A `#define` rather than a typedef, so bindgen cannot emit it.
type UsqLong = u64;

const _: () = assert!(
    core::mem::size_of::<UsqLong>() >= 8,
    "usqLong must hold at least 64 bits"
);

/// The C's `VM_PROXY_MAJOR`.
const VM_PROXY_MAJOR: sqInt = pharo_vm_sys::VM_PROXY_MAJOR as sqInt;
/// The C's `VM_PROXY_MINOR`.
const VM_PROXY_MINOR: sqInt = pharo_vm_sys::VM_PROXY_MINOR as sqInt;

const _: () = assert!(
    VM_PROXY_MINOR == 15,
    "the proxy table below is written for VM_PROXY_MINOR 15; see the module docs"
);

/// The cached proxy. Exported because the C exported it.
#[no_mangle]
pub static mut VM: *mut VirtualMachine = core::ptr::null_mut();

/// The proxy's major version.
extern "C" fn major_version() -> sqInt {
    VM_PROXY_MAJOR
}

/// The proxy's minor version.
extern "C" fn minor_version() -> sqInt {
    VM_PROXY_MINOR
}

/// `fetchIntegerofObject`, with Characters handled.
///
/// A plugin asking for field 0 of a Character gets its code point. Characters
/// became immediates in Spur and stopped having fields at all, so without this
/// every plugin written against the old representation would break.
///
/// # Safety
///
/// `object_pointer` must be an oop, and this must run on the VM thread.
unsafe extern "C" fn intercept_fetch_integer_of_object(
    field_index: sqInt,
    object_pointer: sqInt,
) -> sqInt {
    // SAFETY: delegated to the caller; both are interpreter entry points.
    unsafe {
        if field_index == 0 && pharo_vm_sys::isCharacterObject(object_pointer) != 0 {
            return pharo_vm_sys::characterValueOf(object_pointer);
        }
        pharo_vm_sys::fetchIntegerofObject(field_index, object_pointer)
    }
}

/// Answers the interpreter proxy, building it on first call.
///
/// # Safety
///
/// Must be called on the VM thread. The returned pointer is valid for the
/// life of the process and is shared with every loaded plugin.
#[no_mangle]
pub unsafe extern "C" fn sqGetInterpreterProxy() -> *mut VirtualMachine {
    // SAFETY: VM is only written here, on the VM thread, before any plugin
    // exists to read it.
    unsafe {
        if !VM.is_null() {
            return VM;
        }
        // Generated from the `VM->field = function;` block in
        // sqVirtualMachine.c, with VM_PROXY_MINOR resolved to 15. Order is
        // the C's, with the three slots the C never mentions spelled out at
        // the end. Each entry is type-checked against the field's declared
        // signature, so a wrong pairing is a compile error rather than a
        // corrupted plugin call -- and a struct literal must name every
        // field, so a dropped slot is a compile error too.
        //
        // Built as a value and leaked: the table lives for the whole process,
        // exactly as the C's calloc-and-never-free did. (The C did not check
        // calloc and would have crashed writing through null; allocation
        // failure aborts here instead.)
        VM = Box::leak(Box::new(VirtualMachine {
            majorVersion: Some(major_version),
            minorVersion: Some(minor_version),
            pop: Some(pharo_vm_sys::pop),
            popthenPush: Some(pharo_vm_sys::popthenPush),
            push: Some(pharo_vm_sys::push),
            pushBool: Some(pharo_vm_sys::pushBool),
            pushFloat: Some(pharo_vm_sys::pushFloat),
            pushInteger: Some(pharo_vm_sys::pushInteger),
            stackFloatValue: Some(pharo_vm_sys::stackFloatValue),
            stackIntegerValue: Some(pharo_vm_sys::stackIntegerValue),
            stackObjectValue: Some(pharo_vm_sys::stackObjectValue),
            stackValue: Some(pharo_vm_sys::stackValue),
            argumentCountOf: Some(pharo_vm_sys::argumentCountOf),
            arrayValueOf: Some(pharo_vm_sys::arrayValueOf),
            byteSizeOf: Some(pharo_vm_sys::byteSizeOf),
            fetchArrayofObject: Some(pharo_vm_sys::fetchArrayofObject),
            fetchClassOf: Some(pharo_vm_sys::fetchClassOf),
            fetchFloatofObject: Some(pharo_vm_sys::fetchFloatofObject),
            fetchIntegerofObject: Some(intercept_fetch_integer_of_object),
            fetchPointerofObject: Some(pharo_vm_sys::fetchPointerofObject),
            obsoleteDontUseThisFetchWordofObject: Some(
                pharo_vm_sys::obsoleteDontUseThisFetchWordofObject,
            ),
            firstFixedField: Some(pharo_vm_sys::firstFixedField),
            firstIndexableField: Some(pharo_vm_sys::firstIndexableField),
            literalofMethod: Some(pharo_vm_sys::literalofMethod),
            literalCountOf: Some(pharo_vm_sys::literalCountOf),
            methodArgumentCount: Some(pharo_vm_sys::methodArgumentCount),
            methodPrimitiveIndex: Some(pharo_vm_sys::methodPrimitiveIndex),
            primitiveIndexOf: Some(pharo_vm_sys::primitiveIndexOf),
            primitiveMethod: Some(pharo_vm_sys::primitiveMethod),
            sizeOfSTArrayFromCPrimitive: Some(pharo_vm_sys::sizeOfSTArrayFromCPrimitive),
            slotSizeOf: Some(pharo_vm_sys::slotSizeOf),
            stObjectat: Some(pharo_vm_sys::stObjectat),
            stObjectatput: Some(pharo_vm_sys::stObjectatput),
            stSizeOf: Some(pharo_vm_sys::stSizeOf),
            storeIntegerofObjectwithValue: Some(pharo_vm_sys::storeIntegerofObjectwithValue),
            storePointerofObjectwithValue: Some(pharo_vm_sys::storePointerofObjectwithValue),
            isKindOf: Some(pharo_vm_sys::isKindOf),
            isMemberOf: Some(pharo_vm_sys::isMemberOf),
            isBytes: Some(pharo_vm_sys::isBytes),
            isFloatObject: Some(pharo_vm_sys::isFloatObject),
            isIndexable: Some(pharo_vm_sys::isIndexable),
            isIntegerObject: Some(pharo_vm_sys::isIntegerObject),
            isIntegerValue: Some(pharo_vm_sys::isIntegerValue),
            isPointers: Some(pharo_vm_sys::isPointers),
            isWeak: Some(pharo_vm_sys::isWeak),
            isWords: Some(pharo_vm_sys::isWords),
            isWordsOrBytes: Some(pharo_vm_sys::isWordsOrBytes),
            booleanValueOf: Some(pharo_vm_sys::booleanValueOf),
            checkedIntegerValueOf: Some(pharo_vm_sys::checkedIntegerValueOf),
            floatObjectOf: Some(pharo_vm_sys::floatObjectOf),
            floatValueOf: Some(pharo_vm_sys::floatValueOf),
            integerObjectOf: Some(pharo_vm_sys::integerObjectOf),
            integerValueOf: Some(pharo_vm_sys::integerValueOf),
            positive32BitIntegerFor: Some(pharo_vm_sys::positive32BitIntegerFor),
            positive32BitValueOf: Some(pharo_vm_sys::positive32BitValueOf),
            falseObject: Some(pharo_vm_sys::falseObject),
            nilObject: Some(pharo_vm_sys::nilObject),
            trueObject: Some(pharo_vm_sys::trueObject),
            classArray: Some(pharo_vm_sys::classArray),
            classBitmap: Some(pharo_vm_sys::classBitmap),
            classByteArray: Some(pharo_vm_sys::classByteArray),
            classCharacter: Some(pharo_vm_sys::classCharacter),
            classFloat: Some(pharo_vm_sys::classFloat),
            classLargePositiveInteger: Some(pharo_vm_sys::classLargePositiveInteger),
            classPoint: Some(pharo_vm_sys::classPoint),
            classSemaphore: Some(pharo_vm_sys::classSemaphore),
            classSmallInteger: Some(pharo_vm_sys::classSmallInteger),
            classString: Some(pharo_vm_sys::classString),
            clone: Some(pharo_vm_sys::clone),
            instantiateClassindexableSize: Some(pharo_vm_sys::instantiateClassindexableSize),
            makePointwithxValueyValue: Some(pharo_vm_sys::makePointwithxValueyValue),
            popRemappableOop: Some(pharo_vm_sys::popRemappableOop),
            pushRemappableOop: Some(pharo_vm_sys::pushRemappableOop),
            becomewith: Some(pharo_vm_sys::becomewith),
            byteSwapped: Some(pharo_vm_sys::byteSwapped),
            failed: Some(pharo_vm_sys::failed),
            fullGC: Some(pharo_vm_sys::fullGC),
            primitiveFail: Some(pharo_vm_sys::primitiveFail),
            signalSemaphoreWithIndex: Some(crate::external_semaphores::signalSemaphoreWithIndex),
            success: Some(pharo_vm_sys::success),
            superclassOf: Some(pharo_vm_sys::superclassOf),
            loadBitBltFrom: Some(pharo_vm_sys::loadBitBltFrom),
            copyBits: Some(pharo_vm_sys::copyBits),
            copyBitsFromtoat: Some(pharo_vm_sys::copyBitsFromtoat),
            classExternalAddress: Some(pharo_vm_sys::classExternalAddress),
            ioLoadModuleOfLength: Some(crate::named_prims::ioLoadModuleOfLength),
            ioLoadSymbolOfLengthFromModule: Some(
                crate::named_prims::ioLoadSymbolOfLengthFromModule,
            ),
            isInMemory: Some(pharo_vm_sys::isInMemory),
            signed32BitIntegerFor: Some(pharo_vm_sys::signed32BitIntegerFor),
            signed32BitValueOf: Some(pharo_vm_sys::signed32BitValueOf),
            includesBehaviorThatOf: Some(pharo_vm_sys::includesBehaviorThatOf),
            classLargeNegativeInteger: Some(pharo_vm_sys::classLargeNegativeInteger),
            ioLoadFunctionFrom: Some(crate::named_prims::ioLoadFunctionFrom),
            ioMicroMSecs: Some(pharo_vm_sys::ioMicroMSecs),
            positive64BitIntegerFor: Some(pharo_vm_sys::positive64BitIntegerFor),
            positive64BitValueOf: Some(pharo_vm_sys::positive64BitValueOf),
            signed64BitIntegerFor: Some(pharo_vm_sys::signed64BitIntegerFor),
            signed64BitValueOf: Some(pharo_vm_sys::signed64BitValueOf),
            isArray: Some(pharo_vm_sys::isArray),
            forceInterruptCheck: Some(pharo_vm_sys::forceInterruptCheck),
            fetchLong32ofObject: Some(pharo_vm_sys::fetchLong32ofObject),
            getThisSessionID: Some(pharo_vm_sys::getThisSessionID),
            ioFilenamefromStringofLengthresolveAliases: Some(
                pharo_vm_sys::ioFilenamefromStringofLengthresolveAliases,
            ),
            vmEndianness: Some(pharo_vm_sys::vmEndianness),
            addGCRoot: Some(pharo_vm_sys::addGCRoot),
            removeGCRoot: Some(pharo_vm_sys::removeGCRoot),
            primitiveFailFor: Some(pharo_vm_sys::primitiveFailFor),
            isOopImmutable: Some(pharo_vm_sys::isOopImmutable),
            isOopMutable: Some(pharo_vm_sys::isOopMutable),
            methodReturnBool: Some(pharo_vm_sys::methodReturnBool),
            methodReturnFloat: Some(pharo_vm_sys::methodReturnFloat),
            methodReturnInteger: Some(pharo_vm_sys::methodReturnInteger),
            methodReturnReceiver: Some(pharo_vm_sys::methodReturnReceiver),
            methodReturnString: Some(pharo_vm_sys::methodReturnString),
            methodReturnValue: Some(pharo_vm_sys::methodReturnValue),
            topRemappableOop: Some(pharo_vm_sys::topRemappableOop),
            addHighPriorityTickee: Some(pharo_vm_sys::addHighPriorityTickee),
            addSynchronousTickee: Some(pharo_vm_sys::addSynchronousTickee),
            utcMicroseconds: Some(pharo_vm_sys::ioUTCMicroseconds),
            tenuringIncrementalGC: Some(pharo_vm_sys::tenuringIncrementalGC),
            isYoung: Some(pharo_vm_sys::isYoung),
            isKindOfClass: Some(pharo_vm_sys::isKindOfClass),
            primitiveErrorTable: Some(pharo_vm_sys::primitiveErrorTable),
            primitiveFailureCode: Some(pharo_vm_sys::primitiveFailureCode),
            instanceSizeOf: Some(pharo_vm_sys::instanceSizeOf),
            signedMachineIntegerValueOf: Some(pharo_vm_sys::signedMachineIntegerValueOf),
            stackSignedMachineIntegerValue: Some(pharo_vm_sys::stackSignedMachineIntegerValue),
            positiveMachineIntegerValueOf: Some(pharo_vm_sys::positiveMachineIntegerValueOf),
            stackPositiveMachineIntegerValue: Some(pharo_vm_sys::stackPositiveMachineIntegerValue),
            cStringOrNullFor: Some(pharo_vm_sys::cStringOrNullFor),
            signalNoResume: Some(pharo_vm_sys::signalNoResume),
            isImmediate: Some(pharo_vm_sys::isImmediate),
            characterObjectOf: Some(pharo_vm_sys::characterObjectOf),
            characterValueOf: Some(pharo_vm_sys::characterValueOf),
            isCharacterObject: Some(pharo_vm_sys::isCharacterObject),
            isCharacterValue: Some(pharo_vm_sys::isCharacterValue),
            isPinned: Some(pharo_vm_sys::isPinned),
            pinObject: Some(pharo_vm_sys::pinObject),
            unpinObject: Some(pharo_vm_sys::unpinObject),
            statNumGCs: Some(pharo_vm_sys::statNumGCs),
            stringForCString: Some(pharo_vm_sys::stringForCString),
            primitiveFailForOSError: Some(pharo_vm_sys::primitiveFailForOSError),
            isBooleanObject: Some(pharo_vm_sys::isBooleanObject),
            isPositiveMachineIntegerObject: Some(pharo_vm_sys::isPositiveMachineIntegerObject),
            ptEnterInterpreterFromCallback: Some(pharo_vm_sys::ptEnterInterpreterFromCallback),
            ptExitInterpreterToCallback: Some(pharo_vm_sys::ptExitInterpreterToCallback),
            isNonImmediate: Some(pharo_vm_sys::isNonImmediate),
            platformSemaphoreNew: Some(crate::platform_semaphore::platform_semaphore_new),
            scheduleInMainThread: None,
            waitOnExternalSemaphoreIndex: Some(
                crate::external_semaphores::waitOnExternalSemaphoreIndex,
            ),
            // Declared by virtualMachine.h and never assigned by the C; see the
            // module docs.
            showDisplayBitsLeftTopRightBottom: None,
            sendInvokeCallbackStackRegistersJmpbuf: None,
            reestablishContextPriorToCallback: None,
        }));

        VM
    }
}

/// Prints how long each start-up phase took, when phase 1 asked for it.
///
/// Phase 1 starts the clock and prints the wall-clock time, 2 reports the
/// image load, 3 reports the run. Nothing prints unless phase 1 ran, which is
/// how `-timePhases` stays off by default.
///
/// Prints through `libc::printf`, not `print!`: the C build writes these
/// lines through C stdio, and differential testing compares the interleaved
/// output, so they must share a buffer with the rest of it.
#[no_mangle]
pub extern "C" fn printPhaseTime(phase: c_int) {
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    /// Microseconds in a second, the C's `m`.
    const M: UsqLong = 1_000_000;
    /// Microseconds in a millisecond, the C's `k`.
    const K: UsqLong = 1000;

    extern "C" {
        /// `asctime`, which the `libc` crate does not expose. Answers a
        /// static buffer that already ends in a newline.
        fn asctime(tm: *const libc::tm) -> *mut core::ffi::c_char;
    }

    // Relaxed atomics: only the start-up path calls this, so there is no
    // ordering to establish; the atomics just keep the statics safe to touch.
    static PRINT_TIMES: AtomicBool = AtomicBool::new(false);
    static LAST_USECS: AtomicU64 = AtomicU64::new(0);

    if phase == 1 {
        PRINT_TIMES.store(true, Ordering::Relaxed);
        // SAFETY: time with a null out-parameter, localtime on the result,
        // printf with a literal format string and asctime's static buffer.
        unsafe {
            let now = libc::time(core::ptr::null_mut());
            let tm = libc::localtime(&now);
            // asctime answers a static buffer ending in a newline, which is
            // why the format string has none.
            libc::printf(c"started at %s".as_ptr(), asctime(tm));
            LAST_USECS.store(pharo_vm_sys::ioUTCMicrosecondsNow(), Ordering::Relaxed);
        }
        return;
    }

    if !PRINT_TIMES.load(Ordering::Relaxed) {
        return;
    }

    // SAFETY: interpreter time query and printf with literal format strings
    // and scalar arguments.
    unsafe {
        let now_usecs = pharo_vm_sys::ioUTCMicrosecondsNow();
        let usecs = now_usecs.wrapping_sub(LAST_USECS.load(Ordering::Relaxed));
        LAST_USECS.store(now_usecs, Ordering::Relaxed);

        // Seconds and milliseconds, the milliseconds rounded to nearest.
        let secs = (usecs / M) as core::ffi::c_ulong;
        let millis = ((usecs % M + K / 2) / K) as core::ffi::c_ulong;

        if phase == 2 {
            libc::printf(c"loaded in %lu.%03lus\n".as_ptr(), secs, millis);
        }
        if phase == 3 {
            // Cleared so an error during exit does not print twice.
            PRINT_TIMES.store(false, Ordering::Relaxed);
            if usecs >= 1u64 << 32 {
                libc::printf(c"ran for a long time\n".as_ptr());
            } else {
                libc::printf(c"ran for %lu.%03lus\n".as_ptr(), secs, millis);
            }
        }
    }
}

/// Test-only definitions for the interpreter symbols this crate references but
/// does not provide.
///
/// The proxy table names ~150 functions that live in the generated
/// interpreter, which the unit-test binary does not link. Earlier waves gave
/// each such call a `cfg(test)` seam, but a table of function *pointers* has no
/// call to intercept -- taking the address is enough to need the symbol.
///
/// So the symbols are defined here instead, once, and never called. Their
/// signatures are deliberately empty: C linkage carries no types, so the
/// linker is satisfied, and the assignments in `sqGetInterpreterProxy` are
/// still checked against the *real* declarations in
/// `interpreterProxyFunctions.h` by way of `pharo-vm-sys`. Each body is
/// `unreachable!` so that a test which somehow does call one fails loudly
/// rather than corrupting its own stack.
#[cfg(test)]
#[allow(non_snake_case)]
mod link_stubs {
    #[no_mangle]
    extern "C" fn addGCRoot() {
        unreachable!("addGCRoot is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn addHighPriorityTickee() {
        unreachable!("addHighPriorityTickee is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn addSynchronousTickee() {
        unreachable!("addSynchronousTickee is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn aioInterruptPoll() {
        unreachable!("aioInterruptPoll is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn argumentCountOf() {
        unreachable!("argumentCountOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn arrayValueOf() {
        unreachable!("arrayValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn becomewith() {
        unreachable!("becomewith is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn booleanValueOf() {
        unreachable!("booleanValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn byteSizeOf() {
        unreachable!("byteSizeOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn byteSwapped() {
        unreachable!("byteSwapped is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn cStringOrNullFor() {
        unreachable!("cStringOrNullFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn characterObjectOf() {
        unreachable!("characterObjectOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn characterValueOf() {
        unreachable!("characterValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn checkedIntegerValueOf() {
        unreachable!("checkedIntegerValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classArray() {
        unreachable!("classArray is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classBitmap() {
        unreachable!("classBitmap is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classByteArray() {
        unreachable!("classByteArray is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classCharacter() {
        unreachable!("classCharacter is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classExternalAddress() {
        unreachable!("classExternalAddress is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classFloat() {
        unreachable!("classFloat is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classLargeNegativeInteger() {
        unreachable!("classLargeNegativeInteger is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classLargePositiveInteger() {
        unreachable!("classLargePositiveInteger is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classPoint() {
        unreachable!("classPoint is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classSemaphore() {
        unreachable!("classSemaphore is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classSmallInteger() {
        unreachable!("classSmallInteger is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn classString() {
        unreachable!("classString is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn clone() {
        unreachable!("clone is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn copyBits() {
        unreachable!("copyBits is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn copyBitsFromtoat() {
        unreachable!("copyBitsFromtoat is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn doSignalSemaphoreWithIndex() {
        unreachable!("doSignalSemaphoreWithIndex is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn doWaitSemaphore() {
        unreachable!("doWaitSemaphore is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn failed() {
        unreachable!("failed is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn falseObject() {
        unreachable!("falseObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchArrayofObject() {
        unreachable!("fetchArrayofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchClassOf() {
        unreachable!("fetchClassOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchFloatofObject() {
        unreachable!("fetchFloatofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchIntegerofObject() {
        unreachable!("fetchIntegerofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchLong32ofObject() {
        unreachable!("fetchLong32ofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fetchPointerofObject() {
        unreachable!("fetchPointerofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn firstFixedField() {
        unreachable!("firstFixedField is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn firstIndexableField() {
        unreachable!("firstIndexableField is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn floatArg() {
        unreachable!("floatArg is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn floatObjectOf() {
        unreachable!("floatObjectOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn floatValueOf() {
        unreachable!("floatValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn forceInterruptCheck() {
        unreachable!("forceInterruptCheck is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn fullGC() {
        unreachable!("fullGC is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn getExternalSemaphoreWithIndex() {
        unreachable!("getExternalSemaphoreWithIndex is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn getInterruptPending() {
        unreachable!("getInterruptPending is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn getThisSessionID() {
        unreachable!("getThisSessionID is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn highBit() {
        unreachable!("highBit is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn includesBehaviorThatOf() {
        unreachable!("includesBehaviorThatOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn instanceSizeOf() {
        unreachable!("instanceSizeOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn instantiateClassindexableSize() {
        unreachable!("instantiateClassindexableSize is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn integerArg() {
        unreachable!("integerArg is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn integerObjectOf() {
        unreachable!("integerObjectOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn integerValueOf() {
        unreachable!("integerValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn ioFilenamefromStringofLengthresolveAliases() {
        unreachable!(
            "ioFilenamefromStringofLengthresolveAliases is a link stub; no test may call it"
        )
    }
    #[no_mangle]
    extern "C" fn ioMicroMSecs() {
        unreachable!("ioMicroMSecs is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn ioUTCMicroseconds() {
        unreachable!("ioUTCMicroseconds is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn ioUTCMicrosecondsNow() {
        unreachable!("ioUTCMicrosecondsNow is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isArray() {
        unreachable!("isArray is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isBooleanObject() {
        unreachable!("isBooleanObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isBytes() {
        unreachable!("isBytes is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isCharacterObject() {
        unreachable!("isCharacterObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isCharacterValue() {
        unreachable!("isCharacterValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isFloatObject() {
        unreachable!("isFloatObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isImmediate() {
        unreachable!("isImmediate is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isInMemory() {
        unreachable!("isInMemory is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isIndexable() {
        unreachable!("isIndexable is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isIntegerObject() {
        unreachable!("isIntegerObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isIntegerValue() {
        unreachable!("isIntegerValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isYoung() {
        unreachable!("isYoung is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isKindOf() {
        unreachable!("isKindOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isKindOfClass() {
        unreachable!("isKindOfClass is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isMemberOf() {
        unreachable!("isMemberOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isNonImmediate() {
        unreachable!("isNonImmediate is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isOopImmutable() {
        unreachable!("isOopImmutable is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isOopMutable() {
        unreachable!("isOopMutable is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isPinned() {
        unreachable!("isPinned is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isPointers() {
        unreachable!("isPointers is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isPositiveMachineIntegerObject() {
        unreachable!("isPositiveMachineIntegerObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isWeak() {
        unreachable!("isWeak is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isWords() {
        unreachable!("isWords is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn isWordsOrBytes() {
        unreachable!("isWordsOrBytes is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn literalCountOf() {
        unreachable!("literalCountOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn literalofMethod() {
        unreachable!("literalofMethod is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn loadBitBltFrom() {
        unreachable!("loadBitBltFrom is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn makePointwithxValueyValue() {
        unreachable!("makePointwithxValueyValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodArg() {
        unreachable!("methodArg is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodArgumentCount() {
        unreachable!("methodArgumentCount is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodPrimitiveIndex() {
        unreachable!("methodPrimitiveIndex is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnBool() {
        unreachable!("methodReturnBool is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnFloat() {
        unreachable!("methodReturnFloat is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnInteger() {
        unreachable!("methodReturnInteger is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnReceiver() {
        unreachable!("methodReturnReceiver is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnString() {
        unreachable!("methodReturnString is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn methodReturnValue() {
        unreachable!("methodReturnValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn nilObject() {
        unreachable!("nilObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn objectArg() {
        unreachable!("objectArg is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn obsoleteDontUseThisFetchWordofObject() {
        unreachable!("obsoleteDontUseThisFetchWordofObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pinObject() {
        unreachable!("pinObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pop() {
        unreachable!("pop is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn popRemappableOop() {
        unreachable!("popRemappableOop is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn popthenPush() {
        unreachable!("popthenPush is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn positive32BitIntegerFor() {
        unreachable!("positive32BitIntegerFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn positive32BitValueOf() {
        unreachable!("positive32BitValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn positive64BitIntegerFor() {
        unreachable!("positive64BitIntegerFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn positive64BitValueOf() {
        unreachable!("positive64BitValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn positiveMachineIntegerValueOf() {
        unreachable!("positiveMachineIntegerValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveErrorTable() {
        unreachable!("primitiveErrorTable is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveFail() {
        unreachable!("primitiveFail is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveFailFor() {
        unreachable!("primitiveFailFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveFailForOSError() {
        unreachable!("primitiveFailForOSError is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveFailureCode() {
        unreachable!("primitiveFailureCode is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveIndexOf() {
        unreachable!("primitiveIndexOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn primitiveMethod() {
        unreachable!("primitiveMethod is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn ptEnterInterpreterFromCallback() {
        unreachable!("ptEnterInterpreterFromCallback is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn ptExitInterpreterToCallback() {
        unreachable!("ptExitInterpreterToCallback is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn push() {
        unreachable!("push is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pushBool() {
        unreachable!("pushBool is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pushFloat() {
        unreachable!("pushFloat is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pushInteger() {
        unreachable!("pushInteger is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn pushRemappableOop() {
        unreachable!("pushRemappableOop is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn removeGCRoot() {
        unreachable!("removeGCRoot is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signalNoResume() {
        unreachable!("signalNoResume is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signed32BitIntegerFor() {
        unreachable!("signed32BitIntegerFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signed32BitValueOf() {
        unreachable!("signed32BitValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signed64BitIntegerFor() {
        unreachable!("signed64BitIntegerFor is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signed64BitValueOf() {
        unreachable!("signed64BitValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn signedMachineIntegerValueOf() {
        unreachable!("signedMachineIntegerValueOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn sizeOfAlienData() {
        unreachable!("sizeOfAlienData is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn sizeOfSTArrayFromCPrimitive() {
        unreachable!("sizeOfSTArrayFromCPrimitive is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn slotSizeOf() {
        unreachable!("slotSizeOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stObjectat() {
        unreachable!("stObjectat is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stObjectatput() {
        unreachable!("stObjectatput is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stSizeOf() {
        unreachable!("stSizeOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackFloatValue() {
        unreachable!("stackFloatValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackIntegerValue() {
        unreachable!("stackIntegerValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackObjectValue() {
        unreachable!("stackObjectValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackPositiveMachineIntegerValue() {
        unreachable!("stackPositiveMachineIntegerValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackSignedMachineIntegerValue() {
        unreachable!("stackSignedMachineIntegerValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stackValue() {
        unreachable!("stackValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn startOfAlienData() {
        unreachable!("startOfAlienData is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn statNumGCs() {
        unreachable!("statNumGCs is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn storeIntegerofObjectwithValue() {
        unreachable!("storeIntegerofObjectwithValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn storePointerofObjectwithValue() {
        unreachable!("storePointerofObjectwithValue is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn stringForCString() {
        unreachable!("stringForCString is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn success() {
        unreachable!("success is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn superclassOf() {
        unreachable!("superclassOf is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn tenuringIncrementalGC() {
        unreachable!("tenuringIncrementalGC is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn topRemappableOop() {
        unreachable!("topRemappableOop is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn trueObject() {
        unreachable!("trueObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn unpinObject() {
        unreachable!("unpinObject is a link stub; no test may call it")
    }
    #[no_mangle]
    extern "C" fn vmEndianness() {
        unreachable!("vmEndianness is a link stub; no test may call it")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slots that end up null, and why. See the module docs.
    ///
    /// Named rather than counted so that a new hole has to be added here
    /// deliberately.
    const NULL_SLOTS: &[&str] = &[
        // Assigned NULL by the C.
        "scheduleInMainThread",
        // Declared by virtualMachine.h and never assigned anywhere.
        "showDisplayBitsLeftTopRightBottom",
        "sendInvokeCallbackStackRegistersJmpbuf",
        "reestablishContextPriorToCallback",
    ];

    /// Reads the proxy, building it if this is the first test to ask.
    ///
    /// Safe to call from several tests at once only because the table is
    /// idempotent: the loser of a race leaks one table and both answers are
    /// equally valid. Production has a single caller.
    fn proxy() -> &'static VirtualMachine {
        // SAFETY: every function stored is a plain pointer; nothing is called.
        unsafe { &*sqGetInterpreterProxy() }
    }

    #[test]
    fn the_proxy_is_built_once_and_cached() {
        let first = proxy() as *const VirtualMachine;
        let second = proxy() as *const VirtualMachine;
        assert_eq!(
            first, second,
            "the second call must answer the cached table"
        );
        // SAFETY: reading the exported static, which only this module writes.
        assert_eq!(unsafe { VM }.cast_const(), first);
    }

    #[test]
    fn it_reports_the_version_the_table_was_written_for() {
        let vm = proxy();
        // SAFETY: both slots are filled in unconditionally above.
        unsafe {
            assert_eq!(vm.majorVersion.unwrap()(), 1);
            assert_eq!(vm.minorVersion.unwrap()(), 15);
        }
        assert_eq!(
            VM_PROXY_MINOR, 15,
            "the const assertion should have caught this"
        );
    }

    /// Every `Option<fn>` field of the proxy, by name, as a raw word.
    ///
    /// Reading the struct as words rather than field by field is what makes
    /// the completeness test below possible: naming the fields would mean
    /// writing the same list a third time, which is exactly what this wave
    /// exists to avoid.
    fn slots() -> Vec<usize> {
        let vm = proxy();
        let base = (vm as *const VirtualMachine).cast::<usize>();
        let count = core::mem::size_of::<VirtualMachine>() / core::mem::size_of::<usize>();
        // SAFETY: the struct is entirely function pointers, so every word in
        // it is a readable, initialised pointer-sized value.
        (0..count).map(|i| unsafe { *base.add(i) }).collect()
    }

    #[test]
    fn the_struct_is_nothing_but_pointer_sized_slots() {
        // The word-wise reads above are only valid if the struct has no
        // padding and no narrower fields. If the proxy ever grows an `int`,
        // this fails before the completeness test starts lying.
        assert_eq!(
            core::mem::size_of::<VirtualMachine>() % core::mem::size_of::<usize>(),
            0,
            "the proxy struct should be a whole number of pointer-sized slots"
        );
    }

    #[test]
    fn every_slot_the_c_filled_in_is_filled_in_here() {
        // The C makes 150 assignments, one of them NULL, into a struct of 153
        // slots. Anything else means a slot was dropped or doubled up when the
        // table was regenerated -- which nothing else would catch, since a
        // missing entry is a null pointer a plugin only finds at run time.
        const ASSIGNMENTS: usize = 150;

        let slots = slots();
        let total = slots.len();
        let filled = slots.iter().filter(|w| **w != 0).count();

        assert_eq!(total, 153, "the proxy struct changed size");
        assert_eq!(
            filled,
            ASSIGNMENTS - 1,
            "expected {} filled slots, found {filled} of {total}",
            ASSIGNMENTS - 1
        );
        assert_eq!(
            total - filled,
            NULL_SLOTS.len(),
            "the null slots are {NULL_SLOTS:?}; a different number came out null"
        );
    }

    #[test]
    fn the_named_null_slots_really_are_null() {
        // The four the module docs list, checked by name rather than by
        // counting, so that filling one in on purpose fails here first.
        let vm = proxy();
        assert!(vm.scheduleInMainThread.is_none());
        assert!(vm.showDisplayBitsLeftTopRightBottom.is_none());
        assert!(vm.sendInvokeCallbackStackRegistersJmpbuf.is_none());
        assert!(vm.reestablishContextPriorToCallback.is_none());
    }

    #[test]
    fn the_obsolete_fetch_word_slot_is_still_occupied() {
        // Kept pointing at obsoleteDontUseThisFetchWordofObject so that
        // plugins built before the 64-bit-clean rework still find something
        // there. An empty slot would be a null call.
        let vm = proxy();
        assert!(
            vm.obsoleteDontUseThisFetchWordofObject.is_some(),
            "old plugins call this slot; it must not be null"
        );
        assert!(vm.fetchLong32ofObject.is_some(), "and its replacement");
    }
}
