//! Replaces `src/threadSafeQueue/threadSafeQueue.c` on non-Apple Unix.
//!
//! A FIFO of `void *` shared between threads, used twice: as the FFI worker's
//! task queue (`src/ffi/worker/worker.c`) and as the callback queue
//! (`src/ffi/callbacks/callbacks.c`), both still C.
//!
//! The deque is guarded by a [`std::sync::Mutex`]; the **element semaphore**
//! is supplied by the caller and counts items, so that
//! [`threadsafe_queue_take`] can block until there is something to take. It is
//! reached through its vtable, not through `platform_semaphore_*`, because the
//! callback queue passes a [`crate::pharo_semaphore`] here -- signalling it
//! wakes a Smalltalk process rather than a thread. (The C guarded the links
//! with a second counting semaphore used as a mutex; nothing observable hangs
//! on that choice, and a lock guard cannot forget to release.)
//!
//! # Scope
//!
//! Non-Apple Unix, mirroring where `cmake/rust.cmake` swaps the C out. The
//! C-era reason -- the mutex was built from [`crate::platform_semaphore`],
//! which is POSIX-only -- no longer applies, so widening the scope is now a
//! CMake decision, not a porting one.
//!
//! # The queue type is opaque
//!
//! `threadSafeQueue.h` declares `TSQueue` without defining it, so no C
//! translation unit knows its size or layout, and only this module allocates
//! or frees one. That makes both the allocator and the internal representation
//! implementation details: the C used `malloc`/`free` and a hand-rolled linked
//! list, this uses Rust's allocator and a [`VecDeque`], and nothing can tell
//! the difference.
//!
//! # Divergences, and both are fixes
//!
//! * `threadsafe_queue_take` read `queue->first` *before* taking the mutex,
//!   and only then locked to unlink it. Two threads that were each handed a
//!   permit by the element semaphore could therefore read the same node, both
//!   return its element, and both free it. Here the pop happens under the
//!   mutex, where it plainly belonged. Both queues have a single consumer
//!   today, so no reachable behaviour changes; there is a test with several
//!   consumers that would have caught the old version.
//! * The C's mutex was a separate allocation that `threadsafe_queue_free`
//!   deliberately leaked -- its comment wonders whether freeing would be safe.
//!   Here the mutex lives inside the queue and is freed with it, which is
//!   sound because the free contract already requires that no other thread is
//!   inside any queue operation.

use core::ffi::{c_int, c_void};
use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};

use pharo_vm_sys::Semaphore;

/// The queue. Opaque to C; see the module docs.
pub struct TSQueue {
    /// The elements, oldest first. They are the caller's; nothing here ever
    /// dereferences or frees one.
    items: Mutex<VecDeque<*mut c_void>>,
    /// Counts elements. Owned by the caller of [`threadsafe_queue_new`], and
    /// deliberately not freed here.
    semaphore: *mut Semaphore,
}

impl TSQueue {
    /// Locks the deque. A poisoned lock is ignored: C callers cannot unwind,
    /// so poison can only come from a panicking test thread.
    fn lock(&self) -> MutexGuard<'_, VecDeque<*mut c_void>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Creates an empty queue counted by `semaphore`.
///
/// `semaphore` is not adopted: it is neither signalled here nor freed by
/// [`threadsafe_queue_free`]. (The C could also fail and answer null when its
/// mutex semaphore could not be created; the `Mutex` cannot fail, so neither
/// can this.)
///
/// # Safety
///
/// `semaphore` must be a live [`Semaphore`] that outlives the queue.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_new(semaphore: *mut Semaphore) -> *mut TSQueue {
    Box::into_raw(Box::new(TSQueue {
        items: Mutex::new(VecDeque::new()),
        semaphore,
    }))
}

/// Frees the queue and everything it owns.
///
/// The elements themselves are the caller's and are not touched. Neither is
/// the element semaphore. The mutex goes with the queue -- see the divergence
/// note in the module docs.
///
/// # Safety
///
/// `queue` must come from [`threadsafe_queue_new`], must not be used again,
/// and no other thread may be inside any queue operation.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_free(queue: *mut TSQueue) {
    // SAFETY: delegated to the caller.
    unsafe { drop(Box::from_raw(queue)) };
}

/// Counts the elements currently queued.
///
/// The answer is stale the moment it is returned; `worker.c` uses it only as
/// an "is it empty" test. (O(1) now -- the C walked its list under the mutex.)
///
/// # Safety
///
/// `queue` must be live.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_size(queue: *mut TSQueue) -> c_int {
    // SAFETY: delegated to the caller.
    let queue = unsafe { &*queue };
    queue.lock().len() as c_int
}

/// Appends `element` and signals the element semaphore.
///
/// The signal happens *after* the mutex is released, as in the C, so a waiter
/// woken by it does not immediately block on a mutex this thread still holds.
///
/// # Safety
///
/// `queue` must be live. `element` is stored as an opaque pointer and is never
/// dereferenced here, so any value is accepted -- including null, which
/// [`threadsafe_queue_take`] cannot then distinguish from an empty queue.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_put(queue: *mut TSQueue, element: *mut c_void) {
    // SAFETY: delegated to the caller.
    let queue = unsafe { &*queue };
    queue.lock().push_back(element); // The guard drops here, before the signal.

    // SAFETY: the semaphore is live by the contract of threadsafe_queue_new,
    // and its vtable slots are always filled.
    unsafe {
        let semaphore = queue.semaphore;
        (*semaphore).signal.unwrap()(semaphore);
    }
}

