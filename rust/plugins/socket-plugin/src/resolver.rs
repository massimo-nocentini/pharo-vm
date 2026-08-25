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
use std::ffi::CString;
use std::sync::Mutex;

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
    /// `addrList`: head of the current `getaddrinfo` result chain (owned by
    /// libc, freed with `freeaddrinfo`).
    addr_list: *mut libc::addrinfo,
    /// `addrInfo`: cursor into `addr_list` -- or into `local_info`.
    addr_info: *mut libc::addrinfo,
    /// `localInfo`: a hand-built AF_UNIX result (Boxes leaked into raw
    /// pointers; freed on the next lookup, as the C frees its callocs).
    local_info: *mut libc::addrinfo,
    /// `hostNameInfo` / `servNameInfo` / `nameInfoValid`.
    host_name_info: Vec<u8>,
    serv_name_info: Vec<u8>,
    name_info_valid: bool,
}

// The raw addrinfo pointers make the struct !Send by default. Every access
// happens on the interpreter thread (or under the test lock); the mutex around
// the state enforces exclusivity either way.
unsafe impl Send for ResolverState {}

impl ResolverState {
    const fn new() -> Self {
        Self {
            last_name: Vec::new(),
            last_addr: 0,
            last_error: 0,
            addr_list: ptr::null_mut(),
            addr_info: ptr::null_mut(),
            local_info: ptr::null_mut(),
            host_name_info: Vec::new(),
            serv_name_info: Vec::new(),
            name_info_valid: false,
        }
    }

    /// Frees the previous lookup's results, as the top of
    /// `sqResolverGetAddressInfo...` does.
    fn drop_results(&mut self) {
        if !self.addr_list.is_null() {
            // SAFETY: the pointer came from getaddrinfo and is freed once.
            unsafe { libc::freeaddrinfo(self.addr_list) };
            self.addr_list = ptr::null_mut();
            self.addr_info = ptr::null_mut();
        }
        if !self.local_info.is_null() {
            // SAFETY: both pointers came from Box::into_raw in the local-
            // socket path below and are freed once, ai_addr first as in C.
            unsafe {
                let info = Box::from_raw(self.local_info);
                if !info.ai_addr.is_null() {
                    drop(Box::from_raw(info.ai_addr as *mut libc::sockaddr_un));
                }
                drop(info);
            }
            self.local_info = ptr::null_mut();
            self.addr_info = ptr::null_mut();
        }
    }
}

// Two libc functions the `libc` crate does not re-export; both are in the C
// library every Rust program already links.
extern "C" {
    fn clock() -> libc::clock_t;
    fn gethostbyaddr(
        addr: *const libc::c_void,
        len: libc::socklen_t,
        addrtype: c_int,
    ) -> *mut libc::hostent;
}

static STATE: Mutex<ResolverState> = Mutex::new(ResolverState::new());

fn state() -> std::sync::MutexGuard<'static, ResolverState> {
    // A poisoned lock is unreachable: no code below panics while holding it,
    // and the workspace aborts on panic anyway.
    STATE.lock().unwrap_or_else(|e| e.into_inner())
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
pub fn resolver_status() -> i32 {
    if current_session() == 0 {
        return RESOLVER_UNINITIALISED;
    }
    if state().last_error != 0 {
        return RESOLVER_ERROR;
    }
    RESOLVER_SUCCESS
}

/// `sqResolverError`.
pub fn resolver_error() -> c_int {
    state().last_error
}

/// `h_errno` after a failed `gethostbyaddr`, where the platform exposes it.
fn last_h_errno() -> c_int {
    #[cfg(target_os = "linux")]
    {
        extern "C" {
            // glibc and musl both provide the per-thread h_errno this way.
            fn __h_errno_location() -> *mut c_int;
        }
        unsafe { *__h_errno_location() }
    }
    #[cfg(not(target_os = "linux"))]
    {
        1 // HOST_NOT_FOUND; platforms without the accessor lose the detail
    }
}

/// `sqResolverStartAddrLookup`: reverse lookup via `gethostbyaddr`,
/// synchronously; the result lands in `lastName` ("" on failure).
pub fn start_addr_lookup(net_address: u32) {
    let mut st = state();
    st.last_error = 0;
    let n_addr: u32 = net_address.to_be(); // htonl
    // SAFETY: gethostbyaddr reads 4 bytes at the given pointer; the result
    // points into libc-owned storage valid until the next resolver call, and
    // is copied out immediately.
    let he = unsafe {
        gethostbyaddr(
            &n_addr as *const u32 as *const libc::c_void,
            mem::size_of::<u32>() as libc::socklen_t,
            libc::AF_INET,
        )
    };
    if he.is_null() {
        st.last_error = last_h_errno();
        st.last_name.clear(); // strncpy of "" clears the C buffer too
        return;
    }
    let name = unsafe { core::ffi::CStr::from_ptr((*he).h_name) }.to_bytes();
    let len = name.len().min(MAX_HOST_NAME_LEN); // strncpy truncation
    st.last_name = name[..len].to_vec();
}

