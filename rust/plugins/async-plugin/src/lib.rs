//! Blocking work, off the interpreter thread: one pool, one doorbell, a task
//! registry.
//!
//! # What it is for
//!
//! Reading a 200 MB file from Pharo freezes every Process in the image for the
//! length of the read, for the same reason a DNS lookup used to: the primitive
//! blocks, and a primitive that blocks blocks the GC. `SocketPlugin`'s resolver
//! was fixed one lookup at a time; this is the general form.
//!
//! The shape is the only one a Rust plugin may take (`CLAUDE.md`, *The shape
//! every new capability must take*): **handle, doorbell, collect.** The image
//! submits a job and gets a SmallInteger handle. A worker thread does the work
//! where the image is not. One counted signal wakes one pump Process. A later
//! primitive, on the interpreter thread, copies the bytes out.
//!
//! ```smalltalk
//! | sema index handle |
//! sema := Semaphore new.
//! index := Smalltalk registerExternalObject: sema.
//! nil asyncStart: 4 signalling: index.
//!
//! "one pump Process for the whole runtime; nothing else ever waits"
//! [ | h | [ true ] whileTrue: [
//!     sema wait.
//!     [ (h := nil asyncNextCompleted) notNil ] whileTrue: [
//!         [ self deliver: (nil asyncResult: h) for: h ]
//!             on: Error
//!             do: [ :e | nil asyncCancel: h ] ] ] ] fork.
//!
//! handle := nil asyncReadFile: '/etc/hosts'.
//! ```
//!
//! **That `on: Error do: [ ... asyncCancel: ... ]` is not defensive
//! programming, it is the *ack*.** `primitiveAsyncNextCompleted` peeks, so a
//! completion nobody collects stays at the head of the queue and the loop sees
//! it again immediately -- an infinite loop, which is how this was found. A
//! pump either collects a handle or cancels it; doing neither is not an
//! option. The most likely reason a collection fails is the allocation limit
//! below.
//!
//! # A plugin cannot allocate a large object
//!
//! `primitiveAsyncResult:` builds a ByteArray through the proxy's
//! `instantiateClass:indexableSize:`, which allocates out of what the image has
//! *already* got and does not grow the heap. The interpreter answers a
//! `NoMemory` failure by scavenging and re-running the primitive, then by doing
//! a full GC and re-running it again -- and if the object still does not fit,
//! the primitive fails for good.
//!
//! Measured on a stock Pharo 12.0 image: 1, 4 and 16 MB reads collect; 32 and
//! 40 MB fail. It is headroom, not a constant -- 16 MB fails too once the image
//! is already holding another 16 MB -- so a plugin that wants to hand back
//! something big has to chunk it, and this one does not.
//!
//! What matters is that the failure is *clean*: because the drain peeks, a
//! failed collection leaves the task done, its result intact, and its handle
//! still at the head of the queue. Verified on a live image -- state still 1,
//! `asyncNextCompleted` still answering the same handle, outstanding still 1,
//! and a second attempt failing the same way rather than finding the result
//! gone.
//!
//! # The contract
//!
//! | primitive | answers |
//! |---|---|
//! | `primitiveAsyncStart: workers signalling: semaIndex` | workers actually started |
//! | `primitiveAsyncStop` | `true` if the pool stopped, `false` if a job is outstanding |
//! | `primitiveAsyncReadFile: path` | a task handle |
//! | `primitiveAsyncWriteFile: path contents: bytes` | a task handle |
//! | `primitiveAsyncSleep: millis` | a task handle |
//! | `primitiveAsyncNextCompleted` | the oldest finished handle, **left in place**, or nil |
//! | `primitiveAsyncState: handle` | 0 pending, 1 done, 2 failed, 3 cancelled |
//! | `primitiveAsyncResult: handle` | the answer, **retiring the task** |
//! | `primitiveAsyncError: handle` | errno, or a negative plugin code, or 0 |
//! | `primitiveAsyncCancel: handle` | `true` if it retired one |
//! | `primitiveAsyncOutstanding` | tasks the image has not collected |
//!
//! A `ReadFile` result is a ByteArray, a `WriteFile` result is the byte count,
//! a `Sleep` result is nil. Every task must be collected with
//! `primitiveAsyncResult:` or dropped with `primitiveAsyncCancel:`; a handle
//! that is neither keeps its slot and keeps `primitiveAsyncNextCompleted`
//! answering it.
//!
//! # The three traps, and what was done about each
//!
//! 1. **The drain is peek-and-ack, not pop.** `primitiveAsyncNextCompleted`
//!    *peeks*: the completion stays on the queue until
//!    `primitiveAsyncResult:` or `primitiveAsyncCancel:` takes the task away.
//!    `IntoReturn` runs after a primitive body, and an allocation failure makes
//!    the interpreter scavenge and **re-run** the primitive -- so a pop would
//!    have consumed the completion on a run that never reached the image.
//!    `primitiveAsyncResult:` obeys the same rule internally: it builds the
//!    ByteArray *first* and retires the task only once there is an answer to
//!    hand over.
//!
//!    The half `CLAUDE.md` did not say, and a live run found: **peek-and-ack
//!    only works if the ack is mandatory.** A completion the image cannot
//!    collect blocks the head of the queue and a naive pump spins on it. Hence
//!    the `on: Error do: [ ... asyncCancel: ... ]` above.
//! 2. **The registry holds `Arc<Task>`.** `Registry::with` holds its mutex
//!    across the closure, so a worker reaching its task through the registry
//!    would hold that lock for the length of a file read and stop the
//!    interpreter dead. Workers are handed their `Arc` at submit time and never
//!    look at the registry again.
//! 3. **`shutdownModule` refuses while a job is outstanding.**
//!    `Smalltalk vm unloadModule:` is reachable from ordinary image code and
//!    ends in `dlclose`, which unmaps the text a worker is executing. It also
//!    joins the workers rather than counting them down, because a count reaches
//!    zero while a thread is still running libstd's epilogue -- see
//!    `CLAUDE.md` §1, where that was measured.

