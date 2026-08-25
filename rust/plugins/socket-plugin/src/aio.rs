//! The VM core's async-I/O registry, as this plugin consumes it.
//!
//! The C plugin calls `aioEnable` / `aioHandle` / `aioDisable` / `aioFini`
//! (declared in `include/pharovm/common/aio.h`, defined in `src/unix/aio.c`)
//! as plain undefined symbols in its shared object, resolved against the host
//! process when the VM `dlopen`s the plugin. This port does exactly the same:
//! the `extern "C"` block below leaves the four symbols undefined in
//! `libSocketPlugin.so`, and the dynamic linker binds them to the VM core at
//! load time -- there is no `ioLoadFunctionFrom` indirection to document
//! because the C plugin never used one either.
//!
//! Semantics that matter to this plugin (from `aio.c`):
//!
//! * `aioEnable(fd, data, 0)` puts `fd` into non-blocking mode and records
//!   `data`, which is handed back to every handler. Until then the socket the
//!   plugin created is *blocking*.
//! * Handlers are one-shot: the interest mask is cleared before the handler
//!   runs, and the handler must call `aioHandle` again to be re-armed.
//! * Handlers run from the VM's poll loop on the interpreter thread -- the
//!   same thread primitives run on, never concurrently with them.
//!
//! Under `cfg(test)` the externs are replaced by a small in-crate poll loop
//! with the same one-shot contract, so the socket state machine can be driven
//! end-to-end without a VM. See [`testing`].

use core::ffi::{c_int, c_void};

use pharo_vm_plugin::sqInt;

/// Handler for exceptional conditions.
pub const AIO_X: c_int = 1 << 0;
/// Handler for readability.
pub const AIO_R: c_int = 1 << 1;
/// Handler for writability.
pub const AIO_W: c_int = 1 << 2;
/// Read or exception.
pub const AIO_RX: c_int = AIO_R | AIO_X;
/// Write or exception.
pub const AIO_WX: c_int = AIO_W | AIO_X;
/// Anything.
pub const AIO_RWX: c_int = AIO_R | AIO_W | AIO_X;

/// The handler signature from `aio.h`: `void (*)(sqInt fd, void *clientData,
/// int flag)`. Declared `unsafe` on the Rust side because a handler dereferences
/// the raw `clientData` it registered.
pub type AioHandler = unsafe extern "C" fn(fd: sqInt, client_data: *mut c_void, flag: c_int);

#[cfg(not(test))]
mod backend {
    use super::*;

    // Resolved against the VM core when the plugin's .so is loaded, exactly as
    // the C plugin's undefined references were.
    extern "C" {
        pub fn aioEnable(fd: sqInt, clientData: *mut c_void, flags: c_int);
        pub fn aioHandle(fd: sqInt, handlerFn: AioHandler, mask: c_int);
        pub fn aioDisable(fd: sqInt);
        pub fn aioFini();
    }
}

/// `aioEnable`: register `fd`, set it non-blocking, remember `data`.
pub fn enable(fd: c_int, data: *mut c_void, flags: c_int) {
    #[cfg(not(test))]
    // SAFETY: the VM core exports this symbol for exactly this call shape; the
    // data pointer's lifetime is managed by the caller (it stays valid until
    // aioDisable, as in the C plugin).
    unsafe {
        backend::aioEnable(fd as sqInt, data, flags);
    }
    #[cfg(test)]
    testing::enable(fd, data, flags);
}

/// `aioHandle`: arm `handler` for the events in `mask` on `fd`, once.
pub fn handle(fd: c_int, handler: AioHandler, mask: c_int) {
    #[cfg(not(test))]
    // SAFETY: as in `enable`.
    unsafe {
        backend::aioHandle(fd as sqInt, handler, mask);
    }
    #[cfg(test)]
    testing::handle(fd, handler, mask);
}

/// `aioDisable`: forget `fd` entirely.
pub fn disable(fd: c_int) {
    #[cfg(not(test))]
    // SAFETY: as in `enable`.
    unsafe {
        backend::aioDisable(fd as sqInt);
    }
    #[cfg(test)]
    testing::disable(fd);
}

/// `aioFini`: tear the registry down (network shutdown).
pub fn fini() {
    #[cfg(not(test))]
    // SAFETY: as in `enable`.
    unsafe {
        backend::aioFini();
    }
    #[cfg(test)]
    testing::fini();
}

