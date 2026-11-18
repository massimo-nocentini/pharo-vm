# UtilsPlugin

The UtilsPlugin gives the image four small numerical and textual primitives, written in C:

- the **fused multiply-add** of Floats, `x * y + z` with one rounding (C's `fma`);
- the **splitting of strings** between delimiter characters (as `strtok_r`, and with empty fields);
- the **points of Bézier curves**, by a de Casteljau scheme;
- **Knuth's lagged Fibonacci generator** of random fractions (TAOCP 3.6, his `rng-double.c`, which the plugin runs unchanged).

They come from the UtilsPlugin of the `pharo-10` branch (2023–2024), which is ported here with the same module, primitive names and argument orders. The port checks every argument and fixes the old bugs (see [Changes from pharo-10](#changes-from-pharo-10)).

The Smalltalk side is the package `VM-Plugins-Primitives` of [massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st), the submodule `plugins/UtilsPlugin/vm-plugins-primitives.st`. Each of its methods tries the plugin first. On a VM without it, or when a primitive fails, the method answers through a Smalltalk twin (`UtilsPluginSmalltalk`), which gives the same result bit for bit (NaN payloads aside), or signals the error. So the code runs on any VM, and the plugin only makes it faster.

## Files

| Path | What |
|---|---|
| `plugins/UtilsPlugin/src/common/UtilsPlugin.c` | the VM glue: the module functions and the 7 primitives |
| `plugins/UtilsPlugin/src/engine/upEngine.c`, `include/common/upEngine.h` | the engine, without any VM header: fma, the splitter, the Bézier scheme, the wrapper of Knuth's generator |
| `plugins/UtilsPlugin/src/knuth/rng-double.c` | Knuth's file, unchanged (SHA256 `e8bc0a86…8c46fed`) |
| `plugins/UtilsPlugin/tests/` | the C tests (`run-c-tests.sh`, `enginetest.c`), the Smalltalk loader and runner (`load-utils.st`, `run-st-tests.sh`), the CI probe (`ci-load.st`) |
| `plugins/UtilsPlugin/vm-plugins-primitives.st` | the submodule with the Smalltalk code |
| `tests/wasm/lanes/73-utils.sh`, `tests/wasm/st/utils-lane.st` | the WebAssembly lane |

## Building

`cmake/plugins.cmake` builds the plugin when `FEATURE_PLUGIN_UTILS` is on, as it is by default on every target. It is C99 with no dependency (libm for `fma`).

- **Native VMs:** a shared library, `libUtilsPlugin.so`, `.dylib` or `UtilsPlugin.dll`.
- **WebAssembly VMs:** the plugin is built in, and adds 10,472 bytes to `pharo-web.wasm` (2,040 gzipped).
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

## Primitive contract

Module `UtilsPlugin`, protocol 1 (`primitive_info`).

The primitives check all their arguments before writing anything: the argument count, then the kinds, the sizes and read-only. They fail with these codes:

- `#'bad number of arguments'`;
- `#'bad receiver'` and `#'bad argument'`;
- `#'no modification'` when an argument they would write is read-only;
- `#'insufficient object memory'` when an allocation failed (the VM retries after a GC);
- `#'insufficient C memory'`.

The image's methods then answer through the Smalltalk twin. A forwarder is not of the kind expected; the accessor depths make the VM follow the forwarders and retry.

| Primitive | Receiver, arguments | Answer | Depth |
|---|---|---|---|
| `primitive_info` | – | `{ 1. 'UtilsPlugin 1.0'. 'native' or 'emscripten' }` | 0 |
| `primitive_fma` | a Float; Floats `y`, `z` | the Float `fma(self, y, z)` | 0 |
| `primitive_strtok_r` | a byte string; a byte string or ByteArray `delimiters`; a Boolean | an Array of new ByteStrings | 0 |
| `primitive_decasteljau` | any; an Array of Points (coordinates Floats or SmallIntegers), non-empty; a writable Array at least as large as the parameters; an Array of Floats or SmallIntegers | the receiver, with the new Points stored | 2 |
| `primitive_randomknuth_init` | – | a new state seeded with 314159 | 0 |
| `primitive_randomknuth_start` | an object whose slot 1 is a state and slot 2 a SmallInteger seed (a `KnuthRandom`) | the receiver, its state seeded, with an empty buffer | 2 |
| `primitive_randomknuth_next` | any; a state | the next fraction, a Float; the state advanced | 1 |

A state is valid when:

- it is an Array of 3;
- it holds byte objects of 800 and 8072 bytes;
- its cursor is a SmallInteger in 1..101;
- all three are writable.

Allocation uses only `instantiateClassindexableSize`, which falls back to old space when eden is full, plus immediates. A Float outside the SmallFloat64 range is a BoxedFloat64 the glue fills itself: the proxy's `floatObjectOf` allocates in eden only, and aborts the VM when eden is full. Spur collects no garbage inside a primitive, so the oops held in C stay valid.

When an allocation fails, two primitives may already have done part of their work:

- `primitive_decasteljau` may have stored some of its points;
- `primitive_randomknuth_next` leaves the state consistent: the retry answers the same fraction.

Knuth's code keeps its state in globals. The glue copies each generator's state into them before his functions run and out after: the VM runs primitives on one thread.

## Tests

### C

`sh plugins/UtilsPlugin/tests/run-c-tests.sh [--build <dir>] [--cc <compiler>]` first checks that `src/knuth/rng-double.c` is Knuth's file (its SHA256). It then runs `enginetest.c` in two lanes:

- **`engine`:** with the plugin's flags;
- **`engine-asan`:** under AddressSanitizer and UBSan.

Each lane makes 1,020,351 checks (clang and gcc):

- **Knuth's generator:** against `rng-double.c` compiled on its own, with its globals and `ranf_arr_next`, for 10 seeds × 25,000 fractions, with a second generator interleaved; Knuth's own check value; reseeding; the 30-bit seed.
- **The splitter:** 19 fixed cases and NUL; 200,000 random strings against a model of each mode, and the exclusive mode against glibc's `strtok_r`.
- **The Bézier scheme:** against repeated linear interpolation on 100,000 random curves, and the end points exactly.
- **fma:** its special cases.

`enginetest --vectors` prints the reference fractions that the Smalltalk tests check.

### Smalltalk

`sh plugins/UtilsPlugin/tests/run-st-tests.sh <vm-dir> <image.zip>` copies the image, loads the groups `Core Tests` from the submodule with `load-utils.st`, and runs `VM-Plugins-Primitives-Tests`. The load checks for undeclared variables and one known answer per feature. The runner requires the image to find the plugin when the VM directory has it (`UP_EXPECT_PLUGIN`), so that its tests do not skip themselves.

The 70 tests include:

- **The features on both backends** (`#plugin` and `#smalltalk`, a `ParametrizedTestCase`): fma, splitting, Bézier curves and `KnuthRandom`, with Knuth's check and reference fractions.
- **`UtilsPluginDifferentialTest`:** the primitives against their twins on 40,000 fma cases (every magnitude, near-cancellations, specials), 20,000 splits, 3,000 curves (inside and outside [0, 1], NaNs, infinities), and 30 Knuth streams that move between the plugin and the twin.
- **`UtilsPluginPrimitiveTest`:** the failures of every primitive, which leave their arguments alone, and the forwarders.

Measured on Linux x86-64:

- **With the plugin**, on Pharo 12, 13, 14 and 15: 70 tests pass.
- **Without it** (a VM built without the plugin), on Pharo 12 and 15: 70 tests pass, with the plugin's tests skipped.
- **A longer differential run:** 8,000,000 fma cases, 240,000 curve rounds and 800,000 splits, with no difference.

### CI and WebAssembly

**CI:** each native job of `build.yml` (Linux, macOS, Windows) runs `plugins/UtilsPlugin/tests/ci-load.st` on the VMMaker image with the VM of its zip, and greps `UtilsPlugin native 1 ok`. The probe compiles its own primitive methods and checks one known answer per primitive, Knuth's check included.

**Lane 73-utils** of `make wasm-check` (see `docs/WebAssembly.md`) runs the same probe on the node VM, loads the package, and runs the suite in the node VM and in `pharo-web` (70 tests, none skipped).

## Speed

Measured on Cog (Pharo 12, Linux x86-64, an Intel Xeon Gold 6238R), the best of 3 runs:

| Operation | Plugin | Smalltalk twin | Pharo |
|---|---:|---:|---:|
| `1.1 times: x plus: 0.3` | 0.063 µs | 5.8 µs | 0.013 µs (`1.1 * x + 0.3`, two roundings) |
| `aKnuthRandom next` | 0.087 µs | 0.85 µs | 1.37 µs (`Random next`) |
| split 100 KB of lorem ipsum by spaces | 0.97 ms | 5.8 ms | 7.5 ms (`substrings: ' '`) |
| `bezierPointsAt:` 8 points at 1001 parameters | 0.12 ms | 174 ms | – |

Notes on the table:

- **The twins of fma and of the Bézier scheme are slow:** they compute every fma exactly, with Fractions, so that they answer the plugin's bits.
- **Knuth's generator is faster than Pharo's `Random`**, through the plugin or not: it makes fractions in batches of 1009.

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

The plugin and the Smalltalk code are under the MIT licence of this repository and of vm-plugins-primitives.st. `src/knuth/rng-double.c` is D. E. Knuth's, in the public domain, "freely copyable AS LONG AS YOU MAKE ABSOLUTELY NO CHANGES": it is kept unchanged (the C tests check its SHA256), and the Smalltalk twin is a translation of it, checked against it bit for bit.

## Limitations

- **NaN payloads:** NaNs are compared as NaNs, since their bits depend on the platform (x86's default NaN is negative).
- **The Smalltalk twins of fma and of the Bézier scheme** are exact but slow (Fractions): about 90 and 1500 times the plugin (see [Speed](#speed)).
- **No bulk primitive:** a call answers one fraction or one fma.
- **Not run yet:** macOS, Windows and Linux arm64 build the plugin in CI. Their first CI run is the first test there, but the code is portable C99, with no assembly or OS call.
