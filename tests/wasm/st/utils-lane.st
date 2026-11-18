"Lane 73-utils: the SUnit tests of the UtilsPlugin.

	pharo --headless Pharo.image --no-default-preferences st --quit utils-lane.st

 On an image into which the groups Core and Tests of BaselineOfVMPluginsPrimitives were loaded
 (plugins/UtilsPlugin/tests/load-utils.st), runs the test classes of the package
 VM-Plugins-Primitives-Tests (every feature on the plugin and on its Smalltalk twins, the
 differential tests and those of the primitives' contract), and prints

	VM-Plugins-Primitives-Tests: <run> run, <failures> failures, <errors> errors, <skipped> skipped, plugin <UtilsPlugin info>

 (the plugin's info, nil without it: the tests of the plugin skip themselves without it, and a
 skipped test is no failure), then each test that failed or raised an error, as
 Class>>#selector, one per line.  The classes go through the globals, so that the file compiles
 on any image (none ran then).  The web check of the lane evaluates this file in the REPL of
 the Console page, which prints the same lines."

| testCase classes suite result utils |
testCase := Smalltalk globals at: #TestCase.
classes := (Smalltalk globals allClasses select: [ :each |
	each package notNil
		and: [ each package name asString = 'VM-Plugins-Primitives-Tests'
		and: [ (each inheritsFrom: testCase)
		and: [ each isAbstract not ] ] ] ])
	asSortedCollection: [ :a :b | a name <= b name ].
suite := TestSuite named: 'VM-Plugins-Primitives-Tests'.
classes do: [ :each | suite addTests: each suite tests ].
result := suite run.
utils := Smalltalk globals at: #UtilsPlugin.
Stdio stdout
	nextPutAll: 'VM-Plugins-Primitives-Tests: ';
	print: result runCount;
	nextPutAll: ' run, ';
	print: result failureCount;
	nextPutAll: ' failures, ';
	print: result errorCount;
	nextPutAll: ' errors, ';
	print: result skippedCount;
	nextPutAll: ' skipped, plugin ';
	nextPutAll: utils info printString;
	nextPutAll: OSPlatform current lineEnding.
result failures asArray , result errors asArray do: [ :each |
	Stdio stdout
		nextPutAll: each class name;
		nextPutAll: '>>#';
		nextPutAll: each selector;
		nextPutAll: OSPlatform current lineEnding ].
Stdio stdout flush.
