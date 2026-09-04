//! The socket state machine: `SQSocket` records, the private per-socket
//! state, the aio handlers, and the `sqSocket*` operations from
//! `SocketPluginImpl.c`.
//!
//! # The two-layer socket representation
//!
//! The image holds a socket as a ByteArray of `sizeof(SQSocket)` bytes: the
//! session ID, the socket type, and one pointer to heap state. That outer
//! record's layout is frozen -- `UnixOSProcessPlugin` sizes its own ByteArrays
//! from it. The pointed-to state was `struct privateSocketStruct`; here it is
//! [`PrivateSocket`], a Rust type this crate owns. Its layout is private to
//! this plugin **except for one field**: `UnixOSProcessPlugin`'s
//! `socketDescriptorFrom:` reads `*(int *)privateSocketPtr` to recover the OS
//! file descriptor, with a comment admitting it "will break if anyone ever
//! redefines the data structure". So [`PrivateSocket`] is `repr(C)` with the
//! descriptor first, and everything after that first `int` is genuinely
//! private.
//!
//! # Failure convention
//!
//! `Err(PrimErr::GenericFailure)` stands for the C's `success(false)` /
//! `primitiveFail()`; the callers in `lib.rs` add the `PrimErrBadArgument`
//! shape checks the generated code performed before calling in here.
//!
//! # Safety
//!
//! The functions here are `unsafe fn`s taking raw `*mut SQSocket` pointers,
//! exactly as the C worked: the pointer aims into object memory (or, in tests,
//! at a stack record) and stays valid for the duration of the call, during
//! which nothing allocates object memory. The aio handlers run on the
//! interpreter thread from the VM's poll loop, never concurrently with a
//! primitive.

use core::ffi::{c_int, c_void};
use core::mem;
use core::ptr;
use std::time::Duration;

use pharo_vm_plugin::{sqInt, PrimErr, PrimResult};

use crate::aio;
use crate::options;
use crate::resolver;
use crate::vm_ref;

// Socket types, as the image numbers them.
pub const TCP_SOCKET_TYPE: c_int = 0;
pub const UDP_SOCKET_TYPE: c_int = 1;
pub const RAW_SOCKET_TYPE: c_int = 2;

/// Offset marking a socket the environment provided (systemd socket
/// activation).
pub const REUSE_EXISTING_SOCKET: c_int = 65536;
pub const PROVIDED_TCP_SOCKET_TYPE: c_int = TCP_SOCKET_TYPE + REUSE_EXISTING_SOCKET;

/// This build has no systemd support (`HAVE_SD_DAEMON` is never defined by
/// the CMake tree), so `sd_listen_fds` is the header's stub answering 0 and a
/// provided-TCP request adopts file descriptor `SD_LISTEN_FDS_START` = 3
/// unconditionally -- odd, but exactly what the C compiles to here.
const SD_LISTEN_FDS_START: c_int = 3;

// TCP socket states, as the image knows them.
pub const INVALID: c_int = -1;
pub const UNCONNECTED: c_int = 0;
pub const WAITING_FOR_CONNECTION: c_int = 1;
pub const CONNECTED: c_int = 2;
pub const OTHER_END_CLOSED: c_int = 3;
pub const THIS_END_CLOSED: c_int = 4;

const LINGER_SECS: c_int = 1;

// notify() masks.
const CONN_NOTIFY: c_int = 1 << 0;
const READ_NOTIFY: c_int = 1 << 1;
const WRITE_NOTIFY: c_int = 1 << 2;

/// `union sockaddr_any`: big enough for AF_UNIX, AF_INET and AF_INET6 peers.
#[repr(C)]
#[derive(Clone, Copy)]
pub union SockAddrAny {
    pub sa: libc::sockaddr,
    pub saun: libc::sockaddr_un,
    pub sin: libc::sockaddr_in,
    pub sin6: libc::sockaddr_in6,
}

/// The record the image's socket ByteArray holds -- `SQSocket` in
/// `SocketPlugin.h`. Layout is ABI: `UnixOSProcessPlugin` (and the image, via
/// `socketRecordSize`) depends on its size.
#[repr(C)]
pub struct SQSocket {
    pub session_id: c_int,
    /// 0 = TCP, 1 = UDP (2 = RAW), as the header documents.
    pub socket_type: c_int,
    pub private: *mut PrivateSocket,
}

impl SQSocket {
    /// A zeroed record, as a freshly instantiated ByteArray reads.
    #[cfg(test)]
    pub fn zeroed() -> Self {
        // SAFETY: all-zero bits are a valid SQSocket (null private pointer).
        unsafe { mem::zeroed() }
    }
}

/// `struct privateSocketStruct`: the per-socket heap state.
///
/// `repr(C)` with `fd` first is load-bearing: see the module comment. The
/// remaining fields are free to change.
#[repr(C)]
pub struct PrivateSocket {
    /// The OS file descriptor. MUST remain the first field: other plugins
    /// read it through the raw pointer.
    pub fd: c_int,
    conn_sema: c_int,
    read_sema: c_int,
    write_sema: c_int,
    sock_state: c_int,
    sock_error: c_int,
    peer: SockAddrAny,
    peer_size: libc::socklen_t,
    multi_listen: c_int,
    accepted_sock: c_int,
    socket_type: c_int,
    waiting_to_send: c_int,
}

impl PrivateSocket {
    /// calloc-equivalent: the C allocates these zeroed.
    fn boxed_zeroed() -> *mut PrivateSocket {
        // SAFETY: all-zero bits are valid for every field (the union included).
        Box::into_raw(Box::new(unsafe { mem::zeroed() }))
    }
}

