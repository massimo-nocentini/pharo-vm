//! Replaces `src/semaphores/pharoSemaphore.c`.
//!
//! A [`Semaphore`] whose "handle" is not an OS object at all but an *index*
//! into the image's semaphore table. Signalling it hands the index to the
//! interpreter, which wakes the Smalltalk process waiting on that semaphore at
//! the next event check.
//!
//! It exists so that code holding a `Semaphore *` -- the FFI worker and
//! callback machinery in `src/ffi/`, which is still C -- can be handed either
//! a real platform semaphore or an image one and not care which. See
//! [`crate::platform_semaphore`] for the other implementation of the same
//! vtable.
//!
//! # Waiting does nothing, and that is correct
//!
//! `pharo_semaphore_wait` returns 0 without waiting. There is nothing it could
//! wait on: the image's semaphore is scheduled by the VM, and the only thread
//! that could block here is the one that has to keep running for the VM to do
//! that scheduling. The C carried the same comment; the slot is filled purely
//! so the vtable is complete.
//!
//! # The handle is an integer in a pointer
//!
//! `semaphore_index` is stored by casting it to `void *` and cast back on
//! signal. That is what the C did, it is why `handle` is `void *` rather than
//! a union, and it means these handles must never be dereferenced or freed.

use core::ffi::{c_int, c_void};

use pharo_vm_sys::{sqInt, Semaphore};

/// The two interpreter entry points this module needs.
///
/// Both live in the generated interpreter, which is not linked into the
/// unit-test binary, so `cfg(test)` answers from test-controlled state
/// instead. That is also what makes [`pharo_semaphore_signal`] testable: the
/// test can see which index was signalled.
mod interp {
    use pharo_vm_sys::sqInt;

    #[cfg(all(not(test), target_vendor = "apple"))]
    extern "C" {
        /// `signalSemaphoreWithIndex` as C, for the targets where
        /// `sqExternalSemaphores.c` is still the one that defines it.
        ///
        /// Declared here rather than allowlisted in `pharo-vm-sys`: on the
        /// targets where this workspace *exports* that symbol, binding it is
        /// exactly what `ALLOWED_FUNCTIONS` forbids. The signature is
        /// `include/pharovm/common/sq.h`'s.
        fn signalSemaphoreWithIndex(semaIndex: sqInt) -> sqInt;
    }

    /// Queues a signal for the image semaphore at `index`.
    ///
    /// Since wave 11 this is `crate::external_semaphores` -- except on Apple,
    /// where `external_semaphores` is gated out (see `cmake/rust.cmake`) and
    /// the C file still provides the symbol.
    pub fn signal_semaphore_with_index(index: sqInt) {
        #[cfg(test)]
        super::tests::record_signal(index);
        #[cfg(all(not(test), not(target_vendor = "apple")))]
        // SAFETY: takes an index by value and touches only the request table.
        unsafe {
            crate::external_semaphores::signalSemaphoreWithIndex(index);
        }
        #[cfg(all(not(test), target_vendor = "apple"))]
        // SAFETY: same call, resolved to the C definition.
        unsafe {
            signalSemaphoreWithIndex(index);
        }
    }

    /// Whether the last primitive failed.
    pub fn failed() -> sqInt {
        #[cfg(test)]
        return super::tests::failed_flag();
        #[cfg(not(test))]
        // SAFETY: reads interpreter state, takes no arguments.
        unsafe {
            pharo_vm_sys::failed()
        }
    }
}

/// Does nothing and reports success. See the module docs.
///
/// # Safety
///
/// Takes `semaphore` only to fit the vtable slot; it is not read, so any
/// value is accepted.
#[no_mangle]
pub unsafe extern "C" fn pharo_semaphore_wait(_semaphore: *mut Semaphore) -> c_int {
    0
}

/// Signals the image semaphore this one indexes.
///
/// Returns 0, or -1 if the interpreter reports that the signal failed --
/// which it does through the global primitive-failure flag rather than a
/// return value, hence the `failed()` call.
///
/// # Safety
///
/// `semaphore` must point to a live [`Semaphore`] built by
/// [`pharo_semaphore_new`], and this must run on the VM thread, since
/// `signalSemaphoreWithIndex` and `failed` are interpreter state.
#[no_mangle]
pub unsafe extern "C" fn pharo_semaphore_signal(semaphore: *mut Semaphore) -> c_int {
    // SAFETY: delegated to the caller. `handle` holds an index that was cast
    // to a pointer, never a pointer that can be dereferenced.
    let index = unsafe { (*semaphore).handle as sqInt };
    interp::signal_semaphore_with_index(index);
    if interp::failed() != 0 {
        -1
    } else {
        0
    }
}

