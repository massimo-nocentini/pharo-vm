# PangoPlugin, in Rust

Pango text layout as Pharo *named primitives*, instead of as an image-side FFI
binding.

This is not a port. There is no C `PangoPlugin` and never was, and unlike Cairo
and SDL the VM does not even ship Pango: nothing in `cmake/` downloads it and
nothing in `src/`, `plugins/` or `include/` mentions it. Pango is whatever the
machine has installed. This crate binds it as primitives, with the plugin
owning the Pango objects and the image holding integer handles.

**Nothing downloads Pango, and that is why `FEATURE_LIB_PANGO` defaults OFF.**
`FEATURE_LIB_CAIRO` and `FEATURE_LIB_SDL2` default ON because their flag means
two things at once — *download this library into the bundle* and *build the
plugin that binds it* — so the flag makes its own precondition true. Pango's
flag can only ever mean the second half. An ON default would ship, in every
bundle, a plugin whose `initialiseModule` answers 0 on any machine without a
system Pango, and would make bundle behaviour depend on what the *build*
machine happened to have installed.

## Why `dlopen`, and where it looks

Same reason as `cairo-plugin`, arrived at from the opposite direction: a
link-time binding through `pango-sys` would put a `libpango1.0-dev` on every
machine that builds the VM, in exchange for a library the VM does not use.

So `ffi.rs` declares Pango's entry points by hand and resolves them with
`dlopen`. Everything is resolved from **libpangocairo**, not from libpango:
pangocairo links libpango, so one handle answers both, and the plugin needs
pangocairo anyway to draw. Every entry is optional; a missing one costs its
primitives, not the module.

The candidate list per library is: an environment override
(`PHARO_PANGOCAIRO_LIBRARY`, `PHARO_PANGO_LIBRARY`, `PHARO_GOBJECT_LIBRARY`,
`PHARO_GLIB_LIBRARY`) if set; then the SDK's usual bundled-then-bare names; then,
on non-Windows, absolute paths under `/opt/homebrew/lib`, `/usr/local/lib`,
`/opt/local/lib`, `/usr/lib64`, `/usr/lib`, and the keg-only
`/opt/homebrew/opt/{pango,glib}/lib`.

That last group is not belt-and-braces. It is measured: on macOS a bare-name
`dlopen("libpangocairo-1.0.0.dylib")` does **not** find Homebrew's copy, because
dyld searches no equivalent of `ld.so.conf` and nothing bundles Pango beside the
executable. Without the absolute candidates the plugin declines on a machine
that plainly has Pango installed, and the natural response — "Pango must not be
installed" — is wrong and costs a day. `primitiveLibraryPath` reports which
candidate actually hit, which turns "it declined" into "it declined and here is
where it looked".

If libpangocairo cannot be loaded, or libglib/libgobject cannot, or `g_free`
and `g_object_unref` are missing, `initialiseModule` answers 0 and the VM
rejects the module. That is deliberate and it matters more here than for Cairo:
a plugin that loaded anyway would answer `Unsupported` from all 184 primitives,
which the image cannot tell from a primitive that is merely unimplemented — so
it would have no way to decide whether to fall back to its own FFI binding.

## Handles, not pointers

The image never sees a `PangoLayout *`. Six registries
(`pharo_vm_plugin::handles::Registry`) hold font maps, contexts, layouts, font
descriptions, attribute lists and tab arrays, and the image gets a SmallInteger
carrying a slot index and a generation counter. A stale handle fails its
primitive with `NotFound`; a stale pointer, which is what an FFI binding passes
today, is dereferenced. Handle 0 is never live and always means NULL.

Four release verbs, and which one applies is a property of the type, not of the
call site: `g_object_unref` for font maps, contexts and layouts;
`pango_font_description_free`; `pango_attr_list_unref`; `pango_tab_array_free`.
A registry entry knows its own verb, which is why a `Drop` is the only place
any of them is called.

