# The Bloc image

The Bloc image is the stock Pharo 15 image (build 117, the default image of the site) with [Toplo](https://github.com/pharo-graphics/Toplo) and what it builds on, loaded from their releases:

| Project | Version | What it is |
|---|---|---|
| Toplo | v0.12.0 | widgets: buttons, labels, lists, trees, tables, menus, tab panes, themes |
| [Bloc](https://github.com/pharo-graphics/Bloc) | v3.1.0 | elements, spaces, layouts, events, animations, text |
| [Alexandrie](https://github.com/pharo-graphics/Alexandrie) | v2.11.0 | the canvas of Bloc, on cairo, FreeType and HarfBuzz through the FFI |
| [Album](https://github.com/pharo-graphics/Album) | v0.12.0 | the text editor of Bloc |
| XMLParser | v3.6.2 | for the SVG of Bloc |

and two packages of this repository, `packaging/bloc/src`:

- `PharoBloc-Web`: a space of Bloc in a Morphic host draws into a manual surface of SurfacePlugin, which the WebAssembly VM has not; there it draws into a `PharoBlocMorphicForm` instead (see In the browser).
- `PharoBloc-Welcome`: the window that the image shows the first time that it is opened with a display, itself a space of Bloc with Toplo's widgets, whose buttons open Toplo's mini browser, workspace and login page, and a browser on `ToSandBox`, whose class side has some hundreds of examples. `PharoBlocWelcome open`, or Help > Bloc and Toplo, opens it again.

The site of GitHub Actions offers it as "Pharo 15 (bloc)" (id `bloc`), and the release attaches it as `PharoBloc-<version>-Pharo15.zip`.

## In the browser

The world page starts the image as it starts any other, and a space of Bloc opens there as a window of the world: `BlMorphicWindowHost` is the host that Bloc prefers when the UI is Morphic and SDL2 is not there, as in the world.
Two things make it draw:

- The FFI registry of the VM ([WebAssembly.md](WebAssembly.md), The FFI) has the functions that Alexandrie binds: 198 of cairo, 10 of FreeType and 57 of HarfBuzz (of which 62, 4 and 55 only Alexandrie binds), but for the Quartz and Win32 surfaces of macOS and Windows (4 of cairo), the HarfBuzz ones only in a build with `WASM_PANGO=ON`, which builds HarfBuzz (off by default, on for the site of GitHub Actions; Bloc draws without HarfBuzz, which Alexandrie uses for its shaping examples and tests), with the placeholders that its library finders look for before they load a library (`libcairo.so.2`, `libfreetype.so.6`, `libharfbuzz.so.0`).
  The libraries are those of the VM: cairo 1.18.6, FreeType 2.15.0, HarfBuzz 15.2.0, the versions of the native cairo stack ([CairoStack.md](CairoStack.md)).
  An element with a border, a rounded rectangle and a text, and a Toplo button, drawn by `aeAsForm`, have the same pixels in the node VM as natively, with the native cairo stack (60 ms natively, 159 ms in node for the element, at its first drawing).
- The Morphic host of Bloc keeps two buffers per space, `BlExternalForm`s: manual surfaces of SurfacePlugin over memory outside the image, which cairo draws into and BitBlt copies from through the plugin.
  The WebAssembly VM has no SurfacePlugin (`cmake/plugins.cmake` says why), so `PharoBloc-Web` replaces `BlAbstractMorphicHostSpace>>#newExternalForm`: on a VM where making a manual surface fails (asked once per session), it answers a `PharoBlocMorphicForm`, a 32-bit Form whose bits are pinned, which cairo draws into through the `void *` of `cairo_image_surface_create_for_data`, and which BitBlt copies as any Form; elsewhere it answers Bloc's form, as Bloc's method does.
  The pixels are the same: cairo's ARGB32, native-endian and premultiplied, which BitBlt takes as depth 32.
  The tests of the Morphic host and of the Spec presenters of Bloc (`BlMorphicHostTest`, `SpBlocPresenterTest`, `SpBlSpaceInMorphPresenterTest`), which fail in the node VM without it, pass with it.

On a native VM nothing changes: Bloc opens its spaces in windows of their own through SDL2, or in the world with `BlMorphicWindowHost`, and its forms are manual surfaces.

## Building

    sh packaging/bloc/build.sh <vm-dir> Pharo15.0-SNAPSHOT.build.117.sha.053a562682.arch.64bit.zip PharoBloc.zip

is `packaging/batteries/build.sh` ([Batteries.md](Batteries.md)) with the `load.st` and `smoke.st` of `packaging/bloc`, whose lines begin with `Bloc`, and the image named `Pharo15-bloc`: the same arguments, steps and environment.
`load.st` loads Toplo with Epicea off, as its install page does, clones by HTTPS (`BATTERIES_ICEBERG_REMOTE`), installs the two packages, checks the classes and that no load left a new Undeclared variable but `BlLazyElement` (Album v0.12.0 names it in `AlbTextAdornmentAttributeTaskItRenderingStrategy>>#affect:in:`, which nothing calls; Bloc v3.1.0 has it no more), and prints the commits of the clones.
`BLOC_TOPLO` gives another repository of Toplo (`github://pharo-graphics/Toplo:dev/src`, `tonel://<checkout>/src`), `BLOC_TOPLO_GROUPS` other groups than `default`.

`smoke.st` checks the classes, the welcome window pending and the method of `PharoBloc-Web`; draws with Alexandrie an element, a Toplo button, the pane of the welcome window and into a `PharoBlocMorphicForm`; and runs 23 test packages, 2625 tests, with no failure and no error.
They are those that pass both natively and in the node VM without a display, on the image of the zip; the others fail for reasons that are not the image's:

- Alexandrie-Athens-Tests and Alexandrie-Cairo-Tests: 4 and 1 pixel matches made with another cairo.
- Alexandrie-Cairo-Tests, -CairoFreetype-Tests, -Canvas-Tests, -FontManager-Tests, -FreeType-Tests and -NewTools-Tests: a test resource downloads the Noto Color Emoji font from GitHub with Zinc, which the WebAssembly VM cannot.
- Alexandrie-Blur-Tests and Alexandrie-Harfbuzz-Tests: they read fonts and images of the clone of Alexandrie (`pharo-local/iceberg/pharo-graphics/Alexandrie/tests`), which the build has and the zip has not.
- Bloc-Alexandrie-Tests: a pixel match; Bloc-Tests: focus and settling tests that fail now and then without a display.

The job of GitHub Actions, `bloc`, runs it as the batteries job does, with the VM v12.0.6-beta of files.pharo.org, which has the cairo, FreeType, HarfBuzz and SurfacePlugin that the image uses natively; the WebAssembly job waits for it and gives the image to the site as `bloc=PharoBloc-Pharo15.zip`.
The image is 95 MB (22.8 MB gzipped), its `.changes` 18 MB (3.3 MB gzipped); the zip 34 MB.
The load takes about 90 s, the smoke about 2 minutes.

## Tests

- Lane 83 (`tests/wasm/lanes/83-bloc.sh`), when the build has an image of id `bloc`: the registry and the placeholders, and `smoke.st` on the image in the node VM, where the Morphic hosts of the tests draw into `PharoBlocMorphicForm`s, with no FFI callout failed or trapped (it names those the VM adapted, `PHARO_WASM_FFI_TRACE=1`) (2 checks, 120 s).
- `tests/wasm/world.spec.mjs`, with such a site: the world of the image opens the welcome window, which draws a light pane in many colours, and a click on Mini browser opens Toplo's mini browser.

## Limitations

- Fonts that Alexandrie downloads with Zinc (`AeDownloadedFont`: Noto Color Emoji, Cascadia Code) are not there in the WebAssembly VM, whose sockets reach no web server (the Bloc image has no WebFetch, [WebFetch.md](WebFetch.md)); the fonts that Alexandrie embeds (Source Sans Pro, Source Code Pro) are. Inria Serif, which its tests use, is a file of the clone of Alexandrie, which the zip has not.
- A space draws through cairo into the pinned bits of a Form and is copied by BitBlt, in the WebAssembly VM as natively; a larger space costs more of both. Bloc's own animations run in its pulse loop, a process of the image, at the pace that the VM's slices allow.
- `PharoBloc-Web` replaces a method of Bloc's package BlocHost-Morphic: a new version of Bloc loaded over the image brings Bloc's method back, and the Morphic host fails again in the WebAssembly VM, with the primitive failure of `primCreateManualSurfaceWidth:height:rowPitch:depth:isMSB:`.
