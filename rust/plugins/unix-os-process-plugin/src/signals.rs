//! Signal-to-semaphore forwarding, SIGCHLD reaping, and the sigaltstack
//! machinery -- the delicate part of this plugin.
//!
//! The C kept two `NSIG + 1` arrays: `semaIndices` (which Smalltalk semaphore
//! a signal forwards to) and `originalSigHandlers` (what to restore on
//! unregister/shutdown/exec). Both are read from signal handlers, so the port
//! stores them as atomics -- an atomic load is async-signal-safe, a plain
//! `static mut` read is a data race.
//!
//! Handler bodies mirror the C's exactly, including the parts that are only
//! de-facto safe: `signalSemaphoreWithIndex` is called from the handler, as
//! every Squeak/Pharo VM has always done (the VM's implementation is written
//! for that), and it is flagged in the README rather than redesigned.

use std::ffi::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicI32, AtomicI8, AtomicIsize, AtomicU8, AtomicUsize, Ordering};

use pharo_vm_plugin::sqInt;

use crate::support::io_load_function;

/// `NSIG` as the platform's signal.h defines it: 65 on Linux (real-time
/// signals included), 32 on macOS. The C sized its arrays `NSIG + 1` and
/// looped `1..=NSIG`; so does this port.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub const NSIG: usize = 32;
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub const NSIG: usize = 65;

/// `SIG_ERR`, the error sentinel `forwardSignal:toSemaphoreAt:` answers.
pub const SIG_ERR_VALUE: usize = usize::MAX; // (void *)-1

// A `const` item as the repeat operand is the pre-1.79 idiom for arrays of
// atomics.
#[allow(clippy::declare_interior_mutable_const)]
const ZERO_U8: AtomicU8 = AtomicU8::new(0);
#[allow(clippy::declare_interior_mutable_const)]
const ZERO_USIZE: AtomicUsize = AtomicUsize::new(0);

/// `semaIndices`: semaphore index registered per signal; 0 = unregistered.
/// An `unsigned char` array in the C, so indices silently truncate to 8 bits
/// -- reproduced (and called out in the README).
static SEMA_INDICES: [AtomicU8; NSIG + 1] = [ZERO_U8; NSIG + 1];

/// `originalSigHandlers`: the handler that was in place before we installed
/// forwarding, restored on unregister, shutdown, and before exec.
static ORIGINAL_HANDLERS: [AtomicUsize; NSIG + 1] = [ZERO_USIZE; NSIG + 1];

/// `sigChldSemaIndex`: where the SIGCHLD reaper handler signals.
static SIG_CHLD_SEMA_INDEX: AtomicIsize = AtomicIsize::new(0);

/// `vmThread`: the pthread the interpreter runs on, captured at module load.
static VM_THREAD: AtomicUsize = AtomicUsize::new(0);

/// `useSignalStack`: -1 undecided, 0 no, 1 yes. Decided once by
/// [`need_sigaltstack`], never revisited -- which is what keeps the signal
/// handler path free of the lookup and the malloc.
static USE_SIGNAL_STACK: AtomicI8 = AtomicI8::new(-1);

/// Test seam: when nonzero, [`signal_semaphore_with_index`] calls this
/// `fn(sqInt)` instead of the proxy. Production never sets it.
static SEMAPHORE_TAP: AtomicUsize = AtomicUsize::new(0);

/// Records the interpreter thread. Called from `initialiseModule`, which the
/// VM runs on the interpreter thread.
pub fn note_vm_thread() {
    VM_THREAD.store(unsafe { libc::pthread_self() } as usize, Ordering::Release);
    USE_SIGNAL_STACK.store(-1, Ordering::Release);
}

/// The recorded interpreter pthread, for `primitiveGetThreadID`.
pub fn vm_thread() -> libc::pthread_t {
    VM_THREAD.load(Ordering::Acquire) as libc::pthread_t
}

/// `isVmThread` -- async-signal-safe: pthread_self/pthread_equal only.
fn is_vm_thread() -> bool {
    let this = unsafe { libc::pthread_self() };
    unsafe { libc::pthread_equal(this, vm_thread()) != 0 }
}