**A font map additionally records whether the plugin owns its reference.**
`pango_cairo_font_map_get_default` is transfer=NONE and answers a process-wide
singleton sitting at refcount 1; its neighbour `pango_cairo_font_map_new` is
transfer=FULL. One stray unref of the first destroys text rendering for the
whole process, and the crash lands in whatever touches a font next. So the
default map is registered *borrowed* and destroying that handle retires the
handle and releases nothing.

`primitiveLiveResourceCounts` answers the six registry lengths in that order,
for leak-hunting from the image side.

```smalltalk
"Measure a string. Nothing here draws, and nothing here needs CairoPlugin:
 measurement is the half of Pango that always works."
fontMap := self primDefaultFontMap.
context := self primFontMapCreateContext: fontMap.
layout  := self primLayoutNew: context.

desc := self primFontDescriptionFromString: 'Cantarell 11'.
self primLayoutSetFontDescription: layout with: desc.
self primLayoutSetWidth: layout to: 300 * self primScale.   "Pango units"
self primLayoutSetWrap: layout to: 2.                       "WORD_CHAR"
self primLayoutSetText: layout text: 'Hello, Pango — ligatures and all'.

extent := self primLayoutGetPixelSize: layout.              "a Point"
lines  := self primLayoutGetLineCount: layout.

"Destroy in any order: each handle knows its own release verb, and a
 handle destroyed twice fails the second time rather than freeing twice."
self primFontDescriptionDestroy: desc.
self primLayoutDestroy: layout.
self primContextDestroy: context.
self primFontMapDestroy: fontMap.              "borrowed: releases nothing"
```

## Conventions

| in the image | on the wire | why |
|---|---|---|
| a handle | SmallInteger; 0 is "none/NULL" where a primitive documents it, and is never a live handle | `Registry` generations start at 1, so 0 is free to mean NULL for the four arguments Pango marks nullable: the **layout**'s `set_attributes`, `set_font_description` and `set_tabs`, and `get_metrics`'s desc. `primitiveContextSetFontDescription` looks like a fifth and is not — the gir does not mark it nullable and Pango asserts `desc != NULL` — so 0 fails there like any dead handle |
| a `PangoRectangle` | **Array of 4 SmallIntegers** `#(x y width height)` | four *signed* 32-bit ints, and x, y and width are routinely negative — `index_to_pos` answers a negative `width` for an RTL grapheme. The SDK's only word writer, `Interp::write_words`, takes `&[u32]`, so a ByteArray would force the image to re-sign, which is exactly the decoding step that gets got wrong once |
| two rectangles | one **8-element Array**, ink then logical (or strong then weak) | one primitive, not two, so the image cannot pair a rectangle with a layout it has since changed |
| a size | a **Point**, via `Interp::point` | `get_size`/`get_pixel_size` are a natural Point and Pharo has one |
| a matrix | ByteArray of 48 bytes: 6 native-endian doubles in **Pango's order `xx xy yx yy x0 y0`** | *not* CairoPlugin's order. The middle two are transposed between the two libraries |
| a distance, an extent, a font size | SmallInteger in **Pango units** unless the primitive's name says `Pixel` | the plugin does no unit arithmetic: `PANGO_SCALE` is a macro with no symbol and the header calls it changeable |
| the scale factor | `primitiveScale`, answering `pango_units_from_double(1.0)` | so the image computes rather than hard-codes 1024 |
| unit conversion | `primitiveUnitsFromDouble:` / `primitiveUnitsToDouble:` | the exported functions, not arithmetic |
| pixels | only from `primitiveLayoutGetPixelSize`, `…GetPixelExtents`, `primitiveLineGetPixelExtents` | exactly three functions in Pango speak device pixels; `get_pixel_extents` rounds *outwards* and so is not `PANGO_PIXELS(get_extents)` |
| a string, going in | ByteString or Symbol, transcoded to **UTF-8**, length passed explicitly | Pango requires valid UTF-8 and a Pharo ByteString is Latin-1. `Interp::string_value` transcodes; `c_string_value` would pass raw bytes and every accented character would render as a placeholder box. The length is passed rather than `-1` because a Smalltalk String is not NUL-terminated and may contain a NUL |
| a string, coming out | ByteString of UTF-8 bytes | `pango_layout_get_text` answers Pango's own UTF-8 copy |
| a byte index | SmallInteger, **byte offset into the UTF-8 the plugin gave Pango** | the indices Pango reports do not index the image's string. The image must index against `primitiveLayoutGetText`. Every index primitive says `Index` for bytes |
| a character position | only `primitiveLayoutGetCharacterCount` | the one place Pango counts characters. `pango_layout_get_direction` looks like a second one and is not: its `index` is documented "the byte index of the char", and measured it partitions a Hebrew letter's two bytes, so it is exposed as `primitiveLayoutDirectionAtIndex` |
| a Boolean | Boolean in, Boolean out | `gboolean` is `gint` — four bytes — everywhere inside the plugin. The image never sees the int |
| an enum | SmallInteger, **range-checked**, and version-gated where the range grew | Pango's setters do no checking and its switch statements do not handle out-of-range values. `set_wrap(l, 3)` on a pre-1.56 Pango stores a value that falls through every case in the line breaker |
| a weight, a width | SmallInteger in a **continuous range** — weight 100..1000, width 500..2000 | the header states weight is "a numeric value ranging from 100 to 1000"; membership in the twelve named constants would reject a legal 450 from a variable font's weight axis |
| a colour channel | SmallInteger 0..65535 | `PangoColor` is three `guint16` with no alpha. 8-bit to 16-bit is `v * 257`, not `v << 8` — the image does that conversion, and `0xFF * 257 = 65535` is why |
| a sentinel | passed through untouched: `-1` for "unset" width, negative height for a line count, negative indent for a hanging indent, `-1`/`2147483647` for cursor movement past the ends | none of these is scaled, clamped, or rejected for being negative |
| "no answer" from a nullable getter | `nil`, not 0 | a family with no face, a description with no family set, a context with no matrix: the absence is the answer, and 0 is a legal value for several of them |
| a failure | `PrimErr` → the image's Smalltalk fallback | `Unsupported` = this Pango is too old or the library is absent; `NotFound` = dead handle; `BadIndex` = out of range or not a UTF-8 boundary; `BadArgument` = enum out of range, invalid UTF-8, interior NUL; `OperationFailed` = Pango said no (markup parse, `add_font_file`); `Inappropriate` = the Cairo context has latched an error |

