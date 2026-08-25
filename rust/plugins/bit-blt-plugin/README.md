# BitBltPlugin, in Rust

Replaces `plugins/BitBltPlugin/src/common/BitBltPlugin.c` — the VM's 2D
blitter, ~8,100 lines of Slang-generated C from `BitBltSimulation`
(VMMaker.oscog-eem.2493) — with a function-for-function Rust port behind the
same exports: `primitiveCopyBits`, `primitiveWarpBits`, `primitiveDrawLoop`,
`primitiveDisplayString`, `primitivePixelValueAt`, `primitiveCompareColors`,
plus the `copyBits` / `copyBitsFromtoat` / `loadBitBltFrom` entry points the
Balloon engine calls directly, `moduleUnloaded` for the module loader, and
the same seven `*AccessorDepth` bytes. `nm` on both libraries shows the same
defined-symbol list.

The port keeps the C's names and structure so the two read side by side:

| module | C counterpart |
|---|---|
| `state.rs` | the file-scope statics, `maskTable`, `dither8Lookup`, `default8To32Table` |
| `rules.rs` | the ~40 `opTable` merge functions and partitioned-word helpers |
| `engine.rs` | `clipRange` … `copyLoop` / `copyLoopNoSource` / `copyLoopPixMap`, the rule-34/41 fast paths |
| `warp.rs` | `warpLoop`'s pixel half, `warpPickSourcePixels` / `warpPickSmoothPixels` |
| `load.rs` | `loadBitBltFrom:warping:`, color map / halftone loading, SurfacePlugin lock/unlock, `drawLoopX:Y:` |
| `lib.rs` | the primitives and the exported C API |

**The ARM SIMD fast paths are not ported.** `BitBltArm*` /
`BitBltDispatch.c` (`ENABLE_FAST_BLT`) are an optional acceleration layer
over the generic paths; the generic C is complete without them and is what
the Pharo build compiles. `primitiveCompareColors` is only functional under
`ENABLE_FAST_BLT`, so — exactly as in the C as built — it validates its
arguments and then fails.

## What changed underneath

* **The state is a struct, not globals.** All the C's file statics live in
  one `BitBlt` value behind a mutex (the VM calls primitives from a single
  thread; the lock just makes the global sound Rust). The C's
  function-local static shift/mask tables from `setupColorMasksFrom:to:`
  became fields reached through a small enum instead of self-referential
  pointers.
* **SurfacePlugin is still resolved at runtime** through the proxy's
  `ioLoadFunctionFrom("ioGetSurfaceFormat"/"ioLockSurface"/"ioUnlockSurface",
  "SurfacePlugin")`, cached, and dropped again when `moduleUnloaded`
  announces that plugin's departure — the exact C mechanism.
* **The C's failure-flag control flow is preserved.** The engine talks to
  the proxy through thin raw wrappers (`vmcalls.rs`) and consults `failed()`
  at the same points the generated code does, rather than using the SDK's
  early-return accessors; a specific failure code set mid-way (e.g.
  `PrimErrObjectMoved`) reaches the image unchanged.

## Faithfully preserved oddities

These are behaviors of the shipped C that a from-scratch implementation
would not have. They are kept, verified, and worth knowing about:

* **`pixMask`/`pixPaint` at 32bpp on LP64.** `partitionedAND`'s mask
  variable is `sqInt`, so `maskTable[32]` (an `int` −1) sign-extends to
  64-bit all-ones, which a 32-bit field never equals: rule 26 answers 0 for
  every word on a 64-bit VM. Mirrored, including the different (working)
  behavior when `sqInt` is 32-bit.
* **Skewed rightward overlapping copies are not memmove-exact.** When
  source and destination are the same form on the same row with `dx > sx`
  and the copy is not word- or phase-aligned, the C's reversed `copyLoop`
  (preload plus adjusted skew) produces output that differs from a
  memcpy-through-a-snapshot — and can read one word before the first source
  row. The port reproduces the C word for word (see the golden-vector test);
  vertical, leftward, and aligned overlaps are memmove-exact in both.
* **`skew = -32` is a working configuration.** The C asserts
  `-31 <= skew <= 31`, but production builds compile asserts out and
  word-aligned reversed overlaps reach −32, where the LP64 64-bit shifts
  make the rotate degenerate exactly right. The port's shift helpers encode
  those semantics; its `debug_assert` allows the reachable range.
