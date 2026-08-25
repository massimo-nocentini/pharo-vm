//! `FilePlugin`, in Rust: the VM's core file I/O plugin.
//!
//! Replaces the Slang-generated primitive layer (`FilePlugin.class.st` ->
//! `FilePlugin.c`) plus the hand-written unix support files
//! `sqFilePluginBasicPrims.c`, `sqUnixFile.c`, `sqUnixCharConv.c` and
//! `fileUtils.c`, behind the same twenty-six primitives and the same
//! exported C API (other plugins link against `sqFileStdioHandlesInto`,
//! `sq2uxPath`, `ux2sqPath`, ...).
//!
//! The primitive bodies deliberately mirror the generated C statement for
//! statement -- same stack reads in the same order, same failure codes, same
//! explicit `pop`/`methodReturn*` stack effects -- because the generated C
//! *is* the contract the image was written against. They therefore bypass
//! the SDK's typed-argument conveniences and return [`Done`], and drive the
//! stack through the raw proxy exactly as the C did.

#![allow(non_snake_case)] // exported names are fixed by the image and FilePlugin.h
#![allow(non_upper_case_globals)] // the sqUnixCharConv.h encoding globals

pub mod charconv;
mod cstdio;
pub mod dir;
mod errno;
pub mod sqfile;
mod vmcalls;

// Everything C-linkage also at the crate root, mirroring the flat namespace
// the C plugin exported (and giving tests the same view its consumers get).
pub use charconv::*;
pub use dir::*;
pub use sqfile::*;

use libc::{c_char, c_int};
use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, sqInt, Interp, IntoReturn, PrimErr, PrimResult,
};

pharo_plugin!("FilePlugin", init = init, shutdown = shutdown);

/// `initialiseModule` in the C answers `sqFileInit()`.
fn init() -> bool {
    sqfile::sqFileInit() == 1
}

/// `shutdownModule` in the C answers `sqFileShutdown()`.
fn shutdown() -> bool {
    sqfile::sqFileShutdown() == 1
}

/// The answer of a primitive that has already produced its stack effect.
///
/// The C primitives pop and push explicitly; letting the SDK answer the
/// receiver on top of that would double-pop. Returning `Done` makes the
/// wrapper's answer step a no-op while keeping its panic containment.
struct Done;