/// errno immediately after a failed call (`getLastSocketError` on Unix).
fn last_os_error() -> c_int {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `socketValid`: non-null record and state, live session, matching stamp.
///
/// # Safety
/// `s` must be null or point at a readable `SQSocket`.
pub unsafe fn socket_valid(s: *mut SQSocket) -> bool {
    !s.is_null()
        && !(*s).private.is_null()
        && resolver::current_session() != 0
        && (*s).session_id == resolver::current_session()
}

/// Runs `f` with the descriptor as a borrowed `socket2` socket.
///
/// `PrivateSocket::fd` is the ABI-frozen `int` that `UnixOSProcessPlugin`
/// reads out of the record, so this plugin cannot *own* a `socket2::Socket`
/// there. `SockRef` borrows a descriptor without taking ownership of it,
/// which is exactly the C's model: the descriptor is closed by
/// `sqSocketDestroy`, never by a scope ending.
///
/// Answers `None` for a negative descriptor, which several callers do pass:
/// the C handed `-1` straight to `setsockopt`/`getsockopt` and got `EBADF`
/// back, whereas `BorrowedFd` cannot represent `-1` at all and panics. `None`
/// is that `EBADF`, and each caller maps it to whatever the C did with the
/// failure.
///
/// # Safety
/// A non-negative `fd` must be an open descriptor for the duration of the
/// call.
unsafe fn with_socket<R>(fd: c_int, f: impl FnOnce(socket2::SockRef<'_>) -> R) -> Option<R> {
    if fd < 0 {
        return None;
    }
    // SAFETY: the caller guarantees a non-negative `fd` is open, and the
    // borrow ends with this call, so nothing can close it underneath the
    // reference.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    Some(f(socket2::SockRef::from(&borrowed)))
}

/// `setLinger`: linger for a second on close (or not at all).
///
/// The C built a `struct linger { l_onoff: flag, l_linger: flag * 1 }`, so a
/// true flag is "linger one second" and a false one is "do not linger" --
/// which is what `Option<Duration>` says directly. Errors are ignored, as in
/// C.
fn set_linger(fd: c_int, flag: c_int) {
    let linger = (flag != 0).then(|| Duration::from_secs(LINGER_SECS as u64));
    // SAFETY: callers hold an open descriptor.
    let _ = unsafe { with_socket(fd, |s| s.set_linger(linger)) };
}

/// `socketReadable`: 1 readable, 0 would block, -1 no longer connected.
fn socket_readable(fd: c_int, socket_type: c_int) -> c_int {
    let mut buf = [0u8; 100];
    // SAFETY: MSG_PEEK into a local buffer.
    let n = unsafe {
        if socket_type == UDP_SOCKET_TYPE {
            libc::recvfrom(
                fd,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                libc::MSG_PEEK,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } else {
            libc::recv(fd, buf.as_mut_ptr() as *mut c_void, buf.len(), libc::MSG_PEEK)
        }
    };
    if n > 0 {
        return 1;
    }
    if n < 0 && last_os_error() == libc::EWOULDBLOCK {
        return 0;
    }
    -1 // EOF
}

/// `socketError`: the pending error condition on a descriptor.
///
/// `take_error` is `getsockopt(SO_ERROR)`, and like the C it clears the
/// pending error as a side effect. -1 stands for a failed `getsockopt`, 0 for
/// no pending error.
fn socket_error_of(fd: c_int) -> c_int {
    // SAFETY: callers hold an open descriptor.
    match unsafe { with_socket(fd, |s| s.take_error()) } {
        // No descriptor, or getsockopt itself failed: the C answered -1.
        None | Some(Err(_)) => -1,
        Some(Ok(None)) => 0,
        Some(Ok(Some(e))) => e.raw_os_error().unwrap_or(0),
    }
}

/// The `notify` macro: signal the semaphores named by `mask`.
///
/// # Safety
/// `pss` must point at a live `PrivateSocket`.
unsafe fn notify(pss: *mut PrivateSocket, mask: c_int) {
    if mask & CONN_NOTIFY != 0 {
        vm_ref::signal_semaphore((*pss).conn_sema);
    }
    if mask & READ_NOTIFY != 0 {
        vm_ref::signal_semaphore((*pss).read_sema);
    }
    if mask & WRITE_NOTIFY != 0 {
        vm_ref::signal_semaphore((*pss).write_sema);
    }
}

/// `sendData` for a UDP/RAW socket: one datagram to the peer recorded in
/// [`PrivateSocket::peer`]. Answers the `ssize_t` `send`/`sendto` answered,
/// with `errno` still set from it.
///
/// The C is one line -- `sendto(SOCKET(s), buf, bufSize, 0,
/// (struct sockaddr *)&SOCKETPEER(s), sizeof(SOCKETPEER(s)))` -- and that
/// line does not work on Darwin, for two independent reasons. Both are the
/// primitive failing outright rather than behaving oddly, on the one platform
/// this port had never been run on, so Darwin gets a branch; Linux keeps the
/// C verbatim below.
///
/// 1. **The length.** `sizeof(SOCKETPEER(s))` is the size of the whole
///    `union sockaddr_any` (108 bytes here, 112 on glibc), passed for what is
///    really a 16-byte `sockaddr_in`. Linux's `move_addr_to_kernel` copies the prefix it needs
///    and objects only when the length is too *small* for the family. Darwin
///    stamps the argument into the kernel's copy of the address as its
///    `sa_len`, and the source-address selection the stack runs for a socket
///    that has no local address yet then insists on exactly
///    `sizeof(struct sockaddr_in)` and answers `EINVAL`. Measured on
///    aarch64-apple-darwin: that same oversized length goes through untouched
///    once the socket *is* bound, because that path is not taken -- so this
///    half of the bug is latent rather than constant, and shows up on the
///    datagram a socket sends before it has acquired a local address.
///
/// 2. **The destination.** [`connect_to_port`] and [`connect_to_address`] call
///    `connect(2)` on UDP sockets. On a connected socket the BSD stack refuses
///    a `sendto` that carries a destination at all -- `EISCONN` -- where Linux
///    accepts it and sends to the address given. This half is not latent: it
///    is every `sendData:` after a `connectTo:port:`, which is the ordinary
///    way an image drives a UDP socket. `send(2)` is the same operation
///    without the redundant address, and reaches the same peer: a connected
///    UDP socket only ever receives from the address it is connected to, so
///    the `peer` that [`receive_data`] keeps refreshing cannot drift away from
///    the kernel's.
///
/// This is a **divergence**, not a faithful port, and a deliberate one: being
/// faithful to `sizeof(SOCKETPEER(s))` and to the redundant destination means
/// UDP `sendData:` does not work on macOS at all. Reason 2 above is the same
/// bug in the C, at `plugins/SocketPlugin/src/common/SocketPluginImpl.c:1334`,
/// and is worth filing there.
///
/// Only this entry point is treated. `send_udp_to`
/// (`sqSockettoHostportSendDataBufCount`, the C's line 1428, reached from
/// `primitiveSocketSendUDPDataBufCount`) still carries the C's shape: it
/// always passes a destination, so on Darwin it is subject to the same
/// `EISCONN` rule if the image ever calls it on a socket `connect(2)` has been
/// called on. Its length is already the family's own `sizeof(struct
/// sockaddr_in)`, so reason 1 does not touch it. It is left alone because the
/// image drives it with an explicit host and port -- the case where connecting
/// first makes no sense -- and diverging from the C where nothing is known to
/// be broken buys nothing.
///
/// The peer's family, not `PrivateSocket::peer_size`, decides the length --
/// even though that field is the C's own "dynamic sizeof(peer)".
/// [`receive_data`] zeroes `peer_size` before *every* `recvfrom` and restores
/// it only after a successful one, so on the ordinary would-block polling path
/// it reads zero while `peer` still holds a good address. The family in the
/// union is always in step with the bytes; the size field is not.
///
/// # Safety
/// `fd` is open, `peer` points at an initialised `SockAddrAny`, and `buf`
/// spans `buf_size` readable bytes.
#[cfg(any(target_os = "macos", target_os = "ios"))]
unsafe fn udp_send(
    fd: c_int,
    peer: *const SockAddrAny,
    buf: *const u8,
    buf_size: usize,
) -> libc::ssize_t {
    // `getpeername` succeeds exactly when `connect(2)` has been called, which
    // is the condition Darwin's `EISCONN` is really about; `sock_state` cannot
    // stand in for it, because `create` marks every UDP socket CONNECTED at
    // birth ("UDP sockets are born connected") without any connect(2).
    //
    // The probe is errno-visible, so it is bracketed. On the unconnected path
    // it fails and leaves `errno = ENOTCONN`, and `send_data` treats every
    // `nsent <= 0` as the error path and reads `last_os_error()` there --
    // which a *successful* zero-length datagram reaches, because `sendto`
    // answers 0 for one and leaves errno alone. Without the bracket the image
    // would read `ENOTCONN` out of `sqSocketError` after a send that worked.
    // The C ran no probe and so left whatever errno was already there; saving
    // and restoring is the closest this can come to not having asked.
    let saved_errno = *libc::__error();
    let mut named: SockAddrAny = mem::zeroed();
    let mut named_len = mem::size_of::<SockAddrAny>() as libc::socklen_t;
    let connected = libc::getpeername(
        fd,
        &mut named as *mut SockAddrAny as *mut libc::sockaddr,
        &mut named_len,
    ) == 0;
    *libc::__error() = saved_errno;
    if connected {
        return libc::send(fd, buf as *const c_void, buf_size, 0);
    }
    let addr_len = match i32::from((*peer).sa.sa_family) {
        libc::AF_INET => mem::size_of::<libc::sockaddr_in>(),
        libc::AF_INET6 => mem::size_of::<libc::sockaddr_in6>(),
        libc::AF_UNIX => mem::size_of::<libc::sockaddr_un>(),
        // Nothing else can reach `peer`; if it did, the C's length is still
        // the least surprising answer.
        _ => mem::size_of::<SockAddrAny>(),
    };
    libc::sendto(
        fd,
        buf as *const c_void,
        buf_size,
        0,
        peer as *const libc::sockaddr,
        addr_len as libc::socklen_t,
    )
}

/// `sendData` for a UDP/RAW socket, as the C wrote it: one `sendto` to
/// `SOCKETPEER(s)` with `sizeof(SOCKETPEER(s))` as the length. See the Darwin
/// variant above for why the two platforms differ here at all.
///
/// # Safety
/// As the Darwin variant.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
unsafe fn udp_send(
    fd: c_int,
    peer: *const SockAddrAny,
    buf: *const u8,
    buf_size: usize,
) -> libc::ssize_t {
    libc::sendto(
        fd,
        buf as *const c_void,
        buf_size,
        0,
        peer as *const libc::sockaddr,
        mem::size_of::<SockAddrAny>() as libc::socklen_t,
    )
}

fn make_sockaddr_in(addr: u32, port: u16) -> libc::sockaddr_in {
    // SAFETY: all-zero is a valid sockaddr_in.
    let mut sin: libc::sockaddr_in = unsafe { mem::zeroed() };
    sin.sin_family = libc::AF_INET as libc::sa_family_t;
    sin.sin_port = port.to_be(); // htons
    sin.sin_addr.s_addr = addr.to_be(); // htonl
    sin
}

// ---------------------------------------------------------------------------
// aio handlers -- run from the VM's poll loop on the interpreter thread
// ---------------------------------------------------------------------------

/// `acceptHandler`: an incoming connection (or a listen error) on a server
/// socket.
///
/// # Safety
/// `data` is the `PrivateSocket` registered with `aioEnable`, still live.
pub unsafe extern "C" fn accept_handler(fd: sqInt, data: *mut c_void, flags: c_int) {
    let fd = fd as c_int;
    let pss = data as *mut PrivateSocket;
    if flags & aio::AIO_X != 0 {
        // error during listen()
        aio::disable(fd);
        (*pss).sock_error = socket_error_of(fd);
        (*pss).sock_state = INVALID;
        (*pss).fd = -1;
        (*pss).waiting_to_send = 0;
        libc::close(fd);
    } else {
        // accept() is ready. `accept_raw` for the same reason as `new_raw`
        // at socket creation: the C's accept() leaves FD_CLOEXEC clear.
        // (Only that reason -- socket2's `accept` goes through
        // `set_common_accept_flags`, which sets FD_CLOEXEC and, as its own doc
        // says, deliberately does not touch SO_NOSIGPIPE because an accepted
        // socket inherits it from the listener.)
        //
        // The error is carried out of the call rather than re-read from
        // errno afterwards, so nothing in between can disturb it.
        let accepted = with_socket(fd, |s| s.accept_raw())
            .unwrap_or_else(|| Err(std::io::Error::from_raw_os_error(libc::EBADF)));
        match accepted {
            Err(e) => {
                let errno = e.raw_os_error().unwrap_or(0);
                if errno == libc::ECONNABORTED {
                    // let's just pretend this never happened
                    aio::handle(fd, accept_handler, aio::AIO_RX);
                    return;
                }
                (*pss).sock_error = errno;
                (*pss).sock_state = INVALID;
                aio::disable(fd);
                libc::close(fd);
            }
            Ok((accepted, _peer)) => {
                let new_sock = std::os::fd::IntoRawFd::into_raw_fd(accepted);
                (*pss).sock_state = CONNECTED;
                set_linger(new_sock, 1);
                if (*pss).multi_listen != 0 {
                    if (*pss).accepted_sock > 0 {
                        // an earlier accept was never collected; drop it
                        set_linger((*pss).accepted_sock, 0);
                        libc::close((*pss).accepted_sock);
                    }
                    (*pss).accepted_sock = new_sock;
                } else {
                    // traditional listen: replace server with client in place
                    aio::disable(fd);
                    libc::close(fd);
                    (*pss).fd = new_sock;
                    (*pss).waiting_to_send = 0;
                    aio::enable(new_sock, pss as *mut c_void, 0);
                }
            }
        }
    }
    notify(pss, CONN_NOTIFY);
}

/// `connectHandler`: an asynchronous connect() completed (or failed).
///
/// # Safety
/// As [`accept_handler`].
pub unsafe extern "C" fn connect_handler(fd: sqInt, data: *mut c_void, flags: c_int) {
    let fd = fd as c_int;
    let pss = data as *mut PrivateSocket;

    // If aio called us but the socket was already resolved, just return;
    // avoids a race in the aio machinery.
    if (*pss).sock_state != WAITING_FOR_CONNECTION {
        aio::disable(fd);
        return;
    }

    let error = socket_error_of(fd);
    // The C separates the exception case from the completed-with-error case,
    // but only the log lines differ; the state transitions are identical.
    if flags & aio::AIO_X != 0 || error != 0 {
        aio::disable(fd);
        (*pss).sock_error = error;
        (*pss).sock_state = UNCONNECTED;
    } else {
        (*pss).sock_state = CONNECTED;
        set_linger((*pss).fd, 1);
    }
    notify(pss, CONN_NOTIFY);
}

/// `sendHandler`: the socket can be written again.
///
/// # Safety
/// As [`accept_handler`].
pub unsafe extern "C" fn send_handler(_fd: sqInt, data: *mut c_void, _flags: c_int) {
    let pss = data as *mut PrivateSocket;
    if pss.is_null() {
        return;
    }
    (*pss).waiting_to_send = 0;
    notify(pss, WRITE_NOTIFY);
    notify(pss, READ_NOTIFY);
}

/// `dataHandler`: data (or out-of-band data) arrived.
///
/// # Safety
/// As [`accept_handler`].
pub unsafe extern "C" fn data_handler(fd: sqInt, data: *mut c_void, flags: c_int) {
    let fd = fd as c_int;
    let pss = data as *mut PrivateSocket;
    if pss.is_null() {
        return;
    }
    if flags & aio::AIO_R != 0 {
        let n = socket_readable(fd, (*pss).socket_type);
        if n == 0 {
            // Maybe OOB data woke us; Squeak cannot read OOB, so discard it
            // and keep waiting.
            let mut buf = [0u8; 1];
            libc::recv(fd, buf.as_mut_ptr() as *mut c_void, 1, libc::MSG_OOB);
            aio::handle(fd, data_handler, aio::AIO_RX);
            return;
        }
        if n != 1 {
            (*pss).sock_error = socket_error_of(fd);
            (*pss).sock_state = OTHER_END_CLOSED;
        }
    }
    if flags & aio::AIO_X != 0 {
        // assume out-of-band data has arrived; discard it (ho hum)
        let mut buf = [0u8; 1];
        libc::recv(fd, buf.as_mut_ptr() as *mut c_void, 1, libc::MSG_OOB);
    }
    if flags & aio::AIO_R != 0 {
        notify(pss, READ_NOTIFY);
    }
}

/// `closeHandler`: a deferred close() finished.
///
/// # Safety
/// As [`accept_handler`].
pub unsafe extern "C" fn close_handler(fd: sqInt, data: *mut c_void, _flags: c_int) {
    let fd = fd as c_int;
    let pss = data as *mut PrivateSocket;
    aio::disable(fd);
    libc::close(fd);
    (*pss).sock_state = UNCONNECTED;
    (*pss).fd = -1;
    (*pss).waiting_to_send = 0;
    notify(pss, READ_NOTIFY | CONN_NOTIFY);
}

// ---------------------------------------------------------------------------
// Creation
// ---------------------------------------------------------------------------

/// `sqSocketCreateNetTypeSocketTypeRecvBytesSendBytesSemaIDReadSemaIDWriteSemaID`.
///
/// The receive/send buffer size arguments are accepted and ignored, as they
/// were in C.
///
/// # Safety
/// `s` points at a live, writable `SQSocket` record.
#[allow(clippy::too_many_arguments)] // the C entry point's arity, kept as is
pub unsafe fn create(
    s: *mut SQSocket,
    net_type: sqInt,
    socket_type: sqInt,
    _recv_buf_size: sqInt,
    _send_buf_size: sqInt,
    sema_index: sqInt,
    read_sema_index: sqInt,
    write_sema_index: sqInt,
) -> PrimResult<()> {
    // Unknown domain codes fall through unchanged, as the C switch's missing
    // default lets them.
    let domain = match net_type {
        0 | 2 => libc::AF_INET,  // UNSPECIFIED, INET4
        1 => libc::AF_UNIX,      // LOCAL
        3 => libc::AF_INET6,     // INET6
        other => other as c_int,
    };
    let mut socket_type = socket_type as c_int;

    (*s).session_id = 0;
    // `new_raw`, not `new`: socket2's `new` sets FD_CLOEXEC and the C's bare
    // `socket()` does not. The difference is observable -- UnixOSProcessPlugin
    // forks and execs, and a descriptor that vanished across exec would change
    // what child processes inherit. FD_CLOEXEC is the whole reason, on both
    // platforms. On Apple targets `new` also sets SO_NOSIGPIPE -- `new` only;
    // `accept` routes through socket2's `set_common_accept_flags`, which
    // leaves NOSIGPIPE to be inherited from the listener -- but that flag is
    // not observable here either way: `installErrorHandlers` sets SIGPIPE to
    // SIG_IGN for the whole process (src/unix/debugUnix.c:150-152, "Ignore all
    // broken pipe signals. They will be reported as normal errors by send()
    // and write()"), so a send on a broken connection reports EPIPE on Linux
    // and on Darwin alike, with or without it.
    let make = |ty: socket2::Type| {
        socket2::Socket::new_raw(socket2::Domain::from(domain), ty, None)
            .map(std::os::fd::IntoRawFd::into_raw_fd)
            .unwrap_or(-1)
    };
    let new_socket = if socket_type == TCP_SOCKET_TYPE {
        make(socket2::Type::STREAM)
    } else if socket_type == UDP_SOCKET_TYPE {
        make(socket2::Type::DGRAM)
    } else if socket_type == PROVIDED_TCP_SOCKET_TYPE {
        // See SD_LISTEN_FDS_START: the no-systemd stub adopts fd 3.
        socket_type = TCP_SOCKET_TYPE;
        SD_LISTEN_FDS_START
    } else {
        -1
    };
    if new_socket == -1 {
        // socket() failed, or incorrect socketType
        return Err(PrimErr::GenericFailure);
    }
    // Errors ignored, as in C.
    let _ = with_socket(new_socket, |s| s.set_reuse_address(true));

    let pss = PrivateSocket::boxed_zeroed();
    (*pss).fd = new_socket;
    (*pss).waiting_to_send = 0;
    (*pss).conn_sema = sema_index as c_int;
    (*pss).read_sema = read_sema_index as c_int;
    (*pss).write_sema = write_sema_index as c_int;
    (*pss).socket_type = socket_type;

    // UDP sockets are born "connected".
    if socket_type == UDP_SOCKET_TYPE {
        (*pss).sock_state = CONNECTED;
        aio::enable((*pss).fd, pss as *mut c_void, 0);
    } else {
        (*pss).sock_state = UNCONNECTED;
    }
    (*pss).sock_error = 0;
    // initial UDP peer := wildcard
    (*pss).peer.sin = make_sockaddr_in(u32::from_be(libc::INADDR_ANY), 0);

    (*s).session_id = resolver::current_session();
    (*s).socket_type = socket_type;
    (*s).private = pss;
    // Note: socket is in BLOCKING mode until aioEnable is called for it.
    Ok(())
}

/// `sqSocketCreateRawProtoTypeRecvBytesSendBytesSemaIDReadSemaIDWriteSemaID`:
/// only protocol 1 (ICMP over AF_INET) is supported, as in C.
///
/// # Safety
/// As [`create`].
#[allow(clippy::too_many_arguments)] // the C entry point's arity, kept as is
pub unsafe fn create_raw(
    s: *mut SQSocket,
    _net_type: sqInt,
    protocol: sqInt,
    _recv_buf_size: sqInt,
    _send_buf_size: sqInt,
    sema_index: sqInt,
    read_sema_index: sqInt,
    write_sema_index: sqInt,
) -> PrimResult<()> {
    (*s).session_id = 0;
    let new_socket = match protocol {
        1 => libc::socket(libc::AF_INET, libc::SOCK_RAW, libc::IPPROTO_ICMP),
        _ => -1,
    };
    if new_socket == -1 {
        return Err(PrimErr::GenericFailure);
    }

    let pss = PrivateSocket::boxed_zeroed();
    (*pss).fd = new_socket;
    (*pss).waiting_to_send = 0;
    (*pss).conn_sema = sema_index as c_int;
    (*pss).read_sema = read_sema_index as c_int;
    (*pss).write_sema = write_sema_index as c_int;
    // The C copies the record's (still zeroed) socketType field here, not
    // RAWSocketType -- reproduced faithfully.
    (*pss).socket_type = (*s).socket_type;

    // RAW sockets are born "connected".
    (*pss).sock_state = CONNECTED;
    aio::enable((*pss).fd, pss as *mut c_void, 0);
    (*pss).sock_error = 0;
    (*pss).peer.sin = make_sockaddr_in(u32::from_be(libc::INADDR_ANY), 0);

    (*s).session_id = resolver::current_session();
    (*s).socket_type = RAW_SOCKET_TYPE;
    (*s).private = pss;
    Ok(())
}

// ---------------------------------------------------------------------------
// Status and lifecycle
// ---------------------------------------------------------------------------

/// `sqSocketConnectionStatus`. A socket a handler marked `Invalid` gets its
/// private pointer cleared here -- without freeing it, matching the C's
/// deliberate "safer not to free" leak -- and the primitive fails.
///
/// # Safety
/// As [`create`].
pub unsafe fn connection_status(s: *mut SQSocket) -> PrimResult<c_int> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    if (*(*s).private).sock_state == INVALID {
        // Intentional leak, as in C: the handler that invalidated the socket
        // already closed the descriptor, and freeing here risked a dangling
        // aio client-data pointer.
        (*s).private = ptr::null_mut();
        return Err(PrimErr::GenericFailure);
    }
    Ok((*(*s).private).sock_state)
}

/// `sqSocketListenOnPortBacklogSizeInterface`. The bind result is ignored --
/// the C ignores it too, leaving errors to surface via the accept handler.
///
/// # Safety
/// As [`create`].
pub unsafe fn listen_on_port_backlog_interface(
    s: *mut SQSocket,
    port: sqInt,
    backlog_size: sqInt,
    addr: u32,
) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    // only TCP sockets have a backlog
    if backlog_size > 1 && (*s).socket_type != TCP_SOCKET_TYPE {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    (*pss).multi_listen = c_int::from(backlog_size > 1);
    let saddr = make_sockaddr_in(addr, port as u16);
    libc::bind(
        (*pss).fd,
        &saddr as *const libc::sockaddr_in as *const libc::sockaddr,
        mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    );
    if (*s).socket_type == TCP_SOCKET_TYPE {
        // Errors ignored here, as in C; acceptHandler surfaces them.
        let _ = with_socket((*pss).fd, |sock| sock.listen(backlog_size as c_int));
        (*pss).sock_state = WAITING_FOR_CONNECTION;
        aio::enable((*pss).fd, pss as *mut c_void, 0);
        aio::handle((*pss).fd, accept_handler, aio::AIO_RX); // R => accept()
    }
    Ok(())
}

/// `sqSocketListenOnPortBacklogSize`.
///
/// # Safety
/// As [`create`].
pub unsafe fn listen_on_port_backlog(
    s: *mut SQSocket,
    port: sqInt,
    backlog_size: sqInt,
) -> PrimResult<()> {
    listen_on_port_backlog_interface(s, port, backlog_size, u32::from_be(libc::INADDR_ANY))
}

/// `sqSocketListenOnPort`: TCP starts listening; UDP just binds the port.
///
/// # Safety
/// As [`create`].
pub unsafe fn listen_on_port(s: *mut SQSocket, port: sqInt) -> PrimResult<()> {
    listen_on_port_backlog(s, port, 1)
}

/// `sqSocketListenBacklog`: listen without binding (the address came from
/// `sqSocketBindToAddressSize`).
///
/// # Safety
/// As [`create`].
pub unsafe fn listen_backlog(s: *mut SQSocket, backlog_size: sqInt) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    if backlog_size > 1 && (*s).socket_type != TCP_SOCKET_TYPE {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    (*pss).multi_listen = c_int::from(backlog_size > 1);
    if (*s).socket_type == TCP_SOCKET_TYPE {
        // acceptHandler catches errors
        let _ = with_socket((*pss).fd, |sock| sock.listen(backlog_size as c_int));
        (*pss).sock_state = WAITING_FOR_CONNECTION;
        aio::enable((*pss).fd, pss as *mut c_void, 0);
        aio::handle((*pss).fd, accept_handler, aio::AIO_RX);
    }
    Ok(())
}

/// The common tail of both connect entry points: interpret the result of a
/// nonblocking `connect` on a TCP socket.
///
/// # Safety
/// `pss` live; `fd` is its descriptor.
unsafe fn finish_tcp_connect(pss: *mut PrivateSocket, result: c_int) {
    let last_error = last_os_error();
    if result == 0 {
        // connection completed synchronously
        (*pss).sock_state = CONNECTED;
        notify(pss, CONN_NOTIFY);
        set_linger((*pss).fd, 1);
    } else if last_error == libc::EINPROGRESS || last_error == libc::EWOULDBLOCK {
        // asynchronous connection in progress
        (*pss).sock_state = WAITING_FOR_CONNECTION;
        aio::handle((*pss).fd, connect_handler, aio::AIO_WX); // W => connect()
    } else {
        // connection error
        (*pss).sock_state = UNCONNECTED;
        (*pss).sock_error = last_error;
        notify(pss, CONN_NOTIFY);
    }
}

/// `sqSocketConnectToPort`: TCP opens a connection; UDP/RAW set the peer.
///
/// # Safety
/// As [`create`].
pub unsafe fn connect_to_port(s: *mut SQSocket, addr: u32, port: sqInt) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let saddr = make_sockaddr_in(addr, port as u16);
    if (*s).socket_type != TCP_SOCKET_TYPE {
        // UDP/RAW
        if (*pss).fd >= 0 {
            (*pss).peer.sin = saddr;
            (*pss).peer_size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
            let result = libc::connect(
                (*pss).fd,
                &saddr as *const libc::sockaddr_in as *const libc::sockaddr,
                mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            );
            if result == 0 {
                (*pss).sock_state = CONNECTED;
            }
        }
    } else {
        aio::enable((*pss).fd, pss as *mut c_void, 0);
        let result = libc::connect(
            (*pss).fd,
            &saddr as *const libc::sockaddr_in as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        finish_tcp_connect(pss, result);
    }
    Ok(())
}

/// `sqSocketAcceptFromRecvBytesSendBytesSemaIDReadSemaIDWriteSemaID`: collect
/// a connection the accept handler queued on a multi-listen server socket.
///
/// # Safety
/// `s` and `server` as in [`create`]; `s` is a freshly instantiated record.
pub unsafe fn accept_from(
    s: *mut SQSocket,
    server: *mut SQSocket,
    sema_index: sqInt,
    read_sema_index: sqInt,
    write_sema_index: sqInt,
) -> PrimResult<()> {
    // The image has already called waitForConnection, so there is no need to
    // signal the server's connection semaphore again.
    if !socket_valid(server) || (*(*server).private).multi_listen == 0 {
        return Err(PrimErr::GenericFailure);
    }
    // Check that a connection is there. (`< 0`, not `<= 0`, as in C: a fresh
    // server record's acceptedSock of 0 passes -- the image only calls accept
    // after the connection semaphore fired, which makes this unreachable.)
    if (*(*server).private).accepted_sock < 0 {
        return Err(PrimErr::GenericFailure);
    }

    (*s).session_id = 0;
    let pss = PrivateSocket::boxed_zeroed();
    (*s).private = pss;
    (*pss).fd = (*(*server).private).accepted_sock;
    (*pss).waiting_to_send = 0;
    (*(*server).private).accepted_sock = -1;
    (*(*server).private).sock_state = WAITING_FOR_CONNECTION;
    aio::handle((*(*server).private).fd, accept_handler, aio::AIO_RX);
    (*s).session_id = resolver::current_session();
    (*pss).conn_sema = sema_index as c_int;
    (*pss).read_sema = read_sema_index as c_int;
    (*pss).write_sema = write_sema_index as c_int;
    (*pss).sock_state = CONNECTED;
    (*pss).sock_error = 0;
    // The record's socketType is still zero (fresh ByteArray); the C copies
    // that, so the accepted socket reads as TCP -- which it is.
    (*pss).socket_type = (*s).socket_type;
    aio::enable((*pss).fd, pss as *mut c_void, 0);
    Ok(())
}

/// `sqSocketCloseConnection`.
///
/// # Safety
/// As [`create`].
pub unsafe fn close_connection(s: *mut SQSocket) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let fd = (*pss).fd;
    if fd < 0 {
        return Ok(()); // already closed
    }
    if (*pss).accepted_sock > 0 {
        // a queued accept was never collected
        set_linger((*pss).accepted_sock, 0);
        libc::close((*pss).accepted_sock);
    }
    (*pss).sock_state = THIS_END_CLOSED;
    let result = libc::close(fd);
    let last_error = last_os_error();
    if result == -1 && last_error != libc::EWOULDBLOCK {
        // error
        (*pss).sock_state = UNCONNECTED;
        (*pss).sock_error = last_error;
        aio::disable(fd);
        notify(pss, CONN_NOTIFY);
    } else if result == 0 {
        // close completed synchronously
        (*pss).sock_state = UNCONNECTED;
        aio::disable(fd);
        (*pss).fd = -1;
    } else {
        // asynchronous close in progress
        let _ = with_socket(fd, |sock| sock.shutdown(std::net::Shutdown::Write));
        (*pss).sock_state = THIS_END_CLOSED;
        aio::handle(fd, close_handler, aio::AIO_RWX); // => close() done
    }
    Ok(())
}

/// `sqSocketAbortConnection`: close without lingering.
///
/// # Safety
/// As [`create`].
pub unsafe fn abort_connection(s: *mut SQSocket) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    set_linger((*(*s).private).fd, 0);
    close_connection(s)
}

