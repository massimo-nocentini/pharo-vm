# The cairo stack

Athens draws through **cairo** and reads its fonts through **FreeType**, which the image loads by name (`CairoLibrary`, `FT2FFILibrary`). Every VM of this fork ships the same upstream release of cairo, **1.18.6**, pinned by its SHA256, with the libraries around it at their latest security releases. This page explains what is built, how, what is checked, and how to move to a later release.

| VM | How cairo gets there | cairo | FreeType | pixman | libpng | zlib | fontconfig | expat |
|---|---|---|---|---|---|---|---|---|
| Linux x86_64, aarch64 | built from the pinned archives (`scripts/cairo-stack/build.sh`) | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 (static) | 2.18.4 | 2.10.0 (static) |
| macOS x86_64, arm64 | the same | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 (static) | — | — |
| Windows x86_64, ARM64 | MSYS2's binary packages, pinned by URL and SHA256 (`scripts/cairo-stack/fetch-msys2.sh`) | 1.18.6 + MSYS2's 2 patches | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 | 2.18.3 | 2.10.0 |
| WebAssembly | built by CMake from the same archives (`cmake/emscripten/deps/`, `docs/WebAssembly.md`) | 1.18.6 | 2.15.0 | 0.46.4 | 1.6.59 | 1.4 | — | — |

`cmake/cairo-stack/pins.txt` is the single source of these versions. `check-pins.sh` keeps the wasm pins equal to the native ones.

**Linux needs glibc 2.38 for cairo and FreeType** (Ubuntu 24.04+, Debian 13+, Fedora 39+, RHEL 10): the stack is built on the CI's Ubuntu 24.04 runners, and its libraries use `__isoc23_strtol`, `__isoc23_sscanf` and `fmod@GLIBC_2.38`. The VM itself still needs only glibc 2.34; on glibc 2.34 to 2.37 it starts, but Athens cannot load cairo.

## Files

