# SDL3Plugin, in Rust

SDL3 as Pharo *named primitives*, instead of as an image-side FFI binding.

Like the sibling `cairo-plugin`, this replaces nothing: there is no C
SDL plugin, and nothing in `src/` or `plugins/` mentions SDL. The VM *ships*
SDL — `cmake/importSDL2.cmake` downloads it beside the executable — and
OSWindow reaches it through UFFI. This plugin makes the same library reachable
as primitives, with the plugin owning the SDL objects and the image holding
integer handles.

**The download rules are unchanged.**

## On Linux there is no SDL3 to bind, yet

`cmake/importSDL2.cmake` fetches `SDL3-3.4.10` for Windows and macOS only. The
Linux branch still fetches SDL2 alone, and
`files.pharo.org/.../Linux-x86_64/third-party/SDL3-3.4.10.zip` does not exist.

So on Linux this plugin will find nothing and `initialiseModule` will answer 0,
the VM will reject the module, and the image stays on its SDL2 FFI binding.
That is the designed behaviour, not a failure — but it does mean the Linux
half of this needs `importSDL2.cmake` to start downloading SDL3 (or build it
from source, as `build_SDL2()` already does for SDL2) before it is useful there.

## Why `dlopen`

Same reason as Cairo: the downloaded zip holds runtime objects only, no headers
and no `pkg-config` metadata, so there is nothing to link against. `ffi.rs`
declares SDL3's entry points by hand and resolves them at load time, looking
beside the executable first. Every entry is optional.

## Handles, and SDL's cascade

Windows, renderers and textures live in registries; the image gets
SmallInteger handles that carry a generation counter, a type tag and a session
byte, so a stale one -- or one belonging to another of the three registries --
fails the primitive instead of dereferencing freed memory.

SDL makes this harder than Cairo does, because its objects are **not** reference
counted: destroying a renderer destroys the textures made from it, and
destroying a window destroys its renderer. A handle can therefore be
invalidated by something other than its own destroy primitive. So
`primitiveDestroyWindow` walks the registries and invalidates the renderer and
textures SDL is about to free — otherwise a later `primitiveRenderClear:` would
hand SDL a dangling pointer and the "handles cannot dangle" promise would be a
lie.

`primitiveQuit` releases everything before calling `SDL_Quit`, for the same
reason. Note that module shutdown deliberately does *not* call `SDL_Quit`: the
image may have brought SDL up by another route.

### Divergences: what the handle encoding does and does not promise

A handle carries a **type tag** as well as a slot index and a generation, so a
texture handle passed to a window primitive fails with
`PrimErr::BadArgument` instead of resolving. Before the tag every registry here
shared one encoding, and the first insert into each answered the *same*
integer — a live bug, reproducible on the first two objects of every session.
The tags are declared once in `resources.rs` through
`pharo_vm_plugin::resource_tags!`, which proves them distinct at compile time.

Two consequences worth knowing:

* **`primitiveWindowIsLive` answers `false` for a live resource of the wrong
  kind.** An image that used to read `true` there now reads `false`. That is the
  fix rather than a regression — a texture is not a live window — but it is image-visible.
* **On a 32-bit image there is no session byte.** A handle the image saved in an
  inst var and replayed after a restart is caught on 64-bit (the handle carries
  the low byte of `getThisSessionID`, 255/256 detection) and is **not** caught on
  32-bit, where the 30 available magnitude bits go entirely to index, generation
  and a 4-bit tag. See `pharo_vm_plugin::handles` for the arithmetic.

## Threading

SDL requires video and event calls on the thread that initialised video. Every
primitive runs on the VM's interpreter thread, so as long as the image drives
SDL only through this plugin, that holds by construction. Mixing this plugin
with image-side SDL FFI calls from another Smalltalk process on another native
thread would break it.

## Conventions

| in the image | on the wire |
|---|---|
| a handle | SmallInteger; 0 and negatives are never valid |
| a window/renderer size | answered as a `Point` |
| flags, formats, enumerations | SmallInteger; enumerations are range-checked |
| an `SDL_FRect` | ByteArray of 16 bytes, four native-endian `f32`; **nil means NULL** |
| an `SDL_Rect` | ByteArray of 16 bytes, four native-endian `i32`; nil means NULL |
| a mouse position | ByteArray of 16 bytes, two native-endian doubles |
| a string | ByteString or Symbol; decoded as UTF-8, else Latin-1 |

`primitiveFRectPut:` and `primitiveRectPut:` fill those rectangle ByteArrays,
so the image never has to know that the render calls take single-precision
floats while `SDL_UpdateTexture` takes integers.

Failures follow SDL: a primitive whose SDL call answered `false` fails with
`PrimErr::OperationFailed`, and the message is in `primitiveGetError`.

## The display path

```smalltalk
self primInit: SDL_INIT_VIDEO.
window   := self primCreateWindow: 'Pharo' width: 800 height: 600 flags: 0.
renderer := self primCreateRenderer: window name: nil.
texture  := self primCreateTexture: renderer
                format: SDL_PIXELFORMAT_ARGB8888
                access: 1  "streaming"
                width: 800 height: 600.
"each frame"
self primUpdateTexture: texture rect: nil bits: form bits pitch: 800 * 4.
self primRenderClear: renderer.
self primRenderTexture: renderer texture: texture source: nil destination: nil.
self primRenderPresent: renderer.
```

