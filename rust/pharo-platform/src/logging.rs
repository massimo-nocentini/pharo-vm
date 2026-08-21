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

use core::ffi::{c_char, c_int, c_longlong, c_void, CStr};

/// `LOG_ERROR` from `include/pharovm/debug.h`.
pub(crate) const LOG_ERROR: c_int = 1;
/// `LOG_INFO` from `include/pharovm/debug.h`.
pub(crate) const LOG_INFO: c_int = 3;
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
    /// Two `%s` conversions.
    TwoStrings(Option<String>, Option<String>),
    /// One `%s` and one `%u`.
    StringAndU32(Option<String>, u32),
    /// One `%ld`.
    OneLong(core::ffi::c_long),
    /// No conversions.
    None,
    /// One `%d` conversion.
    OneInt(c_int),
    /// One `%p` conversion, recorded as an integer.
    OnePtr(usize),
    /// The five-argument allocation summary in `memoryUnix.c`.
    AllocationSummary {
        /// The `%zu` size the caller asked for.
        requested_size: usize,
        /// The `%p` address the caller asked for.
        requested_at: usize,
        /// The `%zu` size after page alignment.
        aligned_size: usize,
        /// The `%p` address after page alignment.
        aligned_at: usize,
        /// The `%p` address actually obtained, or null.
        obtained_at: usize,
    },
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

/// `logError(fmt)` / `logDebug(fmt)` with no conversions at all.
///
/// Note that the C passed the message straight through as the format string,
/// so a `%` in it would still be interpreted. Every caller here uses a literal
/// without one.
pub(crate) fn message_no_args(level: c_int, fmt: &'static CStr, site: Site) {
    #[cfg(test)]
    record(level, site, fmt, Args::None);
    #[cfg(not(test))]
    // SAFETY: a literal format string with no conversions and no arguments
    // following it.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
        );
    }
}

/// `logError(fmt, i)` where `fmt` has exactly one `%d`.
pub(crate) fn message_one_int(level: c_int, fmt: &'static CStr, site: Site, i: c_int) {
    #[cfg(test)]
    record(level, site, fmt, Args::OneInt(i));
    #[cfg(not(test))]
    // SAFETY: one %d conversion, one c_int argument.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            i,
        );
    }
}

/// `logDebug(fmt, p)` / `logError(fmt, p)` where `fmt` has exactly one `%p`.
pub(crate) fn message_one_ptr(level: c_int, fmt: &'static CStr, site: Site, p: *const c_void) {
    #[cfg(test)]
    record(level, site, fmt, Args::OnePtr(p as usize));
    #[cfg(not(test))]
    // SAFETY: one %p conversion, one pointer argument. The pointer is only
    // formatted, never dereferenced, so it need not be valid.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            p,
        );
    }
}

/// The one `logDebug` in `memoryUnix.c` that summarises an allocation:
/// `%zu`, `%p`, `%zu`, `%p`, `%p`, in that order.
///
/// Specific rather than generic because matching a format string to a varargs
/// list has to be done by hand, and the only way to keep that honest is to do
/// it once per shape.
pub(crate) fn debug_allocation_summary(
    fmt: &'static CStr,
    site: Site,
    requested_size: usize,
    requested_at: *const c_void,
    aligned_size: usize,
    aligned_at: *const c_void,
    obtained_at: *const c_void,
) {
    #[cfg(test)]
    record(
        LOG_DEBUG,
        site,
        fmt,
        Args::AllocationSummary {
            requested_size,
            requested_at: requested_at as usize,
            aligned_size,
            aligned_at: aligned_at as usize,
            obtained_at: obtained_at as usize,
        },
    );
    #[cfg(not(test))]
    // SAFETY: the conversions are %zu %p %zu %p %p and the five arguments
    // below match them in order and type. The pointers are only formatted.
    unsafe {
        pharo_vm_sys::logMessage(
            LOG_DEBUG,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            requested_size,
            requested_at,
            aligned_size,
            aligned_at,
            obtained_at,
        );
    }
}

/// Renders a `%s` argument for the test recorder.
///
/// # Safety
///
/// `s`, if non-null, must be a NUL-terminated string valid for the call.
#[cfg(test)]
unsafe fn recorded_string(s: *const c_char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: callers of the `%s` helpers promise a NUL-terminated string.
    Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}

/// `logTrace(fmt, a, b)` and friends, where `fmt` has exactly two `%s`.
///
/// # Safety
///
/// `a` and `b`, if non-null, must be NUL-terminated strings valid for the
/// call. Nulls are passed through as the C did; glibc prints "(null)".
pub(crate) unsafe fn message_two_strings(
    level: c_int,
    fmt: &'static CStr,
    site: Site,
    a: *const c_char,
    b: *const c_char,
) {
    #[cfg(test)]
    {
        // SAFETY: `recorded_string` requires the caller's contract, which this
        // function's own contract passes on.
        let args = Args::TwoStrings(unsafe { recorded_string(a) }, unsafe { recorded_string(b) });
        record(level, site, fmt, args);
    }
    #[cfg(not(test))]
    // SAFETY: two %s conversions, two string pointers.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            a,
            b,
        );
    }
}

/// `logDebug(fmt, s, n)` where `fmt` has one `%s` then one `%u`.
///
/// The `%u` takes a `c_uint`, which is how `parameters.c` prints a `size_t`
/// count. That is a mismatched conversion in the C, and passing a `u32` here
/// is what reproduces it rather than fixing it.
///
/// # Safety
///
/// `s`, if non-null, must be a NUL-terminated string valid for the call.
pub(crate) unsafe fn message_string_and_u32(
    level: c_int,
    fmt: &'static CStr,
    site: Site,
    s: *const c_char,
    n: u32,
) {
    #[cfg(test)]
    {
        // SAFETY: delegated to the caller by this function's contract.
        let args = Args::StringAndU32(unsafe { recorded_string(s) }, n);
        record(level, site, fmt, args);
    }
    #[cfg(not(test))]
    // SAFETY: one %s and one %u, matched by a string pointer and a c_uint.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            s,
            n as core::ffi::c_uint,
        );
    }
}

/// `logDebug(fmt, n)` / `logInfo(fmt, n)` where `fmt` has exactly one `%ld`.
pub(crate) fn message_one_long(level: c_int, fmt: &'static CStr, site: Site, n: core::ffi::c_long) {
    #[cfg(test)]
    record(level, site, fmt, Args::OneLong(n));
    #[cfg(not(test))]
    // SAFETY: one %ld conversion, one c_long argument.
    unsafe {
        pharo_vm_sys::logMessage(
            level,
            site.file.as_ptr(),
            site.function.as_ptr(),
            site.line,
            fmt.as_ptr(),
            n,
        );
    }
}
