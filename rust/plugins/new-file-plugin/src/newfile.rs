//! The `NewFile_*` / `NewDirectory_*` API, in Rust.
//!
//! This replaces `plugins/NewFilePlugin/src/unix/UnixFile.c`. The signatures
//! are the ones `plugins/NewFilePlugin/include/common/NewFile.h` declares
//! (`PHARO_NEWFILE_EXPORT`), and every function is exported `extern "C"` under
//! its C name so any caller that resolved these symbols against the C plugin
//! finds them here unchanged.
//!
//! The behaviour is the C's, syscall for syscall: same `open(2)` flag
//! mapping, same 0644 creation mode, same error conventions (`NULL`, `-1`,
//! `0` or `false`, each where the C used it). Oddities are reproduced and
//! marked `Faithful oddity`; the only divergences are the removal of two
//! pieces of undefined behaviour, marked `Divergence` (and listed in the
//! crate README).

use core::ffi::{c_char, c_int};
use core::mem::MaybeUninit;
use core::ptr;
use core::slice;
use std::ffi::c_void;

// ---------------------------------------------------------------------------
// The enums from NewFile.h, as the integers they travel as.
// ---------------------------------------------------------------------------

/// `NewFileOpenModeReadOnly`.
pub const OPEN_MODE_READ_ONLY: c_int = 0;
/// `NewFileOpenModeWriteOnly`.
pub const OPEN_MODE_WRITE_ONLY: c_int = 1;
/// `NewFileOpenModeReadWrite`.
pub const OPEN_MODE_READ_WRITE: c_int = 2;

/// `NewFileCreationDispositionCreateNew`: create, failing if it exists.
pub const CREATION_CREATE_NEW: c_int = 1;
/// `NewFileCreationDispositionCreateAlways`: create or truncate.
pub const CREATION_CREATE_ALWAYS: c_int = 2;
/// `NewFileCreationDispositionOpenExisting`: open, failing if missing.
pub const CREATION_OPEN_EXISTING: c_int = 3;
/// `NewFileCreationDispositionOpenAlways`: open or create.
pub const CREATION_OPEN_ALWAYS: c_int = 4;
/// `NewFileCreationDispositionTruncateExisting`: truncate, failing if missing.
pub const CREATION_TRUNCATE_EXISTING: c_int = 5;

/// `NewFileOpenFlagsAppend`.
pub const OPEN_FLAGS_APPEND: c_int = 1;

/// `NewFileSeekModeSet`.
pub const SEEK_MODE_SET: c_int = 0;
/// `NewFileSeekModeCurrent`.
pub const SEEK_MODE_CURRENT: c_int = 1;
/// `NewFileSeekModeEnd`.
pub const SEEK_MODE_END: c_int = 2;

/// `NewFileMemMapProtectionReadOnly`.
pub const MMAP_PROT_READ_ONLY: c_int = 0;
/// `NewFileMemMapProtectionReadWrite`.
pub const MMAP_PROT_READ_WRITE: c_int = 1;

// ---------------------------------------------------------------------------
// The two opaque records the image holds pointers to.
// ---------------------------------------------------------------------------

/// Mirrors the C's `struct NewFile_s`.
///
/// The struct is opaque to every caller -- `NewFile.h` only forward-declares
/// it -- but the layout is kept field-for-field identical to the C anyway, so
/// a handle is byte-compatible even for code that (wrongly) peeked inside.
#[repr(C)]
pub struct NewFile {
    file_descriptor: c_int,
    memory_map_count: c_int,
    memory_map_length: usize,
    memory_map_address: *mut c_void,
}

/// Mirrors the C's `struct NewDirectory_s`.
#[repr(C)]
pub struct NewDirectory {
    handle: *mut libc::DIR,
}

