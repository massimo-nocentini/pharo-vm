//! Directory operations, ported from `plugins/FilePlugin/src/unix/sqUnixFile.c`
//! (non-`__APPLE__` branches) and `src/unix/fileUtils.c`.
//!
//! The lookup interface is path based: the image supplies the directory path
//! on every call, and a one-entry cache keeps the `DIR *` of the last path
//! open so that scanning a directory entry-by-entry does not reopen it each
//! time. The cache deliberately reopens on any miss instead of rewinding:
//! entries appear to be cached on CIFS-mounted file systems, and files may
//! have been deleted between calls (see the C's comment).

use std::sync::Mutex;

use libc::c_char;
use pharo_vm_plugin::proxy::sqInt;

use crate::charconv::{sq2ux_path, ux2sq_path};
use crate::sqfile::fileOffset_t;
use crate::vmcalls;

/// Lookup statuses, as the Slang class constants.
pub const ENTRY_FOUND: sqInt = 0;
pub const NO_MORE_ENTRIES: sqInt = 1;
pub const BAD_PATH: sqInt = 2;

const DELIMITER: u8 = b'/';

/// MAXPATHLEN from sys/param.h on the platforms this VM targets.
const MAXPATHLEN: usize = 4096;

/// A directory entry's name is a `d_name`, at most 255 bytes; the Slang
/// callers pass a 256-byte buffer. The C passed MAXPATHLEN as the conversion
/// limit and relied on names being short; using the real buffer size removes
/// the latent overrun without changing behaviour for any name a kernel can
/// actually return.
pub const ENTRY_NAME_MAX: usize = 256;

/// What one lookup answers.
pub struct DirEntry {
    pub name: [u8; ENTRY_NAME_MAX],
    pub name_len: sqInt,
    pub creation_date: sqInt,
    pub modification_date: sqInt,
    pub is_directory: bool,
    pub size_if_file: fileOffset_t,
    pub posix_permissions: sqInt,
    pub is_symlink: bool,
}

impl DirEntry {
    fn empty() -> Self {
        Self {
            name: [0; ENTRY_NAME_MAX],
            name_len: 0,
            creation_date: 0,
            modification_date: 0,
            is_directory: false,
            size_if_file: 0,
            posix_permissions: 0,
            is_symlink: false,
        }
    }
}

/// The one-entry directory cache: `lastPath` / `lastPathValid` / `lastIndex`
/// / `openDir` in the C, which relied on the interpreter being
/// single-threaded. A mutex states the same assumption safely.
struct DirCache {
    last_path: Vec<u8>, // NUL-free bytes of the cached path
    valid: bool,
    last_index: sqInt,
    open_dir: *mut libc::DIR,
}

// SAFETY: the VM only calls directory primitives from the interpreter
// thread; the mutex below serialises any other caller. The raw DIR* is never
// used off-thread while another thread holds the lock.
unsafe impl Send for DirCache {}

static CACHE: Mutex<DirCache> = Mutex::new(DirCache {
    last_path: Vec::new(),
    valid: false,
    last_index: -1,
    open_dir: core::ptr::null_mut(),
});

fn lock_cache() -> std::sync::MutexGuard<'static, DirCache> {
    // A poisoned lock means a panic elsewhere; the cache state is still
    // sound (worst case: a stale DIR* that the next miss closes).
    CACHE.lock().unwrap_or_else(|e| e.into_inner())
}

impl DirCache {
    /// `sqCloseDir`: close and invalidate.
    fn close(&mut self) {
        if self.valid {
            // SAFETY: `open_dir` came from opendir and is closed only here.
            unsafe { libc::closedir(self.open_dir) };
        }
        self.valid = false;
        self.last_index = -1;
        self.last_path.clear();
        self.open_dir = core::ptr::null_mut();
    }

