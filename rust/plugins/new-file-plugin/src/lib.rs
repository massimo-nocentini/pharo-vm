//! `NewFilePlugin`, in Rust.
//!
//! The fd-based file plugin: open, read, write, seek, truncate and
//! memory-map by file descriptor, plus directory enumeration. It replaces
//! two layers of C on Unix:
//!
//! * the Slang-generated primitive shims (source of truth:
//!   `smalltalksrc/VMMaker/NewFilePlugin.class.st` -- the generated C is not
//!   checked into this repository), and
//! * the hand-written `plugins/NewFilePlugin/src/unix/UnixFile.c` behind
//!   them, ported in [`newfile`] with its exported C symbols intact.
//!
//! Windows keeps its C (`plugins/NewFilePlugin/src/win/Win32File.c`); this
//! crate refuses to compile anywhere but Unix rather than half-work.
//!
//! # The handle contract
//!
//! Every stateful primitive traffics in an `ExternalAddress`: a byte object
//! one machine word long holding a raw `NewFile_t*` / `NewDirectory_t*` (or,
//! for `primitiveDirectoryNext` and the memory-map primitive, a raw C
//! pointer into OS-owned storage). The word is written and read exactly as
//! the C's `pointerAtPointer:` did, so handles are byte-compatible with the
//! C plugin's -- an image could switch plugins mid-session and its open
//! handles would still parse. It also means the same trust model as C: a
//! forged or double-closed handle is undefined behaviour in both.

// Primitive names, exported C names and the crate's module name are all
// fixed by the image and by NewFile.h.
#![allow(non_snake_case)]

#[cfg(not(unix))]
compile_error!(
    "NewFilePlugin's Rust port covers Unix only; Windows keeps plugins/NewFilePlugin/src/win/Win32File.c"
);

pub mod newfile;

use core::ffi::c_int;
use std::ffi::c_void;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use newfile::{NewDirectory, NewFile};

pharo_plugin!("NewFilePlugin");

/// The Slang shims' `BytesPerWord`: the indexable size of the
/// `ExternalAddress` objects that carry handles.
const WORD_SIZE: usize = core::mem::size_of::<usize>();

// ---------------------------------------------------------------------------
// Proxy plumbing the safe API does not cover
// ---------------------------------------------------------------------------

/// Calls a raw proxy entry (see `pharo-vm-plugin/src/proxy.rs`), failing with
/// `Unsupported` if this VM did not supply it.
macro_rules! proxy_call {
    ($vm:expr, $field:ident ( $($arg:expr),* )) => {{
        // SAFETY: as_raw is the proxy table the VM handed setInterpreter,
        // valid for the process lifetime; the signature is proxy.rs's.
        let f = unsafe { &*$vm.as_raw() }.$field.ok_or(PrimErr::Unsupported)?;
        unsafe { f($($arg),*) }
    }};
}

/// `positive32BitValueOf: (stackValue: offset)`, checking the failure flag
/// the conversion reports through.
fn positive_32(vm: &Interp, offset: sqInt) -> PrimResult<u32> {
    let oop = vm.stack_value(offset)?;
    let v = proxy_call!(vm, positive32BitValueOf(oop.0));
    vm.check_failed()?;
    Ok(v)
}

/// `positive64BitValueOf: (stackValue: offset)`, likewise.
///
/// The proxy entry answers `c_ulong`; this crate only builds on the 64-bit
/// Unix targets the VM ships on, where that *is* `u64` (a 32-bit target
/// would fail to compile here, which beats truncating file offsets).
fn positive_64(vm: &Interp, offset: sqInt) -> PrimResult<u64> {
    let oop = vm.stack_value(offset)?;
    let v: u64 = proxy_call!(vm, positive64BitValueOf(oop.0));
    vm.check_failed()?;
    Ok(v)
}