/// The C's `makeCStringWithFixedString`: a NUL-terminated copy of a
/// fixed-length byte string. An embedded NUL truncates the path at the
/// syscall, exactly as the C copy would.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes (it may be anything,
/// including not NUL-terminated) unless `path_size` is 0.
unsafe fn terminated_path(path: *const c_char, path_size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(path_size + 1);
    if path_size > 0 {
        // SAFETY: caller guarantees `path_size` readable bytes.
        buf.extend_from_slice(unsafe { slice::from_raw_parts(path.cast::<u8>(), path_size) });
    }
    buf.push(0);
    buf
}

// ---------------------------------------------------------------------------
// Directories
// ---------------------------------------------------------------------------

/// Creates a directory with mode 0755. Answers whether `mkdir(2)` succeeded.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_create(path: *const c_char, path_size: usize) -> bool {
    let cpath = unsafe { terminated_path(path, path_size) };
    // SAFETY: cpath is a live, NUL-terminated local buffer.
    unsafe { libc::mkdir(cpath.as_ptr().cast(), 0o755) == 0 }
}

/// Removes an empty directory. Answers whether `rmdir(2)` succeeded.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_removeEmpty(path: *const c_char, path_size: usize) -> bool {
    let cpath = unsafe { terminated_path(path, path_size) };
    // SAFETY: cpath is a live, NUL-terminated local buffer.
    unsafe { libc::rmdir(cpath.as_ptr().cast()) == 0 }
}

/// Opens a directory for enumeration, or answers NULL.
///
/// Divergence: the C built the NUL-terminated copy and then passed the
/// *original, unterminated* pointer to `opendir(3)`, which reads past the
/// buffer until it happens upon a zero byte -- undefined behaviour and,
/// with unlucky heap contents, the wrong directory. The terminated copy is
/// passed here.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_open(
    path: *const c_char,
    path_size: usize,
) -> *mut NewDirectory {
    let cpath = unsafe { terminated_path(path, path_size) };
    // SAFETY: cpath is a live, NUL-terminated local buffer.
    let handle = unsafe { libc::opendir(cpath.as_ptr().cast()) };
    if handle.is_null() {
        return ptr::null_mut();
    }
    Box::into_raw(Box::new(NewDirectory { handle }))
}

/// Rewinds the enumeration. Answers false only for a NULL directory.
///
/// # Safety
///
/// `directory` must be NULL or a live handle from [`NewDirectory_open`].
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_rewind(directory: *mut NewDirectory) -> bool {
    if directory.is_null() {
        return false;
    }
    // SAFETY: non-null, and open is the only producer of these handles.
    unsafe { libc::rewinddir((*directory).handle) };
    true
}

/// The next entry's name, or NULL at the end (or for a NULL directory).
///
/// The pointer aims into `readdir(3)`'s per-stream storage, exactly as the C
/// returned `entry->d_name`: it is valid until the next
/// [`NewDirectory_next`] / [`NewDirectory_close`] on the same directory.
///
/// # Safety
///
/// `directory` must be NULL or a live handle from [`NewDirectory_open`].
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_next(directory: *mut NewDirectory) -> *const c_char {
    if directory.is_null() {
        return ptr::null();
    }
    // SAFETY: non-null, and open is the only producer of these handles.
    let entry = unsafe { libc::readdir((*directory).handle) };
    if entry.is_null() {
        return ptr::null();
    }
    // SAFETY: readdir answered a valid dirent; d_name is its inline array.
    unsafe { (*entry).d_name.as_ptr() }
}

/// Closes the directory and frees the handle. NULL is a no-op.
///
/// # Safety
///
/// `directory` must be NULL or a live handle from [`NewDirectory_open`],
/// not used again afterwards.
#[no_mangle]
pub unsafe extern "C" fn NewDirectory_close(directory: *mut NewDirectory) {
    if directory.is_null() {
        return;
    }
    // SAFETY: non-null, produced by Box::into_raw in NewDirectory_open, and
    // the contract forbids further use.
    unsafe {
        libc::closedir((*directory).handle);
        drop(Box::from_raw(directory));
    }
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

/// Deletes (unlinks) a file. Answers whether `unlink(2)` succeeded.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn NewFile_deleteFile(path: *const c_char, path_size: usize) -> bool {
    let cpath = unsafe { terminated_path(path, path_size) };
    // SAFETY: cpath is a live, NUL-terminated local buffer.
    unsafe { libc::unlink(cpath.as_ptr().cast()) == 0 }
}