/// The registered semaphore index for `sig_num`, or `None` when the signal
/// number cannot index the arrays (the C indexed unchecked; bounds-checking
/// is the port's memory-safety addition).
pub fn sema_index_for(sig_num: sqInt) -> Option<u8> {
    let idx = usize::try_from(sig_num).ok()?;
    if idx > NSIG {
        return None;
    }
    Some(SEMA_INDICES[idx].load(Ordering::Relaxed))
}

/// `sigChldSemaIndex` accessors for `primitiveSetSemaIndex`.
pub fn set_sig_chld_sema_index(index: sqInt) -> sqInt {
    SIG_CHLD_SEMA_INDEX.store(index, Ordering::Relaxed);
    index
}

// ---------------------------------------------------------------------------
// Semaphore signaling (handler context)
// ---------------------------------------------------------------------------

/// Signals a Smalltalk semaphore through the proxy. Called from signal
/// handlers, exactly as the C plugin (and the wider VM) always has.
fn signal_semaphore_with_index(index: sqInt) {
    let tap = SEMAPHORE_TAP.load(Ordering::Acquire);
    if tap != 0 {
        // SAFETY: the tap is only ever set (by tests) to a `fn(sqInt)`.
        let f: fn(sqInt) = unsafe { std::mem::transmute::<usize, fn(sqInt)>(tap) };
        f(index);
        return;
    }
    let vt = pharo_vm_plugin::__private::INTERP.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: process-lifetime proxy table; the entry is the VM's own
    // semaphore-signaling primitive, designed to be called from handlers.
    if let Some(f) = unsafe { (*vt).signalSemaphoreWithIndex } {
        unsafe { f(index) };
    }
}

/// Installs the test tap. Test-only.
#[cfg(test)]
pub fn set_semaphore_tap(f: fn(sqInt)) {
    SEMAPHORE_TAP.store(f as usize, Ordering::Release);
}

#[cfg(test)]
pub fn clear_semaphore_tap() {
    SEMAPHORE_TAP.store(0, Ordering::Release);
}

// ---------------------------------------------------------------------------
// sigaltstack
// ---------------------------------------------------------------------------

