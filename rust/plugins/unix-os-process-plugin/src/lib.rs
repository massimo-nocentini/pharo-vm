//! `UnixOSProcessPlugin`, in Rust: drop-in replacement for the Slang-generated
//! `plugins/UnixOSProcessPlugin/src/common/UnixOSProcessPlugin.c` (OSProcess
//! 4.6.4 Cog, `VMConstruction-Plugins-OSProcessPlugin.oscog-dtl.66`).
//!
//! The plugin is the image's window onto Unix process machinery: fork/exec of
//! child processes, pipes, standard-stream handles, environment access,
//! signal forwarding to Smalltalk semaphores, process/group/session ids,
//! `fcntl` file locking, and a handful of fd-level utilities. All 93 exported
//! primitives of the C plugin are here, under the same names, with the same
//! argument shapes and failure behaviour and the same accessor depths.
//!
//! The README lists every deliberate divergence; the two structural ones are
//! that `vfork()` became `fork()` (Rust cannot host a returns-twice call
//! soundly) and that cross-module symbols (`getProcess*Vector`,
//! `sqFileStdioHandlesInto`) resolve at runtime through the proxy's
//! `ioLoadFunctionFrom` instead of at link time.

// The crate is named for the shared library the VM loads, and the primitive
// names are fixed by the image.
#![allow(non_snake_case)]

// The C had no Darwin branch anywhere -- `grep -ril 'apple\|darwin\|__MACH__'`
// over `plugins/UnixOSProcessPlugin/` finds nothing, and apart from the
// `SQUEAK_BUILTIN_PLUGIN` switches its only conditionals are the
// `__OpenBSD__` include block (UnixOSProcessPlugin.c:28) and four feature
// tests: `isIntegerObject` (:310), `SA_DISABLE` (:1434), `SA_NOCLDSTOP`
// (:4244, :4396, :4414) and `SIG_HOLD` (:4590). The port needs three arms,
// all because Rust cannot lean on the platform's own headers: `NSIG` (signals.rs), the errno location (below) and the
// `FILE *stdin/stdout/stderr` globals (sqfile.rs). All three key off the same
// pair of predicates, so any other Unix -- FreeBSD, illumos -- needs a third
// arm at each. Say that here rather than let it surface as three unresolved
// imports.
#[cfg(not(any(target_vendor = "apple", target_os = "linux")))]
compile_error!(
    "unix-os-process-plugin supports Linux and Apple targets only: NSIG, the \
     errno location and the stdio globals each need an arm for this target"
);

mod signals;
mod spawn;
mod sqfile;
mod support;

use std::ffi::{c_char, c_int, c_void, CString};

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

use signals::{
    clear_kill_list, forward_signal_to_semaphore, restore_original_handlers, sema_index_for,
    set_kill_list, set_sig_chld_handler, set_sig_chld_sema_index, set_signal_handler,
    set_signal_to_send, SIG_ERR_VALUE,
};
use spawn::{create_pipe, fork_and_exec, fork_squeak, ChildFds, ExecSpec};
use sqfile::{
    file_descriptor_from, is_sq_file_object, read_record, session_identifier_from, SQFile,
};
use support::{
    collection_from_bytes, collection_with_trailing_nul, cstr_bytes, first_indexable_field,
    integer_value_raw, st_object_at_put, st_size_of, stack_object_value, this_session_id,
    transient_cstring, CachedFn,
};

/// What the C's `getModuleName()` answered for the external plugin. The VM
/// compares only the prefix up to the requested module name
/// (`strncmp` in `sqNamedPrims.c`), and OSProcess image code reads the rest
/// as a version stamp, so the whole string is kept.
const MODULE_NAME: &str =
    "UnixOSProcessPlugin VMConstruction-Plugins-OSProcessPlugin.oscog-dtl.66 (e)";

/// `versionString` in the C.
const VERSION_STRING: &str = "4.6.4 Cog";

pharo_plugin!(
    "UnixOSProcessPlugin VMConstruction-Plugins-OSProcessPlugin.oscog-dtl.66 (e)",
    init = initialise,
    shutdown = shutdown
);

/// `initialiseModule` / `initializeModuleForPlatform`: reset the kill-on-exit
/// list, register the exit hook, note the interpreter thread, and leave the
/// sigaltstack question undecided. Runs on the interpreter thread.
fn initialise() -> bool {
    clear_kill_list();
    // SAFETY: registering a plain extern "C" fn with the C runtime, as the C
    // plugin's atexit(sendSignalToPids) did.
    unsafe { libc::atexit(signals::send_signal_to_pids) };
    signals::note_vm_thread();
    true
}

/// `shutdownModule`: put back every signal handler this plugin replaced.
fn shutdown() -> bool {
    restore_original_handlers();
    true
}

/// `moduleUnloaded:` -- the C exported a no-op; so do we.
#[no_mangle]
pub extern "C" fn moduleUnloaded(_module_name: *mut c_char) -> sqInt {
    0
}

// ===========================================================================
// Shared pieces
// ===========================================================================

/// Widens any C integer answer to `sqInt` (`isize` has no `From<i32>`).
fn sqint_of<T: Into<i64>>(v: T) -> sqInt {
    v.into() as sqInt
}

/// Errno as the C's `extern int errno` reads gave it.
fn errno() -> c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// The address of the calling thread's `errno`.
///
/// The C declared `extern int errno;` inside four primitives and read the
/// global: `primitiveChdir` (`UnixOSProcessPlugin.c:1660`),
/// `primitiveFileProtectionMask` (:1956), `primitiveFileStat` (:2009) and
/// `primitiveNice` (:2850). On both platforms `errno` is
/// really a macro over a per-thread location, spelled `__errno_location()` by
/// glibc and `__error()` by Darwin's libc; the port calls whichever the
/// target has, which is what the C's `errno` would have expanded to had it
/// included `<errno.h>` instead of redeclaring the name.
#[cfg(target_os = "linux")]
use libc::__errno_location as errno_location;
#[cfg(target_vendor = "apple")]
use libc::__error as errno_location;

/// `errno = 0`, which `primitiveNice` needs before the call.
fn clear_errno() {
    // SAFETY: writing the thread's errno slot, as C's `errno = 0` does.
    unsafe { *errno_location() = 0 };
}

