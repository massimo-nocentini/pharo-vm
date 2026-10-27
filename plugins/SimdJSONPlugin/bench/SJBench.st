"SJBench.st - SimdJSONReader against NeoJSONReader, on the same inputs (Pharo 15, chunk format)

 prepare-image.st files this in, after SimdJSON.st and SJBenchMapping.st, into a copy of an
 image with NeoJSON, natively; the saved image then runs on every VM with run-bench.st:
     pharo --headless sjbench.image --no-default-preferences st --quit run-bench.st
 run-bench.sh does both.  Environment:
     SJ_CORPUS      directory of the corpus: the jsonexamples of simdjson-data (fetch-corpus.sh)
     SJ_FILES       comma-separated files to time (default: SJBench defaultFiles)
     SJ_SERIES      comma-separated series to run (default: SJBench allSeries)
     SJ_CHECK       all (default): check every .json file of the corpus first; files: only the
                    timed ones; none: no check
     SJ_N           timed samples per series, at most (default 10, 5 in WebAssembly)
     SJ_WARMUP      untimed runs before them (default 2, 1 in WebAssembly)
     SJ_BUDGET      ms per series after which no sample starts, once 3 ran (default 20000)
     SJ_MIN_SAMPLE  ms a sample lasts at least: it repeats the run k times (default 50)
     SJ_LABEL, SJ_RUN  the VM and the run printed in each line
     SJ_CPU, SJ_GOVERNOR, SJ_TASKSET  printed in the INFO line (the image cannot read them in
                    the browser)

 Inputs.  Each file is read once as a ByteArray (its UTF-8 bytes) and decoded once into a String;
 neither is timed, and MB/s is always over the UTF-8 bytes.  Three more are generated:
 ndjson-20k.ndjson (20,000 small objects, one per line), small-200.json and small-2k.json.

 Checks.  Before any timing, NeoJSONReader's value is compared with SimdJSONReader's, from the
 String, from the bytes and in tree mode, with deep equality that includes the classes of
 numbers, strings and collections; then, for the timed files, with Symbol keys, as NeoJSONObjects,
 through the mappings of SJBenchMapping and on the NDJSON; and NeoJSONWriter's text of the value
 with SimdJSONWriter's, plain and pretty.  A number of another class but the
 same value is a policy difference, which the exact numbers of the reader should leave at 0.  A
 series whose check found a difference is skipped.

 Series.  A full GC, SJ_WARMUP runs, the calibration of k (doubled until a sample lasts
 SJ_MIN_SAMPLE ms: the microsecond clock has ms resolution in WebAssembly), then up to SJ_N samples
 of k runs.  Printed: median and min ms of one run, MB/s at the median, the ms of the GCs of the
 samples (vm parameters 8 and 10) and k.  Around the samples of an sj series, the counters of the
 reader and of the plugin give the CSPLIT line (the plugin's parse and build ms per run) and the
 STATS line, and must show no cache hit and no failed build.

 Lines are tab-separated: INFO, CHECK, RESULT, CSPLIT, STATS, SPEEDUP, ERROR, SKIP; summarize.py
 reads them.  ASCII only, one method per methodsFor: section (Pharo 15 file-in rules)."

(Object << #SJBench
	slots: { #out. #corpus. #files. #series. #checking. #warmup. #iterations. #budgetMs. #minSampleMs.
		#label. #run. #cpu. #governor. #taskset. #pluginAvailable. #layout. #failed. #medians.
		#errors. #diffs. #policyDiffs. #firstDiff. #firstPolicy };
	tag: 'Bench'; package: 'SimdJSON-Bench') install!

!SJBench class methodsFor: 'running'!
run
	"Run the series of the environment; answer the number of ERROR lines"

	^ self new setUpFromEnvironment; runAll! !

!SJBench class methodsFor: 'defaults'!
defaultFiles

	^ #( 'twitter.json' 'citm_catalog.json' 'canada.json' 'gsoc-2018.json' 'mesh.json'
		'numbers.json' 'random.json' 'twitterescaped.json' 'github_events.json'
		'update-center.json' 'apache_builds.json' 'instruments.json' 'marine_ik.json'
		'ndjson-20k.ndjson' 'small-200.json' 'small-2k.json' )! !

!SJBench class methodsFor: 'defaults'!
allSeries

	^ #( 'neo-string' 'sj-string' 'sj-bytes' 'sj-tree' 'sj-float' 'sj-prim'
		'neo-symbols' 'sj-symbols' 'neo-objects' 'sj-objects'
		'neo-map' 'sj-map' 'neo-map-symbols' 'sj-map-symbols'
		'neo-ndjson-next' 'sj-ndjson-next' 'neo-ndjson-upToEnd' 'sj-ndjson-upToEnd'
		'neo-small' 'sj-small' 'utf8Encoded'
		'neo-write' 'sj-write' 'sj-write-bytes' 'neo-write-pretty' 'sj-write-pretty' 'neo-write-utf8' 'sj-write-utf8' )! !

