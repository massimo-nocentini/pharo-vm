//! Exercises the exported `NewFile_*` / `NewDirectory_*` API against real
//! files, in a directory under cargo's per-crate `target/tmp`.
//!
//! These are exactly the calls the primitives make; only the interpreter
//! plumbing above them (stack access, ExternalAddress traffic) is left for
//! image-side differential testing.

// The crate is named for the shared library the VM loads.
#![allow(non_snake_case)]

use std::ffi::CStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use NewFilePlugin::newfile::*;

/// A fresh directory for one test, under `target/tmp` so nothing touches the
/// system temp directory. Not removed on panic; cargo owns the tree.
fn tempdir(tag: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("newfile-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

/// Calls `NewFile_open` the way the primitive does: path as (pointer, size).
fn open_file(path: &Path, mode: i32, disposition: i32, flags: i32) -> *mut NewFile {
    let bytes = path.as_os_str().as_bytes();
    unsafe { NewFile_open(bytes.as_ptr().cast(), bytes.len(), mode, disposition, flags) }
}

fn delete_file(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    unsafe { NewFile_deleteFile(bytes.as_ptr().cast(), bytes.len()) }
}

fn open_dir(path: &Path) -> *mut NewDirectory {
    let bytes = path.as_os_str().as_bytes();
    unsafe { NewDirectory_open(bytes.as_ptr().cast(), bytes.len()) }
}

fn read_all(file: *mut NewFile, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    let n = unsafe { NewFile_read(file, buf.as_mut_ptr().cast(), 0, len) };
    assert!(n >= 0, "read failed");
    buf.truncate(n as usize);
    buf
}

fn write_all(file: *mut NewFile, bytes: &[u8]) -> i64 {
    unsafe { NewFile_write(file, bytes.as_ptr().cast(), 0, bytes.len()) }
}

// ---------------------------------------------------------------------------
// Open modes and creation dispositions
// ---------------------------------------------------------------------------

#[test]
fn create_new_creates_and_is_exclusive() {
    let dir = tempdir("create-new");
    let path = dir.join("f");

    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_CREATE_NEW, 0);
    assert!(!f.is_null(), "CreateNew on a fresh path must succeed");
    unsafe { NewFile_close(f) };
    assert!(path.exists());

    // O_EXCL: a second CreateNew on the same path fails.
    let g = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_CREATE_NEW, 0);
    assert!(g.is_null(), "CreateNew on an existing path must fail");
}

#[test]
fn open_existing_fails_on_missing_file() {
    let dir = tempdir("open-existing");
    let f = open_file(
        &dir.join("missing"),
        OPEN_MODE_READ_ONLY,
        CREATION_OPEN_EXISTING,
        0,
    );
    assert!(f.is_null());
}

#[test]
fn create_always_truncates() {
    let dir = tempdir("create-always");
    let path = dir.join("f");
    fs::write(&path, b"previous content").unwrap();

    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_CREATE_ALWAYS, 0);
    assert!(!f.is_null());
    assert_eq!(unsafe { NewFile_getSize(f) }, 0, "O_TRUNC must have emptied it");
    unsafe { NewFile_close(f) };
}

#[test]
fn truncate_existing_requires_the_file() {
    let dir = tempdir("truncate-existing");
    let path = dir.join("f");

    // No O_CREAT in this disposition: a missing file fails.
    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_TRUNCATE_EXISTING, 0);
    assert!(f.is_null());

    fs::write(&path, b"previous content").unwrap();
    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_TRUNCATE_EXISTING, 0);
    assert!(!f.is_null());
    assert_eq!(unsafe { NewFile_getSize(f) }, 0);
    unsafe { NewFile_close(f) };
}

#[test]
fn open_always_creates_but_preserves() {
    let dir = tempdir("open-always");
    let path = dir.join("f");

    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, CREATION_OPEN_ALWAYS, 0);
    assert!(!f.is_null(), "OpenAlways creates a missing file");
    assert_eq!(write_all(f, b"kept"), 4);
    unsafe { NewFile_close(f) };

    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_ALWAYS, 0);
    assert!(!f.is_null());
    assert_eq!(unsafe { NewFile_getSize(f) }, 4, "no truncation on reopen");
    unsafe { NewFile_close(f) };
}

