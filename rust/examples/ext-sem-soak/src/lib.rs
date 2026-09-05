//! A soak harness for the VM's external-semaphore request table, driven from
//! genuine foreign threads.
//!
//! # Why this exists
//!
//! `CLAUDE.md` names one experiment as the prerequisite for every asynchronous
//! capability the port still wants -- async DNS, a job pool, fd watches, an
//! AsyncPlugin -- because all of them rest on the same edge: **N background
//! threads incrementing counters in the VM's request table at rate, and the
//! interpreter noticing.** Nothing in the tree had ever done that. SocketPlugin's
//! own `vm_ref` asserted the opposite in a SAFETY comment ("handlers run on the
//! interpreter thread, so this never races a primitive"), and
//! unix-os-process-plugin signals from a *signal handler* into a path that takes
//! a `sem_wait`, which is unsound rather than a model.
//!
//! So this plugin does exactly that and nothing else, and measures four things:
//!
//! * **(a) No lost signals.** Each worker counts what it sent, per index. The
//!   image counts what it received, per index. The two must match exactly --
//!   which is a claim the *counting* request table (`requests`/`responses` per
//!   entry, rather than a flag) should make true by construction, and which no
//!   test has ever checked at rate.
//! * **(b) The contention profile.** Every signal is timed around
//!   `signalSemaphoreWithIndex`, which takes `requestMutex` with signals
//!   masked. The distribution of that call is what a signaller sees of the VM
//!   thread's own `doSignalExternalSemaphores` holding the same mutex.
//! * **(c) `sigprocmask` in a multithreaded process.** The critical section
//!   masks signals with `sigprocmask`, not `pthread_sigmask`, which POSIX
//!   leaves unspecified once there is more than one thread. This harness makes
//!   the process genuinely multithreaded while that path runs; the per-thread
//!   question itself is pinned by a unit test in
//!   `rust/pharo-platform/src/external_semaphores.rs`.
//! * **(d) Wake latency.** A separate low-rate phase signals one index at a
//!   time and has the image acknowledge on wake, so a send can be paired with
//!   its receipt one-to-one. The claim under test is "worst case one relinquish
//!   quantum".
//!
//! # Not a shipping plugin
//!
//! It is absent from `cmake/rust.cmake` on purpose, so no bundle can contain
//! it. Build and install it by hand:
//!
//! ```sh
//! cd rust/plugins && cargo build --release -p ext-sem-soak
//! install -m755 target/release/libExtSemSoak.so <vm directory>/
//! ```
//!
//! and drive it with `soak.st` next to this file. `install`, not `cp`: writing
//! into a mapped inode is SIGBUS.
//!
//! # What a worker is allowed to do
//!
//! Only what any foreign thread may do (`CLAUDE.md`, *The shape every new
//! capability must take*): increment a counter in the request table. It reads
//! no oop, allocates nothing in the image, and answers no value. Everything the
//! image reads back comes out of a later primitive, on the interpreter thread.

// The crate is named for the shared library the VM loads (libExtSemSoak.so),
// which fixes its spelling, and the primitives keep the image's names.
#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use pharo_vm_plugin::{
    pharo_plugin, pharo_primitive, sqInt, Interp, IntoReturn, PrimErr, PrimResult, VirtualMachine,
};

/// Marker: the primitive arranged the stack itself, so the SDK's `IntoReturn`
/// must not add a `methodReturn*` on top. The same two lines every plugin in
/// this tree that answers through `pop`/`push` declares.
struct Answered;

impl IntoReturn for Answered {
    fn into_return(self, _vm: &Interp) -> PrimResult<()> {
        Ok(())
    }
}

pharo_plugin!("ExtSemSoak", init = initialise, shutdown = shut_down);

/// The largest external-semaphore index this harness will touch.
///
/// `INITIAL_EXT_SEM_TABLE_SIZE` is 256 and the image grows the table from its
/// header, so a run that stays inside 256 needs nothing special of the image.
/// A request above `doSignalExternalSemaphores`'s clamp stays pending, which
/// would look exactly like a lost signal -- so the harness refuses the range
/// instead of measuring a bound it does not control.
const MAX_INDEX: sqInt = 255;