/// Validates and copies out an SQFile record, failing like the C's
/// `primitiveFail()` when any of the four checks miss.
fn validated_sq_file(vm: &Interp, oop: Oop) -> PrimResult<SQFile> {
    if !is_sq_file_object(vm, oop)? {
        return Err(PrimErr::GenericFailure);
    }
    read_record(vm, oop)
}

/// The C's pattern `kill(stackIntegerValue(0), SIG)` with the explicit
/// isIntegerObject check so a bad argument answers -1 rather than signaling
/// pid 1.
fn send_signal_to_stack_pid(vm: &Interp, sig: c_int) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let oop = vm.stack_value(0)?;
    if !vm.is_integer_object(oop)? {
        return Ok(-1);
    }
    let pid = vm.stack_integer(0)? as libc::pid_t;
    // SAFETY: kill(2) on an arbitrary pid; the OS enforces permissions.
    Ok(sqint_of(unsafe { libc::kill(pid, sig) }))
}

/// `cString:asCollection:` over a NUL-terminated C string.
///
/// # Safety
///
/// `p` must be a valid NUL-terminated string.
unsafe fn cstring_as_collection(vm: &Interp, class: Oop, p: *const c_char) -> PrimResult<Oop> {
    let bytes = unsafe { cstr_bytes(p) }.to_vec();
    collection_from_bytes(vm, class, &bytes)
}

// ===========================================================================
// Process arguments and environment
// ===========================================================================

