//! Replaces `src/stringUtilities.c`.
//!
//! Two small C string helpers, and the first place in the port where being
//! faithful to the C would mean reproducing a buffer overflow. The rule used
//! here, and stated in each function's docs: **behaviour is preserved exactly
//! for inputs the C handled correctly, and made safe for the inputs where the
//! C wrote out of bounds.** Silently corrupting the heap is not behaviour worth
//! preserving.

use core::ffi::{c_char, CStr};

/// Appends `source` onto the NUL-terminated string in `dest`, truncating to
/// fit `dest_buffer_size` bytes including the terminator.
///
/// This is `strncat` with the size meaning the *buffer* rather than the number
/// of characters to copy.
///
/// # Divergence from the C
///
/// The original computed `destBufferSize - 1` in `size_t`, so:
///
/// * `destBufferSize == 0` underflowed to `SIZE_MAX` and the loop copied the
///   whole of `source` past the end of the buffer;
/// * if `dest` already held `destBufferSize` or more bytes, the loop body never
///   ran but `dest[destIndex] = 0` still wrote at that out-of-bounds index.
///
/// Both are out-of-bounds writes. Here they are no-ops: with no room there is
/// nothing correct to write.
///
/// # Safety
///
/// `dest` must point to a writable buffer of at least `dest_buffer_size` bytes
/// containing a NUL-terminated string, and `source` must be a valid
/// NUL-terminated string that does not overlap the `dest` buffer. Neither may
/// be null.
#[no_mangle]
pub unsafe extern "C" fn vm_string_append_into(
    dest: *mut c_char,
    source: *const c_char,
    dest_buffer_size: usize,
) {
    if dest.is_null() || source.is_null() || dest_buffer_size == 0 {
        return;
    }
    // SAFETY: the caller guarantees a NUL-terminated string in a buffer of at
    // least dest_buffer_size bytes.
    let used = unsafe { libc::strnlen(dest, dest_buffer_size) };
    if used >= dest_buffer_size {
        // No room even for the terminator. The C wrote one anyway, past the end.
        return;
    }

    // SAFETY: source is a valid NUL-terminated string.
    let src = unsafe { CStr::from_ptr(source) }.to_bytes();
    let copied = src.len().min(dest_buffer_size - 1 - used);
    // SAFETY: used + copied + 1 <= dest_buffer_size, so both the copy and the
    // terminator are in bounds, and the caller guarantees non-overlap.
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dest.add(used).cast::<u8>(), copied);
        *dest.add(used + copied) = 0;
    }
}

/// Concatenates two strings into a freshly `malloc`ed one.
///
/// Either argument may be null and is treated as empty, as in the C. The
/// result must be released with `free`, so it is allocated through the same
/// allocator the C used rather than through `Box`.
///
/// # Divergence from the C
///
/// The original did not check `malloc`'s result and would `memcpy` to null
/// under memory pressure. This answers null instead, which is what every
/// caller's `if (!p)` already expects.
///
/// # Safety
///
/// `first` and `second` must each be null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn vm_string_concat(
    first: *const c_char,
    second: *const c_char,
) -> *mut c_char {
    // SAFETY: the caller guarantees each pointer is null or NUL-terminated.
    let a = unsafe { bytes_of(first) };
    let b = unsafe { bytes_of(second) };

    let Some(total) = a.len().checked_add(b.len()).and_then(|n| n.checked_add(1)) else {
        return core::ptr::null_mut();
    };
    // Allocated with malloc, not Rust's allocator: every caller releases this
    // string with free(), and mixing the two is undefined behaviour.
    // SAFETY: total > 0, so malloc's contract is satisfied.
    let buf = unsafe { libc::malloc(total) }.cast::<u8>();
    if buf.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: buf has room for a.len() + b.len() + 1 bytes, and the source
    // slices are distinct allocations from it.
    unsafe {
        core::ptr::copy_nonoverlapping(a.as_ptr(), buf, a.len());
        core::ptr::copy_nonoverlapping(b.as_ptr(), buf.add(a.len()), b.len());
        *buf.add(a.len() + b.len()) = 0;
    }
    buf.cast::<c_char>()
}

