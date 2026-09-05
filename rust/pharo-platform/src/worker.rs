//! Replaces `src/ffi/worker/worker.c` on Unix threaded-FFI builds.
//!
//! The threaded-FFI worker: a (usually detached) thread that takes
//! [`WorkerTask`]s from a queue and executes them, so that a C callout never
//! blocks the interpreter. The image talks to it through
//! `workerPrimitives.c`, still C; callbacks re-enter it through the
//! [`Runner`] vtable embedded at the start of the [`Worker`], read and called
//! by `callbacks.c` / `callbackPrimitives.c`, also still C.
//!
//! A callback pauses the consumption of tasks: `worker_enter_callback` either
//! runs the loop re-entrantly (callback from the worker's own thread, i.e.
//! from inside a callout) or blocks on a semaphore (callback from a foreign
//! thread), and the matching `CALLBACK_RETURN` task ends whichever of the two
//! was waiting.
//!
//! # The C seam
//!
//! The queue, the platform semaphore and `signalSemaphoreWithIndex` are
//! reached by linkage, exactly as the C reached them: on non-Apple Unix the
//! symbols resolve to this crate's own exports
//! ([`crate::thread_safe_queue`], [`crate::platform_semaphore`],
//! [`crate::external_semaphores`]), on Apple to the C implementations CMake
//! keeps building there. They are declared module-locally rather than bound in
//! `pharo-vm-sys` because a symbol this workspace exports on any target must
//! never be allowlisted. Under `cfg(test)` the seam swaps in Rust doubles so
//! the unit tests need neither the C nor a live interpreter; the queue and
//! semaphore doubles are real (tests genuinely block and wake), while
//! `ffi_call` and `signalSemaphoreWithIndex` record their arguments.
//!
//! # Faithful oddities
//!
//! * `nestedRuns` is incremented on every entry to [`worker_run`] but the
//!   `CALLBACK_RETURN` early return skips the decrement. After the first
//!   nested run the count can therefore never get back to zero, and the
//!   worker's teardown branch -- free the queue, free the worker -- becomes
//!   unreachable. Kept, leak and all: releasing a worker is rare enough that
//!   nothing has ever observed it.
//! * `WORKER_RELEASE` sleeps a literal second, "in case we need to receive a
//!   callback_return message".
//! * Executed tasks are never freed (see [`crate::worker_task`]).
//! * A failed `pthread_create` answers null and leaks the half-built worker,
//!   queue included, as the C did.
//! * A null task from the queue while not quitting produces
//!   `perror("No callbacks in the queue")` and another blocking take.
//! * The C included `dispatch/dispatch.h` on Apple and used nothing from it.
//!
//! # Divergences, all invisible
//!
//! * The Worker is allocated zero-initialized where the C's `malloc` left
//!   `threadId` and `selfThread` as garbage (its comment explains it could
//!   not portably assign them). Both are written before any read on every
//!   path the C could survive.
//! * The allocator is Rust's: `worker.h` never defines `struct __Worker`, so
//!   no C knows its size, and allocation and free both live here.

use core::ffi::{c_int, c_void};
use core::ptr;

use pharo_vm_sys::{sqInt, CallbackInvocation, Runner, Semaphore, WorkerTask, WorkerTaskType};

use crate::logging::{self, site, LOG_ERROR, LOG_INFO};
use crate::worker_task::{worker_task_new_callback, worker_task_new_release};

/// The worker. Opaque to C -- `worker.h` declares `struct __Worker` and never
/// defines it -- with one exception that is the whole ABI: the [`Runner`] must
/// stay the **first** field. `callbacks.c` and the primitives pass a
/// `Worker *` where a `Runner *` is expected and cast back, so a pointer to a
/// `Worker` must be a pointer to its `Runner`.
#[repr(C)]
pub struct Worker {
    /// The callback vtable C calls through. First; see above.
    runner: Runner,
    /// Set by the `WORKER_RELEASE` task; only ever touched from the worker's
    /// own thread, as in the C.
    has_to_quit: c_int,
    /// How many [`worker_run`] frames are on this worker's stack. See the
    /// faithful oddity about the missing decrement.
    nested_runs: c_int,
    /// The spawned thread, when there is one. The C never read it back after
    /// `pthread_detach`.
    thread_id: libc::pthread_t,
    /// The task queue. A `TSQueue *` in the C; opaque here because the queue
    /// is only ever touched through the seam.
    task_queue: *mut c_void,
    /// The thread currently inside [`worker_run`], so callbacks can tell
    /// whether they arrived on it. Written at every `worker_run` entry.
    self_thread: libc::pthread_t,
    /// Never used beyond initialization, in the C as here.
    next: *mut Worker,
}

