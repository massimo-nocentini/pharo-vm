# SimdCSV: reading and writing CSV on simdcsv

The SimdCSVPlugin reads CSV with [simdcsv](https://github.com/geofflangdale/simdcsv) (Geoff Langdale, commit `a9494a8`), and the package `SimdCSV` (the submodule `plugins/SimdCSVPlugin/simdcsv.st`) gives Pharo 12 to 15 rows, typed records, tables and columns on top of it, and a writer.
It works in two steps: **the index**, the offset of the unit that ends each field, 32 bits a field, found 64 bytes at a time and stored in a word array of the image; and **the rows**, Arrays of Strings, Integers and Floats that are built in C from such an index, in batches whose object memory is bounded.
The reader and the writer have the protocols of `NeoCSVReader` and `NeoCSVWriter` under their own names; nothing of NeoCSV is required, shadowed or extended, and nothing of Pharo is overridden: the only extension method is `simdCSVFailed`.
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
- `-DSIMDCSV_SOURCE_DIR=<tree>` takes a simdcsv source tree instead, an extraction of that archive or a clone at that commit.

Either way the bytes of `src/main.cpp` from `struct simd_input {` to the comment after `flatten_bits` are cut out at configure time, and their SHA256 must be `bf5511f1…b68d50874`; else the configuration stops and names both hashes. The slice is `<build>/deps/simdcsv-slice/simdcsv_slice.inc`, which only `scKernels.inc` includes.

The engine is the static library `pharo_sc_engine`, in `<build>/deps` (never in `build/vm`, which is packaged whole), compiled with `-O3 -g0 -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off`, hidden visibility, and no `-m` flag (`-m64` and the setjmp/longjmp flags of the VM on Emscripten): the kernels choose their instructions with target pragmas, and the CPU is asked at run time.
It uses no C++ header, exception, RTTI, static constructor, heap or C++ library, and the plugin links it by its file, not by its target, so that CMake does not link the plugin with the C++ driver: `libSimdCSVPlugin.so` needs no libstdc++.
The glue is C99, `-O2`.
Natively `libSimdCSVPlugin.so` is 106 KB (108,408 bytes; 66 KB stripped), on Linux x86_64 with clang 18.1.3, and exports only its module functions, its four primitives and their accessor depths, all 0.

Platforms:

- Linux x86_64 and Emscripten (the node and web VMs of `make wasm`) are built and tested here; the engine's tests also run for arm64 under qemu (`-cpu cortex-a72`, without PMULL);
- macOS (x86_64 and arm64), Windows (MSYS2 with clang on x86_64, CLANGARM64 on arm64) and Linux aarch64 are written for, but **untested** until CI runs.
  Each native job of `.github/workflows/build.yml` loads the plugin of its zip with `plugins/SimdCSVPlugin/tests/ci-load.st`, with the best kernel, with a bogus `SIMDCSV_FORCE_IMPLEMENTATION` and with `fallback`, and checks that `LICENSE.simdcsv` is in the zip.
  Build with `-DFEATURE_PLUGIN_SIMDCSV=OFF` if the build breaks there.

In the WebAssembly VMs the plugin is built in (`SimdCSVPlugin (i)`), with the portable kernel only, and contains no code of simdcsv: no archive is taken and there is no notice.
It adds 38 KB to `pharo-web.wasm` (38,405 bytes; 12,018 with `gzip -9`) and 37 KB to `pharo.wasm` (37,451 bytes; 11,839 with `gzip -9`), measured against the same build with `-DFEATURE_PLUGIN_SIMDCSV=OFF` (Emscripten 6.0.10, 2026-11-24).
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

or, from the command line, `tests/load-simdcsv.st`, which the tests and lane 77 use:

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
  The separator of UTF-8 bytes is ASCII, that of Latin-1 bytes and of ByteStrings at most U+00FF, that of a WideString any character; neither it nor the quote is a line end.
- `SimdCSVReader`: `on:`, `on:do:` (closes), `next`, `atEnd`, `skip`, `skip:`, `do:`, `collect:`, `select:`, `select:thenDo:`, `reject:`, `reject:thenDo:`, `upToEnd`, `close`; `recordNumber`, `header`, `skipFailedRecord`.
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
- `SimdCSVWriter`: `on:`, `dialect:`, `separator:`, `quote:`, `lineEndConvention:` (`#lf`, `#crlf`, `#cr`), `fieldWriter:` (`#quoted`, `#optionalQuoted`, `#raw`), `emptyFieldValue:`, `beUnchecked`, `addField:`, `addFields:`, `addFieldAt:`, `addFieldsAt:`, `addConstantField:`, `addEmptyField`, `namedColumnsConfiguration:`, `writeHeader:`, `nextPut:`, `nextPutAll:`, `flush`, `close`. It is Smalltalk: no primitive writes.

Inputs: a String or a ByteArray, and a `ReadStream` on one, is read in place; a FileReference through `binaryReadStream`; the stream that `readStream` of a file answers (a `ZnCharacterReadStream` that decodes UTF-8 and has not been peeked) through its binary stream, so that code that gives NeoCSV such a stream gets the bytes path by changing the name of the class; another binary stream as bytes; another stream as characters.
Streams are read ahead by a batch: their position while they are read is unspecified.

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
- A call for rows makes at most 4 MiB of objects (`budget`), but always tries one record; so a `do:` over a file of any size makes no full garbage collection and retains nothing (measured below).
- When not even one record fits, the answer is NO_ROOM with the bytes it needs; `SimdCSVBackend` grows the object memory by that and 1 MiB and calls again, then collects all the garbage and calls a last time, then signals `OutOfMemory`.
- The reader reads, indexes and builds in batches: a buffer of `batchBytes:` (1 MiB), an index of at most `maxFields:` (65,536) entries, 4,096 rows or 4 MiB of objects a call.
  A stream that is no file is read 4 KiB first, then twice as much each time, up to a batch; any sequence of short reads costs linear time.
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
| 1 MORE | no slot left in `dest`; the budget is used; no memory, after at least one row |
| 2 ERROR | `code` and `position` |
| 3 TOO_LARGE | more than 4,294,967,294 units from `base` on |
| 4 NO_ROOM | `primitiveRows` only: the first record did not fit the object memory, `need` is the bytes of all its objects |

| code | meaning | from |
|---|---|---|
| 1 | a quote inside an unquoted field (STRICT) | measure, index |
| 2 | text after a closing quote (STRICT) | measure, index |
| 3 | an unterminated quote; `position` is the opening one (STRICT) | measure, index |
| 4 | invalid UTF-8; `position` is the first byte of the bad sequence | rows |
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

**primitiveMeasure and primitiveIndex** read `[from, to]`, which begins at the start of a record, and answer for the COMPLETE records in it: `next` is the start of the first one that is not. Without FINAL neither the open record is complete nor one that ends with a CR in the last position. Events come in input order: no slot left (MORE), an irregular unit with STRICT (ERROR), the end (OK). MORE with `stored` 0 is a record of more fields than `dest` has slots. `minFields` and `maxFields` are 0 when `records` is. Slots of `dest` beyond those stored may hold garbage.

An index entry is the offset of the unit that ends a field (separator, LF, CR), or of the end of the data for a last field without a line end.

**primitiveRows** builds the records of the entries `indexFrom` to `indexTo` (none when `indexTo` is `indexFrom - 1`); the first record starts at `base` when `indexFrom` is 1, else after the entry before. Types: 0 ignore, 1 String, 2 Integer, 3 Float, 4 Float with a decimal comma.

- `fieldCount` n > 0: rows of n fields without the ignored ones, missing ones empty, further ones skipped. 0: rows as wide as their records; fields beyond the types are Strings.
- A cell of type 2 to 4 that the plugin does not convert (and is not empty) is left as a String and counted in `odd`. It converts an integer of at most 18 digits after the leading zeros, and a decimal of at most 19 significant digits whose mantissa is at most 2^53 and whose power of ten is exact, spaces and tabs around; no inf, nan or other text.
- `budget`: bytes of objects after which no further record begins, -1 for none. One record is always tried.
- The cursor trusts neither index nor source: each entry must be at or after the start of its field and within the source, and the unit there must be a separator or a line end (or the end of the data), else ERROR 6: a decreasing, truncated or random index, or a source that was changed, never makes it read outside of either.
- On an error the rows before the record are stored, `row` is the number of the bad record in this call, `nextIndex` its first entry.

Module functions: `initialiseModule` chooses the kernel (the only `getenv`), `shutdownModule` has nothing to release. There is no `primitiveRelease`.

## Tests

- **C tests**: `sh plugins/SimdCSVPlugin/tests/run-c-tests.sh --simdcsv <archive> --emsdk <emsdk> --vm-build <tree>`. It cuts the slice itself and checks the SHA256. Lanes: `engine` (clang), `engine-gcc`, `engine-uchar`, `engine-O0`, `engine-O3`, `engine-asan` (ASan and UBSan), `engine-msan`, `guard` (the choice of the kernel), `upstream` (simdcsv's own `find_indexes` against `sc_index` on regular texts), `symbols`, `wasm64` and `memory64-2` (emcc under node), `glue` (against a fake VM), and, opt-in, `arm64` (qemu), `fuzz` and `4gib`; the static rules R2, R3, R6, R8, R11 and V (the pin) come first, and R1, R5, R7 in `glue.sh`.
  `sctest.cpp` compares every kernel of the machine with a second automaton written from the definition: every text of up to 8 units of the five that matter, the dialects, every pair of bytes against every needle, guard pages, every capacity, every split point, the order of the events, irregular quotes, hostile indexes, 10^6 numbers against `strtod`, UTF-8.
- **Smalltalk tests**: `sh plugins/SimdCSVPlugin/tests/run-st-tests.sh <vm-dir> <image>`, on Pharo 12, 13, 14 and 15, with the plugin and without it (`SIMDCSV_EXPECT_PLUGIN=no`). `SimdCSV-Tests` runs every test on the backends that are there, `SimdCSV-NeoTests` (group `NeoTests`) compares with NeoCSV. `SIMDCSV_SLOW_TESTS=yes` adds 100 MB of text and a comparison with Python's csv module.
- **WebAssembly**: lane 77 (`tests/wasm/lanes/77-simdcsv.sh`, docs/WebAssembly.md).
- **CI**: the step "Load the SimdCSVPlugin of the zip" of each native job.

## Benchmarks

Machine: Intel Xeon Gold 6238R at 2.20 GHz (2 sockets, 56 cores; Linux 6.8.0), 2026-11-24. Every VM and the engine are pinned to a core of their own that was idle (`taskset`); other processes ran on other cores. Image: the stock Pharo 15 image, with SimdCSV and, in the image of the benchmark only, NeoCSV (commit `01948fc`). VMs: the CoInterpreter (JIT) and the StackVM of this branch, clang 18.1.3, the kernel `avx2`; `pharo.wasm` (Emscripten 6.0.10, the StackVM, node 24.19.0), the kernel `fallback`. arm64, macOS and Windows: not measured.

    sh plugins/SimdCSVPlugin/bench/run-bench.sh --cog <tree> --stack <tree> --wasm <make wasm tree> --node <node> \
        --simdcsv simdcsv-a9494a8.tar.gz --image Pharo15.zip --cpu 10 --repeat 3 --out <dir>

`SCBench.st` generates the texts (the same on every VM), checks that each result is that of NeoCSV (879 checks, all equal), and times 5 samples a series, each after a full garbage collection; a number is the median of the 5, then of the runs (3 of the JIT and the engine, 2 of the others). `scbench.cpp` times the engine alone, and simdcsv's `find_indexes` on the same texts. MB are 10^6 bytes. `summarize.py` makes the tables and checks the gates.

The texts: `mixed` (10 columns: an integer, a day, words, some of them quoted, Floats of 2 and 5 decimals, some empty fields; 79 bytes a record), `ints` (8 columns of 1 to 10 digits, 6.4 bytes a field with its separator), `quoted` (4 columns, all quoted), `utf8` (Latin-1 and CJK words), `floats17` (4 Floats of 16 or 17 digits), `inches` (a quote inside an unquoted field in 1 record of 10), `gtfs` (`stop_times.txt`: 6 columns, 54 bytes a record, two times made by converters).
NeoCSV reads a String, or the stream of `file readStream`; SimdCSV reads the bytes of the same text, or the file.

### Gates

| gate | result | measured |
|---|---|---|
| rows >= 8x NeoCSV, 1 MB | met | mixed 10.5x, ints 9.2x, quoted 11.5x, utf8 11.9x |
| the same, 10 MB | **missed** | mixed 8.3x, **ints 4.8x**, quoted 9.4x, utf8 10.8x |
| typed rows of mixed >= 12x | met | 23.1x (1 MB), 19.4x (10 MB) |
| `do:` on a file >= 10x | met | 1 MB: mixed 13.4x, ints 10.6x, quoted 14.3x, utf8 13.8x; 10 MB: 13.5x, 10.6x, 14.1x, 14.2x |
| gtfs, cached converters, >= 6x, 10 MB | met | 9.0x from a FileReference and from `fr readStream` |
| the same, 1 MB | **missed** | 5.2x and 5.3x |
| `count:` >= 1 GB/s | met | 2.9 to 3.9 GB/s (1 MB), 3.0 to 3.4 (10 MB) |
| index >= 0.6x simdcsv's `find_indexes`, regular texts | **missed** | mixed 0.58x, ints 0.57x, quoted 0.52x, utf8 0.50x, floats17 0.56x, gtfs 0.56x |
| rows of inches >= 3x | met | 7.8x (1 MB), 4.3x (10 MB) |
| no full GC in a `do:` over 100 MB, < 5 MB retained | met | 0 full GCs, 0 bytes, in 3 runs |
| WebAssembly (node), rows >= 8x, 1 MB | met | mixed 35.5x, ints 32.5x, quoted 37.3x, utf8 42.0x |
| equal results | met | 879 of 879 checks |

Reported without a gate: gtfs with converters that are not cached is 2.0x to 2.2x NeoCSV (the converter alone, `Time` from the split text, costs 1.9 microseconds, twice a record), and the typed `floats17` 5.0x: the plugin leaves its 17-digit Floats to the image, which converts them exactly.

The missed gates, profiled:

- **index, 0.50x to 0.58x of simdcsv.** The index also checks every quote, finds CR, counts the records and their widths; and simdcsv prefetches. In builds of the engine with one thing changed each (64 MB, median of 9, over simdcsv timed in the same run; these builds are not in the tree; another core than the gates'):

  | engine | mixed | ints | quoted | utf8 | floats17 | gtfs |
  |---|---|---|---|---|---|---|
  | as it is | 0.59 | 0.59 | 0.56 | (0.40) | 0.55 | (0.30) |
  | with simdcsv's prefetch (`__builtin_prefetch(u + p + 64 * i + 128)`) | 0.67 | 0.68 | 0.61 | 0.62 | 0.64 | 0.63 |
  | without the widths of the records | 0.61 | 0.66 | 0.60 | 0.58 | 0.55 | 0.60 |
  | without the check of the quotes | 0.76 | 0.77 | 0.73 | 0.66 | 0.78 | 0.73 |

  (The two numbers in parentheses were disturbed: the same engine gave 0.50 and 0.56 in the runs of the gates.) The check of the quotes is what the engine is for; one line of prefetch would meet the gate on every text (0.61x to 0.68x). The gate is the plugin's against a program that does not do its work: in the reader, the index of `mixed` takes 0.37 of the 8.6 milliseconds of `rowsFrom:`.
- **rows of ints at 10 MB, 4.8x.** 227 ms, of which a full garbage collection takes 96: the 1.6 million Strings that are kept make old space grow by more than its ratio (vm parameter 55, 0.33), and the VM collects. NeoCSV has one too (122 ms of 1,108), which weighs less in it. With the parameter at 8.0 there is none, and `ints` is at 132 ms against 977, 7.4x: the fields are short, so that a String is made for each 6.4 bytes of text, and the scavenges that tenure the kept Strings take 34 ms. `rowsFrom:do:` of the same text, which keeps nothing, takes 102 to 106 ms and makes no collection; on a file it is 10.6x NeoCSV.
- **gtfs cached at 1 MB, 5.2x.** 38 ms: 11 for the records as Arrays of Strings, 5 more for the objects and their setters, and 23 in the two cached converter columns, of which 14 are the first 1,380 different times of each (5 microseconds each: the converter, the copy into the Dictionary, its growth), and 9 the 18,442 lookups of each (0.26 microseconds). At 10 MB the first evaluations are 1 in 130 of the texts, and the gate is met. NeoCSV itself goes from 5.1 to 4.3 MB/s there, with full garbage collections.

### The JIT (CoInterpreter), 10 MB

| series | NeoCSV | SimdCSV | speedup |
|---|---|---|---|
| `rowsFrom:` mixed | 11.4 MB/s | 94.5 MB/s (106 ms) | 8.3x |
| `rowsFrom:` ints | 9.2 | 44.0 (227 ms) | 4.8x |
| `rowsFrom:` quoted | 14.6 | 137.6 (72.7 ms) | 9.4x |
| `rowsFrom:` utf8 | 10.7 | 115.0 (87.0 ms) | 10.8x |
| `rowsFrom:` floats17 | 13.4 | 205.8 (48.6 ms) | 15.4x |
| `rowsFrom:` inches | 9.5 | 41.0 (244 ms) | 4.3x |
| `rowsFrom:` gtfs | 9.2 | 53.4 (187 ms) | 5.8x |
| `rowsFrom:` mixed, from the String | 11.4 | 102.3 (97.7 ms) | 9.0x |
| `rowsFrom:do:` mixed | 12.6 | 132.0 (75.8 ms) | 10.5x |
| 3 columns of 10 by name, mixed | 10.8 | 262.9 (38.0 ms) | 24.3x |
| typed rows, mixed | 6.5 | 127.2 (78.6 ms) | 19.4x |
| typed rows, ints | 5.3 | 142.9 (70.0 ms) | 27.1x |
| typed rows, floats17 | 4.4 | 22.3 (448 ms) | 5.1x |
| 3 Float columns, mixed (table, `columns:as:`) | 7.3 | 203.5 (49.1 ms) | 27.9x |
| `rowsFrom:do:` on a file, mixed | 9.4 | 127.8 (78.2 ms) | 13.5x |
| `rowsFrom:do:` on a file, ints | 8.9 | 94.4 (106 ms) | 10.6x |
| gtfs objects, plain, FileReference | 4.3 | 9.2 (1.08 s) | 2.2x |
| gtfs objects, cached, FileReference | 4.3 | 38.7 (259 ms) | 9.0x |
| gtfs objects, cached, `fr readStream` | 4.3 | 38.8 (258 ms) | 9.0x |
| `count:` mixed | 11.9 | 3,111 (3.2 ms) | 261x |
| `count:` inches | 12.4 | 284 (35 ms) | 23x |
| the table of mixed made | - | 1,524 (6.6 ms) | - |
| `float64ArrayAt:`, one column, mixed | - | 383.8 (26.1 ms) | - |
| `stringFrom:`, all quoted, mixed | 31.2 | 17.6 (568 ms) | 0.56x |
| `stringFrom:`, quoted where needed, mixed | 9.1 | 14.7 (680 ms) | 1.6x |

The writer is Smalltalk: at 1 MB it is as fast as NeoCSV's when it quotes every field (30.4 and 30.7 MB/s) and 1.8 times as fast when it quotes where needed; at 10 MB it makes two full garbage collections a call. This is the benchmark that asks for a primitive that writes.
The whole of `lines` and `split:` of the String, for texts without quotes: 30.1 MB/s (ints, 1 MB), 15.2 (10 MB), 16.4 (utf8, 1 MB), 81.0 (floats17, 1 MB).
100 MB of `mixed` in a file, `rowsFrom:do:` (1.27 million records): 776 to 852 ms (117 to 129 MB/s), 21 scavenges, no full garbage collection, old space grows by 16 MiB (a segment), 0 bytes are retained; NeoCSV 10.8 to 12.6 s.

Transposition. `columns:as:` of 3 Float columns at 10 MB takes 49.1 ms on the JIT: the table 6.6, the rows of the builder 29.4, the transposition in the image 13.1 (27%; 29% at 1 MB). On the StackVM the transposition is 53% (10 MB; 55% at 1 MB), and in WebAssembly 52% (1 MB). Over 50% of the load is the case for a primitive that makes columns (`primitiveColumns`) in a second version: it is there on the StackVM and in WebAssembly, and not on the JIT.

### StackVM and WebAssembly

Speedup over NeoCSV on the same VM, with the plugin.

| series | JIT 1 MB | JIT 10 MB | StackVM 1 MB | StackVM 10 MB | WebAssembly 1 MB |
|---|---|---|---|---|---|
| `rowsFrom:` mixed | 10.5x | 8.3x | 32.6x | 25.9x | 35.5x |
| `rowsFrom:` ints | 9.2x | 4.8x | 29.6x | 13.5x | 32.5x |
| `rowsFrom:` quoted | 11.5x | 9.4x | 37.6x | 28.3x | 37.3x |
| `rowsFrom:` utf8 | 11.9x | 10.8x | 37.5x | 31.8x | 42.0x |
| typed rows, mixed | 23.1x | 19.4x | 83.2x | 77.7x | 81.9x |
| `rowsFrom:do:` on a file, mixed | 13.4x | 13.5x | 46.6x | 45.8x | 41.8x |
| 3 columns of 10, mixed | 22.8x | 24.3x | 71.1x | 82.4x | 72.8x |
| gtfs objects, cached, FileReference | 5.2x | 9.0x | 10.1x | 21.0x | 9.7x |
| `count:` mixed | 295x | 261x | 938x | 781x | 510x |

In WebAssembly (node, 1 MB) `rowsFrom:` reads `mixed` at 65.6 MB/s (NeoCSV: 1.8), `count:` at 963 MB/s, the index alone 1,085 MB/s.
Without the plugin, `SimdCSVSmalltalk` reads `mixed` at 15.9 MB/s on the JIT (NeoCSV: 11.0), 0.73x (`quoted`, WebAssembly) to 1.4x NeoCSV over the texts and the VMs, and 2.2x for typed rows of mixed (JIT).

### The engine

GB/s of 10^9 bytes, at least 64 MB of text, median of 9, then of 3 runs. `index`: one call; `count`: `sc_measure` with the line count; `batches`: the reader's 1 MiB calls with the line count.

| text | avx2 | sse2 | fallback | simdcsv (AVX2) | avx2 / simdcsv | avx2, batches | avx2, count |
|---|---|---|---|---|---|---|---|
| mixed | 2.72 | 2.17 | 1.17 | 4.71 | 0.58 | 2.38 | 2.56 |
| ints | 2.31 | 1.86 | 1.08 | 4.08 | 0.57 | 2.06 | 2.51 |
| quoted | 3.20 | 2.79 | 1.34 | 6.12 | 0.52 | 2.80 | 2.62 |
| utf8 | 3.02 | 2.42 | 1.23 | 6.03 | 0.50 | 2.59 | 2.62 |
| floats17 | 3.25 | 3.11 | 1.40 | 5.80 | 0.56 | 2.58 | 2.48 |
| gtfs | 3.14 | 2.57 | 1.29 | 5.59 | 0.56 | 2.61 | 2.57 |
| inches | 0.24 | 0.24 | 0.25 | (another index) | - | 0.24 | 0.23 |

On `inches` the automaton does nearly all of the work, as a quote inside a field in 1 record of 10 does not leave the kernel 1 KiB of clean text: 0.24 GB/s, which is still 23 times NeoCSV's `do:` for `count:`. simdcsv's index of it is wrong, not only different.

The spread between the runs of a series: 4.2% in the median on the JIT (the largest 32%, rows of gtfs by NeoCSV at 10 MB), 2.6% on the StackVM (37%), 2.0% in WebAssembly (37%).

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
