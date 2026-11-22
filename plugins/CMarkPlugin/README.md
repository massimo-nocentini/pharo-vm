# CMarkPlugin

The CMarkPlugin parses Markdown for Pharo 12 to 15 with [cmark-gfm](https://github.com/github/cmark-gfm) 0.29.0.gfm.14, GitHub's fork of the CommonMark reference parser, in C:

- **parse trees**: CommonMark with the GFM extensions table, strikethrough, autolink, tagfilter and tasklist (chosen per call), and footnotes as an option, answered as a flat tape of node records (a Bitmap of 16 words per node and a ByteArray of their strings) that the image decodes into a tree of `CMarkNode`s, with source positions in characters;
- **rendering** by cmark-gfm itself to HTML, XML, CommonMark, plain text, LaTeX or man;
- **limits instead of aborts**: every C allocation of a call is tracked and limited (a C budget, a size per allocation, a depth), and an object-memory budget makes the image grow its memory before the answer is allocated, so that no input aborts the VM.

cmark-gfm is fetched as its pinned tag archive (SHA256 checked) and compiled with our flags (`cmake/cmark.cmake`), with no patch. The plugin is ON on every 64-bit target, and built in on WebAssembly.

**Layout:**
- `src/common/CMarkPlugin.c`: the glue, the only file that includes the VM's headers.
- `src/engine/`, `include/common/cmEngine.h`: the engine (the parse and its walk into the tape, the text and its indices, the tracking allocator).
- `cmark-config/`: the headers that cmark-gfm's CMake would generate, written by hand (`private/config.h` is seen by cmark-gfm only).
- `tests/`: the C tests (`run-c-tests.sh`), the Smalltalk loader and runner (`load-cmark.st`, `run-st-tests.sh`), and the CI probe (`ci-load.st`).

[docs/CMark.md](../../docs/CMark.md) gives the build option `FEATURE_PLUGIN_CMARK`, the API, the tree format, the primitive contract, the memory limits, the tests and the known limitations.

**The Smalltalk code** is the git submodule `cmark-gfm.st` ([massimo-nocentini/cmark-gfm.st](https://github.com/massimo-nocentini/cmark-gfm.st)); fetch it with `git submodule update --init plugins/CMarkPlugin/cmark-gfm.st`.
- It holds the Tonel packages `CMark` (the tree, its visitor, the decoder, parsing and rendering through the plugin) and `CMark-Roassal` (a document drawn as one Roassal `RSComposite`), with their tests `CMark-Tests` and `CMark-Roassal-Tests`, which Metacello loads with `BaselineOfCMark`.
- Without the plugin, the tree classes, hand-built trees and their Roassal view still work; parsing and rendering signal `CMarkUnavailable`.
- `tests/load-cmark.st` loads the packages from the command line.

**Licences:** the plugin and the packages are under the MIT licence. cmark-gfm is under the BSD-2-Clause licence, with parts under the MIT licence (houdini, the buffer, the utf8proc tables), all in its `COPYING`, which the build installs as `LICENSE.cmark-gfm` next to the plugin (in `Contents/Resources` on macOS) and puts in `THIRD-PARTY-NOTICES.txt` on WebAssembly.