/// `needSigaltstack`: whether handlers must run on an alternate stack (the
/// JIT's native stack cannot take signal frames), allocating one on first
/// need. Decided once; the cached fast path is the only one a signal handler
/// can reach.
pub fn need_sigaltstack() -> bool {
    let cached = USE_SIGNAL_STACK.load(Ordering::Acquire);
    if cached >= 0 {
        return cached != 0;
    }

    // The JIT is detected the way the C did it: os_exports' GetAttributeString
    // answering non-null for attribute 1008.
    let gas = io_load_function("GetAttributeString", "os_exports");
    if gas.is_null() {
        USE_SIGNAL_STACK.store(0, Ordering::Release);
        return false;
    }
    // SAFETY: GetAttributeString has the C signature `char *(int)`.
    let gas: unsafe extern "C" fn(c_int) -> *mut c_char = unsafe { std::mem::transmute(gas) };
    if unsafe { gas(1008) }.is_null() {
        USE_SIGNAL_STACK.store(0, Ordering::Release);
        return false;
    }

    // Same ordering as the C: commit to "yes" first, downgrade on failure.
    USE_SIGNAL_STACK.store(1, Ordering::Release);

    // Reuse an existing alternate stack when one is installed and enabled.
    let mut existing: libc::stack_t = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaltstack(std::ptr::null(), &mut existing) } < 0 {
        log_errno("sigaltstack");
    }
    if !(existing.ss_size == 0 || (existing.ss_flags & libc::SS_DISABLE) != 0) {
        return true;
    }

    let wanted = 1024 * std::mem::size_of::<*const c_void>() * 16;
    let size = wanted.max(libc::MINSIGSTKSZ);
    let sp = unsafe { libc::malloc(size) };
    if sp.is_null() {
        log_msg("sigstack malloc failed");
        USE_SIGNAL_STACK.store(0, Ordering::Release);
        return false;
    }
    let stack = libc::stack_t {
        ss_sp: sp,
        ss_flags: 0,
        ss_size: size,
    };
    if unsafe { libc::sigaltstack(&stack, std::ptr::null_mut()) } < 0 {
        log_msg("sigaltstack install failed");
        unsafe { libc::free(sp) };
        USE_SIGNAL_STACK.store(0, Ordering::Release);
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Installing handlers
// ---------------------------------------------------------------------------

/// `setSignalNumber:handler:` -- `signal()` when no alternate stack is in
/// play, `sigaction` with `SA_ONSTACK | SA_RESTART` when one is. Answers the
/// prior handler address, or [`SIG_ERR_VALUE`].
///
/// Divergence: on `sigaction` failure the C returned the uninitialized
/// `oldHandlerAction.sa_sigaction` (an uninitialized-memory read); this port
/// answers `SIG_ERR` instead, which is what callers already test for.
pub fn set_signal_handler(sig_num: c_int, handler: usize) -> usize {
    if !need_sigaltstack() {
        return unsafe { libc::signal(sig_num, handler) };
    }
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = handler;
    action.sa_flags = libc::SA_ONSTACK | libc::SA_RESTART;
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if unsafe { libc::sigaction(sig_num, &action, &mut old) } != 0 {
        log_errno("signal");
        return SIG_ERR_VALUE;
    }
    old.sa_sigaction
}

/// `forwardSignal:toSemaphoreAt:` -- the registration state machine.
///
/// * `semaphore_index == 0` unregisters: restores the saved handler, clears
///   the slot, answers the restored handler; unregistering an unregistered
///   signal answers `SIG_ERR`.
/// * Registering an already-registered signal answers `SIG_ERR`.
/// * Otherwise installs [`handle_signal`], saves the prior handler, records
///   the index (truncated to `unsigned char`, as the C's array did).
///
/// Also reachable from [`handle_signal`] itself (mirroring the C); on that
/// path every branch is a cached read or an early return, so nothing
/// non-async-signal-safe runs.
pub fn forward_signal_to_semaphore(sig_num: sqInt, semaphore_index: sqInt) -> usize {
    let Ok(sig) = usize::try_from(sig_num) else {
        return SIG_ERR_VALUE;
    };
    if sig == 0 || sig > NSIG {
        // The C indexed its arrays with whatever the image sent; failing is
        // the memory-safe reading of the same situation.
        return SIG_ERR_VALUE;
    }
    if semaphore_index == 0 {
        if SEMA_INDICES[sig].load(Ordering::Relaxed) != 0 {
            let old = ORIGINAL_HANDLERS[sig].load(Ordering::Relaxed);
            let old = set_signal_handler(sig as c_int, old);
            SEMA_INDICES[sig].store(0, Ordering::Relaxed);
            return old;
        }
        return SIG_ERR_VALUE;
    }
    if SEMA_INDICES[sig].load(Ordering::Relaxed) > 0 {
        return SIG_ERR_VALUE;
    }
    let old = set_signal_handler(sig as c_int, handle_signal as *const () as usize);
    if old != SIG_ERR_VALUE {
        ORIGINAL_HANDLERS[sig].store(old, Ordering::Relaxed);
        SEMA_INDICES[sig].store(semaphore_index as u8, Ordering::Relaxed);
    }
    old
}

/// `handleSignal:` -- the forwarding handler. Async-signal-safe by
/// construction: atomic loads, pthread calls, and the proxy's semaphore
/// signal, exactly the C's footprint.
pub extern "C" fn handle_signal(sig_num: c_int) {
    let sig = sig_num as usize;
    if sig > NSIG {
        return;
    }
    // The C read the index into a plain (signed) char and re-invoked
    // forwardSignal:toSemaphoreAt: with it -- a no-op re-registration attempt
    // kept for fidelity (see README).
    let sema_index = SEMA_INDICES[sig].load(Ordering::Relaxed) as i8;
    forward_signal_to_semaphore(sig_num as sqInt, sqInt::from(sema_index));
    if is_vm_thread() {
        // The signed read above means indices 128..=255 never signal -- the
        // C's `char semaIndex` comparison, reproduced.
        if sema_index > 0 {
            signal_semaphore_with_index(sqInt::from(sema_index));
        }
    } else {
        mask_signal_for_this_thread(sig_num);
        resend_signal(sig_num);
    }
}

/// `maskSignalForThisThread:` -- blocks the signal for the delivering pthread
/// so the re-sent signal lands on the interpreter thread instead.
fn mask_signal_for_this_thread(sig_num: c_int) {
    let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig_num);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// `resendSignal:` -- redirects the signal at the interpreter thread.
fn resend_signal(sig_num: c_int) {
    unsafe { libc::pthread_kill(vm_thread(), sig_num) };
}

// ---------------------------------------------------------------------------
// SIGCHLD
// ---------------------------------------------------------------------------

/// `reapChildProcess:` behind the three-argument `sigaction` wrapper the C
/// used on `SA_NOCLDSTOP` platforms (every platform this plugin builds on).
extern "C" fn reap_child_wrapper(
    _sig_num: c_int,
    _info: *mut libc::siginfo_t,
    _ctx: *mut c_void,
) {
    let index = SIG_CHLD_SEMA_INDEX.load(Ordering::Relaxed);
    if index > 0 {
        signal_semaphore_with_index(index);
    }
}

/// `setSigChldHandler` -- SA_NODEFER | SA_NOCLDSTOP (| SA_ONSTACK when the
/// JIT wants the alternate stack), no re-arming needed.
pub fn set_sig_chld_handler() {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = reap_child_wrapper as *const () as usize;
    action.sa_flags = libc::SA_NODEFER | libc::SA_NOCLDSTOP | libc::SA_SIGINFO;
    if need_sigaltstack() {
        action.sa_flags |= libc::SA_ONSTACK;
    }
    unsafe { libc::sigemptyset(&mut action.sa_mask) };
    if unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } != 0 {
        log_errno("signal");
    }
}

