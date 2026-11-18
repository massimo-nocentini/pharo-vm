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

- **Native VMs:** a shared library, `libCairoPlugin.so`, `.dylib` or `CairoPlugin.dll`, next to the other plugins. It has **no build dependency on cairo**: it includes no cairo header and links no cairo library. It declares its own prototypes and structures (`cpCairo.h`), and finds cairo at run time (see [How it reaches cairo](#how-it-reaches-cairo)). It links the SimdUTF engine, `pharo_su_engine` (`suEngine.cpp` and simdutf), privately: it is linked as C++, and exports only the module functions, the 69 primitives and their 69 accessor depths (142 symbols, no `su_`, `simdutf` or `cp_` symbol).
- **WebAssembly VMs:** the plugin is built in, `CairoPlugin (i)`, and only in a build with cairo (`WASM_CAIRO`, see `docs/WebAssembly.md`). It is compiled with cairo.h (`-DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES -DCP_STATIC_CAIRO`), so the compiler checks its 57 prototypes against cairo 1.18.4, and linked to the `pharo_cairo` of the FFI's registry. The SimdUTFPlugin, libSimdUTFFFI and the CairoPlugin share one copy of the engine, the static library `pharo_su_engine`. The plugin adds 62,248 bytes to `pharo-web.wasm` (10,581 with `gzip -9`).
- **Turning it off:** `-DFEATURE_PLUGIN_CAIRO=OFF`.

No CI package is needed: the native build needs no cairo header.

## How it reaches cairo

The plugin is unbound when the VM starts, and every drawing primitive then fails with `#'unsupported operation'`, so the patched methods run their FFI originals. The image binds it (`CairoPlugin bind`) when the package is loaded and at every start of the image.

- **Natively** `primitiveBind` receives the path of the libcairo that the image's FFI loads, `CairoLibrary uniqueInstance libraryName`. It loads it with the VM's own module loader, `ioLoadModuleOfLength`, the code and the module cache of the FFI's `primitiveLoadSymbolFromModule`, and looks up each function in that module. So the plugin and the FFI use **the same libcairo**, in the same process. The plugin never calls `dlopen` or `dlclose` and never links cairo.
- **The probe:** the image also passes the address of `cairo_create` that its FFI resolves. The bind fails with `#'inappropriate operation'` when it differs from the plugin's, and with `#'not found'` when it is nil or NULL (the FFI cannot reach cairo either). Nothing is bound then.
- **On WebAssembly** the table is filled statically with cairo's functions (`'static'`): the library path is checked but not loaded, and the probe is the registry's `cairo_create`, the same function.
- **Versions:** the bind requires cairo 1.16.0 or later (`cairo_version()` ≥ 11600), else it fails with `#'unsupported operation'`. The zips of `.github/workflows/build.yml` ship the files.pharo.org bundles on Linux (x86_64 1.17.4, aarch64 1.16.0) and macOS (arm64, x86_64 1.18.4); on Windows (x86_64 and arm64, both MSYS2 builds: UCRT64 and CLANGARM64) they ship the `libcairo-2.dll` of MSYS2's `cairo` package at build time (`cmake/importMinGWDependencies.cmake`; the files.pharo.org Windows bundles, x86_64 1.17.4 and ARM64 1.16.0, are used only by non-MinGW builds); the WebAssembly VMs link 1.18.4. The `symbols` lane checks that the VM's libcairo and the downloaded files.pharo.org bundles (Linux x86_64 and aarch64, macOS arm64 and x86_64, and Windows x86_64, which no CI zip ships) export the 57 functions. The MSYS2 DLLs of the Windows zips are checked only by the CI step of each job (`plugins/CairoPlugin/tests/ci-load.st`, which binds the plugin to the zip's cairo and requires every function, the optional ones too, and the same pixels).
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
| `CairoPlugin info` | `{ 1. 'CairoPlugin 1.0'. 'native' or 'emscripten'. the cairo version (e.g. 11704) or nil when unbound. 'loader' or 'static'. a missing function or nil. the missing optional functions }`, or nil when the VM has no CairoPlugin |
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
| C2 state | After each shape and text primitive, the cairo state (the matrix bit for bit, the source, the line width, join and cap, the dashes, the current point, the fill rule, the operator, the font matrix, the scaled font) is the state after stock Athens |
| C3 answers | Every patched method answers what the stock one answers (the same objects, equal Points of Floats, the same advance) |
| C4 calls | The calls of each cairo function in `#basic` and `#all` (the plugin's counters plus the FFI callouts left) equal the callouts of `#ffi`, and no patched method fell back; in `#basic` each patched method, sent once (scenes or not), makes one call of the primitive its row names and the cairo calls of its row, and leaves what its `ffi…` copy leaves |
| C5 failures | Every primitive fails with its code on each kind of invalid input, with no cairo call |
| C6 fallback | Unbound, or forced in on a VM without the plugin, every scene draws the `#ffi` pixels; on such a VM `install` refuses and loading installs nothing |
| C7 installer | `install` then `uninstall` leaves every class as it was (the same methods, uFFI's stubs, protocols), Athens-Cairo unmodified for Monticello and RPackage, Epicea unchanged; a reinstall compiles nothing; drift, uFFI overwrites, collisions, subclass overrides, guards that stop matching after the install, stale patches of an older package, a recompiled target class and a process preempted inside a patch are handled as above |
| C8 sessions | Saved installed and started again: bound and installed; started on a VM without the plugin in another directory: uninstalled, still wanted, Athens draws, and **exactly one libcairo** is loaded; back on the plugin VM: installed again |
| C9 same cairo | A probe of another function fails the bind with `#'inappropriate operation'`; a nil or NULL probe or a library that does not load with `#'not found'` (and no symbol is looked up in a NULL module); a path of 256 bytes with `#'bad argument'`; a bad table with `#'bad index'` |
| C10 runner, optional functions | Another runner makes `verify` uninstall and `install` refuse; missing optional functions are listed, and `#text` and lock/unlock are not installed |

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

`sh plugins/CairoPlugin/tests/run-c-tests.sh --vm-build <native tree> --deps <deps> [--emsdk <emsdk> --wasm-build <wasm tree>]` runs its lanes (the header of the script describes them):

| Lane | What | Checks |
|---|---|---|
| R | the static rules: allocation only through the helpers, cairo.h only in `cpCairo.h`, every cairo call through `CP_CALL`, the 69 primitives of `primitives.txt` everywhere, no printing, `isBytes` only in `cp_bytes`, no `storePointerUnchecked`, no loader call before a module check | 9 rules |
| bind | `cpCairo.c` with a dlsym loader on the bundled libcairo: every function bound, matrices, drawing read back, text, missing functions | 213 |
| protocheck | the static table against cairo 1.18.4's headers, and the own types of the native builds field by field and function by function | 2 units |
| targets | the headers and the binding for every shipped ABI (Linux and Windows x86_64 and aarch64, macOS arm64 and x86_64, wasm64) | 7 ABIs |
| symbols | the 57 functions in the VM's libcairo and in every platform's bundle; the 142 exports of `libCairoPlugin.so` | 6 libraries |
| wasm-syntax | the builtin glue with emcc, and its trampoline table | 69 |
| glue-admin, glue-basic, glue-shape, glue-text | the glue against a fake VM (Athens-shaped objects, ExternalAddresses as bytes, forwarders at every level), as a shared and as a builtin plugin, under ASan, UBSan and LSan: answers, failure codes with no cairo call, the state and the pixels against the same calls made directly | 1921 (1996 builtin), 7587, 1574, 1096 |
| glue-wasm64 | the four glue tests built with emcc for wasm64 and run in node, on the static cairo of a wasm tree | 1944, 7587, 1574, 1096 |

`CP_TEST_FONT=<path>` gives the glue tests another font.

### Smalltalk

`sh plugins/CairoPlugin/tests/run-st-tests.sh <vm-dir> <image.zip>` copies the image, loads the groups `Cairo CairoTests` with `load-cairo.st` (from the submodule, or `CP_ST_REPO`), checks that the plugin was found and every group installed (`CP_EXPECT_PLUGIN`), runs `VM-Plugins-Primitives-Cairo-Tests` with `CairoPluginTestRunner` (a line a test, with the reason of each skip), then the session steps (C8; with `CP_VM0`, a VM without the plugin in another directory, the start there and back).

The 51 tests are in `CairoPluginParityTest` (C1, C3), `CairoPluginStateTest` (C2), `CairoPluginCallCountTest` (C4), `CairoPluginPrimitiveTest` (C5, C9), `CairoPluginFallbackTest` (C6), `CairoPluginInstallerTest` (C7, C10) and `CairoPluginSessionTest` (C8). Measured on Linux x86-64:

- **With the plugin**, on Pharo 12, 13, 14 and 15: 48 pass, none skipped, and 3 are not applicable (the tests of a VM without the plugin); the session steps pass, and with `CP_VM0` (Pharo 12 and 15) exactly one libcairo is loaded on either VM.
- **Without it** (`CP_EXPECT_PLUGIN=no`), on Pharo 12 and 15: 23 pass and 28 skip with their reasons.
- **On the WebAssembly node VM**, on Pharo 12 and 15: 48 pass, none skipped, 3 not applicable.

### CI and WebAssembly

**CI:** each native job of `build.yml` (Linux x86_64 and aarch64, macOS, Windows) runs `plugins/CairoPlugin/tests/ci-load.st` on the VMMaker image with the VM of its zip, greps `CairoPlugin native 1 cairo <x.y.z> ok`, and checks that the plugin file is in the zip (on Linux also that it needs no GLIBCXX newer than 3.4.21). The probe needs no package: it draws once with stock Athens (and prints `CairoPlugin FAILED cairo not loadable` when cairo cannot draw), binds the plugin and checks `info` and, natively, that one libcairo is loaded; then it draws a 64×64 scene (rectangles filled and stroked, dashes, a Bézier path, a transformation, the ByteString 'Café' and the WideString 'xλé') by the FFI, by the one-for-one primitives and by the shape and text primitives, through probe subclasses that it compiles, and requires the same SHA-256 of the pixels and a call of every primitive expected. The Linux jobs install `libx11-6` and `libxext6`, which the bundled libcairo needs.

**Lane 74-cairo** of `make wasm-check` (see `docs/WebAssembly.md`) runs the same probe on the node VM (`CairoPlugin emscripten 1 cairo 1.18.4 ok`), loads the packages, checks `info`, runs the suite in the node VM (48 tests, none skipped) and `CairoPluginParityTest` and `CairoPluginPrimitiveTest` in `pharo-web` (19 tests).

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

Measured on 2026-11-18, on an Intel Xeon Gold 6238R (2.20 GHz, 112 threads) under Linux 6.8 x86_64, a machine shared with other users (load average 1.4 to 2.3 before and after every run). The images are Pharo 12 (build 1599) and Pharo 15 (build 117); the VMs are those of pharo-vm 301cfe70c with the plugin's sources:

- **native-cog**: the CoInterpreter with its JIT (Cog), the bundled libcairo 1.17.4; 7 runs per mode, the scenes at their full size (scale 1), 200,000 calls per micro-benchmark;
- **wasm-node**: the StackInterpreter built with Emscripten (`node/pharo`, node 24.19.0), the static cairo 1.18.4; 5 runs per mode, the scenes `a` to `e` at a tenth of their size (scale 0.1), `f` to `h` at full size, 100,000 calls per micro-benchmark; Pharo 15 on the world image of the build (`image/web`), Pharo 12 on the stock image with `tests/web-surface.st`;
- **wasm-web**: `pharo-web`, the module of the pages (`-sMEMORY64=2`), through `vm-driver.js` in node: `b-rects` and the micro-benchmarks.

### Summary

The geometric mean of the scenes' "×" (size 1000 without `h-morph-frame`, which is in the scene tables):

| Pharo | VM | `#basic`, 1000 px | `#all`, 1000 px | `#basic`, 1 px | `#all`, 1 px |
|---|---|---:|---:|---:|---:|
| 15 | native-cog | 1.19× | 1.55× | 1.57× | 2.46× |
| 12 | native-cog | 1.16× | 1.54× | 1.49× | 2.42× |
| 15 | wasm-node | 1.74× | 2.83× | 2.30× | 4.84× |
| 12 | wasm-node | 1.78× | 2.92× | 2.36× | 5.17× |

No mode is measurably slower than the FFI in any scene (the slowest case, `a-polyline` at 1000 px, is 1.00×).

### Scenes, native

Pharo 15, native-cog, median ms (the last two columns: `#all` at 1 px, and `#all` at 1000 px on Pharo 12):

| Scene | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px | × `#all`, Pharo 12 |
|---|---:|---:|---:|---:|---:|---:|---:|
| a-polyline | 261 | 262 | 261 | 1.00 | 1.00 | 1.90 | 1.00 |
| a2-lines | 195 | 146 | 136 | 1.34 | 1.43 | 1.65 | 1.48 |
| b-rects | 170 | 129 | 110 | 1.32 | 1.55 | 2.38 | 1.60 |
| c-bezier | 201 | 160 | 155 | 1.25 | 1.30 | 1.75 | 1.29 |
| d-transforms | 225 | 154 | 142 | 1.46 | 1.58 | 2.57 | 1.56 |
| e-text | 103 | 80.3 | 26.5 | 1.28 | 3.87 | 5.57 | 3.67 |
| e-text-wide | 106 | 83.5 | 29.0 | 1.27 | 3.65 | 5.29 | 3.45 |
| e-text-long | 69.5 | 66.9 | 36.6 | 1.04 | 1.90 | 2.16 | 1.88 |
| f-roassal-first | 164 | 154 | 151 | 1.07 | 1.09 | 1.96 | 1.10 |
| f-roassal-cached | 151 | 145 | 144 | 1.04 | 1.05 | 1.92 | 1.06 |
| g-roassal-svg-first | 72.4 | 62.1 | 59.9 | 1.17 | 1.21 | – | 1.15 |
| g-roassal-svg-cached | 61.8 | 55.7 | 54.2 | 1.11 | 1.14 | – | 1.15 |
| h-morph-frame (800×600) | 125 | 120 | 119 | 1.04 | 1.05 | – | 1.06 |

### Scenes, WebAssembly

Pharo 15, wasm-node, median ms (the node clock has a resolution of 1 ms):

| Scene | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px | × `#all`, Pharo 12 |
|---|---:|---:|---:|---:|---:|---:|---:|
| a-polyline (scale 0.1) | 53 | 32 | 31 | 1.66 | 1.71 | 4.33 | 1.62 |
| a2-lines (0.1) | 135 | 54 | 55 | 2.50 | 2.45 | 2.77 | 3.00 |
| b-rects (0.1) | 113 | 42 | 31 | 2.69 | 3.65 | 5.44 | 3.84 |
| c-bezier (0.1) | 93 | 52 | 51 | 1.79 | 1.82 | 2.27 | 1.94 |
| d-transforms (0.1) | 133 | 50 | 43 | 2.66 | 3.09 | 4.00 | 3.16 |
| e-text (0.1) | 84 | 54 | 9 | 1.56 | 9.3 | 15.8 | 9.4 |
| e-text-wide (0.1) | 85 | 55 | 10 | 1.55 | 8.5 | 13.2 | 8.7 |
| e-text-long (0.1) | 46 | 44 | 9 | 1.05 | 5.1 | 8.4 | 5.2 |
| f-roassal-first | 472 | 326 | 312 | 1.45 | 1.51 | 2.51 | 1.53 |
| f-roassal-cached | 379 | 278 | 267 | 1.36 | 1.42 | 2.71 | 1.45 |
| g-roassal-svg-first | 330 | 187 | 175 | 1.76 | 1.89 | – | 1.93 |
| g-roassal-svg-cached | 238 | 141 | 130 | 1.69 | 1.83 | – | 1.85 |
| h-morph-frame (800×600) | 305 | 225 | 216 | 1.36 | 1.41 | – | 1.44 |

The rows of the plugin modes under 20 ms are ±1 ms; a run of Pharo 15 at scale 1 gives the text and the polyline with every run of 49 ms or more:

| Scene, wasm-node, scale 1 | FFI ms | `#basic` ms | `#all` ms | × `#basic` | × `#all` | × `#all`, 1 px |
|---|---:|---:|---:|---:|---:|---:|
| a-polyline | 493 | 325 | 325 | 1.52 | 1.52 | 4.28 |
| e-text | 832 | 533 | 76 | 1.56 | 10.95 | 16.41 |
| e-text-wide | 852 | 551 | 82 | 1.55 | 10.39 | 15.86 |
| e-text-long | 467 | 435 | 79 | 1.07 | 5.91 | 8.38 |

In `pharo-web` (wasm-web, Pharo 15, scale 0.1), `b-rects` takes 106, 38 and 28 ms (× `#basic` 2.79, × `#all` 3.79), and at 1 px 94, 27 and 17 ms (3.48 and 5.53).

### Micro-benchmarks

ns per call, Pharo 15 (Pharo 12 within 7% of these natively, 11% on wasm-node):

| Call | Cog FFI | Cog `#basic` | Cog `#all` | × | wasm-node FFI | wasm-node `#all` | × | wasm-web FFI | wasm-web `#all` | × |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `newPath` | 124 | 45.5 | 45.7 | 2.7 | 1540 | 160 | 9.6 | 1440 | 160 | 9.0 |
| `moveToX:Y:` | 196 | 62.8 | 62.7 | 3.1 | 1920 | 210 | 9.1 | 1790 | 200 | 9.0 |
| `primRectangleX:y:width:height:` | 284 | 158 | 158 | 1.8 | 2390 | 450 | 5.3 | 2210 | 400 | 5.5 |
| `primSetSourceR:g:b:a:` | 196 | 78.2 | 77.6 | 2.5 | 2150 | 260 | 8.3 | 2020 | 230 | 8.8 |
| `setPathMatrix` | 307 | 78.7 | 78.7 | 3.9 | 3700 | 230 | 16.1 | 3420 | 210 | 16.3 |
| `fill` of an empty path | 132 | 53.5 | 53.8 | 2.5 | 1550 | 170 | 9.1 | 1450 | 150 | 9.7 |
| `setPaint:` + `drawShape:` of a rectangle | 2074 | 1125 | 799 | 1.8 / 2.6 | 19400 | 3000 | 6.5 | 18500 | 2600 | 7.1 |
| `drawString: 'node 123'` | 14683 | 11767 | 1976 | 1.25 / 7.4 | 129600 | 4200 | 30.9 | 122400 | 3400 | 36.0 |

(× is FFI / `#all`; for the last two rows natively, FFI / `#basic` and FFI / `#all`. On WebAssembly `#basic` gives 5400 and 4700 ns for `setPaint:` + `drawShape:`, 90200 and 83500 ns for `drawString:`.)

Each control is timed in a loop, whose cost is included; the net of the loop (the first row) is in parentheses:

| Control | Cog | wasm-node | wasm-web |
|---|---:|---:|---:|
| an empty Smalltalk loop, per iteration | 3.5 | 100 | 80 |
| the Smalltalk side of a void FFI callout (`newPath` with the C call skipped) | 12.6 (9.1) | 270 (170) | 230 (150) |
| an FFI callout of an empty C function (`cairo_version()`) | 100 (96.5) | 1360 (1260) | 1270 (1190) |
| a named primitive that clears the plugin's counters (`CairoPlugin primResetCounters`: two `memset`s of about 1 KB) | 31.1 (27.6) | 120 (20) | 110 (30) |
| a named primitive that does nothing (the FilePlugin's `primitiveDirectoryDelimitor`; a later run of the micro-benchmarks, Pharo 15) | 29.5 (26.9) | 120 (30) | – |

The benchmark now times the second of these primitives (`ctl-empty-prim-FilePlugin-delimitor`) instead of the first, and `summarize.py` gives every control net of the loop.

### What the numbers say

- **A callout costs about 95 ns on Cog and 1.2 to 1.3 µs on the WebAssembly VMs; the call of a named primitive about 25 ns and 20 to 30 ns** (net of the loop; the WebAssembly figures are at the resolution of the node clock). Each one-for-one primitive saves that difference, so the gain is largest where the crossings are a large share of the work: many small shapes (`a2-lines`, `b-rects`, `c-bezier`, `d-transforms`) gain 1.3 to 1.6× natively with every group at 1000 px (1.6 to 2.6× at 1 px, where nothing is rasterized), 1.8 to 3.8× on WebAssembly.
- **Text:** the text group's single primitive per `drawString:` gives 3.4 to 3.9× natively (5.0 to 5.6× at 1 px) and 8.5 to 11× on WebAssembly; WideStrings gain as much as ByteStrings. On `e-text-long`, `#basic` gains nothing (1.04×): Athens' `CairoUTF8Converter`, in Smalltalk, dominates; `#all`, which encodes with simdutf, gains 1.9× natively and 5.9× on WebAssembly.
- **Roassal** gains little natively (1.05 to 1.10× for the graph, 1.14 to 1.21× on SVG), since rasterizing dominates, and 1.4 to 1.9× on WebAssembly. A whole morph frame (`h-morph-frame`) gains 4 to 6% natively and 1.4× on WebAssembly, what its Roassal drawing gains (`f-roassal-cached`: 1.04 to 1.06× and 1.42×): the display path (`asForm` and the SurfacePlugin's callbacks), which stays on the FFI, takes about 2 ms of the 120 ms of a native frame and 3 ms of the 305 ms of a WebAssembly one, so the drawing bounds the frame (natively its rasterizing), and moving the display path to the plugin could win at most about 2%.
- **`a-polyline` at 1000 px shows no gain natively** (1.00×), although building its path of 100,000 segments takes about 13 ms less (the 1 px row: 27.7 to 14.6 ms; a separate measure gives 22.5 to 26.4 ms by the FFI against 10.5 to 12.3 ms in `#basic`): stroking that path takes 235 to 266 ms and varies by more than the saving.

### Noise

- **Within a run**, the spread of the FFI runs (max − min) is mostly 1 to 3% of the median. The scenes heavy in garbage collection and finalization are noisier: `a2-lines` up to 24% (Pharo 12 natively at 1 px; 23% at 1000 px; 22% on WebAssembly at 1 px), with one spike of 106% at 1 px in a first pass; `c-bezier` up to 12% natively, and on WebAssembly 15% at 1000 px and 27% at 1 px (runs of 75 ms, at the node clock's resolution of 1 ms); `a-polyline` on WebAssembly at 1 px 12%; `f-roassal-first` at 1 px up to 18%.
- **Between two passes** of the same runs (native Pharo 12 and 15, wasm-node Pharo 15; `summarize.py --previous`): of 189 medians, the median moved 1.1%, 90% moved less than 4.7%, and the largest move was 25.5% (`a2-lines` at 1 px on Pharo 12, `#all`). Take the ratios as ±5 to 10%; `a2-lines` and the rows at 1 px are the least stable.
- **The pixels** are compared by a fingerprint, the first 12 hex digits of the SHA-256 of the CRC-32s of the surface's 64 KiB chunks (a full SHA-256 takes 3.5 s per surface on Cog in Smalltalk, minutes on WebAssembly); the tests of `VM-Plugins-Primitives-Cairo-Tests` compare the bytes themselves.

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

The plugin and the packages are under the MIT licence of this repository and of vm-plugins-primitives.st. cairo, under the GNU LGPL 2.1 or the MPL 1.1, is not part of the plugin: natively the plugin declares the prototypes of cairo's public functions itself and loads at run time the libcairo that the VM's bundle already ships, and on WebAssembly it is linked with the `pharo_cairo` that the VM already links for the FFI, whose notices are in `THIRD-PARTY-NOTICES.txt`. simdutf is the SimdUTFPlugin's (see `docs/SimdUTF.md`).

## Pushing

The CI's WebAssembly job checks out the submodules, and lane 74 needs the Cairo packages in `plugins/UtilsPlugin/vm-plugins-primitives.st`: push vm-plugins-primitives.st first, then the commit of pharo-vm that moves the submodule to it.
