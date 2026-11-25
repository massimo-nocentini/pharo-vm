"
I am the page that the batteries image shows the first time that it is opened: what the image and its VM add to those of pharo-project, a Markdown text (`packaging/batteries/welcome/welcome.md` of the VM) that cmark-gfm parses and `CMarkRoassalRenderer` draws in an `SpRoassalPresenter`.

`install:` (by `load.st`, when the image is made) keeps the text and asks for the page. I then open once, in the first session with a display (`startUp:`): not in the headless sessions of the build, of the Console or of the Notebook. An image saved after that does not show me again; a fresh copy of the image does.

```smalltalk
PharoBatteriesWelcome open
```

opens me again, as Help > Batteries of the world menu does.

On a VM without the CMarkPlugin, which is needed to parse Markdown, I show the source of the text. I follow the dark or light UI theme when I open.
"
Class {
	#name : 'PharoBatteriesWelcome',
	#superclass : 'SpPresenter',
	#instVars : [
		'roassal'
	],
	#classInstVars : [
		'contents',
		'pending'
	],
	#category : 'PharoBatteries-Welcome',
	#package : 'PharoBatteries-Welcome'
}

{ #category : 'accessing' }
PharoBatteriesWelcome class >> contents [
	"The Markdown text"

	^ contents ifNil: [ '' ]
]

{ #category : 'accessing' }
PharoBatteriesWelcome class >> contents: aString [
	contents := aString
]

{ #category : 'installation' }
PharoBatteriesWelcome class >> install: aString [
	"Show aString, Markdown, the first time that the image is opened with a display"

	self contents: aString.
	pending := true.
	SessionManager default registerUserClassNamed: self name
]

{ #category : 'testing' }
PharoBatteriesWelcome class >> isPending [
	"Whether the page is still to show at the first opening"

	^ pending == true
]

{ #category : 'world menu' }
PharoBatteriesWelcome class >> menuCommandOn: aBuilder [
	<worldMenu>
	(aBuilder item: #PharoBatteriesWelcome)
		parent: #Help;
		order: 0;
		label: 'Batteries';
		help: 'What this image and its VM add to those of pharo-project';
		iconName: #help;
		action: [ self open ]
]

{ #category : 'instance creation' }
PharoBatteriesWelcome class >> open [
	| window |
	window := self new open.
	window centered.
	^ window
]

{ #category : 'installation' }
PharoBatteriesWelcome class >> openPending [
	"The first opening, in the UI process.  An error must not stop the world that starts"

	self isPending ifFalse: [ ^ self ].
	pending := false.
	[ self open ]
		on: Error
		do: [ :error | error traceCr: 'PharoBatteriesWelcome: ' , error description ]
]

{ #category : 'installation' }
PharoBatteriesWelcome class >> startUp: isANewSession [
	(isANewSession and: [ self isPending and: [ Smalltalk isInteractive ] ]) ifFalse: [ ^ self ].
	self currentWorld defer: [ self openPending ]
]

{ #category : 'layout' }
PharoBatteriesWelcome >> defaultLayout [
	^ SpBoxLayout newTopToBottom
		  add: roassal;
		  yourself
]

{ #category : 'rendering' }
PharoBatteriesWelcome >> document [
	"The text parsed, or its source when it cannot be"

	| contents |
	contents := self class contents.
	CMark isAvailable ifFalse: [ ^ self sourceDocument: contents ].
	^ [ CMark parse: contents ]
		  on: CMarkError
		  do: [ :error | self sourceDocument: contents ]
]

{ #category : 'rendering' }
PharoBatteriesWelcome >> drawOn: aCanvas [
	"As CMarkRoassalRenderer>>show:on:, but the wheel scrolls the page instead of zooming"

	| renderer shape controller |
	renderer := CMarkRoassalRenderer theme: self theme.
	shape := renderer render: self document.
	renderer theme background ifNotNil: [ :color | aCanvas color: color ].
	aCanvas add: shape.
	controller := RSCanvasController new.
	(controller interactions at: RSZoomToFitCanvasInteraction) noZoomToFitOnStart.
	controller
		removeInteractionIfPresent: RSZoomableCanvasInteraction;
		addInteraction: (RSScrollWheelCanvasInteraction new
				 speed: 60;
				 yourself).
	aCanvas @ controller.
	aCanvas
		when: RSExtentChangedEvent
		do: [ :event | renderer fitWidthAndTop: shape in: aCanvas ]
		for: self.
	renderer fitWidthAndTop: shape in: aCanvas
]

{ #category : 'actions' }
PharoBatteriesWelcome >> followUrl: aString [
	"Open the URL in the browser of the system; where there is none, as in a web page, put it on the clipboard"

	[ WebBrowser openOn: aString ]
		on: Error
		do: [ :error |
			Clipboard clipboardText: aString.
			self inform: 'Copied to the clipboard: ' , aString ]
]

{ #category : 'initialization' }
PharoBatteriesWelcome >> initializePresenters [
	roassal := self instantiate: SpRoassalPresenter.
	roassal script: [ :canvas | self drawOn: canvas ]
]

{ #category : 'initialization' }
PharoBatteriesWelcome >> initializeWindow: aWindowPresenter [
	aWindowPresenter
		title: 'Pharo, batteries included';
		initialExtent: 760 @ 620
]

{ #category : 'accessing' }
PharoBatteriesWelcome >> roassal [
	"My SpRoassalPresenter"

	^ roassal
]

{ #category : 'rendering' }
PharoBatteriesWelcome >> sourceDocument: aString [
	^ CMarkDocument withAll: {
			  (CMarkParagraph with: { (CMarkEmph with: {
						    (CMarkText literal: 'This VM cannot parse Markdown (it has no CMarkPlugin): here is the source.') }) }).
			  (CMarkCodeBlock literal: aString info: '') }
]

{ #category : 'rendering' }
PharoBatteriesWelcome >> theme [
	| theme |
	theme := Smalltalk ui theme backgroundColor luminance < 0.5
		         ifTrue: [ CMarkRoassalTheme dark ]
		         ifFalse: [ CMarkRoassalTheme default ].
	^ theme
		  codeWrap: #characters;
		  breakLongWords: true;
		  linkAction: [ :url | self followUrl: url ];
		  yourself
]
