//! `stat`/`lstat`/`access`/`readlink` and the attribute-value marshalling
//! rules from the Unix `faSupport.c`.
//!
//! The image is answered the OS's raw values -- `st_mode` bit-for-bit,
//! `time_t` seconds converted to the Squeak epoch -- so this goes through
//! `libc` directly; `std::fs::Metadata` is used only by the tests, as an
//! independent witness.
//!
//! Which value is boxed how (32-bit unsigned, 64-bit unsigned, 64-bit
//! signed, nil) is part of the plugin's contract with the image and is kept
//! as a pure, testable table: see [`stat_attribute_values`] and
//! [`single_attribute_value`] -- including the C's quirk of boxing `st_nlink`
//! 32-bit in the attribute array but 64-bit as a single attribute.

use std::ffi::{c_int, CStr};

use crate::codes::FA_CANT_STAT_PATH;
use crate::fapath::FA_PATH_MAX;

/// Seconds between the Squeak epoch (1 Jan 1901) and the Unix epoch
/// (1 Jan 1970): 52 non-leap years and 17 leap years, as counted by
/// `faConvertUnixToLongSqueakTime`.
pub const SQUEAK_EPOCH_OFFSET: i64 = (52 * 365 + 17 * 366) * 24 * 60 * 60;

/// One `struct stat`, reduced to the fields the plugin answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    /// `st_mode`, raw.
    pub mode: u32,
    /// `st_ino`.
    pub ino: u64,
    /// `st_dev`.
    pub dev: u64,
    /// `st_nlink`.
    pub nlink: u64,
    /// `st_uid`.
    pub uid: u32,
    /// `st_gid`.
    pub gid: u32,
    /// `st_size`, signed as the OS reports it.
    pub size: i64,
    /// `st_atime`, seconds.
    pub atime: i64,
    /// `st_mtime`, seconds.
    pub mtime: i64,
    /// `st_ctime`, seconds.
    pub ctime: i64,
}

impl FileStat {
    /// `S_ISDIR`.
    #[must_use]
    #[allow(clippy::unnecessary_cast)] // mode_t is not u32 on every Unix
    pub fn is_dir(&self) -> bool {
        self.mode & (libc::S_IFMT as u32) == libc::S_IFDIR as u32
    }

    /// `S_ISLNK`.
    #[must_use]
    #[allow(clippy::unnecessary_cast)] // mode_t is not u32 on every Unix
    pub fn is_symlink(&self) -> bool {
        self.mode & (libc::S_IFMT as u32) == libc::S_IFLNK as u32
    }

    // The casts widen platform-dependent field types (u16/u32/i32 on some
    // OSes) exactly as C's integer promotions to the proxy's parameter types
    // do -- including sign-extension for a negative i32 st_dev on macOS.
    #[allow(clippy::unnecessary_cast)]
    fn from_raw(st: &libc::stat) -> Self {
        Self {
            mode: st.st_mode as u32,
            ino: st.st_ino as u64,
            dev: st.st_dev as u64,
            nlink: st.st_nlink as u64,
            uid: st.st_uid as u32,
            gid: st.st_gid as u32,
            size: st.st_size as i64,
            atime: st.st_atime as i64,
            mtime: st.st_mtime as i64,
            ctime: st.st_ctime as i64,
        }
    }
}

/// `stat()`. Any failure is `FA_CANT_STAT_PATH`, whatever errno says --
/// that is the C's mapping.
pub fn stat_path(path: &CStr) -> Result<FileStat, i64> {
    // SAFETY: `path` is NUL-terminated and `st` is a writable out-param the
    // call fully initialises on success.
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    let status = unsafe { libc::stat(path.as_ptr(), st.as_mut_ptr()) };
    if status != 0 {
        return Err(FA_CANT_STAT_PATH);
    }
    // SAFETY: stat() returned 0, so the buffer is initialised.
    Ok(FileStat::from_raw(unsafe { &st.assume_init() }))
}

/// `lstat()`, same error mapping.
pub fn lstat_path(path: &CStr) -> Result<FileStat, i64> {
    // SAFETY: as in `stat_path`.
    let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
    let status = unsafe { libc::lstat(path.as_ptr(), st.as_mut_ptr()) };
    if status != 0 {
        return Err(FA_CANT_STAT_PATH);
    }
    // SAFETY: lstat() returned 0, so the buffer is initialised.
    Ok(FileStat::from_raw(unsafe { &st.assume_init() }))
}

