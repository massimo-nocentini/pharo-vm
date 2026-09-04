//! The network session and the DNS resolver: `sqNetworkInit`, the classic
//! synchronous name lookups, and the 2007 `getaddrinfo`/`getnameinfo` API.
//!
//! All of this state lives in file-static variables in `SocketPluginImpl.c`
//! (`thisNetSession`, `lastName`, `lastAddr`, `lastError`, `resolverSema`, the
//! `addrinfo` chain, the name-info buffers). Here the session and semaphore are
//! atomics and the rest sits behind one mutex -- everything is only ever
//! touched from the interpreter thread (primitives and aio handlers both run
//! there), the lock simply makes that assumption explicit and safe.
//!
//! The resolver is synchronous, as it is in the Unix C plugin: a "start
//! lookup" primitive blocks in `getaddrinfo` and signals the resolver
//! semaphore before returning ("we're done before we even started").
//!
//! Failure convention: `Err(PrimErr::GenericFailure)` stands for the C's
//! `success(false)`, which is how every function here reported failure.

use core::ffi::{c_char, c_int};
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicI32, AtomicIsize, Ordering};
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;

use pharo_vm_plugin::poison::{self, Guarded};
use pharo_vm_plugin::{sqInt, PrimErr, PrimResult};

use crate::address;
use crate::aio;
use crate::vm_ref;

/// Forced to at least 256 by the C header block: 64 is not enough for real
/// FQDNs on Linux.
pub const MAX_HOST_NAME_LEN: usize = 256;

// Resolver states, as the image knows them.
pub const RESOLVER_UNINITIALISED: i32 = 0;
pub const RESOLVER_SUCCESS: i32 = 1;
pub const RESOLVER_ERROR: i32 = 3;

// The generalised address API's portable enumerations (ikp 2007).
pub const SQ_SOCKET_NUMERIC: sqInt = 1 << 0;
pub const SQ_SOCKET_PASSIVE: sqInt = 1 << 1;

pub const SQ_SOCKET_FAMILY_UNSPECIFIED: sqInt = 0;
pub const SQ_SOCKET_FAMILY_LOCAL: sqInt = 1;
pub const SQ_SOCKET_FAMILY_INET4: sqInt = 2;
pub const SQ_SOCKET_FAMILY_INET6: sqInt = 3;
pub const SQ_SOCKET_FAMILY_MAX: sqInt = 4;

pub const SQ_SOCKET_TYPE_UNSPECIFIED: sqInt = 0;
pub const SQ_SOCKET_TYPE_STREAM: sqInt = 1;
pub const SQ_SOCKET_TYPE_DGRAM: sqInt = 2;
pub const SQ_SOCKET_TYPE_MAX: sqInt = 3;

pub const SQ_SOCKET_PROTOCOL_UNSPECIFIED: sqInt = 0;
pub const SQ_SOCKET_PROTOCOL_TCP: sqInt = 1;
pub const SQ_SOCKET_PROTOCOL_UDP: sqInt = 2;
pub const SQ_SOCKET_PROTOCOL_MAX: sqInt = 3;

/// `thisNetSession`: nonzero while the network is initialised. Stamped into
/// socket records and address headers so stale ones are rejected.
static THIS_NET_SESSION: AtomicI32 = AtomicI32::new(0);

/// The image-side semaphore index the resolver signals when a lookup finishes.
static RESOLVER_SEMA: AtomicIsize = AtomicIsize::new(0);

/// The mutable resolver state behind `lastName` and friends.
struct ResolverState {
    /// `lastName`: result of the last address→name lookup, or the name last
    /// looked up. At most [`MAX_HOST_NAME_LEN`] bytes.
    last_name: Vec<u8>,
    /// `lastAddr`: host-order IPv4 result of the last name→address lookup.
    last_addr: u32,
    /// `lastError`: 0 means the last lookup succeeded.
    last_error: c_int,
    /// `addrList` + `localInfo`: the current lookup's results.
    ///
    /// The C kept a libc-owned `addrinfo` linked list here (plus a second,
    /// hand-`calloc`ed one for the AF_UNIX case) and freed it at the top of
    /// the next lookup. These are owned Rust values instead, so the chain
    /// walking, the `freeaddrinfo`, the leaked `Box::into_raw` pair and the
    /// `unsafe impl Send` that all of that forced are gone.
    results: Vec<ResolvedAddr>,
    /// `addrInfo`: which of `results` the accessors answer about. Equal to
    /// `results.len()` when the C's cursor would be NULL.
    cursor: usize,
    /// `hostNameInfo` / `servNameInfo` / `nameInfoValid`.
    host_name_info: Vec<u8>,
    serv_name_info: Vec<u8>,
    name_info_valid: bool,
}

impl ResolverState {
    const fn new() -> Self {
        Self {
            last_name: Vec::new(),
            last_addr: 0,
            last_error: 0,
            results: Vec::new(),
            cursor: 0,
            host_name_info: Vec::new(),
            serv_name_info: Vec::new(),
            name_info_valid: false,
        }
    }

    /// Discards the previous lookup's results, as the top of
    /// `sqResolverGetAddressInfo...` does with its two frees.
    fn drop_results(&mut self) {
        self.results.clear();
        self.cursor = 0;
    }

    /// The entry the accessors answer about; `None` where the C's cursor was
    /// NULL.
    fn current(&self) -> Option<&ResolvedAddr> {
        self.results.get(self.cursor)
    }
}

/// One node of what the C kept as an `addrinfo` chain, owned outright.
///
/// `sockaddr` is the raw `ai_addr` bytes, `ai_addrlen` of them, because that
/// is what `gai_result` copies into the image's SocketAddress ByteArray
/// verbatim -- the image round-trips those bytes back to `bind`/`connect`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedAddr {
    family: c_int,
    socktype: c_int,
    protocol: c_int,
    sockaddr: Vec<u8>,
}

/// The raw `sockaddr` bytes behind a `std::net::SocketAddr`, as `getaddrinfo`
/// would have reported them in `ai_addr`/`ai_addrlen`.
fn sockaddr_bytes(addr: &std::net::SocketAddr) -> Vec<u8> {
    let sa = socket2::SockAddr::from(*addr);
    // SAFETY: `sa` owns a `sockaddr_storage`; `as_ptr()` and `len()` delimit
    // its initialised prefix, which for V4/V6 is exactly the `sockaddr_in` /
    // `sockaddr_in6` the C handed over.
    unsafe { core::slice::from_raw_parts(sa.as_ptr().cast::<u8>(), sa.len() as usize) }.to_vec()
}

