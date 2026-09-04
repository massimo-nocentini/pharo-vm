//! The two address formats the image hands this plugin, as pure functions.
//!
//! * A **net address**: a four-byte ByteArray holding an IPv4 address in
//!   network order (`netAddressToInt:` / `intToNetAddress:` in the generated
//!   plugin).
//! * A **socket address**: a ByteArray holding an 8-byte header -- the network
//!   session ID and the payload size, both native-endian C `int`s -- followed
//!   by a raw OS `sockaddr` (the IPv6-capable API added in 2007). The header
//!   is what `addressValid` in `SocketPluginImpl.c` checks: an address from a
//!   previous network session is rejected.
//!
//! Everything here works on byte slices so it can be unit-tested without a VM.

use core::mem;

/// `sizeof(struct addressHeader)`: two C ints.
pub const ADDRESS_HEADER_SIZE: usize = 8;

/// Reads the given 4-byte net address as a host-order integer.
///
/// Answers `None` when the ByteArray is not exactly 4 bytes, which the caller
/// turns into the same `primitiveFail()` the C's `netAddressToInt:` raised.
pub fn net_address_to_int(bytes: &[u8]) -> Option<u32> {
    let &[a, b, c, d] = bytes else { return None };
    Some(u32::from_be_bytes([a, b, c, d]))
}

/// The 4 bytes of a net address for a host-order integer.
pub fn int_to_net_address(addr: u32) -> [u8; 4] {
    addr.to_be_bytes()
}

/// `addressValid(A, S)`: same session, and the stored size matches the
/// ByteArray's size minus the header.
pub fn address_valid(bytes: &[u8], session: i32) -> bool {
    if session == 0 || bytes.len() < ADDRESS_HEADER_SIZE {
        return false;
    }
    let stored_session = i32::from_ne_bytes(bytes[0..4].try_into().unwrap());
    let stored_size = i32::from_ne_bytes(bytes[4..8].try_into().unwrap());
    stored_session == session && stored_size as usize == bytes.len() - ADDRESS_HEADER_SIZE
}

/// Stamps the header ahead of a payload the caller is about to copy in.
pub fn write_header(bytes: &mut [u8], session: i32, payload_size: i32) {
    bytes[0..4].copy_from_slice(&session.to_ne_bytes());
    bytes[4..8].copy_from_slice(&payload_size.to_ne_bytes());
}

/// The raw `sockaddr` payload after the header.
pub fn payload(bytes: &[u8]) -> &[u8] {
    &bytes[ADDRESS_HEADER_SIZE..]
}

/// Reads a field out of the raw `sockaddr` payload by its offset and width,
/// the way [`set_port`] writes one.
///
/// The payload sits at an arbitrary offset inside an image ByteArray, so it
/// is not necessarily aligned for the struct it represents. Addressing the
/// one field wanted -- rather than reading the whole struct unaligned and
/// discarding the rest -- keeps every access in safe Rust and bounds-checked.
fn payload_field<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    let at = ADDRESS_HEADER_SIZE + offset;
    bytes.get(at..at + N)?.try_into().ok()
}

/// The `sa_family` of the payload, as C reads
/// `socketAddress(addr)->sa_family`.
///
/// A payload too short to hold a `sockaddr` answers `None`; the C
/// dereferenced whatever was there.
fn payload_family(bytes: &[u8]) -> Option<libc::sa_family_t> {
    if bytes.len() < ADDRESS_HEADER_SIZE + mem::size_of::<libc::sockaddr>() {
        return None;
    }
    let raw = payload_field::<{ mem::size_of::<libc::sa_family_t>() }>(
        bytes,
        mem::offset_of!(libc::sockaddr, sa_family),
    )?;
    Some(libc::sa_family_t::from_ne_bytes(raw))
}

/// `sqSocketAddressSizeGetPort`: the port of an AF_INET / AF_INET6 payload,
/// host order. `None` is the C's `success(false)` path.
pub fn get_port(bytes: &[u8], session: i32) -> Option<u16> {
    if !address_valid(bytes, session) {
        return None;
    }
    let family = payload_family(bytes)?;
    let payload_len = bytes.len() - ADDRESS_HEADER_SIZE;
    let offset = match family as i32 {
        libc::AF_INET if payload_len >= mem::size_of::<libc::sockaddr_in>() => {
            mem::offset_of!(libc::sockaddr_in, sin_port)
        }
        libc::AF_INET6 if payload_len >= mem::size_of::<libc::sockaddr_in6>() => {
            mem::offset_of!(libc::sockaddr_in6, sin6_port)
        }
        _ => return None,
    };
    // Both families store the port as a network-order u16 at that offset.
    Some(u16::from_be_bytes(payload_field::<2>(bytes, offset)?))
}

