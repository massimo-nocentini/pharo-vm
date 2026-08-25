//! The SQFile record and the `sqFile*` support API, ported from
//! `plugins/FilePlugin/src/unix/sqFilePluginBasicPrims.c`.
//!
//! The record lives *inside an image-side ByteArray*: `primitiveFileOpen`
//! allocates a ByteArray of `sizeof(SQFile)` bytes and hands its first
//! indexable byte to these functions as a `SQFile *`. Other plugins
//! (UnixOSProcessPlugin, FileAttributesPlugin) reach into the same bytes, so
//! the layout must stay bit-for-bit what the C compiler produced -- the
//! layout test in `tests/layout.rs` pins it.
//!
//! Everything here speaks C stdio (`libc::fopen` family), never `std::fs`:
//! the interpreter and other plugins share the `FILE *` these records hold.

use libc::{c_char, c_int, c_void, FILE};
use pharo_vm_plugin::proxy::sqInt;

use crate::cstdio;
use crate::vmcalls::{self, fail, resolve_filename, succeed, this_session};

/// `fileOffset_t` is `uint64_t` (include/pharovm/imageAccess.h).
#[allow(non_camel_case_types)]
pub type fileOffset_t = u64;

/// The file record stored in an image-side ByteArray.
///
/// Mirrors the non-ACORN branch of the C struct in
/// `plugins/FilePlugin/include/common/FilePlugin.h`:
///
/// ```c
/// typedef struct {
///   int   sessionID;   /* ikp: must be first */
///   void *file;
///   char  writable;
///   char  lastOp;      /* 0 = uncommitted, 1 = read, 2 = write */
///   char  lastChar;    /* one character peek for stdin */
///   char  isStdioStream;
/// } SQFile;
/// ```
#[repr(C)]
pub struct SQFile {
    pub sessionID: c_int,
    pub file: *mut c_void,
    pub writable: c_char,
    pub lastOp: c_char,
    pub lastChar: c_char,
    pub isStdioStream: c_char,
}

impl SQFile {
    /// An all-zero record: no session, no file, nothing committed.
    pub const fn zeroed() -> Self {
        Self {
            sessionID: 0,
            file: core::ptr::null_mut(),
            writable: 0,
            lastOp: 0,
            lastChar: 0,
            isStdioStream: 0,
        }
    }
}

/// `lastOp` values.
pub const UNCOMMITTED: c_char = 0;
pub const READ_OP: c_char = 1;
pub const WRITE_OP: c_char = 2;

const EOF_CHAR: c_char = libc::EOF as c_char; // -1

// ---------------------------------------------------------------------------
// Unaligned field access
//
// The C guards the pointer-sized `file` member with memcpy when
// OBJECTS_32BIT_ALIGNED, because the record sits in a byte array that is only
// guaranteed word alignment on 32-bit images. Reading every field unaligned
// costs nothing on the platforms this VM targets and removes the question.
// ---------------------------------------------------------------------------

/// # Safety
/// `f` must point at (possibly unaligned) storage of at least
/// `size_of::<SQFile>()` readable bytes.
unsafe fn get_file(f: *const SQFile) -> *mut FILE {
    // SAFETY: caller contract; read_unaligned tolerates any alignment.
    unsafe { core::ptr::addr_of!((*f).file).read_unaligned().cast() }
}

/// # Safety
/// As [`get_file`], with the bytes writable.
unsafe fn set_file(f: *mut SQFile, file: *mut FILE) {
    // SAFETY: caller contract.
    unsafe { core::ptr::addr_of_mut!((*f).file).write_unaligned(file.cast()) }
}

/// # Safety
/// As [`get_file`].
unsafe fn get_session(f: *const SQFile) -> c_int {
    // SAFETY: caller contract.
    unsafe { core::ptr::addr_of!((*f).sessionID).read_unaligned() }
}

/// # Safety
/// As [`set_file`].
unsafe fn set_session(f: *mut SQFile, id: c_int) {
    // SAFETY: caller contract.
    unsafe { core::ptr::addr_of_mut!((*f).sessionID).write_unaligned(id) }
}