/// `sqSocketDestroy`: abort if open, then release the private state.
///
/// # Safety
/// As [`create`].
pub unsafe fn destroy(s: *mut SQSocket) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    if (*(*s).private).fd != 0 {
        // close if necessary; inner failures cannot occur on a valid socket
        let _ = abort_connection(s);
    }
    if !(*s).private.is_null() {
        drop(Box::from_raw((*s).private)); // the C's free(PSP(s))
    }
    (*s).private = ptr::null_mut();
    Ok(())
}

/// `sqSocketError`.
///
/// # Safety
/// As [`create`].
pub unsafe fn socket_error(s: *mut SQSocket) -> PrimResult<c_int> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    Ok((*(*s).private).sock_error)
}

// ---------------------------------------------------------------------------
// Addresses and ports (IPv4 int form)
// ---------------------------------------------------------------------------

/// `sqSocketLocalAddress`: 0 when unbound or not AF_INET.
///
/// # Safety
/// As [`create`].
pub unsafe fn local_address(s: *mut SQSocket) -> PrimResult<u32> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let mut saddr: libc::sockaddr_in = mem::zeroed();
    let mut size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    if libc::getsockname(
        (*(*s).private).fd,
        &mut saddr as *mut libc::sockaddr_in as *mut libc::sockaddr,
        &mut size,
    ) != 0
        || i32::from(saddr.sin_family) != libc::AF_INET
    {
        return Ok(0);
    }
    Ok(u32::from_be(saddr.sin_addr.s_addr))
}

