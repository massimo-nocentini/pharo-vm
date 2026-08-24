//! Replaces `src/semaphores/platformSemaphore.c` on non-Apple Unix.
//!
//! A real counting semaphore behind the same [`Semaphore`] vtable that
//! [`crate::pharo_semaphore`] implements, so the FFI worker and callback code
//! in `src/ffi/` can hold either without knowing which. This is the one that
//! actually blocks a thread.
//!
//! # Scope
//!
//! POSIX `sem_t` only. The C has three implementations: `CreateSemaphore` on
//! Windows, `sem_init` on Unix, and `dispatch_semaphore_create` on Apple,
//! where POSIX unnamed semaphores are deprecated and do not work. Only the
//! middle one is ported; `cmake/rust.cmake` keeps compiling the C on Windows
//! and on Apple.
//!
//! # Two layers, both exported
//!
//! `semaphore_*` take a raw `PlatformSemaphore` (a `sem_t *` here);
//! `platform_semaphore_*` take the `Semaphore` wrapper and unwrap `handle`
//! into one. Both layers are exported symbols in the C, so both are here, even
//! though only the outer layer is reached through the vtable.
//!
//! # Return conventions
//!
//! 0 for success. `semaphore_wait` and `semaphore_signal` pass `sem_wait` and
//! `sem_post` through unchanged, so they answer -1 on failure, while the
//! Windows branch of the same functions answers 1. Nothing in the tree checks
//! for a specific non-zero value; both are reproduced in their own branch.
//!
//! # One divergence, and it is a fix
//!
//! `semaphore_new` allocated the `sem_t` and then returned `NULL` without
//! freeing it when `sem_init` failed, leaking it. The Rust frees it. Nothing
//! observes the difference except a heap profile, and `sem_init` only fails
//! when the value exceeds `SEM_VALUE_MAX`.

use core::ffi::{c_int, c_long, c_void};

use pharo_vm_sys::Semaphore;

/// The C's `PlatformSemaphore` on this platform.
type PlatformSemaphore = *mut libc::sem_t;

/// Allocates a POSIX semaphore with `initial_value`, or null on failure.
///
/// The semaphore is process-private (`sem_init`'s `pshared` is 0), so it can
/// only be shared between threads of this process -- which is all the FFI
/// worker needs.
#[no_mangle]
pub extern "C" fn semaphore_new(initial_value: c_long) -> PlatformSemaphore {
    // Boxed rather than malloc'ed: the sem_t is only ever freed by
    // semaphore_release in this module, so the allocator is an implementation
    // detail. SAFETY: an all-zero sem_t is only ever a target for sem_init.
    let wrapper = Box::into_raw(Box::new(unsafe { core::mem::zeroed::<libc::sem_t>() }));
    // `initial_value` is a long but sem_init takes an unsigned int. The C
    // let the implicit conversion happen; a negative or huge value makes
    // sem_init fail with EINVAL, which is handled below.
    // SAFETY: `wrapper` points at one writable sem_t.
    if unsafe { libc::sem_init(wrapper, 0, initial_value as libc::c_uint) } != 0 {
        // The C leaked this. See the module docs.
        // SAFETY: allocated just above and never handed out.
        drop(unsafe { Box::from_raw(wrapper) });
        return core::ptr::null_mut();
    }
    wrapper
}

/// Blocks until `sem` can be decremented.
///
/// Retries on `EINTR`, so a signal delivered to this thread -- the heartbeat's,
/// for instance -- does not turn into a spurious wakeup for the caller.
///
/// # Safety
///
/// `sem` must be a live semaphore from [`semaphore_new`].
#[no_mangle]
pub unsafe extern "C" fn semaphore_wait(sem: PlatformSemaphore) -> c_int {
    loop {
        // SAFETY: delegated to the caller.
        let code = unsafe { libc::sem_wait(sem) };
        if code != -1 {
            return code;
        }
        // last_os_error reads errno portably: `__errno_location` is the
        // glibc/musl spelling only, and this module's scope is every
        // non-Apple Unix.
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return code;
        }
    }
}

/// Increments `sem`, waking one waiter.
///
/// # Safety
///
/// `sem` must be a live semaphore from [`semaphore_new`]. Async-signal-safe:
/// `sem_post` is the one semaphore operation POSIX permits in a signal
/// handler, which is why the heartbeat can use it.
#[no_mangle]
pub unsafe extern "C" fn semaphore_signal(sem: PlatformSemaphore) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe { libc::sem_post(sem) }
}

/// Destroys and frees `sem`. Always reports success, as the C did.
///
/// # Safety
///
/// `sem` must be a live semaphore from [`semaphore_new`] with no waiters, and
/// must not be used again.
#[no_mangle]
pub unsafe extern "C" fn semaphore_release(sem: PlatformSemaphore) -> c_int {
    // SAFETY: delegated to the caller. The C ignored sem_destroy's result and
    // so does this; destroying a semaphore with waiters is undefined either
    // way. The box came from semaphore_new.
    unsafe {
        libc::sem_destroy(sem);
        drop(Box::from_raw(sem));
    }
    0
}