// ---------------------------------------------------------------------------
// Restoration
// ---------------------------------------------------------------------------

/// `restoreDefaultSignalHandlers` -- puts back every handler this plugin
/// replaced. Run in the child before exec and at module shutdown. Leaves
/// `semaIndices` untouched, exactly as the C did.
pub fn restore_original_handlers() {
    for sig in 1..=NSIG {
        if SEMA_INDICES[sig].load(Ordering::Relaxed) > 0 {
            let old = ORIGINAL_HANDLERS[sig].load(Ordering::Relaxed);
            set_signal_handler(sig as c_int, old);
        }
    }
}

// ---------------------------------------------------------------------------
// kill-on-exit bookkeeping (atexit)
// ---------------------------------------------------------------------------

/// The signal `sendSignalToPids` delivers; SIGTERM unless the image chose
/// otherwise through `primitiveKillOnExit`.
static SIG_NUM_TO_SEND: AtomicI32 = AtomicI32::new(libc::SIGTERM);

/// The pids to signal at VM exit. A mutex (not touched from handlers): the
/// primitive writes it on the interpreter thread and `atexit` reads it during
/// normal process teardown.
static KILL_LIST: std::sync::Mutex<Vec<libc::pid_t>> = std::sync::Mutex::new(Vec::new());

/// Replaces the kill-on-exit pid list (the C freed and re-malloc'ed its
/// array).
pub fn set_kill_list(pids: Vec<libc::pid_t>) {
    if let Ok(mut guard) = KILL_LIST.lock() {
        *guard = pids;
    }
}

/// Empties the list, mirroring the C's `pidCount = 0` failure paths.
pub fn clear_kill_list() {
    set_kill_list(Vec::new());
}

pub fn set_signal_to_send(sig: sqInt) {
    SIG_NUM_TO_SEND.store(sig as c_int, Ordering::Relaxed);
}