/// What the worker links against; see "The C seam" in the module docs.
#[cfg(not(test))]
mod seam {
    use core::ffi::{c_int, c_void};

    use pharo_vm_sys::{ffi_cif, sqInt, Semaphore};

    extern "C" {
        fn threadsafe_queue_new(semaphore: *mut Semaphore) -> *mut c_void;
        fn threadsafe_queue_put(queue: *mut c_void, element: *mut c_void);
        fn threadsafe_queue_take(queue: *mut c_void) -> *mut c_void;
        fn threadsafe_queue_size(queue: *mut c_void) -> c_int;
        fn threadsafe_queue_free(queue: *mut c_void);
        fn platform_semaphore_new(initial_value: c_int) -> *mut Semaphore;
        fn signalSemaphoreWithIndex(index: sqInt) -> sqInt;
    }

    /// `threadsafe_queue_new`. The semaphore counts the queue's elements.
    pub(super) unsafe fn queue_new(semaphore: *mut Semaphore) -> *mut c_void {
        // SAFETY: delegated to the caller.
        unsafe { threadsafe_queue_new(semaphore) }
    }

    /// `threadsafe_queue_put`.
    pub(super) unsafe fn queue_put(queue: *mut c_void, element: *mut c_void) {
        // SAFETY: delegated to the caller.
        unsafe { threadsafe_queue_put(queue, element) }
    }

    /// `threadsafe_queue_take`; blocks until there is an element.
    pub(super) unsafe fn queue_take(queue: *mut c_void) -> *mut c_void {
        // SAFETY: delegated to the caller.
        unsafe { threadsafe_queue_take(queue) }
    }

    /// `threadsafe_queue_size`.
    pub(super) unsafe fn queue_size(queue: *mut c_void) -> c_int {
        // SAFETY: delegated to the caller.
        unsafe { threadsafe_queue_size(queue) }
    }

    /// `threadsafe_queue_free`. The element semaphore is not freed with it;
    /// the C leaked it here and so does this.
    pub(super) unsafe fn queue_free(queue: *mut c_void) {
        // SAFETY: delegated to the caller.
        unsafe { threadsafe_queue_free(queue) }
    }

    /// `platform_semaphore_new`: a counting semaphore behind the [`Semaphore`]
    /// vtable.
    pub(super) fn semaphore_new(initial_value: c_int) -> *mut Semaphore {
        // SAFETY: takes no pointers.
        unsafe { platform_semaphore_new(initial_value) }
    }

    /// `signalSemaphoreWithIndex`: wakes the Smalltalk process waiting on the
    /// external semaphore at `index`.
    pub(super) unsafe fn signal_semaphore_with_index(index: sqInt) -> sqInt {
        // SAFETY: delegated to the caller.
        unsafe { signalSemaphoreWithIndex(index) }
    }

    /// `ffi_call`: the callout itself.
    pub(super) unsafe fn call(
        cif: *mut ffi_cif,
        function: *mut c_void,
        return_holder: *mut c_void,
        parameters: *mut *mut c_void,
    ) {
        // SAFETY: delegated to the caller; the transmute reshapes a data
        // pointer into libffi's `void (*)(void)`, which is what the C's
        // implicit conversion did.
        unsafe {
            let function: Option<unsafe extern "C" fn()> = core::mem::transmute(function);
            pharo_vm_sys::ffi_call(cif, function, return_holder, parameters);
        }
    }
}

