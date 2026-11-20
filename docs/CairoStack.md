# The cairo stack

Athens draws through **cairo** and reads its fonts through **FreeType**, which the image loads by name (`CairoLibrary`, `FT2FFILibrary`). Every VM of this fork ships the same upstream release of cairo, **1.18.6**, pinned by its SHA256, with the libraries around it at their latest security releases. Two features of the stack add **Pango** with PangoCairo (text layout) and **Poppler** with poppler-glib (PDF), both on by default, next to the same cairo; [docs/PangoPoppler.md](PangoPoppler.md) explains them from the image's side. This page explains what is built, how, what is checked, and how to move to a later release.

| VM | How cairo gets there | cairo | FreeType | pixman | libpng | zlib | fontconfig | expat |
|---|---|---|---|---|---|---|---|---|
| Linux x86_64, aarch64 | built from the pinned archives (`scripts/cairo-stack/build.sh`) | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 (static) | 2.18.4 | 2.10.0 (static) |
| macOS x86_64, arm64 | the same | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 (static) | 2.18.4, Poppler's only | 2.10.0 (static), Poppler's only |
| Windows x86_64, ARM64 | MSYS2's binary packages, pinned by URL and SHA256 (`scripts/cairo-stack/fetch-msys2.sh`) | 1.18.6 + MSYS2's 2 patches | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 | 2.18.3 | 2.10.0 |
| WebAssembly | built by CMake from the same archives (`cmake/emscripten/deps/`, `docs/WebAssembly.md`) | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 | 2.18.4 with `WASM_PANGO` | 2.10.0 with `WASM_PANGO` |

The features (`FEATURE_LIB_PANGO`, `FEATURE_LIB_POPPLER`; `build.sh --features`):

| Feature | Libraries | Linux, macOS | Windows |
|---|---|---|---|
| `pango` | GLib 2.90.1 (with PCRE2 10.49 and libffi 3.8.1 static inside), FriBidi 1.0.18, HarfBuzz 15.1.0, Pango 1.58.2 and PangoCairo | built from the pinned archives (macOS: proxy-libintl 0.5 static inside GLib) | MSYS2's packages: the same versions, plus libthai 0.1.30 and libdatrie 0.2.14 |
| `poppler` | GLib, libjpeg-turbo 3.2.1 (static inside libpoppler), Poppler 26.11.0 (libpoppler and libpoppler-glib, **GPL**) | built from the pinned archives (macOS also expat and fontconfig) | Poppler built from its pinned archive in the MSYS2 shell, against MSYS2's packages (libjpeg-turbo 3.2.1) |

The WebAssembly VM has neither feature (`cmake/Emscripten.cache.cmake` turns both off), but its own options for the same libraries, `WASM_PANGO` and `WASM_POPPLER` ([docs/WebAssembly.md](WebAssembly.md), The libraries), which build them from these pins.

`cmake/cairo-stack/pins.txt` is the single source of these versions. `check-pins.sh` keeps the wasm pins equal to the native ones, and reports the names that only the native VMs have.

**Linux needs glibc 2.38 for the stack** (Ubuntu 24.04+, Debian 13+, Fedora 39+, RHEL 10): it is built on the CI's Ubuntu 24.04 runners, and its libraries use `__isoc23_strtol`, `__isoc23_sscanf` and `fmod@GLIBC_2.38`. The VM itself still needs only glibc 2.34; on glibc 2.34 to 2.37 it starts, but Athens cannot load cairo. **Poppler also needs the system's libstdc++ with `GLIBCXX_3.4.32`** (that of GCC 13 or later: the same distributions): `libpoppler` and `libpoppler-glib` are C++ and are the only libraries of the stack that need `libstdc++.so.6` and `libgcc_s.so.1`, which the zip does not carry. `build.sh` compiles them against GCC 13's libstdc++ headers, even with clang and with a newer GCC installed (`--gcc-install-dir`), and `check.sh` fails above `GLIBCXX_3.4.32` (3.4.31 today).

## Files