!SJBench class methodsFor: 'defaults'!
checkOfSeries
	"The check that must have found no difference for an sj series to run"

	^ Dictionary newFromPairs: #( 'sj-string' 'string' 'sj-float' 'string' 'sj-prim' 'string'
		'sj-bytes' 'bytes' 'sj-tree' 'tree' 'sj-symbols' 'symbols' 'sj-objects' 'objects'
		'sj-map' 'map' 'sj-map-symbols' 'map-symbols' 'sj-ndjson-next' 'ndjson-next'
		'sj-ndjson-upToEnd' 'ndjson-upToEnd' 'sj-small' 'string'
		'sj-write' 'write' 'sj-write-bytes' 'write' 'sj-write-pretty' 'write-pretty' 'sj-write-utf8' 'write' )! !

!SJBench class methodsFor: 'defaults'!
speedups
	"The pairs of series of the SPEEDUP lines: the median of the first over that of the second"

	^ #( #( 'neo-string' 'sj-string' ) #( 'neo-symbols' 'sj-symbols' ) #( 'neo-objects' 'sj-objects' )
		#( 'neo-map' 'sj-map' ) #( 'neo-map-symbols' 'sj-map-symbols' ) #( 'neo-map' 'sj-map-symbols' )
		#( 'neo-ndjson-next' 'sj-ndjson-next' ) #( 'neo-ndjson-upToEnd' 'sj-ndjson-upToEnd' )
		#( 'neo-small' 'sj-small' ) #( 'sj-tree' 'sj-string' ) #( 'sj-string' 'sj-prim' )
		#( 'neo-write' 'sj-write' ) #( 'neo-write-pretty' 'sj-write-pretty' ) #( 'neo-write-utf8' 'sj-write-utf8' ) )! !

!SJBench class methodsFor: 'defaults'!
pluginCounters
	"The names of the counters of primitiveInfo, in its order"

	^ SimdJSONReader pluginStatisticsKeys collect: [ :each | each asString ]! !

!SJBench class methodsFor: 'inputs'!
generated: aName
	"The String of a generated input, or nil"

	aName = 'ndjson-20k.ndjson' ifTrue: [ ^ self ndjson: 20000 ].
	aName = 'small-200.json' ifTrue: [ ^ self record: 1 ].
	aName = 'small-2k.json' ifTrue: [ ^ self records: 9 ].
	^ nil! !

!SJBench class methodsFor: 'inputs'!
ndjson: aCount
	"aCount small objects, one per line (1.37 MB for 20,000)"

	^ String streamContents: [ :stream |
		1 to: aCount do: [ :i |
			stream
				nextPutAll: '{"id":'; print: i;
				nextPutAll: ',"name":"item'; print: i;
				nextPutAll: '","tags":["a","b"],"v":'; print: i / 7.0;
				nextPut: $};
				nextPut: Character lf ] ]! !

!SJBench class methodsFor: 'inputs'!
record: anInteger
	"An object of about 200 bytes"

	^ String streamContents: [ :stream |
		stream
			nextPutAll: '{"id":'; print: anInteger;
			nextPutAll: ',"name":"user'; print: anInteger;
			nextPutAll: '","email":"user'; print: anInteger;
			nextPutAll: '@example.org","active":true,"score":'; print: anInteger * 7.25;
			nextPutAll: ',"tags":["alpha","beta","gamma"],"address":{"street":"'; print: anInteger;
			nextPutAll: ' Main Street","city":"Springfield","zip":"'; print: 10000 + anInteger;
			nextPutAll: '"},"friends":['; print: anInteger + 1; nextPut: $,; print: anInteger + 2;
			nextPutAll: '],"note":null}' ]! !

!SJBench class methodsFor: 'inputs'!
records: aCount
	"An array of aCount objects of about 200 bytes"

	^ String streamContents: [ :stream |
		stream nextPut: $[.
		(1 to: aCount)
			do: [ :i | stream nextPutAll: (self record: i) ]
			separatedBy: [ stream nextPut: $, ].
		stream nextPut: $] ]! !

