# The batteries image

`PharoBatteries-<version>-Pharo15.zip` is the stock Pharo 15 image (the one the WebAssembly site boots, build 117) with the Smalltalk projects that go with the plugins and the libraries of this VM, loaded by Metacello and saved as `Pharo15-batteries.image`.
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

Every `default` group has the project's tests.
Roassal comes first: Pharo 15 ships Roassal and RTree, and the Roassal packages of LibPangoCairo and LibPoppler then build on the fork.
LibPangoCairo comes before LibPoppler, whose baseline loads its GLib group: each project is locked to its repository once loaded, so that the reference of `BaselineOfLibPoppler` to libpangocairo.st is honoured from where libpangocairo.st came.

Two loads need care, and `load.st` takes it:

- Pharo 15 ships `BaselineOfRoassal` and the Roassal and RTree packages, which Monticello counts as modified: Metacello asks whether to load the new versions over them (`MCMergeOrLoadWarning`), and the answer must be Load (`onWarningLog`, a plain resume, silently keeps the methods of the image). `load.st` then fails unless RTNaryTreeCollection and the other collections of the fork are there and no RTree or Roassal package is left modified.
- `BaselineOfRoassalMN` is a subclass of `BaselineOfRoassal` that only changes the repository of RTree; declaring Roassal as a nested project instead made Metacello keep the Roassal and RTree projects that Pharo registers (`MetacelloConflictingProjectError`). The load also answers a conflict with the incoming project and an upgrade with the incoming version.

Each Metacello load runs with `IceNonInteractiveCredentialsProvider` and HTTPS clones (GitHub answers a repository that does not exist with a 401, which would otherwise wait for credentials forever); the image is saved with the settings of Iceberg of the stock image.
Any error stops `load.st` with exit status 1 before the image is saved, and so does a new variable in `Undeclared` but two that the fork of Roassal has as upstream, classes of Pharo 12 that Pharo 15 has no more (`ClyCommandRegistry` in `RSThemedMapBuilder>>#showMenu:`, `RPackage` in `RSUMLPackageCalypso class>>#classUmlTapActivation2`), which it prints.
It prints a line per project and per repository that Iceberg cloned, with its commit:

    Batteries repository RTree https://github.com/massimo-nocentini/RTree.git master a875929baa2b562d1b4d1b6bdf55882b31678034
    Batteries loaded 7 projects on Pharo 15.0 (0 warnings) ok

The repositories of Iceberg stay registered in the image, without their clones (the zip has none): Iceberg shows them as missing, and clones them again on demand.

On a VM of this repository every project uses its plugin or library; on another VM SimdJSON (which needs the SimdJSONPlugin or libSimdJSONFFI), Pango and Poppler do not work, and the others fall back to Smalltalk.
In the browser, the WebAssembly VM has the plugins but neither Pango nor Poppler ([WebAssembly.md](WebAssembly.md), The libraries): LibPangoCairo and LibPoppler are there, and their tests skip.

## Building it

    sh packaging/batteries/build.sh <vm-dir> Pharo15.0-SNAPSHOT.build.117.sha.053a562682.arch.64bit.zip PharoBatteries.zip

`<vm-dir>` holds a headless VM (`pharo` on Linux, put first on `LD_LIBRARY_PATH` with its `lib/`; `Pharo.app`; `PharoConsole.exe`), whose version must be 10.3.9 or later: Pharo 15 refuses to make a UUID on an older one, and the Tonel reader makes one for every package of a local checkout.
`build.sh` copies the image into a work directory (`BATTERIES_WORK`, else a new one in `TMPDIR`) as `Pharo15-batteries.image`, runs `load.st` on it with `st --save --quit`, starts the image saved and runs `smoke.st` on it with `st --no-source` (which keeps the `.changes` as they are), and zips the image, its `.changes`, the `.sources` and `pharo.version`, at the top of the zip as in the zips of files.pharo.org.
It needs network access to GitHub (git over HTTPS) and `unzip`, and `zip` or `python3`.