/// The raw `sockaddr_un` bytes for a local-socket path -- the struct the C
/// hand-built and reported with `ai_addrlen = sizeof(struct sockaddr_un)`.
///
/// Two things about this struct move between the platforms, and the C moved
/// with them, so this does too. Darwin's `sockaddr_un` carries a leading
/// `sun_len` byte and shortens `sun_path` to 104 (glibc has no length byte and
/// 108 path bytes), which changes both the size of the answer and the bound
/// [`get_address_info`] checks the service name against -- both are read off
/// the real struct here rather than hard-coded. And `sun_len` is deliberately
/// left at the zero `mem::zeroed` gives it: the C had the assignment written
/// out and then commented away (`/*saun->sun_len= sizeof(struct
/// sockaddr_un);*/`), so on Darwin the C shipped a `sockaddr_un` whose length
/// byte says zero. Faithful oddity: it stays zero here. Nothing dereferences
/// it -- the bytes travel to the image as a SocketAddress and come back to
/// `connect`/`bind`, which take an explicit `socklen_t` -- and the C's own
/// Darwin builds have always behaved this way.
fn unix_sockaddr_bytes(path: &[u8]) -> Vec<u8> {
    // SAFETY: all-zero is a valid sockaddr_un.
    let mut saun: libc::sockaddr_un = unsafe { mem::zeroed() };
    saun.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dst, src) in saun.sun_path.iter_mut().zip(path) {
        *dst = *src as c_char;
    }
    // SAFETY: reading a fully initialised POD struct as its own bytes.
    unsafe {
        core::slice::from_raw_parts(
            (&saun as *const libc::sockaddr_un).cast::<u8>(),
            mem::size_of::<libc::sockaddr_un>(),
        )
    }
    .to_vec()
}

/// `S_IFSOCK` widened to the `u32` that `MetadataExt::mode()` answers.
///
/// The C reads this constant out of `<sys/stat.h>` on every Unix and its
/// *value* is `0140000` on both platforms; only its C type moves. `mode_t` is
/// `unsigned int` in glibc and `__uint16_t` in Darwin's `<sys/_types.h>`, so
/// the `libc` crate types `S_IFSOCK` as `u32` on Linux and `u16` on macOS,
/// and `st_mode & S_IFSOCK` -- which compiles on both in C, where the usual
/// arithmetic conversions widen both operands to `int` first -- is a type
/// error in Rust on exactly one of them.
///
/// `From` is that widening spelled out. Deliberately not an `as` cast: `as`
/// would compile whatever the constant's type became, truncating in silence
/// if it ever grew, where `u32::from` accepts only types that fit.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn s_ifsock() -> u32 {
    u32::from(libc::S_IFSOCK)
}

/// The glibc side of the split above: `mode_t` is already `unsigned int`, so
/// there is nothing to widen.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn s_ifsock() -> u32 {
    libc::S_IFSOCK
}

// Compile-time record of both halves of what `s_ifsock` assumes, per
// platform: the literal's suffix pins the type the `libc` crate gives the
// constant, and the comparison pins its value -- `0140000` in both
// `<sys/stat.h>`s, which is why the C's bitmask test means the same thing on
// each. If either moves, this stops compiling instead of quietly changing
// which files look like sockets.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const _: () = assert!(libc::S_IFSOCK == 0o140000u16);
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
const _: () = assert!(libc::S_IFSOCK == 0o140000u32);

// `clock` is not re-exported by the `libc` crate, but is in the C library
// every Rust program already links. (`gethostbyaddr` used to be declared here
// too -- it is deprecated and not thread-safe, and `dns_lookup::lookup_addr`
// does the same job over `getnameinfo`.)
extern "C" {
    fn clock() -> libc::clock_t;
}

static STATE: Mutex<ResolverState> = Mutex::new(ResolverState::new());

/// The resolver state, refused once a panic has torn it.
///
/// Through [`poison::lock`]. The previous note here -- "a poisoned lock is
/// unreachable ... the workspace aborts on panic anyway" -- stopped being true
/// when the plugin cdylibs moved to their own workspace with
/// `panic = "unwind"` (`rust/plugins/Cargo.toml`): a panic under this lock now
/// unwinds past the guard and poisons it. What it would leave behind is a
/// `results` vector re-filled by `get_address_info` with `cursor` still
/// pointing into the old one, and `gai_result` copies `entry.sockaddr` bytes
/// straight into an image ByteArray from there.
fn state() -> PrimResult<Guarded<'static, ResolverState>> {
    poison::lock(&STATE)
}

/// The current network session, 0 when uninitialised.
pub fn current_session() -> i32 {
    THIS_NET_SESSION.load(Ordering::Relaxed)
}

/// The resolver semaphore index registered by `primitiveInitializeNetwork`.
fn resolver_sema() -> c_int {
    RESOLVER_SEMA.load(Ordering::Relaxed) as c_int
}

/// Signals the resolver semaphore, the "lookup finished" notification.
fn signal_resolver() {
    vm_ref::signal_semaphore(resolver_sema());
}

/// `sqNetworkInit`: starts a session unless one is already running. Always
/// answers 0 -- re-initialisation is not an error.
pub fn network_init(resolver_sema_index: sqInt) -> sqInt {
    if current_session() != 0 {
        return 0; // already initialised
    }
    // The C seeds the session id with clock() + time(0); it only needs to be
    // nonzero and different from the previous run's.
    // SAFETY: plain libc calls with no arguments to get wrong.
    let mut session =
        unsafe { (clock() as i64).wrapping_add(libc::time(ptr::null_mut()) as i64) } as i32;
    if session == 0 {
        session = 1; // 0 => uninitialised
    }
    THIS_NET_SESSION.store(session, Ordering::Relaxed);
    RESOLVER_SEMA.store(resolver_sema_index, Ordering::Relaxed);
    0
}

/// `sqNetworkShutdown`: invalidates every open socket and stops the aio layer.
pub fn network_shutdown() {
    THIS_NET_SESSION.store(0, Ordering::Relaxed);
    RESOLVER_SEMA.store(0, Ordering::Relaxed);
    aio::fini();
}

/// `sqResolverAbort`: a no-op, since the Unix resolver is synchronous.
pub fn resolver_abort() {}

/// `sqResolverStatus`.
pub fn resolver_status() -> PrimResult<i32> {
    if current_session() == 0 {
        return Ok(RESOLVER_UNINITIALISED);
    }
    if state()?.last_error != 0 {
        return Ok(RESOLVER_ERROR);
    }
    Ok(RESOLVER_SUCCESS)
}

/// `sqResolverError`.
pub fn resolver_error() -> PrimResult<c_int> {
    Ok(state()?.last_error)
}

/// `<netdb.h>`'s `HOST_NOT_FOUND`: the `h_errno` value the C reported for a
/// reverse lookup that found nothing, and now the only one it can report.
/// See [`start_addr_lookup`].
const HOST_NOT_FOUND: c_int = 1;

