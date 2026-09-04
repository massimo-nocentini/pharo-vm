//! `SurfacePlugin`, in Rust.
//!
//! This plugin is mostly an API for *other plugins*, not for the image: it
//! keeps the process-wide registry of drawable surfaces. BitBltPlugin and the
//! display code fetch its entry points by name through
//! `ioLoadFunctionFrom(..., "SurfacePlugin")`, so everything they expect --
//! `ioRegisterSurface`, `ioUnregisterSurface`, `ioFindSurface` and the
//! `ioGetSurfaceFormat` / `ioLockSurface` / `ioUnlockSurface` /
//! `ioShowSurface` dispatchers -- is exported here as `#[no_mangle] extern
//! "C"` with the exact signatures from
//! `plugins/SurfacePlugin/include/common/SurfacePlugin.h`.
//!
//! The C original is in two halves, both replaced by this crate:
//!
//! * the registry, Slang-generated from
//!   `smalltalksrc/VMMaker/SurfacePlugin.class.st` (see [`registry`]);
//! * the hand-written manual-surface support in
//!   `plugins/SurfacePlugin/src/common/sqManualSurface.c` (see [`manual`]).
//!
//! On top sit six named primitives the image calls directly, chiefly the
//! FFI `ExternalForm`'s create/destroy/set-pointer trio.

// Exported names -- primitives, the surface API, the dispatch-struct fields --
// are fixed by the C header and the image.
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![deny(unsafe_op_in_unsafe_fn)]

mod manual;
mod registry;

use core::ffi::{c_int, c_void};
use core::sync::atomic::Ordering;
use std::sync::Mutex;

use pharo_vm_plugin::poison;
use pharo_vm_plugin::proxy::{sqIntptr_t, usqIntptr_t};
use pharo_vm_plugin::{pharo_plugin, pharo_primitive, Interp, Oop, PrimErr, PrimResult};

use registry::{sqSurfaceDispatch, Registry};

pharo_plugin!("SurfacePlugin", init = initialise, shutdown = shutdown);

/// The registry the whole process shares, as the C's file-static
/// `surfaceArray`/`numSurfaces`/`maxSurfaces` triple was.
///
/// A `Mutex` because a `static` needs `Sync`; contention does not exist -- the
/// VM calls everything here on the interpreter thread (see the `Send`
/// justification in [`registry`]). Client dispatch functions are always
/// invoked *after* the lock is released, so a surface implementation that
/// re-enters the registry (as it legally could under C's bare globals) does
/// not deadlock.
static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

/// Runs `f` with the registry locked, refusing once a panic has torn it.
///
/// Through [`poison::lock`] rather than a plain `lock()`, for both of the
/// reasons that function documents. A surface record is a handle *and* a
/// dispatch table that must agree; a panic between the two writes leaves an
/// ID whose dispatch pointer belongs to some other surface, and every caller
/// below then hands that pointer four out-parameters and calls it. The
/// registry refusing, and the module poisoning, is the only safe answer --
/// recovering the lock would call a stale `sqSurfaceDispatch` entry.
fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> PrimResult<R> {
    let mut guard = poison::lock(&REGISTRY)?;
    Ok(f(&mut guard))
}

/// `initialiseModule`: start from an empty registry.
fn initialise() -> bool {
    with_registry(Registry::reset).is_ok()
}

/// `shutdownModule`: refuse to unload while any surface is registered, as the
/// Slang does.
fn shutdown() -> bool {
    with_registry(|registry| {
        if registry.surface_count() != 0 {
            return false;
        }
        registry.reset();
        true
    })
    // A poisoned registry cannot answer how many surfaces are live, so it
    // cannot say the module is safe to unload: refuse, as for a non-empty one.
    .unwrap_or(false)
}

