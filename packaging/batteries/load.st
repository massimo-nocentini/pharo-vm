"load.st - load the batteries into a stock Pharo 15 image (docs/Batteries.md)

	pharo --headless X.image --no-default-preferences st --save --quit packaging/batteries/load.st

(build.sh runs it on a copy of the image, then smoke.st on the image saved.)  The projects, in this order:

	RoassalMN            github://massimo-nocentini/Roassal:Pharo15/src          Full   (Roassal and RTree of the forks)
	VMPluginsPrimitives  github://massimo-nocentini/vm-plugins-primitives.st:master/src  default
	SimdJSON             github://massimo-nocentini/simdjson.st:master/src       default (NeoJSON of svenvc too)
	SimdUTF              github://massimo-nocentini/simdutf.st:master/src        default
	SimdCSV              github://massimo-nocentini/simdcsv.st:master/src        default
	TimSort              github://massimo-nocentini/timsort.st:master/src        default
	LibPangoCairo        github://massimo-nocentini/libpangocairo.st:master/src  default
	LibPoppler           github://massimo-nocentini/libpoppler.st:master/src     default
	CMark                github://massimo-nocentini/cmark-gfm.st:master/src      default (CMark-Roassal on the fork)
	ContainersEssentials github://massimo-nocentini/Containers-Essentials:master default (Containers-Array2D of pharo-containers too)
	ContainersCategory   github://massimo-nocentini/Containers-Category:master/src default
	Delimcc              github://massimo-nocentini/delimcc.st:master            default (FileTree, at the root of the repository)
	MaterialIcons        github://massimo-nocentini/material-icons.st:master/src SymbolsRoundedSmall SymbolsThemePack SymbolsTests
	                     SymbolsProtocols SymbolsProtocolsTests,
	                     then MaterialSymbolsThemeIcons installColor: Color gray monochrome: true.

Roassal comes first: Pharo 15 ships Roassal and RTree, which the forks replace, and the Roassal packages of
LibPangoCairo and LibPoppler then build on the fork.  LibPangoCairo comes before LibPoppler, whose baseline
needs its GLib group: once loaded, every project is locked to the repository it came from, so that the
reference of BaselineOfLibPoppler to github://massimo-nocentini/libpangocairo.st:master/src is honoured from
there.  CMark comes next: its renderer uses only the Roassal API of the stock images, and draws on the fork here.
ContainersEssentials and ContainersCategory, which Delimcc needs, come before it for the same reason as LibPangoCairo:
BaselineOfContainersEssentials names RoassalMN, VMPluginsPrimitives, LibPangoCairo and LibPoppler by the repositories
above, which their locks hold (so a BATTERIES_<KEY> repository is the one they get, and Roassal is loaded once, by
BaselineOfRoassalMN: nothing loads BaselineOfRoassal).  MaterialIcons comes last, with the Material Symbols at 16, 24
and 32 pixels, their pack of Pharo's icons, the symbols of the common method protocols in the browsers
(MaterialProtocolSymbols) and their tests (not their 192-pixel masters, nor the Material Icons, nor the filled
symbols), and the expression after it makes the pack the icons of the UI, every key in gray,
the status keys of tests, errors and warnings too (monochrome): the image is saved with it.

Environment:
	BATTERIES_<KEY>          the repository of a project instead of the one above, any Metacello repository
	                         (github://..., gitlocal://..., tonel:///<checkout>/src for a local checkout); the keys
	                         are ROASSAL, VMPLUGINSPRIMITIVES, SIMDJSON, SIMDUTF, SIMDCSV, TIMSORT, LIBPANGOCAIRO, LIBPOPPLER,
	                         CMARK, CONTAINERSESSENTIALS, CONTAINERSCATEGORY, DELIMCC, MATERIALICONS
	BATTERIES_<KEY>_GROUPS   its groups, separated by blanks, instead of the ones above
	BATTERIES_ICEBERG_REMOTE the protocol of the clones of github:// repositories, httpsUrl by default (no SSH key
	                         needed) or scpUrl

