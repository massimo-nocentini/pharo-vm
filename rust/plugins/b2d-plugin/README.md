# B2DPlugin, in Rust

Replaces the Slang-generated **`plugins/B2DPlugin/src/common/B2DPlugin.c`**
(VMMaker.oscog-eem.2480, ~15,200 lines) — the Balloon 2D vector-graphics
engine: an active-edge-table scanline rasterizer with fixed-point line and
quadratic-bezier stepping, solid/gradient/bitmap fills, anti-aliasing, and a
resumable state machine that hands "external" edges and fills back to the
image mid-render.

## The contract is unchanged

Same module string (`B2DPlugin VMMaker.oscog-eem.2480 (e)`), same 43
primitives with the same names, argument shapes, failure codes and accessor
depths (all read from the C's `*AccessorDepth` exports, not guessed), plus
`initialiseModule` and `moduleUnloaded`. The exported symbol set was diffed
against the C's — identical.

The engine's entire state lives in an image-side Bitmap, the **work buffer**,
addressed through fixed word offsets. That layout is ABI: every `GW*`/`GE*`/
`GB*`/`GL*`/`GF*` constant is mirrored by name in [`src/consts.rs`], and the
embedded object records (edges, fills) keep their exact sizes and type tags.
The stop-reason protocol (`GErrorNeedFlush`, `GErrorGETEntry`, ...) that the
image resumes rendering off is mirrored exactly, including the `GEF*` failure
codes (100–123) passed to `primitiveFailFor`.

BitBlt is still reached the way the C reaches it: `loadBitBltFrom` and
`copyBitsFromtoat` are fetched at runtime with `ioLoadFunctionFrom` from the
module named by `primitiveSetBitBltPlugin` (default `BitBltPlugin`), cached,
and dropped again when `moduleUnloaded` names that module. As in the C,
`initialiseModule` answers false — and the VM rejects the plugin — when no
BitBlt provider resolves.

## How the port is organized

The C keeps everything in file statics: five raw pointers into image memory
(`workBuffer`, `objBuffer`, `getBuffer`, `aetBuffer`, `spanBuffer`) plus
`objUsed`/`engineStopped`. All of it is re-derived from the engine oop at the
start of every primitive, so this port rebuilds it per call as an
[`engine::Engine`] value. The moving pointers (`allocateGETEntry` does
`aetBuffer += n`) became word *offsets* into the work buffer, which keeps
every access bounds-checked.

Function names and structure follow the C (`aaFirstPixelFromto`,
`stepToFirstWideBezierInat`, ...) so the two read side by side; where Slang
inlined a helper's body (`/* begin foo */`), this port calls the helper — the
bodies are identical. Local-variable widths mirror the C's declarations:
`sqInt` math is 64-bit, `int` locals are `i32` with wrapping arithmetic (the
fixed-point stepping and the radial `ds`/`dt` counters rely on 32-bit
wraparound), and the AA mask expressions reproduce the sign-/zero-extensions
the C's mixed `int`/`unsigned` arithmetic produces on a 64-bit build.

Everything the rasterizer needs from outside the two buffers goes through the
[`engine::Host`] trait — form bits for bitmap fills, the BitBlt blit, and
`ioMicroMSecs` for the profiling counters. The VM-backed implementation lives
in `lib.rs`; the tests provide their own, which is what makes whole renders
testable without a VM.

## What changed underneath

Only memory safety and the removal of undefined behaviour; each is listed.

* **Out-of-bounds work/span-buffer accesses are gone.** The C dereferences
  whatever index a (possibly image-corrupted) work buffer implies. Here every
  access is checked; a violation panics, which the primitive wrapper turns
  into a clean primitive failure (in a `panic = "abort"` build it aborts
  instead of corrupting the heap). The same applies to using a primitive that
  needs the span buffer when the image never supplied one — the C would chase
  a stale pointer from an earlier call.
* **`toggleWideFillOf:` on an external wide edge** (type tags 1/3): the C's
  switch dispatch falls through and reads the stale `dispatchReturnValue`
  static — an unspecified leftover value. This port uses 0 there. (The two
  statics `dispatchedValue`/`dispatchReturnValue` are otherwise plain locals
  now; they never carried state between calls on the reachable paths.)
* **`transformColor:` with alpha 0 under a color transform** divides by zero
  into a NaN/inf and then does C's undefined `double`→`int` conversion;
  Rust's `as` saturates (NaN → 0). Same for enormous Float coordinates in
  `loadPoint:from:` and degenerate fill orientations. Defined output, same
  inputs.
* **ShortPointArray halfword order is fixed little-endian** (`short_at` takes
  the low half of the word first). The C inherits the host's byte order; this
  VM's targets are little-endian, so behaviour is identical there.
* **Dead Slang output was not ported**: `drawWideEdge:from:`,
  `findNextAETEdgeFrom:`, `adjustAALevel`, `estimatedLengthOf:with:`,
  `incrementPoint:by:`, `squaredLengthOf:with:`, `objectHeaderOf:`, the
  no-argument `stepToFirst*`/`stepToNext{Bezier,Line,WideLine}` wrappers, the
  `fillBitmapSpan`/`fillLinearGradient`/`fillRadialGradient` no-argument
  wrappers, `allocateAETEntry:`/`allocateStackEntry:`/`allocateStackFillEntry`
  and the `shortRun*At:from:` helpers exist in the C only as unreferenced
  functions (verified by call-site count).

Deliberately **kept** oddities, because the C has them: the `x0 == y0 &&
x1 == y1` line test in `loadArrayShape...` (looks like it meant
`x0 == x1 && y0 == y1`); the `>` (not `>=`) bound in `loadBitsFrom:`'s
form-array check (the following fetch goes through the VM's own accessor
either way); `findNextExternalFillFromAET` discarding `fillAllFrom:to:`'s
"external fill" answer, so it never reports one (Slang's inlining does the
same — the external-fill pause only ever happens via the recorded
`lastExportedFill` state); `primitiveRegisterExternalFill` allocating
`GEBaseEdgeSize` slots but advancing `objUsed` by `GEBaseFillSize`; and
`primitiveAddPolygon` failing with a bare `primitiveFail` where its sibling
uses `GEFWorkTooBig`.

## Verification

`cargo test -p b2d-plugin` — 22 tests, no VM required:

* **Work buffer**: `primitiveInitializeBuffer`'s exact header layout; the
  `GEFWorkBuffer*` validation codes; allocation exhaustion setting the
  `GErrorNoMoreSpace` stop reason.
* **Fixed-point machinery**: Bresenham setup and stepping vectors (including
  the error-adjust bump and mid-start catch-up), degenerate-bezier forward
  differencing tracking its chord exactly, height-triggered subdivision,
  `absoluteSquared8Dot24`, the AA mask family for levels 1/2/4,
  `transformWidth`, and the `GErrorNeedFlush` stop from a translucent color
  with a flush pending.
* **AET/fill stack**: insertion order (x, then the y/x tiebreak), and the
  depth-sorted show/hide/top protocol with its 32-bit cell truncation.
* **Whole tiny renders**, driven through the same `proceedRendering*` state
  machine the primitives use, asserting every framebuffer pixel: a filled
  rectangle, a triangle from three lines, and the same triangle with a bezier
  hypotenuse (pixel-identical to the line version).
* **Fills**: linear gradient stepping one ramp entry per pixel with the
  exported-fill words recorded; the radial gradient's decreasing+increasing
  parts against a hand-executed trace of the C's arithmetic; a tiled 32-bit
  bitmap fill including the forced-alpha rule; `primitiveMergeFillFrom`'s
  span merge; and the `GErrorGETEntry` stop for an external edge.

## Not verified (needs the image-side differential pass)

* **Everything oop-facing**: the 43 primitives' stack discipline, argument
  validation order, and `pop`/`popthenPush` bookkeeping run only against the
  real VM; here they are faithful transcriptions but untested.
* **BitBlt interaction**: `ioLoadFunctionFrom` resolution, the implicit
  re-load, `primitiveSetBitBltPlugin`, `moduleUnloaded`.
* **Anti-aliased rendering** (AA levels 2/4): the mask/shift setup is tested,
  the AA span loops are not exercised end to end.
* **Wide lines and wide beziers** (brush simulation, entry/exit fill
  validation) and compressed shapes with real run-length data.
* **The external edge/fill resume protocol end to end**
  (`primitiveNext*`/`primitiveAdd*ActiveEdgeEntry`/`primitiveMergeFillFrom`
  round trips), `primitiveCopyBuffer`, `primitiveGetClipRect`'s remappable-oop
  dance, and color-transformed rendering.
* **CMake wiring.** As with jpeg-plugin, the build still compiles the C
  plugin; switching the build over is a separate, reviewable change.