/// `INITIAL_EXT_SEM_TABLE_SIZE` in `sq.h`, and the size
/// `ioInitExternalSemaphores` allocates before the image grows the table from
/// its header. Asserted at compile time so a change to [`MAX_INDEX`] cannot
/// silently outgrow it.
const _: () = assert!(MAX_INDEX < 256);

/// The ceiling on worker threads, so a mistyped argument cannot fork-bomb the
/// machine from a Playground.
const MAX_THREADS: sqInt = 64;

/// The ceiling on the per-signal pause, and the granularity a worker checks
/// [`RUNNING`] at.
///
/// [`primitiveSoakStop`] joins its workers, on the interpreter thread, so a
/// worker that sleeps for a whole `micros` before noticing the stop flag
/// blocks the image for that long. Sleeping in slices of at most
/// `PAUSE_SLICE_MICROS` bounds that at ~10 ms whatever pause was asked for,
/// and `MAX_PAUSE_MICROS` keeps a mistyped argument from parking a thread for
/// a day.
const MAX_PAUSE_MICROS: sqInt = 60_000_000;
/// See [`MAX_PAUSE_MICROS`].
const PAUSE_SLICE_MICROS: u64 = 10_000;

/// How many signal-duration samples to keep.
///
/// **This is a prefix, not a sample.** Once a worker's share
/// (`SAMPLE_CAP / MAX_THREADS`) is full it counts the rest as dropped, so the
/// percentiles describe the *start* of a run rather than a uniform draw from
/// it -- on a ten-minute run at 71k signals/s that is the first 131,072 of
/// 42.7 million. It is enough to answer "what does a signaller pay for
/// `requestMutex`", which is what this measures, and it is not enough to claim
/// a tail. [`primitiveSoakSignalNanos`] reports the kept and dropped counts
/// side by side so the bias is never invisible; a real answer to the tail
/// needs reservoir sampling here.
const SAMPLE_CAP: usize = 1 << 20;

// ---------------------------------------------------------------------------
// Reaching the interpreter proxy from a worker thread
// ---------------------------------------------------------------------------

/// The interpreter proxy.
///
/// Stored at the top of every primitive, as socket-plugin's `vm_ref` does --
/// the value never changes after `setInterpreter`, so the repeated stores are
/// writes of the same word and a worker always reads a value that is not
/// moving. `signalSemaphoreWithIndex` is the VM's designated any-thread entry
/// point -- see `rust/pharo-platform/src/external_semaphores.rs`.
static PROXY: AtomicPtr<VirtualMachine> = AtomicPtr::new(std::ptr::null_mut());

/// Records the proxy so workers can signal later.
fn remember(vm: &Interp) {
    PROXY.store(vm.as_raw(), Ordering::Release);
}

