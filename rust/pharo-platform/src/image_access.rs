//! Replaces `src/imageAccess.c` on Unix.
//!
//! Every read and write of the image file goes through the `FileAccessHandler`
//! defined here. The generated interpreter never calls these functions by
//! name: `imageAccess.h` turns `sqImageFileRead(...)` and friends into
//! `currentFileAccessHandler()->imageFileRead(...)`, so the vtable below is a
//! live ABI compiled into `cointerp.c`, and the indirection is also the seam
//! the image-format tests use to swap in their own handler.
//!
//! # Scope
//!
//! Unix only, for the same reason as [`crate::path_utilities`]: `imageFileOpen`
//! and `imageFileExists` have `_WIN32` branches that go through
//! `vm_string_convert_utf8_to_utf16` and `_wfopen`/`_wstat`, and porting those
//! blind would be guesswork. `cmake/rust.cmake` keeps compiling the C there.
//!
//! # Handles are C `FILE *`
//!
//! `sqImageFile` is `void *`, and the only thing that makes it meaningful is
//! that every function in the vtable agrees on what it points to. The C picked
//! `FILE *`, and a handler installed by a test may well hand one of *these*
//! handles back to one of *those* functions, so this module speaks C stdio via
//! `libc` rather than reaching for `std::fs`. `imageFilePosition` returning
//! `ftell` also means the buffered-stream position is observable.
//!
//! # Faithful oddities
//!
//! Kept deliberately, per the porting rule in `rust/README.md`:
//!
//! * `basicImageFileWrite` returns `bytesToWrite` on success rather than the
//!   `wroteBytes` it accumulated, and on a short write returns
//!   `lastWriteBytes + wroteBytes` -- inconsistent with the read path, which
//!   returns the short count alone. Both reproduced.
//! * The trailing `if (bytesToWrite != wroteBytes)` in the write path cannot
//!   fire: the loop only exits once `wroteBytes >= bytesToWrite`. Kept so the
//!   two paths still read alike.
//! * The `logError` in the write path says "Error reading expected to write".
//!   Copied verbatim, so log output stays diffable against the C build.
//! * `showOutputInConsole` is `false` and nothing in the tree assigns it, so
//!   `basicImageReportProgress` has always returned immediately. The bar it
//!   would have drawn is ported anyway, because that is a decision for a
//!   behaviour commit and not for this one.
//! * The C's `#ifdef posix_fadvise` block never compiled: on glibc
//!   `posix_fadvise` is a function, not a macro, so the preprocessor test is
//!   always false. There is no advice call here either -- adding one would be
//!   a change, not a port.
//!
//! Log lines carry the *C* file name, function name and line number. That
//! looks wrong for a Rust file, and is deliberate: differential testing
//! compares stderr between the two builds, and it can only do that if the
//! `logError` output is byte-identical.

use core::ffi::{c_char, c_int, c_long, c_longlong, c_void, CStr};

use pharo_vm_sys::{sqInt, FileAccessHandler};

/// Read and write chunk size, 128 Kb.
///
/// The C wrote `#define CHUNK_SIZE 128 * 1024` with no parentheses. Every use
/// site is a comparison or a ternary arm, so the missing parentheses never
/// changed a result; the value is the same either way.
///
/// The size itself comes from the analysis of `cp`/`cat` disk access linked in
/// the original: <https://eklitzke.org/efficient-file-copying-on-linux>
const CHUNK_SIZE: usize = 128 * 1024;

/// Width of the progress bar, the C's `BARLENGTH`.
const BAR_LENGTH: usize = 50;

/// The C's `showOutputInConsole`, a file-static that nothing ever assigns.
static SHOW_OUTPUT_IN_CONSOLE: bool = false;

/// The C's `progressText`, likewise never assigned.
static PROGRESS_TEXT: &CStr = c"";

/// `LOG_ERROR` from `include/pharovm/debug.h`.
const LOG_ERROR: c_int = 1;

/// The `__FILENAME__` the C compiler would have produced for this file.
///
/// `debug.h` defines `__FILENAME__` as `__FILE__ + SOURCE_PATH_SIZE`, where
/// CMake sets `SOURCE_PATH_SIZE` to the length of the source root, leaving the
/// repo-relative path.
const C_FILE: &CStr = c"src/imageAccess.c";

