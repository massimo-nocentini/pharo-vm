//! Locale-string parsing, ported byte for byte from `sqUnixLocale.c`.
//!
//! The C file compiles with `CODELEN == 2` (ISO two-letter codes), so the
//! UN three-letter tables it also carries are dead code and are not ported.
//!
//! Everything here works on raw bytes, because the C worked on `char *`
//! straight out of `setlocale`/`getenv`, which owes nobody valid UTF-8.

/// `DEFAULT_LOCALE` in the C: used only when no locale can be determined at
/// all, or when the candidate is degenerate ("C", "POSIX", malformed).
pub const DEFAULT_LOCALE: &[u8] = b"en_US.ISO8859-1";

/// `DEFAULT_COUNTRY` for `CODELEN == 2`.
pub const DEFAULT_COUNTRY: &[u8] = b"US";

/// `DEFAULT_LANGUAGE` for `CODELEN == 2`.
pub const DEFAULT_LANGUAGE: &[u8] = b"en";

/// `getlocale()`: pick the first of `LC_ALL`, `LANG`, the current OS locale;
/// sanitise "C", "POSIX" and anything with a space or slash to the default.
///
/// Faithful oddity: an *empty* candidate ("LC_ALL=" in the environment) is
/// not sanitised — the C's `getenv` answers a non-null empty string, which
/// passes every check and is returned as-is.
pub fn choose_locale<'a>(
    lc_all: Option<&'a [u8]>,
    lang: Option<&'a [u8]>,
    current: Option<&'a [u8]>,
) -> &'a [u8] {
    let Some(locale) = lc_all.or(lang).or(current) else {
        return DEFAULT_LOCALE;
    };
    if locale == b"C" || locale == b"POSIX" || locale.contains(&b' ') || locale.contains(&b'/') {
        DEFAULT_LOCALE
    } else {
        locale
    }
}

/// `getCountry()`: the two bytes between the *last* `_` and the first `.`
/// after it (or the end of the string); `DEFAULT_COUNTRY` otherwise.
///
/// As in the C: exactly two bytes or nothing (so `de_DE@euro` falls back to
/// the default, because `DE@euro` is not two bytes), no alphabetic check,
/// and case is preserved (`en_us.utf8` answers `us`).
pub fn country_of(locale: &[u8]) -> &[u8] {
    if let Some(pos) = locale.iter().rposition(|&b| b == b'_') {
        let after = &locale[pos + 1..];
        let end = after
            .iter()
            .position(|&b| b == b'.')
            .unwrap_or(after.len());
        let code = &after[..end];
        if code.len() == 2 {
            return code;
        }
    }
    DEFAULT_COUNTRY
}