macro_rules! byte_field {
    ($get:ident, $set:ident, $field:ident) => {
        /// # Safety
        /// As [`get_file`] / [`set_file`].
        unsafe fn $get(f: *const SQFile) -> c_char {
            // SAFETY: caller contract; single-byte fields have no alignment.
            unsafe { core::ptr::addr_of!((*f).$field).read() }
        }
        /// # Safety
        /// As [`set_file`].
        unsafe fn $set(f: *mut SQFile, v: c_char) {
            // SAFETY: caller contract.
            unsafe { core::ptr::addr_of_mut!((*f).$field).write(v) }
        }
    };
}

byte_field!(get_writable, set_writable, writable);
byte_field!(get_last_op, set_last_op, lastOp);
byte_field!(get_last_char, set_last_char, lastChar);
byte_field!(get_is_stdio, set_is_stdio, isStdioStream);

fn errno() -> c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `open(path, flags)` retried on EINTR, as the C's `openFileWithFlags`.
fn open_with_flags(path: *const c_char, flags: c_int) -> c_int {
    loop {
        // SAFETY: `path` is a NUL-terminated buffer owned by the caller.
        let fd = unsafe { libc::open(path, flags) };
        if fd >= 0 || errno() != libc::EINTR {
            return fd;
        }
    }
}

/// `open(path, flags, mode)` retried on EINTR.
fn open_with_flags_in_mode(path: *const c_char, flags: c_int, mode: libc::mode_t) -> c_int {
    loop {
        // SAFETY: as in `open_with_flags`.
        let fd = unsafe { libc::open(path, flags, libc::c_uint::from(mode)) };
        if fd >= 0 || errno() != libc::EINTR {
            return fd;
        }
    }
}

/// `fdopen` retried on EINTR, as the C's `openFileDescriptor`.
fn open_file_descriptor(fd: c_int, mode: &'static [u8]) -> *mut FILE {
    debug_assert!(mode.ends_with(b"\0"));
    loop {
        // SAFETY: `fd` is a file descriptor the caller owns; `mode` is a
        // NUL-terminated literal.
        let file = unsafe { libc::fdopen(fd, mode.as_ptr().cast()) };
        if !file.is_null() || errno() != libc::EINTR {
            return file;
        }
    }
}

/// The C's `getSize`: measure by seeking to the end and back, errors ignored
/// exactly as the C ignored them.
///
/// # Safety
/// `file` must be a live `FILE *`.
unsafe fn get_size(file: *mut FILE) -> fileOffset_t {
    // SAFETY: caller contract, throughout.
    unsafe {
        let current = libc::ftell(file);
        libc::fseek(file, 0, libc::SEEK_END);
        let size = libc::ftell(file);
        libc::fseek(file, current, libc::SEEK_SET);
        size as fileOffset_t
    }
}

// ---------------------------------------------------------------------------
// Exported support API (FilePlugin.h)
//
// All of these are exported with C linkage because other plugins link
// against them (cmake/plugins.cmake links FileAttributesPlugin and
// UnixOSProcessPlugin to FilePlugin). None may unwind: they use only libc
// calls and checked arithmetic, no panicking paths.
// ---------------------------------------------------------------------------

/// Is the record non-null, holding a `FILE *`, and from this session?
///
/// # Safety
/// `f` is null or points at `size_of::<SQFile>()` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileValid(f: *mut SQFile) -> sqInt {
    if f.is_null() {
        return 0;
    }
    // SAFETY: non-null per the check above; readable per the contract.
    let valid = unsafe { !get_file(f).is_null() && get_session(f) == this_session() };
    sqInt::from(valid)
}

/// True when the read/write head is at the end, Smalltalk-style: as soon as
/// the last character has been read, using a one-character peek (see the
/// comment block in the C).
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileAtEnd(f: *mut SQFile) -> sqInt {
    // SAFETY: sqFileValid's contract covers all record reads below.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        let fp = get_file(f);
        let fd = libc::fileno(fp);
        if fd == 1 || fd == 2 {
            // Can't peek write-only streams.
            0
        } else if get_is_stdio(f) != 0 {
            // We can't block waiting for interactive input.
            sqInt::from(libc::feof(fp) != 0)
        } else if libc::feof(fp) == 0 {
            // Peek: ungetc(fgetc(fp), fp) sets the eof flag without
            // advancing. File errors deliberately ignored, as the C reverted
            // to (see its comment).
            let c = libc::fgetc(fp);
            sqInt::from(libc::ungetc(c, fp) == libc::EOF && libc::feof(fp) != 0)
        } else {
            1
        }
    }
}

