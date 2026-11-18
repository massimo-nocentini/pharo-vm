"load-cairo.st - load the Smalltalk side of the CairoPlugin with Metacello from the submodule
plugins/UtilsPlugin/vm-plugins-primitives.st (github.com/massimo-nocentini/vm-plugins-primitives.st),
whose BaselineOfVMPluginsPrimitives has the groups Cairo (VM-Plugins-Primitives-Cairo: the facade
CairoPlugin, its installer), CairoTests (VM-Plugins-Primitives-Cairo-Tests) and CairoBenchmarks
(VM-Plugins-Primitives-Cairo-Benchmarks); none of them is in its default group.  The submodule's
commit is the version of the Smalltalk code that goes with this plugin.

	CP_PLUGIN_DIR=<repository>/plugins/UtilsPlugin CP_GROUPS='Cairo CairoTests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-cairo.st

CP_PLUGIN_DIR  the directory of the UtilsPlugin, which holds the submodule (the st command of
               Pharo 15 takes nothing but its file, so the directories come in the environment)
CP_ST_REPO     the Tonel directory to load from instead of the submodule's src (a working copy
               of vm-plugins-primitives.st/src, while the Smalltalk code is being written)
CP_GROUPS      the groups to load, separated by spaces ('Cairo CairoTests' by default)

Loading the package VM-Plugins-Primitives-Cairo installs the patches of Athens when the VM has
the CairoPlugin (and never without it).  The script prints

	CairoPlugin: loaded, plugin <CairoPlugin info, or nil>, installed <the groups installed>

then the groups and packages loaded and what the installer reported.  It exits with status 1,
before the image is saved, when the repository is missing, a load fails, a method of the Cairo
packages uses an undeclared variable, a Cairo package defines a method of a class of Athens,
FreeType or Zinc (an extension: the installer patches Athens at run time, never by loading), or
CairoPlugin is not there.  Metacello makes UUIDs for the packages it loads: the VM needs its
UUIDPlugin."

| environment out fail lineEnd plugin repository groups packages methods undeclared extensions foreign cairo |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-cairo: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
repository := (environment at: 'CP_ST_REPO' ifAbsent: [ '' ]) isEmpty
	ifTrue: [
		plugin := (environment at: 'CP_PLUGIN_DIR' ifAbsent: [ nil ])
			ifNil: [ fail value: 'neither CP_PLUGIN_DIR nor CP_ST_REPO is set' ]
			ifNotNil: [ :path | path asFileReference ].
		plugin / 'vm-plugins-primitives.st' / 'src' ]
	ifFalse: [ (environment at: 'CP_ST_REPO') asFileReference ].
(repository / 'BaselineOfVMPluginsPrimitives') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfVMPluginsPrimitives') fullName
		, ': git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st' ].
(repository / 'VM-Plugins-Primitives-Cairo') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'VM-Plugins-Primitives-Cairo') fullName
		, ': the submodule is too old (bump plugins/UtilsPlugin/vm-plugins-primitives.st to the commit with the Cairo packages)' ].
groups := ((environment at: 'CP_GROUPS' ifAbsent: [ 'Cairo CairoTests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'CP_GROUPS names no group' ].

"The load resumes Metacello's warnings, after printing them"
[ [ Metacello new
		baseline: 'VMPluginsPrimitives';
		repository: 'tonel://' , repository fullName;
		load: groups ]
	on: Warning
	do: [ :warning |
		out nextPutAll: 'VMPluginsPrimitives: '; nextPutAll: warning messageText; nextPutAll: lineEnd; flush.
		warning resume ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'VM-Plugins-Primitives-Cairo' ])
	asSortedCollection asArray.
packages ifEmpty: [ fail value: 'no package VM-Plugins-Primitives-Cairo* was loaded' ].
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
extensions := packages flatCollect: [ :each | each asPackage extensionMethods ].
foreign := extensions select: [ :method |
	| name owner |
	name := method methodClass instanceSide name.
	owner := method methodClass instanceSide package name.
	(#( 'Athens' 'Cairo' 'FreeType' 'Zn' ) anySatisfy: [ :prefix | name beginsWith: prefix ])
		or: [ #( 'Athens' 'FreeType' 'Zinc' ) anySatisfy: [ :prefix | owner beginsWith: prefix ] ] ].
foreign ifNotEmpty: [
	fail value: 'extension methods on Athens, FreeType or Zinc classes: ' , (foreign collect: [ :method | method printString ]) asArray printString ].
cairo := Smalltalk globals at: #CairoPlugin ifAbsent: [ fail value: 'no CairoPlugin after the load' ].

out
	nextPutAll: 'CairoPlugin: loaded, plugin '; nextPutAll: cairo info printString;
	nextPutAll: ', installed '; nextPutAll: cairo installedGroups printString; nextPutAll: lineEnd;
	nextPutAll: 'CairoPlugin: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	nextPutAll: 'CairoPlugin: wanted '; nextPutAll: cairo wantedGroups printString;
	nextPutAll: ', report '; nextPutAll: cairo lastReport printString; nextPutAll: lineEnd;
	flush.
!
