//! Replaces `src/common/sqExternalSemaphores.c` on Unix.
//!
//! Lets any thread signal a Smalltalk semaphore. The VM cannot be re-entered
//! from another thread, so a signal is *recorded* here and acted on later, on
//! the VM thread, during its interrupt check.
//!
//! # The request table
//!
//! One `SignalRequest { requests, responses }` per external semaphore index.
//! A signaller increments `requests`; the VM thread increments `responses`
//! until the two match, running the image's semaphore once per step. Counting
//! rather than flagging is what stops two signals arriving close together from
//! collapsing into one.
//!
//! The table is grown by `realloc` in [`ioSetMaxExtSemTableSize`], which is
//! only safe before other threads exist -- the C's comment says so, and the
//! intended use is to size it once at start-up from the image header.
//!
//! # The tide marks
//!
//! Scanning thousands of indices on every interrupt check would be wasteful,
//! so each signaller widens a low/high watermark pair. There are two pairs.
//! The VM thread resets the *unused* pair to an empty interval, flips
//! `useTideA`, and only then reads the pair that was in use -- so a signaller
//! racing the flip writes into the pair that is about to be read next time
//! round, and no request is lost. That is why there are two, and why the
//! fences around the flip matter.
//!
//! # Signals are blocked around the critical section
//!
//! `SIGCHLD`, `SIGINT`, `SIGSTOP` and `SIGTSTP` are masked while the mutex is
//! held, so a handler cannot run and try to signal a semaphore while this
//! thread owns the lock. `SIGSTOP` cannot actually be blocked -- `sigaddset`
//! accepts it and `sigprocmask` ignores it -- but it is in the C's set and it
//! is in this one.
//!
//! # Memory ordering
//!
//! The C used `volatile` plus `sqLowLevelMFence()`, which is
//! `__sync_synchronize()`, a full barrier. `volatile` in C says nothing about
//! atomicity, so the C is relying on the fences and on these being
//! naturally-aligned word-sized loads and stores. The Rust says what the C
//! meant: the shared variables are atomics accessed with `Relaxed`, and there
//! is a `SeqCst` fence at each point the C had `sqLowLevelMFence()`. That
//! compiles to the same instructions and is defined behaviour rather than
//! merely working.
//!
//! # Faithful oddities
//!
//! * `ioSetMaxExtSemTableSize` rounds the request up to a power of two with
//!   `1 << highBit(n-1)`, so asking for 257 allocates 512. Asking for 0 or 1
//!   reaches `highBit(-1)`, which the C declared as taking an `int`.
//! * The C declared `int highBit(int)` locally while the generated
//!   `cointerp.h` declares `usqInt highBit(usqInt)`. The declaration used for
//!   the call was the wrong one; it happens to work on x86-64 because the
//!   argument is passed in the low half of a register that the compiler
//!   zero-extends, and the result never exceeds 63. The correct signature is
//!   used here, which is identical for every reachable input.
//! * `doSignalExternalSemaphores` clamps `highTide` to the table size the
//!   *image* believes in, which is a different number from
//!   `numSignalRequests`. A request above that clamp stays pending until
//!   something else widens the tide.

use core::ffi::{c_int, c_void};
use core::sync::atomic::{fence, AtomicBool, AtomicI32, AtomicI64, AtomicPtr, Ordering};

use pharo_vm_sys::{sqInt, Semaphore};

use crate::platform_semaphore::platform_semaphore_new;

extern "C" {
    /// Position of the highest set bit. Declared here because the generated
    /// `cointerp.h` is not on `wrapper.h`'s include path; the signature is
    /// that header's, not the C file's. See the module docs.
    fn highBit(value: pharo_vm_sys::usqInt) -> pharo_vm_sys::usqInt;
    /// Wakes the poll loop so a signal is noticed promptly. Declared inside
    /// `sqExternalSemaphores.c` in the C, for want of a common header between
    /// the Unix and Windows aio implementations.
    fn aioInterruptPoll();
}