/// Sets the interpreter's primitive-failure flag, as the generated C's
/// `primitiveFail()` calls on the dispatchers' error paths did.
///
/// These entry points are called by the VM and other plugins directly, not
/// through `#[pharo_primitive]`, so no `Interp` is in scope; the proxy is
/// reached through the pointer the SDK stored in `setInterpreter`. Before
/// that, or on a proxy without the entry, the flag is simply not set --
/// the C would not have been loaded at all in that state.
fn fail_current_primitive() {
    let vt = pharo_vm_plugin::__private::INTERP.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: setInterpreter only ever stores the VM's own process-lifetime
    // proxy table.
    if let Some(primitive_fail) = unsafe { (*vt).primitiveFail } {
        unsafe { primitive_fail() };
    }
}

// ---------------------------------------------------------------------------
// The surface manager API other plugins load by name
// ---------------------------------------------------------------------------

/// Registers a surface; answers true (1) with `*surfaceID` set, false (0)
/// otherwise.
///
/// # Safety
///
/// `dispatch` must be null or point to a `sqSurfaceDispatch` that, together
/// with its function pointers, stays valid until the surface is unregistered;
/// `surfaceID` must be null or writable. Identical to the C contract.
#[no_mangle]
pub unsafe extern "C" fn ioRegisterSurface(
    surfaceHandle: sqIntptr_t,
    dispatch: *mut sqSurfaceDispatch,
    surfaceID: *mut c_int,
) -> c_int {
    // The C wrote through surfaceID unconditionally on success; a null here
    // is a caller bug it turned into a wild write, refused up front instead.
    if surfaceID.is_null() {
        return 0;
    }
    // SAFETY: forwarding the caller's validity promise (see above).
    match with_registry(|r| unsafe { r.register(surfaceHandle as usqIntptr_t, dispatch) }) {
        Ok(Some(id)) => {
            // SAFETY: non-null, caller-writable (checked/promised above).
            unsafe { *surfaceID = id };
            1
        }
        Ok(None) | Err(_) => 0,
    }
}

/// Unregisters a surface; answers true (1) if it existed.
#[no_mangle]
pub extern "C" fn ioUnregisterSurface(surfaceID: c_int) -> c_int {
    c_int::from(with_registry(|r| r.unregister(surfaceID)).unwrap_or(false))
}

/// Finds a surface, optionally insisting on a specific dispatch table, and
/// answers its handle through `surfaceHandle`.
///
/// # Safety
///
/// `surfaceHandle` must be null or writable; `dispatch` is only compared,
/// never dereferenced.
#[no_mangle]
pub unsafe extern "C" fn ioFindSurface(
    surfaceID: c_int,
    dispatch: *mut sqSurfaceDispatch,
    surfaceHandle: *mut sqIntptr_t,
) -> c_int {
    let Ok(Some(handle)) = with_registry(|r| r.find(surfaceID, dispatch)) else {
        return 0;
    };
    // The C dereferenced unconditionally; refuse a null out-pointer instead.
    if surfaceHandle.is_null() {
        return 0;
    }
    // SAFETY: non-null, caller-writable per the contract above.
    unsafe { *surfaceHandle = handle as sqIntptr_t };
    1
}

// ---------------------------------------------------------------------------
// The dispatchers BitBlt and the display code call
// ---------------------------------------------------------------------------
//
// Failure behaviour mirrors the Slang exactly: an unknown surface fails the
// current primitive and answers 0; a registered surface whose table lacks the
// requested function answers -1 without failing (except lockSurface, which the
// Slang treats as mandatory and fails on).
//
// SAFETY (shared): dereferencing the stored dispatch pointer and calling its
// functions relies on the validity `ioRegisterSurface`'s caller promised, the
// same trust the C extended. The pair is copied out and the client called
// after the registry lock is released.