    /// `maybeOpenDir`: reuse the cached `DIR *` for the same path, otherwise
    /// close the old one and open the new.
    fn maybe_open(&mut self, unix_path: &[u8]) -> bool {
        if !self.valid || self.last_path != unix_path {
            // Invalidate the old, open the new.
            if self.valid {
                // SAFETY: as in `close`.
                unsafe { libc::closedir(self.open_dir) };
            }
            self.valid = false;
            self.last_path.clear();
            self.last_path.extend_from_slice(unix_path);
            let mut c_path = unix_path.to_vec();
            c_path.push(0);
            // SAFETY: `c_path` is NUL-terminated.
            let dir = unsafe { libc::opendir(c_path.as_ptr().cast()) };
            if dir.is_null() {
                return false;
            }
            self.open_dir = dir;
            self.valid = true;
            self.last_index = 0; // first entry is index 1
        }
        true
    }
}

/// Converts a Squeak path to a NUL-free unix path, as every entry point did
/// with `sq2uxPath(..., MAXPATHLEN, 1)`. An empty path means ".".
fn to_unix_path(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() {
        return Some(b".".to_vec());
    }
    let mut buf = vec![0u8; MAXPATHLEN + 1];
    let n = sq2ux_path(path, &mut buf, MAXPATHLEN, true);
    if n == 0 {
        // The C treated a zero-length conversion as failure (`!sq2uxPath`).
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

/// `readdir` retried on EINTR, skipping `.` and `..` -- the body of the C's
/// entry loop. Answers the entry name, or `None` at the end of the stream.
///
/// # Safety
/// `dir` is a live `DIR *`.
unsafe fn next_real_entry(dir: *mut libc::DIR) -> Option<Vec<u8>> {
    loop {
        let entry = loop {
            // SAFETY: caller contract.
            unsafe {
                crate::errno::clear();
                let e = libc::readdir(dir);
                if e.is_null() && crate::errno::get() == libc::EINTR {
                    continue;
                }
                break e;
            }
        };
        if entry.is_null() {
            return None;
        }
        // SAFETY: readdir answers a valid entry with a NUL-terminated d_name.
        let name = unsafe {
            let p = core::ptr::addr_of!((*entry).d_name).cast::<c_char>();
            let len = libc::strlen(p);
            core::slice::from_raw_parts(p.cast::<u8>(), len).to_vec()
        };
        // Ignore '.' and '..' (these are not *guaranteed* to be first).
        if name == b"." || name == b".." {
            continue;
        }
        return Some(name);
    }
}

/// Stats `dir_path/name` (following symlinks first, falling back to lstat as
/// the C does) and fills the entry's date/size/mode fields. Answers whether
/// a stat succeeded; on failure the fields keep their defaults.
fn stat_into(entry: &mut DirEntry, dir_path: &[u8], name: &[u8]) -> bool {
    let mut full = Vec::with_capacity(dir_path.len() + 1 + name.len() + 1);
    full.extend_from_slice(dir_path);
    full.push(DELIMITER);
    full.extend_from_slice(name);
    full.push(0);

    // SAFETY: `full` is NUL-terminated; stat writes only into stat_buf.
    let (ok, stat_buf) = unsafe {
        let mut stat_buf: libc::stat = core::mem::zeroed();
        let ok = libc::stat(full.as_ptr().cast(), &mut stat_buf) == 0
            || libc::lstat(full.as_ptr().cast(), &mut stat_buf) == 0;
        (ok, stat_buf)
    };
    if !ok {
        return false;
    }

    // "creation date" is the last status change time, as in the C.
    entry.creation_date = convertToSqueakTime(stat_buf.st_ctime) as sqInt;
    entry.modification_date = convertToSqueakTime(stat_buf.st_mtime) as sqInt;
    if (stat_buf.st_mode & libc::S_IFMT) == libc::S_IFDIR {
        entry.is_directory = true;
    } else {
        entry.size_if_file = stat_buf.st_size as fileOffset_t;
    }
    // stat() follows symlinks, so this is only ever true when stat failed
    // and lstat resolved a dangling link -- exactly the C's behaviour.
    entry.is_symlink = (stat_buf.st_mode & libc::S_IFMT) == libc::S_IFLNK;
    entry.posix_permissions = (stat_buf.st_mode & 0o777) as sqInt;
    true
}

/// The pure-Rust core of `dir_Lookup`: the index-th entry (1-based) of the
/// directory at `path`.
pub fn lookup(path: &[u8], index: sqInt) -> Result<DirEntry, sqInt> {
    let Some(unix_path) = to_unix_path(path) else {
        return Err(BAD_PATH);
    };

    let mut cache = lock_cache();
    let mut index = index;
    let mut cache_hit = false;
    if cache.valid {
        cache.last_index += 1;
        if cache.last_index == index && cache.last_path == unix_path {
            // We can re-use the cached open directory. We want the next
            // entry, so reset index.
            index = 1;
            cache_hit = true;
        }
    }
    if !cache_hit {
        // The directory must be opened or reopened. We can't just rewind:
        // entries appear to be cached on CIFS mounts, and files may have
        // been deleted between calls.
        cache.close();
        if !cache.maybe_open(&unix_path) {
            return Err(BAD_PATH);
        }
        cache.last_index = index;
    }

    let mut name = None;
    for _ in 0..index {
        // SAFETY: the cache holds a live DIR* while `valid`.
        name = unsafe { next_real_entry(cache.open_dir) };
        if name.is_none() {
            return Err(NO_MORE_ENTRIES);
        }
    }
    let name = name.ok_or(NO_MORE_ENTRIES)?; // index <= 0: no entry read

    let mut entry = DirEntry::empty();
    entry.name_len = ux2sq_path(&name, &mut entry.name, ENTRY_NAME_MAX, false);
    if name.len() > MAXPATHLEN || unix_path.len() + 1 + name.len() > MAXPATHLEN {
        return Err(BAD_PATH);
    }
    // A stat failure does not fail the lookup: failing here would invalidate
    // the whole directory (see the C's comment). The fields stay defaulted.
    stat_into(&mut entry, &unix_path, &name);
    Ok(entry)
}

/// The pure-Rust core of `dir_EntryLookup`: the named entry of the directory
/// at `path`.
pub fn entry_lookup(path: &[u8], name: &[u8]) -> Result<DirEntry, sqInt> {
    let Some(unix_path) = to_unix_path(path) else {
        return Err(BAD_PATH);
    };
    if name.len() > MAXPATHLEN || unix_path.len() + 1 + name.len() > MAXPATHLEN {
        return Err(BAD_PATH);
    }

    // Unlike dir_Lookup, a stat failure here means "no such entry".
    let mut entry = DirEntry::empty();
    if !stat_into(&mut entry, &unix_path, name) {
        return Err(NO_MORE_ENTRIES);
    }

    // To match the results of dir_Lookup, copy back the file name. The C
    // passed a 256-byte limit here explicitly.
    entry.name_len = ux2sq_path(name, &mut entry.name, ENTRY_NAME_MAX, false);
    Ok(entry)
}

// ---------------------------------------------------------------------------
// Exported C API (FilePlugin.h)
// ---------------------------------------------------------------------------

/// Ensures the cached open directory is closed. Exported (non-static) in the
/// C on non-Apple unix.
#[no_mangle]
pub extern "C" fn sqCloseDir() {
    lock_cache().close();
}

/// Creates a directory, rwxrwxrwx before umask.
///
/// # Safety
/// `path_string` points at `path_string_length` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn dir_Create(path_string: *mut c_char, path_string_length: sqInt) -> sqInt {
    let Some(path) = (
        // SAFETY: caller contract.
        unsafe { bytes_arg(path_string, path_string_length) }
    ) else {
        return 0;
    };
    if path.len() >= MAXPATHLEN {
        return 0;
    }
    let Some(mut unix_path) = to_unix_path_nonempty(path) else {
        return 0;
    };
    unix_path.push(0);
    // SAFETY: NUL-terminated.
    sqInt::from(unsafe { libc::mkdir(unix_path.as_ptr().cast(), 0o777) } == 0)
}

/// Deletes a directory, dropping it from the lookup cache first.
///
/// # Safety
/// As [`dir_Create`].
#[no_mangle]
pub unsafe extern "C" fn dir_Delete(path_string: *mut c_char, path_string_length: sqInt) -> sqInt {
    let Some(path) = (
        // SAFETY: caller contract.
        unsafe { bytes_arg(path_string, path_string_length) }
    ) else {
        return 0;
    };
    if path.len() >= MAXPATHLEN {
        return 0;
    }
    let Some(mut unix_path) = to_unix_path_nonempty(path) else {
        return 0;
    };
    {
        let mut cache = lock_cache();
        if cache.valid && cache.last_path == unix_path {
            cache.close();
        }
    }
    unix_path.push(0);
    // SAFETY: NUL-terminated.
    sqInt::from(unsafe { libc::rmdir(unix_path.as_ptr().cast()) } == 0)
}

/// The path delimiter, `/`.
#[no_mangle]
pub extern "C" fn dir_Delimitor() -> sqInt {
    sqInt::from(DELIMITER)
}

/// dir_Create/dir_Delete convert without the empty-path-means-"." rule the
/// lookups apply.
fn to_unix_path_nonempty(path: &[u8]) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; MAXPATHLEN + 1];
    let n = sq2ux_path(path, &mut buf, MAXPATHLEN, true);
    if n == 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