/// One external semaphore's counters.
///
/// `requests` is incremented by signallers under the mutex; `responses` only
/// by the VM thread. Atomics because signaller and VM thread genuinely access
/// an entry concurrently -- the C relied on `volatile` word accesses; the
/// atomics say the same thing in defined terms, at the same cost.
#[repr(C)]
struct SignalRequest {
    requests: AtomicI32,
    responses: AtomicI32,
}

/// The VM's own thread, set by `vm_init` in [`crate::client`] on worker-thread
/// builds. Exported because `#if !COGMTVM` puts its definition in this file.
#[no_mangle]
pub static mut ioVMThread: libc::pthread_t = 0;

/// The mutex guarding the request table and the tides.
///
/// A [`Semaphore`] rather than a `pthread_mutex_t` because the FFI code holds
/// the same kind of handle; created with a count of 1.
#[no_mangle]
pub static mut requestMutex: *mut Semaphore = core::ptr::null_mut();

/// The request table. Null until [`ioInitExternalSemaphores`] runs.
static SIGNAL_REQUESTS: AtomicPtr<SignalRequest> = AtomicPtr::new(core::ptr::null_mut());

/// How many entries [`SIGNAL_REQUESTS`] has.
static NUM_SIGNAL_REQUESTS: AtomicI32 = AtomicI32::new(0);

/// Set when at least one request is pending. `sqInt`-sized, as in the C.
static CHECK_SIGNAL_REQUESTS: AtomicI64 = AtomicI64::new(0);

/// The empty interval the unused tide pair is reset to.
const MAX_TIDE: i32 = (u32::MAX >> 1) as i32;
/// See [`MAX_TIDE`].
const MIN_TIDE: i32 = -1;

/// One low/high watermark pair. See the module docs on why there are two.
struct TidePair {
    low: AtomicI32,
    high: AtomicI32,
}

impl TidePair {
    /// A pair holding the empty interval.
    const fn empty() -> Self {
        Self {
            low: AtomicI32::new(MAX_TIDE),
            high: AtomicI32::new(MIN_TIDE),
        }
    }

    /// Widens the interval to include `index`.
    ///
    /// `fetch_min`/`fetch_max` where the C loaded, compared and stored; under
    /// the request mutex the observable behaviour is identical.
    fn widen(&self, index: i32) {
        self.low.fetch_min(index, Ordering::Relaxed);
        self.high.fetch_max(index, Ordering::Relaxed);
    }

    /// Resets the interval to empty.
    fn reset(&self) {
        self.low.store(MAX_TIDE, Ordering::Relaxed);
        self.high.store(MIN_TIDE, Ordering::Relaxed);
    }

    /// Reads the interval as `(low, high)`.
    fn read(&self) -> (i32, i32) {
        (
            self.low.load(Ordering::Relaxed),
            self.high.load(Ordering::Relaxed),
        )
    }
}

/// The two watermark pairs, A then B.
static TIDES: [TidePair; 2] = [TidePair::empty(), TidePair::empty()];

/// Which tide pair signallers are currently widening.
static USE_TIDE_A: AtomicBool = AtomicBool::new(true);

/// The pair signallers are widening right now.
fn active_tide() -> &'static TidePair {
    if USE_TIDE_A.load(Ordering::Relaxed) {
        &TIDES[0]
    } else {
        &TIDES[1]
    }
}

/// `INITIAL_EXT_SEM_TABLE_SIZE` from `sq.h`.
const INITIAL_EXT_SEM_TABLE_SIZE: c_int = 256;

/// The signals masked while the mutex is held. See the module docs.
fn blocked_signal_set() -> libc::sigset_t {
    // SAFETY: sigemptyset initialises the set before anything reads it, and
    // every sigaddset takes a valid signal number.
    unsafe {
        let mut set = core::mem::zeroed::<libc::sigset_t>();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGCHLD);
        libc::sigaddset(&mut set, libc::SIGINT);
        libc::sigaddset(&mut set, libc::SIGSTOP);
        libc::sigaddset(&mut set, libc::SIGTSTP);
        set
    }
}

