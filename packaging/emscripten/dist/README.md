# Pharo 13, 14 and 15 on WebAssembly

The Pharo VM built for WebAssembly, from branch `pharo-12` at commit
`@PHAROVM_COMMIT@` of pharo-vm, with three images from files.pharo.org,
each prepared for the world page:

- Pharo 15 (the default): Pharo15.0-SNAPSHOT, build 50 (`b85960fcc6`);
- Pharo 14: Pharo14.0-SNAPSHOT, build 785 (`74f3a1b398`);
- Pharo 13.1: Pharo13.0-SNAPSHOT, build 749 (`d7c6f761d5`, the stable
  Pharo 13).

The site also serves the `.sources` of Pharo 15 build 41 (`4e572fed79`),
its image before, for the images saved in a browser from that one.  The
VMs have the FFI (libffi), FreeType, cairo and libgit2 built in.
`docs/WebAssembly.md` in the repository says how it is made and how it
works.

    web/            the static site: the Console page with its Notebook tab,
                    and the world page
    node/           the command-line VM for node (pharo, pharo.js, pharo.wasm,
                    and the empty files the image looks for as its libraries)
    serve.mjs       a static HTTP server for web/ (node, no dependencies)
    node-try.sh     runs the node VM on the default image of web/

GitHub Pages serves the site of the latest build of `pharo-12` from
the root of branch `gh-pages`; where this folder is served, the pages are
[web/](web/) (the Console and the Notebook) and
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

### Images

The first visit to the world page asks which image to start: Pharo 15,
14 or 13.1, each a download of its own (about 25 MB gzipped, with its
.sources).  The Images button (on the Console and on the world page)
starts another one fresh, and lists the images saved in this browser:
open, rename or delete each one, or open it on the other page.  Save
keeps the running image in the browser (IndexedDB) under its name, Save
as keeps it as a new one, and the next visit boots the image saved or
used last; `?slot=<id>` and `?fresh=<id>` in the address of a page name
the image (only among the user's own images and the site's).  Each saved
image keeps the files of the image's directory as they were at its last
save too: Iceberg's clones (with their `.git`), the package cache and the
logs.  A saved image takes about 60 MB of the browser's storage, more with
its clones; the Images dialog shows what this browser uses, and offers to
keep it safe from the browser's cleanup.

### Git

Iceberg works in the VMs, on libgit2: repositories in the memory of the
VM, and clones, fetches, pulls and pushes of http:// and https:// remotes.
Git servers, github.com among them, send no CORS headers, so a page reads
them only through a CORS proxy.  By default it is the public proxy
https://cors.isomorphic-git.org/, which sees what you clone; the Settings of
the Console and of the world page (kept in this browser, and used by every
page and the Notebook) choose the site's default, none (requests go to the
git host itself, which works only for hosts that let the page in), or a
proxy of your own.  The setting is never taken from a URL.

A push (or a private repository) needs a user name and a token, which the
VM sends only through a proxy you chose (My proxy, or None): never through
the site's default, which would see them.  When a host asks for them there,
the page says so and offers to choose that proxy for them.  In the world,
Iceberg asks for the token once the push has failed, then push again; in
the Console and the Notebook, store it first:

    IceCredentialStore current
        storeCredential: (IcePlaintextCredentials new username: 'me'; password: '<token>'; yourself)
        forHostname: 'github.com'

Iceberg keeps stored tokens in plain text, in the image and in
pharo-local/iceberg/credentials.fuel, which a Save keeps in the browser and
a Download carries: use a token limited to the repository, which expires.
Do not put the token in the URL of a remote (https://me:token@...): Iceberg
writes that URL into the clone's .git/config, the image and debug logs.  A
proxy can read the token, and a host that redirects a push to another host
through it makes the browser send it there too: the VM then stops, and says
to revoke the token.

A pull whose merge has conflicts is resolved in Iceberg's dialog, in the
world only: in the Console and the Notebook it fails at once
(IceMergeAborted), with nothing merged.

### Your own image

The Console and the world page have an Open button, and take a drop of
files anywhere on the page:

- a Pharo zip as files.pharo.org gives them, as it is, for example
  https://files.pharo.org/image/150/latest-64.zip (Pharo 15) or
  https://files.pharo.org/image/120/latest-64.zip (Pharo 12);
- or an `.image` with its `.changes`, and its `.sources`.

The page unpacks the zip in the browser, uses the image's own `.sources`,
and keeps the image (with that `.sources`) in the browser, as a new saved
image, once it has started: it never replaces one.  On the world page, a
stock image is first prepared for the browser (the OSWindow-Web package is
filed in, with its FreeType fonts, and the image saved, in a few seconds),
and then its world opens.  Pharo 12 to Pharo 15 images were tested.

The first visit downloads about 26 MB (the image and the .sources,
gzipped, and the VM).  The Notebook boots the image of the Console,
read-only; Download gives the image and its .changes; Delete (Console)
removes the image saved in this browser and goes back to the image of the
site.

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
  (libffi, FreeType, cairo, libgit2 and the C library); no
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
