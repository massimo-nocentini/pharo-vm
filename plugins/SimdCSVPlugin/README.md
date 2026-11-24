# SimdCSVPlugin

The SimdCSVPlugin reads CSV for Pharo 12 to 15 with [simdcsv](https://github.com/geofflangdale/simdcsv) (Geoff Langdale, commit a9494a8), in two steps:

- **the index**: the positions of the separators and of the line ends outside of quotes, 64 bytes at a time, as 32-bit offsets in a word array of the image;
- **the rows**: Arrays of Strings, Integers and Floats built in C from such an index, in batches whose object memory is bounded, so that a large file causes no full garbage collection.

Quotes are read as Python's `csv` reads them (lenient), or strictly; the kernels only ever skip over input that a checked bit formula proves regular, and anything else goes through one byte automaton. Neither the index nor the source is trusted: no data makes the plugin read or write outside of its arguments.

The kernels are AVX2 (chosen at run time) and SSE2 on x86_64, NEON on arm64, and a portable one, which is the only one on WebAssembly. Of simdcsv, the part of `src/main.cpp` that computes the bit masks is compiled for AVX2 and NEON, cut at configure time out of the pinned archive and checked by its SHA256 (`cmake/simdcsv.cmake`). The environment variable `SIMDCSV_FORCE_IMPLEMENTATION` (`avx2`, `sse2`, `neon`, `fallback`) forces a kernel. The plugin is ON on every 64-bit target, and built in on WebAssembly.

**Layout:**
- `src/common/SimdCSVPlugin.c`: the glue, the only file that includes the VM's headers.
- `src/engine/`, `include/common/scEngine.h`: the engine, C++17 without the C++ library, which the plugin links by file, so that it needs no libstdc++.
- `tests/`: the C tests (`run-c-tests.sh`, and `glue.sh` for the glue against a fake VM), the Smalltalk loader and runner (`load-simdcsv.st`, `run-st-tests.sh`), and the CI probe (`ci-load.st`).
- `bench/`: the benchmarks.

[docs/SimdCSV.md](../../docs/SimdCSV.md) gives the build option `FEATURE_PLUGIN_SIMDCSV`, the API, the semantics, the primitive contract, the memory limits, the tests and the speed.

**The Smalltalk code** is the repository `simdcsv.st` ([massimo-nocentini/simdcsv.st](https://github.com/massimo-nocentini/simdcsv.st)): the Tonel packages `SimdCSV` (reader, table and writer, with the protocols of NeoCSV) and `SimdCSV-Tests`, which Metacello loads with `BaselineOfSimdCSV`. Without the plugin, a Smalltalk backend answers the same.

**Licences:** the plugin and the packages are under the MIT licence. The part of `src/main.cpp` of simdcsv is under the Apache License 2.0, which the build installs as `LICENSE.simdcsv` next to the plugin (in `Contents/Resources` on macOS). WebAssembly builds contain no simdcsv code.
