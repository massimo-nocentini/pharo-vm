//! `FileAttributesPlugin`, in Rust: file stat/access attributes and directory
//! enumeration for the image's `FileSystem` layer.
//!
//! Replaces the Slang-generated `FileAttributesPlugin.c` plus the hand-written
//! `faCommon.c` and the **Unix** `faSupport.c` behind the same sixteen
//! primitives: same names, same argument shapes, same answers, and -- because
//! the image maps them to exceptions -- exactly the status codes of
//! `faConstants.h`. The Windows support layer keeps its C; see the README.
//!
//! # What changes underneath
//!
//! * **No raw pointer in image memory.** The C hands the image a ByteArray
//!   holding a live `fapath *` and dereferences whatever comes back; here the
//!   same bytes carry a key into a registry, and an unknown key fails with
//!   `FA_BAD_SESSION_ID` instead of reading freed memory. See [`dir`].
//! * **FilePlugin is a runtime lookup, not a link dependency.** The C links
//!   against FilePlugin for its `sq2uxPath`/`ux2sqPath` path-encoding
//!   functions; this port fetches them through `ioLoadFunctionFrom` and falls
//!   back to a built-in equivalent. See [`convert`].
//! * A handful of C undefined behaviours are gone; each is listed in the
//!   README's divergence table.

// Primitive names are fixed by the image; the crate is named for the shared
// library the VM loads.
#![allow(non_snake_case)]
// The support layer is the *Unix* one by design: Windows keeps the C plugin.
#![cfg(unix)]

mod codes;
mod convert;
mod dir;
mod fapath;
mod stat;

use std::ffi::{c_long, c_void, CStr};
use std::os::unix::fs::PermissionsExt;
use std::sync::OnceLock;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use codes::{FA_BAD_SESSION_ID, FA_CANT_STAT_PATH, FA_INTERPRETER_ERROR, FA_INVALID_ARGUMENTS,
    FA_STRING_TOO_LONG};
use convert::{CConvertFn, Converters};
use dir::{DirSession, ReadOutcome};
use fapath::{FaPath, FA_PATH_MAX};
use stat::AttrValue;

// The C's getModuleName answers
// "FileAttributesPlugin FileAttributesPlugin.oscog-akg.49 (e)"; the VM
// compares only the module-name prefix, so the bare name suffices (the
// jpeg-plugin precedent). The image asks for the plugin version through
// primitiveVersionString, not through this string.
pharo_plugin!("FileAttributesPlugin", init = initialise);

/// The C `initialiseModule` answers 1 and does nothing else. In particular it
/// never calls `faCommon.c`'s `faInitialiseModule`, so the session-id global
/// stays 0 for the life of the process -- a fact [`dir`] documents and
/// preserves.
fn initialise() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Proxy plumbing the safe SDK does not cover
// ---------------------------------------------------------------------------

/// Records an OS-level failure exactly as the C does: through the proxy's
/// `primitiveFailForOSError`, which stores `code` for the image and sets the
/// failure to `PrimErrOSError`. Returning the `PrimErr::OSError` this answers
/// makes the SDK call `primitiveFailFor(PrimErrOSError)` afterwards, which
/// re-stores the same failure code and leaves the recorded OS error intact.
fn os_error(vm: &Interp, code: i64) -> PrimErr {
    // SAFETY: as_raw() is the VM's own proxy table; the signature is the one
    // in proxy.rs. A missing entry (pre-1.14 proxy) degrades to a plain
    // OSError failure, as the C's #if fallback does.
    unsafe {
        if let Some(f) = (*vm.as_raw()).primitiveFailForOSError {
            f(code as c_long);
        }
    }
    PrimErr::OSError
}

/// `positive32BitIntegerFor`.
fn positive32(vm: &Interp, value: u32) -> PrimResult<Oop> {
    // SAFETY: proxy call, signature per proxy.rs.
    let f = unsafe { (*vm.as_raw()).positive32BitIntegerFor }.ok_or(PrimErr::Unsupported)?;
    Ok(Oop(unsafe { f(value) }))
}

