"A session of the world of a stock image under OSSDL2Driver, for lane 67
 (tests/wasm/lanes/67-sdl2.sh): the node VM of a build with SDL2 (WASM_SDL2=ON)
 runs it on the stock image, as

	SDL_VIDEODRIVER=dummy pharo --headless Pharo.image --no-default-preferences --interactive st sdl-session.st

 With --interactive and no display of the page, the image opens its world
 through its own OSWindow-SDL2 bindings, on SDL2 of the registry of the FFI
 (the library SDL2, found as the placeholder libSDL2-2.0.so.0), which draw it
 with SDL's dummy video driver.  The session goes through what a user of the
 world does: the tools, the clipboard, the cursors, the title, a resize,
 fullscreen, text input; and it gives the world the events of a user, which
 it queues with SDL_PushEvent (SDL2 of the registry has it for that, the
 image binds it not), as SDL's Emscripten video driver queues those of the
 page sdl.html: a right click on the desktop, which opens the world menu, a
 click into a Playground, '3 + 4' typed into it, Ctrl+A and Ctrl+P, which
 prints 7.  It prints on stdout, one line each:

	major: <n>                the major version of Pharo
	driver: <class>           the driver of the world (OSSDL2Driver)
	sdl: <x.y.z>              the version of SDL
	ok <step>                 a step that worked, or
	error <step>: <message>   one that did not
	<key>: <value>            what a step found (clipboard, extent, scale,
	                          colours, menu, typed, printed)
	session: done             the end, after which the image quits (status 0)

 A step that waits for the world to show what it did waits up to 10 s for it.
 When the world has another driver, the session says 'session: not
 OSSDL2Driver' after it and quits with status 1.  After 100 s, it says
 'session: timed out' and quits with status 3.  (Without SDL2, as with
 PHARO_WASM_FFI_HIDE=SDL2, a stock image does not start its world at all:
 OSSDL2Driver, which it picks since the file libSDL2-2.0.so.0 is there,
 fails to initialize SDL2, before the session runs.)"

