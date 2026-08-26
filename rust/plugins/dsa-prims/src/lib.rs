//! `DSAPrims`, in Rust: helper primitives for the image's DSA and SHA-1 code.
//!
//! Replaces the Slang-generated `plugins/DSAPrims/src/common/DSAPrims.c`
//! (from `DSAPlugin CryptographyPlugins-eem.14`): base-256 big-integer
//! multiply and divide, the SHA-1 message-schedule expansion and block hash,
//! and a highest-non-zero-digit scan. The objects operated on --
//! LargePositiveInteger digit strings, a 64-byte ByteArray block, Bitmaps of
//! 80 schedule words and 5 state words -- are laid out by the image's
//! Smalltalk code, so this is a mechanical translation of the C loops (see
//! [`dsa`]), not a swap-in of a crypto crate.
//!
//! # Argument shapes
//!
//! The C never checks its argument count; the arities here are the ones its
//! `pop` calls assume, which are the only ones under which the C left the
//! stack balanced:
//!
//! | primitive | args | stack (top last) |
//! |---|---|---|
//! | `primitiveBigDivide` | 3 | rem, div, quo |
//! | `primitiveBigMultiply` | 3 | f1, f2, prod |
//! | `primitiveExpandBlock` | 2 | buf, expanded |
//! | `primitiveHashBlock` | 2 | buf, state |
//! | `primitiveHasSecureHashPrimitive` | 0 | -- |
//! | `primitiveHighestNonZeroDigitIndex` | 0 | the receiver is the integer |
//!
//! # What changes underneath
//!
//! The C's validation failures set the failure flag and *kept executing* the
//! body on the unvalidated objects; here a failed check fails the primitive
//! before anything is read or written. The README lists every such
//! divergence; all of them replace undefined behaviour.

// The crate is named for the shared library the VM loads (libDSAPrims.so),
// and the primitive names are fixed by the image.
#![allow(non_snake_case)]

mod dsa;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

// The C answers "DSAPrims CryptographyPlugins-eem.14 (e)" when built as an
// external plugin. The VM accepts it by prefix (`strncmp` against the
// requested name in sqNamedPrims.c's callInitializersIn), so the version
// suffix is kept faithful to the C.
pharo_plugin!("DSAPrims CryptographyPlugins-eem.14 (e)");

// ---------------------------------------------------------------------------
// Raw proxy entries the safe API does not wrap
// ---------------------------------------------------------------------------
//
// The C validates with `fetchClassOf == classLargePositiveInteger`, `isWords`
// and `stSizeOf`, none of which `Interp` exposes. Each helper below makes the
// one call, with the same nullable-entry handling `Interp`'s own wrappers use.

/// `interpreterProxy->classLargePositiveInteger()`.
fn class_large_positive_integer(vm: &Interp) -> PrimResult<Oop> {
    // SAFETY: as_raw is the proxy table the VM handed to setInterpreter, and
    // the signature is the one in proxy.rs.
    let f = unsafe { (*vm.as_raw()).classLargePositiveInteger }.ok_or(PrimErr::Unsupported)?;
    Ok(Oop(unsafe { f() }))
}

/// `interpreterProxy->fetchClassOf(oop)`.
fn fetch_class_of(vm: &Interp, oop: Oop) -> PrimResult<Oop> {
    // SAFETY: as above.
    let f = unsafe { (*vm.as_raw()).fetchClassOf }.ok_or(PrimErr::Unsupported)?;
    Ok(Oop(unsafe { f(oop.0) }))
}

/// `interpreterProxy->stSizeOf(oop)`: indexable slot count -- bytes for a
/// byte object, 32-bit words for a word object. This is the size the C
/// compares, distinct from `byteSizeOf`.
fn st_size_of(vm: &Interp, oop: Oop) -> PrimResult<sqInt> {
    // SAFETY: as above.
    let f = unsafe { (*vm.as_raw()).stSizeOf }.ok_or(PrimErr::Unsupported)?;
    Ok(unsafe { f(oop.0) })
}

/// `interpreterProxy->isWords(oop)`: word-indexable, strictly -- a ByteArray
/// answers false here but true to `isWordsOrBytes`, and the C requires the
/// strict test.
fn is_words(vm: &Interp, oop: Oop) -> PrimResult<bool> {
    // SAFETY: as above.
    let f = unsafe { (*vm.as_raw()).isWords }.ok_or(PrimErr::Unsupported)?;
    Ok(unsafe { f(oop.0) } != 0)
}

/// `interpreterProxy->isOopImmutable(oop)`. Checked up front on every object
/// a primitive writes, so a read-only argument fails cleanly before anything
/// is mutated instead of after a partial write.
fn is_immutable(vm: &Interp, oop: Oop) -> PrimResult<bool> {
    // SAFETY: as above.
    let f = unsafe { (*vm.as_raw()).isOopImmutable }.ok_or(PrimErr::Unsupported)?;
    Ok(unsafe { f(oop.0) } != 0)
}

