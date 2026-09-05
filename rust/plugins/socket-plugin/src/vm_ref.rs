//! Reaching the interpreter proxy from outside a primitive.
//!
//! The aio handlers ([`crate::sock`]) run from the VM's poll loop, where no
//! `&Interp` is in scope, yet they must signal image-side semaphores -- the C
//! plugin's `notify` macro calls `interpreterProxy->signalSemaphoreWithIndex`
//! through the file-global proxy pointer. This module keeps that pointer: every
//! primitive records it on entry (an atomic store of an unchanging value), and
//! the handlers read it back.
//!
//! [`crate::resolver`]'s lookup workers read it back too, and they are *not*
//! on the interpreter thread. That is the one call in this crate that is
//! genuinely concurrent with a primitive, and it is sound because
//! `signalSemaphoreWithIndex` is the VM's designated any-thread entry point:
//! `sqExternalSemaphores.c` (here `rust/pharo-platform/src/external_semaphores.rs`)
//! takes `requestMutex` with signals masked, increments a per-index request
//! counter, widens a tide mark and asks for an interrupt check. It reads no
//! oop and enters no interpreter. The image-side semaphore is run later, on
//! the VM thread, out of `doSignalExternalSemaphores`.
//!
//! In unit tests no proxy ever gets recorded, so signalling is a no-op -- which
//! is also the honest behaviour: there is no image to signal.

use core::ffi::c_int;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use pharo_vm_plugin::{sqInt, Interp, VirtualMachine};

static PROXY: AtomicPtr<VirtualMachine> = AtomicPtr::new(ptr::null_mut());

/// Records the proxy so the aio handlers can signal semaphores later.
///
/// Called at the top of every primitive; the pointer never changes after
/// `setInterpreter`, so repeated stores are harmless.
pub fn remember(vm: &Interp) {
    PROXY.store(vm.as_raw(), Ordering::Release);
}

/// `interpreterProxy->signalSemaphoreWithIndex(index)`, or nothing when no
/// primitive has run yet (then no socket or lookup can exist either).
pub fn signal_semaphore(index: c_int) {
    #[cfg(test)]
    SIGNALS_SENT.fetch_add(1, Ordering::SeqCst);

    let vt = PROXY.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: the pointer is the VM's process-lifetime proxy table, recorded
    // from a live `&Interp`; the entry has the signature virtualMachine.h
    // declares. The table is written once, before any socket or lookup can
    // exist, and never again, so an aio handler on the interpreter thread and
    // a resolver worker on its own thread both read a value that is not
    // changing. What they call is safe to call from either -- see the module
    // docs.
    unsafe {
        if let Some(f) = (*vt).signalSemaphoreWithIndex {
            f(index as sqInt);
        }
    }
}

/// Counts the doorbells rung, so the resolver's tests can assert that a
/// disowned lookup rings none. No proxy is ever recorded under `cargo test`,
/// so this is the only observable effect a signal has there.
#[cfg(test)]
static SIGNALS_SENT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// See [`SIGNALS_SENT`].
#[cfg(test)]
pub fn signals_sent() -> u64 {
    SIGNALS_SENT.load(Ordering::SeqCst)
}
