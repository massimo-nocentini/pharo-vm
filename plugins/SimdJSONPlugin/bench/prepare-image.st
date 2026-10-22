"Prepare the image of the SimdJSON benchmark from a Pharo 15 image (b41) with NeoJSON 11e274d
 loaded.  run-bench.sh runs it with a native VM, on a copy of that image:

	SJ_PLUGIN_DIR=<repository>/plugins/SimdJSONPlugin \
	pharo --headless <copy>.image --no-default-preferences st --save --quit prepare-image.st

 It files in st/SimdJSON.st, which must come up in neo mode (NeoJSON loaded first), then
 bench/SJBenchMapping.st and bench/SJBench.st, and fails before anything is saved when a class
 is missing or a method uses an undeclared variable.  The st command of Pharo 15 takes nothing
 but its file, so the directory comes in the environment.  The saved image runs on every VM."

| environment dir undeclared reader info |
environment := OSEnvironment current.
dir := (environment
	at: 'SJ_PLUGIN_DIR'
	ifAbsent: [ self error: 'prepare-image: SJ_PLUGIN_DIR is not set' ]) asFileReference.
(Smalltalk globals includesKey: #NeoJSONReader) ifFalse: [
	Stdio stderr nextPutAll: 'prepare-image: NeoJSON is not loaded in this image'; lf; flush.
	Smalltalk exitFailure ].

(dir / 'st' / 'SimdJSON.st') fileIn.
(dir / 'bench' / 'SJBenchMapping.st') fileIn.
(dir / 'bench' / 'SJBench.st') fileIn.

reader := Smalltalk globals at: #SimdJSONReader ifAbsent: [ nil ].
(reader notNil and: [ reader mode == #neo ]) ifFalse: [
	Stdio stderr nextPutAll: 'prepare-image: SimdJSONReader is not in neo mode'; lf; flush.
	Smalltalk exitFailure ].
undeclared := (#( 'SimdJSON' 'SimdJSON-Bench' ) flatCollect: [ :name | name asPackage methods ])
	select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	Stdio stderr
		nextPutAll: 'prepare-image: undeclared variables in ';
		nextPutAll: (undeclared collect: [ :method | method printString ]) asArray printString;
		lf;
		flush.
	Smalltalk exitFailure ].

"The plugin is not needed here, only reported"
info := [ reader pluginAvailable ifTrue: [ reader pluginInfo ] ifFalse: [ nil ] ]
	on: Error do: [ :e | nil ].
Stdio stdout
	nextPutAll: 'prepare-image: SimdJSON in neo mode, SimdJSON-Bench loaded, plugin: ';
	nextPutAll: (info ifNil: [ 'not available' ] ifNotNil: [ (info at: 2) , ' simdjson ' , (info at: 3) ]);
	lf;
	flush.
