"
I am the OSWindow driver of the Pharo world in a web page. The WebAssembly VM runs in a Web Worker, and the page owns the canvas. My only contact with the page is the WebDisplayPlugin, a builtin plugin of that VM, plus the interrupt semaphore of its WebHostPlugin.

I am suitable only when the page provides a display (primitiveWebDisplayIsAvailable), so the node VM, the Console page and native VMs never pick me. Natively every primitive fails and its fallback code runs instead, which keeps me testable on a desktop VM: #pushEvent: then stands in for the page.

My class-side #startUp: is registered with the SessionManager. The stock image saves OSSDL2Driver as the driver class, so I install myself before the world opens whenever the page provides a display.

The page queues its input as records of 8 integers:
	type, timestamp, x, y, button | keySym | width, modifiers | (buttons << 8), scanCode | codePoint | dx | height, repeat | dy
Types: 1 mouse move, 2 button press, 3 button release, 4 wheel, 5 key down, 6 key up, 7 text input (one code point), 8 resize, 9 focus (button field 1 in, 0 out), 10 close, 11 save request.
Modifiers: 1 shift, 2 ctrl, 4 alt, 8 cmd; text input ignores them, as with SDL. Press and release number the button as SDL does (1 left, 2 middle, 3 right), and the held buttons are SDL masks (1 left, 2 middle, 4 right). A resize to less than one pixel keeps one.

My event loop runs at lowIOPriority and waits on the input semaphore, which webdisplay_push_event() signals. Alt, Ctrl or Cmd with the period key, and the page's Stop button (vm_interrupt(), which signals my interrupt semaphore), interrupt the busy process as the user interrupt key does on desktop VMs. A save request snapshots the image; the worker then stores it.

The page has one canvas. It shows the newest of my windows that handles events, as the world and the Emergency Debugger do (a window opened without an event handler stays behind it), and that window gets the input. When it is destroyed, the one shown before comes back.
"
Class {
	#name : 'OSWebDriver',
	#superclass : 'OSWindowDriver',
	#instVars : [
		'window',
		'windows',
		'inputSemaphore',
		'interruptSemaphore',
		'eventBuffer',
		'eventProcess',
		'interruptProcess',
		'pendingEvents'
	],
	#category : 'OSWindow-Web',
	#package : 'OSWindow-Web'
}