/// `argumentAtAsType:` -- 1-based index into the VM's argv; out of range
/// answers nil; the answered collection carries the C's trailing NUL byte.
fn argument_at_as_type(vm: &Interp, class: Oop) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let index = vm.stack_integer(0)?;
    // The C linked straight to getProcessArgumentVector; without the VM
    // exporting it there is nothing to answer.
    let Some(entry) = spawn::argument_at(index) else {
        return Err(PrimErr::Unsupported);
    };
    match entry {
        None => vm.nil(),
        Some(bytes) => collection_with_trailing_nul(vm, class, &bytes),
    }
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveArgumentAt(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_string()?;
    argument_at_as_type(vm, class)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveArgumentAtAsBytes(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_byte_array()?;
    argument_at_as_type(vm, class)
}

/// `environmentAtAsType:` -- 1-based index into the environment vector.
fn environment_at_as_type(vm: &Interp, class: Oop) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    if !spawn::have_environment_vector() {
        return Err(PrimErr::GenericFailure);
    }
    let index = vm.stack_integer(0)?;
    match spawn::environment_string_at(index) {
        None => vm.nil(),
        Some(bytes) => collection_with_trailing_nul(vm, class, &bytes),
    }
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveEnvironmentAt(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_string()?;
    environment_at_as_type(vm, class)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveEnvironmentAtAsBytes(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_byte_array()?;
    environment_at_as_type(vm, class)
}

/// `environmentAtSymbolAsType:` -- getenv by key; an absent key fails the
/// primitive (that is how the image spells "no such variable").
fn environment_at_symbol_as_type(vm: &Interp, class: Oop) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let key = transient_cstring(vm, stack_object_value(vm, 0)?)?;
    // SAFETY: getenv with a valid C string; the result is copied before any
    // other environment access can invalidate it.
    let value = unsafe { libc::getenv(key.as_ptr()) };
    if value.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    let bytes = unsafe { cstr_bytes(value) }.to_vec();
    collection_from_bytes(vm, class, &bytes)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveEnvironmentAtSymbol(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_string()?;
    environment_at_symbol_as_type(vm, class)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveEnvironmentAtSymbolAsBytes(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_byte_array()?;
    environment_at_symbol_as_type(vm, class)
}

/// putenv of a 'KEY=value' string. The C string is deliberately leaked into
/// the C heap: putenv keeps the pointer (the C plugin's cStringFromString
/// comment says exactly this). Answers the argument.
#[pharo_primitive(accessor_depth = 2)]
fn primitivePutEnv(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let key_value = stack_object_value(vm, 0)?;
    let cs = transient_cstring(vm, key_value)?;
    let raw = cs.into_raw();
    // SAFETY: `raw` is a valid NUL-terminated heap string which putenv takes
    // ownership of on success.
    if unsafe { libc::putenv(raw) } == 0 {
        Ok(key_value)
    } else {
        // Unlike the C (which leaked here too), reclaim on failure.
        // SAFETY: putenv rejected it, so we still own the allocation.
        drop(unsafe { CString::from_raw(raw) });
        Err(PrimErr::GenericFailure)
    }
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveUnsetEnv(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(1)?;
    let key = transient_cstring(vm, stack_object_value(vm, 0)?)?;
    // SAFETY: unsetenv with a valid C string.
    unsafe { libc::unsetenv(key.as_ptr()) };
    Ok(())
}

// ===========================================================================
// Identity: pids, uids, session
// ===========================================================================

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetPid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::getpid() } as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetPPid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::getppid() } as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetUid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::getuid() } as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetEUid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::geteuid() } as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetGid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::getgid() } as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetEGid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(unsafe { libc::getegid() } as sqInt)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveGetPGid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let pid = vm.stack_integer(0)? as libc::pid_t;
    let pgid = unsafe { libc::getpgid(pid) };
    if pgid == -1 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(pgid as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetPGrp(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let pgid = unsafe { libc::getpgrp() };
    if pgid == -1 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(pgid as sqInt)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSetPGid(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(2)?;
    let pid = vm.stack_integer(1)? as libc::pid_t;
    let pgid = vm.stack_integer(0)? as libc::pid_t;
    if unsafe { libc::setpgid(pid, pgid) } == -1 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// setpgid(0,0) rather than setpgrp(), for the portability reason the C's
/// comment gives.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSetPGrp(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(0)?;
    if unsafe { libc::setpgid(0, 0) } == -1 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSetSid(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let sid = unsafe { libc::setsid() };
    if sid == -1 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(sid as sqInt)
}

/// The session identifier as a ByteArray in machine byte order, so the image
/// can hold the full `int` without SmallInteger games.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetSession(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let id = this_session_id(vm)?;
    if id == 0 {
        return Err(PrimErr::GenericFailure);
    }
    let class = vm.class_byte_array()?;
    collection_from_bytes(vm, class, &id.to_ne_bytes())
}

/// The interpreter pthread's id, as a machine-order ByteArray (a pthread_t
/// can be wider than a SmallInteger likes).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetThreadID(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let class = vm.class_byte_array()?;
    let thread = signals::vm_thread() as usize;
    collection_from_bytes(vm, class, &thread.to_ne_bytes())
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveNice(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let increment = vm.stack_integer(0)? as c_int;
    clear_errno();
    let result = unsafe { libc::nice(increment) };
    // nice() legitimately answers -1; only errno distinguishes failure.
    if result == -1 && errno() != 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(sqint_of(result))
}

// ===========================================================================
// Working directory and paths
// ===========================================================================

/// chdir(2). Answers nil on success, errno on failure -- not a primitive
/// failure, the image reads the errno.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveChdir(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let path = transient_cstring(vm, stack_object_value(vm, 0)?)?;
    // SAFETY: chdir with a valid C string.
    if unsafe { libc::chdir(path.as_ptr()) } != 0 {
        vm.integer_checked(sqint_of(errno()))
    } else {
        vm.nil()
    }
}

/// `getCurrentWorkingDirectoryAsType:`'s core, minus the C's in-image scratch
/// buffers: same growth ladder (100 bytes at a time, up to 5000), same
/// failure behaviour.
fn cwd_as_type(vm: &Interp, class: Oop) -> PrimResult<Oop> {
    let mut size = 100usize;
    loop {
        let mut buf = vec![0u8; size];
        // SAFETY: getcwd into a buffer we own, of the size we state.
        let cwd = unsafe { libc::getcwd(buf.as_mut_ptr().cast::<c_char>(), size) };
        if !cwd.is_null() {
            let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            return collection_from_bytes(vm, class, &buf[..len]);
        }
        if size >= 5000 {
            return Err(PrimErr::GenericFailure);
        }
        size += 100;
    }
}

/// Exported for other plugins/the VM, as the C plugin exported it. Performs
/// the whole primitive protocol itself (answer or fail), answering 0 like the
/// C function did.
#[no_mangle]
pub extern "C" fn getCurrentWorkingDirectoryAsType(classIdentifier: sqInt) -> sqInt {
    let vt = pharo_vm_plugin::__private::INTERP.load(std::sync::atomic::Ordering::Acquire);
    if vt.is_null() {
        return 0;
    }
    // SAFETY: the proxy table the VM handed setInterpreter.
    let vm = unsafe { Interp::from_raw(vt) };
    match cwd_as_type(&vm, Oop(classIdentifier)) {
        Ok(oop) => {
            let _ = vm.return_value(oop);
        }
        Err(code) => vm.fail_for(code),
    }
    0
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetCurrentWorkingDirectory(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let class = vm.class_string()?;
    cwd_as_type(vm, class)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetCurrentWorkingDirectoryAsBytes(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let class = vm.class_byte_array()?;
    cwd_as_type(vm, class)
}

/// `realpathAsType:` -- realpath(3) with a libc-allocated result instead of
/// the C's fixed 1024-byte in-image buffer (which realpath could overrun).
/// The C's `len >= 1024 -> fail` check is kept so the answers agree.
fn realpath_as_type(vm: &Interp, class: Oop) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let path = transient_cstring(vm, stack_object_value(vm, 0)?)?;
    // SAFETY: realpath with a null buffer mallocs the result; freed below.
    let resolved = unsafe { libc::realpath(path.as_ptr(), std::ptr::null_mut()) };
    if resolved.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    let bytes = unsafe { cstr_bytes(resolved) }.to_vec();
    // SAFETY: freeing what realpath malloc'ed.
    unsafe { libc::free(resolved.cast::<c_void>()) };
    if bytes.len() >= 1024 {
        return Err(PrimErr::GenericFailure);
    }
    collection_from_bytes(vm, class, &bytes)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveRealpath(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_string()?;
    realpath_as_type(vm, class)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveRealpathAsBytes(vm: &Interp) -> PrimResult<Oop> {
    let class = vm.class_byte_array()?;
    realpath_as_type(vm, class)
}

// ===========================================================================
// stat(2)
// ===========================================================================

/// The four octal digits of a protection mask, most significant first --
/// exactly the arithmetic the C stored into its answer array.
fn mode_digits(mode: sqInt) -> [sqInt; 4] {
    [
        (mode & 0o7000) >> 9,
        (mode & 0o700) >> 6,
        (mode & 0o70) >> 3,
        mode & 0o7,
    ]
}

fn stat_path(vm: &Interp) -> PrimResult<Option<libc::stat>> {
    let path = transient_cstring(vm, stack_object_value(vm, 0)?)?;
    // SAFETY: zeroed stat buffer of the right type, filled by stat(2).
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::stat(path.as_ptr(), &mut st) } == 0 {
        Ok(Some(st))
    } else {
        Ok(None)
    }
}

fn mask_array(vm: &Interp, mode: sqInt) -> PrimResult<Oop> {
    let mask = vm.instantiate(vm.class_array()?, 4)?;
    for (i, digit) in mode_digits(mode).iter().enumerate() {
        st_object_at_put(vm, mask, i as sqInt + 1, vm.integer(*digit)?)?;
    }
    Ok(mask)
}

/// Protection mask as four octal digits, or errno.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileProtectionMask(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    match stat_path(vm)? {
        Some(st) => mask_array(vm, st.st_mode as sqInt),
        None => vm.integer_checked(sqint_of(errno())),
    }
}

/// {uid. gid. protectionMask}, or errno.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveFileStat(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    match stat_path(vm)? {
        Some(st) => {
            let result = vm.instantiate(vm.class_array()?, 3)?;
            let mask = mask_array(vm, st.st_mode as sqInt)?;
            st_object_at_put(vm, result, 1, vm.integer(st.st_uid as sqInt)?)?;
            st_object_at_put(vm, result, 2, vm.integer(st.st_gid as sqInt)?)?;
            st_object_at_put(vm, result, 3, mask)?;
            Ok(result)
        }
        None => vm.integer_checked(sqint_of(errno())),
    }
}

// ===========================================================================
// Error messages, sizes, names
// ===========================================================================