/// `interpreterProxy->signalSemaphoreWithIndex(index)`, timed.
///
/// Answers the call's duration in nanoseconds, or `None` if no primitive has
/// run yet (then there is nothing to signal and nothing to time).
fn signal_timed(index: sqInt) -> Option<u64> {
    let vt = PROXY.load(Ordering::Acquire);
    if vt.is_null() {
        return None;
    }
    let started = Instant::now();
    // SAFETY: the pointer is the VM's process-lifetime proxy table, recorded
    // from a live `&Interp` before any worker was spawned, and the entry has
    // the signature `virtualMachine.h` declares. Calling it from this thread is
    // the whole point of the experiment, and is what the VM documents the entry
    // as being for.
    unsafe {
        if let Some(f) = (*vt).signalSemaphoreWithIndex {
            f(index);
        }
    }
    Some(started.elapsed().as_nanos().min(u64::MAX as u128) as u64)
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// Set while workers should keep signalling.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Per-index send counts, indexed by the *absolute* external-semaphore index.
///
/// One `AtomicU64` per index rather than a mutex: a worker must not queue
/// behind another worker's bookkeeping, or the contention this measures would
/// be the harness's own.
static SENT: [AtomicU64; (MAX_INDEX + 1) as usize] =
    [const { AtomicU64::new(0) }; (MAX_INDEX + 1) as usize];

/// State only the interpreter thread touches, plus the sample reservoirs the
/// workers append to.
struct Run {
    /// Every thread this module has spawned and not yet joined -- the soak
    /// workers and the one-shot ping threads alike.
    ///
    /// It is the quiescence ledger [`shut_down`] reads, so a ping thread has
    /// to be in it: `dlclose` under a one-shot thread unmaps the same text as
    /// `dlclose` under a worker. `JoinHandle`s rather than a count, because a
    /// count decremented by the thread itself reaches zero while libstd's
    /// epilogue and any TLS destructors -- code in this cdylib -- are still
    /// running.
    workers: Vec<std::thread::JoinHandle<()>>,
    /// Durations of `signalSemaphoreWithIndex` calls, nanoseconds. See (b).
    signal_nanos: Vec<u64>,
    /// How many signal durations were dropped once [`SAMPLE_CAP`] was full.
    signal_nanos_dropped: u64,
    /// Wake latencies, nanoseconds. See (d).
    wake_nanos: Vec<u64>,
    /// How many acks arrived with no outstanding ping.
    unpaired_acks: u64,
}

impl Run {
    const fn new() -> Self {
        Self {
            workers: Vec::new(),
            signal_nanos: Vec::new(),
            signal_nanos_dropped: 0,
            wake_nanos: Vec::new(),
            unpaired_acks: 0,
        }
    }
}

static RUN: Mutex<Run> = Mutex::new(Run::new());

/// When each index was last pinged, as nanoseconds since [`EPOCH`]; -1 for
/// "not outstanding".
///
/// Only the low-rate latency phase writes these, one index at a time, so a
/// ping and its ack pair up exactly. The throughput phase never touches them.
static PINGED_AT: [AtomicI64; (MAX_INDEX + 1) as usize] =
    [const { AtomicI64::new(-1) }; (MAX_INDEX + 1) as usize];

/// The zero for [`PINGED_AT`], set by `initialiseModule`.
///
/// A `Mutex<Option<Instant>>` rather than an atomic because `Instant` is not
/// one; it is read once per ping and once per ack, both off the hot path.
static EPOCH: Mutex<Option<Instant>> = Mutex::new(None);

/// Nanoseconds since [`EPOCH`], or 0 before the module was initialised.
fn now_nanos() -> i64 {
    let epoch = EPOCH.lock().ok().and_then(|g| *g);
    match epoch {
        Some(t0) => t0.elapsed().as_nanos().min(i64::MAX as u128) as i64,
        None => 0,
    }
}

/// The mutex, refusing rather than recovering a poisoned one -- the same rule
/// the SDK's `poison::lock` applies, restated here because this crate keeps its
/// own state.
fn run() -> PrimResult<std::sync::MutexGuard<'static, Run>> {
    RUN.lock().map_err(|_| PrimErr::Unsupported)
}

/// Joins every thread that has finished, leaving the rest.
///
/// Keeps [`Run::workers`] bounded by the number of *live* threads rather than
/// by the number of pings the image has ever sent, and is what lets
/// [`shut_down`] answer "quiescent" precisely: a joined thread is gone, an
/// `is_finished` one merely has its closure behind it.
fn reap(live: &mut Vec<std::thread::JoinHandle<()>>) {
    let mut still_running = Vec::with_capacity(live.len());
    for handle in live.drain(..) {
        if handle.is_finished() {
            let _ = handle.join();
        } else {
            still_running.push(handle);
        }
    }
    *live = still_running;
}

fn initialise() -> bool {
    *EPOCH.lock().expect("a fresh module's epoch is not poisoned") = Some(Instant::now());
    true
}

