//! Descriptor readiness and OS-resolution timers, as Pharo primitives, on the
//! poll loop the VM already runs.
//!
//! # What this is for
//!
//! An image that wants to know when a file descriptor becomes readable has, at
//! present, one option: poll it from a Process, at whatever granularity a
//! `Delay` gives. An image that wants a timer finer or more accurate than the
//! `Delay` machinery has none. Both are already solved inside the VM --
//! `src/unix/aio.c` (epoll) and `src/osx/aioOSX.c` (kqueue) wait on a set of
//! descriptors on every idle turn -- and `aioEnable`/`aioHandle`/`aioDisable`
//! are exported. Nothing was reaching them but `SocketPlugin`.
//!
//! So this plugin is thin on purpose. It adds no reactor, no runtime and **no
//! threads**: it registers a descriptor with the loop that is already running,
//! and rings an image-side semaphore from the handler. `CLAUDE.md` reserves the
//! reactor rewrite -- a persistent `mio::Poll` in place of the
//! `epoll_create1` / N x `epoll_ctl` / `epoll_wait` / `close` the VM does *per
//! poll* -- for a separate, later, Linux-first wave, and this deliberately does
//! not wait for it.
//!
//! # The contract
//!
//! | primitive | answers |
//! |---|---|
//! | `primitiveAioWaitFd: fd events: mask signalling: semaIndex` | a watch handle |
//! | `primitiveAioTimerAfter: milliseconds signalling: semaIndex` | a watch handle |
//! | `primitiveAioResult: handle` | the events that fired, retiring the watch; `-1` while pending |
//! | `primitiveAioCancel: handle` | `true` if it retired one, `false` if the handle named nothing |
//! | `primitiveAioOutstanding` | how many watches are live |
//!
//! `mask` and the answer of `primitiveAioResult:` use `aio.h`'s bits, which the
//! image should mirror: **1 exception, 2 readable, 4 writable.** A result may
//! carry the exception bit alongside the one asked for.
//!
//! **Watches are one-shot.** `aio.c` clears a descriptor's mask before calling
//! its handler, and this plugin never re-arms, so an event is delivered exactly
//! once and the image asks again if it wants more. That is the edge-triggered
//! loop it is meant to be used in:
//!
//! ```smalltalk
//! | sema index handle |
//! sema := Semaphore new.
//! index := Smalltalk registerExternalObject: sema.
//! [ true ] whileTrue: [
//!     handle := self aioWaitFd: fd events: 2 signalling: index.
//!     sema wait.
//!     (self aioResult: handle) = 2 ifTrue: [ "read from fd" ] ]
//! ```
//!
//! A handle must be collected or cancelled. `primitiveAioResult:` retires the
//! watch when it answers a real result, so the loop above leaks nothing; a
//! Process that gives up must call `primitiveAioCancel:`.
//!
//! # Two things this deliberately does not do
//!
//! **It never closes a descriptor the image owns.** Every registration passes
//! `AIO_EXT`, which is why `aioEnable` does not set `O_NONBLOCK | O_ASYNC` on
//! it or `F_SETOWN` it to this process, and why `aioFini` will not close it.
//! Timer descriptors are this plugin's own and are closed on retirement.
//!
//! **It refuses to be unloaded while a watch is armed.** `aio.c` keeps
//! `on_ready` -- a function pointer into this library's text -- in its
//! `descriptorList`, and only `aioDisable` takes it out. See
//! [`watch::is_quiescent`].

// The crate is named for the shared library the VM loads (libAioPlugin.so),
// which fixes its spelling, and the primitives keep the image's names.
#![allow(non_snake_case)]

use std::os::fd::AsRawFd;

use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, sqInt, Interp, IntoReturn, PrimErr, PrimResult,
};

mod aio;
mod timer;
mod vm_ref;
mod watch;

pharo_plugin!("AioPlugin", shutdown = plugin_shutdown);

/// Marker: the primitive arranged the stack itself, so the SDK's `IntoReturn`
/// must not add a `methodReturn*` on top.
struct Answered;