/// `sendSignalToPids` -- the atexit hook registered by `initialiseModule`.
pub extern "C" fn send_signal_to_pids() {
    let sig = SIG_NUM_TO_SEND.load(Ordering::Relaxed);
    // try_lock: if the process is exiting mid-primitive the list is in flux;
    // skipping beats deadlocking inside atexit.
    if let Ok(guard) = KILL_LIST.try_lock() {
        for &pid in guard.iter() {
            unsafe { libc::kill(pid, sig) };
        }
    }
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// The C used the VM's logError/logTrace; a stand-alone cdylib writes to
/// stderr directly. Not for use inside signal handlers.
fn log_msg(msg: &str) {
    eprintln!("UnixOSProcessPlugin: {msg}");
}

fn log_errno(what: &str) {
    eprintln!(
        "UnixOSProcessPlugin: {what}: errno {}",
        std::io::Error::last_os_error()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicIsize;
    use std::sync::Mutex;

    /// Global-state tests must not interleave: registration slots, the tap
    /// and VM_THREAD are process-wide.
    static SIGNAL_TEST_LOCK: Mutex<()> = Mutex::new(());

    static LAST_SIGNALED: AtomicIsize = AtomicIsize::new(0);

    fn tap(index: sqInt) {
        LAST_SIGNALED.store(index, Ordering::SeqCst);
    }

    #[test]
    fn registration_state_machine() {
        let _guard = SIGNAL_TEST_LOCK.lock().unwrap();
        let sig = libc::SIGUSR2 as sqInt;

        // Unregistering an unregistered signal answers SIG_ERR.
        assert_eq!(forward_signal_to_semaphore(sig, 0), SIG_ERR_VALUE);

        let old = forward_signal_to_semaphore(sig, 9);
        assert_ne!(old, SIG_ERR_VALUE, "first registration succeeds");
        assert_eq!(sema_index_for(sig), Some(9));

        // A second registration is refused until the first is removed.
        assert_eq!(forward_signal_to_semaphore(sig, 4), SIG_ERR_VALUE);
        assert_eq!(sema_index_for(sig), Some(9));

        let restored = forward_signal_to_semaphore(sig, 0);
        assert_ne!(restored, SIG_ERR_VALUE);
        assert_eq!(sema_index_for(sig), Some(0));
    }

    #[test]
    fn out_of_range_signals_are_refused_not_indexed() {
        let _guard = SIGNAL_TEST_LOCK.lock().unwrap();
        assert_eq!(forward_signal_to_semaphore(-1, 3), SIG_ERR_VALUE);
        assert_eq!(forward_signal_to_semaphore(0, 3), SIG_ERR_VALUE);
        assert_eq!(
            forward_signal_to_semaphore(NSIG as sqInt + 1, 3),
            SIG_ERR_VALUE
        );
        assert_eq!(sema_index_for(NSIG as sqInt + 40), None);
    }

    #[test]
    fn raised_signal_reaches_the_stubbed_semaphore() {
        let _guard = SIGNAL_TEST_LOCK.lock().unwrap();
        // The delivering thread must count as the VM thread, or the handler
        // re-routes with pthread_kill instead of signaling.
        note_vm_thread();
        set_semaphore_tap(tap);
        LAST_SIGNALED.store(0, Ordering::SeqCst);

        let sig = libc::SIGUSR1 as sqInt;
        assert_ne!(forward_signal_to_semaphore(sig, 42), SIG_ERR_VALUE);
        unsafe { libc::raise(libc::SIGUSR1) };
        assert_eq!(LAST_SIGNALED.load(Ordering::SeqCst), 42);

        // Unregister restores the original disposition; a further raise must
        // not reach the tap.
        LAST_SIGNALED.store(0, Ordering::SeqCst);
        assert_ne!(forward_signal_to_semaphore(sig, 0), SIG_ERR_VALUE);
        clear_semaphore_tap();
        assert_eq!(LAST_SIGNALED.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sigchld_reaper_signals_registered_semaphore() {
        let _guard = SIGNAL_TEST_LOCK.lock().unwrap();
        note_vm_thread();
        set_semaphore_tap(tap);
        LAST_SIGNALED.store(0, Ordering::SeqCst);

        set_sig_chld_sema_index(17);
        set_sig_chld_handler();
        unsafe { libc::raise(libc::SIGCHLD) };
        assert_eq!(LAST_SIGNALED.load(Ordering::SeqCst), 17);

        // Put SIGCHLD back to default so later fork/wait tests are unaffected.
        set_sig_chld_sema_index(0);
        unsafe { libc::signal(libc::SIGCHLD, libc::SIG_DFL) };
        clear_semaphore_tap();
    }

    #[test]
    fn kill_list_replaces_and_clears() {
        set_kill_list(vec![1, 2, 3]);
        clear_kill_list();
        assert!(KILL_LIST.lock().unwrap().is_empty());
    }

    #[test]
    fn kill_on_exit_actually_signals() {
        // A paused child, killed through the atexit hook's own code path.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            // Child: wait to be killed.
            loop {
                unsafe { libc::pause() };
            }
        }
        set_kill_list(vec![pid]);
        set_signal_to_send(libc::SIGKILL as sqInt);
        send_signal_to_pids();
        let mut status = 0;
        let reaped = unsafe { libc::waitpid(pid, &mut status, 0) };
        assert_eq!(reaped, pid);
        assert!(libc::WIFSIGNALED(status));
        assert_eq!(libc::WTERMSIG(status), libc::SIGKILL);
        clear_kill_list();
        set_signal_to_send(libc::SIGTERM as sqInt);
    }
}
