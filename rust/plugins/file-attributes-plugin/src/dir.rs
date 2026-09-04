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
use std::ffi::c_int;
use std::fs::ReadDir;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use pharo_vm_plugin::poison::{self, Guarded};
use pharo_vm_plugin::PrimResult;

use crate::codes::{FA_CANT_OPEN_DIR, FA_CANT_READ_DIR, FA_CORRUPT_VALUE};
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

/// One directory walk: the dual-encoded path and the open directory stream.
///
/// `std::fs::ReadDir` replaces the raw `*mut DIR` here, and takes three
/// things with it:
///
/// * the `unsafe impl Send`, because `ReadDir` is `Send` on its own;
/// * the `Drop` impl, because `ReadDir` closes its own descriptor;
/// * the hand-rolled `errno_location()` (which had `cfg` arms for Linux and
///   the BSDs only, and so would not compile on Solaris or NetBSD). The C
///   cleared errno before the loop purely to tell end-of-stream from a
///   `readdir` failure; `Iterator::next` answering `None` versus `Some(Err)`
///   *is* that distinction, checked by the compiler.
#[derive(Debug)]
pub struct DirSession {
    /// Directory prefix plus the current entry name.
    pub fa: FaPath,
    /// The directory being walked, kept so [`DirSession::rewind`] can re-open
    /// it -- `ReadDir` has no `rewinddir`.
    path: PathBuf,
    /// `None` once closed; every walk method rejects that as the C did.
    dir: Option<ReadDir>,
}

impl DirSession {
    /// `faOpenDirectory`: opens the directory `fa` names and reads the first
    /// entry. `Ok(None)` is an empty directory (the C closes the stream and
    /// reports `FA_NO_MORE_DATA`; `primitiveOpendir` then answers nil).
    pub fn open(fa: FaPath, conv: &Converters) -> Result<Option<Self>, i64> {
        // `plat_cstring` applies the C's embedded-NUL truncation; the bytes
        // that survive it become the path, without a UTF-8 round trip.
        let path = PathBuf::from(std::ffi::OsString::from_vec(
            fa.plat_cstring().into_bytes(),
        ));
        let dir = std::fs::read_dir(&path).map_err(|_| FA_CANT_OPEN_DIR)?;
        let mut session = DirSession {
            fa,
            path,
            dir: Some(dir),
        };
        match session.read(conv) {
            Ok(ReadOutcome::Entry) => Ok(Some(session)),
            Ok(ReadOutcome::NoMoreData) => {
                session.close()?;
                Ok(None)
            }
            // The C returned here leaving the DIR open (a leak); dropping the
            // session closes it, which the image cannot observe.
            Err(status) => Err(status),
        }
    }

    /// `faReadDirectory`: the next entry.
    ///
    /// The C's explicit `.`/`..` skip is gone because `read_dir` never yields
    /// them -- the filtering moved into std, not out of the plugin.
    pub fn read(&mut self, conv: &Converters) -> Result<ReadOutcome, i64> {
        let dir = self.dir.as_mut().ok_or(FA_CORRUPT_VALUE)?;
        let name = match dir.next() {
            None => return Ok(ReadOutcome::NoMoreData),
            Some(Err(_)) => return Err(FA_CANT_READ_DIR),
            Some(Ok(entry)) => entry.file_name().into_vec(),
        };
        self.fa.set_plat_file(&name, conv)?;
        Ok(ReadOutcome::Entry)
    }

    /// `faRewindDirectory`: back to the start, then read the first entry.
    ///
    /// `ReadDir` exposes no `rewinddir`, so this re-opens the directory by
    /// path. Two consequences, neither reachable from image code that is not
    /// racing itself: the path is resolved a second time (so a directory
    /// renamed mid-walk rewinds into whatever now has that name, where
    /// `rewinddir` stayed with the original inode), and a directory removed
    /// mid-walk fails with `FA_CANT_READ_DIR` where the C answered
    /// `FA_NO_MORE_DATA`.
    pub fn rewind(&mut self, conv: &Converters) -> Result<ReadOutcome, i64> {
        if self.dir.is_none() {
            return Err(FA_CORRUPT_VALUE);
        }
        self.dir = Some(std::fs::read_dir(&self.path).map_err(|_| FA_CANT_READ_DIR)?);
        self.read(conv)
    }

