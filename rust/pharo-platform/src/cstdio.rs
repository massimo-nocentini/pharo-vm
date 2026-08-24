//! C stdio objects the `libc` crate does not expose because C hides them
//! behind macros.
//!
//! glibc and musl name the object behind the `stdout` macro `stdout`; the BSDs
//! and macOS name it `__stdoutp`. The per-platform `link_name` lives here once
//! so that every module needing a C `FILE *` for stdout resolves it the same
//! way.

extern "C" {
    #[cfg_attr(
        any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
        link_name = "__stdoutp"
    )]
    static mut stdout: *mut libc::FILE;
}

/// Returns the `FILE *` that C's `stdout` macro would evaluate to.
pub(crate) fn c_stdout() -> *mut libc::FILE {
    // SAFETY: the C runtime initialises this before main and only replaces it
    // via freopen, which nothing here does.
    unsafe { stdout }
}