**Why an Array of SmallIntegers rather than a caller-supplied ByteArray, when
`cairo-plugin` chose the ByteArray.** Cairo's extents are `double`s and the SDK
has `write_f64s`, so the ByteArray is free and the image reads it with
`floatAt:`. Pango's are signed `int`s, the SDK's only word writer is unsigned,
and there is no signed-word path. Adding one to the SDK for this is more change
than it is worth; allocating a 4-element Array per measurement call is a cost
the image will not notice next to laying out text.

`PANGO_SCALE` is 1024 today and the header says it may change, so nothing in
this crate hard-codes it and the image should not either:
`primitiveScale` answers `pango_units_from_double(1.0)`, and
`primitiveUnitsFromDouble:` / `primitiveUnitsToDouble:` do the conversion.

**Pango's matrix is not Cairo's.** `PangoMatrix` is `xx xy yx yy x0 y0` and
`cairo_matrix_t` is `xx yx xy yy x0 y0` — elements 1 and 2 swapped. A pure
scale looks identical in both, which is exactly why the mistake ships: it
surfaces only once something is rotated or sheared. `PangoMatrix` has explicit
`to_pango_array` / `from_cairo_slice` conversions rather than a cast.

**Enumerations are range-checked because Pango does not check them.** An
out-of-range value falls through Pango's switch statements silently — often it
is simply *stored*, and the layout then behaves as though a value it never saw
had been set. Several ranges also grew with the library (`PangoVariant` at
1.50, `PANGO_WRAP_NONE` at 1.56, `PangoTabAlign` past LEFT at 1.50), so those
are checked against `pango_version()` and answer `Unsupported`, not
`BadArgument`, on an older Pango: the image can then decide, rather than being
told its argument was wrong.

Four wire shapes are worth naming individually, because the image cannot guess
them:

* `primitiveLayoutXyToIndex` and `primitiveLineXToIndex` answer a 3-element
  Array `#(hit byteIndex trailing)` whose **first element is a Boolean object**,
  not 0 or 1.
* `primitiveLayoutGetLogAttrs` answers a **Bitmap** of raw 32-bit words, one per
  character plus one, not an Array. The bit numbering is written out in that
  primitive's doc comment.
* `primitiveTabArrayGetTabs` answers a 2N-Array of **interleaved** pairs
  `#(align0 loc0 align1 loc1 …)`, so element pair `2*i` is what
  `primitiveTabArrayGetTab:` answers for index `i`.
* `primitiveParseMarkup` answers `#(attrListHandle plainText accelCharOrNil)`.

And two behaviours the image has to wrap rather than discover:

* `primitiveLayoutSetText:` does **not** clear attributes a previous
  `setMarkup:` left behind — Pango's own documentation says so — so any layout
  that has ever held markup must be followed by
  `primitiveLayoutSetAttributes: layout with: 0`.
* `primitiveTabArraySetPositionsInPixels:` reinterprets and does not rescale.
  An array built in Pango units and then flipped to pixels has every stop 1024
  times too far out. That is Pango's behaviour, not this plugin's.

Byte indices into text cannot always be validated here: an attribute list
carries no text, so `primitiveAttrListSplice:` has no string against which to
check that its offset lands on a character boundary. The image has the string
and must do that check.

## Drawing: the bridge into CairoPlugin

Pango does not draw. `pango_cairo_show_layout` takes a `cairo_t *`, and the
`cairo_t *` belongs to `CairoPlugin`, which holds it behind a handle of its own
for exactly the reasons above. So the twelve `render.rs` primitives take a
*CairoPlugin* handle and borrow the pointer across a small versioned C ABI:
`ioLoadFunctionFrom` resolves `cairoPluginBorrowContext_v1` out of the other
plugin, which fills a 32-byte `#[repr(C)]` struct or refuses.

The borrow never transfers ownership. It is valid for one primitive call, is
never referenced or destroyed here, and is asked for again next time — which
re-resolves the handle, so a context the image has destroyed answers "refused"
rather than a freed pointer. After drawing, `cairoPluginContextStatus_v1` is
consulted, because Cairo latches an error and then silently ignores every later
call; without that check a failed `show_layout` would answer success.

**The handshake refuses unless both plugins resolved the same Cairo.** Two
mapped copies of Cairo, with a `cairo_t *` passed between them, is silent
intermittent corruption inside Cairo rather than anywhere a stack trace would
help — so the bridge compares the address of `cairo_create` as *this* plugin's
libpangocairo resolved it against the address `CairoPlugin` publishes, and
declines when they differ, naming both libraries.

**On Homebrew macOS today it does differ, and text does not draw.**
libpangocairo hard-references `/opt/homebrew/opt/cairo/lib/libcairo.2.dylib` by
absolute install name, while `CairoPlugin` opens the bundle's
`@executable_path/Plugins/libcairo.2.dylib`. All twelve drawing primitives
answer `Unsupported` and `primitiveCairoBridgeStatus` names both files. That is
the correct behaviour and not a defect to work around; it means Pango text
renders only where a Pango built against the bundled Cairo is installed.
`primitiveCairoBridgeStatus` answers an empty String when the bridge works and
a sentence naming one of five reasons when it does not;
`primitiveCairoBridgeRefresh` drops the cached answer, and `moduleUnloaded`
drops it automatically when `CairoPlugin` goes away.

So the image asks before it draws, and has somewhere to go when the answer is
no:

```smalltalk
"Draw, having measured. `cr` is a CairoPlugin context handle."
reason := self primCairoBridgeStatus.
reason isEmpty ifFalse: [ ^ self drawTextTheOldWay: reason ].

"Pen, source, transform and clip are Cairo's state, so the image sets them
 through CairoPlugin; this plugin only draws."
CairoPlugin primMoveTo: cr x: 2.0 y: 2.0.
CairoPlugin primSetSourceRgba: cr r: 0.0 g: 0.0 b: 0.0 a: 1.0.

self primCairoUpdateLayout: cr layout: layout.
self primCairoShowLayout: cr layout: layout.
```

