"
I am the backend of the Pharo world's window in a web page, where the page draws my canvas. I turn the page's event records into OSWindow events (see OSWebDriver for their layout), and pass title, clipboard, cursor and focus requests to the page through the WebDisplayPlugin.

The canvas has one pixel per CSS pixel, so my screenScaleFactor is 1 and the browser scales the canvas on high-density displays.

As with SDL, keys and text are separate events: the page sends text input only for the keys that type a character, so Escape and the shortcuts reach Morphic as key downs, and text input reaches Morphic without modifiers.

The page shows one window at a time (see OSWebDriver): only that one draws, and when it comes back after another one, it draws itself whole again.
"
Class {
	#name : 'OSWebBackendWindow',
	#superclass : 'OSNullBackendWindow',
	#instVars : [
		'lastPosition',
		'modifierState',
		'buttonState',
		'clipboardText',
		'pastedText',
		'driver'
	],
	#category : 'OSWindow-Web',
	#package : 'OSWindow-Web'
}

{ #category : 'accessing' }
OSWebBackendWindow >> clipboardText [
	"Answer the latest of the text pasted into the page and the text copied in the image. A copy replaces the page's text too (primitiveWebSetClipboardText), so while that text is the one seen after the last copy, nothing was pasted since. The page sends UTF-8, but a paste must not open a debugger: bytes that are not UTF-8, or a sequence that the end cuts, read as question marks"

	| pasted |
	pasted := self primClipboardText ifNil: [ #[  ] ].
	pasted = pastedText ifTrue: [ ^ clipboardText ].
	^ [ pasted utf8Decoded ]
		  on: ZnInvalidUTF8 , ZnIncomplete
		  do: [ :error | error resume ]
]

{ #category : 'accessing' }
OSWebBackendWindow >> clipboardText: aString [
	"The browser may refuse to write the system clipboard, and a copy must still paste in Pharo, as it must natively, where the primitives fail: keep the copy, and the page's text as it is after the copy"

	clipboardText := aString ifNil: [ '' ] ifNotNil: [ aString asString ].
	self primSetClipboardText: clipboardText utf8Encoded.
	pastedText := self primClipboardText ifNil: [ #[  ] ]
]

{ #category : 'event handling' }
OSWebBackendWindow >> deliver: anOSEvent [

	(anOSEvent respondsTo: #modifiers) ifTrue: [
		self fillModifiers: anOSEvent modifiers ].
	^ anOSEvent deliver
]

{ #category : 'initialization' }
OSWebBackendWindow >> destroy [
	"OSWindow>>#destroy: the page shows the window it showed before again"

	| owner |
	owner := driver.
	driver := nil.
	owner ifNotNil: [ owner windowDestroyed: self ]
]

{ #category : 'accessing' }
OSWebBackendWindow >> driver: anOSWebDriver [

	driver := anOSWebDriver
]

{ #category : 'accessing' }
OSWebBackendWindow >> extent [
	"The page reports the size of its canvas. Natively, and before the page reports it, the attributes stand in. A hidden canvas has no pixels, but OSWorldRenderer divides by this extent: answer one pixel at least"

	^ (self primCanvasExtent ifNil: [ attributes extent ]) max: 1 @ 1
]

{ #category : 'event handling' }
OSWebBackendWindow >> fillModifiers: modifiers [

	| shift ctrl alt cmd |
	shift := modifierState anyMask: 1.
	ctrl := modifierState anyMask: 2.
	alt := modifierState anyMask: 4.
	cmd := modifierState anyMask: 8.
	modifiers
		leftShift: shift;
		rightShift: shift;
		leftCtrl: ctrl;
		rightCtrl: ctrl;
		leftAlt: alt;
		rightAlt: alt;
		leftCmd: cmd;
		rightCmd: cmd.
	modifiers buttons
		button1: (buttonState anyMask: 1);
		button2: (buttonState anyMask: 2);
		button3: (buttonState anyMask: 4)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleClose: anEvent [

	self deliver: (OSWindowCloseEvent for: osWindow)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleFocus: anEvent [

	self deliver: ((anEvent at: 5) = 0
			 ifTrue: [ OSWindowFocusOutEvent for: osWindow ]
			 ifFalse: [ OSWindowFocusInEvent for: osWindow ])
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleKeyDown: anEvent [

	self deliver: ((OSKeyDownEvent for: osWindow)
			 symbol: (anEvent at: 5);
			 scanCode: (anEvent at: 7);
			 repeat: (anEvent at: 8);
			 position: lastPosition;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleKeyUp: anEvent [

	self deliver: ((OSKeyUpEvent for: osWindow)
			 symbol: (anEvent at: 5);
			 scanCode: (anEvent at: 7);
			 repeat: (anEvent at: 8);
			 position: lastPosition;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleMouseButtonPress: anEvent [

	self updatePosition: anEvent.
	self deliver: ((OSMouseButtonPressEvent for: osWindow)
			 button: (anEvent at: 5);
			 position: lastPosition;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleMouseButtonRelease: anEvent [

	self updatePosition: anEvent.
	self deliver: ((OSMouseButtonReleaseEvent for: osWindow)
			 button: (anEvent at: 5);
			 position: lastPosition;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleMouseMove: anEvent [

	| previous |
	previous := lastPosition.
	self updatePosition: anEvent.
	self deliver: ((OSMouseMoveEvent for: osWindow)
			 position: lastPosition;
			 delta: lastPosition - previous;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleMouseWheel: anEvent [
	"The page sends whole notches, positive away from the user as in SDL"

	self updatePosition: anEvent.
	self deliver: ((OSMouseWheelEvent for: osWindow)
			 position: lastPosition;
			 scrollHorizontal: (anEvent at: 7);
			 scrollVertical: (anEvent at: 8);
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleResize: anEvent [
	"The page may hide or collapse the canvas. Keep one pixel at least, as #extent does"

	| extent |
	extent := ((anEvent at: 5) max: 1) @ ((anEvent at: 7) max: 1).
	attributes extent: extent.
	self deliver: ((OSWindowResizeEvent for: osWindow)
			 width: extent x;
			 height: extent y;
			 yourself)
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleTextInput: anEvent [
	"Text input carries no modifiers, as with OSSDL2BackendWindow: Morphic drops the text typed with Alt, Ctrl or Cmd held, and Option or AltGr type characters such as @ and [ on many layouts"

	^ ((OSTextInputEvent for: osWindow)
		   text: (String with: (Character value: (anEvent at: 7)));
		   position: lastPosition;
		   yourself) deliver
]

{ #category : 'event handling' }
OSWebBackendWindow >> handleWebEvent: anEvent [
	"Deliver anEvent, a record of 8 integers from the page (see OSWebDriver)"

	| handler |
	handler := #( #handleMouseMove: #handleMouseButtonPress:
	              #handleMouseButtonRelease: #handleMouseWheel:
	              #handleKeyDown: #handleKeyUp: #handleTextInput:
	              #handleResize: #handleFocus: #handleClose: )
		           at: (anEvent at: 1)
		           ifAbsent: [ ^ self ].
	modifierState := (anEvent at: 6) bitAnd: 16rFF.
	buttonState := (anEvent at: 6) bitShift: -8.
	self perform: handler with: anEvent
]

{ #category : 'testing' }
OSWebBackendWindow >> handlesEvents [
	"Whether my OSWindow handles events, as the world and the Emergency Debugger do"

	^ osWindow notNil and: [ osWindow eventHandler notNil ]
]

{ #category : 'initialization' }
OSWebBackendWindow >> initialize [

	super initialize.
	lastPosition := 0 @ 0.
	modifierState := 0.
	buttonState := 0.
	clipboardText := ''.
	pastedText := #[  ]
]

{ #category : 'testing' }
OSWebBackendWindow >> isShown [
	"Whether the page shows me (see OSWebDriver)"

	^ driver notNil and: [ driver window == self ]
]

{ #category : 'factory' }
OSWebBackendWindow >> newFormRenderer: aForm [

	^ renderer := OSWebFormRenderer new
		              form: aForm;
		              backendWindow: self;
		              yourself
]

{ #category : 'initialization' }
OSWebBackendWindow >> open [
	"Ask the page to show its canvas, at the size the page reported, if it did (see OSWebDriver>>#afterMainPharoWindowCreated:), with my title. When I come back after another window, draw me whole again"

	attributes extent: self extent.
	self
		primOpenCanvas: attributes extent
		title: (attributes title ifNil: [ '' ]) asString utf8Encoded.
	renderer ifNotNil: [ renderer updateAll ]
]

{ #category : 'primitives' }
OSWebBackendWindow >> primCanvasExtent [
	"Answer the extent of the page's canvas, or nil before the page reports one"

	<primitive: #primitiveWebCanvasExtent module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primClipboardText [
	"Answer the UTF-8 bytes last pasted into the page"

	<primitive: #primitiveWebClipboardText module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primFocus [

	<primitive: #primitiveWebFocus module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primOpenCanvas: anExtent title: titleBytes [

	<primitive: #primitiveWebOpenCanvas module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primSetClipboardText: bytes [

	<primitive: #primitiveWebSetClipboardText module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primSetCursor: bits mask: maskBits extent: anExtent offset: anOffset [
	"bits and maskBits are the words of 1-bit Forms, and anOffset is the Form offset, the negated hot spot"

	<primitive: #primitiveWebSetCursor module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebBackendWindow >> primSetTitle: bytes [

	<primitive: #primitiveWebSetTitle module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'accessing' }
OSWebBackendWindow >> raise [
	"OSWorldRenderer>>#doActivate focuses the window, which raises it: move the keyboard focus to the canvas"

	self isShown ifTrue: [ self primFocus ]
]

{ #category : 'accessing' }
OSWebBackendWindow >> screenScaleFactor [

	^ 1
]

{ #category : 'cursor' }
OSWebBackendWindow >> setMouseCursor: cursor mask: mask andScale: scale [
	"The page turns the 1-bit Forms into a CSS cursor. The canvas is not scaled, so neither is the cursor"

	| maskForm |
	maskForm := mask ifNil: [ cursor ].
	(cursor depth = 1 and: [
		 maskForm depth = 1 and: [ maskForm extent = cursor extent ] ])
		ifFalse: [ ^ self ].
	cursor unhibernate.
	maskForm unhibernate.
	self
		primSetCursor: cursor bits
		mask: maskForm bits
		extent: cursor extent
		offset: cursor offset
]

{ #category : 'accessing' }
OSWebBackendWindow >> title: aString [
	"The page shows the title when it shows me, see #open"

	attributes title: aString.
	self isShown ifTrue: [ self primSetTitle: aString asString utf8Encoded ]
]

{ #category : 'event handling' }
OSWebBackendWindow >> updatePosition: anEvent [

	lastPosition := (anEvent at: 3) @ (anEvent at: 4)
]
