//! Watches driven end to end with no VM, against the `cfg(test)` poll loop in
//! [`crate::aio::testing`] -- same one-shot contract as `aio.c`, dispatched by
//! an explicit turn instead of by the interpreter's idle path.
//!
//! Every test here goes through [`crate::watch`] rather than the primitives,
//! because the primitives need an `&Interp` and there is no VM. What the
//! primitives add on top -- argument checking and the stack discipline -- is
//! the part an image-side pass has to cover; the crate README says so.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::aio::{self, testing::poll_once, AIO_R, AIO_W};
use crate::vm_ref;
use crate::watch::{self, PENDING};

/// The registry and the fake poll loop are process-global, so the tests take
/// turns. Poison is recovered deliberately: one failing test must not cascade
/// into every test that shares the lock, and no image reaches this.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Leaves the registry empty for the next test, however this one ended.
fn drain_watches() {
    for watch in watch::WATCHES.drain() {
        aio::disable(watch.fd());
    }
}

/// A connected pipe, as `File`s so the tests can read and write it. `File` is
/// only a carrier here: nothing about these descriptors is a file.
fn pipe() -> (File, File) {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: a two-element array, which is what pipe(2) writes.
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2)");
    // SAFETY: two fresh descriptors this call owns, each adopted exactly once.
    unsafe {
        use std::os::fd::FromRawFd;
        (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1]))
    }
}

/// Turns the fake poll loop until a handler runs or the budget is spent.
fn pump(budget_ms: libc::c_int) -> usize {
    let mut spent = 0;
    loop {
        let delivered = poll_once(20);
        if delivered > 0 {
            return delivered;
        }
        spent += 20;
        if spent >= budget_ms {
            return 0;
        }
    }
}

#[test]
fn a_readable_descriptor_fires_once_and_rings_the_doorbell() {
    let _guard = serial();
    drain_watches();

    let (reader, writer) = pipe();
    let signals_before = vm_ref::signals_sent();
    let handle = watch::arm(reader.as_raw_fd(), None, AIO_R, 7).expect("armed");

    assert_eq!(watch::peek(handle).unwrap(), PENDING, "nothing yet");
    assert_eq!(pump(100), 0, "and nothing fires while the pipe is empty");
    assert_eq!(
        vm_ref::signals_sent(),
        signals_before,
        "no doorbell for an event that did not happen"
    );

    (&writer).write_all(b"x").expect("wrote to the pipe");
    assert_eq!(pump(2_000), 1, "the handler ran");

    assert_eq!(watch::peek(handle).unwrap(), AIO_R);
    assert_eq!(vm_ref::signals_sent(), signals_before + 1, "one doorbell");

    // One-shot: nothing re-armed it, so a second write delivers nothing.
    (&writer).write_all(b"y").expect("wrote again");
    assert_eq!(pump(100), 0, "a fired watch is not re-armed");
    assert_eq!(vm_ref::signals_sent(), signals_before + 1, "still one");

    let retired = watch::retire(handle).expect("retired");
    assert!(
        !retired.owns_its_descriptor(),
        "an image-owned fd is not this plugin's to close"
    );
    assert_eq!(watch::outstanding(), 0);

    // And the descriptor really is still open: the plugin closed nothing.
    let mut buf = [0u8; 2];
    let n = (&reader).read(&mut buf).expect("the fd outlived the watch");
    assert_eq!(n, 2, "both bytes are still there");
}

