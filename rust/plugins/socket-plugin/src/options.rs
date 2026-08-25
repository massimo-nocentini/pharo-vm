//! The socket-option name table and the value parsing the image's string-based
//! option protocol requires. Pure functions, unit-tested without a VM.
//!
//! The C plugin's comment block explains the deliberate restriction to the
//! portable, integer-valued options; the table below is that table, entry for
//! entry, including its `#ifdef`s (`TCP_CORK` exists on Linux only).

use core::ffi::{c_int, c_long};

/// One row of the C `socketOptions[]` table.
pub struct SocketOption {
    /// The name the image sends.
    pub name: &'static str,
    /// Protocol level for `setsockopt`/`getsockopt`.
    pub level: c_int,
    /// Option name for `setsockopt`/`getsockopt`.
    pub optname: c_int,
}

// SOL_IP / SOL_TCP fall back to the IPPROTO_* constants when the platform does
// not define them, which is what the C's `#ifndef SOL_IP` dance did; the
// IPPROTO values are the portable ones, so they are used directly.
const SOL_IP: c_int = libc::IPPROTO_IP;
const SOL_TCP: c_int = libc::IPPROTO_TCP;

/// `socketOptions[]` from `SocketPluginImpl.c`.
pub static SOCKET_OPTIONS: &[SocketOption] = &[
    SocketOption { name: "SO_DEBUG", level: libc::SOL_SOCKET, optname: libc::SO_DEBUG },
    SocketOption { name: "SO_REUSEADDR", level: libc::SOL_SOCKET, optname: libc::SO_REUSEADDR },
    SocketOption { name: "SO_DONTROUTE", level: libc::SOL_SOCKET, optname: libc::SO_DONTROUTE },
    SocketOption { name: "SO_BROADCAST", level: libc::SOL_SOCKET, optname: libc::SO_BROADCAST },
    SocketOption { name: "SO_SNDBUF", level: libc::SOL_SOCKET, optname: libc::SO_SNDBUF },
    SocketOption { name: "SO_RCVBUF", level: libc::SOL_SOCKET, optname: libc::SO_RCVBUF },
    SocketOption { name: "SO_KEEPALIVE", level: libc::SOL_SOCKET, optname: libc::SO_KEEPALIVE },
    SocketOption { name: "SO_OOBINLINE", level: libc::SOL_SOCKET, optname: libc::SO_OOBINLINE },
    SocketOption { name: "SO_LINGER", level: libc::SOL_SOCKET, optname: libc::SO_LINGER },
    SocketOption { name: "IP_TTL", level: SOL_IP, optname: libc::IP_TTL },
    SocketOption { name: "IP_HDRINCL", level: SOL_IP, optname: libc::IP_HDRINCL },
    SocketOption { name: "IP_MULTICAST_IF", level: SOL_IP, optname: libc::IP_MULTICAST_IF },
    SocketOption { name: "IP_MULTICAST_TTL", level: SOL_IP, optname: libc::IP_MULTICAST_TTL },
    SocketOption { name: "IP_MULTICAST_LOOP", level: SOL_IP, optname: libc::IP_MULTICAST_LOOP },
    SocketOption { name: "IP_ADD_MEMBERSHIP", level: SOL_IP, optname: libc::IP_ADD_MEMBERSHIP },
    SocketOption { name: "IP_DROP_MEMBERSHIP", level: SOL_IP, optname: libc::IP_DROP_MEMBERSHIP },
    SocketOption { name: "TCP_MAXSEG", level: SOL_TCP, optname: libc::TCP_MAXSEG },
    SocketOption { name: "TCP_NODELAY", level: SOL_TCP, optname: libc::TCP_NODELAY },
    #[cfg(target_os = "linux")]
    SocketOption { name: "TCP_CORK", level: SOL_TCP, optname: libc::TCP_CORK },
    SocketOption { name: "SO_REUSEPORT", level: libc::SOL_SOCKET, optname: libc::SO_REUSEPORT },
];

/// `findOption`: looks the name up, with the C's exact quirks -- names of 32
/// bytes or more are never found, and an interior NUL terminates the name
/// early (the C goes through a `strncpy` into a 32-byte buffer).
pub fn find_option(name: &[u8]) -> Option<&'static SocketOption> {
    if name.len() >= 32 {
        return None;
    }
    let effective = match name.iter().position(|&b| b == 0) {
        Some(nul) => &name[..nul],
        None => name,
    };
    SOCKET_OPTIONS
        .iter()
        .find(|opt| opt.name.as_bytes() == effective)
}

