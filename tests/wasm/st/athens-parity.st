"athens-parity.st - what Athens and Roassal draw with cairo, through the image's own
bindings (tests/wasm/lanes/65-cairo.sh)

Draws five scenes, each into a Form of depth 32, and prints the SHA256 of its
bits, a line each, '<key>: <value>':

	major           the major version of Pharo, which names the golden file
	                of the lane, tests/wasm/golden/athens-p<major>.txt
	backend         the class of FFIBackend current
	surface         the package of AthensCairoSurface class>>registerSurface:,
	                OSWindow-Web when the image has the extension of
	                packaging/emscripten/st/OSWindow-Web, which copies the
	                pixels of a surface into a Form (asForm) instead of
	                registering it with the SurfacePlugin, which the
	                WebAssembly VMs do not have
	web package     OSWebDriver packageVersion, or none
	library         CairoLibrary uniqueInstance libraryName, the file that
	                the image found (a placeholder, on WebAssembly)
	cairo version   CairoLibrary uniqueInstance versionString
	athens-shapes   a rectangle, a path filled with a linear gradient, a
	                stroked rectangle, on white, 200x100
	athens-text     'Hello Athens on wasm64' in Source Sans Pro 14, which
	                cairo draws with FreeType (cairo_ft), 240x60
	gradient repeat a linear gradient repeated (cairo_pattern_set_extend,
	                which the image declares with a ulong for C's int enum:
	                the width adapter of the FFI calls it), 100x100
	roassal         an RSCanvas of a box, a label and an ellipse, its morph
	                at 300x200, imageForm
	mondrian        RSMondrian of Collection withAllSubclasses in a tree, its
	                morph at 500x300, imageForm
	athens-png      the size of the PNG that cairo writes of a 50x50 surface
	                (cairo_surface_write_to_png, with libpng and zlib)
	asForm reused   whether asForm answers the same Form twice for a surface
	                whose extent did not change

A scene prints '<extent> <SHA256 of the bits of the Form>', the 32-bit words
of the bits in the byte order of the VM's memory (little-endian, on x86-64
as on WebAssembly), in hex; or, when it raised an error, 'error <class of
the error>: <its messageText>'.  Then 'ms <scene>: <ms>' says how long it
took.  Any error is caught and printed as the value of its line, so that
every line is there; the line 'errors: ' lists the scenes that failed, or
says none.

Environment:

	WASM_ATHENS_SCENES  the scenes to draw, separated by commas (default all
	                    of them), e.g. 'gradient repeat' for the negative
	                    control of the lane, with the adapter off
	WASM_ATHENS_PNG     when set, each scene is also written as a PNG next to
	                    the image, <scene>.png (slow on WebAssembly: about 3 s
	                    a scene, the PNG writer of the image)

It changes nothing in the image and does not save it: run it with st --quit,

	pharo --headless Pharo.image --no-default-preferences st --quit athens-parity.st"

| out describe line env wanted png scenes failed hash |
out := WriteStream on: String new.
describe := [ :error |
	('error ' , error class name , ': ' , error messageText asString)
		collect: [ :character | character isSeparator ifTrue: [ $  ] ifFalse: [ character ] ] ].
line := [ :key :block |
	| value |
	value := [ block value ] on: Error do: [ :error | describe value: error ].
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf ].
env := OSEnvironment current.
wanted := (env at: 'WASM_ATHENS_SCENES' ifAbsent: [ nil ]) ifNotNil: [ :names |
	((names substrings: ',') collect: [ :each | each trimBoth ]) asSet ].