    /// `faCloseDirectory`.
    ///
    /// Dropping the `ReadDir` closes the descriptor. That makes the C's
    /// `FA_UNABLE_TO_CLOSE_DIR` unreachable: std gives no way to observe
    /// `closedir`'s status, which on a live stream can only fail with EBADF.
    /// Closing an already-closed session is still `FA_CORRUPT_VALUE`.
    pub fn close(&mut self) -> Result<(), i64> {
        self.dir.take().ok_or(FA_CORRUPT_VALUE)?;
        Ok(())
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

/// Locks the registry, refusing it once a panic has torn it.
///
/// Through [`poison::lock`]. The earlier justification for recovering the
/// lock -- "the bug was already reported as a primitive failure" -- had the
/// argument backwards: reporting the *first* call as a failure says nothing
/// about the *second*, and what the second would find here is a `DirSession`
/// whose `DIR *` and whose `FaPath` buffer no longer describe the same
/// directory. `readdir` then walks one directory while `process_directory`
/// reports paths from another.
pub fn lock() -> PrimResult<Guarded<'static, HashMap<usize, DirSession>>> {
    poison::lock(registry())
}

/// Stores a session, answering the key to embed in the handle.
pub fn register(session: DirSession) -> PrimResult<usize> {
    static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    lock()?.insert(key, session);
    Ok(key)
}

/// Removes and answers a session; `Ok(None)` for a closed or fabricated key.
pub fn take(key: usize) -> PrimResult<Option<DirSession>> {
    Ok(lock()?.remove(&key))
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
    use pharo_vm_plugin::PrimErr;
    use std::collections::BTreeSet;
    use std::sync::PoisonError;

    /// Serialises the two tests that touch the process-wide session registry.
    ///
    /// The poison test below leaves that registry unusable for as long as it
    /// holds this guard, and the harness runs tests in parallel threads within
    /// one binary -- so "nothing else takes this lock while I have it" is the
    /// only thing that keeps the two from colliding. It recovers its own
    /// poison deliberately, as every `#[cfg(test)]` serialisation lock in this
    /// tree does: one failing test must not cascade into the other, and no
    /// image ever reaches this.
    fn registry_lock() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
    }

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
        let _serial = registry_lock();
        let dir = TempDir::new("dir-registry");
        std::fs::write(dir.path().join("f"), b"x").unwrap();
        let session = open_dir(dir.path()).unwrap().unwrap();
        let key = register(session).expect("a registry nothing has torn");
        assert!(take(key).expect("still untorn").is_some());
        assert!(
            take(key).expect("still untorn").is_none(),
            "a closed handle no longer resolves"
        );
    }

    // -----------------------------------------------------------------------
    // Fail-fast after a panic mid-walk
    // -----------------------------------------------------------------------

    /// A panic while [`lock`] is held refuses every later lock.
    ///
    /// The SDK proves the mechanism in
    /// `pharo-vm-plugin/tests/plugin_mutex_poison.rs`; this proves the
    /// *wiring* here, which is the half a `registry().clear_poison()` slipped
    /// in front of the `poison::lock` would silently undo while all 42 tests
    /// in the crate stayed green.
    ///
    /// The tear is the real invariant a [`DirSession`] carries: its `dir`
    /// stream and its `fa` buffer must describe the *same* directory. `fa` is
    /// both the prefix `read` appends each entry name to and the buffer
    /// `primitiveReaddir` hands the image back, so once the two disagree the
    /// walk enumerates one directory and reports absolute paths inside
    /// another -- and the image `stat`s, opens and deletes what those paths
    /// name. Recovering the lock is what would hand that session out; refusing
    /// is why this registry locks the way it does.
    ///
    /// The panic is raised by this test rather than injected through the proxy
    /// on purpose: every plugin-to-VM call crosses `extern "C"`, whose
    /// abort-on-unwind shim would turn an injected panic into `SIGABRT`
    /// instead of the unwind the hazard is made of.
    #[test]
    fn a_panic_while_the_session_registry_is_held_refuses_every_later_lock() {
        let _serial = registry_lock();

        let walked = TempDir::new("dir-poison-walked");
        std::fs::write(walked.path().join("only-here"), b"x").unwrap();
        let reported = TempDir::new("dir-poison-reported");

        // --- healthy ------------------------------------------------------
        let session = open_dir(walked.path()).unwrap().unwrap();
        let key = register(session).expect("a fresh module hands out the registry");
        assert!(lock().expect("still healthy").contains_key(&key));

        // --- a panic between the two halves of a session ------------------
        let torn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let conv = Converters::default();
            let mut sessions = lock().expect("still healthy");
            let session = sessions.get_mut(&key).expect("just registered");
            // The prefix now names the second directory; the `ReadDir` still
            // walks the first.
            session
                .fa
                .set_st_dir(reported.path().as_os_str().as_encoded_bytes(), &conv)
                .unwrap();
            panic!("a path conversion failed mid-rewind");
        }));
        assert!(torn.is_err());

        // The session really is torn, which is what makes the assertions
        // below mean something. Reached the way the old code reached it --
        // and this is the only place in the crate that may still do so.
        {
            let recovered = registry().lock().unwrap_or_else(PoisonError::into_inner);
            let session = recovered.get(&key).expect("still registered");
            assert_eq!(
                session.path,
                walked.path(),
                "the stream still walks the directory it was opened on"
            );
            assert!(
                session
                    .fa
                    .plat_path()
                    .starts_with(reported.path().as_os_str().as_encoded_bytes()),
                "while the buffer the image is handed names the other one"
            );
        }

        // --- the fix ------------------------------------------------------
        assert_eq!(
            lock().err(),
            Some(PrimErr::Unsupported),
            "a session whose stream and buffer name different directories \
             must never be handed to a caller"
        );
        assert_eq!(
            take(key).err(),
            Some(PrimErr::Unsupported),
            "and every caller propagates the refusal rather than walking on"
        );

        // --- teardown -----------------------------------------------------
        //
        // The assertions are made; this restores the binary for the other
        // test that shares `registry_lock`, which is still blocked on the
        // guard held above. It is the only `clear_poison` in the crate, it is
        // `#[cfg(test)]`, and no image can reach it -- the accessor above is
        // still the only way in from a primitive, and it still refuses.
        registry().clear_poison();
        lock().expect("cleared").clear();
    }
}
