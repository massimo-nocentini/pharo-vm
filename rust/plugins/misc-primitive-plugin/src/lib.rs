//! `MiscPrimitivePlugin`, in Rust.
//!
//! Drop-in replacement for the Slang-generated C plugin
//! (`plugins/MiscPrimitivePlugin/src/common/MiscPrimitivePlugin.c`): the
//! grab-bag of byte-crunching accelerators the image leans on for string
//! comparison, search and hashing, `Bitmap` run-length compression, and
//! 8-bit-to-16-bit sound conversion. Same module name, same nine primitive
//! names, same argument shapes, same failure codes in the same order.
//!
//! The algorithms live in [`algo`], pure and unit-tested; this file is the
//! shells: each primitive fetches its arguments and reruns the C's exact
//! validation sequence -- same checks, same order, same `PrimErr` codes --
//! before calling in. Where the C's generated code would read or write out
//! of bounds (its `firstIndexableField` accesses are unchecked), this port
//! fails the primitive cleanly instead; every such spot is marked and listed
//! in the README.

// Primitive and module names are fixed by the image's pragmas.
#![allow(non_snake_case)]

mod algo;

use pharo_vm_plugin::{pharo_plugin, pharo_primitive, sqInt, Interp, Oop, PrimErr, PrimResult};

// The C answers "MiscPrimitivePlugin VMMaker.oscog-eem.2480 (e)"; the VM
// compares only the prefix against the requested module name, and nothing
// image-side reads the suffix, so the bare name is kept (as jpeg-plugin did).
pharo_plugin!("MiscPrimitivePlugin");

// ---------------------------------------------------------------------------
// Proxy plumbing the safe API does not cover
// ---------------------------------------------------------------------------

/// The C's `isOopImmutable`. A proxy without the entry is a VM built without
/// `IMMUTABILITY`, where the C plugin's macro makes the check constant-false.
fn is_oop_immutable(vm: &Interp, oop: Oop) -> bool {
    // SAFETY: the proxy table pointer comes from the VM via setInterpreter
    // and lives for the process; the field's signature is proxy.rs's.
    unsafe { (*vm.as_raw()).isOopImmutable.is_some_and(|f| f(oop.0) != 0) }
}

/// The element count the C's `sizeOfSTArrayFromCPrimitive` answers: bytes of
/// a byte object, 32-bit words of a Bitmap, elements of a wider array.
///
/// The C derives the oop back from the field pointer and takes `lengthOf:`;
/// `stSizeOf` is the same quantity for the indexable objects reaching here
/// (they have no fixed fields). The caller has already established
/// words-or-bytes, which is what makes the two equivalent.
fn st_size_of(vm: &Interp, oop: Oop) -> PrimResult<sqInt> {
    // SAFETY: as in is_oop_immutable.
    let f = unsafe { (*vm.as_raw()).stSizeOf }.ok_or(PrimErr::Unsupported)?;
    // SAFETY: the entry is the VM's own; oop is a value the VM handed us.
    Ok(unsafe { f(oop.0) })
}

// ---------------------------------------------------------------------------
// The C's argument-fetch idioms
// ---------------------------------------------------------------------------

/// The C's `isBytes(oop)` guard: anything not byte-indexable fails with
/// `PrimErrBadArgument`.
fn check_bytes(vm: &Interp, oop: Oop) -> PrimResult<()> {
    if vm.is_bytes(oop)? {
        Ok(())
    } else {
        Err(PrimErr::BadArgument)
    }
}

/// The C's `stackIntegerValue` + `failed()` sequence: a non-SmallInteger
/// sets the fail flag inside the proxy, and the generated code then fails
/// with `err` -- `BadArgument` where it calls `primitiveFailFor` explicitly,
/// the flag's own `GenericFailure` where it just `return null`s.
fn small_integer(vm: &Interp, oop: Oop, err: PrimErr) -> PrimResult<sqInt> {
    if vm.is_integer_object(oop)? {
        vm.integer_value(oop)
    } else {
        Err(err)
    }
}

/// The C's `arrayValueOf` acceptance test: fails (with `err`, the code its
/// caller's `failed()` path produces) unless `oop` is indexable words or
/// bytes.
fn check_words_or_bytes(vm: &Interp, oop: Oop, err: PrimErr) -> PrimResult<()> {
    if vm.is_words_or_bytes(oop)? {
        Ok(())
    } else {
        Err(err)
    }
}

// ---------------------------------------------------------------------------
// String primitives
// ---------------------------------------------------------------------------

/// `ByteString class>>compare:with:collated:` -- answers 1, 2 or 3.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveCompareString(
    vm: &Interp,
    string1: Oop,
    string2: Oop,
    order: Oop,
) -> PrimResult<isize> {
    // One conjoined check in the C, order first, all failing the same way.
    if !(vm.is_bytes(order)? && vm.is_bytes(string2)? && vm.is_bytes(string1)?) {
        return Err(PrimErr::BadArgument);
    }
    let order = vm.bytes_of(order)?;
    if order.len() < 256 {
        return Err(PrimErr::BadArgument);
    }
    let s1 = vm.bytes_of(string1)?;
    let s2 = vm.bytes_of(string2)?;
    Ok(algo::compare_collated(s1, s2, order))
}

