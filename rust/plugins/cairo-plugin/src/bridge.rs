//! Lending a live `cairo_t *` to another plugin, for one call.
//!
//! PangoPlugin renders a `PangoLayout` with `pango_cairo_show_layout(cr,
//! layout)`, and the `cr` it needs is one of *ours*: the image's contexts live
//! in [`crate::resources::CONTEXTS`], inside this shared library. A second
//! plugin cannot see that registry -- linking this crate as an rlib would give
//! it a second, empty copy of the statics, which would answer `NotFound` for
//! every handle the image ever got from here -- so the only way across is an
//! exported C entry point, looked up through the VM's own `ioLoadFunctionFrom`.
//!
//! This module is that entry point, and it is deliberately tiny:
//!
//! * it **borrows**. The `cairo_t *` is answered for the duration of one
//!   primitive; ownership never moves, no reference is taken, and the consumer
//!   must not destroy it or keep it. There is no window in which the image
//!   could destroy the context underneath the borrower, because the VM does not
//!   re-enter Smalltalk in the middle of a primitive.
//! * it is **versioned in the symbol name**. An incompatible change becomes a
//!   new `_v2` symbol, and a consumer built against a version this library does
//!   not have simply fails to resolve it -- which surfaces as one primitive
//!   answering `Unsupported`, not as a mismatched call against a struct the
//!   callee reads differently from the caller.
//! * it **publishes which Cairo it is**. `libpangocairo` is linked against a
//!   Cairo of its own, quite possibly not the one this plugin dlopened -- on a
//!   homebrew macOS machine it demonstrably is not. Passing a `cairo_t *`
//!   between two copies of Cairo in one process is undefined behaviour that
//!   will not crash reliably, and [`cairoPluginCairoIdentity_v1`] is what lets
//!   the consumer detect that and refuse rather than corrupt the heap.
//!
//! None of these functions is a primitive: `ioLoadFunctionFrom` looks the name
//! up literally with `dlsym` and asks for no `AccessorDepth` byte, so they are
//! plain `#[no_mangle] extern "C"` functions rather than anything
//! `pharo_primitive` emits. Each contains its whole body in a `catch_unwind`,
//! because a panic crossing back into C -- or into another shared library that
//! was never compiled to unwind -- is undefined behaviour.

use core::ffi::{c_char, c_int, c_void};
use core::panic::AssertUnwindSafe;
use std::ffi::CString;
use std::sync::OnceLock;

use pharo_vm_plugin::sqInt;

use crate::ffi::cairo;
use pharo_vm_plugin::handles::Handle;

use crate::resources::{Context, CONTEXTS};

/// The bridge ABI this library implements.
pub const BRIDGE_ABI: u32 = 1;

/// What [`cairoPluginBorrowContext_v1`] fills in.
///
/// **This struct is the ABI.** It is fixed for the life of the `_v1` symbol: no
/// field may be renamed, reordered, resized or inserted, because the consumer
/// declares it a second time in another crate and nothing links the two
/// declarations together. A new field means a new `_v2` symbol carrying a new
/// struct, so that a consumer holding the old shape fails to resolve rather
/// than reading fields that moved.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CairoBridgeContextV1 {
    /// `size_of::<CairoBridgeContextV1>()`, written by the callee.
    pub size: u32,
    /// [`BRIDGE_ABI`].
    pub abi: u32,
    /// The borrowed `cairo_t *`. Never null when the call answers 1.
    pub cr: *mut c_void,
    /// Same value as [`cairoPluginCairoIdentity_v1`].
    pub cairo_identity: *mut c_void,
    /// `cairo_status(cr)` as of this call. Non-zero means the context has
    /// latched an error and will ignore every drawing call made on it.
    pub status: c_int,
    /// Zero.
    pub reserved: c_int,
}

// The wire format, asserted rather than assumed. A consumer in another crate
// writes this layout out by hand; if padding or a field width ever moved here,
// the two would disagree silently and the borrower would read the `cr` field
// out of the middle of `cairo_identity`.
const _: () = assert!(core::mem::size_of::<CairoBridgeContextV1>() == 32);
const _: () = assert!(core::mem::align_of::<CairoBridgeContextV1>() == 8);

/// The highest bridge ABI this library supports.
///
/// The one bridge symbol with no version in its name, because its meaning is
/// fixed forever. A consumer calls it to explain *why* a versioned symbol was
/// missing -- "this CairoPlugin speaks bridge 1, you asked for 2" reads rather
/// better in a diagnostic than "symbol not found".
#[no_mangle]
pub extern "C" fn cairoPluginBridgeAbiVersion() -> u32 {
    BRIDGE_ABI
}

/// The address of `cairo_create` as this plugin resolved it.
///
/// An identity token, never called through. Two plugins that answer the same
/// address are talking to the same Cairo and may pass a `cairo_t *` between
/// them; two that answer different addresses have two copies of Cairo mapped,
/// whose private statics and internal struct layouts do not agree, and must
/// not. The check costs a comparison and saves a class of corruption that no
/// test in this build could catch.
///
/// Null when Cairo did not load.
#[no_mangle]
pub extern "C" fn cairoPluginCairoIdentity_v1() -> *mut c_void {
    std::panic::catch_unwind(|| match cairo() {
        Ok(c) => c
            .cairo_create
            .map_or(core::ptr::null_mut(), |f| f as *const () as *mut c_void),
        Err(_) => core::ptr::null_mut(),
    })
    .unwrap_or(core::ptr::null_mut())
}