/// Blocks the set for the duration of a critical section.
fn block_signals(set: &libc::sigset_t) {
    // SAFETY: a valid set, and a null oldset asks not to be told the previous
    // mask -- which is why this pairs with SIG_UNBLOCK rather than a restore.
    unsafe {
        libc::sigprocmask(libc::SIG_BLOCK, set, core::ptr::null_mut());
    }
}

/// Undoes [`block_signals`].
///
/// `SIG_UNBLOCK` rather than restoring the saved mask, exactly as the C did.
/// A signal that was already blocked by the caller before entering here is
/// therefore *unblocked* on the way out.
fn unblock_signals(set: &libc::sigset_t) {
    // SAFETY: as above.
    unsafe {
        libc::sigprocmask(libc::SIG_UNBLOCK, set, core::ptr::null_mut());
    }
}

/// Masks [`blocked_signal_set`] for the guard's lifetime.
///
/// The drop runs `SIG_UNBLOCK`, with the caveat documented on
/// [`unblock_signals`].
struct SignalBlock {
    set: libc::sigset_t,
}

impl SignalBlock {
    fn new() -> Self {
        let set = blocked_signal_set();
        block_signals(&set);
        Self { set }
    }
}

impl Drop for SignalBlock {
    fn drop(&mut self) {
        unblock_signals(&self.set);
    }
}

/// Holds the request mutex for the guard's lifetime.
///
/// Declared *after* a [`SignalBlock`] so that drop order releases the mutex
/// first and unmasks the signals second, as the C's explicit sequence did --
/// and so that every return path releases both, which the C repeated by hand
/// at each early return.
struct RequestLock;

impl RequestLock {
    fn acquire() -> Self {
        // SAFETY: requestMutex is set by ioInitExternalSemaphores before any
        // signaller can run, and its vtable slots are always filled.
        unsafe {
            let mutex = requestMutex;
            if let Some(wait) = (*mutex).wait {
                wait(mutex);
            }
        }
        Self
    }
}

impl Drop for RequestLock {
    fn drop(&mut self) {
        // SAFETY: as in acquire.
        unsafe {
            let mutex = requestMutex;
            if let Some(signal) = (*mutex).signal {
                signal(mutex);
            }
        }
    }
}

/// The size of the request table.
#[no_mangle]
pub extern "C" fn ioGetMaxExtSemTableSize() -> c_int {
    NUM_SIGNAL_REQUESTS.load(Ordering::Relaxed)
}

/// Grows the request table to at least `n` entries, rounded up to a power of
/// two.
///
/// Only safe before other threads exist: the `realloc` copies the old contents
/// and then swaps the pointer, so a request arriving during the copy can be
/// lost. The C's comment says the same, and says the intended use is to size
/// the table once at start-up from a value in the image header.
///
/// # Safety
///
/// Must run on the VM thread with no signaller active.
#[no_mangle]
pub unsafe extern "C" fn ioSetMaxExtSemTableSize(n: c_int) {
    let old = NUM_SIGNAL_REQUESTS.load(Ordering::Relaxed);
    if old >= n {
        return;
    }

    // `1 << highBit(n - 1)`, which rounds up to a power of two.
    // SAFETY: highBit takes and answers a plain integer.
    let sz = 1i64 << unsafe { highBit((n - 1) as pharo_vm_sys::usqInt) };
    let sz = sz as c_int;
    debug_assert!(sz >= n, "the rounded size must not be smaller");

    let entry = core::mem::size_of::<SignalRequest>();
    // SAFETY: delegated to the caller -- no signaller is active -- and the
    // grown tail is zeroed before the new count is published.
    unsafe {
        let old_table = SIGNAL_REQUESTS.load(Ordering::Relaxed);
        let grown =
            libc::realloc(old_table.cast::<c_void>(), sz as usize * entry).cast::<SignalRequest>();
        if grown.is_null() {
            // The C did not check realloc and would have memset through the
            // null; dying cleanly is the safe spelling of the same outcome.
            libc::abort();
        }
        core::ptr::write_bytes(grown.add(old as usize), 0, (sz - old) as usize);
        SIGNAL_REQUESTS.store(grown, Ordering::Relaxed);
        NUM_SIGNAL_REQUESTS.store(sz, Ordering::Relaxed);
    }
}