/// Test doubles for the seam; see "The C seam" in the module docs.
#[cfg(test)]
mod seam {
    use core::ffi::{c_int, c_void};
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex};

    use pharo_vm_sys::{ffi_cif, sqInt, Semaphore};

    /// A counting semaphore satisfying the [`Semaphore`] vtable, so the
    /// production code's vtable calls work unchanged on it.
    struct TestSemaphore {
        /// First, so a `*mut Semaphore` is a `*mut TestSemaphore`.
        ///
        /// Read only through that cast, which `dead_code` cannot see, so it
        /// says the field is never read. It is the whole point of the struct.
        #[allow(dead_code)]
        vtable: Semaphore,
        count: Mutex<i32>,
        woken: Condvar,
    }

    extern "C" fn test_semaphore_wait(semaphore: *mut Semaphore) -> c_int {
        // SAFETY: every Semaphore handed out by this module heads a live
        // TestSemaphore.
        let sem = unsafe { &*(semaphore as *mut TestSemaphore) };
        let mut count = sem.count.lock().unwrap();
        while *count == 0 {
            count = sem.woken.wait(count).unwrap();
        }
        *count -= 1;
        0
    }

    extern "C" fn test_semaphore_signal(semaphore: *mut Semaphore) -> c_int {
        // SAFETY: as in wait.
        let sem = unsafe { &*(semaphore as *mut TestSemaphore) };
        *sem.count.lock().unwrap() += 1;
        sem.woken.notify_one();
        0
    }

    extern "C" fn test_semaphore_free(semaphore: *mut Semaphore) {
        // SAFETY: allocated by semaphore_new below, freed at most once by the
        // code under test.
        unsafe { drop(Box::from_raw(semaphore as *mut TestSemaphore)) };
    }

    /// The double for `platform_semaphore_new`.
    pub(super) fn semaphore_new(initial_value: c_int) -> *mut Semaphore {
        Box::into_raw(Box::new(TestSemaphore {
            vtable: Semaphore {
                handle: core::ptr::null_mut(),
                wait: Some(test_semaphore_wait),
                signal: Some(test_semaphore_signal),
                free: Some(test_semaphore_free),
            },
            count: Mutex::new(initial_value),
            woken: Condvar::new(),
        })) as *mut Semaphore
    }

    /// A queue with the real one's semantics: the element semaphore counts,
    /// the mutex guards, takers block.
    struct TestQueue {
        items: Mutex<VecDeque<usize>>,
        semaphore: *mut Semaphore,
    }

    pub(super) unsafe fn queue_new(semaphore: *mut Semaphore) -> *mut c_void {
        Box::into_raw(Box::new(TestQueue {
            items: Mutex::new(VecDeque::new()),
            semaphore,
        })) as *mut c_void
    }

    pub(super) unsafe fn queue_put(queue: *mut c_void, element: *mut c_void) {
        // SAFETY: queues come from queue_new and are live for the test.
        let queue = unsafe { &*(queue as *mut TestQueue) };
        queue.items.lock().unwrap().push_back(element as usize);
        // SAFETY: the element semaphore outlives the queue in every test.
        unsafe {
            let semaphore = queue.semaphore;
            (*semaphore).signal.unwrap()(semaphore);
        }
    }

    pub(super) unsafe fn queue_take(queue: *mut c_void) -> *mut c_void {
        // SAFETY: as in queue_put.
        let queue = unsafe { &*(queue as *mut TestQueue) };
        // SAFETY: as in queue_put.
        unsafe {
            let semaphore = queue.semaphore;
            (*semaphore).wait.unwrap()(semaphore);
        }
        queue.items.lock().unwrap().pop_front().unwrap_or_default() as *mut c_void
    }

    pub(super) unsafe fn queue_size(queue: *mut c_void) -> c_int {
        // SAFETY: as in queue_put.
        let queue = unsafe { &*(queue as *mut TestQueue) };
        queue.items.lock().unwrap().len() as c_int
    }

    pub(super) unsafe fn queue_free(queue: *mut c_void) {
        // SAFETY: allocated by queue_new, freed at most once by the code
        // under test. The element semaphore leaks, as with the real queue.
        unsafe { drop(Box::from_raw(queue as *mut TestQueue)) };
    }

    /// Indices passed to the `signalSemaphoreWithIndex` double, oldest first.
    pub(super) static SIGNALLED: Mutex<Vec<sqInt>> = Mutex::new(Vec::new());

    pub(super) unsafe fn signal_semaphore_with_index(index: sqInt) -> sqInt {
        SIGNALLED.lock().unwrap().push(index);
        1
    }

    /// One recorded `ffi_call`, pointers as integers.
    #[derive(Debug, PartialEq, Eq)]
    pub(super) struct RecordedCall {
        pub cif: usize,
        pub function: usize,
        pub return_holder: usize,
        pub parameters: usize,
    }

    /// Calls passed to the `ffi_call` double, oldest first.
    pub(super) static CALLS: Mutex<Vec<RecordedCall>> = Mutex::new(Vec::new());

    pub(super) unsafe fn call(
        cif: *mut ffi_cif,
        function: *mut c_void,
        return_holder: *mut c_void,
        parameters: *mut *mut c_void,
    ) {
        CALLS.lock().unwrap().push(RecordedCall {
            cif: cif as usize,
            function: function as usize,
            return_holder: return_holder as usize,
            parameters: parameters as usize,
        });
    }
}

