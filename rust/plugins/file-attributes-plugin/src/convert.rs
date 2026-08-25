//! Path encoding, matching FilePlugin's `sqUnixCharConv.c`.
//!
//! The C plugin does not convert paths itself: it links against FilePlugin and
//! calls its `sq2uxPath` / `ux2sqPath` (image encoding <-> platform encoding).
//! This port does the same at runtime -- the function pointers are fetched
//! with `ioLoadFunctionFrom("sq2uxPath", "FilePlugin")` and cached -- so the
//! two plugins keep sharing one conversion state, exactly as when they were
//! linked together.
//!
//! When FilePlugin is not available, [`convert_utf8_lossy`] reproduces what
//! those functions do on Linux, where `setLocaleEncoding` is never called and
//! both path encodings stay UTF-8: an iconv UTF-8 -> UTF-8 pass wrapped in
//! `convertChars`'s error handling. That wrapper's behaviour on bad input is
//! peculiar and is reproduced bit for bit:
//!
//! * an invalid sequence is skipped by the *count of leading 1-bits of its
//!   first byte* (`0xFE`/`0xFF` skip one), and a single `?` is emitted -- so
//!   `C3 28` consumes the valid `(` as well;
//! * a truncated sequence at the end of input is skipped to the end, one `?`;
//! * when the output buffer fills (`E2BIG`), conversion simply stops --
//!   truncation is silent, and the callers only ever detect the degenerate
//!   zero-length result.

use std::ffi::{c_char, c_int};

/// `int (*)(char *from, int fromLen, char *to, int toLen, int term)` --
/// the shape of FilePlugin's `sq2uxPath` and `ux2sqPath`.
pub type CConvertFn =
    unsafe extern "C" fn(*mut c_char, c_int, *mut c_char, c_int, c_int) -> c_int;

/// The two conversion directions, each either FilePlugin's own function or
/// the built-in fallback.
#[derive(Debug, Clone, Copy, Default)]
pub struct Converters {
    /// Image (precomposed UTF-8) to platform encoding.
    pub sq2ux: Option<CConvertFn>,
    /// Platform encoding to image encoding.
    pub ux2sq: Option<CConvertFn>,
}

impl Converters {
    /// Converts an image-encoded path for the OS. `to_len` is the C buffer
    /// size; as in the C, one byte of it is reserved for the terminator.
    #[must_use]
    pub fn to_platform(self, input: &[u8], to_len: usize) -> Vec<u8> {
        convert_with(self.sq2ux, input, to_len)
    }

    /// Converts a platform-encoded path for the image.
    #[must_use]
    pub fn to_smalltalk(self, input: &[u8], to_len: usize) -> Vec<u8> {
        convert_with(self.ux2sq, input, to_len)
    }
}

/// Runs one conversion, preferring the C function when it was resolved.
fn convert_with(f: Option<CConvertFn>, input: &[u8], to_len: usize) -> Vec<u8> {
    let Some(f) = f else {
        return convert_utf8_lossy(input, to_len.saturating_sub(1));
    };
    // The C signature takes a non-const `from`; hand it a scratch copy so it
    // can never touch the caller's data.
    let mut from = input.to_vec();
    let mut to = vec![0u8; to_len.max(1)];
    // SAFETY: `f` came from FilePlugin via ioLoadFunctionFrom and has the
    // documented sqUnixCharConv signature; both buffers are live and their
    // lengths are passed alongside. term=1 matches every C call site.
    let n = unsafe {
        f(
            from.as_mut_ptr().cast::<c_char>(),
            input.len() as c_int,
            to.as_mut_ptr().cast::<c_char>(),
            to_len as c_int,
            1,
        )
    };
    let n = if n < 0 { 0 } else { n as usize };
    to.truncate(n.min(to_len));
    to
}