/// `positive64BitIntegerFor`.
fn positive64(vm: &Interp, value: u64) -> PrimResult<Oop> {
    // SAFETY: proxy call, signature per proxy.rs.
    let f = unsafe { (*vm.as_raw()).positive64BitIntegerFor }.ok_or(PrimErr::Unsupported)?;
    Ok(Oop(unsafe { f(value as std::ffi::c_ulong) }))
}

/// `signed64BitIntegerFor`.
fn signed64(vm: &Interp, value: i64) -> PrimResult<Oop> {
    // SAFETY: proxy call, signature per proxy.rs.
    let f = unsafe { (*vm.as_raw()).signed64BitIntegerFor }.ok_or(PrimErr::Unsupported)?;
    Ok(Oop(unsafe { f(value as c_long) }))
}

/// `storePointerofObjectwithValue`: the store-check-aware way to fill a slot
/// of a pointers object (a plain memory write would skip the remembered set).
fn store_pointer(vm: &Interp, index: sqInt, object: Oop, value: Oop) -> PrimResult<()> {
    // SAFETY: proxy call, signature per proxy.rs.
    let f =
        unsafe { (*vm.as_raw()).storePointerofObjectwithValue }.ok_or(PrimErr::Unsupported)?;
    unsafe { f(index, object.0, value.0) };
    Ok(())
}

/// The path-encoding functions the C plugin linked from FilePlugin, resolved
/// once through `ioLoadFunctionFrom` and cached. When FilePlugin (or the
/// proxy entry) is unavailable the fallback conversion in [`convert`] runs
/// instead.
fn load_converters(vm: &Interp) -> Converters {
    static CONVERTERS: OnceLock<Converters> = OnceLock::new();
    *CONVERTERS.get_or_init(|| Converters {
        sq2ux: resolve_from_file_plugin(vm, c"sq2uxPath"),
        ux2sq: resolve_from_file_plugin(vm, c"ux2sqPath"),
    })
}

fn resolve_from_file_plugin(vm: &Interp, name: &CStr) -> Option<CConvertFn> {
    const FILE_PLUGIN: &CStr = c"FilePlugin";
    // SAFETY: proxy call; ioLoadFunctionFrom takes two C strings and answers
    // a symbol address or NULL. It may load FilePlugin as a side effect,
    // which is precisely the sanctioned way to reach another plugin.
    let load = unsafe { (*vm.as_raw()).ioLoadFunctionFrom }?;
    let address = unsafe {
        load(
            name.as_ptr().cast_mut(),
            FILE_PLUGIN.as_ptr().cast_mut(),
        )
    };
    if address.is_null() {
        return None;
    }
    // SAFETY: the symbol is FilePlugin's sq2uxPath/ux2sqPath, whose C
    // signature is fixed by sqUnixCharConv.h and matches CConvertFn.
    Some(unsafe { std::mem::transmute::<*mut c_void, CConvertFn>(address) })
}