`primitiveCairoUpdateLayout` first, always: it re-reads the context's font
options and transform into the layout, and a layout measured before a scale was
applied lays out for the wrong device otherwise. `primitiveCairoShowLayout`
then answers `Inappropriate` if the context had already latched an error and
`OperationFailed` if this call is what latched one — the two cases the image
would otherwise see as text that simply stopped appearing.

## What is covered

**184 primitives** over 264 `pango_*` and 8 `g_*` entry points:

| module | primitives | area |
|---|---|---|
| `lib.rs` | 10 | availability, library paths, version, `PANGO_SCALE`, diagnostics |
| `fontmap.rs` | 34 | font maps, contexts, font enumeration, metrics, languages |
| `font.rs` | 33 | `PangoFontDescription` |
| `layout.rs` | 58 | `PangoLayout`: text, markup, geometry, extents, cursors |
| `lines.rs` | 10 | per-line access by `(layout, index)` |
| `markup.rs` | 12 | `pango_parse_markup`, `PangoAttrList`, `PangoColor` |
| `tabs.rs` | 12 | `PangoTabArray` |
| `render.rs` | 12 | pangocairo drawing, through the bridge |
| `cairo_bridge.rs` | 3 | bridge status, path, refresh |

The table declares more entries than the surface exposes, on purpose: the whole
`_static` family, the eighteen `pango_attr_*_new` constructors, the
serialisation pair and the font-loading entries are declared so that
`missing_entry_points()` checks their *names*, with no primitive over them.

## Not covered

* **Glyph-level APIs.** `PangoGlyphString`, `PangoItem`, `pango_itemize`,
  `pango_shape`, `pango_cairo_show_glyph_string`. A real shaping pipeline, and
  its own piece of work.
* **`PangoLayoutIter`**, and line *handles*. An iterator is invalidated by any
  change to its layout and a handle cannot express that, so no
  `PangoLayoutLine *` ever leaves a primitive: line access is by
  `(layout, index)`, range-checked against `pango_layout_get_line_count` on
  every call.
* **Hand-built attribute lists.** Markup covers the demand; the eighteen
  constructors are declared but unexposed, so adding them is a v2 decision and
  not a v2 discovery.
* **`PangoRenderer`, `PangoCoverage`, `PangoFontset`, `PangoScript`,
  `PangoBidiType`**, and the break/log-attr API beyond `get_log_attrs`.
* **Serialization** (`pango_layout_serialize`, 1.50+). Declared, unexposed.
* **Non-Cairo backends**: `pangoft2`, `pangowin32`, `pangoxft`.
* **A diagnostic channel for GError messages.** `pango_font_map_add_font_file`
  and the markup parsers produce one; `markup.rs` keeps the last markup error
  (`primitiveLastMarkupError`), written by all three markup entry points —
  `primitiveParseMarkup` and both `setMarkup:` forms — but the font-file path
  only answers `OperationFailed`.

## Verification

`cargo test -p pango-plugin` — 77 tests, all of which ran and passed against
Pango 1.58.2 from Homebrew.

The unit tests (44) cover what needs no Pango: `PangoRectangle`'s sixteen bytes
and field offsets, `PangoColor`'s six, `GError`'s measured layout, `gboolean`
being four bytes, the Pango-vs-Cairo matrix field order with a round trip both
ways *and* a demonstration that a pure-scale matrix looks identical in both,
`PANGO_PIXELS`' rounding including its `+512 -> 1` / `-512 -> 0` asymmetry, the
platform-shaped library-name list, the version-gate table, and the narrowing
and enum helpers.

`tests/exports.rs` (8) covers the module contract with no Pango installed:
the module name, the null-proxy refusal, the clean decline, and that no
resource is registered before the image asks for one.

`tests/bridge.rs` (7) covers the bridge's *negative* half by faking the VM
proxy — a zeroed function table is a VM that supports nothing, and one with only
`ioLoadFunctionFrom` filled in is a `CairoPlugin` that is present but exports no
bridge. That is what makes "the cached negative was really dropped" observable:
the same question asked through two different fake VMs must change its answer
across `moduleUnloaded`, and must not change without one.

