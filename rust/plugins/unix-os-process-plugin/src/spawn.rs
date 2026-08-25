//! Child-process machinery: pipes, the argv/env pointer fixing, fork+exec,
//! and access to the VM's argument/environment vectors.
//!
//! The image pre-flattens argv and env into single byte buffers -- a row of
//! pointer-sized slots followed by the NUL-terminated strings -- precisely so
//! that nothing here needs to allocate after the fork. The child-side path in
//! [`fork_and_exec`] is a straight line of async-signal-safe calls: `chdir`,
//! `fflush`/`dup2`, `close`, `sigaction`, `execve`, `_exit`.

use std::ffi::{c_char, c_int, c_void};
use std::mem::size_of;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::signals::restore_original_handlers;
use crate::support::CachedFn;

// ---------------------------------------------------------------------------
// Pipes
// ---------------------------------------------------------------------------

/// `createPipeForReader:writer:` -- a pipe(2) wrapped in stdio streams, the
/// reader `fdopen`ed `"r"` and the writer `"a"` as the C did. Answers
/// (reader, writer), or `None` when `pipe()` fails.
///
/// Note the C never checked the `fdopen` results; a null stream would land in
/// the SQFile record. This port fails the pipe instead (memory safety).
pub fn create_pipe() -> Option<(*mut libc::FILE, *mut libc::FILE)> {
    let mut fds = [0 as c_int; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } == -1 {
        return None;
    }
    let writer = unsafe { libc::fdopen(fds[1], c"a".as_ptr()) };
    let reader = unsafe { libc::fdopen(fds[0], c"r".as_ptr()) };
    if reader.is_null() || writer.is_null() {
        unsafe {
            if reader.is_null() {
                libc::close(fds[0]);
            } else {
                libc::fclose(reader);
            }
            if writer.is_null() {
                libc::close(fds[1]);
            } else {
                libc::fclose(writer);
            }
        }
        return None;
    }
    Some((reader, writer))
}

// ---------------------------------------------------------------------------
// fixPointersInArrayOfStrings:withOffsets:
// ---------------------------------------------------------------------------

/// Why the pointer fixing refused a buffer.
#[derive(Debug, PartialEq, Eq)]
pub enum FixPointersError {
    /// The pointer table (offsets plus its NULL terminator slot) does not fit
    /// in the buffer, or an offset points outside it.
    BadArgument,
}

/// `fixPointersInArrayOfStrings:withOffsets:` -- turns the image's flattened
/// string buffer into a C `char *[]` by writing absolute pointers into its
/// leading slots. Answers the buffer head as `char **`.
///
/// `offsets` are the already-untagged integers from the image's offset array.
/// Checks, as the C did: the slot table must fit (`count * sizeof(char *) <
/// size`), every offset must fall inside the buffer, and the slot after the
/// last written one must already be NULL. One divergence: the C read that
/// terminator slot without checking it fit in the buffer -- an out-of-bounds
/// read of up to `sizeof(char *) - 1` bytes when the image under-sized the
/// buffer; this port requires the terminator slot to fit and fails otherwise.
///
/// # Safety
///
/// `base` must point at `size` writable bytes, pointer-aligned (object memory
/// data is), and unaliased for the duration of the call.
pub unsafe fn fix_pointers(
    base: *mut u8,
    size: usize,
    offsets: &[isize],
) -> Result<*mut *mut c_char, FixPointersError> {
    let count = offsets.len();
    let slot = size_of::<*mut c_char>();
    if count * slot >= size {
        return Err(FixPointersError::BadArgument);
    }
    // The terminator slot the C read blindly; see above.
    if (count + 1) * slot > size {
        return Err(FixPointersError::BadArgument);
    }
    let table = base.cast::<*mut c_char>();
    for (idx, &val) in offsets.iter().enumerate() {
        if (val as usize) >= size {
            return Err(FixPointersError::BadArgument);
        }
        // SAFETY: idx < count, and the whole table fits per the checks above.
        unsafe { table.add(idx).write(base.cast::<c_char>().offset(val)) };
    }
    // SAFETY: the terminator slot is in bounds per the check above.
    if !unsafe { table.add(count).read() }.is_null() {
        return Err(FixPointersError::BadArgument);
    }
    Ok(table)
}

// ---------------------------------------------------------------------------
// The VM's argument and environment vectors
// ---------------------------------------------------------------------------