/// The two `logError` shapes this module needs, behind a seam the tests can
/// observe.
///
/// `logMessage` is variadic, so it cannot be stubbed by defining it in Rust,
/// and linking `src/debug.c` into the unit-test binary would drag in the rest
/// of the platform layer. Under `cfg(test)` the calls are recorded instead,
/// which also lets the tests assert that the C-identical arguments are the ones
/// being passed.
mod logging {
    use super::{C_FILE, LOG_ERROR};
    use core::ffi::{c_int, c_longlong, CStr};

    /// One recorded call, in the shape `logMessage`/`logMessageFromErrno`
    /// would have received it.
    #[cfg(test)]
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct Record {
        /// `LOG_ERROR`, always, for this module.
        pub level: c_int,
        /// `__FILENAME__` at the original C call site.
        pub file: &'static str,
        /// The message for the errno form, or the format string otherwise.
        pub msg: &'static str,
        /// `__FUNCTION__` at the original C call site.
        pub function: &'static str,
        /// `__LINE__` at the original C call site.
        pub line: c_int,
        /// The two `%lld` arguments, absent for the errno form.
        pub args: Option<(c_longlong, c_longlong)>,
    }

    #[cfg(test)]
    thread_local! {
        static RECORDED: std::cell::RefCell<Vec<Record>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    /// Clears the recorded calls and returns what had accumulated.
    #[cfg(test)]
    pub fn take() -> Vec<Record> {
        RECORDED.with(|r| core::mem::take(&mut *r.borrow_mut()))
    }

    #[cfg(test)]
    fn record(rec: Record) {
        RECORDED.with(|r| r.borrow_mut().push(rec));
    }

    /// What the `logErrorFromErrno` macro expands to. `line` is the line in
    /// [`C_FILE`], see the module docs.
    pub fn error_from_errno(msg: &'static CStr, function: &'static CStr, line: c_int) {
        #[cfg(test)]
        {
            record(Record {
                level: LOG_ERROR,
                file: C_FILE.to_str().expect("literal"),
                msg: msg.to_str().expect("literal"),
                function: function.to_str().expect("literal"),
                line,
                args: None,
            });
        }
        #[cfg(not(test))]
        // SAFETY: all four pointers are to 'static NUL-terminated literals, and
        // logMessageFromErrno only reads them.
        unsafe {
            pharo_vm_sys::logMessageFromErrno(
                LOG_ERROR,
                msg.as_ptr(),
                C_FILE.as_ptr(),
                function.as_ptr(),
                line,
            );
        }
    }

    /// The `logError("...%lld...%lld", a, b)` shape, the only variadic form
    /// this module needs.
    pub fn error_two_longlong(
        fmt: &'static CStr,
        function: &'static CStr,
        line: c_int,
        a: c_longlong,
        b: c_longlong,
    ) {
        #[cfg(test)]
        {
            record(Record {
                level: LOG_ERROR,
                file: C_FILE.to_str().expect("literal"),
                msg: fmt.to_str().expect("literal"),
                function: function.to_str().expect("literal"),
                line,
                args: Some((a, b)),
            });
        }
        #[cfg(not(test))]
        // SAFETY: the format string is a literal with exactly two %lld
        // conversions, and exactly two c_longlong arguments follow it.
        unsafe {
            pharo_vm_sys::logMessage(
                LOG_ERROR,
                C_FILE.as_ptr(),
                function.as_ptr(),
                line,
                fmt.as_ptr(),
                a,
                b,
            );
        }
    }
}

/// Reports progress through the *current* handler, not through
/// [`basicImageReportProgress`] directly.
///
/// The C called the `sqImageReportProgress` macro, which dispatches through
/// `currentFileAccessHandler()`. A test that installed its own handler expects
/// its own progress callback to run, so the indirection is load-bearing.
fn report_progress(total_size: usize, current_size: usize) {
    // SAFETY: currentFileAccessHandler never returns null -- it is initialised
    // to &defaultFileAccessHandler and setFileAccessHandler is the only writer.
    // A handler with a null imageReportProgress would have crashed the C too;
    // Option's None arm makes that a no-op here instead, which is the one place
    // this module is deliberately kinder than the original.
    unsafe {
        let handler = currentFileAccessHandler();
        if let Some(f) = (*handler).imageReportProgress {
            f(total_size, current_size);
        }
    }
}

/// `stdout`, which the `libc` crate does not expose because C hides it behind a
/// macro. glibc and musl name the underlying object `stdout`; the BSDs and
/// macOS name it `__stdoutp`.
mod c_stdout {
    extern "C" {
        #[cfg_attr(
            any(target_os = "macos", target_os = "ios", target_os = "freebsd"),
            link_name = "__stdoutp"
        )]
        static mut stdout: *mut libc::FILE;
    }

    /// Returns the `FILE *` that C's `stdout` macro would evaluate to.
    pub fn get() -> *mut libc::FILE {
        // SAFETY: the C runtime initialises this before main and only replaces
        // it via freopen, which nothing here does.
        unsafe { stdout }
    }
}

