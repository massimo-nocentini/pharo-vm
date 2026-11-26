"smoke.st - check the Bloc image that load.st saved (docs/Bloc.md)

	pharo --headless Pharo15-bloc.image --no-default-preferences st --no-source packaging/bloc/smoke.st

(build.sh runs it, natively; the lanes of the WebAssembly VM run it in the node VM too.)  It checks, in order:

	1. the classes of Bloc, Alexandrie, Album, Toplo and of the two packages of src/, the welcome window pending,
	   and the buffers of the Morphic host: PharoBloc-Web's newExternalForm, a PharoBlocMorphicForm only on a VM
	   without SurfacePlugin;
	2. drawings by Alexandrie (cairo, FreeType and HarfBuzz through the FFI): an element with a border, a rounded
	   rectangle and a text, a Toplo button, and the pane of the welcome window, each of its extent and with pixels
	   of more than one colour, and a PharoBlocMorphicForm that cairo draws into;
	3. the test packages below, each of which must run tests, with no failure and no error.

The test packages are those that pass both natively and in the node VM, without a display; those left out fail
for reasons that are not the image's: Alexandrie-Athens-Tests, Alexandrie-Cairo-Tests (4 and 1 pixel matches of
another cairo), Alexandrie-Cairo-Tests, -CairoFreetype-Tests, -Canvas-Tests, -FontManager-Tests, -FreeType-Tests
and -NewTools-Tests (a resource that downloads the Noto Color Emoji font from GitHub with Zinc, which the
WebAssembly VM cannot), Bloc-Alexandrie-Tests (a pixel match) and Bloc-Tests (focus and settling tests that fail
now and then without a display).

It prints one line per check that failed, then

	Bloc smoke classes <n> tests <run> passed <p> skipped <s> failed <f> errors <e> ok

or the same line ending with FAILED, and exits with status 1 when a check failed."

| out lineEnd line problems classes suites run passed skipped failed errors drawn globals |
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
line := [ :string | out nextPutAll: string; nextPutAll: lineEnd; flush ].
problems := OrderedCollection new.
globals := Smalltalk globals.

"1. the classes"
classes := #( #BlElement #BlSpace #BlTextElement #BlMorphicWindowHost #BlAbstractMorphicHostSpace #AeCanvas
	#AeCairoLibrary #AeFreeTypeLibrary #AeHarfbuzzLibrary #AlbEditorElement #ToButton #ToLabel #ToPane #ToSandBox
	#PharoBlocMorphicForm #PharoBlocWelcome ).
