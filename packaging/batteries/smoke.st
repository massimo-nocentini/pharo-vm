"smoke.st - a quick check of an image that load.st saved (docs/Batteries.md)

	[BATTERIES_SMOKE=native] pharo --headless Pharo15-batteries.image --no-default-preferences st --no-source packaging/batteries/smoke.st

The image starts again (so the save of load.st left nothing running), has the classes of every project and the RTree
of the fork, the page of the first opening still to show (PharoBatteriesWelcome), the icons of its UI are the Material Symbols in gray, its facades answer in Smalltalk, and the tests of a
few packages pass:

	basic (any VM, the default): RTree-Tests, TimSort-Smalltalk-Tests, VM-Plugins-Primitives-Tests (the
	        UtilsPlugin's Smalltalk twin, and the plugin too when the VM has it), SimdCSV-Tests (its Smalltalk
	        backend, and the plugin too when the VM has it; its slow tests skip), WebFetch-Tests and
	        WebFetch-Zinc-Tests (on a host made by the tests: those of a real fetch skip), CMark-Tests, CMark-Roassal-Tests
	        and CMark-Calypso-Tests (whose tests that parse skip without the CMarkPlugin), Delimcc-Tests,
	        Delimcc-Continuation-Tests and ContinuationTest (Pharo's, with the fixes of Delimcc-Continuation),
	        MaterialSymbols-Tests (which put the gray pack back after each test) and
	        MaterialSymbols-Protocols-Tests (the protocol symbols, in headless Calypso browsers): CMark-Calypso-Tests
	        skipped when BATTERIES_CMARK_GROUPS, as for load.st, has not its group (CalypsoTests), and the last two
	        when BATTERIES_MATERIALICONS_GROUPS has not theirs (SymbolsTests, SymbolsProtocolsTests), and the
	        package is not there;
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
	#RTLooseQuadtreeCollection #UtilsPlugin #KnuthRandom #SplitMix64 #SimdJSONReader #NeoJSONReader #SimdUTF #SimdCSV
	#SimdCSVReader #TimSort #TimSortSmalltalk #WebFetch #WebFetchZinc #PangoLayout #PopplerDocument #BaselineOfRoassalMN #CMark #CMarkParser
	#CMarkRoassalRenderer #CTArray2D #CTKeyedValuedOrderedCollection #CTCategory #PartialContinuation #DelimccPrompt
	#MaterialIcons #MaterialSymbolsThemeIcons #PharoBatteriesWelcome ).
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

"the page of the first opening: still to show (this session has no display), it names every project and draws"
(Smalltalk at: #PharoBatteriesWelcome ifAbsent: [ nil ]) ifNotNil: [ :welcome |
	| canvas |
	welcome isPending ifFalse: [ problems add: 'the welcome page is not pending' ].
	#( 'Roassal' 'RTree' 'RoassalLayouts' 'cmark-gfm.st' 'libpangocairo.st' 'libpoppler.st' 'material-icons.st' 'simdjson.st'
		'simdcsv.st' 'simdutf.st' 'timsort.st' 'web-fetch.st' 'vm-plugins-primitives.st' 'delimcc.st' 'Containers-Essentials'
		'Containers-Category' ) do: [ :name |
			(welcome contents includesSubstring: 'github.com/massimo-nocentini/' , name , ')') ifFalse: [
				problems add: 'the welcome page does not name ' , name ] ].
	canvas := RSCanvas new extent: 760 @ 620; yourself.
	[ welcome new drawOn: canvas ] on: Error do: [ :error | problems add: 'the welcome page: ' , error description ].
	(canvas shapes isEmpty or: [ (CMark isAvailable and: [ canvas shapes first allChildren size < 200 ]) ])
		ifTrue: [ problems add: 'the welcome page is empty' ] ].

"no class of the session but ExternalObject runs its startUp:, which nulls every external address of the image: run
 again after the world is drawn, it breaks what cairo made for it (an image saved with a Roassal canvas open)"