// The crate is named for the shared library the VM loads (libAsyncPlugin.so),
// which fixes its spelling, and the primitives keep the image's names.
#![allow(non_snake_case)]

use std::path::PathBuf;

use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, sqInt, Interp, IntoReturn, Oop, PrimErr, PrimResult,
};

mod job;
mod pool;
mod task;
mod vm_ref;

use job::Job;
use task::Outcome;

pub use job::MAX_READ_BYTES;

pharo_plugin!("AsyncPlugin", shutdown = plugin_shutdown);

/// The longest `primitiveAsyncSleep:` will occupy a worker. See
/// [`job::Job::Sleep`] for why this job exists at all.
const MAX_SLEEP_MS: sqInt = 60_000;

/// Marker: the primitive arranged the stack itself.
struct Answered;

impl IntoReturn for Answered {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

/// `shutdownModule`: stop the pool, or refuse.
///
/// [`pool::stop`] declines while a job is queued or running and otherwise joins
/// every worker, so a `true` here means no thread of this plugin's is alive.
/// `ioUnloadModule` honours a 0 by leaving the module loaded.
fn plugin_shutdown() -> bool {
    pool::stop()
}

/// Answers a `sqInt` through the C shims' `pop`/`push` pair.
fn answer(vm: &Interp, args: sqInt, value: sqInt) -> PrimResult<Answered> {
    let oop = vm.integer_checked(value)?;
    vm.pop(args + 1)?;
    vm.push(oop)?;
    Ok(Answered)
}

/// Answers an already-built oop the same way.
fn answer_oop(vm: &Interp, args: sqInt, oop: Oop) -> PrimResult<Answered> {
    vm.pop(args + 1)?;
    vm.push(oop)?;
    Ok(Answered)
}

/// Submits `job` and answers its handle. The one place a task is created.
fn submit(vm: &Interp, args: sqInt, job: Job) -> PrimResult<Answered> {
    let task = task::create()?;
    let handle = task.handle();
    // Build the answer before the job can possibly finish, so that a re-run
    // after a scavenge cannot submit the work twice.
    let oop = vm.integer_checked(handle)?;
    if let Err(e) = pool::submit(task, job) {
        let _ = task::remove(handle);
        return Err(e);
    }
    answer_oop(vm, args, oop)
}

/// `primitiveAsyncStart: workers signalling: semaIndex`
///
/// Starts the pool and names the one external-semaphore index every completion
/// will ring. Answers how many workers are running, which may be fewer than
/// asked for if the OS refused a thread. Calling it again does not resize a
/// running pool.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncStart(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(2)?;
    vm_ref::remember(vm);

    let workers = vm.stack_integer(1)?;
    let sema_index = vm.stack_integer(0)?;
    if !(1..=pool::MAX_WORKERS).contains(&workers) || sema_index < 0 {
        return Err(PrimErr::BadArgument);
    }

    vm_ref::set_doorbell(sema_index);
    let started = pool::start(workers as usize)?;
    answer(vm, 2, started as sqInt)
}

/// `primitiveAsyncStop` -- stop the pool, or answer false.
///
/// Refuses while a job is outstanding, for the same reason `shutdownModule`
/// does. An image that wants to stop cleanly cancels or collects first.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncStop(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(0)?;
    vm_ref::remember(vm);

    let stopped = pool::stop();
    let oop = if stopped {
        vm.true_object()?
    } else {
        vm.false_object()?
    };
    answer_oop(vm, 0, oop)
}

/// `primitiveAsyncReadFile: path` -- read a whole file off-thread.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncReadFile(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let path = path_argument(vm, 0)?;
    submit(vm, 1, Job::ReadFile { path })
}

/// `primitiveAsyncWriteFile: path contents: bytes` -- replace a file
/// off-thread, answering the byte count when it lands.
///
/// The contents are copied here, on the interpreter thread, because a worker
/// may not read image memory: the GC moves objects and a foreign thread cannot
/// join the pinning protocol. One copy buys the image its freedom for the
/// length of the write.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncWriteFile(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(2)?;
    vm_ref::remember(vm);

    let path = path_argument(vm, 1)?;
    let contents = vm.bytes_of(vm.stack_value(0)?)?.to_vec();
    submit(vm, 2, Job::WriteFile { path, contents })
}