/// The `Runner.callbackEnterFunction` slot: waits, one way or the other, for
/// the callback in `invocation` to be answered by the image.
///
/// A null payload marks a callback on the worker's own thread -- from inside a
/// callout -- so callouts must keep flowing: run the loop re-entrantly and let
/// its `CALLBACK_RETURN` case return through here. A non-null payload is the
/// platform semaphore a foreign thread parks on; the run loop signals it and
/// this frees it.
///
/// # Safety
///
/// `runner` must head a live [`Worker`], and `invocation` must be live with a
/// payload prepared by [`worker_callback_prepare`].
#[no_mangle]
pub unsafe extern "C" fn worker_enter_callback(
    runner: *mut Runner,
    invocation: *mut CallbackInvocation,
) {
    let worker = runner as *mut Worker;

    // SAFETY: `invocation` is live by this function's contract.
    let payload = unsafe { (*invocation).payload };

    if payload.is_null() {
        // SAFETY: `worker` is live by this function's contract.
        unsafe { worker_run(worker as *mut c_void) };
        return;
    }

    let semaphore = payload as *mut Semaphore;
    // SAFETY: a non-null payload is a live platform semaphore whose vtable
    // slots are always filled; after the free it is never touched again.
    unsafe {
        (*semaphore).wait.unwrap()(semaphore);
        (*semaphore).free.unwrap()(semaphore);
    }
}

/// The `Runner.callbackExitFunction` slot: queues the `CALLBACK_RETURN` task
/// that ends the wait [`worker_enter_callback`] started.
///
/// Named `worker_callback_return` in the C too, although the slot is called
/// "exit"; kept.
///
/// # Safety
///
/// `worker` must head a live [`Worker`]; `invocation` must be live.
#[no_mangle]
pub unsafe extern "C" fn worker_callback_return(
    worker: *mut Runner,
    invocation: *mut CallbackInvocation,
) {
    // SAFETY: both delegated to the caller.
    unsafe {
        let task = worker_task_new_callback(invocation);
        worker_add_call(worker as *mut Worker, task);
    }
}

/// The `Runner.callbackPrepareInvocation` slot: decides how
/// [`worker_enter_callback`] will wait, by thread.
///
/// On the worker's own thread the payload is null -- the mark for "run the
/// loop re-entrantly". On any other thread it is a fresh platform semaphore
/// for that thread to park on.
///
/// # Safety
///
/// `worker` must head a live [`Worker`] whose [`worker_run`] has set
/// `self_thread`; `invocation` must be live.
#[no_mangle]
pub unsafe extern "C" fn worker_callback_prepare(
    worker: *mut Runner,
    invocation: *mut CallbackInvocation,
) {
    // Threads are compared with pthread_equal(), since binary comparison ==
    // is not portable.
    // SAFETY: `worker` is live by this function's contract.
    let same_thread = unsafe {
        libc::pthread_equal(
            (*(worker as *mut Worker)).self_thread,
            libc::pthread_self(),
        )
    };

    // SAFETY: `invocation` is live by this function's contract.
    unsafe {
        (*invocation).payload = if same_thread != 0 {
            ptr::null_mut()
        } else {
            seam::semaphore_new(0) as *mut c_void
        };
    }
}

