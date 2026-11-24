# SimdCSV: reading and writing CSV on simdcsv

The SimdCSVPlugin reads CSV with [simdcsv](https://github.com/geofflangdale/simdcsv) (Geoff Langdale, commit `a9494a8`), and the package `SimdCSV` (the submodule `plugins/SimdCSVPlugin/simdcsv.st`) gives Pharo 12 to 15 rows, typed records, tables and columns on top of it, and a writer.
It works in two steps: **the index**, the offset of the unit that ends each field, 32 bits a field, found 64 bytes at a time and stored in a word array of the image; and **the rows**, Arrays of Strings, Integers and Floats that are built in C from such an index, in batches whose object memory is bounded.
The reader and the writer have the protocols of `NeoCSVReader` and `NeoCSVWriter` (but for what `unsupportedNeoSelectors` of each lists, mostly private messages) under their own names; nothing of NeoCSV is required, shadowed or extended, and nothing of Pharo is overridden: the only extension method is `simdCSVFailed`.
Without the plugin `SimdCSVSmalltalk` answers the same in pure Smalltalk; there is no FFI library (the name `SimdCSV-FFI` is reserved for one).

simdcsv is a benchmark program, not a library: its loop drops up to 64 last bytes and the last field, answers phantom separators from its padding, goes wrong after a quote inside a field, and checks no capacity.
So only its arithmetic is used: the part of `src/main.cpp` that turns 64 bytes into masks and a mask into offsets (see Licences). The loop, the checks, the tail, the line ends, the errors, the other kernels and everything after the index are the plugin's, and the meaning of a text is the automaton of the section Semantics; the kernels are used where a formula proves that they agree with it.

Files, in `plugins/SimdCSVPlugin`:

- `src/common/SimdCSVPlugin.c`: the module functions and the four primitives (C99, with the VM's headers); the only file of `src/common`;
- `src/engine/scEngine.cpp`: the engine, C++17 without the C++ library: the choice of the kernel, the automaton, the cursor over an index, the texts and the numbers; `src/engine/scKernels.inc`: the kernels, a namespace each; `src/engine/scKernelBody.inc`: the block and the loops, included once per kernel (no file of the engine includes a VM header);
- `include/common/scEngine.h`: the interface between the two;
- `simdcsv.st/`: the Smalltalk side, a git submodule of [simdcsv.st](https://github.com/massimo-nocentini/simdcsv.st) (`git submodule update --init plugins/SimdCSVPlugin/simdcsv.st`): Tonel packages in `src/` with `BaselineOfSimdCSV`, whose commit is the version of the Smalltalk code that goes with the plugin, and whose `README.md` is the guide of the API;
- `tests/`: the C tests (`run-c-tests.sh`, `sctest.cpp`, `fuzz.cpp`, and `glue.sh`, `gluetest.c`, `fakevm.c` for the glue), the Smalltalk loader and runner (`load-simdcsv.st`, `run-st-tests.sh`) and the probe of the CI jobs (`ci-load.st`);
- `bench/`: the benchmark (see Benchmarks).

## Building

The plugin is on by default on every 64-bit target: `FEATURE_PLUGIN_SIMDCSV=ON`.
It is off on 32-bit targets, where `-DFEATURE_PLUGIN_SIMDCSV=ON` stops the configuration: the answers carry SmallIntegers above 2^30.
`-DFEATURE_PLUGIN_SIMDCSV=OFF` leaves it out, along with the archive of simdcsv.

    cmake -S . -B build -DFLAVOUR=CoInterpreter              # the plugin is in
    cmake -S . -B build -DFEATURE_PLUGIN_SIMDCSV=OFF         # it is not

simdcsv has no tags. It is taken from the archive of its commit, `https://github.com/geofflangdale/simdcsv/archive/a9494a85c83525bb42ba368d8f9bcf3acf53a8d1.tar.gz` (339,403 bytes, as `simdcsv-a9494a8.tar.gz`), checked against its SHA256 (`27eb10cc…92f256c4`), by `cmake/simdcsv.cmake` with the function of the WebAssembly libraries (`cmake/emscripten/deps/fetch.cmake`), as simdjson and simdutf are:

- the archive comes from `WASM_DEPS_DIR` when that directory holds `simdcsv-a9494a8.tar.gz`, which builds offline, else from `<build>/downloads`, else from GitHub at configure time;
- `-DSIMDCSV_SOURCE_DIR=<tree>` takes a simdcsv source tree instead, an extraction of that archive or a clone at that commit; CRLF in it is read as LF.

Either way the bytes of `src/main.cpp` from `struct simd_input {` to the comment after `flatten_bits` are cut out at configure time, and their SHA256 must be `bf5511f1…b68d50874`; else the configuration stops and names both hashes; that of its `LICENSE` is checked too (`c71d239d…5d0ab4`). The slice is `<build>/deps/simdcsv-slice/simdcsv_slice.inc`, which only `scKernels.inc` includes.

The engine is the static library `pharo_sc_engine`, in `<build>/deps` (never in `build/vm`, which is packaged whole), compiled with `-O3 -g0 -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off`, hidden visibility, and no `-m` flag (`-m64` and the setjmp/longjmp flags of the VM on Emscripten): the kernels choose their instructions with target pragmas, and the CPU is asked at run time.
It uses no C++ header, exception, RTTI, static constructor, heap or C++ library, and the plugin links it by its file, not by its target, so that CMake does not link the plugin with the C++ driver: `libSimdCSVPlugin.so` needs no libstdc++.
The glue is C99, `-O2`.
Natively `libSimdCSVPlugin.so` is 107 KB (109,136 bytes; 66 KB stripped), on Linux x86_64 with clang 18.1.3, and exports only its module functions, its four primitives and their accessor depths, all 0.

Platforms:

- Linux x86_64 and Emscripten (the node and web VMs of `make wasm`) are built and tested here; the engine's tests also run for arm64 under qemu (`-cpu cortex-a72`, without PMULL);
- macOS (x86_64 and arm64), Windows (MSYS2 with clang on x86_64, CLANGARM64 on arm64) and Linux aarch64 are written for, but **untested** until CI runs.
  Each native job of `.github/workflows/build.yml` loads the plugin of its zip with `plugins/SimdCSVPlugin/tests/ci-load.st`, with the best kernel (`neon` on arm64, `avx2` or `sse2` on x86_64: checked by its name), with a bogus `SIMDCSV_FORCE_IMPLEMENTATION` and with `fallback`, and checks that `LICENSE.simdcsv` is in the zip.
  Build with `-DFEATURE_PLUGIN_SIMDCSV=OFF` if the build breaks there.

In the WebAssembly VMs the plugin is built in (`SimdCSVPlugin (i)`), with the portable kernel only, and contains no code of simdcsv: no archive is taken and there is no notice.
It adds 39 KB to `pharo-web.wasm` (38,741 bytes; 12,236 with `gzip -9`) and 38 KB to `pharo.wasm` (37,783 bytes; 11,926 with `gzip -9`), measured against the same build with `-DFEATURE_PLUGIN_SIMDCSV=OFF` (Emscripten 6.0.10, 2026-11-24).
The native tree in which `make wasm` generates the sources builds no plugin, but it configures this one and so takes the archive (docs/WebAssembly.md).

### Kernels

| kernel | where | code |
|---|---|---|
| `avx2` | x86_64 with AVX2, BMI, POPCNT and PCLMUL | simdcsv's masks |
| `sse2` | x86_64 | the plugin's |
| `neon` | arm64 | simdcsv's masks, with a shift instead of PMULL |
| `fallback` | everywhere; the only one in WebAssembly | the plugin's, 64-bit words |

The kernel is chosen once, when the module is loaded: the best one that the CPU supports, or the one that the environment variable `SIMDCSV_FORCE_IMPLEMENTATION` names (`avx2`, `sse2`, `neon`, `fallback`) when it is compiled in and supported; else the variable is refused, and `SimdCSV pluginInfo` says so. `SimdCSV implementationName` answers the kernel.

## Loading the Smalltalk side

The Smalltalk code is in the submodule `simdcsv.st`, as Tonel packages that Metacello loads with `BaselineOfSimdCSV`.
Its groups: `Core` (the package `SimdCSV`, which needs no other project), `Tests` (`SimdCSV-Tests`), `default` (both), and `NeoTests` (`SimdCSV-NeoTests`, the comparison with NeoCSV, which it loads at commit `01948fc`; opt-in).
They run on Pharo 12, 13, 14 and 15.

    Metacello new
        baseline: 'SimdCSV';
        repository: 'tonel://<repository>/plugins/SimdCSVPlugin/simdcsv.st/src';
        load: #( 'Core' ).

or, from the command line, `tests/load-simdcsv.st`, which the tests and lane 79 use:

    SIMDCSV_PLUGIN_DIR=plugins/SimdCSVPlugin SIMDCSV_GROUPS='Core Tests' \
    pharo --headless Pharo.image --no-default-preferences st --save --quit plugins/SimdCSVPlugin/tests/load-simdcsv.st

It loads the groups of `SIMDCSV_GROUPS` (`Core Tests` by default) from `SIMDCSV_ST_DIR` (by default the submodule) and prints `SimdCSV: loaded, protocol 1, plugin SimdCSVPlugin 1.0 simdcsv a9494a8 <kernel> <platform>`, or `SimdCSV: loaded, no plugin`, then the groups, the packages and the test packages.
It exits with 1, before the image is saved, when a load fails, the protocol is not 1, `SIMDCSV_EXPECT_PLUGIN` is `yes` and the image finds no plugin, or `no` and it finds one, a known text does not give the known rows, a method of a SimdCSV package uses an undeclared variable, or an extension selector does not begin with `simdCSV`.
Metacello makes UUIDs: the VM needs its UUIDPlugin.
To change the Smalltalk code, commit in the submodule, push it to simdcsv.st, and commit the new submodule commit here, with the plugin changes it goes with.

## API

The `README.md` of the submodule has the examples (each was run on Pharo 12 to 15, with the plugin and without it); the class comments have the details. This is the summary.

- `SimdCSV` (class side only) is the facade.
  A source is a String, a Symbol, a ByteArray, a FileReference or a stream.
  `rowsFrom:` (an Array of Arrays of Strings), `rowsFrom:do:`, `dictionariesFrom:`, `tableFrom:`, `count:` (the records, the header included; no object is made), each with a form `dialect:`; `rowsFrom:separator:` and `rowsFrom:separator:do:`;
  `separatorOffsetsIn:` and `separatorOffsetsIn:dialect:` (of a ByteArray or a String only): the index itself, a `WordArray` of 0-based offsets;
  `stringFrom:`, `bytesFrom:` (UTF-8) and `write:to:`, each with `dialect:`;
  `backend` (`#plugin` or `#smalltalk`), `pluginAvailable`, `pluginInfo`, `implementationName`.
- `SimdCSVDialect`: `comma`, `semicolon` and `tab` on the class side; `separator:`, `quote:` (a Character up to U+00FF, or nil), `lineEnd:` (`#any` or `#lf`), `strictQuoting:`, `strictFieldCount:`, `skipEmptyLines:`, `keepQuotedEmpty:`, `encoding:` (`#utf8` or `#latin1`), `emptyFieldValue:`, `hasHeader:` (true), `nullTexts:`.
  The separator of UTF-8 bytes is ASCII, that of Latin-1 bytes at most U+00FF, that of characters any (with one beyond U+00FF a ByteString is copied to a WideString); neither it nor the quote is a line end.
- `SimdCSVReader`: `on:`, `on:do:` (closes), `next`, `atEnd`, `skip`, `skip:`, `do:`, `collect:`, `select:`, `select:thenDo:`, `reject:`, `reject:thenDo:`, `upToEnd`, `close`; `recordNumber`, `header`, `skipFailedRecord` (of the record whose error was signalled; nothing without one).
  - As in NeoCSV: `separator:`, `emptyFieldValue:` (also `#passNil`), `beStrict`, `recordClass:`, `recordClassIsIndexable:`, `fieldCount:`, `readHeader`, `skipHeader`, `namedColumnsConfiguration`, `addField`, `addField:`, `addField:converter:`, `addFieldAt:`, `addFieldAt:converter:`, `addFieldConverter:`, `addFields:`, `addFieldsAt:`, `addPlainField`, `addPlainFields:`, `addIgnoredField`, `addIgnoredFields:`, and the forms of `addIntegerField`, `addFloatField`, `addFloatFieldRadixPointComma` and `addSymbolField`.
  - New: `dialect:`, `quote:`, `beStrictQuoting`, `skipEmptyLines`, `keepQuotedEmpty`, `encoding:`, `nullTexts:`, `renameDuplicates`, `selectColumns:`, `column:as:`, `column:as:ifInvalid:`, `column:converter:`, `column:converter:cached:`, `addField:converter:cached:`, `addFieldConverter:cached:`, `ifInvalid:`, `maxRecordBytes:`, `maxFields:`, `batchBytes:`.
    Columns by name need a header, and exclude the fields added by position.
  - `SimdCSVReader unsupportedNeoSelectors` lists what `NeoCSVReader` has and it has not.
  - Types: `#string`, `#integer`, `#float`, `#floatComma`, `#symbol`, `#boolean`.
    Numbers are exact: an Integer of at most 4,300 digits, the nearest Float; the plugin converts the short ones (at most 18 digits; decimals that one exact multiplication or division gives), `SimdCSVNumbers` the others. `nan`, `inf`, `infinity` in any case, with a sign before the last two, are Floats.
  - A cell whose text is not of its type is the value of the block of `ifInvalid:` (the text, the number of the record, that of the field), else `SimdCSVConversionError`, which is resumable with the value. A text of `nullTexts:` is an empty cell in a typed or converted column.
  - `cached: true` keeps the values of a converter by text, at most 10,000 texts a column.
- `SimdCSVTable`: `columnNames`, `renameDuplicates`, `rowCount`, `columnCount`, `isRagged`, `rowAt:`, `at:at:`, `rowsFrom:to:`, `do:`, `asArray`, `column:`, `column:as:`, `column:as:ifInvalid:`, `columns:as:`, `columns:as:ifInvalid:`, `float64ArrayAt:`, `float64ArrayAt:ifInvalid:`.
  A column is a name or a number from 1.
  It holds the text and its index (4 bytes a field, 4 more a record when the records have several widths); a row is an Array of `columnCount` cells. `at:at:` is a call of the primitive: take columns for bulk.
- `SimdCSVWriter`: `on:`, `dialect:`, `separator:`, `quote:`, `lineEndConvention:` (`#lf`, `#crlf`, `#cr`), `fieldWriter:` (`#quoted`, `#optionalQuoted`, `#raw`, `#object`), `emptyFieldValue:`, `beUnchecked`, `addField:`, `addFields:`, `addFieldAt:`, `addFieldsAt:`, the same of `addQuotedField:`, `addOptionalQuotedField:`, `addRawField:` and `addObjectField:`, `addConstantField:`, `addEmptyField`, `namedColumnsConfiguration:`, `writeHeader:`, `nextPut:`, `nextPutAll:`, `flush`, `close`. It is Smalltalk: no primitive writes. `SimdCSV write:to:` also takes a FileReference, whose file is replaced.

Inputs: a String or a ByteArray, and a `ReadStream` on one, is read in place; a FileReference through `binaryReadStream`; the stream that `readStream` of a file answers (a `ZnCharacterReadStream` that decodes UTF-8 and has not been peeked) through its binary stream, so that code that gives NeoCSV such a stream gets the bytes path by changing the name of the class; another binary stream as bytes; another stream as characters.
Streams are read ahead by a batch: their position while they are read is unspecified.
A socket (a stream that understands `isDataAvailable`: SocketStream, ZdcSocketStream) is read as it comes: what has arrived, or one unit when nothing has, so that the records that are complete are delivered before more is waited for; a connection that is closed is the end.
Another stream that waits until it has as much as it is asked for (StdioStream on a pipe, the body of an HTTP answer) gives 4 KiB, or its end, before the first record.
A directory is refused.

Errors, all `SimdCSVError`: `SimdCSVParseError` (`position`, `unitName`, `line`, `record`, `field`, `code`, `columnName`) with `SimdCSVQuoteError`, `SimdCSVInvalidEncoding`, `SimdCSVFieldCountMismatch` (`expected`, `actual`), `SimdCSVConversionError` (`text`, `type`) and `SimdCSVHeaderError` (`kind`); `SimdCSVResourceError` (`kind`: `#recordBytes`, `#fields`, `#indexBytes`, `#inputUnits`); `SimdCSVArgumentError`.
Positions count from 1, the byte order mark included; records count the header.

## Semantics

### The automaton

This is the definition; the kernels are used where it is proved that they agree with it. `END` is the separator, LF or CR (with `lineEnd: #lf` CR is data); the LF after a CR that ended a record belongs to that end.

| state | quote | END | any other unit |
|---|---|---|---|
| start of a field | quoted | the field ends | unquoted |
| unquoted | irregular, literal | the field ends | stay |
| quoted | after a quote | stay | stay |
| after a quote | an escaped quote, quoted | the field ends | irregular, literal, unquoted |

At the end of the data an open record that is not empty ends; a field that is still quoted is kept to the end, or with `strictQuoting:` an error at its opening quote. Lenient (the default) is what Python's `csv` module reads: `"a"b` is `ab`, `a"b` is literal. With `strictQuoting:` the first irregular unit is the error.
A blank line is a record of one empty field, or no record with `skipEmptyLines:`. NUL, controls and bytes of 128 and more are data. There are no comments, escape characters, trimming, or separators of several units.

The text of a quoted field (one that is not empty and begins with the quote): after the first quote, a doubled quote is a quote, the next single one closes, and the rest is literal.
An empty field, quoted or not, is `emptyFieldValue` (nil); with `keepQuotedEmpty:` a quoted one is an empty String.
A cell is a new String of the narrowest class (ByteString up to U+00FF, else WideString).

### The check of a block

For 64 bytes let Q be the mask of the quotes, M the prefix xor of Q (the quoted bytes, from an opening quote to the byte before the closing one), O = Q & M the opening and C = Q & ~M the closing quotes, and T the separators and line ends outside of M, with the carries c of the block before:

    bad = (O & ~((T << 1 | cT) | (C << 1 | cC))) | ((C << 1 | cC) & ~(T | O))

When `bad` is 0, T is what the automaton answers; else its first bit is the first irregular unit, and what is before it is right.
The tests check this with every text of `a`, separator, quote, LF and CR of up to 8 units, at three offsets in a block, against a second automaton, and with random ones (see Tests).
Around an irregular unit the automaton takes over from the start of its record, up to the first record end at least 1 KiB further (twice as much at each further one, up to 64 KiB, until a clean stretch), and the kernel resumes there.
Sources of 32-bit units (WideStrings) always go through the automaton.

### Compared with NeoCSV

The comment of `SimdCSVReader` has the table, and the tests a test for each row (`SimdCSVSemanticsTest`, and `SimdCSV-NeoTests` for what is equal). The differences by default:

- records of several widths keep their own when nothing is declared (NeoCSV cuts and pads them to the first); a blank line is a record of one empty field;
- `"a"b` is one field `ab` (NeoCSV: an extra field);
- a byte order mark is skipped (not with `#latin1`);
- `12abc` in an integer column is a `SimdCSVConversionError` (NeoCSV: 12), a float column always gives Floats, and they are the nearest ones;
- a duplicate name in a header that makes Dictionaries is a `SimdCSVHeaderError` (NeoCSV: the last wins), an empty one is `column` and its number;
- invalid UTF-8 is a `SimdCSVInvalidEncoding` in the fields that are built, with its position (ignored columns and counts are not validated);
- the position of a stream while it is read is unspecified.
- The writer leaves an empty field unquoted (NeoCSVWriter writes `""` for it with `#quoted`).

`beStrict` checks the counts of the fields, as NeoCSV's, and `beStrictQuoting` the quotes. A record is delivered, and its accessors, converters and conversions run, when it is reached by `next` or `do:`, never ahead; errors come in their place, after all the records before them, and the reader then stands on the record in error.

## Memory and limits

- The plugin has no C heap and keeps no state but the choice of the kernel: the index is a word array of the image, and the rows are objects that the primitive makes with `instantiateClass:indexableSize:`, which never collects nor moves. Nothing is pinned.
- A call for rows makes about 4 MiB of objects (`budget`), but always tries one record; and it ends when new space is full, before rows are made in old space. So a `do:` over a file of any size makes no full garbage collection and retains nothing (measured below, 100 MB and 2 GB), if a record is not itself larger than new space.
- When not even one record fits, the answer is NO_ROOM with the bytes it needs; `SimdCSVBackend` grows the object memory by that and 1 MiB and calls again, then collects all the garbage and calls a last time, then signals `OutOfMemory`.
- Where old space cannot grow at will (`SimdCSVBackend isBounded`: WebAssembly, and a native VM with its parameter 67 set) the VM does not collect before old space is full, and is then out of memory, or ends, although much of it may be garbage. There `SimdCSVBackend` looks at the free old space before each call for rows: when what the call can make may not fit with 1 MiB to spare, the memory grows, or else all the garbage is collected (not again before 1 MiB more was used, unless a record needs it); then the budget is reduced; and when the first record does not fit, whose exact size the plugin tells (the budget -2), `OutOfMemory` is signalled and nothing is made.
- The reader reads, indexes and builds in batches: a buffer of `batchBytes:` (1 MiB), an index of at most `maxFields:` (65,536) entries, 4,096 rows or 4 MiB of objects a call.
  The buffer of a file that is smaller than a batch is as large as the file; that of another stream is 4 KiB first, and doubled each time that it was filled, up to a batch; any sequence of short reads costs linear time.
- The cost of an error (with its position and its line), of a record that is skipped, of a configuration message between two records and of a column by name does not grow with the batch or the header.
- A record longer than the buffer doubles it, up to `maxRecordBytes:` (64 MiB; 16 MiB in WebAssembly and in a VM whose old space is bounded; up to two units more pass, the size of a line end): `SimdCSVResourceError` `#recordBytes`, which says where a quote was left open. A record of more than `maxFields:` fields: `#fields`. An unclosed quote in a large file is such a record.
- A table has at most 4,294,967,294 units (`#inputUnits`), and its index at most `SimdCSVTable maxIndexBytes` (1 GiB; 256 MiB in WebAssembly and in a VM whose old space is bounded; `maxIndexBytes:` sets it), checked before it is made (`#indexBytes`).
- A header of 65,536 names and a text of blank lines makes rows of 512 KB for each byte of input: with `do:` a bounded batch of them is alive; `upToEnd` keeps them.
- One process uses a reader at a time.

## Primitive contract

This section is the contract of the four primitives; `SimdCSVPrimitives` calls them and `SimdCSVSmalltalk` answers the same.

Positions are 1-based and inclusive; the values inside an index are 0-based offsets from `base`. A source is an object of bytes (no CompiledMethod) or of 32-bit words. Bad data is an answer with a status, never a failure; `position` 0 is none.

| primitive | arguments | answer |
|---|---|---|
| `primitiveInfo` | none | `{ 1. 'SimdCSVPlugin 1.0'. 'a9494a8'. kernel. 'native' or 'emscripten'. Array of the kernels of this machine, best first. the forced kernel, 'refused <value>' or nil. 4294967294 }` |
| `primitiveMeasure` | source, from, to, dialect | `{ status. records. fields. next. lines. code. position }` |
| `primitiveIndex` | source, base, from, to, dialect, dest, destFrom | `{ status. stored. records. next. lines. minFields. maxFields. code. position }` |
| `primitiveRows` | source, base, to, dialect, index, indexFrom, indexTo, types, fieldCount, dest, destFrom, wideClass, emptyValue, budget | `{ status. rows. nextIndex. odd. code. position. row. field. need }` |

| status | meaning |
|---|---|
| 0 OK | the end of the range (`next` may be before it: the open record is not complete), or of the index |
| 1 MORE | no slot left in `dest`; with a budget: it is used, new space is full, or the next record may take more than it; no memory, after at least one row |
| 2 ERROR | `code` and `position` |
| 3 TOO_LARGE | more than 4,294,967,294 units from `base` on |
| 4 NO_ROOM | `primitiveRows` only: the first record did not fit the object memory (or the budget is -2), `need` is the bytes of all its objects |

| code | meaning | from |
|---|---|---|
| 1 | a quote inside an unquoted field (STRICT) | measure, index |
| 2 | text after a closing quote (STRICT) | measure, index |
| 3 | an unterminated quote; `position` is the opening one (STRICT) | measure, index |
| 4 | invalid UTF-8; `position` is the first byte of the bad sequence. The units are validated as they stand in the source: a quote that is removed from the text never joins the halves of a sequence | rows |
| 5 | a record of another count of fields (EXACT_COUNT): `field` is its count, `position` its start | rows |
| 6 | the index does not fit the source | rows |

**The dialect word**: bits 0 to 20 the separator, 21 to 28 the quote, 29 to 44 the flags; any other bit is refused.

| flag | value | meaning |
|---|---|---|
| STRICT | 1 | an irregular unit is an error |
| LF_ONLY | 2 | CR is data |
| NO_QUOTE | 4 | no unit is a quote |
| SKIP_BLANK | 8 | a blank line is no record |
| LATIN1 | 16 | bytes are characters, else UTF-8 |
| FINAL | 32 | the range ends the data |
| LINES | 64 | count the physical line ends (quoted ones too, a CRLF once) from `from` to `next` |
| EXACT_COUNT | 128 | `primitiveRows` with a fieldCount: another count is an error |
| KEEP_QUOTED_EMPTY | 256 | a quoted empty field of type 1 is a new empty ByteString |

Refused too: a separator that is a line end, or the quote (without NO_QUOTE); a quote that is a line end; for bytes a separator or quote above 127 without LATIN1, or a separator above 255; LATIN1 for words.

**Checks**, in this order; a failure leaves the stack alone:

1. the count of the arguments: `bad number of arguments`;
2. the source: `bad argument`;
3. the numbers are SmallIntegers: `bad argument`;
4. the dialect: `bad argument`;
5. the ranges: `bad index`. Of the source (`from` is at least 1, `to` at most its size, `to` at least `from - 1`; `base` from 1 to `from`), of the index, `destFrom` from 1 to the size of `dest` and 1, `fieldCount` from 0 to 16,777,216;
6. the class of wide strings (32-bit words, no named slots, a class, not BoxedFloat64): `bad argument`;
7. the index (words), the types (nil, or a ByteArray of codes 0 to 4, as many as `fieldCount` when that is not 0), the destination (words for `primitiveIndex`, an Array for `primitiveRows`, not the source): `bad argument`; immutable: `no modification`.

The answer Array is made first: only its allocation fails with `insufficient object memory` (`primitiveInfo` also by its Strings). An allocation that fails for another reason fails the primitive with that reason.

**primitiveMeasure and primitiveIndex** read `[from, to]`, which begins at the start of a record, and answer for the COMPLETE records in it: `next` is the start of the first one that is not. Without FINAL neither the open record is complete nor one that ends with a CR in the last position. Events come in input order: no slot left (MORE), an irregular unit with STRICT (ERROR), the end (OK). MORE with `stored` 0 is a record of more fields than `dest` has slots. A field that ends without a slot is MORE even where its end is a CR in the last position, which without FINAL would end no record. With SKIP_BLANK `next` is after the blank lines before the record that it is the start of, whatever the status. `minFields` and `maxFields` are 0 when `records` is. Slots of `dest` beyond those stored may hold garbage.

An index entry is the offset of the unit that ends a field (separator, LF, CR), or of the end of the data for a last field without a line end.

**primitiveRows** builds the records of the entries `indexFrom` to `indexTo` (none when `indexTo` is `indexFrom - 1`); the first record starts at `base` when `indexFrom` is 1, else after the entry before. Types: 0 ignore, 1 String, 2 Integer, 3 Float, 4 Float with a decimal comma.

- `fieldCount` n > 0: rows of n fields without the ignored ones, missing ones empty, further ones skipped. 0: rows as wide as their records; fields beyond the types are Strings.
- A cell of type 2 to 4 that the plugin does not convert (and is not empty) is left as a String and counted in `odd`. It converts an integer of at most 18 digits after the leading zeros, and a decimal of at most 19 significant digits whose mantissa is at most 2^53 and whose power of ten is exact, spaces and tabs around; no inf, nan or other text.
- `budget`: bytes of objects after which no further record begins, -1 for none. One record is always tried. With a budget no record begins either after one whose Array had to be made in old space (new space is full, and the VM scavenges as soon as the primitive has returned), nor, but for the first, one of which more than 64 KiB and more than the budget might be made (an Array of its width, 24 bytes a field and 4 a unit): so a call makes at most the first record, or twice the budget and 64 KiB. -2: nothing is made, the answer is NO_ROOM with the `need` of the first record (or its ERROR). `SimdCSVSmalltalk` ignores the budget.
- The cursor trusts neither index nor source: each entry must be at or after the start of its field and within the source, and the unit there must be a separator or a line end (or the end of the data), else ERROR 6: a decreasing, truncated or random index, or a source that was changed, never makes it read outside of either.
- On an error the rows before the record are stored, `row` is the number of the bad record in this call, `nextIndex` its first entry.

Module functions: `initialiseModule` chooses the kernel (the only `getenv`), `shutdownModule` has nothing to release. There is no `primitiveRelease`.

## Tests

- **C tests**: `sh plugins/SimdCSVPlugin/tests/run-c-tests.sh --simdcsv <archive> --emsdk <emsdk> --vm-build <tree>`. It cuts the slice itself and checks the SHA256. Lanes: `engine` (clang), `engine-gcc`, `engine-uchar`, `engine-O0`, `engine-O3`, `engine-asan` (ASan and UBSan), `engine-msan`, `guard` (the choice of the kernel), `upstream` (simdcsv's own `find_indexes` against `sc_index` on regular texts), `symbols`, `wasm64` and `memory64-2` (emcc under node), `glue` (against a fake VM), and, opt-in, `arm64` (qemu), `fuzz` and `4gib`; the static rules R2, R3, R6, R8, R11, R12 (the hashes of `cmake/simdcsv.cmake`, the kernels in CI) and V (the pin) come first, and R1, R5, R7 in `glue.sh`.
  `sctest.cpp` compares every kernel of the machine with a second automaton written from the definition: every text of up to 8 units of the five that matter, the dialects, every pair of bytes against every needle, guard pages, every capacity, every split point, the order of the events, irregular quotes, hostile indexes, 10^6 numbers against `strtod`, UTF-8.
- **Smalltalk tests**: `sh plugins/SimdCSVPlugin/tests/run-st-tests.sh <vm-dir> <image>`, on Pharo 12, 13, 14 and 15, with the plugin and without it (`SIMDCSV_EXPECT_PLUGIN=no`). `SimdCSV-Tests` runs every test on the backends that are there, `SimdCSV-NeoTests` (group `NeoTests`) compares with NeoCSV. `SIMDCSV_SLOW_TESTS=yes` adds 100 MB of text and a comparison with Python's csv module.
- **WebAssembly**: lane 79 (`tests/wasm/lanes/79-simdcsv.sh`, docs/WebAssembly.md).
- **CI**: the step "Load the SimdCSVPlugin of the zip" of each native job.

## Benchmarks

Machine: Intel Xeon Gold 6238R at 2.20 GHz (2 sockets, 56 cores; Linux 6.8.0), 2026-11-24. Every VM and the engine are pinned to a core of their own that was idle (`taskset`); other processes ran on other cores. Image: the stock Pharo 15 image, with SimdCSV and, in the image of the benchmark only, NeoCSV (commit `01948fc`). VMs: the CoInterpreter (JIT) and the StackVM of this branch, clang 18.1.3, the kernel `avx2`; `pharo.wasm` (Emscripten 6.0.10, the StackVM, node 24.19.0), the kernel `fallback`. arm64, macOS and Windows: not measured.

    sh plugins/SimdCSVPlugin/bench/run-bench.sh --cog <tree> --stack <tree> --wasm <make wasm tree> --node <node> \
        --simdcsv simdcsv-a9494a8.tar.gz --image Pharo15.zip --cpu 10 --repeat 3 --out <dir>

`SCBench.st` generates the texts (the same on every VM), checks that each result is that of NeoCSV (979 checks, all equal), and times 5 samples a series, each after a full garbage collection that is not timed; a number is the median of the 5, then of the runs (3 of the JIT and the engine, 2 of the others). At 10 MB a sample is one call; in the series "3 calls a sample" (`rowsKept`) it is at least three, and the collections that the rows of one call cause in the next are in the time. `scbench.cpp` times the engine alone, and simdcsv's `find_indexes` on the same texts, in turn. MB are 10^6 bytes. `summarize.py` makes the tables and checks the gates.

The texts: `mixed` (10 columns: an integer, a day, words, some of them quoted, Floats of 2 and 5 decimals, some empty fields; 79 bytes a record), `ints` (8 columns of 1 to 10 digits, 6.4 bytes a field with its separator), `quoted` (4 columns, all quoted), `utf8` (Latin-1 and CJK words), `floats17` (4 Floats of 16 or 17 digits), `inches` (a quote inside an unquoted field in 1 record of 10), `gtfs` (`stop_times.txt`: 6 columns, 54 bytes a record, two times made by converters).
NeoCSV reads a String, or the stream of `file readStream`; SimdCSV reads the bytes of the same text, or the file.

### Gates

| gate | result | measured |
|---|---|---|
| rows >= 8x NeoCSV, 1 MB | met | mixed 10.8x, ints 9.1x, quoted 11.3x, utf8 12.6x; 3 calls a sample: mixed 11.9x, ints 9.1x, quoted 11.0x, utf8 12.1x |
| the same, 10 MB, one call on a heap that was just collected | met | mixed 8.6x, ints 8.2x, quoted 9.2x, utf8 10.9x |
| the same, 10 MB, 3 calls a sample | **missed** | mixed 4.5x, ints 4.9x, quoted 6.1x, utf8 6.7x |
| typed rows of mixed >= 12x | met | 23.1x (1 MB), 20.6x (10 MB) |
| `do:` on a file >= 10x | met | 1 MB: mixed 13.2x, ints 10.2x, quoted 13.4x, utf8 13.6x; 10 MB: mixed 13.3x, ints 10.3x, quoted 13.6x, utf8 14.2x |
| gtfs, cached converters, >= 6x NeoCSV (not cached), 10 MB | met | 8.7x from a FileReference, 8.8x from `fr readStream` |
| the same, 1 MB | **missed** | 5.2x and 5.2x |
| `count:` >= 1 GB/s | met | 2.9 to 4.0 GB/s (1 MB), 3.0 to 3.2 (10 MB) |
| index >= 0.6x simdcsv's `find_indexes`, regular texts | met but for utf8, **missed** | mixed 0.67x, ints 0.69x, quoted 0.62x, utf8 0.59x, floats17 0.64x, gtfs 0.63x |
| rows of inches >= 3x | met | 7.9x (1 MB), 7.1x (10 MB) |
| no full GC in a `do:` over 100 MB, < 5 MB retained | met | 0 full GCs, 0 bytes, in 3 runs |
| WebAssembly (node), rows >= 8x, 1 MB | met | mixed 34.5x, ints 32.1x, quoted 36.9x, utf8 41.8x |
| equal results | met | 979 of 979 checks |

Reported without a gate: the gate of gtfs is against NeoCSV without a cache, as an application that only changes the name of the class and adds `cached: true`; NeoCSV whose converters look the texts up in a Dictionary is at 1.36 s (10 MB), and SimdCSV 5.2x as fast (3.8x at 1 MB). gtfs with converters that are not cached is 2.1x NeoCSV (the converter alone, `Time` from the split text, costs 1.9 microseconds, twice a record), and the typed `floats17` 5.0x: the plugin leaves its 17-digit Floats to the image, which converts them exactly.

The missed gates, profiled:

- **rows at 10 MB, in repeated use: 4.5x to 6.7x.** The Strings that a call keeps make old space grow by a third of its size (vm parameter 55, 0.33), and the VM then collects all of it: 5 to 10 times in the 15 calls of a series, each as long as a call or longer. A call on a heap that was just collected keeps less than that and has none (0 in the samples of `rows`); which of the two a program sees depends on its heap. NeoCSV has as many collections or more, which weigh less in it. `rowsFrom:do:` of the same texts keeps nothing, makes no collection, and has the 10.7x of mixed in any use; so has `do:` on a file.
- **gtfs cached at 1 MB, 5.2x.** Of 38.5 ms, 23 milliseconds are in the two cached converter columns (profiled before the last changes of the reader), 14 of them the first 1,380 different times of each (5 microseconds each: the converter, the copy into the Dictionary, its growth), and 9 the 18,442 lookups of each (0.26 microseconds). At 10 MB the first evaluations are 1 in 130 of the texts, and the gate is met.
- **index, 0.59x to 0.69x of simdcsv.** The index also checks every quote, finds CR, counts the records and their widths. It prefetches as simdcsv does, 768 bytes ahead, and the output 2 KiB ahead: without these two lines it is at 0.50x to 0.60x. In builds of the engine with one thing changed each (64 MB, medians of 3 to 5 runs on a pinned core, over simdcsv timed in the same run):

  | engine | mixed | ints | quoted | utf8 | floats17 | gtfs |
  |---|---|---|---|---|---|---|
  | without a prefetch | 0.59 | 0.60 | 0.53 | 0.52 | 0.56 | 0.58 |
  | of the input, 128 bytes ahead | 0.66 | 0.67 | 0.62 | 0.58 | 0.63 | 0.61 |
  | 512 bytes ahead | 0.67 | 0.69 | 0.64 | 0.61 | 0.66 | 0.66 |
  | 768 bytes ahead | - | 0.69 | 0.64 | 0.60 | - | - |
  | 1,024 bytes ahead | 0.68 | 0.70 | 0.68 | 0.59 | 0.68 | 0.68 |
  | 2,048 bytes ahead | 0.70 | 0.69 | 0.63 | 0.61 | 0.69 | 0.66 |
  | 768 bytes ahead, and of the output (as it is) | - | 0.71 | 0.66 | 0.62 | - | - |

  (The check of the quotes is what the engine is for; a build without it, of the engine before the prefetch, was at 0.66 to 0.78.) The numbers of one build move by 0.01 to 0.05 from run to run; utf8 stands at the threshold. In the reader, the index of `mixed` takes 0.37 of the 8.5 milliseconds of `rowsFrom:` (1 MB).

### The JIT (CoInterpreter), 10 MB

| series | NeoCSV | SimdCSV | speedup |
|---|---|---|---|
| `rowsFrom:` mixed | 11.1 MB/s | 95.9 MB/s (104 ms) | 8.6x |
| `rowsFrom:` ints | 9.2 | 76.0 (132 ms) | 8.2x |
| `rowsFrom:` quoted | 15.1 | 138.4 (72.2 ms) | 9.2x |
| `rowsFrom:` utf8 | 11.0 | 119.3 (83.8 ms) | 10.9x |
| `rowsFrom:` floats17 | 13.7 | 211.6 (47.3 ms) | 15.4x |
| `rowsFrom:` inches | 9.5 | 68.1 (147 ms) | 7.1x |
| `rowsFrom:` gtfs | 10.2 | 105.8 (94.6 ms) | 10.3x |
| `rowsFrom:` mixed, 3 calls a sample | 9.8 | 43.9 (228 ms) | 4.5x |
| `rowsFrom:` ints, 3 calls a sample | 8.8 | 43.2 (232 ms) | 4.9x |
| `rowsFrom:` quoted, 3 calls a sample | 13.4 | 82.4 (121 ms) | 6.1x |
| `rowsFrom:` utf8, 3 calls a sample | 9.5 | 64.0 (156 ms) | 6.7x |
| `rowsFrom:` mixed, from the String | 11.1 | 104.9 (95.3 ms) | 9.4x |
| `rowsFrom:do:` mixed | 12.7 | 135.7 (73.7 ms) | 10.7x |
| 3 columns of 10 by name, mixed | 10.8 | 269.0 (37.2 ms) | 24.9x |
| typed rows, mixed | 6.3 | 129.7 (77.1 ms) | 20.6x |
| typed rows, ints | 5.2 | 139.3 (71.8 ms) | 26.6x |
| typed rows, floats17 | 4.6 | 22.8 (438 ms) | 5.0x |
| 3 Float columns, mixed (table, `columns:as:`) | 7.5 | 208.2 (48.0 ms) | 27.7x |
| `rowsFrom:do:` on a file, mixed | 9.9 | 130.9 (76.4 ms) | 13.3x |
| `rowsFrom:do:` on a file, ints | 9.3 | 95.6 (105 ms) | 10.3x |
| gtfs objects, plain, FileReference | 4.4 | 9.1 (1.10 s) | 2.1x |
| gtfs objects, cached, FileReference | 4.4 | 38.6 (259 ms) | 8.7x |
| gtfs objects, cached, `fr readStream` | 4.4 | 39.0 (257 ms) | 8.8x |
| gtfs objects, cached, NeoCSV with a Dictionary too | 7.4 | 38.6 (259 ms) | 5.2x |
| `count:` mixed | 12.1 | 3,089 (3.2 ms) | 256x |
| `count:` inches | 12.4 | 287.3 (34.8 ms) | 23.2x |
| the table of mixed made | - | 1,520 (6.6 ms) | - |
| `float64ArrayAt:`, one column, mixed | - | 393.0 (25.4 ms) | - |
| `stringFrom:`, all quoted, mixed | 32.5 | 16.6 (601 ms) | 0.51x |
| `stringFrom:`, all quoted, utf8 | 14.7 | 7.0 (1.44 s) | 0.47x |
| `stringFrom:`, quoted where needed, mixed | 8.9 | 13.6 (734 ms) | 1.5x |
| `stringFrom:`, quoted where needed, utf8 | 8.7 | 6.8 (1.46 s) | 0.78x |

The writer is Smalltalk: at 1 MB it is 0.89x as fast as NeoCSV's when it quotes every field (28.3 and 31.9 MB/s) and 1.7x when it quotes where needed; on utf8, where every record goes through its buffer of wide characters, 0.53x and 1.3x. At 10 MB it makes 1 to 7 full garbage collections a call. This is the benchmark that asks for a primitive that writes.
The whole of `lines` and `split:` of the String, for texts without quotes: 29.8 MB/s (ints, 1 MB), 15.2 (10 MB), 15.7 (utf8, 1 MB), 78.8 (floats17, 1 MB).
100 MB of `mixed` in a file, `rowsFrom:do:` (1.27 million records): 735 to 746 ms (134 to 136 MB/s), 21 scavenges, no full garbage collection, old space grows by 16 MiB (a segment), 0 bytes are retained; NeoCSV 10.3 to 10.8 s. 2 GB: 15.3 s, 430 scavenges, no full garbage collection, 1.1 MB of old space used meanwhile (the buffer), nothing retained.

Transposition. `columns:as:` of 3 Float columns at 10 MB takes 48.0 ms on the JIT: the table 6.6, the rows of the builder 27.9, the transposition in the image 13.5 (28%; 29% at 1 MB). On the StackVM the transposition is 55% (10 MB; 56% at 1 MB), and in WebAssembly 59% (1 MB). Over 50% of the load is the case for a primitive that makes columns (`primitiveColumns`) in a second version: it is there on the StackVM and in WebAssembly, and not on the JIT.

### StackVM and WebAssembly

Speedup over NeoCSV on the same VM, with the plugin.

| series | JIT 1 MB | JIT 10 MB | StackVM 1 MB | StackVM 10 MB | WebAssembly 1 MB |
|---|---|---|---|---|---|
| `rowsFrom:` mixed | 10.8x | 8.6x | 32.5x | 26.7x | 34.5x |
| `rowsFrom:` ints | 9.1x | 8.2x | 28.7x | 23.3x | 32.1x |
| `rowsFrom:` quoted | 11.3x | 9.2x | 34.0x | 27.5x | 36.9x |
| `rowsFrom:` utf8 | 12.6x | 10.9x | 37.8x | 32.3x | 41.8x |
| `rowsFrom:` mixed, 3 calls a sample | 11.9x | 4.5x | 37.6x | 13.0x | 36.9x |
| typed rows, mixed | 23.1x | 20.6x | 84.6x | 72.8x | 83.0x |
| `rowsFrom:do:` on a file, mixed | 13.2x | 13.3x | 46.2x | 46.9x | 45.5x |
| 3 columns of 10, mixed | 22.9x | 24.9x | 73.5x | 76.7x | 72.4x |
| gtfs objects, cached, FileReference | 5.2x | 8.7x | 10.1x | 20.3x | 9.1x |
| `count:` mixed | 301x | 256x | 918x | 761x | 514x |

In WebAssembly (node, 1 MB) `rowsFrom:` reads `mixed` at 62.5 MB/s (NeoCSV: 1.8), `count:` at 955 MB/s, the index alone 1,049 MB/s.
Without the plugin, `SimdCSVSmalltalk` reads `mixed` at 15.8 MB/s on the JIT (NeoCSV: 10.9), 0.73x to 1.5x NeoCSV over the texts and the VMs, and 2.2x for typed rows of mixed (JIT).

### Small sources

Microseconds for a source of records of 3 fields of 1 character (`rowsFrom:`, or a reader and `upToEnd`; no full garbage collection in any of these series).

| source | JIT: NeoCSV | JIT: SimdCSV | StackVM: NeoCSV | StackVM: SimdCSV | WebAssembly: NeoCSV | WebAssembly: SimdCSV |
|---|---|---|---|---|---|---|
| a String of 1 record | 1.5 | 2.5 | 5.0 | 13.9 | 10.0 | 27.0 |
| 3 records | 3.2 | 2.9 | 11.5 | 14.3 | 22.0 | 27.5 |
| 100 records | 77.3 | 18.0 | 305.4 | 30.7 | 587.5 | 60.0 |
| a file of 2 records | 23.9 | 33.0 | 71.4 | 85.0 | 148.0 | 182.5 |
| a `ZnCharacterReadStream` on 2 records | 2.8 | 5.9 | 10.2 | 28.3 | 19.0 | 48.0 |

A reader costs about 1 microsecond more than NeoCSV's to make, so that the plugin begins to win at about 3 short lines; a String of one line costs 1.7 times as much as with NeoCSV, a file of two 1.4 times, a stream 2.1 times (a file is asked whether it is a directory, and for its size; the buffer of a stream is 4 KiB). On the StackVM and in WebAssembly the break-even is at about 4 lines.

### The engine

GB/s of 10^9 bytes, at least 64 MB of text, median of 9, then of 3 runs. `index`: one call; `count`: `sc_measure` with the line count; `batches`: the reader's 1 MiB calls with the line count; `avx2 / simdcsv`: the median of the ratios of the 9 repetitions, in which the two were timed in turn.

| text | avx2 | sse2 | fallback | simdcsv (AVX2) | avx2 / simdcsv | of the 3 runs | avx2, batches | avx2, count |
|---|---|---|---|---|---|---|---|---|
| mixed | 3.17 | 2.16 | 1.24 | 4.78 | 0.67 | 0.66 to 0.68 | 2.51 | 2.47 |
| ints | 2.94 | 1.84 | 1.13 | 4.25 | 0.69 | 0.68 to 0.71 | 2.40 | 2.58 |
| quoted | 3.77 | 2.85 | 1.40 | 6.05 | 0.62 | 0.62 to 0.67 | 2.93 | 2.55 |
| utf8 | 3.44 | 2.42 | 1.30 | 5.84 | 0.59 | 0.59 to 0.60 | 2.73 | 2.56 |
| floats17 | 3.92 | 3.19 | 1.46 | 6.04 | 0.64 | 0.64 to 0.66 | 2.99 | 2.61 |
| gtfs | 3.73 | 2.72 | 1.36 | 5.90 | 0.63 | 0.63 to 0.64 | 2.82 | 2.53 |
| inches | 0.30 | 0.30 | 0.30 | (another index) | - | - | 0.29 | 0.29 |

On `inches` the automaton does nearly all of the work, as a quote inside a field in 1 record of 10 does not leave the kernel 1 KiB of clean text: 0.30 GB/s, which is still 23 times NeoCSV's `do:` for `count:`. simdcsv's index of it is wrong, not only different.

The spread between the runs of a series: 2.3% in the median (the largest 83.8%, indexLines floats17 10 plugin) on the JIT, 2.0% in the median (the largest 153.0%, count inches 1 plugin) on the StackVM, 1.0% in the median (the largest 67.9%, count quoted 1 smalltalk) in WebAssembly.

## Licences

The part of `src/main.cpp` of simdcsv a9494a8 (Apache-2.0) from 'struct simd_input' to flatten_bits is included byte for byte (SHA256 checked). On x86_64 `__AVX2__` is defined for the include; on arm64 its `vmull_p64` is replaced by a macro (a shift prefix-xor, no PMULL) and `neonmovemask_bulk`, which upstream does not define, is ours. The loop, the tail, the checks, the other kernels, the builders and the errors are ours. WebAssembly builds contain no simdcsv code.

simdcsv has no NOTICE file. Its licence is installed as `LICENSE.simdcsv` next to the plugin (in `Contents/Resources` on macOS) of every native build.
The plugin is under the MIT licence of this repository, the packages of simdcsv.st under the MIT licence.

## Limitations

- 64-bit VMs only.
- UTF-8 and Latin-1 only; UTF-16 must be decoded first (its byte order mark is an error in UTF-8 bytes).
- No comments, escape characters, trimmed spaces, separators of several characters, or line ends that are CRLF only.
- Neither the kernels nor the reader detect a dialect or the types of the columns.
- Floats of 17 digits are converted in the image (see Benchmarks).
- The WebAssembly build is scalar.
- Wide strings are indexed unit by unit.
- A table transposes columns in the image (see Benchmarks).
- arm64, macOS and Windows are not measured.
