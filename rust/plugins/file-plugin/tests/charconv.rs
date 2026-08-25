//! Known-answer tests for the character conversion layer, exercised through
//! the exported C entry points -- the same surface `sqUnixFile.c` and
//! FileAttributesPlugin consume.

use libc::{c_char, c_int, c_void};

fn conv(
    f: unsafe extern "C" fn(*mut c_char, c_int, *mut c_char, c_int, c_int) -> c_int,
    from: &[u8],
    to_len: usize,
    term: bool,
) -> (Vec<u8>, i32) {
    let mut input = from.to_vec();
    let mut out = vec![0xEEu8; to_len.max(1)]; // poison to catch overwrites
    let n = unsafe {
        f(
            input.as_mut_ptr().cast(),
            from.len() as c_int,
            out.as_mut_ptr().cast(),
            to_len as c_int,
            c_int::from(term),
        )
    };
    (out, n)
}

// ---------------------------------------------------------------------------
// Path conversion: UTF-8 -> UTF-8 by default, so mostly identity with
// iconv's invalid-sequence policy.
// ---------------------------------------------------------------------------

#[test]
fn path_conversion_is_identity_for_valid_utf8() {
    let cases: &[&[u8]] = &[
        b"hello.txt",
        "caf\u{e9}/r\u{e9}sum\u{e9}.pdf".as_bytes(),
        "\u{1F600}emoji dir\u{1F600}/x".as_bytes(),
        b"",
    ];
    for &case in cases {
        let (out, n) = conv(FilePlugin::sq2uxPath, case, 4096, false);
        assert_eq!(&out[..n as usize], case);
        let (back, m) = conv(FilePlugin::ux2sqPath, case, 4096, false);
        assert_eq!(&back[..m as usize], case);
    }
}

#[test]
fn path_conversion_replaces_invalid_utf8_like_the_c() {
    // 0xC3 opens a two-byte sequence; 0x28 is not a continuation, so the C's
    // recovery skips the leading-ones count (2) and substitutes '?'.
    let (out, n) = conv(FilePlugin::sq2uxPath, b"a\xC3\x28b", 64, false);
    assert_eq!(&out[..n as usize], b"a?b");

    // A stray continuation byte skips one and substitutes.
    let (out, n) = conv(FilePlugin::sq2uxPath, b"a\x80b", 64, false);
    assert_eq!(&out[..n as usize], b"a?b");

    // 0xFF skips exactly one.
    let (out, n) = conv(FilePlugin::sq2uxPath, b"\xFFxy", 64, false);
    assert_eq!(&out[..n as usize], b"?xy");

    // Truncated multibyte at end of input (iconv's EINVAL case).
    let (out, n) = conv(FilePlugin::sq2uxPath, b"ab\xE2\x82", 64, false);
    assert_eq!(&out[..n as usize], b"ab?");
}

#[test]
fn terminator_reserves_one_byte_and_nul_terminates() {
    // Room for 3 payload bytes + NUL.
    let (out, n) = conv(FilePlugin::sq2uxPath, b"abcdef", 4, true);
    assert_eq!(n, 3);
    assert_eq!(&out[..4], b"abc\0");
}

#[test]
fn full_output_buffer_stops_conversion() {
    // E2BIG: a 4-byte UTF-8 character does not fit the last free byte; the
    // conversion stops there, as iconv's E2BIG handling did.
    let (out, n) = conv(FilePlugin::sq2uxPath, "ab\u{1F600}".as_bytes(), 3, false);
    assert_eq!(n, 2);
    assert_eq!(&out[..2], b"ab");
}

// ---------------------------------------------------------------------------
// Text conversion: MacRoman (image side) <-> ISO-8859-15 (unix side) by
// default, with line-end mapping.
// ---------------------------------------------------------------------------

#[test]
fn text_conversion_maps_mac_roman_to_latin15() {
    // MacRoman 0x8E is e-acute (U+00E9), which is 0xE9 in Latin-15.
    let (out, n) = conv(FilePlugin::sq2uxText, b"caf\x8E", 64, true);
    assert_eq!(&out[..n as usize], b"caf\xE9");
    // And back.
    let (out, n) = conv(FilePlugin::ux2sqText, b"caf\xE9", 64, true);
    assert_eq!(&out[..n as usize], b"caf\x8E");
}

