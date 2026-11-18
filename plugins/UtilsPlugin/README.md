# UtilsPlugin

The UtilsPlugin gives Pharo 12 to 15 four small primitives, in C:

- the **fused multiply-add** of Floats (`Float>>#times:plus:`, C's `fma`);
- the **splitting of strings** between delimiters (`String>>#strtok:inclusive:`, `String>>#linesIncludeEmptySubstrings:`);
- the **points of Bézier curves** by a de Casteljau scheme (`SequenceableCollection>>#bezierPointsAt:`);
- **Knuth's lagged Fibonacci generator** (`KnuthRandom`, a `Random`), from his `rng-double.c`, kept unchanged in `src/knuth`.

They are ported from the UtilsPlugin of the `pharo-10` branch, with the same module and primitive names.

**Layout:**
- `src/common/UtilsPlugin.c`: the glue.
- `src/engine/upEngine.c`, `include/common/upEngine.h`: the engine.
- `src/knuth/rng-double.c`: Knuth's file.
- `tests/`: the C tests, the Smalltalk loader and runner, and the CI probe.

[docs/UtilsPlugin.md](../../docs/UtilsPlugin.md) gives the build option `FEATURE_PLUGIN_UTILS`, the API, the semantics, the primitive contract, the tests and the speed.

**The Smalltalk code** is the git submodule `vm-plugins-primitives.st` ([massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st)); fetch it with `git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st`.
- It holds the Tonel packages `VM-Plugins-Primitives` and `VM-Plugins-Primitives-Tests`, which Metacello loads with `BaselineOfVMPluginsPrimitives`.
- Every method tries the plugin, then answers through a Smalltalk twin of its primitive (`UtilsPluginSmalltalk`), with the same bits, on a VM without the plugin.
- `tests/load-utils.st` loads the packages from the command line.

**Licences:** the plugin and the packages are under the MIT licence. `src/knuth/rng-double.c` is D. E. Knuth's, in the public domain as long as it is not changed: it is kept as it is, and `tests/run-c-tests.sh` checks its SHA256.