/// Frees the wrapper. There is no OS object to release: the handle is an
/// index.
///
/// # Safety
///
/// `semaphore` must come from [`pharo_semaphore_new`] and must not be used
/// again.
#[no_mangle]
pub unsafe extern "C" fn pharo_semaphore_free(semaphore: *mut Semaphore) {
    // SAFETY: boxed by pharo_semaphore_new; C only ever frees a Semaphore
    // through this vtable slot, so allocation and release stay in this module.
    unsafe { drop(Box::from_raw(semaphore)) }
}

/// Allocates a [`Semaphore`] that signals the image semaphore at
/// `semaphore_index`.
///
/// The C did not check `malloc` and would have written through the null; a
/// failed `Box` allocation aborts instead, which is the defined spelling of
/// the same out-of-memory death.
#[no_mangle]
pub extern "C" fn pharo_semaphore_new(semaphore_index: sqInt) -> *mut Semaphore {
    Box::into_raw(Box::new(Semaphore {
        handle: semaphore_index as *mut c_void,
        wait: Some(pharo_semaphore_wait),
        signal: Some(pharo_semaphore_signal),
        free: Some(pharo_semaphore_free),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    thread_local! {
        /// Indices passed to `signalSemaphoreWithIndex`, in order.
        static SIGNALLED: RefCell<Vec<sqInt>> = const { RefCell::new(Vec::new()) };
        /// What `failed()` should answer.
        static FAILED: Cell<sqInt> = const { Cell::new(0) };
    }

    /// Answers [`super::interp::signal_semaphore_with_index`] during tests.
    pub(super) fn record_signal(index: sqInt) {
        SIGNALLED.with(|s| s.borrow_mut().push(index));
    }

    /// Answers [`super::interp::failed`] during tests.
    pub(super) fn failed_flag() -> sqInt {
        FAILED.with(Cell::get)
    }

    fn take_signalled() -> Vec<sqInt> {
        SIGNALLED.with(|s| core::mem::take(&mut *s.borrow_mut()))
    }

    #[test]
    fn a_new_semaphore_carries_its_index_and_a_full_vtable() {
        let index: sqInt = 42;
        let s = pharo_semaphore_new(index);
        assert!(!s.is_null());

        // SAFETY: s was just allocated and is freed at the end of the test.
        unsafe {
            assert_eq!((*s).handle as sqInt, index, "the index is the handle");
            assert!((*s).wait.is_some());
            assert!((*s).signal.is_some());
            assert!((*s).free.is_some());

            // Waiting is the do-nothing slot; calling it through the vtable is
            // what the FFI worker does, so check it that way.
            assert_eq!((*s).wait.unwrap()(s), 0);

            (*s).free.unwrap()(s);
        }
    }

    #[test]
    fn a_zero_index_round_trips() {
        // Index 0 is a real index, and it is also the null pointer once cast
        // into `handle`. Nothing may treat that as "no semaphore".
        let s = pharo_semaphore_new(0);
        assert!(!s.is_null());
        // SAFETY: as above.
        unsafe {
            assert!((*s).handle.is_null());
            assert_eq!((*s).handle as sqInt, 0);
            pharo_semaphore_free(s);
        }
    }

    #[test]
    fn large_indices_survive_the_pointer_round_trip() {
        // The handle is a pointer, so an index wider than a pointer would be
        // silently truncated. sqInt is pointer-sized, so it cannot be -- this
        // pins that.
        for index in [1 as sqInt, 0x7FFF, sqInt::MAX, -1] {
            let s = pharo_semaphore_new(index);
            // SAFETY: as above.
            unsafe {
                assert_eq!((*s).handle as sqInt, index);
                pharo_semaphore_free(s);
            }
        }
    }

    #[test]
    fn signalling_passes_the_index_through_and_reports_success() {
        let _ = take_signalled();
        FAILED.with(|f| f.set(0));

        let s = pharo_semaphore_new(7);
        // SAFETY: s was just allocated; freed below.
        unsafe {
            assert_eq!((*s).signal.unwrap()(s), 0);
            pharo_semaphore_free(s);
        }

        assert_eq!(take_signalled(), vec![7]);
    }

    #[test]
    fn signalling_reports_minus_one_when_the_primitive_failed() {
        let _ = take_signalled();
        FAILED.with(|f| f.set(1));

        let s = pharo_semaphore_new(9);
        // SAFETY: as above.
        unsafe {
            // The signal is still issued -- the C checked `failed()` after
            // calling, not before -- and only the reported result changes.
            assert_eq!(pharo_semaphore_signal(s), -1);
            pharo_semaphore_free(s);
        }

        assert_eq!(take_signalled(), vec![9]);
        FAILED.with(|f| f.set(0));
    }
}
