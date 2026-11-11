"load-simdjson.st - load SimdJSON with Metacello from the submodule
plugins/SimdJSONPlugin/simdjson.st (github.com/massimo-nocentini/simdjson.st), whose
BaselineOfSimdJSON has the groups Core (SimdJSON), Tests (SimdJSON-Tests), NeoTests
(SimdJSON-NeoTests, with NeoJSON and its tests; loading it generates the SJ<name>
classes in SimdJSON-NeoTests-Generated) and default (all of them).  The submodule's
commit is the version of the Smalltalk code that goes with this plugin.

	SJ_PLUGIN_DIR=<repository>/plugins/SimdJSONPlugin SJ_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-simdjson.st

SJ_PLUGIN_DIR  the directory of the plugin (the st command of Pharo 15 takes nothing but
               its file, so the directories come in the environment)
SJ_GROUPS      the groups to load, separated by spaces ('Core Tests' by default)
NEOJSON_DIR    a clone of https://github.com/svenvc/NeoJSON (tested with 11e274d): when
               it is set, NeoJSON's groups core and tests are loaded from it first and
               locked, so that nothing comes from the network; else a group that needs
               NeoJSON (NeoTests, default) loads it from GitHub, as the baseline says

It prints 'SimdJSON: SimdJSONReader loaded, a subclass of SimdJSONMapper' and the
packages loaded, and exits with status 1, before the image is saved, when the submodule
is missing, a load fails, or a method of a SimdJSON package uses an undeclared variable.
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail plugin repository groups neoDir load reader packages undeclared |
environment := OSEnvironment current.
out := Stdio stdout.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-simdjson: '; nextPutAll: message; nextPutAll: OSPlatform current lineEnding; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'SJ_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'SJ_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
repository := plugin / 'simdjson.st' / 'src'.
(repository / 'BaselineOfSimdJSON') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfSimdJSON') fullName
		, ': git submodule update --init plugins/SimdJSONPlugin/simdjson.st' ].
groups := ((environment at: 'SJ_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'SJ_GROUPS names no group' ].
"Each load resumes Metacello's warnings, after printing them"
load := [ :what :block |
	[ block value ]
		on: Warning
		do: [ :warning |
			out nextPutAll: what; nextPutAll: ': '; nextPutAll: warning messageText;
				nextPutAll: OSPlatform current lineEnding; flush.
			warning resume ] ].

neoDir := environment at: 'NEOJSON_DIR' ifAbsent: [ nil ].
neoDir ifNotNil: [
	| neoRepository |
	neoRepository := neoDir asFileReference / 'repository'.
	neoRepository isDirectory ifFalse: [ fail value: 'no repository directory in ' , neoDir ].
	"(a lock alone would not load it: Metacello skips a locked project that is not loaded)"
	load value: 'NeoJSON' value: [
		Metacello new
			baseline: 'NeoJSON';
			repository: 'tonel://' , neoRepository fullName;
			load: #( 'core' 'tests' ) ].
	Metacello new
		baseline: 'NeoJSON';
		repository: 'tonel://' , neoRepository fullName;
		lock ].

[ load value: 'SimdJSON' value: [
	Metacello new
		baseline: 'SimdJSON';
		repository: 'tonel://' , repository fullName;
		load: groups ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

reader := Smalltalk globals at: #SimdJSONReader ifAbsent: [ nil ].
(reader notNil and: [ reader superclass name == #SimdJSONMapper ]) ifFalse: [
	fail value: 'SimdJSONReader is not a subclass of SimdJSONMapper' ].
packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'SimdJSON' ])
	asSortedCollection asArray.
undeclared := (packages flatCollect: [ :each | each asPackage methods ])
	select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in '
		, (undeclared collect: [ :method | method printString ]) asArray printString ].
out
	nextPutAll: 'SimdJSON: SimdJSONReader loaded, a subclass of SimdJSONMapper'; nextPutAll: OSPlatform current lineEnding;
	nextPutAll: 'SimdJSON: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: OSPlatform current lineEnding;
	flush.
!
