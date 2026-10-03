# Building for WebAssembly (Emscripten)

The VM can be cross-built to WebAssembly with Emscripten.
The build runs from the root of this repository, out of tree, by default in `build-wasm/`.
It builds the StackVM, the interpreter (a JIT needs executable memory), for 64-bit images, with no FFI and no threads, and runs the stock Pharo 12 image.
Two VMs are linked from the same objects:

- a command-line VM for node, with access to the host file system, environment and exit status;
- a VM for a Web Worker, which a static web site runs: the Console, a Smalltalk REPL with Stop, Save, Download and Upload.

Requirements:

- Emscripten 6.0.10 (emsdk).
  The port needs that version, and was tested only with it: the web driver replaces the stream operations of Emscripten's terminal device, which belong to the internals of its file system.
  Nothing checks the version: with another one, run `make wasm-check`, whose lanes 60 and 70 drive those operations in node.
- GNU make 3.81 or later (tested with 3.81, 4.2.1, 4.3 and 4.4.1) and CMake 3.13 or later (tested with 3.13.5 and 3.28.3).
- node, which stages the site, runs the tests and serves the pages (tested with 25.2.1).
- A native C compiler: the sources are generated in a native CMake tree, `build-wasm/host`.
- Network access for the first build (files.pharo.org and GitHub), unless the offline settings below give its inputs.
- A source tree whose path holds no blank.

Engines, as Emscripten's feature matrix gives them for the default settings:

- The node VM uses a 64-bit memory (memory64) and the exnref instructions of WebAssembly exception handling, for setjmp and longjmp.
  It needs node 24.15 or later.
- The web VM has a 32-bit memory with 64-bit pointers (`WASM_WEB_MEMORY64=2`), so it needs only exnref: Chrome and Edge 137, Firefox 131 or Safari 18.4, or later.
  `WASM_SJLJ=wasm-legacy` builds it for engines without exnref (Chrome 95, Firefox 100, Safari 15.2).
- The pages inflate what they download with DecompressionStream (Chrome 80, Firefox 113, Safari 16.4), and say so in a browser without it, so a `wasm-legacy` build needs Chrome 95, Firefox 113 or Safari 16.4, or later.
- The pages were tested in Chromium 153 and Firefox 155.
  Safari is untested.

## Building

    source /path/to/emsdk/emsdk_env.sh
    make wasm                 # everything, in build-wasm/
    make wasm-check           # the node test lanes
    make wasm-serve           # serves build-wasm/web at http://localhost:8080/

`make wasm` first generates the sources of the StackVM in `build-wasm/host`.
That native tree bootstraps a VMMaker image (a Pharo 13 image and VM from files.pharo.org, into which it loads the VMMaker packages of `smalltalksrc` and their dependencies from GitHub), and is configured with SSL and UUID off, since it builds nothing but the sources.
Later, when a `.st` file of `smalltalksrc` changes, comes or goes, `scripts/refreshVMMaker.st` reloads the tree's packages into that image, and the sources are generated again.
Then `build-wasm/cmake` is configured with `emcmake` and the initial cache `cmake/Emscripten.cache.cmake`, and built.
It is configured again when a setting or that initial cache changes, and CMake configures it again by itself when a plugin of `src/emscripten/plugins` comes or goes.
The build downloads the pinned Pharo 12 image (`Pharo12.0-SNAPSHOT.build.1599.sha.7d5f14cb47.arch.64bit.zip`, checked against its SHA256), and stages `build-wasm/node` and `build-wasm/web`.

The first `make wasm` took about 2.5 minutes from a clean clone with the downloads, and about 1.5 minutes offline, on the machine the port was made on: most of that time goes to the VMMaker image, which loads its packages and generates the sources in a single process.
When nothing has changed, it takes under a second.

