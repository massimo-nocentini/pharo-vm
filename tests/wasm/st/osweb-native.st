"Native check of the image side of the browser world: the OSWindow-Web package,
 driven on a desktop Pharo VM.  There every WebDisplayPlugin primitive fails, its
 fallback code stands in for the page, and OSWebDriver>>#pushEvent: queues the
 events the page would send.  It forces the driver and saves the image, so run it
 on a scratch copy of the prepared image, or of a stock image with
 web-bootstrap.st, only:

	pharo --headless <copy>.image --no-default-preferences st --save --quit osweb-native.st
	pharo --headless <copy>.image --no-default-preferences --interactive st osweb-native.st

 (Pharo 15 takes a .st file through the st command only; Pharo 12 takes it so
 too.)  Both make OSWebDriver suitable, and only the second one, with the world
 (--interactive), checks.  After the first one, the second one boots into the world
 through the OSWebDriver startUp: hook; alone, it restarts the world renderer on
 OSWebDriver, which web-bootstrap.st does too.  Each check prints
 'ok <name>' or 'not ok <name>: <reason>', the world is dumped to osweb-native.png
 next to the image, and the VM exits with 0 when every check passed.  The save
 request check saves the image with this script running in it: take a new copy
 for every run, as lane 80 (tests/wasm/lanes/80-world-harness.sh) does with the
 prepared image of build-wasm/image/web when it has a host Pharo.

 The fonts check wants the fonts that the build prepared: PHARO_WEB_FONTS when it is
 set (freetype or bitmap, as for prepare-web-image.st), else the fonts of the manifest
 of the build, WASM_DIR/web/manifest.json (make wasm-check sets WASM_DIR), else
 whichever the image has.  FreeType fonts are those of the stock Pharo 12 and Pharo 15
 images, each standard font a LogicalFont of the family and size below, drawn by a
 FreeTypeFont (the host VM has FreeType); bitmap fonts are drawn by a StrikeFont."

