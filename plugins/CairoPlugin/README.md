# CairoPlugin

The CairoPlugin lets Athens (Athens-Cairo, Pharo 12 to 15) draw through cairo without the callouts of the FFI. It has three kinds of primitives:

- **one-for-one**, the group `#basic`: each replaces one FFI method of Athens (`newPath`, `moveToX:Y:`, `primRectangleX:y:width:height:`, the matrices, the text metrics, `textPath:`...) and makes the same cairo call;
- **shape-level**, the group `#shape`: a Rectangle or an AthensCairoPath, filled with a Color or stroked, in one primitive that makes the cairo calls of Athens, in the same order;
- **text**, the group `#text`: a whole `drawString:` in one primitive, for ByteString, ByteSymbol, WideString and WideSymbol. It encodes UTF-8 with simdutf, through the engine of the SimdUTFPlugin.

The pixels and the cairo state are the same as with the FFI.

**How it reaches cairo:**
- **Natively** the plugin includes no cairo header and links no cairo. When the image binds it, the plugin loads the libcairo that the image's FFI uses, by the same path and through the VM's module loader, so there is one cairo in the process.
- A probe of `cairo_create` makes the bind fail if the plugin and the FFI would reach different cairos.
- **On WebAssembly** it is a builtin plugin, `CairoPlugin (i)`, linked to the `pharo_cairo` of the FFI's registry.

**Build option:** `FEATURE_PLUGIN_CAIRO` (`cmake/plugins.cmake`).
- It is ON on 64-bit targets that build the SimdUTFPlugin (`FEATURE_PLUGIN_SIMDUTF`).
- On Emscripten it also needs a build with cairo (`WASM_CAIRO`).

**Layout:**
- `include/common/cpCairo.h`: the cairo types and the functions the plugin uses, as one X-macro.
- `include/common/cpLayout.h`: the layout table, the slot indices that the image computes by name.
- `src/binding/cpCairo.c`: the binding to cairo, through the module loader, or static on WebAssembly.
- `src/common/CairoPlugin.c`: the glue, with the helpers and the admin primitives. It includes `src/prims/basic.c`, `shape.c` and `text.c`.
- `tests/`: the C tests (`run-c-tests.sh`, fakevm), the Smalltalk loader and runner, and the CI probe `ci-load.st`.
- `bench/`: the benchmark, comparing the FFI with the plugin's groups.

[docs/CairoPlugin.md](../../docs/CairoPlugin.md) gives the full contract: the layout table, the primitives with their failure codes, the installer, the guarantees and limits, the tests and the speed.

**The Smalltalk code** is in the git submodule `plugins/UtilsPlugin/vm-plugins-primitives.st` ([massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st)), the same one as the UtilsPlugin's; fetch it with `git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st`.
- Its `BaselineOfVMPluginsPrimitives` has the groups `Cairo` (`VM-Plugins-Primitives-Cairo`), `CairoTests` and `CairoBenchmarks`. None of them is in `default`.
- The patch is opt-in. `CairoPlugin install` binds the plugin and patches the Athens methods at run time, keeping the originals under `ffi…` selectors. `CairoPlugin uninstall` restores them.
- At every start of the image the plugin is bound again. On a VM without the plugin, Athens keeps its FFI.
- `tests/load-cairo.st` loads the packages from the command line.

```smalltalk
Metacello new
	baseline: 'VMPluginsPrimitives';
	repository: 'github://massimo-nocentini/vm-plugins-primitives.st:master/src';
	load: #( 'Cairo' ).	"binds the plugin and installs #basic #shape #text"
CairoPlugin info.		"#(1 'CairoPlugin 1.0' 'native' 11704 'loader' nil #())"
CairoPlugin uninstall.		"Athens as it was"
CairoPlugin install: #( #basic ).
```

**Speed** (`bench/run-bench.sh`, see docs/CairoPlugin.md; FFI time / plugin time, every group installed, the geometric mean over the scenes, the same pixels and cairo calls in every mode):
- on Cog (Linux x86-64, Pharo 12 and 15), 1.55x for whole scenes on 1000x1000 surfaces: 1.3 to 1.6x for many small shapes, 3.4 to 3.9x for `drawString:`, 1.05 to 1.1x for a Roassal graph and 1.0x for one long polyline (rasterizing dominates); single calls 1.8 to 3.9x faster (`newPath` 124 to 46 ns);
- on the WebAssembly VMs, 2.8 to 2.9x for whole scenes, 1.8 to 3.8x for many small shapes, 1.4 to 1.9x for Roassal, 8.5 to 11x for `drawString:`, and single calls 5 to 16x faster;
- a whole Roassal morph frame gains what its drawing gains, 4 to 6% on Cog and 1.4x on WebAssembly; the display of the surface in the Morph (`asForm`), which stays on the FFI, is only about 2 ms of a 120 ms frame (3 of 305 ms on WebAssembly).

**Tests:**
- `tests/run-c-tests.sh`: the C lanes.
- `tests/run-st-tests.sh`: the Smalltalk suites, on a VM with or without the plugin.
- `tests/ci-load.st`: the probe of the CI jobs, on the VMMaker image. It draws a scene by the FFI and through the primitives, and compares the pixels.
- Lane 74 of `make wasm-check` (`tests/wasm/lanes/74-cairo.sh`): the same on the WebAssembly VMs.

**Licences:** the plugin and the packages are under the MIT licence. cairo (LGPL-2.1 or MPL-1.1) is not part of the plugin: natively it is the library that the VM bundle already ships, loaded at run time, and on WebAssembly the `pharo_cairo` that the VM already links for the FFI.
