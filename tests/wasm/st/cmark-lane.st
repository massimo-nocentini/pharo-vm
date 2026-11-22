"Lane 75-cmark: the SUnit tests of CMark, and the room old space keeps after them.

	CMARK_LANE_PACKAGE=CMark-Tests pharo --headless Pharo.image --no-default-preferences st --quit cmark-lane.st

 On an image into which plugins/CMarkPlugin/tests/load-cmark.st loaded the groups Core and
 Tests of BaselineOfCMark (and Roassal and RoassalTests for the package CMark-Roassal-Tests),
 runs the test classes of the package CMARK_LANE_PACKAGE (CMark-Tests when it is not set, as in
 the web check, whose REPL has no environment of its own), and prints

	CMark: free old space before <bytes>
	<package>: <run> run, <failures> failures, <errors> errors, <skipped> skipped, plugin <platform>
	CMark: free old space after <bytes>
	CMark: grow 64 MiB ok, free old space <bytes>

 (the platform of the plugin, nil without it: the tests of the plugin skip themselves without
 it, and a skipped test is no failure), then each test that failed, raised an error or was
 skipped, as 'failed Class>>#selector', 'error ...' or 'skipped ...', one per line.  The last
 check is the headroom of docs/CMark.md, Memory and limits: on WebAssembly the C heap of the
 plugin never shrinks and grows into the window that old space needs, so after the whole suite
 Smalltalk growMemoryByAtLeast: 64 MiB must still succeed ('CMark: grow 64 MiB FAILED <why>'
 otherwise).  The classes go through the globals, so that the file compiles on any image.  The
 web check of the lane evaluates this file in the REPL of the Console page, which prints the
 same lines."

| out lineEnd packageName testCase classes suite result primitives platform free grown line |
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
packageName := [ OSEnvironment current at: 'CMARK_LANE_PACKAGE' ifAbsent: [ nil ] ]
	on: Error
	do: [ :error | nil ].
(packageName isNil or: [ packageName isEmpty ]) ifTrue: [ packageName := 'CMark-Tests' ].
free := [ :what |
	out
		nextPutAll: 'CMark: free old space ';
		nextPutAll: what;
		nextPutAll: ' ';
		print: (Smalltalk vm parameterAt: 54);
		nextPutAll: lineEnd;
		flush ].
free value: 'before'.
testCase := Smalltalk globals at: #TestCase.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = packageName
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: packageName.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
primitives := Smalltalk globals at: #CMarkPrimitives ifAbsent: [ nil ].
platform := primitives ifNotNil: [ primitives platform ].
out
	nextPutAll: packageName;
	nextPutAll: ': ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors, ';
	print: result skippedCount;
	nextPutAll: ' skipped, plugin ';
	nextPutAll: platform printString;
	nextPutAll: lineEnd;
	flush.
free value: 'after'.
grown := [ Smalltalk growMemoryByAtLeast: 64 * 1024 * 1024 ]
	on: Error
	do: [ :error | error description ].
line := (grown isInteger and: [ grown >= (64 * 1024 * 1024) ])
	ifTrue: [ 'CMark: grow 64 MiB ok, free old space ' , (Smalltalk vm parameterAt: 54) printString ]
	ifFalse: [ 'CMark: grow 64 MiB FAILED ' , grown printString ].
out nextPutAll: line; nextPutAll: lineEnd.
{ 'failed' -> result failures. 'error' -> result errors. 'skipped' -> result skipped } do: [ :kind |
	kind value asArray do: [ :each |
		out
			nextPutAll: kind key;
			nextPutAll: ' ';
			nextPutAll: each class name;
			nextPutAll: '>>#';
			nextPutAll: each selector;
			nextPutAll: lineEnd ] ].
out flush.
