# Pharo, batteries included

This image is the stock Pharo 15 with the projects below. They go with a VM that adds plugins, libraries and platforms to the one of [pharo-project](https://github.com/pharo-project/pharo-vm). This page lists what is ours, over what upstream has. Roll the wheel to scroll, drag to move.

- [The image](#the-image)
- [The VM](#the-vm)
- [The web](#the-web)

Open this page again with **Help > Batteries**.

## The image

### Drawing

| Project | What it adds |
|:---|:---|
| [Roassal](https://github.com/massimo-nocentini/Roassal) | The fork of pharo-graphics: de Casteljau and parametric lines, notes, UML sequence diagrams, random variables, faster hit tests, and fixes of repaints that skipped shapes in view |
| [RTree](https://github.com/massimo-nocentini/RTree) | Fixes and faster updates of the R-tree, and n-ary, packed, grid and loose quadtree collections |
| [RoassalLayouts](https://github.com/massimo-nocentini/RoassalLayouts) | Layouts of trees after Van der Ploeg, and fixes of the tree, graph and force layouts |
| [CMark](https://github.com/massimo-nocentini/cmark-gfm.st) | Markdown: cmark-gfm's parse trees, their Roassal drawing (this page), and a Markdown tab of class comments in Calypso |
| [LibPangoCairo](https://github.com/massimo-nocentini/libpangocairo.st) | GLib, Pango and PangoCairo: text layout in any script, as Roassal shapes too |
| [LibPoppler](https://github.com/massimo-nocentini/libpoppler.st) | PDF documents: their text, and their pages as Forms and Roassal shapes |
| [MaterialIcons](https://github.com/massimo-nocentini/material-icons.st) | The Material Symbols, the icons of this image, and symbols of the method protocols |

### Data

| Project | What it adds |
|:---|:---|
| [SimdJSON](https://github.com/massimo-nocentini/simdjson.st) | JSON read by simdjson and written, with the API of NeoJSON |
| [SimdCSV](https://github.com/massimo-nocentini/simdcsv.st) | CSV read and written, with the protocol of NeoCSV |
| [SimdUTF](https://github.com/massimo-nocentini/simdutf.st) | UTF-8, UTF-16, UTF-32 and Latin-1 transcoding and validation, and base64 |
| [TimSort](https://github.com/massimo-nocentini/timsort.st) | CPython's stable, adaptive sort |
| [VM plugins' primitives](https://github.com/massimo-nocentini/vm-plugins-primitives.st) | Fused multiply-add, strtok, de Casteljau, and the generators of Knuth and Vigna |

### Language

| Project | What it adds |
|:---|:---|
| [Delimcc](https://github.com/massimo-nocentini/delimcc.st) | Multi-prompt delimited continuations, and fixes of Pharo's Continuation |
| [Containers-Essentials](https://github.com/massimo-nocentini/Containers-Essentials), [Containers-Category](https://github.com/massimo-nocentini/Containers-Category) | The collections that Delimcc builds on |

The tests of the projects are in the image too. Some things to evaluate:

```smalltalk
(CMarkRoassalRenderer new canvasFor: (CMark parse: '# Hello *world*')) open.
SimdJSONReader fromString: '{"answer": [42, true, null]}'.
SimdCSV rowsFrom: (String lf join: { 'a,b' . '1,2' }).
#( 3 1 2 ) copy timSort.
```

## The VM

### New plugins

- **SimdJSONPlugin**, **SimdUTFPlugin**, **SimdCSVPlugin**, **CMarkPlugin**, **TimSortPlugin** and **UtilsPlugin**, behind the projects above
- **CairoPlugin**: Athens' calls of cairo as primitives (its image side is opt-in)
- **WebDisplayPlugin** and **WebHostPlugin**, for the browser

### Bundled libraries

- cairo, FreeType and their libraries, from pinned sources and packages checked by SHA256, instead of old prebuilt binaries
- Pango and PangoCairo, HarfBuzz and FriBidi, and Poppler

### Fixes and speed

- LargeIntegers: squaring, Montgomery multiplication and Burnikel-Ziegler division
- A review of the C plugins: bounds, leaks and failures of BitBlt, sockets, files, JPEG, SSL, OSProcess, DSA and others; lookups of names no longer freeze the image
- FFI queues, semaphores, `aio` and the lookup of primitives hardened

### Builds

- Zips of Linux, macOS and Windows, Intel and ARM, built with clang by one workflow
- This image, built, checked and released with them

## The web

- Pharo in the browser, by WebAssembly, with no server behind: the **world**, a **Console** (a REPL) and a **Notebook** of Smalltalk and Markdown cells
- Images of Pharo 15, 14 and 13.1, and this one, or one of your own; each is kept in the browser when it is saved
- The FFI, cairo, FreeType, Pango, Poppler and libgit2; Iceberg clones, pulls and pushes over HTTP
- An interpreter written by hand, in front of the generated one, that makes it faster

The documents are in [docs/](https://github.com/massimo-nocentini/pharo-vm/tree/pharo-12/docs) of the VM.
