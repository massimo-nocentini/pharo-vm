//! The VM core's async-I/O registry, as this plugin consumes it.
//!
//! `aioEnable` / `aioHandle` / `aioDisable` are plain undefined symbols in this
//! shared object, resolved against the host process when the VM `dlopen`s the
//! plugin -- exactly as `SocketPlugin`'s are, and with no `ioLoadFunctionFrom`
//! indirection, because the C plugin never used one either. On ELF that costs
//! nothing; on Mach-O it needs one linker flag per symbol, which `build.rs`
//! passes.
//!
//! The three semantics this plugin depends on, all from `src/unix/aio.c` and
//! `src/osx/aioOSX.c`:
//!
//! * **`AIO_EXT` means "not mine".** Without it `aioEnable` sets
//!   `O_NONBLOCK | O_ASYNC` on the descriptor and `F_SETOWN`s it to this
//!   process, and `aioFini` closes it. Every descriptor this plugin registers
//!   passes `AIO_EXT`: an fd the *image* owns must not have its flags changed
//!   underneath it, and a timer fd is closed by this plugin, on retirement, at
//!   a moment it chooses.
//! * **Handlers are one-shot.** The dispatch loop clears the descriptor's mask
//!   *before* calling the handler (`aio.c`: "Clearing the mask aioHandle will
//!   re add it"), so a handler that does not call `aioHandle` again is never
//!   called again. This plugin never re-arms, which is what makes a watch
//!   one-shot all the way up to the image.
//! * **Handlers run on the interpreter thread**, from the VM's poll loop,
//!   never concurrently with a primitive. That is why the registry in
//!   [`crate::watch`] can be an ordinary mutex read from both.
//!
//! Under `cfg(test)` the externs become a small in-crate poll loop with the
//! same one-shot contract, so watches can be driven end to end with no VM.

use core::ffi::{c_int, c_void};

use pharo_vm_plugin::sqInt;

/// Handler for exceptional conditions.
pub const AIO_X: c_int = 1 << 0;
/// Handler for readability.
pub const AIO_R: c_int = 1 << 1;
/// Handler for writability.
pub const AIO_W: c_int = 1 << 2;
/// External descriptor: aio must not change its flags and must not close it.
pub const AIO_EXT: c_int = 1 << 4;

/// The event bits the image is allowed to ask for.
pub const AIO_EVENTS: c_int = AIO_R | AIO_W | AIO_X;

/// `void (*)(sqInt fd, void *clientData, int flag)` from `aio.h`.
pub type AioHandler = unsafe extern "C" fn(fd: sqInt, client_data: *mut c_void, flag: c_int);

#[cfg(not(test))]
mod backend {
    use super::*;

    extern "C" {
        pub fn aioEnable(fd: sqInt, clientData: *mut c_void, flags: c_int);
        pub fn aioHandle(fd: sqInt, handlerFn: AioHandler, mask: c_int);
        pub fn aioDisable(fd: sqInt);
    }
}

/// `aioEnable(fd, data, AIO_EXT)`: register `fd` without touching its flags.
pub fn enable(fd: c_int, data: *mut c_void) {
    #[cfg(not(test))]
    // SAFETY: the VM core exports this symbol for exactly this call shape.
    // `data` is not a pointer here -- see `crate::watch`, which passes a handle
    // integer -- so there is no lifetime to keep.
    unsafe {
        backend::aioEnable(fd as sqInt, data, AIO_EXT);
    }
    #[cfg(test)]
    testing::enable(fd, data, AIO_EXT);
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

/// `aioDisable`: forget `fd` entirely. Does not close it.
pub fn disable(fd: c_int) {
    #[cfg(not(test))]
    // SAFETY: as in `enable`.
    unsafe {
        backend::aioDisable(fd as sqInt);
    }
    #[cfg(test)]
    testing::disable(fd);
}

/// A miniature `aio.c` for unit tests: the same one-shot contract, driven by an
/// explicit [`testing::poll_once`] instead of the VM's poll loop.
#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Entry {
        data: usize,
        handler: Option<AioHandler>,
        mask: c_int,
    }

    static REGISTRY: Mutex<Option<HashMap<c_int, Entry>>> = Mutex::new(None);

    fn with_registry<R>(f: impl FnOnce(&mut HashMap<c_int, Entry>) -> R) -> R {
        let mut guard = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        f(guard.get_or_insert_with(HashMap::new))
    }

    /// Unlike the real one this never touches the descriptor's flags -- and
    /// asserts why it is entitled not to.
    ///
    /// That assertion is the point: `AIO_EXT` is the whole reason this plugin
    /// may be handed a descriptor the image owns, and it is one argument in one
    /// call, so it is exactly the kind of thing that gets dropped in a refactor
    /// and noticed a year later by whoever's socket stopped blocking.
    pub fn enable(fd: c_int, data: *mut c_void, flags: c_int) {
        assert!(
            flags & AIO_EXT != 0,
            "every descriptor this plugin registers is external: without \
             AIO_EXT, aioEnable sets O_NONBLOCK|O_ASYNC on an fd the image owns"
        );
        with_registry(|reg| {
            reg.insert(
                fd,
                Entry {
                    data: data as usize,
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

    /// One turn of the poll loop: wait up to `timeout_ms`, deliver each ready
    /// handler once with its mask cleared first, and answer how many ran.
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

        // SAFETY: a well-formed pollfd array of its own length.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if ready <= 0 {
            return 0;
        }

        let mut delivered = 0;
        for pfd in &fds {
            if pfd.revents == 0 {
                continue;
            }
            // Clear the mask before the handler runs, exactly as aio.c does.
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
            let errored = pfd.revents & (libc::POLLERR | libc::POLLNVAL) != 0;
            let err_bit = if errored { AIO_X } else { 0 };
            let readable = pfd.revents & (libc::POLLIN | libc::POLLHUP) != 0;
            let writable = pfd.revents & libc::POLLOUT != 0;
            let flag = if mask & AIO_R != 0 && (readable || errored) {
                AIO_R | err_bit
            } else if mask & AIO_W != 0 && (writable || errored) {
                AIO_W | err_bit
            } else if mask & AIO_X != 0 && errored {
                AIO_X
            } else {
                with_registry(|reg| {
                    if let Some(e) = reg.get_mut(&pfd.fd) {
                        e.mask = mask;
                    }
                });
                continue;
            };
            // SAFETY: the handler was registered by this crate and the data
            // word is the handle integer it registered with.
            unsafe { handler(pfd.fd as sqInt, data as *mut c_void, flag) };
            delivered += 1;
        }
        delivered
    }
}
