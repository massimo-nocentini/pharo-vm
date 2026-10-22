"Lane 68-simdjson: the SUnit tests of the SimdJSONPlugin.

	pharo --headless Pharo.image --no-default-preferences st --quit simdjson-lane.st

 On an image into which plugins/SimdJSONPlugin/st/SimdJSON.st, then
 SimdJSON-Tests.st, were filed, runs the test classes of the package
 SimdJSON-Tests but SimdJSONSlowTests (opt-in: a 12 MB document, and an
 old-space cap), and prints

	SimdJSON-Tests: <run> run, <failures> failures, <errors> errors

 then each test that failed or raised an error, as Class>>#selector, one per
 line.  The classes go through the globals, so that the file compiles on any
 image (none ran then).  The web check of the lane evaluates this file in the
 REPL of the Console page, which prints the same lines."

| testCase classes suite result |
testCase := Smalltalk globals at: #TestCase.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = 'SimdJSON-Tests'
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not
		and: [ each name ~~ #SimdJSONSlowTests ] ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'SimdJSON-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
Stdio stdout
	nextPutAll: 'SimdJSON-Tests: ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors';
	lf.
result failures asArray , result errors asArray do: [ :each |
	Stdio stdout
		nextPutAll: each class name;
		nextPutAll: '>>#';
		nextPutAll: each selector;
		lf ].
Stdio stdout flush.
