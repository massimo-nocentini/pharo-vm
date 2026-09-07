//! What the image holds a handle on, and where a worker puts its answer.
//!
//! # `Registry<Arc<Task>>`, and why the `Arc` is the point
//!
//! `handles::Registry::with` holds the registry mutex across the caller's
//! closure. A worker that reached its task *through* the registry would hold
//! that mutex for the length of a file read, and every primitive that touched
//! any task would queue behind it -- the interpreter stopped dead by a plugin,
//! which is the failure this whole wave exists to stop making.
//!
//! So the registry holds `Arc<Task>` and hands out clones. A worker is given
//! its `Arc` when the job is submitted and never looks at the registry again;
//! the registry lock is only ever taken by the interpreter thread, for the few
//! instructions it takes to insert, clone out, or remove. Each task carries its
//! own small mutex for its outcome, contended by exactly one worker and one
//! collecting primitive, and never held across anything that blocks.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::collections::VecDeque;

use pharo_vm_plugin::handles::{Handle, Registry, Resource};
use pharo_vm_plugin::{sqInt, PrimErr, PrimResult};

/// Still queued or running.
pub const PENDING: i32 = 0;
/// Finished; [`Outcome`] holds the answer.
pub const DONE: i32 = 1;
/// Finished badly; [`Task::error`] says how.
pub const FAILED: i32 = 2;
/// Disowned by the image before it finished.
pub const CANCELLED: i32 = 3;

/// The error a job that was cancelled reports.
pub const ERR_CANCELLED: i32 = -1;
/// The error a read reports for a file past [`crate::MAX_READ_BYTES`].
pub const ERR_TOO_LARGE: i32 = -2;
/// The error a job reports when the OS gave no errno of its own.
pub const ERR_UNKNOWN: i32 = -3;

/// What the registry stores: one shared handle on a task.
///
/// A newtype only because coherence requires one. The registry's element has
/// to implement the SDK's `Resource`, and `impl Resource for Arc<Task>` is
/// refused -- both the trait and `Arc` are foreign, and `Task` behind a foreign
/// generic is not enough of a local type for the orphan rule. So the `Arc` the
/// design calls for is wrapped once, here, and unwrapped by [`get`].
///
/// The tag is hand-written rather than declared through `resource_tags!`: that
/// macro exists to prove a plugin's tags are pairwise distinct, and there is
/// one kind of handle in this library for it to be distinct from. `Resource`
/// still checks the tag is non-zero and fits the target's field, which is the
/// part that can be got wrong.
pub struct Shared(Arc<Task>);

impl Resource for Shared {
    const TAG: u8 = 1;
}

/// What a finished job produced.
#[derive(Debug, Default)]
pub enum Outcome {
    /// Nothing to collect -- the job's effect was elsewhere, or it failed.
    #[default]
    Nothing,
    /// Bytes, for a read.
    Bytes(Vec<u8>),
    /// A count, for a write.
    Count(usize),
}

/// One submitted job.
///
/// `state` and `cancelled` are atomics rather than fields of the mutex so that
/// a primitive can ask "is it done yet?" without waiting for a worker that is
/// mid-answer, which is the same reason the DNS resolver keeps its status out
/// of its state lock.
pub struct Task {
    /// This task's own handle, so a worker can name it on the completed queue
    /// without a reverse lookup.
    handle: sqInt,
    state: AtomicI32,
    cancelled: AtomicBool,
    error: AtomicI32,
    outcome: Mutex<Outcome>,
}

impl Task {
    fn new(handle: sqInt) -> Self {
        Self {
            handle,
            state: AtomicI32::new(PENDING),
            cancelled: AtomicBool::new(false),
            error: AtomicI32::new(0),
            outcome: Mutex::new(Outcome::Nothing),
        }
    }

    /// This task's handle.
    #[must_use]
    pub fn handle(&self) -> sqInt {
        self.handle
    }

    /// `PENDING`, `DONE`, `FAILED` or `CANCELLED`.
    #[must_use]
    pub fn state(&self) -> i32 {
        self.state.load(Ordering::Acquire)
    }

    /// The errno, or one of the negative `ERR_*` codes, or 0.
    #[must_use]
    pub fn error(&self) -> i32 {
        self.error.load(Ordering::Relaxed)
    }

