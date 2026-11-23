# Pango and Poppler

The native VMs of this fork carry, next to the cairo that Athens uses, **Pango 1.58.2** with PangoCairo (text layout: wrapping, alignment, ellipsizing, markup, any script that HarfBuzz shapes) and **Poppler 26.11.0** with its GLib interface, poppler-glib (reading and rendering PDF documents), with the **GLib 2.90.1** and **HarfBuzz 15.2.0** they need. The image binds them by uFFI, through two repositories of Smalltalk bindings; the VM has no plugin or primitive for them. This page explains what the VM carries, how the image finds it, and its limits; [docs/CairoStack.md](CairoStack.md) explains how the libraries are built, checked and bumped.

| | Linux x86_64, aarch64 | macOS x86_64, arm64 | Windows x86_64, ARM64 | WebAssembly |
|---|---|---|---|---|
| Pango, PangoCairo | built from the pinned archives, fontconfig and FreeType fonts | built from the pinned archives, CoreText fonts | MSYS2's packages | built from the pinned archives with `WASM_PANGO=ON`, fontconfig and FreeType, the DejaVu fonts of the VM (and the image's own with `PangoImageFonts`) |
| Poppler, poppler-glib | built from the pinned archive | built from the pinned archive | built from the pinned archive against MSYS2's packages | built from the pinned archive with `WASM_POPPLER=ON` |

## The features

Two options of the VM's CMake, both `ON` by default and independent of each other:

- `-DFEATURE_LIB_PANGO=OFF`: no GLib, FriBidi, HarfBuzz, Pango or PangoCairo in the VM (unless Poppler needs GLib);
- `-DFEATURE_LIB_POPPLER=OFF`: no Poppler and no libjpeg-turbo: **a VM without GPL code** (see [Licence](#licence)).

`FEATURE_LIB_CAIRO=OFF` turns both off (`cairo stack: FEATURE_LIB_PANGO and FEATURE_LIB_POPPLER off (FEATURE_LIB_CAIRO is off)`). The WebAssembly VM has neither feature (`cmake/Emscripten.cache.cmake`), but options of its own, `WASM_PANGO` and `WASM_POPPLER`, both off by default, which link the same libraries into it: see [WebAssembly](#webassembly). The configure prints the features it bundles (`cairo stack: features pango poppler`), the bundle's `CAIRO-STACK.txt` names them (`Bundled features: cairo freetype pango poppler`), and `scripts/cairo-stack/build.sh --features` builds a stack with only some of them. The features need more tools than cairo alone (a C++ compiler, python3, cmake ≥ 3.28, on Linux `g++-13`, and with Poppler NASM on Linux x86_64 and gperf on macOS): [docs/CairoStack.md, "Tools"](CairoStack.md#tools).

On Linux, Poppler needs the system's libstdc++ with `GLIBCXX_3.4.32` (GCC 13's or later), as the whole stack needs glibc 2.38: Ubuntu 24.04+, Debian 13+, Fedora 39+, RHEL 10.

## The libraries the image loads

The bindings load five libraries by name, through uFFI's finders: the image directory, then the VM directory (macOS: `Contents/MacOS/Plugins`), then the system's paths (and on macOS LibPoppler loads Poppler's `libfontconfig.1.dylib` by its path, next to libpoppler-glib). Every other library is a dependency of these, found by the dynamic linker inside the bundle (Linux `DT_RPATH $ORIGIN`, macOS `@rpath`, Windows the exe's directory).

| Class (package) | Linux | macOS | Windows |
|---|---|---|---|
| `GLibLibrary` (LibGLib) | `libglib-2.0.so.0`, `libglib-2.0.so` | `libglib-2.0.0.dylib`, `libglib-2.0.dylib` | `libglib-2.0-0.dll` |
| `GObjectLibrary` (LibGLib) | `libgobject-2.0.so.0`, `libgobject-2.0.so` | `libgobject-2.0.0.dylib`, `libgobject-2.0.dylib` | `libgobject-2.0-0.dll` |
| `PangoLibrary` (LibPangoCairo) | `libpango-1.0.so.0`, `libpango-1.0.so` | `libpango-1.0.0.dylib`, `libpango-1.0.dylib` | `libpango-1.0-0.dll` |
| `PangoCairoLibrary` (LibPangoCairo) | `libpangocairo-1.0.so.0`, `libpangocairo-1.0.so` | `libpangocairo-1.0.0.dylib`, `libpangocairo-1.0.dylib` | `libpangocairo-1.0-0.dll` |
| `PopplerLibrary` (LibPoppler) | `libpoppler-glib.so.8`, `libpoppler-glib.so` | `libpoppler-glib.8.dylib`, `libpoppler-glib.dylib` | `libpoppler-glib-8.dll` |

cairo functions are called through Athens' `CairoLibrary`. Each function is looked up in the library that defines it (`g_*` in GLib or GObject, `pango_cairo_*` in PangoCairo): on Windows `GetProcAddress` does not search a DLL's dependencies.

A VM without the stack (another build, a BSD) makes the bindings use the system's libraries: they must then be one GLib ≥ 2.88, HarfBuzz ≥ 11 and Pango ≥ 1.50, and a poppler-glib, linked to the same libcairo as Athens.

## The bindings

| Repository | Packages | Metacello |
|---|---|---|
| [libpangocairo.st](https://github.com/massimo-nocentini/libpangocairo.st) | `LibGLib` and `LibGLib-Cairo` (group `GLib`: the GLib and GObject layer that both repositories share), `LibPangoCairo` (`Core`), `LibPangoCairo-Roassal` (`RSPangoText`), `LibPangoCairo-Microdown`, examples and tests | `baseline: 'LibPangoCairo'; repository: 'github://massimo-nocentini/libpangocairo.st:master/src'` |
| [libpoppler.st](https://github.com/massimo-nocentini/libpoppler.st) | `LibPoppler` (`PopplerDocument`, `PopplerPage`), `LibPoppler-Roassal` (`RSPdfPage`), examples and tests; it loads only the `GLib` group of libpangocairo.st | `baseline: 'LibPoppler'; repository: 'github://massimo-nocentini/libpoppler.st:master/src'` |

Both work on Pharo 12, 13, 14 and 15; their READMEs have the API, the memory and session rules and the units (Pango: user units at 96 dpi and font sizes in points; Poppler: PDF points). Each has `ci/run.sh <vm-dir> <image>`, which loads its checkout offline and runs its tests on a given VM. They are not submodules of this repository yet: `.github/workflows/build.yml` has the steps that will test them on the Linux x86_64 zip ("Check out the bindings", "Test libpangocairo.st and libpoppler.st on the zip"), which do nothing until `bindings/libpangocairo.st` and `bindings/libpoppler.st` exist.

## The C functions the images bind

`cmake/emscripten/ffi/symbols/` has one file per library with the exact list of functions the bindings call, one name per line: `glib.txt`, `gobject.txt`, `harfbuzz.txt` (`hb_version_atleast`, `hb_version_string`: the instance check), `pango.txt`, `pangocairo.txt`, `fontconfig.txt` and `pangoft2.txt` (`FcConfigAppFontAddFile` and `pango_fc_font_map_config_changed`, with which `PangoImageFonts` registers the fonts of the image on Linux and in the WebAssembly VM) and `poppler-glib.txt`, next to `cairo.txt`. They are the contract between this repository and the bindings:

- `scripts/cairo-stack/check.sh` asserts that each library of a stack or bundle exports every name of its list (on Linux also `libfontconfig.so.1` and `libpangoft2-1.0.so.0`; on Windows, the DLLs of `libglib-2.0-0`, `libgobject-2.0-0`, `libharfbuzz-0`, `libpango-1.0-0`, `libpangocairo-1.0-0` and `libpoppler-glib-8`). The cairo functions that the bindings and their tests use beyond `cairo.txt` (PDF surfaces, font options, `cairo_surface_set_mime_data`...) are the `extra_cairo_symbols` of `check.sh`; `cairo.txt`, which feeds the wasm registry, has those that the WebAssembly VM's Pango and Poppler need (`cairo_set_source_surface`, `cairo_surface_set_mime_data`, and the PDF surface's functions when cairo has it).
- A binding that calls a new function adds it to the list first; the stack's check then proves that every VM exports it.
- `cmake/emscripten/ffiLibraries.cmake` makes the tables of the registry of the WebAssembly VM from them, with `WASM_PANGO` and `WASM_POPPLER` ([WebAssembly](#webassembly)): a function the lists lack is missing there. The extra cairo functions of the bindings are in `cairo.txt` too, for that reason.

## One GLib per process

A process must hold one GLib (one GObject type system), one HarfBuzz and one cairo for Pango and Poppler: a second GLib makes the type system and its objects disagree (a crash, at best), and Pango drawing on another libcairo than Athens' passes a `cairo_t` between two instances. The bundle is built so that the libraries find each other (one soname, install name or DLL name each), but the process can still bring another one first:

- **Linux:** a library linking the system's `libglib-2.0.so.0` that is loaded before Pango (a GTK-based plugin, or `LD_PRELOAD`) makes glibc hand that GLib to the bundled Pango and Poppler, by soname.
- **macOS:** a Homebrew GLib that another library loads is a second GLib beside the bundle's (other install name).
- **Windows:** a stale `libglib-2.0-0.dll` & co. next to the image (the old libpoppler.st README told users to copy DLLs there) is found first, by the image directory, and loaded instead of the VM's.

The bindings check it: once per session and library class, before the first object (`GLibInstanceCheck` in LibGLib-Cairo), `glib_check_version(2, 88, 0)` and the addresses of `g_free` and `g_object_unref` looked up through each library must be those of the VM's GLib and GObject, `hb_version_atleast(11, 0, 0)` must hold for Pango, and the cairo it draws with must be Athens' (on Windows: the files of the loaded `libglib-2.0-0.dll`, `libgobject-2.0-0.dll`, `libharfbuzz-0.dll` and `libcairo-2.dll` must lie in the library's directory). A mismatch signals `GLibInstanceMismatch`, naming the library and the paths, instead of a crash. The fix: delete the stale DLLs next to the image; load a GTK-based library after Pango, or not at all.

CI proves the check: on Linux x86_64 it runs `tests/cairo-stack/ci-text.st` with the runner's GLib 2.80 preloaded and requires `PangoCairo FAILED Error: libpangocairo-1.0.so.0 binds a GLib older than 2.88 ...`, exit 0 and no crash.

## Fonts

- **Linux:** Pango uses fontconfig and FreeType (`libpangoft2`, font map `PangoCairoFcFontMap`), with the bundled fontconfig reading the system's configuration and fonts, as cairo does ([docs/CairoStack.md, "fontconfig on Linux"](CairoStack.md#fontconfig-on-linux)). Poppler uses the same fontconfig to substitute the fonts that a PDF does not embed (the base-14 Helvetica, Times, Courier...).
- **macOS:** Pango uses CoreText (no `libpangoft2`, no fontconfig). Poppler uses a fontconfig of its own, which the stack builds only for it, with the bundle's `Contents/Resources/fontconfig/fonts.conf`: LibPoppler loads that file once per session before its first document, located from the resolved `libpoppler-glib` path, unless `FONTCONFIG_FILE` is set ([docs/CairoStack.md, "fontconfig on macOS"](CairoStack.md#fontconfig-on-macos)). The launcher sets no environment variable.
- **Windows:** MSYS2's Pango with its Windows font backend; Poppler is built with `FONT_CONFIGURATION=win32`.
- **CJK and other CMaps of PDFs without embedded fonts** come from poppler-data, which the VM does not ship: Linux and macOS look in `/usr/share/poppler` (a distribution's `poppler-data` package), Windows next to `libpoppler`, where there is none.
- **Thai** line breaking: the native Pango is built without libthai; MSYS2's Pango imports `libthai-0.dll` and `libdatrie-1.dll` (shipped), but libthai's dictionary is not shipped, so libthai cannot break Thai words there either.

## PDF documents and GIO

The bindings open documents **from bytes only** (`poppler_document_new_from_bytes`; `PopplerDocument fromFile:` reads the file in Smalltalk), and do not bind `poppler_document_new_from_file` or `poppler_document_new_from_data`. They never call into GIO's VFS, GSettings, proxies or TLS, so no GIO extension point is used and no GIO module is loaded:

- Linux and macOS: GLib is built with `-Dgio_module_dir=/dev/null/gio-modules`, a directory that cannot exist.
- Windows: MSYS2's relocatable libgio computes its module directory next to the VM (`<vm dir>/lib/gio/modules`, writable by whoever unzipped the VM) and scans it when an extension point is first used. Nothing bound uses one; binding a GIO function that does (file monitors, `GFile` on URIs, settings, network) would make that directory a place from which code is loaded, and needs its own review.

Poppler is built without network, signature, colour-management, JPEG 2000 and TIFF code (no libcurl, NSS, GPGME, lcms2, OpenJPEG, libtiff): a JPEG 2000 image in a PDF is not drawn. JPEG images are decoded by libjpeg-turbo, inside libpoppler (Windows: `libjpeg-8.dll`); without SIMD on macOS x86_64, so more slowly there.

## Checks

- `scripts/cairo-stack/check.sh` on every stack and every zip: the files and names of each feature, one GLib, HarfBuzz, FreeType and cairo, no exported or imported symbol of the private static copies (libffi, PCRE2, libjpeg-turbo, zlib, expat), the exports of the symbol lists, the GLIBCXX ceiling, and the probe `scripts/cairo-stack/probe-text.c`, which loads the libraries by absolute path, draws "Ag שלום" with Pango, writes a 2-page PDF with cairo and reads it back with Poppler (text, ink, a JPEG image's colour), renders a PDF with a non-embedded Helvetica, and checks that each library is loaded once ([docs/CairoStack.md, "What check.sh asserts"](CairoStack.md#what-checksh-asserts)).
- `tests/cairo-stack/ci-text.st` on every native zip, with the VMMaker image: it compiles its own uFFI callouts into a probe class (no binding repository is loaded), checks that each library comes from the VM's bundle and binds its GLib, and prints one line per feature:

  ```
  PangoCairo ok pango 1.58.2 family DejaVu Sans ink 352
  Poppler ok poppler 26.11.0 pages 1 text yes ink 872 base14 text yes ink 1113
  ```

  or `... FAILED <reason>` (a failing `dlopen` or instance check, never a crash), or `... not bundled` (a VM built without the feature). CI greps the `ok` lines with the pinned versions, and checks under `LD_DEBUG=libs` (Linux) and `DYLD_PRINT_LIBRARIES` (macOS) that each library is loaded once, from the zip.

## Licence

**Poppler is GPL.** libpoppler and libpoppler-glib are under the GNU GPL version 2 or (at your option) any later version, their Xpdf-derived parts under the GPL version 2 or version 3; this VM distributes them under the **GPL version 3**, which every part allows. The VM does not link Poppler: the image loads libpoppler-glib by name. A program that uses Poppler through the image may be subject to the GPL; whoever distributes one should read it. The bundle's `THIRD-PARTY-NOTICES.cairo-stack.txt` and `LICENSE.poppler` say so, with the written offer of the source code, and every GitHub release attaches `cairo-stack-sources.tar` with the unmodified Poppler archive ([docs/CairoStack.md, "Licences and sources"](CairoStack.md#licences-and-sources), which also says why the version 3). A VM without Poppler, and so without GPL code, is built with `-DFEATURE_LIB_POPPLER=OFF`.

GLib, Pango and FriBidi are under the LGPL, shared and replaceable; HarfBuzz, PCRE2, libffi and libjpeg-turbo under permissive licences. Both binding repositories are MIT.

## WebAssembly

The WebAssembly VM links GLib, GObject, GIO, FriBidi, HarfBuzz, expat, fontconfig, Pango, pangoft2 and PangoCairo with `WASM_PANGO=ON`, and libjpeg-turbo, Poppler and poppler-glib with `WASM_POPPLER=ON` (which needs `WASM_PANGO`), at the versions and SHA256 of `cmake/cairo-stack/pins.txt`, with the features of the native Linux stack ([docs/WebAssembly.md, "The libraries"](WebAssembly.md#the-libraries)). There is no `dlopen`: the libraries are linked in, and the registry of the FFI answers the library names of the bindings (`libpango-1.0.so.0` is `pango`, `libpoppler-glib.so.8` is `poppler-glib`...). The instance check of the bindings passes: a library of the registry looks up what it lacks in the libraries it depends on, as `dlsym` does, and the VM has one GLib, GObject, HarfBuzz and cairo. Pango and Poppler draw with the DejaVu fonts (Sans, Serif, Sans Mono) that the VMs carry, through fontconfig, with the generic families and the base-14 fonts of PDF mapped to them. The registry has fontconfig (`libfontconfig.so.1`) and pangoft2 (`libpangoft2-1.0.so.0`) too, so `PangoImageFonts` adds the image's Source Sans Pro and Source Code Pro to that fontconfig as on Linux (the image sees a Unix platform): the node VM writes their files to the temporary directory of the host, the web VM to `/tmp` of its memory file system. Its own lane, `tests/wasm/lanes/76-pango-poppler.sh`, runs `tests/cairo-stack/ci-text.st` in both VMs, and the bindings' `ci/run.sh` on the node VM with `LIBPANGOCAIRO_DIR` and `LIBPOPPLER_DIR`.

## Deferred

- **WebAssembly:** the options are off by default, in the site of GitHub Actions too, until their size is accepted ([docs/WebAssembly.md](WebAssembly.md)); libjpeg-turbo has no SIMD there.
- Poppler's API beyond documents, pages, sizes, labels, title, author, text and rendering (annotations, forms, links and outline, attachments, find and selection, signatures, layers, media, saving, per-area text, thumbnails); Pango's beyond layouts, font descriptions and attribute lists (attribute objects per run, tab arrays, a public iterator object, font metrics, the choice of font map, Pango 2; the layout iterator, the x-ranges of a line, `pango_layout_index_to_pos` and `pango_layout_xy_to_index` are bound for the layout geometry of `PangoLayout`, `lineGeometry`, `rangesFrom:to:`, `positionOfIndex:` and `indexAt:`, which the Microdown page of libpangocairo.st uses; the iterator never leaves the method that walks the lines).
- libthai natively and Thai's dictionary on Windows; poppler-data; JPEG 2000, colour management, TIFF and signatures in Poppler.
- One libffi for the VM and the stack on Linux and macOS (the VM links its own; GLib has a private static copy).