OSWebDriver isSuitable ifFalse: [
	OSWebDriver class
		compile: 'primIsAvailable
	"osweb-native.st: a desktop VM stands in for the world page"

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
	(Delay forSeconds: 180) wait.
	say value: 'not ok osweb-native: timed out'.
	Smalltalk exit: 2 ] forkAt: Processor timingPriority - 1.
	[
	[
	| failures check waitFor renderer driver push click key type frameStats desktop playground playgroundText popupMenus debuggers debugger busyUI busy png form before |
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
	           deadline := Time millisecondClockValue + 15000.
	           [ condition value or: [ Time millisecondClockValue > deadline ] ]
		           whileFalse: [ (Delay forMilliseconds: 20) wait ].
	           condition value ].
	push := [ :record | driver pushEvent: record ].
	click := [ :point :button |
	         | mask |
	         mask := #( 1 2 4 ) at: button.
	         push value: { 1. 0. point x. point y. 0. 0. 0. 0 }.
	         push value: { 2. 0. point x. point y. button. mask << 8. 0. 0 }.
	         (Delay forMilliseconds: 50) wait.
	         push value: { 3. 0. point x. point y. button. 0. 0. 0 } ].
	key := [ :symbol :modifiers |
	       push value: { 5. 0. 0. 0. symbol. modifiers. 0. 0 }.
	       push value: { 6. 0. 0. 0. symbol. modifiers. 0. 0 } ].
	type := [ :string | string do: [ :character | push value: { 7. 0. 0. 0. 0. 0. character codePoint. 0 } ] ].
	frameStats := [ renderer osWindowRenderer stats ].
	popupMenus := [ World submorphs select: [ :morph | morph isKindOf: MenuMorph ] ].
	"A point of the desktop, where no window is: a right click there opens the world
	 menu.  (The windows differ: Pharo 15 opens a Welcome window, and a smaller Playground)"
	desktop := [
	           | found |
	           (World bounds height - 60 to: 60 by: -20) do: [ :y |
		           (World bounds width - 20 to: 20 by: -20) do: [ :x |
			           found ifNil: [
				           ((World morphsAt: x @ y) allSatisfy: [ :morph | morph == World ])
					           ifTrue: [ found := x @ y ] ] ] ].
	           found ].
	playgroundText := [ (playground allMorphs detect: [ :morph | morph isKindOf: RubScrolledTextMorph ]) text asString ].
	"The debugger windows, and the one on a process: their label is the print string of
	 the interrupted context, which says nothing reliable"
	debuggers := [
	             World systemWindows select: [ :window |
		             (window model respondsTo: #presenter) and: [ window model presenter isKindOf: StDebugger ] ] ].
	debugger := [ :process |
	            debuggers value
		            detect: [ :window | window model presenter interruptedProcess == process ]
		            ifNone: [ nil ] ].
	"The page pushes events between two slices of the VM, when no Smalltalk process
	 runs.  A user interrupt stops the first runnable process below the one that
	 handles it (UserInterruptHandler>>#processToInterrupt), and that would be this
	 one if it pushed, or polled, then.  So the busy UI process forks the push, and
	 this one waits until it is handled.  The busy loop yields: a process of its
	 priority that it starved, such as the deferred flush of Epicea, would come
	 first otherwise.  Answers the busy process, which the interrupt must debug"
	busyUI := [ :fromThePage |
	          | stopAt handled process |
	          stopAt := Time millisecondClockValue + 8000.
	          handled := Semaphore new.
	          World defer: [
		          process := Processor activeProcess.
		          [
		          (Delay forMilliseconds: 100) wait.
		          fromThePage value.
		          (Delay forMilliseconds: 500) wait.
		          handled signal ] forkAt: Processor highIOPriority.
		          [ Time millisecondClockValue < stopAt ] whileTrue: [ Processor yield ] ].
	          handled waitTimeoutMSecs: 15000.
	          process ].

	((World worldState worldRenderer isKindOf: OSWorldRenderer) and: [
		 World worldState worldRenderer driver isKindOf: OSWebDriver ])
		ifTrue: [ say value: '# the world runs on OSWebDriver already' ]
		ifFalse: [
			say value: '# restarting the world renderer on OSWebDriver'.
			"As web-bootstrap.st does: the restart shuts the current driver down, once
			 its event loop is stopped"
			(World worldState worldRenderer isKindOf: OSWorldRenderer) ifTrue: [
				World worldState worldRenderer driver ifNotNil: [ :old | old shutDown: false ] ].
			OSWindowDriver classPool at: #DriverClass put: OSWebDriver.
			OSWorldRenderer restart ].
	renderer := World worldState worldRenderer.
	check value: 'world through OSWebDriver' value: [
		(waitFor value: [
			 renderer := World worldState worldRenderer.
			 (renderer isKindOf: OSWorldRenderer) and: [
				 (renderer driver isKindOf: OSWebDriver) and: [
					 (renderer osWindowRenderer isKindOf: OSWebFormRenderer) and: [
						 frameStats value first > 0 and: [ frameStats value last > 0 ] ] ] ] ])
			or: [ { renderer. renderer driver. renderer osWindowRenderer } ] ].
	driver := renderer driver.
	check value: 'fonts' value: [
		| wanted told stock wrong |
		told := OSEnvironment current at: 'PHARO_WEB_FONTS' ifAbsent: [ '' ].
		told ifEmpty: [
			told := [
			        ((OSEnvironment current at: 'WASM_DIR' ifAbsent: [ nil ]) ifNotNil: [ :dir |
				         (STONJSON fromString: (dir asFileReference / 'web' / 'manifest.json') contents)
					         at: 'fonts'
					         ifAbsent: [ nil ] ]) ifNil: [ '' ] ]
				        on: Error
				        do: [ :error | '' ] ].
		wanted := told ifEmpty: [
			          FreeTypeSystemSettings loadFt2Library
				          ifTrue: [ 'freetype' ]
				          ifFalse: [ 'bitmap' ] ].
		say value: '# fonts: ' , wanted , (told
				 ifEmpty: [ ', as the image has them (neither PHARO_WEB_FONTS nor WASM_DIR tell)' ]
				 ifNotEmpty: [ '' ]).
		wanted = 'freetype'
			ifTrue: [
				"The standard fonts of the stock images, Pharo 12 and Pharo 15 alike"
				stock := #( #( #defaultFont 'Source Sans Pro' 10 ) #( #listFont 'Source Sans Pro' 10 )
				            #( #menuFont 'Source Sans Pro' 10 ) #( #buttonFont 'Source Sans Pro' 10 )
				            #( #windowTitleFont 'Source Sans Pro' 11 ) #( #balloonFont 'Source Sans Pro' 9 )
				            #( #haloFont 'Source Sans Pro' 9 ) #( #codeFont 'Source Code Pro' 10 ) ).
				wrong := (stock collect: [ :spec |
					          | font |
					          font := StandardFonts perform: spec first.
					          ((font isKindOf: LogicalFont) and: [
						           font familyName = spec second and: [
							           font pointSize = spec third and: [ font realFont isKindOf: FreeTypeFont ] ] ])
						          ifTrue: [ nil ]
						          ifFalse: [ spec first -> font printString ] ]) reject: [ :each | each isNil ].
				(FreeTypeSystemSettings loadFt2Library and: [ wrong isEmpty ]) or: [
					{ FreeTypeSystemSettings loadFt2Library } , wrong asArray ] ]
			ifFalse: [
				FreeTypeSystemSettings loadFt2Library not and: [ StandardFonts defaultFont realFont isKindOf: StrikeFont ] ] ].
	check value: 'title' value: [
		renderer window backendWindow title = Smalltalk imageFile fullName or: [
			renderer window backendWindow title ] ].
	check value: 'cursor' value: [
		renderer activateCursor: Cursor webLink withMask: Cursor webLink maskForm.
		true ].

	before := frameStats value.
	click value: 60 @ 9 value: 1.
	check value: 'click opens a menubar menu' value: [
		waitFor value: [
			(World submorphs anySatisfy: [ :morph | morph isKindOf: MenubarMenuMorph ]) and: [
				frameStats value first > before first ] ] ].
	click value: desktop value value: 1.
	check value: 'click on the desktop closes it' value: [
		waitFor value: [ (World submorphs anySatisfy: [ :morph | morph isKindOf: MenubarMenuMorph ]) not ] ].
	click value: desktop value value: 3.
	check value: 'right click opens the world menu' value: [ waitFor value: [ popupMenus value notEmpty ] ].
	key value: 27 value: 0.
	check value: 'Escape closes it' value: [ waitFor value: [ popupMenus value isEmpty ] ].

	World defer: [ StPlaygroundPresenter open ].
	check value: 'Playground opens' value: [
		waitFor value: [
			(playground := World systemWindows detect: [ :window | window label = 'Playground' ] ifNone: [ nil ]) notNil ] ].
	click value: (playground allMorphs detect: [ :morph | morph isKindOf: RubScrolledTextMorph ]) boundsInWorld center rounded value: 1.
	type value: '3 + 4'.
	check value: 'typing' value: [ (waitFor value: [ playgroundText value = '3 + 4' ]) or: [ playgroundText value ] ].
	key value: 27 value: 0.
	(Delay forMilliseconds: 300) wait.
	check value: 'Escape types nothing' value: [
		playgroundText value = '3 + 4' or: [ playgroundText value asArray collect: [ :character | character codePoint ] ] ].
	key value: $p asInteger value: 2.
	check value: 'Ctrl+P prints 7' value: [
		waitFor value: [
			World allMorphs anySatisfy: [ :morph |
				(morph isKindOf: RubEditingArea) and: [ morph text asString = '7' ] ] ] ].

	png := (FileLocator imageDirectory / 'osweb-native.png') asFileReference.
	png ensureDelete.
	PNGReadWriter putForm: renderer display onFileNamed: png fullName.
	check value: 'PNG of the world' value: [
		form := Form fromFileNamed: png fullName.
		form extent = renderer display extent and: [ form bits asSet size > 16 ] ].

	Clipboard clipboardText: 'from the web'.
	check value: 'clipboard round trip' value: [ Clipboard clipboardText asString = 'from the web' ].
	"A click in the Playground closes the popover of print it"
	click value: (playground allMorphs detect: [ :morph | morph isKindOf: RubScrolledTextMorph ]) boundsInWorld center rounded value: 1.
	key value: $v asInteger value: 2.
	check value: 'Ctrl+V pastes it' value: [
		(waitFor value: [ playgroundText value includesSubstring: 'from the web' ]) or: [ playgroundText value ] ].
	"Option or AltGr type @ on many layouts, so text input with Alt held still types"
	push value: { 7. 0. 0. 0. 0. 4. $@ codePoint. 0 }.
	check value: 'text typed with Alt held' value: [
		(waitFor value: [ playgroundText value includes: $@ ]) or: [ playgroundText value ] ].

	check value: 'no debugger yet' value: [ debuggers value isEmpty or: [ debuggers value ] ].
	busy := busyUI value: [ driver pushEvent: { 5. 0. 0. 0. $. asInteger. 4. 55. 0 } ].
	check value: 'Alt+. interrupts the busy UI process' value: [
		(waitFor value: [ (debugger value: busy) notNil ]) or: [ debuggers value ] ].
	(debugger value: busy) ifNotNil: [ :window | World defer: [ window close ] ].
	check value: 'the debugger closes' value: [ waitFor value: [ debuggers value isEmpty ] ].
	busy := busyUI value: [ (driver instVarNamed: #interruptSemaphore) signal ].
	check value: 'Stop (vm_interrupt) interrupts the busy UI process' value: [
		(waitFor value: [ (debugger value: busy) notNil ]) or: [ debuggers value ] ].
	(debugger value: busy) ifNotNil: [ :window | World defer: [ window close ] ].
	waitFor value: [ debuggers value isEmpty ].

	before := frameStats value.
	push value: #( 8 0 0 0 800 0 500 0 ).
	check value: 'resize to 800x500' value: [
		(waitFor value: [ renderer display extent = (800 @ 500) and: [ World extent = (800 @ 500) ] ])
			or: [ { renderer display extent. World extent } ] ].
	check value: 'a full frame at the new extent' value: [
		waitFor value: [ frameStats value second - before second >= (800 * 500) ] ].
	push value: #( 8 0 0 0 400 0 250 0 ).
	check value: 'resize to 400x250 keeps the canvas scale 1' value: [
		(waitFor value: [ renderer display extent = (400 @ 250) ]) and: [ OSWorldRenderer canvasScaleFactor = 1 ] ].
	"A hidden canvas has no pixels"
	push value: #( 8 0 0 0 0 0 0 0 ).
	check value: 'resize to 0x0 keeps one pixel' value: [
		(waitFor value: [ renderer display extent = (1 @ 1) ]) or: [ renderer display extent ] ].
	push value: #( 8 0 0 0 976 0 665 0 ).
	check value: 'the world comes back from 0x0' value: [
		(waitFor value: [ renderer display extent = (976 @ 665) ]) and: [
			click value: desktop value value: 3.
			(waitFor value: [ popupMenus value notEmpty ]) and: [ debuggers value isEmpty ] ] ].
	key value: 27 value: 0.
	waitFor value: [ popupMenus value isEmpty ].

	before := Smalltalk imageFile modificationTime.
	(Delay forMilliseconds: 1100) wait.
	push value: #( 11 0 0 0 0 0 0 0 ).
	check value: 'save request snapshots the image' value: [
		waitFor value: [ Smalltalk imageFile modificationTime > before ] ].
	check value: 'events after the save' value: [
		(waitFor value: [ (driver instVarNamed: #eventProcess) notNil ]) and: [
			click value: desktop value value: 3.
			waitFor value: [ popupMenus value notEmpty ] ] ].
	key value: 27 value: 0.

	say value: 'osweb-native: ' , failures printString , ' failed'.
	Smalltalk exit: (failures = 0
			 ifTrue: [ 0 ]
			 ifFalse: [ 1 ]) ]
		on: Error
		do: [ :error |
			say value: 'not ok osweb-native: ' , error description , ' in ' , error signalerContext printString.
			Smalltalk exit: 1 ] ] forkAt: Processor userInterruptPriority ]
