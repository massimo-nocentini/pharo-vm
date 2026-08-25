//! The surface registry: `SqueakSurface` slots addressed by integer IDs.
//!
//! This is the Rust counterpart of the Slang-generated half of the C plugin
//! (`smalltalksrc/VMMaker/SurfacePlugin.class.st`): a growable array of
//! `{handle, dispatch}` pairs, where the array index *is* the surface ID that
//! image-side code and other plugins hold on to. The ID allocation behaviour
//! is therefore part of the contract and is replicated exactly:
//!
//! * the array grows by `maxSurfaces * 2 + 10` slots when full, so the first
//!   registration creates 10 slots and ID 0;
//! * a freed slot (dispatch pointer null) is reused, lowest index first;
//! * unregistering does not shift anything, so other IDs stay stable, and a
//!   stale ID that gets reused simply resolves to the new occupant, as in C.
//!
//! The registry itself never calls through the dispatch tables it stores; it
//! only hands the `(handle, dispatch)` pair back to the callers in `lib.rs`,
//! which invoke the client's functions outside the registry lock.

use core::ffi::c_int;
use core::ptr;

use pharo_vm_plugin::proxy::{sqIntptr_t, usqIntptr_t};

/// `fn_getSurfaceFormat` from `SurfacePlugin.h`.
pub type fn_getSurfaceFormat = Option<
    unsafe extern "C" fn(
        surfaceHandle: sqIntptr_t,
        width: *mut c_int,
        height: *mut c_int,
        depth: *mut c_int,
        isMSB: *mut c_int,
    ) -> c_int,
>;

/// `fn_lockSurface` from `SurfacePlugin.h`.
pub type fn_lockSurface = Option<
    unsafe extern "C" fn(
        surfaceHandle: sqIntptr_t,
        pitch: *mut c_int,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
    ) -> sqIntptr_t,
>;

/// `fn_unlockSurface` from `SurfacePlugin.h`.
pub type fn_unlockSurface = Option<
    unsafe extern "C" fn(surfaceHandle: sqIntptr_t, x: c_int, y: c_int, w: c_int, h: c_int) -> c_int,
>;

/// `fn_showSurface` from `SurfacePlugin.h`.
pub type fn_showSurface = Option<
    unsafe extern "C" fn(surfaceHandle: sqIntptr_t, x: c_int, y: c_int, w: c_int, h: c_int) -> c_int,
>;

/// The client-supplied function table, byte-for-byte the C `sqSurfaceDispatch`.
///
/// Clients (other plugins) allocate these and pass a pointer to
/// `ioRegisterSurface`; the layout is ABI, fixed by `SurfacePlugin.h`. Every
/// function slot is nullable -- the Slang code explicitly tolerates a missing
/// `getSurfaceFormat` / `showSurface` / `unlockSurface` by answering -1.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct sqSurfaceDispatch {
    pub majorVersion: c_int,
    pub minorVersion: c_int,
    pub getSurfaceFormat: fn_getSurfaceFormat,
    pub lockSurface: fn_lockSurface,
    pub unlockSurface: fn_unlockSurface,
    pub showSurface: fn_showSurface,
}

/// One `SqueakSurface`: the client's handle plus its dispatch table.
///
/// A null dispatch pointer marks the slot free, exactly as in C -- there is no
/// separate occupancy flag to drift out of sync.
#[derive(Clone, Copy)]
struct Slot {
    handle: usqIntptr_t,
    dispatch: *mut sqSurfaceDispatch,
}

const EMPTY_SLOT: Slot = Slot {
    handle: 0,
    dispatch: ptr::null_mut(),
};

/// The registry: `surfaceArray`, `numSurfaces` and `maxSurfaces` from the
/// Slang code, with `maxSurfaces` carried as `slots.len()`.
pub struct Registry {
    slots: Vec<Slot>,
    /// Live surfaces, i.e. registrations minus unregistrations. Only consulted
    /// for the is-the-array-full test and the shutdown gate, as in C.
    num_surfaces: c_int,
}

// SAFETY: the registry holds raw client pointers, which strips the automatic
// Send. It lives in a process-wide Mutex but is only ever touched from the
// interpreter thread -- the VM calls primitives, module hooks and the exported
// surface API on that one thread. The Mutex is there as the safe way to own
// mutable static state, not because there is real contention.
unsafe impl Send for Registry {}

