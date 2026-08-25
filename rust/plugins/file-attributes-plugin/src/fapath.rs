//! The `fapath` record from `include/unix/faSupport.h`, as owned Rust state.
//!
//! The C keeps every path in a pair of fixed `char[PATH_MAX]` buffers -- the
//! Smalltalk (precomposed UTF-8) form and the platform form -- with a length
//! and, while iterating a directory, a pointer to the file-name part that
//! follows the directory prefix in the same buffer. This struct keeps the
//! same two forms and the same directory/file split, but in `Vec`s; the C's
//! length limits are enforced with the same comparisons so a path the C
//! rejects is rejected here, with the same status.
//!
//! Nothing of this struct crosses to the image: the image only ever sees the
//! opaque session handle built in [`crate::dir`].

use std::ffi::CString;

use crate::codes::FA_STRING_TOO_LONG;
use crate::convert::Converters;

/// `FA_PATH_MAX` is `PATH_MAX` on Unix (4096 on Linux, 1024 on macOS).
pub const FA_PATH_MAX: usize = libc::PATH_MAX as usize;

/// `PATH_SEPARATOR` from the Unix `faSupport.h`.
const PATH_SEPARATOR: u8 = b'/';

/// A path in both encodings, with the directory/file split used while
/// enumerating a directory.
#[derive(Debug, Default, Clone)]
pub struct FaPath {
    /// Smalltalk-encoded path: directory part (with trailing separator while
    /// iterating) followed by the current file name.
    st: Vec<u8>,
    /// Length of the directory part of `st` (`path_len` in C).
    st_dir_len: usize,
    /// Platform-encoded path, same layout.
    ux: Vec<u8>,
    /// Length of the directory part of `ux` (`uxpath_len` in C).
    ux_dir_len: usize,
    /// Whether the file-part APIs are meaningful (`path_file != 0` in C).
    iterating: bool,
}

/// `strlen` semantics for the buffers the C measures with `strlen`: anything
/// after an embedded NUL does not exist.
fn truncate_at_nul(v: &mut Vec<u8>) {
    if let Some(p) = v.iter().position(|&b| b == 0) {
        v.truncate(p);
    }
}

impl FaPath {
    /// An empty record; every use starts with one of the `set_*` calls.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `faSetStDir`: takes a Smalltalk-encoded directory, guarantees the
    /// trailing separator, and derives the platform form. Leaves the record
    /// ready for [`FaPath::set_plat_file`].
    pub fn set_st_dir(&mut self, path: &[u8], conv: &Converters) -> Result<(), i64> {
        if path.len() + 1 >= FA_PATH_MAX {
            return Err(FA_STRING_TOO_LONG);
        }
        self.st = path.to_vec();
        // The C reads path[len-1], which for an empty name is a read before
        // the buffer (undefined). Appending -- the branch a garbage byte
        // takes in practice -- is the defined equivalent.
        if self.st.last() != Some(&PATH_SEPARATOR) {
            self.st.push(PATH_SEPARATOR);
        }
        self.st_dir_len = self.st.len();

        let mut ux = conv.to_platform(&self.st, FA_PATH_MAX);
        if ux.is_empty() {
            // The C treats a zero-length conversion as "too long".
            return Err(FA_STRING_TOO_LONG);
        }
        truncate_at_nul(&mut ux); // C: uxpath_len = strlen(uxpath)
        self.ux_dir_len = ux.len();
        self.ux = ux;
        self.iterating = true;
        Ok(())
    }

    /// `faSetStPath`: a whole Smalltalk-encoded path, no file part.
    pub fn set_st_path(&mut self, path: &[u8], conv: &Converters) -> Result<(), i64> {
        if path.len() >= FA_PATH_MAX {
            return Err(FA_STRING_TOO_LONG);
        }
        self.st = path.to_vec();
        self.st_dir_len = self.st.len();
        self.iterating = false;

        let mut ux = conv.to_platform(path, FA_PATH_MAX);
        if ux.is_empty() {
            // Includes the empty input: sq2uxPath answers 0 bytes and the C
            // calls that FA_STRING_TOO_LONG.
            return Err(FA_STRING_TOO_LONG);
        }
        truncate_at_nul(&mut ux);
        self.ux_dir_len = ux.len();
        self.ux = ux;
        Ok(())
    }

    /// `faSetPlatPathOop`: a whole platform-encoded path, no file part.
    pub fn set_plat_path(&mut self, path: &[u8], conv: &Converters) -> Result<(), i64> {
        if path.len() >= FA_PATH_MAX {
            return Err(FA_STRING_TOO_LONG);
        }
        self.ux = path.to_vec();
        self.ux_dir_len = self.ux.len();
        self.iterating = false;

        let mut st = conv.to_smalltalk(path, FA_PATH_MAX);
        if st.is_empty() {
            return Err(FA_STRING_TOO_LONG);
        }
        truncate_at_nul(&mut st); // C: path_len = strlen(path)
        self.st_dir_len = st.len();
        self.st = st;
        Ok(())
    }

    /// `faSetPlatFile`: the platform-encoded name of the directory entry the
    /// walk just produced. Valid only after [`FaPath::set_st_dir`].
    pub fn set_plat_file(&mut self, name: &[u8], conv: &Converters) -> Result<(), i64> {
        debug_assert!(self.iterating, "set_plat_file needs set_st_dir first");
        // C: len >= uxmax_file_len, where uxmax_file_len = FA_PATH_MAX - uxpath_len.
        if name.len() >= FA_PATH_MAX - self.ux_dir_len {
            return Err(FA_STRING_TOO_LONG);
        }
        self.ux.truncate(self.ux_dir_len);
        self.ux.extend_from_slice(name);

        let st_cap = FA_PATH_MAX - self.st_dir_len; // C: max_file_len
        let mut file = conv.to_smalltalk(name, st_cap);
        if file.is_empty() {
            return Err(FA_STRING_TOO_LONG);
        }
        truncate_at_nul(&mut file);
        self.st.truncate(self.st_dir_len);
        self.st.extend_from_slice(&file);
        Ok(())
    }