| Path | What |
|---|---|
| `cmake/cairo-stack/pins.txt` | the pins: the 16 source archives, and for Windows the MSYS2 packages of the closure of cairo, FreeType, Pango and the DLLs Poppler imports, with their source packages and DLLs |
| `cmake/cairo-stack/keys/*.asc` | the public keys of the signers of zlib, expat, FreeType, pixman, cairo, PCRE2, libjpeg-turbo and Poppler, for `gpgv` at bump time |
| `scripts/cairo-stack/build.sh` | builds the stack into a prefix (Linux, macOS); `--platform mingw` runs `fetch-msys2.sh`, then builds Poppler |
| `scripts/cairo-stack/fetch-msys2.sh` | unpacks the pinned MSYS2 packages into a prefix (Windows); `--closure` computes the closure when bumping |
| `scripts/cairo-stack/check.sh` | the assertions on a prefix or a bundle (see [What check.sh asserts](#what-checksh-asserts)) |
| `scripts/cairo-stack/probe.c` | the toy-text probe that `check.sh` compiles and runs |
| `scripts/cairo-stack/probe-text.c` | the Pango and Poppler probe that `check.sh` compiles and runs |
| `scripts/cairo-stack/check-pins.sh` | pins.txt well formed, equal to the wasm pins, its URLs alive |
| `scripts/cairo-stack/check-latest.sh` | the security watch: the pins against each upstream's latest release |
| `scripts/cairo-stack/requirements.txt` | meson, ninja and pkgconf, pinned by hash |
| `cmake/cairoStack.cmake` | the VM build's side: builds or reuses the stack and puts it into the bundle |
| `cmake/emscripten/ffi/symbols/*.txt` | the C functions the images bind, per library: `check.sh` asserts that each library exports them |
| `packaging/cairo-stack/THIRD-PARTY-NOTICES.head.txt` | the text of the bundle's `THIRD-PARTY-NOTICES.cairo-stack.txt` |
| `packaging/cairo-stack/fonts.conf` | macOS: Poppler's fontconfig configuration (see [fontconfig on macOS](#fontconfig-on-macos)) |
| `tests/cairo-stack/ci-text.st` | CI: draws text with Pango and renders PDFs with Poppler through a zip |

## Tools

To build the stack (Linux and macOS):

- meson ≥ 1.11 (fontconfig 2.18 needs it; Ubuntu 24.04's apt meson is 1.3.2), ninja and pkg-config:

  ```sh
  python3 -m venv ~/.venvs/cairo-stack
  ~/.venvs/cairo-stack/bin/pip install --require-hashes -r scripts/cairo-stack/requirements.txt
  export PATH=~/.venvs/cairo-stack/bin:$PATH
  ```
- cmake (zlib, libpng and expat are built with their own CMake), a C compiler (clang by default, `CC`/`CXX` otherwise), curl;
- on Linux, gperf (fontconfig): `apt install gperf`.

With a feature (Pango or Poppler) also:

- a C++ compiler (HarfBuzz, Poppler) and python3 (meson, `glib-mkenums`);
- cmake ≥ 3.28 (Poppler 26);
- on Linux, GCC 13's libstdc++: `apt install g++-13` (with clang, `build.sh` passes `--gcc-install-dir=<dir of g++-13's libgcc>`, or `$PHARO_CAIRO_STACK_GCC_INSTALL_DIR`; a GCC as `CXX` must be GCC 13);
- with Poppler: on Linux x86_64, NASM ≥ 2.13 (libjpeg-turbo's SIMD): `apt install nasm`; on macOS, gperf (Poppler's fontconfig): the Xcode command-line tools' `/usr/bin/gperf`, else `brew install gperf` (a build tool, never linked).

Windows needs nothing new for cairo and Pango: `fetch-msys2.sh` uses curl, zstd and tar of the MSYS2 shell. Poppler is compiled there: cmake ≥ 3.28, ninja, pkgconf, python3 and the environment's clang/clang++ (`pacboy -S cmake:p ninja:p pkgconf:p python:p clang:p`). A Linux stack built locally needs the glibc of the machine it was built on.

`-DFEATURE_LIB_PANGO=OFF -DFEATURE_LIB_POPPLER=OFF` needs only the tools of cairo and FreeType.

## Building the stack

```sh
sh scripts/cairo-stack/build.sh --platform linux  --prefix <prefix> [--work <dir>] [--deps <dir>] [--downloads <dir>] [--features pango,poppler]
sh scripts/cairo-stack/build.sh --platform darwin --arch arm64 --prefix <prefix> ...   # MACOSX_DEPLOYMENT_TARGET, default 11.0
sh scripts/cairo-stack/build.sh --platform mingw  --arch arm64 --prefix <prefix> ...   # in the MSYS2 shell: fetch-msys2.sh, then Poppler
sh scripts/cairo-stack/fetch-msys2.sh --prefix <prefix> [--env ucrt64|clangarm64] [--features <list>] [--deps <dir>] [--downloads <dir>]
```

`--features` is a comma list of `pango` and `poppler` (default both; `none` or empty: neither); `--only freetype` builds zlib, libpng and FreeType only, and no feature. The map from a feature to its pins is fixed in `build.sh`, `check.sh` and `cmake/cairoStack.cmake`:

| Feature | Pins (in build order, after the core zlib, libpng, FreeType, expat, fontconfig, pixman, cairo) |
|---|---|
| `pango` | libffi, pcre2, proxy-libintl (macOS), glib, fribidi, harfbuzz, pango |
| `poppler` | libffi, pcre2, proxy-libintl (macOS), glib, libjpeg-turbo, poppler; on macOS also expat and fontconfig, which are core on Linux |

The VM's CMake does this for you (`cmake/cairoStack.cmake`): it runs `build.sh` into `<build>/cairo-stack/prefix`, or bundles the prefix of `-DPHARO_CAIRO_STACK_DIR=<prefix>` after validating its manifest. The stack is the only cairo and FreeType a bundle can have: no prebuilt cairo or FreeType is downloaded any more, except Windows' MSYS2 packages pinned in `pins.txt`.

- `-DFEATURE_LIB_PANGO=ON|OFF` and `-DFEATURE_LIB_POPPLER=ON|OFF` (both `ON` by default, independent of each other) choose the features: the configure prints `cairo stack: features pango poppler` (or `none`), passes them to `build.sh --features`, and bundles only theirs. **`-DFEATURE_LIB_POPPLER=OFF` makes a VM without GPL code.** With `FEATURE_LIB_CAIRO=OFF` both are off (`cairo stack: FEATURE_LIB_PANGO and FEATURE_LIB_POPPLER off (FEATURE_LIB_CAIRO is off)`).
- A `PHARO_CAIRO_STACK_DIR` prefix must have every requested feature (it may have more: the bundle takes only the requested ones), and a manifest of `format 2`: a prefix of the earlier scripts is refused ("a stack of an older scripts/cairo-stack/build.sh or fetch-msys2.sh, from before the Pango and Poppler features"), as is one built without a requested feature ("built without the features poppler (its features: pango), which FEATURE_LIB_PANGO/FEATURE_LIB_POPPLER ask for").
- `-DPHARO_CAIRO_STACK_GLIBC_MAX=<x.y>` (Linux, stack built by the configure) is passed to `build.sh --glibc-max`: the stack build fails when a library needs a newer `GLIBC_` symbol version than that.
- Every configure of a tree with `BUILD_BUNDLE` on (the default) acts on the stack, even one that only generates the VM's sources: without the tools it stops and names them. Such a configure can switch the stack off with `-DFEATURE_LIB_CAIRO=OFF -DFEATURE_LIB_FREETYPE2=OFF` (CI's `generate` job and the host tree of `make wasm` do), drop only Pango and Poppler with `-DFEATURE_LIB_PANGO=OFF -DFEATURE_LIB_POPPLER=OFF`, or reuse a stack with `PHARO_CAIRO_STACK_DIR`.
- With `BUILD_BUNDLE=OFF`, or with both `FEATURE_LIB_CAIRO` and `FEATURE_LIB_FREETYPE2` off, the stack is off (a `cairo stack: off` line): the VM bundles no cairo and no FreeType, and the image loads the system's.
- FreeBSD, OpenBSD, MSVC and Cygwin have no stack: their bundles have no cairo and no FreeType (a `not bundled` line), and the image loads the system's (on the BSDs, the cairo package and the FreeType it brings).
- `-DPHARO_CAIRO_STACK=prebuilt`, which bundled the old files.pharo.org libraries (on Linux and Windows FreeType 2.9.1 to 2.12.1, cairo 1.16.0 and 1.17.4 and libpng 1.6.37, with known CVEs; on macOS cairo 1.18.4 and FreeType 2.14.3; on MSYS2, the unpinned packages of the build machine), was removed with the option itself: any value but `source` stops the configure.

**Downloads.** Each archive is looked for in `--deps` (the VM passes `WASM_DEPS_DIR`, which also serves the wasm deps), then in `$PHARO_CAIRO_STACK_DOWNLOADS`, then in `--downloads`. Anything missing is downloaded over HTTPS only (`curl --proto =https --tlsv1.2`) from its URL, or from its `mirror`, into `--downloads`. **Every SHA256 is checked before anything is extracted**: one wrong hash stops the build with the prefix untouched.

**Offline and air-gapped builds** need the archives of the platform and features (all 16 on macOS, 15 on Linux, which has no proxy-libintl; on Windows Poppler's and the MSYS2 packages: `fetch-msys2.sh --list --env <env>` lists them) in `--deps` or `$PHARO_CAIRO_STACK_DOWNLOADS`, a pip wheelhouse for `requirements.txt` (`pip download --require-hashes -r ... -d <dir>`, then `pip install --no-index --find-links <dir> ...`), cmake, a compiler and gperf, and the tools of the features. On macOS GLib's build takes proxy-libintl from the checked archive (copied into GLib's `subprojects/packagecache/`, `--wrap-mode=nodownload`): meson downloads nothing.

**Time.** On a many-core Linux host the stack builds in about 80 s (150 s CPU) for cairo and FreeType alone, about 200 s with Pango and Poppler (Poppler's C++ dominates); on a 4-vCPU CI runner the features add 4 to 6 minutes (about 770 s CPU).

### Build options

All libraries: `--buildtype=release -Db_ndebug=false --wrap-mode=nofallback` (meson), `CMAKE_BUILD_TYPE=Release` (CMake), or autotools (libffi). cairo keeps its assertions on, as every shipped cairo had them. pkg-config sees the prefix and the private static prefix only (`PKG_CONFIG_LIBDIR=<prefix>/lib/pkgconfig:<work>/static/lib/pkgconfig`, `PKG_CONFIG_PATH` unset), CMake the prefix only (`CMAKE_PREFIX_PATH=<prefix>`, `CMAKE_FIND_USE_CMAKE_SYSTEM_PATH=OFF`, on macOS `CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local`), and meson builds no subproject (but proxy-libintl on macOS, from the checked archive): a library of the system, Homebrew or MSYS2 cannot slip in.

- **Private static copies.** zlib, expat, libffi, PCRE2 and libjpeg-turbo are static archives linked into their users (zlib into libpng, FreeType, cairo, GIO and libpoppler; expat into fontconfig; libffi into libgobject; PCRE2 into libglib; libjpeg-turbo into libpoppler). Each is compiled with **hidden visibility** (`-fvisibility=hidden`, CMake `CMAKE_C_VISIBILITY_PRESET=hidden`; PCRE2 also `HAVE_VISIBILITY=OFF`, which empties its `PCRE2_EXPORT`, so its CMake prints a visibility warning), so that no library of the stack exports or imports one of their symbols: the VM's own `libffi.so` and the IJG `jpeg_*` of `libJPEGReadWriter2Plugin` would otherwise interpose. libffi, PCRE2 and libjpeg-turbo are installed into `<work>/static`, never into `<prefix>`, whose `include/` is the CairoPlugin's include path.
- **Reproducible file names.** `-ffile-prefix-map=<work>=/cairo-stack-build` on every compile: no build path in `__FILE__` or `G_STRLOC` strings.
- **Linux, GCC 13's libstdc++** (with a feature): `--gcc-install-dir=<dir>` on every compile and link when `CC` and `CXX` are clang; the directory and `g++-13 --version` go into the manifest.

Link flags:

- Linux: `-Wl,--disable-new-dtags,-rpath,$ORIGIN` (a `DT_RPATH`, searched before `LD_LIBRARY_PATH`, so the libraries find each other even when the image loads cairo by absolute path) and `-Wl,--exclude-libs,libz.a:libexpat.a:libffi.a:libpcre2-8.a:libjpeg.a` (the static copies are not re-exported);
- macOS: `-arch <arch> -Wl,-rpath,@loader_path`, install names `@rpath/<name>`, an ad hoc signature on each dylib.

| Library | Built with | Options |
|---|---|---|
| zlib | CMake, static (PIC), hidden | `ZLIB_BUILD_SHARED=OFF ZLIB_BUILD_STATIC=ON ZLIB_BUILD_TESTING=OFF` |
| libpng | CMake, shared | `PNG_SHARED=ON PNG_STATIC=OFF PNG_TESTS=OFF PNG_TOOLS=OFF PNG_FRAMEWORK=OFF`, the prefix's static zlib |
| FreeType | meson, shared, **never static** | `-Dharfbuzz=disabled -Dbrotli=disabled -Dbzip2=disabled -Dpng=enabled -Dzlib=system -Dtests=disabled`; macOS also `-Dhvf=disabled` (2.15.0's HVF driver would link Apple's libhvf) |
| expat (Linux; macOS with Poppler) | CMake, static (PIC), hidden | `EXPAT_SHARED_LIBS=OFF`, no tools, examples, tests, docs or fuzzers |
| fontconfig (Linux; macOS with Poppler) | meson, shared | `-Ddoc=disabled -Dtests=disabled -Dtools=disabled -Dcache-build=disabled -Dnls=disabled -Diconv=disabled -Dfontations=disabled -Dxml-backend=expat`; Linux the system's directories (see [fontconfig](#fontconfig-on-linux)); macOS `-Ddefault-fonts-dirs=/System/Library/Fonts,/System/Library/Fonts/Supplemental,/Library/Fonts,~/Library/Fonts -Dadditional-fonts-dirs=no` (see [fontconfig on macOS](#fontconfig-on-macos)) |
| pixman | meson, shared | `-Dtests=disabled -Ddemos=disabled -Dgtk=disabled -Dlibpng=disabled -Dopenmp=disabled`; SIMD left to auto (SSE2/SSSE3, A64 NEON) |
| cairo | meson, shared | `-Dtests=disabled -Dgtk2-utils=disabled -Dglib=disabled -Dspectre=disabled -Dlzo=disabled -Dsymbol-lookup=disabled -Dxlib=disabled -Dxcb=disabled -Dxlib-xcb=disabled -Dfreetype=enabled -Dpng=enabled -Dzlib=enabled`; Linux `-Dfontconfig=enabled -Dquartz=disabled -Ddwrite=disabled`; macOS `-Dfontconfig=disabled -Dquartz=enabled -Ddwrite=disabled` |
| libffi | autotools, static (PIC), hidden, `<work>/static` | `--disable-shared --enable-static --with-pic --disable-docs --disable-multi-os-directory`; macOS `CC="$CC -arch <arch> -mmacosx-version-min=..."` (so its assembler files get the minimum macOS too) |
| PCRE2 | CMake, static (PIC), hidden, `<work>/static` | 8-bit only, Unicode, no JIT, no tools, tests, libz, libbz2, libedit or readline |
| proxy-libintl (macOS) | inside GLib's build (meson subproject), static | `--force-fallback-for=intl -Dproxy-libintl:default_library=static` |
| GLib | meson, shared, staged (`lib/`, `include/`, `bin/` copied) | `-Dtests=false -Dinstalled_tests=false -Dnls=disabled -Dlibmount=disabled -Dselinux=disabled -Dxattr=false -Dman-pages=disabled -Ddocumentation=false -Ddtrace=disabled -Dsystemtap=disabled -Dsysprof=disabled -Dintrospection=disabled -Dlibelf=disabled -Dglib_debug=disabled -Dgio_module_dir=/dev/null/gio-modules -Dsysconfdir=/etc -Dlocalstatedir=/var` |
| FriBidi | meson, shared | `-Ddocs=false -Dbin=false -Dtests=false` |
| HarfBuzz | meson, shared | FreeType on; GLib, GObject, cairo, ICU, graphite2, subset, utilities and the rest off; macOS `-Dcoretext=enabled` |
| Pango | meson, shared | `-Dcairo=enabled -Dlibthai=disabled -Dxft=disabled -Dintrospection=disabled -Dsysconfdir=/etc`, no docs, tests or examples; Linux fontconfig and FreeType (with `libpangoft2`); macOS CoreText, no fontconfig, FreeType or `libpangoft2` |
| libjpeg-turbo | CMake, static (PIC), hidden, `<work>/static` | `ENABLE_SHARED=OFF WITH_TURBOJPEG=OFF WITH_TOOLS=OFF WITH_TESTS=OFF`; SIMD on (`REQUIRE_SIMD`) on Linux x86_64 (NASM), Linux aarch64 and macOS arm64 (NEON), off on macOS x86_64 |
| Poppler | CMake, shared, C++23 | glib wrapper and cairo output only: `ENABLE_GLIB=ON ENABLE_CPP=OFF ENABLE_QT5=OFF ENABLE_QT6=OFF ENABLE_UTILS=OFF ENABLE_BOOST=OFF ENABLE_LIBOPENJPEG=OFF ENABLE_LCMS=OFF ENABLE_LIBTIFF=OFF ENABLE_LIBCURL=OFF ENABLE_NSS3=OFF ENABLE_GPGME=OFF ENABLE_BROTLI=OFF ENABLE_HARFBUZZ=OFF ENABLE_LIBJPEG=ON`, no tests, introspection or gtk-doc; Linux and macOS `FONT_CONFIGURATION=fontconfig POPPLER_DATADIR=/usr/share/poppler CMAKE_SKIP_RPATH=ON` with the rpath as a linker flag; Windows `FONT_CONFIGURATION=win32`, relocatable |

`-Dlzo=disabled` and `-Dsymbol-lookup=disabled` keep LZO (GPL-2) and libbfd (GPL-3) out. PNG gives the PNG functions and the SVG surface, zlib the PDF, PostScript and script surfaces. COLRv1 and OT-SVG colour glyphs are on natively (FreeType ≥ 2.13 with its default modules). Poppler without libcurl, NSS, GPGME, lcms2, OpenJPEG, libtiff and brotli has no network, signature, colour-management, JPEG 2000 or TIFF code: a JPEG 2000 image of a PDF is not drawn. `POPPLER_DATADIR=/usr/share/poppler` is where a distribution's poppler-data lives; the stack ships none (the CJK and cyrillic CMaps of PDFs that do not embed their fonts come from there when it is installed).

`PHARO_CAIRO_STACK_<NAME>_ARGS` (NAME in ZLIB LIBPNG FREETYPE EXPAT FONTCONFIG PIXMAN CAIRO LIBFFI PCRE2 PROXY_LIBINTL GLIB FRIBIDI HARFBUZZ PANGO LIBJPEG_TURBO POPPLER) adds meson, CMake or configure arguments to one library, for example `PHARO_CAIRO_STACK_PIXMAN_ARGS=-Da64-neon=disabled`. It is recorded in the manifest and in `CAIRO-STACK.txt`.

**Windows (`build.sh --platform mingw`).** `fetch-msys2.sh` unpacks the packages of the features (`--features`, default both), with their `lib/pkgconfig/*.pc` rewritten to the prefix, glib2's `lib/glib-2.0/include/glibconfig.h` and `bin/glib-mkenums-script.py`. With Poppler, `build.sh` then checks its tools, checks and extracts `poppler-26.11.0.tar.xz` and builds it with CMake and Ninja against the prefix (paths in `cygpath -m` form for the native programs), and copies `libpoppler-<N>.dll` and `libpoppler-glib-<M>.dll` (their names taken from the build) into `<prefix>/bin`, listed as `built` in `dlls.txt`. The compiler is the environment's clang: ucrt64 links libstdc++ and libgcc_s_seh, clangarm64 libc++ (the runtime DLLs of the core).

### The prefix

```
<prefix>/include/                 cairo/, freetype2/, fontconfig/, pixman-1/, libpng16/, glib-2.0/, pango-1.0/, harfbuzz/, fribidi/, poppler/, png.h, zlib.h, expat.h ...
<prefix>/lib/                     the libraries, their symlinks, pkgconfig/ (Windows: bin/*.dll and lib/*.dll.a)
<prefix>/bin/                     GLib's tools (glib-mkenums builds Poppler); never bundled
<prefix>/share/pharo-cairo-stack/
    manifest.txt                  see below
    CAIRO-STACK.txt               the index for the bundle: versions, URLs, SHA256, signers, options, toolchain, features
    licenses/<name>/<files>       the licence files of pins.txt, as in the archive
    meson-summaries/<name>.txt    what meson configured (FreeType, fontconfig, pixman, cairo, GLib, FriBidi, HarfBuzz, Pango)
    cmake-summaries/poppler.txt   what Poppler's CMake configured
    dlls.txt                      Windows: "<dll> <kind> <package> <sha256> <features>" of the closure, and the built Poppler DLLs
    msys2/<package>.PKGINFO       Windows: the packages' metadata
<prefix>/.pharo-cairo-stack       marks a stack prefix: build.sh empties only such a directory
```

The include directories of the plugin and of anything else that includes `cairo-ft.h` are `<prefix>/include`, `<prefix>/include/cairo` and `<prefix>/include/freetype2` (on Linux and Windows `cairo-ft.h` includes `<fontconfig/fontconfig.h>`). The headers of the private static copies are not in `<prefix>/include` on Linux and macOS (on Windows MSYS2's package headers are, which is harmless for those includes).

In `dlls.txt` the `kind` is `root`, `lib` or `runtime` (an MSYS2 package's DLL) or `built` (a Poppler DLL that `build.sh` compiled: its `package` is the source pin `poppler`), and `features` is the comma list of the `msys2-dll` line (`core`, `pango`, `poppler`; `poppler` for the built ones). The bundle's `CAIRO-STACK.txt` repeats them as 6-field `dll <dll> <kind> <package> <sha256> <features>` lines.

**The manifest** has one `key value...` per line (`#` lines are comments):

| Key | Value |
|---|---|
| `format` | `2` (`1` before the features; `cmake/cairoStack.cmake` refuses it) |
| `pins-sha256` | the SHA256 of the `pins.txt` the stack was made from |
| `platform` | `linux`, `darwin` or `mingw` |
| `arch` | `x86_64` or `aarch64` (Linux), `x86_64` or `arm64` (macOS, Windows) |
| `deployment-target` | macOS: `MACOSX_DEPLOYMENT_TARGET`; `-` elsewhere |
| `script` | `build.sh <n>` or `fetch-msys2.sh <n>` (2 today) |
| `only` | `freetype` for `--only freetype`, `-` otherwise (build.sh) |
| `features` | the features of the stack, `pango poppler` in this order, or `-` |
| `msys2-env`, `cairo-version`, `msys2-package <name> <version> <sha256>` | Windows |
| `built-by` | Windows with Poppler: `build.sh <n>`, which built Poppler on top of `fetch-msys2.sh`'s prefix (with its `lib poppler` line) |
| `args <NAME> <value>` | each non-empty `PHARO_CAIRO_STACK_<NAME>_ARGS` |
| `tool <name> <version>` | the toolchain: cc, c++, meson, ninja, pkg-config, cmake, python3, gperf, nasm (Linux x86_64 with Poppler), gcc-install-dir (Linux with a feature: the directory and `g++-13 --version`), glibc (Linux), Xcode and SDK (macOS) |
| `lib <name> <version> <sha256>` | each library built |

`cmake/cairoStack.cmake` refuses a `PHARO_CAIRO_STACK_DIR` whose `format`, `pins-sha256`, `platform`, `arch` or `deployment-target` differ from the build's, or whose `features` lack a requested one: a stale stack from a cache cannot slip in. The `tool` lines are informational.

## What the bundles contain

cairo and FreeType are loaded by the image, and with the features GLib, GObject, Pango, PangoCairo and poppler-glib (the names in bold: [docs/PangoPoppler.md](PangoPoppler.md) has the image's side); everything else is a private dependency of those.

| Library | Linux (zip `lib/`) | macOS (`Contents/MacOS/Plugins`) | Windows (next to the exe) | Feature |
|---|---|---|---|---|
| cairo | `libcairo.so.2.11806.6`, symlinks `libcairo.so.2`, `libcairo.so` | `libcairo.2.dylib`, symlink `libcairo.dylib` | `libcairo-2.dll` | |
| FreeType | `libfreetype.so.6.20.7`, symlinks `.so.6`, `.so` | `libfreetype.6.dylib`, symlink `libfreetype.dylib` | `libfreetype-6.dll` | |
| pixman | `libpixman-1.so.0.46.4`, symlinks | `libpixman-1.0.dylib`, symlink | `libpixman-1-0.dll` | |
| libpng | `libpng16.so.16.59.0`, symlinks | `libpng16.16.59.0.dylib`, symlinks `libpng16.16.dylib`, `libpng16.dylib` | `libpng16-16.dll` | |
| fontconfig | `libfontconfig.so.1.17.0`, symlinks | `libfontconfig.1.dylib` (Poppler's) | `libfontconfig-1.dll` | macOS: poppler |
| zlib, expat | static inside their users | static | `zlib1.dll`, `libexpat-1.dll` | |
| GLib, GObject | `libglib-2.0.so.0.9000.1`, **`.so.0`**, `.so`; the same for `libgobject-2.0` | **`libglib-2.0.0.dylib`**, **`libgobject-2.0.0.dylib`**, symlinks | **`libglib-2.0-0.dll`**, **`libgobject-2.0-0.dll`** | pango or poppler (Windows: libglib is core) |
| GIO, GModule | `libgio-2.0.so.0(.9000.1)`, `libgmodule-2.0.so.0(.9000.1)` | `libgio-2.0.0.dylib`, `libgmodule-2.0.0.dylib` | `libgio-2.0-0.dll`, `libgmodule-2.0-0.dll` | pango or poppler |
| FriBidi | `libfribidi.so.0(.4.0)` | `libfribidi.0.dylib` | `libfribidi-0.dll` | pango |
| HarfBuzz | `libharfbuzz.so.0(.61510.0)` | `libharfbuzz.0.dylib` | `libharfbuzz-0.dll` | pango (Windows: core) |
| Pango, PangoCairo | `libpango-1.0.so.0.5800.2`, **`.so.0`**, `.so`; the same for `libpangocairo-1.0`; `libpangoft2-1.0.so.0(.5800.2)` | **`libpango-1.0.0.dylib`**, **`libpangocairo-1.0.0.dylib`**, symlinks | **`libpango-1.0-0.dll`**, **`libpangocairo-1.0-0.dll`**, `libpangoft2-1.0-0.dll`, `libpangowin32-1.0-0.dll` | pango |
| libthai, libdatrie | — | — | `libthai-0.dll`, `libdatrie-1.dll` | pango |
| Poppler | `libpoppler.so.166(.0.0)`; `libpoppler-glib.so.8.31.0`, **`.so.8`**, `.so` | `libpoppler.166.dylib`; **`libpoppler-glib.8.dylib`**, symlinks | `libpoppler-166.dll`, **`libpoppler-glib-8.dll`** | poppler |
| libffi, PCRE2, libjpeg-turbo | static inside libgobject, libglib, libpoppler | the same (and proxy-libintl inside libglib) | `libffi-8.dll` (core), `libpcre2-8-0.dll` (core), `libjpeg-8.dll` (poppler) | |
| Windows only | | | `libintl-8.dll`, `libiconv-2.dll`, `libgraphite2.dll`, `libbrotlidec.dll`, `libbrotlicommon.dll`, `libbz2-1.dll`, and the C/C++ runtime | |
| notices | `LICENSE.<name>`, `THIRD-PARTY-NOTICES.cairo-stack.txt`, `CAIRO-STACK.txt` | the same, in `Contents/Resources`; with Poppler also `Contents/Resources/fontconfig/fonts.conf` | the same | |

The bundle's `CAIRO-STACK.txt` names its features after its first paragraph (`Bundled features: cairo freetype pango poppler`), and keeps only the pins and DLLs of those. Not bundled: libgthread, libgirepository, libcairo-script-interpreter, `bin/`, `libexec/`, `share/` and the static archives. On Windows a DLL is bundled when its `msys2-dll` features are `core` or name an enabled feature; `libffi-8.dll` is a core DLL, so the Windows VM's FFI has it whatever the features.

Symlinks stay symlinks (`file(COPY)`, `cp -P`): a copied `libfreetype.dylib` would be a second FreeType for dyld.

Without a feature, nothing loads HarfBuzz, GLib or bz2 on Linux and macOS, so they are not shipped there, nor a shared expat (fontconfig links it statically), and on macOS no fontconfig: `check.sh` forbids `libharfbuzz*` in a bundle without Pango, `libglib*`, `libgobject*`, `libgio*` and `libgmodule*` in one with neither feature, and on macOS `libfontconfig*` in one without Poppler; the configure warns about such leftovers of an earlier configure of the tree. HarfBuzz is shipped as Pango's shaper (FreeType is still built without it). On Windows the closure is MSYS2's (FreeType is built with HarfBuzz, brotli and bzip2, and HarfBuzz with GLib), so it is kept whatever the features; `liblzo2-2.dll`, `libcairo-script-interpreter-2.dll` and `libcairo-gobject-2.dll` are in cairo's package but are not imported by `libcairo-2.dll`, and are not shipped.

### One FreeType

cairo and the image must share one FreeType instance: the image hands cairo `FT_Face`s.

- Linux: libcairo, libfontconfig, libharfbuzz, libpangoft2, libpoppler and libpoppler-glib `NEED libfreetype.so.6`, the image opens `libfreetype.so.6`, and glibc reuses an object already loaded under the same soname. No `FT_*` symbol is defined outside libfreetype. `DT_RPATH $ORIGIN` makes libcairo find the bundle's FreeType first even when it is loaded first. A VM started without its launcher, with a system FreeType 2.11 or 2.12 loaded first, would give cairo the wrong `FT_ColorStop` ABI (colour fonts misread).
- macOS: `libfreetype.dylib` (the image's name) is a symlink to `libfreetype.6.dylib` (cairo's `@rpath` name), so dyld sees one image.
- Windows: the image and cairo both name `libfreetype-6.dll`, and Windows reuses a loaded module by name.

### One GLib, one HarfBuzz, one cairo

The same holds for the libraries of the features: the bundled Pango and Poppler must bind the bundled GLib, GObject and HarfBuzz, and draw on Athens' libcairo. On Linux libpango needs `libharfbuzz.so.0`, libpangocairo and libpoppler-glib `libcairo.so.2`, and only libglib, libgobject, libgio and libgmodule define `g_*`; on macOS they load `@rpath/<name>`; on Windows the DLL names. **One GLib per process** is a known issue: see [Known accepted issues](#known-accepted-issues).

## fontconfig on Linux

The bundled fontconfig reads the system's configuration, as the desktop does: `/etc/fonts/fonts.conf` and `conf.d`, `/usr/share/fontconfig/conf.avail`, the fonts of `/usr/share/fonts` and `/usr/local/share/fonts`, the user's `~/.config/fontconfig`. These paths are compiled in; nothing of fontconfig's configuration is installed into the prefix or the bundle (`build.sh` installs fontconfig into a staging directory and copies its `lib/` and `include/` only, and checks that nothing was written to `/etc/fonts` or `/var/cache/fontconfig`). The launcher sets no `FONTCONFIG_FILE`, which children of the VM would inherit. Pango (through `libpangoft2`) and Poppler (to substitute the fonts a PDF does not embed, such as the base-14 Helvetica) use the same fontconfig.

- Without `/etc/fonts/fonts.conf` (a minimal container) fontconfig falls back to its built-in configuration; the old bundles always did, and printed `Fontconfig error: Cannot load default config file`. With no fonts at all, toy text uses cairo's built-in "twin" font.
- Caches: fontconfig 2.18 writes cache version 12 into `~/.cache/fontconfig`; it cannot use the system's caches of another version. The first toy-text call of a user scans the fonts once (measured here: 18–20 ms cold, 6–13 ms warm, on a host with few fonts); where the home is read-only the scan repeats at each start.
- A distribution's `conf.d` newer than 2.18.4 may print "unknown element" warnings.
- Security: fontconfig parses the system and user caches and XML configuration (through expat). 2.18.4 fixes the known cache-validation bugs; see [Known issues](#known-accepted-issues).

## fontconfig on macOS

cairo and Pango use CoreText on macOS; only Poppler uses fontconfig, so the macOS stack builds fontconfig (and its static expat) with the `poppler` feature only. Its default font directories are compiled in as macOS's (`/System/Library/Fonts`, `/System/Library/Fonts/Supplemental`, `/Library/Fonts`, `~/Library/Fonts`), and the bundle carries `Contents/Resources/fontconfig/fonts.conf` (`packaging/cairo-stack/fonts.conf`: those directories, the `xdg` cache directory, and Helvetica, Times and Courier for sans-serif, serif and monospace). Since macOS has no `/etc/fonts/fonts.conf`, the image's LibPoppler loads that file once per session before its first document (`FcConfigCreate`, `FcConfigParseAndLoad`, `FcConfigSetCurrent`, located from the resolved `libpoppler-glib` path), unless `FONTCONFIG_FILE` is set; the launcher sets no environment variable. `check.sh` runs its probe with `FONTCONFIG_FILE` pointing to that file, and CI's text step requires no "Fontconfig error" on stderr.

## What check.sh asserts

```sh
sh scripts/cairo-stack/check.sh --prefix <prefix> --platform linux [--glibc-max 2.39] [--glibcxx-max 3.4.32] [--features <list>]
sh scripts/cairo-stack/check.sh --bundle <unzipped VM or build/vm> --platform linux|darwin|mingw [--minos 11.0] [--arch ...]
sh scripts/cairo-stack/check.sh --prefix <prefix> --platform mingw --env ucrt64|clangarm64     # also on Linux, with llvm-objdump
```

It prints `ok …` or `FAIL …` per assertion and exits 1 on any failure. The features it checks come from the manifest's `features` line (prefix) or the `Bundled features:` line (bundle), or from `--features`; the libraries of a feature that is neither checked nor declared must be absent.

- **Linux:** the file names above, the link names are symlinks (`test -L`); every `NEEDED` is a stack soname or glibc's (`libc libm libpthread libdl librt ld-linux*`), plus `libstdc++.so.6` and `libgcc_s.so.1` for libpoppler and libpoppler-glib only, so no X11, `libz`, `libexpat`, LZO, libbfd, or a system GLib or HarfBuzz; a deny-list with its own message (libnss3, libsmime3, libnspr4, libplc4, libcurl, libgpgme, liblcms2, libopenjp2, libtiff, brotli, Qt, a shared libjpeg, libffi or PCRE2); `RPATH` exactly `$ORIGIN` and no `RUNPATH`; one FreeType, one GLib, one HarfBuzz and one cairo (who needs and who defines `FT_*`, `g_*`, `hb_*`, `cairo_*`); **no symbol of a private static copy** (`ffi_*`, `pcre2_*`, `jpeg_*`, `jinit_*`, `jcopy_*`, zlib's, `XML_*`) defined or undefined in any library's dynamic symbol table; **the highest `GLIBC_` symbol version** (printed as `glibc-max <x.y>`, asserted with `--glibc-max`; 2.38 today) and the highest `GLIBCXX_` (`glibcxx-max`, at most `--glibcxx-max`, 3.4.32 by default with Poppler; 3.4.31 today); no build path in `.rodata` or `.data.rel.ro` but four known, unused ones of GLib, GIO and Pango (`<prefix>/share/locale`, `<prefix>/lib`, `<prefix>/libexec/gio-launch-desktop`, `<prefix>/lib/pango`), and libgio's module directory is `/dev/null/gio-modules`; every library exports the functions of its `cmake/emscripten/ffi/symbols/<name>.txt` (cairo also the names of `extra_cairo_symbols` in `check.sh`); on a prefix, no `etc/`, `var/` or `share/fontconfig`, no `ffi.h`, `pcre2.h`, `jpeglib.h` or their static archives, the licences, the meson summaries (FreeType, Fontconfig, PNG, PDF, SVG, PostScript on; Xlib, XCB, Quartz, DWrite off; LZO and symbol-lookup off; FreeType's zlib `system`; GLib without libmount, SELinux and sysprof; HarfBuzz without GLib, ICU, graphite and subset; Pango with fontconfig and FreeType, without libthai), Poppler's CMake summary (no NSS, GPGME, curl, OpenJPEG, lcms2, libtiff or brotli; glib wrapper, cairo output and libjpeg on; fontconfig), the manifest of the current pins (`format 2`, its features, a `tool gcc-install-dir .../13` line with Poppler); on a bundle, no dropped library and the notices (with Poppler: `LICENSE.poppler`, and a notice with "GNU General Public License", "version 3" and "three years").
- **The probes** (Linux, macOS): `probe.c` is compiled and loads libcairo with `dlopen` by absolute path; `cairo_version_string()` must be the pin, and toy text (`sans-serif`, "Ag") must draw ink with the system's configuration without any fontconfig message on stderr, and (Linux) fall back to twin with an empty configuration. It prints the family and the time of the first `cairo_show_text`, cold and warm. With a feature, `probe-text.c` loads the bundled libraries by absolute path too and prints `pango <version> font-map <type> family <family> ink <n>` (a layout of "Ag שלום" drawn), `poppler <version> pages 2 size 300x200 text "<...>" ink <n> jpeg <r,g,b>` (a 2-page PDF that cairo writes, opened from bytes: its text, its ink, and the colour of a JPEG image decoded by Poppler's DCT decoder), `base14 text "<...>" ink <n>` (a hand-written PDF with a non-embedded Helvetica, substituted through fontconfig) and `instances ok` (one libglib, libgobject, libcairo, libfreetype, libharfbuzz, libpango, libfontconfig in the process, each from the stack).
- **macOS:** the names and symlinks, install names `@rpath/<name>`, dependencies `@rpath/<stack>`, `/usr/lib/` and `/System/` only, no system zlib, no `/usr/local` or `/opt/homebrew`; `LC_RPATH @loader_path`; `minos` ≤ `--minos`; one architecture (`lipo`); `codesign -v`; no `FT_*` outside libfreetype and no symbol of a private static copy (`nm -gU`); libpangocairo and libpoppler-glib load cairo as `@rpath/libcairo.2.dylib`; Pango imports `hb_coretext_font_create`, and there is no `libpangoft2`; with Poppler, fontconfig has the macOS font directories compiled in and `fonts.conf` is in the bundle; the exports, `cairo_quartz_surface_create` included.
- **Windows:** every DLL of the pinned closure for the features (`msys2-dll` lines naming `core` or a checked feature) is there and of the right machine, and imports only DLLs of the closure and of Windows; the `built` Poppler DLLs are there, have the SHA256 of `dlls.txt` (or of the bundle's 6-field `dll` lines), and import only the closure and Windows; in a bundle, each MSYS2 DLL is the pinned package's own copy (its SHA256 in `CAIRO-STACK.txt`), not another MSYS2 package's DLL of the same name (the C/C++ runtime may be the toolchain's), `libffi-8.dll` included; the deny-list; the exports of `libcairo-2.dll`, `libglib-2.0-0.dll`, `libgobject-2.0-0.dll`, `libharfbuzz-0.dll`, `libpango-1.0-0.dll`, `libpangocairo-1.0-0.dll` and `libpoppler-glib-8.dll`, no `FT_*`; MSYS2's cairo is the pinned version; no `liblzo2`, script interpreter or gobject DLL in the bundle. Toy text is ci-load's `CairoToy` line in CI, Pango and Poppler `tests/cairo-stack/ci-text.st`.

## The pins

Format (also at the top of `pins.txt`):

```
# signed-by <primary key fingerprint> <signature url> ...    or    # unsigned: <how the hash was cross-checked>
<name> <version> <sha256> <file> <url> <platforms> <licence files...>
mirror <name> <url>
native-ahead <name> <until YYYY-MM-DD> <reason...>
accepted-behind <name> <version> <until YYYY-MM-DD> <reason...>
msys2-root <feature> <dll>...
msys2 <env> <kind> <package> <version> <sha256> <url> <source sha256> <source url>
msys2-dll <env> <dll> <package> <features>
msys2-mirror <base url>
```

The build order is the file's order. `<platforms>` is `all` or a comma list of `linux`, `darwin` and `mingw`; `mingw` marks the one archive that `build.sh --platform mingw` builds on Windows on top of the MSYS2 prefix, Poppler (MSYS2's own Poppler imports libcurl, NSS, lcms2, OpenJPEG and libtiff: 32 more DLLs). Windows' `msys2` lines are the packages that own a DLL of the closure of the `msys2-root` DLLs (`kind` root, lib or runtime): `core` (`libcairo-2.dll`, `libfreetype-6.dll`, `libffi-8.dll`), `pango` (`libpango-1.0-0.dll`, `libpangocairo-1.0-0.dll`, `libgobject-2.0-0.dll`) and `poppler` (what the built `libpoppler` and `libpoppler-glib` import); the last field of each `msys2-dll` line lists the features whose roots reach it. `pactree` would also reach Python, Tcl/Tk, OpenSSL and others (glib2's tools), which own no such DLL and are not pinned: the end of each section lists them.

To check the deps directory against the pins:

```sh
awk '!/^#/ && NF && $1!="mirror" && $1!="native-ahead" && $1!="accepted-behind" && $1!~/^msys2/ {print $3"  "$4}' cmake/cairo-stack/pins.txt | (cd <deps> && sha256sum -c)
```

### Bump procedure

0. **Authenticate the new archive.** For a signed one, with the committed key:

   ```sh
   gpg --dearmor < cmake/cairo-stack/keys/<name>.asc > /tmp/<name>.gpg
   gpgv --keyring /tmp/<name>.gpg <archive>.sig <archive>                 # zlib (.asc), expat (.asc), FreeType, PCRE2, libjpeg-turbo, Poppler (.sig)
   gpgv --keyring /tmp/cairo.gpg --output - cairo-<v>.tar.xz.sha256sum.asc    # cairo, pixman (.sha512.asc): the signed checksum,
   sha256sum cairo-<v>.tar.xz                                                 # then compare the hash
   ```

   A new signer key is a deliberate change of `keys/`. For an unsigned archive, cross-check the hash with a second source: libpng, its files are byte-identical to the GitHub tag archive that MSYS2's PKGBUILD pins; fontconfig, its `.sha256sum`, and a distribution's record if there is one; GLib and Pango, GNOME's `.sha256sum` and MSYS2's PKGBUILD; libffi, FriBidi and HarfBuzz, MSYS2's PKGBUILD. Write the result into the comment line before the pin.
1. Edit the line in `pins.txt` and, for a library the wasm VM has, the matching `cmake/emscripten/deps/<name>.cmake` (version and SHA256; `WASM_CAIRO_FREETYPE_VERSION` for FreeType). The wasm VM takes the libraries of the features (and expat and fontconfig) from `pins.txt` itself, but compiles the sources harvested from a build of the pinned version: for one of them, run `scripts/wasm-deps/regen-text-stack.sh` (and refresh its patches if they no longer apply), which `check-pins.sh` asks for.
2. For cairo, pixman or FreeType, regenerate the wasm configuration with `cmake/emscripten/deps/cairo/README` and diff it.
3. Run `check-pins.sh`, the stack build and `check.sh`, the C lanes of the CairoPlugin, the Smalltalk suites and the wasm lanes 64, 65 and 74; regenerate the lane-65 goldens. For a library of a feature, `check.sh` (the exports of the symbol lists, the summaries, the probe) and `tests/cairo-stack/ci-text.st` on the built VM, and the suites of libpangocairo.st and libpoppler.st.
4. For cairo, pixman or FreeType, re-measure the round-2 limits of the CairoPlugin.

Particular pins:

- **GLib and proxy-libintl:** proxy-libintl (macOS) is the archive of GLib's `subprojects/proxy-libintl.wrap`, and meson checks its `source_hash`: bump it together with GLib, to the version and SHA256 of the new GLib's wrap. `check-latest.sh` reports it current while it equals that wrap. GLib and Pango take even minor versions only (Pango 1.90 and later is Pango 2: excluded).
- **Poppler** changes the soname of `libpoppler` with every release (`libpoppler.so.166` for 26.11): the bundle tables use a glob and the Windows DLL names come from the build, so nothing else changes. `libpoppler-glib.so.8` is what the image loads: a new soname there changes the names of [docs/PangoPoppler.md](PangoPoppler.md) and of the bindings. If the new Poppler imports another DLL on Windows, add it to the `msys2-root poppler` line and recompute the closure. Poppler needs a C++ standard that macOS's libc++ at the deployment target (11.0) must provide: a release that does not build there is either kept behind (`accepted-behind`) or the deployment target is raised, as a decision.

**Windows (MSYS2).** When a package of the closure changes (cairo, FreeType, Pango, GLib...), or has a security fix:

```sh
sh scripts/cairo-stack/fetch-msys2.sh --closure --env ucrt64     --sources <dir> > ucrt64.txt
sh scripts/cairo-stack/fetch-msys2.sh --closure --env clangarm64 --sources <dir> > clangarm64.txt
```

It reads `https://repo.msys2.org/mingw/<env>/<env>.files`, downloads the packages (checked against the database), walks the DLL imports from the DLLs of each `msys2-root` line with objdump (GNU or LLVM; LLVM's reads ARM64), hashes every source package, and prints the `msys2` and `msys2-dll` lines with their features: replace the two sections of `pins.txt` with them. Check that cairo's source package still holds the pinned upstream archive (its SHA256) and which patches its PKGBUILD applies. `check-pins.sh` then requires: every root a `msys2-dll` of both environments with its feature, the core roots present; no MSYS2 package of Poppler, curl, NSS, NSPR, OpenSSL, lcms2, OpenJPEG, libtiff or GPGME; the libffi package's only DLL `libffi-8.dll` (the Windows VM's FFI uses it, whatever the features); and it warns when MSYS2's Pango, GLib, FriBidi, HarfBuzz, libjpeg-turbo or libffi differs from the source pin (`WARNING msys2 <env> <name> <version> vs native <version>`), as for FreeType.

**Retention.** repo.msys2.org removes superseded packages after a while, so every bump mirrors the pinned MSYS2 files as assets of a release of the fork, and adds the `msys2-mirror` line:

```sh
sh scripts/cairo-stack/fetch-msys2.sh --list --env ucrt64 > files.txt      # and clangarm64
# download each, check its SHA256, then:
gh release create cairo-stack-msys2-<date> --repo <owner>/pharo-vm --notes "MSYS2 packages pinned by cmake/cairo-stack/pins.txt" <files...>
# pins.txt: msys2-mirror https://github.com/<owner>/pharo-vm/releases/download/cairo-stack-msys2-<date>
```

`check-pins.sh` probes every URL and falls back to the mirror; a package gone from both fails it. A source pin whose URL is a GitHub tag archive (proxy-libintl) must have a `mirror` line, since GitHub has regenerated such archives before.

**accepted-behind.** A pin kept behind a newer upstream release on purpose (decision U4; there is none today): `accepted-behind <name> <version> <until YYYY-MM-DD> <reason>` makes the weekly watch (`check-latest.sh`) report that exact release as "accepted" until the date; any other newer release, or the date passing, makes the run red again, so each exception is reviewed.

**native-ahead.** A security fix may go into the native VMs before wasm (whose configuration must be regenerated): `native-ahead <name> <until YYYY-MM-DD> <reason>` makes `check-pins.sh` warn about the difference until that date and fail after it.

## Security bumps

- `scripts/cairo-stack/check-latest.sh` compares each pin with its upstream's latest release (the GitHub releases API for zlib, expat, libffi, PCRE2, FriBidi, HarfBuzz and libjpeg-turbo, without its pre-releases and x.y.9x betas; the GitLab API for fontconfig; GNOME's `cache.json` for GLib and Pango; the cairographics.org listing for cairo and pixman; savannah for FreeType; SourceForge for libpng; poppler.freedesktop.org for Poppler; GLib's wrap for proxy-libintl) and each MSYS2 pin with MSYS2's database. It exits 1 when one is behind, and runs weekly in CI (`.github/workflows/cairo-stack-watch.yml`), so a pin behind its upstream shows as a red run.
- A release that fixes a CVE reachable from the images is bumped within 14 days, natively first (`native-ahead`) if wasm needs a configuration regeneration.
- zlib, expat, libffi, PCRE2 and libjpeg-turbo are static and private on Linux and macOS: distributions' patches no longer reach them, and every fix needs a VM release. GLib, Pango and Poppler are the system's no more either: a VM release carries their fixes.
- Every source pin is at its upstream's latest release: pixman 0.44.2 and zlib 1.3.2, kept behind with `accepted-behind` from 2026-11-18, were bumped to 0.46.4 and 1.4 on 2026-11-19 (zlib 1.4 fixes the `gzwrite` and `infback9` CVEs and window overruns after `inflateSync`, `inflatePrime` or `inflateGetDictionary`; pixman 0.46 adds the `pixman_region64f_*` functions and no security fix). The libraries of the features were added at their latest releases on 2026-11-19.
- Poppler parses untrusted PDFs: its bumps are the likeliest security ones. The bindings open documents from bytes only and bind no annotation, form, link, attachment or signature function (docs/PangoPoppler.md).

## Known accepted issues

- **cairo 1.18.6:** CVE-2025-50422, a reachable assertion in cairo-ft (MR !621 still open): with assertions on, it aborts the VM, as the previous bundles did. The image-information fixes on cairo's master (JPEG, JPEG 2000, JBIG2 bounds) are reachable through `cairo_surface_set_mime_data`, which the Poppler test fixtures and `probe-text.c` call with their own JPEG only. 1.18.6 also creates more scaled fonts for toy text (commit 46d1e26c: the toy faces miss cairo's global cache); Athens' FreeType path is unaffected.
- **expat 2.10.0:** upstream lists unfixed security issues; only fontconfig's XML configuration reaches it.
- **One GLib per process.** A process can hold one GLib, one GObject type system, one HarfBuzz and one cairo for the bundled Pango and Poppler. On Linux, a library linking the system's `libglib-2.0.so.0` that is loaded before Pango (a GTK-based plugin, `LD_PRELOAD`) makes glibc hand Pango that GLib by soname; on macOS a Homebrew GLib loaded by another library is a second GLib beside the bundle's; on Windows a stale `libglib-2.0-0.dll` & co. next to the image (the old libpoppler.st README told users to copy DLLs there) is loaded instead of the VM's. The bindings check this once per session before the first object (GLib ≥ 2.88, the same `g_free`/`g_object_unref` through each library as through the VM's GLib, HarfBuzz ≥ 11, the same cairo as Athens; on Windows the loaded DLLs' files) and signal `GLibInstanceMismatch` instead of crashing; CI runs `ci-text.st` with the system's GLib preloaded and requires a `PangoCairo FAILED` line and no crash. Delete such DLLs next to the image; load GTK-based libraries after Pango, or not at all.
- **GIO modules.** On Linux and macOS GLib is built with `-Dgio_module_dir=/dev/null/gio-modules`: it loads no GIO module from a directory of its own. On Windows MSYS2's relocatable libgio computes its module directory next to the VM (`<vm dir>/lib/gio/modules`, writable by the user who unzipped the VM) and scans it when a GIO extension point is first used (GVfs, GSettings, proxies, TLS; `GIO_MODULE_DIR` and `GIO_EXTRA_MODULES` apply as in any GLib program). Nothing the images bind touches one: Poppler documents are opened from bytes only (`poppler_document_new_from_bytes`, the file read by Smalltalk), and `poppler_document_new_from_file` and `_from_data` are not bound. Binding a function that uses GIO's VFS or settings would make that directory a place from which code is loaded.
- **Windows Thai:** MSYS2's Pango imports `libthai-0.dll` and `libdatrie-1.dll` (shipped) but not libthai's dictionary, so libthai cannot break Thai words there; the native stacks build Pango without libthai.
- **macOS x86_64:** libjpeg-turbo is built without SIMD (no NASM without Homebrew): JPEG images in PDFs decode more slowly on Intel Macs.
- **Windows x86_64 CI** compiles Poppler on every run (the ExternalProject path, not cached), 3 to 5 minutes.
- **Build paths in libpoppler's symbol table:** the NASM objects of libjpeg-turbo keep their source paths (`<work>/src/libjpeg-turbo-<v>/simd/...`) as `STT_FILE` names in libpoppler's `.symtab`; NASM has no prefix map. They are in no loaded section.
- **Windows:** the versions are MSYS2's (cairo 1.18.6 with the D2D/DWrite patch and MR !673, FreeType 2.15.0 with HarfBuzz), recorded in `CAIRO-STACK.txt`.
- Linux aarch64: the `libz.so.1` of the zip (zlib 1.2.11) comes from libgit2's prebuilt dependencies, not from the cairo stack.

## CI

`.github/workflows/build.yml` wires the stack into the zips:

- **Linux and macOS** build the stack on the runner with `build.sh` (meson, ninja and pkgconf from `requirements.txt` in a venv; gperf, nasm and g++-13 from apt on Linux; gperf of the Xcode tools on macOS) into `$RUNNER_TEMP/cairo-stack`, with both features, and configure the VM with `-DPHARO_CAIRO_STACK_DIR` (the Configure step asserts "manifest checked"). The prefix is cached by `actions/cache`, keyed by the OS, the architecture, the hash of `cmake/cairo-stack/**` and `scripts/cairo-stack/**`, and a fingerprint of the toolchain (compilers, glibc or Xcode and SDK, cmake, gperf, python; on Linux also nasm, g++-13 and the GCC installation clang selects). Release runs (pushes to `pharo-12`) do not restore the cache: they always build the stack.
- **Windows ARM64** builds the stack with `build.sh --platform mingw` (the MSYS2 packages and Poppler, cached, keyed by the hash of `cmake/cairo-stack/**` and `scripts/cairo-stack/**` and of `clang --version` and `cmake --version`) and configures with `-DPHARO_CAIRO_STACK_DIR`; **Windows x86_64** uses the ExternalProject path of `cmake/cairoStack.cmake` (Configure asserts "cairo stack: built from"), which also compiles Poppler. pacboy installs no cairo and no freetype; it adds `pkgconf` and `python` for Poppler's build.
- Every native job then runs `check.sh --bundle` on the unzipped zip (`--glibc-max 2.39` and `--glibcxx-max 3.4.32` on Linux, whose summary also reports the VM's own glibc floor; `--minos 11.0` on macOS), and ci-load.st must print `CairoPlugin native 1 cairo <the pin> ok` and `CairoToy ok family … ink <n>` with n > 0. macOS also asserts that dyld loads one libfreetype and one libcairo.
- **"Draw text and a PDF through the zip"** runs `tests/cairo-stack/ci-text.st` on the VMMaker image with the zip's VM. The script binds the libraries of the VM directory by uFFI (it loads neither binding repository), checks that they lie in the bundle and bind its GLib, and must print `PangoCairo ok pango <the pin> family … ink <n>` and `Poppler ok poppler <the pin> pages 1 text yes ink <n> base14 text yes ink <n>` (Windows: Pango's version from the MSYS2 pin). Linux runs it again under `LD_DEBUG=libs` (libglib, libpango, libpoppler-glib and libcairo each initialised once, from the zip), macOS under `DYLD_PRINT_LIBRARIES` (each of nine libraries mapped once, from the app; no "Fontconfig error"). Linux x86_64 also runs it with the runner's GLib preloaded ("Refuse a GLib of the system loaded first"), and requires a `PangoCairo FAILED` line, exit 0 and no crash.
- Linux x86_64 has two steps for the binding repositories, "Check out the bindings" and "Test libpangocairo.st and libpoppler.st on the zip", which do nothing until they are submodules under `bindings/`.
- The **`cairo-stack`** job runs `check-pins.sh` and gathers `cairo-stack-sources.tar` (the 16 archives, each checked against `pins.txt`; MSYS2's source packages of both closures for every feature, from `fetch-msys2.sh --sources --no-runtime-sources`; `cmake/cairo-stack/`, `scripts/cairo-stack/` and `SHA256SUMS`), which the release job attaches. The C/C++ runtime's source packages (GCC, LLVM's libc++, winpthreads: about 350 MB) are left out; drop `--no-runtime-sources` to include them (see [Licences and sources](#licences-and-sources) for why they may be left out with Poppler).
- **`cairo-stack-watch.yml`** runs `check-latest.sh` (its output goes to the job summary) and `check-pins.sh` every Monday, on manual dispatch, and on pushes that change the workflow or `check-latest.sh`: GitHub runs scheduled and dispatched workflows only from the default branch, so the push trigger lets it run on a branch.

## Licences and sources

Each bundle has `LICENSE.<name>` (the licence files of `pins.txt`, or of the MSYS2 packages) and `THIRD-PARTY-NOTICES.cairo-stack.txt` (made from `packaging/cairo-stack/THIRD-PARTY-NOTICES.head.txt`, with the paragraphs of the bundled features only), which elects cairo's LGPL-2.1 and FreeType's FTL, gives FreeType's credit line and BDF, PCF and fthash notices, and offers the sources. On Windows `LICENSE.freetype` is MSYS2's (`FTL.TXT` and `GPLv2.TXT`), which lacks the BDF, PCF and fthash notices: they are in `THIRD-PARTY-NOTICES.cairo-stack.txt`, which every bundle carries. Every GitHub release attaches `cairo-stack-sources.tar`: the 16 pinned archives, `cmake/cairo-stack/` and `scripts/cairo-stack/`. For Windows the sources are MSYS2's source packages of the closure (the upstream archives, MSYS2's patches and PKGBUILDs): `fetch-msys2.sh --sources <dir>` fetches and checks them (about 450 MB with the C/C++ runtime's, 100 MB with `--no-runtime-sources`).

With the features:

- **GLib, Pango, FriBidi** (and on Windows libthai and libdatrie) are under the LGPL, shared and replaceable, like cairo; on macOS proxy-libintl is static inside libglib, which is itself LGPL and replaceable as a whole. PCRE2, libffi and HarfBuzz are under permissive licences; libjpeg-turbo's notice ("based in part on the work of the Independent JPEG Group", its BSD-3-Clause and zlib texts) is in the notice file.
- **Poppler is GPL.** libpoppler and libpoppler-glib are under the GPL version 2 or later, their Xpdf-derived parts under the GPL version 2 or 3; **this VM distributes them under the GPL version 3**, which every part allows, and `LICENSE.poppler` (Poppler's `COPYING` and `COPYING3`) starts with that line. libpoppler links FreeType, which it uses under FreeType's GPL option (version 2 or later). The notice's "Poppler (GPL)" section says that the VM does not link Poppler (the image loads libpoppler-glib by name), that a program using Poppler through the image may be subject to the GPL, and carries the written offer: the release's `cairo-stack-sources.tar` holds the unmodified `poppler-26.11.0.tar.xz` and the build scripts, and for three years whoever distributed the build offers the source on request, through the repository's issues (`$GITHUB_SERVER_URL/$GITHUB_REPOSITORY/issues` at configure time, else "the address from which you obtained this build").
- The GPL version 3 is chosen because of Windows: the C/C++ compiler runtime ships next to libpoppler, and only GPLv3's "System Libraries" definition exempts it from the source offer, so `--no-runtime-sources` can stay. Keeping GPL-2.0-or-later instead means dropping `--no-runtime-sources` in the `cairo-stack` job (about 350 MB more per release) and changing the notice text.
- **A VM without GPL code** is built with `-DFEATURE_LIB_POPPLER=OFF`: no Poppler file, no `LICENSE.poppler`, no GPL section in the notice.