/// The `pthread_create` entry: a shim because a thread entry cannot be an
/// `unsafe fn`, and [`worker_run`] is one.
extern "C" fn worker_thread_entry(worker: *mut c_void) -> *mut c_void {
    // SAFETY: `worker_newSpawning` passes the live worker it just built.
    unsafe { worker_run(worker) }
}

/// Builds a worker; with `spawn` non-zero, also starts its detached thread.
///
/// `runMainThreadWorker` in `pThreadedFFI.c` (still C) is the one caller that
/// passes 0: it builds the worker and then runs [`worker_run`] on the VM's own
/// main thread.
///
/// Answers null when the thread cannot be started, leaking the half-built
/// worker as the C did.
#[no_mangle]
pub extern "C" fn worker_newSpawning(spawn: c_int) -> *mut Worker {
    // Zero-initialized where the C's malloc left garbage; see the module
    // docs. An all-zero Worker is valid: null pointers and None vtable slots,
    // every one of which is assigned below or before first read.
    let mut worker: Box<Worker> = Box::new(unsafe { core::mem::zeroed() });

    worker.has_to_quit = 0;
    worker.nested_runs = 0;
    worker.next = ptr::null_mut();
    // SAFETY: a fresh platform semaphore, owned (and leaked, faithfully) by
    // the queue's user.
    worker.task_queue = unsafe { seam::queue_new(seam::semaphore_new(0)) };
    worker.runner.callbackEnterFunction = Some(worker_enter_callback);
    worker.runner.callbackExitFunction = Some(worker_callback_return);
    worker.runner.callbackPrepareInvocation = Some(worker_callback_prepare);
    worker.runner.callbackStack = ptr::null_mut();

    let worker = Box::into_raw(worker);

    if spawn != 0 {
        // SAFETY: `worker` is live; pthread_create writes thread_id and hands
        // the worker pointer to the new thread, which outlives this call by
        // way of detachment.
        let create_failed = unsafe {
            libc::pthread_create(
                &mut (*worker).thread_id,
                ptr::null(),
                worker_thread_entry,
                worker as *mut c_void,
            ) != 0
        };
        if create_failed {
            // SAFETY: a static NUL-terminated string.
            unsafe { libc::perror(c"pthread_create() error".as_ptr()) };
            return ptr::null_mut();
        }

        // SAFETY: thread_id was just written by a successful pthread_create.
        unsafe { libc::pthread_detach((*worker).thread_id) };
    }

    worker
}

/// [`worker_newSpawning`] with a spawned thread: the shape
/// `primitiveCreateWorker` uses.
#[no_mangle]
pub extern "C" fn worker_new() -> *mut Worker {
    worker_newSpawning(1)
}

/// Queues the release task; the worker quits once the queue has drained.
///
/// # Safety
///
/// `worker` must be live and not already released.
#[no_mangle]
pub unsafe extern "C" fn worker_release(worker: *mut Worker) {
    let task = worker_task_new_release();
    // SAFETY: delegated to the caller.
    unsafe { worker_add_call(worker, task) };
}

/// Queues a callout task. An alias of [`worker_add_call`] in behaviour; both
/// existed in the C and both are exported.
///
/// # Safety
///
/// `worker` must be live; `task` must be a live task the queue may own.
#[no_mangle]
pub unsafe extern "C" fn worker_dispatch_callout(worker: *mut Worker, task: *mut WorkerTask) {
    // SAFETY: delegated to the caller.
    unsafe { worker_add_call(worker, task) };
}

/// Appends `task` to the worker's queue, waking the run loop.
///
/// # Safety
///
/// `worker` must be live; `task` must be a live task the queue may own.
#[no_mangle]
pub unsafe extern "C" fn worker_add_call(worker: *mut Worker, task: *mut WorkerTask) {
    // SAFETY: delegated to the caller.
    unsafe { seam::queue_put((*worker).task_queue, task as *mut c_void) };
}

