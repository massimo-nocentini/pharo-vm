# LargeIntegers, in Rust

Replaces the Slang-generated **LargeIntegers v2.1** plugin
(`plugins/LargeIntegers/src/common/LargeIntegers.c`, VMMaker
`oscog-eem.2495`, ~2,400 lines of machine-produced C) behind the same fifteen
primitives. This is the plugin the image leans on for every arbitrary-precision
`+ - * // \\ bitAnd: bitShift:` and for Montgomery exponentiation in crypto
code, so the port's whole job is to be indistinguishable.

## The contract is unchanged

Same module name — including the version tag, because the image-side code
calls `primGetModuleName` and checks the `v2.1` revision — same primitive
names, same argument shapes, same accessor depths (checked value-for-value
against the C's `...AccessorDepth` exports, including the two the C leaves
unexported as −1), same failure codes, and the same deliberate oddities:

* `primDigitAdd`'s carry case, `primDigitBitShiftMagnitude`'s left shift and
  both halves of `primDigitDivNegative`'s `{quotient. remainder}` Array are
  answered **unnormalized**; the image normalizes them itself, and changing
  that here would change what image code observes.
* `primNormalizePositive` / `primNormalizeNegative` answer the receiver
  *identically* (same oop) when nothing needs trimming.
* The division answers the remainder in the *dividend's* class, while the
  quotient's class follows the `negative` flag — as the C does.
* `primDigitSubtract` on unnormalized operands produces the same mod-2³²ⁿ
  nonsense the C produces: the larger/smaller decision looks only at digit
  counts and top digits. Garbage in, the identical garbage out.
* Failure codes are per-check: a wrong-classed *argument* fails with
  `PrimErrBadArgument`, a wrong-classed *receiver* (the C's `success(...)`
  pattern), negative operands to bit logic, a zero divisor and unnormalized
  division operands all fail with the generic code.

## What changed underneath

**No scratch objects in image memory.** The C converts every SmallInteger
operand by allocating an intermediate LargeInteger object, and its division
allocates shifted copies of both operands as image objects before dividing.
This port reads operands into Rust vectors, computes there
([`src/digits.rs`](src/digits.rs), one function per `cDigit...` helper, digit
representation identical: little-endian 32-bit words over the byte object),
and allocates exactly the objects the image will see. The one observable
consequence: allocation-failure (`PrimErrNoMemory`) can strike at different
points under memory exhaustion.

**No out-of-slack writes.** The C writes whole 32-bit words through partial
trailing words into allocation slack (the carry byte in `primDigitAdd`, the
shift loops, `largeIntgrowTo`). Those writes only ever carry zero bytes, and
this port writes exactly `byte_len` bytes instead (debug assertions enforce
the "beyond is zero" invariant).

**Undefined behaviour removed** (each produced garbage or crashed in C, and
now fails cleanly or is deterministic):

* Zero-length LargeInteger operands: `primDigitSubtract` read one word
  *below* the object when comparing empty magnitudes, and
  `primMontgomeryTimesModulo` read one word past an empty second operand.
  The subtraction now treats the missing top digit as zero (deterministic,
  same code path shape); the Montgomery case fails with the generic code.
* `primGetModuleName` did `strncpy` into a possibly-failed allocation; the
  division built its result Array without checking the allocation and could
  push oop 0. Both are clean `PrimErrNoMemory` failures now.
* The C primitives never check their argument count and read whatever is on
  the stack when installed at the wrong arity; every primitive here fails
  with `PrimErrBadNumArgs` instead.

**Proxy surface.** Beyond the SDK's safe API, the port calls five raw proxy
entries — `fetchClassOf`, `classLargePositiveInteger`,
`classLargeNegativeInteger`, `isBooleanObject`, `positive32BitValueOf`,
`stObjectatput` — the same entries the C imports. No `ioLoadFunctionFrom`
lookups, no state between calls, no dependencies beyond the SDK.

## Verification

`cargo test -p large-integers` — 44 tests over the pure digit core:

* **Cross-checks against `u128` arithmetic** for add, subtract (both signs),
  multiply, and/or/xor, both shifts, `anyBit`, and division (quotient and
  remainder, ~1000 random cases).
* **An independent base-256 schoolbook reference** implemented in the tests
  validates multiplication up to 40-byte operands, and reconstructs division
  inputs as `a = q·b + r` from adversarial digit patterns (0, 1, `FFFFFFFF`,
  `FFFFFFFE`, `80000000`, `7FFFFFFF`) — >1000 cases exercising the
  `cDigitDiv` quotient-estimate corrections, the `r1 = dh` shortcut and the
  add-back path.
* **Montgomery multiplication** against a reference `a·b·R⁻¹ mod m` computed
  with extended-Euclid inverses, plus the `R mod m` identity.
* **Normalization** at every boundary: `SmallInteger maxVal`/`minVal`
  (including −2⁶⁰, the negative value with no positive twin), two-digit
  recombination, 1/2/3-byte trims, leading-zero-digit stripping.
* **The C's exact conventions**: `byteSizeOfCSI:` thresholds, unnormalized
  subtraction wrap-around, result byte lengths of shifts, the trimmed-prefix
  subtraction path, digit/byte round-trips over partial words.

The exported surface of the built `libLargeIntegers.so` was diffed against
the C's export table: all fifteen primitives, `getModuleName`,
`setInterpreter`, and accessor-depth bytes identical (1/1/1/1/3/1/0/2/1/1/
−1/−1/1/1/1).

## Not verified

* **Image-side differential testing.** No VM runs in the porting
  environment, so the glue — stack offsets, failure-code observation by
  fallback code, class identity of results, the `primNormalize*` receiver
  identity, `stObjectatput` on the division Array — is faithful by reading,
  not by execution. The digit arithmetic itself is heavily tested, so a
  differential pass can focus on the plumbing.
* **32-bit images.** The SmallInteger bounds and `byteSizeOfCSI:` cap follow
  the pointer width and are unit-tested only on 64-bit hosts here.
* **Big-endian hosts.** The C swaps digit bytes per access
  (`SQ_SWAP_4_BYTES_IF_BIGENDIAN`); this port assembles digits from bytes
  little-endian, which is endianness-independent by construction — but no
  big-endian machine was available to prove it.
* **CMake wiring.** The crate builds and exports the right symbols, but the
  build still compiles the C plugin; switching the build over is a separate,
  reviewable change, as with jpeg-plugin.
