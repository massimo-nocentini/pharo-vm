"load-utils.st - load the package VM-Plugins-Primitives with Metacello from the submodule
plugins/UtilsPlugin/vm-plugins-primitives.st (github.com/massimo-nocentini/vm-plugins-primitives.st),
whose BaselineOfVMPluginsPrimitives has the groups Core (VM-Plugins-Primitives), Tests
(VM-Plugins-Primitives-Tests) and default (both).  The submodule's commit is the version of the
Smalltalk code that goes with this plugin.

	UP_PLUGIN_DIR=<repository>/plugins/UtilsPlugin UP_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-utils.st

UP_PLUGIN_DIR  the directory of the plugin (the st command of Pharo 15 takes nothing but its
               file, so the directories come in the environment)
UP_GROUPS      the groups to load, separated by spaces ('Core Tests' by default)

It prints 'VMPluginsPrimitives: loaded, plugin <UtilsPlugin info, or nil>', then the groups and
packages loaded.  It exits with status 1, before the image is saved, when the submodule is
missing, a load fails, a method of the packages uses an undeclared variable, or one of the five
features does not answer its known value: 2.0 times: 3 plus: 1, 'a b' strtok: ' ' inclusive:
false, the middle of a segment, the first fraction of (KnuthRandom seed: 310952), and the first
output of (SplitMix64 seed: 1234567).
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail lineEnd plugin repository groups load packages methods undeclared checks utils |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-utils: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'UP_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'UP_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
repository := plugin / 'vm-plugins-primitives.st' / 'src'.
(repository / 'BaselineOfVMPluginsPrimitives') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfVMPluginsPrimitives') fullName
		, ': git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st' ].
groups := ((environment at: 'UP_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'UP_GROUPS names no group' ].
"The load resumes Metacello's warnings, after printing them"
load := [ :what :block |
	[ block value ]
		on: Warning
		do: [ :warning |
			out nextPutAll: what; nextPutAll: ': '; nextPutAll: warning messageText; nextPutAll: lineEnd; flush.
			warning resume ] ].

[ load value: 'VMPluginsPrimitives' value: [
	Metacello new
		baseline: 'VMPluginsPrimitives';
		repository: 'tonel://' , repository fullName;
		load: groups ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'VM-Plugins-Primitives' ])
	asSortedCollection asArray.
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].

utils := Smalltalk globals at: #UtilsPlugin.
checks := {
	'fma' -> [ 2.0 times: 3 plus: 1 ] -> 7.0.
	'strtok' -> [ 'a b' strtok: ' ' inclusive: false ] -> #( 'a' 'b' ).
	'de Casteljau' -> [ { 0 @ 0. 10 @ 20 } bezierPointAt: 0.5 ] -> (5.0 @ 10.0).
	'Knuth' -> [ ((Smalltalk globals at: #KnuthRandom) seed: 310952) next ] -> (Float fromIEEE64Bit: 16r3FC4F4EEA4AAF860).
	'SplitMix64' -> [ ((Smalltalk globals at: #SplitMix64) seed: 1234567) nextInteger64 ] -> 16r599ED017FB08FC85 }.
checks do: [ :check |
	| answer |
	answer := [ check key value value ] on: Error do: [ :error | error description ].
	answer = check value ifFalse: [
		fail value: check key key , ' answers ' , answer printString , ', not ' , check value printString ] ].

out
	nextPutAll: 'VMPluginsPrimitives: loaded, plugin '; nextPutAll: utils info printString; nextPutAll: lineEnd;
	nextPutAll: 'VMPluginsPrimitives: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	flush.
!
