"SUBench.st - SimdUTF against Zinc on the same inputs, with each backend (Pharo 12 to 15, chunk format)

 prepare-image.st files this in, after the groups Core and FFI of SimdUTF, into a copy of an image,
 natively; the saved image then runs on every VM with run-bench.st:
     pharo --headless subench.image --no-default-preferences st --quit run-bench.st
 run-bench.sh does both.  Environment:
     SU_OPS         comma-separated operations (default: SUBench allOps)
     SU_DATA        comma-separated data (default: path, ascii, latin, wide)
     SU_SIZES       comma-separated sizes, in bytes of UTF-8 (default: 52,1024,65536,1048576,10485760;
                    without 10485760 in WebAssembly)
     SU_BACKENDS    comma-separated backends (default: zinc, plugin, ffi, smalltalk)
     SU_N           timed samples per series, at most (default 10, 5 in WebAssembly)
     SU_WARMUP      untimed calls before them (default 2, 1 in WebAssembly)
     SU_BUDGET      ms per series after which no sample starts, once 3 ran (default 10000)
     SU_MIN_SAMPLE  ms a sample lasts at least: it repeats the call k times (default 20, 50 in
                    WebAssembly, whose clock has a resolution of 1 ms)
     SU_LABEL, SU_RUN  the VM and the run printed in each line
     SU_CPU, SU_GOVERNOR, SU_TASKSET  printed in the INFO line (the image cannot read them in the
                    browser)

 Data, generated here, the same on every VM: a String whose UTF-8 has exactly the size in bytes.
     path    a 52-byte ASCII path, '/home/user/src/pharo-vm/plugins/SimdUTFPlugin/Foo.st', at the
             size 52 only: the cost of a call
     ascii   characters 32 to 126
     latin   as ascii, but 1 character in 8 is in 224 to 251 (UTF-8 1.125 times the characters)
     wide    a WideString: 1 character in 16 an emoji (U+1F600 to U+1F64F), 1 in 4 of the others a
             CJK ideograph (U+4E00 to U+9DFF), the rest ASCII
 The inputs of a datum and a size are made once and not timed: the String, its UTF-8 bytes, its
 UTF-16LE bytes and the base64 of its UTF-8 bytes (by Zinc).  MB/s is always over the size, the
 bytes of UTF-8, whatever the operation reads or writes.

 Operations (SUBench opsAndBaselines has the Zinc call of each):
     utf8Encode            SimdUTF utf8Encode: s          s utf8Encoded
     utf8Decode            SimdUTF utf8Decode: b          b utf8Decoded
     utf8SizeOf            SimdUTF utf8SizeOf: s          ZnUTF8Encoder new encodedByteCountForString: s
     utf8CharacterCountOf  SimdUTF utf8CharacterCountOf: b   b utf8Decoded size (Pharo 12 has nothing better)
     isValidUTF8           SimdUTF isValidUTF8: b         b utf8Decoded, false on ZnCharacterEncodingError
     isAscii               SimdUTF isAscii: s             s isAsciiString
     utf16LEEncode         SimdUTF utf16LEEncode: s       ZnUTF16Encoder new beLittleEndian encodeString: s
     utf16LEDecode         SimdUTF utf16LEDecode: u       ZnUTF16Encoder new beLittleEndian decodeBytes: u
     base64Encode          SimdUTF base64Encode: b        ZnBase64Encoder new encode: b
     base64Encode76        lines of 76 and CRLF           ZnBase64Encoder new breakLines encode: b
     base64Decode          SimdUTF base64Decode: t        ZnBase64Encoder new decode: t
 Backends: zinc (the baseline), and SimdUTF with the test hook #backend set to SimdUTFPrimitives
 (plugin), SimdUTFLibrary (ffi) or SimdUTFSmalltalk (smalltalk), so that every size goes through it.
 A backend that is missing (no plugin, no library, as in WebAssembly) gets SKIP lines.

 Checks.  Before a series is timed, its result is compared with Zinc's, value and class; a series
 whose result differs is not timed (CHECK DIFFERENT), and summarize.py fails on it.

 Series.  A full garbage collection, SU_WARMUP calls, then the calls of a sample are k calls, k
 doubled until a sample lasts SU_MIN_SAMPLE ms; then up to SU_N samples, each after Smalltalk
 garbageCollectMost, or after a full garbage collection when its k calls leave 8 MiB or more of
 garbage (the plugin allocates its results in the VM, which collects no garbage for them, so that
 in WebAssembly a series of large results would fill the window of old space, and the facade's own
 full garbage collections would be timed); the best sample counts.  Printed: ns of one call, MB/s, the full GCs during the samples (vm parameter 7), the units
 of the source (characters of a String, bytes of a ByteArray), the samples and k.

 Lines are tab-separated, ASCII only:
     INFO     label run=R interpreter architecture version plugin=... library=... settings
     CHECK    label op data size backend equal|DIFFERENT
     RESULT   label op data size backend ns MBps fullGCs units=U n=N k=K
     SKIP     label op data size backend reason
     ERROR    label op data size backend what
 summarize.py reads them.  One method per methodsFor: section (Pharo 15 file-in rules)."