{ #category : 'class initialization' }
OSWebDriver class >> initialize [
	"Take part in the session startup (see #startUp:), before the world opens: Pharo 12 opens it at the end of the startup list (ErrorHandlerSessionHandler), but Pharo 15 at the default priority of the System category (UIManager class>>#startUp:), so take a priority before it, that of FileLocator. Loading the package again does not register me twice: a registration replaces the one of the same name. (Pharo 15 refuses to unregister a class that is not registered)"

	SessionManager default registerSystemClassNamed: self name atPriority: 90
]

{ #category : 'testing' }
OSWebDriver class >> isSuitable [

	^ self primIsAvailable
]

{ #category : 'testing' }
OSWebDriver class >> isSupported [

	^ self primIsAvailable
]

{ #category : 'primitives' }
OSWebDriver class >> primIsAvailable [
	"Answer whether the page provides a display. Only the world page does"

	<primitive: #primitiveWebDisplayIsAvailable module: #WebDisplayPlugin>
	^ false
]

{ #category : 'system startup' }
OSWebDriver class >> shutDown: quitting [
	"OSWindowDriver is registered too, and shuts the current driver down"
]

{ #category : 'system startup' }
OSWebDriver class >> startUp: isImageStarting [
	"I run in the System startup list, before the world opens (see #initialize). The image may have saved another driver class (the stock image saves OSSDL2Driver), so take over whenever the page provides a display. An image saved in the page keeps me as its driver class, so give the choice back when it starts elsewhere"

	isImageStarting ifFalse: [ ^ self ].
	self isSuitable ifFalse: [
		DriverClass == self ifTrue: [ DriverClass := nil ].
		^ self ].
	DriverClass := self.
	Current := nil
]

{ #category : 'events' }
OSWebDriver >> afterMainPharoWindowCreated: anOSWindow [
	"OSWorldRenderer>>#doActivate opens the world at the extent it was saved with, but the page reported the size of its canvas before the image started: take that size now, before the world paints its first frame, which then fills the canvas"

	| renderer |
	renderer := self currentWorld worldState worldRenderer.
	((renderer respondsTo: #window) and: [ renderer window == anOSWindow ])
		ifTrue: [ renderer checkForNewScreenSize ]
]

{ #category : 'window creation' }
OSWebDriver >> createWindowWithAttributes: attributes osWindow: osWindow [

	| backend |
	backend := OSWebBackendWindow fromAttributes: attributes for: osWindow.
	backend
		osWindow: osWindow;
		driver: self.
	windows addLast: backend.
	self showWindow.
	^ backend
]

{ #category : 'events' }
OSWebDriver >> eventLoop [
	"Drain the queue before waiting: the page may have pushed events while the image was not listening, for instance during a snapshot"

	[
	self processEvents.
	inputSemaphore
		wait;
		consumeAllSignals ] repeat
]

{ #category : 'events' }
OSWebDriver >> handleEvent: anEvent [
	"anEvent is a record of 8 integers, see my class comment"

	(anEvent at: 1) = 11 ifTrue: [ ^ self saveImage ].
	(self isInterruptKey: anEvent) ifTrue: [ ^ self interruptUser ].
	window ifNotNil: [ window handleWebEvent: anEvent ]
]

{ #category : 'initialization' }
OSWebDriver >> initialize [

	super initialize.
	windows := OrderedCollection new.
	inputSemaphore := Semaphore new.
	interruptSemaphore := Semaphore new.
	eventBuffer := IntegerArray new: 8.
	pendingEvents := SharedQueue new
]

{ #category : 'events' }
OSWebDriver >> interruptLoop [
	"The page's Stop button calls vm_interrupt(), which signals interruptSemaphore through the WebHostPlugin"

	[
	interruptSemaphore wait.
	self interruptUser ] repeat
]

{ #category : 'events' }
OSWebDriver >> interruptUser [
	"Interrupt the busy process, as the user interrupt key does on desktop VMs. UserInterruptHandler>>#processToInterrupt takes the first process ready to run below my priority, which can be another process of the priority of the UI, while the UI process runs a loop: no debugger can open then, since the UI process draws it. So interrupt the UI process when it is ready to run, that is, waits on no semaphore, and leave the other cases to UserInterruptHandler"

	| ui |
	ui := MorphicUIManager uiProcess.
	(ui isNil or: [ (self isReadyToRun: ui) not ]) ifTrue: [
		^ UserInterruptHandler new handleUserInterrupt ].
	UserInterruptHandler cmdDotEnabled ifFalse: [ ^ self ].
	[ ui debugWithTitle: 'User Interrupt' ]
		on: Error
		fork: [ :ex | ex pass ]
]

{ #category : 'testing' }
OSWebDriver >> isReadyToRun: aProcess [
	"Whether aProcess is in a run queue of the Processor, rather than waiting on a semaphore (a Delay, say), suspended or terminated"

	| list |
	list := aProcess suspendingList.
	^ list notNil and: [ (list isKindOf: Semaphore) not ]
]

{ #category : 'testing' }
OSWebDriver >> isInterruptKey: anEvent [
	"Alt, Ctrl or Cmd with the period key, as SDL_KeyDownEvent>>#isUserInterrupt checks for OSSDL2Driver"

	^ (anEvent at: 1) = 5 and: [
		  (anEvent at: 5) = $. asInteger and: [
			  (anEvent at: 6) anyMask: 2r1110 ] ]
]

{ #category : 'primitives' }
OSWebDriver >> primNextEventInto: anIntegerArray [
	"Copy the next page event into anIntegerArray (8 integers). Answer false when none is queued"

	<primitive: #primitiveWebNextEvent module: #WebDisplayPlugin>
	| event |
	event := pendingEvents nextOrNil ifNil: [ ^ false ].
	1 to: anIntegerArray size do: [ :index |
		anIntegerArray at: index put: (event at: index ifAbsent: [ 0 ]) ].
	^ true
]

{ #category : 'primitives' }
OSWebDriver >> primSetInputSemaphore: semaphoreIndex [

	<primitive: #primitiveWebSetInputSemaphore module: #WebDisplayPlugin>
	^ nil
]

{ #category : 'primitives' }
OSWebDriver >> primSetInterruptSemaphore: semaphoreIndex [

	<primitive: #primitiveSetInterruptSemaphore module: #WebHostPlugin>
	^ nil
]

{ #category : 'events' }
OSWebDriver >> processEvents [
	"Deliver the queued page events. An error must not stop the event loop, so debug it in a process of its own, as OSSDL2Driver does"

	[ self primNextEventInto: eventBuffer ] whileTrue: [
		[ self handleEvent: eventBuffer ]
			on: UnhandledException
			do: [ :error |
				error freeze.
				[ error debug ] fork ] ]
]

{ #category : 'native fallback' }
OSWebDriver >> pushEvent: anArray [
	"Queue a page event and signal the input semaphore, as webdisplay_push_event() does in the VM. Only the fallback code of #primNextEventInto: reads this queue, so native tests drive me through here"

	pendingEvents nextPut: anArray.
	inputSemaphore signal
]

{ #category : 'window creation' }
OSWebDriver >> registerWindow: aBackendWindow [
	"Pharo 15 takes the current driver for an OSSDL2Driver when it restarts the world renderer on another driver (OSWorldRenderer class>>#restart), which hands it the windows of OSSDL2Driver: they are not mine to show"
]

{ #category : 'events' }
OSWebDriver >> saveImage [
	"The page asked for a snapshot; the worker stores the image once the VM has written it. Fork, because the snapshot shuts this event loop down and starts a new one"

	[ Smalltalk snapshot: true andQuit: false ]
		forkAt: Processor userSchedulingPriority
		named: 'Web save'
]

{ #category : 'window creation' }
OSWebDriver >> showWindow [
	"Show the window that the page should show (see my class comment), unless it is shown"

	| shown |
	shown := windows reverse
		         detect: [ :each | each handlesEvents ]
		         ifNone: [ windows ifEmpty: [ nil ] ifNotEmpty: [ windows last ] ].
	shown == window ifTrue: [ ^ self ].
	window := shown.
	window ifNotNil: [ window open ]
]

{ #category : 'system startup' }
OSWebDriver >> shutDown: quitting [

	self stopEventLoop
]

{ #category : 'events' }
OSWebDriver >> startEventLoop [

	self stopEventLoop.
	interruptSemaphore consumeAllSignals.
	self primSetInputSemaphore:
		(Smalltalk registerExternalObject: inputSemaphore).
	self primSetInterruptSemaphore:
		(Smalltalk registerExternalObject: interruptSemaphore).
	eventProcess := [ self eventLoop ]
		                forkAt: Processor lowIOPriority
		                named: 'Web event loop'.
	interruptProcess := [ self interruptLoop ]
		                    forkAt: Processor userInterruptPriority
		                    named: 'Web interrupt watcher'
]

{ #category : 'system startup' }
OSWebDriver >> startUp: resuming [

	self startEventLoop
]

{ #category : 'events' }
OSWebDriver >> stopEventLoop [
	"Never terminate the active process: an event may have started the shutdown"

	{
		eventProcess.
		interruptProcess } do: [ :process |
		(process isNil or: [ process == Processor activeProcess ]) ifFalse: [
			process terminate ] ].
	eventProcess := interruptProcess := nil.
	self
		primSetInputSemaphore: 0;
		primSetInterruptSemaphore: 0.
	Smalltalk
		unregisterExternalObject: inputSemaphore;
		unregisterExternalObject: interruptSemaphore
]

{ #category : 'window creation' }
OSWebDriver >> unregisterWindowWithId: aWindowId [
	"Pharo 15 takes the current driver for an OSSDL2Driver when it finalizes a window of SDL (SDL_Window class>>#finalizeResourceData:), as after a restart of the world renderer on another driver (see #registerWindow:): it is not mine"
]

{ #category : 'accessing' }
OSWebDriver >> window [
	"The window that the page shows, and that gets its input"

	^ window
]

{ #category : 'window creation' }
OSWebDriver >> windowDestroyed: aBackendWindow [

	windows remove: aBackendWindow ifAbsent: [ ^ self ].
	self showWindow
]

{ #category : 'accessing' }
OSWebDriver >> windows [
	"My windows, the oldest first"

	^ windows
]
