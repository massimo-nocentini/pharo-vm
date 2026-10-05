"What the Pharo world shows, for the tests of the world page: tests/wasm/world-harness.mjs
 and tests/wasm/world.spec.mjs boot the world image with it,

	--headless /pharo/Pharo.image --no-default-preferences --interactive st /pharo/st/world-probe.st

 and read the JSON file it writes every 100 ms, probe.json next to the image:

	seq            how many times it was written
	renderer, driver   the classes of the world renderer and of its driver
	world          the extent of the World, [width, height]
	display        the extent of the renderer's Form, or null
	hand           the position of the hand (the mouse)
	menubar        the items of the menubar, [{label, bounds}], bounds [left, top, right, bottom],
	               in the coordinates of the World, as all bounds
	menus          the open menus, [{class, items: [{label, bounds}]}]
	windows        the system windows, [{label, bounds, model}]
	playground     the text of the Playground window and the bounds of its text, or null
	               (in the World's coordinates too, unlike the bounds of the text morph,
	               which are its owner's: in the smaller Playground of Pharo 15, the centre
	               of those falls on the popover of print it)
	printed        the texts of the popovers of print it (Ctrl+P)
	debugger       whether a debugger window is open
	fonts          the fonts that draw: the class of the font that draws (its realFont)
	               for the standard fonts default, code, menu and windowTitle, and for
	               the first item of the menubar (menubar, or null), and the family of
	               the default font (family)
	glyphs         whether the font that draws the default font has glyphs for a lambda
	               and a right arrow (U+03BB and U+2192), which the embedded TrueType
	               fonts of the image have and its bitmap fonts do not: FreeType answers
	               its glyph indices (FT_Get_Char_Index), a StrikeFont its table
	error          what went wrong, when something did

 It runs at userBackgroundPriority, below the UI process: a user interrupt stops the
 busy UI process, never this one, which a busy UI starves.  Loaded again, as after a
 snapshot of an image it ran in, it stops the probe of the earlier load."

| file text |
file := FileLocator imageDirectory asFileReference / 'probe.json'.
text := [ :morph | morph contents asString ].
(Smalltalk globals at: #WorldProbe ifAbsent: [ nil ]) ifNotNil: [ :old | old terminate ].
Smalltalk globals
	at: #WorldProbe
	put: ([
		| seq rect point labelled drawing glyphs |
		seq := 0.
		rect := [ :r | { r left. r top. r right. r bottom } collect: [ :n | n rounded ] ].
		point := [ :p | { p x rounded. p y rounded } ].
		labelled := [ :items |
			(items collect: [ :item |
				Dictionary new
					at: 'label' put: (text value: item);
					at: 'bounds' put: (rect value: item boundsInWorld);
					yourself ]) asArray ].
		drawing := [ :font |
			font ifNotNil: [
				((font respondsTo: #realFont)
					 ifTrue: [ font realFont ]
					 ifFalse: [ font ]) ] ].
		glyphs := String with: (Character value: 16r3BB) with: (Character value: 16r2192).
		[
		| state |
		seq := seq + 1.
		state := Dictionary new.
		state at: 'seq' put: seq.
		[
		| renderer menubar playground area |
		renderer := World worldState worldRenderer.
		state
			at: 'renderer' put: renderer class name;
			at: 'driver' put: ((renderer respondsTo: #driver)
					 ifTrue: [ renderer driver class name ]
					 ifFalse: [ nil ]);
			at: 'world' put: (point value: World extent);
			at: 'display' put: (((renderer respondsTo: #display) and: [ renderer display notNil ])
					 ifTrue: [ point value: renderer display extent ]
					 ifFalse: [ nil ]);
			at: 'hand' put: (point value: World activeHand position).
		menubar := World submorphs
			           detect: [ :morph | morph isKindOf: MenubarMorph ]
			           ifNone: [ nil ].
		state at: 'menubar' put: (menubar
				 ifNil: [ #(  ) ]
				 ifNotNil: [ labelled value: (menubar allMorphs select: [ :morph | morph isKindOf: MenuItemMorph ]) ]).
		state at: 'menus' put: ((World submorphs select: [ :morph | morph isKindOf: MenuMorph ]) asArray collect: [ :menu |
				 Dictionary new
					 at: 'class' put: menu class name;
					 at: 'items' put: (labelled value: (menu allMorphs select: [ :morph | morph isKindOf: MenuItemMorph ]));
					 yourself ]).
		state at: 'windows' put: (World systemWindows asArray collect: [ :window |
				 Dictionary new
					 at: 'label' put: window label asString;
					 at: 'bounds' put: (rect value: window boundsInWorld);
					 at: 'model' put: window model class name;
					 yourself ]).
		playground := World systemWindows
			              detect: [ :window | window label asString = 'Playground' ]
			              ifNone: [ nil ].
		area := playground ifNotNil: [
			        playground allMorphs
				        detect: [ :morph | morph isKindOf: RubScrolledTextMorph ]
				        ifNone: [ nil ] ].
		state at: 'playground' put: (area ifNotNil: [
				 Dictionary new
					 at: 'text' put: area text asString;
					 at: 'bounds' put: (rect value: area boundsInWorld);
					 yourself ]).
		state at: 'printed' put: ((World allMorphs select: [ :morph |
				  (morph isKindOf: RubEditingArea) and: [
					  morph allOwners anySatisfy: [ :owner | owner isKindOf: SpPopoverMorph ] ] ]) asArray
				 collect: [ :morph | morph text asString ]).
		state at: 'debugger' put: (World systemWindows anySatisfy: [ :window |
				 (window model respondsTo: #presenter) and: [
					 window model presenter isKindOf: StDebugger ] ]).
		state at: 'fonts' put: (Dictionary new
				 at: 'default' put: (drawing value: StandardFonts defaultFont) class name;
				 at: 'code' put: (drawing value: StandardFonts codeFont) class name;
				 at: 'menu' put: (drawing value: StandardFonts menuFont) class name;
				 at: 'windowTitle' put: (drawing value: StandardFonts windowTitleFont) class name;
				 at: 'menubar' put: (menubar ifNotNil: [
						  (menubar allMorphs
							   detect: [ :morph | morph isKindOf: MenuItemMorph ]
							   ifNone: [ nil ]) ifNotNil: [ :item | (drawing value: item font) class name ] ]);
				 at: 'family' put: StandardFonts defaultFont familyName;
				 yourself).
		state at: 'glyphs' put: ((drawing value: StandardFonts defaultFont) in: [ :font |
				 (font isKindOf: FreeTypeFont)
					 ifTrue: [
						 font face isValid and: [
							 glyphs allSatisfy: [ :character | (font face ffiGetCharIndex: character asInteger) ~= 0 ] ] ]
					 ifFalse: [
						 (font respondsTo: #hasGlyphOf:) and: [
							 glyphs allSatisfy: [ :character | font hasGlyphOf: character ] ] ] ]) ]
			on: Error
			do: [ :error | state at: 'error' put: error description ].
		[
		file ensureDelete.
		file writeStreamDo: [ :stream | stream nextPutAll: (STONJSON toString: state) ] ]
			on: Error
			do: [ :error | "the next one, then" ].
		(Delay forMilliseconds: 100) wait ] repeat ]
			 forkAt: Processor userBackgroundPriority
			 named: 'World probe')
