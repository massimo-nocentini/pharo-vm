//! Proxy plumbing the safe `Interp` API does not cover, plus the string and
//! collection helpers the C plugin's `OSProcessPlugin` superclass provided.

use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};

use pharo_vm_plugin::{sqInt, Interp, Oop, PrimErr, PrimResult};

/// Calls a raw proxy entry through `vm.as_raw()`, failing cleanly when the VM
/// left the slot empty (older or differently configured VM).
macro_rules! raw_call {
    ($vm:expr, $field:ident ( $($arg:expr),* $(,)? )) => {{
        // SAFETY: the pointer came from the VM's own proxy table via
        // `Interp::as_raw`, and the signature is the one `proxy.rs` pins.
        let f = unsafe { (*$vm.as_raw()).$field }.ok_or(PrimErr::Unsupported)?;
        unsafe { f($($arg),*) }
    }};
}

/// `stackObjectValue`: the oop at `offset`, failing (like the C accessor) when
/// the slot holds an immediate rather than an object.
pub fn stack_object_value(vm: &Interp, offset: sqInt) -> PrimResult<Oop> {
    let oop = raw_call!(vm, stackObjectValue(offset));
    vm.check_failed()?;
    Ok(Oop(oop))
}

/// `stSizeOf`: the number of indexable slots/bytes, one-based-protocol size.
pub fn st_size_of(vm: &Interp, oop: Oop) -> PrimResult<sqInt> {
    Ok(raw_call!(vm, stSizeOf(oop.0)))
}

/// `stObjectat:put:` -- one-based, store-checked element write.
pub fn st_object_at_put(vm: &Interp, array: Oop, index: sqInt, value: Oop) -> PrimResult<()> {
    raw_call!(vm, stObjectatput(array.0, index, value.0));
    vm.check_failed()
}

/// `firstIndexableField` as a raw pointer. The caller owns the aliasing
/// discipline: no allocation while the pointer is live.
pub fn first_indexable_field(vm: &Interp, oop: Oop) -> PrimResult<*mut c_void> {
    let p = raw_call!(vm, firstIndexableField(oop.0));
    if p.is_null() {
        return Err(PrimErr::BadArgument);
    }
    Ok(p)
}

/// Raw `integerValueOf`: untags without checking, exactly as the C plugin used
/// it on values it assumed were SmallIntegers (garbage in, garbage out, but no
/// undefined behaviour either way).
pub fn integer_value_raw(vm: &Interp, oop: Oop) -> PrimResult<sqInt> {
    Ok(raw_call!(vm, integerValueOf(oop.0)))
}

/// The interpreter session identifier, truncated to the `int` the SQFile
/// record stores (`SESSIONIDENTIFIERTYPE` in the C).
pub fn this_session_id(vm: &Interp) -> PrimResult<c_int> {
    Ok(raw_call!(vm, getThisSessionID()) as c_int)
}

/// The session identifier at full `sqInt` width, for the record validity
/// comparison the C wrote as `getThisSessionID() == sqFile->sessionID`.
pub fn this_session_id_wide(vm: &Interp) -> PrimResult<sqInt> {
    Ok(raw_call!(vm, getThisSessionID()))
}

// ---------------------------------------------------------------------------
// Runtime symbol resolution
// ---------------------------------------------------------------------------

/// The proxy table pointer, reachable without an `Interp` in hand -- needed by
/// signal handlers and by `extern "C"` exports that the VM calls directly.
///
/// `__private::INTERP` is the same slot the SDK's own `setInterpreter` fills;
/// reading it here keeps a single source of truth instead of a second copy.
fn proxy_ptr() -> *mut pharo_vm_plugin::VirtualMachine {
    pharo_vm_plugin::__private::INTERP.load(Ordering::Acquire)
}

