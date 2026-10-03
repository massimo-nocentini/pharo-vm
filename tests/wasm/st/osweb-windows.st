"Native check of the windows of OSWebDriver, the driver of the browser world: the
 page has one canvas, which shows the newest window that handles events, and that
 window gets the input; when it is destroyed, the one shown before comes back and
 draws itself whole again.  As osweb-native.st, it runs on a desktop Pharo VM, where
 every WebDisplayPlugin primitive fails and OSWebDriver>>#pushEvent: queues what the
 page would send.  The Emergency Debugger's window, or one such as it opened in Pharo 15,
 which has no Emergency Debugger, stands for a window that handles events.  It forces
 the driver and saves the image, so run it on a scratch copy of the prepared image of the
 world (build-wasm/image/web) only:

	pharo --headless <copy>.image --no-default-preferences st --save --quit osweb-windows.st
	pharo --headless <copy>.image --no-default-preferences --interactive st osweb-windows.st

 Only the second one, with the world (--interactive), checks.

 Each check prints 'ok <name>' or 'not ok <name>: <reason>', and the VM exits with 0
 when every check passed.  Lane 82 (tests/wasm/lanes/82-osweb-windows.sh) runs it."

OSWebDriver isSuitable ifFalse: [
	OSWebDriver class
		compile: 'primIsAvailable
	"osweb-windows.st: a desktop VM stands in for the world page"

	^ true'
		classified: 'primitives' ].

(CommandLineArguments new hasOption: 'interactive') ifTrue: [
	| say |
	say := [ :line |
	       Stdio stdout
		       nextPutAll: line asString utf8Encoded;
		       lf;
		       flush ].
	[
	(Delay forSeconds: 120) wait.
	say value: 'not ok osweb-windows: timed out'.
	Smalltalk exit: 2 ] forkAt: Processor timingPriority - 1.
	[
	[
	| failures check waitFor renderer driver push menus rightClick worldBackend plain ed closeEd blits |
	failures := 0.
	check := [ :name :block |
	         | result |
	         result := [ block value ] on: Error do: [ :error | error description , ' in ' , error signalerContext printString ].
	         result == true
		         ifTrue: [ say value: 'ok ' , name ]
		         ifFalse: [
			         failures := failures + 1.
			         say value: 'not ok ' , name , ': ' , result printString ] ].
	waitFor := [ :condition |
	           | deadline |
	           deadline := Time millisecondClockValue + 8000.
	           [ condition value or: [ Time millisecondClockValue > deadline ] ]
		           whileFalse: [ (Delay forMilliseconds: 20) wait ].
	           condition value ].
	waitFor value: [
		(World worldState worldRenderer isKindOf: OSWorldRenderer) and: [
			World worldState worldRenderer driver isKindOf: OSWebDriver ] ].
	renderer := World worldState worldRenderer.
	driver := renderer driver.
	push := [ :record | driver pushEvent: record ].
	menus := [ World submorphs select: [ :morph | morph isKindOf: MenuMorph ] ].
	"a right click opens the world menu, if the world gets the input; close it again"
	rightClick := [
	             | opened |
	             push value: { 1. 0. 600. 400. 0. 0. 0. 0 }.
	             push value: { 2. 0. 600. 400. 3. 4 << 8. 0. 0 }.
	             (Delay forMilliseconds: 50) wait.
	             push value: { 3. 0. 600. 400. 3. 0. 0. 0 }.
	             opened := waitFor value: [ menus value notEmpty ].
	             menus value do: [ :menu | menu delete ].
	             waitFor value: [ menus value isEmpty ].
	             opened ].
	"the blits of the world's renderer, which natively only count"
	blits := [ renderer osWindowRenderer stats first ].
	worldBackend := renderer window backendWindow.
	check value: 'the world runs on OSWebDriver, which shows its window' value: [
		(driver isKindOf: OSWebDriver) and: [ driver window == worldBackend and: [ worldBackend isShown ] ] ].
	check value: 'a right click opens the world menu' value: [ rightClick value ].

	"A window without an event handler stays behind the world"
	plain := OSWindow createWithAttributes: (OSWindowAttributes new
			          title: 'plain';
			          extent: 200 @ 100;
			          yourself).
	check value: 'a window without an event handler leaves the world shown' value: [
		driver window == worldBackend and: [ driver windows size = 2 and: [ plain backendWindow isShown not ] ] ].
	check value: 'and the world gets the input' value: [ rightClick value ].
	plain destroy.
	check value: 'which it keeps once that window is destroyed' value: [
		driver window == worldBackend and: [ driver windows size = 1 and: [ rightClick value ] ] ].

	"The Emergency Debugger's window takes the canvas and the input, then gives them back.
	 Pharo 15 has no Emergency Debugger: there, a window such as it opened, with an event
	 handler and a Form renderer"
	(Smalltalk hasClassNamed: #EDDisplayInterface)
		ifTrue: [
			ed := (Smalltalk at: #EDDisplayInterface) new.
			ed ensureWindowOpen.
			closeEd := [ ed terminateWindow ] ]
		ifFalse: [
			ed := OSWindow
				      createWithAttributes: (OSWindowAttributes new
						       title: 'Emergency Debugger';
						       extent: 600 @ 400;
						       resizable: false;
						       yourself)
				      eventHandler: OSWindowEventVisitor new.
			ed newFormRenderer: (Form extent: 600 @ 400 depth: 32).
			closeEd := [ ed destroy ] ].
	check value: 'the Emergency Debugger''s window is shown, not the world' value: [
		driver window ~~ worldBackend and: [ driver window handlesEvents and: [ worldBackend isShown not ] ] ].
	check value: 'the world does not draw meanwhile' value: [
		| before |
		before := blits value.
		World addMorph: (Morph new position: 10 @ 10; extent: 50 @ 50; yourself).
		(Delay forMilliseconds: 300) wait.
		blits value = before ].
	check value: 'nor does it get the input' value: [ rightClick value not ].
	closeEd value.
	check value: 'once that window is destroyed the world is shown again' value: [
		driver window == worldBackend and: [ worldBackend isShown and: [ driver windows size = 1 ] ] ].
	check value: 'and gets the input again' value: [ rightClick value ].
	check value: 'and draws again' value: [
		| before |
		before := blits value.
		World addMorph: (Morph new position: 100 @ 100; extent: 50 @ 50; yourself).
		waitFor value: [ blits value > before ] ].
	say value: 'osweb-windows: ' , failures printString , ' failed'.
	Smalltalk exit: (failures = 0 ifTrue: [ 0 ] ifFalse: [ 1 ]) ]
		on: Error
		do: [ :error |
			say value: 'not ok osweb-windows: ' , error description , ' in ' , error signalerContext printString.
			Smalltalk exit: 1 ] ] forkAt: Processor userInterruptPriority ]