/// Sizes the request table and creates the mutex.
///
/// # Safety
///
/// Must run once, on the VM thread, before any other thread can signal.
#[no_mangle]
pub unsafe extern "C" fn ioInitExternalSemaphores() {
    // SAFETY: delegated to the caller.
    unsafe {
        ioSetMaxExtSemTableSize(INITIAL_EXT_SEM_TABLE_SIZE);
        requestMutex = platform_semaphore_new(1);
    }
}

/// Records a signal for the external semaphore at `index`. Answers 1 when the
/// request was recorded and 0 when it was out of range.
///
/// Index 0 is silently ignored, as the image uses it for "no semaphore".
/// Callable from any thread; that is the whole point of the file.
///
/// # Safety
///
/// [`ioInitExternalSemaphores`] must have run.
#[no_mangle]
pub unsafe extern "C" fn signalSemaphoreWithIndex(index: sqInt) -> sqInt {
    let i = index - 1;

    debug_assert!(
        index >= 0 && index <= NUM_SIGNAL_REQUESTS.load(Ordering::Relaxed) as sqInt,
        "external semaphore index out of range"
    );

    if i < 0 || i >= NUM_SIGNAL_REQUESTS.load(Ordering::Relaxed) as sqInt {
        return 0;
    }
    let i = i as usize;

    {
        let _signals = SignalBlock::new();
        let _lock = RequestLock::acquire();

        fence(Ordering::SeqCst);

        // SAFETY: `i` is in range, and the entry's counters are atomics.
        let entry = unsafe { &*SIGNAL_REQUESTS.load(Ordering::Relaxed).add(i) };
        entry.requests.fetch_add(1, Ordering::Relaxed);

        active_tide().widen(i as i32);

        CHECK_SIGNAL_REQUESTS.store(1, Ordering::Relaxed);
        // SAFETY: tells the interpreter to take its interrupt check soon.
        unsafe { pharo_vm_sys::forceInterruptCheck() };
    } // Mutex released, then signals unmasked.

    // SAFETY: wakes the poll loop so the check happens promptly.
    unsafe { aioInterruptPoll() };

    1
}

/// Whether any signal is waiting to be delivered.
#[no_mangle]
pub extern "C" fn isPendingSemaphores() -> c_int {
    CHECK_SIGNAL_REQUESTS.load(Ordering::Relaxed) as c_int
}