/// # Safety
/// `ptr` points at `len` readable bytes (negative `len` answers None).
unsafe fn bytes_arg<'a>(ptr: *mut c_char, len: sqInt) -> Option<&'a [u8]> {
    if ptr.is_null() || len < 0 {
        return None;
    }
    // SAFETY: caller contract.
    Some(unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) })
}

/// Writes a [`DirEntry`] through the C out-parameters.
///
/// # Safety
/// Every out pointer is non-null and writable; `name` holds at least
/// [`ENTRY_NAME_MAX`] writable bytes.
#[allow(clippy::too_many_arguments)]
unsafe fn write_outputs(
    entry: &DirEntry,
    name: *mut c_char,
    name_length: *mut sqInt,
    creation_date: *mut sqInt,
    modification_date: *mut sqInt,
    is_directory: *mut sqInt,
    size_if_file: *mut fileOffset_t,
    posix_permissions: *mut sqInt,
    is_symlink: *mut sqInt,
) {
    // SAFETY: caller contract, throughout. The C wrote `*name = 0` as a
    // default, then up to `name_len` converted bytes; the same bytes are
    // written here, never the buffer's full capacity.
    unsafe {
        *name = 0;
        let n = (entry.name_len.max(0) as usize).min(ENTRY_NAME_MAX);
        core::ptr::copy_nonoverlapping(entry.name.as_ptr(), name.cast::<u8>(), n);
        *name_length = entry.name_len;
        *creation_date = entry.creation_date;
        *modification_date = entry.modification_date;
        *is_directory = sqInt::from(entry.is_directory);
        *size_if_file = entry.size_if_file;
        *posix_permissions = entry.posix_permissions;
        *is_symlink = sqInt::from(entry.is_symlink);
    }
}