/// Opens a file, answering a heap handle or NULL.
///
/// The flag mapping is the C's exactly: mode picks `O_RDONLY`/`O_WRONLY`/
/// `O_RDWR` (anything else answers NULL before touching the file system),
/// the append flag adds `O_APPEND`, the creation disposition adds `O_CREAT`/
/// `O_EXCL`/`O_TRUNC` -- and, faithful oddity, an *unknown* disposition adds
/// nothing at all, behaving like `OpenExisting`. Files are created 0644.
///
/// # Safety
///
/// `path` must point to `path_size` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn NewFile_open(
    path: *const c_char,
    path_size: usize,
    mode: c_int,
    creation_disposition: c_int,
    flags: c_int,
) -> *mut NewFile {
    let mut open_flags = match mode {
        OPEN_MODE_READ_ONLY => libc::O_RDONLY,
        OPEN_MODE_WRITE_ONLY => libc::O_WRONLY,
        OPEN_MODE_READ_WRITE => libc::O_RDWR,
        _ => return ptr::null_mut(),
    };
    if flags & OPEN_FLAGS_APPEND != 0 {
        open_flags |= libc::O_APPEND;
    }
    match creation_disposition {
        CREATION_CREATE_NEW => open_flags |= libc::O_CREAT | libc::O_EXCL,
        CREATION_CREATE_ALWAYS => open_flags |= libc::O_CREAT | libc::O_TRUNC,
        CREATION_OPEN_EXISTING => {}
        CREATION_OPEN_ALWAYS => open_flags |= libc::O_CREAT,
        CREATION_TRUNCATE_EXISTING => open_flags |= libc::O_TRUNC,
        _ => {} // the C's `default: break`
    }

    let cpath = unsafe { terminated_path(path, path_size) };
    // SAFETY: cpath is a live, NUL-terminated local buffer; the mode
    // argument only matters when O_CREAT is set, as in the C.
    let fd = unsafe { libc::open(cpath.as_ptr().cast(), open_flags, 0o644 as libc::c_uint) };
    if fd < 0 {
        return ptr::null_mut();
    }
    // calloc in the C: every non-fd field starts zeroed.
    Box::into_raw(Box::new(NewFile {
        file_descriptor: fd,
        memory_map_count: 0,
        memory_map_length: 0,
        memory_map_address: ptr::null_mut(),
    }))
}

/// Closes the file descriptor and frees the handle. NULL is a no-op.
///
/// As in the C, an outstanding memory mapping is *not* unmapped here.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`], not used
/// again afterwards.
#[no_mangle]
pub unsafe extern "C" fn NewFile_close(file: *mut NewFile) {
    if file.is_null() {
        return;
    }
    // SAFETY: non-null, produced by Box::into_raw in NewFile_open, and the
    // contract forbids further use.
    unsafe {
        libc::close((*file).file_descriptor);
        drop(Box::from_raw(file));
    }
}

/// The file's size per `fstat(2)`, or -1 for NULL or on error.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_getSize(file: *mut NewFile) -> i64 {
    if file.is_null() {
        return -1;
    }
    let mut s = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: non-null handle; fstat fills `s` on success.
    if unsafe { libc::fstat((*file).file_descriptor, s.as_mut_ptr()) } != 0 {
        return -1;
    }
    // SAFETY: fstat succeeded, so `s` is initialised. st_size is off_t,
    // which is i64 on every Unix this crate builds for.
    unsafe { s.assume_init_ref().st_size }
}