/// `sqSocketRemoteAddress`: the peer for TCP, the recorded peer for UDP/RAW.
///
/// # Safety
/// As [`create`].
pub unsafe fn remote_address(s: *mut SQSocket) -> PrimResult<u32> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    if (*s).socket_type == TCP_SOCKET_TYPE {
        let mut saddr: libc::sockaddr_in = mem::zeroed();
        let mut size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        if libc::getpeername(
            (*(*s).private).fd,
            &mut saddr as *mut libc::sockaddr_in as *mut libc::sockaddr,
            &mut size,
        ) != 0
            || i32::from(saddr.sin_family) != libc::AF_INET
        {
            return Ok(0);
        }
        return Ok(u32::from_be(saddr.sin_addr.s_addr));
    }
    Ok(u32::from_be((*(*s).private).peer.sin.sin_addr.s_addr))
}

/// `sqSocketLocalPort`.
///
/// # Safety
/// As [`create`].
pub unsafe fn local_port(s: *mut SQSocket) -> PrimResult<sqInt> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let mut saddr: libc::sockaddr_in = mem::zeroed();
    let mut size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    if libc::getsockname(
        (*(*s).private).fd,
        &mut saddr as *mut libc::sockaddr_in as *mut libc::sockaddr,
        &mut size,
    ) != 0
        || i32::from(saddr.sin_family) != libc::AF_INET
    {
        return Ok(0);
    }
    Ok(u16::from_be(saddr.sin_port) as sqInt)
}

