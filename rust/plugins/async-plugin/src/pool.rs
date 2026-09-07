//! The runtime: a fixed pool of worker threads and the queue they take from.
//!
//! # Why a thread pool and not tokio
//!
//! Every job this plugin runs is *blocking* -- a file read, a file write, a
//! sleep. An async runtime's value is multiplexing many waits onto few threads,
//! and there is nothing here to multiplex: `std::fs::read` blocks, and under
//! tokio it would be handed straight to `spawn_blocking`, which is a thread
//! pool. What tokio would add is a large dependency tree, a second scheduler
//! inside a VM that already has one, and a set of cancellation and runtime
//! shutdown semantics to explain in a plugin whose contract is meant to fit on
//! one page.
//!
//! What *would* justify it is genuinely async work -- sockets, timers, many
//! thousands of concurrent waits. Sockets already have `SocketPlugin` and the
//! VM's own poll loop; timers have `AioPlugin`, on the same loop, with no
//! threads at all. So the runtime that is missing is exactly the one this is:
//! somewhere to put a blocking call.
//!
//! # Shape
//!
//! `submit` pushes an `Arc<Task>` and its `Job` onto a queue and wakes one
//! worker. A worker pops, runs the job with **no lock held**, writes the answer
//! into the task's own mutex, announces the handle to the pump and rings the
//! doorbell. Nothing in a worker touches the task registry, the interpreter, or
//! an oop.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

use pharo_vm_plugin::{sqInt, PrimErr, PrimResult};

use crate::job::Job;
use crate::task::Task;
use crate::vm_ref;

/// The most workers this plugin will start.
///
/// A ceiling rather than a policy: the image chooses the size, and this stops a
/// mistyped argument from starting a thousand threads. Blocking file I/O
/// saturates a disk long before it saturates this. Typed as `sqInt` so the
/// range check in `primitiveAsyncStart` compares against a stack integer with
/// no cast.
pub const MAX_WORKERS: sqInt = 64;

/// The queue, its workers, and the flag that stops them.
struct Pool {
    queue: Mutex<VecDeque<(Arc<Task>, Job)>>,
    /// Signalled when a job is queued, and broadcast when the pool stops.
    work: Condvar,
    /// Cleared by [`stop`] so parked workers wake and exit.
    running: AtomicBool,
    /// Jobs queued or running: what `shutdownModule` refuses on.
    outstanding: AtomicUsize,
    /// Joined by [`stop`]. Handles, not a count, for the reason
    /// `CLAUDE.md` §1 records: a count reaches zero while a thread is still
    /// running libstd's own epilogue, which is code in this cdylib.
    workers: Mutex<Vec<JoinHandle<()>>>,
}

static POOL: Pool = Pool {
    queue: Mutex::new(VecDeque::new()),
    work: Condvar::new(),
    running: AtomicBool::new(false),
    outstanding: AtomicUsize::new(0),
    workers: Mutex::new(Vec::new()),
};

fn queue() -> MutexGuard<'static, VecDeque<(Arc<Task>, Job)>> {
    POOL.queue.lock().unwrap_or_else(PoisonError::into_inner)
}

fn workers() -> MutexGuard<'static, Vec<JoinHandle<()>>> {
    POOL.workers.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Is the pool running?
#[must_use]
pub fn is_running() -> bool {
    POOL.running.load(Ordering::Acquire)
}

/// Jobs queued or running.
#[must_use]
pub fn outstanding() -> usize {
    POOL.outstanding.load(Ordering::Acquire)
}

/// How many workers are alive.
///
/// `cfg(test)` only: the image already learns this from what
/// `primitiveAsyncStart` answers, and the test needs it to assert that
/// [`stop`] really *joined* them rather than merely asking them to leave.
#[cfg(test)]
#[must_use]
pub fn worker_count() -> usize {
    workers().len()
}

/// Starts `count` workers. Answers how many are running afterwards.
///
/// Starting an already-started pool is not an error and does not resize it:
/// the image calls this from its own start-up, which may run more than once,
/// and a resize would have to move jobs between queues for no benefit.
pub fn start(count: usize) -> PrimResult<usize> {
    let mut live = workers();
    if !live.is_empty() {
        return Ok(live.len());
    }
    POOL.running.store(true, Ordering::Release);
    for index in 0..count {
        match std::thread::Builder::new()
            .name(format!("pharo-async-{index}"))
            .spawn(run_worker)
        {
            Ok(handle) => live.push(handle),
            // Out of threads: run with what started rather than failing after
            // having started some.
            Err(_) => break,
        }
    }
    if live.is_empty() {
        POOL.running.store(false, Ordering::Release);
        return Err(PrimErr::Unsupported);
    }
    Ok(live.len())
}

/// Stops the pool and joins every worker. Answers whether it stopped.
///
/// Refuses while a job is outstanding, because the point of stopping is to make
/// a `dlclose` safe and a running job is a worker executing this library's
/// text. An idle worker is parked on the condvar and exits as soon as it is
/// broadcast to, so this does not block for long once it agrees to run.
pub fn stop() -> bool {
    if outstanding() != 0 {
        return false;
    }
    POOL.running.store(false, Ordering::Release);
    // The lock is taken and dropped so the broadcast cannot land between a
    // worker's predicate check and its wait.
    drop(queue());
    POOL.work.notify_all();

    let live: Vec<JoinHandle<()>> = std::mem::take(&mut workers());
    for handle in live {
        let _ = handle.join();
    }
    true
}

/// Queues a job. The `Arc<Task>` is the worker's only view of it.
pub fn submit(task: Arc<Task>, job: Job) -> PrimResult<()> {
    if !is_running() {
        return Err(PrimErr::Unsupported);
    }
    POOL.outstanding.fetch_add(1, Ordering::AcqRel);
    queue().push_back((task, job));
    POOL.work.notify_one();
    Ok(())
}

/// Decrements [`Pool::outstanding`] however a job ends, unwind included.
///
/// The counter is what `shutdownModule` reads to decide whether a `dlclose` is
/// safe, so a leak is not a slow leak: it refuses every later unload for the
/// life of the process.
struct Outstanding;

impl Drop for Outstanding {
    fn drop(&mut self) {
        POOL.outstanding.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One worker's whole life.
fn run_worker() {
    loop {
        let next = {
            let mut queued = queue();
            loop {
                if !POOL.running.load(Ordering::Acquire) {
                    return;
                }
                if let Some(item) = queued.pop_front() {
                    break item;
                }
                queued = POOL
                    .work
                    .wait(queued)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let (task, job) = next;
        let _outstanding = Outstanding;

        // A panic in a job body must not take the worker down with the task
        // still `PENDING` and the pump still waiting: that is a hang produced
        // by an error path. It becomes an ordinary job failure.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job.run(&task);
        }));
        if outcome.is_err() && task.state() == crate::task::PENDING {
            task.fail(crate::task::ERR_UNKNOWN);
        }

        crate::task::announce(task.handle());
        vm_ref::signal_completion();
    }
}
