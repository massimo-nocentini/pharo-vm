"load-cmark.st - load CMark with Metacello from the submodule plugins/CMarkPlugin/cmark-gfm.st
(github.com/massimo-nocentini/cmark-gfm.st), whose BaselineOfCMark has the groups Core (CMark:
the parse tree, its visitor, the decoder of the plugin's tape, parsing and rendering through the
CMarkPlugin), Tests (CMark-Tests), Roassal (Core and CMark-Roassal: a document as one RSComposite),
RoassalTests (Roassal and CMark-Roassal-Tests), RoassalFork (the user's fork of Roassal, then
Roassal), Calypso (Roassal and CMark-Calypso: the Markdown tab of class comments in Calypso),
CalypsoTests (Calypso, RoassalTests and CMark-Calypso-Tests) and default (Core Tests Roassal
RoassalTests Calypso CalypsoTests).  The submodule's commit is the version of the Smalltalk code
that goes with this plugin (docs/CMark.md).

	CMARK_PLUGIN_DIR=<repository>/plugins/CMarkPlugin CMARK_GROUPS='Core Tests' \
	pharo --headless X.image --no-default-preferences st --save --quit load-cmark.st

CMARK_PLUGIN_DIR     the directory of the plugin (the st command of Pharo 15 takes nothing but
                     its file, so the directories come in the environment); by default the
                     directory above the one of this file, when the command line names it
CMARK_GROUPS         the groups to load, separated by spaces ('Core Tests' by default)
CMARK_EXPECT_PLUGIN  yes, no or any (the default): whether the image must find the plugin

It prints

	CMark: loaded, protocol 1, plugin CMarkPlugin 1.0 cmark-gfm 0.29.0.gfm.14 <platform>

(or 'CMark: loaded, no plugin'), then the groups and packages loaded, and the test packages
loaded ('CMark: test packages <name>...', which tests/run-st-tests.sh runs).  It exits with
status 1, before the image is saved, when the submodule is missing, a load fails,
CMarkPrimitives protocol is not 1, the plugin is expected and the image does not find it (or
'# Hi' does not parse into a heading of level 1), the plugin is not expected and the image finds
it, a method of a CMark package uses an undeclared variable, or an extension method of a CMark
package (on a class that no CMark package defines) has a selector that does not begin with
cmark: nothing of Pharo is overridden.
Metacello's warnings are printed and resumed; MCMergeOrLoadWarning, which Metacello signals when
the group RoassalFork loads the fork of Roassal over the Roassal packages of a stock image, is
answered with load (a plain resume would keep the methods of the image).
Metacello makes UUIDs for the packages it loads: the VM needs its UUIDPlugin."

| environment out fail lineEnd plugin script repository groups expect load packages tests methods ownClasses undeclared extensions primitives info parsed |
environment := OSEnvironment current.
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'load-cmark: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'CMARK_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [
		"the directory above this file's, when the command line names it"
		script := CommandLineArguments new arguments
			detect: [ :each | each endsWith: 'load-cmark.st' ]
			ifNone: [ fail value: 'CMARK_PLUGIN_DIR is not set' ].
		script asFileReference asAbsolute parent parent ]
	ifNotNil: [ :path | path asFileReference ].
repository := plugin / 'cmark-gfm.st' / 'src'.
(repository / 'BaselineOfCMark') isDirectory ifFalse: [
	fail value: 'no ' , (repository / 'BaselineOfCMark') fullName
		, ': git submodule update --init plugins/CMarkPlugin/cmark-gfm.st' ].
groups := ((environment at: 'CMARK_GROUPS' ifAbsent: [ 'Core Tests' ]) substrings: ' ') asArray.
groups ifEmpty: [ fail value: 'CMARK_GROUPS names no group' ].
expect := environment at: 'CMARK_EXPECT_PLUGIN' ifAbsent: [ 'any' ].
expect ifEmpty: [ expect := 'any' ].
(#( 'yes' 'no' 'any' ) includes: expect) ifFalse: [
	fail value: 'CMARK_EXPECT_PLUGIN is yes, no or any, not ' , expect printString ].
"The load prints and resumes Metacello's warnings, and loads over the modified Roassal packages"
load := [ :what :block |
	[ block value ]
		on: Warning
		do: [ :warning |
			out nextPutAll: what; nextPutAll: ': '; nextPutAll: warning class name; nextPutAll: ' ';
				nextPutAll: (warning messageText ifNil: [ '' ]) asString; nextPutAll: lineEnd; flush.
			warning class name = #MCMergeOrLoadWarning
				ifTrue: [ warning load ]
				ifFalse: [ warning resume ] ] ].

[ load value: 'CMark' value: [
	Metacello new
		baseline: 'CMark';
		repository: 'tonel://' , repository fullName;
		load: groups ] ]
	on: Error
	do: [ :error | fail value: 'loading ' , groups printString , ': ' , error description ].

primitives := Smalltalk globals at: #CMarkPrimitives ifAbsent: [ fail value: 'no CMarkPrimitives after the load' ].
primitives protocol = 1 ifFalse: [ fail value: 'CMarkPrimitives protocol is ' , primitives protocol printString , ', not 1' ].
info := primitives pluginInfo.
(expect = 'yes' and: [ info isNil ]) ifTrue: [
	fail value: 'the image finds no CMarkPlugin (CMARK_EXPECT_PLUGIN=yes)' ].
(expect = 'no' and: [ info notNil ]) ifTrue: [
	fail value: 'the image finds a CMarkPlugin (CMARK_EXPECT_PLUGIN=no): ' , info printString ].
info ifNotNil: [
	parsed := [ ((Smalltalk globals at: #CMark) parse: '# Hi') children first level ]
		on: Error
		do: [ :error | error description ].
	parsed = 1 ifFalse: [ fail value: '''# Hi'' parses into ' , parsed printString , ', not a heading of level 1' ] ].

packages := (PackageOrganizer default packageNames select: [ :each | each beginsWith: 'CMark' ])
	asSortedCollection asArray.
tests := packages select: [ :each | each endsWith: '-Tests' ].
methods := packages flatCollect: [ :each | each asPackage methods ].
undeclared := methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
"Opt-in only: every extension selector on a class of no CMark package begins with cmark (D6)"
ownClasses := (packages flatCollect: [ :each | each asPackage definedClasses ]) asIdentitySet.
extensions := (packages flatCollect: [ :each | each asPackage extensionMethods ]) reject: [ :method |
	(ownClasses includes: method methodClass instanceSide) or: [ method selector beginsWith: 'cmark' ] ].
extensions ifNotEmpty: [
	fail value: 'extension selectors without cmark: ' , (extensions collect: [ :method | method printString ]) asArray printString ].

info
	ifNil: [ out nextPutAll: 'CMark: loaded, no plugin' ]
	ifNotNil: [
		out
			nextPutAll: 'CMark: loaded, protocol '; nextPutAll: primitives protocol printString;
			nextPutAll: ', plugin '; nextPutAll: (info at: 2);
			nextPutAll: ' cmark-gfm '; nextPutAll: (info at: 3);
			nextPutAll: ' '; nextPutAll: (info at: 4) ].
out
	nextPutAll: lineEnd;
	nextPutAll: 'CMark: groups '; nextPutAll: groups printString;
	nextPutAll: ', packages '; nextPutAll: packages printString; nextPutAll: lineEnd;
	nextPutAll: 'CMark: test packages'.
tests do: [ :each | out nextPutAll: ' '; nextPutAll: each ].
out nextPutAll: lineEnd; flush.
!