/// `sqSocketRemotePort`.
///
/// # Safety
/// As [`create`].
pub unsafe fn remote_port(s: *mut SQSocket) -> PrimResult<sqInt> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    if (*s).socket_type == TCP_SOCKET_TYPE {
        let mut saddr: libc::sockaddr_in = mem::zeroed();
        let mut size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        if libc::getpeername(
            (*(*s).private).fd,
            &mut saddr as *mut libc::sockaddr_in as *mut libc::sockaddr,
            &mut size,
        ) != 0
            || i32::from(saddr.sin_family) != libc::AF_INET
        {
            return Ok(0);
        }
        return Ok(u16::from_be(saddr.sin_port) as sqInt);
    }
    Ok(u16::from_be((*(*s).private).peer.sin.sin_port) as sqInt)
}

// ---------------------------------------------------------------------------
// Data transfer
// ---------------------------------------------------------------------------

/// `sqSocketReceiveDataAvailable`. Whatever the answer, the data handler is
/// re-armed so the read semaphore fires when data arrives.
///
/// # Safety
/// As [`create`].
pub unsafe fn receive_data_available(s: *mut SQSocket) -> PrimResult<bool> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    if (*pss).sock_state == CONNECTED {
        let n = socket_readable((*pss).fd, (*s).socket_type);
        if n > 0 {
            return Ok(true);
        }
        if n < 0 {
            (*pss).sock_state = OTHER_END_CLOSED;
        }
    }
    aio::handle((*pss).fd, data_handler, aio::AIO_RX);
    Ok(false)
}

/// `sqSocketSendDone`.
///
/// # Safety
/// As [`create`].
pub unsafe fn send_done(s: *mut SQSocket) -> PrimResult<bool> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    if (*pss).sock_state == CONNECTED {
        return Ok((*pss).waiting_to_send == 0);
    }
    Ok(false)
}

/// `sqSocketReceiveDataBufCount`: read into `buf`; 0 also stands for "would
/// block" and for errors already recorded on the socket, as in C.
///
/// # Safety
/// As [`create`]; `buf` spans `buf_size` writable bytes that no allocation
/// moves during the call.
pub unsafe fn receive_data(s: *mut SQSocket, buf: *mut u8, buf_size: usize) -> PrimResult<isize> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    (*pss).peer_size = 0;
    if (*s).socket_type != TCP_SOCKET_TYPE {
        // UDP/RAW: record the sender as the new peer
        let mut addr_size = mem::size_of::<SockAddrAny>() as libc::socklen_t;
        let nread = libc::recvfrom(
            (*pss).fd,
            buf as *mut c_void,
            buf_size,
            0,
            &mut (*pss).peer as *mut SockAddrAny as *mut libc::sockaddr,
            &mut addr_size,
        );
        if nread <= 0 {
            let last_error = last_os_error();
            if nread == -1 && last_error == libc::EWOULDBLOCK {
                return Ok(0); // blocked
            }
            (*pss).sock_error = last_error;
            return Ok(0);
        }
        (*pss).peer_size = addr_size;
        Ok(nread as isize)
    } else {
        // TCP
        let nread = libc::recv((*pss).fd, buf as *mut c_void, buf_size, 0);
        if nread <= 0 {
            let last_error = last_os_error();
            if nread == -1 && last_error == libc::EWOULDBLOCK {
                return Ok(0); // blocked
            }
            // connection reset (or orderly EOF: recv answered 0)
            (*pss).sock_state = OTHER_END_CLOSED;
            (*pss).sock_error = last_error;
            notify(pss, CONN_NOTIFY);
            return Ok(0);
        }
        Ok(nread as isize)
    }
}

/// `sqSocketSendDataBufCount`: write from `buf`; 0 stands for "would block"
/// (send handler armed) and for recorded errors.
///
/// # Safety
/// As [`receive_data`], with `buf` readable.
pub unsafe fn send_data(s: *mut SQSocket, buf: *const u8, buf_size: usize) -> PrimResult<isize> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    if (*s).socket_type != TCP_SOCKET_TYPE {
        // UDP/RAW: send to the recorded peer
        let nsent = udp_send((*pss).fd, &(*pss).peer, buf, buf_size);
        if nsent <= 0 {
            let err = last_os_error();
            if err == libc::EWOULDBLOCK {
                return Ok(0); // asynchronous write in progress
            }
            (*pss).sock_error = err;
            return Ok(0);
        }
        (*pss).waiting_to_send = 0;
        Ok(nsent as isize)
    } else {
        // TCP
        let nsent = libc::send((*pss).fd, buf as *const c_void, buf_size, 0);
        if nsent <= 0 {
            let last_error = last_os_error();
            if nsent == -1 && last_error == libc::EWOULDBLOCK {
                (*pss).waiting_to_send = 1;
                aio::handle((*pss).fd, send_handler, aio::AIO_WX);
                return Ok(0);
            }
            // error: most likely "connection closed by peer"
            (*pss).sock_state = OTHER_END_CLOSED;
            (*pss).sock_error = last_error;
            (*pss).waiting_to_send = 0;
            return Ok(0);
        }
        (*pss).waiting_to_send = 0;
        Ok(nsent as isize)
    }
}

/// `sqSocketReceiveUDPDataBufCountaddressportmoreFlag`: answers (bytes read,
/// sender address, sender port, more flag). The C never sets the more flag.
///
/// # Safety
/// As [`receive_data`].
pub unsafe fn receive_udp(
    s: *mut SQSocket,
    buf: *mut u8,
    buf_size: usize,
) -> PrimResult<(isize, u32, sqInt, bool)> {
    if socket_valid(s) && (*s).socket_type != TCP_SOCKET_TYPE {
        let pss = (*s).private;
        let mut saddr: libc::sockaddr_in = mem::zeroed();
        let mut addr_size = mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        let nread = libc::recvfrom(
            (*pss).fd,
            buf as *mut c_void,
            buf_size,
            0,
            &mut saddr as *mut libc::sockaddr_in as *mut libc::sockaddr,
            &mut addr_size,
        );
        if nread >= 0 {
            return Ok((
                nread as isize,
                u32::from_be(saddr.sin_addr.s_addr),
                u16::from_be(saddr.sin_port) as sqInt,
                false,
            ));
        }
        let last_error = last_os_error();
        if last_error == libc::EWOULDBLOCK {
            // asynchronous read in progress
            return Ok((0, 0, 0, false));
        }
        (*pss).sock_error = last_error;
    }
    Err(PrimErr::GenericFailure)
}