/// Interprets a `sqImageFile` as the `FILE *` this module's handlers store in
/// it.
#[inline]
fn as_file(f: *mut c_void) -> *mut libc::FILE {
    f.cast::<libc::FILE>()
}

/// Borrows a C string as a `Path`, or `None` if the pointer is null.
///
/// The C passed a null straight to `stat`, which fails with `EFAULT` and so
/// reported "does not exist". Returning `None` here reaches the same answer
/// without the UB of `CStr::from_ptr(null)`.
///
/// # Safety
///
/// `p`, if non-null, must point to a NUL-terminated string that stays valid
/// for the duration of the call.
unsafe fn path_from_ptr<'a>(p: *const c_char) -> Option<&'a std::path::Path> {
    use std::os::unix::ffi::OsStrExt;

    if p.is_null() {
        return None;
    }
    // SAFETY: delegated to the caller by this function's contract.
    let bytes = unsafe { CStr::from_ptr(p) }.to_bytes();
    Some(std::path::Path::new(std::ffi::OsStr::from_bytes(bytes)))
}

/// Closes an image file. Returns what `fclose` returned.
///
/// # Safety
///
/// `f` must be a `FILE *` obtained from [`basicImageFileOpen`] and not yet
/// closed.
#[no_mangle]
pub unsafe extern "C" fn basicImageFileClose(f: *mut c_void) -> sqInt {
    // SAFETY: delegated to the caller.
    unsafe { libc::fclose(as_file(f)) as sqInt }
}

/// Opens `file_name` with `mode`, as `fopen` would. Null on failure.
///
/// # Safety
///
/// Both arguments must be NUL-terminated strings valid for the call. `mode` is
/// `char *` rather than `const char *` because that is how `imageAccess.h`
/// declares the vtable slot; it is not written.
#[no_mangle]
pub unsafe extern "C" fn basicImageFileOpen(
    file_name: *const c_char,
    mode: *mut c_char,
) -> *mut c_void {
    // SAFETY: delegated to the caller.
    unsafe { libc::fopen(file_name, mode).cast::<c_void>() }
}

/// Returns the stream position, as `ftell` would.
///
/// # Safety
///
/// `f` must be an open handle from [`basicImageFileOpen`].
#[no_mangle]
pub unsafe extern "C" fn basicImageFilePosition(f: *mut c_void) -> c_long {
    // SAFETY: delegated to the caller.
    unsafe { libc::ftell(as_file(f)) }
}