/// `signed64BitValueOf: (stackValue: offset)`, likewise (`c_long` = `i64`).
fn signed_64(vm: &Interp, offset: sqInt) -> PrimResult<i64> {
    let oop = vm.stack_value(offset)?;
    let v: i64 = proxy_call!(vm, signed64BitValueOf(oop.0));
    vm.check_failed()?;
    Ok(v)
}

/// `positiveMachineIntegerValueOf: (stackValue: offset)`, likewise.
fn positive_machine(vm: &Interp, offset: sqInt) -> PrimResult<usize> {
    let oop = vm.stack_value(offset)?;
    let v = proxy_call!(vm, positiveMachineIntegerValueOf(oop.0));
    vm.check_failed()?;
    Ok(v)
}

/// A fresh `ExternalAddress` of one machine word, zeroed -- what every
/// handle-answering shim instantiates.
fn new_external_address(vm: &Interp) -> PrimResult<Oop> {
    let class = Oop(proxy_call!(vm, classExternalAddress()));
    vm.instantiate(class, WORD_SIZE as sqInt)
}

/// Reads the machine word an `ExternalAddress` holds -- the C's
/// `pointerAtPointer: (firstIndexableField: oop)`.
///
/// The C read a word from whatever `firstIndexableField` answered; requiring
/// a byte object of at least word size makes the read memory-safe without
/// changing what any well-formed handle answers.
fn handle_word(vm: &Interp, oop: Oop) -> PrimResult<usize> {
    let bytes = vm.bytes_of(oop)?;
    let word = bytes.get(..WORD_SIZE).ok_or(PrimErr::BadArgument)?;
    Ok(usize::from_ne_bytes(
        word.try_into().map_err(|_| PrimErr::BadArgument)?,
    ))
}

/// Stores a machine word into an `ExternalAddress` -- the C's
/// `pointerAtPointer:put:`.
fn put_handle_word(vm: &Interp, oop: Oop, value: usize) -> PrimResult<()> {
    vm.write_bytes(oop, 0, &value.to_ne_bytes())
}

/// The `NewFile_t*` handle `offset` slots down the stack.
fn file_handle(vm: &Interp, offset: sqInt) -> PrimResult<*mut NewFile> {
    let oop = vm.stack_value(offset)?;
    Ok(handle_word(vm, oop)? as *mut NewFile)
}

/// The `NewDirectory_t*` handle `offset` slots down the stack.
fn directory_handle(vm: &Interp, offset: sqInt) -> PrimResult<*mut NewDirectory> {
    let oop = vm.stack_value(offset)?;
    Ok(handle_word(vm, oop)? as *mut NewDirectory)
}

/// The path argument: any byte object, taken as `(pointer, size)`.
///
/// Fails with the C's plain `primitiveFail` when it is not bytes. Handed to
/// the C where it lies, which is what the C shim did -- the calls that take
/// one (`mkdir`, `rmdir`, `unlink`) do not allocate, so nothing can move the
/// object while they run.
fn path_ref(vm: &Interp, offset: sqInt) -> PrimResult<&[u8]> {
    let oop = vm.stack_value(offset)?;
    if !vm.is_bytes(oop)? {
        return Err(PrimErr::GenericFailure);
    }
    vm.bytes_of(oop)
}

/// The path argument as an owned copy, for the two primitives that
/// instantiate their answer *before* opening -- the shim's order, which puts
/// an allocation between reading the path and using it.
fn path_arg(vm: &Interp, offset: sqInt) -> PrimResult<Vec<u8>> {
    Ok(path_ref(vm, offset)?.to_vec())
}

