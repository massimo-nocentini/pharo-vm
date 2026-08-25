//! Reading and clearing `errno`, needed because the C loops on
//! `errno = 0; readdir(...); errno == EINTR`.

use libc::c_int;

fn location() -> *mut c_int {
    // SAFETY: both functions answer the calling thread's errno slot.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        libc::__errno_location()
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly"
    ))]
    unsafe {
        libc::__error()
    }
}

pub(crate) fn get() -> c_int {
    // SAFETY: the location is valid for the current thread.
    unsafe { *location() }
}

pub(crate) fn clear() {
    // SAFETY: as in `get`.
    unsafe { *location() = 0 }
}