/// A miniature `aio.c` for unit tests: same one-shot handler contract, driven
/// by an explicit [`testing::poll_once`] instead of the VM's poll loop.
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Entry {
        data: *mut c_void,
        handler: Option<AioHandler>,
        mask: c_int,
    }
    // The registry stores raw client-data pointers. Tests serialise all
    // network activity behind one lock, so the pointers are never used from
    // two threads at once.
    unsafe impl Send for Entry {}

    static REGISTRY: Mutex<Option<HashMap<c_int, Entry>>> = Mutex::new(None);

    fn with_registry<R>(f: impl FnOnce(&mut HashMap<c_int, Entry>) -> R) -> R {
        let mut guard = REGISTRY.lock().unwrap();
        f(guard.get_or_insert_with(HashMap::new))
    }

    pub fn enable(fd: c_int, data: *mut c_void, flags: c_int) {
        // Real aioEnable puts the descriptor into non-blocking mode unless it
        // was declared external; the plugin always passes flags 0.
        if flags == 0 {
            unsafe {
                let arg = libc::fcntl(fd, libc::F_GETFL);
                libc::fcntl(fd, libc::F_SETFL, arg | libc::O_NONBLOCK);
            }
        }
        with_registry(|reg| {
            reg.insert(
                fd,
                Entry {
                    data,
                    handler: None,
                    mask: 0,
                },
            );
        });
    }

    pub fn handle(fd: c_int, handler: AioHandler, mask: c_int) {
        with_registry(|reg| {
            if let Some(entry) = reg.get_mut(&fd) {
                entry.handler = Some(handler);
                entry.mask = mask;
            }
        });
    }

    pub fn disable(fd: c_int) {
        with_registry(|reg| {
            reg.remove(&fd);
        });
    }

    pub fn fini() {
        with_registry(HashMap::clear);
    }

    /// One turn of the poll loop: wait up to `timeout_ms` for any armed event,
    /// deliver each ready handler once (clearing its mask first, as `aio.c`
    /// does), and answer how many handlers ran.
    pub fn poll_once(timeout_ms: c_int) -> usize {
        let armed: Vec<(c_int, c_int)> = with_registry(|reg| {
            reg.iter()
                .filter(|(_, e)| e.mask != 0 && e.handler.is_some())
                .map(|(fd, e)| (*fd, e.mask))
                .collect()
        });
        if armed.is_empty() {
            return 0;
        }

        let mut fds: Vec<libc::pollfd> = armed
            .iter()
            .map(|&(fd, mask)| {
                let mut events: libc::c_short = 0;
                if mask & AIO_R != 0 {
                    events |= libc::POLLIN;
                }
                if mask & AIO_W != 0 {
                    events |= libc::POLLOUT;
                }
                if mask & AIO_X != 0 {
                    events |= libc::POLLPRI;
                }
                libc::pollfd {
                    fd,
                    events,
                    revents: 0,
                }
            })
            .collect();

        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if n <= 0 {
            return 0;
        }

        let mut delivered = 0;
        for pfd in &fds {
            if pfd.revents == 0 {
                continue;
            }
            // Take the entry's arming atomically: clear the mask before the
            // handler runs so it can re-arm itself, exactly like aio.c.
            let Some((data, handler, mask)) = with_registry(|reg| {
                reg.get_mut(&pfd.fd).and_then(|e| {
                    let mask = e.mask;
                    if mask == 0 {
                        return None;
                    }
                    e.mask = 0;
                    e.handler.map(|h| (e.data, h, mask))
                })
            }) else {
                continue;
            };
            let error = pfd.revents & (libc::POLLERR | libc::POLLNVAL) != 0;
            let err_bit = if error { AIO_X } else { 0 };
            let readable = pfd.revents & (libc::POLLIN | libc::POLLHUP) != 0;
            let writable = pfd.revents & libc::POLLOUT != 0;
            let flag = if mask & AIO_R != 0 && (readable || error) {
                AIO_R | err_bit
            } else if mask & AIO_W != 0 && (writable || error) {
                AIO_W | err_bit
            } else if mask & AIO_X != 0 && error {
                AIO_X
            } else {
                // Spurious wakeup for an event we are not armed for: re-arm.
                with_registry(|reg| {
                    if let Some(e) = reg.get_mut(&pfd.fd) {
                        e.mask = mask;
                    }
                });
                continue;
            };
            unsafe { handler(pfd.fd as sqInt, data, flag) };
            delivered += 1;
        }
        delivered
    }
}
