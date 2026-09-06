# The Rust port of the platform layer

`rust/Cargo.toml` and `rust/README.md` both point here. This file records where
the migration has got to and how the last wave was done; `rust/README.md` is
still the place for the build integration and the house rules.

## Scope, restated

The interpreter, the garbage collector and the JIT are **not** part of this.
They are generated from Slang under `smalltalksrc/` — `cointerp.c` is 91,406
lines and contains the GC — and hand-porting them would fork the VM off VMMaker
and lose the simulator. What is being migrated is the hand-written C platform
layer in `src/`.

## Where the layer stands

**4,129 lines ported** across twelve waves. Every wave holds to the same rule:
`abi-check.sh` proves the exported symbol set did not move, and the two builds
are diffed at run time.

| Wave | File | Verified by |
|---|---|---|
| 0–1 | `errorCode.c`, `stringUtilities.c`, `parameterVector.c`, `pathUtilities.c` | (earlier work) |
| 2 | `imageAccess.c` | 54 MB `Smalltalk saveAs:` through the chunked write path |
| 3 | `externalPrimitives.c` | plugin loading, 20 loader trace lines identical |
| 4 | `sqHeapMap.c` | unit tests over the two-level bitmap |
| 5 | `pharoSemaphore.c`, `platformSemaphore.c` | threaded test blocking a real waiter and waking it |
| 6 | `threadSafeQueue.c` | 4 consumers draining 1,000 elements, each delivered once |
| 7 | `memoryUnix.c` | all four mmap addresses identical **without** normalisation |
| 8 | `sqNamedPrims.c` | 136 of 285 trace lines from this file, byte-identical |
| 9 | `parameters.c` | `--help` identical, all 2,438 bytes; 10 command-line shapes |
| 10 | `client.c` | 7 startup shapes, including the errno text on a missing image |
| 11 | `sqExternalSemaphores.c` | 200 cross-thread delay wakeups |
| 12 | `sqVirtualMachine.c` | 19 primitive families through the proxy |

Still C:

- **`utils.c`** (575) — needs splitting first. `isCFramePointerInUse` reads the
  JIT's captured `CStackPointer`/`CFramePointer` and calls
  `ceCaptureCStackPointers`; the result depends on the caller's frame layout,
  so moving it to Rust codegen risks silently breaking the JIT's stack
  handling. The rest of the file is ordinary string and path work.
- **`debug.c`** (167) — permanent C. It defines `logMessage`, which is
  variadic, and stable Rust cannot define a variadic `extern "C"` function.
- **`tty.c`** (71) — built as a separate library, so it needs a different
  integration path from `RUST_REPLACED_C_SOURCES`.
- **The signal-handling tier** (1,921) — `sqTicker.c`, `aio.c`, `heartbeat.c`,
  `debugUnix.c`. This is where the async-signal-safety rule bites hardest and
  where a port can most easily be subtly wrong.
- **`ffi/`** (1,532) — the `sigsetjmp` trampolines `rust/README.md` already
  flags as likely permanent C.

## The plugin layer: all sixteen plugins have Rust replacements

