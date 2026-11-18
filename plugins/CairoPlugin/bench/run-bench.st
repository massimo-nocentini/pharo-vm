"run-bench.st - the benchmark of the CairoPlugin (bench/run-bench.sh, docs/CairoPlugin.md).

	CP_BENCH_SCENES=b-rects,micro pharo --headless X.image --no-default-preferences st run-bench.st

On an image into which the groups Cairo, CairoTests and CairoBenchmarks of
BaselineOfVMPluginsPrimitives were loaded (tests/load-cairo.st), saved and started again
(so that CairoPlugin startUp: bound the plugin), runs CairoPluginBenchmark fromEnvironment
(the environment: CP_BENCH_SCENES, CP_BENCH_SIZES, CP_BENCH_REPS, CP_BENCH_SCALE,
CP_BENCH_ROASSAL_SCALE, CP_BENCH_MODES, CP_BENCH_MICRO_CALLS, CP_BENCH_VM; see the class
comment) and prints its lines (BENCH, CALLS, MICRO, COND, SKIP, ERROR) on stdout, then

	BENCH-DONE failures <n>

where n counts the scenes whose pixels differ between the modes (PIXELS-DIFFER), those whose
cairo calls of a function differ (CALLS-DIFFER), those where a plugin mode fell back to the FFI
(FALLBACK) and the errors.  The VM exits with status 0 when
n is 0, else 1.  With CP_BENCH_REPL set (bench/web-bench.mjs evaluates this file in the REPL of
pharo-web) it does not quit.  The classes go through the globals, so that the file compiles in
any image."

| out benchmark failures |
out := Stdio stdout.
benchmark := Smalltalk globals at: #CairoPluginBenchmark ifAbsent: [ nil ].
failures := benchmark
	ifNil: [
		out nextPutAll: 'ERROR no CairoPluginBenchmark: load the group CairoBenchmarks (tests/load-cairo.st)'; lf.
		1 ]
	ifNotNil: [
		[ benchmark fromEnvironment runOn: out ]
			on: Error
			do: [ :e |
				out nextPutAll: 'ERROR '; nextPutAll: e class name; nextPutAll: ': '; nextPutAll: e description; lf.
				1 ] ].
out nextPutAll: 'BENCH-DONE failures '; nextPutAll: failures printString; lf; flush.
(OSEnvironment current at: 'CP_BENCH_REPL' ifAbsent: [ nil ]) ifNil: [
	failures = 0
		ifTrue: [ Smalltalk snapshot: false andQuit: true ]
		ifFalse: [ Smalltalk exitFailure ] ].