/// Delivers every pending signal. Answers whether a process switch happened.
///
/// `external_semaphore_table_size` is the image's idea of the table size,
/// which is not `numSignalRequests`; clamping to it here saves a bounds check
/// per delivery, at the cost of leaving a higher request pending.
///
/// # Safety
///
/// Must run on the VM thread. [`ioInitExternalSemaphores`] must have run.
#[no_mangle]
pub unsafe extern "C" fn doSignalExternalSemaphores(external_semaphore_table_size: sqInt) -> sqInt {
    let _signals = SignalBlock::new();
    let lock = RequestLock::acquire();

    fence(Ordering::SeqCst);
    if CHECK_SIGNAL_REQUESTS.load(Ordering::Relaxed) == 0 {
        // The guards release the mutex and unmask, in that order.
        return 0;
    }

    let mut switched: sqInt = 0;
    CHECK_SIGNAL_REQUESTS.store(0, Ordering::Relaxed);

    fence(Ordering::SeqCst);
    // Reset the pair that is about to become unused, flip, then read the pair
    // that was in use. See the module docs on why the order matters.
    let use_a = USE_TIDE_A.load(Ordering::Relaxed);
    let (in_use, becoming_unused) = if use_a {
        (&TIDES[0], &TIDES[1])
    } else {
        (&TIDES[1], &TIDES[0])
    };
    becoming_unused.reset();
    USE_TIDE_A.store(!use_a, Ordering::Relaxed);
    fence(Ordering::SeqCst);
    let (low_tide, high_tide) = in_use.read();
    fence(Ordering::SeqCst);

    // The mutex is released before the delivery loop, but the signals stay
    // masked until after it -- the C's exact sequence.
    drop(lock);

    let mut high_tide = high_tide;
    if high_tide as sqInt >= external_semaphore_table_size {
        high_tide = (external_semaphore_table_size - 1) as i32;
    }

    let table = SIGNAL_REQUESTS.load(Ordering::Relaxed);
    for i in low_tide..=high_tide {
        // SAFETY: every index walked lies inside the table: `low_tide` is
        // only ever set to an index a signaller validated, and `high_tide` is
        // clamped above.
        let entry = unsafe { &*table.add(i as usize) };
        while entry.responses.load(Ordering::Relaxed) != entry.requests.load(Ordering::Relaxed) {
            // SAFETY: an interpreter entry point, on the VM thread.
            if unsafe { pharo_vm_sys::doSignalSemaphoreWithIndex((i + 1) as sqInt) } != 0 {
                switched = 1;
            }
            entry.responses.fetch_add(1, Ordering::Relaxed);
        }
    }

    switched
}

