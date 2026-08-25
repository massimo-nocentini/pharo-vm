//! Character-encoding conversion, ported from
//! `plugins/FilePlugin/src/unix/sqUnixCharConv.c` (the `HAVE_ICONV_H`
//! branch, which is what every unix build except macOS compiled).
//!
//! The C represented an encoding as a `void *` that actually points at a
//! NUL-terminated iconv encoding name, kept in seven exported globals
//! (`sqTextEncoding`, `uxPathEncoding`, ...), and fed the names to
//! `iconv_open` on demand. This port keeps the same globals with the same
//! name-pointer representation -- `setEncoding`/`setNEncoding`/`freeEncoding`
//! still work on malloc'd C strings -- but performs the conversions natively
//! for the encodings the VM actually configures: UTF-8, ISO-8859-1,
//! ISO-8859-15 and MacRoman. A pair involving any other name falls back to a
//! bounded copy, exactly what the C did when `iconv_open` failed.
//!
//! The iconv loop's error behaviour is reproduced: an invalid or
//! unconvertible character is replaced by `?` and skipped by the
//! leading-ones count of its first byte, and a full output buffer stops the
//! conversion (E2BIG). One divergence: when that skip count computes to
//! zero the C would spin forever re-converting the same byte; here it skips
//! one byte instead.

use libc::{c_char, c_int, c_void};
use pharo_vm_plugin::proxy::sqInt;

// ---------------------------------------------------------------------------
// The exported encoding globals
//
// `static mut` because they are C ABI data symbols (declared `extern void *`
// in sqUnixCharConv.h); all access goes through raw pointers below.
// ---------------------------------------------------------------------------

static MACINTOSH: &[u8] = b"MACINTOSH\0";
static UTF8_NAME: &[u8] = b"UTF-8\0";
static ISO_8859_1: &[u8] = b"ISO-8859-1\0";
static ISO_8859_15: &[u8] = b"ISO-8859-15\0";

#[no_mangle]
pub static mut localeEncoding: *mut c_void = core::ptr::null_mut();
#[no_mangle]
pub static mut sqTextEncoding: *mut c_void = MACINTOSH.as_ptr() as *mut c_void;
#[no_mangle]
pub static mut uxTextEncoding: *mut c_void = ISO_8859_15.as_ptr() as *mut c_void;
#[no_mangle]
pub static mut sqPathEncoding: *mut c_void = UTF8_NAME.as_ptr() as *mut c_void;
#[no_mangle]
pub static mut uxPathEncoding: *mut c_void = UTF8_NAME.as_ptr() as *mut c_void;
#[no_mangle]
pub static mut uxUTF8Encoding: *mut c_void = UTF8_NAME.as_ptr() as *mut c_void;
#[no_mangle]
pub static mut uxXWinEncoding: *mut c_void = ISO_8859_1.as_ptr() as *mut c_void;

/// The four name strings `freeEncoding` must never free.
fn predefined() -> [*const u8; 4] {
    [
        MACINTOSH.as_ptr(),
        UTF8_NAME.as_ptr(),
        ISO_8859_1.as_ptr(),
        ISO_8859_15.as_ptr(),
    ]
}

// ---------------------------------------------------------------------------
// Encodings and tables
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Enc {
    Utf8,
    Latin1,
    Latin15,
    MacRoman,
}

/// MacRoman 0x80..=0xFF to Unicode. The standard Apple table.
#[rustfmt::skip]
const MAC_ROMAN_HIGH: [u16; 128] = [
    0x00C4, 0x00C5, 0x00C7, 0x00C9, 0x00D1, 0x00D6, 0x00DC, 0x00E1,
    0x00E0, 0x00E2, 0x00E4, 0x00E3, 0x00E5, 0x00E7, 0x00E9, 0x00E8,
    0x00EA, 0x00EB, 0x00ED, 0x00EC, 0x00EE, 0x00EF, 0x00F1, 0x00F3,
    0x00F2, 0x00F4, 0x00F6, 0x00F5, 0x00FA, 0x00F9, 0x00FB, 0x00FC,
    0x2020, 0x00B0, 0x00A2, 0x00A3, 0x00A7, 0x2022, 0x00B6, 0x00DF,
    0x00AE, 0x00A9, 0x2122, 0x00B4, 0x00A8, 0x2260, 0x00C6, 0x00D8,
    0x221E, 0x00B1, 0x2264, 0x2265, 0x00A5, 0x00B5, 0x2202, 0x2211,
    0x220F, 0x03C0, 0x222B, 0x00AA, 0x00BA, 0x03A9, 0x00E6, 0x00F8,
    0x00BF, 0x00A1, 0x00AC, 0x221A, 0x0192, 0x2248, 0x2206, 0x00AB,
    0x00BB, 0x2026, 0x00A0, 0x00C0, 0x00C3, 0x00D5, 0x0152, 0x0153,
    0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019, 0x00F7, 0x25CA,
    0x00FF, 0x0178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02,
    0x2021, 0x00B7, 0x201A, 0x201E, 0x2030, 0x00C2, 0x00CA, 0x00C1,
    0x00CB, 0x00C8, 0x00CD, 0x00CE, 0x00CF, 0x00CC, 0x00D3, 0x00D4,
    0xF8FF, 0x00D2, 0x00DA, 0x00DB, 0x00D9, 0x0131, 0x02C6, 0x02DC,
    0x00AF, 0x02D8, 0x02D9, 0x02DA, 0x00B8, 0x02DD, 0x02DB, 0x02C7,
];

