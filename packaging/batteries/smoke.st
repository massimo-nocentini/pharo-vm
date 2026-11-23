"smoke.st - a quick check of an image that load.st saved (docs/Batteries.md)

	[BATTERIES_SMOKE=native] pharo --headless Pharo15-batteries.image --no-default-preferences st --no-source packaging/batteries/smoke.st

The image starts again (so the save of load.st left nothing running), has the classes of every project and the RTree
of the fork, the icons of its UI are the Material Symbols in gray, its facades answer in Smalltalk, and the tests of a
few packages pass:

	basic (any VM, the default): RTree-Tests, TimSort-Smalltalk-Tests, VM-Plugins-Primitives-Tests (the
	        UtilsPlugin's Smalltalk twin, and the plugin too when the VM has it), CMark-Tests and
	        CMark-Roassal-Tests (whose tests that parse skip without the CMarkPlugin), Delimcc-Tests,
	        Delimcc-Continuation-Tests and ContinuationTest (Pharo's, with the fixes of Delimcc-Continuation),
	        MaterialSymbols-Tests (which put the gray pack back after each test; skipped when
	        BATTERIES_MATERIALICONS_GROUPS, as for load.st, has no SymbolsTests and the package is not there);
	native (BATTERIES_SMOKE=native, on a VM of this repository, whose plugins and cairo stack the tests then
	        use): also SimdJSON-Tests, SimdJSON-FFI-Tests, TimSort-FFI-Tests, LibGLib-Tests, LibPangoCairo-Tests and
	        LibPoppler-Roassal-Tests, each of which must run tests, not only skip them.

One line per failure and error, then

	Batteries smoke <mode> classes <n> tests <run> passed <p> skipped <s> failed <f> errors <e> ok

ending in FAILED instead of ok when anything went wrong; exit status 0 for ok, 1 otherwise.  Nothing is saved
(--no-source keeps the .changes as it is)."

| environment out lineEnd line problems mode classes suites optional run passed skipped failed errors ok |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
line := [ :string | out nextPutAll: string; nextPutAll: lineEnd; flush ].
problems := OrderedCollection new.
mode := (environment at: 'BATTERIES_SMOKE' ifAbsent: [ '' ]) trimBoth ifEmpty: [ 'basic' ].
(#( 'basic' 'native' ) includes: mode) ifFalse: [
	line value: 'Batteries smoke: BATTERIES_SMOKE is ' , mode , ', not basic or native'.
	Smalltalk exitFailure ].

[
"1. a class or two of every project, the fork's RTree and Roassal"
classes := #( #RSCanvas #RSPangoText #RSPdfPage #RTNaryTreeCollection #RTPackedTreeCollection #RTGridCollection
	#RTLooseQuadtreeCollection #UtilsPlugin #KnuthRandom #SplitMix64 #SimdJSONReader #NeoJSONReader #SimdUTF #TimSort
	#TimSortSmalltalk #PangoLayout #PopplerDocument #BaselineOfRoassalMN #CMark #CMarkParser #CMarkRoassalRenderer
	#CTArray2D #CTKeyedValuedOrderedCollection #CTCategory #PartialContinuation #DelimccPrompt #MaterialIcons
	#MaterialSymbolsThemeIcons ).
classes do: [ :name |
	(Smalltalk globals includesKey: name) ifFalse: [ problems add: 'no class ' , name ] ].
(MCWorkingCopy allWorkingCopies select: [ :copy | copy packageName beginsWith: 'RTree' ]) do: [ :copy |
	copy modified ifTrue: [ problems add: copy packageName , ' is modified' ] ].
((IceRepository registry detect: [ :each | each name = 'RTree' ] ifNone: [ nil ])
	ifNil: [ '' ] ifNotNil: [ :repository | [ repository origin url ] on: Error do: [ '' ] ]) asString
		in: [ :url | (url includesSubstring: 'massimo-nocentini/RTree') ifFalse: [ problems add: 'RTree is not the fork: ' , url ] ].

"(load.st gives Iceberg back the settings of the stock image)"
IceCredentialsProvider providerType = IceNonInteractiveCredentialsProvider ifTrue: [
	problems add: 'Iceberg keeps the non-interactive credentials of load.st' ].

"the icons of the UI, as load.st left them: the Material Symbols in gray (five keys that take the colour of the pack)"
((ThemeIcons classVarNamed: #Current) ifNil: [ nil ] ifNotNil: [ :pack | pack class name ]) = #MaterialSymbolsThemeIcons
	ifFalse: [ problems add: 'the icons of the UI are not MaterialSymbolsThemeIcons' ]
	ifTrue: [
		| pack gray keys |
		pack := ThemeIcons current.
		pack color = Color gray ifFalse: [ problems add: 'the Material Symbols are not gray: ' , pack color printString ].
		gray := (Color gray pixelValueForDepth: 32) bitAnd: 16rFFFFFF.
		keys := (((Smalltalk globals at: #MaterialSymbolsThemeIconsArt) entries select: [ :each | each fifth isNil ])
			collect: [ :each | each first ]) first: 5.
		keys do: [ :key |
			| form |
			form := pack iconNamed: key.
			form unhibernate.
			((form bits anySatisfy: [ :pixel | (pixel bitShift: -24) > 0 ]) and: [
				(form bits reject: [ :pixel | (pixel bitShift: -24) = 0 ]) allSatisfy: [ :pixel |
					(pixel bitAnd: 16rFFFFFF) = gray ] ]) ifFalse: [ problems add: 'the icon ' , key , ' is not gray' ] ] ].

"2. the facades, in Smalltalk on any VM"
(#( 3 1 2 ) copy timSort: [ :a :b | a <= b ]) = #( 1 2 3 ) ifFalse: [ problems add: 'timSort: does not sort' ].
#( 3 1 2 ) copy timSortDescending = #( 3 2 1 ) ifFalse: [ problems add: 'timSortDescending does not sort' ].
(2.0 times: 3.0 plus: 1.0) = 7.0 ifFalse: [ problems add: 'Float>>times:plus: is not 7.0' ].

"3. the tests"
suites := #( 'RTree-Tests' 'TimSort-Smalltalk-Tests' 'VM-Plugins-Primitives-Tests' 'CMark-Tests' 'CMark-Roassal-Tests'
	'Delimcc-Tests' 'Delimcc-Continuation-Tests' 'ContinuationTest' 'MaterialSymbols-Tests' ).
mode = 'native' ifTrue: [
	suites := suites , #( 'SimdJSON-Tests' 'SimdJSON-FFI-Tests' 'TimSort-FFI-Tests' 'LibGLib-Tests' 'LibPangoCairo-Tests'
		'LibPoppler-Roassal-Tests' ) ].
"(the groups of MaterialIcons that load.st was given may leave out its tests)"
optional := ((environment at: 'BATTERIES_MATERIALICONS_GROUPS' ifAbsent: [ '' ]) substrings: ' ')
	in: [ :groups | (groups isEmpty or: [ groups includes: 'SymbolsTests' ]) ifTrue: [ #() ] ifFalse: [ #( 'MaterialSymbols-Tests' ) ] ].
run := passed := skipped := failed := errors := 0.
suites do: [ :packageName |
	| package classes suite result ms |
	"(a package, or a class of tests)"
	package := PackageOrganizer default packageNamed: packageName ifAbsent: [ nil ].
	classes := package
		ifNil: [ (Smalltalk globals at: packageName asSymbol ifAbsent: [ nil ]) ifNotNil: [ :class | { class } ] ]
		ifNotNil: [ package definedClasses ].
	classes
		ifNil: [
			(optional includes: packageName)
				ifTrue: [ line value: 'Batteries smoke: ' , packageName , ' not loaded (BATTERIES_MATERIALICONS_GROUPS)' ]
				ifFalse: [ problems add: 'no package ' , packageName ] ]
		ifNotNil: [
			suite := TestSuite named: packageName.
			(classes select: [ :each | (each inheritsFrom: TestCase) and: [ each isAbstract not ] ])
				do: [ :each | suite addTests: each buildSuite tests ].
			ms := Time millisecondsToRun: [ result := suite run ].
			line value: 'Batteries smoke: ' , packageName , ' run ' , result runCount printString , ' passed '
				, result passedCount printString , ' skipped ' , result skippedCount printString , ' failed '
				, result failureCount printString , ' errors ' , result errorCount printString , ' ('
				, (ms // 1000) printString , ' s)'.
			result failures do: [ :each | problems add: 'failure ' , each printString ].
			result errors do: [ :each | problems add: 'error ' , each printString ].
			(mode = 'native' and: [ result runCount = 0 ]) ifTrue: [
				problems add: packageName , ' ran no test (' , result skippedCount printString , ' skipped)' ].
			run := run + result runCount.
			passed := passed + result passedCount.
			skipped := skipped + result skippedCount.
			failed := failed + result failureCount.
			errors := errors + result errorCount ] ] ]
	on: Error
	do: [ :error | problems add: 'error in smoke.st ' , error class name , ': ' , (error messageText ifNil: [ '' ]) asString ].

problems do: [ :each | line value: 'Batteries smoke: ' , each ].
ok := problems isEmpty.
line value: 'Batteries smoke ' , mode , ' classes ' , classes size printString , ' tests ' , run printString , ' passed '
	, passed printString , ' skipped ' , skipped printString , ' failed ' , failed printString , ' errors '
	, errors printString , (ok ifTrue: [ ' ok' ] ifFalse: [ ' FAILED' ]).
ok ifTrue: [ Smalltalk exitSuccess ] ifFalse: [ Smalltalk exitFailure ]