static IO_GET_ENV_VEC: CachedFn = CachedFn::new();
static GET_PROCESS_ENV_VEC: CachedFn = CachedFn::new();
static GET_PROCESS_ARG_COUNT: CachedFn = CachedFn::new();
static GET_PROCESS_ARG_VEC: CachedFn = CachedFn::new();

/// `getEnvironmentVector`'s cache -- only ever holds a vector the VM handed
/// out (those are captured at startup and never reallocated). The
/// no-VM fallback below is deliberately not cached: libc's environment can be
/// reallocated by `putenv`, so a stale pointer would dangle.
static ENV_VEC: AtomicPtr<*mut c_char> = AtomicPtr::new(std::ptr::null_mut());

/// The VM's environment vector, or null when no VM export resolves.
///
/// Resolution order mirrors the C `getEnvironmentVector`: the main module's
/// `ioGetEnvVec` if it exports one, else the core's
/// `getProcessEnvironmentVector` (which the C reached at link time and this
/// port loads through the proxy).
fn vm_environment_vector() -> *mut *mut c_char {
    let cached = ENV_VEC.load(Ordering::Acquire);
    if !cached.is_null() {
        return cached;
    }
    let mut vec: *mut *mut c_char = std::ptr::null_mut();
    let f = IO_GET_ENV_VEC.get("ioGetEnvVec", "");
    if !f.is_null() {
        // SAFETY: ioGetEnvVec has the C signature `char **(void)`.
        let f: unsafe extern "C" fn() -> *mut *mut c_char = unsafe { std::mem::transmute(f) };
        vec = unsafe { f() };
    }
    if vec.is_null() {
        let f = GET_PROCESS_ENV_VEC.get("getProcessEnvironmentVector", "");
        if !f.is_null() {
            // SAFETY: same shape, exported by src/utils.c.
            let f: unsafe extern "C" fn() -> *mut *mut c_char = unsafe { std::mem::transmute(f) };
            vec = unsafe { f() };
        }
    }
    if !vec.is_null() {
        ENV_VEC.store(vec, Ordering::Release);
    }
    vec
}

/// The environment entry at 1-based `index`, copied out, or `None` past the
/// end -- `environmentAtAsType:`'s counting loop, factored out. When no VM is
/// present (tests), falls back to the live process environment.
pub fn environment_string_at(index: isize) -> Option<Vec<u8>> {
    if index < 1 {
        return None;
    }
    let vec = vm_environment_vector();
    if !vec.is_null() {
        // SAFETY: a NULL-terminated vector of NUL-terminated strings the VM
        // captured at startup; entries are copied before returning.
        unsafe {
            let mut count = 0isize;
            while !(*vec.offset(count)).is_null() {
                count += 1;
            }
            if index > count {
                return None;
            }
            return Some(crate::support::cstr_bytes(*vec.offset(index - 1)).to_vec());
        }
    }
    // No VM: rebuild "KEY=value" entries from the live environment, in
    // vector order.
    use std::os::unix::ffi::OsStrExt;
    let (key, value) = std::env::vars_os().nth(index as usize - 1)?;
    let mut entry = key.as_bytes().to_vec();
    entry.push(b'=');
    entry.extend_from_slice(value.as_bytes());
    Some(entry)
}

/// The VM's environment vector for a child's envp, as
/// `forkAndExecInDirectory:` used it when the image passed nil buffers. Null
/// when the VM exports neither accessor.
pub fn vm_environment_vector_for_exec() -> *mut *mut c_char {
    vm_environment_vector()
}

/// Whether any environment vector is reachable at all -- the C's
/// `p == null -> primitiveFail()` case. Only a VM-less, environment-less
/// process answers false.
pub fn have_environment_vector() -> bool {
    !vm_environment_vector().is_null() || std::env::vars_os().next().is_some()
}

/// The VM's argv entry at 1-based `index`, copied out.
///
/// Outer `None`: the VM does not export the accessors (the C plugin would
/// have failed to link at all). Inner `None`: index out of range, which the
/// primitive answers as `nil`.
pub fn argument_at(index: isize) -> Option<Option<Vec<u8>>> {
    let count_fn = GET_PROCESS_ARG_COUNT.get("getProcessArgumentCount", "");
    let vec_fn = GET_PROCESS_ARG_VEC.get("getProcessArgumentVector", "");
    if count_fn.is_null() || vec_fn.is_null() {
        return None;
    }
    // SAFETY: `int getProcessArgumentCount(void)` and
    // `char **getProcessArgumentVector(void)` from src/utils.c.
    let count_fn: unsafe extern "C" fn() -> c_int = unsafe { std::mem::transmute(count_fn) };
    let vec_fn: unsafe extern "C" fn() -> *mut *mut c_char = unsafe { std::mem::transmute(vec_fn) };
    let count = unsafe { count_fn() } as isize;
    let vec = unsafe { vec_fn() };
    if index < 1 || index > count || vec.is_null() {
        return Some(None);
    }
    // SAFETY: index validated against the VM's own count; copied immediately.
    Some(Some(unsafe {
        crate::support::cstr_bytes(*vec.offset(index - 1)).to_vec()
    }))
}

