//! Manual surfaces: surfaces backed by memory the *image* manages.
//!
//! Port of `plugins/SurfacePlugin/src/common/sqManualSurface.c`. A manual
//! surface is created empty; Smalltalk code (the FFI `ExternalForm`) later
//! points it at a buffer with `setManualSurfacePointer`, possibly repeatedly,
//! possibly at null -- while the pointer is null, lock attempts simply fail
//! and BitBlt draws nothing.
//!
//! The surface record lives on the Rust heap and is entered into the shared
//! registry under this module's own dispatch table, so BitBlt reaches it
//! through the exact same `ioLockSurface`/`ioUnlockSurface` path as any
//! OS-provided surface.

use core::ffi::{c_int, c_void};
use core::ptr;

use pharo_vm_plugin::proxy::{sqIntptr_t, usqIntptr_t};

use crate::registry::{sqSurfaceDispatch, Registry};

/// The C `struct ManualSurface`, with the lock flag as a `bool`; the record
/// never crosses the FFI boundary as a struct (only its address travels, as an
/// opaque handle), so the exact C layout is not ABI here.
pub struct ManualSurface {
    width: c_int,
    height: c_int,
    row_pitch: c_int,
    depth: c_int,
    is_msb: c_int,
    ptr: *mut c_void,
    is_locked: bool,
}

impl ManualSurface {
    /// Validates and builds a surface, or `None` for the parameter
    /// combinations `createManualSurface` answers -1 for.
    ///
    /// The checks and their order are the C's: width, height, pitch, depth --
    /// so a pitch that is too small is reported even when the depth is also
    /// out of range. The pitch comparison is widened to i64 because the C's
    /// `(width*depth)/8` overflows `int` for large widths, which is undefined
    /// behaviour there and a plain comparison here.
    pub fn new(
        width: c_int,
        height: c_int,
        row_pitch: c_int,
        depth: c_int,
        is_msb: c_int,
    ) -> Option<Self> {
        if width < 0 || height < 0 {
            return None;
        }
        if i64::from(row_pitch) < i64::from(width) * i64::from(depth) / 8 {
            return None;
        }
        if !(1..=32).contains(&depth) {
            return None;
        }
        Some(Self {
            width,
            height,
            row_pitch,
            depth,
            is_msb,
            ptr: ptr::null_mut(),
            is_locked: false,
        })
    }

    /// `(width, height, depth, isMSB)`, the fields `getSurfaceFormat` reports.
    pub fn format(&self) -> (c_int, c_int, c_int, c_int) {
        (self.width, self.height, self.depth, self.is_msb)
    }

    /// Locks the surface, answering its bits pointer and row pitch.
    ///
    /// Fails (leaving the lock as it found it) if already locked, and fails if
    /// no buffer has been supplied yet -- both exactly as the C.
    pub fn lock(&mut self) -> Option<(*mut c_void, c_int)> {
        let was_locked = self.is_locked;
        self.is_locked = true;
        if was_locked {
            return None;
        }
        if self.ptr.is_null() {
            self.is_locked = false;
            return None;
        }
        Some((self.ptr, self.row_pitch))
    }

    /// Unlocks unconditionally; the C returns success even for an unlocked
    /// surface.
    pub fn unlock(&mut self) {
        self.is_locked = false;
    }

    /// Points the surface at a new buffer (or null). Refused while locked,
    /// because BitBlt is then holding the old pointer.
    pub fn set_pointer(&mut self, ptr: *mut c_void) -> bool {
        if self.is_locked {
            return false;
        }
        self.ptr = ptr;
        true
    }

    #[cfg(test)]
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }
}

// -- the dispatch table BitBlt sees ------------------------------------------
//
// Each function receives the surface's address back as the opaque handle it
// was registered under.
//
// SAFETY (shared by all four): the handle was produced by
// `create_manual_surface_in` from `Box::into_raw` and is never freed (see the
// leak note there), so it points to a live ManualSurface for the life of the
// process; calls arrive only on the interpreter thread, so the short-lived
// &mut cannot alias another.

unsafe extern "C" fn manual_get_format(
    handle: sqIntptr_t,
    width: *mut c_int,
    height: *mut c_int,
    depth: *mut c_int,
    is_msb: *mut c_int,
) -> c_int {
    let surface = unsafe { &*(handle as *const ManualSurface) };
    let (w, h, d, m) = surface.format();
    unsafe {
        *width = w;
        *height = h;
        *depth = d;
        *is_msb = m;
    }
    1
}

unsafe extern "C" fn manual_lock(
    handle: sqIntptr_t,
    pitch: *mut c_int,
    _x: c_int,
    _y: c_int,
    _w: c_int,
    _h: c_int,
) -> sqIntptr_t {
    let surface = unsafe { &mut *(handle as *mut ManualSurface) };
    match surface.lock() {
        Some((bits, row_pitch)) => {
            unsafe { *pitch = row_pitch };
            bits as sqIntptr_t
        }
        None => 0,
    }
}

