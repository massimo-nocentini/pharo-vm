//! File and directory operations against a real temp directory, through the
//! exported C API -- the libc path the VM itself takes. No proxy is present
//! in tests, so the session id is 0 on both sides (records still validate)
//! and filename resolution takes the plain-copy fallback.

use std::path::{Path, PathBuf};

use libc::c_int;
use pharo_vm_plugin::sqInt;
use FilePlugin::sqfile::SQFile;

/// A per-test scratch directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("file-plugin-test-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self, name: &str) -> Vec<u8> {
        let mut p = self.0.as_os_str().as_encoded_bytes().to_vec();
        p.push(b'/');
        p.extend_from_slice(name.as_bytes());
        p
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(name: &mut [u8], write: bool) -> SQFile {
    let mut f = SQFile::zeroed();
    let ok = unsafe {
        FilePlugin::sqFileOpen(
            &mut f,
            name.as_mut_ptr().cast(),
            name.len() as sqInt,
            sqInt::from(write),
        )
    };
    assert_eq!(ok, 1, "open failed for {:?}", String::from_utf8_lossy(name));
    f
}

fn write(f: &mut SQFile, data: &[u8]) -> usize {
    let mut buf = data.to_vec();
    unsafe { FilePlugin::sqFileWriteFromAt(f, buf.len(), buf.as_mut_ptr().cast(), 0) }
}

fn read(f: &mut SQFile, count: usize) -> Vec<u8> {
    let mut buf = vec![0u8; count];
    let n = unsafe { FilePlugin::sqFileReadIntoAt(f, count, buf.as_mut_ptr().cast(), 0) };
    buf.truncate(n);
    buf
}

#[test]
fn open_write_seek_read_roundtrip() {
    let tmp = TempDir::new("roundtrip");
    let mut name = tmp.path("roundtrip.bin");

    let mut f = open(&mut name, true);
    assert_eq!(unsafe { FilePlugin::sqFileValid(&mut f) }, 1);
    assert_eq!(write(&mut f, b"hello, pharo"), 12);
    assert_eq!(unsafe { FilePlugin::sqFileGetPosition(&mut f) }, 12);
    assert_eq!(unsafe { FilePlugin::sqFileSize(&mut f) }, 12);

    // Seek back and read through the same record: the C's lastOp bookkeeping
    // inserts the required fseek between write and read.
    assert_eq!(unsafe { FilePlugin::sqFileSetPosition(&mut f, 7) }, 1);
    assert_eq!(read(&mut f, 5), b"pharo");
    assert_eq!(unsafe { FilePlugin::sqFileAtEnd(&mut f) }, 1);

    assert_eq!(unsafe { FilePlugin::sqFileClose(&mut f) }, 1);
    assert_eq!(unsafe { FilePlugin::sqFileValid(&mut f) }, 0);
}