/// Base address of a read/write buffer object, with the span
/// `[offset, offset + size)` checked against the object.
///
/// The C passed `firstIndexableField(bufferOop)` straight to `read(2)` /
/// `write(2)` with no type or bounds check at all; requiring a word- or
/// byte-indexable object, a span inside it, and (for the destination of a
/// read) mutability removes that undefined behaviour without changing any
/// in-bounds call.
fn buffer_base(
    vm: &Interp,
    oop: Oop,
    offset: usize,
    size: usize,
    written_to: bool,
) -> PrimResult<*mut c_void> {
    if written_to && proxy_call!(vm, isOopImmutable(oop.0)) != 0 {
        return Err(PrimErr::NoModification);
    }
    if !vm.is_words_or_bytes(oop)? {
        return Err(PrimErr::BadArgument);
    }
    let len = usize::try_from(vm.byte_size_of(oop)?)?;
    let end = offset.checked_add(size).ok_or(PrimErr::BadIndex)?;
    if end > len {
        return Err(PrimErr::BadIndex);
    }
    let base = proxy_call!(vm, firstIndexableField(oop.0));
    if base.is_null() {
        return Err(PrimErr::BadArgument);
    }
    Ok(base)
}

// ---------------------------------------------------------------------------
// Directory primitives
// ---------------------------------------------------------------------------

/// `mkdir(path)`. Answers whether it succeeded.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryCreate(vm: &Interp) -> PrimResult<bool> {
    let path = path_ref(vm, 0)?;
    // SAFETY: the object's own bytes, exactly path.len() of them, and the
    // call cannot allocate.
    Ok(unsafe { newfile::NewDirectory_create(path.as_ptr().cast(), path.len()) })
}

/// `rmdir(path)`. Answers whether it succeeded.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryRemoveEmpty(vm: &Interp) -> PrimResult<bool> {
    let path = path_ref(vm, 0)?;
    // SAFETY: the object's own bytes, exactly path.len() of them, and the
    // call cannot allocate.
    Ok(unsafe { newfile::NewDirectory_removeEmpty(path.as_ptr().cast(), path.len()) })
}

/// Opens a directory, answering a fresh `ExternalAddress` holding the
/// `NewDirectory_t*` -- NULL inside when the open failed, as in the C.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryOpen(vm: &Interp) -> PrimResult<Oop> {
    let path = path_arg(vm, 0)?;
    // The shim instantiates the answer before opening; keep that order.
    let directory_oop = new_external_address(vm)?;
    // SAFETY: path is a live local buffer of exactly path.len() bytes.
    let handle = unsafe { newfile::NewDirectory_open(path.as_ptr().cast(), path.len()) };
    put_handle_word(vm, directory_oop, handle as usize)?;
    Ok(directory_oop)
}

/// The next entry's name as a fresh `ExternalAddress` holding a `char*` into
/// `readdir`'s storage -- NULL inside at the end of the enumeration.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDirectoryNext(vm: &Interp) -> PrimResult<Oop> {
    // The shim instantiates the answer before touching the handle.
    let string_oop = new_external_address(vm)?;
    let directory = directory_handle(vm, 0)?;
    // SAFETY: same trust as the C -- the word the image handed back is a
    // handle this plugin issued (NULL answers NULL).
    let name = unsafe { newfile::NewDirectory_next(directory) };
    put_handle_word(vm, string_oop, name as usize)?;
    Ok(string_oop)
}

/// Rewinds the enumeration and answers the receiver.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDirectoryRewind(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?; // the shim pops exactly the directory
    let directory = directory_handle(vm, 0)?;
    // SAFETY: same trust as the C; the result is discarded, as in the shim.
    unsafe { newfile::NewDirectory_rewind(directory) };
    Ok(())
}

/// Closes the directory and answers the receiver.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDirectoryClose(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?; // the shim pops exactly the directory
    let directory = directory_handle(vm, 0)?;
    // SAFETY: same trust as the C -- close frees the handle, and calling it
    // twice on the same handle is the caller's undefined behaviour in both.
    unsafe { newfile::NewDirectory_close(directory) };
    Ok(())
}

// ---------------------------------------------------------------------------
// File primitives
// ---------------------------------------------------------------------------

