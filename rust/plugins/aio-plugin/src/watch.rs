//! The watch registry: what the image holds a handle on, and what the VM's
//! poll loop calls back into.
//!
//! One `Watch` per outstanding request. The image gets a SmallInteger handle
//! (see [`pharo_vm_plugin::handles`] for the encoding, and what a generation, a
//! type tag and a session byte each catch), and that same integer -- not a
//! pointer -- is what is handed to `aioEnable` as its `clientData`. The handler
//! casts it back and looks the watch up.
//!
//! **Passing the handle rather than a pointer is the point.** The C plugins put
//! a `struct *` in `clientData` and the handler dereferences it, which is a
//! use-after-free the moment a descriptor is disabled while an event for it is
//! already in flight. An integer cannot be dangling: a stale one fails
//! [`Registry::with_mut`] with `NotFound`, and the handler does nothing.
//!
//! # Why an ordinary mutex is enough
//!
//! `aio` handlers run on the interpreter thread, from the VM's poll loop,
//! never concurrently with a primitive -- so the registry is only ever touched
//! from one thread and the lock is uncontended by construction. This plugin
//! spawns no threads at all, which is what distinguishes it from the async DNS
//! work: there is no doorbell to ring from anywhere else, because the VM is
//! already doing the waiting.

use core::ffi::{c_int, c_void};
use std::os::fd::OwnedFd;

use pharo_vm_plugin::handles::{Handle, Registry};
use pharo_vm_plugin::{resource_tags, sqInt, PrimResult};

use crate::aio;
use crate::vm_ref;

/// `fired` while nothing has happened yet. Not a valid AIO mask, and the value
/// [`crate::primitiveAioResult`] answers for a watch still outstanding.
pub const PENDING: c_int = -1;

resource_tags! {
    Watch = 1,
}

/// One outstanding request: a descriptor the poll loop is watching, and where
/// to put the answer.
pub struct Watch {
    /// The descriptor registered with `aioEnable`.
    fd: c_int,
    /// The timer descriptor this plugin created and must close, if any. An fd
    /// the *image* owns is never in here, and is never closed by this plugin.
    ///
    /// Never read outside the tests, and that is the point: the field exists to
    /// be *dropped*, which is what closes the descriptor when a watch retires.
    #[cfg_attr(not(test), allow(dead_code))]
    owned: Option<OwnedFd>,
    /// The external-semaphore index to ring when it fires. Zero means "none",
    /// as it does everywhere in the VM.
    sema_index: sqInt,
    /// The AIO flags the handler saw, or [`PENDING`].
    fired: c_int,
}

impl Watch {
    /// The events that fired, or [`PENDING`].
    #[must_use]
    pub fn fired(&self) -> c_int {
        self.fired
    }
}

/// Every outstanding watch.
pub static WATCHES: Registry<Watch> = Registry::new();

/// Registers `fd` with the poll loop and answers the image's handle.
///
/// `owned` carries the descriptor when this plugin created it, so that
/// retirement closes it; for a descriptor the image owns it is `None` and
/// nothing here ever closes it.
///
/// The order matters and is the [`CLAUDE.md` §3 trap 1] shape: the registry
/// entry exists *before* the poll loop is told about it, because the handler
/// can be called as early as the first interrupt check after `aioHandle` and it
/// looks itself up by handle.
///
/// [`CLAUDE.md` §3 trap 1]: allocate first, mutate last, never leave a window
/// where a callback can see half a thing.
pub fn arm(fd: c_int, owned: Option<OwnedFd>, events: c_int, sema_index: sqInt) -> PrimResult<sqInt> {
    let handle = WATCHES.insert(Watch {
        fd,
        owned,
        sema_index,
        fired: PENDING,
    })?;

    aio::enable(fd, handle.raw() as usize as *mut c_void);
    aio::handle(fd, on_ready, events);
    Ok(handle.raw())
}

/// Takes a watch out of the registry and undoes everything [`arm`] did.
///
/// Idempotent from the image's point of view: a handle that names nothing
/// answers `NotFound`, which the cancel primitive turns into `false` rather
/// than a failure.
pub fn retire(raw: sqInt) -> PrimResult<Watch> {
    let handle = Handle::<Watch>::decode(raw)?;
    let watch = WATCHES.remove(handle)?;
    aio::disable(watch.fd);
    // `watch.owned` closes here if this plugin made the descriptor. An
    // image-owned fd is left exactly as it was found -- `AIO_EXT` is what kept
    // aio from touching its flags, and this is what keeps us from closing it.
    Ok(watch)
}

/// Reads a watch's state without disturbing it.
pub fn peek(raw: sqInt) -> PrimResult<c_int> {
    let handle = Handle::<Watch>::decode(raw)?;
    WATCHES.with(handle, Watch::fired)
}

/// How many watches are outstanding.
#[must_use]
pub fn outstanding() -> usize {
    WATCHES.len()
}

/// Whether every watch has been retired.
///
/// `shutdownModule` answers 0 unless this is true. Nothing here runs on another
/// thread, so this is not the `dlclose`-under-a-worker hazard socket-plugin's
/// ledger guards against -- it is the other one: `aio.c` holds `on_ready`, a
/// function pointer into *this* library's text, in its `descriptorList` keyed
/// on fd, and `aioDisable` is the only thing that takes it out. Unloading with
/// a watch still armed leaves the poll loop holding a pointer into an unmapped
/// object, which is the `CLAUDE.md` §1 condition spelled out for exactly this
/// case: "plugins hand function pointers into their own text outward through
/// channels the SDK cannot see".
#[must_use]
pub fn is_quiescent() -> bool {
    WATCHES.is_empty()
}

/// The VM's poll loop calling back, on the interpreter thread.
///
/// Records what fired and rings the doorbell. It deliberately does **not**
/// re-arm: `aio.c` clears the mask before dispatch, so not calling `aioHandle`
/// again is what makes a watch one-shot all the way up to the image. Re-arming
/// is the image asking again, which keeps the plugin from delivering an event
/// nobody is waiting for.
///
/// Answers nothing and cannot fail, so a stale handle -- an event already in
/// flight for a watch the image cancelled in the same turn -- is simply
/// dropped.
unsafe extern "C" fn on_ready(_fd: sqInt, client_data: *mut c_void, flag: c_int) {
    let raw = client_data as usize as sqInt;
    let Ok(handle) = Handle::<Watch>::decode(raw) else {
        return;
    };
    let Ok(sema_index) = WATCHES.with_mut(handle, |watch| {
        watch.fired = flag;
        watch.sema_index
    }) else {
        return;
    };
    if sema_index != 0 {
        vm_ref::signal_semaphore(sema_index);
    }
}

/// The descriptor a watch is on, for the tests.
#[cfg(test)]
impl Watch {
    pub fn fd(&self) -> c_int {
        self.fd
    }

    pub fn owns_its_descriptor(&self) -> bool {
        self.owned.is_some()
    }
}