!SJBench methodsFor: 'setup'!
env: aName default: aString

	| value |
	value := OSEnvironment current at: aName ifAbsent: [ nil ].
	^ (value isNil or: [ value isEmpty ]) ifTrue: [ aString ] ifFalse: [ value ]! !

!SJBench methodsFor: 'setup'!
setUpFromEnvironment

	| wasm |
	wasm := Smalltalk vm architectureName beginsWith: 'wasm'.
	out := Stdio stdout.
	corpus := self env: 'SJ_CORPUS' default: '.'.
	files := (self env: 'SJ_FILES' default: '')
		ifEmpty: [ self class defaultFiles ]
		ifNotEmpty: [ :names | names substrings: ',' ].
	series := (self env: 'SJ_SERIES' default: '')
		ifEmpty: [ self class allSeries ]
		ifNotEmpty: [ :names | names substrings: ',' ].
	checking := self env: 'SJ_CHECK' default: 'all'.
	iterations := (self env: 'SJ_N' default: (wasm ifTrue: [ '5' ] ifFalse: [ '10' ])) asNumber.
	warmup := (self env: 'SJ_WARMUP' default: (wasm ifTrue: [ '1' ] ifFalse: [ '2' ])) asNumber.
	budgetMs := (self env: 'SJ_BUDGET' default: '20000') asNumber.
	minSampleMs := (self env: 'SJ_MIN_SAMPLE' default: '50') asNumber.
	label := self env: 'SJ_LABEL' default: Smalltalk vm interpreterClass asString.
	run := self env: 'SJ_RUN' default: '1'.
	cpu := self env: 'SJ_CPU' default: 'n/a'.
	governor := self env: 'SJ_GOVERNOR' default: 'n/a'.
	taskset := self env: 'SJ_TASKSET' default: 'none'.
	pluginAvailable := [ SimdJSONReader pluginAvailable == true ] on: Error do: [ :e | false ].
	failed := Dictionary new.
	errors := 0! !

!SJBench methodsFor: 'printing'!
line: anArray

	anArray do: [ :each | out nextPutAll: each asString ] separatedBy: [ out nextPut: Character tab ].
	out lf; flush! !

!SJBench methodsFor: 'printing'!
fail: anArray
	"Print an ERROR line"

	errors := errors + 1.
	self line: { 'ERROR'. label } , anArray! !

!SJBench methodsFor: 'printing'!
ms: aNumber

	^ aNumber printShowingDecimalPlaces: 3! !

!SJBench methodsFor: 'printing'!
pathString: aPath
	"A path is nil (the root) or { parent path. key or index }"

	aPath ifNil: [ ^ '$' ].
	^ (self pathString: aPath first) , '/' , aPath last asString! !

!SJBench methodsFor: 'printing'!
pluginString

	| info |
	pluginAvailable ifFalse: [ ^ 'none' ].
	info := SimdJSONReader pluginInfo.
	^ (info at: 2) , ' simdjson ' , (info at: 3) , ' ' , (info at: 4) , ' ' , (info at: 5)! !

!SJBench methodsFor: 'printing'!
readerString
	"The superclass of SimdJSONReader: SimdJSONMapper, never a NeoJSON class"

	^ [ SimdJSONReader superclass name asString ] on: Error do: [ :e | 'none' ]! !

!SJBench methodsFor: 'comparing'!
diffAt: aPath

	diffs := diffs + 1.
	firstDiff ifNil: [ firstDiff := self pathString: aPath ]! !

!SJBench methodsFor: 'comparing'!
policyAt: aPath

	policyDiffs := policyDiffs + 1.
	firstPolicy ifNil: [ firstPolicy := self pathString: aPath ]! !