Object subclass: #SUBench
	instanceVariableNames: 'out label run cpu governor taskset wasm iterations warmup budgetMs minSampleMs ops data sizes backends errors pluginInfo libraryAvailable'
	classVariableNames: ''
	package: 'SUBench'!

!SUBench class methodsFor: 'running'!
run
	"Run the series of the environment; answer the number of ERROR and CHECK DIFFERENT lines"

	^ self new setUpFromEnvironment; runAll! !

!SUBench class methodsFor: 'defaults'!
allOps

	^ #( 'utf8Encode' 'utf8Decode' 'utf8SizeOf' 'utf8CharacterCountOf' 'isValidUTF8' 'isAscii'
		'utf16LEEncode' 'utf16LEDecode' 'base64Encode' 'base64Encode76' 'base64Decode' )! !

!SUBench class methodsFor: 'defaults'!
allData

	^ #( 'path' 'ascii' 'latin' 'wide' )! !

!SUBench class methodsFor: 'defaults'!
allSizes

	^ #( 52 1024 65536 1048576 10485760 )! !

!SUBench class methodsFor: 'defaults'!
allBackends

	^ #( 'zinc' 'plugin' 'ffi' 'smalltalk' )! !

!SUBench class methodsFor: 'defaults'!
backendClassNames
	"The class of each SimdUTF backend, by name, set as the test hook #backend"

	^ Dictionary newFromPairs: #( 'plugin' #SimdUTFPrimitives 'ffi' #SimdUTFLibrary 'smalltalk' #SimdUTFSmalltalk )! !

!SUBench class methodsFor: 'inputs'!
path
	"52 bytes of ASCII"

	^ '/home/user/src/pharo-vm/plugins/SimdUTFPlugin/Foo.st'! !

!SUBench class methodsFor: 'inputs'!
utf8Length: aCode

	aCode < 16r80 ifTrue: [ ^ 1 ].
	aCode < 16r800 ifTrue: [ ^ 2 ].
	aCode < 16r10000 ifTrue: [ ^ 3 ].
	^ 4! !

!SUBench class methodsFor: 'inputs'!
code: aName at: anIndex random: anInteger
	"The code point of the character anIndex of the datum aName"

	aName = 'latin' ifTrue: [
		^ anIndex \\ 8 = 0 ifTrue: [ 224 + (anInteger \\ 28) ] ifFalse: [ 32 + (anInteger \\ 95) ] ].
	aName = 'wide' ifTrue: [
		anIndex \\ 16 = 0 ifTrue: [ ^ 16r1F600 + (anInteger \\ 80) ].
		anIndex \\ 4 = 0 ifTrue: [ ^ 16r4E00 + (anInteger \\ 16r5000) ] ].
	^ 32 + (anInteger \\ 95)! !