/// Takes the next task, blocking on an empty queue -- unless the worker is
/// quitting and the queue has drained, which answers null.
///
/// # Safety
///
/// `worker` must be live, and only the thread inside [`worker_run`] may call
/// this, as in the C.
#[no_mangle]
pub unsafe extern "C" fn worker_next_call(worker: *mut Worker) -> *mut WorkerTask {
    // SAFETY: `worker` is live by this function's contract.
    unsafe {
        if (*worker).has_to_quit != 0 && seam::queue_size((*worker).task_queue) == 0 {
            return ptr::null_mut();
        }

        seam::queue_take((*worker).task_queue) as *mut WorkerTask
    }
}

/// Runs one callout and signals the external semaphore the image is waiting
/// on. `executeWorkerTask` in the C; private there too.
///
/// # Safety
///
/// `task` must be a live `CALLOUT` task whose pointers the image is keeping
/// pinned.
unsafe fn execute_worker_task(_worker: *mut Worker, task: *mut WorkerTask) {
    // SAFETY: delegated to the caller.
    unsafe {
        seam::call(
            (*task).cif,
            (*task).anExternalFunction,
            (*task).returnHolderAddress,
            (*task).parametersAddress as *mut *mut c_void,
        );
        seam::signal_semaphore_with_index((*task).semaphoreIndex as sqInt);
    }
}

