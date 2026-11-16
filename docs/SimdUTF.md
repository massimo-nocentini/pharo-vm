# SimdUTF: transcoding and base64 on simdutf

The SimdUTFPlugin transcodes between UTF-8, UTF-16LE, UTF-16BE, Latin-1 and UTF-32, validates, and encodes and decodes base64 with [simdutf](https://github.com/simdutf/simdutf) 9.4.0, and the package `SimdUTF` (the submodule `plugins/SimdUTFPlugin/simdutf.st`) gives Pharo 12 to 15 an explicit API on top of it: the class side of `SimdUTF`, and `SimdUTFBase64Encoder`, which takes the configuration of Zinc's `ZnBase64Encoder`.
Nothing of Zinc is replaced: `utf8Encoded`, `base64Decoded` and the encoders of Zinc are untouched, and the package adds no method to a `Zn*` class.
On a VM without the plugin the package `SimdUTF-FFI` calls `libSimdUTFFFI`, the same engine as a library of its own, and without both the package answers the same results in pure Smalltalk.

Files, in `plugins/SimdUTFPlugin`:

- `src/common/SimdUTFPlugin.c`: the module functions and the seven primitives (C, with the VM's headers); the only file of `src/common`;
- `src/cxx/suEngine.cpp`: the engine, the only file that includes `simdutf.h` (C++17, without the VM's headers);
- `include/common/suEngine.h`: the interface between the two, and between `libSimdUTFFFI` and the engine;
- `src/ffi/suFFI.c`, `include/ffi/suFFI.h`: `libSimdUTFFFI`, the engine for the FFI (C, without the VM's headers);
- `simdutf.st/`: the Smalltalk side, a git submodule of [simdutf.st](https://github.com/massimo-nocentini/simdutf.st) (`git submodule update --init plugins/SimdUTFPlugin/simdutf.st`): Tonel packages in `src/` with `BaselineOfSimdUTF`, whose commit is the version of the Smalltalk code that goes with the plugin;
- the packages `SimdUTF-Tests` and `SimdUTF-FFI-Tests` of the submodule, and `tests/`: the tests;
- `bench/`: the benchmark.

## Building

The plugin is on by default on every 64-bit target: `FEATURE_PLUGIN_SIMDUTF=ON`.
It is off on 32-bit targets (Linux armv7l, `cmake/vmmaker.cmake`), where `-DFEATURE_PLUGIN_SIMDUTF=ON` stops the configuration: the status word that the primitives answer needs the SmallIntegers of a 64-bit VM (see Answers: the status word).
`-DFEATURE_PLUGIN_SIMDUTF=OFF` leaves it out, along with the download of simdutf.

    cmake -S . -B build -DFLAVOUR=CoInterpreter            # the plugin is in
    cmake -S . -B build -DFEATURE_PLUGIN_SIMDUTF=OFF       # it is not

simdutf is taken from the archive of its tag, `https://github.com/simdutf/simdutf/archive/refs/tags/v9.4.0.tar.gz` (3.5 MB), checked against its SHA256 (`36155505…a5786a`), by `cmake/simdutf.cmake` with the function of the WebAssembly libraries (`cmake/emscripten/deps/fetch.cmake`), natively too, as simdjson is:

- the archive comes from `WASM_DEPS_DIR` when that directory holds `simdutf-9.4.0.tar.gz`, which builds offline, else from `<build>/downloads`, else from GitHub at configure time;
- `-DSIMDUTF_SOURCE_DIR=<tree>` takes a simdutf 9.4.0 source tree instead (its `simdutf_version.h` is checked).

The release's `singleheader.zip` is not used: it has no licence file, which the fetch function requires.
simdutf's own CMake files are not used either: its unity file `src/simdutf.cpp` is compiled, with `src/` and `include/` on the include path, as the static library `pharo_simdutf`, in `<build>/deps` (never in `build/vm`, which is packaged whole), with `-O3 -g0 -fno-exceptions`, C++17 without extensions, hidden visibility, and position-independent code natively; the engine `suEngine.cpp` is compiled the same way into each library that uses it.

- **Features.** simdutf 9.4.0 has seven, each on unless defined to 0; six are on (`SIMDUTF_FEATURE_UTF8`, `UTF16`, `UTF32`, `ASCII`, `LATIN1`, `BASE64`) and `SIMDUTF_FEATURE_DETECT_ENCODING` is off, since nothing uses it: about 55 KB less code natively, and no C API (`src/simdutf_c.cpp` needs all seven), which the library does not use (see Library contract).
  The seven definitions are PUBLIC on `pharo_simdutf`, so the plugin and the library see them too: they change the virtual table of `simdutf::implementation`.
- **No exceptions.** simdutf throws only in its C++23 API; `-fno-exceptions` and the engine's rules (no `new`, no `throw`, no `std::string`) keep the C++ runtime out of the way.
- **No libstdc++ version.** simdutf includes no `<iostream>` and reads no macro that `_MT` sets, so neither of SimdJSON's work-arounds (`-D_GLIBCXX_IOSTREAM`, `sjNoThreads.h`) is needed: `objdump -T build/vm/libSimdUTFPlugin.so | grep -c GLIBCXX` is 0, for the library too; the Linux job of `.github/workflows/build.yml` checks that neither needs a libstdc++ newer than `GLIBCXX_3.4.21` (none at all passes), as it checks SimdJSON's.
- **Kernels.** simdutf chooses its kernel at run time on x86_64 (`icelake` with AVX-512 VBMI2, `haswell` with AVX2, `westmere` with SSE4.2, else `fallback`); Linux and Windows aarch64 and macOS arm64 have the one kernel `arm64`; the WebAssembly VMs have the one kernel `fallback` (simdutf 9.4.0 has none for WebAssembly).
  `SimdUTF implementationName` answers the kernel, and the variable `SIMDUTF_FORCE_IMPLEMENTATION` chooses another one, under the policy of the guard (see The `SIMDUTF_FORCE_IMPLEMENTATION` guard): a kernel the CPU lacks is refused, never run.

Each shared library keeps a private copy of simdutf (about 630 KB of code on x86_64: `libSimdUTFPlugin.so` and `libSimdUTFFFI.so` are each 650 KB), with hidden visibility, so that the VM's `dlopen` with `RTLD_GLOBAL|RTLD_DEEPBIND` (`src/externalPrimitives.c`) never mixes them up, and both can live in one process.

Platforms:

- Linux x86_64 and Emscripten (the node and web VMs of `make wasm`) are built and tested;
- macOS (x86_64 and arm64), Windows (MSYS2 UCRT64 with clang on x86_64, CLANGARM64 on arm64) and Linux aarch64 are written for, as SimdJSON's are, but **untested**: no build of them has run here.
  simdutf itself has never run upstream on Windows arm64 with MinGW clang, nor on macOS x86_64.
  Each native job of `.github/workflows/build.yml` loads the plugin of its zip with `plugins/SimdUTFPlugin/tests/ci-load.st`, three times (`SIMDUTF_FORCE_IMPLEMENTATION` unset, `bogus`, which must be refused, and `fallback`, which must be honoured on x86_64 and refused on arm64), and checks that the licence files are in the zip; the Linux job also calls `libSimdUTFFFI` and requires its answer in each of the three runs (`refused bogus`, and `fallback` honoured on x86_64, refused on aarch64), so the first CI run tells.
  The Linux and macOS steps run under `shell: bash`, whose `pipefail` makes a VM that dies after printing fail the step.
  Build with `-DFEATURE_PLUGIN_SIMDUTF=OFF` if the build breaks there.

`FEATURE_SIMDUTF_FFI` (natively, `ON` by default with the plugin) also builds `libSimdUTFFFI` (`libSimdUTFFFI.so`, `.dylib`, or `SimdUTFFFI.dll` on Windows) from `src/ffi/suFFI.c`, `suEngine.cpp` and `pharo_simdutf`, next to the plugins, so that it is in the zips.
It has hidden visibility and exports only its eleven `su_ffi_*` functions.
It is not a plugin and needs nothing of the VM: copied next to any 64-bit Pharo VM, or named with `SimdUTFLibrary libraryPath:`, it serves that VM's image.

In the WebAssembly VMs the plugin is built in, and the library is not built (there is no `dlopen`).
The C++ link driver and libc++ and libc++abi, which SimdJSON's plugin brings in already, serve both plugins; their licences, and simdutf's, go into `THIRD-PARTY-NOTICES.txt` (see docs/WebAssembly.md).
The SimdUTFPlugin adds 97.7 KB to `pharo-web.wasm` (28.4 KB gzipped), and 107 KB to `pharo.wasm` (29.4 KB gzipped), measured on one build directory with and without it.
The native tree that `make wasm` generates the sources in is configured with `-DFEATURE_PLUGIN_SIMDUTF=OFF`: it builds nothing but the sources.

## Loading the Smalltalk side

The Smalltalk code is in the submodule `simdutf.st`, as Tonel packages that Metacello loads with `BaselineOfSimdUTF`, in Pharo 12, 13, 14 and 15.
Its groups: `Core` (the package `SimdUTF`: the facade, the plugin's primitives and the Smalltalk backend; it needs nothing but Zinc, which every image has), `Tests` (`SimdUTF-Tests`), `FFI` (`SimdUTF-FFI`, the library through the FFI), `FFITests` (`SimdUTF-FFI-Tests`) and `default` (all of them).

    Metacello new
        baseline: 'SimdUTF';
        repository: 'tonel://<repository>/plugins/SimdUTFPlugin/simdutf.st/src';
        load: #( 'Core' 'FFI' ).

or, from the command line, `tests/load-simdutf.st`, which the tests, the benchmark and lane 69 use:

    SU_PLUGIN_DIR=plugins/SimdUTFPlugin SU_GROUPS='Core Tests' \
    pharo --headless Pharo.image --no-default-preferences st --save --quit plugins/SimdUTFPlugin/tests/load-simdutf.st

It loads the groups of `SU_GROUPS` (`Core Tests` by default) and prints `SimdUTF: loaded, protocol 1, backend #<plugin, ffi or smalltalk>, implementation <kernel>`, then the groups and packages loaded.
It exits with 1, before the image is saved, when the submodule is not checked out, a load fails, `SimdUTF protocol` is not 1, the Smalltalk backend does not encode `'hé'`, a method of a SimdUTF package uses an undeclared variable, a SimdUTF package defines a method on a class whose name begins with `Zn`, or an extension method (on a class of no SimdUTF package) has a selector that does not begin with `simd`: the package is opt-in, and changes nothing of Zinc's behaviour.
Metacello makes UUIDs: the VM needs its UUIDPlugin.
To change the Smalltalk code, commit in the submodule, push it to simdutf.st, and commit the new submodule commit here, with the plugin changes it goes with.

## API

Everything is on the class side of `SimdUTF`, and explicit: nothing changes what Zinc's `utf8Encoded`, `utf8Decoded`, `base64Encoded` or encoders do.

    SimdUTF utf8Encode: 'aé€'.                    "#[97 195 169 226 130 172]"
    SimdUTF utf8Decode: #[97 195 169].            "'aé', the narrowest String"
    SimdUTF utf16LEEncode: aString.
    SimdUTF isValidUTF8: bytes.
    SimdUTF base64Encode: bytes.                  "as ByteArray>>base64Encoded"
    SimdUTF base64Decode: 'AQID'.                 "strict; ASCII white space skipped anywhere"
    SimdUTF base64Encoder breakLines; encode: bytes.   "configured as ZnBase64Encoder"

Ranges are 1-based and inclusive; a `from:to:` or an `at:` out of range signals `SubscriptOutOfBounds`.
Each `into:at:` form writes into an existing object and answers the units written; when the room from `index` on is too small, it signals `SimdUTFOutputTooSmall`, whose `needed` is the units it needs, and writes nothing.

UTF-8:

| selector | answer |
|---|---|
| `utf8Encode:`, `utf8Encode:from:to:` | a ByteArray |
| `utf8Encode:from:to:into:at:` | bytes written |
| `utf8SizeOf:`, `utf8SizeOf:from:to:` | the exact bytes of the UTF-8, without encoding (U+FFFF is 3) |
| `utf8Decode:`, `utf8Decode:from:to:` | the narrowest String: a ByteString when every character is below 256, else a WideString; every U+FEFF is kept |
| `utf8DecodeSkippingBOM:` | the same without one leading U+FEFF |
| `utf8DecodeWide:` | always a WideString |
| `utf8DecodeLatin1:` | always a ByteString; a character above U+00FF signals `SimdUTFInvalidCharacter` (code 1002) |
| `utf8Decode:from:to:into:at:` | characters written into a WideString, or into a ByteString (Latin-1 only) |
| `utf8CharacterCountOf:`, `utf8CharacterCountOf:from:to:` | the characters of the UTF-8 |

UTF-16, with no default endianness, and no byte order mark unless one is asked for: `utf16LEEncode:`, `utf16BEEncode:` (ByteArrays), `utf16SizeOf:` (bytes), `utf16LEDecode:`, `utf16BEDecode:` (the narrowest String; a leading U+FEFF is a character).

The general forms, which the conveniences call:

| selector | |
|---|---|
| `encode:from:to:encoding:bom:` (+ `into:at:`) | `#utf8`, `#utf16le` or `#utf16be`, with a byte order mark first when `bom` is true |
| `decode:from:to:encoding:result:skipBOM:` | result `#narrowest`, `#wide` or `#latin1` |
| `decode:from:to:encoding:skipBOM:into:at:` | into a ByteString (Latin-1) or a WideString |
| `sizeOf:from:to:encoding:bom:` | bytes |

A UTF-16 range that starts at an odd offset of the ByteArray is copied first, and the positions of its errors are those of the ByteArray; a UTF-16 result into a destination at an odd offset (an even `index` within `1` to its size + 1) signals `SimdUTFArgumentError`; an `index` out of that range signals `SubscriptOutOfBounds`, whatever its parity.

Validation: `isValidUTF8:` (and `from:to:`), `validateUTF8:` (answers the bytes, or signals `SimdUTFInvalidUTF8`), `isValidUTF16LE:`, `isValidUTF16BE:` (an odd size is invalid), `isValidUTF32:` (a ByteString is; a WideString when every character is a Unicode scalar value), `isAscii:` and `firstNonAsciiIndexIn:` (0, or the absolute index; on a String or a ByteArray, and their `from:to:` forms).

Base64:

| selector | answer | Zinc's |
|---|---|---|
| `base64Encode:` | a ByteString, standard alphabet, `=` padding, no lines | `ByteArray>>base64Encoded`, `ZnBase64Encoder new encode:` |
| `base64URLEncode:` | `-_`, no padding | `ZnBase64Encoder new beForURLEncoding; encode:` |
| `base64Decode:` | a ByteArray; standard alphabet, strict | `String>>base64Decoded` |
| `base64URLDecode:` | URL alphabet, lenient | `ZnBase64Encoder new beForURLEncoding; decode:` |
| `base64Encode:from:to:options:lineLength:lineEnd:` (+ `into:at:`) | the general form: the options and line ends of Constants | |
| `base64Decode:from:to:options:lastChunk:` (+ `into:at:`) | | |
| `base64SizeOf:options:lineLength:lineEnd:` | the characters of the base64 of n bytes | `characterCountFor:` |
| `base64Encoder` | a new `SimdUTFBase64Encoder` | `ZnBase64Encoder new` |

ASCII white space (space, tab, LF, FF, CR) is skipped anywhere in base64 input, and every other character outside the alphabet is an error.
A WideString to decode is narrowed to a ByteString first; one with a character above 127 signals `SimdUTFInvalidBase64` (code 7) at that character, where Zinc answers `#[]`.
Its arguments are checked before, in the order of the primitive (the numbers, the options and the mode, the range, the destination): a bad one signals what it signals with a ByteString source.

`SimdUTFBase64Encoder` takes ZnBase64Encoder's configuration (`breakLines`, `breakLinesAt:` with a multiple of 4, which sets CRLF unless a line end was set, as Zinc's, `lineEndConvention:` `#cr`, `#lf` or `#crlf`, `padding:` `$=` or nil, `noPadding`, `standardAlphabetWith:and:` `$+ $/` or `$- $_`, `beForURLEncoding`, `beStrict`, `beLenient`), plus `beForAnyAlphabet` (decoding takes both alphabets; as in Zinc, the alphabet set last wins: `standardAlphabetWith:and:` or `beForURLEncoding` after it decode their own alphabet only) and `encodedSizeOf:`, and converts with `encode:`, `decode:`, `encode:to:`, `decode:to:` (streams, read to their end), and the `from:to:` and `into:at:` forms.
Its defaults are those of `ZnBase64Encoder new`: standard alphabet, `=` padding, strict, no lines.
`whitespace: #separator` is what it always does (ASCII white space is skipped); `whitespace:` with `nil` or `#any`, and `alphabet:`, signal `SimdUTFArgumentError`: it is not a replacement for code that needs those, nor for decoding a stream chunk by chunk.

Errors: invalid input signals `SimdUTFInvalidUTF8` (codes 1–6 in UTF-8 input), `SimdUTFInvalidUTF16` (code 6, and 1001 for an odd size), `SimdUTFInvalidCharacter` (codes 5 and 6 of a String to encode, and 1002) or `SimdUTFInvalidBase64` (codes 7–9), with simdutf's error `code`, its `codeName` and the absolute `position` of the offending unit (a byte, or a character of a String), in the message too: `invalid UTF-8 (OVERLONG) at byte 1`.
They are subclasses of `SimdUTFEncodingError`, a subclass of Zinc's `ZnCharacterEncodingError`, so a handler of Zinc's errors handles them, and they are not resumable (Zinc's `ZnInvalidUTF8` is).
A receiver or an argument of a kind the API does not take (nil, an Array, a ByteArray to encode, a CompiledMethod; a destination of another kind than a ByteArray or a String, such as a WordArray), a bad option, a destination that is the source, and a read-only destination signal `SimdUTFArgumentError`, whatever the backend; memory that cannot be had signals `OutOfMemory`.

The constants of Constants are class-side selectors too: `encodingUTF8`, `encodingLatin1`, `encodingUTF32`, `encodingUTF16LE`, `encodingUTF16BE`, `flagBOM`, `flagWide`, `flagLatin1`, `opEncoded`, `opDecoded`, `opBase64`, `opValid`, `opAscii`, `base64Standard`, `base64URL`, `base64StandardNoPadding`, `base64URLPadded`, `base64Any`, `lastChunkLoose`, `lastChunkStrict`, `lineEndLF`, `lineEndCR`, `lineEndCRLF`, and `protocol` (1).

Conveniences, one-line calls of the facade (every extension selector of the package begins with `simd`): `String>>simdUTF8Encoded`, `simdUTF8Size`, `simdUTF16LEEncoded`, `simdUTF16BEEncoded`, `simdIsAscii`, `simdBase64Decoded`, `simdBase64URLDecoded`; `ByteArray>>simdUTF8Decoded`, `simdUTF16LEDecoded`, `simdUTF16BEDecoded`, `simdIsValidUTF8`, `simdIsAscii`, `simdUTF8CharacterCount`, `simdBase64Encoded`, `simdBase64URLEncoded`.

## Backends

Three backends answer the same protocol, the one of the primitives (see The primitives): each answers what the primitive answers, a new object, a SmallInteger, a status word, or a `SimdUTFFailure` with the failure code the primitive would have, in the same order of checks, for every source and destination of the kinds that `simdUTFUnits` names: ByteArray, ByteString, ByteSymbol, WideString, WideSymbol and their subclasses (CompiledCode excepted), and nil destinations.
The primitive alone also takes any other byte or word object (a WordArray or a LargePositiveInteger source, a WordArray destination; see Kinds); `SimdUTFSmalltalk` and `SimdUTFLibrary` answer `#'bad argument'` for those, at the check (2) or (7) where the primitive goes on.
The facade passes only the shared kinds (it refuses the others with `SimdUTFArgumentError`), the differential tests use only them, and `SimdUTFPrimitiveTests` and `SimdUTFLibraryTests` pin the difference.

- `SimdUTFPrimitives`: the class side of the seven primitive methods, the plugin itself;
- `SimdUTFLibrary` (package `SimdUTF-FFI`): `libSimdUTFFFI` through the FFI (see Library contract);
- `SimdUTFSmalltalk`: pure Smalltalk, a port of simdutf's scalar code (the UTF-8, UTF-16 and UTF-32 validation with errors and counts, and the forgiving base64 with its loose and strict modes), not Zinc, which drops U+FEFF, reads overlong sequences and gives no positions.
  It is the oracle of the differential tests, and the backend of a VM with neither the plugin nor the library.
  It finds runs of ASCII with `ByteString class>>findFirstInString:inSet:startingAt:`, a primitive with no stop index: on a range followed by more bytes than it holds (and 256), it scans a short range in Smalltalk and a long one in windows copied out of the source (256 bytes, doubling up to 64 KiB), so that a range costs its own size, not the rest of the buffer.
  It copies them with `simdUTFReplaceFrom:to:with:startingAt:` of ByteArray and String (primitive 105, else unit by unit), which writes the destination's own units whatever its class overrides: `ExternalAddress>>replaceFrom:to:with:startingAt:` writes through the address it holds.

The facade chooses once a session (`Smalltalk session`), and again after `SimdUTF resetBackend`:

1. the plugin, when `primitiveInfo` answers protocol 1 and a kernel other than `unsupported`: it serves every call, whatever its size, and the library is never looked up (so never on the web VMs);
2. else, for a source of at least `SimdUTFLibrary minimumUnits` units (characters of a String, bytes of a ByteArray), the library, when `SimdUTF-FFI` is loaded and finds a library that speaks its protocol; it is looked up the first time a source is that large;
3. else `SimdUTFSmalltalk`.

`SimdUTF backend` answers what a large source goes through (`#plugin`, `#ffi` or `#smalltalk`), `pluginAvailable` and `pluginInfo` what `primitiveInfo` answered, `implementationName` the kernel of the plugin, else of the library, else `'smalltalk'`, `simdutfVersion` `'9.4.0'` (or nil in pure Smalltalk) and `forcedImplementation` the slot 7 of `primitiveInfo` (or `su_ffi_forced`; a refused value is cut to its first 55 bytes, see `primitiveInfo`).
A primitive that answers `#'not found'` (a VM without the plugin, after an image was saved on one with it) makes the facade choose again and call once more.

The test hooks, `SimdUTF testHook: aSymbol value: anObject` (nil removes one; each forgets the choice): `#backend`, an object that answers the protocol, used for every call (the parametrized tests and the benchmark set it to each backend in turn); `#pluginMissing` (true), as if the VM had no plugin; `#fallbackMissing` (true), as if `SimdUTF-FFI` were not loaded.
The plugin is never hidden by deleting it: a VM of a build tree finds it through its absolute RUNPATH.

## Semantics against Zinc

Where the explicit API deliberately differs from Zinc (the same in Pharo 12 and Pharo 15 unless stated):

| Input | Zinc (Pharo 12 = Pharo 15) | SimdUTF | Why |
|---|---|---|---|
| U+FEFF inside UTF-8 (`41 EF BB BF 42`) | `'AB'` | `'A'`, U+FEFF, `'B'`; `utf8DecodeSkippingBOM:` drops only a leading one | lossless; U+FEFF mid-text is ZWNBSP |
| `41 EF BB BF` | ZnIncomplete | `'A'`, U+FEFF | same |
| overlong `F0 8F BF BF` | U+FFFF | `SimdUTFInvalidUTF8` OVERLONG at 1 | RFC 3629 |
| truncated `E2 82` | ZnIncomplete (resumable) | `SimdUTFInvalidUTF8` TOO_SHORT at 1 | one class, a position, not resumable |
| UTF-8 size of U+FFFF | 4 | 3 | correct |
| WideString value > U+10FFFF or surrogate, encoded | ZnInvalidUTF8, no position | `SimdUTFInvalidCharacter` at the character | position |
| UTF-16 lone surrogate `D8 00 00 41` | garbage `#(9281)` | `SimdUTFInvalidUTF16` SURROGATE | validation |
| UTF-16 endianness | ZnUTF16Encoder defaults to BE | explicit LE/BE selectors only | explicit API |
| U+FEFF anywhere in UTF-16 input | dropped wherever it occurs (`ZnUTF16Encoder>>processByteOrderMark:` runs on every code unit read) | kept as a character; `SU_F_BOM` skips only one leading one | lossless |
| U+FFFE anywhere in UTF-16 input | taken as a byte-swapped BOM: dropped, and the decoder **swaps endianness** for the rest of the text | the noncharacter U+FFFE, kept; endianness never changes | explicit endianness; the encoder is stateful in Zinc, never in SimdUTF |
| base64 `QQ==QQ==` | `#[65 65]` | `SimdUTFInvalidBase64` code 7 | RFC 4648 |
| base64 non-alphabet characters between quads, including non-ASCII ones (so Zinc decodes a WideString of such characters to `#[]`) | skipped (`whitespace: #any`, Zinc's default, skips every character that is not in the alphabet) | error code 7 at the first such character (a WideString source with a character > 127: code 7 at it; else narrowed to a ByteString first) | garbage acceptance is not offered; no silent loss |
| base64 white space inside a quad (`QU JD`) | error | accepted | simdutf's forgiving base64; RFC 4648 §3.3 |
| base64 `QR==` | `#[65]` | strict (default): code 9; lenient: `#[65]` | strict is canonical |
| base64 URL decoding of white space | refused (`whitespace: nil`) | skipped | simdutf always skips ASCII white space |
| line breaking | after each group completing a line, last one included | **identical** | compatibility, tested |
| result class of a decode | narrowest | narrowest | same |
| Lossy / Debugging encoders (Pharo 15) | U+FFFD | not offered | no shadowing; Zinc unaffected |

Everything else, on valid input, is byte for byte Zinc's, which the equivalence tests check.

## Memory and limits

- A primitive (or library) call allocates at most one object, its result, at its exact size, and only once the input is known to be valid (base64 decoding excepted: its result is allocated before the decode, which is the validation, so invalid base64 leaves a garbage object).
  The facade adds a copy of the source in two cases: a UTF-16 range that starts at an odd byte offset (decoded or measured from a copy; a measure has no result object) and a WideString to decode from base64 (narrowed to a ByteString of the range); so an allocating call makes at most two objects.
  `SimdUTFLibrary` also copies a source that is not exactly a ByteArray, ByteString or WideString (a Symbol, an instance of a subclass: `simdUTFPassableCopy`), and `SimdUTFSmalltalk`'s base64 decode uses an Array of 4 slots.
  The C heap is never used: no `malloc` in the glue, the engine or the library, which matters in WebAssembly, whose memory never shrinks.
- The plugin allocates in the VM (`instantiateClassindexableSize`), in old space when eden has no room.
  When that fails, the primitive answers OVER_BUDGET with the bytes the object needs, instead of failing with `#'insufficient object memory'`, whose retry by the VM collects garbage but never grows the memory; the facade then grows it by those bytes and 1 MiB (`Smalltalk growMemoryByAtLeast:`), since an object of old space needs one free chunk of its size, which free space spread over many chunks does not give, and calls once more.
  When the memory cannot grow (`growMemoryByAtLeast:` signals `OutOfMemory` where old space has a maximum, as in WebAssembly), or when the call answers OVER_BUDGET again, the facade collects all the garbage (`Smalltalk garbageCollect`) and calls a last time; OVER_BUDGET after that signals `OutOfMemory`.
  `SimdUTFMemoryTests` and lane 69 make a result larger than the free old space; `SimdUTFFacadeTests` takes every path with a scripted backend, the memory unable to grow included (vm parameter 67, the maximum of old space, set below its size).
  `SimdUTF roomFor:` (below) does not signal when the memory cannot grow: the allocation that follows collects the garbage.
- The library cannot allocate: the image does, after a sizing call, with `basicNewPinned:`, which takes old space; for an object of 1 MiB or more `SimdUTF roomFor:` first grows the memory by twice its size and 4 MiB when the free old space is below that, as SimdJSON-FFI's does, since `new:` of an object that does not fit collects all the garbage first.
- The `into:at:` forms allocate no result, nothing of the size of the input or output but the source copies above (and, with `SimdUTFLibrary`, a temporary for a destination that is not exactly a ByteArray, ByteString or WideString, copied back): a buffer can be used again, and its size is checked before anything is written (base64 decoding excepted: after an invalid input, the bytes of the destination that the result would have taken are unspecified).
- Sizes are bounded by memory only; a status word holds positions and sizes below 2^48.
- **WebAssembly**:
  - old space is a window of 512 MiB by default (`PHARO_WASM_OLD_SPACE_BASE`, docs/WebAssembly.md), in which Spur asks for more than an object needs, so that a single object of 256 MB fails already: a source and its result of some 100 MB each are about the most that the default window holds; `PHARO_WASM_OLD_SPACE_BASE=0x40000000` doubles it (lane 69 converts 32 MiB that way);
  - the plugin's allocations collect no garbage: a loop of large conversions that allocates little else fills the window with the garbage of its results (419 conversions of 1 MiB in a row in the default window), and then each OVER_BUDGET finds the memory unable to grow, so the facade collects the garbage and calls again; lane 69 makes 1000 conversions of 1 MiB in a row (natively, the same loop grows old space instead);
  - a primitive call runs to its end, and the page gets no slice meanwhile: with simdutf's `fallback` kernel the plugin converts 250 MB/s to 2.9 GB/s in the node and web VMs (see Benchmarks), so 100 MB block the page for 0.04 to 0.4 s, well below the 3 s after which Stop replaces the worker;
  - the Smalltalk backend never runs there, since the plugin is built in.

## Primitive contract

### Module, calls, protocol

- Module `SimdUTFPlugin`; `getModuleName` answers `"SimdUTFPlugin (i)"` builtin, `"SimdUTFPlugin (e)"` external.
- `SU_PROTOCOL` = **1**, plugin version `"SimdUTFPlugin 1.0"`. The image uses the plugin only when `primInfo first = SimdUTF protocol` and the kernel is not `'unsupported'`.
- Every primitive is a class-side method of `SimdUTFPrimitives`: `<primitive: 'primitiveX' module: 'SimdUTFPlugin' error: ec> ^ SimdUTFFailure on: ec`; `primInfo` answers `nil` instead. The receiver is ignored; arguments are counted with `methodArgumentCount()`.
- `setInterpreter` checks `VM_PROXY_MAJOR`/`MINOR`. `initialiseModule` calls `su_init()` and answers 1. `shutdownModule` answers 1: the plugin keeps no state.
- Accessor depth 0 for every primitive: a forwarded argument fails the kind checks with `PrimErrBadArgument`, the VM follows the forwarders and retries.

### Answers: the status word

Every primitive except `primitiveInfo` answers:
- **OK**: the result object (a new ByteArray, ByteString or instance of the given WideString class), or a SmallInteger ≥ 0 (units written into a destination, a measure, or 0 for "valid");
- **not OK**: a negative SmallInteger, the **status word** `w = -(status + 4·code + 4096·payload)`; `status = (-w) bitAnd: 3`, `code = ((-w) bitShift: -2) bitAnd: 1023`, `payload = (-w) bitShift: -12`.

| status | name | code | payload |
|---|---|---|---|
| 1 | `SU_OVER_BUDGET` | 0 | Spur bytes the result object needs, header included (`spurBytes`/`spurWords32` below). Answered when the one result allocation failed (allocate mode only) |
| 2 | `SU_ERROR` | an error code (table below) | the **absolute 1-based index** of the offending unit in the source object (never relative to `start`) |
| 3 | `SU_TOO_LARGE` | 0 | units the destination must have free from `index` on (bytes of a byte destination, words of a word destination). Into mode only; nothing was written |

Error codes: simdutf's `error_code` 1 HEADER_BITS, 2 TOO_SHORT, 3 TOO_LONG, 4 OVERLONG, 5 TOO_LARGE (a code point > U+10FFFF, or a unit > 0x7F for an ASCII check), 6 SURROGATE, 7 INVALID_BASE64_CHARACTER, 8 BASE64_INPUT_REMAINDER, 9 BASE64_EXTRA_BITS, 11 OTHER (`include/simdutf/error.h` of simdutf; pinned by `static_assert` in `suEngine.cpp`); plus **1001 ODD_UTF16_LENGTH** and **1002 NOT_LATIN1** (a character > U+00FF where Latin-1 is required). Code 10 (OUTPUT_BUFFER_TOO_SMALL) is never answered (rooms are checked before conversion); an unexpected one is reported as 11.

Payload < 2^48 always (sizes are bounded by memory), so `w` is a SmallInteger on every 64-bit target, WebAssembly included (`-m64`, `MEMORY64=2`). **64-bit VMs only**: with 31-bit SmallIntegers `w` would wrap once payload ≥ 2^18 (even to a non-negative "units written"), and `spurBytes`/`spurWords32` are the 64-bit formulas. The repository still configures 32-bit VMs (`cmake/vmmaker.cmake`, Linux armv7l), so `cmake/plugins.cmake` defaults the option OFF there and refuses it ON, and the glue has `_Static_assert(sizeof(sqInt) == 8, …)`.

**Invalid input is an answer, not a failure** (SimdJSON's rule, see docs/SimdJSON.md): it never starts the VM's retries. **OVER_BUDGET replaces `PrimErrNoMemory`** for the result allocation, because the VM's retry scavenges, runs a full GC and gives up without growing memory; the glue clears the code with `primitiveFailFor(0)` and answers the word, exactly as SimdJSON's `primitiveWrite` does.

### Failures (`primitiveFailFor`)

| Code | `ec` in the image | When |
|---|---|---|
| `PrimErrBadNumArgs` | `#'bad number of arguments'` | wrong argument count |
| `PrimErrBadArgument` | `#'bad argument'` | a source or destination of the wrong kind, or a source kind that does not fit the operation and encoding (check (4)); a number that is not a SmallInteger; a flag, option, mode, line length or line end outside the accepted set; a `wideStringClass` that is not a class (metaclass check), whose format is not instSpec 10 with 0 fixed fields, or that is `classFloat()`; a result allocation that fails with a code other than `PrimErrNoMemory`; a destination identical to the source; `SU_F_WIDE` with a byte destination or `SU_F_LATIN1` with a word destination |
| `PrimErrBadIndex` | `#'bad index'` | `start < 1`, `start > stop + 1`, `stop > units`; a destination `index < 1` or `> destUnits + 1`; a UTF-16 source with `start − 1` odd; a UTF-16 target into a destination with `index − 1` odd |
| `PrimErrNoModification` | `#'no modification'` | the destination `isOopImmutable` |
| `PrimErrNoMemory` | `#'insufficient object memory'` | only `primitiveInfo` (small objects; the VM's normal retry applies, as SimdJSON's `primitiveInfo`) |
| (module missing) | `#'not found'` (or `nil`) | VM without the plugin |

`PrimErrNoCMemory` is never used: no `malloc` anywhere.

**Check order** (every primitive): (1) argument count; (2) the source is a byte object or a word object (anything else BadArgument); (3) every numeric argument is a SmallInteger; (4) flag/option/mode values, **and the source kind fits the operation and encoding** (else BadArgument): encode/measure-ENCODED take either kind; decode, measure-DECODED, measure-BASE64, base64 encode/decode and validate-VALID with encoding 0/3/4 take a byte source only; validate-VALID with encoding 2 takes a word source only, encoding 1 is always BadArgument; validate-ASCII takes either; (5) source range and UTF-16 source alignment; (6) class arguments; (7) destination: nil, or kind, `≠ source`, mutable, index range, UTF-16 alignment. A failing primitive leaves the stack alone and returns 0. The FFI library and `SimdUTFSmalltalk` apply the same order (see Library contract, and the backend protocol of the package), and the differential tests compare `ec`, for the kinds that `simdUTFUnits` names: those two take no other kind at (2) and (7) (see Backends).

**Order of answers** once every check passed: UTF into mode (encode, decode): validity errors (codes 1–6, 1001) > 1002 NOT_LATIN1 > TOO_LARGE (the source is validated before the room is compared). Base64 decode into mode: TOO_LARGE first (the room is compared with `L` before decoding), then codes 7–9. Base64 encode has no validity error.

**Kinds** (as SimdJSON's `primitiveRead`):
- *byte object*: `isBytes`, and `firstIndexableField` non-NULL when `byteSizeOf > 0` (this refuses CompiledMethods). Units: `byteSizeOf`.
- *word object*: `isWords` (format 10/11: WideString, WideSymbol, WordArray…). Units: `byteSizeOf / 4`.
- *string source*: a byte object (read as Latin-1) or a word object (read as native-endian UTF-32). Which classes are Strings is the image's business; the primitive is permissive and memory-safe. The image-side backends are not: they take the kinds of `simdUTFUnits` only (see Backends).
- *byte source*: a byte object.
- *destination*: `nil`, or a byte object, or (decode only) a word object.

**Class argument check** (`wideStringClass`): `fetchClassOf(fetchClassOf(c)) == fetchClassOf(fetchClassOf(classString()))` (its class is a metaclass) **and** `classFormat(c)` (as SimdJSONPlugin's) has `instSpec == 10` (`firstLongFormat`) and 0 fixed fields **and** `c != classFloat()` (BoxedFloat64 passes the other two checks and makes `instantiateClassindexableSize` fail with `PrimErrBadReceiver` for sizes ≠ 2). The image passes `WideString`.

### Constants (identical in `suEngine.h` and `SimdUTF class`; `suFFI.h` has only `SU_FFI_PROTOCOL` and its two failure words, and takes the numbers below)

| Name | Value | Meaning |
|---|---|---|
| `SU_ENC_UTF8`, `SU_ENC_LATIN1`, `SU_ENC_UTF32`, `SU_ENC_UTF16LE`, `SU_ENC_UTF16BE` | 0, 1, 2, 3, 4 | encodings (0–2 are SimdJSON's `SJ_ENC_*` numbers, `sjBridge.h`) |
| `SU_F_ENCODING` | mask 7 | flags bits 0–2: an encoding |
| `SU_F_BOM` | 8 | encode: write a BOM first; decode / measure-decoded: skip **one** leading U+FEFF in the declared encoding |
| `SU_F_WIDE` | 16 | decode: always the WideString class |
| `SU_F_LATIN1` | 32 | decode: always a ByteString; a character > U+00FF is ERROR 1002 |
| `SU_F_OP` | mask 192 | the operation of measure / validate |
| `SU_OP_ENCODED`, `SU_OP_DECODED`, `SU_OP_BASE64` | 0, 64, 128 | measure: what is measured |
| `SU_OP_VALID`, `SU_OP_ASCII` | 0, 64 | validate: what is checked |
| base64 encode options | 0 standard padded, 1 URL unpadded, 2 standard unpadded, 3 URL padded | simdutf's `base64_options` |
| base64 decode options | 0 standard, 1 URL, 8 standard-or-URL | the same enum; no garbage-accepting options |
| last chunk | 0 loose (padding optional), 1 strict (padding required, zero pad bits) | simdutf's `last_chunk_handling_options`; 2 and 3 refused |
| line end | 0 LF, 1 CR, 2 CRLF | Zinc's `lineEndConvention:` symbols #lf #cr #crlf |
| line length | 0 (no lines) or a positive multiple of 4 below 2^30 | Zinc's `breakLinesAt:` asserts a multiple of 4 |

### The primitives

All on `SimdUTFPrimitives class`. "src" ranges are 1-based inclusive, in source units.

| # | Primitive | Smalltalk method (class side) | Args | Answer |
|---|---|---|---|---|
| 0 | `primitiveInfo` | `primInfo` | 0 | `{ 1. 'SimdUTFPlugin 1.0'. '9.4.0'. kernel. platform. kernels. forced }` |
| 1 | `primitiveEncode` | `encode: aString from: start to: stop flags: flags into: destOrNil at: index` | 6 | new ByteArray / bytes written / status word |
| 2 | `primitiveDecode` | `decode: bytes from: start to: stop flags: flags wideStringClass: wideClass into: destOrNil at: index` | 7 | new ByteString or wideClass instance / characters written / status word |
| 3 | `primitiveMeasure` | `measure: source from: start to: stop flags: flags` | 4 | SmallInteger ≥ 0 / status word |
| 4 | `primitiveValidate` | `validate: source from: start to: stop flags: flags` | 4 | 0 / status word |
| 5 | `primitiveBase64Encode` | `base64Encode: bytes from: start to: stop options: options lineLength: lineLength lineEnd: lineEnd into: destOrNil at: index` | 8 | new ByteString / characters written / status word |
| 6 | `primitiveBase64Decode` | `base64Decode: source from: start to: stop options: options lastChunk: mode into: destOrNil at: index` | 7 | new ByteArray / bytes written / status word |

`index` is a SmallInteger always (ignored when `destOrNil` is nil; the image passes 1).

**0. `primitiveInfo`.** Slots: 1 `SU_PROTOCOL`; 2 plugin version; 3 `SIMDUTF_VERSION`; 4 the active kernel's `name()` after the guard (`'icelake'`, `'haswell'`, `'westmere'`, `'arm64'`, `'fallback'`); 5 `'native'` or `'emscripten'`; 6 an Array of the ByteString names of the compiled-in kernels that are `supported_by_runtime_system()` and not `"unsupported"`, in `get_available_implementations()` order; 7 `nil` when `SIMDUTF_FORCE_IMPLEMENTATION` is unset or empty, the honoured name, or `'refused <value>'`, cut to 63 bytes (`su_info.forced` has 64, NUL included, frozen by `SU_PROTOCOL`): of a value longer than 55 bytes, the first 55 (even within a UTF-8 sequence). Made with `instantiateClassindexableSize` + `memcpy` + `storePointerofObjectwithValue` (new objects may be old).

**1. `primitiveEncode`.** `aString`: a string source. `flags` = target encoding ∈ {0, 3, 4} + optional `SU_F_BOM`; anything else BadArgument. Result kind BYTES (ByteArray). ERROR only for word sources: code 5 (value > 0x10FFFF, including WideString values up to 0x3FFFFFFF) or 6 (surrogate), at the word index.

**2. `primitiveDecode`.** `bytes`: a byte source holding `flags & 7` ∈ {0, 3, 4}; optional `SU_F_BOM`; at most one of `SU_F_WIDE`, `SU_F_LATIN1`. `wideClass` is checked even when unused. Allocate mode answers the narrowest class (ByteString when every character ≤ U+00FF, else wideClass) unless a flag forces it. Into mode: a byte destination receives Latin-1, a word destination UTF-32; answers characters written. ERROR: UTF-8 codes 1–6 at the first byte of the offending sequence (the `count` of `validate_utf8_with_errors`); UTF-16 code 6 at the first byte of the offending 16-bit unit (`start + 2·count`); 1001 when `stop − start + 1` is odd, at `stop`; 1002 (Latin-1 forced or byte destination) at the first byte ≥ 0xC4 (UTF-8) or the first byte of the first unit > 0xFF (UTF-16). Validity errors take precedence over 1002. Every U+FEFF is kept except one leading one under `SU_F_BOM`.

**3. `primitiveMeasure`.**
- `SU_OP_ENCODED` (flags = target {0, 3, 4} + optional BOM; string source): exact bytes of the encoding. ERROR as encode.
- `SU_OP_DECODED` (flags = 64 + source encoding {0, 3, 4} + optional BOM; byte source): `chars·2 + wide`, `wide` = 1 iff a character > U+00FF. ERROR as decode (no 1002).
- `SU_OP_BASE64` (flags = 128 exactly; byte source): **`L`, the portable base64 measure** of the range — exact for every input that decodes. No validation. Definition (the same on every platform and in `SimdUTFSmalltalk`): `count` = bytes in 0x21..0x7F; `padding` = scanning backwards from the end, skipping bytes outside 0x21..0x7F, up to two `=` before the first other byte in 0x21..0x7F; `L = ((count − padding)·3) // 4`. Reason: simdutf's scalar `binary_length_from_base64` tests `input[i] > ' '` on `char` (`include/simdutf/scalar/base64.h` of simdutf), which counts bytes ≥ 0x80 where `char` is unsigned (Linux aarch64: the arm64 kernel and the fallback) and not where it is signed (the x86 kernels use a signed byte compare; WebAssembly). The engine therefore runs `validate_ascii_with_errors` on the range first: all ASCII → `binary_length_from_base64` (both interpretations agree); otherwise a portable scalar loop with the rule above (the signed-`char` interpretation, i.e. x86's). Non-ASCII input never decodes (code 7), so only TOO_LARGE vs ERROR 7 in into mode and the measure value depend on it — now identically everywhere.

**4. `primitiveValidate`.**
- `SU_OP_VALID` + encoding: 0, 3 or 4 on a byte source; 2 on a word source (every word a Unicode scalar value). 1 is BadArgument. ERROR as decode / encode; odd UTF-16 is 1001 at `stop`.
- `SU_OP_ASCII` (flags = 64 exactly) on a byte or word source: ERROR code 5 at the first unit > 0x7F. Bytes: `validate_ascii_with_errors`. Words: simdutf has no ASCII check for UTF-32, so the engine runs a scalar loop over the 32-bit words (any word > 0x7F, values ≥ 2^31 included, is code 5 at that word).
- Answers 0 when valid.

**5. `primitiveBase64Encode`.** `bytes`: a byte source. `options` ∈ 0–3, `lineLength` 0 or a multiple of 4 in [4, 2^30), `lineEnd` ∈ {0, 1, 2} (checked even when `lineLength` = 0). Result kind LATIN1 (a ByteString, as Zinc's `encode:`), or characters written into a byte destination. Size: `T = L + lineEnds·eol`, `L = base64_length_from_binary(n, options)`, `lineEnds = lineLength = 0 ifTrue: [0] ifFalse: [ceil(n/3) // (lineLength/4)]`, `eol` = 1, 1, 2 — **Zinc's rule**: a line end after each group that completes a line, the last group included and counted as 4 even unpadded; none after an incomplete last line. simdutf's `binary_to_base64_with_lines` (LF between lines only) is not used.

**6. `primitiveBase64Decode`.** `source`: a byte source (a WideString is BadArgument; the image narrows it). `options` ∈ {0, 1, 8}, `mode` ∈ {0, 1}. ASCII white space (space, tab, LF, FF, CR) is skipped anywhere. Result kind BYTES. Size `L` = the portable base64 measure of #3 (exact for valid input); into mode checks `L ≤ room` first (TOO_LARGE with payload `L`). ERROR: code 7 at `start + r.count` (the bad character; a misplaced `=` included); codes 8 and 9 at the absolute index of the last non-white-space byte of the range. On ERROR in into mode the bytes `index … index + L − 1` of the destination are unspecified (the only primitive that may write before failing).

**Exports** (the Slang row layout that `cmake/emscripten/genPrimitiveTable.cmake` parses):

```c
#ifdef SQUEAK_BUILTIN_PLUGIN
static char _m[] = "SimdUTFPlugin";
void* SimdUTFPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveBase64Decode\000\000", (void*)primitiveBase64Decode},
	{(void*)_m, "primitiveBase64Encode\000\000", (void*)primitiveBase64Encode},
	{(void*)_m, "primitiveDecode\000\000", (void*)primitiveDecode},
	{(void*)_m, "primitiveEncode\000\000", (void*)primitiveEncode},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveMeasure\000\000", (void*)primitiveMeasure},
	{(void*)_m, "primitiveValidate\000\000", (void*)primitiveValidate},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};
#else
EXPORT(signed char) primitiveBase64DecodeAccessorDepth = 0;
EXPORT(signed char) primitiveBase64EncodeAccessorDepth = 0;
EXPORT(signed char) primitiveDecodeAccessorDepth = 0;
EXPORT(signed char) primitiveEncodeAccessorDepth = 0;
EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveMeasureAccessorDepth = 0;
EXPORT(signed char) primitiveValidateAccessorDepth = 0;
#endif
```

### Allocation, exact sizes, GC safety

**Rule: at most one heap allocation per primitive call, at the exact size, after every check and every validation that can fail** (except base64 decode, whose validation is the decode itself: one allocation of the exact size `L`, then the decode; invalid input leaves that object as garbage). Invalid UTF input never allocates.

- The only allocator is `instantiateClassindexableSize` (falls back to old space when eden is full; sets `PrimErrNoMemory` on failure). The glue has exactly one call site for results (`allocateResult`) plus the `primitiveInfo` helper.
- Never `signed64BitIntegerFor`, `positive64BitIntegerFor`, `floatObjectOf`, `stringForCString`, `methodReturnString`, `clone` (eden-only, or no failure code). Integers only via `integerObjectOf`.
- The proxy cannot shorten an object, hence exact sizes.
- `instantiate…` zero-fills the body, so a result is written twice; accepted (no uninitialised allocation in the proxy).

| Result | Exact size before allocation | Conversion | Kind |
|---|---|---|---|
| Latin-1 → UTF-8 | `utf8_length_from_latin1` (+3 BOM) | `== n` → `memcpy`, else `convert_latin1_to_utf8` | BYTES |
| UTF-32 → UTF-8 | `validate_utf32_with_errors`, then `utf8_length_from_utf32` (+3) | `convert_valid_utf32_to_utf8` | BYTES |
| Latin-1 → UTF-16LE/BE | `2n` (+2) | `convert_latin1_to_utf16le/be` | BYTES |
| UTF-32 → UTF-16LE/BE | validate, then `2·utf16_length_from_utf32` (+2) (C++ only) | `convert_valid_utf32_to_utf16le/be` | BYTES |
| UTF-8 → String | `validate_utf8_with_counts` → `chars = input_count − continuation_count` (on error, position from `validate_utf8_with_errors`, a second call on the error path only); `chars == n` → ASCII; else scan for the first byte ≥ 0xC4 (SWAR per 8-byte word: `((w & 0x7F7F…) + 0x3C3C…) & w & 0x8080…` ≠ 0; tested exhaustively on all 256 values) | ASCII `memcpy`; Latin-1 `convert_valid_utf8_to_latin1`; wide `convert_valid_utf8_to_utf32` | LATIN1 / UTF32 |
| UTF-16 → String | odd length → 1001; `validate_utf16le/be_with_errors`; scan for a unit > 0xFF (endian-aware) | none: `n/2` chars, `convert_valid_utf16xx_to_latin1`; else `utf32_length_from_utf16xx` chars, `convert_valid_utf16xx_to_utf32` | LATIN1 / UTF32 |
| base64 encode | `T` of `primitiveBase64Encode` | `binary_to_base64` once into `dst + E` (`E = lineEnds·eol`); for `k = 0 … lineEnds−1`: `len = min(ll, L − k·ll)`, `memmove(dst + k·(ll+eol), dst + E + k·ll, len)`, then the line end; the tail is already in place. Safe because `k·(ll+eol) ≤ E + k·ll` for `k ≤ lineEnds` | LATIN1 |
| base64 decode | `L` = the portable measure of `primitiveMeasure` (`validate_ascii_with_errors`, then `binary_length_from_base64` or the scalar count) | `base64_to_binary_safe(src, n, dst, outlen = L, options, mode, false)`; SUCCESS with `outlen ≠ L` → ERROR 11 (never; the C tests fuzz it) | BYTES |

The "no byte ≥ 0xC4 ⇔ Latin-1" rule had 0 mismatches on 200k strings natively and in WebAssembly.

**Large outputs.** Two mechanisms, both SimdJSON's:
1. **OVER_BUDGET**: when `allocateResult` fails **with `PrimErrNoMemory`**, `primitiveFailFor(0)` and answer status 1 with the Spur bytes needed. The image grows memory by `need + 1 MiB` (`Smalltalk growMemoryByAtLeast: payload + 1048576`, as `SimdJSONWriter`; an old-space allocation needs one contiguous chunk and fails without growing even when `vm parameterAt: 54` ≥ need) and calls once more; when the memory cannot grow, or on a second OVER_BUDGET, it collects all the garbage and calls a last time, and signals `OutOfMemory` on OVER_BUDGET after that.
2. **Into mode**: every allocating primitive takes an optional destination; it never allocates, so never answers OVER_BUDGET. Used for buffer reuse and by the FFI backend (sizing call + into call).

**GC safety.** Spur collects no garbage inside a proxy allocation (`instantiateClassindexableSize` only schedules a scavenge, and old-space allocation neither collects nor grows the memory), so the source address read before the call (`unitsOf`, then `su_input.base`) stays valid: the engine keeps it, and the pointers it derives from it, across `allocateResult`, and nothing reads it again. This guarantee is the only one: should a VM ever collect inside that allocation, `allocateResult` would have to refresh the source address and the engine re-derive its pointers after `place()`. No oop is cached across calls (the WideString class arrives on every call; the proxy has no `classWideString`). Results are returned by `methodReturnValue` only; bodies are written raw, no pointer stores except in `primitiveInfo`. A retry after a forwarder failure re-reads every argument; nothing has side effects before the allocation.

`spurBytes(n) = spurPointers((n + 7) / 8)`, `spurWords32(n) = spurPointers((n + 1) / 2)`, `spurPointers(s) = 8 + (s >= 255 ? 8 : 0) + 8·max(s, 1)` (as SimdJSONPlugin's `sjText.c`), as `static` functions of the glue.

### The `SIMDUTF_FORCE_IMPLEMENTATION` guard (`su_init`)

```cpp
static char forcedNote[64];      /* "", the honoured name, or "refused <value>" */
static bool ready;
static bool choose() {
	const auto &list = simdutf::get_available_implementations();
	const simdutf::implementation *chosen = list.detect_best_supported();   /* cpuid only, no kernel code */
	const char *forced = std::getenv("SIMDUTF_FORCE_IMPLEMENTATION");
	if (forced && *forced) {
		const simdutf::implementation *f = list[forced];
		if (f && f->name() != "unsupported" && f->supported_by_runtime_system()) {
			chosen = f;
			std::snprintf(forcedNote, sizeof forcedNote, "%s", forced);
		} else {
			std::snprintf(forcedNote, sizeof forcedNote, "refused %s", forced);
		}
	}
	simdutf::get_active_implementation() = chosen;   /* before any first use: set_best() never runs */
	ready = chosen->name() != "unsupported";
	return true;
}
extern "C" int su_init(void) { static const bool once = choose(); (void)once; return ready; }
```

- Policy: honoured only when it names a compiled-in kernel the CPU supports and that is not `unsupported`; otherwise the best kernel, and `primitiveInfo` slot 7 / `su_ffi_forced()` say `refused <value>`, cut to 63 bytes as `forcedNote` is (the first 55 bytes of a longer value; kernel names, at most 8 bytes, are never cut). The guard lane of `run-c-tests.sh` checks it with 80 `x`.
- Assigning before any call replaces simdutf's first-use proxy, whose `set_best()` is the only place simdutf reads the variable itself (`src/implementation.cpp` of simdutf), so simdutf's own `getenv` never runs and no kernel code runs during the choice: `=icelake` cannot raise SIGILL on a CPU without AVX-512 VBMI2, `=bogus` cannot select the `unsupported` singleton that answers 0 everywhere.
- `unsupported` requires no instruction set, so `supported_by_runtime_system()` is true for it: hence the name test.
- Single-kernel builds (arm64, WebAssembly): simdutf's free functions use the single kernel and ignore the pointer; forcing anything but the one kernel is refused, which is reported, and harmless.
- The magic static (`__cxa_guard_*`, already used by simdutf) makes the library's first call thread-safe whatever thread calls it (defensive: the image uses the same-thread runner only).
- `getenv` appears only here.

### The engine interface

`include/common/suEngine.h` is the C interface between the glue, `libSimdUTFFFI` and the engine; it is part of this contract and is not changed without a change of `SU_PROTOCOL`.

Engine rules (`suEngine.cpp`): includes only `simdutf.h`, `<cstdlib>`, `<cstdio>`, `<cstring>`, `<string_view>`; every function `extern "C"`; no `new`, `malloc`/`calloc`/`realloc`, `throw`, `std::vector`, `std::string`; `static_assert`s pin `simdutf::error_code` 0–11; each function re-checks its request (encoding vs operation, flag set, `start0 ≤ stop0 ≤ units`, UTF-16 `start0` even, sink kind vs flags, into-mode UTF-16 `base` 2-aligned) and answers `SU_RC_BAD_*` outside the contract. `su_measure` DECODED with `SU_F_WIDE`/`SU_F_LATIN1` is BAD_ARGUMENT. `su_base64_decode` maps BASE64_INPUT_REMAINDER/EXTRA_BITS positions to the last non-white-space byte of the range.

The glue maps the answer of an engine call: `SU_RC_DONE` + `SU_OK` → the new object (`methodReturnValue(a.oop)`) or `integerObjectOf(out.units)`; `SU_RC_DONE` + `SU_ERROR` → `answerStatus(2, code, position0 + 1)`; `SU_RC_NO_ROOM` → allocate mode: `a.badArgument` → `primitiveFailFor(PrimErrBadArgument)`, else `answerStatus(1, 0, a.need)`; into mode `answerStatus(3, 0, out.units)`; `SU_RC_BAD_ARGUMENT`/`BAD_INDEX` (cannot happen after the glue's checks) → `PrimErrBadArgument`/`PrimErrBadIndex` as a backstop. `primitiveMeasure` DECODED answers `integerObjectOf(2·units + (kind == SU_KIND_UTF32))`.

## Library contract

### Curated `su_ffi_*`, not simdutf's C API

1. **Identical answers**: each `su_ffi_*` runs the plugin's engine and answers the primitive's status words; one image-side decoder serves all backends, and the C tests check the equality.
2. **simdutf's C API cannot cover the scope**: no kernel name, no `binary_length_from_base64`, no `utf16_length_from_utf32`, no guard; it is "experimental" (`include/simdutf_c.h` of simdutf); its `simdutf_result` struct returns would need an `FFIExternalStructure` per call; and it exists only with all seven features (`src/simdutf_c.cpp`), while this build turns `SIMDUTF_FEATURE_DETECT_ENCODING` off.
3. **Fewer callouts**: a decode is 2 callouts (measure, decode) instead of 3–4.
4. **No interposition**: hidden C and C++ visibility; exactly 11 exported symbols (as `libSimdJSONFFI`).

### `include/ffi/suFFI.h`

`include/ffi/suFFI.h` declares the 11 functions and is part of this contract: every `int64` answer is what the primitive of the same name answers, ≥ 0 on success, else the status word with positions 1-based and absolute in the source object; the status-0 words `SU_FFI_BAD_ARGUMENT` (-4) and `SU_FFI_BAD_INDEX` (-8) are the failures the primitive would have; OVER_BUDGET is never answered, since the image allocates.

`suFFI.c`: every function calls `su_init()`; checks NULL `source` (with `units > 0`) and NULL `dst` → BAD_ARGUMENT before anything else (a NULL `dst` with `at0 = 0` would otherwise make `base == NULL`, which the engine reads as allocate mode); `0 ≤ start0 ≤ stop0 ≤ units`, `0 ≤ at0 ≤ dstUnits`, `dst != source`, UTF-16 alignment (BAD_INDEX) and argument values (BAD_ARGUMENT) in the primitive's order; builds `su_input` and an into-sink (`base = dst + at0·unitBytes`, `capacity = dstUnits − at0`, `kind` from `dstWords` / the operation); maps `SU_RC_NO_ROOM` → TOO_LARGE word, `SU_RC_BAD_*` → the two constants, ERROR → status word with `position0 + 1`. No scratch, no static mutable state beyond `su_init`'s: reentrant.

### How the image calls it (`SimdUTF-FFI`)

- **Declarations**: every address is `void *`, never `String`/`char *` (the FFI would marshal strings through Zinc's `utf8Encoded`); numbers `int64`; the three name getters `String` (called once a session).
- **Sources in place**: exactly a ByteArray, ByteString or WideString (`simdUTFFFIEncoding` non-nil, exact-class guards as SimdJSON-FFI); any other accepted source (Symbols, subclass instances) goes as `simdUTFPassableCopy` (a fresh ByteArray/ByteString/WideString of the same units), as `SimdJSONLibrary>>passable:`.
- **Pinning without `tfPointerAddress`** (SimdJSON-FFI owns `WideString>>tfPointerAddress`; a second definition would steal it):

  ```smalltalk
  SimdUTFLibrary >> with: anObject address: aBlock
  	^ self class pinLock critical: [            "a reentrant Mutex: see Runner"
  		| wasPinned |
  		wasPinned := anObject pinInMemory.          "answers the previous state"
  		[ aBlock value: (PointerUtils oopForObject: anObject yourself) ]   "yourself: the forwarder that pinning a young object leaves; an ExternalAddress, whose tfPointerAddress is itself"
  			ensure: [ wasPinned ifFalse: [ anObject unpinInMemory ] ] ]
  ```
- **Checks in Smalltalk first**: the library sees only addresses, so it cannot see the read-only bit, and `primitivePin` does not refuse an immutable object. Before any pinning, copy or callout, each `SimdUTFLibrary` backend method runs the primitive's checks (1)–(7) (see Failures) in Smalltalk, in that order, and answers the same `SimdUTFFailure` (for the kinds of `simdUTFUnits`; another byte or word object, which the primitive takes, is `#'bad argument'` here, see Backends): kinds by `simdUTFUnits`, SmallInteger arguments, flag/option values and source-kind fit, ranges and UTF-16 alignment, the `wideStringClass` (`isClass`, not a metaclass, `isWords`, `instSize = 0`, not `BoxedFloat64`: exactly the primitive's check), and the destination (kind; `destOrNil == source` → `#'bad argument'`; `destOrNil isReadOnlyObject` → `#'no modification'`; index range; alignment). The library's own `SU_FFI_BAD_*` answers stay as a backstop. `SimdUTFSmalltalk` runs the same Smalltalk check method (shared, on `SimdUTFSmalltalk class`, which `SimdUTFLibrary` calls), so it answers `#'no modification'` instead of letting `ModificationForbidden` escape.
- **Results: sizing call, then into call.** Every result created with `basicNewPinned:` is unpinned in an `ensure:` once the callout returns, on every path (success, ERROR, TOO_LARGE), and `SimdUTFLibraryTests` checks `isPinnedInMemory` is false on each answered object.
  - encode: `su_ffi_measure` (ENCODED) → bytes; `self roomFor: bytes`; `ByteArray basicNewPinned: bytes` (old space, no clone); `su_ffi_encode`; unpin.
  - decode: `su_ffi_measure` with flags `64 + (flags bitAnd: 15)` (DECODED + encoding + BOM: the engine refuses DECODED with `SU_F_WIDE`/`SU_F_LATIN1`) → `chars·2 + wide`; class = the checked `wideStringClass` argument `w` when wide or `SU_F_WIDE` was asked, else ByteString; `basicNewPinned: chars`; `su_ffi_decode` (`dstWords` 1 or 0); unpin. When `SU_F_LATIN1` was asked and the measure says wide, `su_ffi_decode` into an empty pinned ByteString answers ERROR 1002 with its position (the engine checks NOT_LATIN1 before the room), exactly as the primitive, and nothing of the size of the text is allocated.
  - base64 encode: `su_ffi_base64_encoded_size` (or the identical Smalltalk formula `SimdUTF base64SizeOf:options:lineLength:lineEnd:`) → `ByteString basicNewPinned:` → `su_ffi_base64_encode`; unpin.
  - base64 decode: `su_ffi_measure` (BASE64) → `L` (exact for valid input) → `ByteArray basicNewPinned: L` → `su_ffi_base64_decode`; unpin; success writes exactly `L`. No extra copy.
  - Into mode, `at0 = index − 1`: a destination whose class is **exactly** ByteArray, ByteString or WideString (`simdUTFFFIEncoding` non-nil) is pinned in place. Any other accepted destination (a subclass instance, e.g. a UUID) goes through an exact temporary of the same units (`simdUTFPassableCopy`), which is copied back with `simdUTFReplaceFrom:to:with:startingAt:` (into the destination's own units, whatever its class overrides: an `ExternalAddress` is a ByteArray, whose `replaceFrom:to:with:startingAt:` would write through the address it holds) on success only. CompiledCode is never a destination or source: `CompiledCode>>simdUTFUnits ^ nil`, since the FFI would otherwise write over the method header.
- `roomFor:` lives on the facade, `SimdUTF class>>roomFor:` (private, package `SimdUTF`, so `SimdUTF-Tests` can test it without the FFI package); `SimdUTFLibrary` calls it. It is a copy of `SimdJSONLibrary>>roomFor:`: from 1 MiB, grow by `2·bytes + 4 MiB` when free old space is below that.
- **Runner**: `SimdUTFLibrary` never overrides `runner`, so callouts use the default `TFSameThreadRunner`, which rules out another OS thread unpinning an object while a callout reads it.
  Green threads of the one OS thread interleave too: `processPreemptionYields` is true in Pharo 12, so a process preempted inside its bracket lets the others of its priority run, and a process that pinned an object already pinned by another one would leave the unpinning to that one, which could unpin it, and a GC move it, while the first still uses its address.
  So the whole bracket, from the pinning of the source to its unpinning (the sizing call, `roomFor:`, the result, the callouts, the destination's bracket and its copy back), runs under one reentrant `Mutex`, `SimdUTFLibrary class>>pinLock`; callouts of the same-thread runner stop the VM anyway, so nothing runs less in parallel.
  Code outside `SimdUTFLibrary` that unpins an object it passes (its own code, SimdJSON-FFI's `tfPointerAddress` bracket) is not covered.
- **Threshold**: the facade uses the library only for sources of at least `SimdUTFLibrary minimumUnits` units (default **64**, `minimumUnits:` sets it). Below, the Smalltalk backend answers: two callouts, the pinning and the allocation of the result cost about 0.8 to 1.8 µs on the native Cog VM, as much as the Smalltalk backend spends on 60 to 120 characters of mixed text (see Benchmarks).
- **Availability**: `SimdUTFLibrary class>>isAvailable`, memoised per `Smalltalk session`, true when the library is found and `su_ffi_protocol = 1` (as `SimdJSONLibrary class>>isAvailable`). `libraryPath:` overrides the search, and puts back every callout of the class as written (`resetCallouts`, a walk over a copy: `FFIMethodRegistry>>resetSingleClass:` removes from the set it walks, and can miss a callout, which then keeps the library it found). Names: `libSimdUTFFFI.so` (`FFIUnix64LibraryFinder findLibrary:`), `libSimdUTFFFI.dylib` (`FFIMacLibraryFinder`), `FFIWindowsLibraryFinder findAnyLibrary: #('SimdUTFFFI.dll' 'libSimdUTFFFI.dll')`.
- No FFI in the WebAssembly VMs (no `dlopen`; a callout would cost 3.5–30 µs).

## Tests

    sh plugins/SimdUTFPlugin/tests/run-c-tests.sh --simdutf <simdutf 9.4.0 tree> --emsdk <emsdk> --vm-build <native build tree>   # C
    sh plugins/SimdUTFPlugin/tests/run-st-tests.sh build/build/vm <Pharo 12 to 15 image or zip>                                  # SUnit

`run-c-tests.sh` builds `tests/sutest.cpp` with the engine, `suFFI.c` and simdutf's unity file, with references of its own written without simdutf (UTF-8 by Table 3-7 of Unicode, UTF-16, and Zinc's base64 encoder with its line rule), and runs it in six lanes: clang++, the same binary under `SIMDUTF_FORCE_IMPLEMENTATION=fallback` (the kernel of the web VMs), g++ with ASan and UBSan, em++ `-m64` and em++ `-sMEMORY64=2` under node, and the guard, one process per value of `SIMDUTF_FORCE_IMPLEMENTATION` (unset, `fallback`, `haswell`, `westmere`, `bogus`, `icelake`, and 80 `x`, whose note is cut to `refused ` and 55 `x`), which must choose the expected kernel and never one the CPU lacks.
Its checks:

- U1 encoding, U2 decoding and U3 measures: every SIMD edge of the lengths, random strings of five classes, sub-ranges with absolute positions, every error code at its position (the scan for a byte ≥ 0xC4 against all 256 byte values at every alignment), the base64 measure of input with bytes ≥ 0x80;
- U4 validation; U5 base64 encoding (every option, line length and line end, against Zinc's rule); U6 base64 decoding against simdutf's own decoder with room for anything, on a million random byte strings; U7 the sinks (one allocation at most, none for invalid UTF input, nothing written without room);
- U8 the guard; U9 every `su_ffi_*` against the engine, failures in the primitive's order, NULL addresses and overflowing sizes;
- U10 (`tests/gluetest.c`, with `--vm-build`): the glue `SimdUTFPlugin.c`, as a shared and as a builtin plugin, against a fake VM (`tests/fakevm.c`) with exact-size objects under ASan: every arity, the order of the checks with every pair of faults, every value of the flags, 200,000 random calls against the engine, OVER_BUDGET with its exact Spur sizes, and the allocation failures that must not become one;
- the rules R1–R7 and V: no eden-only allocator and no `malloc` in the glue, no simdutf header in the glue nor in `suFFI.c`, no VM header in the engine nor in the library, no `std::vector`, `std::string`, `new` or `throw` in the engine, the same set of primitives in the exports, the builtin table and the accessor depths, `getenv` only in the guard, one `.c` in `src/common`, and simdutf 9.4.0.

`SU_SLOW=1` adds round trips of 256 MiB.
They are not run in CI, as SimdJSON's are not.

`run-st-tests.sh <vm-dir> <image.zip or .image>` works on a copy of the image: it loads the groups `Core Tests FFI FFITests` (`SU_GROUPS`) with `load-simdutf.st` from the submodule, checks its banner, runs `SimdUTF-Tests` and `SimdUTF-FFI-Tests` with a JUnit file each, and summarises them.
The banner must name the backend that `SU_EXPECT_BACKEND` names (`plugin`, `ffi`, `smalltalk` or `any`; by default `plugin` when `<vm-dir>` holds the plugin, else `any`): a plugin that does not load would only make its tests skip themselves, which the runner counts as passes.
Its environment: `SU_EXPECT_BACKEND`, `SU_SLOW` (runs `SimdUTFSlowTests`: round trips of 64 MiB and base64 of 48 MiB on each backend), `SU_FFI_LIBRARY` (the `libSimdUTFFFI` to use, given to `SimdUTFLibrary libraryPath:`), `SU_WORK_DIR` and `SU_TIMEOUT` (3600 s by default).
The real path of a missing plugin (its primitives failing with `#'not found'`, the facade taking the library) runs only on a VM without the plugin: `SU_FFI_LIBRARY=<build>/build/vm/libSimdUTFFFI.so sh run-st-tests.sh <vm-dir of a build with FEATURE_PLUGIN_SIMDUTF=OFF> <image>` runs `SimdUTFLibraryTests>>testNotFoundTakesTheLibrary`, which every run on a VM with the plugin skips; `SimdUTFFacadeTests>>testMissingModuleIsNotFound` checks on every VM that a missing module fails with `#'not found'`.

The suites run on every backend: `SimdUTFBackendTestCase` is parametrized by the backend, the plugin (skipped without it) and the Smalltalk one, and `SimdUTF-FFI-Tests` runs each suite again on the library (skipped without it).

- `SimdUTFZincEquivalenceTests`: the results equal Zinc's on valid input (without U+FEFF and U+FFFE in UTF-16, which Zinc's decoder treats as byte order marks wherever they are), random strings of five classes up to 64 KiB, the SIMD edges, base64 with every line length, line end and padding;
- `SimdUTFSemanticsTests`: every row of Semantics against Zinc;
- `SimdUTFErrorTests`: every error code through the API, with its class, code and absolute position; the arguments refused (an Array, a CompiledMethod as source and as destination, a read-only destination, the source as destination), a ByteArray subclass taken; errors not resumable, caught as `ZnCharacterEncodingError`;
- `SimdUTFIntoTests`, `SimdUTFBase64EncoderTests` (the encoder against ZnBase64Encoder on 1,000 random inputs per configuration), `SimdUTFSelectionTests` (the choice of the backend, the session, the hooks), `SimdUTFFacadeTests` (a scripted backend: OVER_BUDGET and the growth, the collection when the memory cannot grow or after a second OVER_BUDGET, then `OutOfMemory`, `#'not found'`, every status), `SimdUTFPrimitiveTests` (the raw contract of the plugin, every failure in the order of the checks, forwarders), `SimdUTFDifferentialTests` (20,000 random calls of each primitive: the plugin's answers equal the Smalltalk backend's, failure codes included), `SimdUTFMemoryTests` (a result larger than the free old space, one OVER_BUDGET);
- `SimdUTFLibraryTests`: the library's own answers, the pinning (every object left as pinned as it was, every result unpinned, an object kept pinned while two processes that yield inside their brackets use it), an `ExternalAddress` destination written into its own bytes, a forced Latin-1 decode of wide text, the copies of Symbols and subclass instances, the library path, the threshold of the facade, and 20,000 random calls of each selector against the Smalltalk backend and the plugin.

The native jobs of `.github/workflows/build.yml` load the plugin and the library of their zip with `tests/ci-load.st` (see Building), which compiles its own primitive methods, since the VMMaker image of CI has no SimdUTF, and prints `SimdUTFPlugin <platform> <kernel> <version> forced <forced or -> ok` and `SimdUTFFFI <protocol> <kernel> forced <forced or ->`.
In the WebAssembly build, lane 69 of `make wasm-check` runs the suite with the node and the web VMs (each on the backend `#plugin`, which the summary line of `tests/wasm/st/simdutf-lane.st` names with the skipped tests), converts 32 MiB, and makes 1000 conversions of 1 MiB in a row (see docs/WebAssembly.md).

## Benchmarks

`plugins/SimdUTFPlugin/bench` times SimdUTF against Zinc, through each backend, on the native Cog and Stack VMs and in the WebAssembly VMs (the node VM, and the web VM driven in node):

    sh plugins/SimdUTFPlugin/bench/run-bench.sh --cog <Cog build> --stack <Stack build> --wasm <make wasm build> \
       --node <node> --image <Pharo 12 to 15 image or zip> --out $OUT --repeat 2
    python3 -I plugins/SimdUTFPlugin/bench/summarize.py $OUT/*.tsv

- `run-bench.sh` prepares the image once, natively (`prepare-image.st` files in `tests/load-simdutf.st` with the groups `Core` and `FFI`, then `SUBench.st`), then runs `run-bench.st` on a fresh copy of it with each VM in turn, pinned to one CPU (`taskset -c 2`), and the whole matrix twice (`--repeat`); each run writes a TSV file, which `summarize.py` turns into tables, and checks the gates below. `--cog` and `--stack` take a build tree or the directory of its `pharo`; `SU_PLUGIN_DIR` names another plugin directory to load the Smalltalk code from.
- The data are generated in the image, the same on every VM, as a String whose UTF-8 has exactly the size: `ascii` (characters 32 to 126), `latin` (1 character in 8 in 224 to 251), `wide` (a WideString: 1 character in 16 an emoji, 1 in 4 of the others a CJK ideograph, the rest ASCII), at 52 B, 1 KiB, 64 KiB, 1 MiB and 10 MiB (10 MiB natively only), and `path`, a path of 52 bytes, for the cost of a call. MB/s is always over the bytes of the UTF-8.
- Each operation is timed as the public API calls it, through each backend, set with the test hook `#backend` so that every size goes through it, and as Zinc does it:

| operation | SimdUTF | Zinc |
|---|---|---|
| `utf8Encode` | `SimdUTF utf8Encode: s` | `s utf8Encoded` |
| `utf8Decode` | `SimdUTF utf8Decode: b` | `b utf8Decoded` |
| `utf8SizeOf` | `SimdUTF utf8SizeOf: s` | `ZnUTF8Encoder new encodedByteCountForString: s` |
| `utf8CharacterCountOf` | `SimdUTF utf8CharacterCountOf: b` | `b utf8Decoded size` (Pharo 12 has no `numberOfCharactersFrom:`) |
| `isValidUTF8` | `SimdUTF isValidUTF8: b` | `b utf8Decoded`, false on `ZnCharacterEncodingError` |
| `isAscii` | `SimdUTF isAscii: s` | `s isAsciiString` |
| `utf16LEEncode`, `utf16LEDecode` | `SimdUTF utf16LEEncode: s`, `utf16LEDecode: u` | `ZnUTF16Encoder new beLittleEndian` `encodeString:`, `decodeBytes:` |
| `base64Encode`, `base64Encode76` | `SimdUTF base64Encode: b`, and lines of 76 with CRLF | `ZnBase64Encoder new encode: b`, with `breakLines` |
| `base64Decode` | `SimdUTF base64Decode: t` | `ZnBase64Encoder new decode: t` |

- Before a series is timed, its result is compared with Zinc's, value and class; a series is a full garbage collection, warm-up calls, then samples of k calls, k doubled until a sample lasts 20 ms (50 ms in WebAssembly, whose clock has a resolution of 1 ms), 10 samples natively and 5 in WebAssembly, each after a scavenge, or after a full garbage collection when its calls leave 8 MiB of garbage or more (the plugin's allocations collect no garbage: see Memory and limits); the best sample counts.
- The gates, which `summarize.py` checks: every result equal to Zinc's, no error, the plugin at least 10 times faster than Zinc natively and 30 times in WebAssembly on every conversion of 1 MiB (encoding, decoding and base64), the library at least 5 times, the plugin at least 3 times on `utf8Encode` and `utf8Decode` of the 52-byte path natively, and the Smalltalk backend at least half as fast as Zinc on every series.

### Results

Measured on 2026-11-16 with `run-bench.sh --repeat 2`; every value is the median of the two runs, each the best sample.

- Machine: 2 x Intel Xeon Gold 6238R @ 2.20GHz (Cascade Lake, AVX-512 but no AVX-512 VBMI2: simdutf picks its `haswell` kernel natively), the machine of docs/SimdJSON.md, Linux 6.8.0-139-generic; the WebAssembly VMs run simdutf's `fallback` kernel.
- Sources: commit 3765447b4 (pharo-12-simdutf) and its uncommitted SimdUTFPlugin tree; simdutf.st b51efd3. VMs: v12.0.6-beta+120.3765447b4, the native CoInterpreter and StackVM (Release, Ubuntu clang 18.1.3), and `make wasm` (Emscripten 6.0.10, wasm64), as `node/pharo` (wasm-node) and as `web/pharo-web.js` through `vm-driver.js` (wasm-web-node), under node 24.19.0.
- Image: Pharo 12.0 SNAPSHOT b1599, prepared by `prepare-image.st`.
- The native VMs ran on CPU 2 and the WebAssembly VMs at the same time on CPU 3 (`--vms`, two invocations); a run took 15 minutes with Cog, 25 with Stack and 9 to 10 with each WebAssembly VM.
  Between the two runs the medians of a VM's series moved by 0.5 to 1.2%; the series that moved most are those of 10 MiB natively, whose allocations meet full garbage collections (up to 134% for one of the library).

Gates: 4532 results equal to Zinc's, no error; the plugin's gates and the library's pass (the smallest speedups at 1 MiB: 21.9x natively, `utf8Encode` of wide text on Cog, and 121x in WebAssembly), so does the 52-byte call (6.4x for `utf8Encode` on Cog, 9.3x for `utf8Decode`).
**The Smalltalk gate fails on `isAscii` only** (42 of the backend's 638 series, all of them `isAscii`): Zinc's `isAsciiString` is a primitive that stops at the first character above 127, which in the latin and wide data is among the first 8; the Smalltalk backend answers the same in 0.3 µs on Cog, three times Zinc's 0.1 µs, for its checks of the arguments and the facade's call. Every other series of the Smalltalk backend is at least as fast as Zinc's but `utf8Encode` of wide text on Stack and in WebAssembly (0.65 to 0.92x) and `utf8SizeOf` of 52 bytes of wide text in WebAssembly (0.87x).

The plugin over Zinc at 1 MiB (Zinc's time over the plugin's):

| op | data | Cog | Stack | node | web |
|---|---|---:|---:|---:|---:|
| utf8Encode | ascii | 37.9 | 123 | 691 | 596 |
| utf8Encode | latin | 46.2 | 127 | 131 | 158 |
| utf8Encode | wide | 21.9 | 74.0 | 122 | 121 |
| utf8Decode | ascii | 81.2 | 284 | 1421 | 1367 |
| utf8Decode | latin | 45.6 | 143 | 137 | 197 |
| utf8Decode | wide | 25.2 | 80.5 | 174 | 184 |
| utf8SizeOf | ascii | 974 | 4057 | 3695 | 4077 |
| utf8SizeOf | latin | 1231 | 4452 | 3784 | 4195 |
| utf8SizeOf | wide | 66.2 | 250 | 168 | 156 |
| utf8CharacterCountOf | ascii | 1482 | 5020 | 4947 | 7168 |
| utf8CharacterCountOf | latin | 193 | 604 | 421 | 613 |
| utf8CharacterCountOf | wide | 317 | 1004 | 477 | 585 |
| isValidUTF8 | ascii | 1539 | 5219 | 5760 | 7218 |
| isValidUTF8 | latin | 425 | 1322 | 536 | 689 |
| isValidUTF8 | wide | 401 | 1252 | 429 | 579 |
| isAscii | ascii | 28.6 | 28.6 | 7.8 | 12.3 |
| isAscii | latin | 0.55 | 0.31 | 0.26 | 0.27 |
| isAscii | wide | 1.2 | 1.0 | 0.97 | 0.99 |
| utf16LEEncode | ascii | 82.0 | 341 | 1126 | 1426 |
| utf16LEEncode | latin | 86.9 | 359 | 1140 | 1463 |
| utf16LEEncode | wide | 45.6 | 184 | 346 | 435 |
| utf16LEDecode | ascii | 124 | 458 | 598 | 771 |
| utf16LEDecode | latin | 129 | 488 | 595 | 754 |
| utf16LEDecode | wide | 37.2 | 130 | 294 | 334 |
| base64Encode | ascii | 68.0 | 242 | 454 | 568 |
| base64Encode | latin | 68.0 | 242 | 444 | 566 |
| base64Encode | wide | 68.7 | 240 | 447 | 564 |
| base64Encode76 | ascii | 60.9 | 204 | 427 | 506 |
| base64Encode76 | latin | 61.6 | 207 | 426 | 503 |
| base64Encode76 | wide | 62.0 | 209 | 425 | 501 |
| base64Decode | ascii | 143 | 550 | 526 | 623 |
| base64Decode | latin | 145 | 558 | 531 | 621 |
| base64Decode | wide | 143 | 547 | 531 | 623 |

(`isAscii` of the latin and wide data stops at one of the first 8 characters: it measures a call, not a scan.)

MB/s at 1 MiB on Cog, and each backend over Zinc:

| op | data | Zinc | plugin | library | Smalltalk | plugin / Zinc | library / Zinc | Smalltalk / Zinc |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| utf8Encode | ascii | 61.2 | 2321 | 2124 | 657 | 37.9 | 34.7 | 10.7 |
| utf8Encode | latin | 40.7 | 1880 | 1752 | 54.9 | 46.2 | 43.0 | 1.3 |
| utf8Encode | wide | 42.9 | 938 | 826 | 51.1 | 21.9 | 19.2 | 1.2 |
| utf8Decode | ascii | 28.7 | 2329 | 2132 | 656 | 81.2 | 74.3 | 22.9 |
| utf8Decode | latin | 28.4 | 1293 | 430 | 43.9 | 45.6 | 15.1 | 1.5 |
| utf8Decode | wide | 28.1 | 708 | 626 | 32.3 | 25.2 | 22.3 | 1.2 |
| utf8SizeOf | ascii | 43.9 | 42759 | 39159 | 1871 | 974 | 892 | 42.6 |
| utf8SizeOf | latin | 46.9 | 57728 | 51297 | 156 | 1231 | 1094 | 3.3 |
| utf8SizeOf | wide | 65.8 | 4357 | 4316 | 142 | 66.2 | 65.6 | 2.2 |
| utf8CharacterCountOf | ascii | 28.7 | 42520 | 38463 | 1869 | 1482 | 1340 | 65.1 |
| utf8CharacterCountOf | latin | 28.4 | 5468 | 3854 | 87.5 | 193 | 136 | 3.1 |
| utf8CharacterCountOf | wide | 28.1 | 8910 | 5372 | 52.0 | 317 | 191 | 1.8 |
| isValidUTF8 | ascii | 28.7 | 44109 | 40264 | 1871 | 1539 | 1405 | 65.3 |
| isValidUTF8 | latin | 28.4 | 12085 | 11633 | 87.4 | 425 | 409 | 3.1 |
| isValidUTF8 | wide | 28.1 | 11246 | 10674 | 52.1 | 401 | 380 | 1.9 |
| isAscii | ascii | 1877 | 53585 | 46877 | 1872 | 28.6 | 25.0 | 1.00 |
| utf16LEEncode | ascii | 15.0 | 1230 | 1220 | 45.6 | 82.0 | 81.3 | 3.0 |
| utf16LEEncode | latin | 16.9 | 1465 | 1451 | 51.1 | 86.9 | 86.1 | 3.0 |
| utf16LEEncode | wide | 21.3 | 971 | 787 | 43.8 | 45.6 | 37.0 | 2.1 |
| utf16LEDecode | ascii | 13.0 | 1605 | 1446 | 24.7 | 124 | 112 | 1.9 |
| utf16LEDecode | latin | 14.5 | 1882 | 535 | 27.7 | 129 | 36.7 | 1.9 |
| utf16LEDecode | wide | 18.0 | 670 | 622 | 33.9 | 37.2 | 34.5 | 1.9 |
| base64Encode | ascii | 25.6 | 1744 | 738 | 44.7 | 68.0 | 28.8 | 1.7 |
| base64Encode | latin | 25.7 | 1748 | 737 | 44.7 | 68.0 | 28.7 | 1.7 |
| base64Encode | wide | 25.2 | 1731 | 738 | 44.5 | 68.7 | 29.3 | 1.8 |
| base64Encode76 | ascii | 24.0 | 1464 | 1451 | 38.0 | 60.9 | 60.4 | 1.6 |
| base64Encode76 | latin | 24.0 | 1480 | 1467 | 38.0 | 61.6 | 61.1 | 1.6 |
| base64Encode76 | wide | 23.6 | 1465 | 1452 | 38.0 | 62.0 | 61.4 | 1.6 |
| base64Decode | ascii | 13.4 | 1924 | 1607 | 20.4 | 143 | 120 | 1.5 |
| base64Decode | latin | 13.4 | 1947 | 1622 | 20.3 | 145 | 121 | 1.5 |
| base64Decode | wide | 13.3 | 1910 | 1596 | 20.4 | 143 | 120 | 1.5 |

One call on the 52-byte path:

| op | Cog Zinc | plugin | library | Smalltalk | Stack Zinc | plugin | Smalltalk | node Zinc | plugin | Smalltalk |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| utf8Encode | 1.12 µs | 176 ns | 1.70 µs | 496 ns | 3.42 µs | 304 ns | 1.20 µs | 15.50 µs | 1.16 µs | 7.75 µs |
| utf8Decode | 1.94 µs | 208 ns | 1.72 µs | 575 ns | 6.93 µs | 368 ns | 1.45 µs | 31.74 µs | 1.41 µs | 11.11 µs |
| utf8SizeOf | 1.27 µs | 149 ns | 800 ns | 326 ns | 5.00 µs | 281 ns | 845 ns | 19.78 µs | 1.24 µs | 5.31 µs |
| utf8CharacterCountOf | 1.94 µs | 168 ns | 815 ns | 378 ns | 6.58 µs | 334 ns | 997 ns | 31.98 µs | 1.34 µs | 6.59 µs |
| isValidUTF8 | 1.95 µs | 145 ns | 791 ns | 327 ns | 6.55 µs | 287 ns | 849 ns | 32.23 µs | 1.21 µs | 4.64 µs |
| isAscii | 109 ns | 144 ns | 768 ns | 297 ns | 129 ns | 321 ns | 783 ns | 427 ns | 1.28 µs | 3.63 µs |
| utf16LEEncode | 3.74 µs | 177 ns | 1.70 µs | 1.40 µs | 14.85 µs | 315 ns | 6.08 µs | 68.36 µs | 1.26 µs | 31.25 µs |
| utf16LEDecode | 4.19 µs | 258 ns | 1.79 µs | 2.56 µs | 15.25 µs | 472 ns | 10.31 µs | 68.85 µs | 1.85 µs | 55.18 µs |
| base64Encode | 2.44 µs | 190 ns | 1.47 µs | 1.55 µs | 8.52 µs | 346 ns | 6.10 µs | 37.60 µs | 1.30 µs | 28.81 µs |
| base64Encode76 | 2.69 µs | 196 ns | 1.54 µs | 1.78 µs | 9.10 µs | 351 ns | 6.33 µs | 42.24 µs | 1.35 µs | 30.76 µs |
| base64Decode | 4.32 µs | 267 ns | 1.79 µs | 3.07 µs | 16.41 µs | 424 ns | 11.41 µs | 76.66 µs | 1.59 µs | 63.96 µs |

Observations:

- The plugin is worth its call at every size: 150 to 270 ns for 52 bytes on Cog, where Zinc takes 1.1 to 4.3 µs, and in WebAssembly 1.2 to 1.9 µs against Zinc's 15 to 77 µs.
- The gains grow as the VM gets slower, since Zinc and the Smalltalk backend run as Smalltalk code and the plugin as C: at 1 MiB, 22 to 145x on Cog for a conversion, 74 to 558x on Stack, 121 to 1463x in WebAssembly, where simdutf's `fallback` kernel still converts 250 MB/s to 2.9 GB/s.
- The measures and validations that allocate nothing (`utf8SizeOf`, `utf8CharacterCountOf`, `isValidUTF8`) run at 4 to 58 GB/s natively. A conversion of 1 MiB natively is bound by its result rather than by simdutf: `utf8Encode` of ASCII text takes 0.45 ms on Cog, most of which is the allocation and zero-filling of a 1 MiB object in old space, and the writing of it (simdutf validates the same input in 24 µs).
- The library is as fast as the plugin on most conversions of 1 MiB and 10 MiB, but its results are made in Smalltalk, pinned (`basicNewPinned:`), and the allocation of a pinned object in old space can cost more than the conversion: some series (`utf8Decode` and `utf16LEDecode` of latin text, `base64Encode` without lines) took 2.4 to 3.5 times the plugin's time at 1 MiB, and up to 9 times at 64 KiB, in both runs; `ByteString basicNewPinned: 87384` alone took 0.19 ms in a loop, where the conversion into an existing String took 7 µs.
- The Smalltalk backend is close to Zinc on non-ASCII text (1.2 to 3.3x at 1 MiB on Cog), which it reads a character at a time as Zinc does, and 10 to 65x faster on ASCII text in UTF-8, which it copies or checks with primitives of the VM.

### The threshold of the library

`SimdUTFLibrary minimumUnits` is where the library becomes faster than the Smalltalk backend, which the benchmark measures with `SU_BACKENDS=ffi,smalltalk SU_SIZES=16,32,64,128,256,512,1024,2048,4096 SU_DATA=ascii,latin,wide` on Cog (`summarize.py` prints the table "Where the library overtakes the Smalltalk backend"):

| op | ascii | latin | wide |
|---|---:|---:|---:|
| `utf8Encode` | - | 114 | 83 |
| `utf8Decode` | - | 64 | 64 |
| `utf8SizeOf` | 1024 | 114 | 83 |
| `utf8CharacterCountOf` | 1024 | 64 | 32 |
| `isValidUTF8` | 1024 | 64 | 32 |
| `isAscii` | 2048 | - | - |
| `utf16LEEncode` | 64 | 114 | 42 |
| `utf16LEDecode` | 128 | 114 | 88 |
| `base64Encode` | 64 | 64 | 64 |
| `base64Encode76` | 64 | 64 | 64 |
| `base64Decode` | 44 | 44 | 44 |

(Units from which the library is at least as fast at every larger size of the sweep; - when it is not: for `utf8Encode` and `utf8Decode` of ASCII text it is from 1024 units, then slower again at 2048 and 4096, where its pinned results cost more, see the observations above; `isAscii` of the latin and wide data answers at one of the first 8 characters.)

The library costs about 0.8 µs a call for a measure or a validation and 1.5 to 1.8 µs for a conversion (two callouts, the pinning, the result), whatever the size; the Smalltalk backend costs about 20 ns a character of mixed text and 1 to 2 ns a character of ASCII text in UTF-8, which it handles with primitives of the VM.
So the library is faster from 32 to 128 units on mixed text and on every UTF-16 and base64 conversion, and from about 1 Ki units of pure ASCII in UTF-8.
`minimumUnits` = T takes the library from T units on; over the series of the sweep, the time the facade then takes, over the faster of the two backends:

| T | at worst | on average |
|---:|---:|---:|
| 16 | 3.68x | 1.249x |
| 32 | 3.54x | 1.158x |
| 64 | 3.40x | 1.113x |
| 88 | 3.40x | 1.107x |
| 128 | 3.40x | 1.119x |
| 256 | 3.84x | 1.184x |
| 512 | 7.32x | 1.320x |
| 1024 | 13.46x | 1.549x |
| 4096 | 41.43x | 2.406x |

The default stays **64**: within 1% of the best threshold measured (88), it loses at most a factor of about 3 on short ASCII text in UTF-8 and on `isAscii` of text whose first non-ASCII character comes early (where the library's 0.8 µs meets the Smalltalk backend's early return), and it gives the library mixed text from 64 characters on, which up to the crossovers of the table above (at most 114) costs at most 10% more than the Smalltalk backend.

The sources of the benchmark are made once and reused, so after their first call they are old and pinning them costs nothing.
A young source, as a String just made is, cost 17 µs more a call with the library of simdutf.st b51efd3, whose `with:address:` passed the forwarder that pinning leaves to `PointerUtils oopForObject:`, whose primitive then failed and was retried once the VM followed it; `with:address:` now follows the forwarder first (`anObject yourself`), so a young source costs 0.6 µs more, and 64 is still the best threshold (measured on fresh copies of 64 B to 16 KiB; with the 17 µs, it would have been 1024).

## Licences

- simdutf is under the Apache License 2.0 or the MIT licence, at the user's choice: both texts are installed as `LICENSE.simdutf` and `LICENSE.simdutf-MIT`, next to the plugin on Linux and Windows (`build/vm`, the `lib` directory of the Linux zip) and in `Pharo.app/Contents/Resources` on macOS, and go into the `THIRD-PARTY-NOTICES.txt` of the WebAssembly VMs, with those of libc++ and libc++abi (Apache License 2.0 with LLVM exceptions), which the plugin links there.
- simdutf's detection of the instruction sets (`include/simdutf/internal/isadetection.h`) comes from PyTorch, under its BSD 3-Clause licence, whose notice the binaries must reproduce: `cmake/thirdPartyNotice.cmake` copies the comment that holds it, verbatim, into `LICENSE.simdutf-isadetection`, installed next to the other two and listed in the simdutf section of `THIRD-PARTY-NOTICES.txt`, and stops the configuration when a new version of simdutf moved it.
  simdjson carries the same notice, now installed as `LICENSE.simdjson-isadetection` (see docs/SimdJSON.md).
- simdutf's UTF-8 validation credits code of Fuchsia (`include/simdutf/scalar/utf8.h`), under the Apache License 2.0, which `LICENSE.simdutf` covers.
- The package `SimdUTF` is under the MIT licence of the submodule `simdutf.st` (its `LICENSE`); its Smalltalk backend is a port of simdutf's scalar code.

This is not legal advice.

## Limitations

- No replacement mode (U+FFFD): invalid input is an error.
- No streaming decoder: every conversion is of a whole buffer, and the stream forms of `SimdUTFBase64Encoder` read their input to its end first.
- No garbage-accepting base64, and no other white space than ASCII's.
- 64-bit VMs only.
- macOS, Windows and Linux aarch64 are untested until CI runs the steps that load the plugin; the performance on arm64 is not measured.
- A conversion is one primitive call: the web VM does not return to its page during it (see Memory and limits).
- Without the plugin, the library pays two callouts and the pinning of its objects per call: below `SimdUTFLibrary minimumUnits` units the Smalltalk backend answers instead (see Benchmarks).