!SUBench class methodsFor: 'inputs'!
generate: aName bytes: aSize
	"A String of the datum aName whose UTF-8 has exactly aSize bytes; a character that would pass
	 the size is replaced by $a.  The random numbers are Park and Miller's, from the same seed"

	| codes seed bytes index code length string |
	codes := OrderedCollection new: aSize.
	seed := 42.
	bytes := 0.
	index := 0.
	[ bytes < aSize ] whileTrue: [
		index := index + 1.
		seed := seed * 16807 \\ 2147483647.
		code := self code: aName at: index random: seed.
		length := self utf8Length: code.
		bytes + length > aSize ifTrue: [ code := 97. length := 1 ].
		codes add: code.
		bytes := bytes + length ].
	string := aName = 'wide' ifTrue: [ WideString new: codes size ] ifFalse: [ ByteString new: codes size ].
	codes withIndexDo: [ :each :i | string at: i put: (Character value: each) ].
	^ string! !

!SUBench class methodsFor: 'inputs'!
string: aName bytes: aSize
	"A String of the datum aName whose UTF-8 has exactly aSize bytes: blocks of 64 KiB, then the rest"

	| block rest count result at |
	aName = 'path' ifTrue: [ ^ self path copy ].
	aSize <= 65536 ifTrue: [ ^ self generate: aName bytes: aSize ].
	block := self generate: aName bytes: 65536.
	rest := self generate: aName bytes: aSize \\ 65536.
	count := aSize // 65536.
	result := block species new: block size * count + rest size.
	at := 0.
	count timesRepeat: [
		result replaceFrom: at + 1 to: at + block size with: block startingAt: 1.
		at := at + block size ].
	result replaceFrom: at + 1 to: result size with: rest startingAt: 1.
	^ result! !