/// `access(path, mode) == 0`.
#[must_use]
pub fn access_ok(path: &CStr, mode: c_int) -> bool {
    // SAFETY: `path` is NUL-terminated.
    unsafe { libc::access(path.as_ptr(), mode) == 0 }
}

/// `readlink()` into an `FA_PATH_MAX` buffer. `None` on failure -- the C
/// then answers the attributes with a nil target rather than failing.
///
/// (The C NUL-terminates at `targetFile[status]`, one byte past the buffer
/// when the target fills it exactly; the length-checked `Vec` removes that
/// out-of-bounds write and nothing else.)
#[must_use]
pub fn read_link(path: &CStr) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; FA_PATH_MAX];
    // SAFETY: `buf` provides FA_PATH_MAX writable bytes, and that is the
    // length passed.
    let n = unsafe { libc::readlink(path.as_ptr(), buf.as_mut_ptr().cast(), FA_PATH_MAX) };
    if n < 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(buf)
}

/// `faConvertUnixToLongSqueakTime`: Unix UTC seconds to Squeak local-time
/// seconds since 1901, via the timezone offset in effect *at that instant*
/// (`tm_gmtoff` of `localtime`).
///
/// The C dereferences `localtime()`'s result unchecked; for a time it cannot
/// represent this port uses a zero offset instead of crashing.
#[must_use]
#[allow(clippy::unnecessary_cast)] // tm_gmtoff is c_long: i32 on 32-bit targets
pub fn squeak_time(unix_time: i64) -> i64 {
    let t = unix_time as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: localtime_r fills `tm` when it answers non-null and leaves it
    // untouched otherwise; we read it only on success.
    let gmtoff = unsafe {
        if libc::localtime_r(&t, tm.as_mut_ptr()).is_null() {
            0
        } else {
            tm.assume_init().tm_gmtoff as i64
        }
    };
    unix_time + gmtoff + SQUEAK_EPOCH_OFFSET
}

/// How a value crosses to the image: which proxy boxing the C used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrValue {
    /// `nilObject`.
    Nil,
    /// `positive32BitIntegerFor`.
    P32(u32),
    /// `positive64BitIntegerFor`.
    P64(u64),
    /// `signed64BitIntegerFor`.
    S64(i64),
}

/// The size the plugin reports: 0 for a directory, `st_size` otherwise --
/// and boxed differently in the two cases, as in C.
fn size_value(fs: &FileStat) -> AttrValue {
    if fs.is_dir() {
        AttrValue::P32(0)
    } else {
        AttrValue::P64(fs.size as u64)
    }
}

/// Slots 1..12 of the attribute array `faFileStatAttributes` fills (slot 0 is
/// the symlink target, handled by the caller): mode, inode, device, nlink,
/// uid, gid, size, and the three timestamps, then nil for creation date and
/// nil for the Windows attribute flags.
#[must_use]
pub fn stat_attribute_values(fs: &FileStat) -> [AttrValue; 12] {
    [
        AttrValue::P32(fs.mode),
        AttrValue::P64(fs.ino),
        AttrValue::P64(fs.dev),
        // 32-bit here but 64-bit in single_attribute_value: the C really
        // boxes nlink both ways depending on the primitive.
        AttrValue::P32(fs.nlink as u32),
        AttrValue::P32(fs.uid),
        AttrValue::P32(fs.gid),
        size_value(fs),
        AttrValue::S64(squeak_time(fs.atime)),
        AttrValue::S64(squeak_time(fs.mtime)),
        AttrValue::S64(squeak_time(fs.ctime)),
        AttrValue::Nil,
        AttrValue::Nil,
    ]
}

/// `primitiveFileMasks`' answer: the eight `S_IF*` masks in the C's fixed
/// slot order (fmt, socket, link, regular, block, directory, character,
/// fifo). All eight exist on Unix; the C's Windows build leaves the socket
/// and link slots nil.
#[must_use]
#[allow(clippy::unnecessary_cast)] // mode_t is not u32 on every Unix
pub fn file_mask_values() -> [u32; 8] {
    [
        libc::S_IFMT as u32,
        libc::S_IFSOCK as u32,
        libc::S_IFLNK as u32,
        libc::S_IFREG as u32,
        libc::S_IFBLK as u32,
        libc::S_IFDIR as u32,
        libc::S_IFCHR as u32,
        libc::S_IFIFO as u32,
    ]
}