/// `getLanguage()`: the first two bytes when both are letters and the third
/// is `.`, `_` or the end of the string; `DEFAULT_LANGUAGE` otherwise.
///
/// The C tests the bytes with `isalpha`, which for the ASCII-only locale
/// names the OS produces is exactly `is_ascii_alphabetic`. (Passing a byte
/// above 0x7F to `isalpha` through a signed `char` is undefined behaviour in
/// the C; here it is simply "not a letter".)
pub fn language_of(locale: &[u8]) -> &[u8] {
    if locale.len() >= 2
        && locale[0].is_ascii_alphabetic()
        && locale[1].is_ascii_alphabetic()
        && matches!(locale.get(2), None | Some(b'.') | Some(b'_'))
    {
        &locale[..2]
    } else {
        DEFAULT_LANGUAGE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- choose_locale (C getlocale) ----------------------------------

    #[test]
    fn choose_prefers_lc_all_over_lang_over_current() {
        let (a, b, c): (&[u8], &[u8], &[u8]) = (b"aa_AA", b"bb_BB", b"cc_CC");
        assert_eq!(choose_locale(Some(a), Some(b), Some(c)), a);
        assert_eq!(choose_locale(None, Some(b), Some(c)), b);
        assert_eq!(choose_locale(None, None, Some(c)), c);
    }

    #[test]
    fn choose_defaults_when_nothing_is_set() {
        assert_eq!(choose_locale(None, None, None), DEFAULT_LOCALE);
    }

    #[test]
    fn choose_sanitises_c_and_posix() {
        assert_eq!(choose_locale(Some(b"C"), None, None), DEFAULT_LOCALE);
        assert_eq!(choose_locale(Some(b"POSIX"), None, None), DEFAULT_LOCALE);
        // ...but only exact matches: "C.UTF-8" passes through.
        assert_eq!(choose_locale(Some(b"C.UTF-8"), None, None), b"C.UTF-8");
    }

    #[test]
    fn choose_sanitises_spaces_and_slashes() {
        assert_eq!(choose_locale(Some(b"en US"), None, None), DEFAULT_LOCALE);
        assert_eq!(choose_locale(Some(b"en/US"), None, None), DEFAULT_LOCALE);
    }

    #[test]
    fn choose_passes_empty_string_through() {
        // Faithful to the C: getenv("LC_ALL") == "" is non-null and survives
        // every sanity check.
        assert_eq!(choose_locale(Some(b""), None, None), b"");
    }

    #[test]
    fn choose_ordinary_locale_passes_through() {
        assert_eq!(
            choose_locale(Some(b"en_US.UTF-8"), None, None),
            b"en_US.UTF-8"
        );
    }

    // ---- country_of (C getCountry) ------------------------------------

    #[test]
    fn country_from_the_usual_shapes() {
        assert_eq!(country_of(b"en_US.UTF-8"), b"US"); // ll_CC.PP
        assert_eq!(country_of(b"en_US"), b"US"); // ll_CC
        assert_eq!(country_of(b"_US.UTF-8"), b"US"); // _CC.PP
        assert_eq!(country_of(b"_US"), b"US"); // _CC
    }

    #[test]
    fn country_defaults_without_an_underscore() {
        assert_eq!(country_of(b"C"), DEFAULT_COUNTRY);
        assert_eq!(country_of(b"POSIX"), DEFAULT_COUNTRY);
        assert_eq!(country_of(b""), DEFAULT_COUNTRY);
        assert_eq!(country_of(b"en"), DEFAULT_COUNTRY);
        assert_eq!(country_of(b"en.UTF-8"), DEFAULT_COUNTRY);
    }

    #[test]
    fn country_requires_exactly_two_bytes() {
        assert_eq!(country_of(b"en_USA"), DEFAULT_COUNTRY); // three
        assert_eq!(country_of(b"en_U"), DEFAULT_COUNTRY); // one
        assert_eq!(country_of(b"en_"), DEFAULT_COUNTRY); // zero
        // An @modifier without a dot makes the segment too long -- the C
        // falls back here, and so do we.
        assert_eq!(country_of(b"de_DE@euro"), DEFAULT_COUNTRY);
        // With the dot, the modifier sits after the terminator and the code
        // is found.
        assert_eq!(country_of(b"de_DE.UTF-8@euro"), b"DE");
    }

    #[test]
    fn country_takes_the_last_underscore() {
        // strrchr: the segment after the *last* underscore is what counts.
        assert_eq!(country_of(b"aa_bb_CC.utf8"), b"CC");
    }

    #[test]
    fn country_preserves_case_and_skips_no_checks() {
        assert_eq!(country_of(b"en_us.utf8"), b"us");
        assert_eq!(country_of(b"en_12"), b"12"); // no isalpha in the C either
    }

    // ---- language_of (C getLanguage) ----------------------------------

    #[test]
    fn language_from_the_usual_shapes() {
        assert_eq!(language_of(b"en_US.UTF-8"), b"en"); // ll_CC.PP
        assert_eq!(language_of(b"en_US"), b"en"); // ll_CC
        assert_eq!(language_of(b"en.UTF-8"), b"en"); // ll.PP
        assert_eq!(language_of(b"en"), b"en"); // ll
    }

    #[test]
    fn language_defaults_on_degenerate_strings() {
        assert_eq!(language_of(b"C"), DEFAULT_LANGUAGE);
        assert_eq!(language_of(b""), DEFAULT_LANGUAGE);
        assert_eq!(language_of(b"e"), DEFAULT_LANGUAGE);
        // Third byte must be '.', '_' or the end: three-letter codes and
        // "POSIX" both fail the shape test.
        assert_eq!(language_of(b"deu"), DEFAULT_LANGUAGE);
        assert_eq!(language_of(b"POSIX"), DEFAULT_LANGUAGE);
    }

    #[test]
    fn language_requires_two_letters() {
        assert_eq!(language_of(b"1x_US"), DEFAULT_LANGUAGE);
        assert_eq!(language_of(b"e1_US"), DEFAULT_LANGUAGE);
        assert_eq!(language_of(b"__US"), DEFAULT_LANGUAGE);
    }

    #[test]
    fn language_preserves_case() {
        // The C copies the bytes untouched, so "EN_US" answers "EN".
        assert_eq!(language_of(b"EN_US"), b"EN");
    }

    #[test]
    fn glibc_composite_locale_strings() {
        // With mixed per-category settings, glibc's setlocale(LC_ALL, "")
        // answers "LC_CTYPE=en_US.UTF-8;LC_NUMERIC=de_DE.UTF-8;...". The C
        // parsed that string as-is; make sure we agree with what it did.
        let composite: &[u8] = b"LC_CTYPE=en_US.UTF-8;LC_NUMERIC=de_DE.UTF-8";
        // Last '_' is in "de_DE.UTF-8" -> "DE".
        assert_eq!(country_of(composite), b"DE");
        // "LC" then '_': two letters with '_' third -> "LC", as in the C.
        assert_eq!(language_of(composite), b"LC");
    }
}
