# LargeIntegers plugin tests

Tests and benchmark of the LargeIntegers plugin: the multiply and divide kernels
(`src/common/LargeIntegersKernels.c`), the glue that calls them
(`src/common/LargeIntegers.c`, from `smalltalksrc/VMMaker/LargeIntegersPlugin.class.st`),
and the crashes and undefined behaviour the rewrite removed.  The reference everywhere is
the plugin as it was before the kernels: every primitive must answer what it answered
(class, byte size, bytes, receiver identity, failure code), except where it used to crash
or read memory it did not own.

| entry point | needs | checks | time |
|---|---|---|---|
| `run-c-tests.sh [scale]` | a C compiler, python3 | kernels against frozen loops and oracles; the plugin through a mock VM | ~30 s (scale 1), ~3 min (scale 10) |
| `run-image-tests.sh <vm-dir> <image> [reference-vm-dir]` | a built VM, a Pharo 12 image, python3 | repros, differential drivers, SUnit, in the real VM | ~10 s |
| `bench.sh <vm-dir> <image> [other-vm-dir]` | the same | ns per call, one or two VMs | 20-30 s per run and VM |

All three work from any directory, write nothing outside a temporary directory (except
`--record`, below) and use only sh, a C compiler (cc, clang or gcc) and the python3
standard library (run as `python3 -I`).  They exit 0 when every check passed.

## C tests: `run-c-tests.sh`

No VM, image or configured build: the plugin's C files are compiled with the repository's
VM headers, and `c/include/config.h` and `c/include/interp.h` stand in for the two headers
a build generates (type widths and byte order; the proxy version and primitive error codes
of the 64-bit Spur `interp.h`).

- **kernels** (`c/kernels_test.c`): `liDigitMultiply` and `liDigitDiv` digit for digit
  against frozen copies of the 32-bit loops the plugin ran before (kept in the test, so that
  the copies in `LargeIntegersKernels.c` cannot change both sides), and against a base-256
  schoolbook multiply and the reconstruction `a = q*b + r` with known `q` and `r`.
  Covered: the Moller-Granlund reciprocal (3M divisors and edges against the exact
  division), random small sizes, Karatsuba forced at thresholds 2..9 limbs, sizes around
  the production threshold with every partial-limb offset, squares, empty operands, product
  lengths n+m and n+m-1, result buffers full of garbage, sizes up to 10000 digits, a failing
  malloc (the 32-bit loop runs instead), adversarial digits (0, 1, FFFFFFFF, 80000000, ...),
  top dividend digits equal to the divisor's.  Squares, which take their own kernels, also
  against the base-256 multiply: one pointer passed twice (x * x) and a copy (equal digits
  at two places), every size around both squaring thresholds, one and two levels of
  Karatsuba squaring above them and the product's threshold, with every partial-limb
  offset; Karatsuba squaring forced at 2..9 limbs and squaring from 1..4 limbs, up to 200
  digits; top digit FFFFFFFF, 80000000 or 1, up to two zero digits above it
  (unnormalized); sizes up to 10000 digits; a failing malloc; operands equal but for one
  digit (a product, not a square); operands untouched.  Every random suite runs twice, with and
  without the dispatch to the 32-bit loops for the smallest operands.  Eight builds: the
  host's limbs (64-bit on x86_64 and aarch64), forced 32-bit limbs, and both with digits
  stored byte-swapped as on a big-endian host (`FAKE_BIGENDIAN`), under ASan+UBSan (when the
  compiler can, UBSan reports fatal); both limb sizes with `LI_STACK_LIMBS=1`, so that
  every scratch buffer is malloc'ed at the size the kernels compute and ASan checks that
  bound, under ASan+UBSan; both limb sizes again at -O2.