/// `sqResolverStartAddrLookup`: reverse lookup, synchronously; the result
/// lands in `lastName` ("" on failure).
///
/// The C called `gethostbyaddr`, which is deprecated, not thread-safe (it
/// answers a pointer into static storage) and IPv4-only. `lookup_addr` does
/// the same job through `getnameinfo`, which is none of those things.
///
/// A failure still reports `HOST_NOT_FOUND` through `sqResolverError`:
/// `getnameinfo` reports EAI codes rather than the `h_errno` the C read, and
/// `last_h_errno` already flattened those to `HOST_NOT_FOUND` on any platform
/// without the accessor.
pub fn start_addr_lookup(net_address: u32) -> PrimResult<()> {
    let mut st = state()?;
    st.last_error = 0;
    let addr = std::net::IpAddr::V4(std::net::Ipv4Addr::from(net_address));
    match dns_lookup::lookup_addr(&addr) {
        Ok(name) => {
            let bytes = name.into_bytes();
            let len = bytes.len().min(MAX_HOST_NAME_LEN); // strncpy truncation
            st.last_name = bytes[..len].to_vec();
        }
        Err(_) => {
            st.last_error = HOST_NOT_FOUND;
            st.last_name.clear(); // strncpy of "" cleared the C buffer too
        }
    }
    Ok(())
}

/// `sqResolverAddrLookupResultSize`.
pub fn addr_lookup_result_size() -> PrimResult<usize> {
    Ok(state()?.last_name.len())
}

/// `sqResolverAddrLookupResult`: copies `lastName` into the answer String.
pub fn addr_lookup_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state()?;
    let n = st.last_name.len().min(dest.len());
    dest[..n].copy_from_slice(&st.last_name[..n]);
    Ok(())
}

/// `nameToAddr`: `getaddrinfo` with no hints, first AF_INET result, host
/// order. Sets `last_error` on failure and answers 0.
///
/// `dns_lookup::getaddrinfo` frees the chain itself and yields owned values,
/// so the pointer walk and the `freeaddrinfo` are gone. Its `LookupError`
/// carries the raw EAI number, which is the value the C stored in `lastError`
/// and the image reads back through `sqResolverError`.
fn name_to_addr(st: &mut ResolverState, host: &str) -> u32 {
    let infos = match dns_lookup::getaddrinfo(Some(host), None, None) {
        Ok(infos) => infos,
        Err(e) => {
            st.last_error = e.error_num();
            return 0;
        }
    };
    infos
        .flatten()
        .find_map(|info| match info.sockaddr {
            std::net::SocketAddr::V4(v4) => Some(u32::from(*v4.ip())), // ntohl
            std::net::SocketAddr::V6(_) => None,
        })
        .unwrap_or(0)
}

/// `sqResolverStartNameLookup`: synchronous forward lookup; signals the
/// resolver semaphore before returning.
pub fn start_name_lookup(host_name: &[u8]) -> PrimResult<()> {
    {
        let mut st = state()?;
        let len = host_name.len().min(MAX_HOST_NAME_LEN);
        // The C copies into a NUL-terminated buffer, so an interior NUL
        // truncates what getaddrinfo sees.
        let effective = match host_name[..len].iter().position(|&b| b == 0) {
            Some(nul) => &host_name[..nul],
            None => &host_name[..len],
        };
        st.last_name = effective.to_vec();
        st.last_error = 0;
        // The C passed a NUL-terminated buffer to getaddrinfo; a host name
        // that is not valid UTF-8 could never have resolved anyway.
        let host = String::from_utf8_lossy(&st.last_name).into_owned();
        st.last_addr = name_to_addr(&mut st, &host);
    }
    // "we're done before we even started"
    signal_resolver();
    Ok(())
}

/// `sqResolverNameLookupResult`: fails if the last lookup failed.
pub fn name_lookup_result() -> PrimResult<u32> {
    let st = state()?;
    if st.last_error != 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.last_addr)
}

/// `sqResolverLocalAddress` (the Unix branch): walk `getifaddrs` for the
/// first AF_INET address on `eth0` or `wlan0`.
///
/// The C's own TODO admits this does not cope with other interface names; the
/// walk is reproduced as is, and that has a blunt consequence on Darwin worth
/// stating rather than discovering: macOS names its interfaces `en0`, `lo0`
/// and so on, so neither name ever matches and this answers 0. The C is no
/// better -- the interface loop is guarded by `#ifndef _WIN32`, so the macOS
/// C plugin has always taken this same branch and always answered 0 too.
/// Faithful, and a fix belongs in the C first.
pub fn resolver_local_address() -> PrimResult<u32> {
    let mut ifaddrs: *mut libc::ifaddrs = ptr::null_mut();
    // SAFETY: standard getifaddrs protocol; freed before every return (the C
    // leaks the list on its error path -- freeing is invisible to the image).
    unsafe {
        if libc::getifaddrs(&mut ifaddrs) == -1 {
            return Err(PrimErr::GenericFailure);
        }
        let mut local_addr: u32 = 0;
        let mut ifa = ifaddrs;
        while !ifa.is_null() {
            let addr = (*ifa).ifa_addr;
            if !addr.is_null() {
                let mut host = [0 as c_char; 1025]; // NI_MAXHOST
                let s = libc::getnameinfo(
                    addr,
                    mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                    host.as_mut_ptr(),
                    host.len() as libc::socklen_t,
                    ptr::null_mut(),
                    0,
                    libc::NI_NUMERICHOST,
                );
                let name = core::ffi::CStr::from_ptr((*ifa).ifa_name).to_bytes();
                if (name == b"eth0" || name == b"wlan0")
                    && i32::from((*addr).sa_family) == libc::AF_INET
                {
                    if s != 0 {
                        libc::freeifaddrs(ifaddrs);
                        return Err(PrimErr::GenericFailure);
                    }
                    if local_addr == 0 {
                        // take the first plausible answer
                        let sin = addr as *const libc::sockaddr_in;
                        local_addr = (*sin).sin_addr.s_addr;
                    }
                }
            }
            ifa = (*ifa).ifa_next;
        }
        libc::freeifaddrs(ifaddrs);
        Ok(u32::from_be(local_addr)) // ntohl
    }
}

// ---------------------------------------------------------------------------
// getaddrinfo: address and service lookup
// ---------------------------------------------------------------------------

/// `EAI_BADHINTS`, from Darwin's `<netdb.h>`. The `libc` crate has no Apple
/// definition for it, and glibc has none at all -- which is the whole point
/// of the split below.
#[cfg(any(target_os = "macos", target_os = "ios"))]
const EAI_BADHINTS: c_int = 12;