/// The fallback conversion: UTF-8 validation with `convertChars`'s error
/// handling, as glibc iconv behaves for UTF-8 -> UTF-8 (overlong forms,
/// surrogates and out-of-range code points are all invalid).
///
/// `content_cap` is the room for converted bytes -- the C buffer size minus
/// the terminator. The result never includes a NUL.
#[must_use]
pub fn convert_utf8_lossy(from: &[u8], content_cap: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(from.len().min(content_cap));
    let mut i = 0;
    'outer: while i < from.len() {
        let rem = &from[i..];
        let (valid_len, has_error) = match std::str::from_utf8(rem) {
            Ok(_) => (rem.len(), false),
            Err(e) => (e.valid_up_to(), true),
        };
        // SAFETY: from_utf8 just validated exactly this prefix.
        let valid = unsafe { std::str::from_utf8_unchecked(&rem[..valid_len]) };
        for ch in valid.chars() {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf).as_bytes();
            if out.len() + encoded.len() > content_cap {
                // iconv's E2BIG: the C sets inbytes to 0 and stops. Nothing
                // after the full buffer is looked at again.
                break 'outer;
            }
            out.extend_from_slice(encoded);
        }
        i += valid_len;
        if !has_error {
            break;
        }
        // Invalid sequence at from[i]. The C skips one byte per leading 1-bit
        // of the first byte (0xFE/0xFF skip one), bounded by the remaining
        // input, and emits '?' only if the output still has room.
        let c = from[i];
        let remaining = from.len() - i;
        let skip = if c == 0xFE || c == 0xFF {
            1
        } else {
            let mut skip = 0usize;
            let mut mask = 0x80u8;
            while skip < remaining && mask != 0 && (c & mask) != 0 {
                skip += 1;
                mask >>= 1;
            }
            // A byte UTF-8 validation rejects always has its top bit set, so
            // skip >= 1; the max is belt and braces against an endless loop.
            skip.max(1)
        };
        i += skip;
        if out.len() < content_cap {
            out.push(b'?');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plenty of room: conversion is identity for valid UTF-8.
    fn conv(input: &[u8]) -> Vec<u8> {
        convert_utf8_lossy(input, 4096)
    }

    #[test]
    fn ascii_is_identity() {
        assert_eq!(conv(b"/etc/passwd"), b"/etc/passwd");
    }

    #[test]
    fn multibyte_is_identity() {
        let s = "caf\u{e9}/\u{1F600}/\u{20AC}".as_bytes();
        assert_eq!(conv(s), s);
    }

    #[test]
    fn empty_converts_to_empty() {
        // The C callers treat this zero-length result as an error; the
        // conversion itself just answers nothing.
        assert_eq!(conv(b""), b"");
    }

    /// `C3 28`: iconv stops at the C3; the skip counts C3's two leading 1-bits
    /// and swallows the perfectly valid `(` with it. Reproduced deliberately.
    #[test]
    fn invalid_lead_swallows_following_byte() {
        assert_eq!(conv(&[0xC3, 0x28, b'A']), b"?A");
    }

    /// `E2 82 41`: three leading 1-bits on E2 skip three bytes, `A` included.
    #[test]
    fn three_byte_lead_swallows_two_more() {
        assert_eq!(conv(&[0xE2, 0x82, 0x41, 0x42]), b"?B");
    }

    #[test]
    fn fe_and_ff_skip_one_byte() {
        assert_eq!(conv(&[0xFF, b'a']), b"?a");
        assert_eq!(conv(&[0xFE]), b"?");
    }

    #[test]
    fn bare_continuation_byte_skips_one() {
        assert_eq!(conv(&[0x80, b'x']), b"?x");
    }

    #[test]
    fn truncated_sequence_at_end_is_one_replacement() {
        assert_eq!(conv(&[b'a', 0xE2, 0x82]), b"a?");
    }

    /// Overlong `/` (C0 AF) is invalid UTF-8; two leading 1-bits skip both.
    #[test]
    fn overlong_encoding_is_replaced() {
        assert_eq!(conv(&[0xC0, 0xAF, b'z']), b"?z");
    }

    /// A UTF-8-encoded surrogate is invalid; ED has three leading 1-bits.
    #[test]
    fn surrogate_is_replaced() {
        assert_eq!(conv(&[0xED, 0xA0, 0x80, b'k']), b"?k");
    }

    /// E2BIG semantics: conversion stops at the first character that does not
    /// fit, never splitting a character.
    #[test]
    fn full_buffer_stops_silently() {
        assert_eq!(convert_utf8_lossy(b"abcd", 3), b"abc");
        // The Euro sign is three bytes and only one fits after "ab".
        assert_eq!(convert_utf8_lossy("ab\u{20AC}".as_bytes(), 3), b"ab");
    }

    /// An invalid sequence at a full buffer is consumed without a '?', and
    /// processing then stops at the next character that does not fit -- the
    /// exact order of convertChars's branches.
    #[test]
    fn invalid_at_full_buffer_emits_nothing() {
        assert_eq!(convert_utf8_lossy(&[b'a', 0xFF, b'b'], 1), b"a");
    }
}