png := (env at: 'WASM_ATHENS_PNG' ifAbsent: [ nil ]) notNil.
failed := OrderedCollection new.
hash := [ :form | (form extent x printString , 'x' , form extent y printString) , ' ' , (SHA256 hashMessage: form bits asByteArray) hex ].
line value: 'major' value: [ SystemVersion current major ].
line value: 'backend' value: [ FFIBackend current class name ].
line value: 'surface' value: [ (AthensCairoSurface class >> #registerSurface:) package name ].
line value: 'web package' value: [
	(Smalltalk globals at: #OSWebDriver ifAbsent: [ nil ])
		ifNil: [ 'none' ]
		ifNotNil: [ :driver | (driver class canUnderstand: #packageVersion) ifTrue: [ driver packageVersion ] ifFalse: [ 'none' ] ] ].
line value: 'library' value: [ CairoLibrary uniqueInstance libraryName ].
line value: 'cairo version' value: [ CairoLibrary uniqueInstance versionString ].
scenes := OrderedCollection new.
scenes add: 'athens-shapes' -> [
	| surface |
	surface := AthensCairoSurface extent: 200 @ 100.
	surface drawDuring: [ :canvas |
		canvas setPaint: Color white.
		canvas drawShape: (0 @ 0 extent: 200 @ 100).
		canvas setPaint: Color red.
		canvas drawShape: (0 @ 0 extent: 50 @ 50).
		canvas setPaint: ((LinearGradientPaint from: 0 @ 0 to: 100 @ 100)
				 colorRamp: { 0 -> Color blue. 1 -> Color green };
				 yourself).
		canvas drawShape: (canvas createPath: [ :path |
				 path moveTo: 10 @ 10; lineTo: 90 @ 20; curveVia: 50 @ 80 to: 10 @ 90; close ]).
		canvas setStrokePaint: Color black.
		canvas drawShape: (Rectangle origin: 60 @ 60 corner: 120 @ 90) ].
	surface asForm ].
scenes add: 'athens-text' -> [
	| surface font |
	surface := AthensCairoSurface extent: 240 @ 60.
	font := LogicalFont familyName: 'Source Sans Pro' pointSize: 14.
	surface drawDuring: [ :canvas |
		canvas setPaint: Color white.
		canvas drawShape: (0 @ 0 extent: 240 @ 60).
		canvas setFont: font.
		canvas setPaint: Color black.
		canvas pathTransform translateBy: 10 @ 35.
		canvas drawString: 'Hello Athens on wasm64' ].
	surface asForm ].
scenes add: 'gradient repeat' -> [
	| surface |
	surface := AthensCairoSurface extent: 100 @ 100.
	surface drawDuring: [ :canvas |
		| paint |
		paint := ((LinearGradientPaint from: 0 @ 0 to: 10 @ 10)
			          colorRamp: { 0 -> Color blue. 1 -> Color green };
			          yourself) asAthensPaintOn: canvas.
		paint repeat.
		canvas setPaint: paint.
		canvas drawShape: (0 @ 0 extent: 100 @ 100) ].
	surface asForm ].
scenes add: 'roassal' -> [
	| canvas morph |
	canvas := RSCanvas new.
	canvas add: (RSBox new size: 40; color: Color red; yourself).
	canvas add: (RSLabel text: 'Roassal').
	canvas add: (RSEllipse new size: 30; color: Color blue; yourself).
	RSHorizontalLineLayout on: canvas nodes.
	canvas zoomToFit.
	morph := canvas createMorph.
	morph extent: 300 @ 200.
	morph imageForm ].
scenes add: 'mondrian' -> [
	| mondrian |
	mondrian := RSMondrian new.
	mondrian nodes: Collection withAllSubclasses.
	mondrian layout tree.
	mondrian build.
	(mondrian canvas createMorph extent: 500 @ 300; yourself) imageForm ].
scenes do: [ :scene |
	(wanted isNil or: [ wanted includes: scene key ]) ifTrue: [
		| form ms |
		form := nil.
		ms := [ form := [ scene value value ] on: Error do: [ :error | error ] ] timeToRun asMilliSeconds.
		(form isKindOf: Exception) ifTrue: [
			failed add: scene key.
			form := describe value: form ].
		line value: scene key value: [ form isString ifTrue: [ form ] ifFalse: [ hash value: form ] ].
		line value: 'ms ' , scene key value: [ ms ].
		(png and: [ form isForm ]) ifTrue: [
			[ form writePNGFileNamed: (Smalltalk imageDirectory / ((scene key copyReplaceAll: ' ' with: '-') , '.png')) fullName ]
				on: Error
				do: [ :error | failed add: scene key , '.png' ] ] ] ].
(wanted isNil or: [ wanted includes: 'athens-png' ]) ifTrue: [
	line value: 'athens-png' value: [
		| surface file |
		failed add: 'athens-png'.
		surface := AthensCairoSurface extent: 50 @ 50.
		surface drawDuring: [ :canvas |
			canvas setPaint: Color blue.
			canvas drawShape: (0 @ 0 extent: 25 @ 25) ].
		file := Smalltalk imageDirectory / 'athens.png'.
		file ensureDelete.
		surface writeToPng: file fullName.
		failed remove: 'athens-png'.
		file size ] ].
(wanted isNil or: [ wanted includes: 'asForm reused' ]) ifTrue: [
	line value: 'asForm reused' value: [
		| surface reused |
		failed add: 'asForm reused'.
		surface := AthensCairoSurface extent: 10 @ 10.
		reused := surface asForm == surface asForm.
		failed remove: 'asForm reused'.
		reused ] ].
line value: 'errors' value: [
	failed isEmpty
		ifTrue: [ 'none' ]
		ifFalse: [ (failed asArray collect: [ :each | each asString ]) printString ] ].
Stdio stdout nextPutAll: out contents; flush.