    /// `faGetStPath` + `faGetStPathLen`: the whole Smalltalk-encoded path.
    #[must_use]
    pub fn st_path(&self) -> &[u8] {
        &self.st
    }

    /// `faGetStFile`: the Smalltalk-encoded name of the current entry.
    #[must_use]
    pub fn st_file(&self) -> &[u8] {
        &self.st[self.st_dir_len..]
    }

    /// `faGetPlatPath` + `faGetPlatPathByteCount`: the whole platform path.
    #[must_use]
    pub fn plat_path(&self) -> &[u8] {
        &self.ux
    }

    /// The platform path as a C string for syscalls. The C hands the raw
    /// buffer to `stat()` and friends, so an embedded NUL truncates -- same
    /// here.
    #[must_use]
    pub fn plat_cstring(&self) -> CString {
        let end = self.ux.iter().position(|&b| b == 0).unwrap_or(self.ux.len());
        CString::new(&self.ux[..end]).expect("no interior NUL after truncation")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fallback() -> Converters {
        Converters::default()
    }

    #[test]
    fn st_dir_appends_separator() {
        let mut fa = FaPath::new();
        fa.set_st_dir(b"/tmp/x", &fallback()).unwrap();
        assert_eq!(fa.st_path(), b"/tmp/x/");
        assert_eq!(fa.plat_path(), b"/tmp/x/");
        assert_eq!(fa.st_file(), b"");
    }

    #[test]
    fn st_dir_keeps_existing_separator() {
        let mut fa = FaPath::new();
        fa.set_st_dir(b"/tmp/x/", &fallback()).unwrap();
        assert_eq!(fa.st_path(), b"/tmp/x/");
    }

    /// The C reads one byte before an empty buffer here; the port pins the
    /// defined outcome: the root directory.
    #[test]
    fn empty_dir_becomes_separator() {
        let mut fa = FaPath::new();
        fa.set_st_dir(b"", &fallback()).unwrap();
        assert_eq!(fa.st_path(), b"/");
    }

    #[test]
    fn st_dir_length_limit_matches_c() {
        let mut fa = FaPath::new();
        // len + 1 >= FA_PATH_MAX rejects, so FA_PATH_MAX - 1 is the first bad length.
        assert_eq!(
            fa.set_st_dir(&vec![b'a'; FA_PATH_MAX - 1], &fallback()),
            Err(FA_STRING_TOO_LONG)
        );
        assert!(fa.set_st_dir(&vec![b'a'; FA_PATH_MAX - 3], &fallback()).is_ok());
    }

    #[test]
    fn st_path_length_limit_matches_c() {
        let mut fa = FaPath::new();
        assert_eq!(
            fa.set_st_path(&vec![b'a'; FA_PATH_MAX], &fallback()),
            Err(FA_STRING_TOO_LONG)
        );
        assert!(fa.set_st_path(&vec![b'a'; FA_PATH_MAX - 1], &fallback()).is_ok());
    }

    /// A zero-length path converts to zero bytes, which the C treats as an
    /// error rather than a valid empty path.
    #[test]
    fn empty_st_path_is_rejected() {
        let mut fa = FaPath::new();
        assert_eq!(fa.set_st_path(b"", &fallback()), Err(FA_STRING_TOO_LONG));
    }

    #[test]
    fn plat_file_composes_both_forms() {
        let mut fa = FaPath::new();
        fa.set_st_dir(b"/tmp", &fallback()).unwrap();
        fa.set_plat_file(b"data.txt", &fallback()).unwrap();
        assert_eq!(fa.st_path(), b"/tmp/data.txt");
        assert_eq!(fa.plat_path(), b"/tmp/data.txt");
        assert_eq!(fa.st_file(), b"data.txt");
        // A second entry replaces, not appends.
        fa.set_plat_file(b"b", &fallback()).unwrap();
        assert_eq!(fa.st_path(), b"/tmp/b");
        assert_eq!(fa.st_file(), b"b");
    }

    #[test]
    fn plat_file_length_limit_matches_c() {
        let mut fa = FaPath::new();
        fa.set_st_dir(b"/tmp", &fallback()).unwrap(); // dir part is 5 bytes
        let room = FA_PATH_MAX - 5;
        assert_eq!(
            fa.set_plat_file(&vec![b'f'; room], &fallback()),
            Err(FA_STRING_TOO_LONG)
        );
        assert!(fa.set_plat_file(&vec![b'f'; room - 1], &fallback()).is_ok());
    }

    #[test]
    fn plat_path_roundtrips_to_st() {
        let mut fa = FaPath::new();
        fa.set_plat_path("/tmp/caf\u{e9}".as_bytes(), &fallback()).unwrap();
        assert_eq!(fa.st_path(), "/tmp/caf\u{e9}".as_bytes());
    }

    #[test]
    fn cstring_truncates_at_embedded_nul() {
        let mut fa = FaPath::new();
        fa.set_plat_path(b"/tmp/a\0b", &fallback()).unwrap();
        assert_eq!(fa.plat_cstring().as_bytes(), b"/tmp/a");
        // And the Smalltalk form was measured with strlen, as in C.
        assert_eq!(fa.st_path(), b"/tmp/a");
    }
}