!SUBench class methodsFor: 'inputs'!
inputs: aName bytes: aSize
	"The inputs of a datum and a size: the String, its UTF-8, its UTF-16LE and the base64 of its
	 UTF-8, made by Zinc"

	| string bytes |
	string := self string: aName bytes: aSize.
	bytes := string utf8Encoded.
	bytes size = aSize ifFalse: [ self error: aName , ' of ' , aSize printString , ' bytes has ' , bytes size printString ].
	^ Dictionary newFromPairs: {
		#string. string.
		#bytes. bytes.
		#utf16. (ZnUTF16Encoder new beLittleEndian; yourself) encodeString: string.
		#base64. ZnBase64Encoder new encode: bytes }! !

!SUBench class methodsFor: 'operations'!
opsAndBaselines
	"For each operation: the input it reads, its SimdUTF block and its Zinc block"

	^ Dictionary newFromPairs: {
		'utf8Encode'. { #string. [ :s | SimdUTF utf8Encode: s ]. [ :s | s utf8Encoded ] }.
		'utf8Decode'. { #bytes. [ :b | SimdUTF utf8Decode: b ]. [ :b | b utf8Decoded ] }.
		'utf8SizeOf'. { #string. [ :s | SimdUTF utf8SizeOf: s ]. [ :s | ZnUTF8Encoder new encodedByteCountForString: s ] }.
		'utf8CharacterCountOf'. { #bytes. [ :b | SimdUTF utf8CharacterCountOf: b ]. [ :b | b utf8Decoded size ] }.
		'isValidUTF8'. { #bytes. [ :b | SimdUTF isValidUTF8: b ].
			[ :b | [ b utf8Decoded. true ] on: ZnCharacterEncodingError do: [ :e | e return: false ] ] }.
		'isAscii'. { #string. [ :s | SimdUTF isAscii: s ]. [ :s | s isAsciiString ] }.
		'utf16LEEncode'. { #string. [ :s | SimdUTF utf16LEEncode: s ].
			[ :s | (ZnUTF16Encoder new beLittleEndian; yourself) encodeString: s ] }.
		'utf16LEDecode'. { #utf16. [ :u | SimdUTF utf16LEDecode: u ].
			[ :u | (ZnUTF16Encoder new beLittleEndian; yourself) decodeBytes: u ] }.
		'base64Encode'. { #bytes. [ :b | SimdUTF base64Encode: b ]. [ :b | ZnBase64Encoder new encode: b ] }.
		'base64Encode76'. { #bytes.
			[ :b | SimdUTF base64Encode: b from: 1 to: b size options: 0 lineLength: 76 lineEnd: 2 ].
			[ :b | ZnBase64Encoder new breakLines; encode: b ] }.
		'base64Decode'. { #base64. [ :t | SimdUTF base64Decode: t ]. [ :t | ZnBase64Encoder new decode: t ] } }! !

!SUBench methodsFor: 'setup'!
env: aName default: aString

	| value |
	value := OSEnvironment current at: aName ifAbsent: [ nil ].
	^ (value isNil or: [ value isEmpty ]) ifTrue: [ aString ] ifFalse: [ value ]! !

!SUBench methodsFor: 'setup'!
list: aName default: anArray

	| value |
	value := self env: aName default: ''.
	^ value isEmpty ifTrue: [ anArray ] ifFalse: [ (value substrings: ',') asArray ]! !

!SUBench methodsFor: 'setup'!
setUpFromEnvironment

	wasm := Smalltalk vm architectureName beginsWith: 'wasm'.
	out := Stdio stdout.
	ops := self list: 'SU_OPS' default: self class allOps.
	data := self list: 'SU_DATA' default: self class allData.
	sizes := (self list: 'SU_SIZES' default: (wasm
		ifTrue: [ self class allSizes copyWithout: 10485760 ]
		ifFalse: [ self class allSizes ])) collect: [ :each | each asNumber ].
	backends := self list: 'SU_BACKENDS' default: self class allBackends.
	iterations := (self env: 'SU_N' default: (wasm ifTrue: [ '5' ] ifFalse: [ '10' ])) asNumber.
	warmup := (self env: 'SU_WARMUP' default: (wasm ifTrue: [ '1' ] ifFalse: [ '2' ])) asNumber.
	budgetMs := (self env: 'SU_BUDGET' default: '10000') asNumber.
	minSampleMs := (self env: 'SU_MIN_SAMPLE' default: (wasm ifTrue: [ '50' ] ifFalse: [ '20' ])) asNumber.
	label := self env: 'SU_LABEL' default: Smalltalk vm interpreterClass asString.
	run := self env: 'SU_RUN' default: '1'.
	cpu := self env: 'SU_CPU' default: 'n/a'.
	governor := self env: 'SU_GOVERNOR' default: 'n/a'.
	taskset := self env: 'SU_TASKSET' default: 'none'.
	SimdUTF testHook: #backend value: nil.
	SimdUTF resetBackend.
	pluginInfo := SimdUTF pluginInfo.
	libraryAvailable := (Smalltalk globals at: #SimdUTFLibrary ifAbsent: [ nil ])
		ifNil: [ false ]
		ifNotNil: [ :library | [ library isAvailable == true ] on: Error do: [ :e | e return: false ] ].
	errors := 0! !

!SUBench methodsFor: 'printing'!
line: anArray

	anArray do: [ :each | out nextPutAll: each asString ] separatedBy: [ out nextPut: Character tab ].
	out nextPutAll: OSPlatform current lineEnding; flush! !

!SUBench methodsFor: 'printing'!
fail: anArray
	"Print an ERROR line"

	errors := errors + 1.
	self line: { 'ERROR'. label } , anArray! !

!SUBench methodsFor: 'printing'!
pluginString

	pluginInfo ifNil: [ ^ 'none' ].
	^ (pluginInfo at: 2) , ' simdutf ' , (pluginInfo at: 3) , ' ' , (pluginInfo at: 4) , ' ' , (pluginInfo at: 5)! !

!SUBench methodsFor: 'printing'!
libraryString

	libraryAvailable ifFalse: [ ^ 'none' ].
	^ [ (Smalltalk globals at: #SimdUTFLibrary) implementationName ] on: Error do: [ :e | e return: 'n/a' ]! !

!SUBench methodsFor: 'measuring'!
time: aBlock repeat: aCount
	"The ms that aCount calls of aBlock take, by the microsecond clock (of ms resolution in
	 WebAssembly)"

	| start |
	start := Time microsecondClockValue.
	aCount timesRepeat: [ aBlock value ].
	^ (Time microsecondClockValue - start) / 1000.0! !

!SUBench methodsFor: 'measuring'!
fullGCs

	^ Smalltalk vm parameterAt: 7! !

!SUBench methodsFor: 'measuring'!
clockResolution
	"The smallest step of the microsecond clock seen, in microseconds"

	| best a b |
	best := SmallInteger maxVal.
	5 timesRepeat: [
		a := Time microsecondClockValue.
		[ (b := Time microsecondClockValue) = a ] whileTrue.
		best := best min: b - a ].
	^ best! !

!SUBench methodsFor: 'measuring'!
collectBefore: aCount calls: aSize
	"A full garbage collection when aCount calls of aSize bytes leave 8 MiB or more of garbage, else
	 a scavenge.  The plugin allocates its results in the VM, which collects no garbage for them:
	 in WebAssembly, where old space cannot grow beyond its window, a series of large results would
	 otherwise fill it, and the samples would time the full garbage collections of the facade
	 (SimdUTF collects when OVER_BUDGET cannot grow the memory)"

	aCount * aSize >= 8388608
		ifTrue: [ Smalltalk garbageCollect ]
		ifFalse: [ Smalltalk garbageCollectMost ]! !

!SUBench methodsFor: 'measuring'!
measure: aBlock size: aSize
	"Answer { the ms of one call in the best sample. the full GCs during the samples. samples.
	 calls per sample }: warmup calls; then the calls per sample doubled until a sample takes
	 minSampleMs; then samples, each after a scavenge (or a full garbage collection, see
	 collectBefore:calls:), at least 3 and at most iterations, none started once budgetMs is spent"

	| repeat t best total count gcs |
	Smalltalk garbageCollect.
	warmup timesRepeat: [ aBlock value ].
	repeat := 1.
	[ self collectBefore: repeat calls: aSize.
	  (t := self time: aBlock repeat: repeat) < minSampleMs and: [ repeat < 1000000 ] ]
		whileTrue: [ repeat := repeat * 2 ].
	best := nil.
	total := 0.
	count := 0.
	gcs := self fullGCs.
	[ count < iterations and: [ count < 3 or: [ total < budgetMs ] ] ] whileTrue: [
		self collectBefore: repeat calls: aSize.
		t := self time: aBlock repeat: repeat.
		best := best ifNil: [ t ] ifNotNil: [ best min: t ].
		total := total + t.
		count := count + 1 ].
	^ { best / repeat. self fullGCs - gcs. count. repeat }! !

!SUBench methodsFor: 'running'!
same: aResult as: aReference

	^ aResult class == aReference class and: [ aResult = aReference ]! !

!SUBench methodsFor: 'running'!
skipReason: aBackend

	aBackend = 'zinc' ifTrue: [ ^ nil ].
	aBackend = 'plugin' ifTrue: [ ^ pluginInfo ifNil: [ 'no SimdUTFPlugin' ] ifNotNil: [ nil ] ].
	aBackend = 'ffi' ifTrue: [ ^ libraryAvailable ifTrue: [ nil ] ifFalse: [ 'no libSimdUTFFFI' ] ].
	aBackend = 'smalltalk' ifTrue: [ ^ nil ].
	^ 'unknown backend'! !

!SUBench methodsFor: 'running'!
use: aBackend
	"Set the test hook #backend for aBackend (none for zinc)"

	SimdUTF testHook: #backend value: (aBackend = 'zinc' ifFalse: [
		Smalltalk globals at: (self class backendClassNames at: aBackend) ])! !

!SUBench methodsFor: 'running'!
series: anOp data: aName size: aSize backend: aBackend input: anInput reference: aReference
	"Check, then time one series"

	| entry block result measured ns units |
	(self skipReason: aBackend) ifNotNil: [ :reason |
		^ self line: { 'SKIP'. label. anOp. aName. aSize. aBackend. reason } ].
	entry := self class opsAndBaselines at: anOp.
	self use: aBackend.
	[ block := aBackend = 'zinc'
		ifTrue: [ [ entry third value: anInput ] ]
		ifFalse: [ [ entry second value: anInput ] ].
	  result := block value.
	  (self same: result as: aReference) ifFalse: [
		errors := errors + 1.
		^ self line: { 'CHECK'. label. anOp. aName. aSize. aBackend. 'DIFFERENT' } ].
	  self line: { 'CHECK'. label. anOp. aName. aSize. aBackend. 'equal' }.
	  result := nil.
	  measured := self measure: block size: aSize ]
		on: Error
		do: [ :e |
			self use: 'zinc'.
			^ self fail: { anOp. aName. aSize. aBackend. e class name , ': ' , e messageText asString } ].
	self use: 'zinc'.
	ns := measured first * 1.0e6.
	units := anInput size.
	self line: { 'RESULT'. label. anOp. aName. aSize. aBackend.
		ns printShowingDecimalPlaces: 1.
		(aSize * 1000.0 / (ns max: 0.001)) printShowingDecimalPlaces: 2.
		measured second.
		'units=' , units printString. 'n=' , measured third printString. 'k=' , measured fourth printString }! !

!SUBench methodsFor: 'running'!
runData: aName size: aSize

	| inputs |
	inputs := [ self class inputs: aName bytes: aSize ] on: Error do: [ :e |
		^ self fail: { '-'. aName. aSize. '-'. 'inputs: ' , e messageText asString } ].
	ops do: [ :op | | entry input reference |
		entry := self class opsAndBaselines at: op ifAbsent: [ nil ].
		entry
			ifNil: [ self fail: { op. aName. aSize. '-'. 'no such operation' } ]
			ifNotNil: [
				input := inputs at: entry first.
				reference := [ entry third value: input ] on: Error do: [ :e |
					self fail: { op. aName. aSize. 'zinc'. 'reference: ' , e messageText asString }.
					nil ].
				reference ifNotNil: [
					backends do: [ :backend |
						self series: op data: aName size: aSize backend: backend input: input reference: reference ] ] ] ].
	inputs := nil.
	Smalltalk garbageCollect! !

!SUBench methodsFor: 'running'!
runAll
	"Print the INFO line and time every series; answer the number of ERROR and CHECK DIFFERENT lines"

	self line: { 'INFO'. label. 'run=' , run. Smalltalk vm interpreterClass. Smalltalk vm architectureName.
		SystemVersion current version. 'plugin=' , self pluginString. 'library=' , self libraryString.
		'warmup=' , warmup printString. 'N=' , iterations printString.
		'budget_ms=' , budgetMs printString. 'min_sample_ms=' , minSampleMs printString.
		'clock_us=' , self clockResolution printString.
		'cpu=' , cpu. 'governor=' , governor. 'taskset=' , taskset }.
	data do: [ :name |
		name = 'path'
			ifTrue: [ (sizes includes: 52) ifTrue: [ self runData: name size: 52 ] ]
			ifFalse: [ sizes do: [ :size | self runData: name size: size ] ] ].
	SimdUTF testHook: #backend value: nil.
	^ errors! !