/// Looks up the index-th entry (1-based) of the directory at the given path.
/// Answers 0 (found), 1 (fewer than `index` entries) or 2 (bad path).
///
/// # Safety
/// `path_string` points at `path_string_length` readable bytes; the out
/// pointers as in [`write_outputs`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dir_Lookup(
    path_string: *mut c_char,
    path_string_length: sqInt,
    index: sqInt,
    name: *mut c_char,
    name_length: *mut sqInt,
    creation_date: *mut sqInt,
    modification_date: *mut sqInt,
    is_directory: *mut sqInt,
    size_if_file: *mut fileOffset_t,
    posix_permissions: *mut sqInt,
    is_symlink: *mut sqInt,
) -> sqInt {
    // SAFETY: caller contract.
    let path = unsafe { bytes_arg(path_string, path_string_length) };
    let Some(path) = path else { return BAD_PATH };
    let (entry, status) = match lookup(path, index) {
        Ok(entry) => (entry, ENTRY_FOUND),
        // Defaults are written even on failure, as the C initialised its
        // outputs before doing anything else.
        Err(status) => (DirEntry::empty(), status),
    };
    // SAFETY: caller contract.
    unsafe {
        write_outputs(
            &entry,
            name,
            name_length,
            creation_date,
            modification_date,
            is_directory,
            size_if_file,
            posix_permissions,
            is_symlink,
        );
    }
    status
}