* **`rgbComponentAlpha8` tests `srcShift == 32`** where its sibling
  `alphaSourceBlendBits8` tests 24 (the LSB destination advance) — kept as
  compiled.
* **`lockSurfaces`' warping branches** lock the overlap rectangle when
  warping and the whole surface otherwise; the generated comments say the
  opposite of the code. The code is what ships, so the code is what is
  ported.
* **Rule 41's 16bpp repack keeps the alpha bit**: `rgbMap:from:to:` is
  called with whole-pixel widths (32→16), which truncates 16-bit halves
  rather than repacking 5-bit channels.

## Deliberate divergences (undefined behavior removed)

Each replaces a C crash or UB with a clean primitive failure or a defined
result; none changes behavior on inputs the C survives:

* A Form depth of 0, or outside 1..=32, fails the load / the pixel-value
  primitive instead of dividing by zero (`32 / depth`, pitch computation).
* A halftone form with a non-positive height fails the load instead of
  `y % 0` in the copy loops.
* A WarpBlt smoothing count below 1 fails the primitive instead of dividing
  by zero in `warpPickSmoothPixels`.
* Bitmap-size checks (`pitch * height`) are computed in 64-bit instead of
  overflowable `int` arithmetic.
* All 32-bit shifts go through helpers that answer 0 for out-of-range
  counts — which is precisely what the C's LP64 `usqInt` shift-then-truncate
  produces, so this is a definition of existing behavior, not a change.
  `1 << 32` in the warp source-map size check keeps the x86/ARM masked-shift
  result (1), as the shipped binaries do.

## Verification

There is no VM in the porting environment, so the engine was verified
against the **real generated C**, compiled standalone from
`plugins/BitBltPlugin/src/common/BitBltPlugin.c` with stub headers and
driven through its own internal functions (`tests/harness/`):

* **400 generated copy scenarios** — all 35 loadable combination rules,
  every source/dest depth pair (1/2/4/8/16/32), MSB and LSB, with and
  without an indexed color map, a halftone, and a source — hashed bitmap for
  bitmap against the C. All match.
* **150 generated WarpBlt scenarios** — both smoothing modes, all depth
  pairs, off-source sampling, x/y clipping advance, indexed maps, LSB
  destinations — against the C's own `warpLoop` run over a faked
  interpreter. All match.
* **Golden overlap vectors** for ten same-form copies, including the
  non-memmove reversed cases, word for word against the C.
* Independent unit tests (not derived from the C): per-pixel reference
  blits across alignments, widths and depths for the same-depth and
  depth-converting loops; hand-derived word vectors for boundary masks and
  skew; every combination rule against its arithmetic definition
  (saturating add, absolute difference, ceil-blends, tallies into the map,
  dither table values); `clipRange` geometry; warp identity / 2× scale /
  box-filter smoothing / pixel extraction at every depth.

`cargo test -p bit-blt-plugin` runs 60 tests; `cargo clippy --all-targets
-- -D warnings` is clean.

### Not verified (needs an image-side differential pass)

* Everything that talks to the interpreter: `loadBitBltFrom:warping:`'s
  fetch/validation sequence, the failure-code plumbing, `copyBitsRule41Test`
  argument fetching, `primitiveDisplayString`'s glyph loop,
  `primitiveDrawLoop`, `primitivePixelValueAt`, `primitiveCompareColors`,
  and the GC-count / form-reload interplay (`statNumGCs`,
  `reloadDestAndSourceForms`).
* OS-surface locking end to end (needs SurfacePlugin and a display); only
  the call sequence was ported by inspection. This includes the C's
  unlock-skipping failure paths in `primitiveDisplayString`, kept as-is.
* The Balloon engine driving `loadBitBltFrom` / `copyBits` /
  `copyBitsFromtoat` across calls.
* Behavior on a 32-bit VM (`sqInt` = 32 bits): the port compiles and the
  sqInt-width arithmetic follows the platform, but all differential vectors
  were captured on LP64.

## Not done yet

* **CMake wiring.** The build still compiles the C plugin; switching over
  means adding this crate to `cmake/rust.cmake` and dropping `BitBltPlugin`
  from the C plugin list — left as a separate, reviewable change.
