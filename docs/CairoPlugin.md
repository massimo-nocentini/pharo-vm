# CairoPlugin

The CairoPlugin lets Athens draw through cairo with named primitives instead of the callouts of the FFI (uFFI and its `TFSameThreadRunner`). It serves Athens-Cairo as Pharo 12, 13, 14 and 15 ship it, and therefore Roassal, Spec's Athens views and the morphs that draw strings on an Athens canvas.

It has three groups of primitives, which the image installs separately:

- **`#basic`, one-for-one:** 58 primitives, each of which replaces one FFI method of Athens (`newPath`, `moveToX:Y:`, `primRectangleX:y:width:height:`, the matrices, the text metrics, `textPath:`…) and makes the same cairo call, with the same arguments.
- **`#shape`, shape-level:** 4 primitives. A Rectangle or an AthensCairoPath, filled with a Color or stroked with a stroke paint of a Color, is drawn by one primitive that makes the cairo calls of Athens, in the same order.
- **`#text`:** 1 primitive. A whole `drawString:` is one primitive, for ByteString, ByteSymbol, WideString and WideSymbol. It encodes the characters in UTF-8 with simdutf, through the engine of the SimdUTFPlugin (see `docs/SimdUTF.md`).

The pixels, the cairo state, the answers and the number of calls of each cairo function are those of the FFI (see [Guarantees](#guarantees)).

The Smalltalk side is the package `VM-Plugins-Primitives-Cairo` of [massimo-nocentini/vm-plugins-primitives.st](https://github.com/massimo-nocentini/vm-plugins-primitives.st), the submodule `plugins/UtilsPlugin/vm-plugins-primitives.st`, which the UtilsPlugin shares (see `docs/UtilsPlugin.md`). It is opt-in: none of its groups is in the baseline's `default`. Loading it patches the hot methods of Athens-Cairo at run time; each patch keeps the original FFI method under an `ffi…` selector and falls back to it whenever its primitive fails. `CairoPlugin uninstall` restores Athens exactly.

## Files

| Path | What |
|---|---|
| `plugins/CairoPlugin/include/common/cpCairo.h` | the cairo types, enums and structures the plugin uses, and its 57 cairo functions as one X-macro: the table type, the names, the flags and the binding API |
| `plugins/CairoPlugin/include/common/cpLayout.h` | the layout table: the slot indices and constants that the image computes by name |
| `plugins/CairoPlugin/src/binding/cpCairo.c` | the binding: the table filled through a loader callback (native), or statically (WebAssembly) |
| `plugins/CairoPlugin/src/common/CairoPlugin.c` | the module functions, the helpers that validate objects, the admin primitives, the counters, the exports table and the accessor depths of the 69 primitives; it includes the three files below |
| `plugins/CairoPlugin/src/prims/basic.c`, `shape.c`, `text.c` | the primitives of `#basic`, `#shape` and `#text` |
| `plugins/CairoPlugin/tests/primitives.txt` | the single list of the primitives: name, accessor depth, group, file and argument count |
| `plugins/CairoPlugin/tests/` | the C tests (`run-c-tests.sh`, fakevm, the glue tests), the Smalltalk loader and runner (`load-cairo.st`, `run-st-tests.sh`), `web-surface.st` for the WebAssembly VMs, and the CI probe (`ci-load.st`) |
| `plugins/CairoPlugin/bench/` | the benchmark (see [Speed](#speed)) |
| `plugins/UtilsPlugin/vm-plugins-primitives.st` | the submodule with the Smalltalk code |
| `tests/wasm/lanes/74-cairo.sh`, `tests/wasm/st/cairo-lane.st` | the WebAssembly lane |

## Building

`cmake/plugins.cmake` builds the plugin when `FEATURE_PLUGIN_CAIRO` is on. It is on by default on 64-bit targets that build the SimdUTFPlugin (`FEATURE_PLUGIN_SIMDUTF`). It is off on 32-bit targets, where turning it on is refused. It is a dependent option (`cmake_dependent_option`): without `FEATURE_PLUGIN_SIMDUTF`, or on Emscripten without cairo (`WASM_CAIRO`), it is off, and the configure says why; the prerequisites are evaluated again at every configure, so turning `FEATURE_PLUGIN_SIMDUTF` or `WASM_CAIRO` off in an existing tree turns the plugin off, and turning them on again turns it back on (an explicit `-DFEATURE_PLUGIN_CAIRO=OFF` stays off).

- **Native VMs:** a shared library, `libCairoPlugin.so`, `.dylib` or `CairoPlugin.dll`, next to the other plugins. It **links no cairo library** and finds cairo at run time (see [How it reaches cairo](#how-it-reaches-cairo)). How it is compiled depends on the cairo the VM bundles (`docs/CairoStack.md`):
  - **With the cairo stack** (the default on Linux, macOS and MinGW: the stack built from `cmake/cairo-stack/pins.txt`, or the prefix of `PHARO_CAIRO_STACK_DIR`), it is compiled with the stack's `cairo.h`: `-DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES -DCP_CHECK_PROTOTYPES`, the stack's three include directories, and on MinGW `CAIRO_WIN32_STATIC_BUILD`. It then uses cairo's own types, and a compile-time check (`_Static_assert` with `__builtin_types_compatible_p`) proves that the type of each of its 57 function pointers is the type of cairo's function: a mismatch stops the build. The check is unevaluated, so the object still references no cairo symbol. The target depends on `pharo_cairo_stack_bundle`, which provides the headers.
  - **Without it** (`BUILD_BUNDLE=OFF`; `FEATURE_LIB_CAIRO=OFF`; FreeBSD, OpenBSD, MSVC, Cygwin, which bundle no cairo: the image loads the system's), it includes no cairo header. It declares its own prototypes and structures (`cpCairo.h`), which the `protocheck` and `targets` lanes of the C tests check against cairo's headers.

  It links the SimdUTF engine, `pharo_su_engine` (`suEngine.cpp` and simdutf), privately. It is linked as C++, and exports only the module functions, the 69 primitives and their 69 accessor depths: 142 symbols, with no `su_`, `simdutf` or `cp_` symbol.
- **WebAssembly VMs:** the plugin is built in, `CairoPlugin (i)`, and only in a build with cairo (`WASM_CAIRO`, see `docs/WebAssembly.md`). It is compiled with cairo.h (`-DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES -DCP_STATIC_CAIRO`), so the compiler checks its 57 prototypes against cairo 1.18.6, and it is linked to the `pharo_cairo` of the FFI's registry. The SimdUTFPlugin, libSimdUTFFFI and the CairoPlugin share one copy of the engine, the static library `pharo_su_engine`. The plugin adds 62,248 bytes to `pharo-web.wasm` (10,581 with `gzip -9`).
- **Turning it off:** `-DFEATURE_PLUGIN_CAIRO=OFF`.

No system cairo package is needed. The native build takes cairo's headers from the stack, which the configure builds or reuses (`cmake/cairoStack.cmake`; the tools it needs are listed in `docs/CairoStack.md`). A build without the stack needs no cairo header at all.

## How it reaches cairo

The plugin is unbound when the VM starts, and every drawing primitive then fails with `#'unsupported operation'`, so the patched methods run their FFI originals. The image binds it (`CairoPlugin bind`) when the package is loaded and at every start of the image.

- **Natively** `primitiveBind` receives the path of the libcairo that the image's FFI loads, `CairoLibrary uniqueInstance libraryName`. It loads it with the VM's own module loader, `ioLoadModuleOfLength`, the code and the module cache of the FFI's `primitiveLoadSymbolFromModule`, and looks up each function in that module. So the plugin and the FFI use **the same libcairo**, in the same process. The plugin never calls `dlopen` or `dlclose` and never links cairo.
- **The probe:** the image also passes the address of `cairo_create` that its FFI resolves. The bind fails with `#'inappropriate operation'` when it differs from the plugin's, and with `#'not found'` when it is nil or NULL (the FFI cannot reach cairo either). Nothing is bound then.
- **On WebAssembly** the table is filled statically with cairo's functions (`'static'`): the library path is checked but not loaded, and the probe is the registry's `cairo_create`, the same function.
- **Versions:** every VM ships **one cairo, 1.18.6**, pinned in `cmake/cairo-stack/pins.txt` (see `docs/CairoStack.md`):
  - **Linux and macOS:** it is built from its release archive;
  - **Windows x86_64 and ARM64:** it is the pinned MSYS2 package (1.18.6 with MSYS2's patches);
  - **WebAssembly:** it is linked statically.

  The bind accepts any cairo from 1.16.0 on (`cairo_version()` ≥ 11600), else it fails with `#'unsupported operation'`. That lower bound is for the image's FFI loading another cairo: the system's, in a build without the stack (as old as the 1.16.0 of Ubuntu 20.04 or Debian 11).

  How the version is checked:
  - The CI jobs require exactly the pinned version: `ci-load.st` must print `CairoPlugin native 1 cairo <the pin> ok` (see [CI and WebAssembly](#ci-and-webassembly)).
  - The `symbols` lane checks that the 57 functions are exported by:
    - the VM's libcairo;
    - with `--zips`, the libcairo of each CI zip (Linux, macOS and Windows);
    - with `--deps`, the old files.pharo.org bundles, the cairo of the VMs before the stack.
  - The `stack` lane runs `scripts/cairo-stack/check.sh` on the tree's bundle (see [Tests](#tests)).
  - In each CI job, `ci-load.st` binds the plugin to the zip's cairo and requires every function (the optional ones too) and the same pixels.
- **Missing functions:** a missing required function makes the bind fail with `#'not found'`; `primitiveInfo` names it. The two optional ones, `cairo_ft_scaled_font_lock_face` and `cairo_ft_scaled_font_unlock_face` (cairo's FreeType backend, which every bundled cairo has), may be missing: the bind succeeds, lists them, and the installer leaves out `#text` and the `lock`/`unlock` patches.
- A bind replaces the previous one as a whole. `primitiveUnbind` and the module's shutdown clear it. The table holds addresses inside libcairo, which must never be unloaded while bound (no code of Pharo does).

### The layout table

The one-for-one primitives *are* the patched Athens methods: the receiver and the arguments are those of the method. C reads the handles from the slots of the receiver at indices that the image computes once, by name, from `allInstVarNames` (`CairoPluginLayout`), and passes at the bind. So a reshaped class stops the bind and the install instead of making C read the wrong slot; and C validates every object it reads before any cairo call.

The table is an Array of 33 elements: 29 SmallIntegers, then the four string classes of the text primitive. Values are 0-based slot indices (the same in Pharo 12 to 15), or constants:

| Position | Name | From | Value |
|---|---|---|---|
| 1 | LAYOUT_VERSION | constant | 1 |
| 2–5 | CANVAS_HANDLE, CANVAS_PATH_TRANSFORM, CANVAS_PAINT, CANVAS_SURFACE | AthensCairoCanvas `handle`, `pathTransform`, `paint`, `surface` | 6, 7, 0, 1 |
| 6 | SURFACE_HANDLE | AthensCairoSurface `handle` | 1 |
| 7 | BUILDER_CONTEXT | AthensCairoPathBuilder `context` | 0 |
| 8 | EXTOBJ_HANDLE | ExternalObject `handle` (the installer checks the same index in AthensCairoMatrix, AthensCairoPath, CairoScaledFont, CairoTextExtents, CairoFontExtents and AthensCairoPatternPaint) | 0 |
| 9 | PAINTMODE_CANVAS | AthensCairoPaintMode `canvas` | 0 |
| 10–15 | STROKE_FILL_PAINT, _WIDTH, _CAP, _JOIN, _DASHES, _DASH_OFFSET | AthensCairoStrokePaint `fillPaint`, `width`, `capStyle`, `joinStyle`, `dashLenghts` (sic), `dashOffset` | 0, 1, 3, 4, 5, 6 |
| 16–18 | RENDERER_FONT, _CANVAS, _GLYPH_EXTENTS | CairoFreetypeFontRenderer `font`, `canvas`, `glyphExtents` | 0, 2, 6 |
| 19–20 | COLOR_RGB, COLOR_ALPHA | Color `rgb`, `alpha` | 0, 3 |
| 21–24 | RECT_ORIGIN, RECT_CORNER, POINT_X, POINT_Y | Rectangle `origin`, `corner`; Point `x`, `y` | 0, 1, 0, 1 |
| 25–27 | COLOR_RED_SHIFT, _GREEN_SHIFT, _COMPONENT_MASK | the class variables of Color (and `ComponentMax = ComponentMask asFloat` is checked) | 20, 10, 1023 |
| 28 | COLOR_ALPHA_MAX | constant | 255 |
| 29 | SURFACE_CURRENT_CANVAS | AthensCairoSurface `currentCanvas` | 0 |
| 30–33 | the string classes | ByteString, ByteSymbol, WideString, WideSymbol | |

The bind checks it: size 33 (or 29, which binds without the text primitive), element 1 = 1, slot indices in 0..254, shifts in 0..62, the mask and the alpha maximum in 1..2^30, positions 30–31 classes of byte instances and 32–33 classes of 32-bit word instances (the text primitive reads every instance of them as Latin-1 bytes or UTF-32 words), else `#'bad index'`.

## Loading

```smalltalk
Metacello new
	baseline: 'VMPluginsPrimitives';
	repository: 'github://massimo-nocentini/vm-plugins-primitives.st:master/src';
	load: #( 'Cairo' )
```

`BaselineOfVMPluginsPrimitives` has the groups `Cairo` (`VM-Plugins-Primitives-Cairo`), `CairoTests` (`VM-Plugins-Primitives-Cairo-Tests`) and `CairoBenchmarks` (`VM-Plugins-Primitives-Cairo-Benchmarks`). None is in `default` (`Core` and `Tests`, the UtilsPlugin's). From this repository, `plugins/CairoPlugin/tests/load-cairo.st` loads the submodule with Metacello (see [Tests](#tests)).

Loading the package on a VM with the plugin binds it and installs every group. On a VM without it the package installs nothing and says so on the Transcript: Athens keeps its FFI. It never installs there, since a patched method whose primitive is missing costs about 60 ns more per call.

## API

| Message | What |
|---|---|
| `CairoPlugin info` | `{ 1. 'CairoPlugin 1.0'. 'native' or 'emscripten'. the cairo version (e.g. 11806) or nil when unbound. 'loader' or 'static'. a missing function or nil. the missing optional functions }`, or nil when the VM has no CairoPlugin |
| `CairoPlugin isAvailable`, `isBound`, `cairoVersion` | whether the VM has the plugin; whether it is bound in this session; the bound version |
| `CairoPlugin bind` | binds to `CairoLibrary uniqueInstance libraryName` with the probe and the layout table; answers the cairo version or signals `CairoPluginError` (`code`: `#'no plugin'`, `#runner`, `#layout`, or the primitive's failure code) |
| `CairoPlugin unbind` | unbinds: every patch then falls back (for the tests) |
| `CairoPlugin groups` | `#(#basic #shape #text)` |
| `CairoPlugin install`, `install: groups` | binds if needed, installs the groups whose guards pass, and wants them at the next starts; answers the groups installed; `lastReport` says what was skipped |
| `CairoPlugin forceInstall: groups` | installs without a bound plugin (the fallback tests); the guards still apply |
| `CairoPlugin uninstall`, `uninstall: groups` | restores the originals and stops wanting the groups |
| `CairoPlugin installedGroups`, `wantedGroups`, `isInstalled` | the groups installed (in the order of `groups`), those installed at each start |
| `CairoPlugin verify` | the problems of the installed patches, empty when none: drift (a patched method replaced since), installed but unbound, a runner other than `TFSameThreadRunner`, a stale patch (compiled by an older version of the package: `install` replaces it), a subclass override or a guard mismatch (the group is uninstalled, a `#basic` method alone, and stays wanted), missing optional functions; it heals a patch that uFFI's first-call generation overwrote, and says so |
| `CairoPlugin counters`, `primitiveCounters`, `resetCounters` | the cairo calls the plugin made, by function (`'cairo_new_path' -> n`), and the successful calls of each primitive |

## The installer

The installer (`CairoPluginInstaller`) changes the methods of Athens-Cairo at run time. The package defines no extension method on an Athens class.

- **Patches.** For each method of a group, the original is kept under `ffi<Selector>` (`ffiNewPath`, `ffiMoveToX:Y:`, `ffiDraw`, `ffiRenderCharacters:from:to:`), compiled from its source, and the method is replaced by one with the pragma `<cairoPluginPatch: #group>`, the primitive, and the fallback to the original:

```smalltalk
moveToX: x Y: y
	<cairoPluginPatch: #basic>
	<primitive: 'primitiveBuilderMoveTo' module: 'CairoPlugin'>
	^ self ffiMoveToX: x Y: y
```

- **The groups.** `#basic` patches 60 methods (`convertString:len:ofFont:toGlyphs:numGlyphs:x:y:` and `freeGlyphs:` in two classes) and adds 2 (`getScaledFont` and `copyPath` answer a fresh `CairoScaledFont basicNew` or `AthensCairoPath basicNew` that the primitive fills). `#shape` patches `AthensCairoCanvas>>draw`, which dispatches a Rectangle or an AthensCairoPath with a Color or an AthensCairoStrokePaint to 4 added methods (anything else: `ffiDraw`). `#text` patches `CairoFreetypeFontRenderer>>renderCharacters:from:to:`, which calls the text primitive when the face has no kerning and the canvas paint is a Color.
- **Uninstall** puts back the original at once: uFFI's source stub, not a callout uFFI generated in this session (which holds the absolute path of this session's libcairo, and would load a second libcairo on a VM in another directory). It removes the added methods and the `ffi…` copies, but only once no process runs a patch or an added method of theirs: a drawing process preempted inside `draw` sends `cairoPlugin…` or `ffiDraw` when it resumes. Until then they wait (`CairoPluginInstaller pending`), and every install, uninstall and verify tries again. An `ffi…` copy is recognised by identity, or by its source once its class was recompiled (`recompileAll`, a class variable added), so a recompilation leaves nothing behind.
- **Upgrades.** An installed patch is kept only while it is what the loaded package compiles (the source of the patch and of its added methods): `install`, so every start, replaces a stale one, and the baseline's post-load does it right after Metacello loads a newer version (`CairoPlugin refreshAfterLoad`; the class-side `initialize` runs again only when its own source changes); `verify` reports one.
- **Quiet.** Install and uninstall run with the system announcements suspended and unpreempted: Athens-Cairo is not marked modified (Monticello, RPackage, Iceberg) and Epicea logs nothing. A first install writes the sources of the `ffi…` copies to `.changes` (130 to 155 bytes each); later installs reuse the compiled methods of a cache and compile nothing.
- **Guards.** Each patched or replicated method of Athens (the shape and text primitives redo the work of several methods) must have the source it had in Pharo 12 to 15 (`CairoPluginGuards`, a SHA-256 of the normalised source). A mismatch skips that method in `#basic`, and the whole group in `#shape` and `#text`. A subclass that overrides a replicated selector refuses its group too (today none does: the PDF, SVG, Export and Quartz surfaces and CharacterBlock override none). The guards are checked again by `verify` (so at every start) and `install`: a group, or a `#basic` method, whose guard stops matching after its install (a Pharo update, a user's fix) is uninstalled and stays wanted, so the change is never bypassed.
- **The runner.** The plugin runs on the VM's thread, as Athens' `TFSameThreadRunner` does. When `CairoLibrary`'s runner is another one, `install` refuses and `bind`, `verify` and the start uninstall every group (still wanted).
- **Sessions.** At each start (`startUp:`, after the FFI) the image binds again and installs the wanted groups, then verifies. When it cannot (no plugin, another cairo, the runner changed) it uninstalls them but keeps them wanted, so a later start on a VM with the plugin installs them again. Every ExternalAddress of a previous session is NULL at the start, and every primitive refuses a NULL handle: the plugin never calls cairo with a pointer of a previous session.
- **Committing Athens-Cairo** from an image with the patches installed would commit them: uninstall first.

## Primitive contract

Module `CairoPlugin`, protocol 1, version `'CairoPlugin 1.0'`. `tests/primitives.txt` is the list of the 69 primitives with their depths, groups, files and argument counts; the R lane of the C tests checks that the code has exactly these.

Every primitive checks, in this order: the argument count, that the plugin is bound, the receiver and the slots it reads, the arguments, then allocates its answer, then calls cairo. **It never fails after its first cairo call** (but for the answer of a getter without side effect, `get_current_point` and `get_operator`, which the retry repeats). Every cairo call is counted (`counters`), and every successful primitive (`primitiveCounters`). An answer mirrors the FFI method: the receiver for a `void` callout, a SmallInteger for an `int` one.

The failure codes:

| Code | When |
|---|---|
| `#'bad number of arguments'` | the argument count differs |
| `#'unsupported operation'` | not bound; an optional function needed is missing |
| `#'bad receiver'` | the receiver or a slot read from it is invalid: not of the kind expected, too short, a handle that is not an ExternalAddress of a pointer's size, a NULL handle, a matrix not of 48 bytes, a malformed Color or stroke paint |
| `#'bad argument'` | an argument, or a slot read from it, is invalid: kind, size, range, NULL; a character the text primitive cannot encode |
| `#'no modification'` | an object the primitive would write is read-only |
| `#'insufficient object memory'` | the allocation of the answer failed (the VM retries after a GC) |
| `#'insufficient C memory'` | a malloc of a buffer failed (the text, the dashes) |
| `#'not found'`, `#'inappropriate operation'`, `#'bad index'` | bind only: the library does not load, a required function is missing, or the probe is nil or NULL; the probe differs; a bad layout table |

Then the patched method runs its original, so the image sees stock Athens: its answer, or its error.

What the primitives accept:

- **A handle** is an ExternalAddress (exactly that class), of a pointer's size, not NULL.
- **A number** is a SmallInteger or a Float (converted as `asFloat` converts it); anything else fails, and the FFI original coerces it.
- **An int** (enums, counts) is a SmallInteger in the range of a C int. Enum values go to cairo unchecked, as with the FFI.
- **Bytes** are a byte object that is **not** an ExternalAddress (an ExternalAddress is a ByteArray whose 8 bytes are a pointer); a C string (#54, #58) must have a NUL inside its body, so the plugin never reads past it (stock Athens would).
- **A matrix** (`AthensCairoMatrix`) is a 48-byte ByteArray, or an ExternalAddress of `externalNew`.
- **A Color**: `rgb` a SmallInteger ≥ 0 and `alpha` a SmallInteger in 0..255; the components are computed with the IEEE operations of `Color>>red`…`alpha`, so they are bit-identical.
- **A Rectangle**: an origin and a corner of Points of numbers; the width and height are computed as Smalltalk does (exactly for SmallIntegers, then converted).

Writes into objects go through `storePointerofObjectwithValue`, after the object was validated (and found writable) before any allocation. Allocation uses only `instantiateClassindexableSize` and immediates. A forwarder is not of the kind expected: the accessor depths make the VM follow the forwarders and retry (every FFI call that passes a young matrix pins it and leaves a forwarder, so this is a common path).

### Admin primitives (class side of `CairoPlugin`)

| Primitive | Method | Arguments | Answer | Depth |
|---|---|---|---|---|
| `primitiveInfo` | `primInfo` | – | the 7 elements of `info` | 0 |
| `primitiveBind` | `primBind:probe:layout:` | the library path (a ByteString of 1 to 255 bytes; more fails with `#'bad argument'`); the probe (an ExternalAddress); the layout table | the cairo version | 1 |
| `primitiveUnbind` | `primUnbind` | – | the receiver | 0 |
| `primitiveCounters` | `primCounters` | – | `{ cairo calls by function. successful calls by primitive }` | 0 |
| `primitiveFunctionNames` | `primFunctionNames` | – | `{ 'cairo_new_path'… . 'primitiveNewPath'… }`, in the order of the counters | 0 |
| `primitiveResetCounters` | `primResetCounters` | – | the receiver | 0 |

### One-for-one primitives (`#basic`)

AthensCairoCanvas (the canvas's `handle`), depth 1 unless said otherwise; each answers the receiver unless said otherwise:

| # | Method | Primitive | Arguments | cairo |
|---|---|---|---|---|
| 1–9 | `newPath`, `fill`, `fillPreserve`, `stroke`, `primPaint`, `primSaveState`, `primRestoreState`, `primClip`, `resetClip` | `primitiveNewPath`, `Fill`, `FillPreserve`, `Stroke`, `Paint`, `Save`, `Restore`, `Clip`, `ResetClip` | – | `new_path` … `reset_clip` |
| 10 | `setPathMatrix` | `primitiveSetPathMatrix` (depth 2) | – | `set_matrix(cr, pathTransform)` |
| 11 | `setPathMatrix:` | `primitiveSetMatrix` | a matrix | `set_matrix` |
| 12 | `primSetSourceR:g:b:a:` | `primitiveSetSourceRGBA` | 4 numbers | `set_source_rgba` |
| 13 | `primSetSource:` | `primitiveSetSource` | an object with a handle | `set_source` |
| 14 | `primResetDashes:` | `primitiveResetDash` | a number | `set_dash(cr, NULL, 0, offset)` |
| 15 | `primSetDashesLengths:count:offset:` | `primitiveSetDash` | bytes, an int 0..size/8, a number | `set_dash` |
| 16 | `primRectangleX:y:width:height:` | `primitiveRectangle` | 4 numbers | `rectangle` |
| 17–19 | `primSetLineWidth:`, `primSetLineJoin:`, `primSetLineCap:` | `primitiveSetLineWidth`, `SetLineJoin`, `SetLineCap` | a number; an int; an int | `set_line_width`, `_join`, `_cap` |
| 20 | `loadPath:` | `primitiveAppendPath` | an AthensCairoPath | `append_path` |
| 21 | `getCurrentPoint` | `primitiveGetCurrentPoint` | – | `get_current_point`; answers a Point of two Floats |
| 22–23 | `setAA:`, `setFillRule:` | `primitiveSetAntialias`, `SetFillRule` | an int | `set_antialias`, `set_fill_rule` |
| 24 | `setScaledFont:` | `primitiveSetScaledFont` | a CairoScaledFont | `set_scaled_font` |
| 25 | `showGlyphs:size:` | `primitiveShowGlyphs` | an ExternalAddress (NULL only with n = 0), or bytes of n × 24; an int n ≥ 0 | `show_glyphs` |
| 26 | `getScaledFont` | `primitiveGetScaledFont`, as the added `cairoPluginGetScaledFontInto:` | a writable `CairoScaledFont basicNew` | `get_scaled_font`, stored in its handle; answers it |
| 27 | `primSetFontSize:` | `primitiveSetFontSize` | a number | `set_font_size` |
| 58 | `textPath:` | `primitiveTextPath` | a C string | `text_path` (Roassal's bordered labels) |

AthensCairoPathBuilder (the `handle` of its `context`), depth 2:

| # | Method | Primitive | Arguments | cairo |
|---|---|---|---|---|
| 28–30 | `moveToX:Y:`, `lineToX:Y:`, `primCurveViaX:Y:viaX:Y:toX:Y:` | `primitiveBuilderMoveTo`, `LineTo`, `CurveTo` | 2, 2, 6 numbers | `move_to`, `line_to`, `curve_to` |
| 31–32 | `closePath`, `newPath` | `primitiveBuilderClosePath`, `NewPath` | – | `close_path`, `new_path` |
| 33 | `copyPath` | `primitiveBuilderCopyPath`, as the added `cairoPluginCopyPathInto:` | a writable `AthensCairoPath basicNew` | `copy_path`, stored in its handle; answers it |
| 34–35 | `arcCenterX:centerY:radius:startAngle:endAngle:`, `arcNegative…` | `primitiveBuilderArc`, `ArcNegative` | 5 numbers | `arc`, `arc_negative` |

AthensCairoMatrix (the receiver's 48 bytes, written by the mutators), depth 1; the primitives call cairo's own `cairo_matrix_*`:

| # | Method | Primitive | Arguments | cairo |
|---|---|---|---|---|
| 37 | `initx:y:sx:sy:shx:shy:` | `primitiveMatrixInit` | 6 numbers | `matrix_init(m, sx, shy, shx, sy, x, y)`, the FFI's order |
| 38 | `primLoadIdentity` | `primitiveMatrixInitIdentity` | – | `matrix_init_identity` |
| 39–41 | `primTranslateX:Y:`, `primScaleX:Y:`, `primRotateByRadians:` | `primitiveMatrixTranslate`, `Scale`, `Rotate` | 2, 2, 1 numbers | `matrix_translate`, `_scale`, `_rotate` |
| 42 | `primMultiplyBy:` | `primitiveMatrixMultiply` | a matrix | `matrix_multiply(m, arg, m)` |
| 43 | `invert` | `primitiveMatrixInvert` | – | `matrix_invert`; answers its status |
| 44 | `primTransformX:Y:` | `primitiveMatrixTransformPoint` | two writable bytes of 8 or more | `matrix_transform_point` |
| 45–47 | `primSetTranslateX:Y:`, `primSetScaleX:Y:`, `primSetRotationInRadians:` | `primitiveMatrixInitTranslate`, `InitScale`, `InitRotate` | 2, 2, 1 numbers | `matrix_init_translate`, `_scale`, `_rotate` |

The others:

| # | Method | Primitive | Contract | Depth |
|---|---|---|---|---|
| 36 | `AthensCairoPath class>>primDestroyPath:` | `primitivePathDestroy` | an ExternalAddress, not NULL; `path_destroy`; the address is not cleared (stock does not) | 0 |
| 48–49 | `AthensCairoPaintMode>>getOperator`, `setOperator:` | `primitiveGetOperator`, `SetOperator` | the handle of its `canvas`; `get_operator` (answers a SmallInteger), `set_operator` | 2 |
| 50 | `AthensCairoSurface>>flush` | `primitiveSurfaceFlush` | `surface_flush` (the PDF, SVG and Export surfaces too) | 1 |
| 51–52 | `CairoScaledFont>>lock`, `unlock` | `primitiveScaledFontLockFace`, `UnlockFace` | `ft_scaled_font_lock_face`, `_unlock_face` (optional functions) | 1 |
| 53 | `CairoScaledFont>>getExtentsOfGlyphs:ofLength:into:` | `primitiveScaledFontGlyphExtents` | glyphs as #25; extents of 48 bytes or an ExternalAddress; `scaled_font_glyph_extents` | 1 |
| 54 | `CairoScaledFont>>getExtentsOf:into:` | `primitiveScaledFontTextExtents` | a C string; extents as #53; `scaled_font_text_extents` | 1 |
| 55 | `CairoScaledFont>>getExtents:` | `primitiveScaledFontExtents` | extents of 40 bytes or more; `scaled_font_extents`; answers 0 | 1 |
| 56 | `convertString:len:ofFont:toGlyphs:numGlyphs:x:y:` of CairoFreetypeFontRenderer and CairoFontMetricsProvider | `primitiveTextToGlyphs` | bytes and a length; a scaled font; cairo's in/out glyph pointer, a writable ExternalAddress (NULL allowed); a writable int32 of 4 bytes or more; 2 numbers; `scaled_font_text_to_glyphs`, both written back; answers its status | 1 |
| 57 | `freeGlyphs:` of the same two classes | `primitiveGlyphFree` | an ExternalAddress (NULL allowed); `glyph_free` | 0 |

`LibC memCopy:to:size:`, which `glyphsOf:from:to:` uses, is not cairo and stays on the FFI.

### Shape primitives (`#shape`)

They are the patched `draw` (`self setPathMatrix. ^ shape paintFillsUsing: paint on: self`) for the four cases below. Each makes exactly the cairo calls of Athens, in the same order; what it saves is Smalltalk work (the double dispatch, the `asFloat`s, the Color component sends, the argument Arrays) and six to nine crossings of the FFI. Depth 2; `m` is the canvas's `pathTransform`:

| Primitive (added method) | Arguments | cairo calls | Answer |
|---|---|---|---|
| `primitiveFillRectangleColor` (`cairoPluginFillRectangle:color:`) | a Rectangle, a Color | `set_matrix`, `set_source_rgba`, `set_dash(NULL, 0, 0.0)`, `new_path`, `rectangle`, `fill` | the canvas's surface |
| `primitiveStrokeRectangle` (`cairoPluginStrokeRectangle:paint:colorClass:`) | a Rectangle, a stroke paint whose `fillPaint` is of the class `Color` | `set_matrix`, `set_source_rgba`, `set_line_width`, `set_line_join`, `set_line_cap`, `set_dash` (nil dashes: `NULL, 0, 0.0`; else the Array's numbers, at most 4096, and `dashOffset`), `new_path`, `rectangle`, `stroke` | the paint |
| `primitiveFillPathColor` (`cairoPluginFillPath:color:`) | an AthensCairoPath, a Color | `set_matrix`, `set_source_rgba`, `set_dash(NULL, 0, 0.0)`, `new_path`, `append_path`, `fill` | the canvas's surface |
| `primitiveStrokePath` (`cairoPluginStrokePath:paint:colorClass:`) | an AthensCairoPath, a stroke paint | as the stroke of a rectangle, with `append_path` | the paint |

The fills also require the surface's `currentCanvas` to be the receiver: Athens fills through it, and it is nil outside `drawDuring:`, where stock Athens raises an error, which the fallback `ffiDraw` raises too. The dashes are in a stack buffer up to 64, else in a malloc made before the first cairo call. The patched `draw` calls them only on an AthensCairoCanvas itself (not a subclass), and falls back to `ffiDraw` for anything else.

### Text primitive (`#text`)

`primitiveShowString`, as the added `CairoFreetypeFontRenderer>>cairoPluginShowString:from:to:pixelSize:colorClass:`, depth 3. The patched `renderCharacters:from:to:` calls it when the face has no kerning (the stock fonts have none) and the canvas paint is a Color; it sets `advance` from the glyph extents when the primitive answers true, keeps it when it answers false, and runs the original when it fails.

- **The string** is an instance of exactly one of the four classes of the layout table: ByteString and ByteSymbol (Latin-1 bytes), WideString and WideSymbol (UTF-32 words). Anything else fails. `start` and `end` are SmallIntegers with 1 ≤ start ≤ end + 1 ≤ size + 1.
- **UTF-8 by simdutf.** The characters from `start` to `end` are encoded with `su_encode` of the SimdUTF engine (`plugins/SimdUTFPlugin/include/common/suEngine.h`) into a stack buffer of 1024 bytes, or a malloc of the exact size. For every valid string these are the bytes of Athens' converter. A character simdutf refuses (a surrogate, a value above U+10FFFF, the leading-char bits Pharo can put in a WideString's characters) fails the primitive with `#'bad argument'` **before any cairo call**, so the stock converter runs and the image sees what stock Athens does with such a string. More than 2^31 − 1 bytes of UTF-8 (cairo takes an int) fails too.
- **The calls**, those of stock `glyphsOf:from:to:` and `renderGlyphs:` (12; 10 when `text_to_glyphs` fails): `set_scaled_font`, `set_font_size`, `get_scaled_font`, `scaled_font_text_to_glyphs`, `scaled_font_glyph_extents` (into the renderer's `glyphExtents`), `ft_scaled_font_lock_face`, `set_matrix`, `set_scaled_font`, `set_source_rgba`, `show_glyphs`, `ft_scaled_font_unlock_face`, `glyph_free`. The one change of order: cairo's glyphs are shown directly and freed after `show_glyphs`, where Athens copies them and frees them before the lock; `glyph_free` touches no state of the cairo context.
- **The answer** is true when `text_to_glyphs` succeeded, false otherwise (stock then draws nothing and keeps the advance).

Roassal never sends `drawString:`: its labels go through the metrics and glyph primitives of `#basic` (51–57) and `textPath:` (58). The text primitive serves the users of `drawString:` (`AthensCanvas>>morphicDrawString:…`, `StringMorph`, `LabelMorph`, `MenuItemMorph>>drawOnAthensCanvas:`).

## Guarantees

The tests check these criteria, natively on Pharo 12 to 15 and on the WebAssembly node VM (see [Tests](#tests)):

| Id | Criterion |
|---|---|
| C1 pixels | Every scene of `CairoPluginScenes` (the benchmark's, each shape with each kind of paint, dashes nil, empty and given, Integer, Float and mixed rectangles, arcs, curves, nested clips, transformations, paint modes, gradients, text in Latin-1, empty, ByteSymbols, WideStrings and WideSymbols in many scripts, 2000-character strings, invalid characters, random WideStrings, the kerning path, Roassal with bordered labels, a fill outside `drawDuring:`) draws the same bytes in the modes `#ffi` (uninstalled), `#basic`, `#all` and `#fallback` (installed, unbound) |
| C2 state | After each shape and text primitive, the cairo state is the state after stock Athens: the matrix bit for bit, the source, the line width, join and cap, the dashes, the current point, the fill rule, the operator, the font matrix, and the scaled font. The scaled font is compared by content: its font face, its font matrix and CTM bit for bit, its options' hash and its status. Comparing addresses no longer works, because since cairo 1.18.6 (commit 46d1e26c) a fresh context may get a new, equal toy scaled font instead of the cached one, in every mode |
| C3 answers | Every patched method answers what the stock one answers (the same objects, equal Points of Floats, the same advance) |
| C4 calls | The calls of each cairo function in `#basic` and `#all` (the plugin's counters plus the FFI callouts left) equal the callouts of `#ffi`, and no patched method fell back; in `#basic` each patched method, sent once (scenes or not), makes one call of the primitive its row names and the cairo calls of its row, and leaves what its `ffi…` copy leaves |
| C5 failures | Every primitive fails with its code on each kind of invalid input, with no cairo call |
| C6 fallback | Unbound, or forced in on a VM without the plugin, every scene draws the `#ffi` pixels; on such a VM `install` refuses and loading installs nothing |
| C7 installer | `install` then `uninstall` leaves every class as it was (the same methods, uFFI's stubs, protocols), Athens-Cairo unmodified for Monticello and RPackage, Epicea unchanged; a reinstall compiles nothing; drift, uFFI overwrites, collisions, subclass overrides, guards that stop matching after the install, stale patches of an older package, a recompiled target class and a process preempted inside a patch are handled as above |
| C8 sessions | Saved installed and started again: bound and installed; started on a VM without the plugin in another directory: uninstalled, still wanted, Athens draws, and **exactly one libcairo** is loaded; back on the plugin VM: installed again |
| C9 same cairo | A probe of another function fails the bind with `#'inappropriate operation'`; a nil or NULL probe or a library that does not load with `#'not found'` (and no symbol is looked up in a NULL module); a path of 256 bytes with `#'bad argument'`; a bad table with `#'bad index'` |
| C10 runner, optional functions | Another runner makes `verify` uninstall and `install` refuse; missing optional functions are listed, and `#text` and lock/unlock are not installed |

## What changes with cairo 1.18.6

This release moves every VM to cairo 1.18.6, FreeType 2.15.0, libpng 1.6.59 and, on Linux, fontconfig 2.18.4 (`docs/CairoStack.md`). The guarantees above compare the modes on one cairo, so they hold unchanged. What Athens users can notice, measured on Linux x86_64 against the previous bundle (cairo 1.17.4, FreeType 2.12.1, pixman 0.40.0), on Pharo 12 and 15 with the same results:

- **Linux needs glibc 2.38 for cairo and FreeType** (Ubuntu 24.04+, Debian 13+, Fedora 39+, RHEL 10). The stack is built on the CI's Ubuntu 24.04 runners. The VM itself still starts on glibc 2.34 to 2.37, but there Athens cannot load cairo, and the plugin stays unbound.
- **Athens text and Morphic text:** identical at 12 and 20 pt, in Source Code Pro 10, in a StringMorph and in a SystemWindow. Text at 9 pt differs in 47 edge pixels. At about 4 px, FreeType 2.15.0 places the diacritics of accented capitals one row differently.
- **Geometry:** fractional and transformed rectangles differ only in antialiased edge pixels, by at most 14/255, from cairo 1.18.6's `cairo_rectangle`. Roassal graphs are identical (1 pixel in one scene).
- **Huge coordinates:** a rectangle from x = −2^40 to 2^40 now fills its band, where 1.17.4 drew only a few columns.
- **Toy text** (`cairo_select_font_face`, `cairo_show_text`; not Athens' `drawString:`, which uses its own FreeType faces) now follows the system's fontconfig configuration on Linux: the distribution's aliases and rendering settings, e.g. subpixel antialiasing on Ubuntu. The old bundle could not read `/etc/fonts`, printed `Fontconfig error: Cannot load default config file`, and used fontconfig's built-in configuration. Under the same configuration, the new libraries draw toy text identically. The previous Linux aarch64 cairo had no fontconfig and always drew toy text with cairo's built-in "twin" font; it should now use the system's fonts (the CI's toy line shows which). The first toy text of a user builds fontconfig's cache (version 12) in `~/.cache/fontconfig`: 16 to 21 ms here.
- **Colour emoji** (COLRv1, CBDT) are drawn natively where a colour font is installed. This was not measured: the test host has none.
- **Toy-font scaled fonts are no longer cached across contexts** (upstream commit 46d1e26c): toy text in short-lived contexts that alternate sizes is slower on 1.18.6. Athens' text is not affected (see [Smalltalk](#smalltalk)).
- **Windows** ships MSYS2's cairo 1.18.6 package with its patches, pinned. Toy text draws with DWrite, as with the previous MSYS2 builds.

## Limits

- **What stays on the FFI:** the display of a surface in a Morph (`asForm`, the SurfacePlugin's callbacks: about 2 ms of the 120 ms of a native frame of 800×600 and 3 ms of the 305 ms of a WebAssembly one, measured apart; the benchmark's `h-morph-frame` includes it), gradients and pattern paints, the `fillRule` getter, `setFontMatrix:`, `AthensCairoTransform`, `primDrawWithAlpha:`, `maskOn:`, the creation of surfaces and fonts, and text with kerning faces.
- **Argument classes are not checked**, as with the FFI: a non-NULL handle of another cairo type reaches cairo. Only the patched methods call the primitives, with what Athens passes; the class of a paint is checked where C reads it again.
- **Enum values go to cairo unchecked**, as with the FFI: a cairo built with assertions aborts on an invalid operator, join or cap.
- **Inherited from Athens:** a handle destroyed while still referenced (Pharo 12's `AthensCairoCanvas>>destroy` does not clear it), and the FT_Face, which must outlive cairo's font face while cairo's holdover cache can keep it longer.
- **libcairo must never be unloaded while the plugin is bound.**
- **A library path of at most 255 bytes**, as the FFI's.
- **One thread:** the plugin is for `TFSameThreadRunner` only.

## Tests

### C

`sh plugins/CairoPlugin/tests/run-c-tests.sh --vm-build <native tree> --deps <deps> [--zips <dir>] [--own-types] [--emsdk <emsdk> --wasm-build <wasm tree>]` runs its lanes; the header of the script describes them. The script reads the tree's cairo stack from `PHARO_CAIRO_STACK_PREFIX` in `--vm-build`'s `CMakeCache.txt`; the prefix counts as empty when `FEATURE_LIB_CAIRO` is off. The versions of the cairo and FreeType archives in `--deps` come from `cmake/cairo-stack/pins.txt`, never from a literal.

| Lane | What | Checks |
|---|---|---|
| R | the static rules: allocation only through the helpers, cairo.h only in `cpCairo.h`, every cairo call through `CP_CALL`, the 69 primitives of `primitives.txt` everywhere, no printing, `isBytes` only in `cp_bytes`, no `storePointerUnchecked`, no loader call before a module check | 9 rules |
| bind | `cpCairo.c` with a dlsym loader on the bundled libcairo: every function bound, matrices, drawing read back, text, missing functions | 213 |
| protocheck | four compiles against the stack's headers, cairo 1.18.6 (the prefix of the tree, with its own `cairo-features.h`; without a stack, the pinned archives of `--deps`):<br>(1) the Emscripten mode;<br>(2) the own types of the builds without the stack, field by field and function by function (`protocheck-own.c`);<br>(3) the native mode with the prototype check (`-DCP_CHECK_PROTOTYPES`);<br>(4) the same with `-include tests/protocheck-wrong.h`, which gives `cairo_set_dash` a wrong parameter type and must fail with exactly the error `cp_fn_set_dash differs from cairo.h`, which shows that the check bites | 4 compiles |
| targets | the headers and the binding with the own types, for every shipped ABI (Linux and Windows x86_64 and aarch64, macOS arm64 and x86_64, wasm64) | 7 ABIs |
| symbols | the 57 functions in:<br>- the VM's libcairo;<br>- with `--zips <dir>`, the libcairo of every CI zip there (`PharoVM-*-bin.zip`; llvm-nm or llvm-readobj);<br>- with `--deps`, the old files.pharo.org bundles, the cairo of the VMs before the stack.<br>`libCairoPlugin.so` must export exactly 142 symbols, have no undefined `cairo_` or `FT_` symbol, and need neither libcairo nor libfreetype | 6 libraries (the VM's and the 5 old bundles), plus one per zip |
| stack | `scripts/cairo-stack/check.sh --bundle` on the tree's `build/vm`: the stack's names, links, dependencies, RPATH, exports, version, toy text and notices (`docs/CairoStack.md`). It also prints the highest `GLIBC_` version the stack needs, and the toy-text probe's lines. It is skipped on a tree without the stack, and fails there if named with `--only` | 46 assertions on Linux x86_64 |
| wasm-syntax | the builtin glue with emcc, and its trampoline table | 69 |
| glue-admin, glue-basic, glue-shape, glue-text | the glue against a fake VM (Athens-shaped objects, ExternalAddresses as bytes, forwarders at every level), under ASan, UBSan and LSan: answers, failure codes with no cairo call, and the state and the pixels against the same calls made directly.<br>In a tree with the stack, each lane builds the plugin 3 times: as the tree ships it (the shipped mode, cairo.h and the prototype check) as a shared and as a builtin plugin, plus once with the own types as a shared plugin (what a build without the stack ships).<br>Without the stack, or with `--own-types`: the own types, shared and builtin | 1921 (1996 builtin), 7587, 1574, 1096, in each build |
| glue-wasm64 | the four glue tests built with emcc for wasm64 and run in node, on the static cairo of a wasm tree | 1944, 7587, 1574, 1096 |

`CP_TEST_FONT=<path>` gives the glue tests another font.

### Smalltalk

`sh plugins/CairoPlugin/tests/run-st-tests.sh <vm-dir> <image.zip>` copies the image, loads the groups `Cairo CairoTests` with `load-cairo.st` (from the submodule, or `CP_ST_REPO`), checks that the plugin was found and every group installed (`CP_EXPECT_PLUGIN`), runs `VM-Plugins-Primitives-Cairo-Tests` with `CairoPluginTestRunner` (a line a test, with the reason of each skip), then the session steps (C8; with `CP_VM0`, a VM without the plugin in another directory, the start there and back).

The 51 tests are in `CairoPluginParityTest` (C1, C3), `CairoPluginStateTest` (C2), `CairoPluginCallCountTest` (C4), `CairoPluginPrimitiveTest` (C5, C9), `CairoPluginFallbackTest` (C6), `CairoPluginInstallerTest` (C7, C10) and `CairoPluginSessionTest` (C8). Measured on Linux x86-64:

- **With the plugin**, on cairo 1.18.6 with Pharo 12, 13, 14 and 15: 48 pass, none skipped, and 3 are not applicable (the tests of a VM without the plugin). The session steps pass, and with `CP_VM0` (a VM built with `-DFEATURE_PLUGIN_CAIRO=OFF`) exactly one libcairo is loaded on either VM. A copy of `build/vm` without the plugin file is not such a VM: the build tree's RUNPATH still finds the plugin in the original directory.
- **The same tests on other cairos:** on the old files.pharo.org 1.17.4 bundle of the former `PHARO_CAIRO_STACK=prebuilt`, since removed (Pharo 12 and 15), and on a VM of the previous release (Pharo 15), 48 pass and 3 are not applicable.
- **Repeated:** `CairoPluginStateTest` and `CairoPluginParityTest` ran 20 rounds in one image without a flap (Pharo 12 to 15 on 1.18.6, and Pharo 15 on 1.17.4).
- **Without the plugin** (`CP_EXPECT_PLUGIN=no`), on Pharo 12 and 15: 23 pass and 28 skip with their reasons.
- **On the WebAssembly node VM** (cairo 1.18.6), on Pharo 12 and 15: 48 pass, none skipped, 3 not applicable.

C2 compares the scaled font by content because of a change in cairo 1.18.6 (commit 46d1e26c). Toy-font scaled fonts no longer come back from cairo's font cache, so 300 fresh canvases get about 260 different scaled-font addresses on 1.18.6, against 1 on 1.17.4. They all have the same content, and the content key separates different fonts. The two tests that compared addresses failed on 1.18.6 even with the FFI alone. Athens' own FreeType fonts are not affected, so the parity test's `getScaledFont` handles stay stable.

### CI and WebAssembly

**CI:** each native job of `build.yml` (Linux x86_64 and aarch64, macOS, Windows) runs `plugins/CairoPlugin/tests/ci-load.st` on the VMMaker image with the VM of its zip, and checks three things:
- `CairoPlugin native 1 cairo <x.y.z> ok`, where `<x.y.z>` is the cairo of `cmake/cairo-stack/pins.txt`, so the zip must ship exactly the pinned cairo;
- the toy line (below);
- the plugin file is in the zip (on Linux also that it needs no GLIBCXX newer than 3.4.21).

Each job also runs `scripts/cairo-stack/check.sh` on its zip (`docs/CairoStack.md`). The probe needs no package: it draws once with stock Athens (and prints `CairoPlugin FAILED cairo not loadable` when cairo cannot draw), binds the plugin and checks `info` and, natively, that one libcairo is loaded; then it draws a 64×64 scene (rectangles filled and stroked, dashes, a Bézier path, a transformation, the ByteString 'Café' and the WideString 'xλé') by the FFI, by the one-for-one primitives and by the shape and text primitives, through probe subclasses that it compiles, and requires the same SHA-256 of the pixels and a call of every primitive expected.

**The toy line.** Before it binds the plugin, independently of the plugin, the probe draws toy text through the image's FFI: `cairo_select_font_face` "sans-serif" and `cairo_show_text` "Ag" at 20 px on an ARGB32 surface. This exercises the text backend that the bundle's cairo picks for toy fonts: fontconfig and FreeType on Linux, Quartz on macOS, DWrite on Windows, and cairo's built-in "twin" font where there are no fonts. It prints

```
CairoToy ok family <family> type <toy|ft|win32|quartz|user|dwrite|unknown> ink <pixels>
```

- `<family>` is the family that FreeType loaded for the type `ft` (e.g. `DejaVu Sans`), `twin` for the type `user`, and the requested `sans-serif` otherwise;
- `<pixels>` is the number of pixels with any non-zero byte.

When the status of the context is not SUCCESS, or nothing is inked, it prints `CairoToy FAILED status <n> <message>, …` instead. The probe goes on either way: the jobs grep `^CairoToy ok family .+ ink [1-9]` and fail without it.

On the Linux x86_64 zip it prints `CairoToy ok family DejaVu Sans type ft ink 200`. The family and the ink depend on the host's fonts and fontconfig settings. With an empty fontconfig configuration it prints `family twin type user ink 134`. On Windows the line proves that MSYS2's cairo draws toy text.

The type comes from `cairo_scaled_font_get_type` and the status from `cairo_status`, both through the FFI, so the WebAssembly registry lists both (`cmake/emscripten/ffi/symbols/cairo.txt`). There the line reads `family twin type user ink 134`.

**Lane 74-cairo** of `make wasm-check` (see `docs/WebAssembly.md`) runs the same probe on the node VM (`CairoPlugin emscripten 1 cairo 1.18.6 ok`, `info` with 11806 and `'static'`), loads the packages, checks `info`, runs the suite in the node VM (48 tests, none skipped) and `CairoPluginParityTest` and `CairoPluginPrimitiveTest` in `pharo-web` (19 tests).

## Speed

`plugins/CairoPlugin/bench` measures Athens drawing through the FFI against the plugin, in one image and one VM, with the package `VM-Plugins-Primitives-Cairo-Benchmarks` (`CairoPluginBenchmark`, group `CairoBenchmarks`).

- **The modes:** `#ffi` (nothing installed: stock Athens), `#basic` (the one-for-one primitives) and `#all` (`#basic`, `#shape` and `#text`). They are switched by the installer, from its cache, and interleaved per repetition, in a rotating order (ffi basic all, basic all ffi, all ffi basic…). Each timed run comes after the switch, an untimed warm-up run and a garbage collection; the tables give the median of the runs, and "×" is the FFI median divided by the mode's.
- **The checks:** before timing, each mode draws the scene once more, counted and fingerprinted. In every row below the pixels are the same in the three modes, the cairo calls are the same (FFI callouts plus the plugin's calls, without the `*_destroy` of finalization), and no patched method fell back to the FFI in `#basic` or `#all` (a fallback, or different pixels, makes the run fail).
- **The scenes** (those of `CairoPluginScenes`, at surface sizes 1000×1000 and 1×1, the 1 px control: the same calls, almost nothing to rasterize):
  - `a-polyline`: one path of 100,000 points, stroked;
  - `a2-lines`: 25,000 paths of one line, stroked with a stroke paint;
  - `b-rects`: 20,000 rectangles, filled and stroked;
  - `c-bezier`: 10,000 ellipses of Bézier curves, filled;
  - `d-transforms`: 20,000 squares, each translated, rotated and scaled;
  - `e-text`: 5,000 short ByteStrings by `drawString:` (Roassal never sends `drawString:`: this is the scene of the morphs and of the users of `drawString:`); `e-text-wide`: the same strings with one character outside Latin-1 each (WideStrings); `e-text-long`: 500 strings of 2000 characters, one in four a WideString;
  - `f-roassal-first`, `f-roassal-cached`: a Roassal graph (1,000 boxes and ellipses with borders, 2,000 translucent lines, a label every 10 nodes) drawn by `RSAthensRenderer` on one persistent canvas, its paths built at each run or cached;
  - `g-roassal-svg-first`, `g-roassal-svg-cached`: the same on an SVG surface;
  - `h-morph-frame`: an `RSAthensMorph` of that graph drawn on a FormCanvas of 800×600, end to end: Roassal, then `asForm` and the SurfacePlugin's callbacks of BitBlt, which stay on the FFI but take only 1 to 2% of the frame (see [Limits](#limits));
  - the micro-benchmarks: ns per call of single patched methods, with four controls.

### The conditions

Measured on 2026-11-18, on an Intel Xeon Gold 6238R (2.20 GHz, 112 threads) under Linux 6.8 x86_64, a machine shared with other users. The load average was 1.2 to 2.5 before and after every run. The images are Pharo 12 (build 1599) and Pharo 15 (build 117). The VMs are built from pharo-vm 03d940501 with the changes that move every VM to cairo 1.18.6:

- **native-cog:** the CoInterpreter with its JIT (Cog), on the bundled stack: libcairo 1.18.6, pixman 0.44.2, FreeType 2.15.0. 7 runs per mode, the scenes at their full size (scale 1), 200,000 calls per micro-benchmark.
- **wasm-node:** the StackInterpreter built with Emscripten (`node/pharo`, node 24.19.0), with the static cairo 1.18.6 and FreeType 2.15.0.
  - 5 runs per mode;
  - the scenes `a` to `e` at a tenth of their size (scale 0.1), `f` to `h` at full size;
  - 100,000 calls per micro-benchmark;
  - each Pharo on the world image of its build (`image/web`).
- **wasm-web:** `pharo-web`, the module of the pages (`-sMEMORY64=2`), through `vm-driver.js` in node: `b-rects` and the micro-benchmarks.

Every run passed its checks: the same pixels and cairo calls in the three modes, and no fallback.

### Summary

The geometric mean of the scenes' "×" (size 1000 without `h-morph-frame`, which is in the scene tables):

| Pharo | VM | `#basic`, 1000 px | `#all`, 1000 px | `#basic`, 1 px | `#all`, 1 px |
|---|---|---:|---:|---:|---:|
| 15 | native-cog | 1.18× | 1.58× | 1.63× | 2.59× |
| 12 | native-cog | 1.19× | 1.56× | 1.56× | 2.49× |
| 15 | wasm-node | 1.78× | 2.93× | 2.39× | 5.11× |
| 12 | wasm-node | 1.81× | 3.02× | 2.39× | 5.20× |

No mode is measurably slower than the FFI in any scene. The smallest gains are 1.04×: `f-roassal-cached` and `e-text-long` in `#basic` natively, and `h-morph-frame`.

**Against the previous cairo.** The same Pharo 15 runs on a VM of the previous release (cairo 1.17.4, in the same session) gave 1.19×, 1.55×, 1.58× and 2.44×: cairo 1.18.6 neither helps nor hurts the plugin. The absolute medians moved by −7% to +9% (Roassal 5 to 7% faster, `a-polyline` and `c-bezier` 4 to 6% slower).

### Scenes, native

Pharo 15, native-cog, median ms. The last two columns are `#all` at 1 px, and `#all` at 1000 px on Pharo 12:

| Scene | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px | × `#all`, Pharo 12 |
|---|---:|---:|---:|---:|---:|---:|---:|
| a-polyline | 277 | 264 | 265 | 1.05 | 1.05 | 2.32 | 1.05 |
| a2-lines | 195 | 156 | 136 | 1.25 | 1.43 | 1.96 | 1.50 |
| b-rects | 167 | 124 | 103 | 1.35 | 1.62 | 2.64 | 1.62 |
| c-bezier | 207 | 168 | 162 | 1.24 | 1.28 | 1.80 | 1.31 |
| d-transforms | 226 | 153 | 144 | 1.47 | 1.57 | 2.58 | 1.64 |
| e-text | 105 | 83.7 | 25.9 | 1.26 | 4.06 | 5.53 | 3.70 |
| e-text-wide | 108 | 85.6 | 27.9 | 1.27 | 3.89 | 5.40 | 3.47 |
| e-text-long | 70.6 | 68.2 | 36.1 | 1.04 | 1.96 | 2.16 | 1.92 |
| f-roassal-first | 155 | 144 | 141 | 1.08 | 1.10 | 1.95 | 1.09 |
| f-roassal-cached | 142 | 136 | 134 | 1.04 | 1.06 | 1.98 | 1.07 |
| g-roassal-svg-first | 73.0 | 62.5 | 60.3 | 1.17 | 1.21 | – | 1.18 |
| g-roassal-svg-cached | 60.7 | 55.3 | 53.2 | 1.10 | 1.14 | – | 1.14 |
| h-morph-frame (800×600) | 117 | 112 | 110 | 1.04 | 1.06 | – | 1.06 |

### Scenes, WebAssembly

Pharo 15, wasm-node, median ms (the node clock has a resolution of 1 ms):

| Scene | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px | × `#all`, Pharo 12 |
|---|---:|---:|---:|---:|---:|---:|---:|
| a-polyline (scale 0.1) | 52 | 31 | 32 | 1.68 | 1.62 | 4.33 | 1.68 |
| a2-lines (0.1) | 133 | 54 | 48 | 2.46 | 2.77 | 3.24 | 3.02 |
| b-rects (0.1) | 113 | 42 | 31 | 2.69 | 3.65 | 5.16 | 3.90 |
| c-bezier (0.1) | 104 | 50 | 47 | 2.08 | 2.21 | 2.83 | 2.33 |
| d-transforms (0.1) | 132 | 49 | 43 | 2.69 | 3.07 | 4.03 | 3.19 |
| e-text (0.1) | 85 | 54 | 8 | 1.57 | 10.62 | 15.80 | 9.44 |
| e-text-wide (0.1) | 86 | 55 | 10 | 1.56 | 8.60 | 16 | 9.56 |
| e-text-long (0.1) | 47 | 43 | 9 | 1.09 | 5.22 | 8.60 | 5.44 |
| f-roassal-first | 471 | 319 | 310 | 1.48 | 1.52 | 2.44 | 1.54 |
| f-roassal-cached | 374 | 274 | 262 | 1.36 | 1.43 | 2.77 | 1.46 |
| g-roassal-svg-first | 331 | 187 | 176 | 1.77 | 1.88 | – | 1.86 |
| g-roassal-svg-cached | 241 | 141 | 130 | 1.71 | 1.85 | – | 1.87 |
| h-morph-frame (800×600) | 304 | 223 | 215 | 1.36 | 1.41 | – | 1.45 |

The rows of the plugin modes under 20 ms are ±1 ms. A run of Pharo 15 at scale 1 times the text and the polyline with every run of 48 ms or more:

| Scene, wasm-node, scale 1 | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px |
|---|---:|---:|---:|---:|---:|---:|
| a-polyline | 496 | 322 | 324 | 1.54 | 1.53 | 4.13 |
| e-text | 827 | 527 | 74 | 1.57 | 11.18 | 16.31 |
| e-text-wide | 842 | 541 | 79 | 1.56 | 10.66 | 16.22 |
| e-text-long | 461 | 429 | 79 | 1.07 | 5.84 | 8.39 |

In `pharo-web` (wasm-web, Pharo 15, scale 0.1), `b-rects` takes 104, 38 and 28 ms (× `#basic` 2.74, × `#all` 3.71), and at 1 px 95, 27 and 16 ms (3.52 and 5.94).

### Micro-benchmarks

ns per call, Pharo 15. Pharo 12 is within 8% of these natively and within 10% on wasm-node.

| Call | Cog FFI | Cog `#basic` | Cog `#all` | × | wasm-node FFI | wasm-node `#all` | × | wasm-web FFI | wasm-web `#all` | × |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `newPath` | 121 | 43.8 | 43.8 | 2.8 | 1550 | 160 | 9.7 | 1430 | 150 | 9.5 |
| `moveToX:Y:` | 187 | 62.5 | 62.6 | 3.0 | 1900 | 210 | 9.0 | 1770 | 200 | 8.9 |
| `primRectangleX:y:width:height:` | 272 | 148 | 147 | 1.8 | 2340 | 470 | 5.0 | 2190 | 400 | 5.5 |
| `primSetSourceR:g:b:a:` | 193 | 75.0 | 74.9 | 2.6 | 2160 | 270 | 8.0 | 2020 | 230 | 8.8 |
| `setPathMatrix` | 315 | 75.1 | 75.0 | 4.2 | 3730 | 230 | 16.2 | 3450 | 220 | 15.7 |
| `fill` of an empty path | 128 | 49.0 | 48.7 | 2.6 | 1560 | 170 | 9.2 | 1430 | 150 | 9.5 |
| `setPaint:` + `drawShape:` of a rectangle | 1933 | 1018 | 671 | 1.9 / 2.9 | 19600 | 3000 | 6.5 | 18200 | 2600 | 7.0 |
| `drawString: 'node 123'` | 14805 | 11839 | 1844 | 1.25 / 8.0 | 130500 | 4100 | 31.8 | 121000 | 3700 | 32.7 |

× is FFI / `#all`. For the last two rows natively it is FFI / `#basic` and FFI / `#all`. On WebAssembly `#basic` gives 5200 and 4700 ns for `setPaint:` + `drawShape:`, and 90000 and 82800 ns for `drawString:`.

Each control is timed in a loop, whose cost is included; the net of the loop (the first row) is in parentheses:

| Control | Cog | wasm-node | wasm-web |
|---|---:|---:|---:|
| an empty Smalltalk loop, per iteration | 2.7 | 100 | 80 |
| the Smalltalk side of a void FFI callout (`newPath` with the C call skipped) | 13.3 (10.6) | 270 (170) | 230 (150) |
| an FFI callout of an empty C function (`cairo_version()`) | 98.3 (95.6) | 1320 (1220) | 1220 (1140) |
| a named primitive that does nothing (the FilePlugin's `primitiveDirectoryDelimitor`) | 28.7 (26.0) | 120 (20) | 110 (30) |

### What the numbers say

- **A callout costs about 90 to 95 ns on Cog and 1.1 to 1.2 µs on the WebAssembly VMs; the call of a named primitive about 26 ns and 20 to 30 ns** (net of the loop; the WebAssembly figures are at the resolution of the node clock).
  - Each one-for-one primitive saves that difference, so the gain is largest where the crossings are a large share of the work.
  - Many small shapes (`a2-lines`, `b-rects`, `c-bezier`, `d-transforms`) gain 1.3 to 1.6× natively with every group at 1000 px, and 1.7 to 3.0× at 1 px, where nothing is rasterized. On WebAssembly they gain 2.2 to 3.9×.
- **Text:**
  - The text group's single primitive per `drawString:` gives 3.5 to 4.1× natively (4.9 to 5.5× at 1 px) and 8.6 to 11× on WebAssembly. WideStrings gain as much as ByteStrings.
  - On `e-text-long`, `#basic` gains nothing (1.04×): Athens' `CairoUTF8Converter`, in Smalltalk, dominates. `#all`, which encodes with simdutf, gains 1.9× natively and 5.2 to 5.8× on WebAssembly.
- **Roassal** gains little natively, since rasterizing dominates: 1.06 to 1.10× for the graph, 1.14 to 1.21× on SVG. On WebAssembly it gains 1.4 to 1.9×.
- **A whole morph frame** (`h-morph-frame`) gains 6% natively and 1.4× on WebAssembly. That is what its Roassal drawing gains (`f-roassal-cached`: 1.06 to 1.07× and 1.43 to 1.46×).
  - The display path (`asForm` and the SurfacePlugin's callbacks) stays on the FFI. It takes about 2 ms of a native frame of about 117 ms, and 3 ms of a WebAssembly frame of about 305 ms.
  - So the drawing bounds the frame (natively, its rasterizing), and moving the display path to the plugin could win at most about 2%.
- **`a-polyline` at 1000 px gains little natively** (1.05×). Building its path of 100,000 segments takes about 13 to 18 ms less (the 1 px rows: 26.6 to 32.3 ms by the FFI, 13.4 to 14.0 ms in `#basic`), but stroking that path takes about 250 ms, and that varies by about as much as the saving.

### Noise

- **Within a run**, the spread of the FFI runs (max − min) is mostly 1 to 4% of the median. The scenes heavy in garbage collection and finalization are noisier:
  - `a2-lines`: 15 to 28% at 1 px natively, and one spike of 97% at 1000 px on Pharo 12, which left the median unchanged against the previous pass (+8%);
  - `c-bezier`: up to 17% natively on Pharo 12, and 12 to 14% on WebAssembly on Pharo 15;
  - `f-roassal-first` at 1 px: 9 to 15% natively;
  - `a-polyline` at 1 px on Pharo 12: 13%.
- **Between two passes:** these runs on cairo 1.18.6 were compared with the same runs on the previous cairo, 1.17.4 natively and 1.18.4 on WebAssembly (`summarize.py --previous`, 306 medians). The median moved 1.9%, 90% moved less than 9.5%, and the largest move was 29.7% (`f-roassal-first` at 1 px on Pharo 12, `#all`).
  - Natively on Pharo 12, every mode of the short scenes was 5 to 12% slower in this pass, and the ratios moved less.
  - On WebAssembly, `c-bezier`'s FFI runs were 10 to 14% slower and its plugin runs 4 to 9% faster.

  Take the ratios as ±5 to 10%. `a2-lines` and the rows at 1 px are the least stable.
- **The pixels** are compared by a fingerprint: the first 12 hex digits of the SHA-256 of the CRC-32s of the surface's 64 KiB chunks. A full SHA-256 takes 3.5 s per surface on Cog in Smalltalk, and minutes on WebAssembly. The tests of `VM-Plugins-Primitives-Cairo-Tests` compare the bytes themselves.

### Running it

```sh
export CP_ST_REPO=<a clone of vm-plugins-primitives.st>/src     # else the submodule
sh plugins/CairoPlugin/bench/run-bench.sh build/vm Pharo15.0-SNAPSHOT-64bit-*.zip --out results
sh plugins/CairoPlugin/bench/run-bench.sh build-wasm/node Pharo15.0-SNAPSHOT-64bit-*.zip --wasm --out results
sh plugins/CairoPlugin/bench/run-bench.sh build-wasm/node Pharo15.0-SNAPSHOT-64bit-*.zip --wasm-web --out results
python3 plugins/CairoPlugin/bench/summarize.py -o REPORT.md [--previous <an earlier results directory>] results
```

`run-bench.sh` works on a copy of the image (on WebAssembly the build's `image/web` when its Pharo version matches, else the image given with `tests/web-surface.st`), loads the groups `Cairo`, `CairoTests` and `CairoBenchmarks` with `tests/load-cairo.st`, runs `bench/run-bench.st` (`--wasm-web`: `bench/web-bench.mjs` in `pharo-web`), and writes `bench-<pharo>-<vm>.txt` with the conditions; it exits 1 when the pixels differ between the modes, a plugin mode fell back to the FFI, or a scene failed. `CP_BENCH_SCENES`, `CP_BENCH_SIZES`, `CP_BENCH_REPS`, `CP_BENCH_SCALE`, `CP_BENCH_ROASSAL_SCALE`, `CP_BENCH_MODES` and `CP_BENCH_MICRO_CALLS` choose the work (the header of `run-bench.sh` and the comment of `CairoPluginBenchmark` give the defaults); `CP_BENCH_VM` names the VM in the lines (e.g. `native-cog-vm0` for a VM without the plugin, where only `#ffi` runs). `summarize.py` writes the Markdown tables, and with `--previous` how much each median moved since an earlier pass; it exits 1 when a line flags different pixels, a fallback or an error.

## Licences

The plugin and the packages are under the MIT licence of this repository and of vm-plugins-primitives.st. simdutf is the SimdUTFPlugin's (see `docs/SimdUTF.md`).

cairo is under the GNU LGPL 2.1 or the MPL 1.1, and the VM's notices elect the LGPL 2.1. It is not part of the plugin.

- **What the plugin takes from cairo.** It takes only what cairo.h declares: the prototypes (as function-pointer types), the enum values, a few macros, and the layouts of the structures it passes (`cairo_matrix_t`, `cairo_glyph_t`, the extents). That is the material that section 5 of the LGPL 2.1 lets "a work that uses the Library" take from the Library's header: numerical parameters, data structure layouts and small macros. The plugin contains no code of cairo, and **links no cairo**.
- **Native VMs:**
  - With the cairo stack, the plugin is compiled with the stack's cairo.h, and the prototype check emits no reference to cairo.
  - Without the stack (`BUILD_BUNDLE=OFF`, `FEATURE_LIB_CAIRO=OFF`, FreeBSD, OpenBSD, MSVC, Cygwin) it declares those prototypes and structures itself (`cpCairo.h`).
  - Either way it binds at run time, through the VM's module loader, to the libcairo that the image's FFI loads. That is a separate, replaceable shared library of the VM's bundle.
- **The bundle.** It carries cairo's licence (`LICENSE.cairo`), `THIRD-PARTY-NOTICES.cairo-stack.txt` with the licence elections and the LGPL source offer, and `CAIRO-STACK.txt`. Every GitHub release attaches `cairo-stack-sources.tar` with the sources of the stack (see `docs/CairoStack.md`, "Licences and sources").
- **On WebAssembly** the plugin is linked with the `pharo_cairo` that the VM already links for the FFI. Its notices are in `THIRD-PARTY-NOTICES.txt`, which names each pinned archive by its SHA256 and offers the sources on request (see `docs/WebAssembly.md`).

## Pushing

The CI's WebAssembly job checks out the submodules, and lane 74 needs the Cairo packages in `plugins/UtilsPlugin/vm-plugins-primitives.st`: push vm-plugins-primitives.st first, then the commit of pharo-vm that moves the submodule to it.