// ---------------------------------------------------------------------------
// fork + exec
// ---------------------------------------------------------------------------

/// Target descriptors for the child's stdio; -1 leaves a stream untouched
/// (both "the image passed nil" and "the handle failed validation" -- the C
/// skipped the dup in both cases).
#[derive(Clone, Copy)]
pub struct ChildFds {
    pub stdin_fd: c_int,
    pub stdout_fd: c_int,
    pub stderr_fd: c_int,
}

/// Everything the child needs, gathered before the fork so the child itself
/// performs only async-signal-safe calls.
pub struct ExecSpec {
    /// NUL-terminated program path.
    pub program: *const c_char,
    /// NULL-terminated argv, from [`fix_pointers`].
    pub argv: *const *mut c_char,
    /// NULL-terminated envp.
    pub envp: *const *mut c_char,
    /// NUL-terminated working directory, or null for "stay put".
    pub working_dir: *const c_char,
    pub fds: ChildFds,
    /// Restore the pre-forwarding signal handlers before exec (the primitive
    /// always does; tests may skip to leave the test process's state alone).
    pub restore_handlers: bool,
}

/// Writes a diagnostic to fd 2 with nothing but `write`(2) -- the child's
/// stand-in for the C's `logErrorFromErrno`.
fn child_complain(what: &[u8], detail: *const c_char) {
    unsafe {
        libc::write(2, what.as_ptr().cast::<c_void>(), what.len());
        if !detail.is_null() {
            let len = libc::strlen(detail);
            libc::write(2, detail.cast::<c_void>(), len);
        }
        libc::write(2, b"\n".as_ptr().cast::<c_void>(), 1);
    }
}

/// One stream's `dupToStd*:` step: skip when absent or already on target,
/// otherwise flush the stream about to be replaced and `dup2` over it.
fn dup_onto(fd: c_int, target: c_int, stream: *mut libc::FILE) {
    if fd < 0 || fd == target {
        return;
    }
    unsafe {
        libc::fflush(stream);
        libc::dup2(fd, target);
    }
}

/// `forkAndExecInDirectory:`'s fork half. Answers the child pid (or -1) in
/// the parent; the child never returns -- it execs or `_exit(-1)`s.
///
/// The C used `vfork()`; Rust cannot host a returns-twice call soundly, so
/// this is `fork()` -- the README spells out the observable differences.
/// The real-interval timer is disabled across the fork and restored in the
/// parent, exactly as the C did, so the child does not inherit a ticking
/// ITIMER_REAL it cannot handle.
///
/// # Safety
///
/// Every pointer in `spec` must be valid and NUL-/NULL-terminated as
/// documented on [`ExecSpec`]; the buffers must stay put until the call
/// returns (the fork snapshots them for the child).
pub unsafe fn fork_and_exec(spec: &ExecSpec) -> libc::pid_t {
    let off: libc::itimerval = unsafe { std::mem::zeroed() };
    let mut saved: libc::itimerval = unsafe { std::mem::zeroed() };
    unsafe { libc::setitimer(libc::ITIMER_REAL, &off, &mut saved) };

    let pid = unsafe { libc::fork() };
    if pid != 0 {
        // Parent (or failed fork): re-enable the timer, report the pid --
        // including -1, which the C also pushed as the answer.
        unsafe { libc::setitimer(libc::ITIMER_REAL, &saved, std::ptr::null_mut()) };
        return pid;
    }

    // Child. Only async-signal-safe territory from here to exec.
    if !spec.working_dir.is_null() && unsafe { libc::chdir(spec.working_dir) } != 0 {
        child_complain(b"chdir: ", spec.working_dir);
        unsafe { libc::_exit(-1) };
    }
    // The C's order: stderr, then stdout, then stdin (with the rewind).
    dup_onto(spec.fds.stderr_fd, 2, crate::sqfile::stderr_stream());
    dup_onto(spec.fds.stdout_fd, 1, crate::sqfile::stdout_stream());
    let stdin_fd = spec.fds.stdin_fd;
    if stdin_fd > 0 {
        unsafe {
            libc::fflush(crate::sqfile::stdin_stream());
            libc::dup2(stdin_fd, 0);
            libc::rewind(crate::sqfile::stdin_stream());
        }
    }
    // Close everything but stdio, so pipes into the dead parent's other
    // children do not linger.
    let limit = unsafe { libc::getdtablesize() } - 1;
    for fd in 3..=limit {
        unsafe { libc::close(fd) };
    }
    if spec.restore_handlers {
        restore_original_handlers();
    }
    unsafe { libc::execve(spec.program, spec.argv.cast(), spec.envp.cast()) };
    child_complain(b"execve: ", spec.program);
    unsafe { libc::_exit(-1) };
}