/// The stat-derived single attributes of `faFileAttribute`, numbers 1..12.
/// 1 (file name) and 12 (creation date) answer nil -- after a successful
/// `stat`, which the caller performs.
#[must_use]
pub fn single_attribute_value(fs: &FileStat, attribute_number: isize) -> AttrValue {
    match attribute_number {
        2 => AttrValue::P32(fs.mode),
        3 => AttrValue::P64(fs.ino),
        4 => AttrValue::P64(fs.dev),
        5 => AttrValue::P64(fs.nlink),
        6 => AttrValue::P32(fs.uid),
        7 => AttrValue::P32(fs.gid),
        8 => size_value(fs),
        9 => AttrValue::S64(squeak_time(fs.atime)),
        10 => AttrValue::S64(squeak_time(fs.mtime)),
        11 => AttrValue::S64(squeak_time(fs.ctime)),
        // 1 = file name, 12 = creation date: nil on Unix.
        _ => AttrValue::Nil,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::ffi::CString;
    use std::os::unix::fs::MetadataExt;

    fn cpath(p: &std::path::Path) -> CString {
        CString::new(p.as_os_str().as_encoded_bytes()).unwrap()
    }

    #[test]
    fn epoch_offset_is_the_known_constant() {
        assert_eq!(SQUEAK_EPOCH_OFFSET, 2_177_452_800);
    }

    /// The conversion is exactly time + gmtoff(time) + offset.
    #[test]
    #[allow(clippy::unnecessary_cast)] // tm_gmtoff is c_long: i32 on 32-bit targets
    fn squeak_time_formula() {
        for &t in &[0i64, 1_000_000_000, 1_700_000_000] {
            let tt = t as libc::time_t;
            let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
            let off = unsafe {
                assert!(!libc::localtime_r(&tt, tm.as_mut_ptr()).is_null());
                tm.assume_init().tm_gmtoff as i64
            };
            assert_eq!(squeak_time(t), t + off + SQUEAK_EPOCH_OFFSET);
        }
    }

    /// stat fields match std::fs::Metadata (an independent path to the same
    /// syscall) on a file we control.
    #[test]
    fn regular_file_fields_match_metadata() {
        let dir = TempDir::new("stat-file");
        let file = dir.path().join("data.bin");
        std::fs::write(&file, [7u8; 321]).unwrap();

        let fs = stat_path(&cpath(&file)).unwrap();
        let meta = std::fs::metadata(&file).unwrap();
        assert_eq!(fs.mode, meta.mode());
        assert_eq!(fs.ino, meta.ino());
        assert_eq!(fs.dev, meta.dev());
        assert_eq!(fs.nlink, meta.nlink());
        assert_eq!(fs.uid, meta.uid());
        assert_eq!(fs.gid, meta.gid());
        assert_eq!(fs.size, 321);
        assert_eq!(fs.mtime, meta.mtime());
        assert_eq!(fs.ctime, meta.ctime());
        assert!(!fs.is_dir());
        assert!(!fs.is_symlink());
    }

    #[test]
    fn directory_size_is_reported_as_zero() {
        let dir = TempDir::new("stat-dir");
        let fs = stat_path(&cpath(dir.path())).unwrap();
        assert!(fs.is_dir());
        // The array/single-attribute rule, not the raw st_size.
        assert_eq!(size_value(&fs), AttrValue::P32(0));
        assert_eq!(single_attribute_value(&fs, 8), AttrValue::P32(0));
    }

    #[test]
    fn missing_path_is_cant_stat() {
        let dir = TempDir::new("stat-enoent");
        let missing = dir.path().join("nope");
        assert_eq!(stat_path(&cpath(&missing)), Err(FA_CANT_STAT_PATH));
        assert_eq!(lstat_path(&cpath(&missing)), Err(FA_CANT_STAT_PATH));
    }

    /// EACCES surfaces as the same FA_CANT_STAT_PATH as ENOENT: the C folds
    /// every stat failure into one code.
    #[test]
    fn unsearchable_directory_is_cant_stat() {
        // Root ignores permission bits.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = TempDir::new("stat-eacces");
        let sub = dir.path().join("locked");
        std::fs::create_dir(&sub).unwrap();
        let file = sub.join("f");
        std::fs::write(&file, b"x").unwrap();
        let locked = cpath(&sub);
        unsafe { libc::chmod(locked.as_ptr(), 0) };
        let result = stat_path(&cpath(&file));
        unsafe { libc::chmod(locked.as_ptr(), 0o755) };
        assert_eq!(result, Err(FA_CANT_STAT_PATH));
    }

    #[test]
    fn symlink_lstat_stat_and_target() {
        let dir = TempDir::new("stat-symlink");
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"hello").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let via_lstat = lstat_path(&cpath(&link)).unwrap();
        assert!(via_lstat.is_symlink());
        let via_stat = stat_path(&cpath(&link)).unwrap();
        assert!(!via_stat.is_symlink());
        assert_eq!(via_stat.size, 5);

        let read = read_link(&cpath(&link)).unwrap();
        assert_eq!(read, target.as_os_str().as_encoded_bytes());
        // readlink on a non-link fails; the plugin then reports a nil target.
        assert!(read_link(&cpath(&target)).is_none());
    }

    /// A dangling symlink: lstat succeeds, stat fails -- the pair of results
    /// primitiveFileAttributes relies on for masks 5 vs 1.
    #[test]
    fn dangling_symlink() {
        let dir = TempDir::new("stat-dangling");
        let link = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
        assert!(lstat_path(&cpath(&link)).unwrap().is_symlink());
        assert_eq!(stat_path(&cpath(&link)), Err(FA_CANT_STAT_PATH));
        assert_eq!(
            read_link(&cpath(&link)).unwrap(),
            dir.path().join("gone").as_os_str().as_encoded_bytes()
        );
    }

    #[test]
    fn access_checks() {
        let dir = TempDir::new("stat-access");
        let file = dir.path().join("f");
        std::fs::write(&file, b"x").unwrap();
        let c = cpath(&file);
        assert!(access_ok(&c, libc::F_OK));
        assert!(access_ok(&c, libc::R_OK));
        assert!(!access_ok(&cpath(&dir.path().join("missing")), libc::F_OK));
        if unsafe { libc::geteuid() } != 0 {
            unsafe { libc::chmod(c.as_ptr(), 0) };
            assert!(!access_ok(&c, libc::R_OK));
            assert!(access_ok(&c, libc::F_OK));
            unsafe { libc::chmod(c.as_ptr(), 0o644) };
        }
    }

    /// The classic POSIX mode-mask values, in the C's slot order.
    #[test]
    fn file_masks_are_the_s_if_constants() {
        assert_eq!(
            file_mask_values(),
            [
                0o170_000, // S_IFMT
                0o140_000, // S_IFSOCK
                0o120_000, // S_IFLNK
                0o100_000, // S_IFREG
                0o060_000, // S_IFBLK
                0o040_000, // S_IFDIR
                0o020_000, // S_IFCHR
                0o010_000, // S_IFIFO
            ]
        );
    }

    /// The two nlink boxings and the nil slots, pinned.
    #[test]
    fn attribute_value_table_shape() {
        let dir = TempDir::new("stat-table");
        let file = dir.path().join("f");
        std::fs::write(&file, b"abc").unwrap();
        let fs = stat_path(&cpath(&file)).unwrap();

        let values = stat_attribute_values(&fs);
        assert_eq!(values[0], AttrValue::P32(fs.mode));
        assert_eq!(values[1], AttrValue::P64(fs.ino));
        assert_eq!(values[2], AttrValue::P64(fs.dev));
        assert_eq!(values[3], AttrValue::P32(fs.nlink as u32));
        assert_eq!(values[4], AttrValue::P32(fs.uid));
        assert_eq!(values[5], AttrValue::P32(fs.gid));
        assert_eq!(values[6], AttrValue::P64(3));
        assert_eq!(values[7], AttrValue::S64(squeak_time(fs.atime)));
        assert_eq!(values[8], AttrValue::S64(squeak_time(fs.mtime)));
        assert_eq!(values[9], AttrValue::S64(squeak_time(fs.ctime)));
        assert_eq!(values[10], AttrValue::Nil);
        assert_eq!(values[11], AttrValue::Nil);

        assert_eq!(single_attribute_value(&fs, 1), AttrValue::Nil);
        assert_eq!(single_attribute_value(&fs, 5), AttrValue::P64(fs.nlink));
        assert_eq!(single_attribute_value(&fs, 8), AttrValue::P64(3));
        assert_eq!(single_attribute_value(&fs, 12), AttrValue::Nil);
    }
}