/// `sqSockettoHostportSendDataBufCount`: UDP send to an explicit host/port.
///
/// # Safety
/// As [`send_data`].
pub unsafe fn send_udp_to(
    s: *mut SQSocket,
    address: u32,
    port: sqInt,
    buf: *const u8,
    buf_size: usize,
) -> PrimResult<isize> {
    if socket_valid(s) && (*s).socket_type != TCP_SOCKET_TYPE {
        let pss = (*s).private;
        let saddr = make_sockaddr_in(address, port as u16);
        let nsent = libc::sendto(
            (*pss).fd,
            buf as *const c_void,
            buf_size,
            0,
            &saddr as *const libc::sockaddr_in as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        if nsent >= 0 {
            (*pss).waiting_to_send = 0;
            return Ok(nsent as isize);
        }
        let last_error = last_os_error();
        if last_error == libc::EWOULDBLOCK {
            (*pss).waiting_to_send = 1;
            aio::handle((*pss).fd, send_handler, aio::AIO_WX);
            // asynchronous write in progress
            return Ok(0);
        }
        (*pss).sock_error = last_error;
    }
    Err(PrimErr::GenericFailure)
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// `sqSocketSetOptions...`: the option arrives as a string; a value of at
/// most four characters that parses entirely as an integer is passed as a C
/// int, anything else verbatim as raw bytes. See the C's long comment for how
/// deliberately unloved this protocol is.
///
/// Answers `(0, negotiated value)`, where the "negotiated" value is just the
/// parsed integer (or 0), as in C.
///
/// # Safety
/// As [`create`].
pub unsafe fn set_options(
    s: *mut SQSocket,
    option_name: &[u8],
    option_value: &[u8],
) -> PrimResult<(sqInt, sqInt)> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let Some(opt) = options::find_option(option_name) else {
        return Err(PrimErr::GenericFailure);
    };
    let mut buf = [0u8; 32];
    if option_value.len() > buf.len() - 1 {
        return Err(PrimErr::GenericFailure);
    }
    buf[..option_value.len()].copy_from_slice(option_value);

    let mut val: c_int = 0;
    let mut option_value_size = option_value.len();
    let (parsed, consumed) = options::parse_c_long(&buf[..option_value.len()]);
    if option_value.len() <= mem::size_of::<c_int>() && consumed == option_value.len() {
        // all option chars are digits: pass the value as a C int
        val = parsed as c_int;
        buf[..mem::size_of::<c_int>()].copy_from_slice(&val.to_ne_bytes());
        option_value_size = mem::size_of::<c_int>();
    }
    if libc::setsockopt(
        (*(*s).private).fd,
        opt.level,
        opt.optname,
        buf.as_ptr() as *const c_void,
        option_value_size as libc::socklen_t,
    ) < 0
    {
        return Err(PrimErr::GenericFailure);
    }
    Ok((0, val as sqInt))
}

/// `sqSocketGetOptions...`: answers `(0, value)` for a known integer option.
///
/// # Safety
/// As [`create`].
pub unsafe fn get_options(s: *mut SQSocket, option_name: &[u8]) -> PrimResult<(sqInt, sqInt)> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let Some(opt) = options::find_option(option_name) else {
        return Err(PrimErr::GenericFailure);
    };
    let mut optval: c_int = 0; // NOT sqInt
    let mut optlen = mem::size_of::<c_int>() as libc::socklen_t;
    if libc::getsockopt(
        (*(*s).private).fd,
        opt.level,
        opt.optname,
        &mut optval as *mut c_int as *mut c_void,
        &mut optlen,
    ) < 0
        || optlen != mem::size_of::<c_int>() as libc::socklen_t
    {
        return Err(PrimErr::GenericFailure);
    }
    Ok((0, optval as sqInt))
}

// ---------------------------------------------------------------------------
// Binding and connecting by IPv4 int / by raw socket address
// ---------------------------------------------------------------------------

/// `sqSocketBindToPort`.
///
/// # Safety
/// As [`create`].
pub unsafe fn bind_to_port(s: *mut SQSocket, addr: u32, port: sqInt) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let inaddr = make_sockaddr_in(addr, port as u16);
    if libc::bind(
        (*pss).fd,
        &inaddr as *const libc::sockaddr_in as *const libc::sockaddr,
        mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    ) < 0
    {
        (*pss).sock_error = last_os_error();
        return Err(PrimErr::GenericFailure);
    }
    Ok(())
}

/// `sqSocketBindToAddressSize`: bind to a header-stamped socket address.
///
/// # Safety
/// As [`create`].
pub unsafe fn bind_to_address(s: *mut SQSocket, addr: &[u8]) -> PrimResult<()> {
    if !(socket_valid(s) && crate::address::address_valid(addr, resolver::current_session())) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let payload = crate::address::payload(addr);
    if libc::bind(
        (*pss).fd,
        payload.as_ptr() as *const libc::sockaddr,
        payload.len() as libc::socklen_t,
    ) == 0
    {
        return Ok(());
    }
    (*pss).sock_error = last_os_error();
    Err(PrimErr::GenericFailure)
}

/// `sqSocketConnectToAddressSize`: TCP connects, UDP/RAW set the peer.
///
/// # Safety
/// As [`create`].
pub unsafe fn connect_to_address(s: *mut SQSocket, addr: &[u8]) -> PrimResult<()> {
    if !(socket_valid(s) && crate::address::address_valid(addr, resolver::current_session())) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let payload = crate::address::payload(addr);
    if (*s).socket_type != TCP_SOCKET_TYPE {
        // UDP/RAW
        if (*pss).fd >= 0 {
            let n = payload.len().min(mem::size_of::<SockAddrAny>());
            ptr::copy_nonoverlapping(
                payload.as_ptr(),
                &mut (*pss).peer as *mut SockAddrAny as *mut u8,
                n,
            );
            (*pss).peer_size = payload.len() as libc::socklen_t;
            let result = libc::connect(
                (*pss).fd,
                payload.as_ptr() as *const libc::sockaddr,
                payload.len() as libc::socklen_t,
            );
            if result == 0 {
                (*pss).sock_state = CONNECTED;
            }
        }
    } else {
        aio::enable((*pss).fd, pss as *mut c_void, 0);
        let result = libc::connect(
            (*pss).fd,
            payload.as_ptr() as *const libc::sockaddr,
            payload.len() as libc::socklen_t,
        );
        finish_tcp_connect(pss, result);
    }
    Ok(())
}

/// `sqSocketLocalAddressSize`: header + whatever getsockname answers, or 0
/// when it fails.
///
/// # Safety
/// As [`create`].
pub unsafe fn local_address_size(s: *mut SQSocket) -> PrimResult<isize> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let mut saddr: SockAddrAny = mem::zeroed();
    let mut size = mem::size_of::<SockAddrAny>() as libc::socklen_t;
    if libc::getsockname(
        (*(*s).private).fd,
        &mut saddr as *mut SockAddrAny as *mut libc::sockaddr,
        &mut size,
    ) != 0
    {
        return Ok(0);
    }
    Ok((crate::address::ADDRESS_HEADER_SIZE + size as usize) as isize)
}

/// `sqSocketLocalAddressResultSize`: writes header + raw local sockaddr; the
/// destination must be sized exactly as `local_address_size` answered.
///
/// # Safety
/// As [`create`].
pub unsafe fn local_address_result(s: *mut SQSocket, dest: &mut [u8]) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let mut saddr: SockAddrAny = mem::zeroed();
    let mut size = mem::size_of::<SockAddrAny>() as libc::socklen_t;
    if libc::getsockname(
        (*(*s).private).fd,
        &mut saddr as *mut SockAddrAny as *mut libc::sockaddr,
        &mut size,
    ) != 0
    {
        return Err(PrimErr::GenericFailure);
    }
    let size = size as usize;
    if dest.len() != crate::address::ADDRESS_HEADER_SIZE + size {
        return Err(PrimErr::GenericFailure);
    }
    crate::address::write_header(dest, resolver::current_session(), size as i32);
    let bytes = core::slice::from_raw_parts(&saddr as *const SockAddrAny as *const u8, size);
    dest[crate::address::ADDRESS_HEADER_SIZE..].copy_from_slice(bytes);
    Ok(())
}

/// `sqSocketRemoteAddressSize`: for TCP this also *caches the peer* into the
/// private state (the later `remote_address_result` consumes it), answering
/// -1 when there is no peer.
///
/// # Safety
/// As [`create`].
pub unsafe fn remote_address_size(s: *mut SQSocket) -> PrimResult<isize> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    if (*s).socket_type == TCP_SOCKET_TYPE {
        let mut saddr: SockAddrAny = mem::zeroed();
        let mut size = mem::size_of::<SockAddrAny>() as libc::socklen_t;
        if libc::getpeername(
            (*pss).fd,
            &mut saddr as *mut SockAddrAny as *mut libc::sockaddr,
            &mut size,
        ) == 0
            && (size as usize) < mem::size_of::<SockAddrAny>()
        {
            ptr::copy_nonoverlapping(
                &saddr as *const SockAddrAny as *const u8,
                &mut (*pss).peer as *mut SockAddrAny as *mut u8,
                size as usize,
            );
            (*pss).peer_size = size;
            return Ok((crate::address::ADDRESS_HEADER_SIZE + size as usize) as isize);
        }
    } else if (*pss).peer_size != 0 {
        return Ok((crate::address::ADDRESS_HEADER_SIZE + (*pss).peer_size as usize) as isize);
    }
    Ok(-1)
}