/// `strtol(buf, &endptr, 0)`, as the option-value parser calls it: skips
/// leading whitespace, accepts an optional sign, auto-detects base (`0x` hex,
/// leading `0` octal, else decimal), and answers `(value, chars consumed)`.
/// No conversion answers `(0, 0)` -- `endptr == buf`.
///
/// The C decides "is this value an integer?" by `endptr - buf ==
/// optionValueSize`, so the consumed count is the part that matters.
pub fn parse_c_long(bytes: &[u8]) -> (c_long, usize) {
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }
    let mut negative = false;
    let mut j = i;
    if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
        negative = bytes[j] == b'-';
        j += 1;
    }
    // Base detection, exactly strtol's: "0x"/"0X" followed by a hex digit is
    // hex; otherwise a leading '0' is octal (and is itself a digit).
    let (base, mut k) = if j + 1 < bytes.len()
        && bytes[j] == b'0'
        && (bytes[j + 1] == b'x' || bytes[j + 1] == b'X')
        && j + 2 < bytes.len()
        && bytes[j + 2].is_ascii_hexdigit()
    {
        (16, j + 2)
    } else if j < bytes.len() && bytes[j] == b'0' {
        (8, j)
    } else {
        (10, j)
    };

    let mut value: c_long = 0;
    let mut digits = 0;
    while k < bytes.len() {
        let d = match bytes[k] {
            b @ b'0'..=b'9' => (b - b'0') as c_long,
            b @ b'a'..=b'f' if base == 16 => (b - b'a' + 10) as c_long,
            b @ b'A'..=b'F' if base == 16 => (b - b'A' + 10) as c_long,
            _ => break,
        };
        if d >= base {
            break;
        }
        // Option values are at most 4 characters when this is reached, so
        // saturation matches strtol's clamping closely enough never to differ.
        value = value.saturating_mul(base).saturating_add(d);
        digits += 1;
        k += 1;
    }
    if digits == 0 {
        return (0, 0);
    }
    (if negative { -value } else { value }, k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_options_resolve() {
        let opt = find_option(b"SO_KEEPALIVE").expect("in the table");
        assert_eq!(opt.level, libc::SOL_SOCKET);
        assert_eq!(opt.optname, libc::SO_KEEPALIVE);

        let opt = find_option(b"TCP_NODELAY").expect("in the table");
        assert_eq!(opt.level, libc::IPPROTO_TCP);
        assert_eq!(opt.optname, libc::TCP_NODELAY);

        let opt = find_option(b"IP_TTL").expect("in the table");
        assert_eq!(opt.level, libc::IPPROTO_IP);
        assert_eq!(opt.optname, libc::IP_TTL);
    }

    #[test]
    fn unknown_and_oversized_names_fail() {
        assert!(find_option(b"SO_BOGUS").is_none());
        assert!(find_option(b"").is_none());
        // 32 bytes and up can never match: the C buffer is char[32].
        assert!(find_option(&[b'A'; 32]).is_none());
        assert!(find_option(&[b'A'; 100]).is_none());
    }

    #[test]
    fn interior_nul_truncates_like_strncpy() {
        assert!(find_option(b"SO_KEEPALIVE\0garbage").is_some());
        assert!(find_option(b"SO_\0KEEPALIVE").is_none());
    }

    #[test]
    fn parse_decimal() {
        assert_eq!(parse_c_long(b"1"), (1, 1));
        assert_eq!(parse_c_long(b"8192"), (8192, 4));
        assert_eq!(parse_c_long(b"-5"), (-5, 2));
        assert_eq!(parse_c_long(b"+7"), (7, 2));
        assert_eq!(parse_c_long(b" 42"), (42, 3), "whitespace counts as consumed");
    }

    #[test]
    fn parse_bases() {
        assert_eq!(parse_c_long(b"0x10"), (16, 4));
        assert_eq!(parse_c_long(b"0XfF"), (255, 4));
        assert_eq!(parse_c_long(b"010"), (8, 3), "leading zero is octal");
        assert_eq!(parse_c_long(b"0"), (0, 1));
        // strtol("0x", ...) converts the "0" and stops at the 'x'.
        assert_eq!(parse_c_long(b"0x"), (0, 1));
    }

    #[test]
    fn parse_stops_at_non_digits() {
        assert_eq!(parse_c_long(b"12ab"), (12, 2));
        assert_eq!(parse_c_long(b""), (0, 0));
        assert_eq!(parse_c_long(b"abc"), (0, 0));
        assert_eq!(parse_c_long(b"-"), (0, 0));
        assert_eq!(parse_c_long(b"1\0"), (1, 1), "NUL ends the number");
        // 0778: '8' is not an octal digit, so only "077" converts.
        assert_eq!(parse_c_long(b"0778"), (0o77, 3));
    }

    /// The whole "is this string all one integer?" decision the C makes:
    /// `size <= sizeof(int) && endptr - buf == size`.
    #[test]
    fn integer_detection_as_the_c_does_it() {
        let is_integer =
            |v: &[u8]| v.len() <= 4 && parse_c_long(v).1 == v.len();
        assert!(is_integer(b"1"));
        assert!(is_integer(b"8192"));
        assert!(!is_integer(b"65536"), "five chars: passed as raw bytes");
        assert!(!is_integer(b"1a"));
        assert!(is_integer(b""), "empty value parses as zero chars of zero");
    }
}