/// Reads `sz * count` bytes into `initial_ptr`, in 128 Kb chunks once the
/// request exceeds one chunk, reporting progress after each.
///
/// Returns the number of bytes read -- note *bytes*, not items, on the chunked
/// path, whereas the single-`fread` fast path returns an item count as `fread`
/// does. The two agree only when `sz` is 1. That inconsistency is the C's, and
/// is safe in practice because the image reader always reads bytes.
///
/// # Safety
///
/// `initial_ptr` must be writable for `sz * count` bytes and `f` must be an
/// open handle.
#[no_mangle]
pub unsafe extern "C" fn basicImageFileRead(
    initial_ptr: *mut c_void,
    sz: usize,
    count: usize,
    f: *mut c_void,
) -> usize {
    // C computed `sz * count` in size_t, which wraps rather than trapping.
    let bytes_to_read = sz.wrapping_mul(count);

    if bytes_to_read <= CHUNK_SIZE {
        // SAFETY: delegated to the caller.
        return unsafe { libc::fread(initial_ptr, sz, count, as_file(f)) };
    }

    let mut read_bytes: usize = 0;
    let mut remaining_bytes = bytes_to_read;
    let mut current_ptr = initial_ptr.cast::<u8>();

    loop {
        let chunk_to_read = remaining_bytes.min(CHUNK_SIZE);

        // SAFETY: current_ptr has advanced by exactly the bytes already read,
        // so chunk_to_read <= remaining_bytes bytes are still in bounds.
        let last_read_bytes =
            unsafe { libc::fread(current_ptr.cast::<c_void>(), 1, chunk_to_read, as_file(f)) };

        if last_read_bytes < chunk_to_read {
            logging::error_from_errno(c"fread", c"basicImageFileRead", 105);
            return last_read_bytes;
        }

        read_bytes += last_read_bytes;
        // SAFETY: as above -- the sum of all last_read_bytes never exceeds
        // bytes_to_read, so this stays within the caller's buffer (one past the
        // end at most, which is a valid pointer to form).
        current_ptr = unsafe { current_ptr.add(last_read_bytes) };
        remaining_bytes -= last_read_bytes;

        report_progress(bytes_to_read, read_bytes);

        // The C's `while(lastReadBytes > 0 && readBytes < bytesToRead)`. The
        // short-read arm above already returned, so the first half can only be
        // false when chunk_to_read was 0, which cannot happen here.
        if !(last_read_bytes > 0 && read_bytes < bytes_to_read) {
            break;
        }
    }

    if bytes_to_read != read_bytes {
        logging::error_two_longlong(
            c"Error reading expected to read: %lld actual read:%lld",
            c"basicImageFileRead",
            118,
            bytes_to_read as c_longlong,
            read_bytes as c_longlong,
        );
    }

    read_bytes
}

/// Seeks to `pos` from the start of the file.
///
/// # Safety
///
/// `f` must be an open handle from [`basicImageFileOpen`].
#[no_mangle]
pub unsafe extern "C" fn basicImageFileSeek(f: *mut c_void, pos: c_long) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe { libc::fseek(as_file(f), pos, libc::SEEK_SET) }
}

/// Seeks to `pos` relative to the end of the file.
///
/// # Safety
///
/// `f` must be an open handle from [`basicImageFileOpen`].
#[no_mangle]
pub unsafe extern "C" fn basicImageFileSeekEnd(f: *mut c_void, pos: c_long) -> c_int {
    // SAFETY: delegated to the caller.
    unsafe { libc::fseek(as_file(f), pos, libc::SEEK_END) }
}

/// Writes `sz * count` bytes from `initial_ptr`, in 128 Kb chunks once the
/// request exceeds one chunk, reporting progress after each.
///
/// See the module docs for the two return-value oddities preserved here.
///
/// # Safety
///
/// `initial_ptr` must be readable for `sz * count` bytes and `f` must be an
/// open handle.
#[no_mangle]
pub unsafe extern "C" fn basicImageFileWrite(
    initial_ptr: *mut c_void,
    sz: usize,
    count: usize,
    f: *mut c_void,
) -> usize {
    let bytes_to_write = sz.wrapping_mul(count);

    if bytes_to_write <= CHUNK_SIZE {
        // SAFETY: delegated to the caller.
        return unsafe { libc::fwrite(initial_ptr, sz, count, as_file(f)) };
    }

    let mut wrote_bytes: usize = 0;
    let mut remaining_bytes = bytes_to_write;
    let mut current_ptr = initial_ptr.cast::<u8>();

    loop {
        let chunk_to_write = remaining_bytes.min(CHUNK_SIZE);

        // SAFETY: current_ptr has advanced by exactly the bytes already
        // written, so chunk_to_write bytes are still in bounds.
        let last_write_bytes =
            unsafe { libc::fwrite(current_ptr.cast::<c_void>(), 1, chunk_to_write, as_file(f)) };

        if last_write_bytes != chunk_to_write {
            logging::error_from_errno(c"fwrite", c"basicImageFileWrite", 153);
            // Verbatim: the read path returns the short count on its own, this
            // one adds the running total to it.
            return last_write_bytes + wrote_bytes;
        }

        wrote_bytes += chunk_to_write;
        // SAFETY: as above.
        current_ptr = unsafe { current_ptr.add(last_write_bytes) };
        remaining_bytes -= last_write_bytes;

        report_progress(bytes_to_write, wrote_bytes);

        if bytes_to_write <= wrote_bytes {
            break;
        }
    }

    if bytes_to_write != wrote_bytes {
        logging::error_two_longlong(
            c"Error reading expected to write: %lld actual wrote:%lld",
            c"basicImageFileWrite",
            166,
            bytes_to_write as c_longlong,
            wrote_bytes as c_longlong,
        );
    }

    // Verbatim: the C returned the requested count, not the accumulated one.
    bytes_to_write
}

