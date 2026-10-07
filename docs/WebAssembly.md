# Building for WebAssembly (Emscripten)

The VM can be cross-built to WebAssembly with Emscripten.
The build runs from the root of this repository, out of tree, by default in `build-wasm/`.
It builds the StackVM, the interpreter (a JIT needs executable memory), for 64-bit images, with the FFI on libffi but no threads, and runs the stock Pharo 12 image.
Pharo 15 images run too, in a site built with one (`WASM_IMAGE_ZIP`) or opened from disk in the pages: the Smalltalk of the pages works with both (see Pharo versions).
Two VMs are linked from the same objects:

- a command-line VM for node, with access to the host file system, environment and exit status;
- a VM for a Web Worker, which a static web site runs: the Console, a Smalltalk REPL with Stop, Save, Download and Open, which starts an image of your own, and its Notebook tab, cells of Smalltalk, which a second VM of their own runs, and of Markdown, which the page renders.

The site also has the world page: the Morphic world of the image in a canvas, drawn through the WebDisplayPlugin.
Both VMs link the libraries that the image calls through its FFI, built from pinned archives: FreeType 2.14.3 for the fonts of the world, and cairo 1.18.4 for Athens and Roassal, by default; libgit2 1.4.4 for Iceberg (`WASM_LIBGIT2=ON`) and SDL2 2.32.10 for the image's own SDL2 driver, with the page `sdl.html` (`WASM_SDL2=ON`), on request.

Requirements:

- Emscripten 6.0.10 (emsdk).
  The port needs that version, and was tested only with it: the web driver replaces the stream operations of Emscripten's terminal device, which belong to the internals of its file system.
  Nothing checks the version: with another one, run `make wasm-check`, whose lanes 60 and 70 drive those operations in node.
- GNU make 3.81 or later (tested with 3.81, 4.2.1, 4.3 and 4.4.1) and CMake 3.13 or later (tested with 3.13.5 and 3.28.3).
- node, which stages the site, runs the tests and serves the pages (tested with 25.2.1).
- A native C compiler: the sources are generated in a native CMake tree, `build-wasm/host`.
- Network access for the first build (files.pharo.org and GitHub, and the sites of the archives of the libraries: github.com, download.savannah.gnu.org, www.cairographics.org and download.sourceforge.net), unless the offline settings below give its inputs.
- A source tree whose path holds no blank.

Engines, as Emscripten's feature matrix gives them for the default settings:

- The node VM uses a 64-bit memory (memory64) and the exnref instructions of WebAssembly exception handling, for setjmp and longjmp.
  It needs node 24.15 or later.
- The web VM has a 32-bit memory with 64-bit pointers (`WASM_WEB_MEMORY64=2`), so it needs only exnref: Chrome and Edge 137, Firefox 131 or Safari 18.4, or later.
  `WASM_SJLJ=wasm-legacy` builds it for engines without exnref (Chrome 95, Firefox 100, Safari 15.2).
- The pages inflate what they download with DecompressionStream (Chrome 80, Firefox 113, Safari 16.4), and say so in a browser without it, so a `wasm-legacy` build needs Chrome 95, Firefox 113 or Safari 16.4, or later.
- The pages were tested in Chromium 153 and Firefox 155.
  Safari is untested.
- The world page, and `sdl.html`, also need OffscreenCanvas: Chrome 69, Firefox 105, Safari 17.

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
The build downloads the pinned Pharo 12 image (`Pharo12.0-SNAPSHOT.build.1599.sha.7d5f14cb47.arch.64bit.zip`, checked against its SHA256) and the pinned archives of the libraries (see The libraries), and stages `build-wasm/node` and `build-wasm/web`.

The first `make wasm` took about 2.5 minutes from a clean clone with the downloads, and about 1.5 minutes offline, on the machine the port was made on: most of that time goes to the VMMaker image, which loads its packages and generates the sources in a single process.
A new build directory with the sources generated already (`WASM_GENERATED`) and the archives of the libraries at hand (`WASM_DEPS_DIR`), with libgit2 and SDL2, took 57 s there, on 112 processors.
When nothing has changed, it takes under a second.