/// The errno behind a failed `std::fs` call -- the same number the C read
/// from the global right after the syscall, but carried by the error value
/// instead of raced for.
fn io_errno(e: &std::io::Error) -> i64 {
    i64::from(e.raw_os_error().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Shared marshalling, mirroring the C helpers
// ---------------------------------------------------------------------------

/// How `attributeArrayformask` reports trouble: either a status code the
/// caller feeds to `primitiveFailForOSError`, or a primitive failure that is
/// already fully described (the C paths that end in `primitiveFailFor` or a
/// failure flagged deeper down).
enum AttrFail {
    Status(i64),
    Prim(PrimErr),
}

impl From<PrimErr> for AttrFail {
    fn from(e: PrimErr) -> Self {
        AttrFail::Prim(e)
    }
}

/// `faCharToByteArray`: a C string's bytes as a new ByteArray. Both C error
/// paths funnel through `primitiveFailForOSError` at the call sites -- with
/// the status for an over-long name, and (a C quirk) with the `PrimErrNoMemory`
/// *code* for a failed allocation.
fn char_to_byte_array(vm: &Interp, bytes: &[u8]) -> PrimResult<Oop> {
    if bytes.len() >= FA_PATH_MAX {
        return Err(os_error(vm, FA_STRING_TOO_LONG));
    }
    let byte_array = vm
        .instantiate(vm.class_byte_array()?, bytes.len() as sqInt)
        .map_err(|_| os_error(vm, PrimErr::NoMemory.code() as i64))?;
    vm.write_bytes(byte_array, 0, bytes)?;
    Ok(byte_array)
}

/// `pathNameToOop`: a platform-encoded name, converted to the image encoding
/// and wrapped in a ByteArray.
fn path_name_to_oop(vm: &Interp, conv: &Converters, plat: &[u8]) -> PrimResult<Oop> {
    if plat.len() >= FA_PATH_MAX {
        return Err(os_error(vm, FA_STRING_TOO_LONG));
    }
    let st = conv.to_smalltalk(plat, FA_PATH_MAX);
    if st.is_empty() {
        return Err(os_error(vm, FA_INVALID_ARGUMENTS));
    }
    char_to_byte_array(vm, &st)
}

/// Boxes one attribute value the way the C proxy calls did.
fn attr_oop(vm: &Interp, value: AttrValue) -> PrimResult<Oop> {
    match value {
        AttrValue::Nil => vm.nil(),
        AttrValue::P32(v) => positive32(vm, v),
        AttrValue::P64(v) => positive64(vm, v),
        AttrValue::S64(v) => signed64(vm, v),
    }
}

/// `faFileStatAttributes`: fills the 13-slot attribute array. With the lstat
/// flag, a symlink's target name goes in slot 0 -- and a failed `readlink`
/// leaves it nil rather than failing, as the C does.
fn fill_stat_array(
    vm: &Interp,
    conv: &Converters,
    fa: &FaPath,
    use_lstat: bool,
    array: Oop,
) -> Result<(), AttrFail> {
    let cpath = fa.plat_cstring();
    let (fs, target) = if use_lstat {
        let fs = stat::lstat_path(&cpath).map_err(AttrFail::Status)?;
        let target = if fs.is_symlink() {
            stat::read_link(&cpath)
        } else {
            None
        };
        (fs, target)
    } else {
        (stat::stat_path(&cpath).map_err(AttrFail::Status)?, None)
    };

    let target_oop = match &target {
        Some(name) => path_name_to_oop(vm, conv, name)?,
        None => vm.nil()?,
    };
    store_pointer(vm, 0, array, target_oop)?;
    for (i, value) in stat::stat_attribute_values(&fs).into_iter().enumerate() {
        let oop = attr_oop(vm, value)?;
        store_pointer(vm, 1 + i as sqInt, array, oop)?;
    }
    Ok(())
}

/// `faAccessAttributes`: R, W, X answered as three booleans.
fn fill_access_array(vm: &Interp, fa: &FaPath, array: Oop) -> PrimResult<()> {
    let cpath = fa.plat_cstring();
    for (i, mode) in [libc::R_OK, libc::W_OK, libc::X_OK].into_iter().enumerate() {
        let oop = if stat::access_ok(&cpath, mode) {
            vm.true_object()?
        } else {
            vm.false_object()?
        };
        store_pointer(vm, i as sqInt, array, oop)?;
    }
    Ok(())
}

/// `attributeArray:for:mask:`: builds the stat array (mask bit 0), the access
/// array (bit 1), or a 2-element array of both; bit 2 selects lstat. The
/// error statuses -- including the C's `-15` for a failed allocation of
/// either inner array -- are kept exactly.
fn attribute_arrays(
    vm: &Interp,
    conv: &Converters,
    fa: &FaPath,
    mask: sqInt,
) -> Result<Oop, AttrFail> {
    let get_stats = mask & 1 != 0;
    let get_access = mask & 2 != 0;
    if !get_stats && !get_access {
        return Err(AttrFail::Status(FA_INVALID_ARGUMENTS));
    }
    let use_lstat = mask & 4 != 0;

    let mut stats_oop = None;
    if get_stats {
        let array = vm
            .instantiate(vm.class_array()?, 13)
            .map_err(|_| AttrFail::Status(FA_INTERPRETER_ERROR))?;
        fill_stat_array(vm, conv, fa, use_lstat, array)?;
        stats_oop = Some(array);
    }
    let mut access_oop = None;
    if get_access {
        let array = vm
            .instantiate(vm.class_array()?, 3)
            .map_err(|_| AttrFail::Status(FA_INTERPRETER_ERROR))?;
        fill_access_array(vm, fa, array)?;
        access_oop = Some(array);
    }
    match (stats_oop, access_oop) {
        (Some(stats), Some(access)) => {
            // This allocation failing ends as a plain PrimErrNoMemory in the
            // C too (the -15 comment there is aspirational).
            let both = vm.instantiate(vm.class_array()?, 2)?;
            store_pointer(vm, 0, both, stats)?;
            store_pointer(vm, 1, both, access)?;
            Ok(both)
        }
        (Some(stats), None) => Ok(stats),
        (None, Some(access)) => Ok(access),
        (None, None) => unreachable!("guarded above"),
    }
}

/// `processDirectory`: the `[entryName, attributes, nil]` array the walk
/// primitives answer; `primitiveOpendir` later fills slot 2 with the handle.
/// An entry that cannot be stat()ed -- vanished between readdir and stat, or
/// a dangling link -- still gets its name, with nil attributes.
fn process_directory(vm: &Interp, conv: &Converters, fa: &FaPath) -> PrimResult<Oop> {
    let entry_name = char_to_byte_array(vm, fa.st_file())?;
    let attributes = match attribute_arrays(vm, conv, fa, 1) {
        Ok(oop) => oop,
        Err(AttrFail::Status(FA_CANT_STAT_PATH)) => vm.nil()?,
        Err(AttrFail::Status(status)) => return Err(os_error(vm, status)),
        Err(AttrFail::Prim(e)) => return Err(e),
    };
    let result = vm.instantiate(vm.class_array()?, 3)?;
    store_pointer(vm, 0, result, entry_name)?;
    store_pointer(vm, 1, result, attributes)?;
    Ok(result)
}

/// Reads a path argument and builds the whole-path `FaPath` (the
/// `faSetStPathOop` step every path-taking primitive starts with).
fn st_path_from(vm: &Interp, conv: &Converters, path_oop: Oop) -> PrimResult<FaPath> {
    // bytes_of enforces the C's isBytes check (PrimErrBadArgument otherwise).
    // The record takes its own copy of the path -- it has to, since it grows
    // a file name onto it -- so the borrow ends with this call, before
    // anything allocates.
    let mut fa = FaPath::new();
    fa.set_st_path(vm.bytes_of(path_oop)?, conv)
        .map_err(|s| os_error(vm, s))?;
    Ok(fa)
}

/// Validates a directory-handle ByteArray: the C's size check, then its
/// session-id check, then this port's registry key (see [`dir`]).
fn validated_session_key(vm: &Interp, handle: Oop) -> PrimResult<usize> {
    // The C runs stSizeOf/arrayValueOf on whatever object arrives; a
    // 2-slot pointers object would pass its size check and be reinterpreted
    // as struct bytes. bytes_of's isBytes rejection closes that hole.
    let bytes = vm.bytes_of(handle)?;
    let (session_id, key) = dir::decode_handle(bytes).ok_or(PrimErr::BadArgument)?;
    if session_id != dir::SESSION_ID {
        return Err(os_error(vm, FA_BAD_SESSION_ID));
    }
    Ok(key)
}

// ---------------------------------------------------------------------------
// The primitives
// ---------------------------------------------------------------------------

/// `chmod()` on the supplied path; answers nil.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveChangeMode(vm: &Interp, file_name: Oop, new_mode: sqInt) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    // `set_permissions` is `chmod` on Unix and passes the mode through
    // unmasked, so the truncation to mode_t is the C's.
    std::fs::set_permissions(
        fa.plat_fs_path(),
        std::fs::Permissions::from_mode(new_mode as u32),
    )
    .map_err(|e| os_error(vm, io_errno(&e)))?;
    vm.nil()
}

/// `chown()` on the supplied path; answers nil.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveChangeOwner(
    vm: &Interp,
    file_name: Oop,
    owner_id: sqInt,
    group_id: sqInt,
) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    std::os::unix::fs::chown(
        fa.plat_fs_path(),
        Some(owner_id as u32),
        Some(group_id as u32),
    )
    .map_err(|e| os_error(vm, io_errno(&e)))?;
    vm.nil()
}

