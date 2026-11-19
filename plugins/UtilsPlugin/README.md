# UtilsPlugin

The UtilsPlugin gives Pharo 12 to 15 five small features, in C:

- the **fused multiply-add** of Floats (`Float>>#times:plus:`, C's `fma`);
- the **splitting of strings** between delimiters (`String>>#strtok:inclusive:`, `String>>#linesIncludeEmptySubstrings:`);
- the **points of Bézier curves** by a de Casteljau scheme (`SequenceableCollection>>#bezierPointsAt:`);
- **Knuth's lagged Fibonacci generator** (`KnuthRandom`, a `Random`), from his `rng-double.c`, kept unchanged in `src/knuth`;
- **Vigna's SplitMix64** (`SplitMix64`, a `Random` of 64-bit outputs), from his `splitmix64.c`, kept unchanged in `src/vigna`.

The first four are ported from the UtilsPlugin of the `pharo-10` branch, with the same module and primitive names; SplitMix64 is new in version 1.1.

**Layout:**
- `src/common/UtilsPlugin.c`: the glue.
- `src/engine/upEngine.c`, `include/common/upEngine.h`: the engine.
- `src/knuth/rng-double.c`: Knuth's file.
- `src/vigna/splitmix64.c`: Vigna's file.
- `tests/`: the C tests, the Smalltalk loader and runner, and the CI probe.

[docs/UtilsPlugin.md](../../docs/UtilsPlugin.md) gives the build option `FEATURE_PLUGIN_UTILS`, the API, the semantics, the primitive contract, the tests and the speed.

**The Smalltalk code** is the git submodule `vm-plugins-primitives.st` ([massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st)); fetch it with `git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st`.
- It holds the Tonel packages `VM-Plugins-Primitives` and `VM-Plugins-Primitives-Tests`, which Metacello loads with `BaselineOfVMPluginsPrimitives`.
- Every method tries the plugin, then answers through a Smalltalk twin of its primitive (`UtilsPluginSmalltalk`), with the same bits, on a VM without the plugin.
- `tests/load-utils.st` loads the packages from the command line.

**Licences:** the plugin and the packages are under the MIT licence. `src/knuth/rng-double.c` is D. E. Knuth's, in the public domain as long as it is not changed: it is kept as it is, and `tests/run-c-tests.sh` checks its SHA256. `src/vigna/splitmix64.c` is Sebastiano Vigna's, dedicated to the public domain: it is kept as it is too, with its notice, and its SHA256 is checked as well.