(Smalltalk at: #ExternalObject) in: [ :externalObject |
	externalObject withAllSubclassesDo: [ :each |
		((each ~~ externalObject and: [ SessionManager default hasRegistered: each name ]) and: [
			 (each class lookupSelector: #startUp:) methodClass == externalObject class ]) ifTrue: [
				problems add: each name , ' inherits startUp: of ExternalObject' ] ] ].

"the icons of the UI, as load.st left them: the Material Symbols in gray, monochrome (five keys that take the colour of
 the pack, and five status keys, which have fixed colours unless the pack is monochrome)"
((ThemeIcons classVarNamed: #Current) ifNil: [ nil ] ifNotNil: [ :pack | pack class name ]) = #MaterialSymbolsThemeIcons
	ifFalse: [ problems add: 'the icons of the UI are not MaterialSymbolsThemeIcons' ]
	ifTrue: [
		| pack gray keys |
		pack := ThemeIcons current.
		pack color = Color gray ifFalse: [ problems add: 'the Material Symbols are not gray: ' , pack color printString ].
		pack monochrome ifFalse: [ problems add: 'the Material Symbols are not monochrome' ].
		gray := (Color gray pixelValueForDepth: 32) bitAnd: 16rFFFFFF.
		keys := #( true false ) flatCollect: [ :plain |
			(((Smalltalk globals at: #MaterialSymbolsThemeIconsArt) entries select: [ :each | each fifth isNil = plain ])
				collect: [ :each | each first ]) first: 5 ].
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
((Smalltalk globals at: #SimdCSV) rowsFrom: 'a,b' , String lf , '1,"x,y"') = #( #( 'a' 'b' ) #( '1' 'x,y' ) ) ifFalse: [
	problems add: 'SimdCSV rowsFrom: does not read' ].

"(the Zinc adapter is loaded, not installed)"
(ZnNetworkingUtils default class name beginsWith: 'WebFetch') ifTrue: [ problems add: 'the Zinc adapter of WebFetch is installed' ].

"3. the tests"
suites := #( 'RTree-Tests' 'TimSort-Smalltalk-Tests' 'VM-Plugins-Primitives-Tests' 'SimdCSV-Tests' 'WebFetch-Tests'
	'WebFetch-Zinc-Tests' 'CMark-Tests' 'CMark-Roassal-Tests' 'CMark-Calypso-Tests' 'Delimcc-Tests' 'Delimcc-Continuation-Tests' 'ContinuationTest'
	'MaterialSymbols-Tests' 'MaterialSymbols-Protocols-Tests' ).
mode = 'native' ifTrue: [
	suites := suites , #( 'SimdJSON-Tests' 'SimdJSON-FFI-Tests' 'TimSort-FFI-Tests' 'LibGLib-Tests' 'LibPangoCairo-Tests'
		'LibPoppler-Roassal-Tests' ) ].
"(the groups of CMark and MaterialIcons that load.st was given may leave out their tests: the package of tests, and
 the variable whose groups lack its group)"
optional := Dictionary new.
#( #( 'BATTERIES_CMARK_GROUPS' 'CalypsoTests' 'CMark-Calypso-Tests' )
	#( 'BATTERIES_MATERIALICONS_GROUPS' 'SymbolsTests' 'MaterialSymbols-Tests' )
	#( 'BATTERIES_MATERIALICONS_GROUPS' 'SymbolsProtocolsTests' 'MaterialSymbols-Protocols-Tests' ) ) do: [ :each |
		| groups |
		groups := (environment at: each first ifAbsent: [ '' ]) substrings: ' '.
		(groups notEmpty and: [ (groups includes: each second) not ]) ifTrue: [ optional at: each third put: each first ] ].
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
			(optional includesKey: packageName)
				ifTrue: [ line value: 'Batteries smoke: ' , packageName , ' not loaded (' , (optional at: packageName) , ')' ]
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