/// Closes the file and invalidates the record. Never retried on error.
///
/// # Safety
/// As [`sqFileValid`], with the record bytes writable.
#[no_mangle]
pub unsafe extern "C" fn sqFileClose(f: *mut SQFile) -> sqInt {
    // SAFETY: caller contract, throughout.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        let result = libc::fclose(get_file(f));
        set_file(f, core::ptr::null_mut());
        set_session(f, 0);
        set_writable(f, 0);
        set_last_op(f, UNCOMMITTED);
        // fclose() can fail for the same reasons fflush() or write() can, so
        // errors must be checked, but it must NEVER be retried.
        if result != 0 {
            return fail();
        }
        1
    }
}

/// Deletes the named file.
///
/// # Safety
/// `sq_file_name` points at `sq_file_name_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileDeleteNameSize(
    sq_file_name: *mut c_char,
    sq_file_name_size: sqInt,
) -> sqInt {
    // SAFETY: caller contract.
    let name = unsafe { name_slice(sq_file_name, sq_file_name_size) };
    let Some(cname) = name.and_then(|n| resolve_filename(n, false)) else {
        return fail();
    };
    // SAFETY: `cname` is NUL-terminated.
    if unsafe { libc::remove(cname.as_ptr().cast()) } != 0 {
        return fail();
    }
    1
}

/// The current position of the read/write head.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileGetPosition(f: *mut SQFile) -> fileOffset_t {
    // SAFETY: caller contract, throughout.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail() as fileOffset_t;
        }
        if get_is_stdio(f) != 0 && get_writable(f) == 0 {
            // One character of pushback for stdio input streams.
            return fileOffset_t::from(get_last_char(f) != EOF_CHAR);
        }
        let position = libc::ftell(get_file(f));
        if position == -1 {
            return fail() as fileOffset_t;
        }
        position as fileOffset_t
    }
}

/// Records this run's session identifier. Called once, from
/// `initialiseModule`. Zero is never a valid session number.
#[no_mangle]
pub extern "C" fn sqFileInit() -> sqInt {
    vmcalls::set_this_session_from_vm();
    1
}

/// Nothing to tear down; the C did nothing either.
#[no_mangle]
pub extern "C" fn sqFileShutdown() -> sqInt {
    1
}

/// # Safety
/// `sq_file_name` points at `size` readable bytes; negative sizes answer None.
unsafe fn name_slice<'a>(sq_file_name: *mut c_char, size: sqInt) -> Option<&'a [u8]> {
    if sq_file_name.is_null() || size < 0 {
        return None;
    }
    // SAFETY: caller contract.
    Some(unsafe { core::slice::from_raw_parts(sq_file_name.cast::<u8>(), size as usize) })
}

/// The permission bits `fopen` would create files with (rw-rw-rw-, before
/// umask), and the write-only variant (-w--w--w-), as in the C.
const CREATE_MODE_RW: libc::mode_t = 0o666;
const CREATE_MODE_W: libc::mode_t = 0o222;