`tests/pango_live.rs` (18) is the part that matters, because the risk this crate
carries is a **mistranscribed signature**: `ffi.rs` was written by hand and a
`c_double` where the header says `float`, or a Rust `bool` where it says
`gboolean`, compiles, links, resolves and then answers garbage. So:
`line_spacing` round-trips through 1.5 *and* 0.1, the second of which a
`c_double` declaration could not answer; `pango_color_parse("#3366cc")` answers
`red == 13107`, which a `c_int` channel would misalign; `pango_units_from_double(1.0)`
answers 1024; a `gboolean` crosses whole in both directions; the borrowed
default font map survives being destroyed by the image; a thousand
create/destroy pairs are exactly neutral in the registries; owned and borrowed
strings are released by opposite rules; and every declared entry point resolves
except those `entries_introduced_after(pango_version())` excuses — the gate
being what separates "misspelt" from "this Pango is older".

Two of the eighteen are about an **index**, which is the same hazard wearing a
different hat: `pango_layout_get_direction` is addressed in bytes, not in
characters, and on ASCII the two are the same number, so a test written in
English could never tell. `get_direction_partitions_a_layouts_text_by_bytes_not_by_characters`
lays out `a` + U+05E9 + `b` — three characters, four bytes — and asserts that
the Hebrew letter's *two* byte offsets answer alike and differently from the
Latin either side; on a character reading, index 2 would be the `b`. Its
neighbour pins the bound `layout.rs` checks every other index against: the
`strlen` of `pango_layout_get_text` is Pango's own `layout->length`, the end
position is a legal caret, and the character count is not the bound.

One of the eighteen is an **end-to-end render**, and it is the one that proves
the stack rather than a signature: it lays "Hello, Pango" out on a real ARGB32
image surface, counts the pixels that changed, and checks that the ink lands
inside the box `get_pixel_size` promised. It draws an *empty* layout first as
its control, so ink counted afterwards cannot be something else painting the
surface. There is no `CairoPlugin` inside a test binary and no bundle to load
one from, so the surface is made from the Cairo reached through the very
libpangocairo the plugin resolved — re-opening a mapped image gets the loader's
same copy, which is exactly the identity the bridge insists on at runtime, and
`the_cairo_reached_through_pangocairo_is_the_one_pangocairo_is_linked_against`
confirms with `dladdr` that it really lands in a Cairo.

They **skip** when no Pango can be loaded, which is an ordinary configuration
for this plugin. Set `PANGO_TESTS_REQUIRED=1` to turn every skip into a failure:

```
PANGO_TESTS_REQUIRED=1 cargo test -p pango-plugin
```

That is what a CI job which has just installed `libpango-1.0-0` and
`libglib2.0-0` wants, because a runner image that ships Pango *incidentally* is
the worst case — the suite would silently stop exercising the transcribed
signatures the day the image dropped it, and nothing would go red.

### Not verified

* **Nothing has run in an image.** No primitive here has been called through the
  VM: the primitives need an interpreter and no built VM existed when this was
  written. The Pango calls are verified; the primitive wrappers around them are
  not.
* **The bridge's positive half.** `pango_cairo_show_layout` is exercised against
  a live `cairo_t` by the render test, so the pangocairo calls themselves are
  verified — but the *borrow* is not: no `cairo_t` has ever crossed from
  `CairoPlugin` into these primitives, which needs a VM with both plugins
  loaded, and on this machine would be refused by the identity handshake
  anyway. `tests/bridge.rs` covers the refusal path instead, which is the path
  this machine actually takes.
* **Only macOS, and only Homebrew's Pango 1.58.2.** The Linux and Windows name
  lists are written but untried, and every version gate below 1.58 is reasoned
  rather than observed.
* **No image-side backend exists.** This plugin is half the change: a text
  backend calling these primitives instead of UFFI still has to be written, and
  until it is, nothing in Pharo uses this.
