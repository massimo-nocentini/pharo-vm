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
use core::sync::atomic::{fence, AtomicBool, AtomicI32, AtomicI64, Ordering};

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
/// by the VM thread.
#[repr(C)]
#[derive(Clone, Copy)]
struct SignalRequest {
    requests: c_int,
    responses: c_int,
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
static mut SIGNAL_REQUESTS: *mut SignalRequest = core::ptr::null_mut();

/// How many entries [`SIGNAL_REQUESTS`] has.
static mut NUM_SIGNAL_REQUESTS: c_int = 0;

/// Set when at least one request is pending. `sqInt`-sized, as in the C.
static CHECK_SIGNAL_REQUESTS: AtomicI64 = AtomicI64::new(0);

/// The empty interval the unused tide pair is reset to.
const MAX_TIDE: i32 = (u32::MAX >> 1) as i32;
/// See [`MAX_TIDE`].
const MIN_TIDE: i32 = -1;

/// Which tide pair signallers are currently widening.
static USE_TIDE_A: AtomicBool = AtomicBool::new(true);
static LOW_TIDE_A: AtomicI32 = AtomicI32::new(MAX_TIDE);
static HIGH_TIDE_A: AtomicI32 = AtomicI32::new(MIN_TIDE);
static LOW_TIDE_B: AtomicI32 = AtomicI32::new(MAX_TIDE);
static HIGH_TIDE_B: AtomicI32 = AtomicI32::new(MIN_TIDE);

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

/// Takes the request mutex.
fn lock_requests() {
    // SAFETY: requestMutex is set by ioInitExternalSemaphores before any
    // signaller can run, and its vtable slots are always filled.
    unsafe {
        let mutex = requestMutex;
        if let Some(wait) = (*mutex).wait {
            wait(mutex);
        }
    }
}

/// Releases the request mutex.
fn unlock_requests() {
    // SAFETY: as above.
    unsafe {
        let mutex = requestMutex;
        if let Some(signal) = (*mutex).signal {
            signal(mutex);
        }
    }
}

/// The size of the request table.
#[no_mangle]
pub extern "C" fn ioGetMaxExtSemTableSize() -> c_int {
    // SAFETY: read on the VM thread; only grown at start-up.
    unsafe { NUM_SIGNAL_REQUESTS }
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
    // SAFETY: delegated to the caller.
    unsafe {
        if NUM_SIGNAL_REQUESTS >= n {
            return;
        }

        // `1 << highBit(n - 1)`, which rounds up to a power of two.
        let sz = 1i64 << highBit((n - 1) as pharo_vm_sys::usqInt);
        let sz = sz as c_int;
        debug_assert!(sz >= n, "the rounded size must not be smaller");

        let entry = core::mem::size_of::<SignalRequest>();
        let grown = libc::realloc(SIGNAL_REQUESTS.cast::<c_void>(), sz as usize * entry)
            .cast::<SignalRequest>();
        // The C did not check realloc either; a null here would take the
        // memset below down with it.
        SIGNAL_REQUESTS = grown;

        core::ptr::write_bytes(
            SIGNAL_REQUESTS.add(NUM_SIGNAL_REQUESTS as usize),
            0,
            (sz - NUM_SIGNAL_REQUESTS) as usize,
        );
        NUM_SIGNAL_REQUESTS = sz;
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

    let blocked = blocked_signal_set();

    debug_assert!(
        // SAFETY: read on any thread; only grown at start-up.
        index >= 0 && index <= unsafe { NUM_SIGNAL_REQUESTS } as sqInt,
        "external semaphore index out of range"
    );

    // SAFETY: NUM_SIGNAL_REQUESTS is only grown at start-up.
    if i < 0 || i >= unsafe { NUM_SIGNAL_REQUESTS } as sqInt {
        return 0;
    }
    let i = i as usize;

    block_signals(&blocked);
    lock_requests();

    fence(Ordering::SeqCst);

    // SAFETY: `i` is in range and the mutex is held, so no other signaller is
    // touching this entry; the VM thread only writes `responses`.
    unsafe {
        let entry = SIGNAL_REQUESTS.add(i);
        (*entry).requests += 1;
    }

    let index_i32 = i as i32;
    if USE_TIDE_A.load(Ordering::Relaxed) {
        if LOW_TIDE_A.load(Ordering::Relaxed) > index_i32 {
            LOW_TIDE_A.store(index_i32, Ordering::Relaxed);
        }
        if HIGH_TIDE_A.load(Ordering::Relaxed) < index_i32 {
            HIGH_TIDE_A.store(index_i32, Ordering::Relaxed);
        }
    } else {
        if LOW_TIDE_B.load(Ordering::Relaxed) > index_i32 {
            LOW_TIDE_B.store(index_i32, Ordering::Relaxed);
        }
        if HIGH_TIDE_B.load(Ordering::Relaxed) < index_i32 {
            HIGH_TIDE_B.store(index_i32, Ordering::Relaxed);
        }
    }

    CHECK_SIGNAL_REQUESTS.store(1, Ordering::Relaxed);
    // SAFETY: tells the interpreter to take its interrupt check soon.
    unsafe { pharo_vm_sys::forceInterruptCheck() };

    unlock_requests();
    unblock_signals(&blocked);

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
    let blocked = blocked_signal_set();
    block_signals(&blocked);
    lock_requests();

    fence(Ordering::SeqCst);
    if CHECK_SIGNAL_REQUESTS.load(Ordering::Relaxed) == 0 {
        unlock_requests();
        unblock_signals(&blocked);
        return 0;
    }

    let mut switched: sqInt = 0;
    CHECK_SIGNAL_REQUESTS.store(0, Ordering::Relaxed);

    fence(Ordering::SeqCst);
    // Reset the pair that is about to become unused, flip, then read the pair
    // that was in use. See the module docs on why the order matters.
    let (low_tide, high_tide) = if USE_TIDE_A.load(Ordering::Relaxed) {
        LOW_TIDE_B.store(MAX_TIDE, Ordering::Relaxed);
        HIGH_TIDE_B.store(MIN_TIDE, Ordering::Relaxed);
        USE_TIDE_A.store(false, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        (
            LOW_TIDE_A.load(Ordering::Relaxed),
            HIGH_TIDE_A.load(Ordering::Relaxed),
        )
    } else {
        LOW_TIDE_A.store(MAX_TIDE, Ordering::Relaxed);
        HIGH_TIDE_A.store(MIN_TIDE, Ordering::Relaxed);
        USE_TIDE_A.store(true, Ordering::Relaxed);
        fence(Ordering::SeqCst);
        (
            LOW_TIDE_B.load(Ordering::Relaxed),
            HIGH_TIDE_B.load(Ordering::Relaxed),
        )
    };
    fence(Ordering::SeqCst);

    unlock_requests();

    let mut high_tide = high_tide;
    if high_tide as sqInt >= external_semaphore_table_size {
        high_tide = (external_semaphore_table_size - 1) as i32;
    }

    // SAFETY: every index walked lies inside the table: `low_tide` is only
    // ever set to an index a signaller validated, and `high_tide` is clamped
    // above.
    unsafe {
        let mut i = low_tide;
        while i <= high_tide {
            let entry = SIGNAL_REQUESTS.add(i as usize);
            while (*entry).responses != (*entry).requests {
                if pharo_vm_sys::doSignalSemaphoreWithIndex((i + 1) as sqInt) != 0 {
                    switched = 1;
                }
                (*entry).responses += 1;
            }
            i += 1;
        }
    }

    unblock_signals(&blocked);

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