/// The bytes of a C string, empty for null.
///
/// # Safety
///
/// `s` must be null or a valid NUL-terminated string.
unsafe fn bytes_of<'a>(s: *const c_char) -> &'a [u8] {
    if s.is_null() {
        return &[];
    }
    // SAFETY: the caller guarantees NUL termination.
    unsafe { CStr::from_ptr(s) }.to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// Calls the FFI function on a fixed-size buffer and reads back the result.
    fn append(initial: &str, source: &str, buffer_size: usize) -> String {
        let mut buf = vec![0u8; buffer_size.max(1) + 8]; // slack to catch overruns
        buf[..initial.len()].copy_from_slice(initial.as_bytes());
        buf[initial.len()] = 0;
        let guard_from = buffer_size.max(1);
        let src = CString::new(source).unwrap();
        unsafe {
            vm_string_append_into(buf.as_mut_ptr().cast(), src.as_ptr(), buffer_size);
        }
        assert!(
            buf[guard_from..].iter().all(|&b| b == 0),
            "wrote past the declared buffer size"
        );
        let end = buf.iter().position(|&b| b == 0).unwrap();
        String::from_utf8(buf[..end].to_vec()).unwrap()
    }

    #[test]
    fn appends_when_there_is_room() {
        assert_eq!(append("foo", "bar", 16), "foobar");
    }

    #[test]
    fn truncates_to_the_buffer_including_the_terminator() {
        // 8 bytes total: 7 characters plus NUL.
        assert_eq!(append("foo", "barbaz", 8), "foobarb");
    }

    #[test]
    fn appending_nothing_leaves_the_string_alone() {
        assert_eq!(append("foo", "", 16), "foo");
    }

    #[test]
    fn a_full_buffer_is_left_untouched() {
        // "foo" plus NUL exactly fills 4 bytes: nothing can be appended.
        assert_eq!(append("foo", "bar", 4), "foo");
    }

    /// The C underflowed `destBufferSize - 1` here and copied the whole source
    /// past the end of the buffer.
    #[test]
    fn zero_buffer_size_writes_nothing() {
        let mut buf = [0u8; 8];
        let src = CString::new("bar").unwrap();
        unsafe {
            vm_string_append_into(buf.as_mut_ptr().cast(), src.as_ptr(), 0);
        }
        assert_eq!(buf, [0u8; 8]);
    }

    #[test]
    fn null_pointers_are_ignored() {
        let src = CString::new("bar").unwrap();
        unsafe {
            vm_string_append_into(core::ptr::null_mut(), src.as_ptr(), 8);
            let mut buf = [0u8; 8];
            vm_string_append_into(buf.as_mut_ptr().cast(), core::ptr::null(), 8);
            assert_eq!(buf, [0u8; 8]);
        }
    }

    /// Reads a concat result and frees it.
    fn concat(a: Option<&str>, b: Option<&str>) -> String {
        let ca = a.map(|s| CString::new(s).unwrap());
        let cb = b.map(|s| CString::new(s).unwrap());
        unsafe {
            let p = vm_string_concat(
                ca.as_ref().map_or(core::ptr::null(), |c| c.as_ptr()),
                cb.as_ref().map_or(core::ptr::null(), |c| c.as_ptr()),
            );
            assert!(!p.is_null());
            let out = CStr::from_ptr(p).to_str().unwrap().to_owned();
            libc::free(p.cast());
            out
        }
    }

    #[test]
    fn concatenates_two_strings() {
        assert_eq!(concat(Some("foo"), Some("bar")), "foobar");
    }

    #[test]
    fn treats_null_as_empty() {
        assert_eq!(concat(None, Some("bar")), "bar");
        assert_eq!(concat(Some("foo"), None), "foo");
        assert_eq!(concat(None, None), "");
    }

    #[test]
    fn concatenating_empty_strings_gives_an_empty_string() {
        assert_eq!(concat(Some(""), Some("")), "");
    }
}
