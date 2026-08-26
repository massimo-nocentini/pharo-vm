# DSAPrims, in Rust

Replaces `plugins/DSAPrims/src/common/DSAPrims.c` (~560 lines of
Slang-generated C, from `DSAPlugin CryptographyPlugins-eem.14`): the helper
primitives behind the image's DSA and SHA-1 code. Base-256 big-integer
multiply and Knuth divide over LargePositiveInteger digit strings, the SHA-1
message-schedule expansion and 80-round block hash over image-side Bitmaps,
and a highest-non-zero-digit scan.

These primitives operate on objects whose layout the image's Smalltalk code
owns — digit strings least-significant byte first, a 64-byte block, Bitmaps of
80 schedule words and 5 state words — so this is a mechanical translation of
the C loops (`src/dsa.rs`), not a swap-in of a crypto crate. The hash is SHA-1
shaped: big-endian block load, rotate-left-1 in the expansion (SHA-1, not
SHA-0), the four standard round constants, Davies-Meyer feed-forward.

## The contract is unchanged

Same six primitives, same argument order, same class/size/kind validation,
same answers, same accessor depths (1 everywhere, −1 for
`primitiveHasSecureHashPrimitive`, matching the C's export table).
`getModuleName` answers the C external build's full string,
`"DSAPrims CryptographyPlugins-eem.14 (e)"`; the VM accepts it by prefix.
Build the crate and drop `libDSAPrims.so` where the VM looks for plugins.

One inferred detail: the C never checks its argument count, so the arities
here are the ones its `pop` calls assume — the only ones under which the C
left the stack balanced. That makes `primitiveBigDivide` and
`primitiveBigMultiply` ternary, `primitiveExpandBlock` and
`primitiveHashBlock` binary, and `primitiveHasSecureHashPrimitive` and
`primitiveHighestNonZeroDigitIndex` unary sends — for the last one, the
LargePositiveInteger is the *receiver*, its comment ("called with one
argument") notwithstanding.

## What changed underneath

**Failure now precedes the body.** The generated C calls `primitiveFailFor`
on a bad argument and then *keeps executing*: `primitiveBigDivide` on a
non-LargePositiveInteger argument still called `firstIndexableField` on it
and ran the whole division through whatever pointer came back, and
`primitiveBigMultiply` with a wrong-sized product still multiplied into it.
The VM discards the result either way, but the memory writes happened. Here
every check fails the primitive before anything is read or written.

**Three undefined behaviours in `bigDivide` are now clean failures**
(`PrimErrBadArgument`):

* a divisor of fewer than 2 digits made the C read a byte *before* the
  object's first field (its base-1 pointer adjustment walks one byte back);
* a divisor whose top digit is zero made the C divide by zero;
* the C never checked the quotient's size and wrote past its end when it was
  smaller than `rem size - div size`.

**The digits a primitive writes are copied out, computed on, and copied
back**, rather than mutated in place through `firstIndexableField`. A
primitive that fails therefore leaves the image's objects untouched, and
aliased arguments (the same object passed as remainder and quotient, say)
cannot interleave reads with writes. That is what the staging buys, so only
the destinations pay for it: the remainder and quotient of `bigDivide` and
the product of `bigMultiply`. The operands those two only *read* -- the
divisor, both factors -- and the block and schedule the two SHA-1 primitives
read are taken where they lie.

**Immutable objects are refused** up front (`PrimErrNoModification`); the C
wrote through Pharo's immutability bit.

Everything else is kept faithful, including the C's oddities: the multiply
*overwrites* (not adds) each row's final carry cell and skips rows for zero
digits, so it assumes a zero-filled product; the divide's quotient-digit
store truncates modulo 256; and `primitiveHighestNonZeroDigitIndex` answers 1
— not 0 — for an all-zero (or empty) number, because the C's scan loop cannot
distinguish "stopped at digit 1" from "ran out". All are pinned by tests.

## Verification

19 unit tests over the pure core, `cargo test -p dsa-prims`:

* **SHA-1 known answers** through the same two entry points the image uses
  (expand, then hash, with standard padding): the empty string, `"abc"`, the
  two-block NIST string, and the million-`a` vector.
* **Expansion**: big-endian word load, and the recurrence checked against an
  independently spelled computation.
* **Multiply**: known answers against `u128` products, a 500-case
  deterministic sweep against a column-wise reference implementation,
  zero-digit rows, and the prefilled-product carry-overwrite quirk.
* **Divide**: known answers and a 2000-case sweep against `u128` division
  (normalised divisors, in-range quotients — the C's own preconditions), a
  constructed vector for the estimate-correction path, a constructed vector
  proving the rare add-back path runs (the core returns an add-back count for
  exactly this), the no-op case, and the three rejected-UB cases.
* **Digit scan**: traced against the C loop, including the all-zero quirk.

## Not verified

No VM runs in this environment, so the proxy-facing layer is untested and an
image-side differential pass should focus on:

* **The inferred arities**, above all that `primitiveHighestNonZeroDigitIndex`
  is a unary send on the integer itself. If the image-side callers disagree,
  the arity checks here will fail the primitives (the C checked nothing).
* The raw proxy calls (`fetchClassOf`, `classLargePositiveInteger`,
  `stSizeOf`, `isWords`, `isOopImmutable`) and the write-backs through
  `write_bytes`/`write_words`.
* Module-name acceptance of the versioned string by prefix.
* Behaviour with divisors the image has *not* normalised (top digit < 128):
  faithful to the C — which computes wrong digits there — but only the
  normalised case is exercised by tests.

## Not done yet

* **CMake wiring.** The crate builds and exports the full C symbol set
  (verified with `nm`: six primitives, six accessor-depth bytes,
  `getModuleName`, `setInterpreter`), but the build still compiles the C
  plugin; switching over is a separate change, as with jpeg-plugin.