/// Repositions the file offset. An unknown seek mode is a no-op, and --
/// as in the C -- the `lseek(2)` result is discarded.
///
/// Divergence: the C dereferenced a NULL `file` here (the one `NewFile_*`
/// entry point without the check); NULL is a no-op instead.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_seek(file: *mut NewFile, offset: i64, seek_mode: c_int) {
    let whence = match seek_mode {
        SEEK_MODE_SET => libc::SEEK_SET,
        SEEK_MODE_CURRENT => libc::SEEK_CUR,
        SEEK_MODE_END => libc::SEEK_END,
        _ => return,
    };
    if file.is_null() {
        return;
    }
    // SAFETY: non-null handle.
    unsafe { libc::lseek((*file).file_descriptor, offset as libc::off_t, whence) };
}

/// The current file offset. Faithful oddity: NULL answers 0 (where every
/// other query answers -1); a failed `lseek(2)` still answers its -1.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_tell(file: *mut NewFile) -> i64 {
    if file.is_null() {
        return 0;
    }
    // SAFETY: non-null handle.
    unsafe { libc::lseek((*file).file_descriptor, 0, libc::SEEK_CUR) as i64 }
}

/// Sets the file's size. Answers whether `ftruncate(2)` succeeded; NULL
/// answers false.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_truncate(file: *mut NewFile, new_file_size: u64) -> bool {
    if file.is_null() {
        return false;
    }
    // SAFETY: non-null handle. The u64 -> off_t conversion is the C's
    // implicit one.
    unsafe { libc::ftruncate((*file).file_descriptor, new_file_size as libc::off_t) == 0 }
}

/// `read(2)` into `buffer + buffer_offset`. Answers the byte count, the
/// syscall's -1 on error, or -1 for NULL.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`], and
/// `buffer + buffer_offset` must be writable for `read_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn NewFile_read(
    file: *mut NewFile,
    buffer: *mut c_void,
    buffer_offset: usize,
    read_size: usize,
) -> i64 {
    if file.is_null() {
        return -1;
    }
    let dst = buffer.cast::<u8>().wrapping_add(buffer_offset);
    // SAFETY: non-null handle; the caller vouches for the buffer span.
    unsafe { libc::read((*file).file_descriptor, dst.cast(), read_size) as i64 }
}

/// `write(2)` from `buffer + buffer_offset`. Answers the byte count, the
/// syscall's -1 on error, or -1 for NULL.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`], and
/// `buffer + buffer_offset` must be readable for `write_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn NewFile_write(
    file: *mut NewFile,
    buffer: *const c_void,
    buffer_offset: usize,
    write_size: usize,
) -> i64 {
    if file.is_null() {
        return -1;
    }
    let src = buffer.cast::<u8>().wrapping_add(buffer_offset);
    // SAFETY: non-null handle; the caller vouches for the buffer span.
    unsafe { libc::write((*file).file_descriptor, src.cast(), write_size) as i64 }
}

/// `pread(2)`: a positional read that leaves the file offset alone.
///
/// # Safety
///
/// As [`NewFile_read`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_readAtOffset(
    file: *mut NewFile,
    buffer: *mut c_void,
    buffer_offset: usize,
    read_size: usize,
    offset: u64,
) -> i64 {
    if file.is_null() {
        return -1;
    }
    let dst = buffer.cast::<u8>().wrapping_add(buffer_offset);
    // SAFETY: non-null handle; the caller vouches for the buffer span. The
    // u64 -> off_t conversion is the C's implicit one.
    unsafe {
        libc::pread(
            (*file).file_descriptor,
            dst.cast(),
            read_size,
            offset as libc::off_t,
        ) as i64
    }
}