!SJBench methodsFor: 'comparing'!
compare: neo with: sj at: aPath
	"Count in diffs where sj differs from neo, classes included (but SmallFloat64 and BoxedFloat64,
	 which only the value chooses), and in policyDiffs where it differs only by the class of a
	 number of the same value.  Mapped objects compare by their instance variables"

	neo isDictionary ifTrue: [
		(sj class == neo class and: [ sj size = neo size ]) ifFalse: [ ^ self diffAt: aPath ].
		neo associationsDo: [ :association | | other |
			other := sj associationAt: association key ifAbsent: [ nil ].
			(other notNil and: [ other key class == association key class ])
				ifTrue: [ self compare: association value with: other value at: { aPath. association key } ]
				ifFalse: [ self diffAt: { aPath. association key } ] ].
		^ self ].
	neo isString ifTrue: [
		(sj class == neo class and: [ sj = neo ]) ifFalse: [ self diffAt: aPath ].
		^ self ].
	(neo isCollection and: [ neo isSequenceable ]) ifTrue: [
		(sj class == neo class and: [ sj size = neo size ]) ifFalse: [ ^ self diffAt: aPath ].
		1 to: neo size do: [ :i | self compare: (neo at: i) with: (sj at: i) at: { aPath. i } ].
		^ self ].
	neo isNumber ifTrue: [
		sj isNumber ifFalse: [ ^ self diffAt: aPath ].
		(neo isFloat and: [ sj isFloat ]) ifTrue: [
			(neo = sj and: [ neo signBit = sj signBit ]) ifFalse: [ self diffAt: aPath ].
			^ self ].
		(sj class == neo class and: [ sj = neo ]) ifTrue: [ ^ self ].
		(sj = neo or: [ sj asFloat = neo asFloat ]) ifTrue: [ ^ self policyAt: aPath ].
		^ self diffAt: aPath ].
	(neo class == Point or: [ neo class name beginsWith: 'SJB' ]) ifTrue: [
		sj class == neo class ifFalse: [ ^ self diffAt: aPath ].
		1 to: neo class instSize do: [ :i |
			self compare: (neo instVarAt: i) with: (sj instVarAt: i) at: { aPath. neo class allInstVarNames at: i } ].
		^ self ].
	neo == sj ifFalse: [ self diffAt: aPath ]! !

!SJBench methodsFor: 'comparing'!
check: aName as: aKind against: neo do: aBlock
	"Compare the value of aBlock with neo; print a CHECK line.  A difference, or an error, skips
	 the series that depend on aKind"

	| sj |
	sj := [ aBlock value ] on: Error do: [ :e |
		(failed at: aName ifAbsentPut: [ Set new ]) add: aKind.
		^ self fail: { aName. 'check ' , aKind. e class name , ': ' , e messageText asString } ].
	diffs := policyDiffs := 0.
	firstDiff := firstPolicy := nil.
	self compare: neo with: sj at: nil.
	diffs = 0 ifFalse: [ (failed at: aName ifAbsentPut: [ Set new ]) add: aKind ].
	self line: { 'CHECK'. label. aName. aKind.
		diffs = 0 ifTrue: [ 'equal' ] ifFalse: [ 'DIFFERENT' ].
		'diffs=' , diffs printString. 'policy=' , policyDiffs printString.
		'firstDiff=' , (firstDiff ifNil: [ '-' ]). 'firstPolicy=' , (firstPolicy ifNil: [ '-' ]) }! !

!SJBench methodsFor: 'comparing'!
check: aName as: aKind neo: neoBlock do: aBlock
	"Compare the values of aBlock and neoBlock, unless NeoJSON fails"

	| neo |
	neo := neoBlock on: Error do: [ :e |
		(failed at: aName ifAbsentPut: [ Set new ]) add: aKind.
		^ self fail: { aName. 'check ' , aKind , ' (NeoJSON)'. e class name , ': ' , e messageText asString } ].
	self check: aName as: aKind against: neo do: aBlock! !

!SJBench methodsFor: 'comparing'!
checkCorpus
	"Check every .json file of the corpus (SJ_CHECK=all) or the timed ones (files), from the
	 String, from the bytes and in tree mode"

	| root names |
	(checking = 'none' or: [ self sjWanted not ]) ifTrue: [ ^ self ].
	root := corpus asFileReference.
	names := checking = 'all'
		ifTrue: [ ((root allChildrenMatching: '*.json')
			collect: [ :each | each fullName allButFirst: root fullName size + 1 ]) asSortedCollection asArray ]
		ifFalse: [ files reject: [ :each | (self class generated: each) notNil ] ].
	names do: [ :each | (self input: each) ifNotNil: [ :input | self checkSimple: each input: input ] ].
	Smalltalk garbageCollect! !

!SJBench methodsFor: 'comparing'!
checkSimple: aName input: anArray
	"Check the String, the bytes and the tree mode against NeoJSON's value of the String"

	| neo |
	neo := [ NeoJSONReader fromString: anArray last ] on: Error do: [ :e |
		(failed at: aName ifAbsentPut: [ Set new ]) addAll: #( 'string' 'bytes' 'tree' ).
		^ self fail: { aName. 'check (NeoJSON)'. e class name , ': ' , e messageText asString } ].
	self check: aName as: 'string' against: neo do: [ SimdJSONReader fromString: anArray last ].
	self check: aName as: 'bytes' against: neo do: [ SimdJSONReader fromBytes: anArray first ].
	self check: aName as: 'tree' against: neo do: [
		(SimdJSONReader on: anArray last readStream) nativeDictionaries: false; next ]! !