/// `forkSqueak:` -- plain fork with the interval timer masked across it; both
/// sides keep running the image. The C's ordering, kept: timer off, then the
/// optional SIGCHLD handler, then the fork, then the timer back on.
pub fn fork_squeak(use_signal_handler: bool) -> libc::pid_t {
    let off: libc::itimerval = unsafe { std::mem::zeroed() };
    let mut saved: libc::itimerval = unsafe { std::mem::zeroed() };
    unsafe {
        libc::setitimer(libc::ITIMER_REAL, &off, &mut saved);
        if use_signal_handler {
            crate::signals::set_sig_chld_handler();
        }
        let pid = libc::fork();
        libc::setitimer(libc::ITIMER_REAL, &saved, std::ptr::null_mut());
        pid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    /// Builds the image's flattened-buffer format: `slots` pointer-sized NULL
    /// slots (one extra for the terminator), then the strings, answering the
    /// buffer and the offsets the image would send.
    fn flatten(strings: &[&str]) -> (Vec<u8>, Vec<isize>) {
        let slot = size_of::<*mut c_char>();
        let head = (strings.len() + 1) * slot;
        let mut buf = vec![0u8; head];
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(buf.len() as isize);
            buf.extend_from_slice(s.as_bytes());
            buf.push(0);
        }
        (buf, offsets)
    }

    #[test]
    fn fix_pointers_builds_a_c_vector() {
        let (mut buf, offsets) = flatten(&["/bin/echo", "hello", "world"]);
        let table =
            unsafe { fix_pointers(buf.as_mut_ptr(), buf.len(), &offsets) }.expect("valid buffer");
        for (i, expected) in ["/bin/echo", "hello", "world"].iter().enumerate() {
            let p = unsafe { table.add(i).read() };
            let s = unsafe { std::ffi::CStr::from_ptr(p) };
            assert_eq!(s.to_str().unwrap(), *expected);
        }
        assert!(unsafe { table.add(3).read() }.is_null(), "NULL-terminated");
    }

    #[test]
    fn fix_pointers_rejects_bad_shapes() {
        // Offset beyond the buffer.
        let (mut buf, _) = flatten(&["x"]);
        let huge = vec![buf.len() as isize + 10];
        assert_eq!(
            unsafe { fix_pointers(buf.as_mut_ptr(), buf.len(), &huge) },
            Err(FixPointersError::BadArgument)
        );

        // Slot table alone fills the buffer: no room for strings.
        let mut tiny = vec![0u8; size_of::<*mut c_char>()];
        let offs = vec![0isize];
        let len = tiny.len();
        assert_eq!(
            unsafe { fix_pointers(tiny.as_mut_ptr(), len, &offs) },
            Err(FixPointersError::BadArgument)
        );

        // Missing NULL terminator slot: the C read past the buffer here; we
        // reject.
        let slot = size_of::<*mut c_char>();
        let mut no_term = vec![0u8; slot + 2];
        no_term[slot] = b'x';
        no_term[slot + 1] = 0;
        let offs = vec![slot as isize];
        let len = no_term.len();
        assert_eq!(
            unsafe { fix_pointers(no_term.as_mut_ptr(), len, &offs) },
            Err(FixPointersError::BadArgument)
        );

        // Non-null terminator slot.
        let (mut buf, offsets) = flatten(&["a", "b"]);
        buf[size_of::<*mut c_char>() * 2] = 1; // corrupt the terminator
        let len = buf.len();
        assert_eq!(
            unsafe { fix_pointers(buf.as_mut_ptr(), len, &offsets) },
            Err(FixPointersError::BadArgument)
        );
    }

    #[test]
    fn pipe_round_trips_bytes() {
        let (reader, writer) = create_pipe().expect("pipe");
        let msg = b"through the pipe";
        unsafe {
            assert_eq!(libc::fwrite(msg.as_ptr().cast(), 1, msg.len(), writer), msg.len());
            libc::fflush(writer);
            let mut back = [0u8; 16];
            assert_eq!(libc::fread(back.as_mut_ptr().cast(), 1, msg.len(), reader), msg.len());
            assert_eq!(&back, msg);
            libc::fclose(writer);
            libc::fclose(reader);
        }
    }

    #[test]
    fn fork_and_exec_runs_echo_through_the_image_buffer_format() {
        // stdout of the child goes into a pipe we read; argv and env travel
        // through the same flattened-buffer + fix_pointers path the primitive
        // uses.
        let mut pipe_fds = [0 as c_int; 2];
        assert_eq!(unsafe { libc::pipe(pipe_fds.as_mut_ptr()) }, 0);

        let (mut argbuf, argoffs) = flatten(&["/bin/echo", "forked", "fine"]);
        let argv =
            unsafe { fix_pointers(argbuf.as_mut_ptr(), argbuf.len(), &argoffs) }.expect("argv");
        let (mut envbuf, envoffs) = flatten(&["OSPP_TEST=1"]);
        let envp =
            unsafe { fix_pointers(envbuf.as_mut_ptr(), envbuf.len(), &envoffs) }.expect("envp");

        let spec = ExecSpec {
            program: c"/bin/echo".as_ptr(),
            argv: argv.cast_const(),
            envp: envp.cast_const(),
            working_dir: c"/".as_ptr(),
            fds: ChildFds {
                stdin_fd: -1,
                stdout_fd: pipe_fds[1],
                stderr_fd: -1,
            },
            restore_handlers: false,
        };
        let pid = unsafe { fork_and_exec(&spec) };
        assert!(pid > 0, "fork succeeded");
        unsafe { libc::close(pipe_fds[1]) };

        let mut out = String::new();
        let mut reader = unsafe { std::fs::File::from_raw_fd(pipe_fds[0]) };
        reader.read_to_string(&mut out).expect("read child output");
        assert_eq!(out, "forked fine\n");

        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
    }

    #[test]
    fn fork_and_exec_missing_program_exits_255() {
        let (mut argbuf, argoffs) = flatten(&["/no/such/binary"]);
        let argv =
            unsafe { fix_pointers(argbuf.as_mut_ptr(), argbuf.len(), &argoffs) }.expect("argv");
        let (mut envbuf, envoffs) = flatten(&[]);
        let envp =
            unsafe { fix_pointers(envbuf.as_mut_ptr(), envbuf.len(), &envoffs) }.expect("envp");

        // Route the child's complaint away from the test output.
        let devnull = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_WRONLY) };
        assert!(devnull >= 0);

        let spec = ExecSpec {
            program: c"/no/such/binary".as_ptr(),
            argv: argv.cast_const(),
            envp: envp.cast_const(),
            working_dir: std::ptr::null(),
            fds: ChildFds {
                stdin_fd: -1,
                stdout_fd: -1,
                stderr_fd: devnull,
            },
            restore_handlers: false,
        };
        let pid = unsafe { fork_and_exec(&spec) };
        assert!(pid > 0);
        unsafe { libc::close(devnull) };
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        // _exit(-1) surfaces as 255: the C's exec-failure signature.
        assert_eq!(libc::WEXITSTATUS(status), 255);
    }

    #[test]
    fn environment_vector_falls_back_to_environ() {
        // No VM in a test process, so the proxy lookups fail and the libc
        // fallback must carry: the vector exists and PATH is in it somewhere
        // (set in any sane test environment).
        std::env::set_var("OSPP_ENV_PROBE", "yes");
        let mut index = 1;
        let mut found = false;
        while let Some(entry) = environment_string_at(index) {
            if entry.starts_with(b"OSPP_ENV_PROBE=") {
                found = true;
                break;
            }
            index += 1;
        }
        assert!(found, "the probe variable is visible through the vector");
        assert!(environment_string_at(0).is_none(), "index is 1-based");
        assert!(environment_string_at(isize::MAX).is_none());
    }
}
