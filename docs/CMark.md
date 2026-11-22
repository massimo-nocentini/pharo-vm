# CMark: Markdown parse trees on cmark-gfm

The CMarkPlugin parses Markdown with [cmark-gfm](https://github.com/github/cmark-gfm) 0.29.0.gfm.14, GitHub's fork of the CommonMark reference parser, and the package `CMark` (the submodule `plugins/CMarkPlugin/cmark-gfm.st`) gives Pharo 12 to 15 the parse tree as Smalltalk objects that can be walked, and cmark-gfm's own renderers; the package `CMark-Roassal` draws a document as one composite Roassal shape.
The plugin answers a parse as a flat **tape** of node records (a Bitmap of 16 words per node, and a ByteArray of the strings of the tree), which the image decodes into a tree of `CMarkNode`s with exact source positions, in characters as well as in cmark's lines and byte columns (see Tree format).
It is CommonMark with the GFM extensions table, strikethrough, autolink, tagfilter and tasklist, chosen per call, and footnotes as a parse option.
No input aborts the VM: every C allocation of a call is tracked and limited, and the limits are answers, not failures (see Memory and limits).
The API is explicit: nothing of Pharo is overridden, and the only extension methods on Pharo's classes are `cmarkFailed` and `cmarkSource`.
There is no FFI library: the names `CMark-FFI`, `CMark-FFI-Tests`, `libCMarkFFI` and the groups `FFI` and `FFITests` are reserved for one.

Files, in `plugins/CMarkPlugin`:

- `src/common/CMarkPlugin.c`: the module functions and the four primitives (C, with the VM's headers); the only file of `src/common`;
- `src/engine/cmEngine.c`: the engine, the only file that includes cmark-gfm's headers: the registration of the extensions, the parse, the walk of the tree into the tape, the render and the kept result; `src/engine/cmText.c`: the sanitising of the source into UTF-8, the line table, the checkpoints and the character indices; `src/engine/cmTrack.c`: the tracking allocator, the context of a call and its one `setjmp` (none of them includes a VM header);
- `include/common/cmEngine.h`: the interface between the glue and the engine (no VM and no cmark-gfm type);
- `cmark-config/`: the headers that cmark-gfm's CMake would generate, written by hand: `private/config.h`, which only cmark-gfm's own files see (the VM's headers include a `config.h` of their own), and `public/cmark-gfm_export.h` and `public/cmark-gfm_version.h`;
- `cmark-gfm.st/`: the Smalltalk side, a git submodule of [cmark-gfm.st](https://github.com/massimo-nocentini/cmark-gfm.st) (`git submodule update --init plugins/CMarkPlugin/cmark-gfm.st`): Tonel packages in `src/` with `BaselineOfCMark`, whose commit is the version of the Smalltalk code that goes with the plugin, and whose `README.md` is the guide of the API;
- `tests/`: the C tests (`run-c-tests.sh`, `enginetest.c`, `gluetest.c`, `fakevm.c`, `fixtures/`), the Smalltalk loader and runner (`load-cmark.st`, `run-st-tests.sh`) and the probe of the CI jobs (`ci-load.st`).

## Building

The plugin is on by default on every 64-bit target: `FEATURE_PLUGIN_CMARK=ON`.
It is off on 32-bit targets, where `-DFEATURE_PLUGIN_CMARK=ON` stops the configuration: the answers carry SmallIntegers above 2^30 (byte sizes, budgets, the statistics).
`-DFEATURE_PLUGIN_CMARK=OFF` leaves it out, along with the archive of cmark-gfm.

    cmake -S . -B build -DFLAVOUR=CoInterpreter            # the plugin is in
    cmake -S . -B build -DFEATURE_PLUGIN_CMARK=OFF         # it is not

cmark-gfm is taken from the archive of its tag, `https://github.com/github/cmark-gfm/archive/refs/tags/0.29.0.gfm.14.tar.gz` (300,120 bytes, as `cmark-gfm-0.29.0.gfm.14.tar.gz`), checked against its SHA256 (`1c1acf50…b7ba11fe`), by `cmake/cmark.cmake` with the function of the WebAssembly libraries (`cmake/emscripten/deps/fetch.cmake`), natively too, as simdjson, simdutf and timsort.c are:

- the archive comes from `WASM_DEPS_DIR` when that directory holds `cmark-gfm-0.29.0.gfm.14.tar.gz`, which builds offline, else from `<build>/downloads`, else from GitHub at configure time;
- `-DCMARK_SOURCE_DIR=<tree>` takes a cmark-gfm source tree instead, an extraction of that archive or a clone of the tag.

Either way the tree must be the pin: it must have `src/cmark-gfm_version.h.in`, and its top `CMakeLists.txt` the lines `set(PROJECT_VERSION_MINOR 29)` and `set(PROJECT_VERSION_GFM 14)`, else the configuration stops.
No patch is applied: the known bugs of gfm.14 are documented (see Limitations) and pinned by the C tests, so that a later patch shows up as an intended change of the tests.

cmark-gfm's own CMake is not used: the 27 files of its library (`src/`, without the command-line program `main.c`) and the 7 of its extensions (`extensions/`) are compiled as the static library `pharo_cmark`, in `<build>/deps` (never in `build/vm`, which is packaged whole), in C99 (`C_STANDARD 99`, `C_EXTENSIONS OFF`, upstream's `-std=c99`), with `-O3 -g0 -DNDEBUG` (upstream's Release; `CMARK_DEBUG_NODES`, which aborts on a failed check of the tree, is never defined), `CMARK_GFM_STATIC_DEFINE` and `CMARK_NO_SHORT_NAMES`, hidden visibility, and position-independent code natively.
`scanners.c` and `ext_scanners.c` ship generated by re2c, so re2c is not needed.
The headers cmark-gfm's CMake would generate are in `plugins/CMarkPlugin/cmark-config`: `private/config.h` is a private include directory of `pharo_cmark`, so that the glue's `#include "config.h"`, through the VM's headers, always finds the VM's own; `public/` (the export macros, empty for a static copy, and the version, `0x001d000e`, `"0.29.0.gfm.14"`) is public with cmark-gfm's `src/` and `extensions/`, which the engine includes.
The VM's build adds no `-pedantic -Werror` to cmark-gfm; the C tests build it with them (see Tests).
The plugin links its own copy of cmark-gfm with hidden visibility, and its engine functions are hidden too (`CM_API`), so that the VM's `dlopen` with `RTLD_GLOBAL|RTLD_DEEPBIND` (`src/externalPrimitives.c`) never mixes them with another copy: `libCMarkPlugin.so` exports nothing but its module functions, its four primitives and their accessor depths.
Natively `libCMarkPlugin.so` is 430 KB (440040 bytes; the glue and engine keep `-g`, cmark-gfm is `-g0`; 343 KB stripped), on Linux x86_64 with clang 18.1.3.

Platforms:

- Linux x86_64 and Emscripten (the node and web VMs of `make wasm`) are built and tested here;
- macOS (x86_64 and arm64), Windows (MSYS2 UCRT64 with clang on x86_64, CLANGARM64 on arm64) and Linux aarch64 are written for, with fixed-width types only (`long` is 32 bits on Windows) and ISO `setjmp`/`longjmp`, but **untested** until CI runs.
  Each native job of `.github/workflows/build.yml` loads the plugin of its zip with `plugins/CMarkPlugin/tests/ci-load.st`, which checks known parse trees and HTML and then the limits, so that every native target runs the `setjmp`/`longjmp` landing of the engine at least once, and checks that `LICENSE.cmark-gfm` is in the zip; the Linux and macOS steps run under `shell: bash`, whose `pipefail` makes a VM that dies after printing fail the step.
  Build with `-DFEATURE_PLUGIN_CMARK=OFF` if the build breaks there.

In the WebAssembly VMs the plugin is built in (`CMarkPlugin (i)`): cmark-gfm is C, so it brings no C++ runtime, and its C flags carry the VM's `-m64` and setjmp/longjmp flags already; its `COPYING` goes into `THIRD-PARTY-NOTICES.txt` (see Licences and docs/WebAssembly.md).
Its defaults are smaller there, since the C heap grows into the window of old space and never gives it back (see Memory and limits).
The CMarkPlugin adds 267 KB to `pharo-web.wasm` (273,814 bytes; 81,072 with `gzip -9`), and 264 KB to `pharo.wasm` (269,824 bytes; 82,013 with `gzip -9`), measured against the same build with `-DFEATURE_PLUGIN_CMARK=OFF` (Emscripten 6.0.10, `-O3`, 2026-11-22).
All builtin plugins share one module, so cmark-gfm's global symbols that have no `cmark_` prefix (`houdini_*`, `_scan_*`, `_ext_scan_at`, `create_*_extension`, `normalize_map_label`) must not clash with another library's; a clean link does not prove it (wasm-ld takes an archive member only for an undefined symbol), so lane 75 compares the defined symbols of every archive and object linked into `pharo.wasm` and `pharo-web.wasm` (none is defined twice, and none of cmark-gfm's is defined by Emscripten's own libraries either, checked by hand).
The native tree that `make wasm` generates the sources in builds no plugin, but no `HOST_PLUGIN_FLAGS` entry turns the CMarkPlugin off there (as for the TimSortPlugin), so its configure includes `cmake/cmark.cmake` and needs the archive (from the `WASM_DEPS_DIR` of the host or from GitHub) for nothing it builds; an offline host tree without it stops at its configure, where `cmake -DFEATURE_PLUGIN_CMARK=OFF build-wasm/host` turns it off.

## Loading the Smalltalk side

The Smalltalk code is in the submodule `cmark-gfm.st`, as Tonel packages that Metacello loads with `BaselineOfCMark`.
Its groups: `Core` (the package `CMark`: the tree, its visitor, the decoder of the tape, the options, the errors, and parsing and rendering through the plugin; it needs no other project), `Tests` (`CMark-Tests`), `Roassal` (`Core` and `CMark-Roassal`, the renderer, which uses only the Roassal of the stock images), `RoassalTests` (`Roassal` and `CMark-Roassal-Tests`), `RoassalFork` (the user's fork of Roassal, branch Pharo15, then `Roassal`; opt-in, since no package requires it) and `default` (`Core`, `Tests`, `Roassal` and `RoassalTests`).
`CMark` and `CMark-Tests` run on Pharo 12, 13, 14 and 15; `CMark-Roassal` uses only the API of Roassal that stock Pharo 12 and 15 both have.

    Metacello new
        baseline: 'CMark';
        repository: 'tonel://<repository>/plugins/CMarkPlugin/cmark-gfm.st/src';
        load: #( 'Core' 'Roassal' ).

or, from the command line, `tests/load-cmark.st`, which the tests and lane 75 use:

    CMARK_PLUGIN_DIR=plugins/CMarkPlugin CMARK_GROUPS='Core Tests' \
    pharo --headless Pharo.image --no-default-preferences st --save --quit plugins/CMarkPlugin/tests/load-cmark.st

It loads the groups of `CMARK_GROUPS` (`Core Tests` by default) and prints `CMark: loaded, protocol 1, plugin CMarkPlugin 1.0 cmark-gfm 0.29.0.gfm.14 <platform>`, or `CMark: loaded, no plugin`, then the groups and packages loaded, and `CMark: test packages ...`.
Without `CMARK_PLUGIN_DIR` it takes the directory above its own when the command line names it.
It exits with 1, before the image is saved, when the submodule is not checked out, a load fails, `CMarkPrimitives protocol` is not 1, `CMARK_EXPECT_PLUGIN` is `yes` and the image finds no plugin (or `'# Hi'` does not parse into a heading of level 1), it is `no` and the image finds one, a method of a CMark package uses an undeclared variable, or an extension method of a CMark package, on a class that no CMark package defines, has a selector that does not begin with `cmark`.
Metacello's warnings are printed and resumed, except `MCMergeOrLoadWarning`, which loading `RoassalFork` over the Roassal packages of a stock image signals, and which is answered with load (a plain resume would keep the image's methods).
Metacello makes UUIDs: the VM needs its UUIDPlugin.
To change the Smalltalk code, commit in the submodule, push it to cmark-gfm.st, and commit the new submodule commit here, with the plugin changes it goes with.

## API

The `README.md` of the submodule has the examples; this is the summary.

### Parsing and rendering

- `CMark` (class side only), the facade: `parse: aStringOrByteArray` (with `CMarkOptions gfm`), `parse:options:`, `html: aString` (gfm), `render: aString format: aSymbol`, `render:format:options:`, `isAvailable`, `version` (the plugin's cmark-gfm version, or nil) and `statistics`; it delegates to the two classes below.
- `CMarkParser`: class side `parse:` and `parse:options:`; instance side `options`, `options:` and `parse:`, which answers a `CMarkDocument`.
  The source is a ByteString (Latin-1), a WideString, or a ByteArray of UTF-8; any other String (Symbols too) and a Text are converted with `asString` (`cmarkSource`, a dispatch on the source's class), and anything else signals `CMarkError`.
  When `options maxInputSize` is not nil and the source is larger (in characters, or bytes for a ByteArray), `CMarkInputTooLarge` is signalled before any call; then the plugin parses, under the budget loop (see Primitive contract, Budgets), and the decoder builds the tree; the statuses 2, 3 and 4 signal `CMarkInputTooLarge`, `CMarkTooDeep` and `CMarkMemoryLimit` (with its `reason`, `#budget`, `#block` or `#malloc`).
- `CMarkFormatter` renders *source text* through cmark-gfm's renderers (so that it is not confused with `CMarkRoassalRenderer`, which renders *trees*): class side `html: aString` (gfm), `render: aString format: aSymbol` (`#html #xml #commonmark #plaintext #latex #man`), `render:format:options:` and `renderBytes:format:options:` (a ByteArray of UTF-8); instance side `options:`, `format:`, `render:` and `renderBytes:`.
  An unknown format is a `CMarkError` before any call; the answer is decoded as the strings of the tree are (see Text, positions and indices).
  There is no Smalltalk CommonMark writer: a tree built or changed by hand cannot reach cmark-gfm's renderers.

### Options

`CMarkOptions` is a mutable builder (`copy` it before reusing it; it defines `=` and `hash`, so do not change one that is a key of a Dictionary).
Its instance variables, with the defaults of `new`: `extensions`, a Set of Symbols (empty); the Booleans `sourcePos` (rendering only: `data-sourcepos` in the output), `hardBreaks`, `noBreaks`, `smart`, `githubPreLang`, `liberalHtmlTag`, `footnotes`, `strikethroughDoubleTilde`, `tablePreferStyleAttributes`, `fullInfoString` and `unsafe`, all false; `width` 0 (no wrapping); `maxDepth` 1000 (nil for no limit); `cBudget` nil (the plugin's default); `maxInputSize` 32 Mi natively and 16 Mi on WebAssembly (`CMarkPrimitives defaultMaxInputSize`; nil for only the plugin's own cap).
Class side: `commonMark` (= `new`) and `gfm` (the five extensions; footnotes stay off) answer fresh instances; `allExtensionNames` answers `#(#table #strikethrough #autolink #tagfilter #tasklist)`.
`optionBits` and `extensionBits` answer the bits of the primitives (see Arguments); the setters answer the receiver; `addExtension:` and `removeExtension:` signal `CMarkError` on an unknown name.

### The tree

The nodes are the classes of the code table (see Tree format), under the abstract `CMarkNode`, `CMarkBlockNode` and `CMarkInlineNode`.
Every node has `children` (an Array), `parent`, `document`, `isBlock`, `isInline`, `isLeaf`, `firstChild`, `lastChild`, `nextSibling`, `previousSibling`, `indexInParent`, `ancestors` (nearest first) and `depth` (the document has 1); the class side answers `typeCode` and `typeName` (cmark-gfm's type strings, `'heading'`…). The decoder maps a code to the class that defines `typeCode` itself, so a subclass of a node class, which inherits the code, does not change the trees of a parse.
Positions: `hasSourcePosition`, `startLine`, `startByteColumn`, `endLine`, `endByteColumn` (cmark-gfm's, whose columns count **bytes** of UTF-8), `startIndex` and `endIndex` (characters of the source), `sourceInterval` and `sourceText` (`document sourceString copyFrom: startIndex to: endIndex`, or nil; a block that ends at `L:0` includes the line terminator).
`plainText` (the literals of the texts, codes and inline HTML, a soft break as a space, a line break as LF), `do:` (pre-order), `allNodes`, `select:`, `detect:ifNone:`, `treeString` (one node per line, indented) and the decoder are iterative: a tree of any depth walks without recursion.
`printOn:` gives `a CMarkHeading(level 1, 1:1-1:15)`.
The attributes of each class are those of the code table; besides them: `CMarkDocument` `source`, `nodeCount`, `treeDepth` (the depth of its deepest node: `depth` is the navigation of every node, 1 for a document), `sourceString` (the source when it is a String; for a ByteArray, decoded from the sanitised bytes when the plugin answered them, so it never raises, and keeping every U+FEFF, which Pharo's `utf8Decoded` drops), `options`, `footnoteDefinitions`, `headings`, `links`; `CMarkList` `isOrdered`, `isBullet`, `isTight`; `CMarkItem` `markerNumber` (the number as written: `1. 1. 1.` gives 1, 1, 1) and `markerText`; `CMarkTaskItem` `isChecked`; `CMarkCodeBlock` `language` and `lines`; `CMarkHeading` `anchor` (GitHub's slug, `-1`, `-2`… for duplicates); `CMarkTable` `alignments` (`#left #center #right nil`), `headerRow`, `bodyRows`; `CMarkTableCell` `column`, `alignment`, `row`, `table`, `isAutocompleted` (a cell that cmark-gfm added to pad a short row: no children and no position); `CMarkLink` and `CMarkImage` `url` and `title` (`''` when absent), `hasTitle`, `isAnchor`, and `altText` for an image; `CMarkFootnoteDefinition` `label`, `ordinal`, `references`; `CMarkFootnoteReference` `ordinal`, `definition`, `referenceIndex` (the n-th reference of the tree to its definition, in source order: see Tree format for where it differs from cmark's `ref_ix`).
Trees can be built by hand, without the plugin: `CMarkNode class>>new`, `addChild:`, `addAll:`, `removeChild:`, `withAll:` (a call that signals `CMarkError`, for a node added to itself or its descendants, added twice, or removed from a node it is not a child of, changes nothing), and `CMarkText literal:`, `CMarkCode literal:`, `CMarkHeading level:with:`, `CMarkParagraph with:`, `CMarkList bullet` and `ordered:`, `CMarkTaskItem checked:`, `CMarkTable alignments:`, `CMarkLink url:title:`, `CMarkImage url:title:`, `CMarkCodeBlock literal:info:`, `CMarkDocument source:`; such nodes have no positions.
`CMarkVisitor` has `visit:`, `visitChildren:`, `visitNode:` and one `visitX:` per concrete class (`visitTaskItem:` defaults to `visitItem:`, `visitTableHeaderRow:` to `visitTableRow:`); visitors recurse, so they are for trees of bounded depth.
The inspector shows a node's tree (`CMark`), its source text when it has a position, and, with `CMark-Roassal`, the rendered node (`Rendered`, for trees of at most 5000 nodes).

### Errors

Every error of CMark is a `CMarkError`: `CMarkUnavailable` (no plugin, or one of another protocol), `CMarkLimitExceeded` (`limit`, `actual`) and its `CMarkInputTooLarge`, `CMarkTooDeep` and `CMarkMemoryLimit`, and `CMarkMalformedTree` (the decoder found a tape that breaks an invariant).
When growing object memory did not make room for an answer, the stock `OutOfMemory` is signalled: it is a condition of the VM, not of CMark, and escapes `on: CMarkError`.

### Without the plugin

`CMarkPrimitives pluginInfo` is nil, parsing and rendering signal `CMarkUnavailable` before anything else, and the rest works: the tree classes, hand-built trees, their visitors and their Roassal view.

### The Roassal view

`CMarkRoassalRenderer` (a `CMarkVisitor`) draws a node as one `RSComposite` whose model is the node: a document as a page of `maxWidth` (640 by default) plus its padding, any other node alone; `canvasFor:` answers a canvas with an `RSCanvasController` whose camera fits the width and shows the top.
Blocks are stacked composites with a transparent frame box each (the hit area, and the node as model); paragraphs and headings are laid out by `CMarkInlineFlow`, which breaks lines only at blanks, measures text by the advance of each character, and merges the words of one run on a line into one label; tables have an automatic layout; links carry one shared popup and a click that opens the URL (`linkAction`) or moves the camera to a heading or a footnote.
`CMarkRoassalTheme` (`default`, `dark`, `compact`) holds the fonts (Source Sans Pro and Source Code Pro, with a fallback), sizes, colours, gaps, `htmlStyle` (`#basic` by default: comments hidden, `<br>` a line break, `<img>` an image or its placeholder, other tags stripped; `#literal`; `#hide`; raw HTML is never executed), `codeWrap`, and the caps `maxNesting` (64), `maxShapes` (100,000) and `maxTableCells` (10,000), beyond which the rest is a muted placeholder.
Images are read from local files only (`imageResolver`, relative to `baseDirectory:`), never from the network.
`CMarkRoassalExporter` exports PNG and SVG through cairo; `CMarkRoassalExamples` opens examples; `asRoassalShape`, `asRoassalShapeWidth:` and `asRoassalCanvas` are on every node.

## Tree format

This section is the canonical description of the tape; `primitiveParse` answers it.

It answers `{ status. code. tape. strings. nodeCount. depth. recordWords. sourceChars. sanitised }`:

| slot | OK | other statuses |
|---|---|---|
| 3 `tape` | a **Bitmap** (`classBitmap()`, Spur format 10, unsigned 32-bit words) of `nodeCount * 16` words | nil |
| 4 `strings` | a **ByteArray** holding every string of the tree as UTF-8, concatenated | nil |
| 5 `nodeCount` | the number of records | 0 |
| 6 `depth` | the depth of the deepest node | the depth reached |
| 7 `recordWords` | 16 | 16 |
| 8 `sourceChars` | the code points of the sanitised input (see Text, positions and indices). For a String source this equals its `size`. | 0, or the value if known |
| 9 `sanitised` | nil, except for a **ByteArray** source whose bytes differ from `T` (invalid UTF-8 or NUL): then a ByteArray holding `T`, so that the image decodes `sourceString` from it and the indices match (Pharo's `utf8Decoded` raises `ZnInvalidUTF8` on invalid input; checked on P12 and P15) | nil |

**Why a flat tape.** The obvious alternative is nested Arrays, one object per node and per string, built by the
plugin as SimdJSON builds its trees. The tape was chosen for these reasons:
1. A parse answers exactly **3 objects** (4 with `sanitised`), whatever the tree's size. That is the shape SimdUTF's answers have already
   proved: one non-pointer result object filled with `memcpy`. The two bulk objects hold no pointers, so they need no
   store barrier and no remembered set. The only pointer stores go into the 9-slot answer, through
   `storePointerofObjectwithValue`.
2. The exact byte need is known after the C walk and before any allocation, so the OVER_BUDGET pattern of SimdJSON
   (status 1, exact need, `growMemoryByAtLeast:`, call again) is exact and cheap. A NoMemory failure can only happen
   on one of 3 allocations, and the result is kept for the retry.
3. The plugin does not depend on the image's class layouts. The node classes and the renderer can change
   without touching C.
4. The parent index makes decoding in Smalltalk one loop with no recursion. cmark's nesting is unbounded.

It costs 64 bytes per node plus the strings. On spec.txt-like input that is about 2-3 tape bytes per input byte;
cmark's own tree takes about 13 bytes per input byte while it lives.

**Record layout.** Records are in **pre-order** of cmark's tree: a parent comes before its children, and siblings
come in cmark's order. That is document order, except that cmark moves the referenced footnote definitions to the end
of the document, sorted by first reference (blocks.c:527-537; with `A[^y] and [^x]` and definitions `x z y`, `y`
comes first and `z` is dropped). So positions are **not monotonic** in record order, and no code (the engine's line
lookups, the decoder, the Roassal side) may assume they are. Record `i` (1-based) occupies words `(i-1)*16 + 1 .. (i-1)*16 + 16`. Smalltalk reads word `k` of record `i` as
`tape at: (i - 1) * 16 + k`. An unused word is 0.

| k | field | meaning |
|---|---|---|
| 1 | code | stable node type code (table below) |
| 2 | parent | the parent's record index (1-based). 0 for the document, which is always record 1. |
| 3 | flags | bits below |
| 4 | startLine | cmark's start line, 1-based. 0 = no position. |
| 5 | startColumn | cmark's start column: a 1-based **byte** offset in the UTF-8 line |
| 6 | endLine | cmark's end line |
| 7 | endColumn | cmark's end column, inclusive, in bytes. 0 means "the block ends at the end of the line before" (see Text, positions and indices). |
| 8 | startIndex | 1-based **character** index in the source of the first character. 0 = no position. |
| 9 | endIndex | 1-based character index of the last character, inclusive. It can be startIndex - 1 for an empty span. 0 = no position. |
| 10 | int1 | per type |
| 11 | int2 | per type |
| 12 | int3 | per type |
| 13 | str1 offset | 0-based byte offset into `strings` |
| 14 | str1 length | bytes. 0 for an absent or empty string. |
| 15 | str2 offset | |
| 16 | str2 length | |

**Flags** (word 3):

| bit | value | name | meaning |
|---|---|---|---|
| 0 | 1 | STR1_ASCII | str1 has only bytes below 0x80 |
| 1 | 2 | STR2_ASCII | the same for str2 |
| 2 | 4 | HAS_POS | startLine >= 1 **and** startColumn >= 1. Autocompleted table cells report `L:0-L:0` and get no HAS_POS. |
| 3 | 8 | TIGHT | list: tight |
| 4 | 16 | FENCED | code block: fenced |
| 5 | 32 | HEADER | table row: the header row |
| 6 | 64 | TASK | item: a task list item (the tasklist extension) |
| 7 | 128 | CHECKED | task item: checked |
| 8 | 256 | EXTENSION | `cmark_node_get_syntax_extension(n) != NULL` |
| 9 | 512 | CLAMPED | a position pointed outside the source and was clamped (see Text, positions and indices) |

**Codes and per-type fields.** The core codes are cmark's enum values (cmark-gfm.h:34-69). The extension types are
runtime globals that depend on registration order (`CMARK_NODE_TABLE` is 0x800c only after registration), so they are
mapped to fixed codes. Each fixed code keeps the block bit (0x8000) or the inline bit (0xc000).

| code | cmark type | class | int1 | int2 | int3 | str1 | str2 | flags used |
|---|---|---|---|---|---|---|---|---|
| 0x8001 | DOCUMENT | CMarkDocument | | | | | | |
| 0x8002 | BLOCK_QUOTE | CMarkBlockQuote | | | | | | |
| 0x8003 | LIST | CMarkList | listType 1 bullet / 2 ordered | delim 0 none / 1 period / 2 paren | start (0 for bullets) | | | TIGHT |
| 0x8004 | ITEM | CMarkItem, or CMarkTaskItem when TASK | the item's marker number as written (`cmark_node_get_item_index`: `1. 1. 1.` gives 1, 1, 1; bullets give 0; only cmark's renderers renumber, render.c:184-191) | | | | | TASK, CHECKED |
| 0x8005 | CODE_BLOCK | CMarkCodeBlock | fence length | fence character (96 or 126; 0 when indented) | fence offset | literal | info string (`""` for indented code) | FENCED |
| 0x8006 | HTML_BLOCK | CMarkHtmlBlock | | | | literal | | |
| 0x8007 | CUSTOM_BLOCK | CMarkCustomBlock | | | | on_enter | on_exit | |
| 0x8008 | PARAGRAPH | CMarkParagraph | | | | | | |
| 0x8009 | HEADING | CMarkHeading | level 1..6 | | | | | |
| 0x800a | THEMATIC_BREAK | CMarkThematicBreak | | | | | | |
| 0x800b | FOOTNOTE_DEFINITION | CMarkFootnoteDefinition | ordinal: 1-based among the definitions, in document order | | | label (`get_literal`) | | |
| 0x8100 | TABLE | CMarkTable | columns | | | alignments: one byte per column, `l` `c` `r` or `-` (none) | | EXTENSION |
| 0x8101 | TABLE_ROW | CMarkTableRow, or CMarkTableHeaderRow when HEADER | | | | | | HEADER, EXTENSION |
| 0x8102 | TABLE_CELL | CMarkTableCell | column, 1-based: the cell's position among its siblings (the image computes it from the index; the word is for C tests and the dump) | | | | | EXTENSION |
| 0x80ff | any other block type | CMarkUnknownBlock | the raw cmark type | | | `cmark_node_get_type_string` | | |
| 0xc001 | TEXT | CMarkText | | | | literal | | |
| 0xc002 | SOFTBREAK | CMarkSoftBreak | | | | | | |
| 0xc003 | LINEBREAK | CMarkLineBreak | | | | | | |
| 0xc004 | CODE | CMarkCode | | | | literal | | |
| 0xc005 | HTML_INLINE | CMarkHtmlInline | | | | literal | | |
| 0xc006 | CUSTOM_INLINE | CMarkCustomInline | | | | on_enter | on_exit | |
| 0xc007 | EMPH | CMarkEmph | | | | | | |
| 0xc008 | STRONG | CMarkStrong | | | | | | |
| 0xc009 | LINK | CMarkLink | | | | url (`""` when absent) | title (`""` when absent) | |
| 0xc00a | IMAGE | CMarkImage | | | | url | title | |
| 0xc00b | FOOTNOTE_REFERENCE | CMarkFootnoteReference | ordinal (the decimal literal as an integer) | the record index of its definition (0 if none) | | literal (the ordinal's text) | | |
| 0xc100 | STRIKETHROUGH | CMarkStrikethrough | | | | | | EXTENSION |
| 0xc0ff | any other inline type | CMarkUnknownInline | the raw cmark type | | | type string | | |

Notes for the engine:
- **Task items**: TASK is set when `cmark_node_get_syntax_extension(n)` is the pointer that
  `cmark_find_syntax_extension("tasklist")` answered, compared by pointer (no private header is needed). CHECKED comes
  from `cmark_gfm_extensions_get_tasklist_item_checked`.
- **Header row**: `cmark_gfm_extensions_get_table_row_is_header`. Table alignments come from
  `cmark_gfm_extensions_get_table_alignments` (`'l'`, `'c'`, `'r'`, 0 -> `-`). The columns come from
  `cmark_gfm_extensions_get_table_columns`. Call it only on TABLE nodes: it dereferences without a NULL check.
- **Footnote reference -> definition**: definitions are moved to the end of the document, sorted by first use. During the walk, the engine records the record index of the k-th definition, and the references to patch. It
  fills int2 after the walk, from the reference's ordinal (the k-th definition after the move has ordinal k,
  checked in blocks.c:488-537). `cmark_node_parent_footnote_def` would need a pointer -> record map and is not used.
  With FOOTNOTES off, neither type appears. cmark's `ref_ix`/`def_count` are private (node.h:93-96); the image
  ranks the references of the tape by position instead (see The tree). That is `ref_ix` (the n of cmark's HTML id
  `fnref-<label>-<n>`) unless cmark dropped a definition that held references to the same footnote:
  `process_footnotes` numbers the references of the whole document before it unlinks the unreferenced and the
  repeated definitions (blocks.c:487-503), so for `x[^a]`, `[^u]: unused[^a]`, `y[^a]`, `[^a]: A` the HTML gives
  `y`'s reference `fnref-a-3` and three backrefs, while its `referenceIndex` is 2 and the definition has 2
  `references`. The public API does not give the dropped references.
- **Autolinks** are plain LINK nodes and cannot be told apart. tagfilter changes nothing in the tree.
- The walk uses `cmark_node_first_child/next/parent`, not the iterator, so it does not depend on cmark's leaf-type
  list. It keeps a stack of record indices (malloc'd, growing, charged) for the parents and the depth.
- Private cmark data that the public API does not give is not in the tape: the bullet character `-*+`, setext versus
  ATX headings, the HTML block kind (node.h:22,38,107). The API section documents it.

## Primitive contract

### Module, calls, protocol

The module is `CMarkPlugin` (`getModuleName` answers `CMarkPlugin (i)` when built in, as in WebAssembly, `CMarkPlugin (e)` as a shared library).
Its primitives are class-side methods of `CMarkPrimitives`, `<primitive: 'primitiveX' module: 'CMarkPlugin' error: ec>`, whose receiver is ignored, and all have accessor depth 0: none reads objects inside its arguments.
The protocol is 1 (`CM_PROTOCOL`, slot 1 of `primitiveInfo`, `CMarkPrimitives protocol`); a change of the arguments or answers of a primitive changes it, and the image takes the plugin only with its own protocol.

| Primitive | Arguments (first to last) | Answer |
|---|---|---|
| `primitiveInfo` | none | an Array of 7 (see primitiveInfo) |
| `primitiveParse` | source, options, extensions, maxDepth, budget, cBudget | an Array of 9 (see Tree format) |
| `primitiveRender` | source, format, options, extensions, width, budget, cBudget | an Array of 5 (see primitiveRender) |
| `primitiveRelease` | none | a SmallInteger: the bytes of C memory it freed (the kept result) |

### Failures

A primitive fails only when the caller has a bug, or when an object allocation fails; a failing primitive leaves the stack alone.
Bad *input* does not exist: any byte sequence parses, and the limits are statuses of the answer, so they never start the VM's retries.

| Code | Name (`ec`) | When |
|---|---|---|
| 5 | `#'bad number of arguments'` | a wrong argument count |
| 3 | `#'bad argument'` | a wrong kind or range, checked in this order: the kind of the source; every number is a SmallInteger; option bits outside `CM_OPT_ALLOWED`; extension bits outside 0..31; maxDepth < 0; width < 0 or > 10000; budget < -1; cBudget < -1; format outside 1..6 |
| 11 | `#'not found'` | after every bad-argument check: a set extension bit whose extension the plugin did not find at its registration (never with a complete build; the C tests force it) |
| 9 | `#'insufficient object memory'` | an allocation of the answer failed; the VM collects garbage and calls again, and the plugin keeps the result it computed, so the retry does not parse again (see The kept result) |

A missing extension and a missing module both read `#'not found'`: `CMarkPrimitives` tells them apart by `pluginInfo` (nil means no plugin; otherwise it signals `CMarkError` 'extension missing from the plugin').

The statuses, slot 1 of the answers of `primitiveParse` and `primitiveRender`, with slot 2, the code:

| Status | Name | Code |
|---|---|---|
| 0 | OK | parse: 0; render: the byte size of the output |
| 1 | OVER_BUDGET | the exact bytes of object memory the full answer needs (see Budgets) |
| 2 | TOO_LARGE | the input's size in sanitised UTF-8 bytes, which is over `CM_MAX_INPUT_BYTES` |
| 3 | TOO_DEEP | `maxDepth + 1`: the walk stops at the first node deeper than maxDepth |
| 4 | C_LIMIT | 1 the C budget was exceeded, 2 one allocation was over `CM_MAX_BLOCK_BYTES`, 3 malloc answered NULL |

### primitiveInfo

It answers `{ 1. 'CMarkPlugin 1.0'. '0.29.0.gfm.14'. platform. extensions. limits. stats }`:

- `platform`: `'native'` or `'emscripten'`;
- `extensions`: an Array of the names of the extensions found at registration, in bit order, `#('table' 'strikethrough' 'autolink' 'tagfilter' 'tasklist')`; if one is missing the Array is shorter, and the image's load check fails;
- `limits`: `{ CM_MAX_INPUT_BYTES. CM_MAX_BLOCK_BYTES. defaultCBudget. CM_REC_WORDS. keepCap }`, that is `{ 268435456. 268435456. 4294967296. 16. 4294967296 }` natively and `{ 268435456. 268435456. 134217728. 16. 8388608 }` on Emscripten;
- `stats`: 8 SmallIntegers, `{ parses. renders. reused. overBudget. tooLarge. tooDeep. cLimit. kept }` (`CMarkPrimitives pluginStatistics` answers them as a Dictionary by these names): `reused` counts the calls answered from the kept result, `kept` the bytes kept now.

### Arguments

- **source**, decided by its exact class or format, in this order: exactly a ByteArray, UTF-8; exactly a ByteString, Latin-1, one byte a character; any object of 32-bit words (a WideString; a WideSymbol, a WordArray or a Bitmap would be read the same way), UTF-32; anything else, ByteSymbol and Array included, `#'bad argument'` (the image passes `asString` of a Symbol).
- **options**: a SmallInteger of cmark-gfm's option bits, passed as they are; `CM_OPT_ALLOWED` = `0x3FF1E`: SOURCEPOS 1<<1, HARDBREAKS 1<<2, SAFE 1<<3 (a no-op), NOBREAKS 1<<4, NORMALIZE 1<<8 (a no-op), VALIDATE_UTF8 1<<9 (always set), SMART 1<<10, GITHUB_PRE_LANG 1<<11, LIBERAL_HTML_TAG 1<<12, FOOTNOTES 1<<13, STRIKETHROUGH_DOUBLE_TILDE 1<<14, TABLE_PREFER_STYLE_ATTRIBUTES 1<<15, FULL_INFO_STRING 1<<16, UNSAFE 1<<17.
  `primitiveParse` always adds SOURCEPOS and VALIDATE_UTF8 (without SOURCEPOS the positions after a multi-line inline are wrong); `primitiveRender` parses with `options | VALIDATE_UTF8` and renders with `options`, so the caller's SOURCEPOS decides whether the output has `data-sourcepos`.
- **extensions**: a bit mask, 1 table, 2 strikethrough, 4 autolink, 8 tagfilter, 16 tasklist (31 all).
- **maxDepth**: 0 means no limit; the document has depth 1.
- **budget**: the bytes of object memory the answer may take, checked before anything is allocated; -1 means no check (see Budgets).
- **cBudget**: the bytes of C memory that cmark-gfm and the engine may hold at once during the call; -1 means the default (see primitiveInfo); a larger value is clamped to the default.
- **format** (render): 1 html, 2 xml, 3 commonmark, 4 plaintext, 5 latex, 6 man.
- **width** (render): the wrap width of commonmark, plaintext, latex and man, 0 for no wrapping; html and xml ignore it.

### Budgets

The object memory an answer needs is `need = spurPointers(9) + spurWords32(16 * nodeCount) + spurBytes(stringBytes)`, plus `spurBytes(|T|)` when the sanitised bytes are answered, with the sizes of Spur's objects (an 8-byte header, an overflow header from 255 slots, at least one slot), as the SimdUTFPlugin computes them; for a render, `spurPointers(5) + spurBytes(outputBytes)`.

- When `budget >= 0` and `need > budget`, the answer is `{ 1. need. nil. nil. 0. depth. 16. sourceChars. nil }` (render: `{ 1. need. nil. ascii. byteSize }`), allocating only the answer's Array (a render answers `{ 1. need. nil. ascii. byteSize }` with the size of its output), and the result is kept.
- Otherwise the plugin allocates the answer Array, the Bitmap, the ByteArray and, when needed, the sanitised ByteArray, copies the tape and the strings, stores them and answers; when an allocation fails, the primitive fails with `#'insufficient object memory'` and the result is kept.

The image passes a budget (`CMarkPrimitives budgetChecked`: on Emscripten, or when the VM reports a limit of old space, `Smalltalk vm parameterAt: 67`) of the free old space, `Smalltalk vm parameterAt: 54`, for sources of more than 65,536 units, and -1 otherwise; on status 1 it grows the memory by the need plus 1 MiB (`Smalltalk growMemoryByAtLeast:`; growing by only the part of the need that the free space lacks is not enough, since the free old space is not contiguous and the answer is 2 or 3 large objects) and calls again with the free old space as the budget, which the kept result answers without parsing; when that still answers 1, it signals `OutOfMemory`.
On `#'insufficient object memory'`, of the first call or of that second one, it measures the need (budget 0), grows by the need plus 1 MiB and calls with -1.
The selectors are `CMarkPrimitives parse:options:extensions:maxDepth:cBudget:` and `render:format:options:extensions:width:cBudget:` (the loop computes the budget itself); the primitives themselves are `primParse:options:extensions:maxDepth:budget:cBudget:` and `primRender:format:options:extensions:width:budget:cBudget:`.
The loop runs inside `ifCurtailed: [ self primRelease ]`, so that an exception inside it releases the kept result at once.

### primitiveRender

It parses the source with the input handling, options and extensions of `primitiveParse`, renders the tree with cmark-gfm's renderer of the format (html gets the parser's list of extensions, which tagfilter and the extensions' HTML need), and frees the tree.
It answers `{ status. code. bytes. ascii. byteSize }`: `bytes` is a ByteArray of the UTF-8 output (nil unless OK), `ascii` is true when every byte is below 0x80, and `byteSize` is the size of the output, also for OVER_BUDGET.
Every renderer, CommonMark included, runs under the C limits: an input whose CommonMark output would make cmark-gfm abort (a deep quote with many lines, whose buffer would pass 1 GiB) answers status 4.
The output is not copied once more: the engine detaches cmark-gfm's output buffer from the tracking allocator (see C memory) and the glue copies it into the ByteArray.

### The kept result

When a call answers OVER_BUDGET, or fails with `#'insufficient object memory'`, the engine keeps what it computed (the tape and the strings, or the output) with a key: the primitive, the sanitised text `T` itself, and every number argument but budget and cBudget.
The next parse or render first computes its own `T`, compares the keys, and either answers from the kept result (`reused`) or frees it; so a ByteArray with invalid bytes and a String with U+FFFD at the same places share a kept result, while the sanitised slot always comes from the current call's source.
Every successful answer frees it, and so do `primitiveRelease` and `shutdownModule`; at most one result is kept, and it counts against the next call's C budget.
A result larger than the keep cap (4 GiB natively, 8 MiB on Emscripten) is not kept: the retry parses again.

### Text, positions and indices

The engine first turns the source into one sanitised UTF-8 buffer `T`, which is what cmark-gfm is fed (in one `cmark_parser_feed`), what the positions refer to, and what `sourceChars` counts:

- a ByteString (Latin-1): bytes below 0x80 are copied, the others become 2 bytes, and NUL becomes U+FFFD;
- a WideString (UTF-32): each code point is encoded; values above 0x10FFFF, surrogates and NUL become U+FFFD;
- a ByteArray (UTF-8): valid sequences are copied; each byte that does not start a valid shortest-form sequence of a non-surrogate code point up to 0x10FFFF becomes one U+FFFD, and NUL becomes U+FFFD; when `T` then differs from the source, slot 9 of the answer holds it, so that the image decodes `sourceString` from it and the indices match (Pharo's `utf8Decoded` raises on invalid input).

The exact UTF-8 size is computed first: over `CM_MAX_INPUT_BYTES` (256 MiB) the answer is status 2, and nothing is allocated.
So for String and WideString sources, character k of the source is code point k of `T`; for a ByteArray of valid UTF-8, the indices are those of `bytes utf8Decoded`.

The character indices (words 8 and 9 of a record) come from cmark-gfm's line and byte column:

- `lineStart(l)` is the byte offset of line l of `T`, cut as cmark-gfm cuts lines (at `\r\n`, a lone `\r` or a lone `\n`), with `lineStart(L+1) = |T|`, the phantom line that blocks ending at the end of the input report as `L+1:0`; the table is sparse (the start of every 64th line, and a scan from the nearest of that checkpoint and the last four lines resolved), since a dense one would take 4 to 8 bytes an input byte for a document of empty lines;
- `charIndex(b)`, for the byte offset b of the last byte of a character, is `(b + 1) - cont(b + 1)`, where `cont(k)` counts the continuation bytes of `T` before k, from a checkpoint every 64 bytes;
- for a node with HAS_POS, `startIndex = charIndex(lineStart(sl) + sc - 1)`, and `endIndex = charIndex(lineStart(el) + ec - 1)` when `ec >= 1`, else the character before line el (its line terminator, so such a block's `sourceText` ends with it), or 0 when line el starts at 0;
- a position beyond line L+1, or a byte column at or beyond the end of its line, is clamped to the last byte of that line (or of `T`), and CLAMPED is set; that covers the angle-autolink columns of gfm.14 (see Limitations), but not the escaped-pipe columns of table cells, which stay inside their line;
- a node without HAS_POS has 0 in words 4 to 9: soft and hard line breaks, the cells cmark-gfm adds to pad a table row, and the text `[^x]` that an unresolved footnote reference becomes.
- cmark-gfm then merges that text `[^x]` into a text just before it, if there is one: `a [^gone]` gives one text `a [^gone]` at 1:1-1:0, i.e. startIndex 1 and endIndex 0, an empty span; only a reference after another kind of node, or at the start of a paragraph and followed by one (`[^nope]*x*`), stays a text without HAS_POS;
- the empty document reports 1:1-0:0: it has HAS_POS and CLAMPED, startIndex 1 and endIndex 0 (an empty span, so that `sourceText` answers an empty String); a text that ends an unterminated last line gets an end column one past the line, which is clamped (CLAMPED, the indices right).

A UTF-8 BOM at the start stays in line 1 and is counted by its columns, as cmark-gfm counts it; it is character 1 of a String source too, and of the `sourceString` of a ByteArray source: Pharo's `utf8Decoded` drops every U+FEFF (and raises on one that ends the bytes), so the image decodes UTF-8 with `cmarkUtf8Decoded`, which splits the bytes at each `EF BB BF`, decodes the pieces with `utf8Decoded` and puts the marks back.
Tabs count as one byte and one character.
The plugin never builds Strings from the tree: the image decodes each one from the strings ByteArray, an ASCII one (flag STR1_ASCII or STR2_ASCII) with `ByteString new:` and `replaceFrom:to:with:startingAt:`, the others with `cmarkUtf8Decoded` (`utf8Decoded`, which answers the narrowest class, but keeping every U+FEFF); the output of a render is decoded the same way, by its `ascii` flag.

### C memory

cmark-gfm aborts the process when its allocator answers NULL, and when a buffer would grow past 1 GiB, which a 1.2 MB input can reach through the CommonMark renderer.
The engine prevents every abort:

- every parse uses `cmark_parser_new_with_mem` with the tracking allocator, through which goes all the memory of a document (nodes, buffers, the extensions' data, the renderer's output, the iterator);
- the tracker keeps the live blocks in a list (a header of 32 bytes each, which keeps 16-byte alignment) and counts their bytes; a request **longjmps** back to the primitive when the live bytes would exceed the call's C budget (status 4, code 1), when one block is over `CM_MAX_BLOCK_BYTES` = 256 MiB (code 2), or when malloc or realloc answers NULL (code 3);
- 256 MiB is enough: a buffer grows to at most its size (one block) plus one addition (another block, or one line), so it stays below 512 MiB and the 1 GiB abort is unreachable; since cmark-gfm grows buffers by 1.5, the cap refuses a buffer first, so a single line or paragraph is limited to about 170 MiB (status 4, code 2);
- the `setjmp` is in one small function that calls one worker; the landing, when it longjmps, first clears cmark-gfm's tables of the special characters of the inline phase (the strikethrough and autolink extensions set them before the inlines and clear them after, so a longjmp in between would leave them set for every later parse of the process), then frees every tracked block in one sweep, without calling cmark-gfm, then the engine's own buffers, and answers status 4;
- on the normal path the engine detaches the render output, frees the parser and the tree, and sweeps: the sweep must find no block (the tests check it; one would be a leak);
- the engine's own buffers (`T`, the line table, the checkpoints, the tape, the strings, the stack of parents, the kept result, the detached output) are plain mallocs, charged to the same C budget; a NULL answers status 4, code 3;
- the tracker is one static context, active only during a call: the VM calls primitives from one thread.

On WebAssembly the longjmp crosses cmark-gfm's frames with the VM's setjmp/longjmp flags; those frames hold no resources.

### Registration and global state

The extensions are registered once (`cmark_gfm_core_extensions_ensure_registered`) by `initialiseModule` and lazily by every primitive, with cmark-gfm's default allocator, before the tracker is installed; they live until the process exits.
`cmark_release_plugins` is never called (the extensions could not be found again), and neither are the arena allocator, `cmark_parse_document`, `cmark_parse_file` and `cmark_markdown_to_html` (rule R8 of the C tests).
After registration a parse reads globals only, except the tables of the special characters, which the landing restores.
`shutdownModule` frees only the kept result.

### The engine interface

`include/common/cmEngine.h` is the interface between the glue and the engine, with no VM and no cmark-gfm type: `cm_init`, `cm_version`, `cm_extension_count`, `cm_extension_name`, `cm_extensions_present`, `cm_parse` (into a `cm_tree`: the tape, the strings, the depth, `sourceChars` and `T`), `cm_render` (into a `cm_output`), `cm_tree_free`, `cm_output_free`, `cm_keep_tree`, `cm_keep_output`, `cm_release_kept`, `cm_kept_bytes` and `cm_get_stats`, all hidden (`CM_API`); `CM_TESTING`, which only the C tests define, adds hooks that lower the limits, pretend an extension is missing and count the tracked blocks.
Sizes are `size_t`, `uint64_t` and `int64_t` only: `long` is 32 bits on Windows.

### Allocation and GC safety

The glue allocates with `instantiateClassindexableSize` only, which never runs a garbage collection inside the call (Spur allocates in old space when eden is full, and answers 0 when it cannot), so the source's bytes stay where they are for the whole primitive; it stores into the answer with `storePointerofObjectwithValue`, and the two bulk objects hold no pointers, so a parse answers exactly 3 objects (4 with the sanitised bytes) whatever the size of the tree.
It uses none of the allocators that allocate in eden only (`floatObjectOf`, `signed64BitIntegerFor`, `positive64BitIntegerFor`, `stringForCString`, `methodReturnString`, `clone`; rule R1 of the C tests).
The builtin table of WebAssembly has one row per primitive and module function, in the shape `cmake/emscripten/genPrimitiveTable.cmake` reads, all of depth 0; as a shared library, each primitive has `EXPORT(signed char) primitiveXAccessorDepth = 0`.

## Memory and limits

| Limit | Native | Emscripten | Effect |
|---|---|---|---|
| `CM_MAX_INPUT_BYTES` (the plugin) | 256 MiB of sanitised UTF-8 | 256 MiB | status 2, `CMarkInputTooLarge` |
| `maxInputSize` (the image, `CMarkOptions`, on `source size` before any call: characters, or bytes of a ByteArray) | 32 Mi | 16 Mi | `CMarkInputTooLarge`, no call; nil leaves only the plugin's cap |
| `CM_MAX_BLOCK_BYTES` | 256 MiB per allocation of cmark-gfm | 256 MiB | status 4, code 2 |
| the default C budget | 4 GiB | 128 MiB | status 4, code 1, `CMarkMemoryLimit`; `cBudget:` may lower it, and a larger value is clamped |
| the keep cap | 4 GiB | 8 MiB | a larger result is not kept |
| maxDepth | the caller's (1000 by default) | the same | status 3, `CMarkTooDeep` |
| offsets of the tape and the strings | 32 bits (strings below 4 GiB) | the same | status 4, code 1 (unreachable under the budgets) |

cmark-gfm's own limits stay: lists nest at most 100 deep, code spans have at most 80 backticks, a table gets at most 0x80000 cells added to pad its rows.
A parse takes some 13 bytes of C memory an input byte while cmark-gfm's tree lives, and the tape 64 bytes a node plus the strings (2 to 3 bytes of tape an input byte on text like the CommonMark spec).
A single line or paragraph is limited to about 170 MiB (see C memory).

**WebAssembly.** The C heap is dlmalloc over `sbrk`: it never shrinks, and it grows into the window of old space, 512 MiB to 1 GiB by default (docs/WebAssembly.md).
One parse of 30 to 40 MB with a C budget of 1 GiB could push it through the whole window, and the image would get an `OutOfMemory` later, far from its cause.
Hence the smaller defaults there, a C budget of 128 MiB, an input of 16 Mi characters and a kept result of 8 MiB, which keep the worst case a small part of the window.
Larger documents need a larger old space (`PHARO_WASM_OLD_SPACE_BASE=0x40000000`), an explicit `cBudget:` and `maxInputSize: nil`.

## Blocking

A parse or a render is one primitive call: the VM does nothing else meanwhile (a parse of 256 MiB takes seconds natively), and the web VM does not return to its page until it ends.
The image-side `maxInputSize` keeps the common case short.

## Tests

    sh plugins/CMarkPlugin/tests/run-c-tests.sh --vm-build <native build tree> --emsdk <emsdk>      # C
    sh plugins/CMarkPlugin/tests/run-st-tests.sh build/build/vm <Pharo 12 to 15 image or zip>       # SUnit

`run-c-tests.sh` compiles cmark-gfm's 34 files itself, with the flags of each lane (`-std=c99 -Wall -Wextra -Wno-unused-parameter -pedantic -Werror`, and the lane's compiler and sanitizers), into an object directory per lane (`--cmark DIR` takes a cmark-gfm tree, by default the one the `--vm-build` tree unpacked), and runs the lanes (`--only` chooses some); every lane defines `CM_TESTING`, which the VM's build never does:

- `engine` (clang -O2, `-Wshorten-64-to-32` on the engine), `engine-gcc`, `engine-uchar` (`-funsigned-char`, the char of AArch64), `engine-asan` (ASan and UBSan, leaks detected) and `engine-wasm64` (emcc `-m64` under node, once with `-sSUPPORT_LONGJMP=wasm` and once with `-sSUPPORT_LONGJMP=emscripten`): `tests/enginetest.c`: the sanitising of every kind of source (overlongs, surrogates, truncated sequences, NUL), positions and indices (multi-byte characters, `\r\n`, a lone `\r`, a BOM, tabs, the phantom line, `L:0` blocks, the empty document, CLAMPED, 200,000 empty lines, 70 lines of 1 MB), the tree of `fixtures/sample.md` against `fixtures/sample.tree`, every code, flag and field, extension masks, options, depths (1,000,000 nested quotes), the limits, a sweep of C budgets over every primitive and format with no block left and the special characters restored after every status 4, the CommonMark blow-up, the renders of the sample in the six formats against `fixtures/sample.*`, determinism, the kept result, the known gfm.14 behaviour, stress inputs, and 20,000 random inputs;
- `glue` (with `--vm-build`): the glue as a shared and as a builtin plugin against a fake VM (`tests/fakevm.c`) under ASan and UBSan: `tests/gluetest.c`: every failure code in the order of the checks, the source kinds, the answer shapes, budget 0 and the reuse, injected allocation failures and their retries, `primitiveRelease`, `primitiveInfo`; first the glue compiles with `-Wall -Werror` against the VM's headers, and its table through `genPrimitiveTable.cmake`;
- `upstream` (optional, `--only upstream`): cmark-gfm's command-line program built on our objects, run by upstream's `spec_tests.py` and `roundtrip_tests.py` (with `python3 -I`, from a copy of the scripts) over `spec.txt`, `smart_punct.txt`, `extensions.txt`, `extensions-table-prefer-style-attributes.txt`, `extensions-full-info-string.txt` and `regression.txt`: everything passes but example 25 of `extensions.txt`, the footnote regression of gfm.14 (see Limitations);
- the rules R1 (no forbidden allocator in the glue), R2 (cmark-gfm's headers only in the engine), R3 (no VM header in the engine), R5 (the exports, the builtin table and the accessor depths name the same four primitives), R7 (one `.c` in `src/common`), R8 (none of `cmark_release_plugins`, the arena, `cmark_parse_document`, `cmark_parse_file`, `cmark_markdown_to_html`, `printf`, `_setjmp`, `long`), R9 (one `setjmp`, and no `cmark-config/private` outside cmark-gfm) and V (the pinned cmark-gfm, with the SHA256 of its sources in `tests/fixtures/cmark-sources.sha256`).

`run-st-tests.sh <vm-dir> <image.zip or .image>` works on a copy of the image: it loads the groups of `CMARK_GROUPS` (`Core Tests` by default) with `load-cmark.st`, checks its banner against `CMARK_EXPECT_PLUGIN` (by default `yes` when `<vm-dir>` holds `libCMarkPlugin.so`, `.dylib`, `CMarkPlugin.dll` or `libCMarkPlugin.dll`), so that a plugin that does not load never passes as a run whose tests all skipped, then runs every test package loaded with a JUnit file each, and summarises them.
Its environment: those, `CMARK_PLUGIN_DIR`, `CMARK_WORK_DIR` and `CMARK_TIMEOUT` (3600 s by default).
A VM without the plugin is that of a build with `FEATURE_PLUGIN_CMARK=OFF`, or a stock one (deleting the plugin from a build tree does not hide it: the VM finds it through its absolute RUNPATH):

    sh run-st-tests.sh build/build/vm <image>                                                  # the plugin
    CMARK_EXPECT_PLUGIN=no sh run-st-tests.sh <vm-dir without the plugin> <image>              # no plugin: the skips
    CMARK_GROUPS='Core Tests Roassal RoassalTests' sh run-st-tests.sh build/build/vm <image>   # the renderer too

- `CMark-Tests`, without the plugin: the decoder on hand-made tapes (every code, flag and field, unknown codes, malformed tapes, a chain 200,000 deep, the sanitised slot, footnotes in their moved order), the nodes built by hand and their navigation, the visitor, the options' bits, and `CMarkUnavailable`; with the plugin: each construct of the sample with its exact positions and `sourceText` for ByteString, WideString and ByteArray sources (invalid UTF-8 and NUL included), the options and extensions per call, the depth and input limits, the renders in the six formats, the budget loop (`OVER_BUDGET`, the reuse, the release after an exception, `CMarkMemoryLimit`), the raw contract of the primitives, and the times of a large parse (printed);
- `CMark-Roassal-Tests`: the invariants of the rendered shapes (models, frames, labels, no clipping, the width, baselines, wrapping, tables, lists, tasks, links, images, raw HTML, deep nesting, the caps), golden structural dumps of the fixtures, the exports, the canvas, and determinism.

The native jobs of `.github/workflows/build.yml` load the plugin of their zip with `tests/ci-load.st`, which compiles its own primitive methods (the VMMaker image of CI has no CMark), and prints `CMarkPlugin native 0.29.0.gfm.14 1 ok` after known trees of `'# Hi'` and of a table, the HTML of `'# Hi'` and the sanitised bytes of an invalid ByteArray, then `CMarkPlugin limits ok` after a parse over a small C budget (status 4), one too deep (status 3), the same tape again, and a sweep of C budgets over a paragraph of inlines with every extension, each status 4 followed by a parse without extensions that must give the tape it gave before (the landing restored the special characters).
In the WebAssembly build, lane 75 of `make wasm-check` runs the probe, the load, `CMark-Tests` with the node and the web VMs, and `CMark-Roassal-Tests` when the image has Roassal, and checks that the C heap left the window of old space room (see docs/WebAssembly.md).

Measured (2026-11-22, Linux x86_64, clang 18.1.3, Emscripten 6.0.10):

- the C lanes: the rules and V, `engine`, `engine-gcc`, `engine-uchar`, `engine-asan` (81,974 checks each), `engine-wasm64` in both setjmp modes (81,973 checks each) and `glue` (1,107 checks shared, 1,127 builtin) pass; `upstream`: spec.txt 670, smart_punct 16, extensions 29 of 30 (spec and roundtrip; example 25, as expected), table-prefer-style-attributes 1, full-info-string 4, regression 26;
- the native CoInterpreter VM builds with the plugin, `libCMarkPlugin.so` exports only its module functions, primitives and accessor depths, and `ci-load.st` prints both lines on stock Pharo 12 and 15;
- `run-st-tests.sh` with that VM: `CMark-Tests` 105 tests, 0 skipped, on stock Pharo 12, 13, 14 and 15; `CMark-Roassal-Tests` 64 tests, 0 skipped, on the same four images and on the batteries image; with a VM without the plugin (Pharo 15 and the batteries image), 60 pass and 45 skip, and 53 pass and 11 skip;
- the WebAssembly lanes 05, 20, 50 and 75 pass on Pharo 15 (lane 75: 8 checks in 37 s, `CMark-Tests` 105 tests on the node and the web VMs, `CMark-Roassal-Tests` 57 passed and 7 skipped), and with `-DFEATURE_PLUGIN_CMARK=OFF` lanes 20 and 50 pass and lane 75 skips.

## Benchmarks

Printed by the tests, not asserted (stock Pharo 15, the native CoInterpreter VM of this repository on Linux x86_64, 2026-11-22):

- `CMarkPerformanceTest`: 10,502,592 characters of repeated fixture text, 1,623,757 nodes: the parse 1.8 s, the decode 1.9 s; a tape of 103,920,464 bytes and 7,548,738 bytes of strings; about 107 image bytes a node, without its strings (1.4 to 1.8 s and 1.6 to 1.9 s over the runs on Pharo 12 to 15);
- `CMarkRoassalPerformanceTest`: the sample, 158 shapes, built in 14 ms and drawn in 9 ms; a hand-built document, 143 shapes, 29 ms and 14 ms; a README-like text of 1,050,073 characters, 100,015 shapes (the `maxShapes` cut), built in 3.0 s and drawn once, whole, in 1.5 s.

## Licences

- cmark-gfm is under the BSD 2-Clause licence (Copyright (c) 2014, John MacFarlane), with parts under MIT licences: houdini (`houdini.h`, `houdini_href_e.c`, `houdini_html_e.c`, `houdini_html_u.c`), the buffer (`buffer.h`, `buffer.c`, `chunk.h`) and the tables derived from utf8proc; all of them are in its `COPYING`, which is shipped as it is: installed as `LICENSE.cmark-gfm` next to the plugin on Linux and Windows (`build/vm`, the `lib` directory of the Linux zip) and in `Pharo.app/Contents/Resources` on macOS, and in the section of cmark-gfm of the `THIRD-PARTY-NOTICES.txt` of the WebAssembly VMs (`pharo_wasm_dep_fetch(... LICENSES COPYING)`).
  There is no GPL part.
  `src/case_fold_switch.inc` is derived from Unicode's CaseFolding.txt and `src/entities.inc` from the WHATWG list of entities; `COPYING` does not cover them, and upstream ships no separate notice for them.
- The plugin is under the MIT licence of this repository; the packages of the submodule `cmark-gfm.st` under its MIT licence (its `LICENSE`).
  The fixtures of both are our own text: no part of the CommonMark or GFM specifications (CC-BY-SA) is copied.

This is not legal advice.

## Limitations

- 64-bit VMs only.
- macOS, Windows and Linux aarch64 are untested until CI runs the steps that load the plugin.
- The known bugs of cmark-gfm 0.29.0.gfm.14 are carried, unpatched, and pinned by the C tests:
  1. a footnote label that holds an inline (`[^a<b>c]`) is truncated, and the dropped source text disappears (a regression of gfm.14, upstream's own example 25 of `extensions.txt` fails); it needs footnotes, which are off in `CMarkOptions gfm`;
  2. `cmark_node_get_type_string` answers `"<unknown>"` for footnote nodes, so the XML render of a document with footnotes is not well-formed (`<<unknown> />`); the tree is not affected, since it is built from the codes;
  3. angle autolinks (`<https://...>`) get columns relative to their paragraph: the engine clamps them and sets CLAMPED, and their indices may point at the wrong characters of the line;
  4. GFM `www.` autolinks start one column early;
  5. in a table cell, the inlines after an escaped pipe start and end one column early per `\|` before them: the table extension unescapes the cell before it parses its inlines and keeps their offsets (a row `| x \| y *e* |` on line 3 puts the emphasis at 3:9-3:11 instead of 3:10-3:12, so its `sourceText` is `' *e'`; upstream's `--sourcepos` says the same; pinned by the positions test `escaped pipe (known bug 5)` of `enginetest.c` and by `CMarkParseTest>>testKnownEscapedPipeBug`); the columns stay inside the line, so CLAMPED does not catch them; the cell's own position is right.
- Columns are cmark-gfm's byte columns; the character indices are computed from them (see Text, positions and indices).
- What cmark-gfm keeps private is not in the tree: the bullet character (`-`, `*` or `+`), setext against ATX headings, the kind of an HTML block; autolinks are plain links, and tagfilter changes nothing in the tree.
- Positions are not monotonic in the order of the records: cmark-gfm moves the referenced footnote definitions to the end of the document, sorted by first reference, and drops the unreferenced ones.
- A document nested deeper than `maxDepth` (1000 by default) signals `CMarkTooDeep` and gives no tree; `maxDepth: nil` lifts the limit.
- There is no Smalltalk CommonMark writer: a tree built or changed by hand cannot be formatted by cmark-gfm.
- GitHub's alerts (`> [!NOTE]`) are a post-processing of GitHub's, not cmark-gfm's: they parse, and render, as plain block quotes.
- On WebAssembly a parse or a render runs within one slice: the page waits for it (see Blocking), and the C heap it grows is never given back (see Memory and limits).
- Without the plugin, only hand-built trees and their Roassal view are available.
- The Roassal view draws with the image's fonts only (no glyph fallback: CJK and emoji may show as boxes), does not highlight code, interpret HTML beyond `#basic`, fetch images from the network, re-lay out on resize, or select text.