/// Looks up the named entry of the directory at the given path. Statuses as
/// [`dir_Lookup`], with 1 meaning "no such entry".
///
/// # Safety
/// As [`dir_Lookup`], plus `name_string` points at `name_string_length`
/// readable bytes.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn dir_EntryLookup(
    path_string: *mut c_char,
    path_string_length: sqInt,
    name_string: *mut c_char,
    name_string_length: sqInt,
    name: *mut c_char,
    name_length: *mut sqInt,
    creation_date: *mut sqInt,
    modification_date: *mut sqInt,
    is_directory: *mut sqInt,
    size_if_file: *mut fileOffset_t,
    posix_permissions: *mut sqInt,
    is_symlink: *mut sqInt,
) -> sqInt {
    // SAFETY: caller contract.
    let (path, entry_name) = unsafe {
        (
            bytes_arg(path_string, path_string_length),
            bytes_arg(name_string, name_string_length),
        )
    };
    let (Some(path), Some(entry_name)) = (path, entry_name) else {
        return BAD_PATH;
    };
    let (entry, status) = match entry_lookup(path, entry_name) {
        Ok(entry) => (entry, ENTRY_FOUND),
        Err(status) => (DirEntry::empty(), status),
    };
    // SAFETY: caller contract.
    unsafe {
        write_outputs(
            &entry,
            name,
            name_length,
            creation_date,
            modification_date,
            is_directory,
            size_if_file,
            posix_permissions,
            is_symlink,
        );
    }
    status
}

/// Unix files are untyped: setting the Mac type/creator is a successful
/// no-op, as in the C.
///
/// # Safety
/// No pointer is dereferenced.
#[no_mangle]
pub unsafe extern "C" fn dir_SetMacFileTypeAndCreator(
    _filename: *mut c_char,
    _filename_size: sqInt,
    _f_type: *mut c_char,
    _f_creator: *mut c_char,
) -> sqInt {
    1
}

/// As [`dir_SetMacFileTypeAndCreator`]: a successful no-op. Note the C never
/// writes the out buffers, and neither does this.
///
/// # Safety
/// No pointer is dereferenced.
#[no_mangle]
pub unsafe extern "C" fn dir_GetMacFileTypeAndCreator(
    _filename: *mut c_char,
    _filename_size: sqInt,
    _f_type: *mut c_char,
    _f_creator: *mut c_char,
) -> sqInt {
    1
}

/// Rebinds stdout to /dev/tty, a debugging aid the C exported.
#[no_mangle]
pub extern "C" fn sqStdoutToDevTTY() {
    // SAFETY: constant NUL-terminated strings; stdout is the C runtime's.
    unsafe {
        // The C logged the errno on failure; this plugin has no logging
        // channel, so the failure is silent.
        libc::freopen(
            c"/dev/tty".as_ptr(),
            c"w".as_ptr(),
            crate::cstdio::c_stdout(),
        );
    }
}

/// Converts a unix time to Squeak's epoch (Jan 1, 1901) plus the VM's GMT
/// offset. From `src/unix/fileUtils.c`; exported because it is non-static
/// there and other platform sources call it.
///
/// 17 leap years and 52 non-leap years separate the epochs.
#[no_mangle]
pub extern "C" fn convertToSqueakTime(unix_time: libc::time_t) -> libc::time_t {
    const EPOCH_DELTA: i64 = (52 * 365 + 17 * 366) * 24 * 60 * 60;
    unix_time
        .wrapping_add(EPOCH_DELTA as libc::time_t)
        .wrapping_add(vmcalls::vm_gmt_offset() as libc::time_t)
}
