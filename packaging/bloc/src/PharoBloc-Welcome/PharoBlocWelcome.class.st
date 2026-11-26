"
I am the window that the Bloc image shows the first time that it is opened: what the image adds to the stock Pharo 15 one, drawn by Bloc and Toplo themselves, with buttons that open some of Toplo's examples.

`install` (by `load.st`, when the image is made) asks for the window. I then open once, in the first session with a display (`startUp:`): not in the headless sessions of the build, of the Console or of the Notebook. An image saved after that does not show me again; a fresh copy of the image does.

```smalltalk
PharoBlocWelcome open
```

opens me again, as Help > Bloc and Toplo of the world menu does. My space opens in the preferred host of Bloc: a window of the world in the browser (`BlMorphicWindowHost`), a window of its own on a native VM with SDL2.
"
Class {
	#name : 'PharoBlocWelcome',
	#superclass : 'Object',
	#classInstVars : [
		'pending'
	],
	#category : 'PharoBloc-Welcome',
	#package : 'PharoBloc-Welcome'
}

{ #category : 'accessing' }
PharoBlocWelcome class >> examples [
	"{ label. help. the class of an example. its class-side selector } of my buttons"

	^ {
		  { 'Mini browser'. 'A code browser made of Toplo lists'. #ToExMiniBrowser. #open }.
		  { 'Workspace'. 'An Album editor that evaluates Smalltalk'. #ToExWorkspace. #open }.
		  { 'Login page'. 'Text fields and buttons'. #ToExLoginPage. #open }.
		  { 'Browse the examples'. 'ToSandBox, whose class side has hundreds of examples'. #ToSandBox. #browse } }
]

{ #category : 'installation' }
PharoBlocWelcome class >> install [
	"Show me the first time that the image is opened with a display"

	pending := true.
	SessionManager default registerUserClassNamed: self name
]

{ #category : 'testing' }
PharoBlocWelcome class >> isPending [
	"Whether the window is still to show at the first opening"

	^ pending == true
]

{ #category : 'world menu' }
PharoBlocWelcome class >> menuCommandOn: aBuilder [
	<worldMenu>
	(aBuilder item: #PharoBlocWelcome)
		parent: #Help;
		order: 0;
		label: 'Bloc and Toplo';
		help: 'What this image adds to the stock Pharo 15 image';
		iconName: #help;
		action: [ self open ]
]

{ #category : 'instance creation' }
PharoBlocWelcome class >> open [
	"Open my space, and answer it"

	^ self new open
]

{ #category : 'installation' }
PharoBlocWelcome class >> openPending [
	"The first opening, in the UI process.  An error must not stop the world that starts"

	self isPending ifFalse: [ ^ self ].
	pending := false.
	[ self open ]
		on: Error
		do: [ :error | error traceCr: 'PharoBlocWelcome: ' , error description ]
]

{ #category : 'installation' }
PharoBlocWelcome class >> startUp: isANewSession [
	(isANewSession and: [ self isPending and: [ Smalltalk isInteractive ] ]) ifFalse: [ ^ self ].
	self currentWorld defer: [ self openPending ]
]

{ #category : 'building' }
PharoBlocWelcome >> buttonFor: anExample [
	"A button that opens anExample, { label. help. class name. selector }"

	| button |
	button := ToButton new.
	button labelText: anExample first.
	button tooltipText: anExample second.
	button id: anExample third asSymbol.
	button clickAction: [ :event | self openExample: anExample ].
	^ button
]

{ #category : 'building' }
PharoBlocWelcome >> label: aString size: aNumber [
	"A label of aString, a line, in a font of aNumber points"

	^ ToLabel new
		  text: (aString asRopedText fontSize: aNumber);
		  yourself
]

{ #category : 'building' }
PharoBlocWelcome >> newPane [
	"My content: a title, what the image has, and the buttons of the examples"

	| pane text buttons |
	pane := ToPane vertical.
	pane padding: (BlInsets all: 20).
	pane addChild: (self label: 'Pharo 15 with Bloc and Toplo' size: 22).
	text := ToLabel new
		        text: (self text asRopedText fontSize: 13);
		        beMultiLine;
		        yourself.
	text margin: (BlInsets top: 12 bottom: 12).
	pane addChild: text.
	buttons := ToPane horizontal.
	self class examples do: [ :each |
		| button |
		button := self buttonFor: each.
		button margin: (BlInsets right: 8).
		buttons addChild: button ].
	pane addChild: buttons.
	^ pane
]

{ #category : 'opening' }
PharoBlocWelcome >> open [
	"Open a space with my pane in the preferred host of Bloc, and answer it"

	| space |
	space := BlSpace new.
	space title: 'Welcome to Bloc and Toplo'.
	space extent: 640 @ 330.
	space root addChild: self newPane.
	space show.
	^ space
]

{ #category : 'opening' }
PharoBlocWelcome >> openExample: anExample [
	"Open anExample, { label. help. class name. selector }, or say why it cannot"

	| class |
	class := Smalltalk globals at: anExample third ifAbsent: [
		         ^ self inform: anExample third , ' is not in the image' ].
	class perform: anExample fourth
]

{ #category : 'accessing' }
PharoBlocWelcome >> text [
	"What the image has, and where to start"

	^ 'This is the stock Pharo 15 image with Toplo v0.12.0, its widgets, and what they build on:
Bloc v3.1.0, its elements, spaces, layouts and events, with Alexandrie, its canvas on cairo,
FreeType and HarfBuzz, and Album v0.12.0, its text editor.

In the browser these libraries are compiled into the WebAssembly VM, and a space of Bloc opens
as a window of the world. This window is a space too: space := BlSpace new. space root addChild: ... . space show.
The examples below open in the same way; the class side of ToSandBox has many more.'
]