/// Reports a surface's width, height, depth and endianness.
///
/// # Safety
///
/// The four out-pointers must be writable, as in C (the registered client
/// function writes through them).
#[no_mangle]
pub unsafe extern "C" fn ioGetSurfaceFormat(
    surfaceID: c_int,
    width: *mut c_int,
    height: *mut c_int,
    depth: *mut c_int,
    isMSB: *mut c_int,
) -> c_int {
    let Ok(Some((handle, dispatch))) = with_registry(|r| r.dispatch_entry(surfaceID)) else {
        fail_current_primitive();
        return 0;
    };
    // SAFETY: see the section comment.
    let Some(get_format) = (unsafe { &*dispatch }).getSurfaceFormat else {
        return -1;
    };
    // SAFETY: see the section comment.
    unsafe { get_format(handle as sqIntptr_t, width, height, depth, isMSB) }
}

/// Locks a surface's bits, answering a pointer to its virtual origin and
/// storing the row pitch, or 0 on failure.
///
/// # Safety
///
/// `pitch` must be writable, as in C.
#[no_mangle]
pub unsafe extern "C" fn ioLockSurface(
    surfaceID: c_int,
    pitch: *mut c_int,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) -> sqIntptr_t {
    let Ok(Some((handle, dispatch))) = with_registry(|r| r.dispatch_entry(surfaceID)) else {
        fail_current_primitive();
        return 0;
    };
    // SAFETY: see the section comment.
    let Some(lock) = (unsafe { &*dispatch }).lockSurface else {
        fail_current_primitive();
        return 0;
    };
    // SAFETY: see the section comment.
    unsafe { lock(handle as sqIntptr_t, pitch, x, y, w, h) }
}

/// Unlocks a (possibly modified) surface; the rectangle is the dirty region,
/// all zero when unmodified. The return value is ignored by callers.
///
/// # Safety
///
/// Callable from C with any arguments; the registered client function is
/// trusted as in the section comment.
#[no_mangle]
pub unsafe extern "C" fn ioUnlockSurface(
    surfaceID: c_int,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) -> c_int {
    let Ok(Some((handle, dispatch))) = with_registry(|r| r.dispatch_entry(surfaceID)) else {
        fail_current_primitive();
        return 0;
    };
    // SAFETY: see the section comment.
    let Some(unlock) = (unsafe { &*dispatch }).unlockSurface else {
        return -1;
    };
    // SAFETY: see the section comment.
    unsafe { unlock(handle as sqIntptr_t, x, y, w, h) }
}

/// Displays part of a surface on the screen; the VM calls this for deferred
/// display updates.
///
/// # Safety
///
/// Callable from C with any arguments; the registered client function is
/// trusted as in the section comment.
#[no_mangle]
pub unsafe extern "C" fn ioShowSurface(
    surfaceID: c_int,
    x: c_int,
    y: c_int,
    w: c_int,
    h: c_int,
) -> c_int {
    let Ok(Some((handle, dispatch))) = with_registry(|r| r.dispatch_entry(surfaceID)) else {
        fail_current_primitive();
        return 0;
    };
    // SAFETY: see the section comment.
    let Some(show) = (unsafe { &*dispatch }).showSurface else {
        return -1;
    };
    // SAFETY: see the section comment.
    unsafe { show(handle as sqIntptr_t, x, y, w, h) }
}

// ---------------------------------------------------------------------------
// The manual-surface C API (also the primitives' backend)
// ---------------------------------------------------------------------------

/// Creates a manual surface; answers a non-negative surface ID, or -1.
#[no_mangle]
pub extern "C" fn createManualSurface(
    width: c_int,
    height: c_int,
    rowPitch: c_int,
    depth: c_int,
    isMSB: c_int,
) -> c_int {
    with_registry(|r| manual::create_manual_surface_in(r, width, height, rowPitch, depth, isMSB))
        // -1 is the C's "could not create", which is what a refused registry
        // is from the caller's side.
        .unwrap_or(-1)
}

/// Destroys a manual surface. Exactly `ioUnregisterSurface`, as in C -- which
/// also means it "destroys" any surface ID it is handed, and that the record
/// itself is never freed (see [`manual`]).
#[no_mangle]
pub extern "C" fn destroyManualSurface(surfaceID: c_int) -> c_int {
    c_int::from(with_registry(|r| r.unregister(surfaceID)).unwrap_or(false))
}