impl Registry {
    /// The state after `initialiseModule`: no array, no surfaces.
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            num_surfaces: 0,
        }
    }

    /// The occupied slot for `id`, or `None` for an out-of-range ID or a freed
    /// slot.
    ///
    /// The C bounds check is `surfaceID < 0 || surfaceID > maxSurfaces`, which
    /// admits `surfaceID == maxSurfaces` and reads one `SqueakSurface` past the
    /// end of the array. Rejecting that ID removes the out-of-bounds read; no
    /// well-behaved caller can hold it, since it is never handed out.
    fn slot(&self, id: c_int) -> Option<&Slot> {
        if id < 0 {
            return None;
        }
        let slot = self.slots.get(id as usize)?;
        (!slot.dispatch.is_null()).then_some(slot)
    }

    /// `ioRegisterSurface`: stores `{handle, dispatch}` and answers the new ID,
    /// or `None` for a null or version-rejected dispatch table.
    ///
    /// # Safety
    ///
    /// A non-null `dispatch` must point to a readable `sqSurfaceDispatch` (the
    /// version check reads it here), and the caller promises -- exactly as a C
    /// caller of `ioRegisterSurface` does -- that the table and its function
    /// pointers stay valid until the surface is unregistered.
    pub unsafe fn register(
        &mut self,
        handle: usqIntptr_t,
        dispatch: *mut sqSurfaceDispatch,
    ) -> Option<c_int> {
        if dispatch.is_null() {
            return None;
        }
        // C: `if(fn->majorVersion != 1 && fn->minorVersion != 0) return 0;`.
        // Note the `&&`: a 2.0 table passes, a 2.1 table is rejected. Odd, but
        // replicated faithfully -- tightening it would refuse clients the C
        // plugin accepted.
        let (major, minor) = unsafe { ((*dispatch).majorVersion, (*dispatch).minorVersion) };
        if major != 1 && minor != 0 {
            return None;
        }

        let index = if self.num_surfaces as usize == self.slots.len() {
            // Full (or first use): grow to maxSurfaces * 2 + 10, zeroing the
            // new slots; the first fresh slot is the answer.
            let index = self.slots.len();
            self.slots.resize(index * 2 + 10, EMPTY_SLOT);
            index
        } else {
            // Reuse the lowest freed slot. When none is free the C leaves its
            // index at -1 and writes surfaceArray[-1]; that is unreachable
            // (numSurfaces < maxSurfaces implies a free slot) and is a clean
            // failure here rather than undefined behaviour.
            self.slots.iter().position(|s| s.dispatch.is_null())?
        };

        self.slots[index] = Slot { handle, dispatch };
        self.num_surfaces += 1;
        Some(index as c_int)
    }

    /// `ioUnregisterSurface`: frees the slot, keeping every other ID stable.
    pub fn unregister(&mut self, id: c_int) -> bool {
        if self.slot(id).is_none() {
            return false;
        }
        self.slots[id as usize] = EMPTY_SLOT;
        self.num_surfaces -= 1;
        true
    }

    /// `ioFindSurface`: the registered handle, optionally insisting the slot
    /// carries exactly the given dispatch table. A null `filter` matches any.
    pub fn find(&self, id: c_int, filter: *mut sqSurfaceDispatch) -> Option<usqIntptr_t> {
        let slot = self.slot(id)?;
        if !filter.is_null() && filter != slot.dispatch {
            return None;
        }
        Some(slot.handle)
    }

    /// The `(handle, dispatch)` pair for the `ioGetSurfaceFormat` /
    /// `ioLockSurface` / `ioUnlockSurface` / `ioShowSurface` dispatchers.
    ///
    /// The caller invokes the client's function *after* releasing the registry
    /// lock, so a surface implementation may re-enter the registry, as it
    /// could under C's lock-free globals.
    pub fn dispatch_entry(&self, id: c_int) -> Option<(usqIntptr_t, *mut sqSurfaceDispatch)> {
        let slot = self.slot(id)?;
        Some((slot.handle, slot.dispatch))
    }

    /// Live surfaces; `shutdownModule` refuses to unload unless this is zero.
    pub fn surface_count(&self) -> c_int {
        self.num_surfaces
    }

    /// `initialiseModule` / `shutdownModule`: drop the array, forget every ID.
    ///
    /// The C `shutdownModule` frees `surfaceArray` but leaves the pointer and
    /// `maxSurfaces` dangling until the next `initialiseModule`; resetting
    /// both here removes that use-after-free window.
    pub fn reset(&mut self) {
        self.slots = Vec::new();
        self.num_surfaces = 0;
    }

    /// `maxSurfaces`: the allocated slot count, for asserting the C growth
    /// schedule.
    #[cfg(test)]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A dispatch table with no functions; enough for identity and version
    /// checks. Leaked on purpose: registered tables must outlive the registry.
    fn table(major: c_int, minor: c_int) -> *mut sqSurfaceDispatch {
        Box::into_raw(Box::new(sqSurfaceDispatch {
            majorVersion: major,
            minorVersion: minor,
            getSurfaceFormat: None,
            lockSurface: None,
            unlockSurface: None,
            showSurface: None,
        }))
    }

    fn register(reg: &mut Registry, handle: usqIntptr_t, dispatch: *mut sqSurfaceDispatch) -> Option<c_int> {
        // SAFETY: `table` builds real, never-freed dispatch structs.
        unsafe { reg.register(handle, dispatch) }
    }

    #[test]
    fn an_empty_registry_finds_and_unregisters_nothing() {
        let mut reg = Registry::new();
        assert_eq!(reg.find(0, ptr::null_mut()), None);
        assert_eq!(reg.dispatch_entry(0), None);
        assert!(!reg.unregister(0));
        assert_eq!(reg.surface_count(), 0);
    }

    #[test]
    fn ids_start_at_zero_and_the_array_grows_by_the_c_schedule() {
        let mut reg = Registry::new();
        let t = table(1, 0);
        // First registration: grow 0 -> 10, take slot 0.
        assert_eq!(register(&mut reg, 100, t), Some(0));
        assert_eq!(reg.slot_count(), 10);
        for i in 1..10 {
            assert_eq!(register(&mut reg, 100 + i as usqIntptr_t, t), Some(i));
        }
        // Full again: grow 10 -> 30, take slot 10.
        assert_eq!(register(&mut reg, 200, t), Some(10));
        assert_eq!(reg.slot_count(), 30);
        assert_eq!(reg.surface_count(), 11);
    }

    #[test]
    fn a_null_dispatch_is_rejected() {
        let mut reg = Registry::new();
        assert_eq!(register(&mut reg, 1, ptr::null_mut()), None);
        assert_eq!(reg.surface_count(), 0);
    }

    /// The C version gate is `major != 1 && minor != 0`, a conjunction: it
    /// only rejects tables that are wrong in *both* fields.
    #[test]
    fn the_version_gate_replicates_the_c_conjunction() {
        let mut reg = Registry::new();
        assert!(register(&mut reg, 1, table(1, 0)).is_some());
        assert!(register(&mut reg, 2, table(1, 7)).is_some());
        assert!(register(&mut reg, 3, table(2, 0)).is_some()); // odd, but C accepts it
        assert_eq!(register(&mut reg, 4, table(2, 1)), None);
    }

    #[test]
    fn freed_slots_are_reused_lowest_first() {
        let mut reg = Registry::new();
        let t = table(1, 0);
        assert_eq!(register(&mut reg, 10, t), Some(0));
        assert_eq!(register(&mut reg, 11, t), Some(1));
        assert_eq!(register(&mut reg, 12, t), Some(2));

        assert!(reg.unregister(1));
        assert!(reg.unregister(0));
        assert_eq!(reg.surface_count(), 1);

        assert_eq!(register(&mut reg, 20, t), Some(0));
        assert_eq!(register(&mut reg, 21, t), Some(1));
        assert_eq!(register(&mut reg, 22, t), Some(3));
        assert_eq!(reg.surface_count(), 4);
    }

    /// Image-side code can hold on to an ID across a free/reuse cycle; like
    /// the C, the stale ID then resolves to the new occupant.
    #[test]
    fn a_stale_id_resolves_to_the_new_occupant() {
        let mut reg = Registry::new();
        let t = table(1, 0);
        assert_eq!(register(&mut reg, 111, t), Some(0));
        assert!(reg.unregister(0));
        assert_eq!(register(&mut reg, 222, t), Some(0));
        assert_eq!(reg.find(0, ptr::null_mut()), Some(222));
    }

    #[test]
    fn out_of_range_ids_fail_cleanly() {
        let mut reg = Registry::new();
        register(&mut reg, 1, table(1, 0));
        assert_eq!(reg.find(-1, ptr::null_mut()), None);
        // id == maxSurfaces is the C off-by-one that read past the array.
        assert_eq!(reg.find(reg.slot_count() as c_int, ptr::null_mut()), None);
        assert_eq!(reg.find(c_int::MAX, ptr::null_mut()), None);
        assert!(!reg.unregister(reg.slot_count() as c_int));
    }

    #[test]
    fn unregistering_twice_fails_the_second_time() {
        let mut reg = Registry::new();
        let id = register(&mut reg, 5, table(1, 0)).unwrap();
        assert!(reg.unregister(id));
        assert!(!reg.unregister(id));
        assert_eq!(reg.surface_count(), 0);
    }

    #[test]
    fn find_honours_the_dispatch_filter() {
        let mut reg = Registry::new();
        let mine = table(1, 0);
        let foreign = table(1, 0);
        let id = register(&mut reg, 77, mine).unwrap();

        assert_eq!(reg.find(id, ptr::null_mut()), Some(77));
        assert_eq!(reg.find(id, mine), Some(77));
        assert_eq!(reg.find(id, foreign), None);
    }

    #[test]
    fn reset_forgets_everything_and_ids_restart_at_zero() {
        let mut reg = Registry::new();
        let t = table(1, 0);
        register(&mut reg, 1, t);
        register(&mut reg, 2, t);
        reg.reset();
        assert_eq!(reg.surface_count(), 0);
        assert_eq!(reg.slot_count(), 0);
        assert_eq!(reg.find(0, ptr::null_mut()), None);
        assert_eq!(register(&mut reg, 3, t), Some(0));
    }

    // -- calling through a fake dispatch table --------------------------------

    /// A recording surface: the handle passed to the fake functions points at
    /// one of these, so the test can check argument passing end to end.
    #[derive(Default)]
    struct Probe {
        lock_args: Option<(c_int, c_int, c_int, c_int)>,
        unlock_args: Option<(c_int, c_int, c_int, c_int)>,
    }

    unsafe extern "C" fn probe_format(
        handle: sqIntptr_t,
        width: *mut c_int,
        height: *mut c_int,
        depth: *mut c_int,
        is_msb: *mut c_int,
    ) -> c_int {
        assert_ne!(handle, 0);
        unsafe {
            *width = 640;
            *height = 480;
            *depth = 32;
            *is_msb = 1;
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
        unsafe { *pitch = 2560 };
        handle
    }

    unsafe extern "C" fn probe_unlock(
        handle: sqIntptr_t,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
    ) -> c_int {
        let probe = unsafe { &mut *(handle as *mut Probe) };
        probe.unlock_args = Some((x, y, w, h));
        1
    }

    #[test]
    fn dispatch_entry_hands_back_the_pair_the_client_registered() {
        let mut reg = Registry::new();
        let mut probe = Probe::default();
        let dispatch = Box::into_raw(Box::new(sqSurfaceDispatch {
            majorVersion: 1,
            minorVersion: 0,
            getSurfaceFormat: Some(probe_format),
            lockSurface: Some(probe_lock),
            unlockSurface: Some(probe_unlock),
            showSurface: None,
        }));
        let handle = &mut probe as *mut Probe as usqIntptr_t;
        let id = register(&mut reg, handle, dispatch).unwrap();

        let (h, d) = reg.dispatch_entry(id).unwrap();
        assert_eq!(h, handle);
        assert_eq!(d, dispatch);

        // Drive the table the way lib.rs's dispatchers do: copy the pair out,
        // then call. The fake functions verify what arrives.
        let table = unsafe { &*d };
        let (mut w, mut ht, mut dp, mut msb) = (0, 0, 0, 0);
        let ok = unsafe {
            table.getSurfaceFormat.unwrap()(h as sqIntptr_t, &mut w, &mut ht, &mut dp, &mut msb)
        };
        assert_eq!((ok, w, ht, dp, msb), (1, 640, 480, 32, 1));

        let mut pitch = 0;
        let bits = unsafe { table.lockSurface.unwrap()(h as sqIntptr_t, &mut pitch, 3, 5, 7, 9) };
        assert_eq!(bits, handle as sqIntptr_t);
        assert_eq!(pitch, 2560);
        assert_eq!(probe.lock_args, Some((3, 5, 7, 9)));

        let ret = unsafe { table.unlockSurface.unwrap()(h as sqIntptr_t, 0, 0, 0, 0) };
        assert_eq!(ret, 1);
        assert_eq!(probe.unlock_args, Some((0, 0, 0, 0)));

        // showSurface was left null: the caller answers -1 for that, so the
        // registry just reports it as absent.
        assert!(table.showSurface.is_none());
    }
}
