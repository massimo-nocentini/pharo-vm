# MiscPrimitivePlugin, in Rust

Replaces the Slang-generated C plugin
(`plugins/MiscPrimitivePlugin/src/common/MiscPrimitivePlugin.c`, ~800 lines)
behind the same nine primitives: the byte-crunching accelerators the image
leans on constantly — collated string comparison, substring/character/set
search, the 28-bit byte-array hash, `String` translation, `Bitmap`
run-length compression and decompression, and 8-bit-to-16-bit sound sample
conversion.

## The contract is unchanged

Same module name, same primitive names, same argument order, same answers,
same failure codes in the same order — each shell in `src/lib.rs` reruns the
C's validation sequence line for line, down to oddities like
`primitiveFindFirstInString` answering `0` (not failing) for an
inclusion map that is not exactly 256 bytes, `primitiveCompressToByteArray`
failing with `PrimErrUnsupported` for an undersized destination, and the
`GenericFailure`-vs-`BadArgument` split between the C's `return null` and
`primitiveFailFor(PrimErrBadArgument)` paths after a bad integer argument.
The accessor depths match the C exports, including the `-1` for
`primitiveCompressToByteArray` (the external C plugin exports no depth for
it, which the loader reads as −1).

The algorithms themselves live in `src/algo.rs` as pure functions, bit-for-bit
transcriptions of the generated C — including the compressor's exact run
segmentation and the three-tier variable-length integer encoding, so the
encoded bytes are identical, not merely decodable.

## What changed underneath

**The out-of-bounds accesses are gone.** The C indexes
`firstIndexableField` pointers unchecked in four places; each is now a clean
primitive failure (details below).

**Argument counts are checked.** The C plugin never consulted
`methodArgumentCount`; installed on a method of the wrong arity it would
read stack slots that hold the receiver or garbage. Here a wrong arity fails
with `PrimErrBadNumArgs` before anything is read.

**Reads and writes no longer interleave.** `primitiveConvert8BitSigned`
computes all samples, then writes them (its size check makes source/target
overlap impossible, so this is unobservable). `primitiveTranslateStringWithTable`
keeps the C's observable behaviour even when the image passes the string as
its *own* translation table — that case runs over a single shared buffer,
each assignment seeing the previous ones, exactly as the C's in-place loop
did.

## The out-of-bounds accesses

All four are undefined behaviour in the C; this port substitutes a defined
failure and nothing else changes:

* **Truncated compressed input.** `primitiveDecompressFromByteArray` checks
  `i < end` only between tokens; a token cut short makes the C read past the
  byte array and keep going on garbage. Here any read outside the array
  fails the primitive with `PrimErrBadIndex`. Runs already decoded stay
  written, matching the C's incremental writes.
* **A start index below 1** made the C's first read `ba[index - 1]` land
  *before* the array. Same `PrimErrBadIndex` now.
* **A "word array" argument that is not one.** The C's `arrayValueOf`
  accepts any indexable object, and the code then reads or writes *4 bytes
  per element* — for a `ByteArray` or 16-bit array that runs up to 4× past
  the object. `primitiveCompressToByteArray` now fails with
  `PrimErrBadArgument` at the point the C would start reading past the end
  (every defined-behaviour path before it, including the destination-size
  check computed from the C's element count, is unchanged);
  `primitiveDecompressFromByteArray` fails with `PrimErrBadIndex` at the
  first write the C would have made out of bounds, while writes the C made
  in bounds land identically. A 64-bit array, which the C handles without
  overrunning (reading its first `size` 32-bit lanes), behaves as before.
  `primitiveConvert8BitSigned` is untouched: its C bounds check is in bytes
  and correct for every indexable class.
* **Decompressing a byte array into itself** (`bm` and `ba` the same oop) is
  possible because of that same `arrayValueOf` laxity; the C then decodes a
  stream it is itself overwriting. This port decodes a snapshot of the
  input. (Reaching this case at all requires the out-of-bounds-write
  territory above.)

One naming note: the C's `getModuleName` answers
`"MiscPrimitivePlugin VMMaker.oscog-eem.2480 (e)"`. The VM compares only the
prefix against the requested module and nothing image-side reads the suffix,
so this port answers the bare `"MiscPrimitivePlugin"`, as jpeg-plugin does.

## Verification

`cargo test -p misc-primitive-plugin` — 28 tests over the pure core:

* **Compress/decompress**: hand-computed streams for every token kind
  (byte-fill, word-fill, verbatim, and the unwritten code-0), the C's
  trailing-word fold, all three integer encodings at their boundaries
  (223/224, 7935/7936), roundtrips of runs at the 1983/1984-word token-width
  boundary, runs ending exactly at the buffer edge, a deterministic
  pseudorandom corpus, and the output-never-exceeds-the-C's-destination-bound
  property the C relies on. Failure paths: every truncation point, the
  overrun check (including for code 0, which writes nothing but is still
  checked), code 0 not advancing the write position, and partial writes
  surviving a mid-stream failure.
* **Comparison/search/hash/translate/convert**: known answers plus the C's
  edge cases — empty strings and keys, starts clamped/past-the-end, matches
  flush at boundaries, collation and match tables that fold case or invert,
  characters outside 0..255 never matching, the hash against an
  independently written reference and hand-computed values, and the sound
  conversion against the C's two-branch expression for all 256 inputs.

## Not done yet

* **No image-side differential run.** There is no runnable VM in the
  development environment, so the shells — argument fetching, the
  failure-code ordering against a live interpreter, the raw-proxy helpers
  (`stSizeOf`, `firstIndexableField`, `isOopImmutable`, the 16-bit lane
  writes of `primitiveConvert8BitSigned`), and actual module loading — are
  faithful by construction but unverified against the C plugin in a running
  image. That is where a differential pass should focus.
* **The aliased-translate branch** (string as its own table) is reasoned to
  match the C, not tested against it.
* **VMs built without `IMMUTABILITY`** (missing `isOopImmutable` proxy
  entry) take the C's constant-false path here; untested.
* **CMake wiring.** As with jpeg-plugin, the build still compiles the C
  plugin; swapping this crate in via `cmake/rust.cmake` is deliberately a
  separate change.