/// Points a manual surface at a new buffer (or null); answers true (1) on
/// success, false (0) for an unknown or locked surface.
///
/// # Safety
///
/// `ptr` must be null or point to a buffer that remains valid (at
/// `rowPitch * height` bytes) until replaced -- BitBlt will read and write
/// through it. Identical to the C contract.
#[no_mangle]
pub unsafe extern "C" fn setManualSurfacePointer(surfaceID: c_int, ptr: *mut c_void) -> c_int {
    with_registry(|r| manual::set_manual_surface_pointer_in(r, surfaceID, ptr)).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------
//
// Accessor depths: the Slang-generated C (not in this tree) exports one per
// primitive. The values below are derived from each primitive's accessor
// chains -- 0 where only stack values and immediates are read, 1 where an
// argument's contents are reached (`firstIndexableField`, `fetchPointer:`,
// or a possible LargeInteger's bytes) -- and the three manual-surface ones
// match the OpenSmalltalk-generated values.

/// `primitiveCreateManualSurface`: width, height, rowPitch, depth, isMSB ->
/// surface ID.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveCreateManualSurface(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(5)?;
    let width = vm.stack_integer(4)?;
    let height = vm.stack_integer(3)?;
    let row_pitch = vm.stack_integer(2)?;
    let depth = vm.stack_integer(1)?;
    let is_msb = vm.boolean_value(vm.stack_value(0)?)?;
    // booleanValueOf reports a non-Boolean through the failure flag; the C
    // checks it before creating anything, and so must we.
    vm.check_failed()?;

    // The C narrows each sqInt to int at the call; `as` truncates the same way.
    let id = createManualSurface(
        width as c_int,
        height as c_int,
        row_pitch as c_int,
        depth as c_int,
        c_int::from(is_msb),
    );
    if id < 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(id as isize)
}

/// `primitiveDestroyManualSurface`: surfaceID -> receiver.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDestroyManualSurface(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?;
    let id = vm.stack_integer(0)?;
    if destroyManualSurface(id as c_int) == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// `primitiveSetManualSurfacePointer`: surfaceID, pointer (a positive machine
/// integer, usually from an FFI `ExternalAddress asInteger`) -> receiver.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveSetManualSurfacePointer(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(2)?;
    let id = vm.stack_integer(1)?;
    let ptr_oop = vm.stack_value(0)?;
    let ptr = positive_machine_integer_value_of(vm, ptr_oop)?;
    // SAFETY: the image promises the pointer's validity, exactly as it
    // promised it to the C primitive; the plugin only stores it here.
    let ok = unsafe { setManualSurfacePointer(id as c_int, ptr as *mut c_void) };
    if ok == 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// `primitiveFindSurface`: surfaceID, handle-holder ByteArray -> Boolean,
/// with the surface's handle stored into the holder on success.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFindSurface(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(2)?;
    let external_id = vm.stack_integer(1)?;
    let holder = vm.stack_value(0)?;
    vm.check_failed()?;
    if !vm.is_bytes(holder)? {
        return Err(PrimErr::BadArgument);
    }

    let Some(handle) = with_registry(|r| r.find(external_id as c_int, core::ptr::null_mut()))?
    else {
        return Ok(false);
    };
    // The C wrote sizeof(sqIntptr_t) native-endian bytes through
    // firstIndexableField with no size check, trusting the image's holder (its
    // own comment says "ByteArray(4)" -- one word short on 64-bit).
    // write_bytes bounds-checks, so an undersized holder is a clean failure
    // here instead of heap corruption.
    vm.write_bytes(holder, 0, &(handle as sqIntptr_t).to_ne_bytes())?;
    Ok(true)
}

