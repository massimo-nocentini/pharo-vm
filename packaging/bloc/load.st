"load.st - load Bloc and Toplo into a stock Pharo 15 image (docs/Bloc.md)

	BATTERIES_HOME=packaging/bloc \
	pharo --headless X.image --no-default-preferences st --save --quit packaging/bloc/load.st

(build.sh runs it on a copy of the image, then smoke.st on the image saved.)  The project:

	Toplo  github://pharo-graphics/Toplo:v0.12.0/src  default

Toplo's baseline loads Bloc v3.1.0 (with Alexandrie v2.11.0, its canvas on cairo, FreeType and HarfBuzz, and
XMLParser) and Album v0.12.0, its text editor: their releases, so that the image is made of the same code on
every build.  The load runs with Epicea off, as Toplo's install page does.

After the project, the two packages of src/ are installed:

	PharoBloc-Web      the buffers of a space of Bloc in a Morphic host on a VM without SurfacePlugin, the WebAssembly
	                   VM (PharoBlocMorphicForm: a Form with pinned bits that cairo draws into, instead of a manual
	                   surface); it replaces BlAbstractMorphicHostSpace>>#newExternalForm, which keeps Bloc's form
	                   where SurfacePlugin is there
	PharoBloc-Welcome  the window that the image shows the first time that it is opened with a display, itself a
	                   space of Bloc with Toplo's widgets (PharoBlocWelcome)

Environment:
	BATTERIES_HOME           the directory of this script, which the script cannot find by itself
	BLOC_TOPLO               the repository of Toplo instead of the one above, any Metacello repository
	                         (github://pharo-graphics/Toplo:dev/src, tonel:///<checkout>/src for a local checkout)
	BLOC_TOPLO_GROUPS        its groups, separated by blanks, instead of default
	BATTERIES_ICEBERG_REMOTE the protocol of the clones of github:// repositories, httpsUrl by default (no SSH key
	                         needed) or scpUrl

Any warning is printed and resumed.  Any error stops the script with exit status 1, before the image is saved.
After the load: Bloc, Alexandrie and Toplo must be there, no load may leave a new Undeclared variable but the
one known, and one line per repository gives what was loaded:

	Bloc project Toplo <repository> groups <groups>
	Bloc repository <name> <remote> <branch> <commit>     (the clones of Iceberg: github:// repositories)
	Bloc loaded Toplo on Pharo <version> ok"

| environment out lineEnd line fail home repository groups remoteType remoteTypeBefore providerBefore
  repositoriesBefore undeclaredBefore newUndeclared warnings started |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
line := [ :string | out nextPutAll: string; nextPutAll: lineEnd; flush ].
fail := [ :message |
	line value: 'Bloc FAILED: ' , message.
	Smalltalk exitFailure ].

home := ((environment at: 'BATTERIES_HOME' ifAbsent: [ '' ]) trimBoth
	ifEmpty: [ fail value: 'BATTERIES_HOME, the directory of load.st, is not set' ]) asFileReference / 'src'.
#( 'PharoBloc-Web' 'PharoBloc-Welcome' ) do: [ :name |
	(home / name) exists ifFalse: [ fail value: 'no ' , name , ' in ' , home pathString ] ].

repository := (environment at: 'BLOC_TOPLO' ifAbsent: [ '' ]) trimBoth
	ifEmpty: [ 'github://pharo-graphics/Toplo:v0.12.0/src' ].