/// Fails with `BadArgument` unless every oop is a LargePositiveInteger --
/// the exact class, as the C's `fetchClassOf == classLargePositiveInteger`
/// tests, not a kind-of check.
fn expect_large_positive_integers(vm: &Interp, oops: &[Oop]) -> PrimResult<()> {
    let clpi = class_large_positive_integer(vm)?;
    for &oop in oops {
        if fetch_class_of(vm, oop)? != clpi {
            return Err(PrimErr::BadArgument);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// Divides `div` into `rem`, leaving the quotient in `quo` and the remainder
/// in `rem`. All three are LargePositiveIntegers; `quo` is expected to arrive
/// zero-filled and sized to hold the quotient.
///
/// The two the division *writes* -- `rem`, which it subtracts into as it
/// goes, and `quo` -- are staged in Rust memory and copied back at the end,
/// so a failure, or the aliasing of two arguments, can never leave a
/// half-subtracted remainder in the image. The divisor is only read, so it
/// is read where it lies.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveBigDivide(vm: &Interp, rem: Oop, div: Oop, quo: Oop) -> PrimResult<()> {
    expect_large_positive_integers(vm, &[rem, div, quo])?;
    if is_immutable(vm, rem)? || is_immutable(vm, quo)? {
        return Err(PrimErr::NoModification);
    }
    // For byte objects stSizeOf and the byte length agree, so the slice
    // lengths are exactly the digit counts the C worked with.
    let mut rem_digits = vm.bytes_of(rem)?.to_vec();
    let mut quo_digits = vm.bytes_of(quo)?.to_vec();

    // The divisor's borrow ends with the call, before either write-back.
    dsa::big_divide(&mut rem_digits, vm.bytes_of(div)?, &mut quo_digits)?;

    vm.write_bytes(rem, 0, &rem_digits)?;
    vm.write_bytes(quo, 0, &quo_digits)?;
    Ok(())
}

/// Multiplies `f1` by `f2` into `prod`. All three are LargePositiveIntegers;
/// `prod` must be exactly `f1 size + f2 size` digits and is expected to
/// arrive zero-filled.
///
/// The factors are read where they lie; the product is staged and copied
/// back for the reason `primitiveBigDivide` stages its two, so that an
/// argument passed twice behaves as it did in the C rather than being
/// accumulated into while it is read.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveBigMultiply(vm: &Interp, f1: Oop, f2: Oop, prod: Oop) -> PrimResult<()> {
    expect_large_positive_integers(vm, &[f1, f2, prod])?;
    let prod_len = st_size_of(vm, prod)?;
    let f1_len = st_size_of(vm, f1)?;
    let f2_len = st_size_of(vm, f2)?;
    if prod_len != f1_len + f2_len {
        return Err(PrimErr::BadArgument);
    }
    if is_immutable(vm, prod)? {
        return Err(PrimErr::NoModification);
    }
    let mut prod_digits = vm.bytes_of(prod)?.to_vec();

    dsa::big_multiply(vm.bytes_of(f1)?, vm.bytes_of(f2)?, &mut prod_digits);

    vm.write_bytes(prod, 0, &prod_digits)?;
    Ok(())
}

/// Expands a 64-byte ByteArray (`buf`) into a Bitmap of 80 32-bit words
/// (`expanded`), reading the block big-endian -- the SHA-1 message schedule.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveExpandBlock(vm: &Interp, buf: Oop, expanded: Oop) -> PrimResult<()> {
    // Same four tests as the C, in the same order.
    if !(is_words(vm, expanded)?
        && vm.is_bytes(buf)?
        && st_size_of(vm, expanded)? == 80
        && st_size_of(vm, buf)? == 64)
    {
        return Err(PrimErr::BadArgument);
    }
    if is_immutable(vm, expanded)? {
        return Err(PrimErr::NoModification);
    }
    // The size check above proved the conversion; the block is expanded out
    // of the object rather than through a copy of it.
    let block: &[u8; 64] = vm
        .bytes_of(buf)?
        .try_into()
        .map_err(|_| PrimErr::BadArgument)?;
    let words = dsa::expand_block(block);
    vm.write_words(expanded, 0, &words)?;
    Ok(())
}

/// Hashes a Bitmap of 80 expanded words (`buf`) into the 5-word SHA-1 state
/// (`state`), updating the state in place.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveHashBlock(vm: &Interp, buf: Oop, state: Oop) -> PrimResult<()> {
    if !(is_words(vm, state)?
        && is_words(vm, buf)?
        && st_size_of(vm, state)? == 5
        && st_size_of(vm, buf)? == 80)
    {
        return Err(PrimErr::BadArgument);
    }
    if is_immutable(vm, state)? {
        return Err(PrimErr::NoModification);
    }
    // The schedule is read where it lies; the state is the accumulator this
    // primitive updates, so it is staged and written back in one go.
    let mut s = [0u32; 5];
    s.copy_from_slice(vm.words_of(state)?);
    let w: &[u32; 80] = vm
        .words_of(buf)?
        .try_into()
        .map_err(|_| PrimErr::BadArgument)?;

    dsa::hash_block(&mut s, w);

    vm.write_words(state, 0, &s)?;
    Ok(())
}

/// Answers true: the secure hash primitives are implemented.
///
/// Accessor depth -1 as in the C, whose builtin export table marks this
/// primitive `\377` and whose external build exports no depth for it at all.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveHasSecureHashPrimitive(vm: &Interp) -> PrimResult<bool> {
    vm.expect_argument_count(0)?;
    Ok(true)
}

/// Answers the 1-based index of the receiver's top-most non-zero digit.
///
/// The receiver is the LargePositiveInteger: the C reads `stackValue(0)` and
/// pops one slot, which balances only for a unary send. Its comment says
/// "called with one argument", but with an argument the C would have left an
/// extra slot on the stack every call.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveHighestNonZeroDigitIndex(vm: &Interp) -> PrimResult<sqInt> {
    vm.expect_argument_count(0)?;
    let arg = vm.stack_value(0)?;
    expect_large_positive_integers(vm, &[arg])?;
    let digits = vm.bytes_of(arg)?;
    Ok(dsa::highest_non_zero_digit_index(digits) as sqInt)
}