/// `ByteString class>>findFirstInString:inSet:startingAt:`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveFindFirstInString(
    vm: &Interp,
    aString: Oop,
    inclusionMap: Oop,
    start: Oop,
) -> PrimResult<isize> {
    check_bytes(vm, aString)?;
    check_bytes(vm, inclusionMap)?;
    let i = small_integer(vm, start, PrimErr::BadArgument)? - 1;
    if i < 0 {
        return Err(PrimErr::BadIndex);
    }
    let map = vm.bytes_of(inclusionMap)?;
    // Not a failure in the C: a map of the wrong size just answers 0.
    if map.len() != 256 {
        return Ok(0);
    }
    let s = vm.bytes_of(aString)?;
    Ok(algo::find_first_in_string(s, map, i as usize) as isize)
}

/// `ByteString>>findSubstring:in:startingAt:matchTable:`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveFindSubstring(
    vm: &Interp,
    key: Oop,
    body: Oop,
    start: Oop,
    matchTable: Oop,
) -> PrimResult<isize> {
    check_bytes(vm, key)?;
    check_bytes(vm, body)?;
    let start = small_integer(vm, start, PrimErr::BadArgument)?;
    check_bytes(vm, matchTable)?;
    let table = vm.bytes_of(matchTable)?;
    if table.len() < 256 {
        return Err(PrimErr::BadArgument);
    }
    let key = vm.bytes_of(key)?;
    let body = vm.bytes_of(body)?;
    Ok(algo::find_substring(key, body, start, table) as isize)
}

/// `ByteString>>indexOfAscii:inString:startingAt:`.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveIndexOfAsciiInString(
    vm: &Interp,
    anInteger: Oop,
    aString: Oop,
    start: Oop,
) -> PrimResult<isize> {
    let an_integer = small_integer(vm, anInteger, PrimErr::BadArgument)?;
    check_bytes(vm, aString)?;
    let start = small_integer(vm, start, PrimErr::BadArgument)?;
    if start < 1 {
        return Err(PrimErr::BadIndex);
    }
    let s = vm.bytes_of(aString)?;
    Ok(algo::index_of_ascii(an_integer, s, start))
}

/// `ByteArray class>>hashBytes:startingWith:`.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveStringHash(vm: &Interp, aByteArray: Oop, speciesHash: Oop) -> PrimResult<isize> {
    // The C reads the hash from the stack before checking the array.
    let hash = small_integer(vm, speciesHash, PrimErr::BadArgument)?;
    check_bytes(vm, aByteArray)?;
    let bytes = vm.bytes_of(aByteArray)?;
    // The C stores the sqInt into an `unsigned int`: truncate the same way.
    Ok(algo::hash_bytes(bytes, hash as u32) as isize)
}

/// `ByteString class>>translate:from:to:table:` -- in place; answers the
/// receiver.
#[pharo_primitive(accessor_depth = 1)]
fn primitiveTranslateStringWithTable(
    vm: &Interp,
    aString: Oop,
    start: Oop,
    stop: Oop,
    table: Oop,
) -> PrimResult<()> {
    check_bytes(vm, aString)?;
    if is_oop_immutable(vm, aString) {
        return Err(PrimErr::NoModification);
    }
    let start = small_integer(vm, start, PrimErr::BadArgument)?;
    let stop = small_integer(vm, stop, PrimErr::BadArgument)?;
    check_bytes(vm, table)?;
    let len = vm.byte_size_of(aString)?;
    if !(start >= 1 && stop <= len) {
        return Err(PrimErr::BadIndex);
    }
    if vm.byte_size_of(table)? < 256 {
        return Err(PrimErr::BadArgument);
    }
    // The C's `for (i = start - 1; i < stop; i++)`: empty when stop < start.
    if stop < start {
        return Ok(());
    }
    let (start0, stop) = (start as usize - 1, stop as usize);
    if aString == table {
        // The C tolerates the string being its own table, each assignment
        // seeing the previous ones -- which is what translating it in place
        // does. (len >= 256 here, since the table check passed.)
        vm.with_bytes_mut(aString, |buf| {
            for i in start0..stop {
                buf[i] = buf[usize::from(buf[i])];
            }
        })
    } else {
        // In place too, the table read inside the view: two distinct oops
        // cannot be one object, so the read cannot collide with the write.
        vm.with_bytes_mut(aString, |buf| {
            algo::translate(&mut buf[start0..stop], vm.bytes_of(table)?);
            Ok(())
        })?
    }
}

// ---------------------------------------------------------------------------
// Sound conversion
// ---------------------------------------------------------------------------

