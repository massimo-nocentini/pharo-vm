"load-simdcsv.st - load SimdCSV with Metacello from the submodule plugins/SimdCSVPlugin/simdcsv.st
(github.com/massimo-nocentini/simdcsv.st), whose BaselineOfSimdCSV has the groups Core (SimdCSV:
the dialect, the reader, the table and the writer, on the SimdCSVPlugin or in pure Smalltalk), Tests
(SimdCSV-Tests), NeoTests (SimdCSV-NeoTests: the comparison with NeoCSV, which Metacello downloads)
and default (Core Tests).  The submodule's commit is the version of the Smalltalk code that goes
with this plugin (docs/SimdCSV.md).

	SIMDCSV_PLUGIN_DIR=<repository>/plugins/SimdCSVPlugin SIMDCSV_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-simdcsv.st

SIMDCSV_PLUGIN_DIR     the directory of the plugin (the st command of Pharo 15 takes nothing but
                       its file, so the directories come in the environment); by default the
                       directory above the one of this file, when the command line names it
SIMDCSV_ST_DIR         the repository of the Smalltalk code, by default simdcsv.st in that directory
SIMDCSV_GROUPS         the groups to load, separated by spaces ('Core Tests' by default)
SIMDCSV_EXPECT_PLUGIN  yes, no or any (the default): whether the image must find the plugin

It prints

	SimdCSV: loaded, protocol 1, plugin SimdCSVPlugin 1.0 simdcsv a9494a8 <kernel> <platform>

(or 'SimdCSV: loaded, no plugin'), then the groups and packages loaded, and the test packages
loaded ('SimdCSV: test packages <name>...', which tests/run-st-tests.sh runs).  It exits with
status 1, before the image is saved, when the submodule is missing, a load fails, SimdCSVBackend
protocol is not 1, the plugin is expected and the image does not find it, the plugin is not expected
and the image finds it, a known text is not indexed and built as it must, a method of a SimdCSV
package uses an undeclared variable, or an extension method of a SimdCSV package (on a class that
no SimdCSV package defines) has a selector that does not begin with simdCSV: nothing of Pharo is
overridden.  Metacello's warnings are printed and resumed.
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail lineEnd plugin script repository groups expect packages tests methods ownClasses undeclared extensions backend info source word index rows answer |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-simdcsv: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'SIMDCSV_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [
		"the directory above this file's, when the command line names it"
		script := CommandLineArguments new arguments
			detect: [ :each | each endsWith: 'load-simdcsv.st' ]
			ifNone: [ fail value: 'SIMDCSV_PLUGIN_DIR is not set' ].
		script asFileReference asAbsolute parent parent ]
	ifNotNil: [ :path | path asFileReference ].
repository := ((environment at: 'SIMDCSV_ST_DIR' ifAbsent: [ nil ])
	ifNil: [ plugin / 'simdcsv.st' ]
	ifNotNil: [ :path | path asFileReference ]) / 'src'.
(repository / 'BaselineOfSimdCSV') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfSimdCSV') fullName
		, ': git submodule update --init plugins/SimdCSVPlugin/simdcsv.st' ].
groups := ((environment at: 'SIMDCSV_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'SIMDCSV_GROUPS names no group' ].
expect := environment at: 'SIMDCSV_EXPECT_PLUGIN' ifAbsent: [ 'any' ].
expect ifEmpty: [ expect := 'any' ].
(#( 'yes' 'no' 'any' ) includes: expect) ifFalse: [
	fail value: 'SIMDCSV_EXPECT_PLUGIN is yes, no or any, not ' , expect printString ].

[ [ Metacello new
	baseline: 'SimdCSV';
	repository: 'tonel://' , repository fullName;
	load: groups ]
	on: Warning
	do: [ :warning |
		out nextPutAll: 'SimdCSV: '; nextPutAll: warning class name; nextPutAll: ' ';
			nextPutAll: (warning messageText ifNil: [ '' ]) asString; nextPutAll: lineEnd; flush.
		warning resume ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

backend := Smalltalk globals at: #SimdCSVBackend ifAbsent: [ fail value: 'no SimdCSVBackend after the load' ].
backend protocol = 1 ifFalse: [ fail value: 'SimdCSVBackend protocol is ' , backend protocol printString , ', not 1' ].
info := backend pluginInfo.
backend pluginProblem ifNotNil: [ :problem | fail value: problem ].
(expect = 'yes' and: [ backend pluginAvailable not ]) ifTrue: [
	fail value: 'the image finds no SimdCSVPlugin (SIMDCSV_EXPECT_PLUGIN=yes)' ].
(expect = 'no' and: [ backend pluginAvailable ]) ifTrue: [
	fail value: 'the image finds a SimdCSVPlugin (SIMDCSV_EXPECT_PLUGIN=no): ' , info printString ].
"a known text, through the backend"
source := ('a,"b, ""c""",1' , (String with: Character lf) , '2,,x') asByteArray.
word := (Smalltalk globals at: #SimdCSVDialect) new wordFor: source final: true lines: true exact: false.
index := WordArray new: 8.
rows := Array new: 2.
answer := [
	backend index: source base: 1 from: 1 to: source size dialect: word into: index from: 1.
	backend rows: source base: 1 to: source size dialect: word index: index from: 1 to: 6 types: #[ 1 1 2 ]
		fieldCount: 3 into: rows from: 1 empty: nil budget: -1.
	rows ]
	on: Error
	do: [ :error | error description ].
answer = #( #( 'a' 'b, "c"' 1 ) #( '2' nil 'x' ) ) ifFalse: [
	fail value: 'the rows of ' , source asString printString , ' are ' , answer printString ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'SimdCSV' ])
	asSortedCollection asArray.
tests := packages select: [ :each | each endsWith: 'Tests' ].
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
"Opt-in only: every extension selector on a class of no SimdCSV package begins with simdCSV"
ownClasses := (packages flatCollect: [ :each | each asPackage definedClasses ]) asIdentitySet.
extensions := (packages flatCollect: [ :each | each asPackage extensionMethods ]) reject: [ :method |
	(ownClasses includes: method methodClass instanceSide) or: [ method selector beginsWith: 'simdCSV' ] ].
extensions ifNotEmpty: [
	fail value: 'extension selectors without simdCSV: ' , (extensions collect: [ :method | method printString ]) asArray printString ].

info
	ifNil: [ out nextPutAll: 'SimdCSV: loaded, no plugin' ]
	ifNotNil: [
		out
			nextPutAll: 'SimdCSV: loaded, protocol '; nextPutAll: backend protocol printString;
			nextPutAll: ', plugin '; nextPutAll: (info at: 2);
			nextPutAll: ' simdcsv '; nextPutAll: (info at: 3);
			nextPutAll: ' '; nextPutAll: (info at: 4);
			nextPutAll: ' '; nextPutAll: (info at: 5) ].
out
	nextPutAll: lineEnd;
	nextPutAll: 'SimdCSV: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	nextPutAll: 'SimdCSV: test packages'.
tests do: [ :each | out nextPutAll: ' '; nextPutAll: each ].
out nextPutAll: lineEnd; flush.