`primitiveUpdateTextureFromBits:` reads straight out of the image object, with
no copy and no pinning — the call completes before the primitive returns, so
the collector never gets a chance to move anything. It checks that the object
holds at least `pitch * rect height` bytes, which SDL does not.

## Events

Two primitives, deliberately.

`primitivePollEventRaw:` copies the 128 bytes of `SDL_Event` verbatim into a
ByteArray. The union is explicitly padded to that size in the header, with a
compile-time assertion beside it, so it does not vary by platform. This is the
escape hatch: anything the plugin does not decode, the image can still reach,
exactly as its FFI binding does today.

`primitivePollEvent:` writes a fixed **64-byte record** instead, so no SDL
field offset leaks into the image:

| offset | width | field |
|---|---|---|
| 0 | u32 | event type (`SDL_EVENT_*`) |
| 4 | u32 | window id |
| 8 | u64 | timestamp, nanoseconds |
| 16 | i32 | `a` |
| 20 | i32 | `b` |
| 24 | i32 | `c` |
| 28 | i32 | `d` |
| 32 | f64 | `x` |
| 40 | f64 | `y` |
| 48 | f64 | `dx` |
| 56 | f64 | `dy` |

What `a`…`dy` carry, per event:

| event | a | b | c | d | x, y | dx, dy |
|---|---|---|---|---|---|---|
| `KEY_DOWN` / `KEY_UP` | scancode | keycode | modifiers | repeat (0/1) | — | — |
| `MOUSE_MOTION` | button state | mouse id | — | — | position | movement |
| `MOUSE_BUTTON_*` | button | clicks | mouse id | — | position | — |
| `MOUSE_WHEEL` | direction | mouse id | whole ticks x | whole ticks y | scroll amount | pointer position |
| `WINDOW_*` | data1 | data2 | — | — | — | — |
| `TEXT_INPUT` | text length in bytes | — | — | — | — | — |
| anything else | 0 | 0 | 0 | 0 | 0 | 0 |

An event the plugin does not decode still answers its type, window and
timestamp, so the image can route on the type and reach for the raw bytes
rather than losing it.

`TEXT_INPUT` is the one payload that does not fit: it carries a `const char *`
SDL owns and reuses. The decoder copies the text out and
`primitiveLastTextInput` answers it — **read it before the next poll.**

Sizes are answered by `primitiveEventRecordSize` and `primitiveRawEventSize`
rather than hard-coded twice.

## Verification

`cargo test -p sdl3-plugin` — 26 tests.

The risk this crate carries is not just a mistranscribed signature but a
mistranscribed **struct layout**: the event decoder reads fields at fixed
offsets out of a union, so a wrong offset is a silent misread rather than a
crash. So the layout test is not written from memory. Every size and offset in
`ffi::tests::event_structs_have_the_headers_layout` was taken from a C program
compiled against the SDL 3.4.10 headers, printing `sizeof` and `offsetof` for
each member, and compared against what Rust produces for the declarations here.

`tests/sdl3_live.rs` was run against a real SDL3 built from the
`release-3.4.10` source — the same version `importSDL2.cmake` names. All 52
declared entry points resolve; a window, renderer and texture were created and
driven through the whole display path headless (`SDL_VIDEODRIVER=dummy`), the
error string was checked to round-trip, and the decoded-event tests drive the
decoder over hand-built structs.

Those live tests **skip** when SDL3 cannot be loaded, which on Linux is the
normal case. To run them:

```
LD_LIBRARY_PATH=<dir with libSDL3.so.0> cargo test -p sdl3-plugin
```

### Not verified

* **Nothing has run in an image.** The primitives need an interpreter and no
  built VM existed when this was written. The SDL calls are verified; the
  primitive wrappers around them are not.
* **No real video driver.** Everything was exercised under the dummy driver.
  Nothing has been on a screen.
* **No real events.** The decoder was driven over structs built by hand, not
  over events SDL produced — `SDL_VIDEODRIVER=dummy` generates none. The
  layouts are checked against the headers, but the mapping from a real
  keystroke to a record has not been observed.
* **Only Linux, and only against an SDL3 built here.** Not the binary the
  bundle ships, because on Linux the bundle ships none.
* **No image-side backend exists.** An OSWindow backend calling these
  primitives instead of UFFI still has to be written.

## Not covered

**63 primitives** over 52 `SDL_*` entry points. The scope is what OSWindow
asks of SDL: a window, a renderer, a texture to blit a Form through, and the
event queue.

Left out: audio, gamepads, joysticks, haptics, sensors, cameras, the GPU API,
render targets, `SDL_Surface`, cursors beyond show/hide, displays and display
modes, message boxes, file dialogs, properties, and the drawing primitives
(`SDL_RenderLine`, `SDL_RenderFillRect`, …).