!SJBench methodsFor: 'comparing'!
checkExtras: aName string: aString
	"The checks of the other series of a timed corpus file"

	(checking = 'none' or: [ self sjWanted not ]) ifTrue: [ ^ self ].
	self check: aName as: 'symbols'
		neo: [ (NeoJSONReader on: aString readStream) propertyNamesAsSymbols: true; next ]
		do: [ (SimdJSONReader on: aString readStream) propertyNamesAsSymbols: true; next ].
	self check: aName as: 'objects'
		neo: [ NeoJSONObject fromString: aString ]
		do: [ self objectsFrom: aString ].
	(SJBenchMapping files includes: aName) ifTrue: [
		self check: aName as: 'map'
			neo: [ (SJBenchMapping configure: (NeoJSONReader on: aString readStream) for: aName) nextAs: #Top ]
			do: [ (SJBenchMapping configure: (SimdJSONReader on: aString readStream) for: aName) nextAs: #Top ].
		self check: aName as: 'map-symbols'
			neo: [ (SJBenchMapping
				configure: ((NeoJSONReader on: aString readStream) propertyNamesAsSymbols: true; yourself)
				for: aName) nextAs: #Top ]
			do: [ (SJBenchMapping
				configure: ((SimdJSONReader on: aString readStream) propertyNamesAsSymbols: true; yourself)
				for: aName) nextAs: #Top ] ].
	Smalltalk garbageCollect! !

!SJBench methodsFor: 'measuring'!
time: aBlock repeat: aCount
	"The ms that aCount runs of aBlock take, by the microsecond clock (of ms resolution in
	 WebAssembly: Emscripten's gettimeofday is Date.now())"

	| start |
	start := Time microsecondClockValue.
	aCount timesRepeat: [ aBlock value ].
	^ (Time microsecondClockValue - start) / 1000.0! !

!SJBench methodsFor: 'measuring'!
gcMs

	^ (Smalltalk vm parameterAt: 8) + (Smalltalk vm parameterAt: 10)! !

!SJBench methodsFor: 'measuring'!
counters
	"The statistics of the reader and the counters of the plugin, as names -> counts"

	| counters info |
	counters := Dictionary new.
	pluginAvailable ifFalse: [ ^ counters ].
	SimdJSONReader statistics keysAndValuesDo: [ :key :value | counters at: key asString put: value ].
	info := SimdJSONReader primInfo.
	(info isArray and: [ info size >= 8 ]) ifTrue: [
		self class pluginCounters withIndexDo: [ :name :i |
			counters at: 'plugin.' , name put: ((info at: 8) at: i) ] ].
	^ counters! !

!SJBench methodsFor: 'measuring'!
counter: aName

	^ self counters at: aName ifAbsent: [ 0 ]! !

!SJBench methodsFor: 'measuring'!
measure: aBlock
	"Answer { the ms of one run in each timed sample. the GC ms during them. runs per sample.
	 the counters before them. the counters after them }: warmup runs; then the runs per sample
	 doubled until a sample takes minSampleMs, so that a clock of ms resolution still measures;
	 then samples, at least 3 and at most iterations, none started once budgetMs is spent"

	| times total gc repeat t before after |
	warmup timesRepeat: [ aBlock value ].
	repeat := 1.
	[ (t := self time: aBlock repeat: repeat) < minSampleMs and: [ repeat < 1000000 ] ]
		whileTrue: [ repeat := repeat * 2 ].
	times := OrderedCollection new.
	total := 0.
	before := self counters.
	gc := self gcMs.
	[ times size < iterations and: [ times size < 3 or: [ total < budgetMs ] ] ] whileTrue: [
		t := self time: aBlock repeat: repeat.
		times add: t / repeat.
		total := total + t ].
	gc := self gcMs - gc.
	after := self counters.
	^ { times asArray. gc. repeat. before. after }! !

!SJBench methodsFor: 'measuring'!
clockResolution
	"The smallest step of the microsecond clock seen, in microseconds"

	| best a b |
	best := SmallInteger maxVal.
	5 timesRepeat: [
		a := Time microsecondClockValue.
		[ (b := Time microsecondClockValue) = a ] whileTrue.
		best := best min: b - a ].
	^ best! !

!SJBench methodsFor: 'measuring'!
sjWanted

	^ pluginAvailable and: [ series anySatisfy: [ :each | each beginsWith: 'sj-' ] ]! !

!SJBench methodsFor: 'measuring'!
series: aSeries file: aName size: aSize do: aBlock

	| check result sorted median |
	(series includes: aSeries) ifFalse: [ ^ self ].
	(aSeries beginsWith: 'sj-') ifTrue: [
		pluginAvailable ifFalse: [ ^ self line: { 'SKIP'. label. aName. aSeries. 'no SimdJSONPlugin' } ].
		check := self class checkOfSeries at: aSeries ifAbsent: [ nil ].
		((failed at: aName ifAbsent: [ #() ]) includes: check)
			ifTrue: [ ^ self line: { 'SKIP'. label. aName. aSeries. 'check ' , check , ' failed' } ] ].
	Smalltalk garbageCollect.
	result := [ self measure: aBlock ] on: Error do: [ :e |
		^ self fail: { aName. aSeries. e class name , ': ' , e messageText asString } ].
	sorted := result first asSortedCollection asArray.
	median := sorted size odd
		ifTrue: [ sorted at: sorted size + 1 // 2 ]
		ifFalse: [ ((sorted at: sorted size // 2) + (sorted at: sorted size // 2 + 1)) / 2 ].
	medians at: aSeries put: median.
	self line: { 'RESULT'. label. aName. aSize. aSeries. 'n=' , sorted size printString.
		self ms: median. self ms: sorted first.
		(aSize / 1.0e6 / (median max: 0.001) * 1000) printShowingDecimalPlaces: 2.
		'gc_ms=' , (result at: 2) printString. 'k=' , (result at: 3) printString }.
	(aSeries beginsWith: 'sj-') ifTrue: [
		self stats: aSeries file: aName runs: sorted size * (result at: 3) from: (result at: 4) to: (result at: 5) ]! !

!SJBench methodsFor: 'measuring'!
stats: aSeries file: aName runs: aCount from: before to: after
	"Print the CSPLIT and STATS lines of the counters of a series; fail on a cache hit or a
	 failed build"

	| delta |
	delta := [ :name | (after at: name ifAbsent: [ 0 ]) - (before at: name ifAbsent: [ 0 ]) ].
	self line: { 'CSPLIT'. label. aName. aSeries.
		'parse_ms=' , (self ms: (delta value: 'plugin.parseNanos') / aCount / 1.0e6).
		'build_ms=' , (self ms: (delta value: 'plugin.buildNanos') / aCount / 1.0e6) }.
	self line: { 'STATS'. label. aName. aSeries. 'runs=' , aCount printString } ,
		(#( 'plugin.calls' 'plugin.parses' 'plugin.builds' 'plugin.cacheHits'
			'plugin.buildFailures' 'plugin.overBudget' 'plugin.measures' 'plugin.gateHits'
			'plugin.enumerations' 'plugin.numberTexts' 'grows' 'tooLarge' 'nativeDictionaries' )
				collect: [ :name | (name copyReplaceAll: 'plugin.' with: '') , '=' , (delta value: name) printString ]).
	(delta value: 'plugin.cacheHits') = 0
		ifFalse: [ self fail: { aName. aSeries. 'cacheHits=' , (delta value: 'plugin.cacheHits') printString } ].
	(delta value: 'plugin.buildFailures') = 0
		ifFalse: [ self fail: { aName. aSeries. 'buildFailures=' , (delta value: 'plugin.buildFailures') printString } ]! !

!SJBench methodsFor: 'running'!
input: aName
	"Answer { the UTF-8 bytes. the String } of a corpus file or a generated input, or nil"

	| string file bytes |
	(string := self class generated: aName) ifNotNil: [ ^ { string utf8Encoded. string } ].
	file := (corpus , '/' , aName) asFileReference.
	file exists ifFalse: [
		self line: { 'SKIP'. label. aName. 'missing in ' , corpus }.
		^ nil ].
	bytes := file binaryReadStreamDo: [ :stream | stream upToEnd ].
	^ { bytes. bytes utf8Decoded }! !

!SJBench methodsFor: 'running'!
primRead: aString
	"The primitive alone: DICT mode, no exact numbers so no fixup, no budget"

	| result |
	result := SimdJSONReader
		primRead: aString from: 1 to: aString size flags: SimdJSONReader flagDict budget: -1
		maxUnits: -1 objectClass: Dictionary associationClass: Association wideStringClass: WideString
		hashSeed: ByteString hash dictLayout: layout.
	(result isArray and: [ (result at: 1) = 0 ])
		ifFalse: [ ^ self error: 'primRead: answered ' , (result isArray
			ifTrue: [ 'status ' , (result at: 1) printString , ' code ' , (result at: 6) printString ]
			ifFalse: [ result printString ]) ].
	^ result! !

!SJBench methodsFor: 'running'!
nextLoop: aReader
	"The values of the input of aReader, one next after the other"

	^ Array streamContents: [ :stream | [ aReader atEnd ] whileFalse: [ stream nextPut: aReader next ] ]! !

!SJBench methodsFor: 'running'!
objectsFrom: aString
	"What NeoJSONObject fromString: answers, read by SimdJSONReader: NeoJSONObjects with Symbol
	 keys, and NeoJSONArrays"

	^ (SimdJSONReader on: aString readStream)
		mapClass: NeoJSONObject;
		listClass: NeoJSONArray;
		propertyNamesAsSymbols: true;
		next! !

!SJBench methodsFor: 'running'!
runCorpusFile: aName bytes: bytes string: string

	| size |
	size := bytes size.
	self checkExtras: aName string: string.
	self series: 'neo-string' file: aName size: size do: [ NeoJSONReader fromString: string ].
	self series: 'sj-string' file: aName size: size do: [ SimdJSONReader fromString: string ].
	self series: 'sj-bytes' file: aName size: size do: [ SimdJSONReader fromBytes: bytes ].
	self series: 'sj-tree' file: aName size: size do: [
		(SimdJSONReader on: string readStream) nativeDictionaries: false; next ].
	self series: 'sj-float' file: aName size: size do: [
		(SimdJSONReader on: string readStream) exactNumbers: false; next ].
	(pluginAvailable and: [ SimdJSONReader nativeDictionariesWork ])
		ifTrue: [ self series: 'sj-prim' file: aName size: size do: [ self primRead: string ] ]
		ifFalse: [ (series includes: 'sj-prim') ifTrue: [
			self line: { 'SKIP'. label. aName. 'sj-prim'. 'no native Dictionaries' } ] ].
	self series: 'neo-symbols' file: aName size: size do: [
		(NeoJSONReader on: string readStream) propertyNamesAsSymbols: true; next ].
	self series: 'sj-symbols' file: aName size: size do: [
		(SimdJSONReader on: string readStream) propertyNamesAsSymbols: true; next ].
	self series: 'neo-objects' file: aName size: size do: [ NeoJSONObject fromString: string ].
	self series: 'sj-objects' file: aName size: size do: [ self objectsFrom: string ].
	(SJBenchMapping files includes: aName) ifTrue: [ self runMapping: aName string: string size: size ].
	self series: 'utf8Encoded' file: aName size: size do: [ string utf8Encoded ].
	self runWrite: aName string: string size: size! !

!SJBench methodsFor: 'running'!
runWrite: aName string: string size: size
	"The value NeoJSONReader reads of the file (Fractions included), written by both
	writers: to a String, plain and pretty, to UTF-8 bytes, and through a
	ZnCharacterWriteStream of UTF-8, as to a file or a socket; MB/s over the input"

	| value |
	(series anySatisfy: [ :each | each includesSubstring: '-write' ]) ifFalse: [ ^ self ].
	value := NeoJSONReader fromString: string.
	(checking = 'none' or: [ self sjWanted not ]) ifFalse: [
		self check: aName as: 'write' neo: [ NeoJSONWriter toString: value ] do: [ SimdJSONWriter toString: value ].
		self check: aName as: 'write-pretty'
			neo: [ NeoJSONWriter toStringPretty: value ] do: [ SimdJSONWriter toStringPretty: value ] ].
	self series: 'neo-write' file: aName size: size do: [ NeoJSONWriter toString: value ].
	self series: 'sj-write' file: aName size: size do: [ SimdJSONWriter toString: value ].
	self series: 'sj-write-bytes' file: aName size: size do: [ SimdJSONWriter toBytes: value ].
	self series: 'neo-write-pretty' file: aName size: size do: [ NeoJSONWriter toStringPretty: value ].
	self series: 'sj-write-pretty' file: aName size: size do: [ SimdJSONWriter toStringPretty: value ].
	self series: 'neo-write-utf8' file: aName size: size do: [ self utf8: value with: NeoJSONWriter ].
	self series: 'sj-write-utf8' file: aName size: size do: [ self utf8: value with: SimdJSONWriter ].
	value := nil.
	Smalltalk garbageCollect! !

!SJBench methodsFor: 'running'!
utf8: value with: aWriterClass
	"The bytes that aWriterClass writes of value through a ZnCharacterWriteStream of UTF-8"

	| bytes |
	bytes := WriteStream on: (ByteArray new: 65536).
	(aWriterClass on: (ZnCharacterWriteStream on: bytes)) nextPut: value.
	^ bytes contents! !

!SJBench methodsFor: 'running'!
runMapping: aName string: string size: size

	self series: 'neo-map' file: aName size: size do: [
		(SJBenchMapping configure: (NeoJSONReader on: string readStream) for: aName) nextAs: #Top ].
	self series: 'sj-map' file: aName size: size do: [
		(SJBenchMapping configure: (SimdJSONReader on: string readStream) for: aName) nextAs: #Top ].
	self series: 'neo-map-symbols' file: aName size: size do: [
		(SJBenchMapping
			configure: ((NeoJSONReader on: string readStream) propertyNamesAsSymbols: true; yourself)
			for: aName) nextAs: #Top ].
	self series: 'sj-map-symbols' file: aName size: size do: [
		(SJBenchMapping
			configure: ((SimdJSONReader on: string readStream) propertyNamesAsSymbols: true; yourself)
			for: aName) nextAs: #Top ]! !

!SJBench methodsFor: 'running'!
runNdjson: aName bytes: bytes string: string

	(checking ~= 'none' and: [ self sjWanted ]) ifTrue: [
		self check: aName as: 'ndjson-upToEnd'
			neo: [ (NeoJSONReader on: string readStream) upToEnd ]
			do: [ (SimdJSONReader on: string readStream) upToEnd ].
		self check: aName as: 'ndjson-next'
			neo: [ (NeoJSONReader on: string readStream) upToEnd ]
			do: [ self nextLoop: (SimdJSONReader on: string readStream) ] ].
	self series: 'neo-ndjson-next' file: aName size: bytes size do: [
		self nextLoop: (NeoJSONReader on: string readStream) ].
	self series: 'sj-ndjson-next' file: aName size: bytes size do: [
		self nextLoop: (SimdJSONReader on: string readStream) ].
	self series: 'neo-ndjson-upToEnd' file: aName size: bytes size do: [
		(NeoJSONReader on: string readStream) upToEnd ].
	self series: 'sj-ndjson-upToEnd' file: aName size: bytes size do: [
		(SimdJSONReader on: string readStream) upToEnd ]! !

!SJBench methodsFor: 'running'!
runSmall: aName bytes: bytes string: string

	(checking ~= 'none' and: [ self sjWanted ]) ifTrue: [
		self checkSimple: aName input: { bytes. string } ].
	self series: 'neo-small' file: aName size: bytes size do: [ NeoJSONReader fromString: string ].
	self series: 'sj-small' file: aName size: bytes size do: [ SimdJSONReader fromString: string ]! !

!SJBench methodsFor: 'running'!
runFile: aName

	| input pairs |
	(input := self input: aName) ifNil: [ ^ self ].
	medians := Dictionary new.
	aName = 'ndjson-20k.ndjson'
		ifTrue: [ self runNdjson: aName bytes: input first string: input last ]
		ifFalse: [
			(aName beginsWith: 'small-')
				ifTrue: [ self runSmall: aName bytes: input first string: input last ]
				ifFalse: [ self runCorpusFile: aName bytes: input first string: input last ] ].
	pairs := self class speedups select: [ :pair | (medians includesKey: pair first) and: [ medians includesKey: pair last ] ].
	pairs ifNotEmpty: [
		self line: { 'SPEEDUP'. label. aName } , (pairs collect: [ :pair |
			pair first , '/' , pair last , '='
				, (((medians at: pair first) / ((medians at: pair last) max: 0.001)) printShowingDecimalPlaces: 2) ]) ].
	input := nil.
	Smalltalk garbageCollect! !

!SJBench methodsFor: 'running'!
runAll
	"Print the INFO line, check the corpus, time the files; answer the number of ERROR lines"

	self line: { 'INFO'. label. 'run=' , run. Smalltalk vm interpreterClass. Smalltalk vm architectureName.
		SystemVersion current version. 'reader=' , self readerString. 'plugin=' , self pluginString.
		'warmup=' , warmup printString. 'N=' , iterations printString.
		'budget_ms=' , budgetMs printString. 'min_sample_ms=' , minSampleMs printString.
		'clock_us=' , self clockResolution printString.
		'cpu=' , cpu. 'governor=' , governor. 'taskset=' , taskset }.
	(pluginAvailable not and: [ series anySatisfy: [ :each | each beginsWith: 'sj-' ] ])
		ifTrue: [ self fail: { '-'. '-'. 'SimdJSONPlugin not available: no sj series' } ].
	pluginAvailable ifTrue: [ layout := SimdJSONReader dictLayout ].
	self checkCorpus.
	files do: [ :each | self runFile: each ].
	^ errors! !