impl IntoReturn for Answered {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

/// `shutdownModule`, refusing while the poll loop still holds a pointer into
/// this library.
///
/// See [`watch::is_quiescent`] for why that is the condition. There is nothing
/// else to tear down: no threads, no runtime, and the only descriptors this
/// plugin owns belong to watches, which is exactly what has to be empty.
fn plugin_shutdown() -> bool {
    watch::is_quiescent()
}

/// Answers a `sqInt` through the C shims' `pop`/`push` pair.
///
/// `argument_count + 1` is what the generated shims pop to answer a value: the
/// arguments and the receiver.
fn answer(vm: &Interp, args: sqInt, value: sqInt) -> PrimResult<Answered> {
    let oop = vm.integer_checked(value)?;
    vm.pop(args + 1)?;
    vm.push(oop)?;
    Ok(Answered)
}

/// `primitiveAioWaitFd: fd events: mask signalling: semaIndex`
///
/// Watches an existing descriptor -- one the image got from somewhere else and
/// still owns -- for the events in `mask`, once. Answers a watch handle.
///
/// The descriptor is registered with `AIO_EXT`, so nothing here changes its
/// flags or closes it. Whether it is a socket, a pipe, a tty or a regular file
/// is the image's business; `poll(2)` reports a regular file as always ready,
/// which is a property of the platform rather than of this plugin.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAioWaitFd(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(3)?;
    vm_ref::remember(vm);

    let fd = vm.stack_integer(2)?;
    let events = vm.stack_integer(1)?;
    let sema_index = vm.stack_integer(0)?;

    // A negative fd is what `aioEnable` logs and ignores, so refuse it here
    // where the image can see the refusal. An events mask of zero would arm
    // nothing and never fire, which is a hang rather than an answer.
    if fd < 0 || fd > i32::MAX as sqInt || sema_index < 0 {
        return Err(PrimErr::BadArgument);
    }
    let events = events as core::ffi::c_int;
    if events & !aio::AIO_EVENTS != 0 || events == 0 {
        return Err(PrimErr::BadArgument);
    }

    let handle = watch::arm(fd as core::ffi::c_int, None, events, sema_index)?;
    answer(vm, 3, handle)
}

/// `primitiveAioTimerAfter: milliseconds signalling: semaIndex`
///
/// Answers a watch handle that fires once, `milliseconds` from now, at whatever
/// resolution the kernel gives -- `timerfd` on Linux, `EVFILT_TIMER` on the
/// kqueue platforms. Zero means as soon as possible, not never.
///
/// The delay a *caller* actually observes is this plus the poll loop's own
/// latency, which is bounded by how long the VM is willing to sit in
/// `epoll_wait` and is not this plugin's to promise.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAioTimerAfter(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(2)?;
    vm_ref::remember(vm);

    let millis = vm.stack_integer(1)?;
    let sema_index = vm.stack_integer(0)?;
    if millis < 0 || millis > timer::MAX_DELAY_MS as sqInt || sema_index < 0 {
        return Err(PrimErr::BadArgument);
    }

    // A descriptor the kernel refuses is an ordinary primitive failure: the
    // image has run out of descriptors, or of timers, and its fallback code is
    // the right place to say so.
    let fd = timer::after(millis as i64).map_err(|_| PrimErr::Unsupported)?;
    let raw = fd.as_raw_fd();
    let handle = watch::arm(raw, Some(fd), aio::AIO_R, sema_index)?;
    answer(vm, 2, handle)
}

/// `primitiveAioResult: handle`
///
/// Answers the events that fired and retires the watch; answers `-1`, leaving
/// the watch armed, while nothing has fired yet.
///
/// The two steps are in that order on purpose, and it is the rule `CLAUDE.md`
/// §3 states for every drain: **allocate first, mutate Rust state last.**
/// `integer_checked` is what can fail here, and a `NoMemory` from it makes the
/// interpreter scavenge and *re-run* the primitive -- so retiring the watch
/// first would lose the result permanently on the retry, with no error
/// anywhere.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAioResult(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let raw = vm.stack_integer(0)?;
    let fired = watch::peek(raw)?;
    if fired == watch::PENDING {
        return answer(vm, 1, watch::PENDING as sqInt);
    }

    let oop = vm.integer_checked(fired as sqInt)?;
    // Only now, with the answer already built, is anything torn down.
    let _retired = watch::retire(raw)?;
    vm.pop(2)?;
    vm.push(oop)?;
    Ok(Answered)
}

/// `primitiveAioCancel: handle`
///
/// Retires a watch whether or not it has fired. Answers whether there was one:
/// a handle that names nothing answers `false` rather than failing, so a
/// Process can cancel unconditionally on its way out.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAioCancel(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let raw = vm.stack_integer(0)?;
    let cancelled = watch::retire(raw).is_ok();

    let oop = if cancelled {
        vm.true_object()?
    } else {
        vm.false_object()?
    };
    vm.pop(2)?;
    vm.push(oop)?;
    Ok(Answered)
}

/// `primitiveAioOutstanding` -- how many watches are live.
///
/// For the image's own leak checking, and for the tests. A number that only
/// grows is a Process that stopped collecting.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAioOutstanding(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(0)?;
    vm_ref::remember(vm);
    answer(vm, 0, watch::outstanding() as sqInt)
}

#[cfg(test)]
mod tests;
