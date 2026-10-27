# SimdJSON: a JSON reader on simdjson

The SimdJSONPlugin parses JSON with [simdjson](https://github.com/simdjson/simdjson) 5.0.5, and `plugins/SimdJSONPlugin/st/SimdJSON.st` gives Pharo 15 a reader with the API of NeoJSON's `NeoJSONReader` on top of it: `SimdJSONReader`, which needs no NeoJSON.
The plugin builds the whole value of a JSON document in one primitive call: Dictionaries and Arrays built in C when the reader is configured as NeoJSON's defaults, or else a tree that the reader turns into the configured classes, or walks for NeoJSON's mappings.
Values, number classes, string classes and stream positions are NeoJSON's, with the exceptions listed below; the errors are classes of its own.
The plugin builds natively (Cog and Stack VMs) and in the WebAssembly VMs, where it is a builtin plugin.

Files, in `plugins/SimdJSONPlugin`:

- `src/common/SimdJSONPlugin.c`: the primitives and the builder of the objects (C, with the VM's headers);
- `src/cxx/sjEngine.cpp`: the parser, its cache, the budget and the walk of simdjson's DOM (C++17, without the VM's headers);
- `src/common/sjText.c`, `include/common/sjText.h`: the scanner of the end of a value, the transcoding and the sizes (C99);
- `include/common/sjBridge.h`: the interface between the two;
- `include/win/sjNoThreads.h`: included first on Windows, to keep simdjson's threads off;
- `st/SimdJSON.st`: the Smalltalk side;
- `st/SimdJSON-Tests.st`, `st/SimdJSON-NeoTests.st`, `tests/`: the tests;
- `bench/`: the benchmark.

## Building

The plugin is on by default, on every platform: `FEATURE_PLUGIN_SIMDJSON=ON`.
`-DFEATURE_PLUGIN_SIMDJSON=OFF` leaves it out, along with the download of simdjson: no C++ is compiled then (CMake still checks for a C++ compiler, which `project(PharoVM)` enables in every configuration).

    cmake -S . -B build -DFLAVOUR=CoInterpreter            # the plugin is in
    cmake -S . -B build -DFEATURE_PLUGIN_SIMDJSON=OFF      # it is not

simdjson is taken from its release archive, `https://github.com/simdjson/simdjson/archive/refs/tags/v5.0.5.tar.gz`, checked against its SHA256 (`50bb1776…f78f571`), by `cmake/simdjson.cmake` with the function of the WebAssembly libraries (`cmake/emscripten/deps/fetch.cmake`), natively too:

- the archive comes from `WASM_DEPS_DIR` when that directory holds `simdjson-5.0.5.tar.gz`, which builds offline, else from `<build>/downloads`, else from GitHub at configure time (8.8 MB);
- `-DSIMDJSON_SOURCE_DIR=<tree>` takes a simdjson 5.0.5 source tree instead (its version is checked).

simdjson's own CMake files are not used: its amalgamation `singleheader/simdjson.cpp` is compiled as the static library `pharo_simdjson`, in `<build>/deps`, with `-O3 -g0 -fno-exceptions`, C++17, no threads, and position-independent code natively; the plugin's C++ is compiled the same way.
Neither has debug information.
The plugin is a shared library natively (`libSimdJSONPlugin.so`), which needs the C++ runtime of the compiler (`libstdc++.so.6` with g++ and clang on Linux).
On Linux simdjson's `<iostream>` is left out (`-D_GLIBCXX_IOSTREAM=1`; simdjson uses only `<ostream>`): from GCC 13 on its static initializer needs `GLIBCXX_3.4.32`, which the libstdc++ of Ubuntu 22.04, Debian 12 or RHEL 9 lacks, while the rest of the VM runs there.
The plugin then needs `GLIBCXX_3.4.21` (GCC 5) at most: `objdump -T build/vm/libSimdJSONPlugin.so | grep -o 'GLIBCXX_[0-9.]*' | sort -uV | tail -1` shows it.
simdjson chooses its kernel at run time (haswell, westmere, icelake... on x86_64; arm64 on aarch64); the WebAssembly VMs use its portable `fallback` kernel.

Platforms:

- Linux (x86_64) and Emscripten (the node and web VMs of `make wasm`) are built and tested;
- macOS (a `.dylib` against the system's libc++) and Windows (MSYS2 UCRT64 with clang on x86_64, MSYS2 CLANGARM64 on arm64, where the C++ runtime is libc++; the install step of `cmake/Windows.cmake` bundles the C++ runtime DLLs) are written for, but **untested**: no build of them has run.
  On Windows, `-U_MT` cannot keep simdjson's threads off, since the mingw-w64 `_mingw.h` defines `_MT` again whenever it is missing; `include/win/sjNoThreads.h`, included first (`-include`) in simdjson.cpp and sjEngine.cpp, reads `_mingw.h` and then undefines `_MT` (checked with the headers of llvm-mingw, which are those of MSYS2).
  Build with `-DFEATURE_PLUGIN_SIMDJSON=OFF` there if the build breaks.
  Each native job of `.github/workflows/build.yml` loads the plugin of its zip with `plugins/SimdJSONPlugin/tests/ci-load.st` (and, on Linux, checks that it needs no libstdc++ newer than GLIBCXX_3.4.21), so the first CI run tells;
- Linux aarch64 compiles simdjson's arm64 kernel, which has not run either.

In the WebAssembly VMs the plugin links libc++ and libc++abi into `pharo.wasm` and `pharo-web.wasm`, about 120 KB, and the C++ link driver links them; their licences, and simdjson's, go into `THIRD-PARTY-NOTICES.txt` (see docs/WebAssembly.md).

## Loading the Smalltalk side

`SimdJSON.st` is one file in the chunk format of Pharo 15, with no dependency but the plugin.
`SimdJSONReader` is a subclass of `SimdJSONMapper`, a port of NeoJSON's mapper and mapping classes (the reading side), and it is strict RFC 8259.
NeoJSON need not be in the image; when it is, whether it was loaded before or after, nothing changes: no NeoJSON class is in the hierarchy of `SimdJSONReader` or of its errors, and no method of the package `SimdJSON` names one (`SimdJSONReaderTests>>testStandalone` checks both).

    pharo --headless Pharo.image --no-default-preferences st --save --quit plugins/SimdJSONPlugin/st/SimdJSON.st

The file prints `SimdJSON: SimdJSONReader loaded, a subclass of SimdJSONMapper`.
Filing it in again is harmless.
Over an image where an older `SimdJSON.st` made `SimdJSONReader` a subclass of `NeoJSONReader` (its former neo mode), the class is made again; its class-side settings (`maxInputUnits:`, `nativeDictionaries:`) go back to their defaults.
Without the plugin, reading signals `SimdJSONResourceError` ('SimdJSONPlugin not available').

## API

`SimdJSONReader` answers NeoJSON's protocol, so replacing `NeoJSONReader` with `SimdJSONReader` is the way to use it:

    SimdJSONReader fromString: '{"a":[1,2.5,1e2,"x"]}'.             "a Dictionary"
    (SimdJSONReader on: aReadStream) next.
    SimdJSONReader fromString: '{"x":1,"y":2}' as: Point.            "with a mapping"
    (SimdJSONReader on: '{"x":1,"y":2}' readStream) mapInstVarsFor: Point; nextAs: Point.
    (SimdJSONReader on: ndjson readStream) upToEnd.                   "every value"

Class side:

| selector | |
|---|---|
| `on:`, `fromString:`, `fromString:as:` | as NeoJSON |
| `onBytes:`, `fromBytes:`, `fromBytes:as:` | UTF-8 bytes (a ByteArray) instead of a String |
| `pluginAvailable`, `pluginInfo`, `implementationName`, `simdjsonVersion`, `platform` | the plugin: whether it is there, its version, simdjson's kernel, `'native'` or `'emscripten'` |
| `maxInputUnits`, `maxInputUnits:` | the largest value the plugin parses, in characters or bytes: 16 Mi in WebAssembly, none natively; nil removes it |
| `nativeDictionaries:` | false turns the Dictionaries built in C off until it is set back to true; the setting is kept by an image save |
| `statistics`, `resetStatistics` | counters: `reads fastValues overBudget grows numberTexts nativeDictionaries tooLarge` |
| `releaseParser` | frees the parser's memory (it grows to the largest document read) |

Instance side, besides NeoJSON's (`next`, `nextAs:`, `nextListAs:`, `upToEnd`, `atEnd`, `failIfNotAtEnd`, `mapClass:`, `listClass:`, `propertyNamesAsSymbols:`, `for:do:`, `for:customDo:`, `mapInstVarsFor:`...):

| selector | |
|---|---|
| `exactNumbers:` | false reads every number with a fraction or an exponent as a Float (`1e2` is `100.0`) |
| `nativeDictionaries:` | false reads into NeoJSON's exact Dictionary layout (see below) |

The stream given to `on:` decides how the input is read:

- a `ReadStream` on a String (ByteString or WideString): the plugin reads the String itself, from the stream's position up to its limit, and the stream is left after the value and the white space that follows it, as NeoJSON leaves it.
  Nothing is read ahead, and reads of the stream between two `next`s are seen;
- a `ReadStream` on a ByteArray: UTF-8, positions in bytes (NeoJSON cannot read bytes);
- any other stream (a socket, a `ZnCharacterReadStream`): the reader reads it to its end in `on:` (which waits for the end of a socket stream), so what is read from that stream afterwards is not seen by the reader.
  `close` closes the stream given to `on:`, as NeoJSON does.

Errors: `SimdJSONParseError` (with simdjson's error `code`, the `position` of the value and simdjson's message, or NeoJSON's message for an error of its rules), `SimdJSONResourceError` (C memory, the input cap, no plugin), `SimdJSONMappingNotFound`, and `OutOfMemory`; the first three are subclasses of `Error`, where NeoJSON raises `NeoJSONParseError` and `NeoJSONMappingNotFound`.
When a read at the top level fails (`nextAs:`, `nextListAs:`, `parseMap`...), the reader and the stream are back before the value when the handler runs, so the handler can read it again: `[ reader nextAs: Point ] on: Error do: [ :e | reader next ]`.

## Semantics against NeoJSON

- **Numbers** are NeoJSON's by default: integers of any size; `1e2` is the Integer 100, `1e-2` the Fraction 1/100, `1e308` a LargePositiveInteger; a number with a fraction and no exponent is the same Float as NeoJSON's, bit for bit.
  The plugin answers the text of the numbers with an exponent, and of the integers beyond 64 bits, which the image converts with NeoJSON's rules.
  A quick scan skips that work for inputs without an exponent.
  `exactNumbers: false` reads them all as Floats instead, which is cheaper.
- **Strings** get the narrowest class, as NeoJSON's: a ByteString when every character is below 256, else a WideString.
  Strings are never shared between values, keys included.
- **Dictionaries**: with NeoJSON's defaults (`mapClass` Dictionary, `listClass` Array, String keys) the Dictionaries and Arrays are built in C.
  They are `=` to NeoJSON's, but their capacity, and so the order in which they enumerate their keys, may differ.
  `nativeDictionaries: false` builds NeoJSON's exact layout, at a cost.
  The C build relies on the layout of Dictionary and Association and on `String>>hash`, which a self-test checks once per session; a failure turns it off for the session and says so on the Transcript.
  With another `mapClass`, `listClass` or `propertyNamesAsSymbols: true`, the reader builds the configured classes; Symbol keys are interned once per distinct key.
- **Duplicate keys**: the last one wins, as in NeoJSON.
- **What simdjson refuses**: leading zeros (`01`), `1.`, a number out of the Float range (`1e400`, `1.0e309`), raw control characters, a form feed inside a value, a lone trail surrogate, a byte order mark, trailing garbage inside a value (`123abc`).
  NeoJSON reads them; `SimdJSONReader` signals `SimdJSONParseError`, and the stream stays before the value.
  NeoJSON's own test `NeoJSONReaderTests>>testIntegers` reads leading zeros, so the compatibility run skips it (see Tests).
  A number that simdjson reads but NeoJSON's rules refuse (`1e-1074`, whose exact value underflows) signals `SimdJSONParseError` with NeoJSON's message.
- **Trailing input** is not checked by `next`, as in NeoJSON: `failIfNotAtEnd` does.
- **NDJSON** and other sequences of values: `next` parses one value at a time, without rescanning the rest, and `upToEnd` answers an Array of them all.

## Memory and limits

- In WebAssembly, and on a VM whose old space has a maximum (`Smalltalk vm parameterAt: 67`), the plugin checks before building anything that the objects of a value above 64 Ki units fit the free old space, and the reader grows the heap otherwise (`Smalltalk growMemoryByAtLeast:`), so a large document gives the image the memory it needs rather than failing half way.
  The objects take at most 32 bytes per byte of input, plus 4 KB (the byte bound, `32 x B + 4096`); when that bound is over the free old space, the plugin measures the exact need with a walk of the parsed document.
  The need is that of every object the call allocates: with Dictionaries built in C it includes the key and the value that a duplicate key replaces, which are allocated and then dropped.
  Natively, where old space grows on demand, the build runs without that check (the walk would cost some 12-15% of a large read): a shortage fails the primitive, the VM collects garbage and calls it again on the parse it kept, and the reader then measures the need, grows the heap and reads once more.
  At a hard limit of the heap, the result is an `OutOfMemory` signal, and the VM lives on.
- A failed allocation never repeats the parse: the plugin keeps the last parse for the VM's retries.
- The parser's buffers grow to the largest document parsed and stay (`releaseParser` frees them).
- Objects nest at most 1024 deep.
- **WebAssembly**:
  - a value is limited to 16 Mi characters (or bytes) by default, since the 32-bit memory of the web VM holds the input, its UTF-8 copy, simdjson's buffers and the objects at the same time; the plugin compares the value itself with the limit, before any parse, so a stream of small values (NDJSON) of any length is read by the plugin.
    Beyond it, reading the value signals `SimdJSONResourceError` (`statistics` counts it as `tooLarge`).
    `SimdJSONReader maxInputUnits: nil` removes the limit; large documents then need a VM started with a larger old space window, `PHARO_WASM_OLD_SPACE_BASE=0x40000000` (see docs/WebAssembly.md);
  - the parser is freed after a document above 16 MiB, since WebAssembly memory never shrinks;
  - a primitive call runs to its end: the web VM, which runs in slices, does not return to its page during one, nor during the GC that the new objects bring about.
    A prototype of the plugin kept the page busy for one slice of 283 ms on semanticscholar-corpus.json (26 ms of parse, 31 ms of build, then a full GC) and of 350 ms on 2 million boxed Floats, where NeoJSON yields every 20 ms.

## Primitive contract

The module `SimdJSONPlugin` has four primitives; `SimdJSON.st` calls them from the class side of `SimdJSONReader`.
`primitiveInfo` answers the protocol version first, and the image uses the plugin only when it is the one it speaks (`SimdJSONReader protocol`, 2).

`primRead: source from: start to: stop flags: flags budget: budget maxUnits: maxUnits objectClass: objClass associationClass: assocClassOrNil wideStringClass: wideClass hashSeed: seed dictLayout: layout` (`primitiveRead`, 11 arguments) reads the JSON value at `start` in `source`:

| argument | |
|---|---|
| `source` | a ByteArray (UTF-8, units are bytes), a ByteString or ByteSymbol (Latin-1 characters), or a WideString (code points); anything else, a CompiledMethod included, fails with `PrimErrBadArgument` |
| `start`, `stop` | the first unit and the last one, 1-based, with `1 <= start <= stop + 1` and `stop <= source size`, else `PrimErrBadIndex` |
| `flags` | the sum of 1 `NEXT` (scan for the end of the value and parse only it; else the whole rest is parsed first, and the value alone when that fails), 2 `EXACT` (numbers with an exponent, and integers beyond 64 bits, as their text), 4 `DICT` (build Dictionaries, Associations and Arrays), 8 `SYMBOL_KEYS` (the keys of the tree are indexes in the Array of slot 5); `DICT` and `SYMBOL_KEYS` together fail with `PrimErrBadArgument`; `SimdJSONReader flagNext`, `flagExact`, `flagDict` and `flagSymbolKeys` name them |
| `budget` | the bytes the objects may take, checked before anything is allocated; -1 for no check |
| `maxUnits` | the most units given to simdjson: a whole rest above it gives way to the value alone, and a value above it answers status 3 before any parse; -1 for no limit |
| `objClass` | `SimdJSONFlatMap` (a variable class: `k1 v1 k2 v2...`), or with `DICT` `Dictionary` |
| `assocClassOrNil` | with `DICT`, `Association`; otherwise ignored |
| `wideClass` | `WideString` |
| `seed` | with `DICT`, `ByteString hash`, the seed of `String>>hash` |
| `layout` | with `DICT`, the 0-based slots of Dictionary `tally` and `array` and of Association `key` and `value`, 4 bits each from the lowest (`SimdJSONReader dictLayout`) |

It answers `{ status. value. nextIndex. fixups. keys. code }`:

| status | value | nextIndex | code |
|---|---|---|---|
| 0 OK | the value | the unit after the value and the white space after it (space, tab, LF, CR, FF) | 0 |
| 1 OVER_BUDGET | nil | `start` | the exact bytes the objects need |
| 2 ERROR | nil | `start` | simdjson's `error_code`, 1001 (byte order mark at the value) or 1002 (the number tokens do not match the document) |
| 3 TOO_LARGE | nil | `start` | the units of the value |

`fixups` is nil or an Array `c1 i1 c2 i2...`: `ci instVarAt: ii` holds the text of a number (a ByteString), which the image converts; `ci` is the result itself, with `ii` 2, for a number at the root.
`keys` is, with `SYMBOL_KEYS`, the Array of the distinct key Strings, in the order they first appear; otherwise nil.

A JSON error, a refused budget and a value over `maxUnits` are statuses, so they never start the VM's retries of a failed primitive.
The primitive fails only with `PrimErrBadNumArgs`, `PrimErrBadArgument` or `PrimErrBadIndex` (a bug of the caller), `PrimErrNoMemory` (an object allocation failed during the build: the VM collects garbage and calls it again up to 5 times, and the plugin keeps the parse for those calls) and `PrimErrNoCMemory` (malloc failed).

| primitive | arguments | answer |
|---|---|---|
| `primitiveInfo` | none | `{ protocol. 'SimdJSONPlugin 1.0'. '5.0.5'. simdjson's kernel. 'native' or 'emscripten'. the parser's capacity. the bytes of the input buffer. stats }`, where stats holds the 12 counters named by `SimdJSONReader pluginStatisticsKeys` (`SJ_NUM_STATS` in `sjBridge.h`) |
| `primitiveErrorMessage` | a code | simdjson's message of an `error_code`, the plugin's for 1001 and 1002, else nil |
| `primitiveRelease` | a Boolean | nil; drops the kept parse, and with true frees the parser and the input buffer |

## Tests

    sh plugins/SimdJSONPlugin/tests/run-c-tests.sh --simdjson <simdjson tree> --corpus <simdjson-data>    # C: clang++, g++ with ASan and UBSan, em++
    sh plugins/SimdJSONPlugin/tests/run-st-tests.sh build/build/vm <Pharo 15 image or zip>                # SUnit: SimdJSON-Tests
    sh plugins/SimdJSONPlugin/tests/run-st-tests.sh build/build/vm <Pharo 15 image or zip> compat         # also SimdJSON-NeoTests, against NeoJSON

`run-st-tests.sh <vm-dir> <image.zip or .image> [compat]` works on a copy of the image: it checks that every `.st` file is ASCII with one method a `methodsFor:` section, files them in, checks the banner of `SimdJSON.st`, runs each package with a JUnit file and summarises it.
Its environment: `NEOJSON_DIR` (compat: a clone of NeoJSON), `SJ_CORPUS` (the `jsonexamples` directory of simdjson-data, for the corpus and jsonchecker tests), `SJ_SLOW` (runs `SimdJSONSlowTests`: 2 million boxed Floats, and an `OutOfMemory` at a capped old space), `SJ_NUM_TOKENS` (random number tokens, 100000 by default), `SJ_WORK_DIR` (where the image copy goes, else a new directory in `TMPDIR`) and `SJ_TIMEOUT` (seconds a step may take, 3600 by default).
`compat` loads NeoJSON from `NEOJSON_DIR` (`st/load-neojson.st`) before `SimdJSON.st`, only to compare with it: `SimdJSONCompatTests` reads the semantics table, random number tokens and the corpus (`SJ_CORPUS`) with both readers and requires the same values, classes and stream positions, an error where NeoJSON has one, and a `SimdJSONParseError` only where NeoJSON is lenient, never on an RFC 8259 input of its list; then NeoJSON's own test classes run against `SimdJSONReader` (as `SJ<name>`, expecting SimdJSON's errors), but `testIntegers`, which only NeoJSON's leniency passes.
In the WebAssembly build, lane 68 of `make wasm-check` runs the suite with the node and the web VMs (see docs/WebAssembly.md).

## Benchmarks

`plugins/SimdJSONPlugin/bench` times `SimdJSONReader` against `NeoJSONReader` (NeoJSON 11e274d) in Pharo 15, on the native Cog and Stack VMs and in the WebAssembly VMs (the node VM, and the web VM driven in node):

    sh plugins/SimdJSONPlugin/bench/fetch-corpus.sh $OUT/corpus
    sh plugins/SimdJSONPlugin/bench/run-bench.sh --cog <Cog build> --stack <Stack build> --wasm <make wasm build> \
       --node <node> --image <Pharo 15 image with NeoJSON> --corpus $OUT/corpus --out $OUT/bench --repeat 2
    python3 -I plugins/SimdJSONPlugin/bench/summarize.py $OUT/bench/*.tsv

- `fetch-corpus.sh` downloads simdjson-data at commit 3519499 (its archive is checked against SHA256 `a2623f4c…a884`; `SJ_CORPUS_ZIP=<file>` takes it from a file).
- `run-bench.sh` prepares the image once, natively (`prepare-image.st` files `SimdJSON.st`, whose `SimdJSONReader` must be a subclass of `SimdJSONMapper`, `SJBenchMapping.st` and `SJBench.st` in; NeoJSON is in the image only to be compared with), then runs `run-bench.st` on a fresh copy of it with each VM in turn, pinned to one CPU (`taskset -c 2`), and the whole matrix twice; each run writes a TSV file, which `summarize.py` turns into the tables below.
- The timed inputs are 13 files of the corpus (13.8 MB: twitter, citm_catalog, canada, gsoc-2018, mesh, numbers, random, twitterescaped, github_events, update-center, apache_builds, instruments, marine_ik), an NDJSON file of 20,000 small objects (1.37 MB), and documents of 200 bytes and 2 KB.
- Every value is first compared with NeoJSON's, on all 30 JSON files of the corpus: deep equality, with the classes of numbers, strings and collections, from the String, from the bytes, in tree mode, then with Symbol keys, as NeoJSONObjects, through the mappings, on the NDJSON.
- A series is a full GC, warm-up runs, then samples of k runs each, k doubled until a sample lasts 50 ms (the clock of the WebAssembly VMs has a resolution of 1 ms), 10 samples natively and 5 in WebAssembly, within 20 s; the median of the samples counts, and MB/s is always over the UTF-8 bytes of the input.
  Around each series, the counters of the reader and of the plugin must show no repeated parse and no failed build.

| series | what it times |
|---|---|
| `neo-string` | `NeoJSONReader fromString: s` |
| `sj-string` | `SimdJSONReader fromString: s`: the headline |
| `sj-bytes` | `SimdJSONReader fromBytes: b` |
| `sj-tree` | `nativeDictionaries: false`: the tree and its conversion in Smalltalk |
| `sj-float` | `exactNumbers: false`: what exact numbers cost |
| `sj-prim` | the primitive alone, Dictionaries built in C, no exact numbers |
| `neo-symbols`, `sj-symbols` | `propertyNamesAsSymbols: true` |
| `neo-objects`, `sj-objects` | `NeoJSONObject fromString:`, and `SimdJSONReader` with NeoJSONObject, NeoJSONArray and Symbol keys |
| `neo-map`, `sj-map`, `neo-map-symbols`, `sj-map-symbols` | `nextAs: #Top` with the mappings of `SJBenchMapping.st` (twitter, citm_catalog, canada), with and without Symbol keys |
| `neo-ndjson-next`, `sj-ndjson-next`, `neo-ndjson-upToEnd`, `sj-ndjson-upToEnd` | the NDJSON, with `next` until `atEnd`, and with `upToEnd` |
| `neo-small`, `sj-small` | the 200-byte and 2 KB documents: the cost of a call |
| `utf8Encoded` | `s utf8Encoded`, for reference |

The gates, which `summarize.py` checks (its exit status): every comparison equal with no number of another class, no ERROR line, and `sj-string` at least 8 times faster than `neo-string` on every file and VM.
The targets, reported only: on Cog, `sj-string` within 3 ms on twitter, 5 ms on citm_catalog and 7.5 ms on canada; Stack within 15% of Cog there; the node VM at least 40 times faster than NeoJSON on twitter (60 as a stretch); `sj-map` 5 times faster than `neo-map` on Cog twitter, and 10 times with Symbol keys; `sj-float` within 10% of `sj-string` on marine_ik.
The times of Cog come from a prototype that called the primitive on the UTF-8 bytes, without exact numbers and without a budget; `sj-string` meets them because the String is transcoded to UTF-8 8 or 16 characters at a time when they are ASCII, the number tokens are looked for only once the walk meets a number with a fraction or an exponent, and natively no budget is given (see Memory and limits).
The layout of the code alone moves a series by up to 15% between two builds of the same sources, so `run-bench.sh` runs the matrix twice, and only the ratios within a run are worth comparing.

### Results

Measured on 2026-10-21 with `run-bench.sh --repeat 2` and `summarize.py`; every value is the median of the two run medians.
`SimdJSON.st` then still had a neo mode, in which `SimdJSONReader` was a subclass of `NeoJSONReader` that NeoJSON's code read again whatever simdjson refused; the benchmark ran in that mode, with an `sj-lenient` series and a gate of no such fallback on a valid input, which are gone since.
A valid input took the same path through the plugin then as it does now, so the times of the other series stand.

- Machine: 2 x Intel Xeon Gold 6238R @ 2.20GHz (Cascade Lake, 28 cores and 56 threads a socket, AVX-512), 503 GB, bare metal, Linux 6.8.0-139-generic.
  No cpufreq driver is loaded (the firmware controls the clock), so the governor is n/a; `/proc/cpuinfo` shows 2200 MHz.
- Sources: commit 2d5cb7c98 (pharo-12-wasm) plus the uncommitted SimdJSONPlugin working tree (SHA256 `c9e0471d…b5b0` of its diff and new files, docs excluded), simdjson 5.0.5, which picks its `haswell` kernel natively (the Xeon lacks the AVX-512 VBMI2 of the `icelake` kernel) and its `fallback` kernel in WebAssembly, which has no SIMD kernel.
- VMs, all v12.0.5-beta+58.2d5cb7c98: native CoInterpreter and StackVM (Release, Ubuntu clang 18.1.3, `-DFEATURE_LIB_SDL2=OFF` and the other options of the common native environment); `make wasm` (Emscripten 6.0.10, wasm64), run as `node/pharo` (wasm-node) and as `web/pharo-web.js` through `vm-driver.js` in node (wasm-web-node), both under node 24.19.0, and in a worker of headless Chromium 153.0.8010.12 (browser-chromium, Playwright 1.63, the harness of the research prototype adapted to load the whole corpus).
- Image: Pharo 15.0 SNAPSHOT b41 (4e572fed79) with NeoJSON 11e274d, prepared by `prepare-image.st` (neo mode).
  Corpus: simdjson-data 3519499, checked by `fetch-corpus.sh` against SHA256 `a2623f4c…a884`.
- Pinning: the 1-minute load average was about 2 on 112 CPUs; one foreign process kept CPU 12 busy.
  To halve the wall time, the matrix ran in two lanes at once on the two sockets, each on a core whose hyperthread sibling was idle: CPU 2 (socket 0) ran native-cog, native-stack and wasm-web-node, and CPU 3 (socket 1) ran wasm-node, then browser-chromium.
  Between the two runs a series moves by 0.4-1.3% (the median over each VM's series), 10.3% at worst (spread table below).
- Run times: native-cog 245 s, native-stack 498 s, wasm-node 910 s, wasm-web-node 870 s, browser-chromium 885 s, for each run.

#### Gates

| gate | result | detail |
|---|---|---|
| 1310 CHECKs equal | pass |  |
| 0 policy differences | pass |  |
| no fallback on valid inputs | pass |  |
| no ERROR line | pass |  |
| sj-string >= 8x neo-string | pass |  |

CHECK covers all 30 JSON files of the corpus on each VM and run (String, bytes and tree mode), plus the Symbol, NeoJSONObject, mapping, NDJSON and lenient checks of the timed files.
`sj-string` is at least 12.8x `neo-string` on every file and VM (random.json on Cog is the lowest).

#### Targets

| target | measured | met |
|---|---|---|
| Cog sj-string <= 3.0 ms on twitter.json | 3.25 ms | no |
| Cog sj-string <= 5.0 ms on citm_catalog.json | 4.99 ms | yes |
| Cog sj-string <= 7.5 ms on canada.json | 7.31 ms | yes |
| Stack sj-string within 15% of Cog on twitter.json | 0.99x Cog | yes |
| Stack sj-string within 15% of Cog on citm_catalog.json | 0.96x Cog | yes |
| Stack sj-string within 15% of Cog on canada.json | 1.00x Cog | yes |
| wasm node sj-string >= 40x neo-string on twitter.json | 100.4x | yes |
| wasm node sj-string >= 60x neo-string on twitter.json (stretch) | 100.4x | yes |
| Cog sj-map >= 5x neo-map on twitter.json | 6.3x | yes |
| Cog sj-map-symbols >= 10x neo-map on twitter.json | 15.5x | yes |
| native-cog sj-string within 10% of sj-float on marine_ik.json | 1.29x sj-float | no |
| native-stack sj-string within 10% of sj-float on marine_ik.json | 1.50x sj-float | no |
| wasm-node sj-string within 10% of sj-float on marine_ik.json | 1.70x sj-float | no |
| wasm-web-node sj-string within 10% of sj-float on marine_ik.json | 1.76x sj-float | no |
| browser-chromium sj-string within 10% of sj-float on marine_ik.json | 1.83x sj-float | no |

Two targets are missed:
- Cog twitter is 8% over 3 ms.
  `sj-bytes` takes 2.72 ms and `sj-float` 3.04 ms there: twitter.json decodes to a WideString, whose transcoding to UTF-8 costs about 0.5 ms; its exponent gate opens on every read (gateHits) but finds no exponent token (enumerations=0).
- `sj-float` is not within 10% of `sj-string` on marine_ik on any VM.
  marine_ik.json yields 620 number texts each read.
  The plugin's own time (CSPLIT) grows by only 1.8 ms on Cog and Stack and by 0.4-0.6 ms in WebAssembly, while the whole read grows by 3.7 ms (Cog), 6.4 ms (Stack) and 21-23 ms (WebAssembly).
  Most of the cost of exact numbers is therefore the exact conversion of the number texts in Smalltalk (about 35 µs each in WebAssembly), not the token scan of the plugin, which stays as decided.

#### NeoJSON against SimdJSONReader on each VM

Median ms of one read; speedup is the NeoJSON median over the SimdJSONReader median; MB/s is over the UTF-8 bytes.
`sj-string` is `SimdJSONReader fromString:` (the String is transcoded in C) and `sj-bytes` is `SimdJSONReader fromBytes:` on the UTF-8 bytes.

#### native-cog

| file | bytes | neo-string | sj-string | sj-bytes | speedup String | speedup bytes | neo MB/s | sj-string MB/s | sj-bytes MB/s |
|---|---|---|---|---|---|---|---|---|---|
| twitter.json | 631515 | 53.19 | 3.25 | 2.72 | 16.4x | 19.6x | 11.9 | 194.6 | 232.3 |
| citm_catalog.json | 1727204 | 133.2 | 4.99 | 4.13 | 26.7x | 32.3x | 13.0 | 346.6 | 418.8 |
| canada.json | 2251051 | 509.1 | 7.31 | 7.06 | 69.7x | 72.1x | 4.4 | 308.0 | 318.6 |
| gsoc-2018.json | 3327831 | 242.9 | 7.31 | 6.93 | 33.2x | 35.1x | 13.7 | 455.6 | 480.6 |
| mesh.json | 723597 | 132.6 | 2.74 | 2.66 | 48.5x | 49.8x | 5.5 | 264.6 | 271.7 |
| numbers.json | 150124 | 29.08 | 0.465 | 0.443 | 62.6x | 65.7x | 5.2 | 323.2 | 339.1 |
| random.json | 510476 | 51.34 | 4.02 | 3.29 | 12.8x | 15.6x | 9.9 | 126.9 | 155.0 |
| twitterescaped.json | 562408 | 54.10 | 2.97 | 2.90 | 18.2x | 18.6x | 10.4 | 189.3 | 193.7 |
| github_events.json | 65132 | 4.55 | 0.193 | 0.182 | 23.7x | 25.0x | 14.3 | 338.3 | 356.7 |
| update-center.json | 533178 | 53.04 | 2.81 | 2.55 | 18.9x | 20.8x | 10.1 | 189.6 | 208.8 |
| apache_builds.json | 127275 | 8.82 | 0.410 | 0.392 | 21.5x | 22.5x | 14.4 | 310.4 | 324.8 |
| instruments.json | 220346 | 18.71 | 0.911 | 0.885 | 20.5x | 21.1x | 11.8 | 241.8 | 249.1 |
| marine_ik.json | 2983466 | 434.2 | 16.51 | 16.16 | 26.3x | 26.9x | 6.9 | 180.7 | 184.6 |

#### native-stack

| file | bytes | neo-string | sj-string | sj-bytes | speedup String | speedup bytes | neo MB/s | sj-string MB/s | sj-bytes MB/s |
|---|---|---|---|---|---|---|---|---|---|
| twitter.json | 631515 | 167.0 | 3.21 | 2.69 | 52.0x | 62.1x | 3.8 | 196.8 | 234.8 |
| citm_catalog.json | 1727204 | 431.3 | 4.80 | 3.94 | 89.8x | 109.4x | 4.0 | 359.8 | 438.0 |
| canada.json | 2251051 | 1655.4 | 7.27 | 7.03 | 227.6x | 235.6x | 1.4 | 309.5 | 320.3 |
| gsoc-2018.json | 3327831 | 756.5 | 7.07 | 6.79 | 107.0x | 111.4x | 4.4 | 470.8 | 490.1 |
| mesh.json | 723597 | 446.2 | 2.81 | 2.73 | 158.7x | 163.2x | 1.6 | 257.5 | 264.6 |
| numbers.json | 150124 | 96.36 | 0.488 | 0.465 | 197.5x | 207.2x | 1.6 | 307.7 | 323.0 |
| random.json | 510476 | 171.3 | 3.97 | 3.23 | 43.1x | 53.1x | 3.0 | 128.5 | 158.2 |
| twitterescaped.json | 562408 | 167.0 | 2.95 | 2.87 | 56.5x | 58.3x | 3.4 | 190.4 | 196.2 |
| github_events.json | 65132 | 14.48 | 0.193 | 0.183 | 75.0x | 79.1x | 4.5 | 337.6 | 356.2 |
| update-center.json | 533178 | 162.7 | 2.75 | 2.48 | 59.2x | 65.7x | 3.3 | 194.1 | 215.3 |
| apache_builds.json | 127275 | 26.81 | 0.410 | 0.391 | 65.3x | 68.6x | 4.8 | 309.9 | 325.5 |
| instruments.json | 220346 | 61.25 | 0.903 | 0.881 | 67.8x | 69.6x | 3.6 | 244.0 | 250.2 |
| marine_ik.json | 2983466 | 1560.3 | 19.17 | 18.84 | 81.4x | 82.8x | 1.9 | 155.6 | 158.4 |

#### wasm-node

| file | bytes | neo-string | sj-string | sj-bytes | speedup String | speedup bytes | neo MB/s | sj-string MB/s | sj-bytes MB/s |
|---|---|---|---|---|---|---|---|---|---|
| twitter.json | 631515 | 740.5 | 7.38 | 6.13 | 100.4x | 120.9x | 0.8 | 85.7 | 103.1 |
| citm_catalog.json | 1727204 | 1915.5 | 13.25 | 10.94 | 144.6x | 175.1x | 0.9 | 130.4 | 157.9 |
| canada.json | 2251051 | 7051.5 | 18.75 | 18.38 | 376.1x | 383.8x | 0.3 | 120.1 | 122.5 |
| gsoc-2018.json | 3327831 | 3494.5 | 23.88 | 23.38 | 146.4x | 149.5x | 0.9 | 139.4 | 142.4 |
| mesh.json | 723597 | 2055.5 | 6.25 | 6.09 | 328.9x | 337.3x | 0.3 | 115.8 | 118.8 |
| numbers.json | 150124 | 454.0 | 1.14 | 1.09 | 397.9x | 415.0x | 0.3 | 131.6 | 137.3 |
| random.json | 510476 | 760.0 | 8.50 | 7.12 | 89.4x | 106.7x | 0.7 | 60.1 | 71.7 |
| twitterescaped.json | 562408 | 738.5 | 6.25 | 6.12 | 118.2x | 120.6x | 0.8 | 90.0 | 91.8 |
| github_events.json | 65132 | 65.00 | 0.508 | 0.496 | 128.0x | 131.0x | 1.0 | 128.3 | 131.3 |
| update-center.json | 533178 | 748.5 | 6.50 | 5.75 | 115.2x | 130.2x | 0.7 | 82.0 | 92.7 |
| apache_builds.json | 127275 | 122.0 | 1.02 | 0.984 | 120.1x | 124.0x | 1.0 | 125.3 | 129.3 |
| instruments.json | 220346 | 280.5 | 1.94 | 1.89 | 144.7x | 148.4x | 0.8 | 113.7 | 116.6 |
| marine_ik.json | 2983466 | 6954.5 | 54.50 | 53.00 | 127.6x | 131.2x | 0.4 | 54.7 | 56.3 |

#### wasm-web-node

| file | bytes | neo-string | sj-string | sj-bytes | speedup String | speedup bytes | neo MB/s | sj-string MB/s | sj-bytes MB/s |
|---|---|---|---|---|---|---|---|---|---|
| twitter.json | 631515 | 692.0 | 6.62 | 5.69 | 104.5x | 121.7x | 0.9 | 95.3 | 111.0 |
| citm_catalog.json | 1727204 | 1797.5 | 11.75 | 9.81 | 153.0x | 183.2x | 1.0 | 147.1 | 176.0 |
| canada.json | 2251051 | 6684.2 | 16.50 | 16.00 | 405.1x | 417.8x | 0.3 | 136.4 | 140.7 |
| gsoc-2018.json | 3327831 | 3183.5 | 19.50 | 19.00 | 163.3x | 167.6x | 1.0 | 170.7 | 175.2 |
| mesh.json | 723597 | 1925.0 | 5.50 | 5.41 | 350.0x | 356.1x | 0.4 | 131.6 | 133.9 |
| numbers.json | 150124 | 426.0 | 1.01 | 0.976 | 422.6x | 436.3x | 0.3 | 149.0 | 153.7 |
| random.json | 510476 | 719.5 | 7.81 | 6.75 | 92.1x | 106.6x | 0.7 | 65.3 | 75.6 |
| twitterescaped.json | 562408 | 695.0 | 5.75 | 5.66 | 120.9x | 122.9x | 0.8 | 97.8 | 99.4 |
| github_events.json | 65132 | 61.00 | 0.473 | 0.461 | 129.0x | 132.3x | 1.1 | 137.8 | 141.3 |
| update-center.json | 533178 | 698.5 | 5.97 | 5.41 | 117.0x | 129.2x | 0.8 | 89.3 | 98.6 |
| apache_builds.json | 127275 | 113.0 | 0.930 | 0.914 | 121.5x | 123.6x | 1.1 | 136.9 | 139.2 |
| instruments.json | 220346 | 264.0 | 1.80 | 1.77 | 146.9x | 149.5x | 0.8 | 122.6 | 124.8 |
| marine_ik.json | 2983466 | 6609.8 | 48.75 | 48.50 | 135.6x | 136.3x | 0.5 | 61.2 | 61.5 |

#### browser-chromium

| file | bytes | neo-string | sj-string | sj-bytes | speedup String | speedup bytes | neo MB/s | sj-string MB/s | sj-bytes MB/s |
|---|---|---|---|---|---|---|---|---|---|
| twitter.json | 631515 | 697.5 | 6.03 | 5.06 | 115.6x | 137.8x | 0.9 | 104.7 | 124.7 |
| citm_catalog.json | 1727204 | 1796.5 | 10.75 | 8.75 | 167.1x | 205.3x | 1.0 | 160.7 | 197.4 |
| canada.json | 2251051 | 6926.0 | 15.12 | 14.75 | 457.9x | 469.6x | 0.3 | 148.8 | 152.6 |
| gsoc-2018.json | 3327831 | 3307.0 | 18.25 | 18.00 | 181.2x | 183.7x | 1.0 | 182.4 | 184.9 |
| mesh.json | 723597 | 2008.0 | 4.94 | 4.91 | 406.6x | 409.3x | 0.4 | 146.6 | 147.5 |
| numbers.json | 150124 | 442.5 | 0.946 | 0.938 | 468.0x | 471.7x | 0.3 | 158.8 | 160.1 |
| random.json | 510476 | 722.0 | 7.00 | 5.97 | 103.1x | 121.0x | 0.7 | 72.9 | 85.5 |
| twitterescaped.json | 562408 | 705.5 | 5.12 | 5.03 | 137.7x | 140.2x | 0.8 | 109.7 | 111.8 |
| github_events.json | 65132 | 61.50 | 0.426 | 0.410 | 144.4x | 150.0x | 1.1 | 153.0 | 158.8 |
| update-center.json | 533178 | 712.5 | 5.41 | 4.69 | 131.8x | 152.0x | 0.7 | 98.6 | 113.7 |
| apache_builds.json | 127275 | 115.5 | 0.821 | 0.797 | 140.8x | 144.9x | 1.1 | 155.2 | 159.7 |
| instruments.json | 220346 | 263.0 | 1.59 | 1.53 | 165.0x | 171.8x | 0.8 | 138.3 | 143.9 |
| marine_ik.json | 2983466 | 6778.5 | 46.75 | 45.75 | 145.0x | 148.2x | 0.4 | 63.8 | 65.2 |

#### The other reading modes

Median ms on four files.
`sj-tree`, `sj-symbols` and `sj-objects` build their containers in Smalltalk, which costs most on the interpreters (Stack and WebAssembly); `sj-prim` is the primitive alone with no exact numbers.

| VM | file | sj-string | sj-tree | sj-float | sj-prim | neo-symbols | sj-symbols | neo-objects | sj-objects |
|---|---|---|---|---|---|---|---|---|---|
| native-cog | twitter.json | 3.25 | 9.14 | 3.04 | 3.04 | 62.95 | 9.16 | 62.42 | 8.52 |
| native-cog | citm_catalog.json | 4.99 | 10.83 | 5.00 | 5.01 | 153.9 | 10.60 | 163.7 | 15.71 |
| native-cog | canada.json | 7.31 | 11.20 | 7.14 | 7.14 | 508.9 | 11.17 | 531.9 | 25.87 |
| native-cog | marine_ik.json | 16.51 | 28.84 | 12.83 | 12.26 | 451.2 | 28.38 | 488.9 | 70.30 |
| native-stack | twitter.json | 3.21 | 15.14 | 2.99 | 2.98 | 207.2 | 15.97 | 210.0 | 15.03 |
| native-stack | citm_catalog.json | 4.80 | 18.51 | 4.80 | 4.79 | 524.2 | 19.74 | 562.7 | 27.62 |
| native-stack | canada.json | 7.27 | 20.36 | 7.10 | 7.08 | 1657.6 | 21.19 | 1756.8 | 44.65 |
| native-stack | marine_ik.json | 19.17 | 51.72 | 12.81 | 12.27 | 1682.4 | 53.37 | 1776.2 | 87.06 |
| wasm-node | twitter.json | 7.38 | 60.00 | 6.56 | 6.56 | 859.5 | 64.00 | 860.5 | 61.00 |
| wasm-node | citm_catalog.json | 13.25 | 74.00 | 13.38 | 11.69 | 2165.0 | 79.00 | 2233.5 | 122.5 |
| wasm-node | canada.json | 18.75 | 79.50 | 18.12 | 15.75 | 7029.5 | 83.00 | 7242.5 | 179.5 |
| wasm-node | marine_ik.json | 54.50 | 206.0 | 32.00 | 26.50 | 7371.5 | 212.0 | 7630.5 | 383.0 |
| wasm-web-node | twitter.json | 6.62 | 56.50 | 5.88 | 5.88 | 808.5 | 60.50 | 814.0 | 59.00 |
| wasm-web-node | citm_catalog.json | 11.75 | 69.50 | 11.62 | 10.12 | 2047.5 | 75.50 | 2108.5 | 116.5 |
| wasm-web-node | canada.json | 16.50 | 72.50 | 15.88 | 14.00 | 6655.2 | 75.00 | 6890.0 | 163.0 |
| wasm-web-node | marine_ik.json | 48.75 | 193.0 | 27.75 | 23.62 | 6955.0 | 200.0 | 7195.5 | 359.5 |
| browser-chromium | twitter.json | 6.03 | 58.50 | 5.41 | 5.31 | 810.0 | 62.00 | 818.0 | 60.50 |
| browser-chromium | citm_catalog.json | 10.75 | 73.00 | 10.81 | 9.69 | 2032.0 | 78.00 | 2100.5 | 123.0 |
| browser-chromium | canada.json | 15.12 | 76.50 | 14.62 | 12.94 | 6842.5 | 76.00 | 7019.5 | 171.0 |
| browser-chromium | marine_ik.json | 46.75 | 203.0 | 25.50 | 21.00 | 7077.5 | 209.0 | 7382.5 | 373.5 |

#### Mapping with `nextAs:`

`nextAs: #Top` with the mappings of `SJBenchMapping.st` on the whole file, with String keys and with `propertyNamesAsSymbols: true`; each speedup compares like with like (the target table above compares `sj-map-symbols` with `neo-map`).

| VM | file | neo-map | sj-map | speedup | neo-map-symbols | sj-map-symbols | speedup |
|---|---|---|---|---|---|---|---|
| native-cog | twitter.json | 56.03 | 8.83 | 6.3x | 60.91 | 3.62 | 16.8x |
| native-cog | citm_catalog.json | 133.1 | 10.77 | 12.4x | 151.4 | 6.48 | 23.4x |
| native-cog | canada.json | 546.7 | 39.37 | 13.9x | 545.4 | 39.26 | 13.9x |
| native-stack | twitter.json | 182.6 | 16.48 | 11.1x | 202.7 | 7.07 | 28.7x |
| native-stack | citm_catalog.json | 452.4 | 22.91 | 19.8x | 528.2 | 11.93 | 44.3x |
| native-stack | canada.json | 1883.7 | 79.30 | 23.8x | 1888.8 | 79.55 | 23.7x |
| wasm-node | twitter.json | 784.0 | 65.00 | 12.1x | 846.5 | 27.00 | 31.4x |
| wasm-node | citm_catalog.json | 1964.0 | 87.00 | 22.6x | 2184.5 | 47.25 | 46.2x |
| wasm-node | canada.json | 7540.0 | 358.0 | 21.1x | 7530.0 | 359.0 | 21.0x |
| wasm-web-node | twitter.json | 735.5 | 61.50 | 12.0x | 798.5 | 24.25 | 32.9x |
| wasm-web-node | citm_catalog.json | 1847.0 | 82.50 | 22.4x | 2064.0 | 44.50 | 46.4x |
| wasm-web-node | canada.json | 7180.5 | 348.5 | 20.6x | 7180.0 | 346.0 | 20.8x |
| browser-chromium | twitter.json | 740.0 | 63.00 | 11.7x | 799.0 | 24.75 | 32.3x |
| browser-chromium | citm_catalog.json | 1845.5 | 83.50 | 22.1x | 2051.5 | 45.00 | 45.6x |
| browser-chromium | canada.json | 7329.5 | 370.5 | 19.8x | 7343.0 | 372.5 | 19.7x |

#### Mapping with `nextListAs:`

The SJBench matrix has no `nextListAs:` series, so these come from a separate script run on the same image, VMs and protocol (`SJBench`'s own check and measuring, 2 runs, CPU 2): list documents cut out of the corpus and written back by NeoJSONWriter (the 100 twitter statuses, the 243 citm_catalog performances, the one canada feature), read with `nextListAs: SJBTweet`, `SJBPerformance` and `SJBFeature`.
Its 60 checks were equal, with 0 policy differences, 0 fallbacks and no ERROR line.

| VM | file | bytes | neo-list | sj-list | speedup | neo-list-symbols | sj-list-symbols | speedup |
|---|---|---|---|---|---|---|---|---|
| native-cog | tweets-list.json | 466565 | 48.59 | 9.08 | 5.4x | 55.58 | 3.42 | 16.3x |
| native-cog | performances-list.json | 452513 | 61.08 | 7.67 | 8.0x | 108.7 | 4.06 | 26.8x |
| native-cog | features-list.json | 2090194 | 437.9 | 39.29 | 11.1x | 437.5 | 39.32 | 11.1x |
| native-stack | tweets-list.json | 466565 | 154.9 | 15.13 | 10.2x | 175.1 | 6.70 | 26.1x |
| native-stack | performances-list.json | 452513 | 214.8 | 14.59 | 14.7x | 306.9 | 8.05 | 38.1x |
| native-stack | features-list.json | 2090194 | 1551.4 | 79.53 | 19.5x | 1553.7 | 79.32 | 19.6x |
| wasm-node | tweets-list.json | 466565 | 710.5 | 66.00 | 10.8x | 782.5 | 26.50 | 29.5x |
| wasm-node | performances-list.json | 452513 | 976.0 | 61.50 | 15.9x | 1242.5 | 33.00 | 37.7x |
| wasm-node | features-list.json | 2090194 | 7017.5 | 377.0 | 18.6x | 7022.0 | 377.5 | 18.6x |
| wasm-web-node | tweets-list.json | 466565 | 634.0 | 59.00 | 10.7x | 692.0 | 23.00 | 30.1x |
| wasm-web-node | performances-list.json | 452513 | 886.5 | 59.50 | 14.9x | 1201.5 | 29.75 | 40.4x |
| wasm-web-node | features-list.json | 2090194 | 6429.8 | 351.5 | 18.3x | 6431.5 | 354.0 | 18.2x |
| browser-chromium | tweets-list.json | 466565 | 670.0 | 63.50 | 10.6x | 733.0 | 25.25 | 29.0x |
| browser-chromium | performances-list.json | 452513 | 934.5 | 67.50 | 13.8x | 1356.0 | 32.25 | 42.0x |
| browser-chromium | features-list.json | 2090194 | 6792.0 | 381.5 | 17.8x | 6798.0 | 384.5 | 17.7x |

#### NDJSON

`ndjson-20k.ndjson` (20,000 small objects, 1.37 MB), read with `next` until `atEnd` and with `upToEnd`.
Each value is one small parse, so the cost of a call dominates: the gain is 5.9x on Cog, below that of whole documents (the 8x gate applies to the corpus files only).

| VM | neo `next` loop | sj `next` loop | speedup | sj MB/s | neo `upToEnd` | sj `upToEnd` | speedup | sj MB/s |
|---|---|---|---|---|---|---|---|---|
| native-cog | 219.2 | 37.03 | 5.9x | 37.2 | 219.3 | 37.19 | 5.9x | 37.0 |
| native-stack | 774.1 | 64.24 | 12.1x | 21.4 | 774.8 | 63.99 | 12.1x | 21.5 |
| wasm-node | 3138.5 | 332.5 | 9.4x | 4.1 | 3134.5 | 330.5 | 9.5x | 4.2 |
| wasm-web-node | 2976.0 | 329.0 | 9.0x | 4.2 | 2974.5 | 329.0 | 9.0x | 4.2 |
| browser-chromium | 2995.0 | 331.0 | 9.0x | 4.2 | 3001.0 | 332.0 | 9.0x | 4.1 |

#### Small documents and the fallback cliff

`sj-lenient` is twitter.json with a leading-zero number in its top object: simdjson rejects it and NeoJSON reads the whole document again, so it costs NeoJSON's time plus one parse, as designed.

| VM | neo 200 B | sj 200 B | speedup | neo 2 KB | sj 2 KB | speedup | sj-lenient (twitter) | against NeoJSON |
|---|---|---|---|---|---|---|---|---|
| native-cog | 0.024 | 0.003 | 8.0x | 0.228 | 0.016 | 14.2x | 57.15 | 1.07x neo-string |
| native-stack | 0.088 | 0.004 | 22.1x | 0.786 | 0.018 | 43.6x | 175.4 | 1.05x neo-string |
| wasm-node | 0.371 | 0.020 | 18.6x | 3.34 | 0.051 | 64.9x | 763.0 | 1.03x neo-string |
| wasm-web-node | 0.353 | 0.020 | 17.7x | 3.19 | 0.050 | 63.8x | 717.5 | 1.04x neo-string |
| browser-chromium | 0.352 | 0.020 | 17.6x | 3.19 | 0.045 | 70.8x | 724.5 | 1.04x neo-string |

#### Plugin time per `sj-string` read (parse / build ms)

| file | native-cog | native-stack | wasm-node | wasm-web-node | browser-chromium |
|---|---|---|---|---|---|
| twitter.json | 0.27 / 2.25 | 0.28 / 2.22 | 1.76 / 4.04 | 1.42 / 3.99 | 1.30 / 3.48 |
| citm_catalog.json | 0.68 / 3.24 | 0.69 / 3.06 | 3.36 / 5.61 | 2.58 / 5.58 | 2.52 / 4.87 |
| canada.json | 3.11 / 3.49 | 3.10 / 3.47 | 9.18 / 5.65 | 7.44 / 5.72 | 7.25 / 4.87 |
| gsoc-2018.json | 1.01 / 5.12 | 1.04 / 4.94 | 8.15 / 8.71 | 6.85 / 7.60 | 5.99 / 7.13 |
| mesh.json | 1.21 / 1.36 | 1.22 / 1.36 | 3.26 / 2.42 | 2.58 / 2.39 | 2.48 / 1.94 |
| numbers.json | 0.21 / 0.21 | 0.22 / 0.22 | 0.60 / 0.38 | 0.49 / 0.38 | 0.47 / 0.34 |
| random.json | 0.35 / 2.49 | 0.35 / 2.47 | 1.77 / 4.55 | 1.53 / 4.48 | 1.36 / 3.91 |
| twitterescaped.json | 0.40 / 2.31 | 0.41 / 2.28 | 1.70 / 4.12 | 1.34 / 4.06 | 1.22 / 3.54 |
| github_events.json | 0.02 / 0.15 | 0.02 / 0.15 | 0.19 / 0.28 | 0.16 / 0.28 | 0.15 / 0.25 |
| update-center.json | 0.27 / 2.08 | 0.27 / 2.15 | 1.81 / 3.86 | 1.52 / 3.83 | 1.39 / 3.29 |
| apache_builds.json | 0.05 / 0.32 | 0.05 / 0.31 | 0.35 / 0.60 | 0.29 / 0.59 | 0.26 / 0.51 |
| instruments.json | 0.12 / 0.71 | 0.12 / 0.70 | 0.55 / 1.27 | 0.42 / 1.26 | 0.40 / 1.10 |
| marine_ik.json | 4.42 / 9.23 | 4.44 / 9.07 | 12.00 / 14.23 | 9.46 / 13.64 | 8.99 / 11.88 |

#### Spread of the medians between the two runs

| VM | series | median spread | largest |
|---|---|---|---|
| native-cog | 164 | 0.8% | 7.4% (citm_catalog.json sj-bytes) |
| native-stack | 164 | 0.4% | 3.9% (gsoc-2018.json sj-float) |
| wasm-node | 164 | 0.4% | 7.3% (citm_catalog.json utf8Encoded) |
| wasm-web-node | 164 | 1.0% | 5.2% (mesh.json sj-tree) |
| browser-chromium | 164 | 1.3% | 10.3% (apache_builds.json utf8Encoded) |

#### Observations

- Stack's `sj-string` equals Cog's (0.96-1.00x) wherever the read is all primitive; on marine_ik Stack is 16% slower, the Smalltalk conversion of the number texts.
- Against NeoJSON the gains grow as the VM gets slower, since NeoJSON runs as Smalltalk code and the plugin as C: 13-70x on Cog, 43-228x on Stack, 89-470x in WebAssembly.
- With the same `pharo-web.wasm`, headless Chromium reads 8-12% faster than node 24 with SimdJSONReader and at the same speed with NeoJSON; the node VM (`node/pharo`) is the slowest of the three, 15-20% behind Chromium with SimdJSONReader.
- In WebAssembly simdjson runs its scalar `fallback` kernel: with the node VM its parse takes 3-9x the native time (canada 9.18 against 3.11 ms, twitter 1.76 against 0.27 ms) and the build 1.5-2x.
- In WebAssembly, the containers built in Smalltalk (`sj-tree`, `sj-symbols`, `sj-objects`) take 4-10x the time of `sj-string`, against 1.5-4x on Cog and 2.5-6x on Stack: the native Dictionaries of the default path matter most there.

### Number tokens

Exact numbers find the tokens with an exponent with a scan of the validated input, after a quick test that skips it when no exponent can be there (canada, for instance).
Both run only once the walk of the document meets its first number with a fraction or an exponent (simdjson's DOUBLE, which every exponent token is): a document without one, such as citm_catalog, github_events or gsoc-2018, runs neither.
The rule set for the scan was to switch to a scan of simdjson's structural indexes if the scan cost more than 15% of the plugin's time on marine_ik, mesh or numbers (`run-c-tests.sh --report`).

`run-c-tests.sh --report` (clang++ -O3, Xeon Gold 6238R, microseconds, best of 21, exact numbers, tree mode, a builder that allocates nothing; `replay` is what plan B's walk of the structural indexes adds to the walk of the DOM, `share` is `enum / (parse + gate + enum + walk)`):

| file | parse | gate | enum | measure | walk | replay | share |
|---|---|---|---|---|---|---|---|
| twitter.json | 253.7 | 1.0 | 0 | 725.7 | 148.5 | 50.7 | 0% (the gate fires, the closer look finds no exponent) |
| canada.json | 3001.1 | 163.0 | 0 | 1236.1 | 1156.8 | 1540.7 | 0% (gate closed) |
| citm_catalog.json | 633.1 | 241.1 | 0 | 658.8 | 349.3 | 190.1 | 0% (gate closed) |
| marine_ik.json | 4250.2 | 1.6 | 1430.8 | 2099.6 | 1990.8 | 1897.6 | 18.6% |
| mesh.json | 1178.1 | 0.2 | 158.6 | 499.8 | 457.1 | 633.7 | 8.8% |
| numbers.json | 208.4 | 0.9 | 31.8 | 63.8 | 61.1 | 146.3 | 10.5% |

(The enum column of the first three files is what an enumeration would cost, 539, 492 and 877 µs; the engine runs none on them. The gate column is the cost of the quick test itself; the engine no longer runs it on citm_catalog, which has no DOUBLE.)
marine_ik passes the 15% line, by this strict measure without the cost of the objects; against the time of the whole primitive on Cog (8.5 ms in the prototype) the share is about 14%, mesh 8% and numbers 10%.
**Decision (final): the scan stays.** Plan B measured slower on all three files (1898, 634 and 146 µs against 1431, 159 and 32), and it would read a private member of simdjson's parser; the switch would cost time instead of saving it.
marine_ik's 18.6% is over the 15% line, and the maintainer chose to keep the scan anyway: it is not to be replaced by the structural-index scan.

The gate fires on the bytes `e`/`E` between a digit and a digit, `+` or `-`, which hashes and URLs inside strings have too; the engine then also requires a JSON delimiter before the number and after its exponent before it enumerates.
In `primitiveInfo`'s statistics `gateHits` counts the gate's openings, and `enumerations` only the reads that passed the closer look too (the bench's STATS lines show both); a document without a DOUBLE counts neither.
`cachedBytes` is the size of the plugin's input buffer: natively it never shrinks; in WebAssembly it is freed with the parser after a document above 16 MiB.

## Licences

- simdjson is under the Apache License 2.0 or the MIT licence, at the user's choice: both texts are installed as `LICENSE.simdjson` and `LICENSE.simdjson-MIT`, next to the plugin on Linux and Windows (`build/vm`, the `lib` directory of the Linux zip) and in `Pharo.app/Contents/Resources` on macOS, and go into the `THIRD-PARTY-NOTICES.txt` of the WebAssembly VMs, with those of libc++ and libc++abi (Apache License 2.0 with LLVM exceptions), which the plugin links there.
- The mapper, the mapping classes and the number parser of `SimdJSON.st` are ports of NeoJSON, under its MIT licence (Copyright (C) 2012 Sven Van Caekenberghe), which the file carries.
- The benchmark corpus is not part of this repository: `fetch-corpus.sh` downloads it, and its files keep the terms of their sources.

This is not legal advice.

## Limitations

- Reading only: there is no writer; NeoJSONWriter or STON writes JSON.
- Pharo 15 only: `SimdJSON.st` is written in the class syntax and chunk format of Pharo 15.
- Only Dictionaries and Arrays are built in C; OrderedDictionary, NeoJSONObject, SmallDictionary and OrderedCollection go through the tree.
- A value is parsed whole: a stream other than a ReadStream on a String or a ByteArray is read to its end first, and the web VM does not return to its page during a primitive call.
- macOS, Windows and Linux aarch64 are untested until CI runs the step that loads the plugin.
