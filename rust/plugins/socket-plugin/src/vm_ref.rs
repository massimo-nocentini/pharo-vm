//! Reaching the interpreter proxy from outside a primitive.
//!
//! The aio handlers ([`crate::sock`]) run from the VM's poll loop, where no
//! `&Interp` is in scope, yet they must signal image-side semaphores -- the C
//! plugin's `notify` macro calls `interpreterProxy->signalSemaphoreWithIndex`
//! through the file-global proxy pointer. This module keeps that pointer: every
//! primitive records it on entry (an atomic store of an unchanging value), and
//! the handlers read it back.
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
    let vt = PROXY.load(Ordering::Acquire);
    if vt.is_null() {
        return;
    }
    // SAFETY: the pointer is the VM's process-lifetime proxy table, recorded
    // from a live `&Interp`; the entry has the signature virtualMachine.h
    // declares. Handlers run on the interpreter thread, so this never races a
    // primitive.
    unsafe {
        if let Some(f) = (*vt).signalSemaphoreWithIndex {
            f(index as sqInt);
        }
    }
}