#[test]
fn read_into_offset_is_zero_based() {
    let tmp = TempDir::new("offset");
    let mut name = tmp.path("offset.bin");
    let mut f = open(&mut name, true);
    write(&mut f, b"XYZ");
    unsafe { FilePlugin::sqFileSetPosition(&mut f, 0) };

    let mut buf = vec![b'.'; 5];
    let n = unsafe { FilePlugin::sqFileReadIntoAt(&mut f, 3, buf.as_mut_ptr().cast(), 2) };
    assert_eq!(n, 3);
    assert_eq!(&buf, b"..XYZ");
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn at_end_is_smalltalk_style_not_feof() {
    let tmp = TempDir::new("atend");
    let mut name = tmp.path("atend.bin");
    let mut f = open(&mut name, true);
    write(&mut f, b"ab");
    unsafe { FilePlugin::sqFileSetPosition(&mut f, 0) };

    // After reading the last byte -- but not past it -- atEnd must already
    // answer true; feof alone would still say false.
    assert_eq!(unsafe { FilePlugin::sqFileAtEnd(&mut f) }, 0);
    assert_eq!(read(&mut f, 2), b"ab");
    assert_eq!(unsafe { FilePlugin::sqFileAtEnd(&mut f) }, 1);
    // The peek must not have consumed anything.
    assert_eq!(unsafe { FilePlugin::sqFileGetPosition(&mut f) }, 2);
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn truncate_shortens_and_flush_sync_succeed() {
    let tmp = TempDir::new("truncate");
    let mut name = tmp.path("truncate.bin");
    let mut f = open(&mut name, true);
    write(&mut f, b"0123456789");
    assert_eq!(unsafe { FilePlugin::sqFileFlush(&mut f) }, 1);
    assert_eq!(unsafe { FilePlugin::sqFileSync(&mut f) }, 1);
    assert_eq!(unsafe { FilePlugin::sqFileTruncate(&mut f, 4) }, 1);
    assert_eq!(unsafe { FilePlugin::sqFileSize(&mut f) }, 4);
    unsafe { FilePlugin::sqFileSetPosition(&mut f, 0) };
    assert_eq!(read(&mut f, 10), b"0123");
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn read_only_open_rejects_writes() {
    let tmp = TempDir::new("readonly");
    let mut name = tmp.path("ro.bin");
    std::fs::write(Path::new(std::str::from_utf8(&name).unwrap()), b"data").unwrap();

    let mut f = open(&mut name, false);
    // Not writable: the write answers 0 (and would set the failure flag if a
    // proxy were present).
    assert_eq!(write(&mut f, b"nope"), 0);
    assert_eq!(read(&mut f, 4), b"data");
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn open_missing_file_read_only_fails_cleanly() {
    let tmp = TempDir::new("missing");
    let mut name = tmp.path("does-not-exist");
    let mut f = SQFile::zeroed();
    let ok =
        unsafe { FilePlugin::sqFileOpen(&mut f, name.as_mut_ptr().cast(), name.len() as sqInt, 0) };
    assert_eq!(ok, 0);
    assert_eq!(unsafe { FilePlugin::sqFileValid(&mut f) }, 0);
}

#[test]
fn open_write_mode_does_not_truncate_existing_content() {
    let tmp = TempDir::new("notrunc");
    let mut name = tmp.path("keep.bin");
    // Unlike fopen("w"), the open dance uses O_RDWR without O_TRUNC.
    std::fs::write(Path::new(std::str::from_utf8(&name).unwrap()), b"keep me").unwrap();
    let mut f = open(&mut name, true);
    assert_eq!(unsafe { FilePlugin::sqFileSize(&mut f) }, 7);
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn open_new_sets_exists_flag_on_collision() {
    let tmp = TempDir::new("opennew");
    let mut name = tmp.path("fresh.bin");

    let mut f = SQFile::zeroed();
    let mut exists: c_int = 0;
    let ok = unsafe {
        FilePlugin::sqFileOpenNew(
            &mut f,
            name.as_mut_ptr().cast(),
            name.len() as sqInt,
            &mut exists,
        )
    };
    assert_eq!((ok, exists), (1, 0));
    write(&mut f, b"x");
    unsafe { FilePlugin::sqFileClose(&mut f) };

    // Second create of the same name: fails, with the exists flag set --
    // what primitiveFileOpenNew turns into PrimErrInappropriate.
    let mut g = SQFile::zeroed();
    let ok = unsafe {
        FilePlugin::sqFileOpenNew(
            &mut g,
            name.as_mut_ptr().cast(),
            name.len() as sqInt,
            &mut exists,
        )
    };
    assert_eq!((ok, exists), (0, 1));
}

#[test]
fn rename_and_delete() {
    let tmp = TempDir::new("rename");
    let mut old = tmp.path("old.bin");
    let mut new = tmp.path("new.bin");
    std::fs::write(Path::new(std::str::from_utf8(&old).unwrap()), b"z").unwrap();

    let ok = unsafe {
        FilePlugin::sqFileRenameOldSizeNewSize(
            old.as_mut_ptr().cast(),
            old.len() as sqInt,
            new.as_mut_ptr().cast(),
            new.len() as sqInt,
        )
    };
    assert_eq!(ok, 1);
    assert!(!Path::new(std::str::from_utf8(&old).unwrap()).exists());

    let ok =
        unsafe { FilePlugin::sqFileDeleteNameSize(new.as_mut_ptr().cast(), new.len() as sqInt) };
    assert_eq!(ok, 1);
    assert!(!Path::new(std::str::from_utf8(&new).unwrap()).exists());

    // Deleting again fails.
    let ok =
        unsafe { FilePlugin::sqFileDeleteNameSize(new.as_mut_ptr().cast(), new.len() as sqInt) };
    assert_eq!(ok, 0);
}

#[test]
fn connect_to_file_descriptor_adopts_the_fd() {
    let tmp = TempDir::new("connect");
    let path = tmp.path("fd.bin");
    let cpath = std::ffi::CString::new(path.clone()).unwrap();
    let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600) };
    assert!(fd >= 0);

    let mut f = SQFile::zeroed();
    assert_eq!(
        unsafe { FilePlugin::sqConnectToFileDescriptor(&mut f, fd, 1) },
        1
    );
    assert_eq!(unsafe { FilePlugin::sqFileValid(&mut f) }, 1);
    assert_eq!(write(&mut f, b"via fd"), 6);
    unsafe { FilePlugin::sqFileClose(&mut f) };

    assert_eq!(
        std::fs::read(Path::new(std::str::from_utf8(&path).unwrap())).unwrap(),
        b"via fd"
    );
}

#[test]
fn a_stale_session_invalidates_the_record() {
    let tmp = TempDir::new("session");
    let mut name = tmp.path("session.bin");
    let mut f = open(&mut name, true);
    // A record written by a previous VM run carries that run's session id.
    f.sessionID = f.sessionID.wrapping_add(1);
    assert_eq!(unsafe { FilePlugin::sqFileValid(&mut f) }, 0);
    // Restore so the FILE* can be closed without leaking.
    f.sessionID = f.sessionID.wrapping_sub(1);
    unsafe { FilePlugin::sqFileClose(&mut f) };
}

#[test]
fn descriptor_type_distinguishes_files() {
    let tmp = TempDir::new("fdtype");
    let path = tmp.path("plain.bin");
    let cpath = std::ffi::CString::new(path).unwrap();
    let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600) };
    assert!(fd >= 0);
    // A regular file answers 3; a bad descriptor answers -1. (Terminals
    // answer 1, but the test environment has no tty to prove it with.)
    assert_eq!(FilePlugin::sqFileDescriptorType(fd), 3);
    unsafe { libc::close(fd) };
    assert_eq!(FilePlugin::sqFileDescriptorType(-1), -1);
}

// ---------------------------------------------------------------------------
// Directories
// ---------------------------------------------------------------------------

#[test]
fn dir_create_and_delete() {
    let tmp = TempDir::new("dircreate");
    let mut name = tmp.path("subdir");
    let ok = unsafe { FilePlugin::dir_Create(name.as_mut_ptr().cast(), name.len() as sqInt) };
    assert_eq!(ok, 1);
    assert!(Path::new(std::str::from_utf8(&name).unwrap()).is_dir());

    let ok = unsafe { FilePlugin::dir_Delete(name.as_mut_ptr().cast(), name.len() as sqInt) };
    assert_eq!(ok, 1);
    assert!(!Path::new(std::str::from_utf8(&name).unwrap()).exists());

    // Deleting a directory that is gone fails.
    let ok = unsafe { FilePlugin::dir_Delete(name.as_mut_ptr().cast(), name.len() as sqInt) };
    assert_eq!(ok, 0);
}

#[test]
fn dir_delimitor_is_slash() {
    assert_eq!(FilePlugin::dir_Delimitor(), '/' as sqInt);
}

#[test]
fn dir_lookup_enumerates_every_entry_once() {
    let tmp = TempDir::new("dirlookup");
    for n in ["alpha", "beta", "gamma"] {
        std::fs::write(
            Path::new(std::str::from_utf8(&tmp.path(n)).unwrap()),
            n.as_bytes(),
        )
        .unwrap();
    }
    let dir_path = tmp.0.as_os_str().as_encoded_bytes().to_vec();

    let mut seen = Vec::new();
    for index in (1 as sqInt).. {
        match FilePlugin::dir::lookup(&dir_path, index) {
            Ok(entry) => {
                let name = entry.name[..entry.name_len as usize].to_vec();
                assert!(!entry.is_directory);
                assert_eq!(entry.size_if_file, name.len() as u64, "file content = name");
                assert!(entry.creation_date > 0);
                assert!(entry.modification_date > 0);
                assert_ne!(entry.posix_permissions, 0);
                seen.push(name);
            }
            Err(status) => {
                assert_eq!(status, FilePlugin::dir::NO_MORE_ENTRIES);
                break;
            }
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()]
    );
}

#[test]
fn dir_lookup_on_missing_directory_is_bad_path() {
    assert!(matches!(
        FilePlugin::dir::lookup(b"/no/such/directory/anywhere", 1),
        Err(FilePlugin::dir::BAD_PATH)
    ));
}

#[test]
fn dir_entry_lookup_finds_by_name() {
    let tmp = TempDir::new("direntry");
    std::fs::write(
        Path::new(std::str::from_utf8(&tmp.path("target.txt")).unwrap()),
        b"12345",
    )
    .unwrap();
    let sub = tmp.path("nested");
    std::fs::create_dir(Path::new(std::str::from_utf8(&sub).unwrap())).unwrap();
    let dir_path = tmp.0.as_os_str().as_encoded_bytes().to_vec();

    let entry = FilePlugin::dir::entry_lookup(&dir_path, b"target.txt").unwrap();
    assert_eq!(&entry.name[..entry.name_len as usize], b"target.txt");
    assert!(!entry.is_directory);
    assert_eq!(entry.size_if_file, 5);

    let entry = FilePlugin::dir::entry_lookup(&dir_path, b"nested").unwrap();
    assert!(entry.is_directory);
    assert_eq!(entry.size_if_file, 0);

    assert!(matches!(
        FilePlugin::dir::entry_lookup(&dir_path, b"absent"),
        Err(FilePlugin::dir::NO_MORE_ENTRIES)
    ));
}

#[test]
fn dir_entry_lookup_c_abi_writes_outputs() {
    let tmp = TempDir::new("direntryc");
    std::fs::write(
        Path::new(std::str::from_utf8(&tmp.path("abi.bin")).unwrap()),
        b"xyz",
    )
    .unwrap();
    let mut dir_path = tmp.0.as_os_str().as_encoded_bytes().to_vec();
    let mut entry_name = b"abi.bin".to_vec();

    let mut name = [0u8; 256];
    let (mut name_len, mut cdate, mut mdate, mut is_dir, mut posix, mut is_link): (
        sqInt,
        sqInt,
        sqInt,
        sqInt,
        sqInt,
        sqInt,
    ) = (0, 0, 0, 0, 0, 0);
    let mut size: u64 = 0;
    let status = unsafe {
        FilePlugin::dir_EntryLookup(
            dir_path.as_mut_ptr().cast(),
            dir_path.len() as sqInt,
            entry_name.as_mut_ptr().cast(),
            entry_name.len() as sqInt,
            name.as_mut_ptr().cast(),
            &mut name_len,
            &mut cdate,
            &mut mdate,
            &mut is_dir,
            &mut size,
            &mut posix,
            &mut is_link,
        )
    };
    assert_eq!(status, FilePlugin::dir::ENTRY_FOUND);
    assert_eq!(&name[..name_len as usize], b"abi.bin");
    assert_eq!((is_dir, size, is_link), (0, 3, 0));
    assert!(cdate > 0 && mdate > 0);
}

#[test]
fn convert_to_squeak_time_offsets_the_epoch() {
    // Without a VM there is no GMT offset, leaving the bare epoch delta:
    // 17 leap + 52 non-leap years.
    const DELTA: i64 = (52 * 365 + 17 * 366) * 24 * 60 * 60;
    assert_eq!(FilePlugin::convertToSqueakTime(0), DELTA);
    assert_eq!(
        FilePlugin::convertToSqueakTime(1_000_000),
        DELTA + 1_000_000
    );
}

#[test]
fn stdio_handles_fill_three_records() {
    let mut records = [SQFile::zeroed(), SQFile::zeroed(), SQFile::zeroed()];
    let mask = unsafe { FilePlugin::sqFileStdioHandlesInto(records.as_mut_ptr()) };
    assert_eq!(mask, 7);
    assert_eq!(records[0].writable, 0);
    assert_eq!(records[1].writable, 1);
    assert_eq!(records[2].writable, 1);
    assert!(!records[0].file.is_null());
    assert!(!records[1].file.is_null());
    assert!(!records[2].file.is_null());
    // stdout/stderr are always flagged stdio streams.
    assert_eq!(records[1].isStdioStream, 1);
    assert_eq!(records[2].isStdioStream, 1);
    assert_eq!(records[0].lastChar, libc::EOF as libc::c_char);
}