    /// Has the image disowned this job?
    ///
    /// Checked by a worker before it starts and, for a job that runs in steps,
    /// between them. Nothing is interrupted mid-syscall: cancelling a task
    /// already inside `read(2)` means the read finishes and its answer is
    /// thrown away, exactly as an aborted DNS lookup finishes and is discarded.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Marks the job disowned. The worker notices when it can.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Records a successful answer and publishes `DONE`.
    ///
    /// The outcome is written *before* the state, and the state is a `Release`
    /// store paired with the `Acquire` in [`state`](Task::state): a collector
    /// that sees `DONE` sees the outcome that goes with it.
    pub fn finish(&self, outcome: Outcome) {
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner) = outcome;
        self.state.store(DONE, Ordering::Release);
    }

    /// Records a failure and publishes `FAILED`.
    pub fn fail(&self, error: i32) {
        self.error.store(error, Ordering::Relaxed);
        self.state.store(FAILED, Ordering::Release);
    }

    /// Records that the job was disowned before it ran, or before it finished.
    pub fn abandon(&self) {
        self.error.store(ERR_CANCELLED, Ordering::Relaxed);
        self.state.store(CANCELLED, Ordering::Release);
    }

    /// The answer, without taking it -- for a primitive that has to build its
    /// reply before it is allowed to mutate anything. See
    /// [`crate::primitiveAsyncResult`].
    pub fn with_outcome<R>(&self, f: impl FnOnce(&Outcome) -> R) -> R {
        f(&self.outcome.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

/// Every submitted task the image has not yet collected.
static TASKS: Registry<Shared> = Registry::new();

/// Handles of finished tasks, oldest first, in the order they finished.
///
/// Pushed by workers, read by the pump. A `VecDeque` behind its own small mutex
/// rather than a channel: the drain has to be able to *peek* without consuming,
/// which no channel offers -- see [`crate::primitiveAsyncNextCompleted`].
static COMPLETED: Mutex<VecDeque<sqInt>> = Mutex::new(VecDeque::new());

fn completed() -> MutexGuard<'static, VecDeque<sqInt>> {
    COMPLETED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Registers a new task and answers it with its handle already filled in.
///
/// The handle has to exist before the `Task` does, because the task carries it;
/// so this inserts a placeholder, learns its handle, and writes it back. The
/// registry is the only thing that can mint a handle, and a task without one
/// could not name itself on the completed queue.
pub fn create() -> PrimResult<Arc<Task>> {
    let handle = TASKS.insert(Shared(Arc::new(Task::new(0))))?;
    let task = Arc::new(Task::new(handle.raw()));
    // Replace the placeholder in the slot the handle already names.
    TASKS.with_mut(handle, |slot| *slot = Shared(Arc::clone(&task)))?;
    Ok(task)
}

/// The task `raw` names, cloned out so the registry lock is released at once.
pub fn get(raw: sqInt) -> PrimResult<Arc<Task>> {
    let handle = Handle::<Shared>::decode(raw)?;
    TASKS.with(handle, |shared| Arc::clone(&shared.0))
}

/// Takes a task out of the registry and off the completed queue.
pub fn remove(raw: sqInt) -> PrimResult<Arc<Task>> {
    let handle = Handle::<Shared>::decode(raw)?;
    let Shared(task) = TASKS.remove(handle)?;
    completed().retain(|&h| h != raw);
    Ok(task)
}

/// Announces a finished task to the pump. Called from a worker.
pub fn announce(handle: sqInt) {
    completed().push_back(handle);
}

/// The oldest finished task's handle, left where it is.
///
/// **Peek, not pop**, and that is the whole of `CLAUDE.md` §3 trap 1: the
/// primitive that answers this may be re-run by the interpreter after a
/// scavenge, and a pop would have consumed the completion the first time round.
#[must_use]
pub fn peek_completed() -> Option<sqInt> {
    completed().front().copied()
}

/// How many tasks the image has not collected.
#[must_use]
pub fn outstanding() -> usize {
    TASKS.len()
}

/// How many finished tasks are waiting to be collected.
///
/// `cfg(test)` only. An image that wants to know how far behind its pump is
/// counts what `primitiveAsyncNextCompleted` hands it; exposing a second,
/// separately-maintained number would be one more thing to keep true.
#[cfg(test)]
#[must_use]
pub fn completed_count() -> usize {
    completed().len()
}

/// Fails a handle that names no live task, for the primitives that need one.
pub fn require(raw: sqInt) -> PrimResult<Arc<Task>> {
    get(raw).map_err(|_| PrimErr::NotFound)
}

/// Empties both, for the tests and for `shutdownModule`.
#[cfg(test)]
pub fn clear() {
    let _ = TASKS.drain();
    completed().clear();
}
