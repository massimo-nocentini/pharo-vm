"Lane 74-cairo: the SUnit tests of the CairoPlugin.

	CP_SCENE_SCALE=0.05 pharo --headless Pharo.image --no-default-preferences st --quit cairo-lane.st

 On an image into which the groups Cairo and CairoTests of BaselineOfVMPluginsPrimitives were
 loaded (plugins/CairoPlugin/tests/load-cairo.st), and which was saved and started again (so
 that CairoPlugin startUp: bound the plugin and installed the groups), prints first

	CairoPlugin: bound <CairoPlugin isBound>, installed <CairoPlugin installedGroups>, info <CairoPlugin info>

 and, when the plugin is bound and every group of CairoPlugin groups is installed, runs the
 test classes of the package VM-Plugins-Primitives-Cairo-Tests (all, or those named in
 CP_LANE_CLASSES, separated by blanks) and prints

	VM-Plugins-Primitives-Cairo-Tests: <run> run, <failures> failures, <errors> errors, <skipped> skipped, <n/a> not applicable, bound <isBound>, installed <installedGroups>

 then each test that failed or raised an error, as 'fail Class>>#selector', and each skipped
 test with its reason, as 'skip Class>>#selector: reason', one per line.  The tests of a VM
 without the plugin skip with a reason that starts with 'n/a: ' on a VM with it: they are
 counted as not applicable, not as skipped.  Without the plugin
 bound or with a group missing it prints instead 'VM-Plugins-Primitives-Cairo-Tests: not run'
 (the lane fails on either line).  The scale of the scenes comes from CP_SCENE_SCALE, which the
 tests read (0.05 on the web VMs, about 30 times slower than Cog).  The classes go through the
 globals, so that the file compiles on any image (none ran then).  The web check of the lane
 evaluates this file in the REPL of the Console page, with CP_SCENE_SCALE and CP_LANE_CLASSES in
 the environment of the VM, which prints the same lines."

| lineEnd out environment cairo bound installed wanted testCase skippedClass only classes suite result skips notApplicable |
lineEnd := OSPlatform current lineEnding.
out := Stdio stdout.
environment := OSEnvironment current.
cairo := Smalltalk globals at: #CairoPlugin ifAbsent: [ nil ].
bound := cairo notNil and: [ cairo isBound ].
installed := cairo ifNil: [ #() ] ifNotNil: [ cairo installedGroups asArray ].
out
	nextPutAll: 'CairoPlugin: bound ';
	nextPutAll: bound printString;
	nextPutAll: ', installed ';
	nextPutAll: installed printString;
	nextPutAll: ', info ';
	nextPutAll: (cairo ifNotNil: [ cairo info ]) printString;
	nextPutAll: lineEnd;
	flush.
wanted := cairo ifNil: [ #( #basic #shape #text ) ] ifNotNil: [ cairo groups asArray ].
(bound and: [ installed = wanted ]) ifFalse: [
	out nextPutAll: 'VM-Plugins-Primitives-Cairo-Tests: not run'; nextPutAll: lineEnd; flush.
	^ self ].

testCase := Smalltalk globals at: #TestCase.
skippedClass := Smalltalk globals at: #TestSkipped.
only := (environment at: 'CP_LANE_CLASSES' ifAbsent: [ '' ]) substrings: ' '.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = 'VM-Plugins-Primitives-Cairo-Tests'
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not
		and: [ only isEmpty or: [ only includes: each name asString ] ] ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'VM-Plugins-Primitives-Cairo-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
"the reasons of the skipped tests: each runs again, its skip caught here"
skips := result skipped asArray collect: [ :each | | reason |
	reason := '(ran without skipping the second time)'.
	[ each runCase ]
		on: skippedClass
		do: [ :skip | reason := skip messageText ].
	each -> reason ].
"the tests of a VM without the plugin skip as 'not applicable' on a VM with it (their reason
 starts with 'n/a: ', CairoPluginTestCase notApplicablePrefix): they are not counted as skips"
notApplicable := skips select: [ :each | each value asString beginsWith: 'n/a: ' ].
skips := skips reject: [ :each | notApplicable includes: each ].
out
	nextPutAll: 'VM-Plugins-Primitives-Cairo-Tests: ';
	nextPutAll: result runCount printString;
	nextPutAll: ' run, ';
	nextPutAll: result failureCount printString;
	nextPutAll: ' failures, ';
	nextPutAll: result errorCount printString;
	nextPutAll: ' errors, ';
	nextPutAll: skips size printString;
	nextPutAll: ' skipped, ';
	nextPutAll: notApplicable size printString;
	nextPutAll: ' not applicable, bound ';
	nextPutAll: cairo isBound printString;
	nextPutAll: ', installed ';
	nextPutAll: cairo installedGroups asArray printString;
	nextPutAll: lineEnd.
result failures asArray , result errors asArray do: [ :each |
	out
		nextPutAll: 'fail ';
		nextPutAll: each class name;
		nextPutAll: '>>#';
		nextPutAll: each selector;
		nextPutAll: lineEnd ].
skips do: [ :each |
	out
		nextPutAll: 'skip ';
		nextPutAll: each key class name;
		nextPutAll: '>>#';
		nextPutAll: each key selector;
		nextPutAll: ': ';
		nextPutAll: each value asString;
		nextPutAll: lineEnd ].
out flush.