/// Refuses to unload while a worker is alive.
///
/// A `dlclose` under a running thread unmaps the code it is executing. That is
/// the same quiescence rule `CLAUDE.md` §1 and §3 spell out for every plugin
/// that hands work to a thread, and this is the smallest possible instance of
/// it: answer 0 and `ioUnloadModule` leaves the library mapped.
fn shut_down() -> bool {
    match RUN.lock() {
        Ok(mut g) => {
            reap(&mut g.workers);
            g.workers.is_empty()
        }
        // A poisoned mutex cannot be inspected for quiescence, so refuse.
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// `primitiveSoakStart: threads from: firstIndex count: indexCount every: micros`
///
/// Spawns `threads` workers, each signalling indices
/// `firstIndex ..< firstIndex + indexCount` round-robin, pausing `micros`
/// microseconds between signals, until [`primitiveSoakStop`]. Answers the
/// receiver. Resets every counter first, so a run is self-contained.
///
/// # `micros = 0` starves the interpreter, and that is a result, not a bug
///
/// The first run of this harness used no pause at all, and the image made no
/// progress whatsoever: eight threads at 700% CPU, and a five-second run had
/// not reached the end of its first `Delay` after two minutes. That is not the
/// request table failing -- it is `signalSemaphoreWithIndex` doing exactly what
/// it is written to do, per signal: take `requestMutex` with signals masked,
/// call `forceInterruptCheck()`, then `aioInterruptPoll()` to wake the poll
/// loop. At an unbounded rate the VM thread spends its whole quantum inside
/// interrupt checks and contending for that mutex, and never gets back to
/// bytecode.
///
/// So a signaller has to be paced by something, and every design in
/// `CLAUDE.md` already is: a DNS worker signals once per lookup, an fd
/// watch once per readiness edge, an AsyncPlugin once per completed job. What
/// this harness has to establish is that the *table* stays correct at rate, and
/// how much rate the VM can absorb -- so the pause is an argument, `0` is kept
/// as the starvation probe, and `soak.st` defaults to a rate an application
/// could plausibly produce.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakStart(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(4)?;
    remember(vm);

    let threads = vm.stack_integer(3)?;
    let first = vm.stack_integer(2)?;
    let count = vm.stack_integer(1)?;
    let micros = vm.stack_integer(0)?;

    if !(1..=MAX_THREADS).contains(&threads)
        || first < 1
        || count < 1
        || first + count - 1 > MAX_INDEX
        || !(0..=MAX_PAUSE_MICROS).contains(&micros)
    {
        return Err(PrimErr::BadArgument);
    }
    let pause_micros = micros as u64;

    let mut state = run()?;
    if !state.workers.is_empty() {
        return Err(PrimErr::Inappropriate); // a run is already in flight
    }
    for entry in SENT.iter() {
        entry.store(0, Ordering::Relaxed);
    }
    state.signal_nanos.clear();
    state.signal_nanos_dropped = 0;
    state.wake_nanos.clear();
    state.unpaired_acks = 0;

    RUNNING.store(true, Ordering::SeqCst);
    for worker in 0..threads {
        // Each worker starts at a different offset so that they do not march
        // in lockstep over the same index, which would understate contention
        // on the tide marks.
        let mut cursor = worker % count;
        let handle = std::thread::Builder::new()
            .name(format!("ext-sem-soak-{worker}"))
            .spawn(move || {
                // Sampled locally and merged in one batch at the end: taking
                // the RUN mutex per signal would measure this harness's
                // contention rather than the VM's.
                let mut samples: Vec<u64> = Vec::new();
                let mut dropped: u64 = 0;
                while RUNNING.load(Ordering::Relaxed) {
                    let index = first + cursor;
                    cursor += 1;
                    if cursor >= count {
                        cursor = 0;
                    }
                    if let Some(nanos) = signal_timed(index) {
                        SENT[index as usize].fetch_add(1, Ordering::Relaxed);
                        if samples.len() < SAMPLE_CAP / MAX_THREADS as usize {
                            samples.push(nanos);
                        } else {
                            dropped += 1;
                        }
                    }
                    // In slices, checking RUNNING between them, so that
                    // `primitiveSoakStop`'s join does not hold the interpreter
                    // for the length of one pause.
                    let mut left = pause_micros;
                    while left > 0 && RUNNING.load(Ordering::Relaxed) {
                        let slice = left.min(PAUSE_SLICE_MICROS);
                        std::thread::sleep(std::time::Duration::from_micros(slice));
                        left -= slice;
                    }
                }
                if let Ok(mut state) = RUN.lock() {
                    state.signal_nanos.extend_from_slice(&samples);
                    state.signal_nanos_dropped += dropped;
                }
            });
        match handle {
            Ok(h) => state.workers.push(h),
            // Out of threads: run with what started rather than failing a
            // primitive half-way through a spawn loop.
            Err(_) => break,
        }
    }
    if state.workers.is_empty() {
        RUNNING.store(false, Ordering::SeqCst);
        return Err(PrimErr::Unsupported);
    }

    vm.pop(4)?;
    Ok(Answered)
}

/// `primitiveSoakStop` -- stops the workers and joins them. Answers the total
/// number of signals sent.
///
/// Joining matters: a worker still running is a worker that can still signal
/// after the image has read its receipt counts, which would make (a) report a
/// loss that never happened.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakStop(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(0)?;
    remember(vm);

    RUNNING.store(false, Ordering::SeqCst);
    let workers = {
        let mut state = run()?;
        std::mem::take(&mut state.workers)
    }; // The mutex is released before the joins: the workers take it themselves
       // on their way out, and holding it here would deadlock every one of them.
    for handle in workers {
        let _ = handle.join();
    }

    let total: u64 = SENT.iter().map(|e| e.load(Ordering::Relaxed)).sum();
    vm.pop(1)?;
    vm.push(vm.integer_checked(total as sqInt)?)?;
    Ok(Answered)
}

