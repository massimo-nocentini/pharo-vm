//! Replaces `src/ffi/worker/workerTask.c` on Unix threaded-FFI builds.
//!
//! Constructors and the (never-called) destructor for [`WorkerTask`], the
//! descriptor the FFI worker's queue carries. Three kinds exist, told apart by
//! `type_`: a `CALLOUT` carries everything `ffi_call` needs, a
//! `CALLBACK_RETURN` carries only the semaphore that identifies a cross-thread
//! callback (null when the callback came from the worker's own thread), and a
//! `WORKER_RELEASE` carries nothing.
//!
//! The struct layout comes from `include/pharovm/ffi/workerTask.h` through
//! `pharo-vm-sys`, not from a restatement here. After this wave only Rust
//! touches the fields -- `workerPrimitives.c` (still C) builds tasks through
//! [`worker_task_new`] and never looks inside one -- but the header remains
//! the single statement of the layout.
//!
//! # Faithful oddities
//!
//! * `workerTask.h` also declares `worker_task_set_main_queue` and
//!   `worker_task_set_queue`; the C never defined them, so neither does this.
//! * Nothing in the tree calls [`worker_task_release`]: tasks are allocated,
//!   queued, executed and then leaked. The C's constructor comment says
//!   freeing is the caller's responsibility, and no caller ever did. The
//!   symbol is exported all the same, because it was.
//!
//! # Divergences, all invisible
//!
//! * The C `malloc`ed and left every unassigned field as heap garbage; here
//!   the unassigned fields are null/zero. No task kind reads a field its
//!   constructor did not set, so no reachable behaviour changes.
//! * The allocator is Rust's, not `malloc`. Sound because allocation and the
//!   (unreachable) free both live in this module -- see the leak note above.

use core::ffi::{c_int, c_void};
use core::ptr;

use pharo_vm_sys::{ffi_cif, CallbackInvocation, WorkerTask, WorkerTaskType};

/// Builds a `CALLOUT` task from the pieces `primitivePerformWorkerCall`
/// gathered: the function, its libffi call interface, the argument vector, the
/// return buffer, and the index of the Smalltalk semaphore to signal when the
/// call has run.
///
/// The pointers are stored, never dereferenced here; the caller keeps them
/// alive until the worker has executed the task (the pinning contract lives in
/// `primitiveCalls.c` and the image).
#[no_mangle]
pub extern "C" fn worker_task_new(
    external_function: *mut c_void,
    cif: *mut ffi_cif,
    parameters: *mut c_void,
    return_holder: *mut c_void,
    semaphore_index: c_int,
) -> *mut WorkerTask {
    Box::into_raw(Box::new(WorkerTask {
        type_: WorkerTaskType::CALLOUT,
        anExternalFunction: external_function,
        cif,
        parametersAddress: parameters,
        returnHolderAddress: return_holder,
        semaphoreIndex: semaphore_index,
        queueHandle: ptr::null_mut(),
        // The C left this one uninitialized; it is never read for a CALLOUT.
        callbackSemaphore: ptr::null_mut(),
    }))
}

/// Builds a `CALLBACK_RETURN` task announcing that `invocation` has finished.
///
/// The task carries the invocation's payload: null when the callback ran on
/// the worker's own thread (the run loop then simply returns), or the platform
/// semaphore a foreign thread is blocked on (the run loop then signals it).
/// See `worker_callback_prepare` in [`crate::worker`] for where the payload is
/// decided.
///
/// # Safety
///
/// `invocation` must point to a live [`CallbackInvocation`].
#[no_mangle]
pub unsafe extern "C" fn worker_task_new_callback(
    invocation: *mut CallbackInvocation,
) -> *mut WorkerTask {
    // Zeroed where the C left malloc garbage in every field it did not
    // assign; see the module docs.
    let mut task: Box<WorkerTask> = Box::new(unsafe { core::mem::zeroed() });
    // SAFETY: `invocation` is live by this function's contract.
    task.callbackSemaphore = unsafe { (*invocation).payload };
    task.type_ = WorkerTaskType::CALLBACK_RETURN;
    Box::into_raw(task)
}

/// Builds the `WORKER_RELEASE` task that tells a worker's run loop to drain
/// its queue and quit.
#[no_mangle]
pub extern "C" fn worker_task_new_release() -> *mut WorkerTask {
    let mut task: Box<WorkerTask> = Box::new(unsafe { core::mem::zeroed() });
    task.callbackSemaphore = ptr::null_mut();
    task.type_ = WorkerTaskType::WORKER_RELEASE;
    Box::into_raw(task)
}

/// Frees a task. Exported for symbol parity; nothing calls it -- see the
/// module docs.
///
/// # Safety
///
/// `task` must come from one of the constructors above and must not be used
/// again.
#[no_mangle]
pub unsafe extern "C" fn worker_task_release(task: *mut WorkerTask) {
    // SAFETY: delegated to the caller.
    unsafe { drop(Box::from_raw(task)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callout_task_carries_what_ffi_call_needs() {
        let function = 0x1000 as *mut c_void;
        let cif = 0x2000 as *mut ffi_cif;
        let parameters = 0x3000 as *mut c_void;
        let return_holder = 0x4000 as *mut c_void;

        let task = worker_task_new(function, cif, parameters, return_holder, 7);
        assert!(!task.is_null());

        // SAFETY: freshly allocated above.
        let seen = unsafe { &*task };
        assert_eq!(seen.type_, WorkerTaskType::CALLOUT);
        assert_eq!(seen.anExternalFunction, function);
        assert_eq!(seen.cif, cif);
        assert_eq!(seen.parametersAddress, parameters);
        assert_eq!(seen.returnHolderAddress, return_holder);
        assert_eq!(seen.semaphoreIndex, 7);
        assert!(seen.queueHandle.is_null());
        assert!(seen.callbackSemaphore.is_null());

        // SAFETY: allocated above, not used again.
        unsafe { worker_task_release(task) };
    }

    #[test]
    fn a_callback_task_carries_the_invocation_payload() {
        let payload = 0x5000 as *mut c_void;
        // SAFETY: an all-zero CallbackInvocation is all pointers, and only
        // `payload` is read.
        let mut invocation: CallbackInvocation = unsafe { core::mem::zeroed() };
        invocation.payload = payload;

        // SAFETY: `invocation` is live for the call.
        let task = unsafe { worker_task_new_callback(&mut invocation) };
        // SAFETY: freshly allocated above.
        let seen = unsafe { &*task };
        assert_eq!(seen.type_, WorkerTaskType::CALLBACK_RETURN);
        assert_eq!(seen.callbackSemaphore, payload);

        // SAFETY: allocated above, not used again.
        unsafe { worker_task_release(task) };
    }

    #[test]
    fn a_release_task_carries_nothing() {
        let task = worker_task_new_release();
        // SAFETY: freshly allocated above.
        let seen = unsafe { &*task };
        assert_eq!(seen.type_, WorkerTaskType::WORKER_RELEASE);
        assert!(seen.callbackSemaphore.is_null());

        // SAFETY: allocated above, not used again.
        unsafe { worker_task_release(task) };
    }
}