unsafe extern "C" fn manual_unlock(
    handle: sqIntptr_t,
    _x: c_int,
    _y: c_int,
    _w: c_int,
    _h: c_int,
) -> c_int {
    let surface = unsafe { &mut *(handle as *mut ManualSurface) };
    surface.unlock();
    1
}

/// A manual surface is not a display; showing it is unsupported, as in C.
unsafe extern "C" fn manual_show(
    _handle: sqIntptr_t,
    _x: c_int,
    _y: c_int,
    _w: c_int,
    _h: c_int,
) -> c_int {
    0
}

/// The one dispatch table shared by every manual surface -- the counterpart of
/// the C's static `manualSurfaceDispatch`.
static MANUAL_SURFACE_DISPATCH: sqSurfaceDispatch = sqSurfaceDispatch {
    majorVersion: 1,
    minorVersion: 0,
    getSurfaceFormat: Some(manual_get_format),
    lockSurface: Some(manual_lock),
    unlockSurface: Some(manual_unlock),
    showSurface: Some(manual_show),
};

/// The registry stores dispatch tables as `*mut` because clients pass them
/// that way; ours is a static that nothing ever writes through, so the
/// `cast_mut` never materialises a mutable access.
pub fn manual_dispatch_ptr() -> *mut sqSurfaceDispatch {
    ptr::addr_of!(MANUAL_SURFACE_DISPATCH).cast_mut()
}

/// `createManualSurface`: answers a non-negative surface ID, or -1.
///
/// The record is `Box::into_raw`ed and -- matching the C, whose
/// `destroyManualSurface` never frees it -- deliberately never reclaimed:
/// `destroy` cannot prove the slot still holds *this* record rather than a
/// reused ID's foreign surface, so freeing would risk a dangling handle. The
/// leak is one small struct per created surface, exactly as in C.
pub fn create_manual_surface_in(
    registry: &mut Registry,
    width: c_int,
    height: c_int,
    row_pitch: c_int,
    depth: c_int,
    is_msb: c_int,
) -> c_int {
    let Some(surface) = ManualSurface::new(width, height, row_pitch, depth, is_msb) else {
        return -1;
    };
    let handle = Box::into_raw(Box::new(surface));
    // SAFETY: the dispatch table is our own static, valid forever; the handle
    // outlives the registration (see the leak note above).
    match unsafe { registry.register(handle as usqIntptr_t, manual_dispatch_ptr()) } {
        Some(id) => id,
        None => {
            // Not registered, so nothing else holds the pointer: reclaim it.
            drop(unsafe { Box::from_raw(handle) });
            -1
        }
    }
}

