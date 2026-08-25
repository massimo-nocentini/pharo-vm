# JPEGReaderPlugin, in Rust

Replaces `plugins/JPEGReaderPlugin/src/common/JPEGReaderPlugin.c` — the
accelerator behind the image's **pure-Smalltalk** JPEG decoder
(`JPEGReadStream` / `JPEGDecompressStream` / `JPEGColorComponent`). Four
primitives take over the decoder's hot loops:

| primitive | accessor depth | does |
|---|---:|---|
| `primitiveDecodeMCU` | 2 | huffman-decodes one 8x8 coefficient block, advancing the stream and the component's DC predictor |
| `primitiveIdctInt` | 1 | dequantises and inverse-transforms one block in place (13-bit fixed-point Loeffler IDCT) |
| `primitiveColorConvertMCU` | 3 | Y'CbCr → 32-bit ARGB for one MCU, with upsampling and error-diffusion dithering |
| `primitiveColorConvertGrayscaleMCU` | 2 | grayscale → 32-bit ARGB, same cursor and dithering |

This is **not** `JPEGReadWriter2Plugin` (the libjpeg-based codec, ported
separately as [`../jpeg-plugin`](../jpeg-plugin)). Nothing here parses a JPEG
file: every piece of decoder state — the byte stream and its bit buffer, the
huffman tables, the quantisation tables, the per-component cursors — lives in
Smalltalk objects whose layout the image owns and hands to every call. No
JPEG crate could substitute, so this is a faithful mechanical translation:
the instance-variable indices, the check order, the fixed-point constants and
the C's exact mix of 32-bit and register-width temporaries (several IDCT
intermediates truncate to 32 bits, observably) are all preserved.

## The contract is unchanged

Same module name (the C's `getModuleName` carries a `VMMaker.oscog-eem.2480`
suffix; the VM compares only the prefix, and this crate answers the bare name
as jpeg-plugin does), same four primitive names, same argument order, same
accessor depths, same failure conditions in the same order.

## What changed underneath

**No cached raw pointers into image memory.** The C held `int*`s to up to
3x128 block WordArrays, the bits array, the residuals and both huffman tables
in globals for the duration of a call. This port copies inputs out, computes
on Rust-owned buffers, and writes results back once.

**Undefined behaviour removed** — each was reachable from a corrupt or
hostile image-side object, and each is now a clean primitive failure (or a
defined value) instead:

* *Stale block pointers.* `nextSampleFrom:` indexed the 128-entry block
  table with no bounds check; a cursor past the blocks actually loaded
  dereferenced whatever a previous call had left there. Now bounds-checked.
* *Division by zero.* The C skips the `dx/sx, dy/sy` division only when
  *both* scales are zero; exactly one zero divided by zero (SIGFPE). Both
  divisions also wrap on `INT_MIN / -1` instead of trapping.
* *Huffman table under/over-reads.* `table[0]` was read even for an empty
  table, and a chain entry with offset 0 could drive the lookup index to -1.
* *Oversized shift counts.* A corrupt image-side `bitCount` (or a huffman
  value decoding to a field width past 31) pushed shift counts beyond the
  operand width — UB in C. The port uses wrapping (count-masked) shifts,
  which is what the C binary does on the hardware this VM ships on.

**No partial writes on failure.** The C decoded straight into the image's
coefficient WordArray, so a failing `primitiveDecodeMCU` left a partially
written block behind (the stream object itself was never stored back, so the
Smalltalk fallback re-decodes and overwrites it — invisible in practice, but
different). This port validates and decodes first, and only a successful
decode touches the array, the stream and the DC predictor.

**Immutability is honoured.** The C wrote through `firstIndexableField`
regardless; the SDK's write path fails cleanly on an immutable target.

**Aliasing corner.** Because inputs are copied out and outputs written back
once, a call that passes the *same* object in two roles (e.g. the bits array
as the residuals array) sees one final state rather than the C's interleaved
writes. The image-side decoder never does this.

## Verification

`cargo test -p jpeg-reader-plugin` — 38 tests over the VM-free core:

* **Bit stream** (7): MSB-first extraction across byte boundaries, `FF 00`
  unstuffing, marker push-back, read-limit enforcement, resuming from stored
  bit state, starvation answering -1, and the load-time validity checks.
* **Huffman** (12): flat and chained tables, JPEG EXTEND sign-extension,
  DC delta accumulation across blocks, zig-zag (natural-order) coefficient
  placement, ZRL runs, EOB, and the failure paths (bad table, starved
  stream, coefficient index past the block).
* **IDCT** (6): all-zero coefficients → flat 127 block; DC-only block →
  exact flat value; mixed and single-AC blocks against a textbook float
  DCT-III reference within +/-2; saturation at 0 and 255; and
  divide-toward-zero descaling (the C divides rather than shifts).
* **Colour conversion** (10): cursor walk across blocks and rows, 2x2
  upsampling, hand-computed fixed-point YCbCr → RGB pixels, clamp-then-
  floor-at-1 per channel, dither residual carry, the grayscale path's
  *missing* zero-clamp (a negative sample masks to a high value — faithful),
  subsampled chroma pacing, and the out-of-range/zero-scale failures.

## Not verified

No VM runs in the porting environment, so the following await the image-side
differential pass:

* **Proxy plumbing**: stack access, the `isWords` / `storeIntegerofObjectwithValue`
  raw-proxy calls, `write_words` into live WordArrays, and module load.
* **Behaviour under the real image-side decoder** (`JPEGReadStream` and
  friends driving all four primitives over an actual JPEG corpus).
* **32-bit VMs**: the port follows `sqInt = isize`, matching the C's
  register-width types, but the UB-corner shift/comparison semantics were
  reasoned out for 64-bit targets; and `storeIntegerofObjectwithValue` of a
  bit buffer near the SmallInteger limit could fail on 32-bit — after the
  stack answer, exactly as in the C. Untested either way.
* **Interrupted-decode stream states** produced by a real progressive/
  restart-marker stream (the tests hand-build stream states instead).

## Not done yet

* **CMake wiring.** The crate builds and exports the same symbol set as the
  C plugin (verified with `nm`: the four primitives, their accessor-depth
  bytes with the C's values 2/3/2/1, `getModuleName`, `setInterpreter`), but
  the build still compiles the C plugin. Switching over is deliberately a
  separate, reviewable change — as with jpeg-plugin.