Distinct from the platform waves above, every C plugin under `plugins/` now
has a native-Rust replacement built on the `pharo-vm-plugin` SDK, following
the jpeg-plugin precedent: same module name, same primitive names, same
accessor depths, same failure behaviour, one crate per plugin under
`rust/plugins/` (UUIDPlugin's lives in `rust/examples/uuid-plugin`).
`USE_RUST_PLUGINS=ON` switches them in; the default build is still all-C.

| Plugin | Crate | Standalone verification |
|---|---|---|
| B2DPlugin | `b2d-plugin` | 22 tests; whole tiny renders asserted per-pixel |
| BitBltPlugin | `bit-blt-plugin` | 60 tests; **550 differential scenarios against the compiled C** |
| DSAPrims | `dsa-prims` | 19 tests; SHA-1 KATs, u128 cross-checks |
| FileAttributesPlugin | `file-attributes-plugin` | 42 tests; `std::fs` as independent witness |
| FilePlugin | `file-plugin` | 35 tests; tempdir ops, record-layout pin |
| JPEGReaderPlugin | `jpeg-reader-plugin` | 38 tests; float-DCT reference for the IDCT |
| JPEGReadWriter2Plugin | `jpeg-plugin` | 90-case corpus diff against the C (earlier work) |
| LargeIntegers | `large-integers` | 44 tests; schoolbook + egcd references, ~3000 random division cases |
| LocalePlugin | `locale-plugin` | 17 tests; locale-string parsing byte-for-byte |
| MiscPrimitivePlugin | `misc-primitive-plugin` | 28 tests; every compression token kind |
| NewFilePlugin | `new-file-plugin` | 28 tests; 5 GB offsets, full open-mode matrix |
| SocketPlugin | `socket-plugin` | 32 tests; real TCP/UDP over loopback through the aio path |
| SqueakSSL | `squeak-ssl` | 17 tests; live in-process TLS handshake |
| SurfacePlugin | `surface-plugin` | 21 tests; registry sequences, dispatch ABI |
| UnixOSProcessPlugin | `unix-os-process-plugin` | 19 tests; real fork/exec, signal→semaphore |
| UUIDPlugin | `uuid-plugin` | proven against a live image (earlier work) |

Every crate passes `cargo build`, `cargo test` and `cargo clippy --all-targets
-- -D warnings`, and each README documents its divergences (memory-safety and
UB removal only — C bugs with observable image-side behaviour are reproduced
and pinned by tests) and its **Not verified** list.

What the standalone tests cannot cover is owed to an image-side differential
pass — the same discipline as the platform waves: build with
`USE_RUST_PLUGINS=OFF` and `ON`, run the image test suite against both, diff.
That pass has not run yet; the per-crate "Not verified" sections say exactly
where it should look. Windows keeps the C plugins throughout, and Apple keeps
the C for the eight POSIX-facing ports (`cmake/rust.cmake` guards the list).
Two deliberate scope cuts: BitBlt's optional ARM SIMD fast paths are not
ported (the generic paths are complete, so ARM loses an acceleration, not a
capability), and SqueakSSL defaults to a vendored, statically linked
OpenSSL 3 (`--no-default-features` restores the C plugin's system linkage).

### Working on the objects, not on copies of them

The first plugins were written to a conservative rule -- read every argument
into a Rust buffer, compute, write the result back -- because it makes the
one invariant that matters easy to see: no borrow of image memory spans an
allocation. LargeIntegers was the first crate to separate that invariant from
the buffers (a magnitude is what *computing* needs, and the primitives that
compute nothing never build one); this pass carried the same reading through
the rest.

The SDK gained the missing half of the story. `bytes_of` / `words_of` already
borrowed an argument's contents in place; `with_bytes_mut` / `with_words_mut`
now scope a `&mut` slice to a closure for the write path, which
`Interp::write_bytes` had deliberately refused to hand out because two live
views of one object would alias -- and the image can pass one object as two
arguments whenever it likes. A thread-local table of the ranges currently lent
out settles that: while a view is live, every other reader and writer of those
bytes fails with `PrimErrInappropriate` instead of aliasing it. Sixteen tests
in `pharo-vm-plugin/tests/inplace.rs` drive both against a fake proxy table.
`read_f64_array::<N>` (a stack array where `read_f64s` allocated a `Vec`) and
`c_string_value` (one copy instead of the `String` that
`CString::new(vm.string_value(oop)?)` puts in between) came with it.

Where that landed, per crate: JPEGReaderPlugin reads the huffman tables and
coefficient blocks where they lie and converts MCUs straight into the
destination bitmap; MiscPrimitivePlugin translates, converts, compresses and
decompresses in place (its decompressor also no longer allocates a vector per
run); SqueakSSL hands OpenSSL the destination ByteArray as the C did;
JPEGReadWriter2Plugin packs each row into the Form's own words; DSAPrims stages
only what it writes; FileAttributesPlugin, NewFilePlugin and SocketPlugin drop
copies that had no allocation to survive. The rest were already right --
BitBltPlugin and B2DPlugin work through raw pointers as the C does, and the
copies left in FilePlugin, LocalePlugin and UnixOSProcessPlugin outlive an
allocation or a `free` and have to.

Each crate's README says which of its copies remain and why. The aliasing
cases that now fail cleanly are the ones the C decoded, compressed or
encrypted out of a buffer it was writing; each is noted as a divergence.

### The plugins unwind; the platform layer aborts

For most of the port there was one cargo workspace and one answer to "what
happens on a panic": `rust/Cargo.toml` set `panic = "abort"` in both profiles,
on the reasoning that a panic must never unwind across `extern "C"` into the
generated interpreter.

That reasoning is right for the platform layer and wrong for the plugins, and
the single workspace made it impossible to say so. Cargo reads `[profile]`
only from a workspace **root** -- a `[profile.release]` in a member crate is
ignored with a warning -- so the strategy could not be stated per crate. The
consequence was that the SDK's headline promise, "every primitive body runs in
`catch_unwind`, so a panic becomes a clean primitive failure", was **false for
every plugin that shipped**: under abort the process is gone before the catch
can run, and an `unwrap()` on `None` inside BitBlt took the user's image with
it. The catch, and the two more in the `pharo_plugin!` macro's module hooks,
were dead code.

The plugin crates therefore moved to a workspace of their own,
`rust/plugins/Cargo.toml`, which sets `panic = "unwind"`; the four crates in
`rust/Cargo.toml` -- `pharo-vm-sys`, `pharo-platform`, `pharo-vm-plugin`,
`pharo-vm-plugin-macros` -- keep `panic = "abort"`. Both example plugins joined
the plugin workspace with a `workspace = "../../plugins"` line rather than
moving on disk; one of them, `uuid-plugin`, actually ships. `cmake/rust.cmake`
now makes one `corrosion_import_crate` call per manifest, each guarded on a
non-empty `CRATES` list -- an empty list omits the keyword and makes corrosion
import *every* package in the manifest, which was already latent in the single
import.

`pharo-platform` must keep aborting, and that is the point of the split rather
than an exception to it: it is linked into `libPharoVMCore`, and its
`#[no_mangle]` functions are reached from the generated interpreter, from the
heartbeat at real-time priority, and from signal handlers, with nothing
catching above them.

**Nothing that aborted before this change stops aborting.** Since Rust 1.71 a
non-`-unwind` `extern "C"` function inserts an abort-on-unwind guard at its
boundary, and the pinned `rust-version` is 1.77, so every unfenced entry point
keeps its old behaviour with no code:

* the `sqSurfaceDispatch` slots in `surface-plugin`, called through a
  function-pointer table by BitBlt and B2D, mid-blit, with a surface locked;
* the aio handlers in `socket-plugin`, registered with the VM's C poll loop
  and answering `()`, with no failure channel to fail through;
* the signal handlers and the `atexit` hook in `unix-os-process-plugin` --
  unwinding out of a signal frame corrupts the interrupted context;
* the `sigsetjmp`/`siglongjmp` pair in `src/ffi/sameThread/`, which is C, stays
  C, and is never crossed by an unwind because `run_primitive`'s catch stops it
  far below the `jmp_buf`.

What changes is that the catches already written become live: BitBlt's four
`extern "C"` entry points, every `cairoPlugin*_v1` bridge function that Pango
`dlsym`s, and the macro's `initialiseModule` / `shutdownModule` arms. So does
the `Drop` on `interp.rs`'s lending table, whose own comment had anticipated
the unwind path that could not happen yet.

#### Poison: a panic mid-mutation is not a mere failure

Turning a panic into a primitive failure is right for a computation and wrong
for a torn invariant. `handles::Registry` holds its mutex across the caller's
closure, so a panic in there leaves a slot half-written -- and `lock()` used to
swallow the resulting `PoisonError` with `PoisonError::into_inner`, justifying
it in a comment with the very promise that abort made vacuous. Under unwind
that swallow would have turned "die on a broken invariant" into "carry on with
one", which is strictly worse than aborting.

Two mechanisms ship with the profile change, in the same commit:

1. `Registry::lock` honours std's mutex poison instead of recovering from it.
   A `Mutex` is poisoned precisely when a guard is dropped during an unwind, so
   this is exact and per-registry: every later call answers
   `PrimErr::Unsupported`, the queries answer their fail-closed values, and
   `drain` answers nothing -- releasing a `cairo_t *` read out of a half-written
   slot would be a double free, and leaking at teardown costs nothing.
2. A module-wide flag in `pharo_vm_plugin::poison`, set from a
   `std::panic::set_hook` installed by `setInterpreter` (the one entry point
   the VM calls for every plugin, before anything else). `run_primitive` reads
   it and fails fast, so state that is *not* behind a registry mutex is covered
   too. The hook poisons only when a thread-local `Section` depth says a
   critical section was open -- the hook runs before the unwind, while the
   guard is still held, which is the only moment that distinction can be drawn.
   A bad-argument panic deep in a computation therefore stays an ordinary
   failure.

The flag is per-dlopened-cdylib, which is the right granularity and the only
one a `set_hook` can reach: each cdylib statically links its own libstd and its
own copy of the SDK, so one plugin disabling itself leaves the other fifteen
alone. The failure code is `Unsupported` (7), never `NoMemory`:
`StackInterpreter >> retryPrimitiveOnFailure` re-dispatches an external
primitive that failed with `PrimErrNoMemory` after a scavenge and again after a
full GC, so a poisoned module answering `NoMemory` would provoke two
collections per call.

There is no un-poisoning. Nothing in the image can re-establish an invariant it
cannot see -- the torn state is a half-inserted pointer in a slot table -- so
the module fails for the life of the process, with one diagnostic line at the
moment of poisoning. `shutdownModule` still runs, so the VM can unload cleanly.

The one place this can regress is shared mutable state the SDK does not own:
`bit-blt-plugin`'s `state()`, `b2d-plugin`'s `with_globals`, and the dlopen
function tables in cairo/pango/sdl3 are invisible to the `Section` gate, so a
panic mid-mutation there becomes a failure over torn state where it used to
abort. `Section::enter()` is public precisely so that wrapping such a site is
two lines; doing it is the follow-up.

## Beyond the C plugins: bindings for libraries no C plugin wrapped

Everything above replaces something. These three do not.

Cairo and SDL are the third-party libraries the VM *ships without using*:
`cmake/importCairo.cmake` and `cmake/importSDL2.cmake` download ready-made
binaries into the directory beside the executable, and nothing in `src/`,
`plugins/` or `include/` mentions either — the image reaches them through UFFI
(Athens-Cairo, OSWindow-SDL2). Pango is a step further out: the VM neither uses
it nor ships it, and it is bound here as a system library. Three new crates make
these libraries reachable as named primitives instead, with the plugin owning
the objects and the image holding integer handles.

| Plugin | Crate | Surface | Standalone verification |
|---|---|---|---|
| CairoPlugin | `cairo-plugin` | 108 primitives over 103 `cairo_*` entry points | 22 tests; run against the bundle's own `cairo-1.17.4` binary, all entry points resolved, drawing asserted per-pixel |
| SDL3Plugin | `sdl3-plugin` | 63 primitives over 52 `SDL_*` entry points | 26 tests; run against SDL3 `release-3.4.10`, all entry points resolved, display path driven headless, struct offsets checked against a C `offsetof` probe |
| PangoPlugin | `pango-plugin` | 184 primitives over 264 `pango_*` and 8 `g_*` entry points | 74 tests; run against a system Pango 1.58.2, all entry points resolved, signatures round-tripped through the real library, skipping when no Pango is installed |

Three things distinguish them from the ports above.

**Nothing is linked, and no download rule changed.** All three crates
`dlopen`. For Cairo and SDL that means whatever the existing CMake fetches, and
it is not a preference: the downloaded zips hold runtime objects only — no
headers, no `.pc` file — so there is nothing to link against, and
`cairo-sys-rs`/`sdl3-sys` would add a `-dev` package requirement to a VM that
already ships the library. `pango-plugin` follows the same rule for the
opposite reason: there is no download to add, so it dlopens whatever the
machine has and would otherwise force a `libpango1.0-dev` on every builder.
The search order is the executable's directory first, then the system loader,
which is where the image's FFI looks too — with Pango adding the usual
system prefixes after those, because on macOS a bare-name `dlopen` does not
reach Homebrew and would decline on a machine that plainly has Pango.

**A missing library is a normal outcome.** `initialiseModule` answers 0 and the
VM rejects the module, so the image can tell "not available here" from "not
implemented" and stay on its FFI binding. On Linux that is the *expected* path
for SDL3 today: `importSDL2.cmake` fetches `SDL3-3.4.10` for Windows and macOS
only, and no Linux artifact exists on `files.pharo.org`. For Pango it is the
*default* path everywhere: nothing downloads it, so the plugin finds a library
only where one is installed system-wide.

**The image gets handles, not pointers.** Every Cairo, SDL and Pango object
lives in a `pharo_vm_plugin::handles::Registry` and the image holds a
SmallInteger — a `Handle<R>` — carrying **four** fields rather than a pointer:
a slot index, a generation counter, a type tag, and a session byte. A stale
pointer, which is what the FFI binding passes today, is dereferenced; each
field turns one class of stale or mistaken handle into a clean primitive
failure instead.

* The **generation** catches a handle on a destroyed resource, which would
  otherwise name whatever took the slot. `NotFound`.
* The **type tag** catches a handle from another registry *in the same shared
  library*. This was a live bug and not a hypothetical: before the tag, all
  three Cairo registries shared one encoding, so `CONTEXTS.insert` and
  `SURFACES.insert` answered the same integer for their first insert and a
  context handle passed to a surface primitive resolved — a `cairo_t *` on its
  way to `cairo_pattern_destroy`. Twelve statics were exposed (Cairo 3, Pango 6,
  SDL3 3). `BadArgument`, distinct from `NotFound` on purpose: one says "wrong
  kind of thing", the other says "that one is gone", and image fallback code
  wants to tell them apart. Tags are hand-picked through the
  `resource_tags!` macro, which proves them pairwise distinct and non-zero at
  compile time. One library is the scope a decode can be confused within,
  because `Handle::decode` compares against the `R::TAG` of a type belonging to
  the library running it — *not* because a handle cannot cross a library
  boundary. One does: `primitiveCairoCreateLayout` forwards a CairoPlugin
  context handle to `cairoPluginBorrowContext_v1`, which is safe only because
  the bridge is an entry point into the minting library. Where two plugins pick
  the same literal for different kinds — CairoPlugin and PangoPlugin both use 2
  — that bridge can be fed the wrong library's handle and resolve it; the
  `handles` module docs scope it.
* The **session byte** catches a handle the image saved in an inst var and
  replayed in a later run. It is the low byte of `getThisSessionID`, a VM global
  set once from `time(NULL) + ioMSecs()` while the image is read — so it changes
  across a snapshot resumed in a new process and does *not* change when an image
  snapshots and keeps running. `NotFound`, like a dead handle, because that is
  what it is. Detection is 255/256, not certainty: the id is time-derived, so
  two launches an exact multiple of 256 seconds apart share a byte.

The budget is the SmallInteger: 60 magnitude bits in a 64-bit image, 30 in a
32-bit one, and the split is `const fn layout(ptr_bytes)` with
`const _: () = assert!(..)` pinning **both** widths, because no 32-bit target
is installed to test against. 64-bit gets 24/20/8/8. **32-bit gets 14/12/4/0 —
no session byte and a 4-bit tag** — because the fields are paid for out of the
index and the generation, and keeping the session there would have left 2^18
mints per registry per session, which a text-rendering image exhausts in
minutes. That trades a rare false accept for a certain outage, which is the
wrong way round. So on 32-bit a handle saved across a snapshot is not detected
as stale, and a registry can mint 2^14 * (2^12 - 1) = 67,092,480 handles for the
life of the process — down from 2^14 * (2^15 - 1) = 536,854,528 — after which
`insert` answers `LimitExceeded` for that registry **permanently**. That is the
part worth reading twice: it is a lifetime budget, not a rate, because a slot
that reaches the top generation is retired rather than refilled, so releasing
resources wins none of it back and only a restart clears it. The 64-bit wall is
the same shape at 2^24 * (2^20 - 1) ≈ 1.8e13, which is out of reach.

`Handle::decode` is the only route from a bare `sqInt` to a typed handle, and
each plugin calls it in exactly one place: the dozen-odd `with_*` / `destroy_*`
accessors in its `resources.rs`. The two hundred-odd primitives above them did
not change, and Cairo's two `#[no_mangle]` bridge entry points keep their
`sqInt` parameter — the exported C ABI is pinned — and decode internally.

**What this does not close.** `surface-plugin` has its *own* hand-written
registry and is deliberately not migrated: its IDs are an array index published
through `SurfacePlugin.h` / `ioRegisterSurface` and read back out of a Form's
`bits` by BitBltPlugin, so a stale ID still resolves to the slot's new occupant,
exactly as in the C. Retagging them would break every C plugin and every image
that computes on a surface ID. The claim is therefore precise: cross-kind
confusion inside one library is impossible, and cross-session reuse is
detectable on 64-bit, **for the registries the SDK owns**.

One image-visible behaviour change falls out of the tag: `is_live` answers
`false` for a live resource of the wrong kind, so `primitiveSurfaceIsLive` on a
pattern handle now says false where it could once say true. That is the fix, not
a regression, but it is written up under *Divergences* in `handles.rs`.

Two consequences of the registries worth knowing: the Cairo plugin *keeps* a pin on
image memory a surface is drawing into when Cairo still holds a reference to
that surface (`primitiveRetainedPinCount` reports how many), and the SDL plugin
invalidates a window's renderer and that renderer's textures when the window is
destroyed, because SDL frees them without reference counting. Pango's registry
carries the same idea one step further: a font map records whether the plugin
*owns* the reference it holds, because `pango_cairo_font_map_get_default` hands
back a process-wide singleton whose neighbour `pango_cairo_font_map_new` hands
back an owned object, and unref'ing the first would break text rendering
everywhere.

`cmake/rust.cmake` carries them in `RUST_ONLY_PLUGINS` rather than
`RUST_REPLACED_PLUGINS` — there is no C plugin to skip — gated on
`FEATURE_LIB_CAIRO`, `FEATURE_LIB_SDL2` and `FEATURE_LIB_PANGO`, and built
under the same `USE_RUST_PLUGINS=ON`. The first two default ON because the flag
also switches on the download that makes the library present; `FEATURE_LIB_PANGO`
defaults **OFF**, because nothing downloads Pango and an ON default would put a
plugin in every bundle that can only decline on a machine without a system
Pango.

**None of them is usable from Pharo yet.** This is half the change: an Athens
backend, an OSWindow backend and a text backend calling these primitives
instead of UFFI still have to be written, in the image, and until they are
nothing in Pharo touches any of these plugins. Each crate's README documents
the primitive-by-primitive contract those backends have to be written against,
including the 64-byte decoded event record `SDL3Plugin` answers and the wire
shapes `PangoPlugin` uses for rectangles, matrices and log attributes.

`PangoPlugin` has one further gap the other two do not, and it is a deployment
question rather than a code one. Drawing text needs a `cairo_t *` that
`CairoPlugin` owns, so the two plugins hand one across a small versioned C ABI
— and the handshake refuses unless both plugins resolved the *same* Cairo. On a
Homebrew macOS today they do not: libpangocairo hard-references
`/opt/homebrew/opt/cairo/lib/libcairo.2.dylib` while `CairoPlugin` opens the
bundle's own copy, so every drawing primitive answers `Unsupported` and
`primitiveCairoBridgeStatus` names both files. Refusing is the correct
behaviour — passing a `cairo_t *` between two mapped Cairos is silent
corruption — but it means text does not render until a Pango built against the
bundled Cairo ships.

## Wave 12: the interpreter proxy

`src/common/sqVirtualMachine.c` becomes `rust/pharo-platform/src/virtual_machine.rs`.
It builds the proxy: the struct of function pointers every plugin is handed by
`setInterpreter`, and through which every external primitive reaches the
interpreter.

### Why it waited

About half of those ~150 functions were declared nowhere but at the top of the
`.c` file. Porting it directly would have deleted the only statement of their
signatures and left the Rust restating them from memory. Getting one wrong is
undefined behaviour that **neither the linker nor `abi-check.sh` can see**,
because C linkage carries no types.

### How it was done instead

The prototype block moved to `include/pharovm/common/interpreterProxyFunctions.h`
— unchanged, same order, same conditionals — and both sides now read it:

- `sqVirtualMachine.c` includes it and is otherwise untouched, which is why the
  all-C build is byte-for-byte unaffected. That was verified before any Rust
  was written.
- `pharo-vm-sys` binds it with bindgen's `allowlist_file` rather than naming
  150 functions, with a five-entry `BLOCKED_FUNCTIONS` list for the ones
  earlier waves already export (`signalSemaphoreWithIndex`,
  `ioLoadFunctionFrom`, and three others). Adding an entry to the proxy needs
  no bindgen change.

The result: every `vm.field = Some(func)` is checked by rustc against a field
type *and* a function type that both came from the C headers. A wrong pairing
does not compile. **All 150 compiled first time.**

The table is pinned to `VM_PROXY_MINOR 15` with a `const` assertion, rather
than carrying the seventeen `#if VM_PROXY_MINOR > N` branches that are all
taken at 15.

The unit-test binary cannot link the interpreter, and a table of function
*pointers* has no call to intercept the way earlier waves' `cfg(test)` seams
did — taking the address is enough to need the symbol. So there is a generated
block of 154 test-only stubs, never called, each `unreachable!`.

### What the completeness test turned up

Four of the 153 slots end up null. `scheduleInMainThread` is assigned `NULL`
outright, which is known. But `showDisplayBitsLeftTopRightBottom`,
`sendInvokeCallbackStackRegistersJmpbuf` and `reestablishContextPriorToCallback`
are declared by `virtualMachine.h` and **never mentioned by the C at all** — a
plugin calling one would call through a null pointer.

Left exactly as they were, since filling them in is a decision about the plugin
ABI rather than a port, but the test names all four so a fifth cannot appear
unnoticed.

### Verification

- `cargo test -p pharo-platform`: 108 pass, 6 new; clippy clean
- `abi-check`: 982 exported symbols, unchanged; `USE_RUST_PLATFORM=OFF` still
  builds and still matches the baseline
- A probe exercising nineteen primitive families through the proxy —
  LargeIntegers, MiscPrimitivePlugin, FilePlugin, float conversion, class
  tests, points, hashing, byte arrays, FFI — gives identical results from both
  builds:

  ```
  #(155 955194 1000 1 #LargePositiveInteger 7 1 'THE QUICK BROWN FOX'
    74165973 1414214 '3.141592653589793' 'Float infinity'
    #(#SmallInteger #ByteString #ByteSymbol #SmallFloat64 #Fraction)
    '(4@6)' true true 10450119 0 'ffi-ok')
  ```

- Startup, `--help`, `--version`, plugins, delays and the missing-image path
  all identical, mmap addresses included unnormalised

### A note on the header

The extraction is a genuine improvement to the C tree independent of the port —
those prototypes belonged in a header — so it is worth keeping even if a wave
is ever reverted.

## Wave 13: asynchronous DNS, and the foreign-thread edge under it

`CLAUDE.md` lists the remaining work in order, and puts async DNS first — not
because it is the largest win, but because it is the *smallest complete
instance* of the only shape a Rust plugin may take: **handle, doorbell,
collect.** The image names a Rust-owned thing, work happens where the image is
not, a counted signal wakes a Process, and a later primitive on the VM thread
copies bytes out. Everything else the port still wants — a job pool, fd
watches, an `AsyncPlugin` — is that same shape at larger scale.

It also puts a prerequisite in front of it, and this wave did the prerequisite
first.

### The prerequisite: nothing had ever signalled from a foreign thread

Every asynchronous design rests on N background threads incrementing counters
in the VM's external-semaphore request table and the interpreter noticing.
Nothing in this tree had ever done that. SocketPlugin's own `vm_ref` asserted
the opposite in a `SAFETY` comment — *"handlers run on the interpreter thread,
so this never races a primitive"* — and `unix-os-process-plugin` signals from a
signal handler into a path that takes a `sem_wait`, which is unsound rather
than a model.

`rust/examples/ext-sem-soak` is a throwaway plugin that does exactly that and
nothing else: N threads signalling a contiguous block of registered indices at
a paced rate, with `soak.st` driving it against a GC-heavy image. It is
deliberately absent from `cmake/rust.cmake`, so no bundle can carry it.

Measured on Linux x86_64, Pharo 12.0 build 1597, against a VM built from this
tree with `USE_RUST_PLATFORM=ON USE_RUST_PLUGINS=ON`:

One run of ten minutes, 8 threads, 50 us apart, ~71,700 signals a second:

| | |
|---|---|
| **(a) Lost signals** | **none.** **43,019,960 sent, 43,019,960 received**, and per index exactly — 5,377,496 / 5,377,494 / 5,377,494 / 5,377,494 / 5,377,494 / 5,377,495 / 5,377,496 / 5,377,497, matched one for one |
| **(b) `signalSemaphoreWithIndex`** | mean 4.8 us, p50 3.4 us, p99 18.9 us, p99.9 25.2 us, max 280 us — what a signaller sees of the VM thread holding the same `requestMutex` |
| **(c) `sigprocmask`** | **per-thread on glibc**, pinned by a new unit test in `external_semaphores.rs` that runs the real `SignalBlock` on another thread and asserts the calling thread's mask does not move. Darwin unverified |
| **(d) Wake latency** | mean 55 us, p50 38 us, p99 766 us, p99.9 787 us, max 7.19 ms, over 2,000 paired sends — spawn, `requestMutex`, `forceInterruptCheck`, `aioInterruptPoll`, the next interrupt check, `doSignalExternalSemaphores` and the scheduler, end to end. "Worst case one relinquish quantum" holds for the body of the distribution; the max is three orders of magnitude above the median, so a design that needs a *bound* rather than a typical case does not have one here |

Two honest limits on those numbers. The run is **ten minutes, not the hour**
`CLAUDE.md` asked for, and it is one machine and one platform. And (b)'s
reservoir keeps the **first** 16,384 samples per worker and counts the rest as
dropped (42,888,888 of them here), so the percentiles describe the start of the
run rather than a uniform sample of it; that is stated in the primitive that
reports it, and would need a proper reservoir sampler to fix.

The counting request table holds, which is what (a) says: `requests` and
`responses` per entry rather than a flag is exactly what stops two signals
arriving close together from collapsing into one, and now something has checked
it at rate rather than by reading the code.

**The one result that changes a design.** The first run used no pause at all,
and the image made *no progress whatsoever* — eight threads at 700% CPU, and a
five-second run had not finished its first `Delay` after two minutes. That is
not the table failing; it is `signalSemaphoreWithIndex` doing what it is
written to do on every single signal: take `requestMutex` with signals masked,
`forceInterruptCheck()`, then `aioInterruptPoll()` to wake the poll loop. At an
unbounded rate the VM thread never gets back to bytecode. So a signaller must
be paced by something — which every design in `CLAUDE.md` already is (one
signal per lookup, per readiness edge, per completed job) — and `AsyncPlugin`'s
"one external-semaphore index for the whole runtime" is now a measured
requirement rather than a tidiness preference. `SOAK_MICROS=0` is kept as the
starvation probe.

### The change itself

`sqResolverStartNameLookup` blocked the whole VM inside `getaddrinfo` and then
signalled the resolver semaphore on the way out — *"we're done before we even
started"* is the C's own comment — so `ResolverBusy` (2) was a state the Unix
plugin could not reach. **The image was written for the other contract all
along**, and Pharo 12.0's `NetNameResolver class >> initialize` says so in a
comment: *"on other platforms, such as Unix, the resolver is synchronous; a
call to, say, the name lookup primitive will block all image processes until it
returns."* `addressForName:timeout:` and `nameForAddress:timeout:` both take a
mutex, wait for the resolver to be ready, start the lookup, wait on the
resolver semaphore while polling the status, and call `primAbortLookup` on a
timeout. So this needed **no image-side change at all**.

The obstacle was self-inflicted rather than architectural: `start_name_lookup`
held the resolver's `STATE` guard across the whole of `getaddrinfo`, and
`resolver_status` took the same guard. Spawning a thread without restructuring
would have frozen the VM on a mutex instead of on a syscall — the same outage
with a worse cause. So:

- `lastError` and a new `LOOKUP_BUSY` flag moved **out** of the mutex into
  atoms, because the image polls `sqResolverStatus` in a loop and that poll
  must never queue behind a worker. Neither is part of the invariant the mutex
  protects, which is `results`/`cursor`.
- A **generation counter**, bumped under the mutex by every start and every
  abort, decides who may commit. A worker that finds the generation moved on
  drops its answer and stays silent. That is what makes `sqResolverAbort` — an
  empty function in the C — mean something.
- The mutex is held for the microseconds it takes to store an answer, and for
  nothing else.

Both directions went asynchronous, because they share `lastName`, `lastError`
and the status word: leaving one synchronous would have let it race the other's
worker.

### Verification: the same image on two VMs

Two VMs built from this tree on Linux x86_64, one all-Rust and one all-C, same
Pharo 12.0 image:

| | C plugin | Rust plugin |
|---|---|---|
| `primStartLookupOfName:` returns after | 2,384 us | **93 us** |
| `resolverStatus` immediately after | 1 (`ResolverReady`) | **2 (`ResolverBusy`)** |
| Smalltalk loop iterations during the query | **0** | **4,049,919** (over 155 ms) |
| `addressForName: 'files.pharo.org'` | `193.49.213.186` | `193.49.213.186` |
| `nameForAddress: 8.8.8.8` | `'dns.google'` | `'dns.google'` |
| `primAbortLookup` while busy | n/a — never busy | 2 -> 1, immediately |
| `Socket newTCP connectToHostNamed: 'files.pharo.org' port: 80` + `GET /` | `HTTP/1.1 301 Moved P...` | `HTTP/1.1 301 Moved P...` |

The last row is the regression check that reads as the least interesting and
matters most: `connectToHostNamed:` resolves through the new asynchronous path
and then drives the socket half unchanged, so an end-to-end HTTP request over a
real network answers byte for byte what the C plugin answers.

**And what it costs.** One Process's lookup gets *slower* end to end, and the
tables above would be dishonest without the number: 200 back-to-back
`NetNameResolver addressForName:` calls for a name the OS resolver has already
cached average **1,644 us on the C plugin and 1,997 us on this one**. The extra
~350 us is a thread spawn, a doorbell round trip and a scheduler wake — the
p50 of (d) plus the spawn. That is the trade, made deliberately: the Process
doing the lookup waits about a fifth longer, and every other Process in the
image stops waiting at all.


Plus 45 unit tests in the crate, 43 of which build on Linux and were run there;
six are new, five of them asserting what the resolver looks like *during* a
lookup, which a `#[cfg(test)]` gate every worker takes makes a fact rather than
a race, and the sixth pinning the quiescence ledger.

### What an adversarial review caught

Six independent reviewers over the change raised twenty findings; nineteen were
refuted on the code. The one that survived is worth recording, because it
generalises to every plugin that hands work to a thread:

**`shutdownModule` answered 1 unconditionally.** `Smalltalk vm unloadModule:
'SocketPlugin'` is reachable from ordinary image code and ends in `dlclose`,
and a `pharo-dns` worker parked in `getaddrinfo` is executing that library's
text and is about to touch its statics. Answering 1 was correct before this
wave — the plugin's only outward function pointers were the aio handlers, and
`aioFini` clears those — and the asynchronous resolver invalidated the
precondition without re-establishing it. The fix is the quiescence ledger
`CLAUDE.md` already specifies, with one refinement the review's own reasoning
forced. A *counter* decremented at the end of the worker's closure — or by a
`Drop` at the end of it — reaches zero while the thread is still running its
epilogue: libstd's thread cleanup and any TLS destructors, all of which is code
in this cdylib, since each one statically links its own libstd. So the ledger
holds `JoinHandle`s, not a count, and `is_quiescent` *joins* the finished ones
— `join` is the only thing that means "this thread is gone". `shutdownModule`
answers 0 while any handle remains, which `ioUnloadModule` honours by leaving
the module loaded. A unit test pins the gap that makes this subtle: after an
abort there is no lookup in flight, and the thread that ran it is still alive.

Verified against the live image, which also exercises `CLAUDE.md`'s
hot-reloading item as a side effect:

```smalltalk
"with a lookup in flight"
NetNameResolver primStartLookupOfName: 'www.kernel.org'.
Smalltalk vm unloadModule: 'SocketPlugin'    "=> PrimitiveFailed; status still 2"
NetNameResolver addressForName: 'www.kernel.org' timeout: 10   "=> 146.75.61.55"

"once quiescent"
Smalltalk vm unloadModule: 'SocketPlugin'    "=> succeeds"
NetNameResolver addressForName: 'files.pharo.org' timeout: 10  "=> 193.49.213.186"
```

The second pair is the interesting one beyond this wave: the module unloads and
is re-`dlopen`ed on the next primitive, in a running image, and answers
correctly. That is the reload loop `CLAUDE.md` §1 wants, working — though it
does not by itself prove `dlclose` *unmapped* anything, which still needs a
version-stamping primitive to settle.

### The dlclose experiment, and what it cost the plan

`CLAUDE.md` §1 wanted hot-reloadable Rust plugins: unload, `install` a new
build, next call runs new code in the same image. It also asked, in one line,
for a version-stamping primitive to check that `dlclose` really unmaps first.
That check has now been run, and it takes the item off the list.

A throwaway plugin outside the tree, whose only primitive answers a
compile-time version number. Five variants, each one: install v1, do a single
thing, `Smalltalk vm unloadModule:`, install v2 over it with a **new inode**,
ask the version again — all inside one image.

| what the plugin did first | mappings after unload | next call answers |
|---|---|---|
| nothing at all | **0** | **v2 — reloaded** |
| touched one `thread_local!`, on the VM thread, once | 4 | v1 — **stale** |
| spawned and joined a thread touching none of its own TLS | 4 | v1 — **stale** |
| both | 4 | v1 — **stale** |
| one `poison::Section::enter()` and nothing else | 4 | v1 — **stale** |

The last row decides it. `Section` is the SDK's own, `poison::lock` opens one
for every guarded mutex, `handles::Registry` opens one for every lock it takes,
and the SDK is statically linked into every plugin cdylib — so that
`thread_local!` is resident in each of them. Any plugin that takes a lock or
touches a registry is pinned, permanently, from its first substantive
primitive. That is all sixteen.

Two things the experiment corrected in the guess this item was written on.

The trigger is **TLS instantiation in the DSO, not threads.** One touch of one
thread-local, on the interpreter thread, with no thread anywhere, is enough,
and it does not wear off — so this is not the `l_tls_dtor_count` race
`CLAUDE.md` named, where the count returns to zero once the thread exits.

And the mapping count stays at **four, not eight**: `dlopen` of the replacement
file hands back the *retained* object rather than mapping the new one. glibc's
`_dl_map_object` looks for an already-loaded object by **name**, so installing a
new inode at the same path changes nothing at all. Which in turn says what a
working reload would have to look like: a new *module name*, not a new file —
and the image's `<primitive: 'x' module: 'FooPlugin'>` pragmas name the module,
so that is a different design than this one, not a fix to it.

What survives is the half already built: `ioUnloadModule` honours a
`shutdownModule` of 0, and socket-plugin's quiescence ledger makes that refusal
mean something. macOS is unmeasured; dyld's rules differ, and the same probe
should be run there before either answer is assumed.

### A note on the environment

This is the first wave with a Linux machine to run on, which is why the claims
above say "measured" where earlier waves said "cross-checked". Two things found
along the way are environment, not port: the all-C build needs system
`libuuid` and OpenSSL development packages that the Rust build vendors away,
and both builds ship a `libgit2` whose `git_libgit2_init` the image fails to
resolve — identically on each, so it is a packaging problem in this tree rather
than a platform-layer regression. The stock `files.pharo.org` VM does not ship
that library and does not hit it.