/// `sqSocketRemoteAddressResultSize`: writes the cached peer and clears it.
///
/// # Safety
/// As [`create`].
pub unsafe fn remote_address_result(s: *mut SQSocket, dest: &mut [u8]) -> PrimResult<()> {
    if !socket_valid(s) {
        return Err(PrimErr::GenericFailure);
    }
    let pss = (*s).private;
    let size = (*pss).peer_size as usize;
    if size == 0 || dest.len() != crate::address::ADDRESS_HEADER_SIZE + size {
        return Err(PrimErr::GenericFailure);
    }
    crate::address::write_header(dest, resolver::current_session(), size as i32);
    let bytes = core::slice::from_raw_parts(&(*pss).peer as *const SockAddrAny as *const u8, size);
    dest[crate::address::ADDRESS_HEADER_SIZE..].copy_from_slice(bytes);
    (*pss).peer_size = 0;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address;
    use crate::aio::testing::poll_once;
    use crate::testing::net_lock;

    const LOCALHOST: u32 = 0x7f00_0001;

    /// The outer record's size is ABI (socketRecordSize, UnixOSProcessPlugin).
    #[test]
    fn record_layout_is_the_c_layout() {
        assert_eq!(mem::offset_of!(SQSocket, session_id), 0);
        assert_eq!(mem::offset_of!(SQSocket, socket_type), 4);
        // Two ints, padding to pointer alignment, one pointer: 16 bytes on
        // 64-bit, 12 on 32-bit -- what the C's sizeof(SQSocket) is.
        assert_eq!(mem::offset_of!(SQSocket, private), 8);
        assert_eq!(mem::size_of::<SQSocket>(), 8 + mem::size_of::<*mut c_void>());
        // The one cross-plugin invariant on the private struct: fd first.
        assert_eq!(mem::offset_of!(PrivateSocket, fd), 0);
    }

    fn init_net() {
        resolver::network_init(0);
    }

    unsafe fn make_tcp() -> SQSocket {
        let mut s = SQSocket::zeroed();
        create(&mut s, 2, TCP_SOCKET_TYPE as sqInt, 8000, 8000, 0, 0, 0).unwrap();
        s
    }

    unsafe fn make_udp() -> SQSocket {
        let mut s = SQSocket::zeroed();
        create(&mut s, 2, UDP_SOCKET_TYPE as sqInt, 8000, 8000, 0, 0, 0).unwrap();
        s
    }

    /// Pump the test aio loop until `cond` holds or the deadline passes.
    fn pump(mut cond: impl FnMut() -> bool) -> bool {
        for _ in 0..200 {
            if cond() {
                return true;
            }
            poll_once(25);
        }
        cond()
    }

    #[test]
    fn invalid_records_are_rejected() {
        let _guard = net_lock();
        init_net();
        let mut fresh = SQSocket::zeroed();
        unsafe {
            assert!(!socket_valid(&mut fresh));
            assert!(connection_status(&mut fresh).is_err());
            assert!(socket_error(&mut fresh).is_err());
            assert!(destroy(&mut fresh).is_err());
            assert!(send_done(&mut fresh).is_err());
        }
    }

    #[test]
    fn create_stamps_the_session_and_destroy_clears_it() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut s = make_tcp();
            assert_eq!(s.session_id, resolver::current_session());
            assert_eq!(s.socket_type, TCP_SOCKET_TYPE);
            assert!(!s.private.is_null());
            assert_eq!(connection_status(&mut s).unwrap(), UNCONNECTED);
            assert_eq!(socket_error(&mut s).unwrap(), 0);
            destroy(&mut s).unwrap();
            assert!(s.private.is_null());
            assert!(destroy(&mut s).is_err(), "double destroy fails cleanly");
        }
    }

    #[test]
    fn unknown_socket_type_fails() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut s = SQSocket::zeroed();
            assert!(create(&mut s, 2, 9, 0, 0, 0, 0, 0).is_err());
            assert!(s.private.is_null());
        }
    }

    #[test]
    fn tcp_connect_send_receive_close() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut server = make_tcp();
            listen_on_port_backlog_interface(&mut server, 0, 8, LOCALHOST).unwrap();
            assert_eq!(connection_status(&mut server).unwrap(), WAITING_FOR_CONNECTION);
            let port = local_port(&mut server).unwrap();
            assert!(port > 0);
            assert_eq!(local_address(&mut server).unwrap(), LOCALHOST);

            let mut client = make_tcp();
            connect_to_port(&mut client, LOCALHOST, port).unwrap();
            // The connect handler and the accept handler both run off the
            // (test) poll loop.
            assert!(pump(|| connection_status(&mut client).unwrap() == CONNECTED));
            assert!(pump(|| (*server.private).accepted_sock > 0));

            let mut conn = SQSocket::zeroed();
            conn.session_id = 0;
            accept_from(&mut conn, &mut server, 0, 0, 0).unwrap();
            assert_eq!(connection_status(&mut conn).unwrap(), CONNECTED);
            assert_eq!(remote_port(&mut client).unwrap(), port);
            assert_eq!(remote_address(&mut client).unwrap(), LOCALHOST);

            assert!(send_done(&mut client).unwrap());
            let sent = send_data(&mut client, b"ping".as_ptr(), 4).unwrap();
            assert_eq!(sent, 4);

            let mut buf = [0u8; 16];
            let mut got = 0isize;
            assert!(pump(|| {
                if got == 0 {
                    got = receive_data(&mut conn, buf.as_mut_ptr(), buf.len()).unwrap();
                }
                got > 0
            }));
            assert_eq!(got, 4);
            assert_eq!(&buf[..4], b"ping");

            // receive_data_available arms the data handler and answers false
            // when nothing is pending.
            assert!(!receive_data_available(&mut conn).unwrap());
            let _ = send_data(&mut client, b"x".as_ptr(), 1).unwrap();
            assert!(pump(|| receive_data_available(&mut conn).unwrap()));

            close_connection(&mut client).unwrap();
            assert_eq!(connection_status(&mut client).unwrap(), UNCONNECTED);
            assert_eq!((*client.private).fd, -1);

            // The peer eventually observes the close: drain the byte, then EOF.
            assert!(pump(|| {
                let mut b = [0u8; 8];
                let _ = receive_data(&mut conn, b.as_mut_ptr(), b.len());
                (*conn.private).sock_state == OTHER_END_CLOSED
            }));

            destroy(&mut conn).unwrap();
            destroy(&mut client).unwrap();
            destroy(&mut server).unwrap();
        }
    }

    #[test]
    fn udp_roundtrip_with_explicit_destination() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut receiver = make_udp();
            assert_eq!(connection_status(&mut receiver).unwrap(), CONNECTED);
            bind_to_port(&mut receiver, LOCALHOST, 0).unwrap();
            let port = local_port(&mut receiver).unwrap();
            assert!(port > 0);

            let mut sender = make_udp();
            let n = send_udp_to(&mut sender, LOCALHOST, port, b"dgram".as_ptr(), 5).unwrap();
            assert_eq!(n, 5);

            let mut buf = [0u8; 32];
            let mut result = (0isize, 0u32, 0 as sqInt, false);
            assert!(pump(|| {
                if result.0 == 0 {
                    result = receive_udp(&mut receiver, buf.as_mut_ptr(), buf.len()).unwrap();
                }
                result.0 > 0
            }));
            assert_eq!(result.0, 5);
            assert_eq!(&buf[..5], b"dgram");
            assert_eq!(result.1, LOCALHOST);
            assert!(result.2 > 0);
            assert!(!result.3, "the C never reports a more flag");

            // The connected-style path: set the peer, then plain send/receive.
            let sender_port = local_port(&mut sender).unwrap();
            connect_to_port(&mut receiver, LOCALHOST, sender_port).unwrap();
            let n = send_data(&mut receiver, b"pong".as_ptr(), 4).unwrap();
            assert_eq!(n, 4);
            let mut got = 0isize;
            assert!(pump(|| {
                if got == 0 {
                    got = receive_data(&mut sender, buf.as_mut_ptr(), buf.len()).unwrap();
                }
                got > 0
            }));
            assert_eq!(got, 4);
            assert_eq!(&buf[..4], b"pong");
            // The sender's peer was recorded by the receive.
            assert_eq!(remote_address(&mut sender).unwrap(), LOCALHOST);

            destroy(&mut sender).unwrap();
            destroy(&mut receiver).unwrap();
        }
    }

    /// The unconnected half of [`udp_send`]: replying through `sendData` to a
    /// peer that only a `receive_data` ever recorded, on a socket `connect(2)`
    /// was never called on.
    ///
    /// This is what stops the Darwin branch from simply always calling `send`:
    /// on a socket with no peer that would be `EDESTADDRREQ`, so the
    /// `getpeername` probe has to send this datagram through `sendto` with a
    /// destination. It also exercises why `PrivateSocket::peer_size` is not
    /// the source of the address length -- every `receive_data` that would
    /// have blocked has zeroed it by the time the reply goes out, and the
    /// assertions below pin that sequence down.
    #[test]
    fn udp_replies_to_the_peer_a_receive_recorded() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut server = make_udp();
            bind_to_port(&mut server, LOCALHOST, 0).unwrap();
            let server_port = local_port(&mut server).unwrap();

            let mut client = make_udp();
            bind_to_port(&mut client, LOCALHOST, 0).unwrap();
            let client_port = local_port(&mut client).unwrap();
            assert_eq!(
                send_udp_to(&mut client, LOCALHOST, server_port, b"ping".as_ptr(), 4).unwrap(),
                4
            );

            // The server never connects; `receive_data` is what fills `peer`.
            let mut buf = [0u8; 32];
            let mut got = 0isize;
            assert!(pump(|| {
                if got == 0 {
                    got = receive_data(&mut server, buf.as_mut_ptr(), buf.len()).unwrap();
                }
                got > 0
            }));
            assert_eq!(&buf[..4], b"ping");
            assert_ne!(
                (*server.private).peer_size,
                0,
                "a successful receive records the size"
            );

            // A receive that finds nothing zeroes peer_size again while
            // leaving `peer` intact -- the state the reply below goes out in.
            assert_eq!(
                receive_data(&mut server, buf.as_mut_ptr(), buf.len()).unwrap(),
                0
            );
            assert_eq!(
                (*server.private).peer_size,
                0,
                "a blocked receive clears it"
            );

            assert_eq!(send_data(&mut server, b"pong".as_ptr(), 4).unwrap(), 4);
            let mut back = 0isize;
            assert!(pump(|| {
                if back == 0 {
                    back = receive_data(&mut client, buf.as_mut_ptr(), buf.len()).unwrap();
                }
                back > 0
            }));
            assert_eq!(&buf[..4], b"pong");
            // It really came back to the client's own port.
            assert_eq!(local_port(&mut client).unwrap(), client_port);

            destroy(&mut client).unwrap();
            destroy(&mut server).unwrap();
        }
    }

    /// The two Darwin kernel rules [`udp_send`] exists for, asserted against
    /// the kernel itself rather than taken on trust, and then shown not to
    /// bite the helper.
    ///
    /// Both calls in the first half are the C's line verbatim --
    /// `sendto(fd, .., &peer, sizeof(union sockaddr_any))`. On Linux both
    /// succeed, which is why the C never had to think about either; on Darwin
    /// the first is `EINVAL` (the length, on a socket that still has no local
    /// address) and the second is `EISCONN` (the destination, on a connected
    /// socket). If a future macOS relaxed either rule this test would fail,
    /// and the branch could go.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn darwin_rejects_the_c_udp_send_shape() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut receiver = make_udp();
            bind_to_port(&mut receiver, LOCALHOST, 0).unwrap();
            let port = local_port(&mut receiver).unwrap();

            let c_length = mem::size_of::<SockAddrAny>() as libc::socklen_t;
            let c_call = |fd: c_int, peer: *const SockAddrAny| -> (libc::ssize_t, c_int) {
                let n = libc::sendto(
                    fd,
                    b"x".as_ptr() as *const c_void,
                    1,
                    0,
                    peer as *const libc::sockaddr,
                    c_length,
                );
                (n, last_os_error())
            };

            // Rule 1: the oversized length, on a socket with no local address.
            let mut fresh = make_udp();
            (*fresh.private).peer.sin = make_sockaddr_in(LOCALHOST, port as u16);
            assert_eq!(
                c_call((*fresh.private).fd, &(*fresh.private).peer),
                (-1, libc::EINVAL),
                "Darwin refuses sizeof(union sockaddr_any) as an addrlen here"
            );
            // The helper picks the family's own length and gets through.
            assert_eq!(send_data(&mut fresh, b"x".as_ptr(), 1).unwrap(), 1);

            // Rule 2: any destination at all, on a connected socket.
            let mut connected = make_udp();
            connect_to_port(&mut connected, LOCALHOST, port).unwrap();
            assert_eq!(
                c_call((*connected.private).fd, &(*connected.private).peer),
                (-1, libc::EISCONN),
                "Darwin refuses a destination on a connected UDP socket"
            );
            // The helper drops the destination and uses send(2).
            assert_eq!(send_data(&mut connected, b"x".as_ptr(), 1).unwrap(), 1);

            destroy(&mut connected).unwrap();
            destroy(&mut fresh).unwrap();
            destroy(&mut receiver).unwrap();
        }
    }

    /// The `getpeername` probe in [`udp_send`] must not be visible in `errno`.
    ///
    /// A zero-length datagram is the case that catches it: `sendto` answers 0
    /// for one, `send_data` treats every `nsent <= 0` as its error path and
    /// reads `errno` there, and the probe on an unconnected socket has just
    /// failed with `ENOTCONN`. Unbracketed, that lands in `sqSocketError` after
    /// a send that worked. The C ran no probe, so the sentinel planted below is
    /// what a stale `errno` looks like and is what has to come back.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    #[test]
    fn udp_send_does_not_leak_the_getpeername_errno() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut receiver = make_udp();
            bind_to_port(&mut receiver, LOCALHOST, 0).unwrap();
            let port = local_port(&mut receiver).unwrap();

            // Unconnected, with a peer only because something recorded one --
            // the path where the probe fails.
            let mut sender = make_udp();
            bind_to_port(&mut sender, LOCALHOST, 0).unwrap();
            (*sender.private).peer.sin = make_sockaddr_in(LOCALHOST, port as u16);
            (*sender.private).sock_error = 0;

            *libc::__error() = 0;
            assert_eq!(
                send_data(&mut sender, b"".as_ptr(), 0).unwrap(),
                0,
                "a zero-length datagram: sendto answers 0, which is not failure"
            );
            assert_eq!(
                (*sender.private).sock_error,
                0,
                "the probe's ENOTCONN must not survive into sqSocketError"
            );
            assert_ne!((*sender.private).sock_error, libc::ENOTCONN);

            destroy(&mut sender).unwrap();
            destroy(&mut receiver).unwrap();
        }
    }

    #[test]
    fn udp_refuses_a_backlog() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut udp = make_udp();
            assert!(listen_on_port_backlog(&mut udp, 0, 4).is_err());
            // backlog 1 is fine: it just binds the port.
            listen_on_port_backlog(&mut udp, 0, 1).unwrap();
            destroy(&mut udp).unwrap();
        }
    }

    #[test]
    fn options_get_and_set() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut s = make_tcp();
            // Integer path: a short all-digit value goes through as an int.
            let (err, val) = set_options(&mut s, b"SO_KEEPALIVE", b"1").unwrap();
            assert_eq!((err, val), (0, 1));
            let (err, val) = get_options(&mut s, b"SO_KEEPALIVE").unwrap();
            assert_eq!(err, 0);
            assert_ne!(val, 0);

            let (_, val) = set_options(&mut s, b"TCP_NODELAY", b"0").unwrap();
            assert_eq!(val, 0);

            // Verbatim path: SO_LINGER takes a struct linger, so its value
            // cannot go through the integer path -- 8 raw bytes pass as-is.
            let linger = libc::linger {
                l_onoff: 1,
                l_linger: 2,
            };
            let mut raw = [0u8; 8];
            raw[..4].copy_from_slice(&linger.l_onoff.to_ne_bytes());
            raw[4..].copy_from_slice(&linger.l_linger.to_ne_bytes());
            let (err, val) = set_options(&mut s, b"SO_LINGER", &raw).unwrap();
            assert_eq!((err, val), (0, 0), "verbatim values report a 0 value");
            // The barf path: "1" parses as an int, and a 4-byte value is too
            // short for struct linger, so setsockopt refuses it.
            assert!(set_options(&mut s, b"SO_LINGER", b"1").is_err());
            // Unknown options barf.
            assert!(set_options(&mut s, b"SO_BOGUS", b"1").is_err());
            assert!(get_options(&mut s, b"SO_BOGUS").is_err());
            destroy(&mut s).unwrap();
        }
    }

    #[test]
    fn address_size_result_roundtrip() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut s = make_tcp();
            bind_to_port(&mut s, LOCALHOST, 0).unwrap();
            let size = local_address_size(&mut s).unwrap();
            assert!(size > address::ADDRESS_HEADER_SIZE as isize);
            let mut addr = vec![0u8; size as usize];
            local_address_result(&mut s, &mut addr).unwrap();
            assert!(address::address_valid(&addr, resolver::current_session()));
            let port = address::get_port(&addr, resolver::current_session()).unwrap();
            assert_eq!(port as sqInt, local_port(&mut s).unwrap());

            // A wrongly sized destination fails, as the C's size check does.
            let mut wrong = vec![0u8; size as usize + 1];
            assert!(local_address_result(&mut s, &mut wrong).is_err());

            // Bind a second socket through the address-object path.
            let mut t = make_tcp();
            let mut bind_addr = addr.clone();
            address::set_port(&mut bind_addr, resolver::current_session(), 0);
            bind_to_address(&mut t, &bind_addr).unwrap();
            assert_eq!(local_address(&mut t).unwrap(), LOCALHOST);

            // Stale-session addresses are rejected.
            let mut stale = addr.clone();
            address::write_header(&mut stale, 12345, (addr.len() - 8) as i32);
            assert!(bind_to_address(&mut t, &stale).is_err());

            destroy(&mut t).unwrap();
            destroy(&mut s).unwrap();
        }
    }

    #[test]
    fn connect_by_address_and_remote_address_result() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut server = make_tcp();
            listen_on_port_backlog_interface(&mut server, 0, 8, LOCALHOST).unwrap();
            let size = local_address_size(&mut server).unwrap();
            let mut addr = vec![0u8; size as usize];
            local_address_result(&mut server, &mut addr).unwrap();

            let mut client = make_tcp();
            connect_to_address(&mut client, &addr).unwrap();
            assert!(pump(|| connection_status(&mut client).unwrap() == CONNECTED));

            let rsize = remote_address_size(&mut client).unwrap();
            assert!(rsize > 0);
            let mut raddr = vec![0u8; rsize as usize];
            remote_address_result(&mut client, &mut raddr).unwrap();
            assert_eq!(
                address::get_port(&raddr, resolver::current_session()),
                address::get_port(&addr, resolver::current_session())
            );
            // The cached peer is consumed: a second result fails until the
            // next size call refreshes it.
            let mut again = vec![0u8; rsize as usize];
            assert!(remote_address_result(&mut client, &mut again).is_err());

            destroy(&mut client).unwrap();
            destroy(&mut server).unwrap();
        }
    }

    #[test]
    fn shutdown_invalidates_open_sockets() {
        let _guard = net_lock();
        init_net();
        unsafe {
            let mut s = make_tcp();
            resolver::network_shutdown();
            assert!(!socket_valid(&mut s));
            assert!(connection_status(&mut s).is_err());
            // Reviving the network does not revive the old session's sockets.
            resolver::network_init(0);
            assert!(!socket_valid(&mut s));
            // The private state is unreachable now -- the C leaks it the same
            // way on shutdown; reclaim manually to keep the test clean.
            drop(Box::from_raw(s.private));
        }
    }
}