/// Blocks the calling Smalltalk process on the external semaphore at
/// `semaphore_index`.
///
/// # Safety
///
/// Must run on the VM thread, inside a primitive.
#[no_mangle]
pub unsafe extern "C" fn waitOnExternalSemaphoreIndex(semaphore_index: sqInt) {
    // SAFETY: delegated to the caller; both calls are interpreter entry
    // points and the oop is used immediately.
    unsafe {
        let a_semaphore_oop = pharo_vm_sys::getExternalSemaphoreWithIndex(semaphore_index);
        pharo_vm_sys::doWaitSemaphore(a_semaphore_oop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Is `sigprocmask` per-thread on this platform?
    ///
    /// The critical section above masks signals with `sigprocmask`, not
    /// `pthread_sigmask`, because the C did. POSIX says the behaviour of
    /// `sigprocmask` "is unspecified in a multi-threaded process", and until
    /// async DNS landed this process had no threads to speak of, so the
    /// question never had to be answered. It does now: every asynchronous
    /// capability `CLAUDE.md` still wants -- the DNS workers, a job pool, an
    /// AsyncPlugin -- signals from a foreign thread and therefore runs this
    /// code on a thread that is not the VM's.
    ///
    /// If `sigprocmask` applied process-wide, then a signaller's critical
    /// section would mask `SIGINT`, `SIGCHLD` and `SIGTSTP` on the *VM*
    /// thread for its duration, and `SIG_UNBLOCK` on the way out would unmask
    /// them there whether or not the VM thread had wanted them blocked -- the
    /// heartbeat, the JIT and `unix-os-process-plugin`'s `SIGCHLD` handling
    /// all care.
    ///
    /// So assert what this platform actually does rather than what POSIX
    /// declines to say. Both glibc and Darwin implement `sigprocmask` as
    /// per-thread (glibc routes it to `rt_sigprocmask` on the calling thread;
    /// Darwin's is documented as equivalent to `pthread_sigmask`), which is
    /// what makes the C's choice harmless. A platform where this fails is a
    /// platform where `block_signals` must be switched to `pthread_sigmask`
    /// before anything else signals from a worker.
    #[test]
    fn sigprocmask_is_per_thread() {
        let set = blocked_signal_set();

        // Nothing of ours is blocked to begin with on this thread.
        // SAFETY: a null `set` with any `how` only reads the current mask.
        let before = unsafe {
            let mut current = core::mem::zeroed::<libc::sigset_t>();
            libc::sigprocmask(libc::SIG_BLOCK, core::ptr::null(), &mut current);
            current
        };
        // SAFETY: `before` was filled by sigprocmask; SIGINT is a valid signal.
        assert_eq!(
            unsafe { libc::sigismember(&before, libc::SIGINT) },
            0,
            "the test thread starts with SIGINT unblocked"
        );

        // Another thread runs the same critical-section masking this file
        // does, and holds it while we look.
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _signals = SignalBlock::new();
            done_tx.send(()).expect("the test thread is still listening");
            rx.recv().expect("the test thread releases us");
        });
        done_rx.recv().expect("the worker masked its signals");

        // SAFETY: as above.
        let during = unsafe {
            let mut current = core::mem::zeroed::<libc::sigset_t>();
            libc::sigprocmask(libc::SIG_BLOCK, core::ptr::null(), &mut current);
            current
        };
        // SAFETY: `during` was filled by sigprocmask.
        assert_eq!(
            unsafe { libc::sigismember(&during, libc::SIGINT) },
            0,
            "a signaller's SIG_BLOCK reached this thread: sigprocmask is \
             process-wide here, and block_signals must use pthread_sigmask"
        );

        tx.send(()).expect("the worker is still waiting");
        worker.join().expect("the worker did not panic");

        // And the worker's SIG_UNBLOCK on the way out did not unmask anything
        // here either -- the direction that would actually break the VM, since
        // `unblock_signals` unblocks rather than restoring a saved mask.
        // SAFETY: as above.
        let after = unsafe {
            let mut current = core::mem::zeroed::<libc::sigset_t>();
            libc::sigprocmask(libc::SIG_BLOCK, core::ptr::null(), &mut current);
            current
        };
        for signal in [libc::SIGCHLD, libc::SIGINT, libc::SIGTSTP] {
            // SAFETY: `before`/`after` were filled by sigprocmask.
            assert_eq!(
                unsafe { libc::sigismember(&before, signal) },
                unsafe { libc::sigismember(&after, signal) },
                "signal {signal} changed on this thread because another \
                 thread ran the critical section"
            );
        }

        // `blocked_signal_set` is what the critical section masks; naming it
        // here keeps this test tied to that set rather than to a copy.
        // SAFETY: `set` was filled by sigemptyset/sigaddset.
        assert_eq!(unsafe { libc::sigismember(&set, libc::SIGINT) }, 1);
    }

    /// `SIGSTOP` is in the C's set and in this one, and `sigprocmask` ignores
    /// it. Pinned so that "we mask SIGSTOP" is never read as "SIGSTOP is
    /// blocked".
    #[test]
    fn sigstop_is_in_the_set_and_cannot_be_blocked() {
        let set = blocked_signal_set();
        // SAFETY: `set` was filled by sigemptyset/sigaddset.
        assert_eq!(
            unsafe { libc::sigismember(&set, libc::SIGSTOP) },
            1,
            "sigaddset accepts SIGSTOP, as it did in the C"
        );

        let _signals = SignalBlock::new();
        // SAFETY: a null `set` only reads the current mask.
        let current = unsafe {
            let mut current = core::mem::zeroed::<libc::sigset_t>();
            libc::sigprocmask(libc::SIG_BLOCK, core::ptr::null(), &mut current);
            current
        };
        // SAFETY: `current` was filled by sigprocmask.
        assert_eq!(
            unsafe { libc::sigismember(&current, libc::SIGSTOP) },
            0,
            "and sigprocmask ignores it, so it is never actually blocked"
        );
    }
}
