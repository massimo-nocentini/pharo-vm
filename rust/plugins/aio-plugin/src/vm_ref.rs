//! Reaching the interpreter proxy from an aio handler.
//!
//! The handler runs from the VM's poll loop with no `&Interp` in scope, yet it
//! must ring an image-side semaphore. Every primitive records the proxy on
//! entry -- an atomic store of a value that never changes after
//! `setInterpreter` -- and the handler reads it back. The same shape as
//! `SocketPlugin`'s module of this name, for the same reason.
//!
//! Unlike that one, every caller here really is on the interpreter thread:
//! this plugin spawns no threads. `signalSemaphoreWithIndex` would be safe
//! from another one anyway -- it is the VM's designated any-thread entry point
//! -- but nothing here needs that.

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use pharo_vm_plugin::{sqInt, Interp, VirtualMachine};

static PROXY: AtomicPtr<VirtualMachine> = AtomicPtr::new(ptr::null_mut());

/// Records the proxy so the handlers can signal later.
pub fn remember(vm: &Interp) {
    PROXY.store(vm.as_raw(), Ordering::Release);
}

/// `interpreterProxy->signalSemaphoreWithIndex(index)`, or nothing when no
/// primitive has run yet -- in which case no watch can exist either.
pub fn signal_semaphore(index: sqInt) {
    #[cfg(test)]
    SIGNALS_SENT.fetch_add(1, Ordering::SeqCst);

    let vt = PROXY.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: the VM's process-lifetime proxy table, recorded from a live
    // `&Interp`; the entry has the signature `virtualMachine.h` declares.
    unsafe {
        if let Some(f) = (*vt).signalSemaphoreWithIndex {
            f(index);
        }
    }
}

/// Counts the doorbells rung, so the tests can assert one per watch. No proxy
/// is ever recorded under `cargo test`, so this is a signal's only effect there.
#[cfg(test)]
static SIGNALS_SENT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// See [`SIGNALS_SENT`].
#[cfg(test)]
pub fn signals_sent() -> u64 {
    SIGNALS_SENT.load(Ordering::SeqCst)
}
