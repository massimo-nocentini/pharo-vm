# UtilsPlugin

The UtilsPlugin gives the image five small numerical and textual features, written in C:

- the **fused multiply-add** of Floats, `x * y + z` with one rounding (C's `fma`);
- the **splitting of strings** between delimiter characters (as `strtok_r`, and with empty fields);
- the **points of Bézier curves**, by a de Casteljau scheme;
- **Knuth's lagged Fibonacci generator** of random fractions (TAOCP 3.6, his `rng-double.c`, which the plugin runs unchanged);
- **Vigna's SplitMix64**, a generator of 64-bit integers (his `splitmix64.c`, which the plugin runs unchanged too).

The first four come from the UtilsPlugin of the `pharo-10` branch (2023–2024), which is ported here with the same module, primitive names and argument orders. The port checks every argument and fixes the old bugs (see [Changes from pharo-10](#changes-from-pharo-10)). SplitMix64 is new in version 1.1 of the plugin, and follows the conventions of Knuth's generator.

The Smalltalk side is the package `VM-Plugins-Primitives` of [massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st), the submodule `plugins/UtilsPlugin/vm-plugins-primitives.st`. Each of its methods tries the plugin first. On a VM without it, or when a primitive fails, the method answers through a Smalltalk twin (`UtilsPluginSmalltalk`), which gives the same result bit for bit (NaN payloads aside), or signals the error. So the code runs on any VM, and the plugin only makes it faster.

## Files

| Path | What |
|---|---|
| `plugins/UtilsPlugin/src/common/UtilsPlugin.c` | the VM glue: the module functions and the 10 primitives |
| `plugins/UtilsPlugin/src/engine/upEngine.c`, `include/common/upEngine.h` | the engine, without any VM header: fma, the splitter, the Bézier scheme, the wrappers of Knuth's generator and of SplitMix64 |
| `plugins/UtilsPlugin/src/knuth/rng-double.c` | Knuth's file, unchanged (SHA256 `e8bc0a86…8c46fed`) |
| `plugins/UtilsPlugin/src/vigna/splitmix64.c` | Vigna's file, unchanged (SHA256 `675db831…8f23a45b`) |
| `plugins/UtilsPlugin/tests/` | the C tests (`run-c-tests.sh`, `enginetest.c`, `vignaref.c`), the Smalltalk loader and runner (`load-utils.st`, `run-st-tests.sh`), the CI probe (`ci-load.st`) |
| `plugins/UtilsPlugin/vm-plugins-primitives.st` | the submodule with the Smalltalk code |
| `tests/wasm/lanes/73-utils.sh`, `tests/wasm/st/utils-lane.st` | the WebAssembly lane |

## Building

`cmake/plugins.cmake` builds the plugin when `FEATURE_PLUGIN_UTILS` is on, as it is by default on every target. It is C99 with no dependency (libm for `fma`).

- **Native VMs:** a shared library, `libUtilsPlugin.so`, `.dylib` or `UtilsPlugin.dll`.
- **WebAssembly VMs:** the plugin is built in, and adds 13,337 bytes to `pharo-web.wasm` (3,718 with `gzip -9`; measured with and without `FEATURE_PLUGIN_UTILS` in one build tree).
- **Turning it off:** `-DFEATURE_PLUGIN_UTILS=OFF`.

The engine is compiled with `-ffp-contract=off`, so the compiler fuses no multiply and add on its own. Every rounding is the one written in `up_bezier`, which the Smalltalk twin repeats.

## Loading

```smalltalk
Metacello new
	baseline: 'VMPluginsPrimitives';
	repository: 'github://massimo-nocentini/vm-plugins-primitives.st:master/src';
	load
```

`BaselineOfVMPluginsPrimitives` has the groups `Core` (`VM-Plugins-Primitives`), `Tests` (`VM-Plugins-Primitives-Tests`) and `default` (both). From this repository, `plugins/UtilsPlugin/tests/load-utils.st` loads the submodule with Metacello (see [Tests](#tests)).

## API

| Message | Answer |
|---|---|
| `aFloat times: y plus: z` | `self * y asFloat + z asFloat`, rounded once: the Float nearest to the exact value, ties to even |
| `aNumber times: y plus: z` | `self * y + z` (exact for Integers and Fractions; `Float` overrides it) |
| `aString strtok: delimiters inclusive: false` | the maximal runs of characters that are not in `delimiters`, as `strtok_r(3)` |
| `aString strtok: delimiters inclusive: true` | every field between two delimiters, empty ones included, but an empty last one |
| `aString linesIncludeEmptySubstrings: aBoolean` | the lines of `#lines` (CR, LF and CR LF each end a line), without the empty ones unless `aBoolean` |
| `points bezierPointsAt: parameters` | an Array of the points of the Bézier curve of the control `points` at each parameter |
| `points bezierPointAt: t` | one of them |
| `UtilsPlugin deCasteljau: controlPoints into: curvePoints at: parameters` | stores the curve points in `curvePoints` (which may be `controlPoints`), answers `curvePoints` |
| `KnuthRandom seed: anInteger`, `KnuthRandom new` | Knuth's generator, a `Random`: `next`, `next:`, `nextInteger:`, `nextBetween:and:`, `shuffleBy:`… |
| `SplitMix64 seed: anInteger`, `SplitMix64 new` | Vigna's generator, a `Random` too, with the same protocol |
| `aSplitMix64 nextInteger64` | the next output, an Integer in [0, 2^64) |
| `aSplitMix64 next` | the top 53 bits of the next output times 2^-53, a Float in [0, 1) |
| `aSplitMix64 fill: aDoubleWordArray` | stores the next outputs in its elements, in order, and answers it |
| `aSplitMix64 nextIntegers64: n` | a new DoubleWordArray of the next n outputs |
| `UtilsPlugin isAvailable`, `UtilsPlugin info` | whether the VM has the plugin; `{ protocol. version. platform }` or nil |
| `UtilsPlugin pluginHidden: aBoolean` | with true, everything takes the Smalltalk way, as on a VM without the plugin (for tests and benchmarks) |

`delimiters` is a String (or Symbol), a ByteArray of character values, or any collection of Characters. The pieces are new Strings, ByteStrings for a ByteString receiver.

## Semantics

### Fused multiply-add

The result is that of C99's `fma` under IEEE 754 round-to-nearest, ties to even:

- **Signed zeros:** an exact zero is +0, except `(-0) + (-0)`. A non-zero result that rounds to zero keeps its sign.
- **Overflow:** a result beyond the largest Float is an infinity.
- **Infinities and NaNs** propagate as in IEEE 754. For example `1e308 * 10 + -inf` is `-inf`, where `1e308 * 10.0 + -inf` is a NaN.

The Smalltalk twin computes the exact value with Fractions, then rounds it with its own `floatNearest:`. It does not use `Integer>>#asFloat`, because Pharo 12's `LargeNegativeInteger>>#asFloat` does not always answer the nearest Float. The differential tests found such a case, which `testRoundingOfLargeIntegers` keeps.

### Splitting

Every character counts, NUL too.

- **Exclusive** (`inclusive: false`): the maximal runs of non-delimiters, never an empty piece.
- **Inclusive** (`inclusive: true`): every delimiter ends a field, so two adjacent delimiters make an empty field. A last field that is empty is dropped.
  - `'a␤'` answers `#('a')`, and `'a␤␤'` answers `#('a' '')`.
  - `''` answers `#()`, and `'␤'` answers `#('')`.
  - With the line ends as delimiters, these are the lines of a text, which is how `linesIncludeEmptySubstrings:` uses it.
- **CR LF:** `strtok:inclusive:` treats each character on its own, so `'a␍␊b'` split inclusively by CR and LF answers `#('a' '' 'b')`. `linesIncludeEmptySubstrings: true` answers `#lines` instead when the text has a CR LF.

### Bézier curves

The scheme is the one of the pharo-10 plugin. Q starts at P1; then for k from 1 to n−1:

- h := h · t(n−k) / (k(1−t) + h · t(n−k)), with h = 1 at first;
- Q := (1−h) Q + h Pk+1.

Each product-sum is fused. The points answered are new Points of Floats.

- **Exact values:** t = 0 answers the first control point, t = 1 the last one, and one control point answers itself.
- **Accuracy:** inside [0, 1] the result agrees with repeated linear interpolation to about 1e-11 relative (C tests, 100,000 random curves).
- **Outside [0, 1]** the curve is extrapolated. A zero divisor gives IEEE infinities and NaNs, never `ZeroDivide`; for example t = −1 on three points divides by zero.
- **Coordinates and parameters** may be any Numbers: they are taken `asFloat`.

### Knuth's generator

`KnuthRandom` is Knuth's `ranf_start` and `ranf_arr_next`. Each cycle calls `ranf_array(buffer, 1009)` and answers the first 100 fractions of the buffer; every fraction is in [0, 1).

- **Seeds:** only the low 30 bits of the seed count, as Knuth's `seed & 0x3fffffff`, and `seed` answers them.
- **Default stream:** `UtilsPlugin newKnuthState` is seeded with 314159, the seed Knuth's code uses when nothing seeded it.
- **`nextInteger: n`** is `(next × n) truncated + 1`, computed with the fraction's exact value, so it stays correct for any n.

The state is the `state` of `Random`: `{ ByteArray of the 100 doubles of ran_u. ByteArray of the 1009 doubles of the buffer. index of the next fraction }`, in the platform's byte order. It is plain objects, so a generator survives an image save, and a state may move between the plugin and the Smalltalk twin.

Knuth's own check holds through the plugin and through the twin: after `ranf_start(310952)` and 2009 cycles, `ran_u[0]` is `0.36410514377569680455`.

### SplitMix64

`SplitMix64` is the `next()` of Sebastiano Vigna's `splitmix64.c` (2015), a fixed-increment version of Java 8's `SplittableRandom`. Its state is one 64-bit integer x. Each output adds the odd constant `0x9e3779b97f4a7c15` to x, modulo 2^64, and answers a mix of the new x (two xorshift-multiplies and a xorshift). It passes BigCrush.

- **A permutation:** x moves by an odd constant and the mix is a bijection, so the 2^64 outputs of a period are the 2^64 integers of 64 bits, each once. A random sequence of 2^32 such values repeats one with a probability of about 40%; SplitMix64 never does, so it fails the tests based on collisions. For the same reason Vigna recommends it to fill the state of another generator from a seed: consecutive outputs never hold the same value twice, and are never all zero.
- **Seeds:** any integer, modulo 2^64 (Vigna: "the state can be seeded with any value"); `seed` answers it. A seed is the state itself: the first output of the seed s is the mix of s + 0x9e3779b97f4a7c15, the n-th that of s + n × 0x9e3779b97f4a7c15, and from 2^64 − 0x9e3779b97f4a7c15 the first state is 0, whose output is 0.
- **`nextInteger64`** answers the output as an Integer in [0, 2^64): a SmallInteger below 2^60, else a LargePositiveInteger.
- **`next`** answers the top 53 bits of the output times 2^-53, a Float in [0, 1). Every such value is a multiple of 2^-53, so the Float is exact (and a SmallFloat64 on 64-bit VMs); 0 gives +0.0.
- **`nextInteger: n`** is the output times n, divided by 2^64 and truncated, plus 1: the 64 bits of the output, for any n. `privateNextValue`, which `Random` uses, is its top 31 bits.
- **`fill:`** stores the next outputs in the elements of a DoubleWordArray, in order, with one primitive call (1.5 ns an output); `nextIntegers64: n` makes the DoubleWordArray. Use it to seed other generators.

The state is the `state` of `Random`, a DoubleWordArray of 1, as `Random` has in Pharo 12 to 15: its element is x, `state first` answers it, and it survives an image save. A state may move between the plugin and the Smalltalk twin, as Knuth's does.

The choices, against Knuth's generator, whose conventions it follows:

- **Vigna's file is included unchanged** (`src/vigna/splitmix64.c`, checked by its SHA256), by `upEngine.c`, which renames its global `x` and its `next` from outside (`#define`), with hidden visibility, as it does for Knuth's file; `up_splitmix64_next` copies a generator's state into x, calls `next()` and copies x back. The file declares `uint64_t next()` without a prototype: a call with no argument compiles without a warning under the plugin's flags (`-Wall`, and `-Wextra` in the C tests), with clang, gcc and emcc; in C23 it is a prototype.
- **No primitive seeds the state:** seeding stores the seed, in Smalltalk (`setStateFromSeed`). The plugin keeps no state between calls.
- **The state is Random's own kind of state**, a DoubleWordArray, read and written in place by the primitives, which take it as their argument, like `primitive_randomknuth_next`. The bytes of x are in the platform's order, as for any DoubleWordArray.
- **Three primitives:** one output as an Integer, one output as a fraction (no LargePositiveInteger is made for `next`), and a fill of a DoubleWordArray. The Integer is made by the glue (a SmallInteger, or a LargePositiveInteger allocated with `instantiateClassindexableSize`), not by the proxy's `positive64BitIntegerFor`, which allocates in eden only and aborts the VM when eden is full.

## Primitive contract

Module `UtilsPlugin`, protocol 1 (`primitive_info`), version `UtilsPlugin 1.1`: version 1.1 adds the three primitives of SplitMix64, and changes none of the others. With a plugin of version 1.0 they fail, and `SplitMix64` answers through its Smalltalk twin.

The primitives check all their arguments before writing anything: the argument count, then the kinds, the sizes and read-only. They fail with these codes:

- `#'bad number of arguments'`;
- `#'bad receiver'` and `#'bad argument'`;
- `#'no modification'` when an argument they would write is read-only;
- `#'insufficient object memory'` when an allocation failed (the VM retries after a GC);
- `#'insufficient C memory'`.

The image's methods then answer through the Smalltalk twin. A forwarder is not of the kind expected; the accessor depths make the VM follow the forwarders and retry.

| Primitive | Receiver, arguments | Answer | Depth |
|---|---|---|---|
| `primitive_info` | – | `{ 1. 'UtilsPlugin 1.1'. 'native' or 'emscripten' }` | 0 |
| `primitive_fma` | a Float; Floats `y`, `z` | the Float `fma(self, y, z)` | 0 |
| `primitive_strtok_r` | a byte string; a byte string or ByteArray `delimiters`; a Boolean | an Array of new ByteStrings | 0 |
| `primitive_decasteljau` | any; an Array of Points (coordinates Floats or SmallIntegers), non-empty; a writable Array at least as large as the parameters; an Array of Floats or SmallIntegers | the receiver, with the new Points stored | 2 |
| `primitive_randomknuth_init` | – | a new state seeded with 314159 | 0 |
| `primitive_randomknuth_start` | an object whose slot 1 is a state and slot 2 a SmallInteger seed (a `KnuthRandom`) | the receiver, its state seeded, with an empty buffer | 2 |
| `primitive_randomknuth_next` | any; a state | the next fraction, a Float; the state advanced | 1 |
| `primitive_splitmix64_next` | any; a SplitMix64 state | the next output, an Integer in [0, 2^64); the state advanced | 0 |
| `primitive_splitmix64_nextfloat` | any; a SplitMix64 state | the top 53 bits of the next output times 2^-53, a Float in [0, 1); the state advanced | 0 |
| `primitive_splitmix64_fill` | any; a SplitMix64 state; a writable DoubleWordArray of at least one element, not the state | the DoubleWordArray, its elements the next outputs in order; the state advanced as many times | 0 |

A state of Knuth's generator is valid when:

- it is an Array of 3;
- it holds byte objects of 800 and 8072 bytes;
- its cursor is a SmallInteger in 1..101;
- all three are writable.

A state of SplitMix64 is valid when it is a writable DoubleWordArray of one element. The proxy has no test of an object's format: the glue takes the objects of bits that are neither bytes nor 32-bit words and have 8 bytes per element, which are those of 64-bit elements but for an empty one (an empty array of 16-bit elements has 0 bytes per element too), so `primitive_splitmix64_fill` refuses an empty DoubleWordArray; `SplitMix64>>fill:` then answers through the twin, which fills nothing. The array to fill and the state must be two objects.
Allocation uses only `instantiateClassindexableSize`, which falls back to old space when eden is full, plus immediates. A Float outside the SmallFloat64 range is a BoxedFloat64 the glue fills itself: the proxy's `floatObjectOf` allocates in eden only, and aborts the VM when eden is full. Spur collects no garbage inside a primitive, so the oops held in C stay valid.

When an allocation fails, two primitives may already have done part of their work:

- `primitive_decasteljau` may have stored some of its points;
- `primitive_randomknuth_next` leaves the state consistent: the retry answers the same fraction.

`primitive_splitmix64_next` and `primitive_splitmix64_nextfloat` advance the state only once their answer is made, so the retry answers the same output; `primitive_splitmix64_fill` allocates nothing.

Knuth's and Vigna's code keep their states in globals. The engine copies each generator's state into them before their functions run and out after: the VM runs primitives on one thread.

## Tests

### C

`sh plugins/UtilsPlugin/tests/run-c-tests.sh [--build <dir>] [--cc <compiler>]` first checks that `src/knuth/rng-double.c` is Knuth's file and `src/vigna/splitmix64.c` Vigna's (their SHA256). It then runs `enginetest.c` in two lanes:

- **`engine`:** with the plugin's flags;
- **`engine-asan`:** under AddressSanitizer and UBSan.

Each lane makes 3,680,642 checks (clang and gcc):

- **Knuth's generator:** against `rng-double.c` compiled on its own, with its globals and `ranf_arr_next`, for 10 seeds × 25,000 fractions, with a second generator interleaved; Knuth's own check value; reseeding; the 30-bit seed.
- **SplitMix64:** the known outputs of 4 seeds (0, whose first outputs are those of the file as it is, `0xe220a8397b1dcdaf`, `0x6e789e6aa1b965f4`, `0x06c45d188009454f`; 1234567, `0x599ed017fb08fc85`…; 2^64 − 1; 2^64 − gamma, where the state wraps to 0); 64 seeds × 20,000 outputs against `splitmix64.c` itself (`vignaref.c` includes it unchanged and sets its static x), with a second generator interleaved, and against the closed form, the n-th output is the mix of seed + n × gamma modulo 2^64; the fill against repeated outputs (200 random lengths, and 0); the fraction (exact, in [0, 1), +0 for 0, 1 − 2^-53 at most).
- **The splitter:** 19 fixed cases and NUL; 200,000 random strings against a model of each mode, and the exclusive mode against glibc's `strtok_r`.
- **The Bézier scheme:** against repeated linear interpolation on 100,000 random curves, and the end points exactly.
- **fma:** its special cases.

`enginetest --vectors` prints the reference values that the Smalltalk tests check: fractions of Knuth's generator, outputs and first fractions of SplitMix64.

### Smalltalk

`sh plugins/UtilsPlugin/tests/run-st-tests.sh <vm-dir> <image.zip>` copies the image, loads the groups `Core Tests` from the submodule with `load-utils.st`, and runs `VM-Plugins-Primitives-Tests`. The load checks for undeclared variables and one known answer per feature. The runner requires the image to find the plugin when the VM directory has it (`UP_EXPECT_PLUGIN`), so that its tests do not skip themselves.

The 88 tests include:

- **The features on both backends** (`#plugin` and `#smalltalk`, a `ParametrizedTestCase`): fma, splitting, Bézier curves, `KnuthRandom`, with Knuth's check and reference fractions, and `SplitMix64` (`UtilsPluginSplitMix64Test`), with the outputs 1, 2, 3, 100 and 1000 of 5 seeds, the fractions, the seeds modulo 2^64, the wrap of the state, the fill and the protocol of `Random`.
- **`UtilsPluginDifferentialTest`:** the primitives against their twins on 40,000 fma cases (every magnitude, near-cancellations, specials), 20,000 splits, 3,000 curves (inside and outside [0, 1], NaNs, infinities), 30 Knuth streams and 60 SplitMix64 streams of 300 outputs and fractions (seeds of every size) that move between the plugin and the twin, and their fills.
- **`UtilsPluginPrimitiveTest`:** the failures of every primitive, which leave their arguments alone, and the forwarders.

Measured on Linux x86-64:

- **With the plugin**, on Pharo 12, 13, 14 and 15: 88 tests pass.
- **Without it** (a VM built without the plugin), on Pharo 12 and 15: 88 tests pass, of which the 50 of the plugin skip.
- **A longer differential run:** 8,000,000 fma cases, 240,000 curve rounds and 800,000 splits, with no difference.

### CI and WebAssembly

**CI:** each native job of `build.yml` (Linux, macOS, Windows) runs `plugins/UtilsPlugin/tests/ci-load.st` on the VMMaker image with the VM of its zip, and greps `UtilsPlugin native 1 ok`. The probe compiles its own primitive methods and checks one known answer per primitive, Knuth's check and the outputs of SplitMix64 included.

**Lane 73-utils** of `make wasm-check` (see `docs/WebAssembly.md`) runs the same probe on the node VM, loads the package, and runs the suite in the node VM and in `pharo-web` (88 tests, none skipped).

## Speed

Measured on Cog (Pharo 12, Linux x86-64, an Intel Xeon Gold 6238R), the best of 3 runs:

| Operation | Plugin | Smalltalk twin | Pharo |
|---|---:|---:|---:|
| `1.1 times: x plus: 0.3` | 0.063 µs | 5.8 µs | 0.013 µs (`1.1 * x + 0.3`, two roundings) |
| `aKnuthRandom next` | 0.087 µs | 0.85 µs | 1.37 µs (`Random next`) |
| `aSplitMix64 next` | 0.063 µs | 1.47 µs | 1.35 µs (`Random next`) |
| `aSplitMix64 nextInteger64` | 0.082 µs | 1.33 µs | – |
| `aSplitMix64 fill:` a DoubleWordArray of 1000 | 1.47 µs | 1313 µs | – |
| split 100 KB of lorem ipsum by spaces | 0.97 ms | 5.8 ms | 7.5 ms (`substrings: ' '`) |
| `bezierPointsAt:` 8 points at 1001 parameters | 0.12 ms | 174 ms | – |

Notes on the table:

- **The twins of fma and of the Bézier scheme are slow:** they compute every fma exactly, with Fractions, so that they answer the plugin's bits.
- **Knuth's generator is faster than Pharo's `Random`**, through the plugin or not: it makes fractions in batches of 1009.
- **SplitMix64 through the plugin is 21 times as fast as `Random`**; its twin computes with LargeIntegers, as fast as `Random`. `nextInteger64` makes a LargePositiveInteger for 15 outputs in 16; `fill:` makes none.

## Changes from pharo-10

The pharo-10 plugin was hand-written C in `extracted/plugins/UtilsPlugin`. What changed:

- **Arguments are checked.** Every primitive validates its arguments and fails with a code instead of reading or writing out of bounds.
- **`primitive_strtok_r`:**
  - It no longer appends an empty string when the result has a single piece: `'abc'` answered `#('abc' '')`.
  - That code had also written past its buffer for one-character strings.
  - NUL is an ordinary character.
  - It no longer reads one byte past the receiver, does not use `strtok_r` (absent from MSVC), and allocates no C memory.
- **`primitive_decasteljau`** reads every control point before it stores anything, accepts SmallInteger coordinates and parameters, and checks the sizes.
- **`primitive_randomknuth_start`** resets the cursor, so a generator seeded again starts its stream over; it used to answer stale fractions of the old buffer.
- **`primitive_randomknuth_init`** seeds the new state with 314159, as Knuth's code does for an unseeded generator.
- **Knuth's code** is included unchanged, as his notice asks ("AS LONG AS YOU MAKE ABSOLUTELY NO CHANGES"), instead of a modified copy.
- **Not ported:**
  - the old in-VM timsort and merge sort (`primitive_timsort`), which the TimSortPlugin replaces (`docs/TimSort.md`);
  - `primitive_sqInt` and `primitive_sqObjectInt`;
  - the other plugins of that branch (Lua, Cairo, TreeSitter, Wolfram, Chicken Scheme, DateTimeFormatter).

The companion repository was rewritten around these four features: its old code for those other plugins was removed.

## Licences

The plugin and the Smalltalk code are under the MIT licence of this repository and of vm-plugins-primitives.st. `src/knuth/rng-double.c` is D. E. Knuth's, in the public domain, "freely copyable AS LONG AS YOU MAKE ABSOLUTELY NO CHANGES": it is kept unchanged (the C tests check its SHA256), and the Smalltalk twin is a translation of it, checked against it bit for bit. `src/vigna/splitmix64.c` is Sebastiano Vigna's, who dedicated it to the public domain (its notice is kept in the file, which is unchanged: the C tests check its SHA256); the Smalltalk twin translates it too.

## Limitations

- **NaN payloads:** NaNs are compared as NaNs, since their bits depend on the platform (x86's default NaN is negative).
- **The Smalltalk twins of fma and of the Bézier scheme** are exact but slow (Fractions): about 90 and 1500 times the plugin (see [Speed](#speed)).
- **One bulk primitive only:** `primitive_splitmix64_fill`; the other calls answer one fraction, one output or one fma.
- **Not run yet:** macOS, Windows and Linux arm64 build the plugin in CI. Their first CI run is the first test there, but the code is portable C99, with no assembly or OS call.
