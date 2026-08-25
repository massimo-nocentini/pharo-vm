//! C stdio objects the `libc` crate does not expose because C hides them
//! behind macros.
//!
//! glibc and musl name the object behind the `stdin` macro `stdin`; the BSDs
//! and macOS name it `__stdinp`. Same per-platform `link_name` scheme as
//! `pharo-platform`'s cstdio module (which this crate cannot depend on: that
//! crate needs the CMake environment, this one deliberately does not).

extern "C" {
    #[cfg_attr(
        any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
        link_name = "__stdinp"
    )]
    static mut stdin: *mut libc::FILE;
    #[cfg_attr(
        any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
        link_name = "__stdoutp"
    )]
    static mut stdout: *mut libc::FILE;
    #[cfg_attr(
        any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
        link_name = "__stderrp"
    )]
    static mut stderr: *mut libc::FILE;
}

/// Returns the `FILE *` that C's `stdin` macro would evaluate to.
pub(crate) fn c_stdin() -> *mut libc::FILE {
    // SAFETY: the C runtime initialises these before main and only replaces
    // them via freopen, which nothing here does.
    unsafe { stdin }
}

/// Returns the `FILE *` that C's `stdout` macro would evaluate to.
pub(crate) fn c_stdout() -> *mut libc::FILE {
    // SAFETY: as in `c_stdin`.
    unsafe { stdout }
}

/// Returns the `FILE *` that C's `stderr` macro would evaluate to.
pub(crate) fn c_stderr() -> *mut libc::FILE {
    // SAFETY: as in `c_stdin`.
    unsafe { stderr }
}
