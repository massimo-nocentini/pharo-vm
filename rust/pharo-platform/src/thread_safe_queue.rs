//! Replaces `src/threadSafeQueue/threadSafeQueue.c` on non-Apple Unix.
//!
//! A FIFO of `void *` shared between threads, used twice: as the FFI worker's
//! task queue (`src/ffi/worker/worker.c`) and as the callback queue
//! (`src/ffi/callbacks/callbacks.c`), both still C.
//!
//! Two semaphores, doing different jobs:
//!
//! * **the mutex**, created here with an initial count of 1, guards the links.
//! * **the element semaphore** is supplied by the caller and counts items, so
//!   that [`threadsafe_queue_take`] can block until there is something to take.
//!   It is reached through its vtable, not through `platform_semaphore_*`,
//!   because the callback queue passes a [`crate::pharo_semaphore`] here --
//!   signalling it wakes a Smalltalk process rather than a thread.
//!
//! # Scope
//!
//! Non-Apple Unix, because it builds its mutex with
//! [`crate::platform_semaphore`], which is POSIX-only. Windows and Apple keep
//! the C.
//!
//! # The queue type is opaque
//!
//! `threadSafeQueue.h` declares `TSQueue` without defining it, so no C
//! translation unit knows its size or layout, and only this module allocates
//! or frees one. That makes the allocator an implementation detail: the C used
//! `malloc`/`free`, this uses Rust's, and nothing can tell the difference.
//!
//! # Faithful oddity
//!
//! `threadsafe_queue_free` does not free the mutex. The C says so in a comment
//! -- "shouldn't we free the mutex here? if so, we should check if the mutex is
//! alive after every wait" -- and it is right that freeing it is not safe
//! without also fixing every waiter. Both queues in the tree live for the
//! process, so this leaks at most twice. Left alone.
//!
//! # One divergence, and it is a fix
//!
//! `threadsafe_queue_take` read `queue->first` *before* taking the mutex, and
//! only then locked to unlink it. Two threads that were each handed a permit
//! by the element semaphore could therefore read the same node, both return
//! its element, and both free it. Here the read happens under the mutex, where
//! it plainly belonged. Both queues have a single consumer today, so no
//! reachable behaviour changes; there is a test with several consumers that
//! would have caught the old version.

use core::ffi::{c_int, c_void};

use pharo_vm_sys::Semaphore;

use crate::platform_semaphore::{
    platform_semaphore_new, platform_semaphore_signal, platform_semaphore_wait,
};

/// A node in the linked list. Owns nothing but its own box: the element is the
/// caller's.
struct TSQueueNode {
    element: *mut c_void,
    next: *mut TSQueueNode,
}

/// The queue. Opaque to C; see the module docs.
///
/// `repr(C)` is not required, since no C code knows this layout, but it keeps
/// the field order the same as the C's for anyone reading both.
#[repr(C)]
pub struct TSQueue {
    first: *mut TSQueueNode,
    last: *mut TSQueueNode,
    mutex: *mut Semaphore,
    /// Counts elements. Owned by the caller of [`threadsafe_queue_new`], and
    /// deliberately not freed here.
    semaphore: *mut Semaphore,
}

/// Creates an empty queue counted by `semaphore`.
///
/// Returns null if the mutex cannot be created, after reporting it with
/// `perror` as the C did. `semaphore` is not adopted: it is neither signalled
/// on failure here nor freed by [`threadsafe_queue_free`].
///
/// # Safety
///
/// `semaphore` must be a live [`Semaphore`] that outlives the queue.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_new(semaphore: *mut Semaphore) -> *mut TSQueue {
    let mutex = platform_semaphore_new(1);
    if mutex.is_null() {
        // SAFETY: a 'static NUL-terminated literal.
        unsafe { libc::perror(c"mutex initialization error in make_queue".as_ptr()) };
        return core::ptr::null_mut();
    }

    Box::into_raw(Box::new(TSQueue {
        first: core::ptr::null_mut(),
        last: core::ptr::null_mut(),
        mutex,
        semaphore,
    }))
}

/// Frees the queue and every node still in it.
///
/// The elements themselves are the caller's and are not touched. Neither is
/// the element semaphore, nor the mutex -- see the module docs.
///
/// # Safety
///
/// `queue` must come from [`threadsafe_queue_new`], must not be used again,
/// and no other thread may be inside any queue operation.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_free(queue: *mut TSQueue) {
    // SAFETY: delegated to the caller.
    unsafe {
        let mutex = (*queue).mutex;
        platform_semaphore_wait(mutex);

        let mut node = (*queue).first;
        while !node.is_null() {
            let next = (*node).next;
            drop(Box::from_raw(node));
            node = next;
        }

        drop(Box::from_raw(queue));

        // Signalling after the queue is gone is fine: `mutex` is a copy of the
        // pointer, and the mutex outlives the queue by design.
        platform_semaphore_signal(mutex);
    }
}

/// Counts the elements currently queued.
///
/// Walks the list under the mutex, so it is O(n) and the answer is stale the
/// moment it is returned. `worker.c` uses it only as an "is it empty" test.
///
/// # Safety
///
/// `queue` must be live.
#[no_mangle]
pub unsafe extern "C" fn threadsafe_queue_size(queue: *mut TSQueue) -> c_int {
    // SAFETY: delegated to the caller; the walk stays under the mutex.
    unsafe {
        platform_semaphore_wait((*queue).mutex);

        let mut size: c_int = 0;
        let mut node = (*queue).first;
        while !node.is_null() {
            size += 1;
            node = (*node).next;
        }

        platform_semaphore_signal((*queue).mutex);
        size
    }
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
    let node = Box::into_raw(Box::new(TSQueueNode {
        element,
        next: core::ptr::null_mut(),
    }));

    // SAFETY: delegated to the caller; `node` was just allocated.
    unsafe {
        platform_semaphore_wait((*queue).mutex);

        if (*queue).first.is_null() {
            (*queue).first = node;
            (*queue).last = node;
        } else {
            (*(*queue).last).next = node;
            (*queue).last = node;
        }

        platform_semaphore_signal((*queue).mutex);

        let semaphore = (*queue).semaphore;
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
    unsafe {
        // Block until the queue has elements.
        let semaphore = (*queue).semaphore;
        if (*semaphore).wait.unwrap()(semaphore) != 0 {
            libc::perror(c"Failed semaphore wait on thread safe queue".as_ptr());
            return core::ptr::null_mut();
        }

        platform_semaphore_wait((*queue).mutex);

        // Reading `first` under the mutex rather than before taking it; see
        // the divergence note in the module docs.
        let node = (*queue).first;
        if node.is_null() {
            platform_semaphore_signal((*queue).mutex);
            return core::ptr::null_mut();
        }

        if (*queue).first == (*queue).last {
            (*queue).first = core::ptr::null_mut();
            (*queue).last = core::ptr::null_mut();
        } else {
            (*queue).first = (*node).next;
        }

        platform_semaphore_signal((*queue).mutex);

        let node = Box::from_raw(node);
        node.element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Draining to empty resets both `first` and `last`; a stale `last`
        // would make the next put append to a freed node.
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
        // the walk in threadsafe_queue_free, which is where a mistake would
        // corrupt the heap and show up under the test runner's allocator.
        let q = Fixture::new();
        for i in 1..=10 {
            q.put(i);
        }
        assert_eq!(q.size(), 10);
        // Dropped here with ten nodes still queued.
    }
}
