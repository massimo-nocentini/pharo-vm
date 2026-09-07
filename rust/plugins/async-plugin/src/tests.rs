//! The pool driven end to end with no VM.
//!
//! Everything here goes through [`crate::pool`], [`crate::task`] and
//! [`crate::job`] rather than the primitives, because a primitive needs an
//! `&Interp` and there is none in a unit test. What the primitives add on top --
//! argument checking, the stack discipline, and building a ByteArray -- is what
//! the live run in the README covers, and the README says which parts remain
//! uncovered.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::job::Job;
use crate::pool;
use crate::task::{self, Outcome, DONE, ERR_CANCELLED, FAILED, PENDING};
use crate::vm_ref;

/// The pool and the registries are process-global, so the tests take turns.
/// Poison is recovered deliberately: one failing test must not cascade into
/// every test that shares the lock, and no image reaches this.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running pool and an empty registry, however the previous test ended.
fn fresh_pool(workers: usize) {
    task::clear();
    if !pool::is_running() {
        pool::start(workers).expect("a pool");
    }
}

/// Blocks until `f` is true, or fails the test.
fn until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Submits a job the way `crate::submit` does, minus the interpreter.
fn submit(job: Job) -> std::sync::Arc<task::Task> {
    let entry = task::create().expect("a task");
    pool::submit(std::sync::Arc::clone(&entry), job).expect("queued");
    entry
}

fn scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("pharo-async-plugin-{name}"))
}

#[test]
fn a_read_lands_as_bytes_and_rings_one_doorbell() {
    let _guard = serial();
    fresh_pool(2);

    let path = scratch("read");
    std::fs::write(&path, b"the contents").expect("wrote the fixture");
    let before = vm_ref::signals_sent();

    let entry = submit(Job::ReadFile { path: path.clone() });
    let handle = entry.handle();
    until("the read to finish", || entry.state() != PENDING);

    assert_eq!(entry.state(), DONE);
    assert_eq!(entry.error(), 0);
    entry.with_outcome(|outcome| match outcome {
        Outcome::Bytes(bytes) => assert_eq!(bytes, b"the contents"),
        other => panic!("a read answers bytes, not {other:?}"),
    });
    assert_eq!(vm_ref::signals_sent(), before + 1, "one job, one doorbell");

    // The completion is announced, and the peek does not consume it.
    assert_eq!(task::peek_completed(), Some(handle));
    assert_eq!(task::peek_completed(), Some(handle), "peek, not pop");

    task::remove(handle).expect("collected");
    assert_eq!(task::peek_completed(), None, "collecting is what consumes it");
    assert_eq!(task::outstanding(), 0);

    std::fs::remove_file(&path).ok();
}