groups := ((environment at: 'BLOC_TOPLO_GROUPS' ifAbsent: [ '' ]) substrings: ' ') asArray
	ifEmpty: [ #( 'default' ) ].

remoteType := ((environment at: 'BATTERIES_ICEBERG_REMOTE' ifAbsent: [ '' ]) trimBoth ifEmpty: [ 'httpsUrl' ]) asSymbol.
remoteTypeBefore := Iceberg remoteTypeSelector.
providerBefore := IceCredentialsProvider providerType.
Iceberg remoteTypeSelector: remoteType.
"(GitHub answers 401 for a repository that does not exist, or not yet: an error, not a prompt that waits forever)"
IceCredentialsProvider providerType: IceNonInteractiveCredentialsProvider.
line value: 'Bloc: ' , SystemVersion current printString , ', clones by ' , remoteType.

repositoriesBefore := IceRepository registry copy.
undeclaredBefore := Undeclared keys asSet.
warnings := 0.
started := Time millisecondClockValue.
line value: 'Bloc: loading Toplo ' , groups printString , ' from ' , repository.
[
EpMonitor disableDuring: [
	[ Metacello new
		baseline: 'Toplo';
		repository: repository;
		onConflictUseIncoming;
		onUpgrade: [ :ex | ex useIncoming ];
		onWarning: [ :warning |
			warnings := warnings + 1.
			line value: 'Bloc: warning ' , warning class name , ': ' , (warning messageText ifNil: [ '' ]) asString.
			warning resume ];
		load: groups ]
		on: Warning
		do: [ :warning |
			warnings := warnings + 1.
			line value: 'Bloc: warning ' , warning class name , ': ' , (warning messageText ifNil: [ '' ]) asString.
			warning resume ] ].
line value: 'Bloc: loaded Toplo in ' , ((Time millisecondClockValue - started) // 1000) printString , ' s' ]
	on: Error
	do: [ :error |
		line value: 'Bloc FAILED: ' , error class name , ': ' , (error messageText ifNil: [ '' ]) asString.
		error signalerContext ifNotNil: [ :context |
			(context stack first: (context stack size min: 25)) do: [ :each | line value: '    ' , each printString ] ].
		Smalltalk exitFailure ].

"The packages of src/ (snapshots, as prepare-web-image.st installs OSWindow-Web), and the window of the first opening"
#( 'PharoBloc-Web' 'PharoBloc-Welcome' ) do: [ :name |
	[ (TonelReader on: home fileName: name) snapshot install.
	line value: 'Bloc: installed ' , name ]
		on: Error
		do: [ :error | fail value: name , ': ' , error class name , ': ' , (error messageText ifNil: [ '' ]) asString ] ].
(Smalltalk at: #PharoBlocWelcome) install.

#( #BlElement #BlSpace #AeCanvas #AeCairoLibrary #AeHarfbuzzLibrary #ToButton #ToLabel #AlbEditorElement #PharoBlocMorphicForm
	#PharoBlocWelcome )
	do: [ :name |
		(Smalltalk globals includesKey: name) ifFalse: [ fail value: 'no ' , name , ' after the load' ] ].

"One is known, and only printed: Album v0.12.0 names BlLazyElement, which Bloc v3.1.0 has no more
 (AlbTextAdornmentAttributeTaskItRenderingStrategy>>#affect:in:, which nothing of the image calls)"
newUndeclared := Undeclared keys asSet reject: [ :each | undeclaredBefore includes: each ].
newUndeclared do: [ :name |
	(SystemNavigation default allReferencesTo: (Undeclared associationAt: name)) do: [ :method |
		line value: 'Bloc: Undeclared ' , name , ' in ' , method printString ] ].
((Smalltalk globals at: #BlAbstractMorphicHostSpace) >> #newExternalForm) package name = 'PharoBloc-Web'
	ifFalse: [ fail value: 'BlAbstractMorphicHostSpace>>#newExternalForm is not the one of PharoBloc-Web' ].
newUndeclared := newUndeclared reject: [ :each | #( #BlLazyElement ) includes: each ].
newUndeclared ifNotEmpty: [
	fail value: 'new Undeclared variables ' , newUndeclared asSortedCollection asArray printString ].

"What was loaded"
line value: 'Bloc project Toplo ' , repository , ' groups ' , (' ' join: groups).
(IceRepository registry reject: [ :each | repositoriesBefore includes: each ]) do: [ :each |
	| remote branch commit |
	remote := [ each origin url ] on: Error do: [ '?' ].
	branch := [ each branchName ] on: Error do: [ '?' ].
	commit := [ each headCommit id ] on: Error do: [ '?' ].
	line value: 'Bloc repository ' , each name , ' ' , remote asString , ' ' , branch asString , ' ' , commit asString ].
line value: 'Bloc loaded Toplo on Pharo ' , SystemVersion current major printString , '.'
	, SystemVersion current minor printString , ' (' , warnings printString , ' warnings) ok'.

"The settings of Iceberg as the stock image has them"
Iceberg remoteTypeSelector: remoteTypeBefore.
IceCredentialsProvider providerType: providerBefore.
Smalltalk garbageCollect.
"the image is saved by st --save --quit"