/// Blocks on the semaphore behind `semaphore`.
///
/// # Safety
///
/// `semaphore` must come from [`platform_semaphore_new`] and still be live.
#[no_mangle]
pub unsafe extern "C" fn platform_semaphore_wait(semaphore: *mut Semaphore) -> c_int {
    // SAFETY: delegated to the caller; `handle` is the sem_t * stored by
    // platform_semaphore_new.
    unsafe { semaphore_wait((*semaphore).handle.cast::<libc::sem_t>()) }
}

/// Signals the semaphore behind `semaphore`.
///
/// # Safety
///
/// As [`platform_semaphore_wait`].
#[no_mangle]
pub unsafe extern "C" fn platform_semaphore_signal(semaphore: *mut Semaphore) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe { semaphore_signal((*semaphore).handle.cast::<libc::sem_t>()) }
}

/// Releases the semaphore and frees the wrapper.
///
/// # Safety
///
/// As [`platform_semaphore_wait`], and `semaphore` must not be used again.
#[no_mangle]
pub unsafe extern "C" fn platform_semaphore_free(semaphore: *mut Semaphore) {
    // SAFETY: delegated to the caller. Both boxes were allocated by this
    // module -- C only ever frees a Semaphore through this vtable slot.
    unsafe {
        semaphore_release((*semaphore).handle.cast::<libc::sem_t>());
        drop(Box::from_raw(semaphore));
    }
}

/// Allocates a [`Semaphore`] backed by a POSIX semaphore with
/// `initial_value`.
///
/// Note that the *inner* semaphore failing leaves a wrapper with a null
/// handle rather than a null wrapper -- that is the C's behaviour, and the
/// FFI code that calls this does not check either, so changing it here would
/// only move where the crash happens. (The C could also answer null when the
/// wrapper's own malloc failed; a failed `Box` allocation aborts instead.)
#[no_mangle]
pub extern "C" fn platform_semaphore_new(initial_value: c_int) -> *mut Semaphore {
    Box::into_raw(Box::new(Semaphore {
        handle: semaphore_new(c_long::from(initial_value)).cast::<c_void>(),
        wait: Some(platform_semaphore_wait),
        signal: Some(platform_semaphore_signal),
        free: Some(platform_semaphore_free),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn a_new_semaphore_has_a_handle_and_a_full_vtable() {
        let s = platform_semaphore_new(0);
        assert!(!s.is_null());
        // SAFETY: just allocated, freed at the end of the test.
        unsafe {
            assert!(!(*s).handle.is_null(), "sem_init should have succeeded");
            assert!((*s).wait.is_some());
            assert!((*s).signal.is_some());
            assert!((*s).free.is_some());
            (*s).free.unwrap()(s);
        }
    }

    #[test]
    fn an_initial_count_lets_that_many_waits_through_without_blocking() {
        let s = platform_semaphore_new(3);
        // SAFETY: live for the whole test.
        unsafe {
            // Three permits, so three waits return immediately. A fourth would
            // block, which is checked by the threaded test below rather than
            // here.
            for _ in 0..3 {
                assert_eq!((*s).wait.unwrap()(s), 0);
            }
            (*s).free.unwrap()(s);
        }
    }

    #[test]
    fn a_signal_releases_a_blocked_waiter() {
        let s = platform_semaphore_new(0);
        // A pointer cannot cross a thread boundary on its own; the semaphore
        // outlives both threads because this one joins before freeing it.
        let raw = s as usize;

        static WOKE: AtomicUsize = AtomicUsize::new(0);
        WOKE.store(0, Ordering::SeqCst);

        let waiter = std::thread::spawn(move || {
            let s = raw as *mut Semaphore;
            // SAFETY: the main thread keeps `s` alive until this joins.
            let code = unsafe { (*s).wait.unwrap()(s) };
            WOKE.store(1, Ordering::SeqCst);
            code
        });

        // Give the waiter time to actually block. If it did not, the assertion
        // below still holds, so this is not a race that can produce a false
        // pass -- only a weaker test.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            WOKE.load(Ordering::SeqCst),
            0,
            "waiter should still be blocked"
        );

        // SAFETY: s is live and owned by this thread.
        unsafe {
            assert_eq!((*s).signal.unwrap()(s), 0);
        }

        assert_eq!(waiter.join().expect("waiter panicked"), 0);
        assert_eq!(WOKE.load(Ordering::SeqCst), 1);

        // SAFETY: the waiter has joined, so there are no waiters left.
        unsafe { (*s).free.unwrap()(s) };
    }

    #[test]
    fn an_impossible_initial_value_answers_null_rather_than_leaking() {
        // sem_init rejects a value above SEM_VALUE_MAX with EINVAL. This is
        // the path the C leaked its sem_t on.
        let sem = semaphore_new(c_long::MAX);
        assert!(sem.is_null());
    }
}