#[test]
fn a_write_replaces_the_file_and_answers_its_length() {
    let _guard = serial();
    fresh_pool(2);

    let path = scratch("write");
    std::fs::write(&path, b"old").expect("wrote the fixture");

    let entry = submit(Job::WriteFile {
        path: path.clone(),
        contents: b"a longer new value".to_vec(),
    });
    until("the write to finish", || entry.state() != PENDING);

    assert_eq!(entry.state(), DONE);
    entry.with_outcome(|outcome| match outcome {
        Outcome::Count(n) => assert_eq!(*n, 18),
        other => panic!("a write answers a count, not {other:?}"),
    });
    assert_eq!(
        std::fs::read(&path).expect("read it back"),
        b"a longer new value"
    );

    task::remove(entry.handle()).expect("collected");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_missing_file_fails_with_the_platform_errno() {
    let _guard = serial();
    fresh_pool(2);

    let entry = submit(Job::ReadFile {
        path: scratch("definitely-not-here"),
    });
    until("the read to fail", || entry.state() != PENDING);

    assert_eq!(entry.state(), FAILED);
    assert_eq!(
        entry.error(),
        libc::ENOENT,
        "the image gets the OS's own error number"
    );
    entry.with_outcome(|outcome| {
        assert!(matches!(outcome, Outcome::Nothing), "and no answer");
    });

    task::remove(entry.handle()).expect("collected");
}

/// The `CLAUDE.md` §3 trap 2 property: a worker must not hold the registry
/// lock, or every primitive queues behind the longest job.
#[test]
fn the_registry_stays_usable_while_a_job_runs() {
    let _guard = serial();
    fresh_pool(2);

    let slow = submit(Job::Sleep { millis: 400 });
    let other = submit(Job::Sleep { millis: 0 });

    // While `slow` is still running, the interpreter side must be able to look
    // things up. If a worker held the registry lock across its job, this would
    // block for the length of the sleep instead of returning at once.
    let started = Instant::now();
    for _ in 0..1_000 {
        assert!(task::get(slow.handle()).is_ok());
        assert_eq!(task::outstanding(), 2);
    }
    let elapsed = started.elapsed();
    assert_eq!(slow.state(), PENDING, "the slow job really is still running");
    assert!(
        elapsed < Duration::from_millis(200),
        "1,000 registry reads took {elapsed:?}: something holds the lock across a job"
    );

    until("both jobs to finish", || {
        slow.state() != PENDING && other.state() != PENDING
    });
    task::remove(slow.handle()).expect("collected");
    task::remove(other.handle()).expect("collected");
}

#[test]
fn a_cancelled_job_is_abandoned_rather_than_answered() {
    let _guard = serial();
    fresh_pool(2);

    let entry = submit(Job::Sleep { millis: 2_000 });
    entry.cancel();
    until("the job to notice", || entry.state() != PENDING);

    assert_eq!(entry.state(), task::CANCELLED);
    assert_eq!(entry.error(), ERR_CANCELLED);
    // The sleep gave up in a slice rather than serving its whole 2 s.
    task::remove(entry.handle()).expect("collected");
    assert_eq!(task::outstanding(), 0);
}

/// What `shutdownModule` answers, and why.
#[test]
fn the_pool_refuses_to_stop_while_a_job_is_outstanding() {
    let _guard = serial();
    fresh_pool(2);

    let entry = submit(Job::Sleep { millis: 300 });
    until("the job to start", || pool::outstanding() == 1);
    assert!(
        !pool::stop(),
        "a running job is a worker executing this library's text"
    );
    assert!(pool::is_running(), "and refusing left the pool alone");

    until("the job to finish", || entry.state() != PENDING);
    task::remove(entry.handle()).expect("collected");
    until("the pool to go idle", || pool::outstanding() == 0);

    assert!(pool::stop(), "idle, so it stops");
    assert!(!pool::is_running());
    assert_eq!(pool::worker_count(), 0, "and every worker was joined");

    // Leave one running for whichever test goes next.
    pool::start(2).expect("restarted");
}

/// Completions come out in the order they finished, which is what lets the
/// pump drain with a single loop.
#[test]
fn completions_queue_in_the_order_they_finish() {
    let _guard = serial();
    fresh_pool(4);

    let slow = submit(Job::Sleep { millis: 250 });
    let quick = submit(Job::Sleep { millis: 0 });

    until("the quick job", || quick.state() != PENDING);
    assert_eq!(
        task::peek_completed(),
        Some(quick.handle()),
        "the quick one finished first, so it is first out"
    );
    assert_eq!(slow.state(), PENDING);

    task::remove(quick.handle()).expect("collected");
    until("the slow job", || slow.state() != PENDING);
    assert_eq!(task::peek_completed(), Some(slow.handle()));
    task::remove(slow.handle()).expect("collected");
    assert_eq!(task::completed_count(), 0);
}

/// A handle from a collected task must not resolve to whatever took its slot.
#[test]
fn a_stale_handle_does_not_resolve_to_the_next_task() {
    let _guard = serial();
    fresh_pool(2);

    let first = submit(Job::Sleep { millis: 0 });
    until("the first job", || first.state() != PENDING);
    let stale = first.handle();
    task::remove(stale).expect("collected");

    let second = submit(Job::Sleep { millis: 0 });
    until("the second job", || second.state() != PENDING);

    assert_ne!(stale, second.handle(), "a reused slot answers a new handle");
    assert!(task::get(stale).is_err(), "and the old handle names nothing");
    task::remove(second.handle()).expect("collected");
}

/// Peek-and-ack only works if the ack happens: an uncollected completion is at
/// the head of the queue and everything behind it is unreachable.
///
/// This is the liveness half of `CLAUDE.md` §3 trap 1, and it is not a
/// hypothetical -- a pump written without an error arm span on a completion it
/// could not allocate for, forever, on the first live run of this plugin. The
/// property is deliberate (it is what makes a re-run after a scavenge safe),
/// so it is pinned here rather than fixed.
#[test]
fn an_uncollected_completion_blocks_everything_behind_it() {
    let _guard = serial();
    fresh_pool(4);

    let first = submit(Job::Sleep { millis: 0 });
    until("the first job", || first.state() != PENDING);
    let second = submit(Job::Sleep { millis: 0 });
    until("the second job", || second.state() != PENDING);

    assert_eq!(task::completed_count(), 2, "both are queued for the pump");
    for _ in 0..5 {
        assert_eq!(
            task::peek_completed(),
            Some(first.handle()),
            "the head does not move on its own, however often it is peeked"
        );
    }

    // The ack -- `primitiveAsyncResult:` and `primitiveAsyncCancel:` both do
    // this, and a pump that does neither never reaches `second`.
    task::remove(first.handle()).expect("acked");
    assert_eq!(task::peek_completed(), Some(second.handle()));
    task::remove(second.handle()).expect("acked");
    assert_eq!(task::peek_completed(), None);
}