/// Opens the named file, recording its state in `f`. Fails with no side
/// effects if `f` is already open. Always binary mode; the image does any
/// line-end mapping.
///
/// The open dance follows the C exactly: `open()` is used instead of
/// `fopen()` because fopen cannot express "create only if absent" or "write
/// without truncation"; see the comments inline.
///
/// # Safety
/// `f` points at a writable SQFile record; `sq_file_name` at
/// `sq_file_name_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileOpen(
    f: *mut SQFile,
    sq_file_name: *mut c_char,
    sq_file_name_size: sqInt,
    write_flag: sqInt,
) -> sqInt {
    // SAFETY: caller contract for the record...
    if unsafe { sqFileValid(f) } != 0 {
        // Don't open an already open file.
        return fail();
    }
    // SAFETY: ...and for the name bytes.
    let name = unsafe { name_slice(sq_file_name, sq_file_name_size) };
    // Can fail when alias resolution is enabled.
    let Some(cname) = name.and_then(|n| resolve_filename(n, true)) else {
        return fail();
    };
    let path = cname.as_ptr().cast::<c_char>();

    let mut mode: &'static [u8];
    let mut fd;
    if write_flag != 0 {
        let mut retried = 0;
        loop {
            mode = b"r+b\0";
            fd = open_with_flags(path, libc::O_RDWR);
            // Could have failed if we lack read permission or it didn't exist.
            if fd < 0 {
                if errno() == libc::EACCES {
                    // This does no truncation, unlike the equivalent with
                    // fopen().
                    mode = b"wb\0";
                    fd = open_with_flags(path, libc::O_WRONLY);
                } else if errno() == libc::ENOENT {
                    mode = b"r+b\0";
                    fd = open_with_flags_in_mode(
                        path,
                        libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                        CREATE_MODE_RW,
                    );
                    // Could have failed if we lack read permission or it
                    // already exists.
                    if fd < 0 && errno() == libc::EACCES {
                        mode = b"wb\0";
                        fd = open_with_flags_in_mode(
                            path,
                            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
                            CREATE_MODE_W,
                        );
                    }
                    // The C then adjusted the Mac type/creator of the new
                    // file; both halves of that are no-ops on unix (and the
                    // C's probe read an uninitialized buffer doing it), so
                    // nothing is done here.
                }
            }
            // Retry once if creating anew lost a race with another creator
            // (EEXIST under O_EXCL).
            if fd < 0 && errno() == libc::EEXIST && retried < 1 {
                retried += 1;
            } else {
                break;
            }
        }
    } else {
        mode = b"rb\0";
        fd = open_with_flags(path, libc::O_RDONLY);
    }

    if fd >= 0 {
        let file = open_file_descriptor(fd, mode);
        if !file.is_null() {
            // SAFETY: record writable per the caller contract.
            unsafe {
                set_session(f, this_session());
                set_file(f, file);
                set_writable(f, c_char::from(write_flag != 0));
                set_last_op(f, UNCOMMITTED);
            }
            return 1;
        }
        // close() the bad fd to avoid leaking file descriptors; NEVER
        // reattempt close() if it fails, even on EINTR.
        // SAFETY: `fd` is the descriptor we just opened.
        unsafe { libc::close(fd) };
    }

    // SAFETY: record writable per the caller contract.
    unsafe {
        set_session(f, 0);
        set_writable(f, 0);
    }
    fail()
}

/// Opens the named file only if it does not exist yet, for writing (and
/// reading when permitted). Sets `*exists` when the failure was "already
/// there", which `primitiveFileOpenNew` turns into `PrimErrInappropriate`.
///
/// # Safety
/// As [`sqFileOpen`], plus `exists` points at a writable `int`.
#[no_mangle]
pub unsafe extern "C" fn sqFileOpenNew(
    f: *mut SQFile,
    sq_file_name: *mut c_char,
    sq_file_name_size: sqInt,
    exists: *mut c_int,
) -> sqInt {
    // SAFETY: caller contract, throughout.
    unsafe {
        *exists = 0;
        if sqFileValid(f) != 0 {
            return fail();
        }
        let name = name_slice(sq_file_name, sq_file_name_size);
        let Some(cname) = name.and_then(|n| resolve_filename(n, true)) else {
            return fail();
        };
        let path = cname.as_ptr().cast::<c_char>();

        let mut mode: &'static [u8] = b"r+b\0";
        let mut fd = open_with_flags_in_mode(
            path,
            libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
            CREATE_MODE_RW,
        );
        // Could have failed if we lack read permission or it already exists.
        if fd < 0 && errno() == libc::EACCES {
            mode = b"wb\0";
            fd = open_with_flags_in_mode(
                path,
                libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY,
                CREATE_MODE_W,
            );
        }

        if fd >= 0 {
            // (Mac type/creator adjustment: no-op on unix, as in sqFileOpen.)
            let file = open_file_descriptor(fd, mode);
            if !file.is_null() {
                set_session(f, this_session());
                set_file(f, file);
                set_writable(f, 1);
                set_last_op(f, UNCOMMITTED);
                return 1;
            }
            libc::close(fd);
        } else if errno() == libc::EEXIST {
            *exists = 1;
        }

        set_session(f, 0);
        set_writable(f, 0);
        fail()
    }
}