/// `sqResolverAddrLookupResultSize`.
pub fn addr_lookup_result_size() -> usize {
    state().last_name.len()
}

/// `sqResolverAddrLookupResult`: copies `lastName` into the answer String.
pub fn addr_lookup_result(dest: &mut [u8]) {
    let st = state();
    let n = st.last_name.len().min(dest.len());
    dest[..n].copy_from_slice(&st.last_name[..n]);
}

/// `nameToAddr`: `getaddrinfo` with no hints, first AF_INET result, host
/// order. Sets `last_error` on failure and answers 0.
fn name_to_addr(st: &mut ResolverState, host: &CString) -> u32 {
    let mut result: *mut libc::addrinfo = ptr::null_mut();
    // SAFETY: host is a valid C string; result is freed below.
    let error = unsafe { libc::getaddrinfo(host.as_ptr(), ptr::null(), ptr::null(), &mut result) };
    if error != 0 {
        st.last_error = error;
        return 0;
    }
    let mut address = 0u32;
    let mut cursor = result;
    while !cursor.is_null() && address == 0 {
        // SAFETY: cursor walks the chain getaddrinfo returned.
        unsafe {
            if (*cursor).ai_family == libc::AF_INET {
                let sin = (*cursor).ai_addr as *const libc::sockaddr_in;
                address = u32::from_be((*sin).sin_addr.s_addr); // ntohl
            }
            cursor = (*cursor).ai_next;
        }
    }
    // SAFETY: freed exactly once.
    unsafe { libc::freeaddrinfo(result) };
    address
}

/// `sqResolverStartNameLookup`: synchronous forward lookup; signals the
/// resolver semaphore before returning.
pub fn start_name_lookup(host_name: &[u8]) {
    {
        let mut st = state();
        let len = host_name.len().min(MAX_HOST_NAME_LEN);
        // The C copies into a NUL-terminated buffer, so an interior NUL
        // truncates what getaddrinfo sees.
        let effective = match host_name[..len].iter().position(|&b| b == 0) {
            Some(nul) => &host_name[..nul],
            None => &host_name[..len],
        };
        st.last_name = effective.to_vec();
        st.last_error = 0;
        let host = CString::new(st.last_name.clone()).expect("interior NULs were stripped");
        st.last_addr = name_to_addr(&mut st, &host);
    }
    // "we're done before we even started"
    signal_resolver();
}

/// `sqResolverNameLookupResult`: fails if the last lookup failed.
pub fn name_lookup_result() -> PrimResult<u32> {
    let st = state();
    if st.last_error != 0 {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.last_addr)
}

/// `sqResolverLocalAddress` (the Unix branch): walk `getifaddrs` for the
/// first AF_INET address on `eth0` or `wlan0`.
///
/// The C's own TODO admits this does not cope with other interface names; the
/// walk is reproduced as is.
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

