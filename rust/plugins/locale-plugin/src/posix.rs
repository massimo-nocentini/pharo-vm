//! The POSIX support layer: what `sqUnixLocale.c` asked the C library.
//!
//! Deliberately thin. The point of this plugin is to answer what the C
//! answered *on the same system*, so every value comes from the platform's
//! `setlocale`/`localeconv`/`nl_langinfo`/`localtime`, not from locale data
//! reimplemented in Rust.
//!
//! # Thread-unsafety, exactly as the C had it
//!
//! `setlocale`, `localeconv` and `nl_langinfo` hand out pointers into libc's
//! per-process locale state and are not thread-safe. The C plugin lived with
//! that because primitives only ever run on the interpreter thread; the same
//! holds here, and every string is copied out of libc's buffers before the
//! call returns, so no libc pointer is retained across calls (the C *did*
//! retain `setlocale`'s and `localeconv`'s pointers — see the README).

use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::ptr;
use std::sync::OnceLock;

use crate::parse;

/// The locale string captured by `sqLocInitialize()`, owned.
///
/// The C kept the raw pointer `setlocale` returned; we snapshot the bytes,
/// which is the same value with no dangling risk if anything in the process
/// calls `setlocale` again later.
static LOCALE: OnceLock<Vec<u8>> = OnceLock::new();

/// `sqLocInitialize()`: adopt the environment's locale and remember its name.
///
/// Idempotent. The C reads `localeString` lazily in `getCountry`/`getLanguage`
/// and would dereference NULL had initialisation been skipped; initialising on
/// first use instead is this port's only defence there (the VM does call
/// `initialiseModule` before any primitive, so the path is theoretical).
pub fn locale_string() -> &'static [u8] {
    LOCALE.get_or_init(install_locale)
}

fn install_locale() -> Vec<u8> {
    // setlocale(LC_ALL, "") installs the environment's locale and answers its
    // name; that answer is the string the C parsed, composite forms included.
    // SAFETY: called on the interpreter thread only; the result is copied out
    // before any further libc call can invalidate it.
    unsafe {
        let installed = libc::setlocale(libc::LC_ALL, c"".as_ptr());
        if !installed.is_null() {
            return CStr::from_ptr(installed).to_bytes().to_vec();
        }
    }

    // The environment names a locale the system does not have. Fall back the
    // way the C's getlocale() did, then try to install the choice, ignoring
    // whether that took -- exactly as the C ignored it.
    let lc_all = env_bytes("LC_ALL");
    let lang = env_bytes("LANG");
    // SAFETY: setlocale(_, NULL) is a pure query; copied out immediately.
    let current = unsafe {
        let p = libc::setlocale(libc::LC_ALL, ptr::null());
        if p.is_null() {
            None
        } else {
            Some(CStr::from_ptr(p).to_bytes().to_vec())
        }
    };
    let chosen =
        parse::choose_locale(lc_all.as_deref(), lang.as_deref(), current.as_deref()).to_vec();
    if let Ok(name) = CString::new(chosen.clone()) {
        // SAFETY: interpreter thread only; `name` outlives the call.
        unsafe { libc::setlocale(libc::LC_ALL, name.as_ptr()) };
    }
    chosen
}

/// An environment variable's raw bytes, as C's `getenv` saw them.
fn env_bytes(name: &str) -> Option<Vec<u8>> {
    std::env::var_os(name).map(|v| v.as_os_str().as_bytes().to_vec())
}

/// `getCountry()`: the two-letter country code parsed from the locale string.
pub fn country() -> &'static [u8] {
    parse::country_of(locale_string())
}

/// `getLanguage()`: the two-letter language code parsed from the locale string.
pub fn language() -> &'static [u8] {
    parse::language_of(locale_string())
}

// ---- localeconv-backed answers --------------------------------------------

/// A string field of the process `lconv`, copied out immediately.
///
/// The C cached the `localeconv()` pointer at initialisation and read through
/// it on every primitive; libc answers the same static record each time, and
/// nothing calls `setlocale` after initialisation, so re-querying here is the
/// same data without holding a raw pointer in a static.
fn lconv_bytes(field: impl FnOnce(&libc::lconv) -> *mut libc::c_char) -> Vec<u8> {
    // SAFETY: localeconv answers a pointer to libc's static lconv, valid until
    // the next localeconv/setlocale call -- none of which happens before the
    // copy below completes. Interpreter thread only.
    unsafe {
        let lc = libc::localeconv();
        if lc.is_null() {
            // POSIX never answers NULL; the empty string is what an
            // unavailable field would have produced anyway.
            return Vec::new();
        }
        let p = field(&*lc);
        if p.is_null() {
            return Vec::new();
        }
        CStr::from_ptr(p).to_bytes().to_vec()
    }
}