/// Non-zero if `a_path` names something that `stat` can see.
///
/// # Safety
///
/// `a_path`, if non-null, must be a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn basicImageFileExists(a_path: *const c_char) -> c_int {
    // `std::fs::metadata` is `stat(2)` with the large-file variant picked
    // correctly for the target, which matters on 32-bit hosts where the C was
    // compiled with _FILE_OFFSET_BITS=64 and a hand-rolled `libc::stat` would
    // not be. Both follow symlinks and both report failure as "does not exist".
    // SAFETY: delegated to the caller.
    match unsafe { path_from_ptr(a_path) } {
        Some(path) => std::fs::metadata(path).is_ok() as c_int,
        None => 0,
    }
}

/// Draws the image load/save progress bar on stdout.
///
/// A no-op in every build: see the module docs on `showOutputInConsole`.
// The `if total_size != 0 { .. / total_size }` shape is the C's own guard, kept
// so the two read alike; `checked_div` would say the same thing less plainly.
#[allow(clippy::manual_checked_ops)]
#[no_mangle]
pub extern "C" fn basicImageReportProgress(total_size: usize, current_size: usize) {
    if !SHOW_OUTPUT_IN_CONSOLE {
        return;
    }

    if total_size != 0 {
        // size_t arithmetic, wrapping as the C's did, then narrowed to int for
        // the %d conversion.
        let percentage = (current_size.wrapping_mul(100) / total_size) as c_int;

        let mut bar = [0u8; BAR_LENGTH + 1];
        for (i, cell) in bar[..BAR_LENGTH].iter_mut().enumerate() {
            // `100 / BARLENGTH` is integer division in the C too, so the step
            // is 2, not 2.0.
            let threshold = ((i + 1) * (100 / BAR_LENGTH)) as c_int;
            *cell = if percentage >= threshold { b'#' } else { b'-' };
        }

        // SAFETY: the format string has %s, %s, %d and is given two
        // NUL-terminated byte strings and an int. `bar` keeps its final NUL
        // because the loop only writes the first BAR_LENGTH cells.
        unsafe {
            libc::printf(
                c"\r%s: [%s] %d%%".as_ptr(),
                PROGRESS_TEXT.as_ptr(),
                bar.as_ptr(),
                percentage,
            );
        }
    } else {
        // SAFETY: one %s, one NUL-terminated byte string.
        unsafe {
            libc::printf(c"\r%s...".as_ptr(), PROGRESS_TEXT.as_ptr());
        }
    }

    if total_size <= current_size {
        // SAFETY: a literal format string with no conversions.
        unsafe {
            libc::printf(c"\n".as_ptr());
        }
    }

    // SAFETY: stdout is always a valid stream.
    unsafe {
        libc::fflush(c_stdout::get());
    }
}

/// Non-zero if `a_path` names a directory.
///
/// # Safety
///
/// `a_path`, if non-null, must be a NUL-terminated string valid for the call.
#[no_mangle]
pub unsafe extern "C" fn basicImageIsDirectory(a_path: *const c_char) -> c_int {
    // SAFETY: delegated to the caller.
    match unsafe { path_from_ptr(a_path) } {
        // The C returned 0 when stat failed, then S_ISDIR of the mode.
        Some(path) => std::fs::metadata(path).map_or(0, |m| m.is_dir() as c_int),
        None => 0,
    }
}

/// The handler installed at startup, wired to the `basicImage*` functions
/// above.
///
/// `static mut` rather than `static` on purpose: the C definition was a
/// writable global, and a plain `static` with function-pointer initialisers
/// lands in `.data.rel.ro`, which would fault an embedder that assigns to it.
#[no_mangle]
pub static mut defaultFileAccessHandler: FileAccessHandler = FileAccessHandler {
    imageFileClose: Some(basicImageFileClose),
    imageFileOpen: Some(basicImageFileOpen),
    imageFilePosition: Some(basicImageFilePosition),
    imageFileRead: Some(basicImageFileRead),
    imageFileSeek: Some(basicImageFileSeek),
    imageFileSeekEnd: Some(basicImageFileSeekEnd),
    imageFileWrite: Some(basicImageFileWrite),
    imageFileExists: Some(basicImageFileExists),
    imageReportProgress: Some(basicImageReportProgress),
    imageIsDirectory: Some(basicImageIsDirectory),
};