/// Wraps an existing file descriptor (fdopen) into the record.
///
/// # Safety
/// `sq_file` points at a writable SQFile record; `fd` is a descriptor whose
/// access mode is compatible with `write_flag`.
#[no_mangle]
pub unsafe extern "C" fn sqConnectToFileDescriptor(
    sq_file: *mut SQFile,
    fd: c_int,
    write_flag: sqInt,
) -> sqInt {
    let file = open_file_descriptor(fd, if write_flag != 0 { b"wb\0" } else { b"rb\0" });
    if file.is_null() {
        return fail();
    }
    // SAFETY: caller contract.
    unsafe { sqConnectToFile(sq_file, file.cast(), write_flag) }
}

/// Populates the record with an existing `FILE *`.
///
/// # Safety
/// `sq_file` points at a writable SQFile record; `file` is a live `FILE *`.
#[no_mangle]
pub unsafe extern "C" fn sqConnectToFile(
    sq_file: *mut SQFile,
    file: *mut c_void,
    write_flag: sqInt,
) -> sqInt {
    // SAFETY: caller contract. Note the C assigns writeFlag to a char, so it
    // truncates; reproduced with the `as` casts.
    unsafe {
        set_file(sq_file, file.cast());
        set_session(sq_file, this_session());
        set_last_op(sq_file, UNCOMMITTED);
        set_writable(sq_file, write_flag as c_char);
    }
    1
}

/// Fills `files[0..3]` with records for stdin, stdout and stderr and answers
/// the availability bit-mask -- always 7 on unix, as in the C.
///
/// # Safety
/// `files` points at three writable SQFile records.
#[no_mangle]
pub unsafe extern "C" fn sqFileStdioHandlesInto(files: *mut SQFile) -> sqInt {
    // SAFETY: caller contract, throughout; the stdio FILE pointers come from
    // the C runtime.
    unsafe {
        let f0 = files;
        set_session(f0, this_session());
        set_file(f0, cstdio::c_stdin());
        set_writable(f0, 0);
        set_last_op(f0, READ_OP);
        set_is_stdio(
            f0,
            c_char::from(libc::isatty(libc::fileno(cstdio::c_stdin())) != 0),
        );
        set_last_char(f0, EOF_CHAR);

        let f1 = files.add(1);
        set_session(f1, this_session());
        set_file(f1, cstdio::c_stdout());
        set_writable(f1, 1);
        set_is_stdio(f1, 1);
        set_last_char(f1, EOF_CHAR);
        set_last_op(f1, WRITE_OP);

        let f2 = files.add(2);
        set_session(f2, this_session());
        set_file(f2, cstdio::c_stderr());
        set_writable(f2, 1);
        set_is_stdio(f2, 1);
        set_last_char(f2, EOF_CHAR);
        set_last_op(f2, WRITE_OP);
    }
    7
}

/// What kind of stream a descriptor is: 1 terminal, 3 regular file,
/// -1 error. (The C's comment promises 2 for pipes but its code never
/// answers it; reproduced as-is.)
#[no_mangle]
pub extern "C" fn sqFileDescriptorType(fd_num: c_int) -> sqInt {
    // SAFETY: isatty/fstat accept any descriptor value and report errors.
    unsafe {
        if libc::isatty(fd_num) != 0 {
            return 1;
        }
        let mut stat_buf: libc::stat = core::mem::zeroed();
        if libc::fstat(fd_num, &mut stat_buf) != 0 {
            return -1;
        }
    }
    3
}