/// `primitiveSoakSentAt: index` -- how many signals a run sent to one index.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakSentAt(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    remember(vm);
    let index = vm.stack_integer(0)?;
    if !(0..=MAX_INDEX).contains(&index) {
        return Err(PrimErr::BadArgument);
    }
    let sent = SENT[index as usize].load(Ordering::Relaxed);
    vm.pop(2)?;
    vm.push(vm.integer_checked(sent as sqInt)?)?;
    Ok(Answered)
}

/// `primitiveSoakSignalNanos: which` -- the (b) contention profile.
///
/// `which` selects: 0 samples kept, 1 dropped, 2 mean, 3 median, 4 p99,
/// 5 p999, 6 max. Nanoseconds.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakSignalNanos(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    remember(vm);
    let which = vm.stack_integer(0)?;
    let answer = {
        let mut state = run()?;
        let dropped = state.signal_nanos_dropped;
        statistic(&mut state.signal_nanos, dropped, which)?
    };
    vm.pop(2)?;
    vm.push(vm.integer_checked(answer)?)?;
    Ok(Answered)
}

/// `primitiveSoakPing: index` -- one timestamped signal, for the (d) phase.
///
/// Sent from a thread of its own, because a signal from the interpreter thread
/// measures nothing: the whole question is how long it takes the VM to notice
/// a *foreign* thread. Answers the receiver.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakPing(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    remember(vm);
    let index = vm.stack_integer(0)?;
    if !(1..=MAX_INDEX).contains(&index) {
        return Err(PrimErr::BadArgument);
    }

    // Stamped before the thread is spawned, so the measurement includes the
    // spawn -- which is honest for "how long until the image runs", and is the
    // only part of it this harness could otherwise hide from itself.
    PINGED_AT[index as usize].store(now_nanos(), Ordering::SeqCst);
    let spawned = std::thread::Builder::new()
        .name("ext-sem-soak-ping".to_owned())
        .spawn(move || {
            signal_timed(index);
        });
    match spawned {
        Ok(handle) => {
            // The window between the spawn and this push is not a hole in the
            // ledger: both run on the interpreter thread inside this
            // primitive, and `shutdownModule` is another primitive, so nothing
            // can read the ledger in between. And if `run()` refuses -- the
            // mutex is poisoned -- the handle is dropped unrecorded, which is
            // harmless because `shut_down` already answers false forever on a
            // poisoned ledger.
            let mut state = run()?;
            reap(&mut state.workers);
            state.workers.push(handle);
        }
        Err(_) => {
            PINGED_AT[index as usize].store(-1, Ordering::SeqCst);
            return Err(PrimErr::Unsupported);
        }
    }

    vm.pop(1)?;
    Ok(Answered)
}

