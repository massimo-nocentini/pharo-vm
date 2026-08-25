# SurfacePlugin, in Rust

Replaces both halves of the C plugin: the surface registry that VMMaker
generates from `smalltalksrc/VMMaker/SurfacePlugin.class.st` (the generated C
is not checked into this tree; the Slang is the spec this port was written
against) and the hand-written manual-surface support in
`plugins/SurfacePlugin/src/common/sqManualSurface.c` (~167 lines).

This plugin is mostly an API for *other plugins*, not for the image.
BitBltPlugin and the display code fetch its entry points by name through
`ioLoadFunctionFrom(..., "SurfacePlugin")`, so the crate's whole point is its
exported C surface:

| exported for other plugins / the VM | |
|---|---|
| `ioRegisterSurface` / `ioUnregisterSurface` / `ioFindSurface` | the registry |
| `ioGetSurfaceFormat` / `ioLockSurface` / `ioUnlockSurface` / `ioShowSurface` | the dispatchers |
| `createManualSurface` / `destroyManualSurface` / `setManualSurfacePointer` | the manual-surface backend |

All with the exact signatures from
`plugins/SurfacePlugin/include/common/SurfacePlugin.h`, including a
`repr(C)` `sqSurfaceDispatch` that is field-for-field the C struct, with every
function slot nullable as the original tolerates.

## The contract is unchanged

Same module name, same six primitives (`primitiveCreateManualSurface`,
`primitiveDestroyManualSurface`, `primitiveSetManualSurfacePointer`,
`primitiveFindSurface`, `primitiveRegisterSurface`,
`primitiveUnregisterSurface`), same `initialiseModule`/`shutdownModule`
behaviour (shutdown refuses while any surface is registered).

Surface **ID allocation is replicated exactly**, because image-side code holds
on to IDs: the array grows by `maxSurfaces * 2 + 10` when full (so the first
registration creates 10 slots and answers ID 0), freed slots are reused lowest
index first, other IDs never move, and a stale ID whose slot was reused
resolves to the new occupant — as in C. The registry's odder corners are kept
too: the version gate is the C's literal `major != 1 && minor != 0`
conjunction (a 2.0 dispatch table is accepted, a 2.1 table is not), and
`destroyManualSurface` is exactly `ioUnregisterSurface`, so it "destroys"
whatever surface ID it is handed.

## What changed underneath

Each of these removes undefined behaviour; none changes what a well-behaved
caller observes.

* **The off-by-one bounds check.** Every C lookup admits
  `surfaceID == maxSurfaces` (`surfaceID > maxSurfaces` instead of `>=`) and
  reads one `SqueakSurface` past the array. That ID is never handed out, and
  is rejected here.
* **The `surfaceArray[-1]` write.** If the reuse scan found no free slot the C
  left its index at -1 and wrote before the array. Unreachable in practice
  (`numSurfaces < maxSurfaces` implies a free slot); a clean failure here.
* **`primitiveFindSurface`'s holder write.** The C wrote
  `sizeof(sqIntptr_t)` bytes through `firstIndexableField` with no size check
  — its own comment says "ByteArray(4)", one word short on 64-bit. The write
  is bounds-checked here, so an undersized (or immutable) holder is a clean
  primitive failure instead of heap corruption. Likewise the C dereferenced
  the `surfaceID` / `surfaceHandle` out-pointers unconditionally; a null one
  now answers false instead of crashing.
* **`setManualSurfacePointer` type confusion.** The C looked the ID up with a
  NULL dispatch filter and cast whatever handle came back to
  `ManualSurface*`; handed the ID of a *non-manual* surface it wrote through a
  reinterpreted foreign handle. The lookup here filters on the manual dispatch
  table, so that case answers false.
* **`int` overflow in the pitch check.** `createManualSurface`'s
  `rowPitch < (width*depth)/8` overflows `int` for large widths (UB in C);
  the comparison is widened to i64.
* **`shutdownModule` dangling state.** The C freed `surfaceArray` but left the
  pointer and `maxSurfaces` behind; a call between shutdown and the next
  `initialiseModule` used freed memory. The registry is fully reset here.
* **Dropped:** the C's `logTrace` calls (manual-surface lock/unlock/format
  tracing). Nothing else consumes them.

Kept deliberately, oddness and all: the manual-surface record is **never
freed** — the C's `destroyManualSurface` cannot prove the slot still holds its
own record rather than a reused ID's foreign surface, so it leaks the ~40-byte
struct, and so does this port; and a failed re-lock leaves the surface locked
(the C sets `isLocked` before testing it and never clears it on that path).

The registry is global state touched only from the interpreter thread; it
lives in a `Mutex<Registry>` (the safe way to own a mutable static, per the
SDK's `INTERP` precedent), and client dispatch functions are always invoked
*after* the lock is released, so a surface implementation that re-enters the
registry — legal under C's bare globals — cannot deadlock.

## Accessor depths

The Slang-generated C exports one per primitive, and is not in this tree to
read them from. The values used (create 0, destroy 0, setPointer 1, find 1,
register 1, unregister 0 → see `src/lib.rs`) were derived from each
primitive's accessor chains — 0 where only stack values and immediates are
read, 1 where an argument's contents are reached — and the three
manual-surface primitives match the values in OpenSmalltalk's generated
`SurfacePlugin.c`. Confirming the three Pharo-only primitives against a
freshly generated `SurfacePlugin.c` belongs to the differential pass; a value
erring high only costs work on the failure path.

## Verification

`cargo test -p surface-plugin`: 21 tests.

* **Registry, pure** (`src/registry.rs`): register/find/unregister sequences,
  the exact C growth schedule (0 → 10 → 30 slots), lowest-first slot reuse,
  stale-ID-resolves-to-new-occupant, the version-gate conjunction, the
  out-of-range boundaries including the C's off-by-one ID, and calls through a
  fake dispatch table verifying the registered handle and every argument
  arrive at the client's functions verbatim.
* **Manual surfaces** (`src/manual.rs`): the C's validation table and its
  ordering, the lock/unlock/set-pointer state machine (no lock without a
  buffer, no double lock, no pointer change while locked), and
  create/destroy/set-pointer driven through a registry.
* **The exported C API** (`src/lib.rs`): one sequential end-to-end scenario
  against the real process-wide registry — register, find with and without
  filter, all four dispatchers (including the unknown-surface and
  missing-function answer codes: 0 with a primitive failure vs. -1), manual
  surfaces through the same path, and the `shutdownModule` gate.

## Not verified

No VM runs in the build environment, so everything past the proxy boundary is
untested and is where the image-side differential pass should focus:

* the six primitives end to end (stack access, failure codes, what gets
  pushed) against a real image;
* interop with the *C* BitBltPlugin and display code loading `ioLockSurface`
  et al. from this library — in particular a real `ExternalForm`
  create/set-pointer/BitBlt/destroy cycle;
* `primitiveRegisterSurface`'s ExternalAddress handling
  (`fetchPointer: 0 ofObject:` on both addresses, the kind checks) against
  real FFI objects;
* the `primitiveFail()` calls on the dispatchers' error paths (exercised only
  with a null proxy in tests);
* accessor depths, as above.

## Not done yet

* **CMake wiring.** The crate builds and exports the right symbols, but the
  build still compiles the C plugin; switching over is a separate,
  reviewable change (jpeg-plugin precedent).