| Path | What |
|---|---|
| `cmake/cairo-stack/pins.txt` | the pins: the 7 source archives, and for Windows the MSYS2 packages of the closure of cairo and FreeType, with their source packages and DLLs |
| `cmake/cairo-stack/keys/*.asc` | the public keys of the signers of zlib, expat, FreeType, pixman and cairo, for `gpgv` at bump time |
| `scripts/cairo-stack/build.sh` | builds the stack into a prefix (Linux, macOS); `--platform mingw` runs `fetch-msys2.sh` |
| `scripts/cairo-stack/fetch-msys2.sh` | unpacks the pinned MSYS2 packages into a prefix (Windows); `--closure` computes the closure when bumping |
| `scripts/cairo-stack/check.sh` | the assertions on a prefix or a bundle (see [What check.sh asserts](#what-checksh-asserts)) |
| `scripts/cairo-stack/probe.c` | the toy-text probe that `check.sh` compiles and runs |
| `scripts/cairo-stack/check-pins.sh` | pins.txt well formed, equal to the wasm pins, its URLs alive |
| `scripts/cairo-stack/check-latest.sh` | the security watch: the pins against each upstream's latest release |
| `scripts/cairo-stack/requirements.txt` | meson, ninja and pkgconf, pinned by hash |
| `cmake/cairoStack.cmake` | the VM build's side: builds or reuses the stack and puts it into the bundle |

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

Windows needs nothing new: `fetch-msys2.sh` uses curl, zstd and tar of the MSYS2 shell. A Linux stack built locally needs the glibc of the machine it was built on.

## Building the stack

```sh
sh scripts/cairo-stack/build.sh --platform linux  --prefix <prefix> [--work <dir>] [--deps <dir>] [--downloads <dir>]
sh scripts/cairo-stack/build.sh --platform darwin --arch arm64 --prefix <prefix> ...   # MACOSX_DEPLOYMENT_TARGET, default 11.0
sh scripts/cairo-stack/fetch-msys2.sh --prefix <prefix> [--env ucrt64|clangarm64] [--deps <dir>] [--downloads <dir>]
```

The VM's CMake does this for you (`cmake/cairoStack.cmake`): in source mode it runs `build.sh` into `<build>/cairo-stack/prefix`, or bundles the prefix of `-DPHARO_CAIRO_STACK_DIR=<prefix>` after validating its manifest.

- `-DPHARO_CAIRO_STACK_GLIBC_MAX=<x.y>` (Linux, source mode) is passed to `build.sh --glibc-max`: the stack build fails when a library needs a newer `GLIBC_` symbol version than that.
- Every configure of a tree with `BUILD_BUNDLE` on (the default) acts on the stack, even one that only generates the VM's sources: without the tools it stops and names them. Such a configure can switch the stack off with `-DFEATURE_LIB_CAIRO=OFF -DFEATURE_LIB_FREETYPE2=OFF` (CI's `generate` job and the host tree of `make wasm` do), or reuse a stack with `PHARO_CAIRO_STACK_DIR`.

**Downloads.** Each archive is looked for in `--deps` (the VM passes `WASM_DEPS_DIR`, which also serves the wasm deps), then in `$PHARO_CAIRO_STACK_DOWNLOADS`, then in `--downloads`. Anything missing is downloaded over HTTPS only (`curl --proto =https --tlsv1.2`) from its URL, or from its `mirror`, into `--downloads`. **Every SHA256 is checked before anything is extracted**: one wrong hash stops the build with the prefix untouched.

**Offline and air-gapped builds** need the 7 archives (or the MSYS2 packages: `fetch-msys2.sh --list --env <env>` lists them) in `--deps` or `$PHARO_CAIRO_STACK_DOWNLOADS`, a pip wheelhouse for `requirements.txt` (`pip download --require-hashes -r ... -d <dir>`, then `pip install --no-index --find-links <dir> ...`), cmake, a compiler and gperf.

**Time.** On a 32-core Linux host the stack builds in about 80 s (150 s CPU).

### Build options

All libraries: `--buildtype=release -Db_ndebug=false --wrap-mode=nofallback` (meson) or `CMAKE_BUILD_TYPE=Release` (CMake). cairo keeps its assertions on, as every shipped cairo had them. pkg-config sees the prefix only (`PKG_CONFIG_LIBDIR=<prefix>/lib/pkgconfig`, `PKG_CONFIG_PATH` unset), CMake too (`CMAKE_PREFIX_PATH=<prefix>`, `CMAKE_FIND_USE_CMAKE_SYSTEM_PATH=OFF`, on macOS `CMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local`), and meson builds no subproject: a library of the system, Homebrew or MSYS2 cannot slip in.

Link flags:

- Linux: `-Wl,--disable-new-dtags,-rpath,$ORIGIN` (a `DT_RPATH`, searched before `LD_LIBRARY_PATH`, so the libraries find each other even when the image loads cairo by absolute path) and `-Wl,--exclude-libs,libz.a:libexpat.a` (the static zlib and expat are not re-exported);
- macOS: `-arch <arch> -Wl,-rpath,@loader_path`, install names `@rpath/<name>`.

| Library | Built with | Options |
|---|---|---|
| zlib | CMake, static (PIC) | `ZLIB_BUILD_SHARED=OFF ZLIB_BUILD_STATIC=ON ZLIB_BUILD_TESTING=OFF` |
| libpng | CMake, shared | `PNG_SHARED=ON PNG_STATIC=OFF PNG_TESTS=OFF PNG_TOOLS=OFF PNG_FRAMEWORK=OFF`, the prefix's static zlib |
| FreeType | meson, shared, **never static** | `-Dharfbuzz=disabled -Dbrotli=disabled -Dbzip2=disabled -Dpng=enabled -Dzlib=system -Dtests=disabled`; macOS also `-Dhvf=disabled` (2.15.0's HVF driver would link Apple's libhvf) |
| expat (Linux) | CMake, static (PIC) | `EXPAT_SHARED_LIBS=OFF`, no tools, examples, tests, docs or fuzzers |
| fontconfig (Linux) | meson, shared | `-Ddoc=disabled -Dtests=disabled -Dtools=disabled -Dcache-build=disabled -Dnls=disabled -Diconv=disabled -Dfontations=disabled -Dxml-backend=expat`, the system's directories (see [fontconfig](#fontconfig-on-linux)) |
| pixman | meson, shared | `-Dtests=disabled -Ddemos=disabled -Dgtk=disabled -Dlibpng=disabled -Dopenmp=disabled`; SIMD left to auto (SSE2/SSSE3, A64 NEON) |
| cairo | meson, shared | `-Dtests=disabled -Dgtk2-utils=disabled -Dglib=disabled -Dspectre=disabled -Dlzo=disabled -Dsymbol-lookup=disabled -Dxlib=disabled -Dxcb=disabled -Dxlib-xcb=disabled -Dfreetype=enabled -Dpng=enabled -Dzlib=enabled`; Linux `-Dfontconfig=enabled -Dquartz=disabled -Ddwrite=disabled`; macOS `-Dfontconfig=disabled -Dquartz=enabled -Ddwrite=disabled` |

`-Dlzo=disabled` and `-Dsymbol-lookup=disabled` keep LZO (GPL-2) and libbfd (GPL-3) out. PNG gives the PNG functions and the SVG surface, zlib the PDF, PostScript and script surfaces. COLRv1 and OT-SVG colour glyphs are on natively (FreeType ≥ 2.13 with its default modules).

`PHARO_CAIRO_STACK_<NAME>_ARGS` (NAME in ZLIB LIBPNG FREETYPE EXPAT FONTCONFIG PIXMAN CAIRO) adds meson or CMake arguments to one library, for example `PHARO_CAIRO_STACK_PIXMAN_ARGS=-Da64-neon=disabled`. It is recorded in the manifest and in `CAIRO-STACK.txt`.

### The prefix

```
<prefix>/include/                 cairo/, freetype2/, fontconfig/, pixman-1/, libpng16/, png.h, zlib.h, expat.h ...
<prefix>/lib/                     the libraries, their symlinks, pkgconfig/ (Windows: bin/*.dll and lib/*.dll.a)
<prefix>/share/pharo-cairo-stack/
    manifest.txt                  see below
    CAIRO-STACK.txt               the index for the bundle: versions, URLs, SHA256, signers, options, toolchain
    licenses/<name>/<files>       the licence files of pins.txt, as in the archive
    meson-summaries/<name>.txt    what meson configured (FreeType, fontconfig, pixman, cairo)
    dlls.txt                      Windows: "<dll> <kind> <package> <sha256>" of the closure
    msys2/<package>.PKGINFO       Windows: the packages' metadata
<prefix>/.pharo-cairo-stack       marks a stack prefix: build.sh empties only such a directory
```

The include directories of the plugin and of anything else that includes `cairo-ft.h` are `<prefix>/include`, `<prefix>/include/cairo` and `<prefix>/include/freetype2` (on Linux and Windows `cairo-ft.h` includes `<fontconfig/fontconfig.h>`).

**The manifest** has one `key value...` per line (`#` lines are comments):

| Key | Value |
|---|---|
| `format` | `1` |
| `pins-sha256` | the SHA256 of the `pins.txt` the stack was made from |
| `platform` | `linux`, `darwin` or `mingw` |
| `arch` | `x86_64` or `aarch64` (Linux), `x86_64` or `arm64` (macOS, Windows) |
| `deployment-target` | macOS: `MACOSX_DEPLOYMENT_TARGET`; `-` elsewhere |
| `script` | `build.sh <n>` or `fetch-msys2.sh <n>` |
| `only` | `freetype` for `--only freetype`, `-` otherwise (build.sh) |
| `msys2-env`, `cairo-version`, `msys2-package <name> <version> <sha256>` | Windows |
| `args <NAME> <value>` | each non-empty `PHARO_CAIRO_STACK_<NAME>_ARGS` |
| `tool <name> <version>` | the toolchain: cc, meson, ninja, pkg-config, cmake, gperf, glibc (Linux), Xcode and SDK (macOS) |
| `lib <name> <version> <sha256>` | each library built |

`cmake/cairoStack.cmake` refuses a `PHARO_CAIRO_STACK_DIR` whose `pins-sha256`, `platform`, `arch` or `deployment-target` differ from the build's: a stale stack from a cache cannot slip in. The `tool` lines are informational.

## What the bundles contain

Only cairo and FreeType are loaded by the image; everything else is a private dependency of those two.

| Library | Linux (zip `lib/`) | macOS (`Contents/MacOS/Plugins`) | Windows (next to the exe) |
|---|---|---|---|
| cairo | `libcairo.so.2.11806.6`, symlinks `libcairo.so.2`, `libcairo.so` | `libcairo.2.dylib`, symlink `libcairo.dylib` | `libcairo-2.dll` |
| FreeType | `libfreetype.so.6.20.7`, symlinks `.so.6`, `.so` | `libfreetype.6.dylib`, symlink `libfreetype.dylib` | `libfreetype-6.dll` |
| pixman | `libpixman-1.so.0.46.4`, symlinks | `libpixman-1.0.dylib`, symlink | `libpixman-1-0.dll` |
| libpng | `libpng16.so.16.59.0`, symlinks | `libpng16.16.59.0.dylib`, symlinks `libpng16.16.dylib`, `libpng16.dylib` | `libpng16-16.dll` |
| fontconfig | `libfontconfig.so.1.17.0`, symlinks | — | `libfontconfig-1.dll` |
| zlib, expat | static inside their users | static | `zlib1.dll`, `libexpat-1.dll` |
| Windows only | | | `libharfbuzz-0.dll`, `libglib-2.0-0.dll`, `libintl-8.dll`, `libiconv-2.dll`, `libpcre2-8-0.dll`, `libgraphite2.dll`, `libbrotlidec.dll`, `libbrotlicommon.dll`, `libbz2-1.dll`, and the C/C++ runtime |
| notices | `LICENSE.<name>`, `THIRD-PARTY-NOTICES.cairo-stack.txt`, `CAIRO-STACK.txt` | the same, in `Contents/Resources` | the same |

Symlinks stay symlinks (`file(COPY)`, `cp -P`): a copied `libfreetype.dylib` would be a second FreeType for dyld.

Nothing loads harfbuzz, bz2 or expat on Linux and macOS, so they are no longer shipped there. On Windows the closure is MSYS2's (FreeType is built with HarfBuzz, brotli and bzip2), so it is kept; `liblzo2-2.dll`, `libcairo-script-interpreter-2.dll` and `libcairo-gobject-2.dll` are in cairo's package but are not imported by `libcairo-2.dll`, and are not shipped.

### One FreeType

cairo and the image must share one FreeType instance: the image hands cairo `FT_Face`s.

- Linux: libcairo and libfontconfig `NEED libfreetype.so.6`, the image opens `libfreetype.so.6`, and glibc reuses an object already loaded under the same soname. No `FT_*` symbol is defined in libcairo or libfontconfig. `DT_RPATH $ORIGIN` makes libcairo find the bundle's FreeType first even when it is loaded first. A VM started without its launcher, with a system FreeType 2.11 or 2.12 loaded first, would give cairo the wrong `FT_ColorStop` ABI (colour fonts misread).
- macOS: `libfreetype.dylib` (the image's name) is a symlink to `libfreetype.6.dylib` (cairo's `@rpath` name), so dyld sees one image.
- Windows: the image and cairo both name `libfreetype-6.dll`, and Windows reuses a loaded module by name.

## fontconfig on Linux

The bundled fontconfig reads the system's configuration, as the desktop does: `/etc/fonts/fonts.conf` and `conf.d`, `/usr/share/fontconfig/conf.avail`, the fonts of `/usr/share/fonts` and `/usr/local/share/fonts`, the user's `~/.config/fontconfig`. These paths are compiled in; nothing of fontconfig's configuration is installed into the prefix or the bundle (`build.sh` installs fontconfig into a staging directory and copies its `lib/` and `include/` only, and checks that nothing was written to `/etc/fonts` or `/var/cache/fontconfig`). The launcher sets no `FONTCONFIG_FILE`, which children of the VM would inherit.

- Without `/etc/fonts/fonts.conf` (a minimal container) fontconfig falls back to its built-in configuration; the old bundles always did, and printed `Fontconfig error: Cannot load default config file`. With no fonts at all, toy text uses cairo's built-in "twin" font.
- Caches: fontconfig 2.18 writes cache version 12 into `~/.cache/fontconfig`; it cannot use the system's caches of another version. The first toy-text call of a user scans the fonts once (measured here: 18–20 ms cold, 6–13 ms warm, on a host with few fonts); where the home is read-only the scan repeats at each start.
- A distribution's `conf.d` newer than 2.18.4 may print "unknown element" warnings.
- Security: fontconfig parses the system and user caches and XML configuration (through expat). 2.18.4 fixes the known cache-validation bugs; see [Known issues](#known-accepted-issues).

## What check.sh asserts

```sh
sh scripts/cairo-stack/check.sh --prefix <prefix> --platform linux [--glibc-max 2.39]
sh scripts/cairo-stack/check.sh --bundle <unzipped VM or build/vm> --platform linux|darwin|mingw [--minos 11.0] [--arch ...]
```

It prints `ok …` or `FAIL …` per assertion and exits 1 on any failure.

- **Linux:** the file names above, the link names are symlinks (`test -L`); every `NEEDED` is a stack soname or glibc's (`libc libm libpthread libdl librt ld-linux*`), so no X11, `libz`, `libexpat`, LZO, libbfd, glib or harfbuzz; `RPATH` exactly `$ORIGIN` and no `RUNPATH`; libcairo and libfontconfig `NEED libfreetype.so.6`, define no `FT_*` and export no zlib or expat symbol; **the highest `GLIBC_` symbol version** (printed as `glibc-max <x.y>`, asserted with `--glibc-max`; 2.38 today); libcairo exports every function of `cmake/emscripten/ffi/symbols/cairo.txt` the images bind; on a prefix, no `etc/`, `var/` or `share/fontconfig`, the licences, the meson summaries (FreeType, Fontconfig, PNG, PDF, SVG, PostScript on; Xlib, XCB, Quartz, DWrite off; LZO and symbol-lookup off; FreeType's zlib `system`) and the manifest of the current pins; on a bundle, no dropped library and the notices.
- **The probe** (Linux, macOS): `probe.c` is compiled and loads libcairo with `dlopen` by absolute path; `cairo_version_string()` must be the pin, and toy text (`sans-serif`, "Ag") must draw ink with the system's configuration without any fontconfig message on stderr, and (Linux) fall back to twin with an empty configuration. It prints the family and the time of the first `cairo_show_text`, cold and warm.
- **macOS:** the names and symlinks, install names `@rpath/<name>`, dependencies `@rpath/<stack>`, `/usr/lib/` and `/System/` only, no system zlib; `LC_RPATH @loader_path`; `minos` ≤ `--minos`; one architecture (`lipo`); `codesign -v`; no `FT_*` in libcairo; the exports, `cairo_quartz_surface_create` included.
- **Windows:** every DLL of the pinned closure (`msys2-dll` lines) is there and of the right machine, and imports only DLLs of the closure and of Windows; in a bundle, each is the pinned package's own copy (its SHA256 in `CAIRO-STACK.txt`), not another MSYS2 package's DLL of the same name (the C/C++ runtime may be the toolchain's); the exports of `libcairo-2.dll`, no `FT_*`; MSYS2's cairo is the pinned version; no `liblzo2`, script interpreter or gobject DLL in the bundle. Toy text is ci-load's `CairoToy` line in CI.

## The pins

Format (also at the top of `pins.txt`):

```
# signed-by <primary key fingerprint> <signature url> ...    or    # unsigned: <how the hash was cross-checked>
<name> <version> <sha256> <file> <url> <platforms> <licence files...>
mirror <name> <url>
native-ahead <name> <until YYYY-MM-DD> <reason...>
msys2 <env> <kind> <package> <version> <sha256> <url> <source sha256> <source url>
msys2-dll <env> <dll> <package>
msys2-mirror <base url>
```

The build order is the file's order. Windows' `msys2` lines are the packages that own a DLL of the closure of `libcairo-2.dll` and `libfreetype-6.dll` (`kind` root, lib or runtime); `pactree` would also reach Python, Tcl/Tk, OpenSSL and others (glib2's tools), which own no such DLL and are not pinned: the end of each section lists them.

To check the deps directory against the pins:

```sh
awk '!/^#/ && NF && $1!="mirror" && $1!="native-ahead" && $1!~/^msys2/ {print $3"  "$4}' cmake/cairo-stack/pins.txt | (cd <deps> && sha256sum -c)
```

### Bump procedure

0. **Authenticate the new archive.** For a signed one, with the committed key:

   ```sh
   gpg --dearmor < cmake/cairo-stack/keys/<name>.asc > /tmp/<name>.gpg
   gpgv --keyring /tmp/<name>.gpg <archive>.sig <archive>                 # zlib (.asc), expat (.asc), FreeType (.sig)
   gpgv --keyring /tmp/cairo.gpg --output - cairo-<v>.tar.xz.sha256sum.asc    # cairo, pixman (.sha512.asc): the signed checksum,
   sha256sum cairo-<v>.tar.xz                                                 # then compare the hash
   ```

   A new signer key is a deliberate change of `keys/`. For an unsigned archive (libpng, fontconfig), cross-check the hash with a second source (libpng: its files are byte-identical to the GitHub tag archive that MSYS2's PKGBUILD pins; fontconfig: its `.sha256sum`, and a distribution's record if there is one). Write the result into the comment line before the pin.
1. Edit the line in `pins.txt` and the matching `cmake/emscripten/deps/<name>.cmake` (version and SHA256; `WASM_CAIRO_FREETYPE_VERSION` for FreeType).
2. For cairo, pixman or FreeType, regenerate the wasm configuration with `cmake/emscripten/deps/cairo/README` and diff it.
3. Run `check-pins.sh`, the stack build and `check.sh`, the C lanes of the CairoPlugin, the Smalltalk suites and the wasm lanes 64, 65 and 74; regenerate the lane-65 goldens.
4. For cairo, pixman or FreeType, re-measure the round-2 limits of the CairoPlugin.

**Windows (MSYS2).** When the cairo or FreeType package changes, or a package of the closure has a security fix:

```sh
sh scripts/cairo-stack/fetch-msys2.sh --closure --env ucrt64     --sources <dir> > ucrt64.txt
sh scripts/cairo-stack/fetch-msys2.sh --closure --env clangarm64 --sources <dir> > clangarm64.txt
```

It reads `https://repo.msys2.org/mingw/<env>/<env>.files`, downloads the packages (checked against the database), walks the DLL imports from `libcairo-2.dll` and `libfreetype-6.dll` with objdump (GNU or LLVM; LLVM's reads ARM64), hashes every source package, and prints the `msys2` and `msys2-dll` lines: replace the two sections of `pins.txt` with them. Check that cairo's source package still holds the pinned upstream archive (its SHA256) and which patches its PKGBUILD applies.

**Retention.** repo.msys2.org removes superseded packages after a while, so every bump mirrors the pinned MSYS2 files as assets of a release of the fork, and adds the `msys2-mirror` line:

```sh
sh scripts/cairo-stack/fetch-msys2.sh --list --env ucrt64 > files.txt      # and clangarm64
# download each, check its SHA256, then:
gh release create cairo-stack-msys2-<date> --repo <owner>/pharo-vm --notes "MSYS2 packages pinned by cmake/cairo-stack/pins.txt" <files...>
# pins.txt: msys2-mirror https://github.com/<owner>/pharo-vm/releases/download/cairo-stack-msys2-<date>
```

`check-pins.sh` probes every URL and falls back to the mirror; a package gone from both fails it.

**accepted-behind.** A pin kept behind a newer upstream release on purpose (decision U4; there is none today): `accepted-behind <name> <version> <until YYYY-MM-DD> <reason>` makes the weekly watch (`check-latest.sh`) report that exact release as "accepted" until the date; any other newer release, or the date passing, makes the run red again, so each exception is reviewed.

**native-ahead.** A security fix may go into the native VMs before wasm (whose configuration must be regenerated): `native-ahead <name> <until YYYY-MM-DD> <reason>` makes `check-pins.sh` warn about the difference until that date and fail after it.

## Security bumps

- `scripts/cairo-stack/check-latest.sh` compares each pin with its upstream's latest release (the GitHub releases API for zlib and expat, the GitLab API for fontconfig, the cairographics.org listing for cairo and pixman, savannah for FreeType, SourceForge for libpng) and each MSYS2 pin with MSYS2's database. It exits 1 when one is behind, and runs weekly in CI (`.github/workflows/cairo-stack-watch.yml`), so a pin behind its upstream shows as a red run.
- A release that fixes a CVE reachable from the images is bumped within 14 days, natively first (`native-ahead`) if wasm needs a configuration regeneration.
- zlib and expat are static and private on Linux and macOS: distributions' patches no longer reach them, and every fix needs a VM release.
- Every source pin is at its upstream's latest release: pixman 0.44.2 and zlib 1.3.2, kept behind with `accepted-behind` from 2026-11-18, were bumped to 0.46.4 and 1.4 on 2026-11-19 (zlib 1.4 fixes the `gzwrite` and `infback9` CVEs and window overruns after `inflateSync`, `inflatePrime` or `inflateGetDictionary`; pixman 0.46 adds the `pixman_region64f_*` functions and no security fix).

## Known accepted issues

- **cairo 1.18.6:** CVE-2025-50422, a reachable assertion in cairo-ft (MR !621 still open): with assertions on, it aborts the VM, as the previous bundles did. The image-information fixes on cairo's master (JPEG, JPEG 2000, JBIG2 bounds) are reachable only through `cairo_surface_set_mime_data`, which no image binds. 1.18.6 also creates more scaled fonts for toy text (commit 46d1e26c: the toy faces miss cairo's global cache); Athens' FreeType path is unaffected.
- **expat 2.10.0:** upstream lists unfixed security issues; only fontconfig's XML configuration reaches it.
- **Windows:** the versions are MSYS2's (cairo 1.18.6 with the D2D/DWrite patch and MR !673, FreeType 2.15.0 with HarfBuzz), recorded in `CAIRO-STACK.txt`.
- Linux aarch64: the `libz.so.1` of the zip (zlib 1.2.11) comes from libgit2's prebuilt dependencies, not from the cairo stack.

## CI

`.github/workflows/build.yml` wires the stack into the zips:

- **Linux and macOS** build the stack on the runner with `build.sh` (meson, ninja and pkgconf from `requirements.txt` in a venv; gperf from apt on Linux) into `$RUNNER_TEMP/cairo-stack`, and configure the VM with `-DPHARO_CAIRO_STACK_DIR` (the Configure step asserts "manifest checked"). The prefix is cached by `actions/cache`, keyed by the OS, the architecture, the hash of `cmake/cairo-stack/**` and `scripts/cairo-stack/**`, and a fingerprint of the toolchain (compilers, glibc or Xcode and SDK, cmake, gperf, python). Release runs (pushes to `pharo-12`) do not restore the cache: they always build the stack.
- **Windows ARM64** fetches the MSYS2 packages with `fetch-msys2.sh` (cached, keyed by the hash of `cmake/cairo-stack/**` and `scripts/cairo-stack/**`) and configures with `-DPHARO_CAIRO_STACK_DIR`; **Windows x86_64** uses the ExternalProject path of `cmake/cairoStack.cmake` (Configure asserts "cairo stack: built from"). pacboy installs no cairo and no freetype.
- Every native job then runs `check.sh --bundle` on the unzipped zip (`--glibc-max 2.39` on Linux, whose summary also reports the VM's own glibc floor; `--minos 11.0` on macOS), and ci-load.st must print `CairoPlugin native 1 cairo <the pin> ok` and `CairoToy ok family … ink <n>` with n > 0. macOS also asserts that dyld loads one libfreetype and one libcairo.
- The **`cairo-stack`** job runs `check-pins.sh` and gathers `cairo-stack-sources.tar` (the 7 archives, each checked against `pins.txt`; MSYS2's source packages of both closures, from `fetch-msys2.sh --sources --no-runtime-sources`; `cmake/cairo-stack/`, `scripts/cairo-stack/` and `SHA256SUMS`), which the release job attaches. The C/C++ runtime's source packages (GCC, LLVM's libc++, winpthreads: about 350 MB) are left out; drop `--no-runtime-sources` to include them.
- **`cairo-stack-watch.yml`** runs `check-latest.sh` (its output goes to the job summary) and `check-pins.sh` every Monday, on manual dispatch, and on pushes that change the workflow or `check-latest.sh`: GitHub runs scheduled and dispatched workflows only from the default branch, so the push trigger lets it run on a branch.

## Licences and sources

Each bundle has `LICENSE.<name>` (the licence files of `pins.txt`, or of the MSYS2 packages) and `THIRD-PARTY-NOTICES.cairo-stack.txt`, which elects cairo's LGPL-2.1 and FreeType's FTL, gives FreeType's credit line and BDF, PCF and fthash notices, and offers the sources. On Windows `LICENSE.freetype` is MSYS2's (`FTL.TXT` and `GPLv2.TXT`), which lacks the BDF, PCF and fthash notices: they are in `THIRD-PARTY-NOTICES.cairo-stack.txt`, which every bundle carries. Every GitHub release attaches `cairo-stack-sources.tar`: the 7 pinned archives, `cmake/cairo-stack/` and `scripts/cairo-stack/`. For Windows the sources are MSYS2's source packages of the closure (the upstream archives, MSYS2's patches and PKGBUILDs): `fetch-msys2.sh --sources <dir>` fetches and checks them (about 450 MB with the C/C++ runtime's, 100 MB with `--no-runtime-sources`).