/// `sqResolverGetAddressInfoHostSizeServiceSizeFlagsFamilyTypeProtocol`.
///
/// Synchronous; frees the previous results, runs the lookup, signals the
/// resolver semaphore. On Linux a `getaddrinfo` failure "succeeds with zero
/// results" -- the C could only distinguish impossible constraints from
/// genuine failure through `EAI_BADHINTS`, which glibc does not define, so
/// its Linux build compiled down to exactly this.
pub fn get_address_info(
    host: &[u8],
    serv: &[u8],
    flags: sqInt,
    family: sqInt,
    type_: sqInt,
    protocol: sqInt,
) -> PrimResult<()> {
    let mut st = state();
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
        if let Ok(path) = CString::new(serv_c.clone()) {
            // SAFETY: plain stat on a NUL-terminated path.
            let mut stat_buf: libc::stat = unsafe { mem::zeroed() };
            let stated = unsafe { libc::stat(path.as_ptr(), &mut stat_buf) };
            if stated == 0 && stat_buf.st_mode & libc::S_IFSOCK != 0 {
                let mut saun: Box<libc::sockaddr_un> = Box::new(unsafe { mem::zeroed() });
                saun.sun_family = libc::AF_UNIX as libc::sa_family_t;
                for (dst, src) in saun.sun_path.iter_mut().zip(serv_c.iter()) {
                    *dst = *src as c_char;
                }
                let mut info: Box<libc::addrinfo> = Box::new(unsafe { mem::zeroed() });
                info.ai_family = libc::AF_UNIX;
                info.ai_socktype = libc::SOCK_STREAM;
                info.ai_addrlen = mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
                info.ai_addr = Box::into_raw(saun) as *mut libc::sockaddr;
                let info = Box::into_raw(info);
                st.local_info = info;
                st.addr_info = info;
                drop(st);
                signal_resolver();
                return Ok(());
            }
        }
    }

    let mut request: libc::addrinfo = unsafe { mem::zeroed() };
    if flags & SQ_SOCKET_NUMERIC != 0 {
        request.ai_flags |= libc::AI_NUMERICHOST;
    }
    if flags & SQ_SOCKET_PASSIVE != 0 {
        request.ai_flags |= libc::AI_PASSIVE;
    }
    request.ai_family = match family {
        SQ_SOCKET_FAMILY_LOCAL => libc::AF_UNIX,
        SQ_SOCKET_FAMILY_INET4 => libc::AF_INET,
        SQ_SOCKET_FAMILY_INET6 => libc::AF_INET6,
        _ => 0,
    };
    request.ai_socktype = match type_ {
        SQ_SOCKET_TYPE_STREAM => libc::SOCK_STREAM,
        SQ_SOCKET_TYPE_DGRAM => libc::SOCK_DGRAM,
        _ => 0,
    };
    request.ai_protocol = match protocol {
        SQ_SOCKET_PROTOCOL_TCP => libc::IPPROTO_TCP,
        SQ_SOCKET_PROTOCOL_UDP => libc::IPPROTO_UDP,
        _ => 0,
    };

    let host_cs = CString::new(host_c).expect("interior NULs were stripped");
    let serv_cs = CString::new(serv_c).expect("interior NULs were stripped");
    let mut list: *mut libc::addrinfo = ptr::null_mut();
    // SAFETY: standard getaddrinfo protocol; NULL node/service when empty, as
    // the C passes 0 for zero sizes.
    let gai_error = unsafe {
        libc::getaddrinfo(
            if host.is_empty() { ptr::null() } else { host_cs.as_ptr() },
            if serv.is_empty() { ptr::null() } else { serv_cs.as_ptr() },
            &request,
            &mut list,
        )
    };
    if gai_error != 0 {
        // Succeed with zero results: see the function comment.
        list = ptr::null_mut();
    }
    st.addr_list = list;
    st.addr_info = list;
    drop(st);
    signal_resolver();
    Ok(())
}

/// `sqResolverGetAddressInfoSize`: -1 when the cursor is exhausted (not a
/// failure -- the image tests for -1).
pub fn gai_size() -> isize {
    let st = state();
    if st.addr_info.is_null() {
        return -1;
    }
    // SAFETY: addr_info points into the live result chain.
    (address::ADDRESS_HEADER_SIZE + unsafe { (*st.addr_info).ai_addrlen } as usize) as isize
}

/// `sqResolverGetAddressInfoResultSize`: writes header + raw sockaddr.
pub fn gai_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state();
    if st.addr_info.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: addr_info points into the live result chain; ai_addr spans
    // ai_addrlen bytes.
    let (addr, len) = unsafe {
        (
            (*st.addr_info).ai_addr as *const u8,
            (*st.addr_info).ai_addrlen as usize,
        )
    };
    if dest.len() < address::ADDRESS_HEADER_SIZE + len {
        return Err(PrimErr::GenericFailure);
    }
    address::write_header(dest, current_session(), len as i32);
    let payload = unsafe { core::slice::from_raw_parts(addr, len) };
    dest[address::ADDRESS_HEADER_SIZE..address::ADDRESS_HEADER_SIZE + len]
        .copy_from_slice(payload);
    Ok(())
}