#[test]
fn invalid_mode_answers_null_without_touching_the_file_system() {
    let dir = tempdir("invalid-mode");
    let path = dir.join("f");
    let f = open_file(&path, 3, CREATION_CREATE_ALWAYS, 0);
    assert!(f.is_null());
    assert!(!path.exists(), "the mode switch rejects before open(2)");
}

#[test]
fn unknown_disposition_behaves_like_open_existing() {
    // The C's `default: break` adds no creation flags at all.
    let dir = tempdir("unknown-disposition");
    let path = dir.join("f");

    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, 99, 0);
    assert!(f.is_null(), "no O_CREAT: a missing file fails");

    fs::write(&path, b"content").unwrap();
    let f = open_file(&path, OPEN_MODE_WRITE_ONLY, 99, 0);
    assert!(!f.is_null(), "an existing file opens");
    assert_eq!(unsafe { NewFile_getSize(f) }, 7, "and is not truncated");
    unsafe { NewFile_close(f) };
}

#[test]
fn path_size_is_honoured_over_the_buffer_length() {
    let dir = tempdir("path-size");
    let real = dir.join("real");
    fs::write(&real, b"x").unwrap();

    // Hand a longer buffer with trailing garbage; only path_size counts.
    let mut bytes = real.as_os_str().as_bytes().to_vec();
    let size = bytes.len();
    bytes.extend_from_slice(b"-GARBAGE");
    let f = unsafe {
        NewFile_open(
            bytes.as_ptr().cast(),
            size,
            OPEN_MODE_READ_ONLY,
            CREATION_OPEN_EXISTING,
            0,
        )
    };
    assert!(!f.is_null());
    unsafe { NewFile_close(f) };
}

#[test]
fn permission_denied_answers_null() {
    // Meaningless as root, where DAC checks do not apply.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let dir = tempdir("permissions");
    let path = dir.join("locked");
    fs::write(&path, b"secret").unwrap();
    let cpath = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    unsafe { libc::chmod(cpath.as_ptr(), 0) };

    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(f.is_null());

    unsafe { libc::chmod(cpath.as_ptr(), 0o644) }; // let the cleanup remove it
}

// ---------------------------------------------------------------------------
// Reading, writing, seeking
// ---------------------------------------------------------------------------

#[test]
fn write_then_read_roundtrip() {
    let dir = tempdir("roundtrip");
    let f = open_file(
        &dir.join("f"),
        OPEN_MODE_READ_WRITE,
        CREATION_CREATE_ALWAYS,
        0,
    );
    assert!(!f.is_null());

    assert_eq!(write_all(f, b"hello, newfile"), 14);
    unsafe { NewFile_seek(f, 0, SEEK_MODE_SET) };
    assert_eq!(read_all(f, 64), b"hello, newfile");
    unsafe { NewFile_close(f) };
}

#[test]
fn buffer_offset_lands_inside_the_buffer() {
    let dir = tempdir("buffer-offset");
    let path = dir.join("f");
    fs::write(&path, b"ABCDE").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());

    let mut buf = [0u8; 10];
    let n = unsafe { NewFile_read(f, buf.as_mut_ptr().cast(), 4, 5) };
    assert_eq!(n, 5);
    assert_eq!(&buf, b"\0\0\0\0ABCDE\0");
    unsafe { NewFile_close(f) };
}

#[test]
fn write_on_a_read_only_descriptor_answers_minus_one() {
    let dir = tempdir("readonly-write");
    let path = dir.join("f");
    fs::write(&path, b"x").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());
    assert_eq!(write_all(f, b"nope"), -1, "EBADF surfaces as the syscall's -1");
    unsafe { NewFile_close(f) };
}