/// `lchown()` -- the owner of the link itself; answers nil.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveSymlinkChangeOwner(
    vm: &Interp,
    file_name: Oop,
    owner_id: sqInt,
    group_id: sqInt,
) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    std::os::unix::fs::lchown(
        fa.plat_fs_path(),
        Some(owner_id as u32),
        Some(group_id as u32),
    )
    .map_err(|e| os_error(vm, io_errno(&e)))?;
    vm.nil()
}

/// One attribute of a file: 1-12 from `stat()`, 13-15 from `access()`,
/// 16 = is-symlink from `lstat()`.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileAttribute(
    vm: &Interp,
    file_name: Oop,
    attribute_number: sqInt,
) -> PrimResult<Oop> {
    if !(1..=16).contains(&attribute_number) {
        return Err(PrimErr::BadArgument);
    }
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    let cpath = fa.plat_cstring();
    if attribute_number <= 12 {
        let fs = stat::stat_path(&cpath).map_err(|s| os_error(vm, s))?;
        attr_oop(vm, stat::single_attribute_value(&fs, attribute_number))
    } else if attribute_number < 16 {
        let mode = match attribute_number {
            13 => libc::R_OK,
            14 => libc::W_OK,
            _ => libc::X_OK,
        };
        if stat::access_ok(&cpath, mode) {
            vm.true_object()
        } else {
            vm.false_object()
        }
    } else {
        let fs = stat::lstat_path(&cpath).map_err(|s| os_error(vm, s))?;
        if fs.is_symlink() {
            vm.true_object()
        } else {
            vm.false_object()
        }
    }
}