/// The eight positions where ISO-8859-15 departs from ISO-8859-1.
const LATIN15_DELTA: [(u8, u16); 8] = [
    (0xA4, 0x20AC), // euro
    (0xA6, 0x0160), // S caron
    (0xA8, 0x0161), // s caron
    (0xB4, 0x017D), // Z caron
    (0xB8, 0x017E), // z caron
    (0xBC, 0x0152), // OE
    (0xBD, 0x0153), // oe
    (0xBE, 0x0178), // Y diaeresis
];

fn latin15_to_unicode(b: u8) -> u32 {
    for &(byte, cp) in &LATIN15_DELTA {
        if byte == b {
            return u32::from(cp);
        }
    }
    u32::from(b)
}

fn unicode_to_latin15(cp: u32) -> Option<u8> {
    for &(byte, special) in &LATIN15_DELTA {
        if u32::from(special) == cp {
            return Some(byte);
        }
        // The Latin-1 character this slot replaced is not in Latin-15.
        if u32::from(byte) == cp {
            return None;
        }
    }
    u8::try_from(cp).ok()
}

fn mac_roman_to_unicode(b: u8) -> u32 {
    if b < 0x80 {
        u32::from(b)
    } else {
        u32::from(MAC_ROMAN_HIGH[usize::from(b - 0x80)])
    }
}

fn unicode_to_mac_roman(cp: u32) -> Option<u8> {
    if cp < 0x80 {
        return Some(cp as u8);
    }
    MAC_ROMAN_HIGH
        .iter()
        .position(|&u| u32::from(u) == cp)
        .map(|i| 0x80 + i as u8)
}

