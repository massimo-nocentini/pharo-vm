"load-timsort.st - load TimSort with Metacello from the submodule
plugins/TimSortPlugin/timsort.st (github.com/massimo-nocentini/timsort.st), whose
BaselineOfTimSort has the groups Core (TimSort and TimSort-Smalltalk, the port of timsort.c),
Tests (TimSort-Tests), FFI (TimSort-FFI: libTimSortFFI through the FFI, for a VM without the
plugin), FFITests (TimSort-FFI-Tests), Smalltalk (TimSort-Smalltalk alone), SmalltalkTests
(TimSort-Smalltalk-Tests) and default (all of them).  The submodule's commit is the version of
the Smalltalk code that goes with this plugin.

	TS_PLUGIN_DIR=<repository>/plugins/TimSortPlugin TS_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-timsort.st

TS_PLUGIN_DIR  the directory of the plugin (the st command of Pharo 15 takes nothing but
               its file, so the directories come in the environment)
TS_GROUPS      the groups to load, separated by spaces ('Core Tests' by default)

It prints 'TimSort: loaded, protocol 1, backend #<b>, blockSortBackend #smalltalk,
resumableBackend <#plugin, #ffi or nil>, plugin <info>',
then, when TimSort-FFI is loaded, 'TimSort: library <true or false> <coroutine switch>', and
the groups and packages loaded.  It exits with status 1, before the image is saved, when the
submodule is missing, a load fails, TimSort protocol is not 1, the port TimSortSmalltalk or the
facade do not sort #(3 1 2), a method of a TimSort package uses an undeclared variable, a
TimSort package defines a method on a class whose name begins with Zn, or an extension method
of a TimSort package (on a class of no TimSort package) has a selector that does not begin with
timSort: nothing of Pharo's own sort is replaced.
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail lineEnd plugin repository groups load packages methods ownClasses undeclared zinc extensions facade ported sorted library |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-timsort: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'TS_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'TS_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
repository := plugin / 'timsort.st' / 'src'.
(repository / 'BaselineOfTimSort') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfTimSort') fullName
		, ': git submodule update --init plugins/TimSortPlugin/timsort.st' ].
groups := ((environment at: 'TS_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'TS_GROUPS names no group' ].
"The load resumes Metacello's warnings, after printing them"
load := [ :what :block |
	[ block value ]
		on: Warning
		do: [ :warning |
			out nextPutAll: what; nextPutAll: ': '; nextPutAll: warning messageText; nextPutAll: lineEnd; flush.
			warning resume ] ].

[ load value: 'TimSort' value: [
	Metacello new
		baseline: 'TimSort';
		repository: 'tonel://' , repository fullName;
		load: groups ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

"Every group loads TimSort-Smalltalk (Core, Smalltalk, SmalltalkTests) or TimSort with it"
ported := [ (Smalltalk globals at: #TimSortSmalltalk) sort: (Array with: 3 with: 1 with: 2) ]
	on: Error
	do: [ :error | error description ].
ported = #( 1 2 3 ) ifFalse: [ fail value: 'TimSortSmalltalk sorts #(3 1 2) as ' , ported printString ].
facade := Smalltalk globals at: #TimSort ifAbsent: [ nil ].
facade ifNotNil: [
	facade protocol = 1 ifFalse: [ fail value: 'TimSort protocol is ' , facade protocol printString , ', not 1' ].
	sorted := [ facade sort: (Array with: 3 with: 1 with: 2) ]
		on: Error
		do: [ :error | error description ].
	sorted = #( 1 2 3 ) ifFalse: [ fail value: 'TimSort sorts #(3 1 2) as ' , sorted printString ] ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'TimSort' ])
	asSortedCollection asArray.
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
"Opt-in only: nothing on Zinc's classes, and every extension selector begins with timSort"
zinc := methods select: [ :method | method methodClass instanceSide name beginsWith: 'Zn' ].
zinc ifNotEmpty: [
	fail value: 'methods on Zn classes: ' , (zinc collect: [ :method | method printString ]) asArray printString ].
ownClasses := (packages flatCollect: [ :each | each asPackage definedClasses ]) asIdentitySet.
extensions := (packages flatCollect: [ :each | each asPackage extensionMethods ]) reject: [ :method |
	(ownClasses includes: method methodClass instanceSide) or: [ method selector beginsWith: 'timSort' ] ].
extensions ifNotEmpty: [
	fail value: 'extension selectors without timSort: ' , (extensions collect: [ :method | method printString ]) asArray printString ].

facade ifNotNil: [
	out
		nextPutAll: 'TimSort: loaded, protocol '; nextPutAll: facade protocol printString;
		nextPutAll: ', backend '; nextPutAll: facade backend printString;
		nextPutAll: ', blockSortBackend '; nextPutAll: facade blockSortBackend printString;
		nextPutAll: ', resumableBackend '; nextPutAll: facade resumableBackend printString;
		nextPutAll: ', plugin '; nextPutAll: facade pluginInfo printString; nextPutAll: lineEnd ].
library := Smalltalk globals at: #TimSortLibrary ifAbsent: [ nil ].
library ifNotNil: [
	out
		nextPutAll: 'TimSort: library '; nextPutAll: library isAvailable printString;
		nextPutAll: ' '; nextPutAll: (library isAvailable ifTrue: [ library coroutineImplementation ] ifFalse: [ '-' ]);
		nextPutAll: lineEnd ].
out
	nextPutAll: 'TimSort: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	flush.
!