classes do: [ :name | (globals includesKey: name) ifFalse: [ problems add: 'no class ' , name ] ].
problems ifEmpty: [
	(globals at: #PharoBlocWelcome) isPending ifFalse: [ problems add: 'the welcome window is not pending' ].
	((globals at: #BlAbstractMorphicHostSpace) >> #newExternalForm) package name = 'PharoBloc-Web'
		ifFalse: [ problems add: 'BlAbstractMorphicHostSpace>>#newExternalForm is not the one of PharoBloc-Web' ].
	line value: 'Bloc smoke: ' , SystemVersion current printString , ', PharoBlocMorphicForm needed: '
		, (globals at: #PharoBlocMorphicForm) isNeeded printString , ' (the VM '
		, ((globals at: #PharoBlocMorphicForm) isNeeded ifTrue: [ 'has no' ] ifFalse: [ 'has' ]) , ' SurfacePlugin)' ].

"2. the drawings"
drawn := [ :name :form :extent |
	| colours |
	colours := Set new.
	0 to: form height - 1 by: 2 do: [ :y | 0 to: form width - 1 by: 2 do: [ :x | colours add: (form pixelValueAt: x @ y) ] ].
	(extent isNil or: [ form extent = extent ]) ifFalse: [
		problems add: name , ' drawn as ' , form extent printString , ', not ' , extent printString ].
	colours size > 1 ifFalse: [ problems add: name , ' drawn in one colour' ].
	line value: 'Bloc smoke: drew ' , name , ', ' , form extent printString , ', ' , colours size printString , ' colours' ].
problems ifEmpty: [
	[
	| element button pane form |
	element := (globals at: #BlElement) new
		extent: 200 @ 100;
		background: Color red;
		border: ((globals at: #BlBorder) paint: Color blue width: 4);
		geometry: ((globals at: #BlRoundedRectangleGeometry) cornerRadius: 12);
		yourself.
	element addChild: (((globals at: #BlTextElement) new text: ('Hello Bloc' asRopedText fontSize: 24))
		position: 20 @ 30;
		yourself).
	drawn value: 'an element' value: element aeAsForm value: 200 @ 100.
	button := (globals at: #ToButton) new labelText: 'Toplo button'; yourself.
	drawn value: 'a Toplo button' value: button aeAsForm value: nil.
	pane := (globals at: #PharoBlocWelcome) new newPane.
	drawn value: 'the welcome pane' value: pane aeAsForm value: nil.
	"what a space of a Morphic host draws into on the WebAssembly VM, here too"
	form := (globals at: #PharoBlocMorphicForm) newExtent: 64 @ 32.
	((globals at: #AeCairoImageSurface)
		unownedNewForData: form pointer
		format: (globals at: #AeCairoSurfaceFormat) argb32
		width: 64
		height: 32
		stride: form stride) in: [ :surface |
			surface newContextDo: [ :context |
				context sourceColor: Color green; rectangle: (8 @ 8 extent: 24 @ 16); fill ].
			surface flush.
			surface externallyFree ].
	drawn value: 'a PharoBlocMorphicForm' value: form value: 64 @ 32.
	(form colorAt: 16 @ 16) = Color green ifFalse: [
		problems add: 'cairo drew ' , (form colorAt: 16 @ 16) printString , ' into a PharoBlocMorphicForm, not green' ] ]
		on: Error
		do: [ :error |
			problems add: 'drawing: ' , error class name , ': ' , (error messageText ifNil: [ '' ]) asString.
			error signalerContext ifNotNil: [ :context |
				(context stack first: (context stack size min: 12)) do: [ :each | line value: '    ' , each printString ] ] ] ].

"3. the tests"
suites := #( 'Album-Tests' 'Alexandrie-Base-Tests' 'Alexandrie-Blur-Tests' 'Alexandrie-Harfbuzz-Tests'
	'Alexandrie-ImageMatch-Tests' 'Bloc-Animation-Tests' 'Bloc-Display-Tests' 'Bloc-Infinite-Tests' 'Bloc-Layout-Tests'
	'Bloc-SVG-Tests' 'Bloc-Spec2-Tests' 'Bloc-Text-Rope-Tests' 'Bloc-Text-Tests' 'Bloc-UnitedTests' 'Toplo-Tests'
	'Toplo-Theme-Bee-Tests' 'Toplo-Widget-Album-Tests' 'Toplo-Widget-Button-Tests' 'Toplo-Widget-Image-Tests'
	'Toplo-Widget-List-Tests' 'Toplo-Widget-Menu-Tests' 'Toplo-Widget-Select-Tests' 'Toplo-Widget-TabPane-Tests'
	'Toplo-Widget-Table-Tests' 'Toplo-Widget-Tree-Tests' ).
run := passed := skipped := failed := errors := 0.
suites do: [ :packageName |
	(PackageOrganizer default packageNamed: packageName ifAbsent: [ nil ])
		ifNil: [ problems add: 'no package ' , packageName ]
		ifNotNil: [ :package |
			| suite result started |
			suite := TestSuite named: packageName.
			((package definedClasses select: [ :each | (each inheritsFrom: TestCase) and: [ each isAbstract not ] ])
				asSortedCollection: [ :a :b | a name <= b name ]) do: [ :each | suite addTests: each buildSuite tests ].
			started := Time millisecondClockValue.
			result := suite run.
			line value: 'Bloc smoke: ' , packageName , ' ' , result runCount printString , ' run, '
				, result passedCount printString , ' passed, ' , result skippedCount printString , ' skipped, '
				, result failureCount printString , ' failed, ' , result errorCount printString , ' errors in '
				, ((Time millisecondClockValue - started) // 1000) printString , ' s'.
			result runCount = 0 ifTrue: [ problems add: packageName , ' ran no test' ].
			(result failures , result errors) do: [ :each | problems add: 'test ' , each printString ].
			run := run + result runCount.
			passed := passed + result passedCount.
			skipped := skipped + result skippedCount.
			failed := failed + result failureCount.
			errors := errors + result errorCount ] ].

problems do: [ :each | line value: 'Bloc smoke problem: ' , each ].
line value: 'Bloc smoke classes ' , classes size printString , ' tests ' , run printString , ' passed '
	, passed printString , ' skipped ' , skipped printString , ' failed ' , failed printString , ' errors '
	, errors printString , (problems isEmpty ifTrue: [ ' ok' ] ifFalse: [ ' FAILED' ]).
problems isEmpty
	ifTrue: [ Smalltalk exit: 0 ]
	ifFalse: [ Smalltalk exitFailure ]
