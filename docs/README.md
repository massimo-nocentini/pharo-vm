# Pharo 15 on WebAssembly

The Pharo VM built for WebAssembly, from branch `pharo-12-wasm` at commit
`2a85a1e5b` of pharo-vm, with a Pharo 15 image: Pharo15.0-SNAPSHOT, build 41
(`4e572fed79`, from https://files.pharo.org/image/150/latest-64.zip as of
1 October 2026), prepared for the world page.  `docs/WebAssembly.md` in the
repository says how it is made and how it works.

    web/            the static site: the Console page and the world page
    node/           the command-line VM for node (pharo, pharo.js, pharo.wasm)
    serve.mjs       a static HTTP server for web/ (node, no dependencies)
    node-try.sh     runs the node VM on the image of web/

Where this folder is served, as GitHub Pages serves the `docs` folder of a
branch, the pages are [web/](web/) (the Console) and
[web/world.html](web/world.html) (the Pharo world).

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
- http://localhost:8080/world.html — the Pharo world, the Morphic desktop
  in a canvas.  Open a Playground (Browse > Playground), type `3 + 4` and
  press Ctrl+P (Cmd+P on macOS).  Alt+. or the Stop button interrupts a
  busy evaluation and opens a debugger on it.

### Your own image

Both pages have an Open button, and take a drop of files anywhere on
the page:

- a Pharo zip as files.pharo.org gives them, as it is, for example
  https://files.pharo.org/image/150/latest-64.zip (Pharo 15) or
  https://files.pharo.org/image/120/latest-64.zip (Pharo 12);
- or an `.image` with its `.changes`, and its `.sources`.

The page unpacks the zip in the browser, uses the image's own `.sources`,
and keeps the image (with that `.sources`) in the browser once it has
started.  On the world page, a stock image is first prepared for the
browser (the OSWindow-Web package and bitmap fonts are filed in and the
image saved, in a few seconds), and then its world opens.  Pharo 12 and
Pharo 15 images were tested; Pharo 13 and 14 were not.

The first visit downloads about 25 MB (the image and the .sources,
gzipped).  Save keeps the image in the browser (IndexedDB), and the next
visit, on either page, boots it; Download gives the image and its
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

- The interpreter only (no JIT): about 5 times slower than the native
  StackVM, 13 to 25 times slower than the native JIT.
- No FFI, so no FreeType (the world uses bitmap fonts), Cairo or libgit2
  (Iceberg); no name resolution, fork, OSProcess, SSL or threads.
- Old space is at most 512 MiB.
- A primitive that runs for long does not yield: Stop then restarts the
  worker after 3 s, losing what was not saved.
- The canvas has one pixel per CSS pixel, so it is blurred on HiDPI
  screens.
