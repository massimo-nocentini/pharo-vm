"Prepare the image of the TimSort benchmark from a Pharo 12 to 15 image.  run-bench.sh runs it
 with a native VM, on a copy of that image:

	TS_PLUGIN_DIR=<repository>/plugins/TimSortPlugin TS_BENCH_DIR=<that>/bench TS_GROUPS='Core FFI' \
	pharo --headless <copy>.image --no-default-preferences st --save --quit prepare-image.st

 It files in tests/load-timsort.st of TS_PLUGIN_DIR, which loads the groups of TS_GROUPS (Core and
 FFI: the packages TimSort and TimSort-Smalltalk, and TimSort-FFI for libTimSortFFI) with
 Metacello from the submodule timsort.st and checks them, then files in TS_BENCH_DIR/TSBench.st,
 and fails before anything is saved when a class is missing or a method of TSBench uses an
 undeclared variable.  Metacello makes UUIDs: the VM needs its UUIDPlugin.  The st command of
 Pharo 15 takes nothing but its file, so the directories come in the environment.  The saved image
 runs on every VM."

| environment plugin bench lineEnd fail undeclared facade info |
environment := OSEnvironment current.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'prepare-image: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'TS_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'TS_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
bench := (environment at: 'TS_BENCH_DIR' ifAbsent: [ nil ])
	ifNil: [ plugin / 'bench' ]
	ifNotNil: [ :path | path asFileReference ].
(plugin / 'tests' / 'load-timsort.st') fileIn.
(Smalltalk globals includesKey: #TimSort) ifFalse: [ fail value: 'TimSort is not loaded' ].
(bench / 'TSBench.st') fileIn.
(Smalltalk globals includesKey: #TSBench) ifFalse: [ fail value: 'TSBench is not defined' ].
undeclared := 'TSBench' asPackage methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].

"The backends are not needed here, only reported"
facade := Smalltalk globals at: #TimSort.
info := facade pluginInfo.
Stdio stdout
	nextPutAll: 'prepare-image: TimSort and TSBench loaded, plugin: ';
	nextPutAll: (info ifNil: [ 'not available' ] ifNotNil: [ (info at: 2) , ' ' , (info at: 3) , ' ' , (info at: 4) ]);
	nextPutAll: ', TimSortLibrary: ';
	nextPutAll: ((Smalltalk globals at: #TimSortLibrary ifAbsent: [ nil ])
		ifNil: [ 'not loaded' ]
		ifNotNil: [ :library | library isAvailable ifTrue: [ library coroutineImplementation ] ifFalse: [ 'not available' ] ]);
	nextPutAll: lineEnd;
	flush.
