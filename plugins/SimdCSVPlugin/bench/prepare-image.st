"Prepare the image of the SimdCSV benchmark from a Pharo 12 to 15 image.  run-bench.sh runs it
 with a native VM, on a copy of that image:

	SIMDCSV_PLUGIN_DIR=<repository>/plugins/SimdCSVPlugin SC_BENCH_DIR=<that>/bench \
	SIMDCSV_GROUPS='Core NeoTests' \
	pharo --headless <copy>.image --no-default-preferences st --save --quit prepare-image.st

 It files in tests/load-simdcsv.st of SIMDCSV_PLUGIN_DIR, which loads the groups Core and NeoTests
 (the second brings NeoCSV, the baseline of the benchmark; Metacello downloads it) from
 SIMDCSV_ST_DIR, by default the submodule simdcsv.st, and checks them; then SC_BENCH_DIR/SCBench.st.
 It fails before anything is saved when a class is missing or a method of SCBench uses an
 undeclared variable.  With SC_DUMP_DIR it also writes the data there as files of SC_DUMP_BYTES
 bytes (10,000,000), for scbench.cpp.  Metacello makes UUIDs: the VM needs its UUIDPlugin.  The
 saved image runs on every VM."

| environment plugin bench lineEnd fail undeclared info |
environment := OSEnvironment current.
lineEnd := OSPlatform current lineEnding.
fail := [ :message |
	Stdio stderr nextPutAll: 'prepare-image: '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
plugin := (environment at: 'SIMDCSV_PLUGIN_DIR' ifAbsent: [ nil ])
	ifNil: [ fail value: 'SIMDCSV_PLUGIN_DIR is not set' ]
	ifNotNil: [ :path | path asFileReference ].
bench := (environment at: 'SC_BENCH_DIR' ifAbsent: [ nil ])
	ifNil: [ plugin / 'bench' ]
	ifNotNil: [ :path | path asFileReference ].
(plugin / 'tests' / 'load-simdcsv.st') fileIn.
#( #SimdCSV #NeoCSVReader #NeoCSVWriter ) do: [ :each |
	(Smalltalk globals includesKey: each) ifFalse: [ fail value: each , ' is not loaded' ] ].
(bench / 'SCBench.st') fileIn.
(Smalltalk globals includesKey: #SCBench) ifFalse: [ fail value: 'SCBench is not defined' ].
undeclared := 'SCBench' asPackage methods select: [ :method | method usesUndeclareds ].
undeclared ifNotEmpty: [
	fail value: 'undeclared variables in ' , (undeclared collect: [ :method | method printString ]) asArray printString ].
(environment at: 'SC_DUMP_DIR' ifAbsent: [ nil ]) ifNotNil: [ :path |
	(Smalltalk at: #SCBench)
		dump: path
		size: (environment at: 'SC_DUMP_BYTES' ifAbsent: [ '10000000' ]) asInteger ].

"The backend is not needed here, only reported"
info := (Smalltalk at: #SimdCSV) pluginInfo.
Stdio stdout
	nextPutAll: 'prepare-image: SimdCSV, NeoCSV and SCBench loaded, plugin: ';
	nextPutAll: (info ifNil: [ 'not available' ] ifNotNil: [ (info at: 2) , ' simdcsv ' , (info at: 3) , ' ' , (info at: 4) ]);
	nextPutAll: lineEnd;
	flush.