/// `pwrite(2)`: a positional write that leaves the file offset alone.
///
/// # Safety
///
/// As [`NewFile_write`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_writeAtOffset(
    file: *mut NewFile,
    buffer: *const c_void,
    buffer_offset: usize,
    write_size: usize,
    offset: u64,
) -> i64 {
    if file.is_null() {
        return -1;
    }
    let src = buffer.cast::<u8>().wrapping_add(buffer_offset);
    // SAFETY: non-null handle; the caller vouches for the buffer span. The
    // u64 -> off_t conversion is the C's implicit one.
    unsafe {
        libc::pwrite(
            (*file).file_descriptor,
            src.cast(),
            write_size,
            offset as libc::off_t,
        ) as i64
    }
}

/// Maps the whole file `MAP_SHARED`, reference-counting repeat calls: a
/// second map answers the first mapping's address.
///
/// Faithful oddities, all the C's:
/// * the length is the file size *at first map time*, and a failed
///   `NewFile_getSize` (-1) becomes `SIZE_MAX`, so the `mmap(2)` below fails
///   and NULL is answered -- only an exactly-zero length returns early;
/// * an unknown protection maps `PROT_NONE`;
/// * a failed `mmap` leaves `MAP_FAILED` stored in the handle (harmless: the
///   count stays 0, so nothing ever unmaps it).
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_memoryMap(file: *mut NewFile, protection: c_int) -> *mut c_void {
    if file.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: non-null handle throughout; NewFile_getSize accepts it too.
    unsafe {
        if (*file).memory_map_count > 0 {
            (*file).memory_map_count += 1;
            return (*file).memory_map_address;
        }

        (*file).memory_map_length = NewFile_getSize(file) as usize;
        if (*file).memory_map_length == 0 {
            return ptr::null_mut();
        }

        let prot = match protection {
            MMAP_PROT_READ_ONLY => libc::PROT_READ,
            MMAP_PROT_READ_WRITE => libc::PROT_READ | libc::PROT_WRITE,
            _ => libc::PROT_NONE,
        };

        (*file).memory_map_address = libc::mmap(
            ptr::null_mut(),
            (*file).memory_map_length,
            prot,
            libc::MAP_SHARED,
            (*file).file_descriptor,
            0,
        );
        if (*file).memory_map_address == libc::MAP_FAILED {
            return ptr::null_mut();
        }

        (*file).memory_map_count += 1;
        (*file).memory_map_address
    }
}

/// Drops one reference to the mapping.
///
/// Faithful oddity, kept deliberately (see the README): the C's condition is
/// inverted, so the *last* unmap returns before `munmap(2)` -- the final
/// mapping is never released -- while an unmap that still leaves references
/// outstanding unmaps immediately, dangling them. Reproduced exactly so a
/// differential run against the C sees identical behaviour; "fixing" it here
/// would make pointers valid under the C dangle under Rust and vice versa.
///
/// # Safety
///
/// `file` must be NULL or a live handle from [`NewFile_open`].
#[no_mangle]
pub unsafe extern "C" fn NewFile_memoryUnmap(file: *mut NewFile) {
    // SAFETY: guarded non-null handle throughout.
    unsafe {
        if file.is_null() || (*file).memory_map_count <= 0 {
            return;
        }
        (*file).memory_map_count -= 1;
        if (*file).memory_map_count == 0 {
            return;
        }
        libc::munmap((*file).memory_map_address, (*file).memory_map_length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminated_path_appends_nul() {
        let p = b"abc";
        let copy = unsafe { terminated_path(p.as_ptr().cast(), p.len()) };
        assert_eq!(copy, b"abc\0");
    }

    #[test]
    fn terminated_path_of_empty_is_just_nul() {
        let copy = unsafe { terminated_path(ptr::null(), 0) };
        assert_eq!(copy, b"\0");
    }

    #[test]
    fn terminated_path_keeps_embedded_nul() {
        // The syscall then sees the truncated "ab", as it would in C.
        let p = b"ab\0cd";
        let copy = unsafe { terminated_path(p.as_ptr().cast(), p.len()) };
        assert_eq!(copy, b"ab\0cd\0");
    }
}
