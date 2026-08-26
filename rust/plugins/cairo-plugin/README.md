# CairoPlugin, in Rust

Cairo as Pharo *named primitives*, instead of as an image-side FFI binding.

This is not a port. There is no C `CairoPlugin` and never was: nothing in
`src/`, `plugins/` or `include/` mentions Cairo. What the VM does today is
*ship* it — `cmake/importCairo.cmake` downloads a ready-made `libcairo` into
the directory beside the executable — and the image reaches it through UFFI.
This plugin makes the same library reachable as primitives, with the plugin
owning the Cairo objects and the image holding integer handles.

**The download rules are unchanged.** `importCairo.cmake` fetches exactly what
it fetched before. This crate binds whatever lands there.

## Why `dlopen` and not a link-time binding

The zip the build downloads contains runtime objects and nothing else — on
Linux/x86-64 it is `libcairo.so`, `libcairo.so.2`, `libcairo.so.2.11704.0`, and
that is all. No headers, no `.pc` file. There is nothing for a linker or for
`pkg-config` to consume, so `cairo-sys-rs` could not be used without adding a
`libcairo-dev` build requirement to a VM that already ships Cairo.

So `ffi.rs` declares Cairo's entry points by hand and resolves them with
`dlopen`, looking beside the executable first and falling back to the system
loader — the same order and the same file the image's FFI would have found.
Every entry is optional; a missing one costs its primitives, not the module.

If Cairo cannot be loaded at all, `initialiseModule` answers 0 and the VM
rejects the module. That is deliberate: the image can then tell "no Cairo
here" from "primitive not implemented" and stay on its FFI binding.

## Handles, not pointers

The image never sees a `cairo_t *`. Every surface, context and pattern lives
in a registry (`pharo_vm_plugin::handles::Registry`) and the image gets a
SmallInteger carrying a slot index and a generation counter.

That closes a hole the FFI binding cannot close. A stale `cairo_t` address
passed back from the image is dereferenced; a stale *handle* fails the
primitive with `PrimErr::NotFound` and the image runs its Smalltalk fallback.
Destroying twice fails the second time instead of freeing twice. And because
the generation counter moves on when a slot is reused, an old handle never
resolves to whatever took its place — unlike SurfacePlugin's surface IDs,
where it does.

```smalltalk
surface := self primImageSurfaceCreate: 0 width: 100 height: 100.
context := self primContextCreate: surface.
self primSetSourceRgba: context r: 1.0 g: 0.0 b: 0.0 a: 1.0.
self primPaint: context.
self primContextDestroy: context.
self primSurfaceDestroy: surface.
```

## Conventions

| in the image | on the wire |
|---|---|
| a handle | SmallInteger; 0 and negatives are never valid |
| a coordinate, a colour component | Float (SmallIntegers are accepted too) |
| a `cairo_format_t`, operator, line cap… | SmallInteger, **range-checked** |
| a `cairo_matrix_t` | ByteArray of 48 bytes: 6 native-endian doubles, in the header's order `xx yx xy yy x0 y0` |
| extents (x1 y1 x2 y2) | ByteArray of 32 bytes, filled by the primitive |
| text extents | ByteArray of 48 bytes: `x_bearing y_bearing width height x_advance y_advance` |
| font extents | ByteArray of 40 bytes: `ascent descent height max_x_advance max_y_advance` |
| a string | ByteString or Symbol; decoded as UTF-8, else Latin-1 |

Out-parameters are caller-supplied ByteArrays of an exact size, checked before
the call — pass one of the wrong length and the primitive fails without having
touched the context. Nothing crosses the boundary as an address.

Enumerations are range-checked because Cairo does not check them: an
out-of-range operator puts the context into a permanent error state and every
later call on it silently does nothing, which the image would notice as
drawing that stopped appearing rather than as a failed primitive.

## Pixels: two routes