/// `primitiveRegisterSurface`: handle ExternalAddress, dispatch
/// ExternalAddress, ID-holder ByteArray -> Boolean, with the new surface ID
/// stored into the holder on success.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveRegisterSurface(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(3)?;
    let handle_oop = vm.stack_value(2)?;
    let dispatch_oop = vm.stack_value(1)?;
    let holder = vm.stack_value(0)?;
    vm.check_failed()?;

    if !vm.is_bytes(holder)? {
        return Err(PrimErr::BadArgument);
    }
    if vm.byte_size_of(holder)? < 4 {
        return Err(PrimErr::BadArgument);
    }
    let external_address = class_external_address(vm)?;
    if !is_kind_of_class(vm, dispatch_oop, external_address)? {
        return Err(PrimErr::BadArgument);
    }
    if !is_kind_of_class(vm, handle_oop, external_address)? {
        return Err(PrimErr::BadArgument);
    }

    // An ExternalAddress stores the machine pointer as its first word; the
    // Slang reads both with fetchPointer: 0 ofObject:.
    let dispatch = vm.fetch_pointer(0, dispatch_oop)?.0 as *mut sqSurfaceDispatch;
    let handle = vm.fetch_pointer(0, handle_oop)?.0 as usqIntptr_t;

    // SAFETY: the dispatch pointer and its lifetime are the image's promise,
    // passed through unchanged -- the same trust the C placed in it.
    let Some(id) = with_registry(|r| unsafe { r.register(handle, dispatch) })? else {
        return Ok(false);
    };
    // The Slang stores the int through the holder's first indexable field; 4
    // native-endian bytes, matching the >= 4 size check above.
    vm.write_bytes(holder, 0, &id.to_ne_bytes())?;
    Ok(true)
}

/// `primitiveUnregisterSurface`: surfaceID -> Boolean.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveUnregisterSurface(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(1)?;
    let id = vm.stack_integer(0)?;
    vm.check_failed()?;
    with_registry(|r| r.unregister(id as c_int))
}

// ---------------------------------------------------------------------------
// Raw proxy entries the safe Interp does not cover
// ---------------------------------------------------------------------------

/// `positiveMachineIntegerValueOf`: a pointer-wide unsigned value from a
/// SmallInteger or LargePositiveInteger, failing (via the failure flag,
/// checked here) for anything else.
fn positive_machine_integer_value_of(vm: &Interp, oop: Oop) -> PrimResult<usize> {
    let raw = vm.as_raw();
    // SAFETY: proxy table pointer from the VM; entry checked for presence.
    let f = unsafe { (*raw).positiveMachineIntegerValueOf }.ok_or(PrimErr::Unsupported)?;
    // SAFETY: signature fixed by virtualMachine.h.
    let value = unsafe { f(oop.0) };
    vm.check_failed()?;
    Ok(value)
}

/// The class `ExternalAddress`, for the kind checks in
/// `primitiveRegisterSurface`.
fn class_external_address(vm: &Interp) -> PrimResult<Oop> {
    let raw = vm.as_raw();
    // SAFETY: as in positive_machine_integer_value_of.
    let f = unsafe { (*raw).classExternalAddress }.ok_or(PrimErr::Unsupported)?;
    // SAFETY: nullary accessor answering a well-known oop.
    Ok(Oop(unsafe { f() }))
}

/// `isKindOfClass`: instance-of-or-subclass test against a class oop.
fn is_kind_of_class(vm: &Interp, oop: Oop, class: Oop) -> PrimResult<bool> {
    let raw = vm.as_raw();
    // SAFETY: as in positive_machine_integer_value_of.
    let f = unsafe { (*raw).isKindOfClass }.ok_or(PrimErr::Unsupported)?;
    // SAFETY: signature fixed by virtualMachine.h.
    Ok(unsafe { f(oop.0, class.0) } != 0)
}

