"Lane 77-simdcsv: the SUnit tests of the SimdCSVPlugin.

	pharo --headless Pharo.image --no-default-preferences st --quit simdcsv-lane.st

 On an image into which plugins/SimdCSVPlugin/tests/load-simdcsv.st loaded the groups Core and
 Tests of BaselineOfSimdCSV, runs the test classes of the package SimdCSV-Tests but
 SimdCSVSlowTest (opt-in: 100 MB, and Python), and prints

	SimdCSV-Tests: <run> run, <failures> failures, <errors> errors, <skipped> skipped, backend #<backend>

 (the backend that SimdCSVBackend chose, #plugin when the plugin is there: the tests of the plugin
 skip themselves without it, and a skipped test is no failure), then each test that failed, raised
 an error or was skipped, as 'failed Class>>#selector', 'error ...' or 'skipped ...', one per line.
 The tests run twice, on the plugin and on the Smalltalk backend (SimdCSVTestCase), with the smaller
 inputs of SimdCSVTestCase class>>isSmall on WebAssembly.  The classes go through the globals, so
 that the file compiles on any image (none ran then).  The web check of the lane evaluates this
 file in the REPL of the Console page, which prints the same lines."

| out lineEnd testCase classes suite result |
out := Stdio stdout.
lineEnd := OSPlatform current lineEnding.
testCase := Smalltalk globals at: #TestCase.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = 'SimdCSV-Tests'
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not
		and: [ each name ~~ #SimdCSVSlowTest ] ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'SimdCSV-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
out
	nextPutAll: 'SimdCSV-Tests: ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors, ';
	print: result skippedCount;
	nextPutAll: ' skipped, backend ';
	print: (Smalltalk globals at: #SimdCSVBackend) backend;
	nextPutAll: lineEnd.
{ 'failed' -> result failures. 'error' -> result errors. 'skipped' -> result skipped } do: [ :kind |
	kind value asArray do: [ :each |
		out
			nextPutAll: kind key;
			nextPutAll: ' ';
			nextPutAll: each printString;
			nextPutAll: lineEnd ] ].
out flush.