To build offline, give the inputs that the first build would download: a VMMaker image, the Pharo VM that runs it, and the image zip (from https://files.pharo.org/image/120/).
With a native build of this repository in `build/` that generated its sources, on Linux, and the zip in the source tree:

    make wasm WASM_VMMAKER_IMAGE=build/build/vmmaker/image/VMMaker.image \
              WASM_VMMAKER_VM=build/build/vmmaker/vm/pharo \
              WASM_IMAGE_ZIP=Pharo12.0-SNAPSHOT.build.1599.sha.7d5f14cb47.arch.64bit.zip

An earlier `make wasm` has the same inputs in `build-wasm/host/build/vmmaker/` and `build-wasm/downloads/`.
`WASM_GENERATED` instead takes sources generated already (see Settings).

The site must be served over HTTP: browsers run no workers from `file://` URLs.
It needs no special headers (no COOP or COEP, since nothing uses SharedArrayBuffer), so any static web server will do.

## This produces

- `build-wasm/node/pharo`: the command-line VM, a shell script that runs `pharo.js` (and `pharo.wasm`) next to it with node.
  `NODE` selects the node binary, and `NODE_OPTIONS_WASM` gives it options.
- `build-wasm/web/`: the static site.
  It holds the Console page (`index.html`), the web VM (`pharo-web.js`, the factory `createPharoVM`, and `pharo-web.wasm`), the scripts of the worker, the image, its `.changes` and the `.sources`, gzipped, in `image/`, the Smalltalk scripts of the pages in `st/`, and `manifest.json`, which lists the files the worker loads.
  The pages load every file with the build id in its URL (`?v=<id>`), so a new build is never mixed with a cached one.
  The browser downloads about 23 MB: an image of about 54 MB and the 43 MB `.sources`, gzipped.
- `build-wasm/image/stock/`: the unpacked Pharo 12 image (as `Pharo.image` and `Pharo.changes`, with its `.sources`); `build-wasm/image/stock.stamp` records the SHA256 of the zip it came from.
- `build-wasm/host/`: the native tree that generates the sources, in `host/generated/64/{vm,plugins}`, and its VMMaker image (in `host/build/vmmaker/image`, or in `host/vmmaker-image` for a copy of `WASM_VMMAKER_IMAGE`).
- `build-wasm/cmake/`: the CMake tree of the WebAssembly build.
- `build-wasm/downloads/` and `build-wasm/tests-run/`: the downloaded image zip, and the scratch directory of `make wasm-check`.
- `build-wasm/config.make`, `build-wasm/config-host.make` and `build-wasm/.make-wasm`: the recorded settings, and the mark of a directory that `make wasm` builds in.

The node VM takes the usual arguments of the VM, for example on a copy of the stock image:

    mkdir -p build-wasm/try && cp build-wasm/image/stock/* build-wasm/try/
    build-wasm/node/pharo --headless build-wasm/try/Pharo.image --no-default-preferences eval '3 + 4'
    build-wasm/node/pharo --headless build-wasm/try/Pharo.image --no-default-preferences st --quit script.st

## The Console page

`index.html` runs the REPL of `st/web-repl.st` in the image, in a Web Worker.
Each line typed is evaluated, and its value printed, or the error it raised, with up to five frames.
A syntax error says where it is, as in `Error: CodeError Undeclared variable x (line 1, column 1)`.
A Warning that nothing handles is reported (as `Error: Warning ...`), and the evaluation goes on.
The REPL is also the error handler of the other processes: an error in a process that an evaluation forked is reported, and only that process ends.
Output streams as the VM writes it, and the terminal keeps the last 20000 lines.

- Enter evaluates, and Shift+Enter adds a line.
  Up and Down recall the history, which is kept in the browser's localStorage when there is one.
- Stop (Esc, or Ctrl+C with nothing selected) interrupts the evaluation, also one that has not started yet.
  When nothing could be interrupted after the first prompt (the image registered no interrupt semaphore), or when the worker says nothing for 3 s after the Stop (a primitive that runs that long never yields), the worker is replaced and the VM starts again: the changes since the last Save are lost.
- Save evaluates `Smalltalk snapshot: true andQuit: false`.
  Whenever the image is saved, by Save or by the image itself, the worker keeps the image and its `.changes` in the browser's IndexedDB, and the next visit boots them ("Started the image saved in this browser").
  The `.changes` is stored again when it changed: about 0.5 s after the VM goes idle, at the latest 5 s after it was last stored, and when the VM exits.
  When the page goes away, the worker tries to store it too, but may not have the time.
- The browser asks before the page goes while a save is being stored, or when code was changed since the image was last stored.
- Download gives the image and its `.changes` as last saved.
- Upload starts an image of your own: choose its `.image` and `.changes`.
  It replaces the saved image only once it has started: one that crashes or quits before leaves the saved image as it was.
- Reset deletes the saved image, and starts the original one.
  When the saved image itself does not start, the notice offers Reset next to Restart.
- Restart starts the VM again from the saved image.
  Ctrl+D on an empty line ends the session (after asking), and Ctrl+L clears the terminal.
- When two tabs share the saved image, the last one to save wins.
  The other tab says that its image was replaced ("Another tab has replaced the image saved in this browser"), and stops storing its `.changes` until it saves again.
- For screen readers, a live region next to the status pill says how the VM goes: loading, starting, ready, and how it ended.

## Other targets

`make wasm-check-browser` runs the Playwright spec of the Console (see Tests).
Without Playwright (`PLAYWRIGHT_MODULE`, or a `playwright` package that node finds from `tests/wasm/lib`) it fails, and says so.
`make wasm-clean` removes the WebAssembly build (`build-wasm/cmake`, `node`, `web` and `tests-run`), and keeps the generated sources, the images and the downloads.
`make wasm-distclean` also removes `build-wasm/host`, `build-wasm/image` and the recorded settings, and, with `WASM_CLEAN_DOWNLOADS=1`, `build-wasm/downloads`.

GNU make reads `GNUmakefile` before `Makefile`.
In an in-source CMake build, the default goal and every goal that `GNUmakefile` does not define go to that build's `Makefile`, as before.

The WebAssembly tree can also be configured with CMake alone, from sources generated already:

    emcmake cmake -C cmake/Emscripten.cache.cmake -S . -B build-wasm-cmake -DGENERATED_SOURCE_DIR=$PWD/build-wasm/host
    cmake --build build-wasm-cmake

It stages `node/` and `web/` in its build directory, or in `WASM_STAGE_DIR`.
Its cache variables are the settings `WASM_SJLJ`, `WASM_WEB_MEMORY64`, `WASM_STACK_SIZE`, `WASM_INITIAL_MEMORY`, `WASM_MAXIMUM_MEMORY`, `WASM_OLD_SPACE_BASE`, `WASM_SLICE_MS` and `WASM_IMAGE_ZIP` below, `CMAKE_BUILD_TYPE` (`Debug` for `WASM_DEBUG=1`), `WASM_STAGE_DIR`, and `NODE_JS_EXECUTABLE`, the node that stages `web/`.

## Settings

Settings are given on the `make` command line and recorded in the build directory, in `config.make` (and the two VMMaker settings in `config-host.make`): changing one configures the build again, and repeating it changes nothing.
They are recorded, not remembered: every `make wasm...` command for a build directory must repeat them, since a setting left out goes back to its default.
To keep two builds, give each its own `WASM_BUILDDIR`:

    make wasm WASM_BUILDDIR=build-wasm-debug WASM_DEBUG=1

- `WASM_BUILDDIR`: the build directory (default `build-wasm`; not recorded).
  It must be one path, not empty, and neither it nor what its symbolic links resolve to may contain a blank or any of ``' " ` \ $ ; & | < > ( ) { } [ ] * ? % : #``; a relative one is taken from the current directory, whose path must then hold no blank either.
  It must not be `/`, `$HOME`, the source tree, a directory holding it, or a directory holding a `CMakeLists.txt` or a `.git`.
  And it must be new or empty, or marked by an earlier `make wasm` (with `.make-wasm`, or the `config.make` of an older version), or a `build-wasm*` directory of the source tree, where the inputs of an offline build can be put first.
  An empty `WASM_BUILDDIR=`, as an unset shell variable gives, is refused rather than taken for the default: leave the setting out instead.
- `WASM_DEBUG`: `1` for a Debug build (default `0`): `-O0 -g`, the C asserts of the VM, Emscripten's `-sASSERTIONS=2`, and a checked stack (`-sSTACK_OVERFLOW_CHECK=2`).
  The VM is many times slower with its asserts, so the time limits of the lanes fail by design (see Troubleshooting).
- `WASM_SJLJ`: how the VM's setjmp and longjmp are compiled (default `wasm`).
  `wasm` uses WebAssembly exception handling with exnref, `wasm-legacy` its legacy instructions (try, delegate and rethrow, for engines without exnref; Firefox logs them as deprecated), and `emscripten` calls out to JavaScript.
- `WASM_WEB_MEMORY64`: the memory of the web VM (default `2`).
  `2` is a 32-bit memory with 64-bit pointers, which is 15 to 18% faster and runs in engines without memory64; `1` is memory64, as in the node VM, and the Console says so in a browser without it.
  The node VM always has memory64, so that a stray pointer traps rather than wrapping around.
- `WASM_STACK_SIZE`: the C stack (default `8MB`).
- `WASM_INITIAL_MEMORY`, `WASM_MAXIMUM_MEMORY`: the linear memory, its initial size (default `32MB`) and its maximum (default `4GB`); it grows between them.
- `WASM_OLD_SPACE_BASE`: where old space begins (default `0x20000000`, 512 MiB), a power of two above new space.
  Old space stays below twice that address, so it can grow to that size, 512 MiB by default, and twice it must fit in `WASM_MAXIMUM_MEMORY`.
  Past that window an allocation fails with an `OutOfMemory`, and an image that does not fit in it does not start.
  The environment variable `PHARO_WASM_OLD_SPACE_BASE` overrides it when the VM starts, for example `0x40000000` for 1 GiB.
  Changing the setting recompiles the VM core.
- `WASM_SLICE_MS`: the length of a slice of the VM in milliseconds (default `20`); `PHARO_WASM_SLICE_MS` overrides it when the VM starts.
- `WASM_HOST_PHARO`: a native Pharo VM (not recorded).
  By default, it is the VM that runs VMMaker: `WASM_VMMAKER_VM`, or the one that the host tree downloads, `build-wasm/host/build/vmmaker/vm/pharo`.
  With `WASM_GENERATED`, which skips the host tree, that is only `WASM_VMMAKER_VM`, or a VM left there by an earlier build of the same directory: give it otherwise.
  The lanes use it to load an image that the WebAssembly VM saved (S14b, which is skipped without it).
- `WASM_IMAGE_ZIP`: a local copy of the Pharo 12 image zip, instead of downloading it into `build-wasm/downloads`.
  Any zip holding one image, its `.changes` and a `.sources` is accepted; one that is not the pinned image gets a note with its SHA256.
- `WASM_GENERATED`: a directory holding `generated/64`, the generated StackVM sources, which skips the host tree.
  For example, `make wasm WASM_BUILDDIR=build-wasm-2 WASM_GENERATED=build-wasm/host WASM_HOST_PHARO=build-wasm/host/build/vmmaker/vm/pharo` builds a second directory with the sources and the native VM of the first.
  The sources must come from this tree's `smalltalksrc`: the configuration refuses sources without the Emscripten hooks.
- `WASM_VMMAKER_IMAGE`, `WASM_VMMAKER_VM`: a VMMaker image, copied with its directory into `build-wasm/host/vmmaker-image` instead of bootstrapping one, and the Pharo VM that runs it (by default the one the host tree downloads, PharoVM 10.3.1 on Linux x86_64).
  The copy is refreshed from `smalltalksrc` the first time it is used, which takes about 12 s.
  These two are recorded in `config-host.make`: changing them configures the host tree again.
- `WASM_PORT`: the port of `make wasm-serve` (default `8080`; not recorded).
- `WASM_JOBS`: the jobs of the CMake builds when `make` itself runs without `-j` (default: the number of processors; not recorded); with `-jN`, they share make's job slots.
- `WASM_CLEAN_DOWNLOADS`: `1` makes `make wasm-distclean` remove `build-wasm/downloads` too (default `0`; not recorded).
- `CMAKE`, `EMCMAKE`, `NODE`: the cmake, emcmake (default: from `$EMSDK`, else from the PATH) and node of the build, found on the PATH by the shell, and recorded.
  (make's own lookup would stop at the directories named `cmake` and `node` that `emsdk_env.sh` puts on the PATH.)

The other `WASM_` names of `GNUmakefile` (`WASM_GOALS`, `WASM_REQUESTED`, `WASM_CMAKE_FLAGS`, `WASM_MARK`, `WASM_SPECIAL_CHARACTERS`, `WASM_BUILDDIR_CHECKS`, `WASM_BUILDDIR_CHECKED` and `WASM_CHECK_BUILDDIR`) are internal, not settings, and `WASM_DIR` is a variable of the lanes (see Tests).

When the VM runs:

- `NODE`, `NODE_OPTIONS_WASM`: the node that `build-wasm/node/pharo` runs (default: `node` on the PATH), and its options, for example `--stack-size=900` or `--liftoff-only`.
- `PHARO_WASM_SLICE_MS`, `PHARO_WASM_OLD_SPACE_BASE`: override `WASM_SLICE_MS` and `WASM_OLD_SPACE_BASE`.
  An old-space base that is not a power of two above new space, or whose window does not fit in the memory, stops the VM with a message.
- `PHARO_WASM_STATS`: `1` makes the node VM print its slices on stderr when it ends: `[pharo-wasm] slices=N busy=N sleeping=N`.
- `--maxFramesToLog`: the Smalltalk frames printed per process when the VM reports an error, 1000 by default on Emscripten (elsewhere, every frame); 0 prints every frame.
  A crash deep in a recursion would otherwise print hundreds of megabytes, which takes the VM minutes.
  The default is `PHARO_DEFAULT_MAX_FRAMES_TO_LOG`, in `include/pharovm/emscripten/sqPlatformSpecific.h`.

## How it works

### Slices

A browser worker, and node's event loop, need their thread back for input, timers and output, and the VM cannot run on a thread of its own: threads need SharedArrayBuffer, which needs cross-origin isolation, and JSPI is not available everywhere.
So the VM runs in slices and returns to its host between them (`src/emscripten/emscriptenMain.c`).
`main()` only gets the VM ready and returns with the runtime alive.
Every slice is then a call of `vm_resume()`: the first one starts the VM, and the later ones call `interpret()` again, which carries on from the state the interpreter saved in its globals.
A slice ends in `ioReturnToHostIfRequested()`, the last statement of `ioSynchronousCheckForEvents()` (`src/common/sqTicker.c`).
There the interpreter has nothing left on the C stack but returns, so the driver can `longjmp` back to `vm_resume()`; it forces an interrupt check first, so that the next slice runs the rest of the event check.
So the call must stay the last statement of the event check, and the callers of the event check must only return to the interpreter's loop: the lanes that run with 1 ms slices, and a check of Debug builds that every slice starts at the same depth of the C stack (`STACK DRIFT`), guard this.
`vm_resume()` answers BUSY (the slice was over: resume at once), SLEEPING (the image is idle: resume after `vm_wakeup_ms()`, or on input) or EXITED.
When the VM exits, `exit()` flushes the C streams and unwinds out of `vm_resume()` as an `ExitStatus`, which carries the status.
The node VM runs the slices from node's event loop (`packaging/emscripten/node/node-pump.js`), and the pages from their worker (`packaging/emscripten/web/vm-driver.js`).

Entering `interpret()` again is also what makes the VM fast.
V8 runs a function that is entered once in its baseline code (Liftoff), and uses its optimised code only when the function is called again: 20 loops of `inject:into:` over 100000 elements took 1217 ms in one activation, and 585 ms with the same objects re-entered every slice.

### The heartbeat

There is no heartbeat thread either.
Instead, `pollHeartbeat()` (the macro `PHARO_POLL_HEARTBEAT()`) counts down before the stack-limit check of every interpreted method activation, full block activation and backward jump: 14 sites in the generated StackVM, so that even a loop without sends polls.
Every 2000 polls, `ioHeartbeatPoll()` reads the clock, beats every 2 ms as the thread would, and asks for the end of the slice once it is over (`src/common/heartbeat.c`).
The clocks are brought up to date whenever they are read.
When the image is idle, `ioRelinquishProcessorForMicroseconds()` delivers the input that is ready and, unless that woke something up, ends the slice as SLEEPING until the next wakeup.
An idle image still wakes about every 16 ms, as it does natively: the Morphic UI process of the headless image waits on a Delay of at most 16 ms (`MinCycleLapse`) after each cycle.
Slang emits the poll from `StackInterpreter>>pollHeartbeat`, and `include/pharovm/common/sq.h` defines it to nothing on every other platform.
So the VM is built without threads: with `-pthread`, `include/pharovm/emscripten/sqPlatformSpecific.h` stops the build with an error.

### Memory

Linear memory starts at 0 and grows only at the top, so the fixed addresses of the 64-bit memory map cannot be used.
Under `__EMSCRIPTEN__` the Slang memory map puts the stack pages at 64 MiB and new space at 128 MiB.
`src/emscripten/memoryEmscripten.c` puts old space at `WASM_OLD_SPACE_BASE` (512 MiB) instead of the 2^40 that the memory map asks for, and keeps its segments below twice that address, so that the VM still tells young objects from old ones by their address bits.
A segment that would not fit there is refused, and Spur reports an `OutOfMemory`: "Cannot allocate N bytes of old space at P (...): old space must fit in [base, 2*base) (PHARO_WASM_OLD_SPACE_BASE moves it)".
It manages that address space on top of `sbrk()`: the ranges it skips are holes that cost nothing until they are used, freed segments become holes again, and a hole at the top grows with the break.
Everything stays below 4 GiB, so the same layout works with both memories.
The VM has about 710 MB of linear memory once the image has booted, which V8 on Linux commits only as it is touched.

Perm space is not supported: WebAssembly memory cannot reach its address, 2^41.
An image with perm space, or `--minPermSpaceSize`, does not start ("Cannot allocate N bytes of perm space at 0x20000000000: WebAssembly memory cannot reach it"), and the primitives that move objects there (90, 91 and 93) fail with `#'insufficient object memory'`.

### Primitives and plugins

There is no `dlopen`.
The VM core and every plugin are static libraries, and the plugins are built in: FilePlugin, NewFilePlugin, FileAttributesPlugin, FloatArrayPlugin, LargeIntegers, MiscPrimitivePlugin, LocalePlugin, SocketPlugin, BitBltPlugin, B2DPlugin, DSAPrims, JPEGReaderPlugin and JPEGReadWriter2Plugin, and the plugins written for this platform in `src/emscripten/plugins`, such as WebHostPlugin, which Stop uses.
`cmake/plugins.cmake` leaves out UnixOSProcessPlugin (fork) and SurfacePlugin on Emscripten, and the initial cache SqueakSSL and UUIDPlugin; `UUID new` still works, through SocketPlugin.

WebAssembly checks the type of every indirect call.
The interpreter calls every named primitive as `void (*)(void)`, while the plugins define many of them as `sqInt f(void)`, and such a call traps.
So at build time `cmake/emscripten/genPrimitiveTable.cmake` reads the `<X>_exports` table of the interpreter and of every plugin, and writes a translation unit that includes the unchanged source and adds a `void` trampoline for every primitive (every row with an accessor depth), in a `<X>_primitives` table of the same rows; the named-primitive lookup of `src/common/sqNamedPrims.c` answers from those tables.
No source of the interpreter or of a plugin changes, and natively nothing does.
Emscripten's own answer, `-sEMULATE_FUNCTION_POINTER_CASTS`, made a VM without the trampolines 3.3 to 5.5 times slower.
The links use `-sTABLE_BASE=1024`, so that no function pointer has the value of a quick primitive index (up to 519), which shares the primitive function slot.
They also use `-Wl,--fatal-warnings`: wasm-ld links a stub that traps for a direct call whose prototype differs from its definition, and only warns.
Lanes 10 and 50 check all this.

### What else differs

- The Release build defines NDEBUG, as CMake's Release flags do: with the asserts, a 1M-deep recursion took 34 s instead of 1 s.
- The FFI is off, so the image uses its NullFFIBackend, and the support primitive that the image calls at every start to initialise its callbacks does nothing, silently.
- `getaddrinfo` is disabled (`EAI_FAIL`): Emscripten's version fails in wasm64 with a JavaScript TypeError, which ends the VM.
  Name resolution fails instead with an error that the image can catch.
- `aio` sets no `F_SETOWN` or `SIGIO`, and does not `fsync`.
- The microsecond clock has a resolution of 1 ms (`Date.now()`), and `ioHighResClock()` answers monotonic nanoseconds.
- The image attributes 1001, 1002 and 1003 are `unix`, `Emscripten` and `wasm64`, and the platform is `Unix64Platform`.
- The path of the VM is its argv[0] when no file has that name: `/pharo/pharo` in the pages, so that `Smalltalk vm directory` is `/pharo`.

### The web pages

The page compiles `pharo-web.wasm` once, and gives it to every worker it starts (`packaging/emscripten/web/vm-worker.js`).
The worker fetches the files of `manifest.json`, inflates them as they come (DecompressionStream), writes them into `/pharo` in memory, and boots the image with `--headless /pharo/Pharo.image --no-default-preferences st --no-source /pharo/st/web-repl.st`; `--no-source` keeps the REPL, which is filed in at every boot, out of the `.changes`.
What the VM writes to stdout and stderr is posted at the end of every slice, and the worker pauses the VM while more than 1 MiB of it is not acknowledged: the page acknowledges what it rendered, or, while it is hidden and gets no animation frames, what it received.
Stdin is Emscripten's terminal device with its read and poll replaced: a read never blocks, and the REPL waits with `waitForData`, so the VM sleeps until the page sends a line.
Stop calls `vm_interrupt()`, which signals the semaphore that the REPL registered with the WebHostPlugin.
When the image saves, the image file access handler tells the host (`Module.onPharoHost`), and after the slice the worker stores the image and its `.changes` in IndexedDB.
The database belongs to the directory of the site: `pharo-wasm` for a site at the root of its origin, and `pharo-wasm:/dir/` for one in `/dir/`.
The store is guarded by a per-save id, so that a tab never pairs its `.changes` with the image of another tab.

## Tests

`make wasm-check` runs `tests/wasm/run-lanes.sh`, which runs every `tests/wasm/lanes/*.sh` in order; `sh tests/wasm/run-lanes.sh 20 60` runs only the lanes whose names start with 20 or 60.
A lane that cannot run here is skipped, with the reason, and the run ends with the list of the lanes and of the checks that were skipped.
`make wasm-check` gives the lanes `NODE`, `WASM_DIR` (the build directory), `GEN` (the generated sources), `HOST_PHARO` (`WASM_HOST_PHARO`), `SRCDIR` and `TEST_DIR` (`build-wasm/tests-run`).
Run by hand, `run-lanes.sh` defaults them for `build-wasm`, except `HOST_PHARO`, which stays empty unless it is given: the checks that need a native Pharo VM are then skipped.
The lanes are meant for Release builds.
On the machine the port was made on, they passed in under 3 minutes:

| Lane | What it checks | Checks | Time |
|---|---|---|---|
| 10-types | `tests/wasm/check-types.sh` compiles the generated interpreter with `sqVirtualMachine.c`, `client.c` and `sqExternalSemaphores.c` as one translation unit: every interpreterProxy slot and prototype must have the interpreter's type; and both link commands of the CMake tree (Unix Makefiles or Ninja) carry `-Wl,--fatal-warnings` | 0 errors | < 1 s |
| 20-smoke | `tests/wasm/wasm-smoke.sh`, S1 to S21 with the node VM: the exact output of `eval` and `st`, the platform, files, environment, Delays, preemption, deep recursion, a growing heap, a snapshot reloaded in WebAssembly and natively, exit statuses, time zones, the builtin plugins, the slices and where old space is placed | 27 | 23 s |
| 22-old-space | `tests/wasm/st/oldspace-window.st`, `oldspace-regrow.st` and `permspace.st` with the node VM: old space ends in an `OutOfMemory` at the end of its window, and the young objects stored at its top survive a scavenge; a freed segment at the top is used and grown again; the perm-space primitives fail | 3 | 11 s |
| 25-session-id | `tests/wasm/session-id.mjs`: a snapshot booted again within the same second gets a session ID of its own and refuses the old session's file handles; the C streams are flushed on exit | 8 | 2 s |
| 30-engines | S2, S8 and S9 again, in Liftoff code only (`--liftoff-only`) with a 900 KB stack, and in a worker thread with a 1 MB stack, the stack of a browser worker; S2t checks that V8 compiled nothing with TurboFan, and fails when the V8 flags of the lane do not reach node | 8 | 16 s |
| 40-bench | `tests/wasm/bench.sh`: the measures below, next to reference numbers; it fails when 300k-deep recursion takes 1 s, when `eval '3+4'` takes 1.5 s, or when 2 s of computing run in fewer than 10 slices | 8 | 13 s |
| 50-prim-audit | `tests/wasm/prim-audit.mjs` reads `pharo.wasm`: the named-primitive lookup answers the trampoline tables, every primitive is called through a `() -> ()` function, every function pointer is at least 1024, nothing goes through `-sEMULATE_FUNCTION_POINTER_CASTS`, and the builtin modules are the expected ones | every row | < 1 s |
| 56-memory-unit | `tests/wasm/memory-unit.c` drives `src/emscripten/memoryEmscripten.c` as Spur does, against a stub of `pharovm/pharo.h`: fixed spaces, segments that grow again where freed ones were and come back zeroed, old space that never leaves its window, and a first segment larger than the window | 55 + 2 | 1 s |
| 60-vm-harness | `tests/wasm/vm-harness.js`: the web VM and the REPL through `vm-driver.js`, in node, with the default engine and then in Liftoff code only | 32 + 32 | 30 s |
| 62-web-repl-regress | `tests/wasm/web-repl-regress.js`, through `vm-driver.js` as lane 60: a Stop of an evaluation that has not started yet, a Warning that nothing handles, and where a syntax error is | 3 | 3 s |
| 70-worker-harness | `tests/wasm/worker-harness.js`: `vm-worker.js` as staged, in worker threads behind a shim of the worker globals: the protocol, Stop, output credit, the downloads, persistence, two workers on one saved image | 16 | 25 s |

The bench measured, on that machine with node 25.2.1:

| Measure | This build | Native StackVM |
|---|---|---|
| `eval '3+4'`, wall time | 315 ms | 80 ms |
| 300k-deep block recursion | 175 ms | |
| 1M-deep block recursion | 863 ms | 766 ms |
| tinyBenchmarks bytecodes/s | 161.6M | 941.0M |
| tinyBenchmarks sends/s | 10.2M | 48.0M |
| `26 benchFib` | 38 ms | 8 ms |
| GC stress, 3M arrays | 2258 ms | 1460 ms |

The lanes take these settings:

- `WASM_CHECK_TIMEOUT`: the seconds that a run of the VM, or of a unit test, may take (default 120); the harnesses that boot several images stop after 300 or 600 s.
  A run that is stopped then fails its check, or its lane, which says that it timed out.
- `WASM_CHECK_TRIES`: the runs that a check that measures a time may take while that time is over its limit (default 3).
  The best run counts: a loaded machine slows a run down, never speeds one up.
- `WASM_BENCH_RECURSION_MS` (1000), `WASM_BENCH_EVAL_MS` (1500) and `WASM_BENCH_SLICES` (10): the limits of the bench.
  `WASM_BENCH_BYTECODES` (100000000) and `WASM_BENCH_SENDS` (6000000) are those of the tinyBenchmarks warnings, which fail the bench only with `WASM_BENCH_STRICT=1`.
- `WASM_CC`: the compiler of lanes 10 and 56, instead of `emcc -m64` from the PATH or `$EMSDK`; an exported `CC` or `CFLAGS` is ignored.
  Run by hand, `GEN=build-wasm/host/generated/64 sh tests/wasm/check-types.sh` takes the clang or emcc of `CC` (default `emcc -m64`), and exits 0 without errors, 1 for type errors or a failing compiler, and 2 when it cannot check (a `CC` that is not clang, or no generated interpreter in `GEN`).

`run-lanes.sh` empties its `TEST_DIR` only when it is a scratch directory: the default `build-wasm/tests-run`, an empty directory, or one that it used before.
One run at a time uses a `TEST_DIR`: a second `make wasm-check` of the same build waits for the first one.
The runner exits 2 when it may not use its `TEST_DIR`, or cannot make its lock there.

`tests/wasm/page.spec.mjs` drives the Console in real browsers through Playwright, which is not a build dependency.
`PLAYWRIGHT_MODULE` names the Playwright package, and `BROWSERS` the browsers (default `chromium`); Playwright's own variables, such as `PLAYWRIGHT_BROWSERS_PATH`, apply:

    PLAYWRIGHT_MODULE=/path/to/node_modules/playwright BROWSERS=chromium,firefox make wasm-check-browser

or for one spec, on any staged site:

    PLAYWRIGHT_MODULE=/path/to/node_modules/playwright BROWSERS=chromium,firefox node tests/wasm/page.spec.mjs build-wasm/web

The Console spec has 32 checks per browser, and passed in Chromium 153 and Firefox 155 in about 80 s.
`tests/wasm/pages-regress.spec.mjs` holds regression checks of the pages, and runs the same way; `make wasm-check-browser` does not run it.
`SHOTS=1` saves screenshots of the pages, light and dark, at 1280 and 360 px, in `build-wasm/tests-run/shots`.

## Limitations

- StackVM only: about 5 times slower than the native StackVM, and 13 times (bytecodes) to 25 times (sends) slower than the native JIT on tinyBenchmarks; on the machine the port was made on, the JIT ran 2181M bytecodes/s and 260M sends/s.
- No FFI: the image has its NullFFIBackend, so `Smalltalk os environment at:put:`, FreeType, Cairo and libgit2 (Iceberg) are unavailable.
- No `dlopen`, and no external plugins: every plugin is built in.
- No name resolution (`getaddrinfo` is disabled), and sockets are of little use in browsers.
- No `fork`, OSProcess, SSL, threads or TFWorker, and no external surfaces (SurfacePlugin is left out).
- Old space is at most 512 MiB by default, and perm space is not supported (see `WASM_OLD_SPACE_BASE`).
  Spur asks for more old space than an object needs (512 MiB for a 256 MB ByteArray), so a single object of 256 MB already fails with the default base, in both VMs.
  The VM has about 710 MB of linear memory once booted.
- The clock has a resolution of 1 ms, and browsers clamp nested timers, which wake a sleeping VM, to 4 ms.
- A long primitive (a huge `LargePositiveInteger` operation, say) does not yield: Stop then replaces the worker after 3 s, losing what was not saved.
- Stop ends the evaluation, not the processes that it forked: one that never waits keeps the VM busy until Restart.
- One image is saved per browser and site directory, shared by all its tabs and pages; the last save wins.
- Safari is untested.

## Troubleshooting

- `... lacks the Emscripten hooks of the Slang sources ... it was generated by a VMMaker image older than this tree`: the sources given (`WASM_GENERATED`, or `GENERATED_SOURCE_DIR` on the CMake-only route) predate this tree's `smalltalksrc`.
  Run `make wasm` without `WASM_GENERATED`, which refreshes the VMMaker image and generates the sources again.
  Sources generated by an older version of this tree may have the hooks but not every later change: lane 25-session-id, for one, fails on sources generated before the session-ID fix.
- `emcmake (...) not found: source emsdk_env.sh of the Emscripten SDK ..., or set EMCMAKE`: the shell has not sourced `emsdk_env.sh`.
- `CMake Error: The source directory ".../cmake/Emscripten.cache.cmake" is a file, not a directory.`, or `The source directory ".../host" does not exist.`: the CMake is older than 3.13, which `-S` and `-B` need.
- `download.cmake: cannot download ...`, or a VMMaker bootstrap that cannot reach the network: build offline (see Building).
- `WASM_BUILDDIR (...) is not empty, and make wasm never built there`, and the other refusals of `WASM_BUILDDIR`: name a new or empty directory, or create `<dir>/.make-wasm` to build in a directory anyway.
  `WASM_BUILDDIR is empty`: leave the setting out for `build-wasm`.
  `WASM_BUILDDIR (...) holds a blank`, or `... is relative to the current directory, ..., whose path holds a blank`: name a path without blanks, or an absolute one.
- `make wasm cannot find its source tree (...): the path of the tree must hold no blank`: move the source tree to a path without blanks.
- `internal error: invalid --jobserver-auth string 'fifo:...'` from `make -jN wasm`: that make is not the one that CMake chose for the trees (their `CMAKE_MAKE_PROGRAM`), and the two do not share job slots in the same way (make 4.4 over trees configured with make 4.3, say); use that make, or run without `-j`.
- The page says that it must be served over HTTP: it was opened from a `file://` URL.
  Run `make wasm-serve`, or serve `build-wasm/web` with any static web server.
- The Console says that the browser has no 64-bit WebAssembly memory: the build has `WASM_WEB_MEMORY64=1`.
  Build with the default, `2`.
- The page says that the browser cannot inflate the files of Pharo: the browser has no DecompressionStream (see the engines above).
- Firefox warns that the WebAssembly `try` instruction is deprecated: the build has `WASM_SJLJ=wasm-legacy`.
- With `WASM_DEBUG=1`, the time checks of the lanes (S6b, S8 and the bench) and the checks of an empty stderr (S2, S13) fail: the VM is many times slower with its asserts (the lanes take over 20 minutes), and Emscripten's assertions print "Heap resize call ..." and "program exited (with status: 0), but keepRuntimeAlive() is set ..." when the image quits.
- `run-lanes: waiting for the run N ...`: another run uses that `TEST_DIR`; if it is gone, remove the lock that the message names.
- An `OutOfMemory` for a large object, and the VM logs `Cannot allocate N bytes of old space at P ...: old space must fit in [base, 2*base)`: raise the old-space base, for example with `PHARO_WASM_OLD_SPACE_BASE=0x40000000` (1 GiB of old space).
- `Cannot allocate N bytes of perm space at 0x20000000000: WebAssembly memory cannot reach it (perm space is not supported)`: the image has perm space, or the VM was given `--minPermSpaceSize`; neither works in WebAssembly.
- A crash prints at most 1000 frames per process: pass `--maxFramesToLog=<n>` for more, or `--maxFramesToLog=0` for all of them.
