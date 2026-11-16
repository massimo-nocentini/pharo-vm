"Lane 69-simdutf: the SUnit tests of the SimdUTFPlugin.

	pharo --headless Pharo.image --no-default-preferences st --quit simdutf-lane.st

 On an image into which the groups Core and Tests of BaselineOfSimdUTF were
 loaded (plugins/SimdUTFPlugin/tests/load-simdutf.st), runs the test classes of the package
 SimdUTF-Tests but SimdUTFSlowTests (opt-in: inputs of 48 to 64 MiB), and prints

	SimdUTF-Tests: <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend #<backend>

 (the backend that SimdUTF chose, #plugin when the plugin is there: its tests
 skip themselves without it, and a skipped test is no failure), then each
 test that failed or raised an error, as Class>>#selector, one per
 line.  The classes go through the globals, so that the file compiles on any
 image (none ran then).  The web check of the lane evaluates this file in the
 REPL of the Console page, which prints the same lines."

| testCase classes suite result |
testCase := Smalltalk globals at: #TestCase.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = 'SimdUTF-Tests'
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not
		and: [ each name ~~ #SimdUTFSlowTests ] ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'SimdUTF-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
Stdio stdout
	nextPutAll: 'SimdUTF-Tests: ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors, ';
	print: result skippedCount;
	nextPutAll: ' skipped, backend ';
	print: (Smalltalk globals at: #SimdUTF) backend;
	nextPutAll: OSPlatform current lineEnding.
result failures asArray , result errors asArray do: [ :each |
	Stdio stdout
		nextPutAll: each class name;
		nextPutAll: '>>#';
		nextPutAll: each selector;
		nextPutAll: OSPlatform current lineEnding ].
Stdio stdout flush.
