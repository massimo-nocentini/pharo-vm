"TSBench.st - TimSort against Pharo's sort on the same data, with each backend (Pharo 12 to 15, chunk format)

 prepare-image.st files this in, after the groups Core and FFI of TimSort, into a copy of an image,
 natively; the saved image then runs on every VM with run-bench.st:
     pharo --headless tsbench.image --no-default-preferences st --quit run-bench.st
 run-bench.sh does both.  Environment:
     TS_CASES       comma-separated cases (default: TSBench allCases)
     TS_SHAPES      comma-separated shapes (default: random, sorted, reversed, fewUnique, nearlySorted)
     TS_SIZES       comma-separated sizes, in elements (default: 1000,10000,100000,1000000; without
                    1000000 in WebAssembly)
     TS_BACKENDS    comma-separated series (default: sort, asSortedCollection, plugin, ffi, smalltalk)
     TS_N           timed samples per series, at most (default 5, 3 in WebAssembly)
     TS_WARMUP      untimed sorts before them (default 1)
     TS_BUDGET      ms per series after which no sample starts, once 3 ran (default 10000)
     TS_MIN_SAMPLE  ms a sample lasts at least: it sorts k copies (default 20, 50 in WebAssembly,
                    whose clock has a resolution of 1 ms)
     TS_LABEL, TS_RUN  the VM and the run printed in each line
     TS_CPU, TS_GOVERNOR, TS_TASKSET  printed in the INFO line (the image cannot read them in the
                    browser)

 Cases, each a collection of n elements made here from Park and Miller's random numbers of the
 same seed, the same on every VM (TSBench cases has the TimSort call and Pharo's of each):
     int      mode a: an Array of SmallIntegers in [0, n)        timSort         sort
     float    mode a: an Array of Floats in [0, 1000)            timSort         sort
     string   mode a: an Array of ByteStrings, the printStrings of integers in [0, 10 n)
                                                                 timSort         sort
     f64      mode b: a Float64Array, as float                   timSort         sort
     i32      mode b: an IntegerArray in [-2^30, 2^30)           timSort         sort
     u8       mode b: a ByteArray                                timSort         sort
     key      mode c: an Array of Associations int -> index      timSortBy: [ :e | e key ]
                                                                 sort: [ :a :b | a key <= b key ]
     block    mode d: an Array of SmallIntegers, as int          timSort: [ :a :b | a <= b ]
                                                                 sort: [ :a :b | a <= b ]
              (the series plugin and ffi: TimSort sort: c with: [ :a :b | a <= b ] backend:
              #plugin or #ffi, the resumable C sort; smalltalk: timSort:, the default, the port)
 Shapes: random; sorted and reversed (the random values, sorted); fewUnique (10 distinct values,
 in random order); nearlySorted (sorted, then n // 100 random pairs swapped).
 Series: sort (Pharo's call above, the baseline), asSortedCollection (Pharo's, for the cases of
 mode a: the time includes making the SortedCollection; not on fewUnique beyond 10^4 elements, where
 its quicksort is quadratic: 1.7 s at 10^5 on Cog, 35 s in WebAssembly), and TimSort's call with the test hook
 #backend set to TimSortPrimitives (plugin), TimSortLibrary (ffi) or TimSortPortBackend
 (smalltalk), so that every size goes through it.  The sorts by a sortBlock (block) name the
 resumable C sort of the plugin or the library explicitly (TimSort sort:with:backend:), and the
 series smalltalk times timSort: itself, whose default is the port on every VM.  A backend that
 is missing (no plugin, no library, as in WebAssembly), or has no resumable sort for block (no
 stack switching, as in WebAssembly), gets SKIP lines.

 Checks.  Before a series is timed its result is compared with Pharo's sort, the same object at
 every index (= for the buffers); a series whose result differs is not timed (CHECK DIFFERENT), and
 summarize.py fails on it.

 Series.  The sorts are in place: a sample sorts k fresh copies of the data, made before it and not
 timed, k doubled until a sample lasts TS_MIN_SAMPLE ms; then up to TS_N samples, each after
 Smalltalk garbageCollectMost; the best and the median sample count.  Printed: ms of one sort,
 best and median, the full GCs during the samples (vm parameter 7), the samples and k.

 Lines are tab-separated, ASCII only:
     INFO     label run=R interpreter architecture version plugin=... library=... settings
     CHECK    label case shape n series equal|DIFFERENT
     RESULT   label case shape n series bestMs medianMs fullGCs n=N k=K
     SKIP     label case shape n series reason
     ERROR    label case shape n series what
 summarize.py reads them.  One method per methodsFor: section (Pharo 15 file-in rules)."

Object subclass: #TSBench
	instanceVariableNames: 'out label run cpu governor taskset wasm iterations warmup budgetMs minSampleMs cases shapes sizes backends errors pluginInfo libraryAvailable'
	classVariableNames: ''
	package: 'TSBench'!

!TSBench class methodsFor: 'running'!
run
	"Run the series of the environment; answer the number of ERROR and CHECK DIFFERENT lines"

	^ self new setUpFromEnvironment; runAll! !

!TSBench class methodsFor: 'defaults'!
allCases

	^ #( 'int' 'float' 'string' 'f64' 'i32' 'u8' 'key' 'block' )! !

!TSBench class methodsFor: 'defaults'!
allShapes

	^ #( 'random' 'sorted' 'reversed' 'fewUnique' 'nearlySorted' )! !

!TSBench class methodsFor: 'defaults'!
allSizes

	^ #( 1000 10000 100000 1000000 )! !

!TSBench class methodsFor: 'defaults'!
allBackends

	^ #( 'sort' 'asSortedCollection' 'plugin' 'ffi' 'smalltalk' )! !

!TSBench class methodsFor: 'defaults'!
backendClassNames
	"The class of each TimSort backend, by name, set as the test hook #backend"

	^ Dictionary newFromPairs: #( 'plugin' #TimSortPrimitives 'ffi' #TimSortLibrary 'smalltalk' #TimSortPortBackend )! !

!TSBench class methodsFor: 'cases'!
cases
	"For each case: the value of a random integer r in [1, 2^31 - 2] (n the size; fewUnique takes
	 r \\ 10 instead), the collection made of the Array of values, TimSort's call, Pharo's call"

	^ Dictionary newFromPairs: {
		'int'. { [ :r :n | r \\ n ]. [ :values | values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'float'. { [ :r :n | r / 2147483647.0 * 1000 ]. [ :values | values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'string'. { [ :r :n | (r \\ (10 * n)) printString ]. [ :values | values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'f64'. { [ :r :n | r / 2147483647.0 * 1000 ]. [ :values | Float64Array withAll: values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'i32'. { [ :r :n | r - 1073741824 ]. [ :values | IntegerArray withAll: values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'u8'. { [ :r :n | r \\ 256 ]. [ :values | ByteArray withAll: values ]. [ :c | c timSort ]. [ :c | c sort ] }.
		'key'. { [ :r :n | r \\ n ]. [ :values | values withIndexCollect: [ :each :i | each -> i ] ].
			[ :c | c timSortBy: [ :e | e key ] ]. [ :c | c sort: [ :a :b | a key <= b key ] ] }.
		'block'. { [ :r :n | r \\ n ]. [ :values | values ]. [ :c | c timSort: [ :a :b | a <= b ] ]. [ :c | c sort: [ :a :b | a <= b ] ] } }! !

!TSBench class methodsFor: 'cases'!
modeACases

	^ #( 'int' 'float' 'string' )! !

!TSBench class methodsFor: 'cases'!
data: aCase shape: aShape size: n
	"The collection of aCase and aShape, of n elements"

	| entry seed values swaps i j t |
	entry := self cases at: aCase.
	seed := 42.
	values := Array new: n.
	1 to: n do: [ :k |
		seed := seed * 16807 \\ 2147483647.
		values at: k put: (aShape = 'fewUnique'
			ifTrue: [ entry first value: seed \\ 10 + 1 value: 10 ]
			ifFalse: [ entry first value: seed value: n ]) ].
	(#( 'sorted' 'reversed' 'nearlySorted' ) includes: aShape) ifTrue: [ values sort ].
	aShape = 'reversed' ifTrue: [ values := values reversed ].
	aShape = 'nearlySorted' ifTrue: [
		swaps := n // 100.
		swaps timesRepeat: [
			seed := seed * 16807 \\ 2147483647.
			i := seed \\ n + 1.
			seed := seed * 16807 \\ 2147483647.
			j := seed \\ n + 1.
			t := values at: i.
			values at: i put: (values at: j).
			values at: j put: t ] ].
	^ entry second value: values! !

!TSBench methodsFor: 'setup'!
env: aName default: aString

	| value |
	value := OSEnvironment current at: aName ifAbsent: [ nil ].
	^ (value isNil or: [ value isEmpty ]) ifTrue: [ aString ] ifFalse: [ value ]! !

!TSBench methodsFor: 'setup'!
list: aName default: anArray

	| value |
	value := self env: aName default: ''.
	^ value isEmpty ifTrue: [ anArray ] ifFalse: [ (value substrings: ',') asArray ]! !

!TSBench methodsFor: 'setup'!
setUpFromEnvironment

	wasm := Smalltalk vm architectureName beginsWith: 'wasm'.
	out := Stdio stdout.
	cases := self list: 'TS_CASES' default: self class allCases.
	shapes := self list: 'TS_SHAPES' default: self class allShapes.
	sizes := (self list: 'TS_SIZES' default: (wasm
		ifTrue: [ self class allSizes copyWithout: 1000000 ]
		ifFalse: [ self class allSizes ])) collect: [ :each | each asNumber ].
	backends := self list: 'TS_BACKENDS' default: self class allBackends.
	iterations := (self env: 'TS_N' default: (wasm ifTrue: [ '3' ] ifFalse: [ '5' ])) asNumber.
	warmup := (self env: 'TS_WARMUP' default: '1') asNumber.
	budgetMs := (self env: 'TS_BUDGET' default: '10000') asNumber.
	minSampleMs := (self env: 'TS_MIN_SAMPLE' default: (wasm ifTrue: [ '50' ] ifFalse: [ '20' ])) asNumber.
	label := self env: 'TS_LABEL' default: Smalltalk vm interpreterClass asString.
	run := self env: 'TS_RUN' default: '1'.
	cpu := self env: 'TS_CPU' default: 'n/a'.
	governor := self env: 'TS_GOVERNOR' default: 'n/a'.
	taskset := self env: 'TS_TASKSET' default: 'none'.
	TimSort testHook: #backend value: nil.
	TimSort resetBackend.
	pluginInfo := TimSort pluginInfo.
	libraryAvailable := (Smalltalk globals at: #TimSortLibrary ifAbsent: [ nil ])
		ifNil: [ false ]
		ifNotNil: [ :library | [ library isAvailable == true ] on: Error do: [ :e | e return: false ] ].
	errors := 0! !

!TSBench methodsFor: 'printing'!
line: anArray

	anArray do: [ :each | out nextPutAll: each asString ] separatedBy: [ out nextPut: Character tab ].
	out nextPutAll: OSPlatform current lineEnding; flush! !

!TSBench methodsFor: 'printing'!
fail: anArray
	"Print an ERROR line"

	errors := errors + 1.
	self line: { 'ERROR'. label } , anArray! !

!TSBench methodsFor: 'printing'!
pluginString

	pluginInfo ifNil: [ ^ 'none' ].
	^ (pluginInfo at: 2) , ' ' , (pluginInfo at: 3) , ' ' , (pluginInfo at: 4) , ' coroutines ' , (pluginInfo at: 5) printString! !

!TSBench methodsFor: 'printing'!
libraryString

	libraryAvailable ifFalse: [ ^ 'none' ].
	^ [ (Smalltalk globals at: #TimSortLibrary) coroutineImplementation ] on: Error do: [ :e | e return: 'n/a' ]! !

!TSBench methodsFor: 'measuring'!
fullGCs

	^ Smalltalk vm parameterAt: 7! !

!TSBench methodsFor: 'measuring'!
clockResolution
	"The smallest step of the microsecond clock seen, in microseconds"

	| best a b |
	best := SmallInteger maxVal.
	5 timesRepeat: [
		a := Time microsecondClockValue.
		[ (b := Time microsecondClockValue) = a ] whileTrue.
		best := best min: b - a ].
	^ best! !

!TSBench methodsFor: 'measuring'!
sample: aBlock on: aCollection copies: aCount
	"The ms that sorting aCount copies of aCollection takes, the copies made before"

	| copies start |
	copies := (1 to: aCount) collect: [ :i | aCollection copy ].
	Smalltalk garbageCollectMost.
	start := Time microsecondClockValue.
	copies do: [ :each | aBlock value: each ].
	^ (Time microsecondClockValue - start) / 1000.0! !

!TSBench methodsFor: 'measuring'!
measure: aBlock on: aCollection
	"Answer { the ms of one sort in the best sample. in the median sample. the full GCs during the
	 samples. samples. sorts per sample }: warmup sorts; then the copies per sample doubled until a
	 sample takes minSampleMs; then samples, at least 3 and at most iterations, none started once
	 budgetMs is spent"

	| repeat t times total gcs sorted |
	Smalltalk garbageCollect.
	warmup timesRepeat: [ aBlock value: aCollection copy ].
	repeat := 1.
	[ (t := self sample: aBlock on: aCollection copies: repeat) < minSampleMs and: [ repeat < 4096 ] ]
		whileTrue: [ repeat := repeat * 2 ].
	times := OrderedCollection new.
	total := 0.
	gcs := self fullGCs.
	[ times size < iterations and: [ times size < 3 or: [ total < budgetMs ] ] ] whileTrue: [
		t := self sample: aBlock on: aCollection copies: repeat.
		times add: t.
		total := total + t ].
	sorted := times asSortedCollection asArray.
	^ { sorted first / repeat. (sorted at: sorted size + 1 // 2) / repeat. self fullGCs - gcs. times size. repeat }! !

!TSBench methodsFor: 'running'!
same: aResult as: aReference

	aResult class == aReference class ifFalse: [ ^ false ].
	aResult size = aReference size ifFalse: [ ^ false ].
	aResult isArray ifFalse: [ ^ aResult = aReference ].
	1 to: aResult size do: [ :i | (aResult at: i) == (aReference at: i) ifFalse: [ ^ false ] ].
	^ true! !

!TSBench methodsFor: 'running'!
skipReason: aBackend case: aCase shape: aShape size: n

	aBackend = 'sort' ifTrue: [ ^ nil ].
	aBackend = 'asSortedCollection' ifTrue: [
		(self class modeACases includes: aCase) ifFalse: [ ^ 'mode a only' ].
		"(SortedCollection sorts by a quicksort, quadratic on few distinct values: 1.7 s at 10^5 on Cog)"
		^ (aShape = 'fewUnique' and: [ n > 10000 ]) ifTrue: [ 'quadratic in Pharo' ] ifFalse: [ nil ] ].
	aBackend = 'plugin' ifTrue: [
		pluginInfo ifNil: [ ^ 'no TimSortPlugin' ].
		^ (aCase = 'block' and: [ (TimSort resumableBackends includes: #plugin) not ]) ifTrue: [ 'no resumable sort' ] ].
	aBackend = 'ffi' ifTrue: [
		libraryAvailable ifFalse: [ ^ 'no libTimSortFFI' ].
		^ (aCase = 'block' and: [ (TimSort resumableBackends includes: #ffi) not ]) ifTrue: [ 'no resumable sort' ] ].
	aBackend = 'smalltalk' ifTrue: [ ^ nil ].
	^ 'unknown series'! !

!TSBench methodsFor: 'running'!
use: aBackend
	"Set the test hook #backend for aBackend (none for Pharo's series)"

	TimSort testHook: #backend value: ((self class backendClassNames includesKey: aBackend) ifTrue: [
		Smalltalk globals at: (self class backendClassNames at: aBackend) ])! !

!TSBench methodsFor: 'running'!
blockFor: aBackend case: aCase
	"What a series does to a copy of the data: sorts it in place, or answers the sorted Array"

	| entry |
	entry := self class cases at: aCase.
	aBackend = 'sort' ifTrue: [ ^ entry fourth ].
	aBackend = 'asSortedCollection' ifTrue: [ ^ [ :c | c asSortedCollection ] ].
	"(the sorts by a sortBlock run in the port by default: the resumable C sorts are named)"
	(aCase = 'block' and: [ aBackend = 'plugin' or: [ aBackend = 'ffi' ] ]) ifTrue: [
		| name |
		name := aBackend asSymbol.
		^ [ :c | TimSort sort: c with: [ :a :b | a <= b ] backend: name ] ].
	^ entry third! !

!TSBench methodsFor: 'running'!
series: aBackend case: aCase shape: aShape size: n data: aCollection reference: aReference
	"Check, then time one series"

	| block copy result measured |
	(self skipReason: aBackend case: aCase shape: aShape size: n) ifNotNil: [ :reason |
		^ self line: { 'SKIP'. label. aCase. aShape. n. aBackend. reason } ].
	self use: aBackend.
	[ block := self blockFor: aBackend case: aCase.
	  copy := aCollection copy.
	  result := block value: copy.
	  "(asSortedCollection is not stable: its equal elements are compared by value)"
	  aBackend = 'asSortedCollection' ifTrue: [ copy := result asArray ].
	  result := nil.
	  (aBackend = 'asSortedCollection' ifTrue: [ copy = aReference ] ifFalse: [ self same: copy as: aReference ]) ifFalse: [
		errors := errors + 1.
		self use: 'sort'.
		^ self line: { 'CHECK'. label. aCase. aShape. n. aBackend. 'DIFFERENT' } ].
	  self line: { 'CHECK'. label. aCase. aShape. n. aBackend. 'equal' }.
	  copy := nil.
	  measured := self measure: block on: aCollection ]
		on: Error
		do: [ :e |
			self use: 'sort'.
			^ self fail: { aCase. aShape. n. aBackend. e class name , ': ' , e messageText asString } ].
	self use: 'sort'.
	self line: { 'RESULT'. label. aCase. aShape. n. aBackend.
		measured first printShowingDecimalPlaces: 4.
		measured second printShowingDecimalPlaces: 4.
		measured third.
		'n=' , measured fourth printString. 'k=' , measured fifth printString }! !

!TSBench methodsFor: 'running'!
runCase: aCase shape: aShape size: n

	| data reference |
	data := [ self class data: aCase shape: aShape size: n ] on: Error do: [ :e |
		^ self fail: { aCase. aShape. n. '-'. 'data: ' , e messageText asString } ].
	reference := [ (self blockFor: 'sort' case: aCase) value: data copy ] on: Error do: [ :e |
		^ self fail: { aCase. aShape. n. 'sort'. 'reference: ' , e messageText asString } ].
	backends do: [ :backend |
		self series: backend case: aCase shape: aShape size: n data: data reference: reference ].
	data := reference := nil.
	Smalltalk garbageCollect! !

!TSBench methodsFor: 'running'!
runAll
	"Print the INFO line and time every series; answer the number of ERROR and CHECK DIFFERENT lines"

	self line: { 'INFO'. label. 'run=' , run. Smalltalk vm interpreterClass. Smalltalk vm architectureName.
		SystemVersion current version. 'plugin=' , self pluginString. 'library=' , self libraryString.
		'warmup=' , warmup printString. 'N=' , iterations printString.
		'budget_ms=' , budgetMs printString. 'min_sample_ms=' , minSampleMs printString.
		'clock_us=' , self clockResolution printString.
		'cpu=' , cpu. 'governor=' , governor. 'taskset=' , taskset }.
	cases do: [ :case |
		(self class cases includesKey: case)
			ifFalse: [ self fail: { case. '-'. '-'. '-'. 'no such case' } ]
			ifTrue: [
				sizes do: [ :n |
					shapes do: [ :shape | self runCase: case shape: shape size: n ] ] ] ].
	TimSort testHook: #backend value: nil.
	^ errors! !