/// Reads `count` bytes into the byte array at `start_index` (zero-based).
/// Stdio streams use a non-blocking OS-level `read()` so the VM thread is
/// not frozen waiting for interactive input; regular files strictly use
/// `fread()` to preserve libc's buffer (see the C's comment).
///
/// # Safety
/// `f` as [`sqFileValid`] (writable); `byte_array_index + start_index` points
/// at `count` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileReadIntoAt(
    f: *mut SQFile,
    count: usize,
    byte_array_index: *mut c_char,
    start_index: usize,
) -> usize {
    // SAFETY: caller contract, throughout.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail() as usize;
        }
        let file = get_file(f);
        if get_writable(f) != 0 {
            if get_is_stdio(f) != 0 {
                return fail() as usize;
            }
            if get_last_op(f) == WRITE_OP {
                // Seek between writing and reading, as stdio requires.
                libc::fseek(file, 0, libc::SEEK_CUR);
            }
        }

        let fd = libc::fileno(file);
        let dst = byte_array_index.add(start_index);
        let bytes_read;
        if get_is_stdio(f) != 0 {
            libc::clearerr(file);
            let original_flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, original_flags | libc::O_NONBLOCK);

            let mut n;
            loop {
                n = libc::read(fd, dst.cast(), count);
                if !(n < 0 && errno() == libc::EINTR) {
                    break;
                }
            }
            libc::fcntl(fd, libc::F_SETFL, original_flags);

            bytes_read = if n < 0 { 0 } else { n as usize };
        } else {
            // Buffered fread for regular files, retried on EINTR.
            loop {
                libc::clearerr(file);
                let n = libc::fread(dst.cast(), 1, count, file);
                if !(n == 0 && libc::ferror(file) != 0 && errno() == libc::EINTR) {
                    bytes_read = n;
                    break;
                }
            }
        }

        // Support for skipping back one character on stdio streams.
        if get_is_stdio(f) != 0 && bytes_read > 0 {
            set_last_char(f, *dst.add(bytes_read - 1));
        }
        set_last_op(f, READ_OP);
        bytes_read
    }
}

/// Renames a file.
///
/// # Safety
/// Both name pointers hold their stated number of readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileRenameOldSizeNewSize(
    sq_old_name: *mut c_char,
    sq_old_name_size: sqInt,
    sq_new_name: *mut c_char,
    sq_new_name_size: sqInt,
) -> sqInt {
    // SAFETY: caller contract.
    let (old_name, new_name) = unsafe {
        (
            name_slice(sq_old_name, sq_old_name_size),
            name_slice(sq_new_name, sq_new_name_size),
        )
    };
    let (Some(c_old), Some(c_new)) = (
        old_name.and_then(|n| resolve_filename(n, false)),
        new_name.and_then(|n| resolve_filename(n, false)),
    ) else {
        return fail();
    };
    // SAFETY: both buffers are NUL-terminated.
    if unsafe { libc::rename(c_old.as_ptr().cast(), c_new.as_ptr().cast()) } != 0 {
        return fail();
    }
    1
}

/// Moves the read/write head. On a stdio input stream only the one-character
/// pushback positions are honoured.
///
/// # Safety
/// As [`sqFileValid`] (writable record).
#[no_mangle]
pub unsafe extern "C" fn sqFileSetPosition(f: *mut SQFile, position: fileOffset_t) -> sqInt {
    // SAFETY: caller contract, throughout.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        if get_is_stdio(f) != 0 {
            // Support one character of pushback for stdio streams.
            if get_writable(f) == 0 && get_last_char(f) != EOF_CHAR {
                let current_pos: fileOffset_t = 1; // lastChar != EOF here
                if current_pos == position {
                    return 1;
                }
                if current_pos.wrapping_sub(1) == position {
                    libc::ungetc(c_int::from(get_last_char(f) as u8), get_file(f));
                    set_last_char(f, EOF_CHAR);
                    return 1;
                }
            }
            return fail();
        }
        libc::fseek(get_file(f), position as libc::c_long, libc::SEEK_SET);
        set_last_op(f, UNCOMMITTED);
    }
    1
}

/// The file's length in bytes. Meaningless (and failed) for stdio streams.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileSize(f: *mut SQFile) -> fileOffset_t {
    // SAFETY: caller contract, throughout.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail() as fileOffset_t;
        }
        if get_is_stdio(f) != 0 {
            return fail() as fileOffset_t;
        }
        get_size(get_file(f))
    }
}

/// Flushes stdio buffers. Must keep supporting read-only files for
/// historical reasons, so EBADF is ignored, as in the C.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileFlush(f: *mut SQFile) -> sqInt {
    // SAFETY: caller contract.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        if libc::fflush(get_file(f)) != 0 && errno() != libc::EBADF {
            return fail();
        }
    }
    1
}

