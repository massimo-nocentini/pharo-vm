"web-surface.st - FALLBACK ONLY: give a stock Pharo image the AthensCairoSurface methods of
OSWindow-Web, so that Athens draws on the WebAssembly VMs, which have no SurfacePlugin.

The images that users run on the web are built with the package OSWindow-Web
(packaging/emscripten/st/OSWindow-Web, the image/web of a build): use them.  This script is for
the images that do not have it, the stock images of the tests (Pharo 12, or a build without
image/web): the CairoPlugin's lane (tests/wasm/lanes/74-cairo.sh) and benchmark
(plugins/CairoPlugin/bench/run-bench.sh --wasm) run it before loading the Cairo packages.

	pharo --headless X.image --no-default-preferences st --save --quit web-surface.st

It compiles the three methods of packaging/emscripten/st/OSWindow-Web/AthensCairoSurface.extension.st,
with the same sources (the lane checks they are equal), in the protocol *OSWindow-Web:
AthensCairoSurface class>>registerSurface: registers nothing and answers nil (the stock method
registers the surface with the SurfacePlugin, and fails without it), unregisterSurfaceWithId: does
nothing, and asForm answers a Form holding a copy of the surface's pixels.  When the image already
has them (image/web), it changes nothing.  It prints 'web-surface: compiled' or
'web-surface: already there'.  It does not save the image: run it with --save."

| out sources |
out := Stdio stdout.
sources := {
	AthensCairoSurface class -> 'registerSurface: anAthensCairoSurface
	"The web VMs have no SurfacePlugin, through which the stock method shares the pixels of a surface with BitBlt, and fails without: register nothing, and answer nil for the id slot of the surface, where #asForm keeps a copy of the pixels instead. Natively the prepared image takes this path too"

	self checkSession.
	^ nil'.
	AthensCairoSurface class -> 'unregisterSurfaceWithId: anAthensCairoSurfaceId
	"Nothing was registered (see #registerSurface:)"'.
	AthensCairoSurface -> 'asForm
	"A Form of my extent holding a copy of my pixels as they are now, the premultiplied ARGB32 of cairo, as the Form of the stock method holds them through the SurfacePlugin (see AthensCairoSurface class>>#registerSurface:): drawing it gives the same pixels. The Form is kept in my id slot, which the SurfacePlugin left free, and reused while my extent is the same, so a Form answered earlier takes my newer pixels at each call, where the stock one always shows them. My resource data, taken while the id slot was still nil, does not hold the Form: finalizing me keeps no copy of my pixels"

	| form |
	self checkSession.
	self flush.
	form := id.
	(form isForm and: [ form extent = self extent ]) ifFalse: [
		form := Form extent: self extent depth: 32.
		id := form ].
	"(the rows of a 32-bit format have 4 bytes a pixel and no padding, as the Form''s have: the minimum only keeps a surface of another format from writing past the Form)"
	LibC
		memCopy: self getDataPtr
		to: form bits
		size: (self stride * self height min: form bits byteSize).
	^ form' }.
((AthensCairoSurface class >> #registerSurface:) protocolName = '*OSWindow-Web')
	ifTrue: [ out nextPutAll: 'web-surface: already there'; lf; flush ]
	ifFalse: [
		sources do: [ :each | each key compile: each value classified: '*OSWindow-Web' ].
		out nextPutAll: 'web-surface: compiled'; lf; flush ].
!
