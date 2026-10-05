"ft-parity.st - the text that FreeType draws, through the image's own bindings
(tests/wasm/lanes/64-freetype.sh, and tests/wasm/lanes/59-ffi-clean.sh)

Draws 'Hello Pharo AV Wa' in black, with the default font of the image
(StandardFonts defaultFont), at 2@2 of a white Form of 300x40 and depth 32,
as a Morphic canvas draws text, then prints what it found, a line each,
'<key>: <value>':

	major           the major version of Pharo, which names the golden file
	                of the lane, tests/wasm/golden/ft-p<major>.txt
	backend         the class of FFIBackend current
	loadFt2Library  FreeTypeSystemSettings loadFt2Library
	realFont        the print string of the font that drew, the realFont
	                of the default font
	realFont class  its class: FreeTypeFont when FreeType drew
	version         FT2Library current libraryVersion, or the description
	                of what the lookup raised
	glyphs          whether the font that drew has glyphs for a lambda and
	                a right arrow (U+03BB and U+2192): FreeType answers its
	                glyph indices (FT_Get_Char_Index), a StrikeFont its
	                table; Pharo 15 has no hasGlyphsForAll:
	nonwhite        how many pixels are not white
	bits            the SHA256 of the bits of the Form, its 32-bit words in
	                the byte order of the VM's memory (little-endian, on
	                x86-64 as on WebAssembly), in hex
	draw ms         the time of the first drawing, which loads the face of
	                the font and renders its glyphs; then warm draw ms, the
	                time of a second one
	errors          the errors the drawing raised, or none

The drawing is written to ft.png next to the image.  Any error is caught and
printed as the value of its line, so that every line is there.  It changes
nothing in the image and does not save it: run it with st --quit,

	pharo --headless Pharo.image --no-default-preferences st --quit ft-parity.st"

| out line font form nonwhite bytes errors draw ms real glyphs |
out := WriteStream on: String new.
line := [ :key :block |
	| value |
	value := [ block value ] on: Error do: [ :error | 'error: ' , error description ].
	out nextPutAll: key; nextPutAll: ': '; nextPutAll: (value isString ifTrue: [ value ] ifFalse: [ value printString ]); lf ].
errors := OrderedCollection new.
line value: 'major' value: [ SystemVersion current major ].
line value: 'backend' value: [ FFIBackend current class name ].
line value: 'loadFt2Library' value: [ FreeTypeSystemSettings loadFt2Library ].
font := StandardFonts defaultFont.
form := Form extent: 300 @ 40 depth: 32.
draw := [
	form fillWhite.
	[ form getCanvas drawString: 'Hello Pharo AV Wa' at: 2 @ 2 font: font color: Color black ]
		on: Error
		do: [ :error | errors add: error description ] ].
ms := draw timeToRun asMilliSeconds.
real := [ font realFont ] on: Error do: [ :error | nil ].
line value: 'realFont' value: [ real printString ].
line value: 'realFont class' value: [ real class name ].
line value: 'version' value: [ FT2Library current libraryVersion ].
glyphs := String with: (Character value: 16r3BB) with: (Character value: 16r2192).
line value: 'glyphs' value: [
	(real isKindOf: FreeTypeFont)
		ifTrue: [ real face isValid and: [ glyphs allSatisfy: [ :character | (real face ffiGetCharIndex: character asInteger) ~= 0 ] ] ]
		ifFalse: [ (real respondsTo: #hasGlyphOf:) and: [ glyphs allSatisfy: [ :character | real hasGlyphOf: character ] ] ] ].
nonwhite := 0.
0 to: form width - 1 do: [ :x |
	0 to: form height - 1 do: [ :y |
		(form colorAt: x @ y) = Color white ifFalse: [ nonwhite := nonwhite + 1 ] ] ].
line value: 'nonwhite' value: [ nonwhite ].
line value: 'bits' value: [
	bytes := form bits asByteArray.
	(SHA256 hashMessage: bytes) hex ].
line value: 'draw ms' value: [ ms ].
[ form writePNGFileNamed: (Smalltalk imageDirectory / 'ft.png') fullName ]
	on: Error
	do: [ :error | errors add: 'ft.png: ' , error description ].
line value: 'warm draw ms' value: [ draw timeToRun asMilliSeconds ].
line value: 'errors' value: [
	errors isEmpty
		ifTrue: [ 'none' ]
		ifFalse: [ (errors asArray collect: [ :each | each asString ]) printString ] ].
Stdio stdout nextPutAll: out contents; flush.