/// Does a `getaddrinfo` failure fail the primitive, or does it "succeed with
/// zero results"?
///
/// The C answers with a preprocessor conditional whose comment is worth
/// quoting, because it explains a branch that reads like an accident
/// (abridged -- the log lines are dropped):
///
/// ```text
/// /* Linux gives you either <netdb.h> with   correct NI_* bit definitions and no  EAI_* definitions at all
///    or                <bind/netdb.h> with incorrect NI_* bit definitions and the EAI_* definitions we need.
///    We cannot distinguish between impossible constraints and genuine lookup failure, so err conservatively. */
/// #    if defined(EAI_BADHINTS)
///       if (EAI_BADHINTS != gaiError) { lastError= gaiError; goto fail; }
/// #    else
///       ...
/// #    endif
///       addrList= 0;      /* succeed with zero results for impossible constraints */
/// ```
///
/// So the intended behaviour is the `#if` arm: an unsatisfiable *hint*
/// combination is an empty answer, and everything else -- a name that does not
/// resolve, a service that does not exist -- is an error the image sees, with
/// `sqResolverError` carrying the EAI code and `sqResolverStatus` answering
/// `ResolverError`. Only on Linux, where glibc defines no `EAI_BADHINTS`, did
/// the C fall back to "err conservatively" and swallow every failure as an
/// empty result.
///
/// Darwin's `<netdb.h>` does define `EAI_BADHINTS`, so the C plugin on a Mac
/// has always taken the first arm. This port had implemented the glibc arm
/// unconditionally, which on Darwin turned every failed lookup into a silent
/// success with nothing in it. Each platform now gets the arm its own headers
/// selected, which is what "faithful" means for a `#if defined(...)`.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn gai_error_is_fatal(eai: c_int) -> bool {
    eai != EAI_BADHINTS
}

/// The glibc arm of the conditional above: no `EAI_BADHINTS` exists, so the C
/// could not tell impossible constraints from a genuine lookup failure and
/// treated both as zero results.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn gai_error_is_fatal(_eai: c_int) -> bool {
    false
}

/// `sqResolverGetAddressInfoHostSizeServiceSizeFlagsFamilyTypeProtocol`.
///
/// Synchronous; frees the previous results, runs the lookup, signals the
/// resolver semaphore. What a `getaddrinfo` failure means is decided by
/// [`gai_error_is_fatal`], and the two platforms decide it differently --
/// because the C did.
pub fn get_address_info(
    host: &[u8],
    serv: &[u8],
    flags: sqInt,
    family: sqInt,
    type_: sqInt,
    protocol: sqInt,
) -> PrimResult<()> {
    let mut st = state()?;
    st.drop_results();

    if current_session() == 0
        || host.len() > MAX_HOST_NAME_LEN
        || serv.len() > MAX_HOST_NAME_LEN
        || !(0..SQ_SOCKET_FAMILY_MAX).contains(&family)
        || !(0..SQ_SOCKET_TYPE_MAX).contains(&type_)
        || !(0..SQ_SOCKET_PROTOCOL_MAX).contains(&protocol)
    {
        return Err(PrimErr::GenericFailure);
    }

    // The C copies into NUL-terminated stack buffers; an interior NUL
    // truncates. sun_path is 108 bytes on Linux; the C compares servSize
    // against it before taking the local-socket branch.
    let truncate_at_nul = |bytes: &[u8]| -> Vec<u8> {
        match bytes.iter().position(|&b| b == 0) {
            Some(nul) => bytes[..nul].to_vec(),
            None => bytes.to_vec(),
        }
    };
    let host_c = truncate_at_nul(host);
    let serv_c = truncate_at_nul(serv);

    // Local (AF_UNIX) sockets: a service name that stats as a socket becomes
    // a hand-built result. The C's mode test is `st_mode & S_IFSOCK`, a
    // bitmask intersection that also matches regular files (S_IFREG shares a
    // bit) -- reproduced as is.
    // sizeof(((struct sockaddr_un *)0)->sun_path), the C's bound.
    let sun_path_len = {
        // SAFETY: all-zero is a valid sockaddr_un; only its field length is
        // consulted.
        let probe: libc::sockaddr_un = unsafe { mem::zeroed() };
        probe.sun_path.len()
    };
    if !serv.is_empty()
        && family == SQ_SOCKET_FAMILY_LOCAL
        && serv.len() < sun_path_len
        && flags & SQ_SOCKET_NUMERIC == 0
    {
        {
            use std::os::unix::fs::MetadataExt;
            let path = std::path::Path::new(std::ffi::OsStr::from_bytes(&serv_c));
            // `MetadataExt::mode()` is `st_mode` verbatim, so the C's bitmask
            // intersection above is applied to exactly the same value.
            let mode = std::fs::metadata(path).map(|md| md.mode()).unwrap_or(0);
            if mode & s_ifsock() != 0 {
                // The C hand-built an `addrinfo` here with `ai_protocol` left
                // at the zero `calloc` gave it; that zero is preserved.
                st.results = vec![ResolvedAddr {
                    family: libc::AF_UNIX,
                    socktype: libc::SOCK_STREAM,
                    protocol: 0,
                    sockaddr: unix_sockaddr_bytes(&serv_c),
                }];
                st.cursor = 0;
                drop(st);
                signal_resolver();
                return Ok(());
            }
        }
    }

    let mut ai_flags = 0;
    if flags & SQ_SOCKET_NUMERIC != 0 {
        ai_flags |= libc::AI_NUMERICHOST;
    }
    if flags & SQ_SOCKET_PASSIVE != 0 {
        ai_flags |= libc::AI_PASSIVE;
    }
    let hints = dns_lookup::AddrInfoHints {
        flags: ai_flags,
        address: match family {
            SQ_SOCKET_FAMILY_LOCAL => libc::AF_UNIX,
            SQ_SOCKET_FAMILY_INET4 => libc::AF_INET,
            SQ_SOCKET_FAMILY_INET6 => libc::AF_INET6,
            _ => 0,
        },
        socktype: match type_ {
            SQ_SOCKET_TYPE_STREAM => libc::SOCK_STREAM,
            SQ_SOCKET_TYPE_DGRAM => libc::SOCK_DGRAM,
            _ => 0,
        },
        protocol: match protocol {
            SQ_SOCKET_PROTOCOL_TCP => libc::IPPROTO_TCP,
            SQ_SOCKET_PROTOCOL_UDP => libc::IPPROTO_UDP,
            _ => 0,
        },
    };

    // NULL node/service where the image passed a zero size, as the C did.
    let host_s = String::from_utf8_lossy(&host_c).into_owned();
    let serv_s = String::from_utf8_lossy(&serv_c).into_owned();
    let node = (!host.is_empty()).then_some(host_s.as_str());
    let service = (!serv.is_empty()).then_some(serv_s.as_str());

    // Entries whose family this plugin cannot describe are dropped:
    // `AddrInfo::sockaddr` is V4 or V6, and the C's chain never carried
    // anything else out of getaddrinfo either.
    st.results = match dns_lookup::getaddrinfo(node, service, Some(hints)) {
        Ok(infos) => infos
            .flatten()
            .map(|info| ResolvedAddr {
                family: info.address,
                socktype: info.socktype,
                protocol: info.protocol,
                sockaddr: sockaddr_bytes(&info.sockaddr),
            })
            .collect(),
        Err(e) => {
            // `LookupError::error_num()` is not always an EAI code. dns-lookup
            // short-circuits before it ever calls `getaddrinfo(3)` -- both
            // node and service `None` (its `addrinfo.rs:218`), or a string
            // with an interior NUL -- and `From<io::Error> for LookupError`
            // stamps those with `err_num: 0` (its `err.rs:153`). Zero is not
            // an EAI code at all, and the fatal test below would take it for
            // one: on Darwin `gai_error_is_fatal(0)` is true, so the primitive
            // would fail while storing 0 in `lastError`, after which
            // `sqResolverStatus` answers `ResolverSuccess` and
            // `sqResolverError` answers 0 for the lookup that just failed --
            // and whatever error was recorded before is gone. The image can
            // reach that in one step: `primitiveResolverGetAddressInfo` takes
            // two ByteArrays and never checks either for emptiness.
            //
            // The C had no pre-flight to fail: it passed the two NULLs
            // straight to `getaddrinfo`, which on Darwin answers `EAI_NONAME`
            // (8 -- probed on aarch64-apple-darwin, and the same for a zeroed
            // hints struct). Normalising to that before the test is what puts
            // this back on the C's path. Linux is unaffected either way:
            // `gai_error_is_fatal` is `false` there, so any failure is still
            // swallowed as zero results.
            let eai = match e.error_num() {
                0 => libc::EAI_NONAME,
                n => n,
            };
            if gai_error_is_fatal(eai) {
                // The C's `goto fail`: record the EAI code, fail the
                // primitive, and -- unlike every other exit from here --
                // leave the resolver semaphore unsignalled.
                st.last_error = eai;
                return Err(PrimErr::GenericFailure);
            }
            // "succeed with zero results for impossible constraints"
            Vec::new()
        }
    };
    st.cursor = 0;
    drop(st);
    signal_resolver();
    Ok(())
}