/// `sqResolverGetAddressInfoFamily`.
pub fn gai_family() -> PrimResult<sqInt> {
    let st = state();
    if st.addr_info.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: live cursor.
    Ok(match unsafe { (*st.addr_info).ai_family } {
        libc::AF_UNIX => SQ_SOCKET_FAMILY_LOCAL,
        libc::AF_INET => SQ_SOCKET_FAMILY_INET4,
        libc::AF_INET6 => SQ_SOCKET_FAMILY_INET6,
        _ => SQ_SOCKET_FAMILY_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoType`.
pub fn gai_type() -> PrimResult<sqInt> {
    let st = state();
    if st.addr_info.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: live cursor.
    Ok(match unsafe { (*st.addr_info).ai_socktype } {
        libc::SOCK_STREAM => SQ_SOCKET_TYPE_STREAM,
        libc::SOCK_DGRAM => SQ_SOCKET_TYPE_DGRAM,
        _ => SQ_SOCKET_TYPE_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoProtocol`.
pub fn gai_protocol() -> PrimResult<sqInt> {
    let st = state();
    if st.addr_info.is_null() {
        return Err(PrimErr::GenericFailure);
    }
    // SAFETY: live cursor.
    Ok(match unsafe { (*st.addr_info).ai_protocol } {
        libc::IPPROTO_TCP => SQ_SOCKET_PROTOCOL_TCP,
        libc::IPPROTO_UDP => SQ_SOCKET_PROTOCOL_UDP,
        _ => SQ_SOCKET_PROTOCOL_UNSPECIFIED,
    })
}

/// `sqResolverGetAddressInfoNext`: advances the cursor, answers whether an
/// entry remains.
pub fn gai_next() -> bool {
    let mut st = state();
    if st.addr_info.is_null() {
        return false;
    }
    // SAFETY: live cursor.
    st.addr_info = unsafe { (*st.addr_info).ai_next };
    !st.addr_info.is_null()
}

// ---------------------------------------------------------------------------
// getnameinfo: reverse host/service lookup
// ---------------------------------------------------------------------------

/// `sqResolverGetNameInfoSizeFlags`.
pub fn get_name_info(addr: &[u8], flags: sqInt) -> PrimResult<()> {
    {
        let mut st = state();
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
    let st = state();
    if !st.name_info_valid {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.host_name_info.len())
}

/// `sqResolverGetNameInfoHostResultSize`.
pub fn ni_host_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state();
    if !st.name_info_valid || dest.len() < st.host_name_info.len() {
        return Err(PrimErr::GenericFailure);
    }
    dest[..st.host_name_info.len()].copy_from_slice(&st.host_name_info);
    Ok(())
}

/// `sqResolverGetNameInfoServiceSize`.
pub fn ni_service_size() -> PrimResult<usize> {
    let st = state();
    if !st.name_info_valid {
        return Err(PrimErr::GenericFailure);
    }
    Ok(st.serv_name_info.len())
}

/// `sqResolverGetNameInfoServiceResultSize`.
pub fn ni_service_result(dest: &mut [u8]) -> PrimResult<()> {
    let st = state();
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
        assert_eq!(resolver_status(), RESOLVER_UNINITIALISED);
        assert_eq!(network_init(5), 0);
        assert_ne!(current_session(), 0);
        assert_eq!(network_init(6), 0, "re-init is not an error");
        assert_eq!(resolver_status(), RESOLVER_SUCCESS);
        network_shutdown();
        assert_eq!(current_session(), 0);
        network_init(5);
    }

    #[test]
    fn numeric_name_lookup() {
        let _guard = net_lock();
        network_init(0);
        start_name_lookup(b"127.0.0.1");
        assert_eq!(resolver_error(), 0);
        assert_eq!(name_lookup_result().unwrap(), 0x7f00_0001);
        // The looked-up name is what addr-lookup-result answers afterwards.
        assert_eq!(addr_lookup_result_size(), 9);
        let mut buf = vec![0u8; 9];
        addr_lookup_result(&mut buf);
        assert_eq!(&buf, b"127.0.0.1");
    }

    #[test]
    fn failed_name_lookup_reports_error() {
        let _guard = net_lock();
        network_init(0);
        // RFC 6761 reserves .invalid: this cannot resolve.
        start_name_lookup(b"does-not-exist.invalid");
        assert_ne!(resolver_error(), 0);
        assert!(name_lookup_result().is_err());
        assert_eq!(resolver_status(), RESOLVER_ERROR);
        // Clean up for the next test.
        start_name_lookup(b"127.0.0.1");
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

        let size = gai_size();
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
        while gai_next() {}
        assert_eq!(gai_size(), -1);
        assert!(gai_family().is_err());
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
        start_addr_lookup(0x7f00_0001);
        // Whether the sandbox can reverse-resolve 127.0.0.1 is environment-
        // dependent; the contract is: either a name arrived and no error, or
        // no name and an error.
        let size = addr_lookup_result_size();
        if size == 0 {
            assert_ne!(resolver_error(), 0);
        } else {
            assert_eq!(resolver_error(), 0);
        }
        // Restore a clean resolver state.
        start_name_lookup(b"127.0.0.1");
    }
}
