# The batteries image

`PharoBatteries-<version>-Pharo15.zip` is the stock Pharo 15 image (the one the WebAssembly site boots, build 117) with the Smalltalk projects that go with the plugins and the libraries of this VM, Delimcc and the Material Symbols, loaded by Metacello and saved as `Pharo15-batteries.image`, whose UI has the Material Symbols as its icons, in gray.
CI builds it on every run and the release of a push to `pharo-12` carries it next to the VM zips; the WebAssembly site offers it as one more image (see [The site](#the-site)).

## What it holds

The projects, loaded in this order by `packaging/batteries/load.st`:

| Project | Baseline, groups | Repository |
|---|---|---|
| Roassal and RTree of the forks (the RTree audit's fixes, RTNaryTreeCollection and the other new collections) | `RoassalMN`, `Full` | `github://massimo-nocentini/Roassal:Pharo15/src`, which takes RTree from `github://massimo-nocentini/RTree:master/src` |
| The UtilsPlugin's image side (fma, strtok, de Casteljau, Knuth's and Vigna's generators; [UtilsPlugin.md](UtilsPlugin.md)) | `VMPluginsPrimitives`, `default` (Core and Tests: not the opt-in Cairo groups, whose package patches Athens at every start) | `github://massimo-nocentini/vm-plugins-primitives.st:master/src` |
| SimdJSON, with NeoJSON and the generated NeoJSON tests ([SimdJSON.md](SimdJSON.md)) | `SimdJSON`, `default` | `github://massimo-nocentini/simdjson.st:master/src` |
| SimdUTF ([SimdUTF.md](SimdUTF.md)) | `SimdUTF`, `default` | `github://massimo-nocentini/simdutf.st:master/src` |
| TimSort ([TimSort.md](TimSort.md)) | `TimSort`, `default` | `github://massimo-nocentini/timsort.st:master/src` |
| GLib, Pango and PangoCairo by uFFI ([PangoPoppler.md](PangoPoppler.md)) | `LibPangoCairo`, `default` | `github://massimo-nocentini/libpangocairo.st:master/src` |
| poppler-glib by uFFI | `LibPoppler`, `default` | `github://massimo-nocentini/libpoppler.st:master/src` |
| CMark: Markdown parse trees of the CMarkPlugin (cmark-gfm), their Roassal view, and a Markdown tab of class comments in Calypso ([CMark.md](CMark.md)) | `CMark`, `default` (Core, Tests, Roassal, RoassalTests, Calypso and CalypsoTests: not the opt-in RoassalFork group, since the fork is loaded first anyway) | `github://massimo-nocentini/cmark-gfm.st:master/src` |
| Containers-Essentials, which Delimcc needs (with Containers-Array2D of pharo-containers) | `ContainersEssentials`, `default` | `github://massimo-nocentini/Containers-Essentials:master` (Tonel at the root of the repository) |
| Containers-Category, which Delimcc needs | `ContainersCategory`, `default` | `github://massimo-nocentini/Containers-Category:master/src` |
| Delimcc: multi-prompt delimited continuations (shift, reset and their kin), and the fixes to Pharo's `Continuation` (Delimcc-Continuation, with the fixed `ContinuationTest`) | `Delimcc`, `default` (Delimcc, Delimcc-Tests, Delimcc-Continuation and Delimcc-Continuation-Tests) | `github://massimo-nocentini/delimcc.st:master` (FileTree at the root of the repository) |
| Material Symbols Rounded (all 3944, at 16, 24 and 32 pixels), `MaterialSymbolsThemeIcons`, the icons of Pharo's UI drawn with them, and `MaterialProtocolSymbols`, a symbol next to the common method protocols (accessing, testing, instance creation, printing...) in Calypso's protocol pane, the status bar of its method editor and the protocol column of its query browsers (Settings > Appearance > "Material Symbols of protocols" turns them off) | `MaterialIcons`, `SymbolsRoundedSmall`, `SymbolsThemePack`, `SymbolsTests`, `SymbolsProtocols` and `SymbolsProtocolsTests` (the `default` group: not the 192-pixel masters, nor the Material Icons Rounded, nor the filled symbols) | `github://massimo-nocentini/material-icons.st:master/src` |

Every `default` group has the project's tests, and so do the groups of MaterialIcons.
After loading MaterialIcons, `load.st` evaluates `MaterialSymbolsThemeIcons installColor: Color gray monochrome: true.`: the image is saved with the Material Symbols as the icons of its UI, every key in gray, the status keys of tests, errors, warnings... too (they are told apart by their shapes: a check, an X, a ! in a disc; `installColor: Color gray` would keep their colours), over the default pack of Pharo 15, which `MaterialSymbolsThemeIcons uninstall` puts back.
Roassal comes first: Pharo 15 ships Roassal and RTree, and the Roassal packages of LibPangoCairo and LibPoppler then build on the fork.
LibPangoCairo comes before LibPoppler, whose baseline loads its GLib group: each project is locked to its repository once loaded, so that the reference of `BaselineOfLibPoppler` to libpangocairo.st is honoured from where libpangocairo.st came.
CMark comes next: `CMark-Roassal` uses only the Roassal API of the stock images, and draws here on the fork loaded first.
ContainersEssentials and ContainersCategory come before Delimcc, which needs them, for the reason LibPangoCairo comes before LibPoppler: `BaselineOfContainersEssentials` names RoassalMN, VMPluginsPrimitives, LibPangoCairo and LibPoppler by the repositories of the table, whose locks Metacello honours, so that a `BATTERIES_<KEY>` repository is also the one they get, and Roassal is loaded once, by `BaselineOfRoassalMN` (nothing loads `BaselineOfRoassal`, whose registration of Pharo stays as it is). The `postLoadDoIt` of `BaselineOfContainersEssentials` fails unless the fork's Roassal-UML has the UML sequence diagrams (`RSUMLSequenceBuilder`).
MaterialIcons comes last; it needs nothing else.

Two loads need care, and `load.st` takes it:

- Pharo 15 ships `BaselineOfRoassal` and the Roassal and RTree packages, which Monticello counts as modified: Metacello asks whether to load the new versions over them (`MCMergeOrLoadWarning`), and the answer must be Load (`onWarningLog`, a plain resume, silently keeps the methods of the image). `load.st` then fails unless RTNaryTreeCollection and the other collections of the fork are there and no RTree or Roassal package is left modified.
- `BaselineOfRoassalMN` is a subclass of `BaselineOfRoassal` that only changes the repository of RTree; declaring Roassal as a nested project instead made Metacello keep the Roassal and RTree projects that Pharo registers (`MetacelloConflictingProjectError`). The load also answers a conflict with the incoming project and an upgrade with the incoming version.

Each Metacello load runs with `IceNonInteractiveCredentialsProvider` and HTTPS clones (GitHub answers a repository that does not exist with a 401, which would otherwise wait for credentials forever); the image is saved with the settings of Iceberg of the stock image.
Any error stops `load.st` with exit status 1 before the image is saved, and so do icons of the UI that are not `MaterialSymbolsThemeIcons`, and a new variable in `Undeclared` but three, which it prints: two that the fork of Roassal has as upstream, classes of Pharo 12 that Pharo 15 has no more (`ClyCommandRegistry` in `RSThemedMapBuilder>>#showMenu:`, `RPackage` in `RSUMLPackageCalypso class>>#classUmlTapActivation2`), and `LibLua`, of a binding of Lua that the batteries do not load (`CTNotebookKeyedValuedOrderedCollection>>#liblua` in Containers-Essentials-Outliner).
It prints a line per project and per repository that Iceberg cloned, with its commit:

    Batteries repository RTree https://github.com/massimo-nocentini/RTree.git master a875929baa2b562d1b4d1b6bdf55882b31678034
    Batteries loaded 12 projects on Pharo 15.0 (0 warnings) ok

The repositories of Iceberg stay registered in the image, without their clones (the zip has none): Iceberg shows them as missing, and clones them again on demand.

On a VM of this repository every project uses its plugin or library; on another VM SimdJSON (which needs the SimdJSONPlugin or libSimdJSONFFI), Pango and Poppler do not work, and the others fall back to Smalltalk.
In the browser, the WebAssembly VM has the plugins, and Pango and Poppler when it is built with `WASM_PANGO=ON` and `WASM_POPPLER=ON` ([WebAssembly.md](WebAssembly.md), The libraries), as the site of GitHub Actions is: there LibPangoCairo and LibPoppler work as natively, with the fonts of the VM (DejaVu).

## Building it

    sh packaging/batteries/build.sh <vm-dir> Pharo15.0-SNAPSHOT.build.117.sha.053a562682.arch.64bit.zip PharoBatteries.zip

`<vm-dir>` holds a headless VM (`pharo` on Linux, put first on `LD_LIBRARY_PATH` with its `lib/`; `Pharo.app`; `PharoConsole.exe`), whose version must be 10.3.9 or later: Pharo 15 refuses to make a UUID on an older one, and the Tonel reader makes one for every package of a local checkout.
`build.sh` copies the image into a work directory (`BATTERIES_WORK`, else a new one in `TMPDIR`) as `Pharo15-batteries.image`, runs `load.st` on it with `st --save --quit`, starts the image saved and runs `smoke.st` on it with `st --no-source` (which keeps the `.changes` as they are), and zips the image, its `.changes`, the `.sources` and `pharo.version`, at the top of the zip as in the zips of files.pharo.org.
It needs network access to GitHub (git over HTTPS) and `unzip`, and `zip` or `python3`.

The settings, all environment variables:

- `BATTERIES_<KEY>`: the repository of a project instead of its `master` (or `Pharo15`) on GitHub, any Metacello repository; for a local checkout `tonel://<checkout>/src`, whose commit `build.sh` prints. The keys are `ROASSAL`, `VMPLUGINSPRIMITIVES`, `SIMDJSON`, `SIMDUTF`, `TIMSORT`, `LIBPANGOCAIRO`, `LIBPOPPLER`, `CMARK`, `CONTAINERSESSENTIALS`, `CONTAINERSCATEGORY`, `DELIMCC` (`filetree://<checkout>` for a local checkout) and `MATERIALICONS`.
- `BATTERIES_<KEY>_GROUPS`: its groups, separated by blanks.
- `BATTERIES_SMOKE`: `basic` (the default) or `native`, below.
- `BATTERIES_NAME` (the name of the image, `Pharo15-batteries`), `BATTERIES_TIMEOUT` (seconds per run of the VM, 1800), `BATTERIES_ICEBERG_REMOTE` (`httpsUrl`, or `scpUrl` to clone with SSH).

For example, with the checkouts of libpangocairo.st and libpoppler.st next to this repository, and a VM of this repository built in `build/`:

    BATTERIES_LIBPANGOCAIRO=tonel://$PWD/../libpangocairo.st/src \
    BATTERIES_LIBPOPPLER=tonel://$PWD/../libpoppler.st/src \
    BATTERIES_SMOKE=native \
        sh packaging/batteries/build.sh build/build/vm Pharo15.0-SNAPSHOT.build.117.sha.053a562682.arch.64bit.zip PharoBatteries.zip

`smoke.st` checks that the image starts again, that a class or two of every project are there, the RTree of the fork (its collections, its packages not modified, its repository), the settings of Iceberg, that the icons of the UI are `MaterialSymbolsThemeIcons` in gray and monochrome (five of its keys and five of its status keys drawn in `Color gray` only), `timSort:`, `timSortDescending` and `Float>>#times:plus:`, and runs the tests of RTree-Tests, TimSort-Smalltalk-Tests, VM-Plugins-Primitives-Tests, CMark-Tests, CMark-Roassal-Tests, CMark-Calypso-Tests, Delimcc-Tests, Delimcc-Continuation-Tests, Pharo's `ContinuationTest` (with the fixes of Delimcc-Continuation), MaterialSymbols-Tests, which put the gray pack back after each test, and MaterialSymbols-Protocols-Tests, which open headless Calypso browsers, each unless `BATTERIES_CMARK_GROUPS` (for CMark-Calypso-Tests: `CalypsoTests`) or `BATTERIES_MATERIALICONS_GROUPS` (for the last two: `SymbolsTests`, `SymbolsProtocolsTests`) leaves out its group and the package is not there (about 2 min, 80 s of them for MaterialSymbols-Tests; without the CMarkPlugin, the CMark tests that parse skip themselves).
It runs in the work directory of `build.sh`, where Iceberg has its clones: on an image unzipped elsewhere the check of the repository of RTree fails, since the zip has none.
With `BATTERIES_SMOKE=native`, on a VM of this repository, it also runs SimdJSON-Tests, SimdJSON-FFI-Tests, TimSort-FFI-Tests, LibGLib-Tests, LibPangoCairo-Tests and LibPoppler-Roassal-Tests, each of which must run tests, not only skip them (about 50 s more).
Its last line is `Batteries smoke <mode> classes <n> tests <run> passed <p> skipped <s> failed <f> errors <e> ok`, or `FAILED`.

## In CI

The `batteries` job of `.github/workflows/build.yml` builds it on ubuntu-24.04 with the VM v12.0.6-beta of files.pharo.org (pinned by URL and SHA256) and the Pharo 15 zip of the WebAssembly job (the same URL and SHA256), runs the basic smoke, and uploads `PharoBatteries-<version>-Pharo15.zip` as an artifact, `<version>` being the one of the VM zips (`v12.0.6-beta-735842692`).
It needs no other job, so it runs while the generate job and the native jobs do; the WebAssembly job, which stages it, waits for it (a few minutes), and the release of a push to `pharo-12` attaches it.
The plugins and the libraries that the projects use are checked on the VM zips by the native jobs (their `ci-load.st` steps, and `tests/cairo-stack/ci-text.st` for Pango and Poppler), not by this job; `BATTERIES_SMOKE=native` runs the projects' own tests on such a VM, by hand.

It loads every project from `master` (Roassal from `Pharo15`) on GitHub: the job passes only when those branches hold the code above (for libpangocairo.st and libpoppler.st, their `stack` branches published as `master`; for Containers-Essentials, a `master` whose `postLoadDoIt` finds the UML sequence diagrams in the Roassal-UML of the `Pharo15` of Roassal).

## The site

The WebAssembly site of CI offers the image as the extra image `batteries` (`WASM_EXTRA_IMAGES="... batteries=PharoBatteries-<version>-Pharo15.zip"`, [WebAssembly.md](WebAssembly.md), Building): its pages list it as **Pharo 15 (batteries)** (a second image of one release gets its id after the label), prepared for the world as the others, and it is not the default, which stays the stock Pharo 15.
It adds about 41 MB to the site (its image, 93 MB, 28 MB gzipped, its `.changes`, 12 MB, 4.7 MB gzipped, and its own copy of the `.sources` of Pharo 15, 8 MB gzipped), downloaded only by those who choose it; before Delimcc and the Material Symbols it was 27 MB (an image of 68 MB, 18 MB gzipped).
Most of the growth is the Material Symbols, whose Forms are in the image and whose sources (their PNGs, in literal byte arrays) are in the `.changes`. The image loads them at 16, 24 and 32 pixels only (`SymbolsRoundedSmall`, the `default` of the baseline); `BATTERIES_MATERIALICONS_GROUPS='SymbolsRounded SymbolsThemePack SymbolsTests'` adds their 192-pixel masters, from which `formNamed:size:` draws the other sizes sharply (without them they come from the 32-pixel icon), and makes about 64 MB in all (an image of 116 MB, 43 MB gzipped, and a `.changes` of 23 MB, 12 MB gzipped). MaterialSymbols-Tests runs either way: its tests of the masters skip themselves without them. When `BATTERIES_MATERIALICONS_GROUPS` leaves out `SymbolsTests`, the smoke that `build.sh` runs with the same variable prints `MaterialSymbols-Tests not loaded`; run by hand on such an image without that variable, it fails with `no package MaterialSymbols-Tests`.