/// `sqResolverGetAddressInfoSize`: -1 when the cursor is exhausted (not a
/// failure -- the image tests for -1).
pub fn gai_size() -> PrimResult<isize> {
    let st = state()?;
    Ok(match st.current() {
        None => -1,
        Some(entry) => (address::ADDRESS_HEADER_SIZE + entry.sockaddr.len()) as isize,
    })
}

/// `sqResolverGetAddressInfoResultSize`: writes header + raw sockaddr.
pub fn gai_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state()?;
    let entry = st.current().ok_or(PrimErr::GenericFailure)?;
    let len = entry.sockaddr.len();
    if dest.len() < address::ADDRESS_HEADER_SIZE + len {
        return Err(PrimErr::GenericFailure);
    }
    address::write_header(dest, current_session(), len as i32);
    dest[address::ADDRESS_HEADER_SIZE..address::ADDRESS_HEADER_SIZE + len]
        .copy_from_slice(&entry.sockaddr);
    Ok(())
}

/// `sqResolverGetAddressInfoFamily`.
pub fn gai_family() -> PrimResult<sqInt> {
    let st = state()?;
    let entry = st.current().ok_or(PrimErr::GenericFailure)?;
    Ok(match entry.family {
        libc::AF_UNIX => SQ_SOCKET_FAMILY_LOCAL,
        libc::AF_INET => SQ_SOCKET_FAMILY_INET4,
        libc::AF_INET6 => SQ_SOCKET_FAMILY_INET6,
        _ => SQ_SOCKET_FAMILY_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoType`.
pub fn gai_type() -> PrimResult<sqInt> {
    let st = state()?;
    let entry = st.current().ok_or(PrimErr::GenericFailure)?;
    Ok(match entry.socktype {
        libc::SOCK_STREAM => SQ_SOCKET_TYPE_STREAM,
        libc::SOCK_DGRAM => SQ_SOCKET_TYPE_DGRAM,
        _ => SQ_SOCKET_TYPE_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoProtocol`.
pub fn gai_protocol() -> PrimResult<sqInt> {
    let st = state()?;
    let entry = st.current().ok_or(PrimErr::GenericFailure)?;
    Ok(match entry.protocol {
        libc::IPPROTO_TCP => SQ_SOCKET_PROTOCOL_TCP,
        libc::IPPROTO_UDP => SQ_SOCKET_PROTOCOL_UDP,
        _ => SQ_SOCKET_PROTOCOL_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoNext`: advances the cursor, answers whether an
/// entry remains.
pub fn gai_next() -> PrimResult<bool> {
    let mut st = state()?;
    if st.cursor >= st.results.len() {
        // Already past the end: the C's NULL cursor could not advance.
        return Ok(false);
    }
    st.cursor += 1;
    Ok(st.cursor < st.results.len())
}

// ---------------------------------------------------------------------------
// getnameinfo: reverse host/service lookup
// ---------------------------------------------------------------------------

/// `sqResolverGetNameInfoSizeFlags`.
pub fn get_name_info(addr: &[u8], flags: sqInt) -> PrimResult<()> {
    {
        let mut st = state()?;
        st.name_info_valid = false;

        if !address::address_valid(addr, current_session()) {
            return Err(PrimErr::GenericFailure);
        }

        let mut ni_flags = libc::NI_NOFQDN;
        if flags & SQ_SOCKET_NUMERIC != 0 {
            ni_flags |= libc::NI_NUMERICHOST | libc::NI_NUMERICSERV;
        }

        let payload = address::payload(addr);
        let mut host = [0 as c_char; MAX_HOST_NAME_LEN + 1];
        let mut serv = [0 as c_char; MAX_HOST_NAME_LEN + 1];
        // SAFETY: the payload is the raw sockaddr the image round-tripped from
        // a previous *AddressResult primitive; getnameinfo only reads it.
        // ByteArray data is 8-byte aligned, so the sockaddr at offset 8 is too.
        let gai_error = unsafe {
            libc::getnameinfo(
                payload.as_ptr() as *const libc::sockaddr,
                payload.len() as libc::socklen_t,
                host.as_mut_ptr(),
                host.len() as libc::socklen_t,
                serv.as_mut_ptr(),
                serv.len() as libc::socklen_t,
                ni_flags,
            )
        };
        if gai_error != 0 {
            st.last_error = gai_error;
            return Err(PrimErr::GenericFailure);
        }

        let take = |buf: &[c_char]| -> Vec<u8> {
            buf.iter()
                .take_while(|&&c| c != 0)
                .map(|&c| c as u8)
                .collect()
        };
        st.host_name_info = take(&host);
        st.serv_name_info = take(&serv);
        st.name_info_valid = true;
    }
    signal_resolver();
    Ok(())
}

/// `sqResolverGetNameInfoHostSize`.
pub fn ni_host_size() -> PrimResult<usize> {
    let st = state()?;
    if !st.name_info_valid {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.host_name_info.len())
}

/// `sqResolverGetNameInfoHostResultSize`.
pub fn ni_host_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state()?;
    if !st.name_info_valid || dest.len() < st.host_name_info.len() {
        return Err(PrimErr::GenericFailure);
    }
    dest[..st.host_name_info.len()].copy_from_slice(&st.host_name_info);
    Ok(())
}

/// `sqResolverGetNameInfoServiceSize`.
pub fn ni_service_size() -> PrimResult<usize> {
    let st = state()?;
    if !st.name_info_valid {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.serv_name_info.len())
}

/// `sqResolverGetNameInfoServiceResultSize`.
pub fn ni_service_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state()?;
    if !st.name_info_valid || dest.len() < st.serv_name_info.len() {
        return Err(PrimErr::GenericFailure);
    }
    dest[..st.serv_name_info.len()].copy_from_slice(&st.serv_name_info);
    Ok(())
}

// ---------------------------------------------------------------------------
// The local host's own name
// ---------------------------------------------------------------------------

fn host_name() -> PrimResult<Vec<u8>> {
    let mut buf = [0 as c_char; MAX_HOST_NAME_LEN + 1];
    // SAFETY: gethostname NUL-terminates within the given size on success.
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } != 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(buf
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect())
}

/// `sqResolverHostNameSize`.
pub fn host_name_size() -> PrimResult<usize> {
    Ok(host_name()?.len())
}

/// `sqResolverHostNameResultSize`.
pub fn host_name_result(dest: &mut [u8]) -> PrimResult<()> {
    let name = host_name()?;
    if dest.len() < name.len() {
        return Err(PrimErr::GenericFailure);
    }
    dest[..name.len()].copy_from_slice(&name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::net_lock;

    #[test]
    fn session_starts_and_stops() {
        let _guard = net_lock();
        network_shutdown();
        assert_eq!(resolver_status().unwrap(), RESOLVER_UNINITIALISED);
        assert_eq!(network_init(5), 0);
        assert_ne!(current_session(), 0);
        assert_eq!(network_init(6), 0, "re-init is not an error");
        assert_eq!(resolver_status().unwrap(), RESOLVER_SUCCESS);
        network_shutdown();
        assert_eq!(current_session(), 0);
        network_init(5);
    }

    #[test]
    fn numeric_name_lookup() {
        let _guard = net_lock();
        network_init(0);
        start_name_lookup(b"127.0.0.1").unwrap();
        assert_eq!(resolver_error().unwrap(), 0);
        assert_eq!(name_lookup_result().unwrap(), 0x7f00_0001);
        // The looked-up name is what addr-lookup-result answers afterwards.
        assert_eq!(addr_lookup_result_size().unwrap(), 9);
        let mut buf = vec![0u8; 9];
        addr_lookup_result(&mut buf).unwrap();
        assert_eq!(&buf, b"127.0.0.1");
    }

    #[test]
    fn failed_name_lookup_reports_error() {
        let _guard = net_lock();
        network_init(0);
        // RFC 6761 reserves .invalid: this cannot resolve.
        start_name_lookup(b"does-not-exist.invalid").unwrap();
        assert_ne!(resolver_error().unwrap(), 0);
        assert!(name_lookup_result().is_err());
        assert_eq!(resolver_status().unwrap(), RESOLVER_ERROR);
        // Clean up for the next test.
        start_name_lookup(b"127.0.0.1").unwrap();
    }

    #[test]
    fn get_address_info_numeric_roundtrip() {
        let _guard = net_lock();
        network_init(0);
        get_address_info(
            b"127.0.0.1",
            b"80",
            SQ_SOCKET_NUMERIC,
            SQ_SOCKET_FAMILY_INET4,
            SQ_SOCKET_TYPE_STREAM,
            SQ_SOCKET_PROTOCOL_TCP,
        )
        .unwrap();

        let size = gai_size().unwrap();
        assert!(size >= (address::ADDRESS_HEADER_SIZE + 8) as isize);
        assert_eq!(gai_family().unwrap(), SQ_SOCKET_FAMILY_INET4);
        assert_eq!(gai_type().unwrap(), SQ_SOCKET_TYPE_STREAM);
        assert_eq!(gai_protocol().unwrap(), SQ_SOCKET_PROTOCOL_TCP);

        let mut addr = vec![0u8; size as usize];
        gai_result(&mut addr).unwrap();
        assert!(address::address_valid(&addr, current_session()));
        assert_eq!(address::get_port(&addr, current_session()), Some(80));

        // The reverse direction: numeric name info on the produced address.
        get_name_info(&addr, SQ_SOCKET_NUMERIC).unwrap();
        let mut host = vec![0u8; ni_host_size().unwrap()];
        ni_host_result(&mut host).unwrap();
        assert_eq!(&host, b"127.0.0.1");
        let mut serv = vec![0u8; ni_service_size().unwrap()];
        ni_service_result(&mut serv).unwrap();
        assert_eq!(&serv, b"80");

        // Exhaust the cursor.
        while gai_next().unwrap() {}
        assert_eq!(gai_size().unwrap(), -1);
        assert!(gai_family().is_err());
    }

    /// The AF_UNIX shortcut: a service name that stats as a socket never
    /// reaches `getaddrinfo` at all, it becomes a hand-built result.
    ///
    /// Everything this touches is a place the two platforms disagree, which is
    /// why it is worth having as a live test rather than only a compile-time
    /// one: [`s_ifsock`] has to widen `S_IFSOCK` from Darwin's 16-bit `mode_t`,
    /// the `sun_path` bound the service name is measured against is 104 bytes
    /// there and 108 on Linux, and the `sockaddr_un` handed back is 106 bytes
    /// with a leading `sun_len` on Darwin against 110 flat bytes on Linux. The
    /// assertions are written against the real structs so they say the right
    /// thing on each.
    #[test]
    fn local_socket_service_name_is_answered_without_getaddrinfo() {
        let _guard = net_lock();
        network_init(0);

        let path = std::env::temp_dir().join(format!("pharo-sock-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind AF_UNIX");
        let serv = path.as_os_str().as_bytes();
        assert!(
            serv.len() < 104,
            "the test path must fit Darwin's sun_path too"
        );

        get_address_info(
            b"",
            serv,
            0,
            SQ_SOCKET_FAMILY_LOCAL,
            SQ_SOCKET_TYPE_STREAM,
            SQ_SOCKET_PROTOCOL_TCP,
        )
        .unwrap();

        assert_eq!(gai_family().unwrap(), SQ_SOCKET_FAMILY_LOCAL);
        assert_eq!(gai_type().unwrap(), SQ_SOCKET_TYPE_STREAM);
        // The C calloc'd the addrinfo and never set ai_protocol, so the
        // requested TCP is not what comes back: zero is.
        assert_eq!(gai_protocol().unwrap(), SQ_SOCKET_PROTOCOL_UNSPECIFIED);

        let size = gai_size().unwrap();
        assert_eq!(
            size,
            (address::ADDRESS_HEADER_SIZE + mem::size_of::<libc::sockaddr_un>()) as isize,
            "ai_addrlen is sizeof(struct sockaddr_un), whatever that is here"
        );
        let mut addr = vec![0u8; size as usize];
        gai_result(&mut addr).unwrap();
        let payload = address::payload(&addr);
        assert_eq!(
            i32::from(payload[mem::offset_of!(libc::sockaddr_un, sun_family)]),
            libc::AF_UNIX,
            "sun_family, wherever the platform puts it"
        );
        let path_at = mem::offset_of!(libc::sockaddr_un, sun_path);
        assert_eq!(&payload[path_at..path_at + serv.len()], serv);
        assert_eq!(payload[path_at + serv.len()], 0, "and NUL-terminated");

        // Faithful oddity: the C has the sun_len assignment written out and
        // commented away, so on Darwin the length byte stays zero.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        assert_eq!(payload[mem::offset_of!(libc::sockaddr_un, sun_len)], 0);

        drop(listener);
        let _ = std::fs::remove_file(&path);
        start_name_lookup(b"127.0.0.1").unwrap();
    }

    #[test]
    fn get_address_info_rejects_bad_enums() {
        let _guard = net_lock();
        network_init(0);
        assert!(get_address_info(b"x", b"", 0, SQ_SOCKET_FAMILY_MAX, 0, 0).is_err());
        assert!(get_address_info(b"x", b"", 0, 0, SQ_SOCKET_TYPE_MAX, 0).is_err());
        assert!(get_address_info(b"x", b"", 0, 0, 0, SQ_SOCKET_PROTOCOL_MAX).is_err());
        assert!(get_address_info(&[0u8; 257], b"", 0, 0, 0, 0).is_err());
    }

    /// The `#if defined(EAI_BADHINTS)` split, from the outside.
    ///
    /// `AI_NUMERICHOST` (the image's `SQ_SOCKET_NUMERIC`) against a name that
    /// is not a numeric address fails inside `getaddrinfo` without a packet
    /// leaving the machine, so this asks the question offline: `EAI_NONAME`,
    /// not `EAI_BADHINTS`. Darwin's `<netdb.h>` defines `EAI_BADHINTS`, so the
    /// C plugin there takes the arm that reports the error; glibc does not,
    /// so on Linux the C swallows it as a successful lookup with no results.
    /// Each half is asserted on the platform it belongs to -- see
    /// [`gai_error_is_fatal`].
    ///
    /// The second half of the test asks the same thing with both arguments
    /// empty, which is the one failure dns-lookup produces *without* calling
    /// `getaddrinfo` and so without an EAI code to report; the answer has to
    /// come out the same.
    #[test]
    fn get_address_info_failure_follows_the_platforms_netdb() {
        let _guard = net_lock();
        network_init(0);
        let lookup = || {
            get_address_info(
                b"not.a.numeric.address",
                b"",
                SQ_SOCKET_NUMERIC,
                SQ_SOCKET_FAMILY_INET4,
                SQ_SOCKET_TYPE_STREAM,
                SQ_SOCKET_PROTOCOL_TCP,
            )
        };

        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            assert!(lookup().is_err(), "the C's `goto fail`");
            assert_ne!(
                resolver_error().unwrap(),
                0,
                "lastError carries the EAI code"
            );
            assert_ne!(
                resolver_error().unwrap(),
                EAI_BADHINTS,
                "only EAI_BADHINTS is the succeed-with-nothing case"
            );
            assert_eq!(resolver_status().unwrap(), RESOLVER_ERROR);
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            assert!(lookup().is_ok(), "succeed with zero results");
            assert_eq!(resolver_error().unwrap(), 0);
            assert_eq!(gai_size().unwrap(), -1, "and there really are none");
        }

        // Both arguments empty is the same question asked through a different
        // door. The C passed NULL/NULL to `getaddrinfo`, which answers
        // `EAI_NONAME`; dns-lookup refuses to make the call at all and hands
        // back an error whose `error_num()` is 0, which is not an EAI code.
        // `get_address_info` normalises that to `EAI_NONAME` before deciding,
        // so each platform lands on the same arm as above -- and in
        // particular `sqResolverError` cannot answer 0 for a primitive that
        // failed.
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            assert!(
                get_address_info(
                    b"",
                    b"",
                    0,
                    SQ_SOCKET_FAMILY_INET4,
                    SQ_SOCKET_TYPE_STREAM,
                    SQ_SOCKET_PROTOCOL_TCP,
                )
                .is_err(),
                "the C's `goto fail`, reached with no node and no service"
            );
            assert_eq!(
                resolver_error().unwrap(),
                libc::EAI_NONAME,
                "what getaddrinfo(NULL, NULL, ..) answers on Darwin"
            );
            assert_eq!(resolver_status().unwrap(), RESOLVER_ERROR);
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            assert!(
                get_address_info(
                    b"",
                    b"",
                    0,
                    SQ_SOCKET_FAMILY_INET4,
                    SQ_SOCKET_TYPE_STREAM,
                    SQ_SOCKET_PROTOCOL_TCP,
                )
                .is_ok(),
                "succeed with zero results"
            );
            assert_eq!(resolver_error().unwrap(), 0);
            assert_eq!(gai_size().unwrap(), -1, "and there really are none");
        }

        // Leave the shared resolver state clean for the other tests.
        start_name_lookup(b"127.0.0.1").unwrap();
    }

    #[test]
    fn name_info_needs_a_valid_address() {
        let _guard = net_lock();
        network_init(0);
        let bogus = vec![0u8; 24]; // zero session in the header
        assert!(get_name_info(&bogus, 0).is_err());
        assert!(ni_host_size().is_err());
        assert!(ni_service_size().is_err());
    }

    #[test]
    fn host_name_is_reported() {
        let _guard = net_lock();
        let size = host_name_size().unwrap();
        let mut buf = vec![0u8; size + 8];
        host_name_result(&mut buf).unwrap();
        assert!(buf[..size].iter().all(|&b| b != 0));
        let mut short = vec![0u8; size.saturating_sub(1)];
        if size > 0 {
            assert!(host_name_result(&mut short).is_err(), "too small fails");
        }
    }

    #[test]
    fn reverse_lookup_records_a_result_or_an_error() {
        let _guard = net_lock();
        network_init(0);
        start_addr_lookup(0x7f00_0001).unwrap();
        // Whether the sandbox can reverse-resolve 127.0.0.1 is environment-
        // dependent; the contract is: either a name arrived and no error, or
        // no name and an error.
        let size = addr_lookup_result_size().unwrap();
        if size == 0 {
            assert_ne!(resolver_error().unwrap(), 0);
        } else {
            assert_eq!(resolver_error().unwrap(), 0);
        }
        // Restore a clean resolver state.
        start_name_lookup(b"127.0.0.1").unwrap();
    }

    // -----------------------------------------------------------------------
    // Fail-fast after a panic mid-lookup
    // -----------------------------------------------------------------------

    /// A panic while [`state`] is held refuses every later lock.
    ///
    /// The SDK proves the mechanism in
    /// `pharo-vm-plugin/tests/plugin_mutex_poison.rs`; this proves the
    /// *wiring* here, which is the half a `STATE.clear_poison()` slipped in
    /// front of the `poison::lock` would silently undo while all 38 tests in
    /// the crate stayed green.
    ///
    /// The tear is the one the accessor's own doc names. `get_address_info`
    /// replaces `results` with the new lookup's answers and only then resets
    /// `cursor`; a panic between those two leaves the cursor indexing the new
    /// list at a position that meant something in the old one. Nothing about
    /// that is detectable downstream -- `current()` finds an entry, `gai_size`
    /// reports its length, and `gai_result` copies its `sockaddr` bytes
    /// verbatim into the image's SocketAddress ByteArray, which the image
    /// round-trips straight back into `bind`/`connect`. So a recovered lock
    /// does not merely answer stale data: it points the image's next
    /// connection at a host it never asked to resolve. Refusing is the only
    /// answer, and refusing is what all eight of this module's `getaddrinfo`
    /// accessors now propagate -- which is the other half of what this test
    /// pins, since making them fallible was the change that made refusing
    /// expressible at all.
    ///
    /// It holds [`net_lock`] for its whole body, including the window in which
    /// `STATE` is poisoned: every test in this crate that can reach `STATE`
    /// takes that lock, so none of them observes the window. The teardown at
    /// the end is what hands the binary back.
    ///
    /// The panic is raised by this test rather than injected through the proxy
    /// on purpose: every plugin-to-VM call crosses `extern "C"`, whose
    /// abort-on-unwind shim would turn an injected panic into `SIGABRT`
    /// instead of the unwind the hazard is made of.
    #[test]
    fn a_panic_while_the_resolver_state_is_held_refuses_every_later_lock() {
        use std::sync::PoisonError;

        let _guard = net_lock();
        network_init(0);

        // --- healthy: a lookup, and a caller part-way through its answers --
        get_address_info(
            b"127.0.0.1",
            b"80",
            SQ_SOCKET_NUMERIC,
            SQ_SOCKET_FAMILY_INET4,
            SQ_SOCKET_TYPE_STREAM,
            SQ_SOCKET_PROTOCOL_TCP,
        )
        .expect("a fresh module resolves");

        // A second answer, so that walking the list is meaningful -- one
        // `getaddrinfo` call routinely answers several, and the image walks
        // them with `sqResolverGetAddressInfoNext`.
        let asked_for = {
            let mut st = state().expect("a fresh module hands out the state");
            let first = st.results.first().expect("one answer").clone();
            let second = ResolvedAddr {
                sockaddr: vec![0xAA; first.sockaddr.len()],
                ..first
            };
            st.results.push(second.clone());
            second.sockaddr
        };
        assert!(gai_next().expect("still healthy"), "walked to the second");

        // --- a panic between the refill and the cursor reset ---------------
        let never_asked_for = vec![0xBBu8; asked_for.len()];
        let replacement = never_asked_for.clone();
        let torn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut st = state().expect("still healthy");
            let template = st.results.first().expect("one answer").clone();
            // The next lookup's answers are in place ...
            st.results = vec![
                template.clone(),
                ResolvedAddr {
                    sockaddr: replacement,
                    ..template
                },
            ];
            // ... and `st.cursor = 0` is the statement after this one.
            panic!("getaddrinfo raised mid-refill");
        }));
        assert!(torn.is_err());

        // The state really is torn, which is what makes the assertions below
        // mean something. Reached the way the old code reached it -- and this
        // is the only place in the crate that may still do so.
        {
            let recovered = STATE.lock().unwrap_or_else(PoisonError::into_inner);
            assert_eq!(recovered.cursor, 1, "still indexing the old list");
            assert_eq!(
                recovered.current().map(|e| e.sockaddr.clone()),
                Some(never_asked_for),
                "a swallowed poison hands the image this address, which it \
                 never asked to resolve, in place of the one it did"
            );
        }

        // --- the fix -------------------------------------------------------
        assert_eq!(
            state().err(),
            Some(PrimErr::Unsupported),
            "a cursor indexing the wrong list must never be handed to a caller"
        );

        // And every accessor above it propagates that refusal rather than
        // answering out of the torn pair. These eight are the ones the repair
        // pass made fallible; a `PrimResult` they threw away would put this
        // whole file back where it started.
        let mut dest = vec![0u8; address::ADDRESS_HEADER_SIZE + asked_for.len()];
        assert_eq!(gai_size().err(), Some(PrimErr::Unsupported));
        assert_eq!(gai_result(&mut dest).err(), Some(PrimErr::Unsupported));
        assert_eq!(gai_family().err(), Some(PrimErr::Unsupported));
        assert_eq!(gai_type().err(), Some(PrimErr::Unsupported));
        assert_eq!(gai_protocol().err(), Some(PrimErr::Unsupported));
        assert_eq!(gai_next().err(), Some(PrimErr::Unsupported));
        assert_eq!(resolver_status().err(), Some(PrimErr::Unsupported));
        assert_eq!(resolver_error().err(), Some(PrimErr::Unsupported));
        assert!(
            dest.iter().all(|&b| b == 0),
            "and nothing was copied into the image's SocketAddress ByteArray"
        );

        // --- teardown ------------------------------------------------------
        //
        // The assertions are made; this hands the binary back to the other 37
        // tests, which are all still blocked on the `net_lock` held above. It
        // is the only `clear_poison` in this crate outside `net_lock` itself,
        // it is `#[cfg(test)]`, and no image can reach it -- `state` is still
        // the only way in from a primitive, and it still refuses.
        STATE.clear_poison();
        *state().expect("cleared") = ResolverState::new();
    }
}
