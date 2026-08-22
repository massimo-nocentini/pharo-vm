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