/// The attribute array/arrays selected by the mask: bit 0 = stat(), bit 1 =
/// access(), bit 2 = use lstat().
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileAttributes(vm: &Interp, file_name: Oop, attribute_mask: sqInt) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    match attribute_arrays(vm, &conv, &fa, attribute_mask) {
        Ok(oop) => Ok(oop),
        Err(AttrFail::Status(status)) => Err(os_error(vm, status)),
        Err(AttrFail::Prim(e)) => Err(e),
    }
}

/// `access(path, F_OK)`.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileExists(vm: &Interp, file_name: Oop) -> PrimResult<bool> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    Ok(stat::access_ok(&fa.plat_cstring(), libc::F_OK))
}

/// The eight `S_IF*` masks, in the C's fixed slot order.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveFileMasks(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let masks = vm.instantiate(vm.class_array()?, 8)?;
    for (i, mask) in stat::file_mask_values().into_iter().enumerate() {
        let oop = positive32(vm, mask)?;
        store_pointer(vm, i as sqInt, masks, oop)?;
    }
    Ok(masks)
}

/// Windows only; the Unix build always fails, code 1, as the C does.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveLogicalDrives(_vm: &Interp) -> PrimResult<Oop> {
    Err(PrimErr::GenericFailure)
}

/// Opens a directory: answers `[firstEntryName, attributes, handle]`, or nil
/// for an empty directory, or fails with `FA_CANT_OPEN_DIR`.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveOpendir(vm: &Interp, dir_name: Oop) -> PrimResult<Oop> {
    // The converters are resolved first, so the path's borrow spans nothing
    // but the record's own copy of it (see `st_path_from`).
    let conv = load_converters(vm);
    let mut fa = FaPath::new();
    fa.set_st_dir(vm.bytes_of(dir_name)?, &conv)
        .map_err(|s| os_error(vm, s))?;

    let session = match DirSession::open(fa, &conv) {
        Ok(Some(session)) => session,
        Ok(None) => return vm.nil(),
        Err(status) => return Err(os_error(vm, status)),
    };
    let result = process_directory(vm, &conv, &session.fa)?;
    let handle = vm.instantiate(vm.class_byte_array()?, dir::HANDLE_BYTES as sqInt)?;
    let key = dir::register(session);
    if let Err(e) = vm.write_bytes(handle, 0, &dir::encode_handle(key)) {
        dir::take(key);
        return Err(e);
    }
    store_pointer(vm, 2, result, handle)?;
    Ok(result)
}