/// The run loop. Takes tasks until told to quit; also the `pthread` entry of
/// spawned workers (through [`worker_thread_entry`]) and re-entered by
/// [`worker_enter_callback`] for same-thread callbacks.
///
/// Always answers null: the C's returns are `return NULL` and a fall-through.
///
/// # Safety
///
/// `a_worker` must point to a live [`Worker`]. After a run that ends by
/// drained release with no nested runs pending, the worker is freed and must
/// not be used again.
#[no_mangle]
pub unsafe extern "C" fn worker_run(a_worker: *mut c_void) -> *mut c_void {
    let worker = a_worker as *mut Worker;

    // SAFETY (all accesses below): `worker` is live by this function's
    // contract, and these fields are only ever touched from the thread inside
    // the run loop, as in the C.
    let my_run = unsafe { (*worker).nested_runs };

    unsafe {
        (*worker).self_thread = libc::pthread_self();
        (*worker).nested_runs += 1;
    }

    loop {
        // SAFETY: `worker` is live; this is the loop thread.
        let task = unsafe { worker_next_call(worker) };

        if task.is_null() {
            // SAFETY: field access as above; perror takes a static string.
            unsafe {
                if (*worker).has_to_quit != 0 {
                    break;
                }
                libc::perror(c"No callbacks in the queue".as_ptr());
            }
            continue;
        }

        // SAFETY: tasks in the queue are live; nothing frees them (see
        // crate::worker_task).
        let task_type = unsafe { (*task).type_ };

        if task_type == WorkerTaskType::WORKER_RELEASE {
            unsafe {
                (*worker).has_to_quit = 1;
                // We wait in case we need to receive a callback_return
                // message.
                libc::sleep(1);
            }
        } else if task_type == WorkerTaskType::CALLOUT {
            // SAFETY: a CALLOUT task's pointers are pinned by the image.
            unsafe { execute_worker_task(worker, task) };
        } else if task_type == WorkerTaskType::CALLBACK_RETURN {
            // Stop consuming tasks and return. A semaphore means the callback
            // ran on a foreign thread that is parked on it: signal it and
            // keep running. No semaphore means the callback was re-entrant on
            // this thread: return to the worker_run frame below this one --
            // skipping the epilogue, so nestedRuns keeps this run's
            // increment. Faithful; see the module docs.
            // SAFETY: the semaphore, when present, is live until the signal
            // hands ownership back to the parked thread.
            unsafe {
                let semaphore = (*task).callbackSemaphore as *mut Semaphore;
                if !semaphore.is_null() {
                    (*semaphore).signal.unwrap()(semaphore);
                } else {
                    return ptr::null_mut();
                }
            }
        } else {
            logging::message_one_int(
                LOG_ERROR,
                c"Unsupported task type: %d",
                site!(
                    c"src/ffi/worker/worker.c",
                    c"worker_run",
                    186
                ),
                task_type.0 as c_int,
            );
            // SAFETY: a static (empty) NUL-terminated string, as the C passed.
            unsafe { libc::perror(c"".as_ptr()) };
        }
    }

    logging::message_two_ints(
        LOG_INFO,
        c"Finishing Nested run: %d from %d\n",
        site!(c"src/ffi/worker/worker.c", c"worker_run", 198),
        unsafe { (*worker).nested_runs },
        my_run,
    );

    // SAFETY: field access as above; the teardown branch is the last touch.
    unsafe {
        (*worker).nested_runs -= 1;

        if (*worker).nested_runs == 0 {
            seam::queue_free((*worker).task_queue);
            drop(Box::from_raw(worker));
        }
    }

    ptr::null_mut()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;
    use crate::worker_task::worker_task_new;
    use std::time::Duration;

    /// Empties both recorders; call at the start of every test, under the
    /// globals lock.
    fn reset_recorders() {
        seam::SIGNALLED.lock().unwrap().clear();
        seam::CALLS.lock().unwrap().clear();
    }

    /// A worker without a thread, run by the test itself.
    fn unspawned_worker() -> *mut Worker {
        let worker = worker_newSpawning(0);
        assert!(!worker.is_null());
        worker
    }

    #[test]
    fn a_callout_is_executed_and_its_semaphore_signalled() {
        let _guard = test_support::lock_globals();
        reset_recorders();

        let worker = unspawned_worker();
        let task = worker_task_new(
            0x1000 as *mut c_void,
            0x2000 as *mut pharo_vm_sys::ffi_cif,
            0x3000 as *mut c_void,
            0x4000 as *mut c_void,
            9,
        );
        // SAFETY: worker and task are live; release makes the run terminate.
        unsafe {
            worker_dispatch_callout(worker, task);
            worker_release(worker);
            worker_run(worker as *mut c_void);
            // `worker` is freed now.
        }

        assert_eq!(
            *seam::CALLS.lock().unwrap(),
            vec![seam::RecordedCall {
                cif: 0x2000,
                function: 0x1000,
                return_holder: 0x4000,
                parameters: 0x3000,
            }]
        );
        assert_eq!(*seam::SIGNALLED.lock().unwrap(), vec![9]);
    }

    #[test]
    fn release_drains_tasks_queued_behind_it_before_quitting() {
        let _guard = test_support::lock_globals();
        reset_recorders();

        let worker = unspawned_worker();
        let task = worker_task_new(
            0x1000 as *mut c_void,
            0x2000 as *mut pharo_vm_sys::ffi_cif,
            ptr::null_mut(),
            0x4000 as *mut c_void,
            3,
        );
        // Release first, then a callout: the release sets has_to_quit but the
        // loop must still execute what is already queued.
        // SAFETY: as in the previous test.
        unsafe {
            worker_release(worker);
            worker_dispatch_callout(worker, task);
            worker_run(worker as *mut c_void);
        }

        assert_eq!(seam::CALLS.lock().unwrap().len(), 1);
        assert_eq!(*seam::SIGNALLED.lock().unwrap(), vec![3]);
    }

    #[test]
    fn a_same_thread_callback_return_stops_the_loop_and_pins_the_worker() {
        let _guard = test_support::lock_globals();
        reset_recorders();

        let worker = unspawned_worker();
        // A same-thread callback's payload is null; its return task therefore
        // carries a null semaphore.
        // SAFETY: an all-zero invocation is all pointers; only payload is
        // read.
        let mut invocation: CallbackInvocation = unsafe { core::mem::zeroed() };

        // SAFETY: worker, task and invocation are live.
        unsafe {
            let task = worker_task_new_callback(&mut invocation);
            worker_add_call(worker, task);

            assert!(worker_run(worker as *mut c_void).is_null());

            // The early return skipped the epilogue: the increment is still
            // there, which is the faithful leak -- the worker was not freed,
            // so this read is safe.
            assert_eq!((*worker).nested_runs, 1);
        }
    }

    #[test]
    fn a_cross_thread_callback_return_signals_the_parked_semaphore() {
        let _guard = test_support::lock_globals();
        reset_recorders();

        let worker = unspawned_worker();
        let semaphore = seam::semaphore_new(0);
        // SAFETY: as above; payload carries the semaphore a foreign thread
        // would be parked on.
        let mut invocation: CallbackInvocation = unsafe { core::mem::zeroed() };
        invocation.payload = semaphore as *mut c_void;

        // SAFETY: worker, tasks and semaphore are live for the whole test.
        unsafe {
            let task = worker_task_new_callback(&mut invocation);
            worker_add_call(worker, task);
            worker_release(worker);
            worker_run(worker as *mut c_void);

            // The loop signalled rather than returned: a wait must now come
            // straight back instead of blocking.
            assert_eq!((*semaphore).wait.unwrap()(semaphore), 0);
            (*semaphore).free.unwrap()(semaphore);
        }
    }

    #[test]
    fn prepare_marks_a_same_thread_callback_with_a_null_payload() {
        let _guard = test_support::lock_globals();

        let worker = unspawned_worker();
        // SAFETY: worker is live; the test thread poses as the run loop's.
        unsafe { (*worker).self_thread = libc::pthread_self() };

        // SAFETY: as in the earlier tests.
        let mut invocation: CallbackInvocation = unsafe { core::mem::zeroed() };
        invocation.payload = 0xdead as *mut c_void;

        // SAFETY: worker and invocation are live.
        unsafe {
            worker_callback_prepare(worker as *mut Runner, &mut invocation);
        }
        assert!(invocation.payload.is_null());
    }

    #[test]
    fn prepare_gives_a_foreign_thread_callback_a_semaphore() {
        let _guard = test_support::lock_globals();

        let worker = unspawned_worker();
        // SAFETY: worker is live; the run loop's thread is this one, so a
        // spawned thread is foreign.
        unsafe { (*worker).self_thread = libc::pthread_self() };
        let worker_address = worker as usize;

        let payload = std::thread::spawn(move || {
            let worker = worker_address as *mut Worker;
            // SAFETY: the worker outlives the join below.
            let mut invocation: CallbackInvocation = unsafe { core::mem::zeroed() };
            // SAFETY: worker and invocation are live.
            unsafe {
                worker_callback_prepare(worker as *mut Runner, &mut invocation);
            }
            invocation.payload as usize
        })
        .join()
        .expect("prepare thread panicked");

        assert_ne!(payload, 0, "a foreign thread must get a semaphore to park on");
        let semaphore = payload as *mut Semaphore;
        // SAFETY: the semaphore is live and this is its last use.
        unsafe { (*semaphore).free.unwrap()(semaphore) };
    }

    #[test]
    fn enter_callback_parks_on_the_payload_semaphore_until_signalled() {
        let _guard = test_support::lock_globals();

        let worker = unspawned_worker();
        let semaphore = seam::semaphore_new(0);

        // Signal arrives from another thread after a beat; enter must block
        // until then, then free the semaphore itself.
        let semaphore_address = semaphore as usize;
        let signaller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let semaphore = semaphore_address as *mut Semaphore;
            // SAFETY: enter_callback is still parked on the semaphore.
            unsafe { (*semaphore).signal.unwrap()(semaphore) };
        });

        // SAFETY: worker, invocation and semaphore are live.
        unsafe {
            let mut invocation: CallbackInvocation = core::mem::zeroed();
            invocation.payload = semaphore as *mut c_void;
            worker_enter_callback(worker as *mut Runner, &mut invocation);
        }

        signaller.join().expect("signaller panicked");
    }

    #[test]
    fn a_spawned_worker_executes_tasks_on_its_own_thread() {
        let _guard = test_support::lock_globals();
        reset_recorders();

        let worker = worker_new();
        assert!(!worker.is_null());

        let task = worker_task_new(
            0x7000 as *mut c_void,
            0x8000 as *mut pharo_vm_sys::ffi_cif,
            ptr::null_mut(),
            ptr::null_mut(),
            42,
        );
        // SAFETY: worker and task are live; the worker thread owns them from
        // here.
        unsafe { worker_dispatch_callout(worker, task) };

        // The detached worker thread picks the task up on its own time.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while seam::SIGNALLED.lock().unwrap().is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "worker thread never executed the task"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(*seam::SIGNALLED.lock().unwrap(), vec![42]);

        // SAFETY: the worker is still live; its thread frees it after the
        // release drains.
        unsafe { worker_release(worker) };
    }
}