#[test]
fn append_flag_forces_writes_to_the_end() {
    let dir = tempdir("append");
    let path = dir.join("f");
    fs::write(&path, b"base-").unwrap();
    let f = open_file(
        &path,
        OPEN_MODE_WRITE_ONLY,
        CREATION_OPEN_EXISTING,
        OPEN_FLAGS_APPEND,
    );
    assert!(!f.is_null());

    // O_APPEND ignores the explicit rewind.
    unsafe { NewFile_seek(f, 0, SEEK_MODE_SET) };
    assert_eq!(write_all(f, b"tail"), 4);
    unsafe { NewFile_close(f) };
    assert_eq!(fs::read(&path).unwrap(), b"base-tail");
}

#[test]
fn seek_modes_and_tell() {
    let dir = tempdir("seek");
    let path = dir.join("f");
    fs::write(&path, b"0123456789").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());

    unsafe { NewFile_seek(f, 4, SEEK_MODE_SET) };
    assert_eq!(unsafe { NewFile_tell(f) }, 4);

    unsafe { NewFile_seek(f, 3, SEEK_MODE_CURRENT) };
    assert_eq!(unsafe { NewFile_tell(f) }, 7);

    unsafe { NewFile_seek(f, -2, SEEK_MODE_END) };
    assert_eq!(unsafe { NewFile_tell(f) }, 8);

    // An unknown mode is a no-op, per the C's `default: return`.
    unsafe { NewFile_seek(f, 1, 99) };
    assert_eq!(unsafe { NewFile_tell(f) }, 8);

    assert_eq!(read_all(f, 8), b"89");
    unsafe { NewFile_close(f) };
}

#[test]
fn large_offsets_survive_the_64_bit_path() {
    let dir = tempdir("large-offset");
    let f = open_file(
        &dir.join("sparse"),
        OPEN_MODE_READ_WRITE,
        CREATION_CREATE_ALWAYS,
        0,
    );
    assert!(!f.is_null());

    // Past 2^32: a 32-bit offset would wrap to 704_643_072.
    const FAR: i64 = 5_000_000_000;
    unsafe { NewFile_seek(f, FAR, SEEK_MODE_SET) };
    assert_eq!(unsafe { NewFile_tell(f) }, FAR);
    assert_eq!(write_all(f, b"!"), 1);
    assert_eq!(unsafe { NewFile_getSize(f) }, FAR + 1);

    let mut byte = [0u8; 1];
    let n = unsafe { NewFile_readAtOffset(f, byte.as_mut_ptr().cast(), 0, 1, FAR as u64) };
    assert_eq!((n, byte[0]), (1, b'!'));
    unsafe { NewFile_close(f) };
}

#[test]
fn positional_read_write_leave_the_position_alone() {
    let dir = tempdir("positional");
    let f = open_file(
        &dir.join("f"),
        OPEN_MODE_READ_WRITE,
        CREATION_CREATE_ALWAYS,
        0,
    );
    assert!(!f.is_null());
    assert_eq!(write_all(f, b".........."), 10);
    unsafe { NewFile_seek(f, 3, SEEK_MODE_SET) };

    let n = unsafe { NewFile_writeAtOffset(f, b"XY".as_ptr().cast(), 0, 2, 7) };
    assert_eq!(n, 2);
    assert_eq!(unsafe { NewFile_tell(f) }, 3, "pwrite must not move the offset");

    let mut buf = [0u8; 2];
    let n = unsafe { NewFile_readAtOffset(f, buf.as_mut_ptr().cast(), 0, 2, 7) };
    assert_eq!(n, 2);
    assert_eq!(&buf, b"XY");
    assert_eq!(unsafe { NewFile_tell(f) }, 3, "pread must not move the offset");
    unsafe { NewFile_close(f) };
}

