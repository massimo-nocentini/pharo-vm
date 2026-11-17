# TimSort: stable sorting on timsort.c

The TimSortPlugin sorts with [timsort.c](https://github.com/massimo-nocentini/timsort.c) at commit fd70381, a port to C of the sort of CPython's lists (`listsort` of `Objects/listobject.c`: Tim Peters' timsort with the powersort merge policy), and the package `TimSort` (the submodule `plugins/TimSortPlugin/timsort.st`) gives Pharo 12 to 15 an explicit API on top of it: the class side of `TimSort`, and extension methods whose selectors all begin with `timSort`.
Nothing of Pharo's own sort is replaced: `sort`, `sort:`, `sorted:`, `asSortedCollection` and `SortedCollection` are untouched, and the package adds no method whose selector does not begin with `timSort`.
On a VM without the plugin the package `TimSort-FFI` calls `libTimSortFFI`, the same engine as a library of its own, and without both the package `TimSort-Smalltalk`, a port of timsort.c to Smalltalk with the same comparisons in the same order, answers the same results.

It sorts in four modes, each in place:

- (a) an Array of numbers, or of strings, in their natural order, in C, without running any Smalltalk;
- (b) a numeric buffer (Float64Array, Float32Array, DoubleWordArray, IntegerArray, WordArray, Bitmap, DoubleByteArray, ByteArray) by value;
- (c) any collection by keys that a block computes once per element: C sorts a permutation by the keys (an argsort), which is then applied;
- (d) any collection by any sortBlock: by default, on every VM, the Smalltalk port, which the benchmarks found faster than the C sort for this mode in every case (see Benchmarks); explicitly (`TimSort sort:with:backend: #plugin` or `#ffi`), the resumable C sort, which runs on a stack of its own (a coroutine) and stops at every comparison, which the image answers by evaluating the block in the calling Process, then resumes; no FFI callback is involved.

Files, in `plugins/TimSortPlugin`:

- `src/common/TimSortPlugin.c`: the module functions and the eight primitives (C, with the VM's headers); the only file of `src/common`;
- `src/engine/tsEngine.c`: the engine, the only file that includes `timsort.h` (C, without the VM's headers): typed sorts of buffers, argsorts, the sort of words by a comparison, and the exact orders of numbers and strings;
- `src/coro/`: the resumable sort (`tsCoro.c`) and its stack switches, one file per target (`tsCoroX64.c`, `tsCoroA64.c`, `tsCoroPosix.c`, `tsCoroFibers.c`), compiled natively only;
- `include/common/tsEngine.h`, `include/common/tsCoro.h`: the interfaces of the engine and of the resumable sort, which the plugin and `libTimSortFFI` share;
- `src/ffi/tsFFI.c`, `include/ffi/tsFFI.h`: `libTimSortFFI`, the engine and the resumable sort for the FFI (C, without the VM's headers);
- `LICENSE.timsort`: the licence notice of timsort.c (see Licences);
- `timsort.st/`: the Smalltalk side, a git submodule of [timsort.st](https://github.com/massimo-nocentini/timsort.st) (`git submodule update --init plugins/TimSortPlugin/timsort.st`): Tonel packages in `src/` with `BaselineOfTimSort`, whose commit is the version of the Smalltalk code that goes with the plugin, and whose `README.md` is the guide of the API;
- the packages `TimSort-Tests`, `TimSort-Smalltalk-Tests` and `TimSort-FFI-Tests` of the submodule, and `tests/`: the tests;
- `bench/`: the benchmark.

## Building

The plugin is on by default on every 64-bit target: `FEATURE_PLUGIN_TIMSORT=ON`.
It is off on 32-bit targets (Linux armv7l, `cmake/vmmaker.cmake`), where `-DFEATURE_PLUGIN_TIMSORT=ON` stops the configuration: a request of the resumable sort, `(i << 32) | j`, needs the SmallIntegers of a 64-bit VM (see The resumable sort).
`-DFEATURE_PLUGIN_TIMSORT=OFF` leaves it out, along with the download of timsort.c.

    cmake -S . -B build -DFLAVOUR=CoInterpreter            # the plugin is in
    cmake -S . -B build -DFEATURE_PLUGIN_TIMSORT=OFF       # it is not

timsort.c has no release and no tag: it is taken from the archive of its commit, `https://github.com/massimo-nocentini/timsort.c/archive/fd70381039da47507984b9c1306914828871afbf.tar.gz` (33 KB), checked against its SHA256 (`3e1eb62a…090d9850016`), by `cmake/timsort.cmake` with the function of the WebAssembly libraries (`cmake/emscripten/deps/fetch.cmake`), natively too, as simdjson and simdutf are:

- the archive comes from `WASM_DEPS_DIR` when that directory holds `timsort.c-fd70381.tar.gz`, which builds offline, else from `<build>/downloads`, else from GitHub at configure time;
- `-DTIMSORT_SOURCE_DIR=<tree>` takes a timsort.c source tree instead, a clone at that commit (its `src/timsort.h` is checked: the include guard, `list_sort_impl` and the `PTRDIFF_MAX` bound of that commit).

GitHub makes the archives of commits on demand; if their bytes ever change, the configuration stops with fetch.cmake's message, which names both hashes, and the archive of the pin, put in `WASM_DEPS_DIR`, builds again.
timsort.c's own Makefile is not used: `src/timsort.c` is compiled as the static library `pharo_timsort`, in `<build>/deps` (never in `build/vm`, which is packaged whole), with `-O3 -g0 -DNDEBUG` (its assertions are for its own tests), hidden visibility, and position-independent code natively.
Its one global function, `list_sort_impl`, has a generic name: it is renamed `pharo_timsort_list_sort_impl` by a definition that every file including `timsort.h` sees, and with hidden visibility it never leaves the library that links it.
The engine is the only file that includes `timsort.h`; the plugin and the library each link their own copy of the engine, the resumable sort and timsort.c, with hidden visibility, so that the VM's `dlopen` with `RTLD_GLOBAL|RTLD_DEEPBIND` (`src/externalPrimitives.c`) never mixes them up, and both can live in one process.
Natively `libTimSortPlugin.so` is 44 KB and `libTimSortFFI.so` 32 KB (Linux x86_64, clang 18); the plugin exports nothing but its module functions, its eight primitives and their accessor depths (20 symbols), the library nothing but its 22 `ts_ffi_*` functions.

The resumable sort switches C stacks:

- x86_64 System V (Linux, macOS) and AArch64 (Linux, macOS) with a switch written in assembly (`tsCoroX64.c`, `tsCoroA64.c`: the callee-saved registers, the floating-point control words of x86_64, `endbr64` under CET and `bti c` under BTI, ELF and Mach-O spellings), on stacks of 256 KB that `mmap` maps with a guard page below them (`tsCoroPosix.c`);
- Windows x64 and arm64 with Fibers (`tsCoroFibers.c`: `ConvertThreadToFiberEx` once, unless the thread already is a fiber, never converted back, `CreateFiberEx` with 256 KB, `SwitchToFiber`);
- nothing on Emscripten, and on any other target: `src/coro` is not compiled into the WebAssembly VMs, and every file of it compiles to nothing on a target it does not serve; the coroutine primitives are there, and fail with `#'unsupported operation'` (see The resumable sort).

A process that runs with a shadow stack (x86_64 CET user shadow stacks, Linux 6.6 and later, which glibc enables only for programs marked for them; AArch64 GCS, Linux 6.13 and later) cannot return into another stack with a plain `ret`: there the resumable sort reports itself unavailable, and the image runs the sorts by a sortBlock in Smalltalk, as in WebAssembly.

Platforms:

- Linux x86_64 and Emscripten (the node and web VMs of `make wasm`) are built and tested;
- macOS (x86_64 and arm64), Windows (MSYS2 UCRT64 with clang on x86_64, CLANGARM64 on arm64) and Linux aarch64 are written for, as SimdUTF's are, but **untested**: no build of them has run here.
  The switches of AArch64 and of Mach-O were cross-compiled with clang (`--target=aarch64-linux-gnu`, `x86_64-apple-macos11`, `arm64-apple-macos11`), and the Fibers against the headers of mingw-w64 (`x86_64-w64-mingw32`, `aarch64-w64-mingw32`), with `-Werror`, but neither linked nor run.
  Each native job of `.github/workflows/build.yml` loads the plugin and the library of its zip with `plugins/TimSortPlugin/tests/ci-load.st`, which sorts numbers, strings, a Float64Array and keys, and a sort by a sortBlock through the resumable sort of each, and checks that `LICENSE.timsort` is in the zip, so the first CI run tells.
  The Linux and macOS steps run under `shell: bash`, whose `pipefail` makes a VM that dies after printing fail the step.
  Build with `-DFEATURE_PLUGIN_TIMSORT=OFF` if the build breaks there.

`FEATURE_TIMSORT_FFI` (natively, `ON` by default with the plugin) also builds `libTimSortFFI` (`libTimSortFFI.so`, `.dylib`, or `TimSortFFI.dll` on Windows, which MinGW may name `libTimSortFFI.dll`) from `src/ffi/tsFFI.c`, the engine, `src/coro` and `pharo_timsort`, next to the plugins, so that it is in the zips.
It is not a plugin and needs nothing of the VM: copied next to any 64-bit Pharo VM, or named with `TimSortLibrary libraryPath:`, it serves that VM's image.

In the WebAssembly VMs the plugin is built in, without `src/coro`, and the library is not built (there is no `dlopen`).
timsort.c is C, so the plugin brings no C++ runtime; its notice goes into `THIRD-PARTY-NOTICES.txt` (see Licences and docs/WebAssembly.md).
The TimSortPlugin adds 19.4 KB to `pharo-web.wasm` (6.3 KB gzipped), and 19.0 KB to `pharo.wasm` (7.0 KB gzipped), measured on two build directories of one checkout, with and without it.
The native tree that `make wasm` generates the sources in builds no plugin (only the VMMaker VM, the VMMaker image and the generated sources), but no `HOST_PLUGIN_FLAGS` entry turns the TimSortPlugin off there, so its configure includes `cmake/timsort.cmake` and needs the archive of timsort.c (33 KB), from the `WASM_DEPS_DIR` of the host or from GitHub, for nothing it builds; an offline host tree without it stops at its configure, where `cmake -DFEATURE_PLUGIN_TIMSORT=OFF build-wasm/host` turns it off.

## Loading the Smalltalk side

The Smalltalk code is in the submodule `timsort.st`, as Tonel packages that Metacello loads with `BaselineOfTimSort`, in Pharo 12, 13, 14 and 15.
Its groups: `Core` (the packages `TimSort`, the facade, the plugin's primitives and the backends, and `TimSort-Smalltalk`, the port, which the facade needs; neither needs any other project), `Tests` (`TimSort-Tests`), `FFI` (`TimSort-FFI`, the library through the FFI), `FFITests` (`TimSort-FFI-Tests`), `Smalltalk` (`TimSort-Smalltalk` alone, without the facade), `SmalltalkTests` (`TimSort-Smalltalk-Tests`) and `default` (all of them).

    Metacello new
        baseline: 'TimSort';
        repository: 'tonel://<repository>/plugins/TimSortPlugin/timsort.st/src';
        load: #( 'Core' 'FFI' ).

or, from the command line, `tests/load-timsort.st`, which the tests, the benchmark and lane 71 use:

    TS_PLUGIN_DIR=plugins/TimSortPlugin TS_GROUPS='Core Tests' \
    pharo --headless Pharo.image --no-default-preferences st --save --quit plugins/TimSortPlugin/tests/load-timsort.st

It loads the groups of `TS_GROUPS` (`Core Tests` by default) and prints `TimSort: loaded, protocol 1, backend #<plugin, ffi or smalltalk>, blockSortBackend #smalltalk, resumableBackend <#plugin, #ffi or nil>, plugin <what primitiveInfo answers, or nil>`, then, with `TimSort-FFI`, `TimSort: library <available> <stack switch>`, then the groups and packages loaded.
It exits with 1, before the image is saved, when the submodule is not checked out, a load fails, `TimSort protocol` is not 1, `TimSortSmalltalk` or the facade do not sort `#(3 1 2)`, a method of a TimSort package uses an undeclared variable, a TimSort package defines a method on a class whose name begins with `Zn`, or an extension method (on a class of no TimSort package) has a selector that does not begin with `timSort`: the package is opt-in, and changes nothing of Pharo's own sort.
Metacello makes UUIDs: the VM needs its UUIDPlugin.
To change the Smalltalk code, commit in the submodule, push it to timsort.st, and commit the new submodule commit here, with the plugin changes it goes with.

## API

The API is on the class side of `TimSort`, and in extension methods of `SequenceableCollection` (with the behaviour of `OrderedCollection`, `SortedCollection`, `Interval` and `Symbol` described in Semantics) whose selectors all begin with `timSort`.
Each sort works in place and answers the receiver; each `timSorted…` form sorts a copy, as `sorted:` does.
The `README.md` of the submodule has examples of each mode.

| Mode | Extension | Facade |
|---|---|---|
| a, b: the natural order | `timSort`, `timSortDescending`, `timSorted` | `TimSort sort: c`, `sortDescending: c`, `sort: c from: i to: j descending: aBoolean`, `sortBuffer: b`, `sortBuffer: b from: i to: j descending: aBoolean` |
| c: by keys | `timSortBy: keyBlock`, `timSortDescendingBy: keyBlock`, `timSortedBy: keyBlock` | `TimSort sort: c byKey: keyBlock`, `sortDescending: c byKey: keyBlock`, `sort: c from: i to: j byKey: keyBlock descending: aBoolean`; `argSort: keys`, `argSortDescending: keys`, `argSort: keys from: i to: j descending: aBoolean` (the permutation, an Array of the indices i to j: position k of the sorted range takes the element at index `perm at: k`) |
| d: by a sortBlock | `timSort: aBlock`, `timSorted: aBlock` | `TimSort sort: c with: aBlock`, `sort: c from: i to: j with: aBlock` (the port); `sort: c with: aBlock backend: aSymbol`, `sort: c from: i to: j with: aBlock backend: aSymbol` (`#smalltalk`, the default, or `#plugin` or `#ffi`: the resumable C sort) |

`sort:with:backend:` with `#plugin` or `#ffi` signals `TimSortBackendUnavailable` (an Error; `backendName`, and a `messageText` that says why), before the collection is looked at, when that backend has no resumable sort here: no plugin, or a plugin that cannot switch stacks (WebAssembly, a shadow stack); `TimSort-FFI` not loaded, no library, or a library that cannot switch; or any other name than the three.
The rest of the class side: `backend` and `blockSortBackend` (see Backends), `resumableBackends` (the backends whose resumable C sort works here, `#(#plugin #ffi)`, one of them or none), `resumableBackend` (the first of them, or nil; it does not probe the library when the plugin has one), `coroutinesAvailable` (whether there is one), `liveCoroutines` (the resumable sorts live in the plugin, the library once probed and the chosen backend, 0 between two sorts), `pluginAvailable`, `pluginInfo` (what `primitiveInfo` answers), `resetBackend`, `protocol` (1), the kind codes `kindFloat64` … `kindUInt8` with `kindOf: aBuffer`, `stringClasses`, and the test hooks `testHook:` and `testHook:value:` (`#backend`, `#pluginMissing`, `#fallbackMissing`).

## Backends

| Backend | `TimSort backend` | What it sorts natively | The rest |
|---|---|---|---|
| `TimSortPrimitives`, the plugin | `#plugin` | Arrays of numbers (SmallInteger, Float, normalized LargeInteger) or of strings of the 4 classes, every buffer kind, argsorts of both; mode d through the resumable sort, when named (`backend: #plugin`) | declined: the port sorts that call |
| `TimSortLibrary`, `libTimSortFFI` (package `TimSort-FFI`) | `#ffi` | Arrays of SmallIntegers; Arrays of non-NaN Floats with SmallIntegers within ±2^53; every buffer kind; argsorts of these; mode d through the resumable sort, when named (`backend: #ffi`) | declined (strings, LargeIntegers, SmallIntegers beyond 2^53 among Floats): the port |
| `TimSortPortBackend`, the port `TimSortSmalltalk` (package `TimSort-Smalltalk`) | `#smalltalk` | everything, in Smalltalk; mode d by default | none |

- The choice is made once per session (`Smalltalk session`): the test hook `#backend` if set, else the plugin when `primitiveInfo` answers protocol 1, else the library when `TimSort-FFI` is loaded and `TimSortLibrary isAvailable`, else the port.
- The sorts by a sortBlock (mode d) run in the port by default, whatever the chosen backend, on every VM: `TimSort blockSortBackend` answers `#smalltalk`.
  This is a measured choice (see Benchmarks, mode d): the port makes the same comparisons as the resumable C sort, in the same order, with the same result, and the C sort adds a primitive call and two stack switches (or a callout) to each comparison, so it was slower than the port in every case of the benchmark, 1.1 to 6.5 times through the plugin and 3.5 to 17 times through the library.
- The resumable C sort is reached by naming it, `TimSort sort: c with: aBlock backend: #plugin` (or `#ffi`, and the `from:to:` form): the chosen backend when it has that name (the test hook `#backend` too), else the plugin for `#plugin` and the library for `#ffi`, when its resumable sort works (`primitiveInfo` slot 5, `ts_ffi_coro_supported`); `TimSort resumableBackends` lists those.
  In WebAssembly there is none: `resumableBackends` is empty, and `backend: #plugin` signals `TimSortBackendUnavailable`.
- A backend answers what its primitive answers, or a `TimSortFailure` with the failure code; any failure but `#'no modification'` and `#'bad index'` (which the facade checks first anyway) makes the facade sort that call with the port, on the untouched collection, except after `#'insufficient C memory'`, after which an Array or an 8-byte buffer holds a permutation of its elements, the start of the sort in C (see Memory and limits).
- The port gives the same results as the C sort for the same comparisons (`TimSort-Smalltalk-Tests` checks it against traces of timsort.c), so a decline changes the time, not the result; but the port finishes an `#'insufficient C memory'` from the permutation the C sort left, so that equal elements that are not identical (`1` and `1.0`, `0.0` and `-0.0` in a buffer) may then come in another order than Pharo's.

## Semantics

All backends give identical results; the tests compare them with Pharo's sort, and with each other, object by object (`==` at every index), and bit by bit for buffers.

- **Ascending** is exactly Pharo's `sort` (`sort: [ :a :b | a <= b ]`); **descending** is exactly Pharo's `sort: [ :a :b | a >= b ]`, timsort's `reverse` flag: equal elements keep their order in both directions.
  Pharo's sort is a stable merge sort, timsort with a strict `<` is stable, and a stable sort of a consistent order has one result only, so they agree to the object.
- **Adaptive.** Sorted, reversed and nearly sorted data take about one comparison per element (exactly n - 1 for sorted or reversed data); random data take about as many as Pharo's merge sort.
- **Numbers** (mode a, and the keys of mode c, natively when every element of the range is a number of these classes): SmallInteger, Float (SmallFloat64 or BoxedFloat64), and normalized LargePositiveInteger and LargeNegativeInteger.
  - They are compared exactly, as the image's code compares them (`Number>>adaptToFloat:andCompare:`): an Integer and a Float without rounding, at any magnitude, by every backend.
    The native JIT compares a SmallInteger beyond 2^53 with a Float after converting the SmallInteger to a Float, so in that zone Pharo's own `sort` may answer another order than the exact one; the tests take an exact reference there (`asExactFraction`).
    The port compares with Pharo's `<=`, but a range that mixes Floats with SmallIntegers beyond 2^53 with an exact sortBlock (`TimSortPortBackend class>>sortBlockFor:from:to:descending:`, which sends `adaptToFloat:andCompare:` for a Float and an Integer), so it answers the plugin's order on every VM (and what the library declines, which the port sorts, too).
  - -0.0, 0.0 and 0 are equal; the infinities are below and above every integer.
  - NaN, Fraction, ScaledDecimal, a LargeInteger that is not normalized, and a mix with anything else are declined: Pharo's comparisons decide, through the port, with Pharo's errors.
- **Strings**: elements whose class is exactly `ByteString`, `ByteSymbol`, `WideString` or `WideSymbol` (`TimSort stringClasses`, given to the primitive as an argument), compared by code point (unsigned 32-bit units for the wide ones), case-sensitive, a proper prefix first: what Pharo's `<=` does for them.
  A subclass of String, or strings mixed with anything else, go to the port: strings mixed with numbers raise Pharo's error.
- **Numeric buffers** (mode b): by value, signed or unsigned as the class says (kinds below); floats with every NaN after every number, whatever its sign and payload, the NaNs equal to each other, and -0.0 equal to 0.0.
  Descending is the same order with timsort's reverse flag: the NaNs come first, equal elements keep their order.
  In Pharo that order is `sort: [ :a :b | b isNaN or: [ a <= b ] ]`, and descending `sort: [ :a :b | a isNaN or: [ b isNaN not and: [ a >= b ] ] ]`.
  Elements are moved, never converted, so their bits are kept (a signalling NaN of a Float32Array too).
- **By keys** (mode c): the keys are `aCollection collect: keyBlock`, computed once, in order, one evaluation per element; the permutation that sorts them, stably, is then applied to the collection.
  The result is Pharo's `sort: [ :a :b | (keyBlock value: a) <= (keyBlock value: b) ]`, which evaluates the block about 2 n log n times.
  Keys may be any objects: Arrays of numbers or strings and buffers are sorted natively, any other key through the port.
- **By a sortBlock** (mode d): the sort asks `lt(i, j)` := `(aBlock value: (c at: j) value: (c at: i)) not`, which for a sortBlock in the style of `[ :a :b | a <= b ]` gives exactly Pharo's `sort: aBlock`. The port does so by default; the resumable C sort, when named, makes the same comparisons in the same order, so both give the same result for the same block.
  - The block runs in the calling Process: errors, non-local returns (`^`) and `Process>>terminate` go to the caller as in Pharo, and the resumable C sort is freed by an `ensure:` on every exit, also when another Process terminates this one at any moment: the facade owns the sort from the call that opens it (through the library, whose callout runs Smalltalk after the C call, that call runs at the highest priority, so that no other Process runs before the id is assigned); a resumable exception (a Warning) that is resumed continues the sort.
  - The request the C sort waits for is kept outside the frame that evaluates the block, so that the Restart of the debugger on that frame evaluates the block again on it.
  - A non-Boolean answer raises Pharo's error (`NonBooleanReceiver`).
  - While the resumable C sort runs only indices move: a sort left by an error leaves the collection untouched (the port, the default, leaves a permutation of it).
- **Inconsistent orders** (NaN among objects, a strict block like `[ :a :b | a < b ]`, a block that contradicts itself) have no single stable result: every backend gives timsort's, the C sort and the port the same one, since they make the same comparisons in the same order; it is not necessarily Pharo's merge sort's (with a strict block, equal elements come out in an order of their own).
  The tests compare those with each other, not with Pharo's sort.
- **Read-only receivers** (`isReadOnlyObject`, the internal Array of an OrderedCollection too) signal `ModificationForbidden` before anything is touched, on every backend, one for each read-only object.
  This is stricter than Pharo, whose `sort` sorts a read-only Float32Array, Float64Array or OrderedCollection anyway, and on purpose.
  The exception has the retry of Pharo's write barrier (`basicAt:put:`, or `instVarAt:put:` for an OrderedCollection, of the value a slot holds): a handler that makes the object writable and sends `retryModification` lets the sort go on, as with Pharo's `sort`; when the object is still read-only after the handler resumed, nothing is sorted.
  `ModificationForbidden` is not resumable in Pharo 12 to 15 (`retryModification` resumes it unchecked); a read-only object without slots (`#()`) has nothing to change and signals nothing.
- **Collections**: Array and its subclasses (a WeakArray too, through `at:put:`); OrderedCollection, the slots `firstIndex` to `lastIndex` of its internal Array, in place; SortedCollection, whose sortBlock the sort sets as `sort:` does (`timSort` sets nil, `timSort: aBlock` sets aBlock, `timSortBy:` the matching `<=` block of the keys); Interval and Symbol, which cannot change, do what Pharo's `sort` does (its error), and their `timSorted…` answer what `sorted…` answers; a String, a Text, and any other SequenceableCollection, through `at:` and `at:put:` (the port copies runs with `replaceFrom:to:with:startingAt:` only within an Array: a Text takes only a Text there).
- **Ranges** (`from:to:`) are 1-based and inclusive, an empty range is `start = stop + 1`, anything else signals `SubscriptOutOfBounds`; fewer than two elements are never compared.

Kinds of buffers (`TimSort kindOf:`; the codes are the primitive's):

| Code | `TimSort` | Element | Classes |
|---|---|---|---|
| 1 | `kindFloat64` | IEEE binary64 | Float64Array |
| 2 | `kindFloat32` | IEEE binary32 | Float32Array |
| 3 | `kindInt64` | signed 64 | none in Pharo 12 to 15 (the keys of the library) |
| 4 | `kindUInt64` | unsigned 64 | DoubleWordArray |
| 5 | `kindInt32` | signed 32 | IntegerArray |
| 6 | `kindUInt32` | unsigned 32 | WordArray, Bitmap |
| 7 | `kindUInt16` | unsigned 16 | DoubleByteArray |
| 8 | `kindUInt8` | unsigned 8 | ByteArray |

## Memory and limits

- **No allocation in the VM.** The sorting primitives allocate no object: they permute the slots of the Array or of the permutation, or the elements of the buffer, in place, so no garbage collection can happen during them and no oop moves; only `primitiveInfo` allocates its answer.
- **C memory.** timsort mallocs its merge buffer, at most n/2 words of 8 bytes (4 MB for 10^6 elements), and the engine sorts the narrow kinds (1, 2 and 4 bytes) as 8-byte words in a buffer of n words that it mallocs (8 MB for 10^6 elements), written back only on success; both are freed before the primitive returns.
  When malloc fails the primitive fails with `#'insufficient C memory'` and the facade sorts with the port; a narrow buffer is then untouched, and an 8-byte buffer or an Array holds a permutation of its elements.
  In WebAssembly the C heap grows into the window of old space and never gives memory back (see docs/WebAssembly.md): lane 71 sorts a ByteArray of 16 MiB, for which the engine mallocs 128 MiB, in the default window.
- **The resumable sort** holds, per live sort, its permutation (n int64) and a stack of 256 KB plus a guard page (of which about 4.3 KB is ever touched); at most 64 sorts are live in each of the plugin and the library, and the 65th fails (`#'inappropriate operation'`, the facade then sorts with the port), so 64 nested explicit sorts by a sortBlock run in C and deeper ones in Smalltalk.
  A finished sort returns its stack at once, and one stack is kept for the next sort; the facade frees every sort in an `ensure:`, and `shutdownModule` frees them all.
  The plugin takes at most 2^28 elements in a resumable sort (larger ones go to the port), so that every request is a SmallInteger; the library 2^31 - 1.
- **Blocking.** A sort of modes a, b and c is one primitive call: the VM does nothing else meanwhile, and the web VM does not return to its page (10^6 numbers take 0.1 to 0.3 s natively, about as long in WebAssembly, where the plugin runs the same C; see Benchmarks).
  A sort by a sortBlock returns to the image at every comparison.
- **Ids.** A resumable sort is named by a SmallInteger id, never by a C pointer: each load of the plugin or the library draws a base in [2^32, 2^58) from its clocks and addresses at its first sort, ids count up from it and are never reused while it is loaded, so a stale id fails (`#'not found'`) instead of crashing.
  A Process suspended in the middle of a sort across a snapshot holds an id of the old session, of an earlier load: it names a live sort of the new session only by a 2^-58 chance, so it fails with `#'not found'`, and the facade sorts that collection, untouched, with the port; the facade frees an id only in the session that opened it.

## Primitive contract

### Module, calls, protocol

The module is `TimSortPlugin` (`moduleName` `TimSortPlugin (i)` when built in, as in WebAssembly, `TimSortPlugin (e)` as a shared library).
Its primitives are class-side methods of `TimSortPrimitives`, `<primitive: 'primitiveX' module: 'TimSortPlugin' error: ec>`, whose receiver is ignored.
The protocol is 1 (`TS_PROTOCOL`, slot 1 of `primitiveInfo`, `TimSort protocol`); a change of the arguments or answers of a primitive changes it, and the image takes the plugin only with its own protocol.
The glue answers SmallIntegers, Booleans and the objects it was given; it never runs Smalltalk, never keeps an oop across calls, and never allocates but in `primitiveInfo`.

### Failures

The argument count is checked first; every failure code is one of the standard `PrimErr` set, and a failing primitive leaves its arguments untouched, except after `#'insufficient C memory'` (see Memory and limits):

| Code | Name (`ec`) | For |
|---|---|---|
| 3 | `#'bad argument'` | types, kinds, formats, families, a declined element, the size of perm |
| 4 | `#'bad index'` | ranges |
| 5 | `#'bad number of arguments'` | the argument count |
| 6 | `#'inappropriate operation'` | 64 resumable sorts live; the permutation of a sort not done |
| 7 | `#'unsupported operation'` | the resumable sort where it is not available (WebAssembly, a shadow stack) |
| 8 | `#'no modification'` | a read-only array, buffer or perm |
| 9 | `#'insufficient object memory'` | `primitiveInfo`'s allocations (the VM collects garbage and calls again) |
| 10 | `#'insufficient C memory'` | malloc or mmap failed |
| 11 | `#'not found'` | a stale or unknown id |

Arguments common to several primitives:

- `descending` is `true` or `false`, else `#'bad argument'`; `start` and `stop` are SmallIntegers, else `#'bad argument'`;
- the range is 1-based and inclusive: `1 <= start`, `stop <= size`, `start <= stop + 1`, else `#'bad index'`; n = stop - start + 1;
- `stringClasses` is an Array of exactly 4 classes, `{ByteString. ByteSymbol. WideString. WideSymbol}` (`TimSort stringClasses`), else `#'bad argument'`: an element is a string when its class is identical to one of them, of bytes (not a CompiledMethod) or of 32-bit words;
- an Array is `isArray`, format 2 (Array and its subclasses; a WeakArray is not);
- a buffer of a kind has the instance specification of the kind (formats 9, 10-11, 12-15 or 16-23; never a CompiledMethod), and is neither a BoxedFloat64 nor a LargeInteger.

### The primitives

| # | Primitive | Arguments (first is deepest) | Depth | Answer |
|---|---|---|---|---|
| 1 | `primitiveInfo` | — | 0 | `{1. 'TimSortPlugin 1.0'. 'timsort.c fd70381'. 'native' or 'emscripten'. coroutineSupport. liveCoroutines}` |
| 2 | `primitiveSortObjects` | array, start, stop, descending, stringClasses | 1 | the array, its range sorted |
| 3 | `primitiveSortBuffer` | buffer, kind, start, stop, descending | 0 | the buffer, its range sorted |
| 4 | `primitiveArgSort` | keys, kindOrNil, perm, start, stop, descending, stringClasses | 1 | perm, the indices start..stop sorted by their keys |
| 5 | `primitiveBlockSortNew` | n, descending | 0 | an id in [2^32, 2^59) |
| 6 | `primitiveBlockSortNext` | id, answer | 0 | a request `(i << 32) \| j` ≥ 0, or -1 done, -2 no C memory, -3 aborted |
| 7 | `primitiveBlockSortPermutation` | id, perm | 0 | perm, filled with the 1-based indices |
| 8 | `primitiveBlockSortFree` | id | 0 | true (freed), false (not live) |

- **primitiveInfo**: slot 5 is whether this VM runs resumable sorts (false in WebAssembly and under a shadow stack), slot 6 how many are live.
- **primitiveSortObjects**: checks, in order, the count, `isArray(array)`, the types of start, stop, descending and stringClasses, read-only (`#'no modification'`), the range; then, when n ≥ 2, a pre-pass classifies every element of the range into one family (numbers: SmallInteger, a Float that is not NaN, a normalized LargePositiveInteger or LargeNegativeInteger; or strings of the 4 classes), else `#'bad argument'` with the array untouched; then the slots are sorted in place, all SmallIntegers by their tagged values, other numbers by the exact comparison, strings by code point.
- **primitiveSortBuffer**: checks the count, the kind (a SmallInteger in [1, 8]), the buffer against the kind, the types, read-only, the range over the elements (bytes / element size), then sorts the elements of the range.
- **primitiveArgSort**: keys is an Array (kindOrNil nil, classified as above) or a buffer of the kind; perm an Array of exactly n slots, not the keys, not read-only; the range is over the keys.
  perm is filled with the SmallIntegers start..stop and sorted, stably, by `keys at: each`.
- **Accessor depth.** `primitiveSortObjects` and `primitiveArgSort` read the elements of an argument, so their accessor depth is 1: when one fails, natively, on an element that is a forwarder (whose class reads as nil, which the pre-pass declines), the VM follows the forwarders in the slots of the arguments and calls again.
  The builtin table of WebAssembly carries the same depths:

```c
#ifdef SQUEAK_BUILTIN_PLUGIN
static char _m[] = "TimSortPlugin";
void* TimSortPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveArgSort\000\001", (void*)primitiveArgSort},
	{(void*)_m, "primitiveBlockSortFree\000\000", (void*)primitiveBlockSortFree},
	{(void*)_m, "primitiveBlockSortNew\000\000", (void*)primitiveBlockSortNew},
	{(void*)_m, "primitiveBlockSortNext\000\000", (void*)primitiveBlockSortNext},
	{(void*)_m, "primitiveBlockSortPermutation\000\000", (void*)primitiveBlockSortPermutation},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveSortBuffer\000\000", (void*)primitiveSortBuffer},
	{(void*)_m, "primitiveSortObjects\000\001", (void*)primitiveSortObjects},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};
#else
EXPORT(signed char) primitiveArgSortAccessorDepth = 1;
EXPORT(signed char) primitiveSortObjectsAccessorDepth = 1;
/* … and 0 for the six others */
#endif
```

### Allocation and GC safety

The sorting primitives allocate nothing in the VM and run no Smalltalk, so nothing can move between the checks and the sort: the glue sorts the 8-byte slots of the Array or of perm, or the elements of the buffer, through `firstIndexableField`, as raw words (the same references in the same object: no store check is needed).
The comparisons of objects read the elements through the proxy (`isFloatObject`, `floatValueOf`, the bytes of a LargeInteger, the units of a string) and never write them.
`primitiveInfo` allocates its Array and three ByteStrings with `instantiateClassindexableSize` only, and on a failed allocation fails with `#'insufficient object memory'`, which makes the VM collect garbage and call it again; the glue uses none of the allocators that allocate in eden only (`floatObjectOf`, `signed64BitIntegerFor`, `stringForCString`, …), and no `malloc` (rule R1 of `run-c-tests.sh`).

### The resumable sort

The four coroutine primitives drive a sort that runs timsort (`ts_sort_words` of the engine) on a C stack of its own, over a permutation of n indices, and suspends at each comparison:

1. `primitiveBlockSortNew` (n, descending) opens a sort and answers its id (see Ids above): n a SmallInteger in [0, 2^28], else `#'bad argument'`; `#'unsupported operation'` where the VM cannot switch stacks; `#'inappropriate operation'` when 64 are live; `#'insufficient C memory'`.
2. `primitiveBlockSortNext` (id, answer) resumes it with the answer to the last request, and answers the next one: a request `r = (i << 32) | j` (0-based indices of the collection's elements, i, j < n) asks whether element i must come strictly before element j; the image answers 1 (yes) or 0 (no) at the next call, or a negative answer to abort.
   The answer of the first call is ignored (the image passes 0).
   The image decodes it as `i := r bitShift: -32. j := r bitAnd: 16rFFFFFFFF` and answers `(aBlock value: (c at: j + 1) value: (c at: i + 1)) ifTrue: [ 0 ] ifFalse: [ 1 ]`.
   The end answers -1 (done: the permutation is sorted), -2 (timsort's merge buffer could not be allocated) or -3 (aborted), and keeps answering it; n < 2 answers -1 at once; a stale id fails with `#'not found'`.
3. `primitiveBlockSortPermutation` (id, perm) fills perm, an Array of n slots, with the 1-based original indices in their sorted order (position k takes the element at `perm at: k`): `#'not found'`, `#'bad argument'` for the size, `#'no modification'`, and `#'inappropriate operation'` for a sort that is not done, in that order.
4. `primitiveBlockSortFree` (id) frees it (a pending sort is resumed with aborts to its end first) and answers true, or false for an id that is not live, so that an `ensure:` may free twice.

The facade's loop (`TimSort class>>blockSort:from:to:with:id:backend:state:session:`) calls `Next` once per comparison, evaluates the block between two calls, in the calling Process, keeps the pending request in `state` (so that restarting its frame answers the right request), and frees the sort in an `ensure:` of `TimSort class>>blockSort:from:to:with:`, which assigns the id within the protected block; any failure, -2 or -3 makes it sort that call with the port, on the untouched collection.
The sorts are not tied to a Process: two Processes may each run a sort, interleaved, and a sort may run inside the block of another (nested), each on its own stack; one thread drives them all (the VM's).
A request costs about 0.11 µs through the plugin on the Cog VM (two stack switches of some 35 ns and a named-primitive call), and about 0.27 µs through the library (a uFFI callout), block included (see Benchmarks).

### The engine interface

`include/common/tsEngine.h` is the interface of the engine, which the glue and `tsFFI.c` call: `ts_sort_words` (words by a comparison given by the caller, which may abort the sort), `ts_sort_buffer` (in place for the 8-byte kinds, widened into 8-byte words for the narrow ones), `ts_argsort_buffer` (a permutation of words sorted by typed keys, with a shift and an origin for the tagged 1-based indices of the plugin and the 0-based ones of the library), `ts_number_compare` and `ts_string_compare` (the exact orders above, pure C, so the C tests check them without a VM), and the statuses: 0 ok, -1 done, -2 no memory, -3 aborted, -4 bad argument, -5 unsupported, -6 limit.
Every function is reentrant (a comparison may run another sort), holds no global state, and is `TS_INTERNAL` (hidden).
`include/common/tsCoro.h` is the interface of the resumable sort: the table of ids (`ts_coro_open`, `lookup`, `close`, `close_all`, `live`), `ts_coro_next`, `result`, `size`, `permutation`, and `ts_coro_available` / `ts_coro_impl` (`asm-x86_64-sysv`, `asm-aarch64`, `fibers`).

## Library contract

### `include/ffi/tsFFI.h`

`libTimSortFFI` exports 22 functions, all prefixed `ts_ffi_`, and nothing else (the lane `ffi-exports` of `run-c-tests.sh` checks the dynamic symbols against the header):

- `ts_ffi_protocol` (1), `ts_ffi_version` (`'TimSortPlugin 1.0'`), `ts_ffi_timsort_version` (`'fd70381'`): static strings;
- `ts_ffi_sort_{f64,f32,i64,u64,i32,u32,u16,u8}(base, n, descending)`: a buffer of n elements sorted in place, with the order of the kinds above (any non-zero `descending` is descending); 0, -2 (no memory, as in the plugin) or -4 (n < 0, NULL with n > 0, a base not aligned to the element);
- `ts_ffi_argsort_{i64,u64,f64}(keys, idx, n, descending)`: idx filled with 0..n-1 and sorted, stably, by the keys (idx[0] the index of the smallest key); both 8-byte aligned and distinct (-4 when idx overlaps the keys);
- `ts_ffi_coro_supported`, `ts_ffi_coro_impl` (as `ts_coro_impl`, or `none`), `ts_ffi_coro_new(n, descending)` (an id in [2^32, 2^59), or -4 for n outside [0, 2^31 - 1], -6 when 64 are live, -2), `ts_ffi_coro_next(id, answer)` (a request, -1, -2, -3; -4 for an id not live), `ts_ffi_coro_permutation(id, dst, n)` (0-based indices; -4 when not live, not done or n is not the sort's), `ts_ffi_coro_free(id)` (0, or -4 for an id not live), `ts_ffi_coro_live`, `ts_ffi_coro_size(id)` (the n of a live sort, or -4: the image tells an id not live from a wrong size with it, and keeps no table of the sorts); without a stack switch every `ts_ffi_coro_*` answers -5 before looking at its arguments, and `live` 0.

The library takes ids, never C pointers, as the plugin does: a stale id fails instead of crashing.

### How the image calls it (`TimSort-FFI`)

`TimSortLibrary`, a subclass of `FFILibrary`, finds the library as `libTimSortFFI.so`, `libTimSortFFI.dylib`, `TimSortFFI.dll` or `libTimSortFFI.dll` next to the VM or the image (the FFI's library finders), or at the path given to `libraryPath:`, which resets its callouts and makes `TimSort` choose its backends again; `isAvailable` asks `ts_ffi_protocol` once a session.
It answers the backend protocol of `TimSortPrimitives`, with the same checks in the same order and the same failure codes, so the facade cannot tell the two apart but by what they decline:

- an Array of SmallIntegers is copied into a private buffer of int64, sorted with `ts_ffi_sort_i64`, and stored back; an Array of Floats and SmallIntegers within ±2^53 is argsorted by its float64 values, and the elements themselves put back in that order (a BoxedFloat64 keeps its identity, and the order is stable); anything else is declined;
- a buffer's range is copied into a private pinned buffer of the same element size, sorted, and copied back (the caller's object is never pinned, and is untouched on a failure); the narrow kinds of an argsort are widened in Smalltalk to int64 or float64 keys;
- the sorts by a sortBlock go through `ts_ffi_coro_*`, a callout per comparison; the facade opens them at the highest priority (about 3 µs a sort);
- there is no lock and no state but the availability of the library, and no callback.

## Tests

    sh plugins/TimSortPlugin/tests/run-c-tests.sh --vm-build <native build tree> --emsdk <emsdk>      # C
    sh plugins/TimSortPlugin/tests/run-st-tests.sh build/build/vm <Pharo 12 to 15 image or zip>       # SUnit

`run-c-tests.sh` builds each test with the sources it tests and timsort.c, and runs it in lanes (`--only` chooses some, `TS_TEST_ARGS=--slow` makes them longer):

- `engine` (clang -O2), `engine-asan` (gcc, ASan and UBSan), `engine-wasm64` (emcc -m64 under node): `tests/enginetest.c`, 53.7 million checks: E1 the sort of words against a stable reference (sizes 0 to 10^6, 9 patterns, both directions, exactly n - 1 comparisons on sorted, reversed and equal data, aborts at 70 positions, nested sorts), E2 the 8 kinds bit for bit (NaNs of both signs and any payload, ±inf, -0.0, subnormals, the integer extremes, NaN last ascending and first descending), E3 the argsorts, E4 the exact order of numbers (1,486 edges as integers, LargeIntegers with and without high zero bytes and doubles with both neighbours, 2.2 million pairs against an exact big-integer reference), E5 strings, E6 malloc failures;
- `coro`, `coro-asan`: `tests/corotest.c`, 92,955 checks of the resumable sort (92,947 under ASan): the same permutation as the direct sort and as an independent stable sort, with as many requests as direct comparisons, over 896 sorts; aborts, frees of pending sorts, 16,101 nested sorts with up to 4 suspended, requests answered from random stack depths, 64 live sorts interleaved and the 65th refused, 1000 sorts through the table, ids from a base of the load in [2^32, 2^58), failures of malloc and mmap under `RLIMIT_AS` (n = `TS_CORO_MAX_SIZE` passes the check of the arguments and fails for memory), the guard page, and the stack used (4,344 bytes at most, against a bound of 64 KB); ASan knows of each switch (`__sanitizer_start_switch_fiber`);
- `glue` (with `--vm-build`): the glue `TimSortPlugin.c`, as a shared and as a builtin plugin, against a fake VM (`tests/fakevm.c`, which knows SmallFloat64, BoxedFloat64, LargeIntegers, the 4 string classes, forwarders and read-only objects) under ASan and UBSan: `tests/gluetest.c`, 266,825 checks (266,857 as a builtin plugin): the rows and depths of the exports, `primitiveInfo` and its allocation failures, every arity, 63 single faults and every pair of them in the order of the checks (nothing allocated, stored or changed), 3,000 random calls per family against the engine, every buffer class against every kind, keys of every family, and the coroutine primitives (ids, stale ids, the table, n = 2^28, which ASan refuses to allocate (one warning line) and which must fail with `#'insufficient C memory'`, not as an argument, the permutation's failures, interleaved sorts, `shutdownModule`); first the glue compiles with `-Wall -Werror` in both forms and its exports table through `cmake/emscripten/genPrimitiveTable.cmake`;
- `ffi`, `ffi-asan`: `tests/ffitest.c`, 43,174 checks (43,172 under ASan, which skips n = 2^31 - 1 under `RLIMIT_AS`) of the 22 functions against a reference of its own (also through a built library: `ffitest --lib <libTimSortFFI>`), and `ffi-exports`: the library, built with hidden visibility, exports exactly the `TS_FFI_EXPORT` functions of `tsFFI.h`;
- the rules R1 (no allocator but in `primitiveInfo`, no malloc in the glue), R2 (`timsort.h` only in the engine), R3 (no VM header in the engine, `src/coro`, `src/ffi` and `include/`), R5 (the exports, the builtin table and the accessor depths name the same eight primitives with the same depths), R7 (one `.c` in `src/common`) and V (the pinned timsort.c).

`run-st-tests.sh <vm-dir> <image.zip or .image>` works on a copy of the image: it loads the groups `Core Tests FFI FFITests SmalltalkTests` (`TS_GROUPS`) with `load-timsort.st` from the submodule, checks its banner, runs `TimSort-Tests`, `TimSort-Smalltalk-Tests` and `TimSort-FFI-Tests` with a JUnit file each, and summarises them.
The backends must be those that `TS_EXPECT_BACKEND`, `TS_EXPECT_BLOCK_BACKEND` and `TS_EXPECT_RESUMABLE` name: the backend (`plugin`, `ffi`, `smalltalk` or `any`; by default `plugin` when `<vm-dir>` holds the plugin, else `ffi` when the library is expected), the default of the sorts by a sortBlock (the same values; `smalltalk` by default, on every VM) and `TimSort resumableBackend`, the first resumable C sort (`plugin`, `ffi`, `none` or `any`; by default the backend, and `none` when the backend is `smalltalk`); and the library must be available when `TS_EXPECT_LIBRARY` is `yes` (by default when `<vm-dir>` holds it or `TS_FFI_LIBRARY` names it): a plugin or a library that does not load would only make their tests skip themselves, which the runner counts as passes.
Its environment: those, `TS_FFI_LIBRARY` (the `libTimSortFFI` to use, given to `TimSortLibrary libraryPath:` before the backends are checked), `TS_WORK_DIR` and `TS_TIMEOUT` (3600 s by default).
Three configurations cover the backends; a VM without the plugin is that of a build with `FEATURE_PLUGIN_TIMSORT=OFF`, or a stock one (deleting the plugin from a build tree does not hide it: the VM finds it through its absolute RUNPATH):

    sh run-st-tests.sh build/build/vm <image>                                                      # the plugin
    TS_FFI_LIBRARY=build/build/vm/libTimSortFFI.so sh run-st-tests.sh <vm-dir without the plugin> <image>   # the library
    TS_GROUPS='Core Tests SmalltalkTests' TS_EXPECT_BACKEND=smalltalk sh run-st-tests.sh <vm-dir without the plugin> <image>

The suites run on every backend: `TimSortBackendTestCase` is parametrized by the backend, the plugin (skipped without it) and the port, and `TimSort-FFI-Tests` runs each of its suites again on the library, with the plugin hidden by a test hook.

- `TimSortObjectsTests`: Pharo's `sort` and `sort: [ :a :b | a >= b ]`, object by object, on 28 sizes and 5 shapes of numbers of every class, strings of the 4 classes (é, α, €, U+1F600, NUL), the edges of the numbers, the exact order around 2^53 to 2^62 (every backend, and the argsorts), and what is declined (Characters, Fractions, ScaledDecimals, NaN, nil, mixed families with Pharo's error, LargeIntegers that are not normalized);
- `TimSortBufferTests`: the 8 classes at 12 sizes, both directions and ranges, bit for bit against Pharo's `sort:` with the NaN-last blocks, the bits of NaNs (a signalling one too), read-only buffers;
- `TimSortKeyTests`: argsorts of Arrays, buffers, OrderedCollections, Intervals and Strings, the key block called once per element and in order, keys of every family, ranges;
- `TimSortBlockTests`: the default of mode d is the port on every backend (the block is evaluated by a sorter of `TimSortSmalltalk`, with no resumable sort live, for `timSort:`, `timSorted:`, `sort:with:` and `sort:from:to:with:`); then, through `sort:with:backend:` naming the resumable C sort of the backend of the suite (the plugin, or the library in `TimSort-FFI-Tests`; the port in the suite of the port), mode d against `sort:` (`<=`, `>=`, by size, Associations), n - 1 comparisons on sorted data, strict blocks (against the port), errors in the block, non-Boolean answers, `^` out of the block, a Process terminated inside it, and 300 Processes terminated by another one at any moment of their sorts, the Restart of the frame that evaluates the block, full garbage collections in it, nested sorts 70 deep (beyond the 64 of the table), two Processes interleaved, Warnings resumed, and no sort left live after any of them;
- `TimSortCollectionTests`: Array subclasses, Interval, Symbol, OrderedCollection, SortedCollection and its later `add:` (equal elements after the equal ones, as with `sort:`), Strings, Texts, read-only receivers and the retry of a handler that makes them writable, ranges, `timSorted` copies;
- `TimSortFacadeTests` (a scripted backend: every failure code of every primitive, what the backends are given, the protocol of mode d, a sort resumed in another session, the default that never reaches a resumable sort, `TimSortBackendUnavailable` before anything is looked at), `TimSortSelectionTests` (the choice of the backends, the session, the hooks, the default of mode d in six configurations of the hooks and each of `resumableBackends` named in each, the explicit backends and the unavailable ones), `TimSortPrimitiveTests` (the raw contract of the plugin: the failures in the order of the checks, forwarders made with `becomeForward:` and followed by the VM, the coroutine primitives, `#'unsupported operation'` in WebAssembly), `TimSortDifferentialTests` (1,300 random calls, the plugin against the port, and the resumable sorts of `resumableBackends` against `TimSortSmalltalk` with inconsistent blocks: strict, hashed, random and lying);
- `TimSort-Smalltalk-Tests`: the port against traces of timsort.c (the same comparisons in the same order on 265 inputs) and against Pharo's sort, Texts, and a descending sort stopped by an error, which leaves the slice reversed back;
- `TimSortLibraryTests`: the library's own answers (`ts_ffi_coro_size` too), 3,000 random calls of each selector with the same answers as the plugin, the library path, and `backend: #ffi` without the plugin.

The native jobs of `.github/workflows/build.yml` load the plugin and the library of their zip with `tests/ci-load.st` (see Building), which compiles its own primitive methods and callouts, since the VMMaker image of CI has no TimSort, and prints `TimSortPlugin native timsort.c fd70381 coroutines yes <comparisons> ok` and `TimSortFFI 1 fd70381 <switch> coroutines yes <comparisons> ok`: it drives the resumable sort of each through its primitives and callouts, whatever the facade's default, so that CI runs the stack switches of every OS.
In the WebAssembly build, lane 71 of `make wasm-check` runs `TimSort-Tests` and `TimSort-Smalltalk-Tests` with the node and the web VMs (on the backend `#plugin`, with the sorts by a sortBlock in the port and no resumable C sort, which the summary line of `tests/wasm/st/timsort-lane.st` names with the skipped tests), checks that the resumable sort is reported unsupported, that `backend: #plugin` signals `TimSortBackendUnavailable` and that a sort by a sortBlock still answers Pharo's result, and sorts 2^20 SmallIntegers, a Float64Array of 32 MiB and a ByteArray of 16 MiB in the default window of old space (see docs/WebAssembly.md).

Measured on 2026-11-17, on Linux x86_64 (the native CoInterpreter, clang 18) and with `make wasm` (Emscripten 6.0.10, the image Pharo 15 b117), from the uncommitted tree of branch pharo-12-timsort (38fe98eec) and timsort.st:

- `run-c-tests.sh` with `--vm-build` and `--emsdk`: every lane and rule passed, in 2 min 18 s;
- `run-st-tests.sh` on Pharo 12 b1599 and Pharo 15 b117, each in the three configurations (the plugin; the library only, next to the VM and through `TS_FFI_LIBRARY`; Smalltalk only): no failure and no error, `TimSort-Tests` 148 tests, `TimSort-Smalltalk-Tests` 34, `TimSort-FFI-Tests` 82 (after mode d became the port by default: run again with these counts, banners `blockSortBackend #smalltalk` and `resumableBackend` `#plugin`, `#ffi`, `#ffi` and `nil`);
- `ci-load.st` with the VMMaker image's Pharo 13, and Pharo 15 (Pharo 12 before the review fixes): `TimSortPlugin native timsort.c fd70381 coroutines yes 13 ok`, `TimSortFFI 1 fd70381 asm-x86_64-sysv coroutines yes 13 ok`;
- lanes 05, 10, 20, 50 (419 rows in 19 modules) and 71 (7 checks, 205 s; the suite 180 tests, 2 skipped) of `make wasm-check` passed (05, 20, 50 and 71 again after mode d became the port by default; 10, 68 and 69 before, which it does not touch).

## Benchmarks

`plugins/TimSortPlugin/bench` times TimSort against Pharo's own sort, through each backend, on the native Cog and Stack VMs and in the WebAssembly VMs (the node VM, and the web VM driven in node):

    sh plugins/TimSortPlugin/bench/run-bench.sh --cog <Cog build> --stack <Stack build> --wasm <make wasm build> \
       --node <node> --image <Pharo 12 to 15 image or zip> --out $OUT --repeat 2
    python3 -I plugins/TimSortPlugin/bench/summarize.py $OUT/*.tsv

- `run-bench.sh` prepares the image once, natively (`prepare-image.st` files in `tests/load-timsort.st` with the groups `Core` and `FFI`, then `TSBench.st`), then runs `run-bench.st` on a fresh copy of it with each VM in turn, pinned to one CPU (`taskset -c 2`), and the whole matrix twice (`--repeat`); each run writes a TSV file, which `summarize.py` turns into tables, and checks the gates below. `--vms` runs some of the VMs, `--cpu` pins them elsewhere.
- The data are made in the image from Park and Miller's random numbers of one seed, the same on every VM, at 10^3, 10^4, 10^5 and 10^6 elements (10^6 natively only), in five shapes: `random`, `sorted` and `reversed` (the random values sorted), `fewUnique` (10 distinct values in random order) and `nearlySorted` (sorted, then n/100 random pairs swapped).
- The cases, each timed as the public API sorts it and as Pharo does:

| case | data | TimSort | Pharo |
|---|---|---|---|
| `int` (mode a) | an Array of SmallIntegers in [0, n) | `c timSort` | `c sort`, `c asSortedCollection` |
| `float` (mode a) | an Array of Floats in [0, 1000) | `c timSort` | `c sort`, `c asSortedCollection` |
| `string` (mode a) | an Array of the ByteStrings of integers in [0, 10 n) | `c timSort` | `c sort`, `c asSortedCollection` |
| `f64`, `i32`, `u8` (mode b) | a Float64Array, an IntegerArray, a ByteArray | `c timSort` | `c sort` |
| `key` (mode c) | an Array of Associations int -> index | `c timSortBy: [ :e \| e key ]` | `c sort: [ :a :b \| a key <= b key ]` |
| `block` (mode d) | an Array of SmallIntegers | `c timSort: [ :a :b \| a <= b ]` (the default, the port), and `TimSort sort: c with: [ :a :b \| a <= b ] backend: #plugin` or `#ffi` (the resumable C sorts) | `c sort: [ :a :b \| a <= b ]` |

- `asSortedCollection` is not timed on `fewUnique` data beyond 10^4 elements: SortedCollection sorts by a quicksort that is quadratic on few distinct values (1.7 s for 10^5 elements on Cog, where `sort` takes 34 ms; 35 s in WebAssembly), and 10^6 would take minutes per call.
- TimSort is timed through each backend, set with the test hook `#backend` so that every size goes through it: `plugin`, `ffi` and `smalltalk` (the port); the library and the port take what they decline to the port, as the facade does (the strings of `ffi` are the port's).
  For `block`, the series `plugin` and `ffi` name the resumable C sort (`sort:with:backend:`), skipped where it does not exist (WebAssembly), and `smalltalk` times `timSort:` itself, the default.
- Before a series is timed, its result is compared with Pharo's `sort`, object by object (`asSortedCollection`, which is not stable, by value); a sample sorts k fresh copies of the data, made before it and not timed, k doubled until a sample lasts 20 ms (50 ms in WebAssembly, whose clock has a resolution of 1 ms), 5 samples natively and 3 in WebAssembly, each after a scavenge; the median sample counts, and the median of the runs.
- The gates, which `summarize.py` checks: every result equal to Pharo's, no error, and natively, on the random data at 10^5 and 10^6 elements, the plugin at least 1.5 times faster than Pharo's `sort` on every case of modes a and b, and 1.2 times on mode c (whose time includes the n evaluations of the key block and the permutation applied in Smalltalk).

### Results

Measured on 2026-11-17 with `run-bench.sh --repeat 1`, the three VMs at the same time, each pinned to a CPU of its own (Cog on 2, node on 3, Stack on 4); every value is the median sample of its series.

- Machine: 2 x Intel Xeon Gold 6238R @ 2.20GHz, the machine of docs/SimdUTF.md, Linux 6.8.0-139-generic.
- Sources: branch pharo-12-timsort at 38fe98eec with its uncommitted TimSortPlugin tree, timsort.c fd70381, and the working tree of timsort.st. VMs: the native CoInterpreter and StackVM (Release, Ubuntu clang 18.1.3), and `make wasm` (Emscripten 6.0.10, wasm64) as `node/pharo` (wasm-node) under node 24.19.0; the web VM was not timed (lane 71 runs the suite in it).
- Image: Pharo 12.0 SNAPSHOT b1599, prepared by `prepare-image.st`.
- A run took 14 minutes with Cog, 27 with Stack and 9 with node.

Gates: 1790 results equal to Pharo's sort, object by object, no error; every gate passes (the smallest speedups of the plugin natively on random data at 10^5 and 10^6: 1.66x for strings and 1.42x for mode c, on Cog).

Pharo's `sort` time over TimSort's (above 1, TimSort is faster) at 10^6 elements natively and 10^5 in node, by backend; `ffi` and `port` are the library and `TimSortSmalltalk` through the test hook, with what they decline sorted by the port (the strings of `ffi`); for `block`, `plugin` and `ffi` are the resumable C sorts and `port` is the default of `timSort:`, and `plugin` in node is the port (this run predates `sort:with:backend:`, and the plugin then fell back to the port where it has no resumable sort; the bench now skips it there):

| case | shape | Cog: sort ms | plugin | ffi | port | Stack: sort ms | plugin | ffi | port | node (10^5): sort ms | plugin | port |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| int | random | 457 | 3.12x | 2.46x | 0.69x | 1363 | 9.19x | 4.61x | 0.72x | 620 | 43.5x | 0.74x |
| int | sorted | 233 | 68.1x | 5.35x | 11.2x | 736 | 214x | 4.85x | 12.0x | 358 | 424x | 9.81x |
| int | reversed | 240 | 26.4x | 4.93x | 4.64x | 769 | 82.9x | 4.88x | 4.82x | 370 | 247x | 4.16x |
| int | fewUnique | 390 | 6.50x | 3.84x | 1.15x | 1220 | 20.0x | 5.82x | 1.32x | 576 | 86.9x | 1.32x |
| int | nearlySorted | 343 | 20.3x | 5.97x | 1.48x | 1116 | 64.7x | 6.74x | 2.21x | 519 | 216x | 2.75x |
| float | random | 499 | 2.11x | 1.29x | 0.64x | 1395 | 5.89x | 2.68x | 0.70x | 638 | 19.0x | 0.73x |
| float | sorted | 248 | 23.6x | 1.60x | 11.0x | 766 | 71.9x | 2.85x | 12.0x | 366 | 129x | 10.0x |
| float | reversed | 243 | 21.7x | 1.76x | 6.62x | 783 | 68.6x | 2.87x | 6.78x | 379 | 128x | 5.41x |
| float | fewUnique | 418 | 4.25x | 1.93x | 1.11x | 1269 | 12.7x | 3.63x | 1.31x | 598 | 36.8x | 1.31x |
| float | nearlySorted | 375 | 11.0x | 2.33x | 1.55x | 1160 | 34.2x | 3.95x | 2.26x | 538 | 82.8x | 2.60x |
| string | random | 935 | 1.66x | 0.81x | 0.80x | 2610 | 4.51x | 0.82x | 0.82x | 953 | 11.2x | 0.79x |
| string | sorted | 505 | 8.91x | 7.87x | 7.90x | 1630 | 27.2x | 10.3x | 10.0x | 546 | 64.2x | 9.25x |
| string | reversed | 498 | 8.71x | 5.95x | 5.97x | 1472 | 27.0x | 7.26x | 7.27x | 553 | 62.3x | 5.70x |
| string | fewUnique | 691 | 3.25x | 1.42x | 1.42x | 2311 | 10.8x | 1.70x | 1.69x | 921 | 16.3x | 1.55x |
| string | nearlySorted | 785 | 6.36x | 2.79x | 2.81x | 2392 | 20.4x | 3.42x | 3.37x | 791 | 46.5x | 3.28x |
| f64 | random | 887 | 4.96x | 3.60x | 0.98x | 3181 | 18.3x | 13.4x | 1.43x | 1057 | 69.3x | 1.13x |
| f64 | sorted | 654 | 251x | 9.72x | 5.16x | 2520 | 994x | 40.4x | 9.74x | 897 | 1946x | 9.15x |
| f64 | reversed | 652 | 160x | 9.23x | 4.69x | 2485 | 624x | 39.0x | 8.45x | 909 | 1419x | 7.58x |
| f64 | fewUnique | 747 | 10.4x | 5.35x | 1.65x | 2853 | 41.0x | 21.7x | 2.52x | 1047 | 155x | 2.00x |
| f64 | nearlySorted | 718 | 37.7x | 8.48x | 2.41x | 2759 | 148x | 34.7x | 4.18x | 1006 | 447x | 3.88x |
| i32 | random | 536 | 3.68x | 2.55x | 0.70x | 2082 | 14.6x | 10.3x | 1.06x | 731 | 54.1x | 0.86x |
| i32 | sorted | 469 | 156x | 9.39x | 4.41x | 1691 | 572x | 36.7x | 8.60x | 541 | 899x | 7.84x |
| i32 | reversed | 470 | 104x | 9.00x | 3.80x | 1715 | 391x | 35.8x | 6.91x | 573 | 741x | 5.73x |
| i32 | fewUnique | 550 | 8.97x | 4.38x | 1.40x | 2029 | 34.4x | 17.0x | 2.03x | 685 | 112x | 1.51x |
| i32 | nearlySorted | 518 | 30.5x | 28.2x | 2.04x | 1934 | 116x | 108x | 3.45x | 641 | 306x | 3.07x |
| u8 | random | 447 | 4.93x | 4.90x | 0.76x | 1286 | 14.4x | 14.2x | 0.93x | 585 | 60.0x | 0.90x |
| u8 | sorted | 243 | 98.6x | 75.2x | 5.45x | 722 | 296x | 226x | 6.21x | 337 | 599x | 5.18x |
| u8 | reversed | 254 | 47.4x | 41.8x | 2.95x | 776 | 147x | 129x | 3.19x | 382 | 344x | 3.03x |
| u8 | fewUnique | 417 | 7.04x | 6.94x | 0.92x | 1221 | 20.8x | 20.5x | 1.18x | 562 | 91.8x | 1.26x |
| u8 | nearlySorted | 363 | 27.2x | 26.0x | 1.58x | 1109 | 84.9x | 79.9x | 2.26x | 510 | 281x | 2.60x |
| key | random | 774 | 1.42x | 1.05x | 0.62x | 1795 | 2.98x | 2.03x | 0.73x | 643 | 7.94x | 0.66x |
| key | sorted | 390 | 1.28x | 0.82x | 1.17x | 949 | 2.66x | 1.52x | 2.14x | 365 | 5.98x | 3.17x |
| key | reversed | 399 | 1.27x | 0.83x | 1.07x | 999 | 2.74x | 1.58x | 1.82x | 387 | 6.14x | 2.32x |
| key | fewUnique | 616 | 1.57x | 1.08x | 0.84x | 1557 | 3.52x | 2.17x | 1.12x | 609 | 8.96x | 1.13x |
| key | nearlySorted | 531 | 1.60x | 1.07x | 1.04x | 1388 | 3.67x | 2.14x | 1.61x | 545 | 8.65x | 2.03x |
| block | random | 533 | 0.24x | 0.10x | 0.80x | 1416 | 0.47x | 0.15x | 0.75x | 594 | 0.72x | 0.72x |
| block | sorted | 307 | 2.21x | 0.83x | 14.3x | 823 | 3.90x | 1.25x | 13.7x | 343 | 9.80x | 9.66x |
| block | reversed | 319 | 1.76x | 0.67x | 5.87x | 856 | 3.13x | 1.00x | 5.49x | 365 | 4.29x | 4.35x |
| block | fewUnique | 470 | 0.56x | 0.23x | 1.34x | 1299 | 1.11x | 0.35x | 1.44x | 559 | 1.30x | 1.29x |
| block | nearlySorted | 425 | 1.08x | 0.45x | 1.76x | 1196 | 2.18x | 0.70x | 2.43x | 524 | 2.80x | 2.82x |

- **Modes a and b.** On random data the plugin sorts 10^6 SmallIntegers in 147 ms on both native VMs (Pharo's `sort` takes 457 ms on Cog and 1363 ms on Stack), a Float64Array in 179 ms (887 and 3181 ms), and 10^5 SmallIntegers in 14 ms in node (620 ms): the C code is the same everywhere, so the speedup grows with the cost of Pharo's sort, from 1.7 to 5 times on Cog to 11 to 69 times in WebAssembly.
  Floats and strings are compared through the proxy (a few calls per comparison) and gain least: 2.1x and 1.7x on Cog.
  Timsort is adaptive: sorted, reversed and nearly sorted data take about one comparison an element, where Pharo's merge sort takes n log n / 2, so there the plugin is 6 to 251 times faster on Cog.
- **Mode c** (by keys) evaluates the key block n times and applies the permutation in Smalltalk: 1.3x to 1.6x on Cog, 2.7x to 3.7x on Stack, 6x to 9x in node.
- **Mode d** (by a sortBlock) through the resumable C sort costs about 0.11 µs a comparison over the block on Cog (a named-primitive call and two stack switches), more than Pharo's whole comparison: on random data it is 4 times slower than Pharo's `sort:` (2205 ms against 533 at 10^6), and through the library, a uFFI callout a comparison, 10 times slower; on sorted data, with few comparisons, it is 2.2 times faster.
  The port runs the same comparisons in Smalltalk: 0.8x on random data and 14x on sorted data on Cog (669 ms and 21 ms at 10^6), and 0.72x random, 9.8x sorted in node.
  This is why the port is the default of mode d on every VM (see below).
- **The port** (`TimSortSmalltalk`) takes 1.0 to 1.6 times Pharo's time on random data (Pharo's merge sort is simpler per comparison), and on Cog is 4.4 to 14 times faster on sorted data, 3.0 to 6.6 times on reversed data and 1.5 to 2.8 times on nearly sorted data (mode c, whose time is mostly the key block and the permutation: 1.0x to 1.2x): without the plugin and the library TimSort is at most about 1.6 times slower than Pharo's sort.
- **Pharo's `asSortedCollection`** is faster than `sort` on distinct numbers (1.2x to 1.8x on Cog) but not stable, and quadratic on few distinct values: 15 ms for 10^4 elements of 10 values against 2.8 ms for `sort`, and 1.7 s for 10^5 against 34 ms; TimSort has no such case (6.5x Pharo's `sort` at 10^6 on Cog).

### Mode d: why the port is the default

The resumable C sort and the port run the same algorithm and make the same comparisons in the same order, so they answer the same result for the same block (`TimSortDifferentialTests`, `TimSort-Smalltalk-Tests`); only the cost around each comparison differs.
In C a comparison is a return to the image, the evaluation of the block, and a primitive call (or a uFFI callout, with the library) that switches to the sort's stack and back; in the port it is the evaluation of the block from a Smalltalk method.
What the C side saves, the bookkeeping of the runs and the merges, is cheap next to a comparison that crosses into the image and back, on the Stack VM too.
The benchmark above times both on the same data, every size from 10^3 to 10^6 and every shape natively; how many times faster the port was than each resumable C sort (the time of the C sort over the port's), the range over the four sizes:

| shape | Cog: port vs plugin | port vs ffi | Stack: port vs plugin | port vs ffi |
|---|---:|---:|---:|---:|
| random | 2.8–3.3x | 6.6–7.7x | 1.5–1.6x | 4.7–5.0x |
| sorted | 6.2–6.5x | 13.5–17.2x | 3.4–3.5x | 9.8–11.0x |
| reversed | 3.3–3.4x | 7.4–8.7x | 1.7–1.8x | 5.1–5.5x |
| fewUnique | 2.4–2.5x | 5.6–5.9x | 1.2–1.4x | 3.8–4.2x |
| nearlySorted | 1.6–3.0x | 4.0–6.9x | 1.1–1.5x | 3.5–4.7x |

The port was faster in all 40 native cases (1.1 to 6.5 times than the plugin, 3.5 to 17 times than the library), and in WebAssembly there is no resumable sort.
So `timSort: aBlock` and `TimSort sort:with:` run the port on every VM, with the same results as before; the resumable C sort remains for `TimSort sort: c with: aBlock backend: #plugin` (or `#ffi`), tested by the same suites, and timed by the bench, should a VM or a JIT change the balance (with a block much costlier than the switches the two converge, since both evaluate it the same number of times).

## Licences

- timsort.c has no licence file: it is a port of CPython's `listsort` (`Objects/listobject.c` of CPython 3.13 and 3.14), a derivative work of CPython under the PSF License Agreement for Python, version 2.
  `plugins/TimSortPlugin/LICENSE.timsort` holds the notice that clause 2 of that agreement asks to retain: where timsort.c comes from, the summary of the changes made that clause 3 asks for, "Copyright (c) 2001-2024 Python Software Foundation; All Rights Reserved", and the full text of the agreement.
  It is installed as `LICENSE.timsort` next to the plugin on Linux and Windows (`build/vm`, the `lib` directory of the Linux zip) and in `Pharo.app/Contents/Resources` on macOS, and goes into the `THIRD-PARTY-NOTICES.txt` of the WebAssembly VMs, in the section of timsort (`cmake/timsort.cmake` adds it to the notice that the fetch of the archive records).
- The package `TimSort` and the rest of the submodule `timsort.st` are under its MIT licence (its `LICENSE`); the port `TimSort-Smalltalk` is a translation of timsort.c, so it is a derivative work of CPython too, under the same PSF agreement: `timsort.st/LICENSE.timsort` holds its notice (the agreement, the copyright line, and the changes, those of timsort.c and the translation), and its README says so.

This is not legal advice.

## Limitations

- 64-bit VMs only.
- macOS, Windows and Linux aarch64 are untested until CI runs the steps that load the plugin and the library: the switches of AArch64 and Mach-O and the Fibers of Windows have never run; the performance on arm64 is not measured.
- The sorts by a sortBlock run in the port by default on every VM: with a cheap block they are about 1.25 to 1.7 times slower than Pharo's `sort:` on random data, and faster on sorted, reversed or nearly sorted data; use modes a, b and c where they apply, whose comparisons run in C.
- WebAssembly has no resumable sort (no stack switching without ASYNCIFY or JSPI), and neither has a process with a shadow stack (CET, GCS): there `sort:with:backend: #plugin` (or `#ffi`) signals `TimSortBackendUnavailable`.
- The resumable sort costs about 0.11 µs a comparison on top of the block: with a cheap block it is slower than Pharo's `sort:` on random data, faster on sorted or nearly sorted data, where it makes few comparisons, and slower than the port, which makes the same comparisons in Smalltalk, on every shape measured (see Benchmarks, Mode d).
- Natively the plugin compares Floats and strings through the proxy, a few calls per comparison: on random data on Cog it is 1.7 times Pharo's speed for strings and 2.1 times for Floats, where SmallIntegers reach 3.1 times and buffers 3.7 to 5 times (4.5 to 18 times on the Stack VM).
- A sort of modes a, b and c is one primitive call, which blocks the VM, and the web VM's page, meanwhile.
- At most 64 resumable sorts live at once, in each of the plugin and the library; deeper nests of explicit sorts by a sortBlock run in the port.
- Ids of resumable sorts are not kept across a snapshot: a Process saved in the middle of an explicit resumable sort by a sortBlock finishes it in the port after the restart (see Memory and limits).