/// Flushes kernel-level buffers of written data to disk.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileSync(f: *mut SQFile) -> sqInt {
    // SAFETY: caller contract.
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        if libc::fsync(libc::fileno(get_file(f))) != 0 {
            return fail();
        }
    }
    1
}

/// Truncates (or extends) the file to `offset` bytes.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn sqFileTruncate(f: *mut SQFile, offset: fileOffset_t) -> sqInt {
    // SAFETY: caller contract. sqFTruncate(f,o) is
    // ftruncate(fileno(f), o) on unix (sqPlatformSpecific.h).
    unsafe {
        if sqFileValid(f) == 0 {
            return fail();
        }
        libc::fflush(get_file(f));
        if libc::ftruncate(libc::fileno(get_file(f)), offset as libc::off_t) != 0 {
            return fail();
        }
    }
    1
}

/// Writes `count` bytes from the byte array at `start_index` (zero-based).
/// A short write records a failure but still answers the byte count, as the
/// C did.
///
/// # Safety
/// `f` as [`sqFileValid`] (writable record);
/// `byte_array_index + start_index` points at `count` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn sqFileWriteFromAt(
    f: *mut SQFile,
    count: usize,
    byte_array_index: *mut c_char,
    start_index: usize,
) -> usize {
    // SAFETY: caller contract, throughout.
    unsafe {
        if !(sqFileValid(f) != 0 && get_writable(f) != 0) {
            return fail() as usize;
        }
        let file = get_file(f);
        if get_last_op(f) == READ_OP {
            // Seek between reading and writing, as stdio requires.
            libc::fseek(file, 0, libc::SEEK_CUR);
        }
        let src = byte_array_index.add(start_index);
        let bytes_written = libc::fwrite(src.cast(), 1, count, file);
        if bytes_written != count {
            fail();
        }
        set_last_op(f, WRITE_OP);
        bytes_written
    }
}

/// This run's session identifier.
#[no_mangle]
pub extern "C" fn sqFileThisSession() -> sqInt {
    this_session() as sqInt
}

/// The aio read-ready callback: signal the semaphore the image registered,
/// then disable further callbacks for the descriptor.
///
/// Exported (non-static) in the C, so exported here.
///
/// # Safety
/// Called by the VM's aio poll loop with the values registered in
/// [`waitForDataonSemaphoreIndex`]; `client_data` carries the semaphore
/// index, not a pointer.
#[no_mangle]
pub unsafe extern "C" fn signalOnDataArrival(fd: sqInt, client_data: *mut c_void, _flag: c_int) {
    vmcalls::signal_semaphore_with_index(client_data as sqInt);
    if let Some(aio) = vmcalls::aio() {
        // SAFETY: resolved from the VM core's own exports.
        unsafe { (aio.disable)(fd) };
    }
}

/// Arranges for `semaphore_index` to be signalled when the file has data,
/// without blocking.
///
/// The C called the core's `aioEnable`/`aioHandle` directly (link-time
/// resolution); here they are resolved through `ioLoadFunctionFrom` and the
/// call fails cleanly if the core does not export them.
///
/// # Safety
/// As [`sqFileValid`].
#[no_mangle]
pub unsafe extern "C" fn waitForDataonSemaphoreIndex(
    file: *mut SQFile,
    semaphore_index: sqInt,
) -> sqInt {
    // SAFETY: caller contract.
    if unsafe { sqFileValid(file) } == 0 {
        return fail();
    }
    let Some(aio) = vmcalls::aio() else {
        // Divergence: the C could not be loaded at all without these
        // symbols; failing the primitive is the closest runtime analogue.
        return fail();
    };
    /// AIO_R / AIO_EXT from include/pharovm/common/aio.h.
    const AIO_R: c_int = 1 << 1;
    const AIO_EXT: c_int = 1 << 4;
    // SAFETY: `file` is valid (checked above), so its FILE* is live; the aio
    // functions come from the VM core's export table.
    unsafe {
        let fd = libc::fileno(get_file(file)) as sqInt;
        (aio.enable)(fd, semaphore_index as *mut c_void, AIO_EXT);
        (aio.handle)(fd, signalOnDataArrival, AIO_R);
    }
    succeed()
}