/// `SampledSound class>>convert8bitSignedFrom:to16Bit:` -- answers the
/// receiver.
#[pharo_primitive(accessor_depth = 2)]
fn primitiveConvert8BitSigned(
    vm: &Interp,
    aByteArray: Oop,
    aSoundBuffer: Oop,
) -> PrimResult<()> {
    check_bytes(vm, aByteArray)?;
    // arrayValueOf's failure is turned into BadArgument by the C here.
    check_words_or_bytes(vm, aSoundBuffer, PrimErr::BadArgument)?;
    if is_oop_immutable(vm, aSoundBuffer) {
        return Err(PrimErr::NoModification);
    }
    let src_len = usize::try_from(vm.byte_size_of(aByteArray)?)?;
    if usize::try_from(vm.byte_size_of(aSoundBuffer)?)? < 2 * src_len {
        return Err(PrimErr::BadArgument);
    }
    // Sample by sample into the buffer, as the C's interleaved loop did.
    // The size check excluded aliasing already (a buffer of at least twice
    // the array's size cannot be the array unless both are empty, and an
    // empty view collides with nothing).
    vm.with_bytes_mut(aSoundBuffer, |dst| {
        let src = vm.bytes_of(aByteArray)?;
        for (slot, &sample) in dst.chunks_exact_mut(2).zip(src) {
            slot.copy_from_slice(&algo::sample_16(sample).to_ne_bytes());
        }
        Ok(())
    })?
}

// ---------------------------------------------------------------------------
// Bitmap compression
// ---------------------------------------------------------------------------

/// `Bitmap class>>compress:toByteArray:` -- answers the encoded byte count.
///
/// No accessor-depth export in the external C plugin, so the loader takes
/// -1; the built-in export table says -1 explicitly. Kept identical.
#[pharo_primitive(accessor_depth = -1)]
fn primitiveCompressToByteArray(vm: &Interp, bm: Oop, ba: Oop) -> PrimResult<isize> {
    // arrayValueOf failure surfaces as a bare `return null` in the C: the
    // flag's own GenericFailure, not BadArgument.
    check_words_or_bytes(vm, bm, PrimErr::GenericFailure)?;
    check_bytes(vm, ba)?;
    if is_oop_immutable(vm, ba) {
        return Err(PrimErr::NoModification);
    }
    let size = usize::try_from(st_size_of(vm, bm)?)?;
    let dest_size = usize::try_from(vm.byte_size_of(ba)?)?;
    if dest_size < algo::compress_bound(size) {
        return Err(PrimErr::Unsupported);
    }
    // The C now reads `size` 32-bit words through the element pointer. For a
    // byte-indexable or 16-bit bm that runs past the object -- undefined
    // behaviour this port replaces with a clean failure (see README).
    let bm_bytes = usize::try_from(vm.byte_size_of(bm)?)?;
    match size.checked_mul(4) {
        Some(span) if span <= bm_bytes => {}
        _ => return Err(PrimErr::BadArgument),
    }
    // Encoded straight into the destination, as the C did. Its view is
    // taken before the bitmap is read, so passing one byte object as both --
    // which the C would have compressed while overwriting it -- fails here
    // instead.
    let written = vm.with_bytes_mut(ba, |dst| {
        let words = vm.words_of(bm)?;
        PrimResult::Ok(algo::compress_into(&words[..size], dst))
    })??;
    debug_assert!(written <= dest_size);
    Ok(written as isize)
}

/// `Bitmap>>decompress:fromByteArray:at:` -- fills `bm` in place; answers
/// the receiver.
#[pharo_primitive(accessor_depth = 0)]
fn primitiveDecompressFromByteArray(
    vm: &Interp,
    bm: Oop,
    ba: Oop,
    index: Oop,
) -> PrimResult<()> {
    // The C calls arrayValueOf(bm) first but only consults failed() after
    // the immutability, bytes and index checks, so those codes win; the
    // deferred arrayValueOf/stackIntegerValue failures share GenericFailure.
    if is_oop_immutable(vm, bm) {
        return Err(PrimErr::NoModification);
    }
    check_bytes(vm, ba)?;
    let index = small_integer(vm, index, PrimErr::GenericFailure)?;
    check_words_or_bytes(vm, bm, PrimErr::GenericFailure)?;
    // The C's pastEnd is bm's element count; its writes are still 32-bit
    // words, unchecked against the object's actual byte size. write_words
    // checks, so a write the C would have made out of bounds (byte or
    // 16-bit bm) fails with BadIndex instead -- runs the C wrote in bounds
    // land identically.
    let past_end = usize::try_from(st_size_of(vm, bm)?)?;
    // Decoded straight into the bitmap, run by run, as the C wrote it. The
    // destination's view is taken first, so the image passing the same byte
    // object as both bm and ba -- the C decoding a stream it is itself
    // overwriting -- fails here instead of being reproduced.
    vm.with_words_mut(bm, |dst| {
        algo::decompress(vm.bytes_of(ba)?, index - 1, past_end, dst)
            // WouldOverrun is the C's explicit PrimErrBadIndex; TruncatedInput
            // is where the C reads past the byte array instead of failing.
            .map_err(|_| PrimErr::BadIndex)
    })?
}
