//! Directory enumeration sessions and the opaque handle the image holds.
//!
//! # The handle
//!
//! `primitiveOpendir` answers (in slot 2 of its result) a ByteArray the size
//! of the C `FAPathPtr` -- `{ int sessionId; fapath *faPath; }` -- and the
//! walk primitives get it back on every call. The C validated it by size and
//! by comparing the session id against its `vmSessionId` global, then
//! *trusted the raw pointer*. Two things make that check vacuous and unsafe
//! in the C as shipped:
//!
//! * the generated `initialiseModule` never calls `faInitialiseModule`, so
//!   `vmSessionId` stays 0 for the life of the process -- and 0 is also what
//!   `faInvalidateSessionId` writes, so a closed (or zeroed, or stale) handle
//!   still passes the session check;
//! * the pointer is then dereferenced as-is: a handle from a previous run, or
//!   16 arbitrary bytes, is a use-after-free or wild read.
//!
//! This port keeps the same byte layout and the same size-and-session-id
//! validation (against the same never-initialised 0), but the pointer field
//! carries a key into a process-local registry instead of an address. A key
//! that is not registered -- closed, stale, or fabricated -- fails with
//! `FA_BAD_SESSION_ID`, the error the C's design intended for exactly those
//! handles. That is the port's memory-safety divergence; see the README.

use std::collections::HashMap;
use std::ffi::{c_int, CStr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use crate::codes::{
    FA_CANT_OPEN_DIR, FA_CANT_READ_DIR, FA_CORRUPT_VALUE, FA_UNABLE_TO_CLOSE_DIR,
};
use crate::convert::Converters;
use crate::fapath::FaPath;

/// What one step of the walk produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The session's `FaPath` now names the next entry.
    Entry,
    /// `FA_NO_MORE_DATA`: the stream is exhausted.
    NoMoreData,
}

/// One directory walk: the dual-encoded path and the open `DIR` stream.
#[derive(Debug)]
pub struct DirSession {
    /// Directory prefix plus the current entry name.
    pub fa: FaPath,
    dir: *mut libc::DIR,
}

// SAFETY: primitives run only on the interpreter thread, and every access
// goes through the registry Mutex anyway; the DIR stream itself carries no
// thread affinity.
unsafe impl Send for DirSession {}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "emscripten"))]
fn errno_location() -> *mut c_int {
    // SAFETY: always valid to call; answers this thread's errno slot.
    unsafe { libc::__errno_location() }
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn errno_location() -> *mut c_int {
    // SAFETY: as above.
    unsafe { libc::__error() }
}

impl DirSession {
    /// `faOpenDirectory`: opens the directory `fa` names and reads the first
    /// entry. `Ok(None)` is an empty directory (the C closes the stream and
    /// reports `FA_NO_MORE_DATA`; `primitiveOpendir` then answers nil).
    pub fn open(fa: FaPath, conv: &Converters) -> Result<Option<Self>, i64> {
        let cpath = fa.plat_cstring();
        // SAFETY: `cpath` is NUL-terminated.
        let dir = unsafe { libc::opendir(cpath.as_ptr()) };
        if dir.is_null() {
            return Err(FA_CANT_OPEN_DIR);
        }
        let mut session = DirSession { fa, dir };
        match session.read(conv) {
            Ok(ReadOutcome::Entry) => Ok(Some(session)),
            Ok(ReadOutcome::NoMoreData) => {
                session.close()?;
                Ok(None)
            }
            // The C returns here leaving the DIR open (a leak); Drop closes
            // it, which the image cannot observe.
            Err(status) => Err(status),
        }
    }

    /// `faReadDirectory`: the next entry, skipping `.` and `..`.
    pub fn read(&mut self, conv: &Converters) -> Result<ReadOutcome, i64> {
        if self.dir.is_null() {
            return Err(FA_CORRUPT_VALUE);
        }
        // The C clears errno once before the loop: it is the only way to tell
        // end-of-stream from a readdir failure.
        // SAFETY: writing this thread's errno slot.
        unsafe { *errno_location() = 0 };
        loop {
            // SAFETY: `self.dir` is a live stream from opendir.
            let entry = unsafe { libc::readdir(self.dir) };
            if entry.is_null() {
                // SAFETY: reading this thread's errno slot.
                let e = unsafe { *errno_location() };
                return if e == 0 {
                    Ok(ReadOutcome::NoMoreData)
                } else {
                    Err(FA_CANT_READ_DIR)
                };
            }
            // SAFETY: the OS NUL-terminates d_name.
            let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }
                .to_bytes()
                .to_vec();
            if name != b"." && name != b".." {
                self.fa.set_plat_file(&name, conv)?;
                return Ok(ReadOutcome::Entry);
            }
        }
    }

    /// `faRewindDirectory`: back to the start, then read the first entry.
    pub fn rewind(&mut self, conv: &Converters) -> Result<ReadOutcome, i64> {
        if self.dir.is_null() {
            return Err(FA_CORRUPT_VALUE);
        }
        // SAFETY: live stream.
        unsafe { libc::rewinddir(self.dir) };
        self.read(conv)
    }

    /// `faCloseDirectory`.
    pub fn close(&mut self) -> Result<(), i64> {
        if self.dir.is_null() {
            return Err(FA_CORRUPT_VALUE);
        }
        // SAFETY: live stream; ownership of the handle ends here whatever
        // closedir answers, so it is nulled either way.
        let status = unsafe { libc::closedir(self.dir) };
        self.dir = std::ptr::null_mut();
        if status != 0 {
            return Err(FA_UNABLE_TO_CLOSE_DIR);
        }
        Ok(())
    }
}