/// The file this plugin loaded Cairo from, for diagnostics. Null if none.
///
/// Static for the life of the process: the `CString` is built once and kept, so
/// the caller may hold the pointer as long as this library is loaded and need
/// not free it. Answering a freshly-allocated string would need a matching
/// `free` entry point, and a cross-library `free` is exactly the sort of thing
/// this module exists to avoid.
#[no_mangle]
pub extern "C" fn cairoPluginCairoPath_v1() -> *const c_char {
    static PATH: OnceLock<Option<CString>> = OnceLock::new();
    std::panic::catch_unwind(|| {
        PATH.get_or_init(|| CString::new(cairo().ok()?.path.clone()).ok())
            .as_ref()
            .map_or(core::ptr::null(), |s| s.as_ptr())
    })
    .unwrap_or(core::ptr::null())
}

/// Answers the `cairo_t *` a live context handle names, for one call.
///
/// `out` must point at a writable [`CairoBridgeContextV1`] and `out_size` must
/// be its size; the struct is written whole or not at all, so the caller need
/// not initialise it first. Answers 1 when it was filled, 0 otherwise -- an
/// unknown or stale handle, a destroyed context, a size the callee does not
/// recognise, or a Cairo that never loaded.
///
/// # The borrow
///
/// **Ownership never transfers.** The pointer is valid for the duration of the
/// caller's primitive and no longer. The caller must not call `cairo_destroy`
/// on it, must not take a reference on it, and must not store it anywhere that
/// outlives the call -- not in a struct, not in a static, not in a handle of
/// its own. If it needs the context again it must ask again, which re-resolves
/// the handle through the registry, so a context destroyed in the meantime
/// answers 0 rather than handing back a freed pointer.
///
/// No `cairo_reference` is taken on purpose. It would trade a use-after-free
/// for a leak plus a surface pin that [`crate::resources`] would then have to
/// account for, and it buys nothing: the VM does not re-enter Smalltalk in the
/// middle of a primitive, so there is no moment at which the image could
/// destroy the context while the borrower holds it.
///
/// # Safety
///
/// `out` must be non-null, writable for `out_size` bytes, and aligned for
/// [`CairoBridgeContextV1`]. Declared `unsafe` because it writes through a
/// pointer the caller supplies -- the exported C symbol is identical either
/// way, and `clippy::not_unsafe_ptr_arg_deref` is deny-by-default here, so the
/// marker is not optional.
#[no_mangle]
pub unsafe extern "C" fn cairoPluginBorrowContext_v1(
    handle: sqInt,
    out: *mut CairoBridgeContextV1,
    out_size: u32,
) -> c_int {
    // AssertUnwindSafe: the captured state is a raw pointer and two integers,
    // and the closure writes through the pointer only on the success path,
    // after every check has passed. There is no partially-updated invariant a
    // panic could expose to the caller.
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        if out.is_null() {
            return 0;
        }
        // The size is an argument rather than only a field so that nothing
        // reads caller memory before writing it: a caller built against a
        // different struct is refused without either side touching the other's
        // uninitialised bytes.
        if out_size as usize != core::mem::size_of::<CairoBridgeContextV1>() {
            return 0;
        }
        let Ok(c) = cairo() else { return 0 };
        let (Some(create), Some(status_of)) = (c.cairo_create, c.cairo_status) else {
            return 0;
        };
        // Resolved every time, never cached. A destroyed context answers
        // NotFound here rather than a dangling pointer, which is the whole
        // point of the registry's generation counter -- and the handle is
        // decoded rather than trusted, because this entry point takes a raw
        // `sqInt` from another shared library, so a value the stack could
        // never carry can arrive here.
        let Ok(cr) = Handle::decode(handle).and_then(|h| CONTEXTS.with(h, Context::as_ptr)) else {
            return 0;
        };
        if cr.is_null() {
            return 0;
        }
        // SAFETY: `cr` is a live context this registry owns and has not
        // released, and `status_of` came out of the same Cairo that created it.
        let status = unsafe { status_of(cr) };
        let filled = CairoBridgeContextV1 {
            size: core::mem::size_of::<CairoBridgeContextV1>() as u32,
            abi: BRIDGE_ABI,
            cr: cr.cast::<c_void>(),
            cairo_identity: create as *const () as *mut c_void,
            status,
            reserved: 0,
        };
        // SAFETY: delegated to this function's contract -- `out` is non-null,
        // writable and aligned, and `out_size` matched the struct's size, so
        // one whole-struct write stays inside the caller's storage.
        unsafe { out.write(filled) };
        1
    }))
    .unwrap_or(0)
}

/// `cairo_status` of the context `handle` names, or -1 when it cannot be asked.
///
/// For a borrower to call *after* drawing. Cairo latches errors and then
/// silently ignores every later call on a broken context, so a
/// `pango_cairo_show_layout` onto one paints nothing and reports nothing;
/// asking afterwards turns that into a primitive failure at the point of the
/// mistake rather than into drawing the image notices has stopped appearing.
///
/// -1 rather than a `cairo_status_t` because every real status is
/// non-negative, so the "cannot ask" case cannot be confused with an answer.
#[no_mangle]
pub extern "C" fn cairoPluginContextStatus_v1(handle: sqInt) -> c_int {
    std::panic::catch_unwind(|| {
        let Ok(c) = cairo() else { return -1 };
        let Some(status_of) = c.cairo_status else {
            return -1;
        };
        let Ok(cr) = Handle::decode(handle).and_then(|h| CONTEXTS.with(h, Context::as_ptr)) else {
            return -1;
        };
        if cr.is_null() {
            return -1;
        }
        // SAFETY: as in `cairoPluginBorrowContext_v1` -- a live context this
        // registry owns, and Cairo's own `cairo_status`.
        unsafe { status_of(cr) }
    })
    .unwrap_or(-1)
}