#[pharo_primitive(accessor_depth = 0)]
fn primitiveErrorMessageAt(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let index = vm.stack_integer(0)? as c_int;
    // SAFETY: strerror answers a process-lifetime (or thread-local) string;
    // copied immediately.
    let msg = unsafe { libc::strerror(index) };
    if msg.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    let class = vm.class_string()?;
    unsafe { cstring_as_collection(vm, class, msg) }
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSizeOfInt(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(std::mem::size_of::<c_int>() as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSizeOfPointer(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(std::mem::size_of::<*const c_void>() as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveModuleName(vm: &Interp) -> PrimResult<&'static str> {
    vm.expect_argument_count(0)?;
    Ok(MODULE_NAME)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveVersionString(vm: &Interp) -> PrimResult<&'static str> {
    vm.expect_argument_count(0)?;
    Ok(VERSION_STRING)
}

// ===========================================================================
// Pipes and standard handles
// ===========================================================================

/// Builds the {reader. writer} answer both pipe primitives share. The C
/// stored reader at index 1 (last remappable pushed, first popped) and writer
/// at index 2.
fn pipe_array(vm: &Interp, session: sqfile::SessionId, ignore_sigpipe: bool) -> PrimResult<Oop> {
    if ignore_sigpipe {
        // makePipeForReader:writer: arms SIG_IGN on SIGPIPE first, so a
        // reader that goes away cannot kill the VM.
        set_signal_handler(libc::SIGPIPE, libc::SIG_IGN);
    }
    let (reader, writer) = create_pipe().ok_or(PrimErr::GenericFailure)?;
    let byte_array = vm.class_byte_array()?;
    let size = std::mem::size_of::<SQFile>() as sqInt;
    let writer_oop = vm.instantiate(byte_array, size)?;
    vm.write_bytes(
        writer_oop,
        0,
        SQFile::for_stream(writer, session, true).as_bytes(),
    )?;
    let reader_oop = vm.instantiate(byte_array, size)?;
    vm.write_bytes(
        reader_oop,
        0,
        SQFile::for_stream(reader, session, false).as_bytes(),
    )?;
    let result = vm.instantiate(vm.class_array()?, 2)?;
    st_object_at_put(vm, result, 1, reader_oop)?;
    st_object_at_put(vm, result, 2, writer_oop)?;
    Ok(result)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveCreatePipe(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let session = this_session_id(vm)?;
    pipe_array(vm, session, false)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveCreatePipeWithSessionIdentifier(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let session = session_identifier_from(vm, stack_object_value(vm, 0)?)?;
    pipe_array(vm, session, false)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveMakePipe(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let session = this_session_id(vm)?;
    pipe_array(vm, session, true)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveMakePipeWithSessionIdentifier(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let session = session_identifier_from(vm, stack_object_value(vm, 0)?)?;
    pipe_array(vm, session, true)
}

/// FilePlugin's `sqFileStdioHandlesInto`, resolved at runtime. The support
/// code lives in the FilePlugin library, but may equally be linked into the
/// VM core, so both are tried.
static STDIO_FROM_FILEPLUGIN: CachedFn = CachedFn::new();
static STDIO_FROM_CORE: CachedFn = CachedFn::new();

fn stdio_handles_fn() -> Option<unsafe extern "C" fn(*mut SQFile) -> sqInt> {
    let mut p = STDIO_FROM_FILEPLUGIN.get("sqFileStdioHandlesInto", "FilePlugin");
    if p.is_null() {
        p = STDIO_FROM_CORE.get("sqFileStdioHandlesInto", "");
    }
    if p.is_null() {
        return None;
    }
    // SAFETY: the symbol has the C signature `sqInt (SQFile files[3])`.
    Some(unsafe {
        std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut SQFile) -> sqInt>(p)
    })
}

/// `getStdHandle:` -- 0 = stdin, 1 = stdout, 2 = stderr, answered as an
/// SQFile ByteArray filled in by the FilePlugin.
fn get_std_handle(vm: &Interp, n: usize) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let f = stdio_handles_fn().ok_or(PrimErr::Unsupported)?;
    let mut records = [SQFile::zeroed(), SQFile::zeroed(), SQFile::zeroed()];
    // SAFETY: exactly three records, as the signature requires.
    let valid_mask = unsafe { f(records.as_mut_ptr()) };
    if valid_mask & (1 << n) == 0 {
        return Err(PrimErr::Unsupported);
    }
    let oop = vm.instantiate(vm.class_byte_array()?, std::mem::size_of::<SQFile>() as sqInt)?;
    vm.write_bytes(oop, 0, records[n].as_bytes())?;
    Ok(oop)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetStdInHandle(vm: &Interp) -> PrimResult<Oop> {
    get_std_handle(vm, 0)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetStdOutHandle(vm: &Interp) -> PrimResult<Oop> {
    get_std_handle(vm, 1)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveGetStdErrHandle(vm: &Interp) -> PrimResult<Oop> {
    get_std_handle(vm, 2)
}

/// The `...WithSessionIdentifier` stdio variants build the record from this
/// plugin's own view of the C stdio streams, stamping in the session the
/// image passed.
fn std_handle_with_session(
    vm: &Interp,
    stream: *mut libc::FILE,
    writable: bool,
) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let oop = vm.instantiate(vm.class_byte_array()?, std::mem::size_of::<SQFile>() as sqInt)?;
    let session = session_identifier_from(vm, stack_object_value(vm, 0)?)?;
    vm.write_bytes(oop, 0, SQFile::for_stream(stream, session, writable).as_bytes())?;
    Ok(oop)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveGetStdInHandleWithSessionIdentifier(vm: &Interp) -> PrimResult<Oop> {
    std_handle_with_session(vm, sqfile::stdin_stream(), false)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveGetStdOutHandleWithSessionIdentifier(vm: &Interp) -> PrimResult<Oop> {
    std_handle_with_session(vm, sqfile::stdout_stream(), true)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveGetStdErrHandleWithSessionIdentifier(vm: &Interp) -> PrimResult<Oop> {
    std_handle_with_session(vm, sqfile::stderr_stream(), true)
}

// ===========================================================================
// SQFile-level operations
// ===========================================================================

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileFlush(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let record = validated_sq_file(vm, vm.stack_value(0)?)?;
    // SAFETY: the validated record's stream; fflush(NULL) is defined (flush
    // everything), matching what the C would have done with a null field.
    Ok(sqint_of(unsafe { libc::fflush(record.file) }))
}

/// The session argument is popped but never compared -- the C validated
/// against the interpreter's own session exactly as the plain variant does.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileFlushWithSessionIdentifier(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(2)?;
    let record = validated_sq_file(vm, vm.stack_value(1)?)?;
    // SAFETY: as in primitiveSQFileFlush.
    Ok(sqint_of(unsafe { libc::fflush(record.file) }))
}

fn set_blocking_state(vm: &Interp, oop: Oop, non_blocking: bool) -> PrimResult<sqInt> {
    validated_sq_file(vm, oop)?;
    let descriptor = file_descriptor_from(vm, oop)?;
    if descriptor < 0 {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: fcntl flag surgery on a validated descriptor.
    unsafe {
        let flags = libc::fcntl(descriptor, libc::F_GETFL);
        let flags = if non_blocking {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        Ok(sqint_of(libc::fcntl(descriptor, libc::F_SETFL, flags)))
    }
}

/// The `WithSessionIdentifier` variants additionally require the passed
/// session to match the record's.
fn require_matching_session(vm: &Interp, record: &SQFile) -> PrimResult<()> {
    let passed = session_identifier_from(vm, stack_object_value(vm, 0)?)?;
    if passed != record.session_id {
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetBlocking(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    set_blocking_state(vm, vm.stack_value(0)?, false)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetBlockingWithSessionIdentifier(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(2)?;
    let oop = vm.stack_value(1)?;
    let record = validated_sq_file(vm, oop)?;
    require_matching_session(vm, &record)?;
    set_blocking_state(vm, oop, false)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetNonBlocking(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    set_blocking_state(vm, vm.stack_value(0)?, true)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetNonBlockingWithSessionIdentifier(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(2)?;
    let oop = vm.stack_value(1)?;
    let record = validated_sq_file(vm, oop)?;
    require_matching_session(vm, &record)?;
    set_blocking_state(vm, oop, true)
}

fn set_unbuffered(record: &SQFile) -> sqInt {
    // SAFETY: fflush answers the indicator the primitive reports; setbuf's
    // void result is discarded, exactly the C's (self-described) near-useless
    // protocol.
    unsafe {
        let ret = libc::fflush(record.file);
        libc::setbuf(record.file, std::ptr::null_mut());
        sqint_of(ret)
    }
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetUnbuffered(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let record = validated_sq_file(vm, vm.stack_value(0)?)?;
    Ok(set_unbuffered(&record))
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveSQFileSetUnbufferedWithSessionIdentifier(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(2)?;
    let record = validated_sq_file(vm, vm.stack_value(1)?)?;
    require_matching_session(vm, &record)?;
    Ok(set_unbuffered(&record))
}

fn eof_flag(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(1)?;
    let record = validated_sq_file(vm, vm.stack_value(0)?)?;
    if record.file.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: feof on the validated stream.
    Ok(unsafe { libc::feof(record.file) } != 0)
}

/// Deprecated in the C ("return values are reversed") but still exported;
/// both variants share one body there too.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveIsAtEndOfFile(vm: &Interp) -> PrimResult<bool> {
    eof_flag(vm)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveTestEndOfFileFlag(vm: &Interp) -> PrimResult<bool> {
    eof_flag(vm)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveUnixFileNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let oop = vm.stack_value(0)?;
    validated_sq_file(vm, oop)?;
    Ok(sqint_of(file_descriptor_from(vm, oop)?))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveUnixFileClose(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let handle = vm.stack_integer(0)? as c_int;
    // SAFETY: close(2) on an arbitrary descriptor; -1/EBADF is the answer for
    // a bad one, as the image expects.
    Ok(sqint_of(unsafe { libc::close(handle) }))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveDup(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let fd = vm.stack_integer(0)? as c_int;
    // SAFETY: dup(2); failure is the -1 answer.
    Ok(sqint_of(unsafe { libc::dup(fd) }))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveDupTo(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(2)?;
    let new_fd = vm.stack_integer(0)? as c_int;
    let old_fd = vm.stack_integer(1)? as c_int;
    // SAFETY: dup2(2); failure is the -1 answer.
    Ok(sqint_of(unsafe { libc::dup2(old_fd, new_fd) }))
}

// ===========================================================================
// File locking
// ===========================================================================

// `struct flock`'s `l_type` is a `short` on every Unix, but the constants
// that go into it are not. glibc declares `F_RDLCK`/`F_WRLCK`/`F_UNLCK` as
// `int` with the values 0/1/2; Darwin's `<sys/fcntl.h>` declares them as
// `short` with the values 1/3/2 -- different width *and* different numbers.
// The C never had to notice: it wrote `lockStruct.l_type = <constant>` at
// five sites -- `F_WRLCK` at UnixOSProcessPlugin.c:2684 and :4019, `F_RDLCK`
// at :2687 and :4022, `F_UNLCK` at :4138 -- and let each platform's own
// compiler do the conversion. Rust will not, so the conversion is named once
// here rather than spelled out at those call sites.
//
// The numbers reach the image: `primitiveTestLockableFileRegion` answers the
// raw `l_type` of a blocking lock in slot 3, so a Mac answers 3 (F_WRLCK)
// where Linux answers 1. The C's answer was platform-dependent in exactly the
// same way, so the port keeps it rather than normalising -- see the crate
// README's macOS section.
const F_RDLCK_SHORT: libc::c_short = libc::F_RDLCK as libc::c_short;
const F_WRLCK_SHORT: libc::c_short = libc::F_WRLCK as libc::c_short;
const F_UNLCK_SHORT: libc::c_short = libc::F_UNLCK as libc::c_short;

// Compile-time proof that the narrowing above loses nothing on this target.
// A platform whose lock constants outgrow `l_type` stops the build here
// instead of silently requesting a truncated lock type.
const _: () = {
    assert!(F_RDLCK_SHORT as i64 == libc::F_RDLCK as i64);
    assert!(F_WRLCK_SHORT as i64 == libc::F_WRLCK as i64);
    assert!(F_UNLCK_SHORT as i64 == libc::F_UNLCK as i64);
};

fn lock_struct(lock_type: libc::c_short, start: sqInt, len: sqInt) -> libc::flock {
    // SAFETY: flock is plain data; zero then fill, so platform-extra fields
    // stay zeroed like the C's stack struct plus explicit assignments.
    // (Darwin orders the fields l_start, l_len, l_pid, l_type, l_whence and
    // Linux l_type, l_whence, l_start, l_len, l_pid; every access here is by
    // name, so only the two field *types* above needed attention.)
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = lock_type;
    lock.l_whence = libc::SEEK_SET as libc::c_short;
    lock.l_start = start as libc::off_t;
    lock.l_len = len as libc::off_t;
    lock.l_pid = 0;
    lock
}

fn region_file_no(vm: &Interp, oop: Oop) -> PrimResult<c_int> {
    validated_sq_file(vm, oop)?;
    let fd = file_descriptor_from(vm, oop)?;
    if fd < 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(fd)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveLockFileRegion(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(4)?;
    let exclusive = vm.stack_value(0)? == vm.true_object()?;
    let len = vm.stack_integer(1)?;
    let start = vm.stack_integer(2)?;
    let file_no = region_file_no(vm, vm.stack_value(3)?)?;
    let lock = lock_struct(
        if exclusive { F_WRLCK_SHORT } else { F_RDLCK_SHORT },
        start,
        len,
    );
    // SAFETY: fcntl file-lock request on a validated descriptor.
    Ok(sqint_of(unsafe {
        libc::fcntl(file_no, libc::F_SETLK, &lock)
    }))
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveUnlockFileRegion(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(3)?;
    let len = vm.stack_integer(0)?;
    let start = vm.stack_integer(1)?;
    let file_no = region_file_no(vm, vm.stack_value(2)?)?;
    let lock = lock_struct(F_UNLCK_SHORT, start, len);
    // SAFETY: as above; unlocking an unlocked region is a harmless success.
    Ok(sqint_of(unsafe {
        libc::fcntl(file_no, libc::F_SETLK, &lock)
    }))
}

/// F_GETLK probe. Answers -1 when fcntl fails, else the six-slot array the C
/// documented: {lockable. l_pid. l_type. l_whence. l_start. l_len}.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveTestLockableFileRegion(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(4)?;
    let exclusive = vm.stack_value(0)? == vm.true_object()?;
    let len = vm.stack_integer(1)?;
    let start = vm.stack_integer(2)?;
    let file_no = region_file_no(vm, vm.stack_value(3)?)?;
    let mut lock = lock_struct(
        if exclusive { F_WRLCK_SHORT } else { F_RDLCK_SHORT },
        start,
        len,
    );
    // SAFETY: F_GETLK writes the conflicting lock back into the struct.
    let result = unsafe { libc::fcntl(file_no, libc::F_GETLK, &mut lock) };
    if result == -1 {
        return vm.integer(sqint_of(result));
    }
    let lockable = if lock.l_type == F_UNLCK_SHORT {
        vm.true_object()?
    } else {
        vm.false_object()?
    };
    let array = vm.instantiate(vm.class_array()?, 6)?;
    st_object_at_put(vm, array, 1, lockable)?;
    st_object_at_put(vm, array, 2, vm.integer(lock.l_pid as sqInt)?)?;
    st_object_at_put(vm, array, 3, vm.integer(sqint_of(lock.l_type))?)?;
    st_object_at_put(vm, array, 4, vm.integer(sqint_of(lock.l_whence))?)?;
    st_object_at_put(vm, array, 5, vm.integer(lock.l_start as sqInt)?)?;
    st_object_at_put(vm, array, 6, vm.integer(lock.l_len as sqInt)?)?;
    Ok(array)
}

// ===========================================================================
// Signals
// ===========================================================================

/// kill(pid, 0) as an existence-and-permission probe. A non-integer argument
/// answers false (the common "child pid is nil after image restart" case).
#[pharo_primitive(accessor_depth = 0)]
fn primitiveCanReceiveSignals(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(1)?;
    let oop = vm.stack_value(0)?;
    if !vm.is_integer_object(oop)? {
        return Ok(false);
    }
    let pid = vm.stack_integer(0)? as libc::pid_t;
    // SAFETY: kill with signal 0 only probes.
    Ok(unsafe { libc::kill(pid, 0) } == 0)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigabrtTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGABRT)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigalrmTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGALRM)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigchldTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGCHLD)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigcontTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGCONT)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSighupTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGHUP)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigintTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGINT)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigkillTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGKILL)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigpipeTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGPIPE)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigquitTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGQUIT)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigstopTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGSTOP)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigtermTo(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGTERM)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigusr1To(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGUSR1)
}

#[pharo_primitive(accessor_depth = 0)]
fn primitiveSendSigusr2To(vm: &Interp) -> PrimResult<sqInt> {
    send_signal_to_stack_pid(vm, libc::SIGUSR2)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigChldNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGCHLD))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigHupNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGHUP))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigIntNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGINT))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigKillNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGKILL))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigPipeNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGPIPE))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigQuitNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGQUIT))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigTermNumber(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGTERM))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigUsr1Number(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGUSR1))
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveSigUsr2Number(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(sqint_of(libc::SIGUSR2))
}

