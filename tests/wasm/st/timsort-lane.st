"Lane 71-timsort: the SUnit tests of the TimSortPlugin.

	pharo --headless Pharo.image --no-default-preferences st --quit timsort-lane.st

 On an image into which the groups Core, Tests and SmalltalkTests of BaselineOfTimSort were
 loaded (plugins/TimSortPlugin/tests/load-timsort.st), runs the test classes of the packages
 TimSort-Tests (every suite on the plugin and on the port TimSortSmalltalk) and
 TimSort-Smalltalk-Tests (the port itself), and prints

	TimSort-Tests: <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend #<backend>, blockSortBackend #<backend>, resumableBackend <backend or nil>

 (the backends that TimSort chose: #plugin when the plugin is there, since its tests skip
 themselves without it, and a skipped test is no failure; #smalltalk for the sorts by a
 sortBlock, the default on every VM; and nil for the resumable C sort, which WebAssembly cannot
 run), then each test that failed or raised an error,
 as Class>>#selector, one per line.  The classes go through the globals, so that the file
 compiles on any image (none ran then).  The web check of the lane evaluates this file in the
 REPL of the Console page, which prints the same lines."

| testCase packages classes suite result facade |
testCase := Smalltalk globals at: #TestCase.
packages := #( 'TimSort-Tests' 'TimSort-Smalltalk-Tests' ).
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ (packages includes: each package name asString)
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'TimSort-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
facade := Smalltalk globals at: #TimSort.
Stdio stdout
	nextPutAll: 'TimSort-Tests: ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors, ';
	print: result skippedCount;
	nextPutAll: ' skipped, backend ';
	print: facade backend;
	nextPutAll: ', blockSortBackend ';
	print: facade blockSortBackend;
	nextPutAll: ', resumableBackend ';
	print: facade resumableBackend;
	nextPutAll: OSPlatform current lineEnding.
result failures asArray , result errors asArray do: [ :each |
	Stdio stdout
		nextPutAll: each class name;
		nextPutAll: '>>#';
		nextPutAll: each selector;
		nextPutAll: OSPlatform current lineEnding ].
Stdio stdout flush.
