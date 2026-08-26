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

## Beyond the C plugins: bindings for the two downloaded libraries

Everything above replaces something. These two do not.

Cairo and SDL are the only third-party libraries the VM *ships without using*:
`cmake/importCairo.cmake` and `cmake/importSDL2.cmake` download ready-made
binaries into the directory beside the executable, and nothing in `src/`,
`plugins/` or `include/` mentions either — the image reaches them through UFFI
(Athens-Cairo, OSWindow-SDL2). Two new crates make the same libraries reachable
as named primitives instead, with the plugin owning the objects and the image
holding integer handles.

| Plugin | Crate | Surface | Standalone verification |
|---|---|---|---|
| CairoPlugin | `cairo-plugin` | 108 primitives over 103 `cairo_*` entry points | 22 tests; run against the bundle's own `cairo-1.17.4` binary, all entry points resolved, drawing asserted per-pixel |
| SDL3Plugin | `sdl3-plugin` | 63 primitives over 52 `SDL_*` entry points | 26 tests; run against SDL3 `release-3.4.10`, all entry points resolved, display path driven headless, struct offsets checked against a C `offsetof` probe |

Three things distinguish them from the ports above.

**The download rules are untouched.** Both crates `dlopen` whatever the
existing CMake fetches. That is not a preference: the downloaded zips hold
runtime objects only — no headers, no `.pc` file — so there is nothing to link
against, and `cairo-sys-rs`/`sdl3-sys` would add a `-dev` package requirement
to a VM that already ships the library. The search order is the executable's
directory first, then the system loader, which is where the image's FFI looks
too.

**A missing library is a normal outcome.** `initialiseModule` answers 0 and the
VM rejects the module, so the image can tell "not available here" from "not
implemented" and stay on its FFI binding. On Linux that is the *expected* path
for SDL3 today: `importSDL2.cmake` fetches `SDL3-3.4.10` for Windows and macOS
only, and no Linux artifact exists on `files.pharo.org`.

**The image gets handles, not pointers.** Every Cairo and SDL object lives in a
`pharo_vm_plugin::handles::Registry` and the image holds a SmallInteger carrying
a slot index and a generation counter. A stale handle fails its primitive with
`NotFound`; a stale pointer, which is what the FFI binding passes today, is
dereferenced. Two consequences worth knowing: the Cairo plugin *keeps* a pin on
image memory a surface is drawing into when Cairo still holds a reference to
that surface (`primitiveRetainedPinCount` reports how many), and the SDL plugin
invalidates a window's renderer and that renderer's textures when the window is
destroyed, because SDL frees them without reference counting.

`cmake/rust.cmake` carries them in `RUST_ONLY_PLUGINS` rather than
`RUST_REPLACED_PLUGINS` — there is no C plugin to skip — gated on
`FEATURE_LIB_CAIRO` and `FEATURE_LIB_SDL2`, and built under the same
`USE_RUST_PLUGINS=ON`.

**Neither is usable from Pharo yet.** This is half the change: an Athens
backend and an OSWindow backend calling these primitives instead of UFFI still
have to be written, in the image, and until they are nothing in Pharo touches
either plugin. Each crate's README documents the primitive-by-primitive
contract those backends have to be written against, including the 64-byte
decoded event record `SDL3Plugin` answers.

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
