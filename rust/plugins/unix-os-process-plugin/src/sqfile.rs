//! The FilePlugin's `SQFile` record, as this plugin sees it.
//!
//! The image hands SQFile records around as plain ByteArrays; this plugin
//! peeks inside them exactly as the C did, so the layout here must match
//! `plugins/FilePlugin/include/common/FilePlugin.h` byte for byte. A layout
//! test pins the sizes this code assumes.

use std::ffi::{c_char, c_int};
use std::mem::size_of;

use pharo_vm_plugin::{sqInt, Interp, Oop, PrimResult};

use crate::support::{first_indexable_field, this_session_id_wide};

/// `SESSIONIDENTIFIERTYPE` in the C: a plain `int`.
pub type SessionId = c_int;

/// Mirror of the C `SQFile` (non-ACORN branch): `sessionID` first, then the
/// stream pointer, then four flag chars. 24 bytes on 64-bit, 12 on 32-bit.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SQFile {
    pub session_id: SessionId,
    pub file: *mut libc::FILE,
    pub writable: c_char,
    /// 0 = uncommitted, 1 = read, 2 = write.
    pub last_op: c_char,
    pub last_char: c_char,
    pub is_stdio_stream: c_char,
}

impl SQFile {
    /// A record with every byte zero, padding included -- the shape a freshly
    /// instantiated ByteArray has, and the base the field setters write over.
    pub fn zeroed() -> Self {
        // SAFETY: all fields admit the zero bit pattern (null FILE* included).
        unsafe { std::mem::zeroed() }
    }

    /// The record the pipe and stdio primitives build: session, stream,
    /// writability; `lastOp` explicitly 0 as in the C.
    pub fn for_stream(file: *mut libc::FILE, session: SessionId, writable: bool) -> Self {
        let mut f = Self::zeroed();
        f.file = file;
        f.session_id = session;
        f.writable = c_char::from(writable);
        f.last_op = 0;
        f
    }

    /// The record as the bytes the image stores. Safe to expose because the
    /// struct starts from `zeroed()`, so padding bytes are initialized.
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: SQFile is repr(C), fully initialized (including padding by
        // construction), and the slice lives as long as the borrow.
        unsafe { std::slice::from_raw_parts((self as *const Self).cast::<u8>(), size_of::<Self>()) }
    }
}

/// Copies the SQFile record out of a ByteArray the image handed us.
///
/// Copying (rather than aliasing image memory) means later proxy calls cannot
/// invalidate the record mid-primitive.
pub fn read_record(vm: &Interp, oop: Oop) -> PrimResult<SQFile> {
    let p = first_indexable_field(vm, oop)?;
    // SAFETY: callers validate size via is_sq_file_object / byte_size_of
    // before reading; byte objects are only guaranteed 8-byte aligned at the
    // start, so read unaligned.
    Ok(unsafe { p.cast::<SQFile>().read_unaligned() })
}

/// `isSQFileObject:` -- the fourfold check every SQFile-taking primitive ran:
/// byte-indexable, exactly `sizeof(SQFile)` bytes, session identifier matching
/// this interpreter session, and not all zeros.
pub fn is_sq_file_object(vm: &Interp, oop: Oop) -> PrimResult<bool> {
    if !vm.is_bytes(oop)? {
        return Ok(false);
    }
    if vm.byte_size_of(oop)? != size_of::<SQFile>() as sqInt {
        return Ok(false);
    }
    let bytes = vm.bytes_of(oop)?;
    // sessionIdentifierFromSqFile: the int in the record's first four bytes,
    // compared (int-promoted) against the full-width session id.
    let mut sid = [0u8; size_of::<SessionId>()];
    sid.copy_from_slice(&bytes[..size_of::<SessionId>()]);
    if SessionId::from_ne_bytes(sid) as sqInt != this_session_id_wide(vm)? {
        return Ok(false);
    }
    // isNonNullSQFile: guards against the common failure mode of a record of
    // all zeros.
    Ok(bytes.iter().any(|&b| b != 0))
}

/// `fileDescriptorFrom:` -- the Unix descriptor inside a SQFile ByteArray, or
/// -1 when the object fails validation.
///
/// The C called `fileno()` on whatever pointer the record held; a null stream
/// answers -1 here instead of crashing (the one memory-safety addition).
pub fn file_descriptor_from(vm: &Interp, oop: Oop) -> PrimResult<c_int> {
    if !is_sq_file_object(vm, oop)? {
        return Ok(-1);
    }
    let record = read_record(vm, oop)?;
    if record.file.is_null() {
        return Ok(-1);
    }
    // SAFETY: the record passed validation; the stream pointer is the one the
    // FilePlugin (or this plugin) stored -- the same trust the C extended.
    Ok(unsafe { libc::fileno(record.file) })
}

