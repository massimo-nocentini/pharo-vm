//! Replaces `src/pathUtilities.c` on Unix.
//!
//! Path assembly against fixed-size caller buffers -- the classic C
//! buffer-handling shape, and the reason this file was high on the port list.
//! Every function takes a `char *target` plus its size and truncates to fit.
//!
//! # Scope
//!
//! Unix only. The C file has a `#ifdef _WIN32` branch for every function, and
//! the Windows ones use different APIs (`GetCurrentDirectoryW`,
//! `FindFirstFileW`) with subtly different behaviour. Porting those without a
//! Windows machine to test on would be guesswork, so `cmake/rust.cmake` keeps
//! compiling the C file there.
//!
//! # Divergences, all of them bugs in the original
//!
//! Preserved behaviour for every input the C handled correctly. Three inputs
//! it did not:
//!
//! * `vm_path_join_into` read `first[strlen(first) - 1]` -- index `-1` when
//!   `first` is empty.
//! * Several functions computed `targetSize - 1` in `size_t`, which underflows
//!   to `SIZE_MAX` when the size is 0.
//! * `vm_path_find_files_with_extension_in_folder` tested `if (!extension)`
//!   where it plainly meant `if (!fileExtension)`, then passed the null
//!   `fileExtension` to `strcmp`. Any file without a dot in its name crashed
//!   the scan.

use core::ffi::{c_char, CStr};

use crate::error_code::VMErrorCode;

/// The platform's path separator, matching the C's `SEPARATOR_CHAR`.
const SEPARATOR: u8 = b'/';

/// Copies `s` into `target`, truncating to `target_size` including the NUL.
///
/// Mirrors the C's `strncpy(target, s, targetSize - 1); target[targetSize-1] = 0`,
/// but is a no-op rather than an out-of-bounds write when `target_size` is 0.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes.
unsafe fn write_c_string(target: *mut c_char, target_size: usize, s: &[u8]) {
    if target.is_null() || target_size == 0 {
        return;
    }
    let n = s.len().min(target_size - 1);
    // SAFETY: n < target_size, and `s` is a distinct allocation from `target`.
    unsafe {
        core::ptr::copy_nonoverlapping(s.as_ptr(), target.cast::<u8>(), n);
        *target.add(n) = 0;
    }
}

/// The bytes of a C string, or `None` for null.
///
/// # Safety
///
/// `s` must be null or a valid NUL-terminated string.
unsafe fn bytes<'a>(s: *const c_char) -> Option<&'a [u8]> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller guarantees NUL termination.
    Some(unsafe { CStr::from_ptr(s) }.to_bytes())
}

/// Is `path` absolute?
///
/// # Safety
///
/// `path` must be null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn vm_path_is_absolute_path(path: *const c_char) -> bool {
    // SAFETY: contract above.
    matches!(unsafe { bytes(path) }, Some([SEPARATOR, ..]))
}

/// Writes the process's working directory into `target`.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn vm_path_get_current_working_dir_into(
    target: *mut c_char,
    target_size: usize,
) -> VMErrorCode {
    if target.is_null() || target_size == 0 {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }
    let Ok(cwd) = std::env::current_dir() else {
        return VMErrorCode::VM_ERROR;
    };
    let bytes = path_bytes(&cwd);
    // getcwd fails with ERANGE rather than truncating, so refuse too.
    if bytes.len() >= target_size {
        return VMErrorCode::VM_ERROR;
    }
    // SAFETY: checked to fit, and target is writable for target_size.
    unsafe { write_c_string(target, target_size, bytes) };
    VMErrorCode::VM_SUCCESS
}

/// A path's bytes, without a lossy round-trip through `str`.
fn path_bytes(p: &std::path::Path) -> &[u8] {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes()
}