`primitiveImageSurfaceCreate` has Cairo allocate the pixels, and
`primitiveImageSurfaceReadInto:` / `primitiveImageSurfaceWriteFrom:` copy them
to and from an image object, packed to `width * bytesPerPixel` per row
whatever stride Cairo chose. Safe, and it costs a copy per frame.

`primitiveImageSurfaceCreateForBitmap:` has Cairo draw straight into an image
object — the copy-free path Athens needs. The object is **pinned** for as long
as Cairo can reach it, and the stride must be one `cairo_format_stride_for_width`
produced (`primitiveFormatStrideForWidth:`), which the primitive checks along
with the object's size.

Releasing that pin is the subtle part. `cairo_surface_destroy` drops *our*
reference; a context or pattern made from the surface holds one of its own, so
the surface can outlive the image's handle and still be writing into the object
we pinned. Unpinning then would let the collector move memory Cairo is about to
write to. So the surface's reference count is consulted first, and if anything
else still holds one the pin is **kept** rather than released.
`primitiveRetainedPinCount` reports how many; anything but zero means the image
destroyed a surface before the contexts drawn on it.

## What is covered

**108 primitives** over 103 `cairo_*` entry points, chosen to cover what an Athens backend
asks of Cairo: image surfaces, contexts, the whole path and painting API,
sources and gradients, the graphics state, transformations, the four extents
calls and three hit tests, and the toy text API. `src/context.rs` is macro-
generated for the sixty-odd calls that differ only in name and arity.

## Not covered

* **Scaled fonts and glyphs.** `cairo_show_glyphs`, `cairo_scaled_font_*`,
  font options. This is where a real text stack lives, and binding it means
  binding `cairo_glyph_t` arrays as well — its own piece of work, and the
  image's font handling has to move with it. The toy API (`cairo_show_text`,
  `cairo_text_extents`) is here and is enough to draw a string.
* **Non-image surfaces**: PDF, SVG, PostScript, and the X11/Quartz/Win32
  backends.
* **Paths as data**: `cairo_copy_path` and `cairo_append_path`, which need
  `cairo_path_t` marshalled both ways.
* **Regions**, `cairo_device_t`, `cairo_raster_source_*`, mesh patterns.

None of these is hard to add; they were left out to keep the first cut
reviewable.

## Verification

`cargo test -p cairo-plugin` — 22 tests.

The unit tests cover what does not need Cairo: matrix marshalling, the struct
sizes the ByteArray conventions rest on, status mapping, format validation.

`tests/cairo_live.rs` is the part that matters, because the risk this crate
carries is a **mistranscribed signature** — `ffi.rs` was written by hand from
`cairo.h`, and nothing in this tree would catch an error in it. Those tests
were run against the real bundled binary: `cairo-1.17.4.zip` from
`files.pharo.org/vm/pharo-spur64/Linux-x86_64/third-party/`, the same artifact
`importCairo.cmake` downloads. All 103 declared entry points resolve
(`every_entry_point_this_plugin_declares_really_exists`), and painting, filling,
matrix field order, `user_to_device`, and drawing into borrowed memory were
asserted per-pixel.

They **skip** when no Cairo can be loaded, which is the case on a bare build
machine — `cargo test` has no VM executable to sit beside. To run them for
real, put a `libcairo.so.2` on the loader path:

```
LD_LIBRARY_PATH=<dir with libcairo.so.2> cargo test -p cairo-plugin
```

### Not verified

* **Nothing has run in an image.** No primitive here has been called through
  the VM, because the primitives need an interpreter and no built VM existed
  when this was written. The Cairo calls are verified; the primitive wrappers
  around them are not.
* **The pinned-bitmap path** (`primitiveImageSurfaceCreateForBitmap:`) has
  been exercised in its Cairo half — a surface over borrowed memory writes into
  that memory — but not in its pinning half, which needs a live object memory.
* **Only Linux.** The library-name list for macOS and Windows is written but
  untried.
* **No image-side backend exists.** This plugin is only half the change: an
  Athens backend calling these primitives instead of UFFI still has to be
  written, and until it is, nothing in Pharo uses this.
