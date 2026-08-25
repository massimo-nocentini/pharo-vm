//! `LocalePlugin`, in Rust.
//!
//! Replaces the Slang-generated `plugins/LocalePlugin/src/common/LocalePlugin.c`
//! together with the Unix support layer `src/unix/sqUnixLocale.c`, behind the
//! same fourteen exports. Windows and macOS keep their C support layers; this
//! crate ports the Unix one only.
//!
//! Every answer still comes from the platform's own C library — `setlocale`,
//! `localeconv`, `nl_langinfo`, `localtime` — through [`posix`], so on the
//! same system the image sees what the C plugin showed it. The one pure-Rust
//! part is the locale-*string* parsing in [`parse`], ported byte for byte and
//! unit-tested against the C's quirks.
//!
//! # Answer shapes the image relies on
//!
//! The generated C fixes some string sizes at allocation and copies fewer
//! bytes into them, leaving NUL tails:
//!
//! * country and language: a **3-byte** String holding a 2-letter code and a
//!   trailing NUL (`CODELEN == 2`; the UN 3-letter tables in the C are dead
//!   code);
//! * decimal and digit-grouping symbols: a **1-byte** String, whatever the
//!   locale's symbol length (see the README for the C's overflow here);
//! * currency symbol and the date/time formats: sized to the actual bytes.

// The crate is named for the shared library the VM loads (libLocalePlugin.so),
// which fixes its spelling.
#![allow(non_snake_case)]
#![deny(unsafe_op_in_unsafe_fn)]

// The port covers the *Unix* support layer: tm_gmtoff, nl_langinfo and the
// POSIX locale model. Other platforms keep the C plugin.
#[cfg(not(unix))]
compile_error!("LocalePlugin's Rust port targets Unix; Windows keeps the C plugin.");

pub mod parse;
mod posix;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

pharo_plugin!("LocalePlugin", init = initialise);

/// `initialiseModule` → `sqLocInitialize()`: adopt the environment's locale.
/// The C answers 1 unconditionally, so this never declines the load.
fn initialise() -> bool {
    posix::locale_string();
    true
}

/// Answers a fresh String of `size` indexable bytes with `contents` at the
/// front — the generated C's `instantiateClassindexableSize` + `safestrcpy`
/// pattern. `instantiate` zero-fills, so when `contents` is shorter the tail
/// stays NUL, exactly as the C left it.
fn string_answer(vm: &Interp, size: sqInt, contents: &[u8]) -> PrimResult<Oop> {
    debug_assert!(contents.len() as sqInt <= size);
    let oop = vm.instantiate(vm.class_string()?, size)?;
    vm.write_bytes(oop, 0, contents)?;
    Ok(oop)
}

/// A 3-char String holding the 2-letter ISO 3166 country code (plus NUL).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveCountry(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    string_answer(vm, 3, posix::country())
}

/// A 3-char String holding the 2-letter ISO 639 language code (plus NUL).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveLanguage(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    string_answer(vm, 3, posix::language())
}

/// true when the currency symbol precedes the amount.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveCurrencyNotation(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(posix::currency_notation())
}

/// The locale's currency symbol, sized to its actual bytes ("" in the
/// C/POSIX locale).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveCurrencySymbol(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let symbol = posix::currency_symbol();
    string_answer(vm, symbol.len() as sqInt, &symbol)
}

/// A 1-char String holding the decimal separator.
///
/// The size is fixed at 1 by the generated C; a multi-byte separator is
/// truncated to its first byte here, where the C overflowed the object
/// instead (see the README).
#[pharo_primitive(accessor_depth = -1)]
fn primitiveDecimalSymbol(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let symbol = posix::decimal_point();
    string_answer(vm, 1, &symbol[..symbol.len().min(1)])
}

/// A 1-char String holding the thousands separator, NUL when the locale has
/// none. Same fixed-size-1 contract (and truncation) as the decimal symbol.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveDigitGroupingSymbol(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let symbol = posix::thousands_sep();
    string_answer(vm, 1, &symbol[..symbol.len().min(1)])
}

/// true when the metric system applies. The Unix support layer hardwires 1.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveMeasurementMetric(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(true)
}

/// The date format, from `nl_langinfo(D_FMT)`.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveLongDateFormat(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let fmt = posix::date_format();
    string_answer(vm, fmt.len() as sqInt, &fmt)
}

/// The date format again: the Unix C answers `D_FMT` for the short format
/// too, so long and short are identical on this platform.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveShortDateFormat(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let fmt = posix::date_format();
    string_answer(vm, fmt.len() as sqInt, &fmt)
}

/// The time format, from `nl_langinfo(T_FMT)`.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveTimeFormat(vm: &Interp) -> PrimResult<Oop> {
    vm.expect_argument_count(0)?;
    let fmt = posix::time_format();
    string_answer(vm, fmt.len() as sqInt, &fmt)
}

/// true when DST is in effect right now.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveDaylightSavings(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    // The C dereferenced localtime()'s result unchecked; libc declining is
    // theoretical, and failing cleanly beats crashing the VM.
    posix::daylight_savings().ok_or(PrimErr::GenericFailure)
}

/// Minutes east of UTC for the current local time.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveTimezoneOffset(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    posix::timezone_offset_minutes().ok_or(PrimErr::GenericFailure)
}

/// Minutes the VM's time value is offset from UTC: hardwired 0 on Unix,
/// because the VM clock already runs in local time terms.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveVMOffsetToUTC(vm: &Interp) -> PrimResult<isize> {
    vm.expect_argument_count(0)?;
    Ok(0)
}