/// Resolves `src` against the working directory if it is relative.
///
/// A leading `./` is dropped, as in the C.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes; `src` must be a valid
/// NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn vm_path_make_absolute_into(
    target: *mut c_char,
    target_size: usize,
    src: *const c_char,
) -> VMErrorCode {
    // SAFETY: contract above.
    let Some(src_bytes) = (unsafe { bytes(src) }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if target.is_null() || target_size == 0 {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }

    if src_bytes.first() == Some(&SEPARATOR) {
        // SAFETY: as above.
        unsafe { write_c_string(target, target_size, src_bytes) };
        return VMErrorCode::VM_SUCCESS;
    }

    let Ok(cwd) = std::env::current_dir() else {
        return VMErrorCode::VM_ERROR;
    };
    let mut out = cwd.into_os_string().into_encoded_bytes();
    if out.last() != Some(&SEPARATOR) {
        out.push(SEPARATOR);
    }
    // "./foo" joins as "foo".
    let tail = src_bytes
        .strip_prefix(b"./".as_slice())
        .unwrap_or(src_bytes);
    out.extend_from_slice(tail);

    // SAFETY: as above; write_c_string truncates rather than overflowing.
    unsafe { write_c_string(target, target_size, &out) };
    VMErrorCode::VM_SUCCESS
}

/// Writes everything before the last separator in `src`.
///
/// With no separator, the whole of `src` is written -- which is what the C did,
/// odd as it reads.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes; `src` must be a valid
/// NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn vm_path_extract_dirname_into(
    target: *mut c_char,
    target_size: usize,
    src: *const c_char,
) -> VMErrorCode {
    // SAFETY: contract above.
    let Some(src_bytes) = (unsafe { bytes(src) }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if target.is_null() || target_size == 0 {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }
    let cut = match src_bytes.iter().rposition(|&b| b == SEPARATOR) {
        Some(i) => i,
        None => src_bytes.len(),
    };
    // SAFETY: as above.
    unsafe { write_c_string(target, target_size, &src_bytes[..cut]) };
    VMErrorCode::VM_SUCCESS
}

/// Writes the last path component of `src`, **including its leading
/// separator**, or the empty string if there is no separator.
///
/// Keeping the separator looks like a bug, and might be, but callers were
/// written against it: `vm_path_extract_basename_into` on `/a/b` answers `/b`,
/// not `b`. Changing that is a behaviour change, not a port.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes; `src` must be a valid
/// NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn vm_path_extract_basename_into(
    target: *mut c_char,
    target_size: usize,
    src: *const c_char,
) -> VMErrorCode {
    // SAFETY: contract above.
    let Some(src_bytes) = (unsafe { bytes(src) }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if target.is_null() || target_size == 0 {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }
    let tail = match src_bytes.iter().rposition(|&b| b == SEPARATOR) {
        Some(i) => &src_bytes[i..],
        None => &[][..],
    };
    // SAFETY: as above.
    unsafe { write_c_string(target, target_size, tail) };
    VMErrorCode::VM_SUCCESS
}

/// Joins two path fragments with exactly one separator between them.
///
/// # Safety
///
/// `target` must be writable for `target_size` bytes; `first` and `second`
/// must be valid NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn vm_path_join_into(
    target: *mut c_char,
    target_size: usize,
    first: *const c_char,
    second: *const c_char,
) -> VMErrorCode {
    // SAFETY: contract above.
    let (Some(a), Some(b)) = (unsafe { bytes(first) }, unsafe { bytes(second) }) else {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    };
    if target.is_null() || target_size == 0 {
        return VMErrorCode::VM_ERROR_NULL_POINTER;
    }

    let mut out = a.to_vec();
    // The C indexed `first[strlen(first) - 1]` unconditionally, reading
    // `first[-1]` for an empty `first`.
    if out.last() != Some(&SEPARATOR) {
        out.push(SEPARATOR);
    }
    out.extend_from_slice(b);

    // SAFETY: as above.
    unsafe { write_c_string(target, target_size, &out) };
    VMErrorCode::VM_SUCCESS
}