#[test]
fn a_timer_fires_and_closes_its_own_descriptor() {
    let _guard = serial();
    drain_watches();

    let fd = crate::timer::after(50).expect("a timer");
    let raw = fd.as_raw_fd();
    let signals_before = vm_ref::signals_sent();
    let handle = watch::arm(raw, Some(fd), AIO_R, 3).expect("armed");

    assert_eq!(watch::peek(handle).unwrap(), PENDING);
    assert_eq!(pump(4_000), 1, "the timer fired");
    assert_eq!(watch::peek(handle).unwrap(), AIO_R);
    assert_eq!(vm_ref::signals_sent(), signals_before + 1);

    let retired = watch::retire(handle).expect("retired");
    assert!(
        retired.owns_its_descriptor(),
        "a timer descriptor is this plugin's, and is closed with the watch"
    );
    drop(retired);

    // The descriptor is closed: fcntl on it now fails with EBADF. Racy only if
    // something else opened one in between, and nothing in this test does.
    // SAFETY: a plain fcntl query on an integer; EBADF is the expected answer.
    assert_eq!(
        unsafe { libc::fcntl(raw, libc::F_GETFD) },
        -1,
        "the timer descriptor was closed with its watch"
    );
}

#[test]
fn a_cancelled_watch_delivers_nothing() {
    let _guard = serial();
    drain_watches();

    let (reader, writer) = pipe();
    let handle = watch::arm(reader.as_raw_fd(), None, AIO_R, 5).expect("armed");
    let signals_before = vm_ref::signals_sent();

    assert!(watch::retire(handle).is_ok(), "cancelled before it fired");
    (&writer).write_all(b"x").expect("wrote to the pipe");

    assert_eq!(pump(100), 0, "aioDisable took it off the loop");
    assert_eq!(vm_ref::signals_sent(), signals_before, "no doorbell");
    assert!(watch::peek(handle).is_err(), "the handle names nothing now");
    assert!(watch::retire(handle).is_err(), "and cancelling twice fails");
}

/// A handle from a retired watch must not resolve to whatever took its slot --
/// which is what the generation counter in the handle encoding is for.
#[test]
fn a_stale_handle_does_not_resolve_to_the_next_watch() {
    let _guard = serial();
    drain_watches();

    let (first_reader, _first_writer) = pipe();
    let stale = watch::arm(first_reader.as_raw_fd(), None, AIO_R, 1).expect("armed");
    watch::retire(stale).expect("retired");

    let (second_reader, _second_writer) = pipe();
    let fresh = watch::arm(second_reader.as_raw_fd(), None, AIO_R, 2).expect("armed");

    assert_ne!(stale, fresh, "a reused slot still answers a new handle");
    assert!(
        watch::peek(stale).is_err(),
        "the old handle names the old watch, which is gone"
    );
    assert_eq!(watch::peek(fresh).unwrap(), PENDING);

    watch::retire(fresh).expect("cleaned up");
}

/// The condition `shutdownModule` refuses on: `aio.c` holds a pointer into
/// this library's text for as long as a watch is armed.
#[test]
fn the_module_is_not_quiescent_while_a_watch_is_armed() {
    let _guard = serial();
    drain_watches();
    assert!(watch::is_quiescent(), "no watches, nothing registered");

    let (reader, _writer) = pipe();
    let handle = watch::arm(reader.as_raw_fd(), None, AIO_R, 9).expect("armed");
    assert!(
        !watch::is_quiescent(),
        "the poll loop holds on_ready keyed on this fd"
    );

    watch::retire(handle).expect("retired");
    assert!(watch::is_quiescent(), "aioDisable gave it back");
}

/// Writability is a separate event, and a fresh pipe is writable at once --
/// so this also pins that the mask is honoured rather than ignored.
#[test]
fn a_writable_descriptor_fires_for_write_not_read() {
    let _guard = serial();
    drain_watches();

    let (reader, writer) = pipe();
    let write_watch = watch::arm(writer.as_raw_fd(), None, AIO_W, 4).expect("armed");
    let read_watch = watch::arm(reader.as_raw_fd(), None, AIO_R, 4).expect("armed");

    assert_eq!(pump(2_000), 1, "exactly one of the two is ready");
    assert_eq!(
        watch::peek(write_watch).unwrap(),
        AIO_W,
        "an empty pipe is writable"
    );
    assert_eq!(
        watch::peek(read_watch).unwrap(),
        PENDING,
        "and not readable"
    );

    watch::retire(write_watch).expect("retired");
    watch::retire(read_watch).expect("retired");
}