/// Registers (or, with 0/nil, unregisters) forwarding of a Unix signal to a
/// Smalltalk semaphore. Answers the prior handler address as a pointer-sized
/// ByteArray the image treats as opaque.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveForwardSignalToSemaphore(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(2)?;
    let index_oop = vm.stack_value(0)?;
    let semaphore_index = if index_oop == vm.nil()? {
        0
    } else if vm.is_integer_object(index_oop)? {
        vm.stack_integer(0)?
    } else {
        return Err(PrimErr::GenericFailure);
    };
    let sig_num = vm.stack_integer(1)?;
    let handler = forward_signal_to_semaphore(sig_num, semaphore_index);
    if handler == SIG_ERR_VALUE {
        return Err(PrimErr::GenericFailure);
    }
    let class = vm.class_byte_array()?;
    collection_from_bytes(vm, class, &handler.to_ne_bytes())
}

/// The semaphore index registered for a signal. Out-of-range signal numbers
/// were an unchecked array read in the C; here they fail cleanly.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveSemaIndexFor(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let sig_num = vm.stack_integer(0)?;
    let index = sema_index_for(sig_num).ok_or(PrimErr::BadArgument)?;
    Ok(sqint_of(index))
}

/// Where the SIGCHLD reaper signals; answers the index it stored.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveSetSemaIndex(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(1)?;
    let index = vm.stack_integer(0)?;
    Ok(set_sig_chld_sema_index(index))
}