/// `unlink(path)`. Answers whether it succeeded.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveDeleteFile(vm: &Interp) -> PrimResult<bool> {
    let path = path_ref(vm, 0)?;
    // SAFETY: the object's own bytes, exactly path.len() of them, and the
    // call cannot allocate.
    Ok(unsafe { newfile::NewFile_deleteFile(path.as_ptr().cast(), path.len()) })
}

/// `open: path mode: mode creationDisposition: disposition flags: flags`,
/// answering a fresh `ExternalAddress` holding the `NewFile_t*` -- NULL
/// inside when the open failed.
///
/// Divergence: the generated C converted the three integers, ignored the
/// failure flag, and opened the file anyway with whatever the failed
/// conversions left behind -- observable as a stray file on a *failing*
/// primitive. A conversion failure here fails before any side effect.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileOpen(vm: &Interp) -> PrimResult<Oop> {
    let flags = positive_32(vm, 0)?;
    let creation_disposition = positive_32(vm, 1)?;
    let mode = positive_32(vm, 2)?;
    let path = path_arg(vm, 3)?;
    // The shim instantiates the answer before opening; keep that order.
    let file_oop = new_external_address(vm)?;
    // SAFETY: path is a live local buffer of exactly path.len() bytes. The
    // u32 -> int casts are the C's unsigned-to-enum conversions.
    let handle = unsafe {
        newfile::NewFile_open(
            path.as_ptr().cast(),
            path.len(),
            mode as c_int,
            creation_disposition as c_int,
            flags as c_int,
        )
    };
    put_handle_word(vm, file_oop, handle as usize)?;
    Ok(file_oop)
}

/// Closes the file and answers the receiver.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileClose(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?; // the shim pops exactly the file
    let file = file_handle(vm, 0)?;
    // SAFETY: same trust as the C -- close frees the handle, and calling it
    // twice on the same handle is the caller's undefined behaviour in both.
    unsafe { newfile::NewFile_close(file) };
    Ok(())
}

/// The file's size in bytes, or -1 on error.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileGetSize(vm: &Interp) -> PrimResult<isize> {
    let file = file_handle(vm, 0)?;
    // SAFETY: same trust as the C (NULL answers -1).
    Ok(unsafe { newfile::NewFile_getSize(file) } as isize)
}

/// `seek: offset mode: mode`, answering the receiver. Errors are silently
/// ignored, as in the C.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileSeek(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(3)?; // the shim pops mode, offset and file
    let seek_mode = positive_32(vm, 0)?;
    let seek_offset = signed_64(vm, 1)?;
    let file = file_handle(vm, 2)?;
    // SAFETY: same trust as the C (NULL is a no-op).
    unsafe { newfile::NewFile_seek(file, seek_offset, seek_mode as c_int) };
    Ok(())
}

/// The current file position (0 for a NULL handle, as in the C).
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileTell(vm: &Interp) -> PrimResult<isize> {
    let file = file_handle(vm, 0)?;
    // SAFETY: same trust as the C.
    Ok(unsafe { newfile::NewFile_tell(file) } as isize)
}

/// `ftruncate` to the given size. Answers whether it succeeded.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileTruncate(vm: &Interp) -> PrimResult<bool> {
    let new_file_size = positive_64(vm, 0)?;
    let file = file_handle(vm, 1)?;
    // SAFETY: same trust as the C (NULL answers false).
    Ok(unsafe { newfile::NewFile_truncate(file, new_file_size) })
}

/// `read: file into: buffer at: bufferOffset size: readSize`, answering the
/// byte count read (or the syscall's -1).
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileReadInto(vm: &Interp) -> PrimResult<isize> {
    let read_size = positive_machine(vm, 0)?;
    let buffer_offset = positive_machine(vm, 1)?;
    let buffer_oop = vm.stack_value(2)?;
    let file = file_handle(vm, 3)?;
    let base = buffer_base(vm, buffer_oop, buffer_offset, read_size, true)?;
    // SAFETY: buffer_base checked that [buffer_offset, +read_size) lies
    // inside a mutable indexable object, and nothing allocates before the
    // call, so the pointer stays valid.
    let n = unsafe { newfile::NewFile_read(file, base, buffer_offset, read_size) };
    Ok(n as isize)
}

