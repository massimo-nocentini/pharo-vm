# Differential harness against the C plugin

The unit tests in `src/tests.rs` carry golden vectors and FNV-1a hashes
captured from the **actual** `plugins/BitBltPlugin/src/common/BitBltPlugin.c`,
compiled standalone against the stub headers in this directory and driven
through its own internal functions (`performCopyLoop`,
`tryCopyingBitsQuickly`, `warpLoop`). This is how they were produced:

```sh
cd rust/plugins/bit-blt-plugin/tests/harness
CFLAGS="-O0 -I. -I../../../../../plugins/BitBltPlugin/src/common"
gcc $CFLAGS -o overlap_golden overlap_golden.c && ./overlap_golden
gcc $CFLAGS -o copy_sweep copy_sweep.c && ./copy_sweep
gcc $CFLAGS -o warp_sweep warp_sweep.c && ./warp_sweep
```

* `overlap_golden.c` — ten overlapping same-form copies (including the
  skewed rightward cases where the C's reversed loop is not memmove-exact),
  printed as full destination bitmaps.
* `copy_sweep.c` — 400 generated scenarios over all 35 loadable combination
  rules, every depth pair, MSB/LSB, indexed color maps, halftones, and
  no-source fills; prints one hash per scenario.
* `warp_sweep.c` — 150 WarpBlt scenarios (both smoothing modes, all depth
  pairs, off-source sampling, clipping advance) with the interpreter faked by
  the stub functions at the top of the file; prints one hash per scenario.

The stub headers exist only to satisfy the plugin's includes; `virtualMachine.h`
declares just enough of the proxy for the file to compile. `NDEBUG` is defined
so the C's `assert`s are compiled out, as in a production VM build — several
reachable states (e.g. `skew = -32`) violate them by design.

The generators use a shared LCG so the Rust tests rebuild bit-identical
inputs; if you change a driver, re-capture its output and update the matching
constants in `src/tests.rs`.