| say |
say := [ :line | Stdio stdout nextPutAll: line asString; lf; flush ].
[ (Delay forSeconds: 100) wait. say value: 'session: timed out'. Smalltalk exit: 3 ] forkAt: Processor timingPriority - 1.
[ | step until w r osWindow windowId event push key click text area playground popovers desktop shown |
  "step NAME BLOCK: says 'ok NAME', or 'error NAME: ...' when BLOCK raised or answered false"
  step := [ :name :block |
	[ (block value) == false
		ifTrue: [ say value: 'error ' , name , ': not shown within 10 s' ]
		ifFalse: [ say value: 'ok ' , name ] ]
		on: Error
		do: [ :e | say value: 'error ' , name , ': ' , e class name , ' ' , e messageText asString ] ].
  "until BLOCK: waits for BLOCK to answer true, at most 10 s; answers what it last answered"
  until := [ :block | | t0 |
	t0 := Time millisecondClockValue.
	[ block value or: [ Time millisecondClockValue - t0 > 10000 ] ] whileFalse: [ (Delay forMilliseconds: 50) wait ].
	block value ].
  say value: 'major: ' , SystemVersion current major printString.
  until value: [ (w := self currentWorld) notNil and: [ w worldState worldRenderer notNil ] ].
  r := w worldState worldRenderer.
  say value: 'driver: ' , ([ r driver class name ] on: Error do: [ :e | 'none (' , e messageText asString , ')' ]).
  ([ r driver class name = #OSSDL2Driver ] on: Error do: [ :e | false ]) ifFalse: [
	say value: 'session: not OSSDL2Driver'.
	Smalltalk exit: 1 ].
  osWindow := r osWindow.
  say value: 'sdl: ' , ([ | v | v := SDL2 version. v major printString , '.' , v minor printString , '.' , v patch printString ]
	on: Error do: [ :e | 'error ' , e messageText asString ]).

  "The tools, and what the world asks of its window"
  step value: 'playground' value: [
	playground := (Smalltalk tools workspace openContents: '') window.
	until value: [ w systemWindows includes: playground ] ].
  step value: 'browser' value: [ | n |
	n := w systemWindows size.
	Smalltalk tools browser open.
	until value: [ w systemWindows size > n ] ].
  step value: 'clipboard' value: [
	Clipboard clipboardText: 'hello from pharo'.
	say value: 'clipboard: ' , Clipboard clipboardText asString.
	Clipboard clipboardText asString = 'hello from pharo' ].
  step value: 'cursor' value: [
	w currentHand showTemporaryCursor: Cursor wait.
	(Delay forMilliseconds: 200) wait.
	w currentHand showTemporaryCursor: Cursor crossHair.
	(Delay forMilliseconds: 200) wait.
	w currentHand showTemporaryCursor: nil ].
  step value: 'title' value: [ osWindow title: 'sdl-session' ].
  step value: 'resize' value: [
	osWindow extent: 1000 @ 700.
	shown := until value: [ w extent = (1000 @ 700) and: [ r display extent = (1000 @ 700) ] ].
	say value: 'extent: ' , w extent printString , ' display ' , r display extent printString.
	shown ].
  step value: 'fullscreen' value: [
	osWindow fullscreen: true.
	(Delay forMilliseconds: 300) wait.
	osWindow fullscreen: false ].
  step value: 'textinput' value: [
	osWindow startTextInputAtRectangle: (10 @ 10 extent: 10 @ 10).
	osWindow stopTextInput ].
  step value: 'dpi' value: [ say value: 'scale: ' , osWindow screenScaleFactor printString ].
  "the pixels of the world: the colours of a grid of 40 by 40 of its Form"
  step value: 'drawn' value: [ | form colours |
	(Delay forMilliseconds: 500) wait.
	form := r display.
	colours := Set new.
	0 to: 39 do: [ :i |
		0 to: 39 do: [ :j |
			colours add: (form pixelValueAt: (form width * i // 40) @ (form height * j // 40)) ] ].
	say value: 'colours: ' , colours size printString.
	colours size >= 8 ].

  "The events of a user, queued as SDL's Emscripten driver queues those of the page"
  SDL2 class compile: 'pushEventForSdlSession: event
	^ self ffiCall: #( int SDL_PushEvent ( SDL_Event event ) )' classified: 'zz-sdl-session'.
  windowId := osWindow backendWindow windowId.
  "push TYPE BLOCK: an SDL_Event of TYPE at windowID, which BLOCK fills in (offsets from 1)"
  push := [ :type :block |
	event := SDL_Event new.
	event getHandle unsignedLongAt: 1 put: type.
	event getHandle unsignedLongAt: 5 put: SDL2 ticks.
	event getHandle unsignedLongAt: 9 put: windowId.
	block value: event getHandle.
	(SDL2 pushEventForSdlSession: event) = 1 ifFalse: [ self error: 'SDL_PushEvent failed: ' , SDL2 getErrorMessage asString ] ].
  "click BUTTON POINT: SDL_MOUSEMOTION, then SDL_MOUSEBUTTONDOWN and UP (SDL_MouseButtonEvent)"
  click := [ :button :point |
	push value: 16r400 value: [ :h | h signedLongAt: 21 put: point x. h signedLongAt: 25 put: point y ].
	#( 16r401 16r402 ) with: #( 1 0 ) do: [ :type :pressed |
		push value: type value: [ :h |
			h unsignedByteAt: 17 put: button.
			h unsignedByteAt: 18 put: pressed.
			h unsignedByteAt: 19 put: 1.
			h signedLongAt: 21 put: point x.
			h signedLongAt: 25 put: point y ].
		(Delay forMilliseconds: 50) wait ] ].
  "key SYM SCANCODE MOD: SDL_KEYDOWN, then SDL_KEYUP (SDL_KeyboardEvent)"
  key := [ :sym :scancode :mod |
	#( 16r300 16r301 ) with: #( 1 0 ) do: [ :type :pressed |
		push value: type value: [ :h |
			h unsignedByteAt: 13 put: pressed.
			h signedLongAt: 17 put: scancode.
			h signedLongAt: 21 put: sym.
			h unsignedShortAt: 25 put: mod ].
		(Delay forMilliseconds: 50) wait ] ].
  "text STRING: an SDL_TEXTINPUT for each character, as a browser sends them (SDL_TextInputEvent)"
  text := [ :string |
	string do: [ :character |
		push value: 16r303 value: [ :h | h unsignedByteAt: 13 put: character asInteger. h unsignedByteAt: 14 put: 0 ].
		(Delay forMilliseconds: 30) wait ] ].
  area := [ playground allMorphs detect: [ :m | m isKindOf: RubScrolledTextMorph ] ifNone: [ nil ] ].
  popovers := [ (w allMorphs select: [ :m |
	(m isKindOf: RubEditingArea) and: [ m allOwners anySatisfy: [ :o | o isKindOf: SpPopoverMorph ] ] ])
		collect: [ :m | m text asString ] ].
  "a point of the desktop, where no morph is, from the right of the world"
  desktop := [ | candidates |
	candidates := OrderedCollection new.
	(w width - 8) to: 8 by: -16 do: [ :x |
		(w height // 2) to: w height - 40 by: 16 do: [ :y | candidates add: x @ y ] ].
	candidates detect: [ :p | w submorphs noneSatisfy: [ :m | m visible and: [ m fullBounds containsPoint: p ] ] ] ].
  step value: 'menu' value: [ | menus |
	click value: 3 value: desktop value.
	shown := until value: [ (w submorphs select: [ :m | m isKindOf: MenuMorph ]) notEmpty ].
	menus := w submorphs select: [ :m | m isKindOf: MenuMorph ].
	say value: 'menu: ' , (menus inject: 0 into: [ :n :m | n + (m allMorphs count: [ :i | i isKindOf: MenuItemMorph ]) ]) printString , ' items'.
	menus do: [ :m | w defer: [ m delete ] ].
	until value: [ (w submorphs select: [ :m | m isKindOf: MenuMorph ]) isEmpty ].
	shown ].
  step value: 'type' value: [
	playground comeToFront.
	(Delay forMilliseconds: 300) wait.
	click value: 1 value: area value boundsInWorld center.
	(Delay forMilliseconds: 300) wait.
	text value: '3 + 4'.
	shown := until value: [ area value text asString = '3 + 4' ].
	say value: 'typed: ' , area value text asString.
	shown ].
  step value: 'print' value: [
	key value: $a asInteger value: 4 value: 16r40.		"Ctrl+A: SDLK_a, SDL_SCANCODE_A, KMOD_LCTRL"
	key value: $p asInteger value: 19 value: 16r40.		"Ctrl+P"
	shown := until value: [ popovers value includes: '7' ].
	say value: 'printed: ' , (popovers value asArray printString).
	shown ].
  say value: 'session: done'.
  Smalltalk snapshot: false andQuit: true ] forkAt: Processor userSchedulingPriority.