/// The handler in force. Read through [`currentFileAccessHandler`], replaced
/// through [`setFileAccessHandler`].
#[no_mangle]
pub static mut fileAccessHandler: *mut FileAccessHandler =
    core::ptr::addr_of_mut!(defaultFileAccessHandler);

/// Returns the handler in force. Never null.
///
/// # Safety
///
/// Not synchronised, exactly as the C was not. Callers must not race a
/// [`setFileAccessHandler`] against this; in practice the handler is swapped
/// only by single-threaded test setup.
#[no_mangle]
pub unsafe extern "C" fn currentFileAccessHandler() -> *mut FileAccessHandler {
    // SAFETY: see this function's contract.
    unsafe { fileAccessHandler }
}

/// Installs `a_file_access_handler` as the handler in force.
///
/// # Safety
///
/// The handler must outlive every subsequent image access. See
/// [`currentFileAccessHandler`] on the absent synchronisation.
#[no_mangle]
pub unsafe extern "C" fn setFileAccessHandler(a_file_access_handler: *mut FileAccessHandler) {
    // SAFETY: see this function's contract.
    unsafe {
        fileAccessHandler = a_file_access_handler;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard};

    /// Serialises the tests that swap the global handler.
    ///
    /// `fileAccessHandler` is process-wide and cargo runs tests in threads, so
    /// anything touching it has to take this first. The C had no lock either;
    /// this exists for the test harness, not for the port.
    static HANDLER_LOCK: Mutex<()> = Mutex::new(());

    fn lock_handler() -> MutexGuard<'static, ()> {
        HANDLER_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A file that removes itself when the test ends.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pharo-image-access-{}-{tag}-{n}",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }

        fn c_path(&self) -> CString {
            use std::os::unix::ffi::OsStrExt;
            CString::new(self.0.as_os_str().as_bytes()).expect("temp path has no interior NUL")
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Opens through the exported entry point, as the C macros would.
    fn open(file: &TempFile, mode: &str) -> *mut c_void {
        let path = file.c_path();
        let mut mode = CString::new(mode)
            .expect("mode has no interior NUL")
            .into_bytes_with_nul();
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let f = unsafe { basicImageFileOpen(path.as_ptr(), mode.as_mut_ptr().cast::<c_char>()) };
        assert!(!f.is_null(), "could not open {}", file.0.display());
        f
    }

    fn close(f: *mut c_void) {
        // SAFETY: f came from `open` and is closed exactly once.
        assert_eq!(unsafe { basicImageFileClose(f) }, 0);
    }

    fn write_all(f: *mut c_void, bytes: &[u8]) -> usize {
        // SAFETY: the slice is readable for its whole length.
        unsafe { basicImageFileWrite(bytes.as_ptr() as *mut c_void, 1, bytes.len(), f) }
    }

    fn read_into(f: *mut c_void, buf: &mut [u8]) -> usize {
        // SAFETY: the slice is writable for its whole length.
        unsafe { basicImageFileRead(buf.as_mut_ptr().cast::<c_void>(), 1, buf.len(), f) }
    }

    /// Bytes with enough structure that a misplaced chunk boundary shows up.
    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    #[test]
    fn small_writes_and_reads_round_trip() {
        let _ = logging::take();
        let file = TempFile::new("small");
        let data = pattern(1024);

        let f = open(&file, "wb");
        // Under one chunk, so this is the plain `fwrite` path and the return
        // value is an item count.
        assert_eq!(write_all(f, &data), data.len());
        close(f);

        let f = open(&file, "rb");
        let mut buf = vec![0u8; data.len()];
        assert_eq!(read_into(f, &mut buf), data.len());
        close(f);

        assert_eq!(buf, data);
        assert!(logging::take().is_empty(), "the happy path logs nothing");
    }

    #[test]
    fn chunked_writes_and_reads_round_trip() {
        let _ = logging::take();
        let file = TempFile::new("chunked");
        // Deliberately not a multiple of CHUNK_SIZE, so the last chunk is short.
        let data = pattern(CHUNK_SIZE * 2 + 7777);

        let f = open(&file, "wb");
        assert_eq!(write_all(f, &data), data.len());
        close(f);

        assert_eq!(
            std::fs::metadata(&file.0).expect("written").len(),
            data.len() as u64
        );

        let f = open(&file, "rb");
        let mut buf = vec![0u8; data.len()];
        assert_eq!(read_into(f, &mut buf), data.len());
        close(f);

        assert_eq!(buf, data);
        assert!(logging::take().is_empty(), "the happy path logs nothing");
    }

    #[test]
    fn a_short_chunked_read_returns_only_the_last_chunk() {
        let _ = logging::take();
        let file = TempFile::new("short");
        // One full chunk plus a bit, then a request for three chunks.
        let on_disk = pattern(CHUNK_SIZE + 4096);
        std::fs::write(&file.0, &on_disk).expect("write");

        let f = open(&file, "rb");
        let mut buf = vec![0u8; CHUNK_SIZE * 3];
        let got = read_into(f, &mut buf);
        close(f);

        // Verbatim C behaviour: the first chunk succeeded and was counted into
        // `readBytes`, but the short second chunk returns *its own* count and
        // discards the running total. A caller that trusted this would silently
        // lose the first 128 Kb.
        assert_eq!(got, 4096);
        assert_eq!(
            logging::take(),
            vec![logging::Record {
                level: LOG_ERROR,
                file: "src/imageAccess.c",
                msg: "fread",
                function: "basicImageFileRead",
                line: 105,
                args: None,
            }]
        );
    }

    #[test]
    fn seek_and_position_track_the_stream() {
        let file = TempFile::new("seek");
        let data = pattern(4096);
        std::fs::write(&file.0, &data).expect("write");

        let f = open(&file, "rb");
        // SAFETY: f is open for the whole block.
        unsafe {
            assert_eq!(basicImageFilePosition(f), 0);

            assert_eq!(basicImageFileSeek(f, 100), 0);
            assert_eq!(basicImageFilePosition(f), 100);

            let mut byte = [0u8; 1];
            assert_eq!(read_into(f, &mut byte), 1);
            assert_eq!(byte[0], data[100]);
            assert_eq!(basicImageFilePosition(f), 101);

            // SEEK_END with a negative offset, which is how the image reader
            // finds the trailer.
            assert_eq!(basicImageFileSeekEnd(f, -8), 0);
            assert_eq!(basicImageFilePosition(f), data.len() as c_long - 8);
        }
        close(f);
    }

    #[test]
    fn exists_and_is_directory_agree_with_stat() {
        let file = TempFile::new("exists");
        let missing = file.c_path();

        // SAFETY: every path below is a NUL-terminated string that outlives
        // the call.
        unsafe {
            assert_eq!(basicImageFileExists(missing.as_ptr()), 0);
            assert_eq!(basicImageIsDirectory(missing.as_ptr()), 0);

            std::fs::write(&file.0, b"x").expect("write");
            assert_eq!(basicImageFileExists(missing.as_ptr()), 1);
            assert_eq!(basicImageIsDirectory(missing.as_ptr()), 0);

            let dir = CString::new(std::env::temp_dir().to_str().expect("utf8"))
                .expect("temp dir has no interior NUL");
            assert_eq!(basicImageFileExists(dir.as_ptr()), 1);
            assert_eq!(basicImageIsDirectory(dir.as_ptr()), 1);
        }
    }

    #[test]
    fn a_null_path_is_reported_as_absent() {
        // The C reached `stat(NULL, ...)`, which fails with EFAULT and so
        // answered "no" without crashing. Same answer, no UB.
        // SAFETY: a null path is explicitly allowed by both functions.
        unsafe {
            assert_eq!(basicImageFileExists(core::ptr::null()), 0);
            assert_eq!(basicImageIsDirectory(core::ptr::null()), 0);
        }
    }

    #[test]
    fn the_default_handler_is_installed_and_every_slot_works_through_it() {
        let _guard = lock_handler();

        let file = TempFile::new("vtable");
        let path = file.c_path();
        let mut wb = *b"wb\0";
        let mut rb = *b"rb\0";
        let data = pattern(2048);

        // SAFETY: the guard keeps the other handler test out, and every
        // pointer below outlives the call it is passed to.
        unsafe {
            let handler = currentFileAccessHandler();
            assert_eq!(handler, core::ptr::addr_of_mut!(defaultFileAccessHandler));
            let h = &*handler;

            // Exercising the slots rather than comparing their addresses:
            // `imageAccess.h` indexes this struct by position, so a slot
            // swapped with its neighbour would still type-check. Only calling
            // through catches that -- and function-pointer equality could not
            // be trusted to catch it anyway, since two functions may share an
            // address.
            assert_eq!(h.imageFileExists.unwrap()(path.as_ptr()), 0);

            let f = h.imageFileOpen.unwrap()(path.as_ptr(), wb.as_mut_ptr().cast::<c_char>());
            assert!(!f.is_null());
            assert_eq!(
                h.imageFileWrite.unwrap()(data.as_ptr() as *mut c_void, 1, data.len(), f),
                data.len()
            );
            assert_eq!(h.imageFilePosition.unwrap()(f), data.len() as c_long);
            assert_eq!(h.imageFileClose.unwrap()(f), 0);

            assert_eq!(h.imageFileExists.unwrap()(path.as_ptr()), 1);
            assert_eq!(h.imageIsDirectory.unwrap()(path.as_ptr()), 0);

            let f = h.imageFileOpen.unwrap()(path.as_ptr(), rb.as_mut_ptr().cast::<c_char>());
            assert!(!f.is_null());

            assert_eq!(h.imageFileSeekEnd.unwrap()(f, -4), 0);
            assert_eq!(h.imageFilePosition.unwrap()(f), data.len() as c_long - 4);

            assert_eq!(h.imageFileSeek.unwrap()(f, 512), 0);
            let mut buf = vec![0u8; 256];
            assert_eq!(
                h.imageFileRead.unwrap()(buf.as_mut_ptr().cast::<c_void>(), 1, buf.len(), f),
                buf.len()
            );
            assert_eq!(buf, data[512..768]);
            assert_eq!(h.imageFileClose.unwrap()(f), 0);

            // The progress slot is the one the chunk loops dispatch through.
            assert!(h.imageReportProgress.is_some());
            h.imageReportProgress.unwrap()(data.len(), data.len());
        }
    }

    /// Counts calls made to the handler installed by the test below.
    static PROGRESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static LAST_PROGRESS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn counting_progress(total_size: usize, current_size: usize) {
        assert_ne!(total_size, 0);
        PROGRESS_CALLS.fetch_add(1, Ordering::SeqCst);
        LAST_PROGRESS.store(current_size, Ordering::SeqCst);
    }

    #[test]
    fn progress_is_reported_through_the_installed_handler() {
        let _guard = lock_handler();
        PROGRESS_CALLS.store(0, Ordering::SeqCst);
        LAST_PROGRESS.store(0, Ordering::SeqCst);

        let file = TempFile::new("progress");
        // Three chunks exactly, so the callback count is predictable.
        let data = pattern(CHUNK_SIZE * 3);

        // SAFETY: the guard serialises this against the other handler test, and
        // `mine` outlives the window in which it is installed.
        unsafe {
            let mut mine = defaultFileAccessHandler;
            mine.imageReportProgress = Some(counting_progress);

            let previous = currentFileAccessHandler();
            setFileAccessHandler(&mut mine);

            let f = open(&file, "wb");
            assert_eq!(write_all(f, &data), data.len());
            close(f);

            let after_write = PROGRESS_CALLS.swap(0, Ordering::SeqCst);

            let f = open(&file, "rb");
            let mut buf = vec![0u8; data.len()];
            assert_eq!(read_into(f, &mut buf), data.len());
            close(f);

            let after_read = PROGRESS_CALLS.load(Ordering::SeqCst);
            setFileAccessHandler(previous);

            // This is the point of the test: the chunk loops dispatch through
            // `currentFileAccessHandler()`, not through
            // `basicImageReportProgress` directly, so a replaced handler sees
            // the callbacks.
            assert_eq!(after_write, 3, "one per 128 Kb chunk written");
            assert_eq!(after_read, 3, "one per 128 Kb chunk read");
            assert_eq!(LAST_PROGRESS.load(Ordering::SeqCst), data.len());
            assert_eq!(buf, data);
        }
    }

    #[test]
    fn report_progress_is_silent_while_the_console_flag_is_off() {
        // Not a behaviour anyone should rely on, but it is the behaviour the C
        // shipped: `showOutputInConsole` is false and nothing assigns it, so
        // this call writes nothing and, in particular, does not divide by
        // `total_size` of 0.
        assert!(!SHOW_OUTPUT_IN_CONSOLE);
        basicImageReportProgress(0, 0);
        basicImageReportProgress(100, 50);
    }
}
