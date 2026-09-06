//! A one-shot timer as a *descriptor*, so the VM's existing poll loop can wait
//! on it with no reactor rewrite and no thread.
//!
//! That is the whole trick of this plugin: the VM already knows how to wait for
//! a descriptor to become readable, so a timer only has to be something that
//! becomes readable when it expires. Linux has exactly that in `timerfd`; the
//! kqueue platforms get it by nesting a kqueue whose only registered event is
//! an `EVFILT_TIMER`, because a kqueue descriptor is itself readable when it
//! has events pending.
//!
//! Both are created non-blocking and close-on-exec, and neither is ever read
//! from: a watch fires once, is not re-armed, and the descriptor is closed on
//! retirement. Draining it would be work with no observer.

use std::io;
use std::os::fd::OwnedFd;

/// The longest timer this plugin will create.
///
/// Not a technical limit -- both back ends accept far more -- but a bound on
/// what an image can ask the VM to hold a descriptor open for. Roughly 24 days,
/// which is past any plausible use and short of the point where the
/// millisecond arithmetic below has to be thought about.
pub const MAX_DELAY_MS: i64 = 1 << 31;

/// A descriptor that becomes readable `millis` from now, once.
#[cfg(target_os = "linux")]
pub fn after(millis: i64) -> io::Result<OwnedFd> {
    use std::os::fd::FromRawFd;

    // SAFETY: a plain libc call; the descriptor is adopted by OwnedFd below,
    // which is the only owner from here on.
    let raw = unsafe {
        libc::timerfd_create(
            libc::CLOCK_MONOTONIC,
            libc::TFD_NONBLOCK | libc::TFD_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor this call owns.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };

    // `it_interval` zero is what makes it one-shot. A zero `it_value` would
    // *disarm* the timer rather than fire immediately, so a request for 0 ms
    // becomes 1 ns -- the earliest a timerfd can be asked to expire.
    let spec = libc::itimerspec {
        it_interval: libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        },
        it_value: if millis <= 0 {
            libc::timespec {
                tv_sec: 0,
                tv_nsec: 1,
            }
        } else {
            libc::timespec {
                tv_sec: millis / 1000,
                tv_nsec: (millis % 1000) * 1_000_000,
            }
        },
    };
    // SAFETY: a live descriptor from timerfd_create and a fully initialised
    // itimerspec; no old value is requested.
    let armed = unsafe {
        libc::timerfd_settime(
            std::os::fd::AsRawFd::as_raw_fd(&fd),
            0,
            &spec,
            std::ptr::null_mut(),
        )
    };
    if armed < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

/// A descriptor that becomes readable `millis` from now, once.
///
/// The kqueue platforms have no `timerfd`, but they do have `EVFILT_TIMER`, and
/// a kqueue descriptor is readable exactly when it has an event pending -- so a
/// kqueue holding one one-shot timer *is* a timer descriptor. `EV_ONESHOT`
/// makes the kernel drop the registration after it fires, which matches this
/// plugin never re-arming.
///
/// **Cross-checked, not run**: no kqueue platform was available to execute
/// this. See the crate README.
#[cfg(not(target_os = "linux"))]
pub fn after(millis: i64) -> io::Result<OwnedFd> {
    use std::os::fd::{AsRawFd, FromRawFd};

    // SAFETY: a plain libc call; adopted by OwnedFd immediately below.
    let raw = unsafe { libc::kqueue() };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `raw` is a fresh descriptor this call owns.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // kqueue() does not take a flags argument, so close-on-exec is set after
    // the fact rather than at creation as on Linux.
    // SAFETY: a live descriptor of ours.
    unsafe {
        libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
    }

    // `data` is the timeout in the units `fflags` selects; the default is
    // milliseconds, which is what this plugin's contract is stated in.
    let change = libc::kevent {
        ident: 1,
        filter: libc::EVFILT_TIMER,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
        fflags: 0,
        data: millis.max(0) as _,
        udata: std::ptr::null_mut(),
    };
    // SAFETY: one fully initialised change, no events requested, no timeout.
    let registered = unsafe {
        libc::kevent(
            fd.as_raw_fd(),
            &change,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if registered < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    /// A timer becomes readable, and not before it should.
    #[test]
    fn a_timer_fires_after_its_delay_and_not_before() {
        let fd = after(120).expect("a timer descriptor");
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };

        // SAFETY: one well-formed pollfd.
        let early = unsafe { libc::poll(&mut poll, 1, 20) };
        assert_eq!(early, 0, "not readable 20 ms into a 120 ms timer");

        poll.revents = 0;
        // SAFETY: as above.
        let late = unsafe { libc::poll(&mut poll, 1, 2_000) };
        assert_eq!(late, 1, "readable once the delay has passed");
        assert_ne!(poll.revents & libc::POLLIN, 0);
    }

    /// Zero is "as soon as possible", not "never" -- the distinction a bare
    /// `timerfd_settime` gets wrong, since a zero `it_value` disarms.
    #[test]
    fn a_zero_delay_still_fires() {
        let fd = after(0).expect("a timer descriptor");
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one well-formed pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, 2_000) };
        assert_eq!(ready, 1, "a zero delay fires rather than disarming");
    }
}