/// `ioLoadFunctionFrom(fnName, moduleName)` through the proxy.
///
/// The C plugin resolved `getProcess*Vector`, `sqFileStdioHandlesInto` and
/// friends at link time; a stand-alone cdylib cannot, so every cross-module
/// symbol goes through here at first use. Never call this from a signal
/// handler: it allocates the C strings.
pub fn io_load_function(fn_name: &str, module_name: &str) -> *mut c_void {
    let vt = proxy_ptr();
    if vt.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: the table was stored by setInterpreter and lives for the
    // process; ioLoadFunctionFrom is the documented lookup entry point.
    let Some(f) = (unsafe { (*vt).ioLoadFunctionFrom }) else {
        return std::ptr::null_mut();
    };
    let (Ok(fn_c), Ok(mod_c)) = (CString::new(fn_name), CString::new(module_name)) else {
        return std::ptr::null_mut();
    };
    unsafe { f(fn_c.as_ptr().cast_mut(), mod_c.as_ptr().cast_mut()) }
}

/// Cached result of an `io_load_function` lookup: untried / missing / found.
///
/// The C plugin cached these pointers in function-local statics; the atomic
/// makes the same idiom sound. A racing double lookup is harmless -- both
/// writers store the same answer.
pub struct CachedFn(AtomicUsize);

const UNTRIED: usize = 0;
const MISSING: usize = 1;

impl CachedFn {
    pub const fn new() -> Self {
        Self(AtomicUsize::new(UNTRIED))
    }

    /// The resolved address, looking it up on first use. Null when the symbol
    /// is genuinely absent.
    pub fn get(&self, fn_name: &str, module_name: &str) -> *mut c_void {
        match self.0.load(Ordering::Acquire) {
            UNTRIED => {
                let p = io_load_function(fn_name, module_name);
                self.0.store(
                    if p.is_null() { MISSING } else { p as usize },
                    Ordering::Release,
                );
                p
            }
            MISSING => std::ptr::null_mut(),
            addr => addr as *mut c_void,
        }
    }
}

// ---------------------------------------------------------------------------
// Strings and collections
// ---------------------------------------------------------------------------

/// `cString:asCollection:` -- a new String/ByteArray of exactly `bytes.len()`
/// indexable bytes, copied from `bytes`.
pub fn collection_from_bytes(vm: &Interp, class: Oop, bytes: &[u8]) -> PrimResult<Oop> {
    let oop = vm.instantiate(class, bytes.len() as sqInt)?;
    vm.write_bytes(oop, 0, bytes)?;
    Ok(oop)
}

/// The `len + 1` variant `argumentAtAsType:` / `environmentAtAsType:` used: a
/// collection one byte longer than the C string, with a trailing NUL byte
/// left in place. The C really did answer strings carrying that extra byte;
/// keep it, the image compensates.
pub fn collection_with_trailing_nul(vm: &Interp, class: Oop, bytes: &[u8]) -> PrimResult<Oop> {
    let oop = vm.instantiate(class, bytes.len() as sqInt + 1)?;
    // A fresh Spur object is zero-filled, so the NUL terminator is already
    // there; only the payload needs writing.
    vm.write_bytes(oop, 0, bytes)?;
    Ok(oop)
}

/// The bytes of a NUL-terminated C string, without the terminator.
///
/// # Safety
///
/// `p` must point at a valid NUL-terminated string.
pub unsafe fn cstr_bytes<'a>(p: *const c_char) -> &'a [u8] {
    unsafe { std::ffi::CStr::from_ptr(p) }.to_bytes()
}

/// `transientCStringFromString:` without the in-image scratch object: copies
/// the byte object's contents into an owned C string.
///
/// The C's `strncpy` stopped at the first NUL, so a Smalltalk string with an
/// embedded NUL was silently truncated there; this reproduces that.
pub fn transient_cstring(vm: &Interp, oop: Oop) -> PrimResult<CString> {
    let bytes = vm.bytes_of(oop)?;
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    CString::new(&bytes[..end]).map_err(|_| PrimErr::BadArgument)
}