impl IntoReturn for Done {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

/// Calls a raw proxy entry, failing with `Unsupported` if this VM lacks it.
///
/// The `allow(unused_unsafe)` is for nested `px!` invocations, whose inner
/// expansion sits textually inside the outer one's unsafe block.
macro_rules! px {
    ($vm:expr, $field:ident ( $($arg:expr),* $(,)? )) => {{
        // SAFETY: as_raw() is the VM's own process-lifetime proxy table.
        #[allow(unused_unsafe)]
        let f = unsafe { (*$vm.as_raw()).$field }.ok_or(PrimErr::Unsupported)?;
        // SAFETY: the signature is the one fixed by the published proxy ABI.
        #[allow(unused_unsafe)]
        let answer = unsafe { f($($arg),*) };
        answer
    }};
}

/// `interpreterProxy failed`, as a bool.
macro_rules! px_failed {
    ($vm:expr) => {
        px!($vm, failed()) != 0
    };
}

const FILE_RECORD_SIZE: usize = core::mem::size_of::<SQFile>();

// ---------------------------------------------------------------------------
// Exported non-primitive entry points (generated FilePlugin.c exports these
// for the VM and other plugins)
// ---------------------------------------------------------------------------

/// `FilePlugin>>#fileRecordSize`: bytes an image-side file record occupies.
#[no_mangle]
pub extern "C" fn fileRecordSize() -> usize {
    FILE_RECORD_SIZE
}

/// `FilePlugin>>#fileValueOf:`: the record inside a file ByteArray, or null
/// (with the failure flag set) when the object is not a file record.
///
/// # Safety
/// Must run on the interpreter thread with a live proxy; `object_pointer`
/// is any oop.
#[no_mangle]
pub unsafe extern "C" fn fileValueOf(object_pointer: sqInt) -> *mut SQFile {
    let Some(p) = vmcalls::proxy() else {
        return core::ptr::null_mut();
    };
    let (Some(is_bytes), Some(byte_size_of), Some(first_field), Some(primitive_fail)) = (
        p.isBytes,
        p.byteSizeOf,
        p.firstIndexableField,
        p.primitiveFail,
    ) else {
        return core::ptr::null_mut();
    };
    // SAFETY: proxy entries supplied by the VM.
    unsafe {
        if !(is_bytes(object_pointer) != 0
            && byte_size_of(object_pointer) == FILE_RECORD_SIZE as sqInt)
        {
            primitive_fail();
            return core::ptr::null_mut();
        }
        first_field(object_pointer).cast()
    }
}

/// `FilePlugin>>#fileOpenName:size:write:`: allocates the record ByteArray,
/// opens the file into it, answers the oop (even on failure, as the C did --
/// the caller checks the failure flag).
///
/// # Safety
/// Interpreter thread with a live proxy; `name_index` points at `name_size`
/// readable bytes.
#[no_mangle]
pub unsafe extern "C" fn fileOpenNamesizewrite(
    name_index: *mut c_char,
    name_size: sqInt,
    write_flag: sqInt,
) -> sqInt {
    let Some(p) = vmcalls::proxy() else { return 0 };
    let (Some(instantiate), Some(class_byte_array), Some(failed)) =
        (p.instantiateClassindexableSize, p.classByteArray, p.failed)
    else {
        return 0;
    };
    // SAFETY: proxy entries supplied by the VM; fileValueOf validates the
    // fresh oop; sqFileOpen's contract is met by the caller's name bytes.
    unsafe {
        let file_oop = instantiate(class_byte_array(), FILE_RECORD_SIZE as sqInt);
        let file = fileValueOf(file_oop);
        if failed() == 0 {
            sqfile::sqFileOpen(file, name_index, name_size, write_flag);
        }
        file_oop
    }
}

/// `FilePlugin>>#fileOpenNewName:size:`: as [`fileOpenNamesizewrite`] but
/// create-only, failing with `PrimErrInappropriate` when the file exists.
///
/// # Safety
/// As [`fileOpenNamesizewrite`].
#[no_mangle]
pub unsafe extern "C" fn fileOpenNewNamesize(name_index: *mut c_char, name_size: sqInt) -> sqInt {
    let Some(p) = vmcalls::proxy() else { return 0 };
    let (Some(instantiate), Some(class_byte_array), Some(failed), Some(fail_for)) = (
        p.instantiateClassindexableSize,
        p.classByteArray,
        p.failed,
        p.primitiveFailFor,
    ) else {
        return 0;
    };
    // SAFETY: as in `fileOpenNamesizewrite`.
    unsafe {
        let file_oop = instantiate(class_byte_array(), FILE_RECORD_SIZE as sqInt);
        let file = fileValueOf(file_oop);
        if failed() == 0 {
            let mut exists: c_int = 0;
            sqfile::sqFileOpenNew(file, name_index, name_size, &mut exists);
            if failed() != 0 && exists != 0 {
                fail_for(PrimErr::Inappropriate.code());
            }
        }
        file_oop
    }
}

/// `FilePlugin>>#setMacFile:Type:AndCreator:`: exported for the VM's image
/// saving; a successful no-op on unix, as the C's `dir_SetMacFileTypeAndCreator`.
///
/// # Safety
/// `file_name` is NUL-terminated; nothing is written through any pointer.
#[no_mangle]
pub unsafe extern "C" fn setMacFileTypeAndCreator(
    file_name: *mut c_char,
    type_string: *mut c_char,
    creator_string: *mut c_char,
) -> sqInt {
    // SAFETY: strlen only reads to the caller-guaranteed NUL; the callee is
    // a no-op that dereferences nothing.
    unsafe {
        let len = libc::strlen(file_name) as sqInt;
        dir::dir_SetMacFileTypeAndCreator(file_name, len, type_string, creator_string)
    }
}

// ---------------------------------------------------------------------------
// File primitives
// ---------------------------------------------------------------------------

/// `fileValueOf(stackValue(offset))`, the opening move of most primitives.
fn file_on_stack(vm: &Interp, offset: sqInt) -> PrimResult<*mut SQFile> {
    let oop = px!(vm, stackValue(offset));
    // SAFETY: interpreter thread, proxy live (we are inside a primitive).
    Ok(unsafe { fileValueOf(oop) })
}

#[pharo_primitive(accessor_depth = 2)]
fn primitiveFileOpen(vm: &Interp) -> PrimResult<Done> {
    let write_flag = px!(vm, booleanValueOf(px!(vm, stackValue(0))));
    let name_pointer = px!(vm, stackValue(1));
    if px!(vm, isBytes(name_pointer)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let name_size = px!(vm, byteSizeOf(name_pointer));
    // Copy the name out of image memory before fileOpenNamesizewrite
    // allocates the record ByteArray. (The C passed the image pointer across
    // the allocation; Spur never moves objects on allocation, but the copy
    // removes the question at no observable cost.)
    let name_ptr = px!(vm, firstIndexableField(name_pointer));
    let mut name =
        // SAFETY: a bytes object holds byteSizeOf readable bytes.
        unsafe { core::slice::from_raw_parts(name_ptr.cast::<u8>(), name_size.max(0) as usize) }
            .to_vec();
    let file_oop =
        // SAFETY: the copied name fulfils the callee's contract.
        unsafe { fileOpenNamesizewrite(name.as_mut_ptr().cast(), name_size, write_flag) };
    if !px_failed!(vm) {
        px!(vm, methodReturnValue(file_oop));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 2)]
fn primitiveFileOpenNew(vm: &Interp) -> PrimResult<Done> {
    let name_pointer = px!(vm, stackValue(0));
    if px!(vm, isBytes(name_pointer)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let name_size = px!(vm, byteSizeOf(name_pointer));
    let name_ptr = px!(vm, firstIndexableField(name_pointer));
    // Copied before allocation, as in primitiveFileOpen.
    let mut name =
        // SAFETY: a bytes object holds byteSizeOf readable bytes.
        unsafe { core::slice::from_raw_parts(name_ptr.cast::<u8>(), name_size.max(0) as usize) }
            .to_vec();
    // SAFETY: the copied name fulfils the callee's contract.
    let file_oop = unsafe { fileOpenNewNamesize(name.as_mut_ptr().cast(), name_size) };
    if !px_failed!(vm) {
        px!(vm, methodReturnValue(file_oop));
    }
    Ok(Done)
}

/// Reads the machine address out of a pointer-sized ByteArray
/// (`FilePlugin>>#pointerFrom:`). Null with the failure flag set otherwise.
fn pointer_from(vm: &Interp, pointer_byte_array: sqInt) -> PrimResult<*mut libc::c_void> {
    let class_ok = px!(
        vm,
        isKindOf(pointer_byte_array, c"ByteArray".as_ptr().cast_mut())
    ) != 0;
    if !(class_ok
        && px!(vm, stSizeOf(pointer_byte_array))
            == core::mem::size_of::<*mut libc::c_void>() as sqInt)
    {
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(core::ptr::null_mut());
    }
    let ptr = px!(vm, arrayValueOf(pointer_byte_array));
    if px_failed!(vm) {
        return Ok(core::ptr::null_mut());
    }
    // SAFETY: the object holds exactly pointer-size bytes (checked above);
    // read unaligned, as the C's byte-by-byte union copy.
    Ok(unsafe { ptr.cast::<*mut libc::c_void>().read_unaligned() })
}

/// Allocates a record ByteArray and connects it to a `FILE *`
/// (`FilePlugin>>#connectToFile:write:`).
fn connect_to_file(vm: &Interp, cfile: *mut libc::c_void, write_flag: sqInt) -> PrimResult<sqInt> {
    let file_oop = px!(
        vm,
        instantiateClassindexableSize(px!(vm, classByteArray()), FILE_RECORD_SIZE as sqInt)
    );
    // SAFETY: interpreter thread; validated oop.
    let file = unsafe { fileValueOf(file_oop) };
    if !px_failed!(vm) {
        // SAFETY: `file` is a valid record (fileValueOf succeeded, or the
        // failed() check above skips this); `cfile` is the caller's FILE*.
        unsafe { sqfile::sqConnectToFile(file, cfile, write_flag) };
    }
    Ok(file_oop)
}

/// As [`connect_to_file`], from a file descriptor
/// (`FilePlugin>>#connectToFd:write:`).
fn connect_to_fd(vm: &Interp, fd: c_int, write_flag: sqInt) -> PrimResult<sqInt> {
    let file_oop = px!(
        vm,
        instantiateClassindexableSize(px!(vm, classByteArray()), FILE_RECORD_SIZE as sqInt)
    );
    // SAFETY: as in `connect_to_file`.
    let file = unsafe { fileValueOf(file_oop) };
    if !px_failed!(vm) {
        // SAFETY: as in `connect_to_file`.
        unsafe { sqfile::sqConnectToFileDescriptor(file, fd, write_flag) };
    }
    Ok(file_oop)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveConnectToFile(vm: &Interp) -> PrimResult<Done> {
    let write_flag = px!(vm, booleanValueOf(px!(vm, stackValue(0))));
    let cfile_oop = px!(vm, stackValue(1));
    let cfile = pointer_from(vm, cfile_oop)?;
    if px_failed!(vm) {
        // Ensure that the appropriate failure code has been set.
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let file_oop = connect_to_file(vm, cfile, write_flag)?;
    if !px_failed!(vm) {
        px!(vm, methodReturnValue(file_oop));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveConnectToFileDescriptor(vm: &Interp) -> PrimResult<Done> {
    let write_flag = px!(vm, booleanValueOf(px!(vm, stackValue(0))));
    let fd_pointer = px!(vm, stackValue(1));
    if px!(vm, isIntegerObject(fd_pointer)) == 0 {
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let fd = px!(vm, integerValueOf(fd_pointer)) as c_int;
    if px_failed!(vm) {
        // Ensure that the appropriate failure code has been set.
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let file_oop = connect_to_fd(vm, fd, write_flag)?;
    if !px_failed!(vm) {
        px!(vm, methodReturnValue(file_oop));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileAtEnd(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    let mut at_end: sqInt = 0;
    if !px_failed!(vm) {
        // SAFETY: `file` came from fileValueOf and the failure flag is clear.
        at_end = unsafe { sqfile::sqFileAtEnd(file) };
    }
    if !px_failed!(vm) {
        px!(vm, methodReturnBool(at_end));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileClose(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::sqFileClose(file) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(1)); // pop file; leave rcvr on stack
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileDelete(vm: &Interp) -> PrimResult<Done> {
    let name_pointer = px!(vm, stackValue(0));
    if px!(vm, isBytes(name_pointer)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let name_index = px!(vm, firstIndexableField(name_pointer));
    let name_size = px!(vm, byteSizeOf(name_pointer));
    // SAFETY: a bytes object holds byteSizeOf readable bytes; no allocation
    // happens before the callee copies them.
    unsafe { sqfile::sqFileDeleteNameSize(name_index.cast(), name_size) };
    if !px_failed!(vm) {
        px!(vm, pop(1));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileDescriptorType(vm: &Interp) -> PrimResult<Done> {
    let fd_pointer = px!(vm, stackValue(0));
    if px!(vm, isIntegerObject(fd_pointer)) == 0 {
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let fd = px!(vm, integerValueOf(fd_pointer)) as c_int;
    if px_failed!(vm) {
        // Ensure that the appropriate failure code has been set.
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let file_type = sqfile::sqFileDescriptorType(fd);
    // Unconditionally, as the Slang: no failed-check before answering.
    px!(vm, methodReturnInteger(file_type));
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileFlush(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::sqFileFlush(file) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(1));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileGetPosition(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    let mut position: fileOffset_t = 0;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        position = unsafe { sqfile::sqFileGetPosition(file) };
    }
    if !px_failed!(vm) {
        let oop = px!(vm, positive64BitIntegerFor(position as libc::c_ulong));
        px!(vm, methodReturnValue(oop));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileRead(vm: &Interp) -> PrimResult<Done> {
    let count = px!(vm, positiveMachineIntegerValueOf(px!(vm, stackValue(0))));
    let start_index = px!(vm, positiveMachineIntegerValueOf(px!(vm, stackValue(1))));
    let array = px!(vm, stackValue(2));
    let file = file_on_stack(vm, 3)?;
    // Buffer can be any indexable words or bytes object except CompiledMethod.
    if px_failed!(vm) || px!(vm, isWordsOrBytes(array)) == 0 {
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let slot_size = px!(vm, slotSizeOf(array)) as usize;
    // The index arithmetic wraps exactly as the C's size_t arithmetic did.
    if start_index >= 1 && start_index.wrapping_add(count).wrapping_sub(1) <= slot_size {
        let element_size = (px!(vm, byteSizeOf(array)) as usize)
            .checked_div(slot_size)
            .unwrap_or(1);
        let dst = px!(vm, firstIndexableField(array));
        // SAFETY: `dst` addresses the array's indexable bytes and the bounds
        // were checked against slotSize just above; `file` is checked by the
        // callee.
        let bytes_read = unsafe {
            sqfile::sqFileReadIntoAt(
                file,
                count.wrapping_mul(element_size),
                dst.cast(),
                (start_index - 1).wrapping_mul(element_size),
            )
        };
        if !px_failed!(vm) {
            let oop = px!(vm, integerObjectOf((bytes_read / element_size) as sqInt));
            px!(vm, methodReturnValue(oop));
        }
    } else {
        px!(vm, primitiveFailFor(PrimErr::BadIndex.code()));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileRename(vm: &Interp) -> PrimResult<Done> {
    let new_name_pointer = px!(vm, stackValue(0));
    let old_name_pointer = px!(vm, stackValue(1));
    if !(px!(vm, isBytes(new_name_pointer)) != 0 && px!(vm, isBytes(old_name_pointer)) != 0) {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let new_name_index = px!(vm, firstIndexableField(new_name_pointer));
    let new_name_size = px!(vm, byteSizeOf(new_name_pointer));
    let old_name_index = px!(vm, firstIndexableField(old_name_pointer));
    let old_name_size = px!(vm, byteSizeOf(old_name_pointer));
    // SAFETY: bytes objects hold their byteSizeOf readable bytes.
    unsafe {
        sqfile::sqFileRenameOldSizeNewSize(
            old_name_index.cast(),
            old_name_size,
            new_name_index.cast(),
            new_name_size,
        )
    };
    if !px_failed!(vm) {
        px!(vm, pop(2));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileSetPosition(vm: &Interp) -> PrimResult<Done> {
    if px!(vm, byteSizeOf(px!(vm, stackValue(0)))) > core::mem::size_of::<fileOffset_t>() as sqInt {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let new_position = px!(vm, positive64BitValueOf(px!(vm, stackValue(0)))) as fileOffset_t;
    let file = file_on_stack(vm, 1)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::sqFileSetPosition(file, new_position) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(2)); // pop position, file; leave rcvr on stack
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileSize(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    let mut size: fileOffset_t = 0;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        size = unsafe { sqfile::sqFileSize(file) };
    }
    if !px_failed!(vm) {
        let oop = px!(vm, positive64BitIntegerFor(size as libc::c_ulong));
        px!(vm, methodReturnValue(oop));
    }
    Ok(Done)
}

/// No accessor depth was exported by the generated C for this primitive
/// (Slang computes none for it), so -1 -- "does not traverse" -- keeps the
/// VM's view identical.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveFileStdioHandles(vm: &Interp) -> PrimResult<Done> {
    let mut records = [SQFile::zeroed(), SQFile::zeroed(), SQFile::zeroed()];
    // SAFETY: three writable records, as the contract asks.
    let valid_mask = unsafe { sqfile::sqFileStdioHandlesInto(records.as_mut_ptr()) };
    if valid_mask < 0 {
        px!(vm, primitiveFailForOSError(valid_mask as libc::c_long));
        return Ok(Done);
    }
    let mut result = px!(vm, instantiateClassindexableSize(px!(vm, classArray()), 3));
    if result == 0 {
        px!(vm, primitiveFailFor(PrimErr::NoMemory.code()));
        return Ok(Done);
    }
    px!(vm, pushRemappableOop(result));
    for index in 0..3 {
        if valid_mask & (1 << index) != 0 {
            result = px!(
                vm,
                instantiateClassindexableSize(px!(vm, classByteArray()), FILE_RECORD_SIZE as sqInt)
            );
            if result == 0 {
                px!(vm, popRemappableOop());
                px!(vm, primitiveFailFor(PrimErr::NoMemory.code()));
                return Ok(Done);
            }
            px!(
                vm,
                storePointerofObjectwithValue(index, px!(vm, topRemappableOop()), result)
            );
            let dst = px!(vm, firstIndexableField(result));
            // SAFETY: the fresh ByteArray holds FILE_RECORD_SIZE bytes; the
            // source is our local record. (The C memcpy'd uninitialized
            // padding here; ours is zeroed.)
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (&records[index as usize] as *const SQFile).cast::<u8>(),
                    dst.cast::<u8>(),
                    FILE_RECORD_SIZE,
                );
            }
        }
    }
    result = px!(vm, popRemappableOop());
    px!(vm, methodReturnValue(result));
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileSync(vm: &Interp) -> PrimResult<Done> {
    let file = file_on_stack(vm, 0)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::sqFileSync(file) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(1));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileTruncate(vm: &Interp) -> PrimResult<Done> {
    if px!(vm, isIntegerObject(px!(vm, stackValue(0)))) == 0
        && px!(vm, byteSizeOf(px!(vm, stackValue(0))))
            > core::mem::size_of::<fileOffset_t>() as sqInt
    {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let truncate_position = px!(vm, positive64BitValueOf(px!(vm, stackValue(0)))) as fileOffset_t;
    let file = file_on_stack(vm, 1)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::sqFileTruncate(file, truncate_position) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(2)); // pop position, file; leave rcvr on stack
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveFileWrite(vm: &Interp) -> PrimResult<Done> {
    let count = px!(vm, positiveMachineIntegerValueOf(px!(vm, stackValue(0))));
    let start_index = px!(vm, positiveMachineIntegerValueOf(px!(vm, stackValue(1))));
    let array = px!(vm, stackValue(2));
    let file = file_on_stack(vm, 3)?;
    // Buffer can be any indexable words or bytes object except CompiledMethod.
    if px_failed!(vm) || px!(vm, isWordsOrBytes(array)) == 0 {
        px!(vm, primitiveFailFor(PrimErr::BadArgument.code()));
        return Ok(Done);
    }
    let slot_size = px!(vm, slotSizeOf(array)) as usize;
    if !(start_index >= 1 && start_index.wrapping_add(count).wrapping_sub(1) <= slot_size) {
        px!(vm, primitiveFailFor(PrimErr::BadIndex.code()));
        return Ok(Done);
    }
    // Note: adjust startIndex for zero-origin byte indexing.
    let element_size = (px!(vm, byteSizeOf(array)) as usize)
        .checked_div(slot_size)
        .unwrap_or(1);
    let src = px!(vm, firstIndexableField(array));
    // SAFETY: `src` addresses the array's indexable bytes, bounds checked
    // above; `file` is checked by the callee.
    let bytes_written = unsafe {
        sqfile::sqFileWriteFromAt(
            file,
            count.wrapping_mul(element_size),
            src.cast(),
            (start_index - 1).wrapping_mul(element_size),
        )
    };
    if !px_failed!(vm) {
        let oop = px!(vm, integerObjectOf((bytes_written / element_size) as sqInt));
        px!(vm, methodReturnValue(oop));
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveWaitForDataWithSemaphore(vm: &Interp) -> PrimResult<Done> {
    // The Slang unwraps without an integer check, so a non-integer sets the
    // failure flag through integerValueOf and the fileValueOf failure check
    // below catches it.
    let semaphore_index = px!(vm, integerValueOf(px!(vm, stackValue(0))));
    let file = file_on_stack(vm, 1)?;
    if !px_failed!(vm) {
        // SAFETY: as in primitiveFileAtEnd.
        unsafe { sqfile::waitForDataonSemaphoreIndex(file, semaphore_index) };
    }
    if !px_failed!(vm) {
        px!(vm, pop(2));
    }
    Ok(Done)
}

// ---------------------------------------------------------------------------
// Directory primitives
// ---------------------------------------------------------------------------

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryCreate(vm: &Interp) -> PrimResult<Done> {
    let dir_name = px!(vm, stackValue(0));
    if px!(vm, isBytes(dir_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let dir_name_index = px!(vm, firstIndexableField(dir_name));
    let dir_name_size = px!(vm, byteSizeOf(dir_name));
    // SAFETY: a bytes object holds byteSizeOf readable bytes.
    if unsafe { dir::dir_Create(dir_name_index.cast(), dir_name_size) } == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    px!(vm, pop(1));
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryDelete(vm: &Interp) -> PrimResult<Done> {
    let dir_name = px!(vm, stackValue(0));
    if px!(vm, isBytes(dir_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let dir_name_index = px!(vm, firstIndexableField(dir_name));
    let dir_name_size = px!(vm, byteSizeOf(dir_name));
    // SAFETY: as in primitiveDirectoryCreate.
    if unsafe { dir::dir_Delete(dir_name_index.cast(), dir_name_size) } == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    px!(vm, pop(1));
    Ok(Done)
}

/// No accessor depth was exported by the generated C (the Slang method is
/// `<doNotGenerate>`-backed), so -1 keeps the VM's view identical.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveDirectoryDelimitor(vm: &Interp) -> PrimResult<Done> {
    if px!(vm, minorVersion()) >= 13 {
        let ch = px!(vm, characterObjectOf(dir::dir_Delimitor()));
        px!(vm, popthenPush(1, ch));
    } else {
        px!(vm, primitiveFail());
    }
    Ok(Done)
}

/// Builds the 7-slot entry array (name, creation date, modification date,
/// is-directory, size, posix permissions, is-symlink) --
/// `FilePlugin>>#makeDirEntryName:...isSymlink:`.
///
/// The Slang guarded the allocations against a moving GC with
/// `remapOop:in:`; Spur allocations never move objects (they fail instead),
/// so plain sequential allocation is equivalent. Allocation failure answers
/// `NoMemory` rather than storing into a nil array as the C would have.
fn make_dir_entry(vm: &Interp, e: &dir::DirEntry) -> PrimResult<sqInt> {
    let name_size = e.name_len.clamp(0, dir::ENTRY_NAME_MAX as sqInt);
    let results = px!(vm, instantiateClassindexableSize(px!(vm, classArray()), 7));
    let name_string = px!(
        vm,
        instantiateClassindexableSize(px!(vm, classString()), name_size)
    );
    // The date casts truncate to 32 bits exactly as the C's `unsigned int`
    // parameter did.
    let create_date_oop = px!(vm, positive32BitIntegerFor(e.creation_date as libc::c_uint));
    let mod_date_oop = px!(
        vm,
        positive32BitIntegerFor(e.modification_date as libc::c_uint)
    );
    let file_size_oop = px!(vm, positive64BitIntegerFor(e.size_if_file as libc::c_ulong));
    let posix_oop = px!(
        vm,
        positive32BitIntegerFor(e.posix_permissions as libc::c_uint)
    );
    if results == 0 || name_string == 0 {
        return Err(PrimErr::NoMemory);
    }

    // Copy the name into the Smalltalk string.
    let string_ptr = px!(vm, firstIndexableField(name_string));
    // SAFETY: the fresh String holds name_size indexable bytes; the source
    // is our local buffer.
    unsafe {
        core::ptr::copy_nonoverlapping(
            e.name.as_ptr(),
            string_ptr.cast::<u8>(),
            name_size as usize,
        );
    }

    let true_oop = px!(vm, trueObject());
    let false_oop = px!(vm, falseObject());
    px!(vm, storePointerofObjectwithValue(0, results, name_string));
    px!(
        vm,
        storePointerofObjectwithValue(1, results, create_date_oop)
    );
    px!(vm, storePointerofObjectwithValue(2, results, mod_date_oop));
    px!(
        vm,
        storePointerofObjectwithValue(
            3,
            results,
            if e.is_directory { true_oop } else { false_oop }
        )
    );
    px!(vm, storePointerofObjectwithValue(4, results, file_size_oop));
    px!(vm, storePointerofObjectwithValue(5, results, posix_oop));
    px!(
        vm,
        storePointerofObjectwithValue(6, results, if e.is_symlink { true_oop } else { false_oop })
    );
    Ok(results)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryLookup(vm: &Interp) -> PrimResult<Done> {
    let index = px!(vm, stackIntegerValue(0));
    let path_name = px!(vm, stackValue(1));
    if px!(vm, isBytes(path_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let path_index = px!(vm, firstIndexableField(path_name));
    let path_size = px!(vm, byteSizeOf(path_name)).max(0) as usize;
    // Copied out of image memory: make_dir_entry below allocates.
    let path =
        // SAFETY: a bytes object holds byteSizeOf readable bytes.
        unsafe { core::slice::from_raw_parts(path_index.cast::<u8>(), path_size) }.to_vec();

    let outcome = dir::lookup(&path, index);
    if px_failed!(vm) {
        return Ok(Done);
    }
    match outcome {
        Err(dir::NO_MORE_ENTRIES) => {
            // No more entries; answer nil.
            let nil = px!(vm, nilObject());
            px!(vm, popthenPush(3, nil)); // pop pathName, index, rcvr
        }
        Err(_) => {
            // Bad path.
            px!(vm, primitiveFail());
        }
        Ok(entry) => {
            let results = make_dir_entry(vm, &entry)?;
            px!(vm, popthenPush(3, results)); // pop pathName, index, rcvr
        }
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryEntry(vm: &Interp) -> PrimResult<Done> {
    let requested_name = px!(vm, stackValue(0));
    let path_name = px!(vm, stackValue(1));
    if px!(vm, isBytes(path_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    // The Slang read requestedName's bytes without checking it is a bytes
    // object -- undefined behaviour for anything else. Checking is the
    // memory-safe equivalent; a well-typed call never notices.
    if px!(vm, isBytes(requested_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let path_index = px!(vm, firstIndexableField(path_name));
    let path_size = px!(vm, byteSizeOf(path_name)).max(0) as usize;
    let req_index = px!(vm, firstIndexableField(requested_name));
    let req_size = px!(vm, byteSizeOf(requested_name)).max(0) as usize;
    // Copied out of image memory: make_dir_entry below allocates.
    // SAFETY: bytes objects hold their byteSizeOf readable bytes.
    let (path, req) = unsafe {
        (
            core::slice::from_raw_parts(path_index.cast::<u8>(), path_size).to_vec(),
            core::slice::from_raw_parts(req_index.cast::<u8>(), req_size).to_vec(),
        )
    };

    let outcome = dir::entry_lookup(&path, &req);
    if px_failed!(vm) {
        return Ok(Done);
    }
    match outcome {
        Err(dir::NO_MORE_ENTRIES) => {
            // No such entry; answer nil.
            let nil = px!(vm, nilObject());
            px!(vm, popthenPush(3, nil)); // pop name, pathName, rcvr
        }
        Err(_) => {
            px!(vm, primitiveFail());
        }
        Ok(entry) => {
            let results = make_dir_entry(vm, &entry)?;
            px!(vm, popthenPush(3, results));
        }
    }
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectoryGetMacTypeAndCreator(vm: &Interp) -> PrimResult<Done> {
    let creator_string = px!(vm, stackValue(0));
    let type_string = px!(vm, stackValue(1));
    let file_name = px!(vm, stackValue(2));
    if !(px!(vm, isBytes(creator_string)) != 0 && px!(vm, byteSizeOf(creator_string)) == 4) {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    if !(px!(vm, isBytes(type_string)) != 0 && px!(vm, byteSizeOf(type_string)) == 4) {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    if px!(vm, isBytes(file_name)) == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let creator_index = px!(vm, firstIndexableField(creator_string));
    let type_index = px!(vm, firstIndexableField(type_string));
    let file_name_index = px!(vm, firstIndexableField(file_name));
    let file_name_size = px!(vm, byteSizeOf(file_name));
    // SAFETY: validated bytes objects; the callee is a no-op on unix.
    let ok = unsafe {
        dir::dir_GetMacFileTypeAndCreator(
            file_name_index.cast(),
            file_name_size,
            type_index.cast(),
            creator_index.cast(),
        )
    };
    if ok == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    px!(vm, pop(3));
    Ok(Done)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveDirectorySetMacTypeAndCreator(vm: &Interp) -> PrimResult<Done> {
    let creator_string = px!(vm, stackValue(0));
    let type_string = px!(vm, stackValue(1));
    let file_name = px!(vm, stackValue(2));
    if !(px!(vm, isBytes(creator_string)) != 0
        && px!(vm, isBytes(type_string)) != 0
        && px!(vm, isBytes(file_name)) != 0
        && px!(vm, byteSizeOf(creator_string)) == 4
        && px!(vm, byteSizeOf(type_string)) == 4)
    {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    let creator_index = px!(vm, firstIndexableField(creator_string));
    let type_index = px!(vm, firstIndexableField(type_string));
    let file_name_index = px!(vm, firstIndexableField(file_name));
    let file_name_size = px!(vm, byteSizeOf(file_name));
    // SAFETY: as in primitiveDirectoryGetMacTypeAndCreator.
    let ok = unsafe {
        dir::dir_SetMacFileTypeAndCreator(
            file_name_index.cast(),
            file_name_size,
            type_index.cast(),
            creator_index.cast(),
        )
    };
    if ok == 0 {
        px!(vm, primitiveFail());
        return Ok(Done);
    }
    px!(vm, pop(3));
    Ok(Done)
}