impl Drop for DirSession {
    fn drop(&mut self) {
        if !self.dir.is_null() {
            // SAFETY: still-live stream; last use of the pointer.
            unsafe { libc::closedir(self.dir) };
        }
    }
}

// ---------------------------------------------------------------------------
// The registry and the image-side handle bytes
// ---------------------------------------------------------------------------

/// The C `FAPathPtr`, kept only for its size and field offsets so the
/// ByteArray the image holds is indistinguishable from the C one's.
#[repr(C)]
#[allow(dead_code)] // never instantiated; measured with size_of only
struct FaPathPtrLayout {
    session_id: c_int,
    fa_path: usize,
}

/// `sizeof(FAPathPtr)`: 16 bytes on 64-bit targets, 8 on 32-bit.
pub const HANDLE_BYTES: usize = std::mem::size_of::<FaPathPtrLayout>();

/// Offset of the pointer/key field: after the id plus the alignment padding
/// the C struct has.
const KEY_OFFSET: usize = HANDLE_BYTES - std::mem::size_of::<usize>();

/// The value `vmSessionId` holds in the C plugin: 0, forever, because the
/// generated `initialiseModule` never calls `faInitialiseModule`. Handles are
/// stamped and checked with it all the same, as the C does.
pub const SESSION_ID: i32 = 0;

/// Live sessions, keyed by the value stored in the handle's pointer field.
fn registry() -> &'static Mutex<HashMap<usize, DirSession>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, DirSession>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Locks the registry. A poisoned lock (a caught panic mid-primitive) is
/// recovered rather than propagated: failing every later walk would punish
/// the image for a bug already reported as a primitive failure.
pub fn lock() -> MutexGuard<'static, HashMap<usize, DirSession>> {
    registry().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Stores a session, answering the key to embed in the handle.
pub fn register(session: DirSession) -> usize {
    static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    lock().insert(key, session);
    key
}

/// Removes and answers a session; `None` for a closed or fabricated key.
pub fn take(key: usize) -> Option<DirSession> {
    lock().remove(&key)
}

/// Builds the handle bytes: session id, padding zeroed, key in the pointer
/// slot, all native-endian as the C's struct memcpy was.
#[must_use]
pub fn encode_handle(key: usize) -> [u8; HANDLE_BYTES] {
    let mut bytes = [0u8; HANDLE_BYTES];
    bytes[..4].copy_from_slice(&SESSION_ID.to_ne_bytes());
    bytes[KEY_OFFSET..].copy_from_slice(&key.to_ne_bytes());
    bytes
}