/// The next entry of an open walk, or nil at the end.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveReaddir(vm: &Interp, dir_pointer: Oop) -> PrimResult<Oop> {
    let key = validated_session_key(vm, dir_pointer)?;
    let conv = load_converters(vm);
    let mut sessions = dir::lock();
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| os_error(vm, FA_BAD_SESSION_ID))?;
    match session.read(&conv) {
        Ok(ReadOutcome::NoMoreData) => vm.nil(),
        Ok(ReadOutcome::Entry) => process_directory(vm, &conv, &session.fa),
        Err(status) => Err(os_error(vm, status)),
    }
}

/// Rewinds the walk and answers the first entry.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveRewinddir(vm: &Interp, dir_pointer: Oop) -> PrimResult<Oop> {
    let key = validated_session_key(vm, dir_pointer)?;
    let conv = load_converters(vm);
    let mut sessions = dir::lock();
    let session = sessions
        .get_mut(&key)
        .ok_or_else(|| os_error(vm, FA_BAD_SESSION_ID))?;
    match session.rewind(&conv) {
        // The C does not special-case FA_NO_MORE_DATA here: rewinding a
        // directory that has become empty re-answers the *previous* entry
        // from the stale path buffer. Kept, quirk and all.
        Ok(_) => process_directory(vm, &conv, &session.fa),
        Err(status) => Err(os_error(vm, status)),
    }
}

/// Ends a walk; answers the handle.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveClosedir(vm: &Interp, dir_pointer: Oop) -> PrimResult<Oop> {
    let key = validated_session_key(vm, dir_pointer)?;
    // Removing first mirrors the C, which invalidates the session before
    // checking closedir's status; a close failure still ends the session.
    let mut session = dir::take(key).ok_or_else(|| os_error(vm, FA_BAD_SESSION_ID))?;
    session.close().map_err(|s| os_error(vm, s))?;
    Ok(dir_pointer)
}

/// `PATH_MAX` as the plugin was compiled with it.
#[pharo_primitive(accessor_depth = -1)]
fn primitivePathMax(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(FA_PATH_MAX as sqInt)
}

/// Platform-encoded name to image-encoded ByteArray.
#[pharo_primitive(accessor_depth = 0)]
fn primitivePlatToStPath(vm: &Interp, file_name: Oop) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let mut fa = FaPath::new();
    fa.set_plat_path(vm.bytes_of(file_name)?, &conv)
        .map_err(|s| os_error(vm, s))?;
    let result = vm.instantiate(vm.class_byte_array()?, fa.st_path().len() as sqInt)?;
    vm.write_bytes(result, 0, fa.st_path())?;
    Ok(result)
}

/// Image-encoded name to platform-encoded ByteArray.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveStToPlatPath(vm: &Interp, file_name: Oop) -> PrimResult<Oop> {
    let conv = load_converters(vm);
    let fa = st_path_from(vm, &conv, file_name)?;
    let result = vm.instantiate(vm.class_byte_array()?, fa.plat_path().len() as sqInt)?;
    vm.write_bytes(result, 0, fa.plat_path())?;
    Ok(result)
}

/// The plugin's own version, as the C's `primitiveVersionString` answers it.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveVersionString(vm: &Interp) -> PrimResult<&'static str> {
    vm.expect_argument_count(0)?;
    Ok("2.0.8")
}

// ---------------------------------------------------------------------------

/// Shared test scaffolding: a self-cleaning directory under the build tree
/// (kept out of /tmp so sandboxed test runs stay within the workspace).
#[cfg(test)]
pub(crate) mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(tag: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let mut path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target"));
            path.push("fa-plugin-tests");
            path.push(format!(
                "{tag}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("test scratch directory");
            TempDir(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