#[test]
fn truncate_shrinks_and_extends() {
    let dir = tempdir("truncate");
    let f = open_file(
        &dir.join("f"),
        OPEN_MODE_READ_WRITE,
        CREATION_CREATE_ALWAYS,
        0,
    );
    assert!(!f.is_null());
    assert_eq!(write_all(f, b"0123456789"), 10);

    assert!(unsafe { NewFile_truncate(f, 4) });
    assert_eq!(unsafe { NewFile_getSize(f) }, 4);

    // ftruncate also extends, zero-filling.
    assert!(unsafe { NewFile_truncate(f, 8) });
    assert_eq!(unsafe { NewFile_getSize(f) }, 8);
    let mut buf = [1u8; 8];
    let n = unsafe { NewFile_readAtOffset(f, buf.as_mut_ptr().cast(), 0, 8, 0) };
    assert_eq!(n, 8);
    assert_eq!(&buf, b"0123\0\0\0\0");
    unsafe { NewFile_close(f) };
}

#[test]
fn delete_file_answers_success() {
    let dir = tempdir("delete");
    let path = dir.join("f");
    fs::write(&path, b"x").unwrap();
    assert!(delete_file(&path));
    assert!(!path.exists());
    assert!(!delete_file(&path), "a missing file answers false");
}

// ---------------------------------------------------------------------------
// NULL-handle conventions -- each is a distinct value the image can see
// ---------------------------------------------------------------------------

#[test]
fn null_handles_answer_the_c_error_values() {
    let null = std::ptr::null_mut::<NewFile>();
    unsafe {
        assert_eq!(NewFile_getSize(null), -1);
        assert_eq!(NewFile_tell(null), 0, "tell's odd 0-for-NULL, kept");
        assert!(!NewFile_truncate(null, 10));
        let mut buf = [0u8; 4];
        assert_eq!(NewFile_read(null, buf.as_mut_ptr().cast(), 0, 4), -1);
        assert_eq!(NewFile_write(null, buf.as_ptr().cast(), 0, 4), -1);
        assert_eq!(NewFile_readAtOffset(null, buf.as_mut_ptr().cast(), 0, 4, 0), -1);
        assert_eq!(NewFile_writeAtOffset(null, buf.as_ptr().cast(), 0, 4, 0), -1);
        NewFile_seek(null, 10, SEEK_MODE_SET); // no-op, must not crash
        NewFile_close(null); // no-op
        assert!(NewFile_memoryMap(null, MMAP_PROT_READ_ONLY).is_null());
        NewFile_memoryUnmap(null); // no-op

        let nulldir = std::ptr::null_mut::<NewDirectory>();
        assert!(!NewDirectory_rewind(nulldir));
        assert!(NewDirectory_next(nulldir).is_null());
        NewDirectory_close(nulldir); // no-op
    }
}

// ---------------------------------------------------------------------------
// Directories
// ---------------------------------------------------------------------------

/// Drains the enumeration into owned names ("." and ".." included).
fn list(dir: *mut NewDirectory) -> Vec<String> {
    let mut names = Vec::new();
    loop {
        let name = unsafe { NewDirectory_next(dir) };
        if name.is_null() {
            return names;
        }
        // Copy out immediately: the pointer only lives until the next call.
        names.push(
            unsafe { CStr::from_ptr(name) }
                .to_string_lossy()
                .into_owned(),
        );
    }
}

#[test]
fn directory_create_list_rewind_remove() {
    let root = tempdir("dirs");
    let sub = root.join("sub");
    let sub_bytes = sub.as_os_str().as_bytes();

    assert!(unsafe { NewDirectory_create(sub_bytes.as_ptr().cast(), sub_bytes.len()) });
    assert!(
        !unsafe { NewDirectory_create(sub_bytes.as_ptr().cast(), sub_bytes.len()) },
        "mkdir on an existing directory answers false"
    );

    fs::write(sub.join("a.txt"), b"a").unwrap();
    fs::write(sub.join("b.txt"), b"b").unwrap();

    let dir = open_dir(&sub);
    assert!(!dir.is_null());
    let mut names = list(dir);
    names.sort();
    assert_eq!(names, [".", "..", "a.txt", "b.txt"]);

    assert!(unsafe { NewDirectory_rewind(dir) });
    let mut again = list(dir);
    again.sort();
    assert_eq!(again, [".", "..", "a.txt", "b.txt"]);
    unsafe { NewDirectory_close(dir) };

    // rmdir refuses a non-empty directory, then accepts an empty one.
    assert!(!unsafe { NewDirectory_removeEmpty(sub_bytes.as_ptr().cast(), sub_bytes.len()) });
    assert!(delete_file(&sub.join("a.txt")));
    assert!(delete_file(&sub.join("b.txt")));
    assert!(unsafe { NewDirectory_removeEmpty(sub_bytes.as_ptr().cast(), sub_bytes.len()) });
    assert!(!sub.exists());
}

