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

/// First indexable byte of `oop`, for the two writes the safe API cannot
/// express (16-bit lanes of a word object).
fn first_field_ptr(vm: &Interp, oop: Oop) -> PrimResult<*mut u8> {
    // SAFETY: as in is_oop_immutable.
    let f = unsafe { (*vm.as_raw()).firstIndexableField }.ok_or(PrimErr::Unsupported)?;
    // SAFETY: the entry is the VM's own; oop is a value the VM handed us.
    let p = unsafe { f(oop.0) };
    if p.is_null() {
        return Err(PrimErr::BadArgument);
    }
    Ok(p.cast::<u8>())
}

/// Writes `values` as consecutive native-endian `u16`s from the start of
/// `oop`'s indexable bytes -- what the C does through an `unsigned short *`.
///
/// The caller has checked mutability; bounds are re-checked here so the
/// unsafe block stands on its own.
fn write_u16s(vm: &Interp, oop: Oop, values: &[u16]) -> PrimResult<()> {
    let byte_len = usize::try_from(vm.byte_size_of(oop)?)?;
    let span = values.len().checked_mul(2).ok_or(PrimErr::BadIndex)?;
    if span > byte_len {
        return Err(PrimErr::BadIndex);
    }
    let base = first_field_ptr(vm, oop)?;
    // SAFETY: base points at byte_len bytes owned by the object; the span
    // was just checked against it; write_unaligned because only whole-word
    // alignment is guaranteed; `values` is a Rust-owned slice, so it cannot
    // overlap the destination.
    unsafe {
        let dst = base.cast::<u16>();
        for (i, v) in values.iter().enumerate() {
            dst.add(i).write_unaligned(*v);
        }
    }
    Ok(())
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
        // seeing the previous ones. One shared buffer reproduces that; only
        // region indices are ever assigned, so writing the region back
        // suffices. (len >= 256 here, since the table check passed.)
        let mut buf = vm.bytes_of(aString)?.to_vec();
        for i in start0..stop {
            buf[i] = buf[usize::from(buf[i])];
        }
        vm.write_bytes(aString, start0, &buf[start0..stop])
    } else {
        let mut region = vm.bytes_of(aString)?[start0..stop].to_vec();
        let table = vm.bytes_of(table)?;
        algo::translate(&mut region, table);
        vm.write_bytes(aString, start0, &region)
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
    let bytes = vm.bytes_of(aByteArray)?;
    if vm.byte_size_of(aSoundBuffer)? < 2 * bytes.len() as sqInt {
        return Err(PrimErr::BadArgument);
    }
    // The size check just excluded aliasing (a buffer of at least twice the
    // array's size cannot be the array unless both are empty), so reading
    // everything before writing matches the C's interleaved loop.
    let samples: Vec<u16> = bytes.iter().map(|&b| algo::sample_16(b)).collect();
    write_u16s(vm, aSoundBuffer, &samples)
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
    let out = {
        let words = vm.words_of(bm)?;
        algo::compress(&words[..size])
    };
    debug_assert!(out.len() <= dest_size);
    vm.write_bytes(ba, 0, &out)?;
    Ok(out.len() as isize)
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
    // Snapshot the encoded bytes: nothing stops the image passing the same
    // byte object as both bm and ba, and the C then decodes a stream it is
    // itself overwriting. Reading a copy keeps the borrow away from the
    // writes; the self-overwriting case is in the C's out-of-bounds-write
    // territory anyway (see README).
    let encoded = vm.bytes_of(ba)?.to_vec();
    algo::decompress(&encoded, index - 1, past_end, &mut |k, words| {
        vm.write_words(bm, k, words)
    })
    .map_err(|e| match e {
        algo::DecompressError::Sink(code) => code,
        // WouldOverrun is the C's explicit PrimErrBadIndex; TruncatedInput
        // is where the C reads past the byte array instead of failing.
        _ => PrimErr::BadIndex,
    })
}