/// Removes and returns the first element, blocking until there is one.
///
/// Returns null if the wait fails, or if the queue is somehow empty despite
/// the semaphore granting a permit.
///
/// # Safety
///
/// `queue` must be live.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_take(queue: *mut TSQueue) -> *mut c_void {
    // SAFETY: delegated to the caller.
    let queue = unsafe { &*queue };

    // Block until the queue has elements.
    // SAFETY: the semaphore is live and its vtable slots filled; perror takes
    // a 'static NUL-terminated literal.
    unsafe {
        let semaphore = queue.semaphore;
        if (*semaphore).wait.unwrap()(semaphore) != 0 {
            libc::perror(c"Failed semaphore wait on thread safe queue".as_ptr());
            return core::ptr::null_mut();
        }
    }

    // Popping under the mutex; see the divergence note in the module docs.
    queue.lock().pop_front().unwrap_or(core::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_semaphore::platform_semaphore_new;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A queue plus the element semaphore it counts with, torn down together.
    struct Fixture {
        queue: *mut TSQueue,
        semaphore: *mut Semaphore,
    }

    impl Fixture {
        fn new() -> Self {
            let semaphore = platform_semaphore_new(0);
            assert!(!semaphore.is_null());
            // SAFETY: the semaphore is live and outlives the queue.
            let queue = unsafe { threadsafe_queue_new(semaphore) };
            assert!(!queue.is_null());
            Self { queue, semaphore }
        }

        fn put(&self, value: usize) {
            // SAFETY: the queue is live; the element is an opaque integer.
            unsafe { threadsafe_queue_put(self.queue, value as *mut c_void) }
        }

        fn take(&self) -> usize {
            // SAFETY: the queue is live.
            let element = unsafe { threadsafe_queue_take(self.queue) };
            element as usize
        }

        fn size(&self) -> c_int {
            // SAFETY: the queue is live.
            unsafe { threadsafe_queue_size(self.queue) }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            // SAFETY: every test joins its threads before dropping.
            unsafe {
                threadsafe_queue_free(self.queue);
                (*self.semaphore).free.unwrap()(self.semaphore);
            }
        }
    }

    #[test]
    fn elements_come_back_in_the_order_they_went_in() {
        let q = Fixture::new();
        assert_eq!(q.size(), 0);

        for i in 1..=5 {
            q.put(i);
        }
        assert_eq!(q.size(), 5);

        for i in 1..=5 {
            assert_eq!(q.take(), i, "queue must be FIFO, not LIFO");
        }
        assert_eq!(q.size(), 0);
    }

    #[test]
    fn the_head_and_tail_stay_consistent_across_emptying_and_refilling() {
        // With the C's linked list, draining to empty had to reset both
        // `first` and `last`; kept as a regression test of the same shape.
        let q = Fixture::new();
        q.put(1);
        assert_eq!(q.take(), 1);
        assert_eq!(q.size(), 0);

        q.put(2);
        q.put(3);
        assert_eq!(q.size(), 2);
        assert_eq!(q.take(), 2);
        assert_eq!(q.take(), 3);
        assert_eq!(q.size(), 0);
    }

    #[test]
    fn taking_blocks_until_something_is_put() {
        let q = Fixture::new();
        let raw = q.queue as usize;

        static TOOK: AtomicUsize = AtomicUsize::new(0);
        TOOK.store(0, Ordering::SeqCst);

        let consumer = std::thread::spawn(move || {
            let queue = raw as *mut TSQueue;
            // SAFETY: the main thread keeps the queue alive until this joins.
            let element = unsafe { threadsafe_queue_take(queue) };
            TOOK.store(1, Ordering::SeqCst);
            element as usize
        });

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(TOOK.load(Ordering::SeqCst), 0, "consumer should be blocked");

        q.put(99);
        assert_eq!(consumer.join().expect("consumer panicked"), 99);
    }

    #[test]
    fn several_consumers_never_receive_the_same_element() {
        // This is the divergence: the C read `queue->first` before taking the
        // mutex, so two consumers each holding a permit could return -- and
        // free -- the same node.
        const CONSUMERS: usize = 4;
        const PER_CONSUMER: usize = 250;
        const TOTAL: usize = CONSUMERS * PER_CONSUMER;

        let q = Fixture::new();
        let raw = q.queue as usize;

        let consumers: Vec<_> = (0..CONSUMERS)
            .map(|_| {
                std::thread::spawn(move || {
                    let queue = raw as *mut TSQueue;
                    (0..PER_CONSUMER)
                        .map(|_| {
                            // SAFETY: the queue outlives every consumer.
                            let element = unsafe { threadsafe_queue_take(queue) };
                            element as usize
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        // Elements are 1..=TOTAL so that 0 (a null element) is distinguishable.
        for i in 1..=TOTAL {
            q.put(i);
        }

        let mut seen: Vec<usize> = consumers
            .into_iter()
            .flat_map(|c| c.join().expect("consumer panicked"))
            .collect();
        seen.sort_unstable();

        assert_eq!(seen.len(), TOTAL);
        assert_eq!(
            seen,
            (1..=TOTAL).collect::<Vec<_>>(),
            "every element exactly once"
        );
        assert_eq!(q.size(), 0);
    }

    #[test]
    fn freeing_a_non_empty_queue_releases_its_nodes() {
        // Nothing here can assert the absence of a leak, but it does exercise
        // the teardown of a queue that still holds elements, which is where a
        // mistake would corrupt the heap and show up under the test runner's
        // allocator.
        let q = Fixture::new();
        for i in 1..=10 {
            q.put(i);
        }
        assert_eq!(q.size(), 10);
        // Dropped here with ten elements still queued.
    }
}