#[test]
fn directory_open_missing_answers_null() {
    let root = tempdir("dir-missing");
    assert!(open_dir(&root.join("nope")).is_null());
}

// ---------------------------------------------------------------------------
// Memory mapping
// ---------------------------------------------------------------------------

#[test]
fn memory_map_reads_the_file() {
    let dir = tempdir("mmap-read");
    let path = dir.join("f");
    fs::write(&path, b"mapped contents").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());

    let addr = unsafe { NewFile_memoryMap(f, MMAP_PROT_READ_ONLY) };
    assert!(!addr.is_null());
    let seen = unsafe { std::slice::from_raw_parts(addr.cast::<u8>(), 15) };
    assert_eq!(seen, b"mapped contents");

    // Faithful oddity: the count-reaches-zero unmap returns before
    // munmap(2), so the mapping stays live -- which this read relies on.
    unsafe { NewFile_memoryUnmap(f) };
    let still = unsafe { std::slice::from_raw_parts(addr.cast::<u8>(), 6) };
    assert_eq!(still, b"mapped");
    unsafe { NewFile_close(f) };
}

#[test]
fn memory_map_read_write_hits_the_file() {
    let dir = tempdir("mmap-write");
    let path = dir.join("f");
    fs::write(&path, b"....").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_WRITE, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());

    let addr = unsafe { NewFile_memoryMap(f, MMAP_PROT_READ_WRITE) };
    assert!(!addr.is_null());
    unsafe { addr.cast::<u8>().write(b'X') };

    // MAP_SHARED: the store is visible through the descriptor.
    let mut buf = [0u8; 4];
    let n = unsafe { NewFile_readAtOffset(f, buf.as_mut_ptr().cast(), 0, 4, 0) };
    assert_eq!(n, 4);
    assert_eq!(&buf, b"X...");
    unsafe { NewFile_close(f) };
}

#[test]
fn memory_map_is_reference_counted_per_the_c() {
    let dir = tempdir("mmap-count");
    let path = dir.join("f");
    fs::write(&path, b"counted").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());

    let a = unsafe { NewFile_memoryMap(f, MMAP_PROT_READ_ONLY) };
    let b = unsafe { NewFile_memoryMap(f, MMAP_PROT_READ_ONLY) };
    assert_eq!(a, b, "a nested map answers the same mapping");

    // Faithful to the C's inverted count check: this unmap (2 -> 1) munmaps
    // immediately, so `a` is dangling from here on -- do not touch it.
    unsafe { NewFile_memoryUnmap(f) };
    // 1 -> 0 returns early; 0 is ignored. Neither may crash.
    unsafe { NewFile_memoryUnmap(f) };
    unsafe { NewFile_memoryUnmap(f) };
    unsafe { NewFile_close(f) };
}

#[test]
fn memory_map_of_an_empty_file_answers_null() {
    let dir = tempdir("mmap-empty");
    let path = dir.join("f");
    fs::write(&path, b"").unwrap();
    let f = open_file(&path, OPEN_MODE_READ_ONLY, CREATION_OPEN_EXISTING, 0);
    assert!(!f.is_null());
    assert!(unsafe { NewFile_memoryMap(f, MMAP_PROT_READ_ONLY) }.is_null());
    unsafe { NewFile_close(f) };
}