The settings, all environment variables:

- `BATTERIES_<KEY>`: the repository of a project instead of its `master` (or `Pharo15`) on GitHub, any Metacello repository; for a local checkout `tonel://<checkout>/src`, whose commit `build.sh` prints. The keys are `ROASSAL`, `VMPLUGINSPRIMITIVES`, `SIMDJSON`, `SIMDUTF`, `TIMSORT`, `LIBPANGOCAIRO` and `LIBPOPPLER`.
- `BATTERIES_<KEY>_GROUPS`: its groups, separated by blanks.
- `BATTERIES_SMOKE`: `basic` (the default) or `native`, below.
- `BATTERIES_NAME` (the name of the image, `Pharo15-batteries`), `BATTERIES_TIMEOUT` (seconds per run of the VM, 1800), `BATTERIES_ICEBERG_REMOTE` (`httpsUrl`, or `scpUrl` to clone with SSH).

For example, with the checkouts of libpangocairo.st and libpoppler.st next to this repository, and a VM of this repository built in `build/`:

    BATTERIES_LIBPANGOCAIRO=tonel://$PWD/../libpangocairo.st/src \
    BATTERIES_LIBPOPPLER=tonel://$PWD/../libpoppler.st/src \
    BATTERIES_SMOKE=native \
        sh packaging/batteries/build.sh build/build/vm Pharo15.0-SNAPSHOT.build.117.sha.053a562682.arch.64bit.zip PharoBatteries.zip

`smoke.st` checks that the image starts again, that a class or two of every project are there, the RTree of the fork (its collections, its packages not modified, its repository), the settings of Iceberg, `timSort:`, `timSortDescending` and `Float>>#times:plus:`, and runs the tests of RTree-Tests, TimSort-Smalltalk-Tests and VM-Plugins-Primitives-Tests (about 15 s).
With `BATTERIES_SMOKE=native`, on a VM of this repository, it also runs SimdJSON-Tests, SimdJSON-FFI-Tests, TimSort-FFI-Tests, LibGLib-Tests, LibPangoCairo-Tests and LibPoppler-Roassal-Tests, each of which must run tests, not only skip them (about 50 s).
Its last line is `Batteries smoke <mode> classes <n> tests <run> passed <p> skipped <s> failed <f> errors <e> ok`, or `FAILED`.

## In CI

The `batteries` job of `.github/workflows/build.yml` builds it on ubuntu-24.04 with the VM v12.0.6-beta of files.pharo.org (pinned by URL and SHA256) and the Pharo 15 zip of the WebAssembly job (the same URL and SHA256), runs the basic smoke, and uploads `PharoBatteries-<version>-Pharo15.zip` as an artifact, `<version>` being the one of the VM zips (`v12.0.6-beta-735842692`).
It needs no other job, so it runs while the generate job and the native jobs do; the WebAssembly job, which stages it, waits for it (a few minutes), and the release of a push to `pharo-12` attaches it.
The plugins and the libraries that the projects use are checked on the VM zips by the native jobs (their `ci-load.st` steps, and `tests/cairo-stack/ci-text.st` for Pango and Poppler), not by this job; `BATTERIES_SMOKE=native` runs the projects' own tests on such a VM, by hand.

It loads every project from `master` (Roassal from `Pharo15`) on GitHub: the job passes only when those branches hold the code above (for libpangocairo.st and libpoppler.st, their `stack` branches published as `master`).

## The site

The WebAssembly site of CI offers the image as the extra image `batteries` (`WASM_EXTRA_IMAGES="... batteries=PharoBatteries-<version>-Pharo15.zip"`, [WebAssembly.md](WebAssembly.md), Building): its pages list it as **Pharo 15 (batteries)** (a second image of one release gets its id after the label), prepared for the world as the others, and it is not the default, which stays the stock Pharo 15.
It adds about 26 MB to the site (its image, 18 MB gzipped, its `.changes`, and its own copy of the `.sources` of Pharo 15, 7.9 MB gzipped), downloaded only by those who choose it.