/// `primitiveAsyncSleep: millis` -- occupy a worker. Diagnostic; see
/// [`job::Job::Sleep`].
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncSleep(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let millis = vm.stack_integer(0)?;
    if !(0..=MAX_SLEEP_MS).contains(&millis) {
        return Err(PrimErr::BadArgument);
    }
    submit(
        vm,
        1,
        Job::Sleep {
            millis: millis as u64,
        },
    )
}

/// `primitiveAsyncNextCompleted` -- the oldest finished task's handle, or nil.
///
/// **Peeks.** The handle stays on the completed queue until the task is retired
/// by `primitiveAsyncResult:` or `primitiveAsyncCancel:`, so this primitive has
/// no side effect at all and re-running it after a scavenge answers the same
/// thing. That is `CLAUDE.md` §3 trap 1, and it is why the pump's inner loop
/// must collect what it is handed rather than merely counting it.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncNextCompleted(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(0)?;
    vm_ref::remember(vm);

    let oop = match task::peek_completed() {
        Some(handle) => vm.integer_checked(handle)?,
        None => vm.nil()?,
    };
    answer_oop(vm, 0, oop)
}

/// `primitiveAsyncState: handle` -- 0 pending, 1 done, 2 failed, 3 cancelled.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncState(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let handle = vm.stack_integer(0)?;
    let state = task::require(handle)?.state();
    answer(vm, 1, state as sqInt)
}

/// `primitiveAsyncError: handle` -- the errno, a negative plugin code, or 0.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncError(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let handle = vm.stack_integer(0)?;
    let error = task::require(handle)?.error();
    answer(vm, 1, error as sqInt)
}

/// `primitiveAsyncResult: handle` -- the answer, retiring the task.
///
/// Fails with `Inappropriate` while the task is still pending, so a pump that
/// collects something it was not handed gets a failure rather than a nil it
/// might mistake for an empty file.
///
/// The order of the last three statements is the rule `CLAUDE.md` §3 states and
/// this plugin exists to demonstrate: **allocate first, mutate Rust state
/// last.** `read_bytes` can answer `NoMemory`, and the interpreter responds to
/// that by scavenging and re-running the primitive -- so retiring the task
/// before the ByteArray existed would lose the result permanently on the retry,
/// with no error anywhere.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncResult(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let handle = vm.stack_integer(0)?;
    let entry = task::require(handle)?;
    if entry.state() == task::PENDING {
        return Err(PrimErr::Inappropriate);
    }

    // Built before anything is torn down.
    let oop = entry.with_outcome(|outcome| match outcome {
        Outcome::Bytes(bytes) => byte_array(vm, bytes),
        Outcome::Count(count) => vm.integer_checked(*count as sqInt),
        Outcome::Nothing => vm.nil(),
    })?;

    // Only now, with the answer in hand.
    let _retired = task::remove(handle)?;
    answer_oop(vm, 1, oop)
}

/// `primitiveAsyncCancel: handle` -- disown a task.
///
/// Answers whether there was one, so a Process can cancel unconditionally on
/// its way out. A job already running is not interrupted: it finishes and its
/// answer is discarded, exactly as an aborted DNS lookup does.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncCancel(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    vm_ref::remember(vm);

    let handle = vm.stack_integer(0)?;
    let cancelled = match task::get(handle) {
        Ok(entry) => {
            entry.cancel();
            task::remove(handle).is_ok()
        }
        Err(_) => false,
    };
    let oop = if cancelled {
        vm.true_object()?
    } else {
        vm.false_object()?
    };
    answer_oop(vm, 1, oop)
}

/// `primitiveAsyncOutstanding` -- tasks the image has not collected.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveAsyncOutstanding(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(0)?;
    vm_ref::remember(vm);
    answer(vm, 0, task::outstanding() as sqInt)
}

/// Reads a path argument as an owned `PathBuf`.
///
/// Owned, because the worker outlives the primitive and may not look at image
/// memory. Bytes rather than `String`: a path is not required to be UTF-8 on
/// Unix, and `sq2uxPath`'s callers have always passed whatever the image held.
fn path_argument(vm: &Interp, offset: sqInt) -> PrimResult<PathBuf> {
    use std::os::unix::ffi::OsStrExt;

    let bytes = vm.bytes_of(vm.stack_value(offset)?)?;
    if bytes.is_empty() {
        return Err(PrimErr::BadArgument);
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes).to_owned()))
}

/// A fresh ByteArray holding `bytes`.
fn byte_array(vm: &Interp, bytes: &[u8]) -> PrimResult<Oop> {
    let class = vm.class_byte_array()?;
    let oop = vm.instantiate(class, bytes.len() as sqInt)?;
    vm.write_bytes(oop, 0, bytes)?;
    Ok(oop)
}

#[cfg(test)]
mod tests;