// ===========================================================================
// Child processes
// ===========================================================================

/// waitpid(WNOHANG). No reapable child answers nil; otherwise {pid. status}.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveReapChildProcess(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(1)?;
    let pid_to_handle = vm.stack_integer(0)? as libc::pid_t;
    let mut exit_status: c_int = 0;
    // SAFETY: waitpid never touches memory beyond the status out-parameter.
    let pid_result = unsafe { libc::waitpid(pid_to_handle, &mut exit_status, libc::WNOHANG) };
    if pid_result <= 0 {
        return vm.nil();
    }
    let array = vm.instantiate(vm.class_array()?, 2)?;
    st_object_at_put(vm, array, 1, vm.integer(pid_result as sqInt)?)?;
    st_object_at_put(vm, array, 2, vm.integer(sqint_of(exit_status))?)?;
    Ok(array)
}

/// Registers pids to be signalled when the VM exits. A nil signum keeps the
/// SIGTERM default; a non-integer signum empties the list but still succeeds
/// (that is what the C's failure path amounted to).
#[pharo_primitive(accessor_depth = 1)]
fn primitiveKillOnExit(vm: &Interp) -> PrimResult<()> {
    vm.expect_argument_count(2)?;
    let pids = vm.stack_value(1)?;
    let signum = vm.stack_value(0)?;
    if !vm.is_pointers(pids)? {
        // The C ran stSizeOf/firstIndexableField on whatever arrived; a
        // non-array fails cleanly here instead.
        return Err(PrimErr::BadArgument);
    }
    let count = usize::try_from(st_size_of(vm, pids)?)?;
    let base = first_indexable_field(vm, pids)?.cast::<sqInt>();
    let mut list = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: i < slot count of a pointers object; slots are sqInt oops.
        // (The C read one element past the end here; see README.)
        let oop = unsafe { base.add(i).read() };
        list.push(integer_value_raw(vm, Oop(oop))? as libc::pid_t);
    }
    set_kill_list(list);
    if signum != vm.nil()? {
        if vm.is_integer_object(signum)? {
            set_signal_to_send(vm.integer_value(signum)?);
        } else {
            clear_kill_list();
        }
    }
    Ok(())
}