/// `sqLocCurrencyNotation()`: does the currency symbol precede the amount?
///
/// The C answers `p_cs_precedes` as a truth value. POSIX uses `CHAR_MAX` for
/// "unavailable" (the C/POSIX locale does exactly that), and `CHAR_MAX` is
/// non-zero, so "unavailable" reads as *true* -- faithfully preserved.
pub fn currency_notation() -> bool {
    // SAFETY: as in lconv_bytes; a plain byte field, read then discarded.
    unsafe {
        let lc = libc::localeconv();
        !lc.is_null() && (*lc).p_cs_precedes != 0
    }
}

/// `localeConv->currency_symbol`.
pub fn currency_symbol() -> Vec<u8> {
    lconv_bytes(|lc| lc.currency_symbol)
}

/// `localeConv->decimal_point`.
pub fn decimal_point() -> Vec<u8> {
    lconv_bytes(|lc| lc.decimal_point)
}

/// `localeConv->thousands_sep`.
pub fn thousands_sep() -> Vec<u8> {
    lconv_bytes(|lc| lc.thousands_sep)
}

// ---- nl_langinfo-backed answers --------------------------------------------

/// `nl_langinfo(D_FMT)`: the date format. The C answers this for *both* the
/// long and the short date format primitives.
pub fn date_format() -> Vec<u8> {
    langinfo(libc::D_FMT)
}

/// `nl_langinfo(T_FMT)`: the time format.
pub fn time_format() -> Vec<u8> {
    langinfo(libc::T_FMT)
}

fn langinfo(item: libc::nl_item) -> Vec<u8> {
    // SAFETY: nl_langinfo answers a pointer into libc's locale data, valid
    // until the next nl_langinfo/setlocale call; copied out immediately, and
    // only ever called from the interpreter thread.
    unsafe {
        let p = libc::nl_langinfo(item);
        if p.is_null() {
            Vec::new()
        } else {
            CStr::from_ptr(p).to_bytes().to_vec()
        }
    }
}

// ---- time-backed answers ---------------------------------------------------

/// Broken-down local time for "now", or None if libc declines (it does not,
/// in practice; the C dereferenced `localtime`'s result unchecked).
fn local_now() -> Option<libc::tm> {
    // SAFETY: localtime_r fills the caller's tm and touches no shared buffer;
    // the C used plain localtime, safe for it only because primitives run on
    // one thread. Zero-initialising tm is fine -- it is plain data, and
    // localtime_r overwrites every field it defines.
    unsafe {
        let now = libc::time(ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            None
        } else {
            Some(tm)
        }
    }
}

/// `sqLocGetTimezoneOffset()`: minutes east of UTC, via `tm_gmtoff` -- the
/// C's `HAVE_TM_GMTOFF` branch, which is the one the Unix build compiles.
/// The comment there says it all: "Match the behaviour of
/// convertToSqueakTime()."
pub fn timezone_offset_minutes() -> Option<isize> {
    local_now().map(|tm| (tm.tm_gmtoff / 60) as isize)
}

/// `sqLocDaylightSavings()`: is DST in effect right now?
pub fn daylight_savings() -> Option<bool> {
    local_now().map(|tm| tm.tm_isdst > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One test rather than several: these calls poke libc's shared locale
    /// state, and cargo runs test functions on parallel threads. Ordering
    /// them inside a single function keeps the layer single-threaded, as it
    /// is under the VM.
    #[test]
    fn posix_layer_answers_are_sane() {
        // Capturing the locale first mirrors initialiseModule's ordering.
        let locale = locale_string();
        assert_eq!(locale, locale_string(), "captured once, then stable");

        // Codes are exactly two bytes in every fallback path.
        assert_eq!(country().len(), 2);
        assert_eq!(language().len(), 2);

        // POSIX guarantees a non-empty decimal point in every locale.
        assert!(!decimal_point().is_empty());
        // thousands_sep and currency_symbol may legitimately be empty (they
        // are in the C/POSIX locale); just exercise the paths.
        let _ = thousands_sep();
        let _ = currency_symbol();
        let _ = currency_notation();

        // Date and time formats exist in every locale.
        assert!(!date_format().is_empty());
        assert!(!time_format().is_empty());

        // A UTC offset is at most a day in either direction (real zones stay
        // within [-12h, +14h]).
        let offset = timezone_offset_minutes().expect("localtime_r works");
        assert!((-24 * 60..=24 * 60).contains(&offset));
        assert!(daylight_savings().is_some());
    }
}
