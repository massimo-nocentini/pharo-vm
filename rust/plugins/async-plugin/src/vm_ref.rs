//! The one doorbell, reachable from a worker thread.
//!
//! Every primitive records the interpreter proxy on entry -- an atomic store of
//! a value that never changes after `setInterpreter` -- and the workers read it
//! back. `signalSemaphoreWithIndex` is the VM's designated any-thread entry
//! point: it takes `requestMutex` with signals masked, increments a per-index
//! request counter, widens a tide mark and asks for an interrupt check. It
//! reads no oop and enters no interpreter.
//!
//! # One signal per completion, deliberately not coalesced
//!
//! `rust/examples/ext-sem-soak` measured this path: mean 4.8 us per signal, and
//! an image that keeps up with 43 million of them over ten minutes without
//! losing one. It also measured where it breaks -- an *unpaced* signaller, one
//! with no work between signals, starves the interpreter outright, because
//! `signalSemaphoreWithIndex` calls `forceInterruptCheck` and `aioInterruptPoll`
//! every time.
//!
//! A pool of N threads doing real work cannot get near that: every signal here
//! is preceded by a job. So this rings once per completion and does not try to
//! coalesce on the queue's empty-to-non-empty edge. Coalescing would be a few
//! microseconds cheaper and would put a contract on the image -- "drain until
//! nil before waiting again" -- whose violation is a pump asleep with work
//! queued. A hang is a worse failure than a signal that was not strictly
//! necessary.

use core::ptr;
use core::sync::atomic::{AtomicIsize, AtomicPtr, Ordering};

use pharo_vm_plugin::{sqInt, Interp, VirtualMachine};

static PROXY: AtomicPtr<VirtualMachine> = AtomicPtr::new(ptr::null_mut());

/// The single external-semaphore index for the whole runtime. Zero means the
/// image has not started one, and nothing is signalled.
static DOORBELL: AtomicIsize = AtomicIsize::new(0);

/// Records the proxy so the workers can signal later.
pub fn remember(vm: &Interp) {
    PROXY.store(vm.as_raw(), Ordering::Release);
}

/// Sets the index every completion rings.
pub fn set_doorbell(index: sqInt) {
    DOORBELL.store(index, Ordering::Release);
}

/// The index every completion rings, or 0.
pub fn doorbell() -> sqInt {
    DOORBELL.load(Ordering::Acquire)
}

/// Rings it, from whichever thread finished a job.
pub fn signal_completion() {
    #[cfg(test)]
    SIGNALS_SENT.fetch_add(1, Ordering::SeqCst);

    let index = doorbell();
    if index == 0 {
        return;
    }
    let vt = PROXY.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: the VM's process-lifetime proxy table, recorded from a live
    // `&Interp` before any worker was spawned, with the signature
    // `virtualMachine.h` declares. Safe from any thread -- see the module docs.
    unsafe {
        if let Some(f) = (*vt).signalSemaphoreWithIndex {
            f(index);
        }
    }
}

/// Counts the doorbells rung, so the tests can assert one per completion. No
/// proxy is ever recorded under `cargo test`, so this is a signal's only
/// effect there.
#[cfg(test)]
static SIGNALS_SENT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// See [`SIGNALS_SENT`].
#[cfg(test)]
pub fn signals_sent() -> u64 {
    SIGNALS_SENT.load(Ordering::SeqCst)
}