Pharo ships BaselineOfRoassal and the Roassal and RTree packages, which Monticello counts as modified: Metacello
asks whether to load the new versions over them (MCMergeOrLoadWarning), and the answer is Load (a plain resume
would keep the methods of the image).  Any other warning is printed and resumed.  Any error stops the script with
exit status 1, before the image is saved.  After the load: the fork's RTree must be there (RTNaryTreeCollection,
and no RTree package left modified), the icons of the UI must be MaterialSymbolsThemeIcons, no load may leave a new
Undeclared variable, and one line per repository gives what was loaded:

	Batteries project <name> <repository> groups <groups>
	Batteries repository <name> <remote> <branch> <commit>     (the clones of Iceberg: github:// repositories)
	Batteries loaded <n> projects on Pharo <version> ok"

| environment out lineEnd line fail projects remoteType remoteTypeBefore providerBefore repositoriesBefore
  undeclaredBefore newUndeclared warnings loaded |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
line := [ :string | out nextPutAll: string; nextPutAll: lineEnd; flush ].
fail := [ :message |
	line value: 'Batteries FAILED: ' , message.
	Smalltalk exitFailure ].

"{ key. baseline. default repository. default groups }"
projects := {
	{ 'ROASSAL'. 'RoassalMN'. 'github://massimo-nocentini/Roassal:Pharo15/src'. #( 'Full' ) }.
	{ 'VMPLUGINSPRIMITIVES'. 'VMPluginsPrimitives'. 'github://massimo-nocentini/vm-plugins-primitives.st:master/src'. #( 'default' ) }.
	{ 'SIMDJSON'. 'SimdJSON'. 'github://massimo-nocentini/simdjson.st:master/src'. #( 'default' ) }.
	{ 'SIMDUTF'. 'SimdUTF'. 'github://massimo-nocentini/simdutf.st:master/src'. #( 'default' ) }.
	{ 'SIMDCSV'. 'SimdCSV'. 'github://massimo-nocentini/simdcsv.st:master/src'. #( 'default' ) }.
	{ 'TIMSORT'. 'TimSort'. 'github://massimo-nocentini/timsort.st:master/src'. #( 'default' ) }.
	{ 'LIBPANGOCAIRO'. 'LibPangoCairo'. 'github://massimo-nocentini/libpangocairo.st:master/src'. #( 'default' ) }.
	{ 'LIBPOPPLER'. 'LibPoppler'. 'github://massimo-nocentini/libpoppler.st:master/src'. #( 'default' ) }.
	{ 'CMARK'. 'CMark'. 'github://massimo-nocentini/cmark-gfm.st:master/src'. #( 'default' ) }.
	{ 'CONTAINERSESSENTIALS'. 'ContainersEssentials'. 'github://massimo-nocentini/Containers-Essentials:master'. #( 'default' ) }.
	{ 'CONTAINERSCATEGORY'. 'ContainersCategory'. 'github://massimo-nocentini/Containers-Category:master/src'. #( 'default' ) }.
	{ 'DELIMCC'. 'Delimcc'. 'github://massimo-nocentini/delimcc.st:master'. #( 'default' ) }.
	{ 'MATERIALICONS'. 'MaterialIcons'. 'github://massimo-nocentini/material-icons.st:master/src'.
		#( 'SymbolsRoundedSmall' 'SymbolsThemePack' 'SymbolsTests' 'SymbolsProtocols' 'SymbolsProtocolsTests' ). 'MaterialSymbolsThemeIcons installColor: Color gray monochrome: true.' } }.
"{ key. baseline. repository. groups. the expression evaluated after the load, or nil }"
projects := projects collect: [ :each |
	| key repository groups |
	key := each first.
	repository := (environment at: 'BATTERIES_' , key ifAbsent: [ '' ]) trimBoth ifEmpty: [ each third ].
	groups := ((environment at: 'BATTERIES_' , key , '_GROUPS' ifAbsent: [ '' ]) substrings: ' ') asArray
		ifEmpty: [ each fourth ].
	{ key. each second. repository. groups. each size >= 5 ifTrue: [ each fifth ] ifFalse: [ nil ] } ].

remoteType := ((environment at: 'BATTERIES_ICEBERG_REMOTE' ifAbsent: [ '' ]) trimBoth ifEmpty: [ 'httpsUrl' ]) asSymbol.
remoteTypeBefore := Iceberg remoteTypeSelector.
providerBefore := IceCredentialsProvider providerType.
Iceberg remoteTypeSelector: remoteType.
"(GitHub answers 401 for a repository that does not exist, or not yet: an error, not a prompt that waits forever)"
IceCredentialsProvider providerType: IceNonInteractiveCredentialsProvider.
line value: 'Batteries: ' , SystemVersion current printString , ', clones by ' , remoteType.

repositoriesBefore := IceRepository registry copy.
undeclaredBefore := Undeclared keys asSet.
warnings := 0.
[
projects do: [ :project |
	| started |
	started := Time millisecondClockValue.
	line value: 'Batteries: loading ' , project second , ' ' , project fourth printString , ' from ' , project third.
	[ Metacello new
		baseline: project second;
		repository: project third;
		onConflictUseIncoming;
		onUpgrade: [ :ex | ex useIncoming ];
		onLock: [ :ex | ex honor ];
		onWarning: [ :warning |
			(warning isKindOf: MCMergeOrLoadWarning)
				ifTrue: [ warning load ]
				ifFalse: [
					warnings := warnings + 1.
					line value: 'Batteries: warning ' , warning class name , ': ' , (warning messageText ifNil: [ '' ]) asString.
					warning resume ] ];
		load: project fourth ]
		on: Warning
		do: [ :warning |
			warnings := warnings + 1.
			line value: 'Batteries: warning ' , warning class name , ': ' , (warning messageText ifNil: [ '' ]) asString.
			warning resume ].
	"(a lock alone does not do: Metacello skips a locked project that is not loaded yet)"
	Metacello new
		baseline: project second;
		repository: project third;
		lock.
	"(compiled now: its classes are there only after the load)"
	project fifth ifNotNil: [ :expression |
		line value: 'Batteries: evaluating ' , expression.
		Smalltalk compiler evaluate: expression ].
	line value: 'Batteries: loaded ' , project second , ' in '
		, ((Time millisecondClockValue - started) // 1000) printString , ' s' ] ]
	on: Error
	do: [ :error |
		line value: 'Batteries FAILED: ' , error class name , ': ' , (error messageText ifNil: [ '' ]) asString.
		error signalerContext ifNotNil: [ :context |
			(context stack first: (context stack size min: 25)) do: [ :each | line value: '    ' , each printString ] ].
		Smalltalk exitFailure ].

"The fork's RTree: its classes, and no RTree package left modified (the stock methods kept)"
#( #RTNaryTreeCollection #RTPackedTreeCollection #RTGridCollection #RTLooseQuadtreeCollection ) do: [ :name |
	(Smalltalk globals includesKey: name) ifFalse: [ fail value: 'no ' , name , ': the RTree of the fork is not loaded' ] ].
(MCWorkingCopy allWorkingCopies select: [ :copy |
	(copy packageName beginsWith: 'RTree') or: [ copy packageName beginsWith: 'Roassal' ] ])
		do: [ :copy | copy modified ifTrue: [ fail value: copy packageName , ' is modified after the load' ] ].

"The icons of the UI: the Material Symbols, in gray (ThemeIcons current would load the default pack when none is)"
((ThemeIcons classVarNamed: #Current) ifNil: [ '' ] ifNotNil: [ :pack | pack class name ]) = #MaterialSymbolsThemeIcons
	ifFalse: [ fail value: 'the icons of the UI are not MaterialSymbolsThemeIcons' ].

"Three are known, and only printed: classes of Pharo 12 that Pharo 15 has no more, in the fork of Roassal as
 upstream (RSThemedMapBuilder>>#showMenu: names ClyCommandRegistry, RSUMLPackageCalypso
 class>>#classUmlTapActivation2 RPackage), and LibLua, of a binding of Lua that the batteries do not load
 (CTNotebookKeyedValuedOrderedCollection>>#liblua of Containers-Essentials-Outliner)"
newUndeclared := Undeclared keys asSet reject: [ :each | undeclaredBefore includes: each ].
newUndeclared do: [ :name |
	(SystemNavigation default allReferencesTo: (Undeclared associationAt: name)) do: [ :method |
		line value: 'Batteries: Undeclared ' , name , ' in ' , method printString ] ].
newUndeclared := newUndeclared reject: [ :each | #( #ClyCommandRegistry #RPackage #LibLua ) includes: each ].
newUndeclared ifNotEmpty: [
	fail value: 'new Undeclared variables ' , newUndeclared asSortedCollection asArray printString ].

"What was loaded"
projects do: [ :project |
	line value: 'Batteries project ' , project second , ' ' , project third , ' groups ' , (' ' join: project fourth) ].
(IceRepository registry reject: [ :each | repositoriesBefore includes: each ]) , (IceRepository registry select: [ :each |
	(repositoriesBefore includes: each) and: [ each location notNil and: [ each location exists ] ] ])
	do: [ :repository |
		| remote branch commit |
		remote := [ repository origin url ] on: Error do: [ '?' ].
		branch := [ repository branchName ] on: Error do: [ '?' ].
		commit := [ repository headCommit id ] on: Error do: [ '?' ].
		line value: 'Batteries repository ' , repository name , ' ' , remote asString , ' ' , branch asString , ' '
			, commit asString ].
loaded := projects size.
line value: 'Batteries loaded ' , loaded printString , ' projects on Pharo ' , SystemVersion current major printString , '.'
	, SystemVersion current minor printString , ' (' , warnings printString , ' warnings) ok'.

"The settings of Iceberg as the stock image has them"
Iceberg remoteTypeSelector: remoteTypeBefore.
IceCredentialsProvider providerType: providerBefore.
Smalltalk garbageCollect.
"the image is saved by st --save --quit"