To build offline, give the inputs that the first build would download: a VMMaker image, the Pharo VM that runs it, the image zip (from https://files.pharo.org/image/120/), and a directory holding the archives of the libraries.
With a native build of this repository in `build/` that generated its sources, on Linux, and the zip and the archives in the source tree:

    make wasm WASM_VMMAKER_IMAGE=build/build/vmmaker/image/VMMaker.image \
              WASM_VMMAKER_VM=build/build/vmmaker/vm/pharo \
              WASM_IMAGE_ZIP=Pharo12.0-SNAPSHOT.build.1599.sha.7d5f14cb47.arch.64bit.zip \
              WASM_DEPS_DIR=deps-archives

An earlier `make wasm` has the same inputs in `build-wasm/host/build/vmmaker/` and `build-wasm/downloads/`.
`WASM_GENERATED` instead takes sources generated already (see Settings).

The site must be served over HTTP: browsers run no workers from `file://` URLs.
It needs no special headers (no COOP or COEP, since nothing uses SharedArrayBuffer), so any static web server will do.

## This produces

- `build-wasm/node/pharo`: the command-line VM, a shell script that runs `pharo.js` (and `pharo.wasm`) next to it with node.
  `NODE` selects the node binary, and `NODE_OPTIONS_WASM` gives it options.
  Next to it are `THIRD-PARTY-NOTICES.txt` and the empty placeholders of the libraries of the FFI (`libcairo.so.2`, and `libgit2.so.1.4.4` and `libSDL2-2.0.so.0` with those libraries; see The FFI).
- `build-wasm/web/`: the static site.
  It holds the Console page (`index.html`), the web VM (`pharo-web.js`, the factory `createPharoVM`, and `pharo-web.wasm`), the scripts of the pages and of the workers (see The web pages), the image, its `.changes` and the `.sources`, gzipped, in `image/`, the Smalltalk scripts of the pages in `st/`, `THIRD-PARTY-NOTICES.txt`, which the three pages link to, and `manifest.json`, which lists the files the worker loads and says what the build has (`ffi`, `fonts`, `git`, `gitHttp`, `sdl2`, `world`, `libraries`, `webPackage`).
  The pages load every file with the build id in its URL (`?v=<id>`), so a new build is never mixed with a cached one.
  The browser downloads about 25 MB: an image of about 55 MB (15.4 MB gzipped), the 43 MB `.sources` (7.6 MB gzipped), and `pharo-web.wasm`, 2.1 MB in the default build (743 KB gzipped).
- `build-wasm/web/world.html`: the world page.
  With the world (`WASM_WORLD=ON`, the default, and a native Pharo VM, `WASM_HOST_PHARO`), `web/` holds the world image, which is the stock image with the OSWindow-Web package, prepared at build time; both pages boot it.
  Without it, `web/` holds the stock image, and the world page says that the build has no world image.
- `build-wasm/web/sdl.html`, with `WASM_SDL2=ON` only: the world through the image's own SDL2 driver (see The world through SDL2).
- `build-wasm/image/stock/`: the unpacked stock image (as `Pharo.image` and `Pharo.changes`, with its `.sources`); `build-wasm/image/stock.stamp` records the SHA256 of the zip it came from.
- `build-wasm/image/web/`: `Pharo-web.image` and `.changes`, the world image, and `OSWindow-Web.st`, the package filed out for `web-bootstrap.st`.
- `build-wasm/host/`: the native tree that generates the sources, in `host/generated/64/{vm,plugins}`, and its VMMaker image (in `host/build/vmmaker/image`, or in `host/vmmaker-image` for a copy of `WASM_VMMAKER_IMAGE`).
- `build-wasm/cmake/`: the CMake tree of the WebAssembly build, with the unpacked archives of the libraries in `cmake/deps/<name>` and `THIRD-PARTY-NOTICES.txt` in `cmake/wasm/`.
- `build-wasm/downloads/` and `build-wasm/tests-run/`: the downloaded image zip and archives of the libraries, and the scratch directory of `make wasm-check`.
- `build-wasm/config.make`, `build-wasm/config-host.make` and `build-wasm/.make-wasm`: the recorded settings, and the mark of a directory that `make wasm` builds in.

The workflow `.github/workflows/build.yml` makes such a site and node VM on every push, with the Pharo 15 image, `WASM_LIBGIT2=ON` and `WASM_SDL2=ON`, so they have the FFI, FreeType, cairo, libgit2, SDL2 (`sdl.html`) and the Notebook.
It zips them as `PharoVM-<version>-WebAssembly-bin.zip`, with `serve.mjs` and the `README.md` and `node-try.sh` of `packaging/emscripten/dist` (the README says how to use them), and publishes the zip with the release of the push.
For a push to `pharo-12-wasm` it also deploys `web/` to the root of branch `gh-pages`, which GitHub Pages serves.

The node VM takes the usual arguments of the VM, for example on a copy of the stock image:

    mkdir -p build-wasm/try && cp build-wasm/image/stock/* build-wasm/try/
    build-wasm/node/pharo --headless build-wasm/try/Pharo.image --no-default-preferences eval '3 + 4'
    build-wasm/node/pharo --headless build-wasm/try/Pharo.image --no-default-preferences st --quit script.st

## The Console page

`index.html` runs the REPL of `st/web-repl.st` in the image, in a Web Worker.
It has two tabs, Console and Notebook (see The Notebook), and opens the one last used.
Each line typed is evaluated, and its value printed, or the error it raised, with up to five frames.
A syntax error says where it is, as in `Error: CodeError Undeclared variable x (line 1, column 1)` (`OCCodeError` in Pharo 15).
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
- Open, or a drop of files on the page, starts an image of your own: a Pharo `.zip` as files.pharo.org gives them, even with the files in a directory, or an `.image` with its `.changes`, and its `.sources` when it has one of its own.
  The page unpacks the zip in the browser, with its progress, while the VM goes on, and checks the size and CRC-32 of every entry and that the image is a 64-bit Spur image.
  It refuses anything else, and says why (no image or several, not a whole zip, a damaged or encrypted entry, a method other than deflate, a split zip, a 32-bit image), and the session goes on as it was.
  The `.changes` and the `.sources` are those of the image's base name, or else the only ones of their kind.
  The image gets its own `.sources` when it has one, and the site's otherwise.
  It replaces the saved image only once it has started: one that crashes or quits before leaves the saved image as it was.
  The saved image then keeps its own `.sources` too, and the page asks before it replaces a saved image.
- Reset deletes the saved image, and starts the original one.
  When the saved image itself does not start, the notice offers Reset next to Restart.
- Restart starts the VM again from the saved image.
  Ctrl+D on an empty line ends the session (after asking), and Ctrl+L clears the terminal.
- When two tabs share the saved image, the last one to save wins.
  The other tab says that its image was replaced ("Another tab has replaced the image saved in this browser"), and stops storing its `.changes` until it saves again.
- Settings, shown when the VM has the smart-HTTP transport of libgit2 (`WASM_LIBGIT2=ON`), holds the CORS proxy of git's requests (see libgit2).
- For screen readers, a live region next to the status pill says how the VM goes: loading, starting, ready, and how it ended.

While the Notebook tab shows, Esc, Ctrl+C and Ctrl+L do nothing to the Console, and a drop of files goes to the Notebook.

On a site built with the world, the Console links to the world page when the image can open it, and offers to save first when something was evaluated since the last save.
An image that cannot yet (a stock image opened from disk, or one saved with an older version of OSWindow-Web) gets the offer "Prepare for the world", which files in the OSWindow-Web package (`CodeImporter evaluateFileNamed: '/pharo/st/web-bootstrap.st'`) and saves.

## The Notebook

The Notebook tab of the Console page holds cells of Smalltalk, which a second VM evaluates one at a time, and of Markdown, which the page renders (`notebook-lib.js`); the VM is the kernel, `st/web-notebook.st`, in a worker of its own, driven by `nb-kernel.js`.
The first visit to the tab starts it.
It boots the image saved in this browser, read-only, or else the site's, and never saves: a snapshot in a cell ends ok and leaves the saved image as it was.
When the Console saves, resets or replaces its image, a notice offers to Restart the kernel on it.
In a page opened from `file://`, or in a browser without the memory of the build, the tab starts no worker and says why.

- A code cell is parsed whole before anything runs: a syntax error runs nothing, and says its line and column.
  Then its statements are compiled and run one at a time, as one DoIt, so that a class can be defined and used in the same cell.
  The value of the last statement is shown, as its printString (cut at 64 K characters, and shown 30 lines high until it is opened); `^` ends the cell with its value.
- A variable that a cell assigns, and nobody declared, is a variable of the notebook (a WorkspaceVariable), which the next cells see; the temporaries of a cell are its own.
  `Notebook variables` answers their names, and `Notebook forgetVariables` forgets them.
- An error says its text, the statement, line and column of the cell, and the chain of frames, outermost first; `Notebook lastError` answers it.
- Shift+Enter runs a cell and selects the next one, Ctrl+Enter runs it in place, and Alt+Enter runs it and inserts a cell below.
  Esc is command mode, where `?` lists every shortcut: a and b insert a cell, d d deletes it (z undoes it), m and y make it text or code, Alt+Up and Down move it, o hides its output, i i interrupts and 0 0 restarts the kernel.
  Cells run in their order of request, each once the cell before is done, and their counts say in which order they ran.
- Run all runs every code cell from the top, and stops at the first one that does not end ok: an error, a syntax error or a Stop cancels the cells queued after it (a cell whose process terminates itself does not).
  The example notebook (More, Load example), 13 cells, stops at its error on purpose.

The class `Notebook` shows rich output in the cell of the process that calls it:

- `Notebook show: anObject` shows a display, a String as text, a Form, a Morph, a Roassal canvas or shape, or an AthensCairoSurface as an image, and anything else as its printString; `show:id:` replaces a display shown with the same id in the same cell, as a progress bar does.
- `Notebook text:`, `html:`, `svg:`, `markdown:`, `table:` and `table:header:` make displays; a table's Strings, numbers and Characters are escaped.
- `Notebook image:` makes the display of what an object looks like, as the raw pixels of a Form, which the page turns into a PNG (the VM has no ZipPlugin to encode one); `Notebook png:` takes the bytes of a PNG.
- `Notebook clearOutput` clears the output of the cell so far.
- A cell whose value is a Form, a Morph, a Roassal canvas or shape, or an Athens cairo surface shows it as an image.

Output goes to the cell that produced it:

- What a cell writes on the Transcript, stdout and stderr lies between its start and its end: the kernel flushes them before each of its events, which are JSON lines on an Emscripten character device of their own, `/dev/nbevents`, with binary attachments.
- What a process that an earlier cell forked shows on the Transcript, or displays, goes to that cell, while it shows that run; an error that nothing handles in such a process is reported to that cell, and the process is terminated.
- While a cell runs, its output keeps the first and the last 512 KB of its streams and its first and last 500 outputs, with a note of what was left out; a display over 4 MB comes with its size only, and an image has at most 16 MiB of pixel data (4 Mi pixels of 32 bits) and 16384 pixels a side.

Stop (the button, i i, or Ctrl+C with nothing selected anywhere on the tab) signals the semaphore that the kernel registered with the WebHostPlugin: the cell that runs is terminated, and its ensure: blocks run, or the cell about to start does not run; the cells queued after it are cancelled.
A Stop that comes while no cell runs does nothing.
After a Stop, the page's watchdog gives the cell 3 s to end, more while it writes output, up to 10 s; after that the worker is terminated, the cell ends "killed", and a fresh kernel starts, without the variables.
Stop ends neither the processes that cells forked nor the system's: More, Stop background processes, terminates those that cells forked at the priority of the cells or below, but not the processes of the system that a cell made start (the Morphic UI process, the event loops of the drivers).
Restart starts a new kernel: the counts start at 1 again, and what ran before is marked stale; More, Restart & run all, then runs every cell.
More also has Add text cell, Clear all outputs (of the cells not running or queued), New notebook (which asks before it replaces one that has content), and Theme, which goes from the system's to light and dark, for the whole page, as the Console's button does.

The notebook is saved in this browser's localStorage, 0.6 s after a change and when the page is hidden or goes, under a key of the site's directory (`pharo-wasm.notebook:<dir>`), with the outputs of each cell capped at 64 KB (and without outputs when the whole would pass 2 MB); Ctrl+S saves at once.
When another tab of the site saves its notebook, a notice offers "Load theirs" or "Keep mine".
More, Export, gives it as `.st` (the chunk format that a fileIn reads, each cell a chunk marked by a `"%%"` comment, a Markdown cell's text in the comment), as `.json` (with the outputs) or as `.html` (below); Import takes a `.st` or a `.json` of at most 5 MB, 5000 cells and 1000 outputs per cell, and asks before it replaces a notebook that has content.
A `.st` file of another kind is read as Pharo's chunk reader reads it: its DoIts are the cells, and its method definitions and class comments are left out, with a warning.
More, Upload files, or a drop of files on the tab, writes them into the kernel's `/pharo`, now and into every new kernel of the page (the page keeps them in its memory, not across a reload): `'name.st' asFileReference fileIn` then files one in.

The page never inserts markup as given.
The HTML and SVG of `html:` and `svg:`, and those of imported notebooks, are parsed inertly (DOMParser) and rebuilt element by element from an allowlist: the ids and the `url(#...)` references are rewritten, the styles filtered, links kept only to http, https, mailto and the fragments, images only as data: URLs of PNG, JPEG, GIF or WebP, filters charged against a budget of their painting cost, HTML elements inside SVG dropped, and markup nested deeper than the browsers' parsers take in linear time refused.
Markdown, of the text cells and of `markdown:`, is built from elements and text nodes by a renderer of the page in time linear in its source.
The Content-Security-Policy of `index.html` is a second line of defence behind them: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'`.
The kernel is not a sandbox: a cell can call it and write events of its own, which the page checks like any other.

Code cells are colored as they are typed, by the highlighter of `notebook-st.js`, which reads as Pharo's scanner does, in linear time, on any text, unterminated too.
Its kinds are comment, string, character, number (radixes, exponents, scaled decimals), quote (symbols, and literal and byte arrays and what is inside them), constant (`nil`, `true`, `false`), special (`self`, `super`, `thisContext`, `^`, `:=`), keyword, paren (brackets, period, cascade, the bars of temporaries, the `<>` of pragmas) and global:

| Kind | Light | Dark |
|---|---|---|
| comment | `#565d66` | `#9aa3ae` |
| string, character | `#336316` | `#a8cc7a` |
| number | `#962a5a` | `#f28db2` |
| quote | `#255e86` | `#82b8e8` |
| constant | `#7239ad` | `#c4a5ff` |
| special | `#984208` | `#f5a524` |
| keyword | `#0b645c` | `#5ccfbf` |
| paren | `#49515a` | `#a3abb5` |
| global | `#3445b4` | `#a3adff` |

Each color has a contrast of at least 4.5:1 on the editor, on the selection tint and on the line of an error: the lowest measured is 4.79:1, of a comment on the line of an error in the light theme.
The highlighter agrees with the image's own scanner on every token of every method of the images: 3,675,607 tokens of 138,191 methods on Pharo 12 (RBScanner), and 3,698,571 of 140,181 on Pharo 15 (OCScanner), in about 2 s each; lane 61 checks 5000 methods by default (`NB_SCAN_METHODS`).
The colors are an inert overlay behind the editor's textarea, in its box and font, painted when a cell comes near the visible part of the notebook and repainted line by line on an edit, for cells of up to 32 KB; an input method composes in the textarea.
Markdown cells are not colored, but their fenced code is when the fence names no language, or `st`, `smalltalk` or `pharo`, up to 32 KB per render.
A cell of 32 KB (885 lines) painted first in 32 ms (Chromium) and 37 ms (Firefox), and an edit repainted it in 16 ms and 13 ms on average; the slowest of the adversarial 500 KB inputs of the spec took 157 ms and 139 ms.

More, Export .html, downloads one page of what the notebook shows: the Markdown cells rendered (also those being edited, their Smalltalk fences colored), the code cells with their count, status bar and colored source (also cells not yet scrolled into view, up to 32 KB a cell and 1 MB in all, the rest plain), and their outputs as shown, without buttons: streams, values in full, errors with their bar and call history, displays and images as data: URLs, and hidden outputs in a closed `<details>`.
Its title is the first heading, else the notebook's.
It has no script and loads nothing: its Content-Security-Policy allows inline style and data: images and fonts only.
It is built in a document of its own from trees that the page already trusts, serialized and read back before it is saved: an output whose markup would parse into another tree is left out with a note.
It follows the light or dark preference, prints light, and fits 320 px.
The example notebook, run, exported in 146 ms (Chromium) and 199 ms (Firefox) to 28 KB; 40 cells of 31 KB each, 1.2 MB of sources, in 1.8 s and 1.9 s to 10 MB.

The kernel costs a second VM: on the machine the port was made on it was ready 1.9 s (Chromium) and 2.0 s (Firefox) after the first visit to the tab, about 0.9 s after the Console's prompt (1.0 s and 1.1 s after the page was opened); it reads 19.8 MB of the site's 42.8 MB `.sources` at every boot, which the REPL does not.
In node, the kernel booted in 1.9 s against 0.8 s for the REPL on Pharo 12, and in 2.2 s against 1.0 s on Pharo 15 (27.7 MB of its 46.1 MB `.sources` read).
Its worker has a linear memory of its own, as large as the Console's: 675 MiB once Pharo 12 has booted, 683 MiB for Pharo 15.
It fetches the `.sources` once per boot, and the image only when this browser has no saved image.

## The world page

`world.html` boots the saved image, or the world image of the build, with `--interactive`, and the world draws itself in the canvas.
The canvas follows the size of the page, and the world has its size in CSS pixels.
On a high-density display the world draws in device pixels: the page reports the canvas in device pixels too, from which Pharo's world renderer takes `devicePixelRatio` for its `canvasScaleFactor`, as with SDL on a Retina display, and draws a Form that much larger, which the page shows one pixel per device pixel; it follows the ratio when the window moves to another screen, or with the zoom of the page.
The world's text is drawn with FreeType, in the stock fonts of the image (Source Sans Pro and Source Code Pro): text beyond Latin-1, emphasis such as the strike-out of deprecated classes, and exact font sizes are drawn as natively.
With cairo, Athens draws too: Roassal, the Canvas view of the Inspector, Epicea's Code Changes (whose graph Hiedra draws) and the color picker of the Settings.

- The mouse buttons, the wheel and the keyboard reach the world as the events of an SDL2 window would.
  Text typed with Shift, Option (macOS) or AltGr is text.
  On macOS Cmd is sent as Ctrl, Pharo's shortcut key.
  On Windows, Ctrl+Alt types the AltGr character of the key, when the key has one; elsewhere, Ctrl+Alt is a shortcut.
- What the world copies (Ctrl/Cmd+C or X) goes to the system clipboard, and Ctrl/Cmd+V pastes the system clipboard, through the browser's paste event.
  When the browser refuses a copy to the system clipboard, Ctrl/Cmd+V pastes the world's own copy instead, until the page loses the focus to another window or something else on the page is copied.
- The browser keeps some keys for itself, such as Ctrl/Cmd+W, T and N.
  Pharo's Ctrl+O, Ctrl+W would close the tab, so the page asks before it goes while the world has taken input since the last save.
- F6 (or Shift+F6) moves the keyboard from the world to the toolbar, and Tab goes back into the world.
- Stop (or Alt+. in the world) interrupts the busy process and opens a debugger on it.
  That is the UI process whenever it is ready to run, so that a loop evaluated in a Playground is interrupted even when another process of its priority is ready too: UserInterruptHandler alone may pick that one, and then no debugger opens, since the busy UI process would draw it.
  When the worker then says nothing for 3 s, it is replaced, as in the Console.
- Save asks the image to save itself, and the worker keeps it as the Console does: the next visit, here or in the Console, boots it.
  Download gives the image and its `.changes`.
- The canvas shows one OSWindow at a time: the newest one that has an event handler, such as the window of the Emergency Debugger of Pharo 12, takes the canvas and the input, and the world comes back when it closes.
- The status pill says Busy while the VM has not slept for a second, or while it does not answer at all (a long primitive).
- Open, or a drop of files on the page, starts an image of your own, as in the Console.
  An image without the OSWindow-Web package, a stock image say, or with an older version of it (the site has version 5), is prepared first: the worker boots it with the REPL, files in the package and saves, the pill says Preparing meanwhile, and then the world boots from the saved image.
  The stock image of the build was unpacked, prepared and painted in 4.3 s (Chromium) and 5.0 s (Firefox) after it was chosen.
- The Console button opens the Console page.
  When the world has not drawn 30 s after the VM started, or the VM ended before it drew, the notice points to the Console, which can run the image or reset it.

The first frame came 1.4 s (Chromium) to 2.0 s (Firefox) after the page was first opened on localhost, nothing cached, and a key took 40 to 57 ms (median) to reach the canvas.

## The world through SDL2

A build with `WASM_SDL2=ON` also has `sdl.html`, which no page links to: there the image draws its world as the desktop VMs do, through its own OSSDL2Driver and SDL2, rather than through OSWebDriver and the WebDisplayPlugin.
Any image runs there, a stock image of Pharo 12 or Pharo 15 too, unprepared, and so does code that uses the SDL2 bindings of OSWindow.
The worker boots the image with `--interactive` and no display, and SDL's Emscripten video driver draws into the canvas of the page, transferred to the worker as an OffscreenCanvas.
That driver expects the DOM of the main thread, which a worker lacks: `sdl-shim.js` gives it the few objects it touches (`screen`, the canvas's box, style and events, a `document` that is an EventTarget, `navigator.userActivation`), and the page forwards its pointer, wheel and keyboard events to the worker, where SDL's handlers queue them for the image.
No threads, COOP or COEP are needed.
SurfacePlugin is built into these VMs, since an SDL world blits its Form through it; the stock image's Athens then draws through it too.
Nothing is kept in this browser: Download gives what the image saved, and Restart boots the same image again.
The clipboard, resizing, HiDPI and input methods are SDL's, not bridged to the page.

It costs 156 KiB of gzipped wasm and 10 KiB of JavaScript, and, idle, about 3.5 (Chromium) to 4 to 6 (Firefox, which varies from run to run) times the VM time of the world page, and 186 slices a second against 61, since the image polls `SDL_PollEvent` every 5 ms and every present uploads the whole window.
The first present came 1.3 s (Chromium) to 1.4 s (Firefox) after the worker started.
The world page stays the default.

## Other targets

`make wasm-check-browser` runs the Playwright specs of the pages (see Tests).
Without Playwright (`PLAYWRIGHT_MODULE`, or a `playwright` package that node finds from `tests/wasm/lib`) it fails, and says so.
It skips, and says why, `tests/wasm/world.spec.mjs` on a build without the world image, `ffi.spec.mjs` without the FFI, `git.spec.mjs` without libgit2, and `sdl.spec.mjs` without SDL2: `web/manifest.json` says what the build has.
`make wasm-clean` removes the WebAssembly build (`build-wasm/cmake`, `node`, `web` and `tests-run`), and keeps the generated sources, the images and the downloads, the image zip and the archives of the libraries.
`make wasm-distclean` also removes `build-wasm/host`, `build-wasm/image` and the recorded settings, and, with `WASM_CLEAN_DOWNLOADS=1`, `build-wasm/downloads`.

GNU make reads `GNUmakefile` before `Makefile`.
In an in-source CMake build, the default goal and every goal that `GNUmakefile` does not define go to that build's `Makefile`, as before.

The WebAssembly tree can also be configured with CMake alone, from sources generated already:

    emcmake cmake -C cmake/Emscripten.cache.cmake -S . -B build-wasm-cmake -DGENERATED_SOURCE_DIR=$PWD/build-wasm/host
    cmake --build build-wasm-cmake

It stages `node/` and `web/` in its build directory, or in `WASM_STAGE_DIR`, and downloads the archives of the libraries into `downloads/` there.
Its cache variables are the settings `WASM_SJLJ`, `WASM_WEB_MEMORY64`, `WASM_STACK_SIZE`, `WASM_INITIAL_MEMORY`, `WASM_MAXIMUM_MEMORY`, `WASM_OLD_SPACE_BASE`, `WASM_SLICE_MS`, `WASM_WORLD`, `WASM_FFI`, `WASM_FREETYPE`, `WASM_CAIRO`, `WASM_CAIRO_PDF`, `WASM_LIBGIT2`, `WASM_LIBGIT2_HTTP`, `WASM_SDL2`, `WASM_IMAGE_ZIP`, `WASM_DEPS_DIR` and `WASM_HOST_PHARO` below, `CMAKE_BUILD_TYPE` (`Debug` for `WASM_DEBUG=1`), `WASM_STAGE_DIR`, and `NODE_JS_EXECUTABLE`, the node that stages `web/`.
`WASM_HOST_PHARO` has no default there, so the world image is prepared only when it is given.
Two more are options for developers of the FFI, which `make wasm` does not give: `WASM_FFI_LIBC_ALL=ON` lists every function of the C library in the registry, not only those the images call (about 100 KB more of gzipped wasm), and `WASM_FFI_TEST_LIBRARY=ON` links the library of the FFI tests into the web VM too, as it always is into the node VM.

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
  The libraries keep their optimisation, with `-g`.
  The VM is many times slower with its asserts, so the time limits of the lanes fail by design (see Troubleshooting).
- `WASM_SJLJ`: how the VM's setjmp and longjmp are compiled (default `wasm`).
  `wasm` uses WebAssembly exception handling with exnref, `wasm-legacy` its legacy instructions (try, delegate and rethrow, for engines without exnref; Firefox logs them as deprecated), and `emscripten` calls out to JavaScript.
  The libraries are compiled in the same mode, since every object of a link must share it.
- `WASM_WEB_MEMORY64`: the memory of the web VM (default `2`).
  `2` is a 32-bit memory with 64-bit pointers, which is 15 to 18% faster and runs in engines without memory64; `1` is memory64, as in the node VM, and the Console says so in a browser without it.
  The node VM always has memory64, so that a stray pointer traps rather than wrapping around.
- `WASM_STACK_SIZE`: the C stack (default `8MB`); FreeType's autofitter needs more than Emscripten's default of 64 KB.
- `WASM_INITIAL_MEMORY`, `WASM_MAXIMUM_MEMORY`: the linear memory, its initial size (default `32MB`) and its maximum (default `4GB`); it grows between them.
- `WASM_OLD_SPACE_BASE`: where old space begins (default `0x20000000`, 512 MiB), a power of two above new space.
  Old space stays below twice that address, so it can grow to that size, 512 MiB by default, and twice it must fit in `WASM_MAXIMUM_MEMORY`.
  Past that window an allocation fails with an `OutOfMemory`, and an image that does not fit in it does not start.
  The environment variable `PHARO_WASM_OLD_SPACE_BASE` overrides it when the VM starts, for example `0x40000000` for 1 GiB.
  Changing the setting recompiles the VM core.
- `WASM_SLICE_MS`: the length of a slice of the VM in milliseconds (default `20`); `PHARO_WASM_SLICE_MS` overrides it when the VM starts.
- `WASM_WORLD`: `ON` (the default) prepares the world image with `WASM_HOST_PHARO`, on a build that has the world, and stages it in `web/`; `OFF` stages the stock image.
- `WASM_FFI`: `ON` (the default) builds the FFI, libffi and the registry of the libraries; `OFF` builds the VMs without any of them, on the image's NullFFIBackend, and so without any library.
- `WASM_FREETYPE`: `ON` (the default) builds FreeType, and the world image keeps the FreeType fonts of the image; `OFF` gives the world image bitmap fonts instead.
- `WASM_CAIRO`: `AUTO` (the default) builds cairo when FreeType is built, `ON` stops the configuration without FreeType, and `OFF` leaves it out.
- `WASM_CAIRO_PDF`: `ON` adds the PDF, PostScript and script surfaces of cairo, for AthensCairoPDFSurface (default `OFF`): 68 KB more of gzipped wasm, and cairo's configuration with zlib.
- `WASM_LIBGIT2`: `ON` builds libgit2, for Iceberg (default `OFF`): 258.6 KiB more of gzipped wasm.
- `WASM_LIBGIT2_HTTP`: with libgit2, `ON` (the default) gives it the smart-HTTP transport of `src/emscripten/gitSupport.c`; `OFF` leaves libgit2 with local repositories only.
- `WASM_SDL2`: `ON` builds SDL2, SurfacePlugin and the page `sdl.html` (default `OFF`): 156 KiB more of gzipped wasm.
- `WASM_DEPS_DIR`: a directory holding the pinned archives of the libraries, by their file names, instead of downloading them (see The libraries).
- `WASM_HOST_PHARO`: a native Pharo VM.
  By default, it is the VM that runs VMMaker: `WASM_VMMAKER_VM`, or the one that the host tree downloads, `build-wasm/host/build/vmmaker/vm/pharo`.
  With `WASM_GENERATED`, which skips the host tree, that is only `WASM_VMMAKER_VM`, or a VM left there by an earlier build of the same directory: give it otherwise.
  The lanes use it to load an image that the WebAssembly VM saved (S14b, which is skipped without it), to compare FreeType, cairo and libgit2 with the native VM (a note, never a failure), and to check the highlighter of the Notebook against the image's scanner.
  It also prepares the world image (`packaging/emscripten/st/prepare-web-image.st`, in about a second), and lanes 80 and 82 run `tests/wasm/st/osweb-native.st` and `tests/wasm/st/osweb-windows.st` with it; without it, `web/` gets the stock image.
- `WASM_IMAGE_ZIP`: a local image zip, instead of downloading the pinned Pharo 12 one into `build-wasm/downloads`: a copy of that one for an offline build, or a Pharo 15 one (from https://files.pharo.org/image/150/), which the site then runs.
  Any zip holding one image, its `.changes` and a `.sources` is accepted; one that is not the pinned image gets a note with its SHA256.
- `WASM_GENERATED`: a directory holding `generated/64`, the generated StackVM sources, which skips the host tree.
  For example, `make wasm WASM_BUILDDIR=build-wasm-2 WASM_GENERATED=build-wasm/host WASM_HOST_PHARO=build-wasm/host/build/vmmaker/vm/pharo` builds a second directory with the sources and the native VM of the first.
  The sources must come from this tree's `smalltalksrc`: the configuration refuses sources without the Emscripten hooks.
- `WASM_VMMAKER_IMAGE`, `WASM_VMMAKER_VM`: a VMMaker image, copied with its directory into `build-wasm/host/vmmaker-image` instead of bootstrapping one, and the Pharo VM that runs it (by default the one the host tree downloads, PharoVM 10.3.1 on Linux x86_64).
  The copy is refreshed from `smalltalksrc` the first time it is used, which takes about 12 s.
  These two are recorded in `config-host.make`: changing them configures the host tree again, and a new `WASM_VMMAKER_VM`, through the default `WASM_HOST_PHARO`, the WebAssembly tree too, unless `WASM_HOST_PHARO` is given.
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
- `PHARO_WASM_FFI_HIDE`, `PHARO_WASM_FFI_TRACE`, `PHARO_WASM_FFI_ADAPT`: the switches of the FFI (see The FFI).
- `PHARO_WASM_GIT_PROXY`: the CORS proxy of libgit2's HTTP requests in the node VM (see libgit2).
- `--maxFramesToLog`: the Smalltalk frames printed per process when the VM reports an error, 1000 by default on Emscripten (elsewhere, every frame); 0 prints every frame.
  A crash deep in a recursion would otherwise print hundreds of megabytes, which takes the VM minutes.
  The default is `PHARO_DEFAULT_MAX_FRAMES_TO_LOG`, in `include/pharovm/emscripten/sqPlatformSpecific.h`.

The environment of the web VM is its defaults and what its worker sets, never the host's: the variables above are for the node VM.

## How it works

### Slices

A browser worker, and node's event loop, need their thread back for input, timers and output, and the VM cannot run on a thread of its own: threads need SharedArrayBuffer, which needs cross-origin isolation, and JSPI is not available everywhere.
So the VM runs in slices and returns to its host between them (`src/emscripten/emscriptenMain.c`).
`main()` only gets the VM ready and returns with the runtime alive.
Every slice is then a call of `vm_resume()`: the first one starts the VM, and the later ones call `interpret()` again, which carries on from the state the interpreter saved in its globals.
A slice ends in `ioReturnToHostIfRequested()`, the last statement of `ioSynchronousCheckForEvents()` (`src/common/sqTicker.c`).
There the interpreter has nothing left on the C stack but returns, so the driver can `longjmp` back to `vm_resume()`; it forces an interrupt check first, so that the next slice runs the rest of the event check.
So the call must stay the last statement of the event check, and the callers of the event check must only return to the interpreter's loop: the lanes that run with 1 ms slices, and a check of Debug builds that every slice starts at the same depth of the C stack (`STACK DRIFT`), guard this.
A slice never ends inside an FFI callback, whose interpreter runs on the C (and JavaScript) frames of the callout (see The FFI).
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
The VM has about 710 MB of linear memory once the image has booted (675 MiB for Pharo 12, 683 MiB for Pharo 15), which V8 on Linux commits only as it is touched.

Perm space is not supported: WebAssembly memory cannot reach its address, 2^41.
An image with perm space, or `--minPermSpaceSize`, does not start ("Cannot allocate N bytes of perm space at 0x20000000000: WebAssembly memory cannot reach it"), and the primitives that move objects there (90, 91 and 93) fail with `#'insufficient object memory'`.

### Primitives and plugins

There is no `dlopen`.
The VM core and every plugin are static libraries, and the plugins are built in: FilePlugin, NewFilePlugin, FileAttributesPlugin, FloatArrayPlugin, LargeIntegers, MiscPrimitivePlugin, LocalePlugin, SocketPlugin, BitBltPlugin, B2DPlugin, DSAPrims, JPEGReaderPlugin and JPEGReadWriter2Plugin, and the plugins written for this platform in `src/emscripten/plugins`, such as WebHostPlugin, which Stop uses.
UUIDPlugin is built in as well: it uses the `uuid_generate` of Emscripten's JavaScript library (`crypto.getRandomValues`), and Pharo 15 makes its UUIDs only with it.
So is SurfacePlugin with SDL2 (`WASM_SDL2`): `plugins/SurfacePlugin/src/common/sqManualSurface.c` then takes the surface functions of BitBlt with their own types.
`cmake/plugins.cmake` leaves out UnixOSProcessPlugin (fork), and SurfacePlugin without SDL2, on Emscripten, and the initial cache SqueakSSL.
The 20 primitives of `src/ffi` are in the builtin table of the VM's support code (`vmsupport_exports`, generated by `cmake/emscripten/genSupportTable.cmake`).

WebAssembly checks the type of every indirect call.
The interpreter calls every named primitive as `void (*)(void)`, while the plugins define many of them as `sqInt f(void)`, and such a call traps.
So at build time `cmake/emscripten/genPrimitiveTable.cmake` reads the `<X>_exports` table of the interpreter and of every plugin, and writes a translation unit that includes the unchanged source and adds a `void` trampoline for every primitive (every row with an accessor depth), in a `<X>_primitives` table of the same rows; the named-primitive lookup of `src/common/sqNamedPrims.c` answers from those tables.
No source of the interpreter or of a plugin changes, and natively nothing does.
Emscripten's own answer, `-sEMULATE_FUNCTION_POINTER_CASTS`, made a VM without the trampolines 3.3 to 5.5 times slower.
The links use `-sTABLE_BASE=1024`, so that no function pointer has the value of a quick primitive index (up to 519), which shares the primitive function slot.
They also use `-Wl,--fatal-warnings`: wasm-ld links a stub that traps for a direct call whose prototype differs from its definition, and only warns.
Lanes 10 and 50 check all this.

### The FFI

The VMs have FEATURE_FFI with the same-thread runner of `src/ffi` (no worker threads), on upstream libffi 3.8.0, the release with a WebAssembly port for 64-bit pointers (`src/wasm`), built from its pinned archive by `cmake/emscripten/deps/libffi.cmake`.
The image gets its TFFIBackend: callouts through uFFI, variadic ones with `fixedArgumentCount:`, structures by value, callbacks, and `Smalltalk os environment at:put:`.
Every object of libffi is compiled once for 64-bit pointers and serves both links, the memory64 node VM and the web VM.
The FFI adds 18.1 KiB of gzipped wasm, and 10000 callouts of `strlen` took 302 ms in node.

There is no `dlopen`, so the libraries are linked in, and listed in a static registry, generated at build time (`cmake/emscripten/ffiRegistry.cmake`, `include/pharovm/emscripten/ffiRegistry.h`).
`src/externalPrimitives.c`, under `__EMSCRIPTEN__`, reduces the name the image gives to a library of the registry: its base name, without a leading `lib`, without what follows its first `.`, and without a version after a `-` (`libc.so.6` is `c`, `libSDL2-2.0.so.0` is `SDL2`, `libgit2.so.1.4.4` is `git2`, `/x/libfoo.so.1` is `foo`).
Each library is a sorted table of functions, taken by name from a list, `cmake/emscripten/ffi/symbols/<library>.txt`, every name of which must be declared by the library's public headers, or the build stops; its `# unavailable` sections list the names the images bind that the library of the VM does not define.
The libraries of the registry (`cmake/emscripten/ffiLibraries.cmake`):

| Library | The image's | Functions | Built |
|---|---|---|---|
| `c`, with the aliases `m`, `dl`, `pthread`, `rt` and `LibC` | `libc.so.6` | 144 | always |
| `freetype` | `libfreetype.so.6` (FT2FFILibrary) | 17 | `WASM_FREETYPE` |
| `cairo` | `libcairo.so.2` (CairoLibrary) | 120 | `WASM_CAIRO` |
| `git2` | `libgit2.so.1.4.4` (LGitLibrary) | 239 | `WASM_LIBGIT2` |
| `SDL2` | `libSDL2-2.0.so.0` (the SDL2 class of OSWindow-SDL2) | 119, and `SDL_PushEvent` | `WASM_SDL2` |
| `TestLibrary` | `libTestLibrary.so` | 122 | the node VM (and the web VM with `WASM_FFI_TEST_LIBRARY`) |

The lists are what Pharo 12 (build 1599) and Pharo 15 (build 41) bind, test packages included, and, for the C library, some more for the callouts of the tests and of the image's users.
`packaging/emscripten/tools/ffi-symbols.st` lists them: run on an image, natively or in the node VM, it prints a line for each function a method binds, with the library of the registry it is bound in and the method (on Pharo 12: 752 rows, 663 functions of 15 libraries).
Lane 59 checks with it that the registry has every function of a library it registers.

The library finders of the image look for the file of a library before they load it, in the VM's directory: the sites and the node VM have empty placeholders of those files (`libcairo.so.2`, `libgit2.so.1.4.4`, `libSDL2-2.0.so.0`), which the worker writes into `/pharo` from `manifest.libraries`, and which the VM never reads.

libffi's `ffi_call` is JavaScript: it calls the function from JavaScript, which converts the arguments and the result by the types of the function's own WebAssembly signature.
So a declaration that does not match the C function, harmless natively, throws a TypeError, or traps, and would end the VM.
`emscriptenFFICall` (`src/emscripten/emscriptenSupport.c`) guards every callout: one that threw fails with `PrimErrFFIException`, and says why on stderr ("FFI callout failed, its declaration does not match function N: ..." or "FFI callout trapped in function N: ..."), and the VM goes on.
libffi's own return of a 64-bit integer that comes back as a Number is widened, signed or unsigned, in a copy of its `src/wasm/ffi.c`.

The declarations of the images do not always have the integer widths of C, and work natively, where an int passed for a long fills the register: `cairo_pattern_set_extend` takes an int that Pharo declares `ulong`, FreeType's `FT_Error` is an int that Pharo declares `long`, and `git_tree_entrycount` answers a `size_t` that Pharo declares `int`.
So the registry also carries the C signature of every function, which `cmake/emscripten/ffiSignatures.mjs` reads from the headers, and `src/emscripten/ffiAdapt.c` adapts a callout whose integer arguments or result differ in width from the function's, as an x86-64 caller would, instead of failing it.
It also calls a variadic function with its variable arguments as C takes them, also when the declaration has no `fixedArgumentCount:`; one with a float, or an integer narrower than an int, among its variable arguments fails, and says "FFI callout to a variadic function <name>: declare it with fixedArgumentCount:".
The adapter changes nothing else: a mismatch of an integer with a float, a struct, or more than 16 arguments goes to the guard.
In the test runs it adapted `abs`, `bsearch`, `sprintf` and `strlen` (lane 58), `cairo_pattern_set_extend` and `cairo_scaled_font_extents` (lane 65), and `git_oid_nfmt` and `git_tree_entrycount` (lane 66).

Three environment variables of the node VM switch the FFI for the tests, read at the first load or callout:

- `PHARO_WASM_FFI_HIDE`: a comma-separated list of reduced library names that the registry does not answer, so that the image finds them missing (`PHARO_WASM_FFI_HIDE=freetype`).
- `PHARO_WASM_FFI_TRACE=1`: a line on stderr for each function the first time a callout to it is adapted, `FFI callout adapted: <name> (declared <classes>, C <signature>)`.
- `PHARO_WASM_FFI_ADAPT=0`: adapt no callout.

A callback runs a nested interpreter on top of the C and JavaScript frames of its callout, so a slice never ends inside one: `src/ffi/sameThread/sameThread.c` keeps the depth of callbacks (`emscriptenCallbackDepth`), and `ioReturnToHostIfRequested()` returns to the host only at depth 0.
Callbacks are reentrant (lane 58 nests them 50 deep), but a callback that computes for long keeps the VM from its host meanwhile: no output, input or Stop reaches it until it returns, and a 2 s loop in a callback ended no slice (55 slices for the run, against 151 with the same loop outside a callback).
A callback declared with another type than the C function pointer traps in the indirect call that reaches it, and its callout fails; a callout that fails inside a callback, whose interpreter the exception unwound, stops the VM ("an FFI callout failed inside a callback, which it unwound").
There are no threads: `callbackFromAnotherThread` of the test library fails, and TFWorker does not run.

### The libraries

Every library is built by a file of `cmake/emscripten/deps` from its pinned release archive, and linked into both VMs as a static library:

| Library | Archive | Built with | Optimisation |
|---|---|---|---|
| libffi 3.8.0 | `libffi-3.8.0.tar.gz` | `WASM_FFI` | `-O2` |
| FreeType 2.14.3 | `freetype-2.14.3.tar.xz` | `WASM_FREETYPE` | `-O2` |
| zlib 1.3.2 | `zlib-1.3.2.tar.gz` | cairo or libgit2 | `-O2` |
| libpng 1.6.58 | `libpng-1.6.58.tar.gz` | cairo | `-O2` |
| pixman 0.44.2 | `pixman-0.44.2.tar.gz` | cairo | `-O2` |
| cairo 1.18.4 | `cairo-1.18.4.tar.xz` | `WASM_CAIRO` | `-O2` |
| libgit2 1.4.4 | `libgit2-1.4.4.tar.gz` | `WASM_LIBGIT2` | `-Os` |
| SDL2 2.32.10 | `SDL2-2.32.10.tar.gz` | `WASM_SDL2` | `-Os` |

`cmake/emscripten/deps/fetch.cmake` looks for each archive in `WASM_DEPS_DIR`, then in `build-wasm/downloads`, where an earlier configure downloaded it, and otherwise downloads it there from its release URL.
The archive must have its pinned SHA256: one in `build-wasm/downloads` that has not is removed and downloaded again, and one in `WASM_DEPS_DIR`, or a download, that has not stops the configuration, with a message that names the file, both hashes and `WASM_DEPS_DIR`.
It is unpacked again only when the one unpacked had another hash.
No configure step of a library runs: what their autotools, meson or CMake would make is committed next to the file that builds them (FreeType's module list and options; the config headers and source lists of pixman, cairo and libgit2, which `cmake/emscripten/deps/cairo/README` and `libgit2/sources.cmake` say how to make again; SDL's `SDL_config_emscripten.h`), and each file checks the version of the archive it was made for.
The libraries are compiled with flags of their own, not with the VM's warnings, definitions and Debug optimisation: `-m64`, the setjmp/longjmp flags of the VM, their optimisation and `-w`.
The VM links one zlib, which libpng, cairo's PDF surface and libgit2 share.

- FreeType: its 26 sources, with a module list of its own (TrueType, CFF, autofit, smooth and raster) and 12 options off: no zlib, png, bzip2, brotli or harfbuzz, no colour or variable fonts, no Mac fonts.
  The fonts are those the images embed: lane 64 draws text with FreeType and compares it with the golden files of Pharo 12 and Pharo 15, which are those of the native VM (with FreeType 2.12.1: the same 688 pixels, the same hash).
  It adds 123.6 KiB of gzipped wasm, and about 0.3 s to the first frame of the world.
- cairo, with pixman, libpng and zlib, on the VM's FreeType (the build checks that cairo sees 2.14.3): its image surface only, no fontconfig, and no renderers of COLR v1 and OT-SVG glyphs, which that FreeType cannot feed.
  AthensCairoSurface, in OSWindow-Web, copies cairo's pixels into a Form instead of going through SurfacePlugin, which the default VM leaves out.
  Lane 65 compares shapes, gradients, FreeType text, a Roassal canvas and a Mondrian view with the golden hashes of the native VM.
  It adds 318 KiB of gzipped wasm.
- libgit2 and SDL2: see below.

Some libraries were deliberately left out:

- harfbuzz: FreeType is built without it, and no image binds it.
- fontconfig: cairo is built without it, since the image makes its font faces from the FT_Face of its own fonts, and no image binds it.
- OpenSSL: the SqueakSSL plugin is off, and the libraries get no TLS of their own; the browser does the TLS of what goes through it, such as the requests of libgit2's smart-HTTP transport.
- libssh2: a browser opens no socket, so there is no SSH: libgit2 has no SSH transport, and OSWindow-Web makes Iceberg's remotes https:// ones.
- bzip2: FreeType reads no compressed fonts here (the images embed theirs), and nothing else asks for it.

### libgit2

`make wasm WASM_LIBGIT2=ON` links libgit2 1.4.4, the version that the native VMs ship, whose structures Pharo 12's LGit bindings match exactly (`git_fetch_options` is 208 bytes in 1.4.4 and 216 in 1.9), built at `-Os` from the 169 sources that a configure of upstream's CMake lists: no threads, SSH, HTTPS, NTLM, GSSAPI or iconv, the C library's regcomp, SHA1DC, and the bundled http-parser.
Iceberg then works on the file system of the VM, through the image's own bindings (65 FFI methods and 9 callback classes in lane 66): init, commit, status, diff, log, branches and file:// clones.

libgit2's own transports would resolve names, and `getaddrinfo` fails with `EAI_FAIL` (it is wrapped: Emscripten's ends a memory64 runtime), so a git:// remote fails with "failed to resolve address for <host>", and the VM goes on.
With `WASM_LIBGIT2_HTTP` (the default), `src/emscripten/gitSupport.c` registers a smart-HTTP transport for the http and https remotes instead: each request of git's smart HTTP protocol is a synchronous XMLHttpRequest of the worker, which blocks the VM until the response has come, and the browser does the TLS.
Git servers, github.com among them, send no CORS headers, so a page of another origin reads them only through a CORS proxy: the proxy is a URL prefix put in place of the scheme of the remote's URL (`https://github.com/o/r.git` through `https://proxy.example/` is requested as `https://proxy.example/github.com/o/r.git/info/refs?...`).
The proxy is a setting of the Console page (Settings), kept in this browser's localStorage, which the world page, `sdl.html` and the Notebook use too (a change reaches their running VMs), and `PHARO_WASM_GIT_PROXY` in the node VM; there is none by default.
It is never taken from a URL: the proxy sees the code and any credentials that pass through it, and a link must not choose where a user's git traffic goes; a `?gitProxy=` of the page's URL is ignored.
A request that the browser does not give the answer of (CORS, the network) fails with "git http: request failed (CORS or network)", and a remote that asks for credentials (401, 403 or 407) fails with `GIT_EAUTH`.
OSWindow-Web (version 4 and later) sets Iceberg's remotes to https.
In the browsers, a clone through the proxy of the spec took 509 ms (Chromium) and 404 ms (Firefox), with 2 requests.

The working copies live in the memory of the VM, and are lost on a reload: only the image is kept in the browser.
libgit2 adds 258.6 KiB of gzipped wasm; the default build is unchanged.

### SDL2

`make wasm WASM_SDL2=ON` links SDL2 2.32.10, Emscripten's version of its port, built at `-Os` from the signed release archive with a committed `SDL_config_emscripten.h`: the Emscripten video driver, the dummy video driver for node (`SDL_VIDEODRIVER=dummy`), dummy audio, joystick and haptic drivers, no OpenGL ES 2 or EGL, and no iconv.
The image needs no more than its video, events, timer and clipboard: a worker has neither an AudioContext nor gamepads.
The worker gives the VM `SDL_EMSCRIPTEN_KEYBOARD_ELEMENT=#canvas`, so that SDL takes the keys from the canvas.
See The world through SDL2 for the page.

### What else differs

- The Release build defines NDEBUG, as CMake's Release flags do: with the asserts, a 1M-deep recursion took 34 s instead of 1 s.
- With `WASM_FFI=OFF`, the image uses its NullFFIBackend, and the support primitive that the image calls at every start to initialise its callbacks does nothing, silently.
- `getaddrinfo` is disabled (`EAI_FAIL`): Emscripten's version fails in wasm64 with a JavaScript TypeError, which ends the VM.
  Name resolution fails instead with an error that the image can catch.
- `aio` sets no `F_SETOWN` or `SIGIO`, and does not `fsync`.
- The microsecond clock has a resolution of 1 ms (`Date.now()`), and `ioHighResClock()` answers monotonic nanoseconds.
- The image attributes 1001, 1002 and 1003 are `unix`, `Emscripten` and `wasm64`, and the platform is `Unix64Platform`.
- The path of the VM is its argv[0] when no file has that name: `/pharo/pharo` in the pages, so that `Smalltalk vm directory` is `/pharo`.

### The web pages

The files of `web/`, from `packaging/emscripten/web`:

- `index.html`, the Console page, and `page.js`, which drives its VM; `notebook.js`, its Notebook tab, with `notebook-lib.js` (the Markdown renderer, the sanitizer of markup, the `.json` format), `notebook-st.js` (what is Smalltalk: the brackets, the highlighter, the `.st` format, the example) and `nb-kernel.js` (the client of the kernel).
- `world.html` and `world.js`, the world page, with `keymap.js` (the keys of the page as SDL keycodes, scancodes and modifiers) and `display-worker.js` (the display of the WebDisplayPlugin, in the worker).
- `sdl.html` and `sdl.js`, the world through SDL2, with `sdl-shim.js` (the DOM of SDL, in the worker), with `WASM_SDL2=ON` only.
- `vm-worker.js`, the worker of every VM, with `vm-driver.js` (the slices, the stdio devices and `/dev/nbevents`) and `vm-storage.js` (the saved image in IndexedDB); `open-image.js`, which reads the images of the user's own; `pharo-logo.png`.
- `pharo-web.js` and `pharo-web.wasm`, the VM; `manifest.json`; `THIRD-PARTY-NOTICES.txt`; `image/`, the image, its `.changes` and the `.sources`, gzipped; `st/`, `web-repl.st`, `web-notebook.st`, `web-bootstrap.st` and `OSWindow-Web.st`.

The page compiles `pharo-web.wasm` once, and gives it to every worker it starts (`packaging/emscripten/web/vm-worker.js`).
The worker fetches the files of `manifest.json`, inflates them as they come (DecompressionStream), writes them into `/pharo` in memory, with the placeholders of the libraries, and boots the image in one of four modes (`PharoVMDriver.vmArgs`): `console`, the REPL, `--headless /pharo/Pharo.image --no-default-preferences st --no-source /pharo/st/web-repl.st`; `notebook`, the same with `web-notebook.st`; and `world` and `sdl`, `--headless /pharo/Pharo.image --no-default-preferences --interactive`, with a display or without one.
`--no-source` keeps the script, which is filed in at every boot, out of the `.changes`.
What the VM writes to stdout and stderr is posted at the end of every slice, and the worker pauses the VM while more than 1 MiB of it is not acknowledged, until less than 512 KiB is: the page acknowledges what it rendered, or, while it is hidden and gets no animation frames, what it received.
Stdin is Emscripten's terminal device with its read and poll replaced: a read never blocks, and the REPL waits with `waitForData`, so the VM sleeps until the page sends a line.
Stop calls `vm_interrupt()`, which signals the semaphore that the REPL, or the kernel, registered with the WebHostPlugin.
When the image saves, the image file access handler tells the host (`Module.onPharoHost`), and after the slice the worker stores the image and its `.changes` in IndexedDB.
The database belongs to the directory of the site: `pharo-wasm` for a site at the root of its origin, and `pharo-wasm:/dir/` for one in `/dir/`.
The store is guarded by a per-save id, so that a tab never pairs its `.changes` with the image of another tab.
In the Console, the worker names the file `/pharo/.pharo-web-world` in `PHARO_WEB_WORLD_FILE`, and the REPL writes there whether the image has OSWebDriver, and its version, when it starts and before it saves: so the page knows whether the image can open the world.
A worker in `notebook` mode boots the saved image but never stores anything: it ignores the VM's saves, `save`, uploads and preparations.

The messages between a page and its worker:

- page to worker: `init` (the compiled module, the manifest's URL, the build, the mode, the slice, whether to boot the saved image, an upload, a preparation, a display, the git proxy, a canvas), `input`, `eof`, `interrupt`, `save`, `ack`, `fs`, `download`, `resetStorage`, `flush`, `display`, `gitProxy`, `sdl` and `stats`;
- worker to page: `progress`, `ready` (what was booted, and what the VM has), `output` (text of fds 1 and 2, bytes of fd 3), `state`, `tick`, `interrupted`, `edited`, `storing`, `saved`, `prepared`, `superseded`, `reset`, `exit`, `crash`, `fs-result`, `file`, `error`, `display`, `sdl` and `stats`.

The notebook kernel reads its requests on the VM's stdin, one JSON line each (`run {rid, name, code}`, `ping`, `stop-background`), and writes its events on `/dev/nbevents` (`hello`, `start`, `display`, `clear`, `stream`, `error`, `done`, `pong`, `stopped`, `bad-request`), which the worker posts as fd 3; the requests are checked strictly, and the page checks every event before it allocates anything for it.

What the user opens is read by `open-image.js`, which the pages load: it reads the central directory of a zip, inflates its entries with `DecompressionStream('deflate-raw')` (and takes stored ones as they are), pairs the files, and checks the image's header before the VM is touched.
The page gives the files to a new worker in `init.upload` (`{image, changes, sources}`).
A `.sources` of the image's own goes to `/pharo/<its name>` instead of the site's, which is then not fetched, and the saved image keeps it in IndexedDB under the key `Pharo.sources`.
On the world page, `init.prepare` boots an image that has no OSWindow-Web, or an older version of it, with the REPL, which evaluates `CodeImporter evaluateFileNamed: '/pharo/st/web-bootstrap.st'. Smalltalk snapshot: true andQuit: false`; the worker answers `prepared {error, saved}`, and the page then boots the world from the saved image.

### The world

The Pharo world draws itself into a Form, and the VM only blits it.
SDL2 under Emscripten would need the DOM of the main thread, or pthreads, and the image's SDL2 driver blits through SurfacePlugin, so the world page does without it (`sdl.html` gives it a shim of the DOM instead).
The world image has the OSWindow-Web package (`packaging/emscripten/st/OSWindow-Web`): OSWebDriver, a backend window and a Form renderer, which a startUp: hook picks whenever the page gives a display, and the extension of AthensCairoSurface that copies cairo's pixels into a Form.
It keeps the FreeType fonts of the image, unless the build has no FreeType, and then gets bitmap fonts: `OSWebDriver>>setUpImage` takes the fonts the build gives from `webimage-fonts.txt`, and the manifest says which (`fonts`).
The package is at version 5 (`OSWebDriver class>>packageVersion`, the manifest's `webPackage`): version 2 kept the FreeType fonts, 3 added the AthensCairoSurface extension, 4 makes Iceberg's remotes https:// ones, and 5 draws the world in device pixels on high-density displays; an image saved with an older version is prepared again when the world page opens it.
The native Pharo VM prepares it at build time, which is fast and keeps the world image independent of the VM being built; it is prepared again when a class of the package changes, comes or goes.

`src/emscripten/plugins/WebDisplayPlugin.c` is a builtin plugin of 13 primitives.
The image blits each damaged rectangle of its 32-bit Form into the frame, an RGBA copy, and presents the frame once per Morphic cycle.
The dirty rectangles (up to 64, then their bounding box) go to the page, and the slice ends, so that the canvas shows them at once.
The page's events come back as records of 8 integers, in a ring of 256 records, and each one signals the image's input semaphore.
The title, the cursor (1-bit Forms, as RGBA), the clipboard and the focus go to the page as well.
In the worker, `display-worker.js` paints the dirty rectangles, straight from the VM's memory, into the OffscreenCanvas that the page transferred to it.
`world.js` sends it the size of the canvas, in CSS pixels and in device pixels, and the input, which `keymap.js` maps to SDL keycodes, scancodes and modifiers.

A stock image cannot open the world page as it is: its world starts through OSSDL2Driver.
The world page prepares the stock images that it opens, and the Console offers to ("Prepare for the world"): both run `packaging/emscripten/st/web-bootstrap.st`, which files in the package and sets up the fonts, and then save.

### Pharo versions

The site runs the pinned Pharo 12 image by default, and was also tested with Pharo 15 (Pharo15.0-SNAPSHOT, build 41): built into the site with `WASM_IMAGE_ZIP`, and opened from disk on either page.
The VM is the same; what differs is the Smalltalk of the pages (`packaging/emscripten/st`), which works with both:

- Pharo 15's command line (Clap) takes a single file after `st`, with `--save` and `--quit` as its own options, and a file only after `st`: the world image is prepared with `st --save --quit prepare-web-image.st`, which finds its directories in `PHARO_WEB_ST_DIR` and `PHARO_WEB_IMAGE_DIR`.
- Pharo 15's chunk reader takes one method per `methodsFor:` section, so `web-repl.st`, `web-notebook.st` and the `OSWindow-Web.st` that `prepare-web-image.st` writes (the same from both versions) give each method its section, and define every class before its methods; `web-notebook.st` is ASCII, since Pharo 15 mangles the other characters of a file-in.
- Pharo 15 handles errors through `ErrorHandler default`, opens its world in `UIManager class>>startUp:` (so OSWebDriver registers for an earlier startup), and does not boot with an empty `.changes`: the pages give an image opened without one a `.changes` that holds only its version header, `"VERSION:1.0"!`.
- The libraries of the registry are those both images bind: they bind the same functions of cairo, FreeType and SDL2, and each binds one function of libgit2 that the other does not.

## Tests

`make wasm-check` runs `tests/wasm/run-lanes.sh`, which runs every `tests/wasm/lanes/*.sh` in order; `sh tests/wasm/run-lanes.sh 20 60` runs only the lanes whose names start with 20 or 60.
A lane that cannot run here is skipped, with the reason, and the run ends with the list of the lanes and of the checks that were skipped.
`make wasm-check` gives the lanes `NODE`, `WASM_DIR` (the build directory), `GEN` (the generated sources), `HOST_PHARO` (`WASM_HOST_PHARO`), `SRCDIR` and `TEST_DIR` (`build-wasm/tests-run`).
Run by hand, `run-lanes.sh` defaults them for `build-wasm`, except `HOST_PHARO`, which stays empty unless it is given: the checks that need a native Pharo VM are then skipped.
The lanes are meant for Release builds.
On the machine the port was made on, the 23 lanes passed in 14 minutes on a build with libgit2 and SDL2, with `NOTEBOOK_IMAGES` of a Pharo 15 image, 9 minutes of which were lane 61; without libgit2 and SDL2, lanes 66 and 67 are skipped.
The times below are those of that run:

| Lane | What it checks | Checks | Time |
|---|---|---|---|
| 05-build-rules | the rules of `GNUmakefile` and `cmake/emscripten/webimage.cmake`, in small trees where stand-ins of cmake, emcmake, the Pharo VMs and the specs log what they are asked to do: what is configured, generated or prepared again when a setting (the libraries' and `WASM_DEPS_DIR` too), the initial cache, a `.st` file or a class of OSWindow-Web changes, comes or goes, the fonts of the world image, the patch of libffi, the guard of `WASM_BUILDDIR`, and which specs `make wasm-check-browser` runs; it needs GNU make and cmake, and builds no VM | every rule | 7 s |
| 10-types | `tests/wasm/check-types.sh` compiles the generated interpreter with `sqVirtualMachine.c`, `client.c` and `sqExternalSemaphores.c` as one translation unit: every interpreterProxy slot and prototype must have the interpreter's type; both link commands of the CMake tree (Unix Makefiles or Ninja) carry `-Wl,--fatal-warnings`, and neither module has a signature mismatch stub | 0 errors | 1 s |
| 20-smoke | `tests/wasm/wasm-smoke.sh`, S1 to S22 with the node VM: the exact output of `eval` and `st`, the platform and the FFI backend, files, the environment (`setenv` through the FFI), Delays, preemption, deep recursion, a growing heap, a snapshot reloaded in WebAssembly and natively, exit statuses, time zones, the builtin plugins, the slices and where old space is placed | 28 | 24 s |
| 22-old-space | `tests/wasm/st/oldspace-window.st`, `oldspace-regrow.st` and `permspace.st` with the node VM: old space ends in an `OutOfMemory` at the end of its window, and the young objects stored at its top survive a scavenge; a freed segment at the top is used and grown again; the perm-space primitives fail | 3 | 12 s |
| 25-session-id | `tests/wasm/session-id.mjs`: a snapshot booted again within the same second gets a session ID of its own and refuses the old session's file handles; the C streams are flushed on exit | 8 | 2 s |
| 30-engines | S2, S8 and S9 again, in Liftoff code only (`--liftoff-only`) with a 900 KB stack, and in a worker thread with a 1 MB stack, the stack of a browser worker; S2t checks that V8 compiled nothing with TurboFan, and fails when the V8 flags of the lane do not reach node | 8 | 17 s |
| 40-bench | `tests/wasm/bench.sh`: the measures below, next to reference numbers; it fails when 300k-deep recursion takes 1 s, when `eval '3+4'` takes 1.5 s, or when 2 s of computing run in fewer than 10 slices | 8 | 13 s |
| 50-prim-audit | `tests/wasm/prim-audit.mjs` reads `pharo.wasm`: the named-primitive lookup answers the trampoline tables, every primitive is called through a `() -> ()` function, every function pointer is at least 1024, nothing goes through `-sEMULATE_FUNCTION_POINTER_CASTS`, the builtin modules are the expected ones (17 with SDL2), and the 20 support primitives of `src/ffi` are there | every row (403) | < 1 s |
| 55-webdisplay-unit | `tests/wasm/webdisplay-unit.c` on the frame code of the WebDisplayPlugin, then its primitives on bad arguments in the node VM | 235 + 34 | 2 s |
| 56-memory-unit | `tests/wasm/memory-unit.c` drives `src/emscripten/memoryEmscripten.c` as Spur does, against a stub of `pharovm/pharo.h`: fixed spaces, segments that grow again where freed ones were and come back zeroed, old space that never leaves its window, and a first segment larger than the window | 55 + 2 | 1 s |
| 58-ffi | `tests/wasm/st/ffi.st` with the node VM: callouts to the C library and to the FFI test library, scalars, a variadic function, structures by value, callbacks, reentrant ones 50 deep, results declared wider than the function's, the declarations that the adapter adapts (traced with `PHARO_WASM_FFI_TRACE=1`), and exactly two guarded failures and one of the adapter, each with its one line on stderr; a bench of 10000 `strlen` callouts (`WASM_FFI_BENCH_MS`, default 10000); then a 2 s loop in a callback must end no slice (`PHARO_WASM_STATS`) | 3 (26 in `ffi.st`) | 9 s |
| 59-ffi-clean | the stock image and the world image boot, run `tests/wasm/st/ffi-session.st` (the image's own callouts: the environment, getpid, strerror, a qsort with a callback), save, and run it again on the saved image, then draw text with the image's default font, all without a line of the guard or anything on stderr; a negative control, one mismatched callout, shows that such a line would be seen; and `ffi-symbols.st` says that the registry has every function that the image binds in a library of the registry | 12 | 27 s |
| 60-vm-harness | `tests/wasm/vm-harness.js`: the web VM and the REPL through `vm-driver.js`, in node, with the default engine and then in Liftoff code only | 32 + 32 | 33 s |
| 61-notebook-harness | `tests/wasm/notebook-lib.test.mjs` on the libraries of the Notebook (the brackets, the highlighter and their fuzz, the `.st` and `.json` formats, the frames of `/dev/nbevents`, the PNGs, the sanitizer, in linear time) and, with `HOST_PHARO`, the highlighter against the image's own scanner; then `tests/wasm/notebook-harness.js`, the kernel through `vm-driver.js` (requests, values, errors, Stop, output credit, encodings, rich output, background processes, snapshots, the end of stdin; the FFI, cairo and FreeType of the build) and the page's client against the real `vm-worker.js` in worker threads (the queue, Stop and its watchdog, restarts, exits, floods, uploads, the saved image), with the default engine and in Liftoff code only, on the stock image and on each image directory of `NOTEBOOK_IMAGES` | 28 + 64 + 64 per image | 548 s (2 images) |
| 62-web-repl-regress | `tests/wasm/web-repl-regress.js`, through `vm-driver.js` as lane 60: a Stop of an evaluation that has not started yet, a Warning that nothing handles, where a syntax error is, and the file in which the REPL tells the page about its image | 4 | 3 s |
| 64-freetype | `tests/wasm/st/ft-parity.st` on the stock and world images: FreeType 2.14.3 draws a line of the default font with the 688 pixels and the hash of the golden file of the image's version (`tests/wasm/golden/ft-p12.txt`, `ft-p15.txt`); with FreeType hidden (`PHARO_WASM_FFI_HIDE=freetype`) the image falls back to StrikeFont; a note of what FreeType costs a boot | 4 | 10 s |
| 65-cairo | `tests/wasm/st/athens-parity.st` on the world image: shapes, FreeType text, a repeated gradient, a Roassal canvas, a Mondrian and a PNG drawn by cairo 1.18.4, against the golden hashes (`athens-p12.txt`, `athens-p15.txt`); the functions the adapter adapts (`adapted-65.txt`); with `PHARO_WASM_FFI_ADAPT=0` the gradient fails as a primitive; the stock image without SurfacePlugin fails cleanly, or, with SDL2, draws through it the shapes of the golden file; with cairo hidden, a SymbolNotFoundError | 5 | 20 s |
| 66-libgit2 | `tests/wasm/st/iceflow.st` on the stock and world images: a repository, a package, two commits and a diff, the log, a branch, the status and a file:// clone through Iceberg, against the golden output of the native VM (`iceflow-p12.txt`, `iceflow-p15.txt`); the adapted functions (`adapted-66.txt`) and the control without the adapter; a git:// clone of an unresolvable host fails cleanly; with git2 hidden, a SymbolNotFoundError; skipped without libgit2 | 6 | 13 s |
| 67-sdl2 | `tests/wasm/st/sdl-session.st` on the stock and world images, with SDL's dummy video driver: the world opens through OSSDL2Driver on SDL 2.32.10, a Playground and a Browser, the clipboard, the cursors, the title, a resize, fullscreen, text input, the colours of the world, and the events of a user queued with `SDL_PushEvent` (the world menu, `3 + 4` typed and printed); SurfacePlugin is built in; with SDL2 hidden, a SymbolNotFoundError; skipped without SDL2 | 4 | 12 s |
| 70-worker-harness | `tests/wasm/worker-harness.js`: `vm-worker.js` as staged, in worker threads behind a shim of the worker globals: the protocol, Stop, output credit, the downloads, persistence, two workers on one saved image, an image opened with a `.sources` of its own, the preparation of an image for the world and the versions of OSWindow-Web, the placeholders of the libraries, and the notebook mode, which stores nothing | 30 | 70 s |
| 72-open-image | `tests/wasm/open-image.test.mjs` on `open-image.js` in node: zips (deflated and stored entries, data descriptors, UTF-8 names, directories and the files that the Finder adds, zip64), how the files are paired, every refusal, the image headers, drops, and the build's stock image zipped as files.pharo.org does and opened; `OPEN_ZIPS`, paths separated by colons, adds zips of your own | 11 | 3 s |
| 80-world-harness | `tests/wasm/keymap.test.mjs`, then `tests/wasm/world-harness.mjs`, which plays the world page in node with a memory framebuffer, and `tests/wasm/st/osweb-native.st` on the native VM | 16 + 15 + 27 | 18 s |
| 82-osweb-windows | `tests/wasm/st/osweb-windows.st` on the native VM: a second OSWindow without an event handler leaves the world shown, and the window of the Emergency Debugger (in Pharo 15, which has none, a window of the same kind) takes the canvas and the input, then gives them back | 11 | 10 s |

The bench measured, on that machine with node 25.2.1, on a build with libgit2 and SDL2:

| Measure | This build | Native StackVM |
|---|---|---|
| `eval '3+4'`, wall time | 325 ms | 80 ms |
| 300k-deep block recursion | 189 ms | |
| 1M-deep block recursion | 934 ms | 766 ms |
| tinyBenchmarks bytecodes/s | 154.5M | 941.0M |
| tinyBenchmarks sends/s | 9.5M | 48.0M |
| `26 benchFib` | 41 ms | 8 ms |
| 20 x 100000 `inject:into:` | 568 ms | 120 ms |
| GC stress, 3M arrays | 2510 ms | 1460 ms |

The lanes take these settings:

- `WASM_CHECK_TIMEOUT`: the seconds that a run of the VM, or of a unit test, may take (default 120); the harnesses that boot several images stop after 300 or 600 s.
  A run that is stopped then fails its check, or its lane, which says that it timed out.
- `WASM_CHECK_TRIES`: the runs that a check that measures a time may take while that time is over its limit (default 3).
  The best run counts: a loaded machine slows a run down, never speeds one up.
- `WASM_BENCH_RECURSION_MS` (1000), `WASM_BENCH_EVAL_MS` (1500) and `WASM_BENCH_SLICES` (10): the limits of the bench.
  `WASM_BENCH_BYTECODES` (100000000) and `WASM_BENCH_SENDS` (6000000) are those of the tinyBenchmarks warnings, which fail the bench only with `WASM_BENCH_STRICT=1`.
- `WASM_FFI_BENCH_MS`: the limit of lane 58's bench of 10000 callouts, in ms (default 10000).
- `NOTEBOOK_IMAGES`: image directories, separated by colons, on which lane 61 runs too, after the build's stock image: a Pharo 15 one on a Pharo 12 build, say.
  `NB_SCAN_METHODS`: the methods of the image whose tokens lane 61 compares with the highlighter (default 5000).
  `NB_CASES`: a regular expression; lane 61 then runs only the cases of `tests/wasm/notebook-harness.js` whose names match it.
- `WASM_CC`: the compiler of lanes 10 and 56, instead of `emcc -m64` from the PATH or `$EMSDK`; an exported `CC` or `CFLAGS` is ignored.
  Lane 55 compiles with it too.
  Run by hand, `GEN=build-wasm/host/generated/64 sh tests/wasm/check-types.sh` takes the clang or emcc of `CC` (default `emcc -m64`), and exits 0 without errors, 1 for type errors or a failing compiler, and 2 when it cannot check (a `CC` that is not clang, or no generated interpreter in `GEN`).

`run-lanes.sh` empties its `TEST_DIR` only when it is a scratch directory: the default `build-wasm/tests-run`, an empty directory, or one that it used before.
One run at a time uses a `TEST_DIR`: a second `make wasm-check` of the same build waits for the first one.
The runner exits 2 when it may not use its `TEST_DIR`, or cannot make its lock there.

The browser specs drive the pages in real browsers through Playwright, which is not a build dependency, served by `packaging/emscripten/tools/serve.mjs` without COOP or COEP headers (`tests/wasm/lib/pw.mjs`).
Each browser gets a context of its own, so its IndexedDB and localStorage start empty, and every spec ends with a check that no page or worker logged an error.
The specs that open images zip the build's stock image, `<web>/../image/stock`, so the web directory they test must have that directory next to it.
`make wasm-check-browser` runs, in order:

- `tests/wasm/page.spec.mjs`, the Console: evaluation, the keys, history, Stop, Save, Download, the persistence, two tabs, Open with good and damaged zips, drops, Settings and the git proxy (also that a `?gitProxy=` is ignored), the tabs, and the Content-Security-Policy; `OPEN_ZIPS` (paths separated by colons, Pharo downloads say) makes it open those zips too.
  42 checks per browser.
- `tests/wasm/notebook.spec.mjs`, the Notebook tab: the lazy start, Run all on the example, rich outputs, the keys, Stop and its watchdog, the coloring, its contrast and its budget, the caps of the outputs, the sanitizer, the command keys, Upload, the layouts at 390, 360 and 320 px, Restart, Import and Export, autosave across tabs and reloads, Export .html opened from `file://`, the image of the Console, and no worker from `file://` or without memory64.
  `NB_OPEN_ZIP` names a Pharo zip (of the other version, say) that the Console opens last: the kernel then boots it, and Run all on the example passes on it too.
  58 checks per browser.
- `tests/wasm/world.spec.mjs`, the world page, on a build with the world image.
  It boots the world with `st /pharo/st/world-probe.st` too (`tests/wasm/st/world-probe.st`), which writes `/pharo/probe.json` (the menus, windows, Playground text, debuggers and fonts of the world), and reads it through `window.PharoWorld`, which the world page gives tests: `stats` (the workers started, the frames and when they came, the times from a key to the next frame), `state` (the status pill), `unsaved` and `readFile(path)`.
  It checks that FreeType draws the world, and, with cairo, that the Inspector's Roassal canvas, Code Changes and the color picker draw.
  34 checks per browser.
- `tests/wasm/ffi.spec.mjs`, on a build with the FFI: a qsort callback, setenv, an adapted callout and a mismatched one in the Console.
  6 checks per browser.
- `tests/wasm/git.spec.mjs`, on a build with libgit2 and its transport, and a git with `git http-backend`: a clone without a proxy fails with "request failed (CORS or network)", a clone and a fetch through the proxy of Settings, and again after a reload; every request reached the git server through the proxy.
  7 checks per browser.
- `tests/wasm/sdl.spec.mjs`, on a build with SDL2: `sdl.html` paints the world through OSSDL2Driver, the world menu, typing into a Playground and printing, a stock image opened unprepared, the git proxy of Settings (a change reaches the VM, one with credentials is refused, a Restart gives it in its init), the idle cost against the world page (printed, never a failure), and the world page still on OSWebDriver.
  9 checks per browser.

`tests/wasm/pages-regress.spec.mjs` holds regression checks of the pages (the live regions, input methods, output while the page is hidden, the questions before the page goes, Reset after a failed boot, two sites of one origin, a closed database connection, a browser without DecompressionStream, a notebook cell of 5 MB of output while hidden, the world's progress bar, F6, a refused clipboard write and a world that does not open), and runs the same way; `make wasm-check-browser` does not run it.
`PLAYWRIGHT_MODULE` names the Playwright package, and `BROWSERS` the browsers (default `chromium`); Playwright's own variables, such as `PLAYWRIGHT_BROWSERS_PATH`, apply:

    PLAYWRIGHT_MODULE=/path/to/node_modules/playwright BROWSERS=chromium,firefox make wasm-check-browser

or for one spec, on any staged site:

    PLAYWRIGHT_MODULE=/path/to/node_modules/playwright BROWSERS=chromium,firefox node tests/wasm/page.spec.mjs build-wasm/web

On a build with libgit2 and SDL2, in Chromium 153 and Firefox 155 together, the Console spec passed its 84 checks in 117 s, the Notebook spec its 116 in 183 s (with `NB_OPEN_ZIP` of Pharo 15), the world spec its 68 in 112 s, the FFI spec its 12 in 7 s, the git spec its 14 in 12 s, the SDL spec its 18 in 69 s, and `pages-regress.spec.mjs` its 35 in 143 s.
`SHOTS=1` saves screenshots of the pages, light and dark, at 1280 and 360 px, in `build-wasm/tests-run/shots`.

## Limitations

- StackVM only: 5 (sends) to 6 (bytecodes) times slower than the native StackVM, and 14 times (bytecodes) to 27 times (sends) slower than the native JIT on tinyBenchmarks; on the machine the port was made on, the JIT ran 2181M bytecodes/s and 260M sends/s.
- No `dlopen`, and no external plugins: every plugin is built in, and the FFI reaches only the libraries and functions of its registry (see The FFI); a library the image loads by another name, or a function that no list names, is not found.
- A declaration that does not match its C function other than in the width of an integer fails as a primitive, where natively it may work; a callback declared with another type than C's traps, and fails its callout.
- A callback keeps the VM from its host until it returns: no output, input or Stop meanwhile.
- No name resolution (`getaddrinfo` is disabled), and sockets are of little use in browsers.
- No `fork`, OSProcess, SSL, SSH, threads or TFWorker, and no external surfaces without SDL2 (SurfacePlugin is left out).
- libgit2 reaches http:// and https:// remotes only, through a CORS proxy for a server of another origin, without credentials; its working copies are lost on a reload.
- Old space is at most 512 MiB by default, and perm space is not supported (see `WASM_OLD_SPACE_BASE`).
  Spur asks for more old space than an object needs (512 MiB for a 256 MB ByteArray), so a single object of 256 MB already fails with the default base, in both VMs.
  The VM has about 710 MB of linear memory once booted, and the Notebook's kernel as much again.
- The clock has a resolution of 1 ms, and browsers clamp nested timers, which wake a sleeping VM, to 4 ms: 200 Delays of 1 ms took 1135 ms in Chromium and 1333 ms in Firefox.
  In Firefox, `performance.now()`, the monotonic clock of the VM and of its heartbeat, steps by 1 ms in a worker of a page that is not cross-origin isolated (the pages are not), against 0.1 ms in Chromium.
- A long primitive (a huge `LargePositiveInteger` operation, say) does not yield: Stop then replaces the worker after 3 s, losing what was not saved.
- Stop ends the evaluation, not the processes that it forked: one that never waits keeps the VM busy until Restart.
- One image is saved per browser and site directory, shared by all its tabs and pages; the last save wins.
- The Notebook:
  - its cells have no stdin: a cell that reads it says so, and waits until it is stopped;
  - a process that a cell forks and that waits on stdin in the background takes the bytes of the page's next request, which the kernel then refuses as a bad request: the cell sent never ends, until Stop and the watchdog, or Restart;
  - a block of an earlier cell that returns with `^` (a non-local return) has no cell to return to, and fails with BlockCannotReturn in the cell that calls it; an exception or a context kept from an earlier cell cannot be resumed;
  - what a background process writes on `Stdio stdout` or `Stdio stderr` (not on the Transcript) goes to the cell that runs then, or to the kernel's messages;
  - a cell can write events of its own on the kernel's device, a done of its own count say, which ends that cell early; the page checks every event, and the cells after it get their own results;
  - a huge output of right-to-left text takes long to lay out in Chromium: 500000 characters written in one line took 9.7 s (Hebrew) and 24.3 s (Arabic) to show, against 0.14 s for Latin text, and 1.0 s and 0.9 s in Firefox.
- `sdl.html` keeps nothing in the browser, and its clipboard, resizing, HiDPI and input methods are SDL's, not bridged to the page; it costs about 3.5 to 6 times the idle VM time of the world page.
- Safari is untested.
- The canvas of `sdl.html` has one pixel per CSS pixel, so it is blurred on HiDPI screens (the world page draws in device pixels).
  Input methods commit their text when the composition ends, touch gestures are not supported, and touch screens are untested.

## Licences

`THIRD-PARTY-NOTICES.txt`, in `node/` and `web/` and linked from the pages, gives the licences of the software that the WebAssembly modules contain besides the VM, which is under the MIT License (`LICENSE`).
`cmake/emscripten/stage.cmake` assembles it at configure time: `packaging/emscripten/THIRD-PARTY-NOTICES.head.txt`, then, for each library linked into the VM, its version, the URL of its archive and the SHA256 of that archive, with the full text of the licence files the library declares (`pharo_wasm_dep_fetch(... LICENSES ...)`), then the runtime of Emscripten: its JavaScript support code, the C library (musl) and the compiler runtime (compiler-rt).
It was 166694 bytes on a build with libgit2 and SDL2, and 102448 bytes on a default build.

Most libraries are under permissive licences: libffi and pixman under MIT licences, FreeType under the FreeType License, zlib under the zlib licence, libpng under its own, SDL2 under the zlib licence, with a BSD licence for its YUV conversions, and the http-parser that libgit2 bundles under an MIT licence.
Two components are under the LGPL or the MPL:

- cairo, available under the GNU LGPL 2.1 or the MPL 1.1; the notices give its `COPYING`, `COPYING-LGPL-2.1` and `COPYING-MPL-1.1`.
- LibXDiff, which libgit2 includes (its `src/xdiff`), under the LGPL 2.1 or later; libgit2 itself is under the GPL 2 with the linking exception of its `COPYING`, which also holds the text of the LGPL 2.1, and the notice of LibXDiff's sources is added to it (`xdiff-NOTICE.txt`).

Their sources, as those of every library, are those of the pinned archives that the notices name, with their SHA256, built as `cmake/emscripten/deps` of this repository says, and are available on request from whoever distributed the build.

## Troubleshooting

- `... lacks the Emscripten hooks of the Slang sources ... it was generated by a VMMaker image older than this tree`: the sources given (`WASM_GENERATED`, or `GENERATED_SOURCE_DIR` on the CMake-only route) predate this tree's `smalltalksrc`.
  Run `make wasm` without `WASM_GENERATED`, which refreshes the VMMaker image and generates the sources again.
  Sources generated by an older version of this tree may have the hooks but not every later change: lane 25-session-id, for one, fails on sources generated before the session-ID fix.
- `emcmake (...) not found: source emsdk_env.sh of the Emscripten SDK ..., or set EMCMAKE`: the shell has not sourced `emsdk_env.sh`.
- `CMake Error: The source directory ".../cmake/Emscripten.cache.cmake" is a file, not a directory.`, or `The source directory ".../host" does not exist.`: the CMake is older than 3.13, which `-S` and `-B` need.
- `download.cmake: cannot download ...`, or a VMMaker bootstrap that cannot reach the network: build offline (see Building).
- `Cannot download <library> <version> (<file>) from <url>: ...; to build offline, put <file> in a directory and give it as WASM_DEPS_DIR`: the archive of a library could not be downloaded; get it elsewhere, check its SHA256, and give its directory as `WASM_DEPS_DIR`.
- `<archive> is not <library> <version> (<file>): its SHA256 is ..., not ...`: the file of that name in `WASM_DEPS_DIR`, or the one downloaded, is not the pinned archive; put the pinned one there, or remove it from `WASM_DEPS_DIR` to download it.
- `WASM_DEPS_DIR: ... does not exist`: give an existing directory, or leave the setting out.
- `WASM_CAIRO=ON needs FreeType, which WASM_FREETYPE=OFF leaves out`: give `WASM_CAIRO=AUTO` or `OFF`, or keep FreeType.
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
- The world page says that the build has no world image: the build had `WASM_WORLD=OFF`, or no `WASM_HOST_PHARO`, as with `WASM_GENERATED` alone (CMake said `WASM_HOST_PHARO is not set: the world image is not prepared, and web/ gets the stock image`).
- `FFI callout failed, its declaration does not match function N: ...` or `FFI callout trapped in function N: ...` on stderr (the worker's console in a page), and a primitive failure: the image declares that function otherwise than C does, in a way that the adapter does not adapt; `PHARO_WASM_FFI_TRACE=1` in the node VM shows the calls it does adapt.
- A `SymbolNotFoundError` for a function of a library: the function is in no list of the registry, or the build has not that library (`manifest.json` lists the libraries of the site, and the configuration prints `wasm deps: ffi ON, freetype ON, cairo ON (auto), ...`).
- Iceberg says `There is no libgit2 available in your system!`: the build has no libgit2; build with `WASM_LIBGIT2=ON`.
  A clone says `git http: request failed (CORS or network)`: the remote sends no CORS headers; give a CORS proxy in the Console's Settings.
- With `WASM_DEBUG=1`, the time checks of the lanes (S6b, S8 and the bench) and the checks of an empty stderr (S2, S13) fail: the VM is many times slower with its asserts (lanes 20, 30, 40 and 80 alone took 17 minutes, and failed so), and Emscripten's assertions print "Heap resize call ..." and "program exited (with status: 0), but keepRuntimeAlive() is set ..." when the image quits.
  In lane 80, the world harness fails its keystroke median, and its cases that wait for the world (Stop, the save) can time out.
- `run-lanes: waiting for the run N ...`: another run uses that `TEST_DIR`; if it is gone, remove the lock that the message names.
- An `OutOfMemory` for a large object, and the VM logs `Cannot allocate N bytes of old space at P ...: old space must fit in [base, 2*base)`: raise the old-space base, for example with `PHARO_WASM_OLD_SPACE_BASE=0x40000000` (1 GiB of old space).
- `Cannot allocate N bytes of perm space at 0x20000000000: WebAssembly memory cannot reach it (perm space is not supported)`: the image has perm space, or the VM was given `--minPermSpaceSize`; neither works in WebAssembly.
- A crash prints at most 1000 frames per process: pass `--maxFramesToLog=<n>` for more, or `--maxFramesToLog=0` for all of them.
