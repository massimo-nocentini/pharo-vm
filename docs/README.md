# Pharo 15 on WebAssembly

The Pharo VM built for WebAssembly, from branch `pharo-12-wasm` at commit
`61f5d0ef5` of pharo-vm, with a Pharo 15 image: Pharo15.0-SNAPSHOT, build 41
(`4e572fed79`, from https://files.pharo.org/image/150/latest-64.zip as of
1 October 2026), prepared for the world page.  The VMs have the FFI
(libffi), FreeType, cairo, libgit2 and SDL2 built in.  `docs/WebAssembly.md`
in the repository says how it is made and how it works.

    web/            the static site: the Console page with its Notebook tab,
                    the world page and the world page through SDL2
    node/           the command-line VM for node (pharo, pharo.js, pharo.wasm,
                    and the empty files the image looks for as its libraries)
    serve.mjs       a static HTTP server for web/ (node, no dependencies)
    node-try.sh     runs the node VM on the image of web/

Where this folder is served, as GitHub Pages serves the `docs` folder of a
branch, the pages are [web/](web/) (the Console and the Notebook),
[web/world.html](web/world.html) (the Pharo world) and
[web/sdl.html](web/sdl.html) (the Pharo world through SDL2).

## In a browser

The pages must be served over HTTP (they say so when opened as files).
Serve `web/` with the included server, or with any static server:

    node serve.mjs web 8080
    # or: python3 -m http.server 8080 --directory web

then open:

- http://localhost:8080/ — the Console, a Smalltalk REPL.
  Try `3 + 4`, `SystemVersion current version`, `100 factorial`.
  Enter evaluates, Shift+Enter adds a line, Esc or Ctrl+C stops,
  Up/Down recall the history.
- The Notebook tab of the Console: cells of Smalltalk, run by a second VM
  of their own, and of Markdown.  Shift+Enter runs a cell and moves on,
  Ctrl+Enter runs it in place, Run all runs them all; More has Stop
  background processes, Upload files (into `/pharo`), Import and Export
  (`.st`, `.json`, or a static `.html` page), and the example notebook.
  A variable that a cell assigns is kept for the next cells; `Notebook
  show:`, `html:`, `svg:`, `markdown:`, `table:` and `image:` show rich
  outputs, and a Form, a Morph or a Roassal canvas shows as an image.
  The notebook is saved in the browser as you type.
- http://localhost:8080/world.html — the Pharo world, the Morphic desktop
  in a canvas, its text drawn by FreeType.  Open a Playground (Browse >
  Playground), type `3 + 4` and press Ctrl+P (Cmd+P on macOS).  Alt+. or
  the Stop button interrupts a busy evaluation and opens a debugger on it.
  Roassal and the other Athens views draw through cairo.
- http://localhost:8080/sdl.html — the same world drawn as the desktop VMs
  draw it, through OSSDL2Driver and SDL2; it runs stock images too,
  unprepared, but keeps nothing in the browser and costs several times
  the idle time of the world page.

### Git

Iceberg works in the VMs, on libgit2: repositories in the memory of the
VM, and clones of http:// and https:// remotes.  Git servers, github.com
among them, send no CORS headers, so a page reads them only through a CORS
proxy, which the Console's Settings hold (kept in this browser, and used by
the world pages and the Notebook too); there is none by default, and it is
never taken from a URL, since the proxy sees the code and any credentials
that pass through it.  The working copies are lost on a reload: only the
image is kept in the browser.

### Your own image

The Console, the world page and sdl.html have an Open button, and take a
drop of files anywhere on the page:

- a Pharo zip as files.pharo.org gives them, as it is, for example
  https://files.pharo.org/image/150/latest-64.zip (Pharo 15) or
  https://files.pharo.org/image/120/latest-64.zip (Pharo 12);
- or an `.image` with its `.changes`, and its `.sources`.

The page unpacks the zip in the browser, uses the image's own `.sources`,
and keeps the image (with that `.sources`) in the browser once it has
started.  On the world page, a stock image is first prepared for the
browser (the OSWindow-Web package is filed in, with its FreeType fonts,
and the image saved, in a few seconds), and then its world opens.  Pharo 12 and
Pharo 15 images were tested; Pharo 13 and 14 were not.

The first visit downloads about 26 MB (the image and the .sources,
gzipped, and the VM).  Save keeps the image in the browser (IndexedDB),
and the next visit, on the Console or the world page, boots it (the
Notebook boots it too, read-only); Download gives the image and its
.changes; Reset (Console) goes back to the image of the site.

No special headers are needed (no SharedArrayBuffer, no COOP/COEP): the
VM runs in a Web Worker and gives the browser its thread back between
slices.

Browsers: Chrome or Edge 137, Firefox 131, or later (tested with
Chromium 153 and Firefox 155).  Safari 18.4 or later should work but is
untested.

## With node

node 24.15 or later.  `node-try.sh` unpacks the image of `web/` into
`try/` the first time, then runs the VM on it with the arguments given:

    ./node-try.sh eval '3 + 4'
    ./node-try.sh eval 'SystemVersion current version'
    ./node-try.sh st --quit script.st

`node/pharo` takes the usual arguments of the VM on any Pharo 64-bit
image:

    node/pharo --headless path/to/Pharo.image --no-default-preferences eval '3 + 4'

## Limits

- The interpreter only (no JIT): about 5 to 6 times slower than the
  native StackVM, 14 to 27 times slower than the native JIT.
- The FFI reaches only the libraries and functions built into the VM
  (libffi, FreeType, cairo, libgit2, SDL2 and the C library); no
  name resolution, fork, OSProcess, SSL, SSH or threads.
- Old space is at most 512 MiB; the Notebook's VM takes as much memory
  again as the Console's.
- A primitive that runs for long does not yield: Stop then restarts the
  worker after 3 s, losing what was not saved.
- Notebook cells have no stdin.
- The canvas has one pixel per CSS pixel, so it is blurred on HiDPI
  screens.

`web/THIRD-PARTY-NOTICES.txt` gives the licences of the libraries in the
VMs; cairo (LGPL 2.1 or MPL 1.1) and LibXDiff of libgit2 (LGPL 2.1 or
later) are among them, and the sources of every library are those of the
pinned archive that it names.
