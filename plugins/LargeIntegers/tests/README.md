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
  without the dispatch to the 32-bit loops for the smallest operands.  Six builds: the
  host's limbs (64-bit on x86_64 and aarch64), forced 32-bit limbs, and both with digits
  stored byte-swapped as on a big-endian host (`FAKE_BIGENDIAN`), under ASan+UBSan (when the
  compiler can, UBSan reports fatal); both limb sizes again at -O2.
- **oracle**: the -O2 builds print 3000 products, 3000 squares (sizes around the squaring
  thresholds of both limb sizes, up to 2500 digits) and 3000 quotients and remainders,
  which `c/oracle.py` checks with Python's integers.
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

`scale` (default 1, about 860,000 kernel cases per build) multiplies the random counts;
`run-c-tests.sh 10` runs about 8.5 million per build; the cases depend only on the scale,
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
unbalanced), the small operands where
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
and pays the argument-count check: about +2.5 ns, +5%.  Above about 1 us, rows vary by 10% or more on a busy host.

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

## Not covered

A run on a 32-bit host, a big-endian host (only the emulated digit order), aarch64,
Windows, macOS or wasm: the C tests assume a 64-bit host, and the image tests and the
benchmark were run on x86_64 Linux only.  A real out-of-memory in the image, or a real
malloc failure in the kernels (only injected ones).  The Smalltalk simulation of the
plugin.