/// `setManualSurfacePointer`: answers true (1) on success.
///
/// Divergence from C: the lookup passes the manual dispatch table as the
/// filter, where the C passed NULL and then cast whatever handle came back to
/// `ManualSurface*`. Handing this primitive the ID of a *non-manual* surface
/// therefore answers false here, where the C reinterpreted a foreign handle
/// (type confusion, and a wild write through `surface->ptr`).
pub fn set_manual_surface_pointer_in(registry: &Registry, id: c_int, ptr: *mut c_void) -> c_int {
    let Some(handle) = registry.find(id, manual_dispatch_ptr()) else {
        return 0;
    };
    // SAFETY: the filter proved the handle was registered by
    // `create_manual_surface_in`, so it is a live, never-freed ManualSurface.
    let surface = unsafe { &mut *(handle as *mut ManualSurface) };
    c_int::from(surface.set_pointer(ptr))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_validates_like_the_c() {
        assert!(ManualSurface::new(0, 0, 0, 1, 0).is_some());
        assert!(ManualSurface::new(100, 100, 400, 32, 1).is_some());
        // width * depth / 8 == pitch exactly is accepted.
        assert!(ManualSurface::new(10, 10, 40, 32, 0).is_some());

        assert!(ManualSurface::new(-1, 10, 40, 32, 0).is_none(), "negative width");
        assert!(ManualSurface::new(10, -1, 40, 32, 0).is_none(), "negative height");
        assert!(ManualSurface::new(10, 10, 39, 32, 0).is_none(), "pitch too small");
        assert!(ManualSurface::new(10, 10, 40, 0, 0).is_none(), "depth 0");
        assert!(ManualSurface::new(10, 10, 400, 33, 0).is_none(), "depth 33");
        assert!(ManualSurface::new(10, 10, 0, -8, 0).is_none(), "negative depth");
        // The C computed (width*depth)/8 in int, overflowing for this pair;
        // the widened comparison just rejects the pitch.
        assert!(ManualSurface::new(c_int::MAX, 1, c_int::MAX, 32, 0).is_none());
    }

    #[test]
    fn a_surface_without_a_buffer_refuses_to_lock() {
        let mut s = ManualSurface::new(4, 4, 16, 32, 0).unwrap();
        assert_eq!(s.lock(), None);
        // The failed attempt must not leave it locked.
        assert!(!s.is_locked());
    }

    #[test]
    fn lock_answers_the_buffer_and_pitch_and_is_not_reentrant() {
        let mut s = ManualSurface::new(4, 4, 16, 32, 0).unwrap();
        let mut buffer = [0u8; 16 * 4];
        let bits = buffer.as_mut_ptr().cast::<c_void>();
        assert!(s.set_pointer(bits));

        assert_eq!(s.lock(), Some((bits, 16)));
        assert_eq!(s.lock(), None, "double lock fails");
        // The C leaves the surface locked after a failed re-lock (isLocked was
        // set before the wasLocked test and never cleared on that path).
        assert!(s.is_locked());

        s.unlock();
        assert_eq!(s.lock(), Some((bits, 16)));
    }

    #[test]
    fn the_pointer_cannot_change_while_locked() {
        let mut s = ManualSurface::new(4, 4, 16, 32, 0).unwrap();
        let mut buffer = [0u8; 16 * 4];
        let bits = buffer.as_mut_ptr().cast::<c_void>();
        assert!(s.set_pointer(bits));
        s.lock().unwrap();

        assert!(!s.set_pointer(ptr::null_mut()));
        s.unlock();
        assert!(s.set_pointer(ptr::null_mut()));
        assert_eq!(s.lock(), None, "null pointer again refuses to lock");
    }

    #[test]
    fn format_reports_what_was_created() {
        let s = ManualSurface::new(31, 17, 128, 16, 1).unwrap();
        assert_eq!(s.format(), (31, 17, 16, 1));
    }

    #[test]
    fn create_registers_and_destroy_unregisters() {
        let mut reg = Registry::new();
        let id = create_manual_surface_in(&mut reg, 8, 8, 32, 32, 1);
        assert!(id >= 0);
        assert_eq!(reg.surface_count(), 1);

        // Reachable through the generic surface path, like any client surface.
        let (handle, dispatch) = reg.dispatch_entry(id).unwrap();
        assert_eq!(dispatch, manual_dispatch_ptr());
        let (mut w, mut h, mut d, mut m) = (0, 0, 0, 0);
        let ok = unsafe {
            (*dispatch).getSurfaceFormat.unwrap()(
                handle as sqIntptr_t,
                &mut w,
                &mut h,
                &mut d,
                &mut m,
            )
        };
        assert_eq!((ok, w, h, d, m), (1, 8, 8, 32, 1));

        // destroyManualSurface is exactly ioUnregisterSurface.
        assert!(reg.unregister(id));
        assert_eq!(reg.surface_count(), 0);
        assert_eq!(reg.find(id, ptr::null_mut()), None);
    }

    #[test]
    fn invalid_parameters_do_not_touch_the_registry() {
        let mut reg = Registry::new();
        assert_eq!(create_manual_surface_in(&mut reg, -1, 8, 32, 32, 1), -1);
        assert_eq!(reg.surface_count(), 0);
        assert_eq!(reg.slot_count(), 0);
    }

    #[test]
    fn set_pointer_goes_through_the_registry() {
        let mut reg = Registry::new();
        let id = create_manual_surface_in(&mut reg, 4, 4, 16, 32, 0);
        let mut buffer = [0u8; 64];
        let bits = buffer.as_mut_ptr().cast::<c_void>();

        assert_eq!(set_manual_surface_pointer_in(&reg, id, bits), 1);
        assert_eq!(set_manual_surface_pointer_in(&reg, id + 1, bits), 0, "unknown id");

        // Lock through the dispatch table, then refuse the pointer change.
        let (handle, dispatch) = reg.dispatch_entry(id).unwrap();
        let mut pitch = 0;
        let locked = unsafe {
            (*dispatch).lockSurface.unwrap()(handle as sqIntptr_t, &mut pitch, 0, 0, 4, 4)
        };
        assert_eq!(locked, bits as sqIntptr_t);
        assert_eq!(pitch, 16);
        assert_eq!(set_manual_surface_pointer_in(&reg, id, ptr::null_mut()), 0);
    }

    /// A non-manual surface's ID must be refused, not reinterpreted (this is
    /// the documented divergence from the C's NULL filter).
    #[test]
    fn set_pointer_refuses_a_foreign_surface() {
        let mut reg = Registry::new();
        let foreign = Box::into_raw(Box::new(sqSurfaceDispatch {
            majorVersion: 1,
            minorVersion: 0,
            getSurfaceFormat: None,
            lockSurface: None,
            unlockSurface: None,
            showSurface: None,
        }));
        let id = unsafe { reg.register(12345, foreign) }.unwrap();
        assert_eq!(set_manual_surface_pointer_in(&reg, id, ptr::null_mut()), 0);
    }
}