/// Reads the C encoding name behind one of the `void *` handles and matches
/// it against the names the VM configures (case already uppercased by
/// `setNEncoding`; accept lowercase too for robustness).
///
/// # Safety
/// `code` is null or points at a NUL-terminated string.
unsafe fn parse_encoding(code: *const c_void) -> Option<Enc> {
    if code.is_null() {
        return None;
    }
    // SAFETY: caller contract.
    let name = unsafe {
        let p = code.cast::<c_char>();
        core::slice::from_raw_parts(p.cast::<u8>(), libc::strlen(p))
    };
    let mut upper = [0u8; 32];
    if name.len() > upper.len() {
        return None;
    }
    for (dst, src) in upper.iter_mut().zip(name) {
        *dst = src.to_ascii_uppercase();
    }
    match &upper[..name.len()] {
        b"UTF-8" | b"UTF8" => Some(Enc::Utf8),
        b"ISO-8859-1" | b"ISOLATIN1" | b"LATIN1" => Some(Enc::Latin1),
        b"ISO-8859-15" | b"ISOLATIN9" | b"LATIN9" => Some(Enc::Latin15),
        b"MACINTOSH" | b"MACROMAN" | b"MAC-ROMAN" | b"MAC" | b"CSMACINTOSH" => Some(Enc::MacRoman),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The conversion core (pure, unit-tested)
// ---------------------------------------------------------------------------

enum DecodeErr {
    /// EILSEQ / EINVAL: invalid or truncated sequence at this position.
    Bad,
}

/// Decodes one character, answering (code point, bytes consumed).
fn decode_one(enc: Enc, input: &[u8]) -> Result<(u32, usize), DecodeErr> {
    let b0 = input[0];
    match enc {
        Enc::Latin1 => Ok((u32::from(b0), 1)),
        Enc::Latin15 => Ok((latin15_to_unicode(b0), 1)),
        Enc::MacRoman => Ok((mac_roman_to_unicode(b0), 1)),
        Enc::Utf8 => decode_utf8(input),
    }
}

/// Strict UTF-8, rejecting what iconv rejects: bad leading bytes, bad
/// continuations, overlong forms, surrogates, and values past U+10FFFF.
fn decode_utf8(input: &[u8]) -> Result<(u32, usize), DecodeErr> {
    let b0 = input[0];
    let (len, init, min) = match b0 {
        0x00..=0x7F => return Ok((u32::from(b0), 1)),
        0xC0..=0xDF => (2, u32::from(b0 & 0x1F), 0x80),
        0xE0..=0xEF => (3, u32::from(b0 & 0x0F), 0x800),
        0xF0..=0xF7 => (4, u32::from(b0 & 0x07), 0x10000),
        _ => return Err(DecodeErr::Bad), // stray continuation, 0xF8..0xFF
    };
    if input.len() < len {
        return Err(DecodeErr::Bad); // truncated at end of input (EINVAL)
    }
    let mut cp = init;
    for &b in &input[1..len] {
        if b & 0xC0 != 0x80 {
            return Err(DecodeErr::Bad);
        }
        cp = (cp << 6) | u32::from(b & 0x3F);
    }
    if cp < min || cp > 0x10FFFF || (0xD800..=0xDFFF).contains(&cp) {
        return Err(DecodeErr::Bad);
    }
    Ok((cp, len))
}

enum EncodeErr {
    /// EILSEQ on the output side: the target has no such character.
    Unmappable,
    /// E2BIG: the encoded bytes do not fit the remaining room.
    NoRoom,
}

/// Encodes one code point into `out`, answering the bytes written.
fn encode_one(enc: Enc, cp: u32, out: &mut [u8]) -> Result<usize, EncodeErr> {
    match enc {
        Enc::Utf8 => {
            let mut buf = [0u8; 4];
            let n = match cp {
                0..=0x7F => {
                    buf[0] = cp as u8;
                    1
                }
                0x80..=0x7FF => {
                    buf[0] = 0xC0 | (cp >> 6) as u8;
                    buf[1] = 0x80 | (cp & 0x3F) as u8;
                    2
                }
                0x800..=0xFFFF => {
                    buf[0] = 0xE0 | (cp >> 12) as u8;
                    buf[1] = 0x80 | ((cp >> 6) & 0x3F) as u8;
                    buf[2] = 0x80 | (cp & 0x3F) as u8;
                    3
                }
                _ => {
                    buf[0] = 0xF0 | (cp >> 18) as u8;
                    buf[1] = 0x80 | ((cp >> 12) & 0x3F) as u8;
                    buf[2] = 0x80 | ((cp >> 6) & 0x3F) as u8;
                    buf[3] = 0x80 | (cp & 0x3F) as u8;
                    4
                }
            };
            if out.len() < n {
                return Err(EncodeErr::NoRoom);
            }
            out[..n].copy_from_slice(&buf[..n]);
            Ok(n)
        }
        Enc::Latin1 => put_byte(u8::try_from(cp).ok(), out),
        Enc::Latin15 => put_byte(unicode_to_latin15(cp), out),
        Enc::MacRoman => put_byte(unicode_to_mac_roman(cp), out),
    }
}

fn put_byte(b: Option<u8>, out: &mut [u8]) -> Result<usize, EncodeErr> {
    let b = b.ok_or(EncodeErr::Unmappable)?;
    if out.is_empty() {
        return Err(EncodeErr::NoRoom);
    }
    out[0] = b;
    Ok(1)
}

/// The C's error-recovery skip: count the leading one bits of the offending
/// byte (0xFE/0xFF skip one), capped by the remaining input. The C could
/// compute zero for a byte below 0x80 and then loop forever; a minimum of
/// one is the only divergence.
fn error_skip(input: &[u8]) -> usize {
    let c = input[0];
    let mut skip = 0usize;
    if c == 0xFE || c == 0xFF {
        skip = 1;
    } else {
        let mut mask = 0x80u8;
        while skip < input.len() && (mask & c) != 0 {
            skip += 1;
            mask >>= 1;
        }
    }
    skip.max(1)
}

/// The iconv-loop equivalent: converts `from` into `to` (whose usable room
/// is `to.len()`; the terminator byte, when requested, is *outside* that
/// room, exactly as the C reserved `toLen - term`).
///
/// Answers the number of bytes written.
pub(crate) fn convert_between(from: &[u8], from_enc: Enc, to: &mut [u8], to_enc: Enc) -> usize {
    let mut in_pos = 0usize;
    let mut out_pos = 0usize;
    while in_pos < from.len() {
        match decode_one(from_enc, &from[in_pos..]) {
            Ok((cp, consumed)) => match encode_one(to_enc, cp, &mut to[out_pos..]) {
                Ok(n) => {
                    out_pos += n;
                    in_pos += consumed;
                }
                Err(EncodeErr::Unmappable) => {
                    // Skip by the *input* byte's pattern and substitute '?',
                    // as the C's EILSEQ handler did.
                    in_pos += error_skip(&from[in_pos..]);
                    if out_pos < to.len() {
                        to[out_pos] = b'?';
                        out_pos += 1;
                    }
                }
                Err(EncodeErr::NoRoom) => break, // E2BIG: stop consuming
            },
            Err(DecodeErr::Bad) => {
                in_pos += error_skip(&from[in_pos..]);
                if out_pos < to.len() {
                    to[out_pos] = b'?';
                    out_pos += 1;
                }
            }
        }
    }
    out_pos
}

/// The C's `convertCopy` fallback: a bounded `strncpy` (stops at a NUL and
/// zero-fills, exactly as strncpy does), used when an encoding is unknown.
pub(crate) fn convert_copy(from: &[u8], to: &mut [u8], to_len: usize, term: bool) -> usize {
    let room = to_len.saturating_sub(usize::from(term)).min(to.len());
    let len = room.min(from.len());
    let nul = from[..len].iter().position(|&b| b == 0);
    let copy = nul.unwrap_or(len);
    to[..copy].copy_from_slice(&from[..copy]);
    for b in &mut to[copy..len] {
        *b = 0;
    }
    if term && len < to.len() {
        to[len] = 0;
    }
    len
}

/// `convertChars` over slices: picks native conversion when both encodings
/// are known, the copying fallback otherwise. `to` must have `to_len` bytes
/// of capacity (`to_len - term` usable).
fn convert_chars_slices(
    from: &[u8],
    from_enc: Option<Enc>,
    to: &mut [u8],
    to_enc: Option<Enc>,
    to_len: usize,
    term: bool,
) -> usize {
    match (from_enc, to_enc) {
        (Some(f), Some(t)) => {
            let room = to_len.saturating_sub(usize::from(term)).min(to.len());
            let n = {
                let (usable, _) = to.split_at_mut(room);
                convert_between(from, f, usable, t)
            };
            if term && n < to.len() {
                to[n] = 0;
            }
            n
        }
        // The C warned via iconvFail once, then copied; there is no logging
        // channel here, so just copy.
        _ => convert_copy(from, to, to_len, term),
    }
}

/// CR -> LF over the converted output (`sq2uxLines`).
fn sq2ux_lines(buf: &mut [u8]) {
    for b in buf {
        if *b == 0x0D {
            *b = 0x0A;
        }
    }
}

/// LF -> CR over the converted output (`ux2sqLines`).
fn ux2sq_lines(buf: &mut [u8]) {
    for b in buf {
        if *b == 0x0A {
            *b = 0x0D;
        }
    }
}

// ---------------------------------------------------------------------------
// Rust-facing helpers for the rest of the crate
// ---------------------------------------------------------------------------

/// `sq2uxPath` over slices, using the live global encodings.
pub(crate) fn sq2ux_path(from: &[u8], to: &mut [u8], to_len: usize, term: bool) -> sqInt {
    // SAFETY: reading the globals, which only ever hold null or a
    // NUL-terminated name; mutation happens only on the interpreter thread.
    let (f, t) = unsafe {
        (
            parse_encoding(*core::ptr::addr_of!(sqPathEncoding)),
            parse_encoding(*core::ptr::addr_of!(uxPathEncoding)),
        )
    };
    convert_chars_slices(from, f, to, t, to_len, term) as sqInt
}

/// `ux2sqPath` over slices, using the live global encodings.
pub(crate) fn ux2sq_path(from: &[u8], to: &mut [u8], to_len: usize, term: bool) -> sqInt {
    // SAFETY: as in `sq2ux_path`.
    let (f, t) = unsafe {
        (
            parse_encoding(*core::ptr::addr_of!(uxPathEncoding)),
            parse_encoding(*core::ptr::addr_of!(sqPathEncoding)),
        )
    };
    convert_chars_slices(from, f, to, t, to_len, term) as sqInt
}

// ---------------------------------------------------------------------------
// Exported C API (sqUnixCharConv.h)
// ---------------------------------------------------------------------------

/// # Safety
/// `ptr` points at `len` readable bytes when `len > 0`.
unsafe fn in_slice<'a>(ptr: *mut c_char, len: c_int) -> &'a [u8] {
    if ptr.is_null() || len <= 0 {
        return &[];
    }
    // SAFETY: caller contract.
    unsafe { core::slice::from_raw_parts(ptr.cast::<u8>(), len as usize) }
}

/// # Safety
/// `ptr` points at `len` writable bytes when `len > 0`.
unsafe fn out_slice<'a>(ptr: *mut c_char, len: c_int) -> &'a mut [u8] {
    if ptr.is_null() || len <= 0 {
        return &mut [];
    }
    // SAFETY: caller contract; the caller guarantees no aliasing with the
    // input, as the C interface always did.
    unsafe { core::slice::from_raw_parts_mut(ptr.cast::<u8>(), len as usize) }
}

/// The general conversion entry point, `convertChars` in the C. `norm` is
/// the macOS HFS+ normalisation flag and is ignored on this branch, as the
/// iconv-based C ignored it.
///
/// # Safety
/// `from` holds `from_len` readable bytes; `to` holds `to_len` writable
/// bytes; the code handles are null or NUL-terminated names.
#[no_mangle]
pub unsafe extern "C" fn convertChars(
    from: *mut c_char,
    from_len: c_int,
    from_code: *mut c_void,
    to: *mut c_char,
    to_len: c_int,
    to_code: *mut c_void,
    _norm: c_int,
    term: c_int,
) -> c_int {
    // SAFETY: caller contract.
    let (from, to_buf, f, t) = unsafe {
        (
            in_slice(from, from_len),
            out_slice(to, to_len),
            parse_encoding(from_code),
            parse_encoding(to_code),
        )
    };
    convert_chars_slices(from, f, to_buf, t, to_len.max(0) as usize, term != 0) as c_int
}

macro_rules! converter {
    ($name:ident, $from:ident, $to:ident, $lines:expr) => {
        /// One of the seven fixed converters the C generated with its
        /// `Convert` macro; see the module docs.
        ///
        /// # Safety
        /// As [`convertChars`].
        #[no_mangle]
        pub unsafe extern "C" fn $name(
            from: *mut c_char,
            from_len: c_int,
            to: *mut c_char,
            to_len: c_int,
            term: c_int,
        ) -> c_int {
            // SAFETY: caller contract; globals hold null or valid names.
            let (from, to_buf, f, t) = unsafe {
                (
                    in_slice(from, from_len),
                    out_slice(to, to_len),
                    parse_encoding(*core::ptr::addr_of!($from)),
                    parse_encoding(*core::ptr::addr_of!($to)),
                )
            };
            let n = convert_chars_slices(from, f, to_buf, t, to_len.max(0) as usize, term != 0);
            let lines: Option<fn(&mut [u8])> = $lines;
            if let Some(fix) = lines {
                fix(&mut to_buf[..n]);
            }
            n as c_int
        }
    };
}

converter!(sq2uxText, sqTextEncoding, uxTextEncoding, Some(sq2ux_lines));
converter!(ux2sqText, uxTextEncoding, sqTextEncoding, Some(ux2sq_lines));
// Composed paths for non-Mac unix: no normalisation, no line conversion.
converter!(sq2uxPath, sqPathEncoding, uxPathEncoding, None);
converter!(ux2sqPath, uxPathEncoding, sqPathEncoding, None);
converter!(sq2uxUTF8, sqTextEncoding, uxUTF8Encoding, Some(sq2ux_lines));
converter!(ux2sqUTF8, uxUTF8Encoding, sqTextEncoding, Some(ux2sq_lines));
converter!(ux2sqXWin, uxXWinEncoding, sqTextEncoding, Some(ux2sq_lines));

/// Frees an encoding handle, unless it is one of the predefined names.
///
/// # Safety
/// `encoding` is null, predefined, or a pointer this module malloc'd.
#[no_mangle]
pub unsafe extern "C" fn freeEncoding(encoding: *mut c_void) {
    for p in predefined() {
        if core::ptr::eq(encoding.cast::<u8>().cast_const(), p) {
            return;
        }
    }
    // SAFETY: caller contract -- anything else stored in the globals came
    // from libc::malloc in setNEncoding. The C freed null too (free(NULL)
    // is defined); libc::free accepts null likewise.
    unsafe { libc::free(encoding) }
}

/// Sets `*encoding` from the first `n` bytes of `raw_name`, uppercased,
/// resolving aliases and interning the predefined names.
///
/// # Safety
/// `encoding` points at a writable handle; `raw_name` holds `n` readable
/// bytes.
#[no_mangle]
pub unsafe extern "C" fn setNEncoding(encoding: *mut *mut c_void, raw_name: *mut c_char, n: c_int) {
    let n = n.max(0) as usize;
    // SAFETY: `malloc(n + 1)` bytes are writable; caller contract for
    // raw_name.
    unsafe {
        let name = libc::malloc(n + 1).cast::<u8>();
        if name.is_null() {
            return; // the C would have crashed; declining is the safe analogue
        }
        for i in 0..n {
            *name.add(i) = (*raw_name.add(i) as u8).to_ascii_uppercase();
        }
        *name.add(n) = 0;

        let locale = *core::ptr::addr_of!(localeEncoding);
        if !(*encoding).is_null() && *encoding != locale {
            freeEncoding(*encoding);
        }
        if !locale.is_null() && libc::strcmp(name.cast(), locale.cast()) == 0 {
            *encoding = locale;
            libc::free(name.cast());
            return;
        }
        for p in predefined() {
            if libc::strcmp(name.cast(), p.cast()) == 0 {
                *encoding = p.cast_mut().cast();
                libc::free(name.cast());
                return;
            }
        }
        // Aliases, as the C's table.
        for (alias, target) in [
            (&b"UTF8\0"[..], UTF8_NAME),
            (&b"MACROMAN\0"[..], MACINTOSH),
            (&b"MAC-ROMAN\0"[..], MACINTOSH),
        ] {
            if libc::strcmp(name.cast(), alias.as_ptr().cast()) == 0 {
                *encoding = target.as_ptr().cast_mut().cast();
                libc::free(name.cast());
                return;
            }
        }
        *encoding = name.cast();
    }
}

/// `setEncoding`: as [`setNEncoding`] with `strlen(name)`.
///
/// # Safety
/// `encoding` as in [`setNEncoding`]; `raw_name` is NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn setEncoding(encoding: *mut *mut c_void, raw_name: *mut c_char) {
    // SAFETY: caller contract.
    unsafe { setNEncoding(encoding, raw_name, libc::strlen(raw_name) as c_int) }
}

/// Adopts the encoding suffix of a locale name (`en_US.UTF-8@mod` ->
/// `UTF-8`) as the locale encoding, and points the Squeak-text, unix-text,
/// unix-path and X11 encodings at it -- exactly the set the C reassigned.
///
/// # Safety
/// `locale` is NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn setLocaleEncoding(locale: *mut c_char) {
    if locale.is_null() {
        return;
    }
    // SAFETY: caller contract, throughout.
    unsafe {
        let mut p = locale;
        while *p != 0 {
            let c = *p;
            p = p.add(1);
            if c == b'.' as c_char {
                let mut len = 0;
                while *p.add(len) != 0 && *p.add(len) != b'@' as c_char {
                    len += 1;
                }
                setNEncoding(core::ptr::addr_of_mut!(localeEncoding), p, len as c_int);
                let enc = *core::ptr::addr_of!(localeEncoding);
                *core::ptr::addr_of_mut!(sqTextEncoding) = enc;
                *core::ptr::addr_of_mut!(uxTextEncoding) = enc;
                *core::ptr::addr_of_mut!(uxPathEncoding) = enc;
                *core::ptr::addr_of_mut!(uxXWinEncoding) = enc;
                return;
            }
        }
    }
}

/// Copies a Squeak path (whose bytes live at `sq_name_index`) into `ux_name`
/// as a NUL-terminated unix path.
///
/// The C's own comment: lots of image-generated code assumes 1000 chars max
/// path length, hence the hard-coded limit.
///
/// # Safety
/// `ux_name` holds 1000 writable bytes; `sq_name_index` is the address of
/// `sq_name_length` readable bytes (the VM passes `pointerForOop`, an
/// identity on unix).
#[no_mangle]
pub unsafe extern "C" fn sqFilenameFromString(
    ux_name: *mut c_char,
    sq_name_index: sqInt,
    sq_name_length: c_int,
) {
    // SAFETY: caller contract.
    unsafe {
        sq2uxPath(
            sq_name_index as *mut c_char,
            sq_name_length,
            ux_name,
            1000,
            1,
        );
    }
}
