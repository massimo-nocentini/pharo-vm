"Prepare the image of the SimdUTF benchmark from a Pharo 12 to 15 image.  run-bench.sh runs it
 with a native VM, on a copy of that image:

	SU_PLUGIN_DIR=<repository>/plugins/SimdUTFPlugin SU_BENCH_DIR=<that>/bench SU_GROUPS='Core FFI' \
	pharo --headless <copy>.image --no-default-preferences st --save --quit prepare-image.st

 It files in tests/load-simdutf.st of SU_PLUGIN_DIR, which loads the groups of SU_GROUPS (Core and
 FFI: the package SimdUTF, and SimdUTF-FFI for libSimdUTFFFI) with Metacello from the submodule
 simdutf.st and checks them, then files in SU_BENCH_DIR/SUBench.st, and fails before anything is
 saved when a class is missing or a method of SUBench uses an undeclared variable.  Metacello
 makes UUIDs: the VM needs its UUIDPlugin.  The st command of Pharo 15 takes nothing but its file,
 so the directories come in the environment.  The saved image runs on every VM."

| environment plugin bench lineEnd fail undeclared facade info |
environment := OSEnvironment current.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'prepare-image: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'SU_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'SU_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
bench := (environment at: 'SU_BENCH_DIR' ifAbsent: [ nil ])
	ifNil: [ plugin / 'bench' ]
	ifNotNil: [ :path | path asFileReference ].
(plugin / 'tests' / 'load-simdutf.st') fileIn.
(Smalltalk globals includesKey: #SimdUTF) ifFalse: [ fail value: 'SimdUTF is not loaded' ].
(bench / 'SUBench.st') fileIn.
(Smalltalk globals includesKey: #SUBench) ifFalse: [ fail value: 'SUBench is not defined' ].
undeclared := 'SUBench' asPackage methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].

"The backends are not needed here, only reported"
facade := Smalltalk globals at: #SimdUTF.
info := facade pluginInfo.
Stdio stdout
	nextPutAll: 'prepare-image: SimdUTF and SUBench loaded, plugin: ';
	nextPutAll: (info ifNil: [ 'not available' ] ifNotNil: [ (info at: 2) , ' simdutf ' , (info at: 3) , ' ' , (info at: 4) ]);
	nextPutAll: ', SimdUTFLibrary: ';
	nextPutAll: ((Smalltalk globals at: #SimdUTFLibrary ifAbsent: [ nil ])
		ifNil: [ 'not loaded' ]
		ifNotNil: [ :library | library isAvailable ifTrue: [ library implementationName ] ifFalse: [ 'not available' ] ]);
	nextPutAll: lineEnd;
	flush.