/// The self-test primitive for the pointer fixing; answers the (mutated)
/// string buffer.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveFixPointersInArrayOfStrings(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(3)?;
    let _count = vm.stack_integer(0)?; // read for validation, unused: as in C
    let offsets = stack_object_value(vm, 1)?;
    let strings = stack_object_value(vm, 2)?;
    fix_image_pointers(vm, strings, offsets)?;
    Ok(strings)
}

/// `fixPointersInArrayOfStrings:withOffsets:` against image objects: gathers
/// the untagged offsets, then rewrites the buffer's leading pointer slots.
fn fix_image_pointers(
    vm: &Interp,
    flattened: Oop,
    offsets: Oop,
) -> PrimResult<*mut *mut c_char> {
    if !vm.is_bytes(flattened)? || !vm.is_pointers(offsets)? {
        return Err(PrimErr::BadArgument);
    }
    let count = usize::try_from(st_size_of(vm, offsets)?)?;
    let offsets_base = first_indexable_field(vm, offsets)?.cast::<sqInt>();
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        // SAFETY: i < slot count; raw untag mirrors the C's integerValueOf.
        let oop = unsafe { offsets_base.add(i).read() };
        values.push(integer_value_raw(vm, Oop(oop))?);
    }
    let size = usize::try_from(vm.byte_size_of(flattened)?)?;
    let base = first_indexable_field(vm, flattened)?.cast::<u8>();
    // SAFETY: base/size describe the byte object's contents; nothing
    // allocates while the pointer is in use.
    unsafe { spawn::fix_pointers(base, size, &values) }.map_err(|_| PrimErr::BadArgument)
}

