//! The `logError` / `logDebug` / `logTrace` macros from
//! `include/pharovm/debug.h`, as functions.
//!
//! # Why this is not just an `extern "C"` call
//!
//! `logMessage` is variadic, which means two things. It cannot be stubbed by
//! defining it in Rust, since Rust cannot define a variadic `extern "C"`
//! function on stable; and calling it needs a format string matched by hand to
//! the argument list. So each shape the port actually uses gets its own
//! function here, with the format string and its arguments checked in one
//! place, and `cfg(test)` records the call instead of making it.
//!
//! Recording rather than calling also keeps `src/debug.c` -- and through it the
//! rest of the platform layer -- out of the unit-test link, and lets a test
//! assert that the arguments really are the ones the C passed.
//!
//! # Why the call sites carry C file names and line numbers
//!
//! `debug.h` fills in `__FILENAME__`, `__FUNCTION__` and `__LINE__` at the
//! macro call site. A ported module passes the values the *C* compiler would
//! have produced, so that the log output of the two builds is byte-identical
//! and differential testing can diff stderr. Every caller therefore names a
//! `.c` file that no longer compiles into the Rust build; that is deliberate.

use core::ffi::{c_char, c_int, c_longlong, CStr};

/// `LOG_ERROR` from `include/pharovm/debug.h`.
pub(crate) const LOG_ERROR: c_int = 1;
/// `LOG_DEBUG` from `include/pharovm/debug.h`.
pub(crate) const LOG_DEBUG: c_int = 4;
/// `LOG_TRACE` from `include/pharovm/debug.h`.
pub(crate) const LOG_TRACE: c_int = 5;

/// Where a log call claims to come from: the `__FILENAME__`, `__FUNCTION__`
/// and `__LINE__` the C preprocessor would have supplied.
///
/// `file` is repo-relative because `debug.h` defines `__FILENAME__` as
/// `__FILE__ + SOURCE_PATH_SIZE`, and CMake sets `SOURCE_PATH_SIZE` to the
/// length of the source root.
#[derive(Clone, Copy)]
pub(crate) struct Site {
    /// The `.c` file this call site had before the port.
    pub file: &'static CStr,
    /// The C function name.
    pub function: &'static CStr,
    /// The line in `file`.
    pub line: c_int,
}

/// Declares a [`Site`] with less ceremony at the call site.
macro_rules! site {
    ($file:expr, $function:expr, $line:expr) => {
        $crate::logging::Site {
            file: $file,
            function: $function,
            line: $line,
        }
    };
}
pub(crate) use site;

/// The arguments that followed the format string.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum Args {
    /// The `logErrorFromErrno` form, which takes a message and no varargs.
    FromErrno,
    /// Two `%lld` conversions.
    TwoLongLong(c_longlong, c_longlong),
    /// One `%s` conversion. Null is recorded as `None`.
    OneString(Option<String>),
}

/// One recorded call, in the shape the C function would have received it.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct Record {
    /// `LOG_ERROR`, `LOG_DEBUG` or `LOG_TRACE`.
    pub level: c_int,
    /// The `__FILENAME__` of the original call site.
    pub file: String,
    /// The `__FUNCTION__` of the original call site.
    pub function: String,
    /// The `__LINE__` of the original call site.
    pub line: c_int,
    /// The message, for the errno form, or the format string otherwise.
    pub msg: String,
    /// What followed it.
    pub args: Args,
}

#[cfg(test)]
thread_local! {
    static RECORDED: std::cell::RefCell<Vec<Record>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Clears the recorded calls and returns what had accumulated on this thread.
#[cfg(test)]
pub(crate) fn take() -> Vec<Record> {
    RECORDED.with(|r| core::mem::take(&mut *r.borrow_mut()))
}

#[cfg(test)]
fn record(level: c_int, site: Site, msg: &CStr, args: Args) {
    let rec = Record {
        level,
        file: site.file.to_string_lossy().into_owned(),
        function: site.function.to_string_lossy().into_owned(),
        line: site.line,
        msg: msg.to_string_lossy().into_owned(),
        args,
    };
    RECORDED.with(|r| r.borrow_mut().push(rec));
}

/// What the `logErrorFromErrno` macro expands to.
pub(crate) fn error_from_errno(msg: &'static CStr, site: Site) {
    #[cfg(test)]
    record(LOG_ERROR, site, msg, Args::FromErrno);
    #[cfg(not(test))]
    // SAFETY: all four pointers are to 'static NUL-terminated strings, and
    // logMessageFromErrno only reads them.
    unsafe {
        pharo_vm_sys::logMessageFromErrno(
            LOG_ERROR,
            msg.as_ptr(),
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
        );
    }
}

/// `logError(fmt, a, b)` where `fmt` has exactly two `%lld` conversions.
pub(crate) fn error_two_longlong(fmt: &'static CStr, site: Site, a: c_longlong, b: c_longlong) {
    #[cfg(test)]
    record(LOG_ERROR, site, fmt, Args::TwoLongLong(a, b));
    #[cfg(not(test))]
    // SAFETY: the format string is a literal with exactly two %lld
    // conversions, and exactly two c_longlong arguments follow it.
    unsafe {
        pharo_vm_sys::logMessage(
            LOG_ERROR,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            a,
            b,
        );
    }
}

/// `logDebug(fmt, s)` / `logTrace(fmt, s)` where `fmt` has exactly one `%s`.
///
/// # Safety
///
/// `s`, if non-null, must be a NUL-terminated string valid for the call.
/// A null is passed straight through, as the C did: glibc prints "(null)",
/// which is what the original produced for the same input.
pub(crate) unsafe fn message_one_string(
    level: c_int,
    fmt: &'static CStr,
    site: Site,
    s: *const c_char,
) {
    #[cfg(test)]
    {
        let recorded = if s.is_null() {
            None
        } else {
            // SAFETY: delegated to the caller by this function's contract.
            Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
        };
        record(level, site, fmt, Args::OneString(recorded));
    }
    #[cfg(not(test))]
    // SAFETY: the format string is a literal with exactly one %s conversion,
    // and exactly one string pointer follows it.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            s,
        );
    }
}