/// As [`primitiveFileReadInto`], but `pread(2)` at a file offset, leaving
/// the file position alone.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileReadIntoAtFileOffset(vm: &Interp) -> PrimResult<isize> {
    let file_offset = positive_64(vm, 0)?;
    let read_size = positive_machine(vm, 1)?;
    let buffer_offset = positive_machine(vm, 2)?;
    let buffer_oop = vm.stack_value(3)?;
    let file = file_handle(vm, 4)?;
    let base = buffer_base(vm, buffer_oop, buffer_offset, read_size, true)?;
    // SAFETY: as in primitiveFileReadInto.
    let n = unsafe { newfile::NewFile_readAtOffset(file, base, buffer_offset, read_size, file_offset) };
    Ok(n as isize)
}

/// `write: file from: buffer at: bufferOffset size: writeSize`, answering
/// the byte count written (or the syscall's -1).
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileWriteFrom(vm: &Interp) -> PrimResult<isize> {
    let write_size = positive_machine(vm, 0)?;
    let buffer_offset = positive_machine(vm, 1)?;
    let buffer_oop = vm.stack_value(2)?;
    let file = file_handle(vm, 3)?;
    let base = buffer_base(vm, buffer_oop, buffer_offset, write_size, false)?;
    // SAFETY: buffer_base checked that [buffer_offset, +write_size) lies
    // inside an indexable object, and nothing allocates before the call.
    let n = unsafe { newfile::NewFile_write(file, base, buffer_offset, write_size) };
    Ok(n as isize)
}

/// As [`primitiveFileWriteFrom`], but `pwrite(2)` at a file offset, leaving
/// the file position alone.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileWriteFromAtFileOffset(vm: &Interp) -> PrimResult<isize> {
    let file_offset = positive_64(vm, 0)?;
    let write_size = positive_machine(vm, 1)?;
    let buffer_offset = positive_machine(vm, 2)?;
    let buffer_oop = vm.stack_value(3)?;
    let file = file_handle(vm, 4)?;
    let base = buffer_base(vm, buffer_oop, buffer_offset, write_size, false)?;
    // SAFETY: as in primitiveFileWriteFrom.
    let n = unsafe { newfile::NewFile_writeAtOffset(file, base, buffer_offset, write_size, file_offset) };
    Ok(n as isize)
}

/// Answers true: the image probes this to decide the plugin is installed.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveIsAvailable(_vm: &Interp) -> PrimResult<bool> {
    Ok(true)
}

/// Maps the file into memory, answering a fresh `ExternalAddress` holding
/// the mapping's address -- NULL inside on failure.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveMemoryMapWithProtection(vm: &Interp) -> PrimResult<Oop> {
    let protection = positive_32(vm, 0)?;
    // The shim instantiates the answer before touching the handle.
    let pointer_oop = new_external_address(vm)?;
    let file = file_handle(vm, 1)?;
    // SAFETY: same trust as the C (NULL answers NULL).
    let address = unsafe { newfile::NewFile_memoryMap(file, protection as c_int) };
    put_handle_word(vm, pointer_oop, address as usize)?;
    Ok(pointer_oop)
}

/// Drops one reference to the file's memory mapping and answers the
/// receiver. See [`newfile::NewFile_memoryUnmap`] for the faithful oddity in
/// its counting.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveMemoryUnmap(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?; // the shim pops exactly the file
    let file = file_handle(vm, 0)?;
    // SAFETY: same trust as the C.
    unsafe { newfile::NewFile_memoryUnmap(file) };
    Ok(())
}