/// The stack layout `primitiveForkExec` / `primitiveForkAndExecInDirectory`
/// share: 0 workingDir, 1 envOffsets, 2 envVecBuffer, 3 argOffsets,
/// 4 argVecBuffer, 5 stdErr, 6 stdOut, 7 stdIn, 8 executableFile.
fn fork_and_exec_in_directory(vm: &Interp, use_signal_handler: bool) -> PrimResult<sqInt> {
    if vm.argument_count()? != 9 {
        return Err(PrimErr::BadNumArgs);
    }
    if use_signal_handler {
        // Armed before anything can fail, as in the C.
        set_sig_chld_handler();
    }
    let read = |offset: sqInt| stack_object_value(vm, offset).map_err(|_| PrimErr::BadArgument);
    let working_dir = read(0)?;
    let env_offsets = read(1)?;
    let env_vec_buffer = read(2)?;
    let arg_offsets = read(3)?;
    let arg_vec_buffer = read(4)?;
    let std_err = read(5)?;
    let std_out = read(6)?;
    let std_in = read(7)?;
    let executable_file = read(8)?;
    let nil = vm.nil()?;

    // Everything the child needs is computed before the fork; the pointers
    // aim into image memory, which the fork snapshots for the child. The C
    // (under vfork) ran these checks in the child and reported through the
    // shared failure flag; here a validation failure fails the primitive
    // without creating a child at all -- see the README.
    let nul_terminated_bytes = |oop: Oop| -> PrimResult<*const c_char> {
        if !vm.is_bytes(oop)? || !vm.bytes_of(oop)?.contains(&0) {
            return Err(PrimErr::BadArgument);
        }
        Ok(first_indexable_field(vm, oop)?.cast::<c_char>().cast_const())
    };
    let working_dir_ptr = if working_dir == nil {
        std::ptr::null()
    } else {
        nul_terminated_bytes(working_dir)?
    };
    let program = nul_terminated_bytes(executable_file)?;
    let fd_for = |oop: Oop| -> PrimResult<c_int> {
        if oop == nil {
            Ok(-1)
        } else {
            file_descriptor_from(vm, oop)
        }
    };
    let fds = ChildFds {
        stderr_fd: fd_for(std_err)?,
        stdout_fd: fd_for(std_out)?,
        stdin_fd: fd_for(std_in)?,
    };
    let envp = if env_vec_buffer == nil {
        let vec = spawn::vm_environment_vector_for_exec();
        if vec.is_null() {
            return Err(PrimErr::BadArgument);
        }
        vec.cast_const()
    } else {
        fix_image_pointers(vm, env_vec_buffer, env_offsets)?.cast_const()
    };
    let argv = fix_image_pointers(vm, arg_vec_buffer, arg_offsets)?.cast_const();

    let spec = ExecSpec {
        program,
        argv,
        envp,
        working_dir: working_dir_ptr,
        fds,
        restore_handlers: true,
    };
    // SAFETY: every pointer in the spec was validated above and aims either
    // into image memory (stable: nothing allocates between here and the
    // fork) or at the VM's own environment vector.
    let pid = unsafe { fork_and_exec(&spec) };
    Ok(pid as sqInt)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveForkAndExecInDirectory(vm: &Interp) -> PrimResult<sqInt> {
    fork_and_exec_in_directory(vm, true)
}

#[pharo_primitive(accessor_depth = 1)]
fn primitiveForkExec(vm: &Interp) -> PrimResult<sqInt> {
    fork_and_exec_in_directory(vm, false)
}

/// Exported for the VM/other plugins, exactly as the C exported it.
#[no_mangle]
pub extern "C" fn forkSqueak(useSignalHandler: sqInt) -> libc::pid_t {
    fork_squeak(useSignalHandler != 0)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveForkSqueak(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(fork_squeak(true) as sqInt)
}

#[pharo_primitive(accessor_depth = -1)]
fn primitiveForkSqueakWithoutSigHandler(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    Ok(fork_squeak(false) as sqInt)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three names that must agree: the macro's literal, this constant,
    /// and what the exported getModuleName answers.
    #[test]
    fn module_name_matches_export() {
        let p = getModuleName();
        let s = unsafe { std::ffi::CStr::from_ptr(p) };
        assert_eq!(s.to_str().unwrap(), MODULE_NAME);
        assert!(MODULE_NAME.starts_with("UnixOSProcessPlugin"));
    }

    #[test]
    fn mode_digits_split_like_the_c() {
        assert_eq!(mode_digits(0o4755), [4, 7, 5, 5]);
        assert_eq!(mode_digits(0o0644), [0, 6, 4, 4]);
        assert_eq!(mode_digits(0o7777), [7, 7, 7, 7]);
        assert_eq!(mode_digits(0), [0, 0, 0, 0]);
    }

    /// A compile-time check dressed as a test: the two type ascriptions below
    /// are what say `flock`'s lock fields really are `c_short` on this
    /// target. If a platform ever widened them, the `F_*_SHORT` constants
    /// would be narrowing something that fits and this stops building --
    /// which is the failure mode worth catching, because at run time a
    /// truncated `l_type` is just a lock request the kernel rejects.
    #[test]
    fn flock_lock_fields_are_c_short() {
        let lock = lock_struct(F_WRLCK_SHORT, 7, 16);
        let l_type: libc::c_short = lock.l_type;
        let l_whence: libc::c_short = lock.l_whence;
        assert_eq!(l_type, F_WRLCK_SHORT);
        assert_eq!(l_whence, libc::SEEK_SET as libc::c_short);
        assert_eq!(lock.l_start, 7);
        assert_eq!(lock.l_len, 16);
        assert_eq!(lock.l_pid, 0);
    }

    /// The lock-type numbers this build hands the image in slot 3 of
    /// `primitiveTestLockableFileRegion`. They are the platform's own, as the
    /// C's were -- Darwin numbers RDLCK/UNLCK/WRLCK 1/2/3 where glibc numbers
    /// them 0/2/1 -- so the same conflicting lock answers a different integer
    /// on a Mac. Pinned so that stays a decision on record rather than a
    /// surprise during the image-side pass.
    #[test]
    fn lock_type_numbers_are_the_platforms_own() {
        let triple = (F_RDLCK_SHORT, F_UNLCK_SHORT, F_WRLCK_SHORT);
        if cfg!(target_vendor = "apple") {
            assert_eq!(triple, (1, 2, 3));
        } else {
            assert_eq!(triple, (0, 2, 1));
        }
    }

    /// End to end through `fcntl`, because the widths only matter once the
    /// kernel sees them: take an exclusive lock, have a *child* probe it with
    /// `F_GETLK` (a process never conflicts with its own locks, and record
    /// locks are not inherited across `fork`), and check the `l_type` the
    /// child reads back is the one slot 3 would report. A wrong-width
    /// `l_type` reaches Darwin as 0, which is not a lock type there, and the
    /// `F_SETLK` below fails outright.
    #[test]
    fn lock_struct_round_trips_through_fcntl() {
        use std::os::unix::io::AsRawFd;

        let _guard = crate::signals::SIGNAL_TEST_LOCK.lock().unwrap();

        let path = std::env::temp_dir().join(format!("uosp-flock-{}", std::process::id()));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("temp file");
        let fd = file.as_raw_fd();

        let lock = lock_struct(F_WRLCK_SHORT, 0, 16);
        assert_eq!(
            unsafe { libc::fcntl(fd, libc::F_SETLK, &lock) },
            0,
            "exclusive lock taken: {}",
            std::io::Error::last_os_error()
        );

        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork");
        if pid == 0 {
            let mut probe = lock_struct(F_WRLCK_SHORT, 0, 16);
            let rc = unsafe { libc::fcntl(fd, libc::F_GETLK, &mut probe) };
            let answer = if rc == 0 { probe.l_type as c_int } else { -1 };
            // SAFETY: async-signal-safe exit from a forked child.
            unsafe { libc::_exit(answer) };
        }
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(
            libc::WEXITSTATUS(status),
            F_WRLCK_SHORT as c_int,
            "the blocking lock's l_type is what the image reads from slot 3"
        );

        let unlock = lock_struct(F_UNLCK_SHORT, 0, 16);
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETLK, &unlock) }, 0);
        drop(file);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn errno_helpers_round_trip() {
        clear_errno();
        assert_eq!(errno(), 0);
        // Provoke a real error and observe it.
        let fd = unsafe { libc::open(c"/definitely/not/here".as_ptr(), libc::O_RDONLY) };
        assert_eq!(fd, -1);
        assert_eq!(errno(), libc::ENOENT);
    }
}