- **kernels, recursive division** (same program and builds): Burnikel & Ziegler's
  division against the frozen loop (quotient and the whole remainder buffer) and the
  reconstruction.  Divisors of `LI_BZ_LIMBS` - 1, `LI_BZ_LIMBS`, + 1, 2x - 1, 2x, 2x + 1,
  3x + 2 and 5x + 3 limbs, quotients of 1, 2, vn - 1, vn, vn + 1, 3vn + 5 and 10vn limbs,
  every partial-limb offset of both; short quotients of `LI_BZ_QUOTIENT_LIMBS` - 1, + 0,
  + 1 limbs, and divisors that many limbs longer than the quotient, over divisors of 2
  `LI_BZ_QUOTIENT_LIMBS` - 1 to 3 `LI_BZ_LIMBS` limbs; the recursion forced at thresholds
  2..9 limbs (short quotients from 1..6, Karatsuba sometimes at 2..6) on 40,000 small
  operands; operands up to 20,000 digits; dividends `q*v + r` built to make the estimates
  too large: divisors 80000000 over zeros or FFFFFFFF digits, all FFFFFFFF, ones in the
  low half; quotients of all-ones digits; `r = v - 1`, 0, `v - small`.  `liDivRemBZ` also
  called directly, on limbs, with the contract of Knuth's algorithm D (the dividend's top
  vn limbs below the divisor) rather than `liDigitDiv`'s (its top limb below the
  divisor's): 10,000 dividends whose top limbs equal the divisor's down to a lower limb,
  half of them a top block whose quotient reaches B^qb and is corrected, a path
  `liDigitDiv` never takes; against Knuth's algorithm D and `u = q v + r`, `r < v`.  A test
  hook (`LI_DIV_ADDBACK`) counts the add-backs of each correction loop: every loop must
  run, some twice, none three times (at scale 1 about 6,800 (64-bit limbs) and 6,100
  (32-bit limbs) add-backs at the production thresholds, 280,000 and 535,000 forced, and
  63,000 and 65,000 in the direct calls).  A failing malloc for the recursion's scratch
  alone (Knuth's algorithm D would fit on the stack): the 32-bit loop.  The scratch:
  `LI_STACK_LIMBS` limbs on the stack, one more malloc'ed, and a size larger than a
  `size_t` holds answered with no malloc (the 32-bit loop runs instead).
- **oracle**: the -O2 builds print 3000 products, 3000 squares (sizes around the squaring
  thresholds of both limb sizes, up to 2500 digits) and 4000 quotients and remainders
  (1000 of them over the Burnikel & Ziegler threshold, a third adversarial), which
  `c/oracle.py` checks with Python's integers.
- **calls** (`c/plugin_harness.c calls`): the plugin as a shared object, driven through a
  mock `struct VirtualMachine` with Spur-shaped objects (byte bodies malloc'ed apart at their
  rounded size, so ASan sees any access outside an object): 84,773 calls of every primitive
  over SmallIntegers, normalized, unnormalized and empty LargeIntegers of 1 to 2100 bytes
  (both sides of the plugin's 128-digit stack buffers and of Karatsuba) and wrong types:
  nil and a Float as receivers and pair arguments; nil, true, a ByteArray, a Float and
  LargeIntegers as shift counts and anyBit bounds; those and negative or 33-bit
  SmallIntegers as each of Montgomery's receiver and three arguments.  Then 320 multiplies
  of a receiver by itself, one object (x * x, as `squared` sends it; the pairs above
  multiply equal digits held in two objects): the pool, and LargeIntegers of 19 to 709
  bytes around the squaring thresholds, of 1500 to 40,000 bytes, with top digits
  FFFFFFFF, 80000000, 1, all ones.
  The answers must match `expected/mock-calls.sha256`, one SHA-256 per section and
  primitive (see `digest.py`), recorded from the plugin before the kernels (the
  same-object section, added last, later, from the same source: the digests above it did
  not change).
- **ub**: the shapes that made that plugin write past an object (left shift of an
  unnormalized receiver), read the word below two empty operands (subtract) and read slot 0
  of an empty operand (Montgomery): right answers, no ASan report.
- **oom**: every allocation of 28 calls fails in turn (60 injections, each in a forked
  child): PrimErrNoMemory, the stack untouched, no crash.
- **arity**: every primitive at every wrong argument count 0..4, the caller's frame filled
  with a word that is not an oop: PrimErrBadNumArgs, the stack untouched, that word never
  dereferenced (180 calls).

`scale` (default 1, about 890,000 kernel cases per build) multiplies the random counts;
`run-c-tests.sh 10` runs about 8.8 million per build; the cases depend only on the scale,
not on the compiler.  `CC=gcc` or `CC=clang` picks the compiler; `--build DIR` keeps the
build and logs.  Each test program runs under `timeout` when there is one, for
`LI_TIMEOUT` seconds (300 x scale by default): a kernel that loops fails the run instead of
hanging it.

**The reference.**  The pre-kernel plugin is not kept in this directory.  Its answers are:
the digests of `calls` (and of the image drivers, below), the frozen loops, and the
expected outputs of the repros; the oracles do not depend on it.  To compare with it call by
call, give its source:

```
git show "$(git log -1 --format=%H --grep='sub-quadratic kernels' -- plugins/LargeIntegers)^:plugins/LargeIntegers/src/common/LargeIntegers.c" > /tmp/LargeIntegers-before.c
sh plugins/LargeIntegers/tests/run-c-tests.sh --reference-c /tmp/LargeIntegers-before.c
```

`calls` then also prints every call that differs, and `ub`, `oom` and `arity` run on the
reference, for information: it trips ASan on all three ub shapes, crashes on 26 of the oom
injections and fails all 180 arity calls.  `--record` (with `--reference-c`) rewrites
`expected/mock-calls.sha256` from the reference, for when the harness's call set changes.

## Image tests: `run-image-tests.sh`

```
sh plugins/LargeIntegers/tests/run-image-tests.sh build/build/vm path/to/Pharo12.0-SNAPSHOT-64bit-7d5f14cb47.image [reference-vm-dir]
```

`<vm-dir>` holds `pharo` (or `Pharo.app`) and its plugins; it is put in `LD_LIBRARY_PATH`,
since a build's `pharo` would otherwise find the libraries of its own build directory.
Each VM run works on a fresh copy of the image (with its `.changes`, and its `.sources`
linked) in a directory of its own, with a timeout (`LI_TIMEOUT`, 300 s; a run takes a few
seconds); a crash, a timeout
or an error exit fails the step.  The scripts are `st/lib.st` (one `Object>>li...` method per
primitive, whose only fallback answers the failure code, so image code never masks a
failure; a SmallInteger-only PRNG; operands built byte by byte) followed by a driver.

- **repros** (`st/repro.st`, one VM run each, compared with `expected/repros.txt`):
  `li01` the left shift of a 64,000,000-byte receiver holding 1 (a segmentation fault
  before); `li05` empty minus empty; `li06` Montgomery with an empty second operand made by
  the plugin (eight answers of stale eden bytes before, a failure now); `li07` eight
  primitives installed at the wrong arity (the caller's slots overwritten before); `arity`
  all 15 primitives x wrong counts 0..4 x two receivers, 3000 calls each from a method
  without temps (garbage, `#'bad argument'` or a segmentation fault before; now 120 of 120
  shapes answer `#'bad number of arguments'`).
- **diff** (`st/diff.st`): 86,417 calls, all pairs over 94 operands with add, sub, and, or,
  xor, compare, multiply and divide, shifts, anyBit, the normalizes, operands up to 8000
  digits, Montgomery, wrong types.  The 71 shapes the old plugin could not run safely run
  too, and must answer right (a shift equal to that of the normalized receiver, a
  Montgomery failure); with those lines put back as the old plugin's `SKIP-LI0x` lines, the
  output must match `expected/image-diff.sha256` (54 digests, per section and operation).
- **kernels** (`st/diff-kernels.st`): 6,297 multiplies and divisions over the kernel
  thresholds (Karatsuba at 192 bytes +-16, one-digit quotients, divisors around 24 digits,
  exact divisions, operands up to 40,000 bytes), against `expected/image-kernels.sha256`.
- **sunit** (`st/imagetests.st`): LargePositiveIntegerTest, LargeNegativeIntegerTest,
  IntegerTest, IntegerDigitLogicTest, SmallIntegerTest, NumberTest, FloatTest and
  FractionTest: no failure, no error.

With `[reference-vm-dir]`, diff and kernels also run on that VM (without the unsafe shapes)
and the two outputs must be identical line for line.  `--record` (needs the reference)
rewrites the two image digest files from the reference's output.  The digests were recorded
from a VM built before the kernels, and are identical for the current one; on that old VM
this script fails li01 (crash), li06, li07, arity and diff (wrong answers to the unsafe
Montgomery shapes), and passes li05, kernels and sunit.

## Benchmark: `bench.sh`

`st/bench.st`: ns per direct call of multiply and divide from 64 to 262,144 bits (n x n,
squares x * x and x times a copy of x, 2n / n, n / n, n x SmallInteger, n / SmallInteger,
unbalanced, short quotients over long divisors), the small operands where
the kernels pick the 32-bit loops or the limb code, a few other small calls, and image
expressions.  With a second VM the runs alternate (`LI_BENCH_RUNS`, 3 by default) and the
table gives the medians and their ratio:

```
LI_BENCH_RUNS=5 sh plugins/LargeIntegers/tests/bench.sh old/build/vm image.image build/build/vm
```

Reference numbers (ns per call unless ms; Xeon Gold 6238R, clang 18 -O2, medians of 5
alternating runs; "before" is the plugin before the kernels):

| call, operand bits | before | after | after/before |
|---|---|---|---|
| mul 256x256 | 169 | 151 | 0.89 |
| mul 1024x1024 | 948 | 445 | 0.47 |
| mul 4096x4096 | 11,163 | 3,041 | 0.27 |
| mul 65536x65536 | 2,928,286 | 249,050 | 0.085 |
| mul 262144x262144 | 46.8 ms | 2.27 ms | 0.048 |
| div 128/64 | 373 | 236 | 0.63 |
| div 2048/1024 | 2,672 | 701 | 0.26 |
| div 131072/65536 | 6.41 ms | 1.11 ms | 0.17 |
| div 4096 / SmallInteger | 2,594 | 830 | 0.32 |
| SmallInteger x SmallInteger, // | 162 / 340 | 92 / 187 | 0.57 / 0.55 |
| image `(3 raisedTo: 100000) printString` | 38.6 ms | 10.3 ms | 0.27 |
| image `(3 raisedTo: 200000) squared` | 91.4 ms | 4.7 ms | 0.051 |
| image `(7 raisedTo: 300000) // (3 raisedTo: 200000)` | 434.6 ms | 50.6 ms | 0.12 |

Every other call on operands up to 128 bytes is 0.73-1.02 of before (add and sub gain
most, left shift and bit logic are at parity), except `normalize`, which allocates nothing
and pays the argument-count check: about +2.5 ns, +5%.  Above about 1 us, rows vary by 10%
or more on a busy host.

In C micro-benchmarks on this Xeon (Cascade Lake), code placement alone moves the time of
products, squares and divisions of operands below 32 digits by 10-25% either way between
two builds of the same code (the jump-conditional-code erratum): build both sides with
`-mbranches-within-32B-boundaries`, or compare in the image, before acting on such a
difference.

The recursive division (Burnikel & Ziegler, from 48 64-bit limbs) against the kernels
before it, same host and settings: div 32768/16384 bits 0.75, 65536/32768 0.61,
131072/65536 0.48 (1.02 ms to 0.49 ms), 524288/262144 0.28 (16.0 ms to 4.5 ms), image
`(7 raisedTo: 300000) // (3 raisedTo: 200000)` 0.39 (47.9 ms to 18.8 ms); the rows below
3072 bits run the same code as before.

The squaring kernels, against the same plugin without them (medians of 3 alternating runs,
same host; every other row within noise):

| call, operand bits | without | with | ratio |
|---|---|---|---|
| sqr 1024 (x * x / x * copy of x) | 434 / 426 | 365 / 366 | 0.84 / 0.86 |
| sqr 2048 | 1,015 / 1,141 | 786 / 802 | 0.77 / 0.70 |
| sqr 4096 | 3,408 / 3,389 | 1,998 / 2,220 | 0.59 / 0.66 |
| sqr 65536 | 254,553 / 256,656 | 175,923 / 174,797 | 0.69 / 0.68 |
| sqr 262144 | 2.31 ms / 2.32 ms | 1.59 ms / 1.59 ms | 0.69 / 0.69 |
| image `(3 raisedTo: 200000) squared` | 4.84 ms | 3.38 ms | 0.70 |
| image 10 squarings of 3^200000 | 34.1 ms | 23.1 ms | 0.68 |

## Kernel thresholds

Measured on the same host with `LI_TUNABLE` builds (32-bit limbs with `-DLI_LIMB_BITS=32` on
x86_64, not on a 32-bit host):

| knob | 64-bit limbs | 32-bit limbs | measurement |
|---|---|---|---|
| `LI_KARATSUBA_LIMBS` (shorter operand) | 24 (48 digits, 1536 bits) | 32 | 16..32 within 2%; Karatsuba/schoolbook 0.93 at 24 limbs, 0.78 at 64, 0.36 at 512 |
| `LI_MUL_LOOP_DIGIT_PRODUCT` (the 32-bit loop when digits x digits <= it, short operand >= 3 digits) | 25 | (below Karatsuba) | 5x5 digits: loop 24.3 ns, limbs 27.6; 5x6: 31.8, 28.3 |
| `LI_DIV_LOOP_QUOTIENT_DIGITS` (the 32-bit loop for quotients up to it, divisor < 24 digits) | 1 | 2 | 1, 2 and 3 equal within noise in the image |
| `LI_SQR_BASECASE_LIMBS` (squares of at least this many limbs take the squaring kernels) | 3 | 11 | 64-bit: below 3 limbs the one-limb stream and the 32-bit loop run first; 6 digits: square 26.2 ns, product 32.6. 32-bit, from the wasm build (emcc -m64 -sMEMORY64=2 -O2, node 25), square/loop for one pointer and a copy: 8 digits 1.03 / 1.13, 10 1.01 / 1.04-1.08, 11 0.89-0.91 / 0.97, 12 0.90 / 0.97, 14 0.83 / 0.88, 16 0.85 (copy); on x86_64 with 32-bit limbs the square wins from 8 (0.95) |
| `LI_SQR_KARATSUBA_LIMBS` | 44 (88 digits) | 48 | schoolbook squaring is 0.52-0.55 of schoolbook multiplication from 20 limbs on (64-bit limbs; 0.61-0.75 from 12 with 32-bit limbs); one level of Karatsuba squaring over it: 1.01 at 38 limbs, 0.98 at 42, 0.97 at 47, 0.89 at 59 (64-bit); 1.02 at 42, 0.98 at 53 (32-bit); thresholds 36..48 within 2% (64-bit), 44..52 within 1% (32-bit) |
| `LI_BZ_LIMBS` (Burnikel & Ziegler when divisor and quotient both have this many limbs; its recursion's base) | 48 (96 digits, 3072 bits) | 32 | below: 64-bit limbs 1.01-1.02 of Knuth D at 32..44 limbs with thresholds 24..40; 32-bit limbs 1.03-1.10 at 16..24 limbs with 16..24; at it: 0.95 and 0.92-0.98 |
| `LI_BZ_QUOTIENT_LIMBS` (with a divisor of `LI_BZ_LIMBS` limbs, Burnikel & Ziegler for a shorter quotient of at least this many limbs, the divisor at least this many longer) | 24 | 12 | see below |

The squaring thresholds come from a C micro-benchmark over `liDigitMultiply` (best of 7
runs of 10 ms per size, one pinned core, clang 18 -O2), squares against products of two
different operands of the same length, 1 to 2000 limbs in steps of 15%: the square is never
slower (1.02 at most, at the sizes where both run the same code, noise) and takes, of the
product's time:

| limbs | 3 | 8 | 16 | 25 | 44 | 101 | 308 | 1084 | 2000 |
|---|---|---|---|---|---|---|---|---|---|
| 64-bit limbs | 0.80 | 0.78 | 0.64 | 0.61 | 0.63 | 0.69 | 0.67 | 0.69 | 0.68 |
| 32-bit limbs | 1.00 | 0.96 | 0.84 | 0.79 | 0.61 | 0.61 | 0.66 | 0.67 | 0.66 |

(The 32-bit row is from x86_64 with the squaring kernels from 8 limbs; they now start at
11, the wasm build's threshold, and squares of 8 to 10 limbs run the 32-bit loop.)  In the
wasm build, products of 1 to 4 digits, which never reach the squaring code, take 1 to 2 ns
more than before the squaring kernels (5-10%; 0.98-1.00 between two builds of the same
code): the 32-bit loop is the same, inlined into a larger function.

Equal digits in two places cost a memcmp more (0.62 to 0.89 of the product with 64-bit
limbs); operands of the same length that differ only in their top digit, the worst case of
that comparison, 1 to 4 ns more than before.  Measured with
`-mbranches-within-32B-boundaries` on both sides: without it, the alignment of the
unchanged 32-bit loop moved the time of products of 4 to 24 digits by up to 25% either
way between two builds of the same code (the Xeon's jump-alignment erratum).

The recursive division against Knuth's algorithm D (`liBZLimbs` set past every size), ns
per `liDigitDiv` of a random 2n-limb dividend by an n-limb divisor, or 10n by n; a C
program that includes `LargeIntegersKernels.c` with `LI_TUNABLE`, best of 4 runs pinned
to one core, clang 18 -O2 (gcc 13 -O2 for the last column):

| n limbs | 64-bit 2n/n: Knuth D | BZ/Knuth D | 10n/n | 32-bit 2n/n | 10n/n | 64-bit 2n/n, gcc |
|---|---|---|---|---|---|---|
| 48 | 3,176 | 0.95 | 0.95 | 0.88 | 0.88 | 0.80 |
| 64 | 5,359 | 0.92 | 0.94 | 0.78 | 0.81 | 0.78 |
| 128 | 20,656 | 0.77 | 0.77 | 0.68 | 0.69 | 0.61 |
| 256 | 76,724 | 0.65 | 0.66 | 0.57 | 0.57 | 0.47 |
| 1024 | 1,160,104 | 0.42 | 0.42 | 0.36 | 0.36 | 0.28 |
| 4096 | 18,337,563 | 0.25 | 0.25 | 0.21 | 0.21 | 0.16 |
| 10000 | 109,149,964 | 0.18 | | 0.15 | | 0.11 |

Quotients of 48..65 limbs (64-bit) by divisors of 48..2048 limbs: 0.67-0.98; of 101 or 201
limbs: 0.46-0.99; with 32-bit limbs, quotients of 32..201 limbs: 0.39-0.94.  No shape
measured slower than Knuth D at or above the thresholds.

A quotient shorter than `LI_BZ_LIMBS` is one top block: Knuth D on the top of the divisor,
then the product of the quotient with the rest.  Measured against Knuth D alone with
production builds (constant thresholds) in one program, best of 7 alternating runs pinned
to one core: on x86_64 with clang 18 -O2 and `-mbranches-within-32B-boundaries`, in wasm
with emcc -m64 -sMEMORY64=2 -O2 under node 25; quotient / divisor limbs:

| limbs | 64-bit | | 32-bit (x86_64) | | 32-bit (wasm) |
|---|---|---|---|---|---|
| 24 / 48, 64 | 0.97, 1.01 | 12 / 32, 48 | 0.85, 0.82 | 12 / 32, 48 | 0.95, 0.90 |
| 24 / 96, 256, 1000 | 0.90, 0.89, 0.89 | 12 / 96, 1000 | 0.74, 0.73 | 12 / 96, 1000 | 0.81, 0.74 |
| 28 / 56, 96, 1000 | 0.95, 0.88, 0.84 | 24 / 48, 64 | 0.85, 0.82 | 24 / 48, 64 | 0.92, 0.88 |
| 32 / 64, 96, 1000 | 0.93, 0.87, 0.84 | 24 / 200, 1000 | 0.75, 0.78 | 24 / 200, 1000 | 0.74, 0.73 |
| 40 / 80, 512, 1000 | 0.86, 0.80, 0.78 | 32 / 64, 1000 | 0.80, 0.66 | 30 / 60 | 0.96 |

Lower thresholds: with 64-bit limbs, quotients of 16..20 limbs are 0.91-1.16 (the product
with the rest is schoolbook below `LI_KARATSUBA_LIMBS`); with 32-bit limbs, quotients of
8 limbs gain from 40-limb divisors on (0.96 wasm, 0.88 x86_64), but are 0.995 at 8 / 32
and 1.015 at 24 / 32 on wasm.  Without the condition on the divisor's length: 1.03 at
30 / 48, 1.04 at 40 / 48, 1.00 at 36 / 56 and 40 / 64 (64-bit), 1.02 at 28 / 32 (32-bit,
x86_64).  In the image (`bench.sh`, medians of 5 alternating runs, against the plugin
before Burnikel & Ziegler, whose calls also allocate and shift): div 7680/6144 bits (24 /
96 limbs) 0.99, 18432/16384 (32 / 256) 0.96, 67072/65536 (24 / 1024) 0.95, 68096/65536
(40 / 1024) 0.85.

## Not covered

A run on a 32-bit host, a big-endian host (only the emulated digit order), aarch64,
Windows, macOS or wasm: the C tests assume a 64-bit host, and the image tests and the
benchmark were run on x86_64 Linux only.  A real out-of-memory in the image, or a real
malloc failure in the kernels (only injected ones).  The Smalltalk simulation of the
plugin.