/// `primitiveSoakAck: index` -- called by the image's Process when it wakes.
///
/// Records the latency and answers it in nanoseconds, or -1 when no ping was
/// outstanding (a wake this harness did not cause).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakAck(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    remember(vm);
    let index = vm.stack_integer(0)?;
    if !(0..=MAX_INDEX).contains(&index) {
        return Err(PrimErr::BadArgument);
    }

    let pinged = PINGED_AT[index as usize].swap(-1, Ordering::SeqCst);
    let latency = if pinged < 0 {
        -1
    } else {
        (now_nanos() - pinged).max(0)
    };
    {
        let mut state = run()?;
        if latency < 0 {
            state.unpaired_acks += 1;
        } else if state.wake_nanos.len() < SAMPLE_CAP {
            state.wake_nanos.push(latency as u64);
        }
    }

    vm.pop(2)?;
    vm.push(vm.integer_checked(latency as sqInt)?)?;
    Ok(Answered)
}

/// `primitiveSoakWakeNanos: which` -- the (d) wake-latency distribution, with
/// the same selectors as [`primitiveSoakSignalNanos`]; 1 answers the number of
/// acks that had no ping outstanding rather than a dropped-sample count.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveSoakWakeNanos(vm: &Interp) -> PrimResult<Answered> {
    vm.expect_argument_count(1)?;
    remember(vm);
    let which = vm.stack_integer(0)?;
    let answer = {
        let mut state = run()?;
        let unpaired = state.unpaired_acks;
        statistic(&mut state.wake_nanos, unpaired, which)?
    };
    vm.pop(2)?;
    vm.push(vm.integer_checked(answer)?)?;
    Ok(Answered)
}

/// The statistic selector shared by the two distribution primitives.
///
/// Sorts in place, so the caller passes `&mut`: the samples are read many more
/// times than they are written and a run is over by the time anything asks.
fn statistic(samples: &mut [u64], side_count: u64, which: sqInt) -> PrimResult<sqInt> {
    if which == 0 {
        return Ok(samples.len() as sqInt);
    }
    if which == 1 {
        return Ok(side_count as sqInt);
    }
    if samples.is_empty() {
        return Ok(-1);
    }
    if which == 2 {
        let sum: u128 = samples.iter().map(|&n| n as u128).sum();
        return Ok((sum / samples.len() as u128) as sqInt);
    }
    samples.sort_unstable();
    // `len - 1` scaled, so p100 is the last element and p0 the first; integer
    // arithmetic throughout, since a nanosecond is finer than this measures.
    let pick = |numerator: usize, denominator: usize| -> sqInt {
        let last = samples.len() - 1;
        samples[last * numerator / denominator] as sqInt
    };
    match which {
        3 => Ok(pick(1, 2)),      // median
        4 => Ok(pick(99, 100)),   // p99
        5 => Ok(pick(999, 1000)), // p99.9
        6 => Ok(samples[samples.len() - 1] as sqInt),
        _ => Err(PrimErr::BadArgument),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reservoir arithmetic, which is the only thing here that can be
    /// wrong without an image to notice.
    #[test]
    fn percentiles_pick_the_right_samples() {
        let mut samples: Vec<u64> = (1..=100).collect();
        let samples = samples.as_mut_slice();
        assert_eq!(statistic(samples, 0, 0).unwrap(), 100);
        assert_eq!(statistic(samples, 7, 1).unwrap(), 7);
        assert_eq!(statistic(samples, 0, 2).unwrap(), 50, "mean, truncated");
        assert_eq!(statistic(samples, 0, 3).unwrap(), 50, "median");
        assert_eq!(statistic(samples, 0, 6).unwrap(), 100, "max");
        assert!(statistic(samples, 0, 7).is_err());
    }

    /// An empty run must answer -1 rather than divide by zero.
    #[test]
    fn an_empty_reservoir_answers_minus_one() {
        let samples: &mut [u64] = &mut [];
        assert_eq!(statistic(samples, 0, 0).unwrap(), 0);
        assert_eq!(statistic(samples, 0, 2).unwrap(), -1);
        assert_eq!(statistic(samples, 0, 6).unwrap(), -1);
    }

}