/// Serialisation for the tests that touch the process-wide [`REGISTRY`].
///
/// Two of them do -- `tests::the_exported_api_end_to_end` here and
/// `registry::tests::a_panic_while_the_surface_registry_is_held_refuses_every_later_lock`
/// -- and the second leaves that registry poisoned for as long as it holds
/// this guard. The harness runs tests in parallel threads within one binary,
/// so "nothing else takes this lock while I have it" is the only thing that
/// keeps the two apart. It recovers its own poison deliberately, as every
/// `#[cfg(test)]` serialisation lock in this tree does: one failing test must
/// not cascade into the other, and no image ever reaches this.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::{Mutex, MutexGuard, PoisonError};

    static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

    pub fn registry_lock() -> MutexGuard<'static, ()> {
        REGISTRY_LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::sqSurfaceDispatch;

    /// A recording client surface for the exported API.
    #[derive(Default)]
    struct Probe {
        lock_args: Option<(c_int, c_int, c_int, c_int)>,
        unlocked: bool,
        shown: Option<(c_int, c_int, c_int, c_int)>,
    }

    unsafe extern "C" fn probe_format(
        _handle: sqIntptr_t,
        width: *mut c_int,
        height: *mut c_int,
        depth: *mut c_int,
        is_msb: *mut c_int,
    ) -> c_int {
        unsafe {
            *width = 320;
            *height = 200;
            *depth = 16;
            *is_msb = 0;
        }
        1
    }

    unsafe extern "C" fn probe_lock(
        handle: sqIntptr_t,
        pitch: *mut c_int,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
    ) -> sqIntptr_t {
        let probe = unsafe { &mut *(handle as *mut Probe) };
        probe.lock_args = Some((x, y, w, h));
        unsafe { *pitch = 640 };
        0x5157
    }

    unsafe extern "C" fn probe_unlock(
        handle: sqIntptr_t,
        _x: c_int,
        _y: c_int,
        _w: c_int,
        _h: c_int,
    ) -> c_int {
        unsafe { &mut *(handle as *mut Probe) }.unlocked = true;
        1
    }

    unsafe extern "C" fn probe_show(
        handle: sqIntptr_t,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
    ) -> c_int {
        unsafe { &mut *(handle as *mut Probe) }.shown = Some((x, y, w, h));
        1
    }

    /// One sequential scenario driving the *exported* C API against the real
    /// process-wide registry. A single test function on purpose: the registry
    /// is global, and parallel test threads would interleave IDs.
    ///
    /// (`fail_current_primitive` is a no-op throughout -- no interpreter has
    /// called `setInterpreter` -- which is exactly the situation these paths
    /// must tolerate.)
    #[test]
    fn the_exported_api_end_to_end() {
        let _serial = crate::testing::registry_lock();
        let mut probe = Probe::default();
        let dispatch = Box::into_raw(Box::new(sqSurfaceDispatch {
            majorVersion: 1,
            minorVersion: 0,
            getSurfaceFormat: Some(probe_format),
            lockSurface: Some(probe_lock),
            unlockSurface: Some(probe_unlock),
            showSurface: Some(probe_show),
        }));
        let handle = &mut probe as *mut Probe as sqIntptr_t;

        // Register: ID written through the out-pointer.
        let mut id: c_int = -1;
        assert_eq!(unsafe { ioRegisterSurface(handle, dispatch, &mut id) }, 1);
        assert_eq!(id, 0);
        // Null dispatch and null out-pointer both refuse.
        assert_eq!(
            unsafe { ioRegisterSurface(handle, core::ptr::null_mut(), &mut id) },
            0
        );
        assert_eq!(
            unsafe { ioRegisterSurface(handle, dispatch, core::ptr::null_mut()) },
            0
        );

        // Find, with and without the dispatch filter.
        let mut found: sqIntptr_t = 0;
        assert_eq!(unsafe { ioFindSurface(id, core::ptr::null_mut(), &mut found) }, 1);
        assert_eq!(found, handle);
        assert_eq!(unsafe { ioFindSurface(id, dispatch, &mut found) }, 1);
        assert_eq!(
            unsafe { ioFindSurface(id, manual::manual_dispatch_ptr(), &mut found) },
            0,
            "wrong dispatch table must not match"
        );
        assert_eq!(unsafe { ioFindSurface(99, core::ptr::null_mut(), &mut found) }, 0);

        // The four dispatchers, arguments passed through verbatim.
        let (mut w, mut h, mut d, mut m) = (0, 0, 0, 0);
        assert_eq!(
            unsafe { ioGetSurfaceFormat(id, &mut w, &mut h, &mut d, &mut m) },
            1
        );
        assert_eq!((w, h, d, m), (320, 200, 16, 0));

        let mut pitch = 0;
        assert_eq!(unsafe { ioLockSurface(id, &mut pitch, 1, 2, 3, 4) }, 0x5157);
        assert_eq!(pitch, 640);
        assert_eq!(probe.lock_args, Some((1, 2, 3, 4)));

        assert_eq!(unsafe { ioUnlockSurface(id, 0, 0, 3, 4) }, 1);
        assert!(probe.unlocked);

        assert_eq!(unsafe { ioShowSurface(id, 9, 8, 7, 6) }, 1);
        assert_eq!(probe.shown, Some((9, 8, 7, 6)));

        // Unknown surface: dispatchers answer 0 (and would fail the primitive).
        assert_eq!(unsafe { ioGetSurfaceFormat(42, &mut w, &mut h, &mut d, &mut m) }, 0);
        assert_eq!(unsafe { ioLockSurface(42, &mut pitch, 0, 0, 0, 0) }, 0);
        assert_eq!(unsafe { ioUnlockSurface(42, 0, 0, 0, 0) }, 0);
        assert_eq!(unsafe { ioShowSurface(42, 0, 0, 0, 0) }, 0);

        // A surface whose table lacks a function: -1, except the mandatory
        // lockSurface, which answers 0 like a failure.
        let bare = Box::into_raw(Box::new(sqSurfaceDispatch {
            majorVersion: 1,
            minorVersion: 0,
            getSurfaceFormat: None,
            lockSurface: None,
            unlockSurface: None,
            showSurface: None,
        }));
        let mut bare_id: c_int = -1;
        assert_eq!(unsafe { ioRegisterSurface(0xB0B, bare, &mut bare_id) }, 1);
        assert_eq!(
            unsafe { ioGetSurfaceFormat(bare_id, &mut w, &mut h, &mut d, &mut m) },
            -1
        );
        assert_eq!(unsafe { ioLockSurface(bare_id, &mut pitch, 0, 0, 0, 0) }, 0);
        assert_eq!(unsafe { ioUnlockSurface(bare_id, 0, 0, 0, 0) }, -1);
        assert_eq!(unsafe { ioShowSurface(bare_id, 0, 0, 0, 0) }, -1);

        // Manual surfaces travel the same registry.
        let manual_id = createManualSurface(4, 4, 16, 32, 1);
        assert!(manual_id > bare_id);
        let mut buffer = [0u8; 64];
        assert_eq!(
            unsafe { setManualSurfacePointer(manual_id, buffer.as_mut_ptr().cast()) },
            1
        );
        assert_eq!(
            unsafe { ioLockSurface(manual_id, &mut pitch, 0, 0, 4, 4) },
            buffer.as_mut_ptr() as sqIntptr_t
        );
        assert_eq!(pitch, 16);
        assert_eq!(unsafe { ioUnlockSurface(manual_id, 0, 0, 4, 4) }, 1);
        assert_eq!(
            unsafe { setManualSurfacePointer(id, core::ptr::null_mut()) },
            0,
            "a foreign surface is not a manual surface"
        );
        assert_eq!(destroyManualSurface(manual_id), 1);
        assert_eq!(destroyManualSurface(manual_id), 0);

        // shutdownModule refuses while surfaces remain, then succeeds.
        assert!(!shutdown());
        assert_eq!(ioUnregisterSurface(id), 1);
        assert_eq!(ioUnregisterSurface(id), 0);
        assert_eq!(ioUnregisterSurface(bare_id), 1);
        assert!(shutdown());
        assert!(initialise());
        assert_eq!(unsafe { ioFindSurface(0, core::ptr::null_mut(), &mut found) }, 0);
    }
}