#[test]
fn text_conversion_converts_line_ends() {
    // sq2ux: CR -> LF, applied after the conversion.
    let (out, n) = conv(FilePlugin::sq2uxText, b"a\x0Db", 64, true);
    assert_eq!(&out[..n as usize], b"a\x0Ab");
    // ux2sq: LF -> CR.
    let (out, n) = conv(FilePlugin::ux2sqText, b"a\x0Ab", 64, true);
    assert_eq!(&out[..n as usize], b"a\x0Db");
}

#[test]
fn utf8_text_conversion_uses_mac_roman_on_the_image_side() {
    // MacRoman 0xA5 is the bullet, U+2022.
    let (out, n) = conv(FilePlugin::sq2uxUTF8, b"a\xA5b", 64, true);
    assert_eq!(&out[..n as usize], "a\u{2022}b".as_bytes());
    let (out, n) = conv(FilePlugin::ux2sqUTF8, "a\u{2022}b".as_bytes(), 64, true);
    assert_eq!(&out[..n as usize], b"a\xA5b");
}

#[test]
fn mac_roman_euro_and_latin15_delta_roundtrip() {
    // Euro: MacRoman 0xDB <-> Latin-15 0xA4 (one of the eight positions
    // where Latin-15 departs from Latin-1).
    let (out, n) = conv(FilePlugin::sq2uxText, b"\xDB", 64, false);
    assert_eq!(&out[..n as usize], b"\xA4");
    let (out, n) = conv(FilePlugin::ux2sqText, b"\xA4", 64, false);
    assert_eq!(&out[..n as usize], b"\xDB");
}

#[test]
fn unmappable_characters_become_question_marks() {
    // MacRoman 0xB0 is U+221E (infinity), which Latin-15 lacks.
    let (out, n) = conv(FilePlugin::sq2uxText, b"x\xB0y", 64, false);
    assert_eq!(&out[..n as usize], b"x?y");
}

#[test]
fn xwin_conversion_is_latin1_to_mac_roman() {
    // Latin-1 0xE9 (e-acute) -> MacRoman 0x8E.
    let (out, n) = conv(FilePlugin::ux2sqXWin, b"caf\xE9", 64, true);
    assert_eq!(&out[..n as usize], b"caf\x8E");
}

// ---------------------------------------------------------------------------
// convertChars and the copying fallback
// ---------------------------------------------------------------------------

#[test]
fn convert_chars_with_unknown_encoding_copies() {
    let mut from = b"payload".to_vec();
    let mut to = vec![0u8; 16];
    let mut bogus = b"KOI8-R\0".to_vec(); // not one of the four known names
    let utf8 = b"UTF-8\0";
    let n = unsafe {
        FilePlugin::convertChars(
            from.as_mut_ptr().cast(),
            from.len() as c_int,
            bogus.as_mut_ptr().cast::<c_void>(),
            to.as_mut_ptr().cast(),
            to.len() as c_int,
            utf8.as_ptr() as *mut c_void,
            0,
            1,
        )
    };
    assert_eq!(n, 7);
    assert_eq!(&to[..8], b"payload\0");
}

#[test]
fn convert_chars_converts_between_named_encodings() {
    let mut from = b"caf\xE9".to_vec(); // Latin-1
    let mut to = vec![0u8; 16];
    let latin1 = b"ISO-8859-1\0";
    let utf8 = b"UTF-8\0";
    let n = unsafe {
        FilePlugin::convertChars(
            from.as_mut_ptr().cast(),
            from.len() as c_int,
            latin1.as_ptr() as *mut c_void,
            to.as_mut_ptr().cast(),
            to.len() as c_int,
            utf8.as_ptr() as *mut c_void,
            0,
            0,
        )
    };
    assert_eq!(&to[..n as usize], "caf\u{e9}".as_bytes());
}

#[test]
fn sq_filename_from_string_nul_terminates() {
    let name = b"some/dir/file.txt";
    let mut out = vec![0xEEu8; 1000];
    unsafe {
        FilePlugin::sqFilenameFromString(
            out.as_mut_ptr().cast(),
            name.as_ptr() as isize,
            name.len() as c_int,
        );
    }
    assert_eq!(&out[..name.len()], name);
    assert_eq!(out[name.len()], 0);
}