/// `sqSocketAddressSizeSetPort`: stores `port` (host order in, network order
/// stored) into an AF_INET / AF_INET6 payload. `false` is `success(false)`.
pub fn set_port(bytes: &mut [u8], session: i32, port: u16) -> bool {
    if !address_valid(bytes, session) {
        return false;
    }
    let Some(family) = payload_family(bytes) else {
        return false;
    };
    let offset = match family as i32 {
        libc::AF_INET => mem::offset_of!(libc::sockaddr_in, sin_port),
        libc::AF_INET6 => mem::offset_of!(libc::sockaddr_in6, sin6_port),
        _ => return false,
    };
    let at = ADDRESS_HEADER_SIZE + offset;
    let Some(slot) = bytes.get_mut(at..at + 2) else {
        return false;
    };
    slot.copy_from_slice(&port.to_be_bytes());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inet_address(session: i32, addr: u32, port: u16) -> Vec<u8> {
        let mut sin: libc::sockaddr_in = unsafe { mem::zeroed() };
        sin.sin_family = libc::AF_INET as libc::sa_family_t;
        sin.sin_port = port.to_be();
        sin.sin_addr.s_addr = addr.to_be();
        let payload = unsafe {
            core::slice::from_raw_parts(
                &sin as *const _ as *const u8,
                mem::size_of::<libc::sockaddr_in>(),
            )
        };
        let mut bytes = vec![0u8; ADDRESS_HEADER_SIZE + payload.len()];
        write_header(&mut bytes, session, payload.len() as i32);
        bytes[ADDRESS_HEADER_SIZE..].copy_from_slice(payload);
        bytes
    }

    #[test]
    fn net_address_roundtrip() {
        assert_eq!(net_address_to_int(&[127, 0, 0, 1]), Some(0x7f00_0001));
        assert_eq!(int_to_net_address(0x7f00_0001), [127, 0, 0, 1]);
        for addr in [0u32, 1, 0x0a00_0001, 0xffff_ffff] {
            assert_eq!(net_address_to_int(&int_to_net_address(addr)), Some(addr));
        }
    }

    #[test]
    fn net_address_rejects_wrong_sizes() {
        assert_eq!(net_address_to_int(&[]), None);
        assert_eq!(net_address_to_int(&[1, 2, 3]), None);
        assert_eq!(net_address_to_int(&[1, 2, 3, 4, 5]), None);
    }

    #[test]
    fn header_validation() {
        let bytes = inet_address(42, 0x7f00_0001, 8080);
        assert!(address_valid(&bytes, 42));
        assert!(!address_valid(&bytes, 43), "wrong session");
        assert!(!address_valid(&bytes, 0), "no session yet");
        assert!(!address_valid(&bytes[..7], 42), "shorter than the header");

        let mut wrong_size = bytes.clone();
        wrong_size.push(0); // size field no longer matches the payload length
        assert!(!address_valid(&wrong_size, 42));
    }

    #[test]
    fn port_get_and_set() {
        let mut bytes = inet_address(7, 0x7f00_0001, 8080);
        assert_eq!(get_port(&bytes, 7), Some(8080));
        assert_eq!(get_port(&bytes, 8), None, "wrong session fails");

        assert!(set_port(&mut bytes, 7, 443));
        assert_eq!(get_port(&bytes, 7), Some(443));
        assert!(!set_port(&mut bytes, 8, 443), "wrong session fails");
    }

    #[test]
    fn ipv6_port() {
        let mut sin6: libc::sockaddr_in6 = unsafe { mem::zeroed() };
        sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
        sin6.sin6_port = 53u16.to_be();
        let payload = unsafe {
            core::slice::from_raw_parts(
                &sin6 as *const _ as *const u8,
                mem::size_of::<libc::sockaddr_in6>(),
            )
        };
        let mut bytes = vec![0u8; ADDRESS_HEADER_SIZE + payload.len()];
        write_header(&mut bytes, 9, payload.len() as i32);
        bytes[ADDRESS_HEADER_SIZE..].copy_from_slice(payload);

        assert_eq!(get_port(&bytes, 9), Some(53));
        assert!(set_port(&mut bytes, 9, 5353));
        assert_eq!(get_port(&bytes, 9), Some(5353));
    }

    /// Why nothing above hard-codes an offset or a width.
    ///
    /// The payload these functions read is a raw OS `sockaddr`, and 4.4BSD's
    /// layout -- a `sa_len` byte first, then a one-byte `sa_family` -- is
    /// still Darwin's, while glibc dropped the length byte and made
    /// `sa_family` a 16-bit field at offset 0. The C got this for free by
    /// dereferencing the struct; the Rust gets it from `offset_of!` and
    /// `size_of`, and this pins what those answer on each platform so a wrong
    /// answer is a failing test rather than a misread port number.
    #[test]
    fn sa_family_sits_where_the_platform_puts_it() {
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        {
            assert_eq!(mem::size_of::<libc::sa_family_t>(), 1, "Darwin: __uint8_t");
            assert_eq!(
                mem::offset_of!(libc::sockaddr, sa_family),
                1,
                "Darwin: sa_len comes first"
            );
        }
        #[cfg(not(any(target_os = "macos", target_os = "ios")))]
        {
            assert_eq!(
                mem::size_of::<libc::sa_family_t>(),
                2,
                "glibc: unsigned short"
            );
            assert_eq!(
                mem::offset_of!(libc::sockaddr, sa_family),
                0,
                "glibc: no length byte"
            );
        }
        // Either way the port lands at the same place in each family, which
        // is the only thing get_port/set_port actually need.
        assert_eq!(mem::offset_of!(libc::sockaddr_in, sin_port), 2);
        assert_eq!(mem::offset_of!(libc::sockaddr_in6, sin6_port), 2);
    }

    #[test]
    fn unknown_family_fails() {
        let mut bytes = inet_address(3, 0, 1);
        // Corrupt the family: neither AF_INET nor AF_INET6.
        let bogus: libc::sa_family_t = 200;
        let off = ADDRESS_HEADER_SIZE + mem::offset_of!(libc::sockaddr, sa_family);
        bytes[off..off + mem::size_of::<libc::sa_family_t>()]
            .copy_from_slice(&bogus.to_ne_bytes());
        assert_eq!(get_port(&bytes, 3), None);
        assert!(!set_port(&mut bytes, 3, 1));
    }
}