/// `sessionIdentifierFrom:` -- decodes a session ByteArray, answering 0 (the
/// C's `null`) when the object is not a byte array of exactly the right size.
pub fn session_identifier_from(vm: &Interp, oop: Oop) -> PrimResult<SessionId> {
    if !vm.is_bytes(oop)? {
        return Ok(0);
    }
    let bytes = vm.bytes_of(oop)?;
    Ok(session_from_bytes(bytes).unwrap_or(0))
}

/// The pure decoding step: native-endian `int` from exactly-sized bytes.
pub fn session_from_bytes(bytes: &[u8]) -> Option<SessionId> {
    let arr: [u8; size_of::<SessionId>()] = bytes.try_into().ok()?;
    Some(SessionId::from_ne_bytes(arr))
}

// ---------------------------------------------------------------------------
// The C stdio globals (stdin/stdout/stderr as FILE*)
// ---------------------------------------------------------------------------

/// The platform's `FILE *stdin/stdout/stderr` globals, which the C reached by
/// name (`UnixOSProcessPlugin.c:497`, :533-535, :570, :2437, :2477, :2517).
///
/// `stdin` is a macro, not a symbol, and the two platforms expand it
/// differently: glibc to the `stdin` object itself, Darwin's `<stdio.h>` to
/// `__stdinp`. The C never had to know -- it wrote `stdin` and let the
/// preprocessor pick -- so this is one of the three places where Rust, having
/// no headers to lean on, needs the branch the C did not.
#[cfg(target_vendor = "apple")]
mod cstdio {
    extern "C" {
        #[link_name = "__stdinp"]
        pub static mut STDIN: *mut libc::FILE;
        #[link_name = "__stdoutp"]
        pub static mut STDOUT: *mut libc::FILE;
        #[link_name = "__stderrp"]
        pub static mut STDERR: *mut libc::FILE;
    }
}

#[cfg(not(target_vendor = "apple"))]
mod cstdio {
    extern "C" {
        #[link_name = "stdin"]
        pub static mut STDIN: *mut libc::FILE;
        #[link_name = "stdout"]
        pub static mut STDOUT: *mut libc::FILE;
        #[link_name = "stderr"]
        pub static mut STDERR: *mut libc::FILE;
    }
}

/// The process's `FILE* stdin`.
pub fn stdin_stream() -> *mut libc::FILE {
    // SAFETY: reading a C global the runtime initialized before main.
    unsafe { cstdio::STDIN }
}

/// The process's `FILE* stdout`.
pub fn stdout_stream() -> *mut libc::FILE {
    // SAFETY: as above.
    unsafe { cstdio::STDOUT }
}

/// The process's `FILE* stderr`.
pub fn stderr_stream() -> *mut libc::FILE {
    // SAFETY: as above.
    unsafe { cstdio::STDERR }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the layout this whole module assumes against FilePlugin.h:
    /// sessionID at offset 0, pointer-aligned struct, four trailing chars.
    #[test]
    fn sqfile_layout_matches_fileplugin_h() {
        let expected = if size_of::<usize>() == 8 { 24 } else { 12 };
        assert_eq!(size_of::<SQFile>(), expected);
        assert_eq!(std::mem::offset_of!(SQFile, session_id), 0);
        assert_eq!(
            std::mem::offset_of!(SQFile, file),
            size_of::<usize>() // int + padding up to pointer alignment
        );
    }

    #[test]
    fn session_round_trips_through_bytes() {
        let id: SessionId = 0x1234_5678;
        assert_eq!(session_from_bytes(&id.to_ne_bytes()), Some(id));
        assert_eq!(session_from_bytes(&[1, 2, 3]), None, "wrong size rejected");
    }

    #[test]
    fn stream_record_serializes_with_zero_padding() {
        let f = SQFile::for_stream(std::ptr::null_mut(), 7, true);
        let bytes = f.as_bytes();
        assert_eq!(bytes.len(), size_of::<SQFile>());
        assert_eq!(&bytes[..4], &7i32.to_ne_bytes());
        assert_eq!(session_from_bytes(&bytes[..4]), Some(7));
    }

    #[test]
    fn stdio_globals_resolve() {
        assert!(!stdin_stream().is_null());
        assert!(!stdout_stream().is_null());
        assert!(!stderr_stream().is_null());
    }
}