/// Counts files in `search_path` whose name ends with `extension`, writing the
/// first match's full path into `image_path_buffer`.
///
/// The buffer is only written when it starts out empty, matching the C's
/// `hasPreviousOutput` logic: a caller scanning several directories keeps the
/// first hit.
///
/// # Safety
///
/// `search_path` and `extension` must be valid NUL-terminated strings, and
/// `image_path_buffer` writable for `image_path_buffer_size` bytes.
#[no_mangle]
pub unsafe extern "C" fn vm_path_find_files_with_extension_in_folder(
    search_path: *const c_char,
    extension: *const c_char,
    image_path_buffer: *mut c_char,
    image_path_buffer_size: usize,
) -> usize {
    // SAFETY: contract above.
    let (Some(dir), Some(ext)) = (unsafe { bytes(search_path) }, unsafe { bytes(extension) })
    else {
        return 0;
    };
    if image_path_buffer.is_null() || image_path_buffer_size == 0 {
        return 0;
    }
    // SAFETY: the buffer is writable for image_path_buffer_size bytes and the
    // C read its first byte the same way to decide whether it already holds a
    // result.
    let mut has_previous_output = unsafe { *image_path_buffer } != 0;

    use std::os::unix::ffi::OsStrExt;
    let dir_path = std::path::Path::new(std::ffi::OsStr::from_bytes(dir));
    let Ok(entries) = std::fs::read_dir(dir_path) else {
        return 0;
    };

    let mut count = 0usize;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.as_bytes();
        // The C took strrchr(name, '.') and then tested the *wrong* pointer for
        // null, so a name with no dot reached strcmp(NULL, ...). Skipping such
        // names is what it meant to do.
        let Some(dot) = name.iter().rposition(|&b| b == b'.') else {
            continue;
        };
        if &name[dot..] != ext {
            continue;
        }

        if !has_previous_output {
            let mut full = dir.to_vec();
            full.push(SEPARATOR);
            full.extend_from_slice(name);
            // snprintf truncates; so does this.
            // SAFETY: as above.
            unsafe { write_c_string(image_path_buffer, image_path_buffer_size, &full) };
        }
        count += 1;
        has_previous_output = true;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    const N: usize = 256;

    /// Calls a `(target, size, src)` function and reads the result back.
    fn with_target<F>(size: usize, f: F) -> (VMErrorCode, String)
    where
        F: FnOnce(*mut c_char, usize) -> VMErrorCode,
    {
        let mut buf = vec![0u8; size.max(1) + 8];
        let guard_from = size.max(1);
        let rc = f(buf.as_mut_ptr().cast(), size);
        assert!(
            buf[guard_from..].iter().all(|&b| b == 0),
            "wrote past the declared buffer size"
        );
        let end = buf.iter().position(|&b| b == 0).unwrap();
        (rc, String::from_utf8(buf[..end].to_vec()).unwrap())
    }

    fn dirname(src: &str, size: usize) -> String {
        let s = CString::new(src).unwrap();
        with_target(size, |t, n| unsafe {
            vm_path_extract_dirname_into(t, n, s.as_ptr())
        })
        .1
    }

    fn basename(src: &str, size: usize) -> String {
        let s = CString::new(src).unwrap();
        with_target(size, |t, n| unsafe {
            vm_path_extract_basename_into(t, n, s.as_ptr())
        })
        .1
    }

    fn join(a: &str, b: &str, size: usize) -> String {
        let ca = CString::new(a).unwrap();
        let cb = CString::new(b).unwrap();
        with_target(size, |t, n| unsafe {
            vm_path_join_into(t, n, ca.as_ptr(), cb.as_ptr())
        })
        .1
    }

    #[test]
    fn recognises_absolute_paths() {
        let abs = CString::new("/usr/bin").unwrap();
        let rel = CString::new("bin/pharo").unwrap();
        let empty = CString::new("").unwrap();
        unsafe {
            assert!(vm_path_is_absolute_path(abs.as_ptr()));
            assert!(!vm_path_is_absolute_path(rel.as_ptr()));
            assert!(!vm_path_is_absolute_path(empty.as_ptr()));
            assert!(!vm_path_is_absolute_path(core::ptr::null()));
        }
    }

    #[test]
    fn dirname_cuts_at_the_last_separator() {
        assert_eq!(dirname("/a/b/c.image", N), "/a/b");
        assert_eq!(dirname("/a", N), "");
    }

    /// The C copied the whole string when there was no separator.
    #[test]
    fn dirname_without_a_separator_keeps_everything() {
        assert_eq!(dirname("c.image", N), "c.image");
    }

    #[test]
    fn dirname_truncates_to_the_buffer() {
        assert_eq!(dirname("/aaaa/bbbb/c.image", 6), "/aaaa");
    }

    /// Keeping the leading separator is the C's behaviour; see the docs.
    #[test]
    fn basename_keeps_its_leading_separator() {
        assert_eq!(basename("/a/b/c.image", N), "/c.image");
    }

    #[test]
    fn basename_without_a_separator_is_empty() {
        assert_eq!(basename("c.image", N), "");
    }

    #[test]
    fn join_inserts_exactly_one_separator() {
        assert_eq!(join("/a/b", "c.image", N), "/a/b/c.image");
        assert_eq!(join("/a/b/", "c.image", N), "/a/b/c.image");
    }

    /// The C read `first[-1]` here.
    #[test]
    fn join_with_an_empty_first_component() {
        assert_eq!(join("", "c.image", N), "/c.image");
    }

    #[test]
    fn join_truncates_to_the_buffer() {
        assert_eq!(join("/aaa", "bbbbbbbbbb", 8), "/aaa/bb");
    }

    /// Every one of these underflowed `targetSize - 1` in the C.
    #[test]
    fn a_zero_sized_target_writes_nothing() {
        let src = CString::new("/a/b/c").unwrap();
        let mut buf = [0u8; 8];
        unsafe {
            let p = buf.as_mut_ptr().cast();
            assert_eq!(
                vm_path_extract_dirname_into(p, 0, src.as_ptr()),
                VMErrorCode::VM_ERROR_NULL_POINTER
            );
            assert_eq!(
                vm_path_extract_basename_into(p, 0, src.as_ptr()),
                VMErrorCode::VM_ERROR_NULL_POINTER
            );
            assert_eq!(
                vm_path_join_into(p, 0, src.as_ptr(), src.as_ptr()),
                VMErrorCode::VM_ERROR_NULL_POINTER
            );
            assert_eq!(
                vm_path_make_absolute_into(p, 0, src.as_ptr()),
                VMErrorCode::VM_ERROR_NULL_POINTER
            );
        }
        assert_eq!(buf, [0u8; 8]);
    }

    #[test]
    fn make_absolute_passes_absolute_paths_through() {
        let src = CString::new("/already/absolute").unwrap();
        let (rc, out) = with_target(N, |t, n| unsafe {
            vm_path_make_absolute_into(t, n, src.as_ptr())
        });
        assert_eq!(rc, VMErrorCode::VM_SUCCESS);
        assert_eq!(out, "/already/absolute");
    }

    #[test]
    fn make_absolute_prefixes_the_working_directory() {
        let cwd = std::env::current_dir().unwrap();
        let src = CString::new("some.image").unwrap();
        let (rc, out) = with_target(1024, |t, n| unsafe {
            vm_path_make_absolute_into(t, n, src.as_ptr())
        });
        assert_eq!(rc, VMErrorCode::VM_SUCCESS);
        assert_eq!(out, format!("{}/some.image", cwd.display()));
    }

    #[test]
    fn make_absolute_drops_a_leading_dot_slash() {
        let cwd = std::env::current_dir().unwrap();
        let src = CString::new("./some.image").unwrap();
        let (_, out) = with_target(1024, |t, n| unsafe {
            vm_path_make_absolute_into(t, n, src.as_ptr())
        });
        assert_eq!(out, format!("{}/some.image", cwd.display()));
    }

    #[test]
    fn current_working_dir_is_reported() {
        let cwd = std::env::current_dir().unwrap();
        let (rc, out) = with_target(1024, |t, n| unsafe {
            vm_path_get_current_working_dir_into(t, n)
        });
        assert_eq!(rc, VMErrorCode::VM_SUCCESS);
        assert_eq!(out, cwd.display().to_string());
    }

    #[test]
    fn a_too_small_working_dir_buffer_is_an_error() {
        let (rc, _) = with_target(2, |t, n| unsafe {
            vm_path_get_current_working_dir_into(t, n)
        });
        assert_eq!(rc, VMErrorCode::VM_ERROR);
    }

    #[test]
    fn finds_files_by_extension_and_reports_the_first() {
        let dir = std::env::temp_dir().join(format!("pharo-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.image"), b"").unwrap();
        std::fs::write(dir.join("b.image"), b"").unwrap();
        std::fs::write(dir.join("c.changes"), b"").unwrap();
        // The C crashed on this one: no dot, so strrchr answered NULL and the
        // guard tested the wrong pointer.
        std::fs::write(dir.join("README"), b"").unwrap();

        let cdir = CString::new(dir.to_str().unwrap()).unwrap();
        let ext = CString::new(".image").unwrap();
        let mut buf = vec![0u8; 1024];
        let n = unsafe {
            vm_path_find_files_with_extension_in_folder(
                cdir.as_ptr(),
                ext.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
            )
        };
        assert_eq!(n, 2, "two .image files");
        let end = buf.iter().position(|&b| b == 0).unwrap();
        let found = String::from_utf8(buf[..end].to_vec()).unwrap();
        assert!(found.ends_with(".image"), "wrote a path: {found}");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_folder_finds_nothing() {
        let cdir = CString::new("/definitely/not/here").unwrap();
        let ext = CString::new(".image").unwrap();
        let mut buf = vec![0u8; 64];
        let n = unsafe {
            vm_path_find_files_with_extension_in_folder(
                cdir.as_ptr(),
                ext.as_ptr(),
                buf.as_mut_ptr().cast(),
                buf.len(),
            )
        };
        assert_eq!(n, 0);
    }
}
