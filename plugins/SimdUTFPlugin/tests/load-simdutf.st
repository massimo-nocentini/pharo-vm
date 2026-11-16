"load-simdutf.st - load SimdUTF with Metacello from the submodule
plugins/SimdUTFPlugin/simdutf.st (github.com/massimo-nocentini/simdutf.st), whose
BaselineOfSimdUTF has the groups Core (SimdUTF), Tests (SimdUTF-Tests), FFI (SimdUTF-FFI:
libSimdUTFFFI through the FFI, for a VM without the plugin), FFITests (SimdUTF-FFI-Tests) and
default (all of them).  The submodule's commit is the version of the Smalltalk code that goes
with this plugin.

	SU_PLUGIN_DIR=<repository>/plugins/SimdUTFPlugin SU_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-simdutf.st

SU_PLUGIN_DIR  the directory of the plugin (the st command of Pharo 15 takes nothing but
               its file, so the directories come in the environment)
SU_GROUPS      the groups to load, separated by spaces ('Core Tests' by default)

It prints 'SimdUTF: loaded, protocol 1, backend #<b>, implementation <k>' and the groups and
packages loaded, and exits with status 1, before the image is saved, when the submodule is
missing, a load fails, SimdUTF protocol is not 1, the pure-Smalltalk backend does not encode
'hé', a method of a SimdUTF package uses an undeclared variable, a SimdUTF package defines a
method on a class whose name begins with Zn, or an extension method of a SimdUTF package (on a
class of no SimdUTF package) has a selector that does not begin with simd.
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail lineEnd plugin repository groups load packages methods ownClasses undeclared zinc extensions facade encoded |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-simdutf: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'SU_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'SU_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
repository := plugin / 'simdutf.st' / 'src'.
(repository / 'BaselineOfSimdUTF') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfSimdUTF') fullName
		, ': git submodule update --init plugins/SimdUTFPlugin/simdutf.st' ].
groups := ((environment at: 'SU_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'SU_GROUPS names no group' ].
"The load resumes Metacello's warnings, after printing them"
load := [ :what :block |
	[ block value ]
		on: Warning
		do: [ :warning |
			out nextPutAll: what; nextPutAll: ': '; nextPutAll: warning messageText; nextPutAll: lineEnd; flush.
			warning resume ] ].

[ load value: 'SimdUTF' value: [
	Metacello new
		baseline: 'SimdUTF';
		repository: 'tonel://' , repository fullName;
		load: groups ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

facade := Smalltalk globals at: #SimdUTF ifAbsent: [ nil ].
facade ifNil: [ fail value: 'SimdUTF is not loaded' ].
facade protocol = 1 ifFalse: [ fail value: 'SimdUTF protocol is ' , facade protocol printString , ', not 1' ].
encoded := [ (Smalltalk globals at: #SimdUTFSmalltalk) encode: 'hé' from: 1 to: 2 flags: 0 into: nil at: 1 ]
	on: Error
	do: [ :error | error description ].
encoded = #[104 195 169] ifFalse: [
	fail value: 'SimdUTFSmalltalk encodes ''hé'' as ' , encoded printString , ', not #[104 195 169]' ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'SimdUTF' ])
	asSortedCollection asArray.
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
"Opt-in only: nothing on Zinc's classes, and every extension selector begins with simd"
zinc := methods select: [ :method | method methodClass instanceSide name beginsWith: 'Zn' ].
zinc ifNotEmpty: [
	fail value: 'methods on Zn classes: ' , (zinc collect: [ :method | method printString ]) asArray printString ].
ownClasses := (packages flatCollect: [ :each | each asPackage definedClasses ]) asIdentitySet.
extensions := (packages flatCollect: [ :each | each asPackage extensionMethods ]) reject: [ :method |
	(ownClasses includes: method methodClass instanceSide) or: [ method selector beginsWith: 'simd' ] ].
extensions ifNotEmpty: [
	fail value: 'extension selectors without simd: ' , (extensions collect: [ :method | method printString ]) asArray printString ].

out
	nextPutAll: 'SimdUTF: loaded, protocol '; nextPutAll: facade protocol printString;
	nextPutAll: ', backend '; nextPutAll: facade backend printString;
	nextPutAll: ', implementation '; nextPutAll: facade implementationName; nextPutAll: lineEnd;
	nextPutAll: 'SimdUTF: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	flush.
!