/// Reads a handle back. `None` only for a wrong length -- the C's
/// `PrimErrBadArgument` case; the id and key are validated by the caller.
#[must_use]
pub fn decode_handle(bytes: &[u8]) -> Option<(i32, usize)> {
    if bytes.len() != HANDLE_BYTES {
        return None;
    }
    let session_id = i32::from_ne_bytes(bytes[..4].try_into().ok()?);
    let key = usize::from_ne_bytes(bytes[KEY_OFFSET..].try_into().ok()?);
    Some((session_id, key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::collections::BTreeSet;

    fn open_dir(path: &std::path::Path) -> Result<Option<DirSession>, i64> {
        let conv = Converters::default();
        let mut fa = FaPath::new();
        fa.set_st_dir(path.as_os_str().as_encoded_bytes(), &conv)
            .unwrap();
        DirSession::open(fa, &conv)
    }

    #[test]
    fn handle_layout_matches_the_c_struct() {
        // int + padding + pointer.
        #[cfg(target_pointer_width = "64")]
        assert_eq!(HANDLE_BYTES, 16);
        #[cfg(target_pointer_width = "32")]
        assert_eq!(HANDLE_BYTES, 8);
        assert_eq!(std::mem::size_of::<c_int>(), 4);
    }

    #[test]
    fn handle_roundtrip() {
        let bytes = encode_handle(0x1234_5678);
        assert_eq!(decode_handle(&bytes), Some((SESSION_ID, 0x1234_5678)));
        assert_eq!(decode_handle(&bytes[1..]), None);
        assert_eq!(decode_handle(&[]), None);
    }

    #[test]
    fn empty_directory_answers_none() {
        let dir = TempDir::new("dir-empty");
        assert!(open_dir(dir.path()).unwrap().is_none());
    }

    #[test]
    fn missing_directory_is_cant_open() {
        let dir = TempDir::new("dir-missing");
        assert_eq!(
            open_dir(&dir.path().join("nope")).unwrap_err(),
            FA_CANT_OPEN_DIR
        );
    }

    #[test]
    fn walk_yields_every_entry_and_skips_dots() {
        let dir = TempDir::new("dir-walk");
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let conv = Converters::default();
        let mut session = open_dir(dir.path()).unwrap().unwrap();
        let mut seen = BTreeSet::new();
        seen.insert(session.fa.st_file().to_vec());
        while session.read(&conv).unwrap() == ReadOutcome::Entry {
            assert!(seen.insert(session.fa.st_file().to_vec()), "no duplicates");
        }
        let expected: BTreeSet<Vec<u8>> = ["a.txt", "b.txt", "c.txt"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect();
        assert_eq!(seen, expected);
        // The full path is composed for stat()ing each entry.
        assert!(session.fa.plat_path().starts_with(dir.path().as_os_str().as_encoded_bytes()));
        session.close().unwrap();
    }

    #[test]
    fn rewind_restarts_the_walk() {
        let dir = TempDir::new("dir-rewind");
        std::fs::write(dir.path().join("only"), b"x").unwrap();
        let conv = Converters::default();
        let mut session = open_dir(dir.path()).unwrap().unwrap();
        assert_eq!(session.fa.st_file(), b"only");
        assert_eq!(session.read(&conv).unwrap(), ReadOutcome::NoMoreData);
        assert_eq!(session.rewind(&conv).unwrap(), ReadOutcome::Entry);
        assert_eq!(session.fa.st_file(), b"only");
        session.close().unwrap();
    }

    #[test]
    fn close_twice_is_corrupt_value() {
        let dir = TempDir::new("dir-close");
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        let mut session = open_dir(dir.path()).unwrap().unwrap();
        session.close().unwrap();
        assert_eq!(session.close().unwrap_err(), FA_CORRUPT_VALUE);
    }

    #[test]
    fn registry_register_take_take() {
        let dir = TempDir::new("dir-registry");
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        let session = open_dir(dir.path()).unwrap().unwrap();
        let key = register(session);
        assert!(take(key).is_some());
        assert!(take(key).is_none(), "a closed handle no longer resolves");
    }
}
