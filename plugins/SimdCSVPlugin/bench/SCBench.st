"SCBench.st - SimdCSV against NeoCSV on the same texts, with each backend (Pharo 12 to 15, chunk format)

 prepare-image.st files this in, after the groups Core and NeoTests of SimdCSV (the second loads
 NeoCSV), into a copy of an image, natively; the saved image then runs on every VM with
 run-bench.st:
     pharo --headless scbench.image --no-default-preferences st --quit run-bench.st
 run-bench.sh does both.  Environment:
     SC_OPS         comma-separated operations (default: SCBench allOps)
     SC_DATA        comma-separated data (default: SCBench allData)
     SC_SIZES       comma-separated sizes in MB of 1,000,000 bytes (default: 1,10; 1 in WebAssembly)
     SC_BACKENDS    comma-separated backends (default: neo, lines, plugin, smalltalk)
     SC_N           timed samples per series (default 5)
     SC_WARMUP      untimed calls before them (default 1)
     SC_BUDGET      ms per series after which no sample starts, once 3 ran (default 30000)
     SC_MIN_SAMPLE  ms a sample lasts at least: it repeats the call k times (default 20, 50 in
                    WebAssembly, whose clock has a resolution of 1 ms)
     SC_SMALLTALK_MAX  the largest size, in MB, of the smalltalk series (default 1)
     SC_DIR         the directory of the files that are read (default: the current one)
     SC_LARGE       the size in MB of the file of the operation large (default 100; 0 leaves it out;
                    0 in WebAssembly)
     SC_LABEL, SC_RUN  the VM and the run printed in each line
     SC_CPU, SC_GOVERNOR, SC_TASKSET  printed in the INFO line
 SCBench dump: aDirectory writes the data as files, for scbench.cpp.

 Data, generated here, the same on every VM: a header and records, at least the size in bytes of
 UTF-8 and less than a record more.  The random numbers are Park and Miller's, from the same seed.
     mixed     10 columns: id, day, name, city, amount, lat, lon, quantity, code, note; one city in 8
               and one note in 16 is quoted (a comma, a doubled quote), one note in 4 is empty
     ints      8 columns of integers of 1 to 10 digits, no quotes
     quoted    4 columns, all quoted: a word, two words with a comma, a text with doubled quotes, a
               number
     utf8      4 columns: an id, a Latin-1 word, a CJK word, a word
     floats17  4 columns of Floats between 0 and 1 in their shortest text, 16 or 17 digits mostly
     inches    4 columns; in one record in 10 a field has a quote inside (5in as 5 and a quote)
     gtfs      6 columns in the manner of stop_times.txt; the times fall on whole minutes (1,380
               different ones)

 Operations (bytes: the ByteArray of UTF-8; string: the String it decodes to; file: that ByteArray
 in a file).  blocksOf:size: has the call of each backend.
     count       the records of bytes                       no NeoCSV: its do: of string, counted
     index       the index of bytes in one call, into a WordArray that is there (plugin only)
     indexLines  the same with the count of the lines, as the reader asks for it (plugin only)
     rows        bytes to Arrays of Strings                 string to Arrays of Strings
     rowsString  string to Arrays of Strings                the same series of NeoCSV as rows
     rowsDo      each row of bytes to an empty block        do: of string
     columns3    3 columns of mixed by name, as Strings     the 7 others ignored, of string
     typed       rows with integers and Floats, of bytes    addIntegerField, addFloatField, of string
     floats3     3 columns of mixed as Arrays of Floats, the table made of bytes
                                                            3 addFloatField, then transposed
     floats3build  the making of the table alone, and
     floats3rows   the building of the rows of 3 Floats alone, not transposed (plugin only)
     float64     one column of the table as a Float64Array (the table is there)
     table       the table of bytes
     fileDo      each row of file to an empty block         do: of file readStream
     writer      rows to a String, quoted (mixed)           NeoCSVWriter, the same
     writerOptional  the same, quoted where it is needed
     gtfsFile    gtfs records, instances of SCBenchStopTime, from file
     gtfsFileCached  the same with cached: true
     gtfsStream  the same as gtfsFile from file readStream (NeoCSV: the series of all four)
     gtfsStreamCached
     large       each row of a file of SC_LARGE MB (mixed) to an empty block: the time, the full
                 garbage collections, what old space grew by, and what is retained (a LARGE line)
 Backends: neo (NeoCSV, the baseline), lines (aString lines, each split at the separator: rows of
 data without quotes only, wrong on the others, where it is left out), plugin (SimdCSV on the
 SimdCSVPlugin), smalltalk (SimdCSV with the test hook #pluginMissing).

 Checks.  Before a series is timed, its result is compared with that of NeoCSV (the Floats up to 1
 part in 10^12, since NeoCSV does not round them exactly); a series whose result differs is not
 timed (CHECK DIFFERENT), and summarize.py fails on it.

 Series.  A full garbage collection, SC_WARMUP calls, then the calls of a sample are k calls, k
 doubled until a sample lasts SC_MIN_SAMPLE ms; then SC_N samples, each after a full garbage
 collection.  Printed: the ns of one call (the median of the samples), MB/s over the bytes of
 UTF-8, the full garbage collections during the samples (vm parameter 7), the fastest and the
 slowest sample, the samples and k.

 Lines are tab-separated, ASCII only:
     INFO     label run=R interpreter architecture version plugin=... settings
     CHECK    label op data bytes backend equal|DIFFERENT
     RESULT   label op data bytes backend ns MBps fullGCs min=NS max=NS n=N k=K
     LARGE    label bytes backend ms MBps fullGCs scavenges grown=BYTES retained=BYTES
     SKIP     label op data bytes backend reason
     ERROR    label op data bytes backend what
 summarize.py reads them.  One method per methodsFor: section (Pharo 15 file-in rules)."

Object subclass: #SCBenchStopTime
	instanceVariableNames: 'tripId arrivalTime departureTime stopId sequence traveledDistance'
	classVariableNames: ''
	package: 'SCBench'!

SCBenchStopTime comment: 'I am a record of the series `gtfs` of `SCBench`: a line of a `stop_times.txt`, with a setter for each field.'!


!SCBenchStopTime methodsFor: 'accessing'!
tripId: anObject

	tripId := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
arrivalTime: anObject

	arrivalTime := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
departureTime: anObject

	departureTime := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
stopId: anObject

	stopId := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
sequence: anObject

	sequence := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
traveledDistance: anObject

	traveledDistance := anObject! !

!SCBenchStopTime methodsFor: 'accessing'!
values

	^ { tripId. arrivalTime. departureTime. stopId. sequence. traveledDistance }! !

!SCBenchStopTime methodsFor: 'comparing'!
= anObject

	^ self species = anObject species and: [ self values = anObject values ]! !

!SCBenchStopTime methodsFor: 'comparing'!
hash

	^ tripId hash bitXor: sequence hash! !

Object subclass: #SCBench
	instanceVariableNames: 'out label run cpu governor taskset wasm iterations warmup budgetMs minSampleMs smalltalkMax largeMB ops data sizes backends errors pluginInfo directory seed inputs'
	classVariableNames: ''
	package: 'SCBench'!

SCBench comment: 'I time SimdCSV, NeoCSV and, where they can do it, `lines` and `split:` on texts that I generate, and print a line for each series. The comment at the start of `SCBench.st` has the data, the operations, the backends, the checks, the settings (environment variables) and the format of the lines.

```smalltalk
SCBench run
```'!


!SCBench class methodsFor: 'running'!
run
	"Run the series of the environment; answer the number of ERROR and CHECK DIFFERENT lines"

	^ self new setUpFromEnvironment; runAll! !

!SCBench class methodsFor: 'running'!
dump: aDirectory size: bytes
	"Write the data as files, for scbench.cpp"

	self allData do: [ :each |
		| text file |
		text := self new generate: each bytes: bytes.
		file := aDirectory asFileReference / (each , '.csv').
		file ensureDelete.
		file binaryWriteStreamDo: [ :stream | stream nextPutAll: text utf8Encoded ] ]! !

!SCBench class methodsFor: 'defaults'!
allOps

	^ #( 'count' 'index' 'indexLines' 'rows' 'rowsString' 'rowsDo' 'columns3' 'typed' 'floats3' 'floats3build' 'floats3rows' 'float64' 'table' 'fileDo' 'writer' 'writerOptional' 'gtfsFile' 'gtfsFileCached' 'gtfsStream' 'gtfsStreamCached' )! !

!SCBench class methodsFor: 'defaults'!
allData

	^ #( 'mixed' 'ints' 'quoted' 'utf8' 'floats17' 'inches' 'gtfs' )! !

!SCBench class methodsFor: 'defaults'!
allBackends

	^ #( 'neo' 'lines' 'plugin' 'smalltalk' )! !

!SCBench class methodsFor: 'defaults'!
dataOf: anOp
	"The data of an operation"

	(#( 'count' 'index' 'indexLines' 'rows' ) includes: anOp) ifTrue: [ ^ self allData ].
	(#( 'rowsString' 'rowsDo' ) includes: anOp) ifTrue: [ ^ #( 'mixed' 'utf8' ) ].
	anOp = 'typed' ifTrue: [ ^ #( 'mixed' 'ints' 'floats17' ) ].
	anOp = 'fileDo' ifTrue: [ ^ #( 'mixed' 'ints' 'quoted' 'utf8' ) ].
	(anOp beginsWith: 'gtfs') ifTrue: [ ^ #( 'gtfs' ) ].
	^ #( 'mixed' )! !

!SCBench class methodsFor: 'defaults'!
headers

	^ Dictionary newFromPairs: {
		'mixed'. 'id,day,name,city,amount,lat,lon,quantity,code,note'.
		'ints'. 'a,b,c,d,e,f,g,h'.
		'quoted'. '"word","pair","text","number"'.
		'utf8'. 'id,latin,cjk,word'.
		'floats17'. 'a,b,c,d'.
		'inches'. 'id,item,size,price'.
		'gtfs'. 'trip_id,arrival_time,departure_time,stop_id,stop_sequence,shape_dist_traveled' }! !

!SCBench methodsFor: 'generating'!
random
	"Park and Miller"

	^ seed := seed * 16807 \\ 2147483647! !

!SCBench methodsFor: 'generating'!
word: aStream
	"4 to 11 letters"

	4 + (self random \\ 8) timesRepeat: [ aStream nextPut: (Character value: 97 + (self random // 7 \\ 26)) ]! !

!SCBench methodsFor: 'generating'!
decimal: digits below: anInteger on: aStream
	"A decimal of that many fraction digits, less than anInteger"

	| fraction |
	fraction := (self random // 3 \\ (10 raisedTo: digits)) printString.
	aStream print: self random // 5 \\ anInteger; nextPut: $..
	digits - fraction size timesRepeat: [ aStream nextPut: $0 ].
	aStream nextPutAll: fraction! !

!SCBench methodsFor: 'generating'!
two: anInteger on: aStream

	anInteger < 10 ifTrue: [ aStream nextPut: $0 ].
	aStream print: anInteger! !

!SCBench methodsFor: 'generating'!
mixed: i on: s

	s print: i; nextPutAll: ',2026-'.
	self two: self random \\ 12 + 1 on: s.
	s nextPut: $-.
	self two: self random \\ 28 + 1 on: s.
	s nextPut: $,.
	self word: s.
	s nextPut: $,.
	i \\ 8 = 0
		ifTrue: [ s nextPut: $". self word: s. s nextPutAll: ', '. self word: s. s nextPut: $" ]
		ifFalse: [ self word: s ].
	s nextPut: $,.
	self decimal: 2 below: 10000 on: s.
	s nextPutAll: ',43.'; print: 10000 + (self random \\ 90000).
	s nextPutAll: ',11.'; print: 10000 + (self random \\ 90000).
	s nextPut: $,; print: self random \\ 1000; nextPutAll: ',K'; print: 1000 + (self random \\ 9000); nextPut: $,.
	i \\ 4 = 0 ifTrue: [ ^ self ].
	i \\ 16 = 1
		ifTrue: [ s nextPutAll: '"the ""'. self word: s. s nextPutAll: '"" one"' ]
		ifFalse: [ self word: s ]! !

!SCBench methodsFor: 'generating'!
ints: i on: s

	1 to: 8 do: [ :k |
		k > 1 ifTrue: [ s nextPut: $, ].
		s print: self random \\ (10 raisedTo: (self random // 11 \\ 10) + 1) ]! !

!SCBench methodsFor: 'generating'!
quoted: i on: s

	s nextPut: $".
	self word: s.
	s nextPutAll: '","'.
	self word: s.
	s nextPutAll: ', '.
	self word: s.
	s nextPutAll: '","a ""'.
	self word: s.
	s nextPutAll: '"" b","'; print: self random; nextPut: $"! !

!SCBench methodsFor: 'generating'!
utf8: i on: s

	s print: i; nextPut: $,.
	self word: s.
	s nextPut: (Character value: 224 + (self random \\ 28)).
	self word: s.
	s nextPut: $,.
	2 + (self random \\ 5) timesRepeat: [ s nextPut: (Character value: 16r4E00 + (self random \\ 16r5000)) ].
	s nextPut: $,.
	self word: s! !

!SCBench methodsFor: 'generating'!
floats17: i on: s
	"Floats of 53 random bits over 2^53"

	1 to: 4 do: [ :k |
		k > 1 ifTrue: [ s nextPut: $, ].
		s print: (self random * 4194304 + (self random \\ 4194304)) asFloat / 9007199254740992.0 ]! !

!SCBench methodsFor: 'generating'!
inches: i on: s

	s print: i; nextPut: $,.
	self word: s.
	s nextPut: $,; print: self random \\ 40 + 1.
	s nextPutAll: (i \\ 10 = 0 ifTrue: [ '" pipe,' ] ifFalse: [ 'in pipe,' ]).
	self decimal: 2 below: 100 on: s! !

!SCBench methodsFor: 'generating'!
gtfs: i on: s
	"Trips of 40 stops, a minute or two apart"

	| stop minute |
	stop := i - 1 \\ 40.
	minute := (i - 1 // 40 * 7 + (stop * 3 // 2)) \\ 1380.
	s nextPutAll: 'T'; print: 100000 + (i - 1 // 40); nextPut: $,.
	2 timesRepeat: [
		self two: minute // 60 on: s.
		s nextPut: $:.
		self two: minute \\ 60 on: s.
		s nextPutAll: ':00,' ].
	s nextPutAll: 'FM'; print: 1000 + (self random \\ 3000); nextPut: $,; print: stop + 1; nextPut: $,.
	self decimal: 12 below: 20000 on: s! !

!SCBench methodsFor: 'generating'!
generate: aName bytes: aSize
	"A String of the datum whose UTF-8 has at least aSize bytes, less than a record more"

	| stream selector i bytes chunk |
	seed := 42.
	selector := (aName , ':on:') asSymbol.
	stream := WriteStream on: (String new: aSize).
	stream nextPutAll: (self class headers at: aName); nextPut: Character lf.
	bytes := stream position.
	i := 0.
	[ bytes < aSize ] whileTrue: [
		chunk := String streamContents: [ :s |
			self perform: selector with: (i := i + 1) with: s.
			s nextPut: Character lf ].
		bytes := bytes + (aName = 'utf8' ifTrue: [ chunk utf8Encoded size ] ifFalse: [ chunk size ]).
		stream nextPutAll: chunk ].
	^ stream contents! !

!SCBench methodsFor: 'generating'!
inputsOf: aName size: aSize
	"A Dictionary: bytes, string, file"

	| string bytes file |
	string := self generate: aName bytes: aSize.
	bytes := string utf8Encoded.
	file := directory / ('scbench-' , aName , '-' , bytes size printString , '.csv').
	file exists ifTrue: [ file delete ].
	file binaryWriteStreamDo: [ :stream | stream nextPutAll: bytes ].
	^ Dictionary newFromPairs: { #string. string. #bytes. bytes. #file. file }! !

!SCBench methodsFor: 'setup'!
env: aName default: aString

	| value |
	value := OSEnvironment current at: aName ifAbsent: [ '' ].
	^ value isEmpty ifTrue: [ aString ] ifFalse: [ value ]! !

!SCBench methodsFor: 'setup'!
list: aName default: anArray

	| value |
	value := self env: aName default: ''.
	^ value isEmpty ifTrue: [ anArray ] ifFalse: [ (value substrings: ',') asArray ]! !

!SCBench methodsFor: 'setup'!
setUpFromEnvironment

	wasm := Smalltalk vm architectureName beginsWith: 'wasm'.
	out := Stdio stdout.
	ops := self list: 'SC_OPS' default: self class allOps.
	data := self list: 'SC_DATA' default: self class allData.
	sizes := (self list: 'SC_SIZES' default: (wasm ifTrue: [ #( '1' ) ] ifFalse: [ #( '1' '10' ) ])) collect: [ :each | each asInteger * 1000000 ].
	backends := self list: 'SC_BACKENDS' default: self class allBackends.
	iterations := (self env: 'SC_N' default: '5') asInteger.
	warmup := (self env: 'SC_WARMUP' default: '1') asInteger.
	budgetMs := (self env: 'SC_BUDGET' default: '30000') asInteger.
	minSampleMs := (self env: 'SC_MIN_SAMPLE' default: (wasm ifTrue: [ '50' ] ifFalse: [ '20' ])) asInteger.
	smalltalkMax := (self env: 'SC_SMALLTALK_MAX' default: '1') asInteger * 1000000.
	largeMB := (self env: 'SC_LARGE' default: (wasm ifTrue: [ '0' ] ifFalse: [ '100' ])) asInteger.
	label := self env: 'SC_LABEL' default: Smalltalk vm interpreterClass asString.
	run := self env: 'SC_RUN' default: '1'.
	cpu := self env: 'SC_CPU' default: 'n/a'.
	governor := self env: 'SC_GOVERNOR' default: 'n/a'.
	taskset := self env: 'SC_TASKSET' default: 'none'.
	directory := (self env: 'SC_DIR' default: FileSystem workingDirectory fullName) asFileReference.
	SimdCSVBackend clearTestHooks.
	pluginInfo := SimdCSV pluginInfo.
	errors := 0! !

!SCBench methodsFor: 'printing'!
line: anArray

	anArray do: [ :each | out nextPutAll: each asString ] separatedBy: [ out nextPut: Character tab ].
	out nextPutAll: OSPlatform current lineEnding; flush! !

!SCBench methodsFor: 'printing'!
fail: anArray
	"Print an ERROR line"

	errors := errors + 1.
	self line: { 'ERROR'. label } , anArray! !

!SCBench methodsFor: 'printing'!
info

	self line: {
		'INFO'. label. 'run=' , run. Smalltalk vm interpreterClass. Smalltalk vm architectureName.
		SystemVersion current shortVersionString.
		'plugin=' , (pluginInfo ifNil: [ 'none' ] ifNotNil: [ (pluginInfo at: 2) , ' ' , (pluginInfo at: 3) , ' ' , (pluginInfo at: 4) , ' ' , (pluginInfo at: 5) ]).
		'neocsv=' , (Smalltalk globals includesKey: #NeoCSVReader) asString.
		'cpu=' , cpu. 'governor=' , governor. 'taskset=' , taskset.
		'n=' , iterations asString. 'warmup=' , warmup asString. 'minSample=' , minSampleMs asString. 'budget=' , budgetMs asString }! !

!SCBench methodsFor: 'blocks'!
typesOf: aName
	"The types of the columns of a typed series"

	aName = 'mixed' ifTrue: [ ^ #( integer string string string float float float integer string string ) ].
	aName = 'ints' ifTrue: [ ^ Array new: 8 withAll: #integer ].
	^ Array new: 4 withAll: #float! !

!SCBench methodsFor: 'blocks'!
declare: types in: aReader
	"The fields of a reader of SimdCSV or of NeoCSV; nil is an ignored one"

	types do: [ :type |
		type = #integer ifTrue: [ aReader addIntegerField ].
		type = #float ifTrue: [ aReader addFloatField ].
		type = #string ifTrue: [ aReader addField ].
		type ifNil: [ aReader addIgnoredField ] ].
	^ aReader! !

!SCBench methodsFor: 'blocks'!
typed: aReader of: aName

	aReader skipHeader.
	^ (self declare: (self typesOf: aName) in: aReader) upToEnd! !

!SCBench methodsFor: 'blocks'!
neoColumns3: aReader type: type
	"name, amount and quantity, or amount, lat and lon, as rows"

	aReader skipHeader.
	^ (self declare: (type = #string
		ifTrue: [ { nil. nil. #string. nil. #string. nil. nil. #string. nil. nil } ]
		ifFalse: [ { nil. nil. nil. nil. #float. #float. #float. nil. nil. nil } ]) in: aReader) upToEnd! !

!SCBench methodsFor: 'blocks'!
transposed: rows

	^ (1 to: 3) asArray collect: [ :column | rows collect: [ :row | row at: column ] ]! !

!SCBench methodsFor: 'blocks'!
converter

	^ [ :text |
	  | parts |
	  parts := text splitOn: $:.
	  Time hour: parts first asInteger minute: parts second asInteger second: parts third asInteger nanoSecond: 0 ]! !

!SCBench methodsFor: 'blocks'!
gtfs: aReader cached: aBoolean
	"The reader of an application, in which only the class of the reader is another; aBoolean: with cached: true"

	aReader skipHeader; recordClass: SCBenchStopTime; addField: #tripId:.
	aBoolean
		ifTrue: [
			aReader
				addField: #arrivalTime: converter: self converter cached: true;
				addField: #departureTime: converter: self converter cached: true ]
		ifFalse: [
			aReader
				addField: #arrivalTime: converter: self converter;
				addField: #departureTime: converter: self converter ].
	^ aReader
		addField: #stopId:;
		addIntegerField: #sequence:;
		addField: #traveledDistance:;
		upToEnd! !

!SCBench methodsFor: 'blocks'!
write: rows with: aClass optional: aBoolean

	^ String streamContents: [ :stream |
		| writer |
		writer := aClass on: stream.
		writer lineEndConvention: #lf.
		aBoolean ifTrue: [ writer fieldWriter: #optionalQuoted ].
		writer nextPutAll: rows ]! !

!SCBench methodsFor: 'blocks'!
neoBlockOf: anOp data: aName
	"What NeoCSV does for the operation, or nil"

	| string file |
	string := inputs at: #string.
	file := inputs at: #file.
	anOp = 'count' ifTrue: [ ^ [ | count | count := 0. (NeoCSVReader on: string readStream) do: [ :row | count := count + 1 ]. count ] ].
	(#( 'rows' 'rowsString' ) includes: anOp) ifTrue: [ ^ [ (NeoCSVReader on: string readStream) upToEnd ] ].
	anOp = 'rowsDo' ifTrue: [ ^ [ (NeoCSVReader on: string readStream) do: [ :row | ]. nil ] ].
	anOp = 'columns3' ifTrue: [ ^ [ self neoColumns3: (NeoCSVReader on: string readStream) type: #string ] ].
	anOp = 'typed' ifTrue: [ ^ [ self typed: (NeoCSVReader on: string readStream) of: aName ] ].
	anOp = 'floats3' ifTrue: [ ^ [ self transposed: (self neoColumns3: (NeoCSVReader on: string readStream) type: #float) ] ].
	anOp = 'fileDo' ifTrue: [ ^ [ | stream | stream := file readStream. [ (NeoCSVReader on: stream) do: [ :row | ] ] ensure: [ stream close ]. nil ] ].
	anOp = 'writer' ifTrue: [ ^ [ self write: (inputs at: #rows) with: NeoCSVWriter optional: false ] ].
	anOp = 'writerOptional' ifTrue: [ ^ [ self write: (inputs at: #rows) with: NeoCSVWriter optional: true ] ].
	(anOp beginsWith: 'gtfs') ifTrue: [ ^ [ | stream | stream := file readStream. [ self gtfs: (NeoCSVReader on: stream) cached: false ] ensure: [ stream close ] ] ].
	^ nil! !

!SCBench methodsFor: 'blocks'!
linesBlockOf: anOp data: aName
	"aString lines, each split, right only where there are no quotes"

	| string |
	(anOp = 'rows' and: [ #( 'ints' 'utf8' 'floats17' ) includes: aName ]) ifFalse: [ ^ nil ].
	string := inputs at: #string.
	^ [ string lines collect: [ :each | $, split: each ] ]! !

!SCBench methodsFor: 'blocks'!
indexBlock: bytes lines: aBoolean
	"The ends of all the fields, into a WordArray that is there"

	| word index |
	word := SimdCSVDialect comma wordFor: bytes final: true lines: aBoolean exact: false.
	index := WordArray new: ((SimdCSVBackend measure: bytes from: 1 to: bytes size dialect: word) at: 3).
	^ [ (SimdCSVBackend index: bytes base: 1 from: 1 to: bytes size dialect: word into: index from: 1) at: 2 ]! !

!SCBench methodsFor: 'blocks'!
rowsOfFloats3: aTable
	"The rows of the 3 Floats, not transposed, as SimdCSVTable>>cellsOf:empty:do: asks for them"

	| columns types |
	types := ByteArray new: aTable columnCount.
	columns := #( 5 6 7 ).
	columns do: [ :each | types at: each put: 3 ].
	aTable recordsFrom: 2 to: aTable rowCount + 1 types: types empty: nil dialect: SimdCSVDialect comma do: [ :rows :count :odd :record | ]! !

!SCBench methodsFor: 'blocks'!
simdBlockOf: anOp data: aName plugin: isPlugin
	"What SimdCSV does for the operation, or nil"

	| bytes string file table |
	bytes := inputs at: #bytes.
	string := inputs at: #string.
	file := inputs at: #file.
	anOp = 'count' ifTrue: [ ^ [ SimdCSV count: bytes ] ].
	anOp = 'index' ifTrue: [ ^ isPlugin ifTrue: [ self indexBlock: bytes lines: false ] ].
	anOp = 'indexLines' ifTrue: [ ^ isPlugin ifTrue: [ self indexBlock: bytes lines: true ] ].
	anOp = 'rows' ifTrue: [ ^ [ SimdCSV rowsFrom: bytes ] ].
	anOp = 'rowsString' ifTrue: [ ^ [ SimdCSV rowsFrom: string ] ].
	anOp = 'rowsDo' ifTrue: [ ^ [ SimdCSV rowsFrom: bytes do: [ :row | ]. nil ] ].
	anOp = 'columns3' ifTrue: [ ^ [ (SimdCSVReader on: bytes) readHeader; selectColumns: #( name amount quantity ); upToEnd ] ].
	anOp = 'typed' ifTrue: [ ^ [ self typed: (SimdCSVReader on: bytes) of: aName ] ].
	anOp = 'floats3' ifTrue: [ ^ [ (SimdCSV tableFrom: bytes) columns: #( amount lat lon ) as: #( float float float ) ] ].
	anOp = 'table' ifTrue: [ ^ [ SimdCSV tableFrom: bytes ] ].
	anOp = 'floats3build' ifTrue: [ ^ isPlugin ifTrue: [ [ SimdCSV tableFrom: bytes ] ] ].
	anOp = 'floats3rows' ifTrue: [ table := SimdCSV tableFrom: bytes. ^ isPlugin ifTrue: [ [ self rowsOfFloats3: table ] ] ].
	anOp = 'float64' ifTrue: [ table := SimdCSV tableFrom: bytes. ^ [ table float64ArrayAt: #amount ] ].
	anOp = 'fileDo' ifTrue: [ ^ [ SimdCSV rowsFrom: file do: [ :row | ]. nil ] ].
	anOp = 'writer' ifTrue: [ ^ [ self write: (inputs at: #rows) with: SimdCSVWriter optional: false ] ].
	anOp = 'writerOptional' ifTrue: [ ^ [ self write: (inputs at: #rows) with: SimdCSVWriter optional: true ] ].
	anOp = 'gtfsFile' ifTrue: [ ^ [ SimdCSVReader on: file do: [ :reader | self gtfs: reader cached: false ] ] ].
	anOp = 'gtfsFileCached' ifTrue: [ ^ [ SimdCSVReader on: file do: [ :reader | self gtfs: reader cached: true ] ] ].
	anOp = 'gtfsStream' ifTrue: [ ^ [ SimdCSVReader on: file readStream do: [ :reader | self gtfs: reader cached: false ] ] ].
	anOp = 'gtfsStreamCached' ifTrue: [ ^ [ SimdCSVReader on: file readStream do: [ :reader | self gtfs: reader cached: true ] ] ].
	^ self error: 'no operation ' , anOp! !

!SCBench methodsFor: 'blocks'!
blockOf: anOp data: aName backend: aBackend

	aBackend = 'neo' ifTrue: [ ^ self neoBlockOf: anOp data: aName ].
	aBackend = 'lines' ifTrue: [ ^ self linesBlockOf: anOp data: aName ].
	SimdCSVBackend testHook: #pluginMissing value: (aBackend = 'smalltalk' ifTrue: [ true ]).
	^ self simdBlockOf: anOp data: aName plugin: aBackend = 'plugin'! !

!SCBench methodsFor: 'checking'!
is: a like: b
	"Equal, but for the last bits of Floats"

	(a isFloat and: [ b isNumber ]) ifTrue: [ ^ (a - b) abs <= (b abs * 1.0e-12) ].
	(a isArray and: [ b isArray ]) ifTrue: [
		a size = b size ifFalse: [ ^ false ].
		1 to: a size do: [ :i | (self is: (a at: i) like: (b at: i)) ifFalse: [ ^ false ] ].
		^ true ].
	(a isCollection and: [ b isCollection and: [ a isString not and: [ b isString not ] ] ]) ifTrue: [ ^ self is: a asArray like: b asArray ].
	^ a = b! !

!SCBench methodsFor: 'checking'!
check: aBlock op: anOp against: neoBlock
	"Whether the result is that of NeoCSV; where NeoCSV does not do the operation, the result only has to come.
	 NeoCSVWriter puts quotes around an empty field, SimdCSVWriter does not: the rows that are read back count"

	| result |
	result := aBlock value.
	(anOp beginsWith: 'writer') ifTrue: [ ^ (NeoCSVReader on: result readStream) upToEnd = (inputs at: #rows) ].
	neoBlock ifNil: [ ^ true ].
	^ self is: result like: neoBlock value! !

!SCBench methodsFor: 'measuring'!
time: aBlock repeat: aCount
	"The ms that aCount calls of aBlock take, by the microsecond clock"

	| start |
	start := Time microsecondClockValue.
	aCount timesRepeat: [ aBlock value ].
	^ (Time microsecondClockValue - start) / 1000.0! !

!SCBench methodsFor: 'measuring'!
fullGCs

	^ Smalltalk vm parameterAt: 7! !

!SCBench methodsFor: 'measuring'!
usedBytes
	"The bytes of old space that are not free"

	^ (Smalltalk vm parameterAt: 1) - (Smalltalk vm parameterAt: 54)! !

!SCBench methodsFor: 'measuring'!
measure: aBlock
	"Answer { the median ms of one call. the least. the largest. the full GCs during the samples.
	 samples. calls per sample }"

	| repeat times total gcs t |
	Smalltalk garbageCollect.
	warmup timesRepeat: [ aBlock value ].
	repeat := 1.
	[ Smalltalk garbageCollect.
	  (self time: aBlock repeat: repeat) < minSampleMs and: [ repeat < 1000000 ] ] whileTrue: [ repeat := repeat * 2 ].
	times := OrderedCollection new.
	total := 0.
	gcs := 0.
	[ times size < iterations and: [ times size < 3 or: [ total < budgetMs ] ] ] whileTrue: [
		| before |
		Smalltalk garbageCollect.
		before := self fullGCs.
		t := self time: aBlock repeat: repeat.
		gcs := gcs + self fullGCs - before.
		total := total + t.
		times add: t / repeat ].
	times := times asArray sorted.
	^ { times at: times size + 1 // 2. times first. times last. gcs. times size. repeat }! !

!SCBench methodsFor: 'running'!
runOp: anOp data: aName backend: aBackend

	| key block neoBlock answer bytes |
	bytes := (inputs at: #bytes) size.
	key := { anOp. aName. bytes. aBackend }.
	[
	(aBackend = 'plugin' and: [ pluginInfo isNil ]) ifTrue: [ ^ self line: { 'SKIP'. label } , key , #( 'no plugin' ) ].
	(aBackend = 'smalltalk' and: [ bytes > (smalltalkMax + 1000) ]) ifTrue: [ ^ self line: { 'SKIP'. label } , key , #( 'SC_SMALLTALK_MAX' ) ].
	(aBackend = 'smalltalk' and: [ anOp beginsWith: 'writer' ]) ifTrue: [ ^ self line: { 'SKIP'. label } , key , #( 'the writer has one backend' ) ].
	(aBackend = 'neo' and: [ #( 'rowsString' 'gtfsFileCached' 'gtfsStream' 'gtfsStreamCached' ) includes: anOp ]) ifTrue: [ ^ self ].
	block := self blockOf: anOp data: aName backend: aBackend.
	block ifNil: [ ^ self ].
	aBackend = 'neo' ifFalse: [
		neoBlock := self neoBlockOf: anOp data: aName.
		(self check: block op: anOp against: neoBlock)
			ifTrue: [ self line: { 'CHECK'. label } , key , #( 'equal' ) ]
			ifFalse: [
				errors := errors + 1.
				^ self line: { 'CHECK'. label } , key , #( 'DIFFERENT' ) ] ].
	answer := self measure: block.
	self line: { 'RESULT'. label } , key , {
		((answer at: 1) * 1000000) rounded printString.
		(bytes / 1000.0 / (answer at: 1)) printShowingDecimalPlaces: 2.
		answer at: 4.
		'min=' , ((answer at: 2) * 1000000) rounded printString.
		'max=' , ((answer at: 3) * 1000000) rounded printString.
		'n=' , (answer at: 5) printString.
		'k=' , (answer at: 6) printString } ]
		on: Error
		do: [ :error | self fail: key , { error class name , ' ' , (error messageText ifNil: [ '' ]) asString } ]! !

!SCBench methodsFor: 'running'!
runData: aName size: aSize

	| wanted |
	wanted := ops select: [ :each | (self class dataOf: each) includes: aName ].
	wanted ifEmpty: [ ^ self ].
	inputs := self inputsOf: aName size: aSize.
	(wanted anySatisfy: [ :each | each beginsWith: 'writer' ]) ifTrue: [ inputs at: #rows put: (SimdCSV rowsFrom: (inputs at: #bytes)) ].
	wanted do: [ :op | backends do: [ :backend | self runOp: op data: aName backend: backend ] ].
	SimdCSVBackend clearTestHooks.
	(inputs at: #file) ensureDelete.
	inputs := nil! !

!SCBench methodsFor: 'running'!
runLarge: aBackend
	"do: over a file of largeMB MB; the file is made of one text of 10 MB, whose header comes once"

	| text bytes rest file count before grown start ms gcs scavenges retained |
	text := self generate: 'mixed' bytes: 10000000.
	bytes := text utf8Encoded.
	rest := bytes copyFrom: (bytes indexOf: 10) + 1 to: bytes size.
	file := directory / 'scbench-large.csv'.
	file ensureDelete.
	file binaryWriteStreamDo: [ :stream |
		stream nextPutAll: bytes.
		largeMB // 10 - 1 timesRepeat: [ stream nextPutAll: rest ] ].
	text := bytes := rest := nil.
	SimdCSVBackend testHook: #pluginMissing value: (aBackend = 'smalltalk' ifTrue: [ true ]).
	count := 0.
	2 timesRepeat: [
		3 timesRepeat: [ Smalltalk garbageCollect ].
		before := self usedBytes.
		grown := Smalltalk vm parameterAt: 1.
		gcs := self fullGCs.
		scavenges := Smalltalk vm parameterAt: 9.
		start := Time microsecondClockValue.
		count := 0.
		aBackend = 'neo'
			ifTrue: [ | stream | stream := file readStream. [ (NeoCSVReader on: stream) do: [ :row | count := count + 1 ] ] ensure: [ stream close ] ]
			ifFalse: [ SimdCSV rowsFrom: file do: [ :row | count := count + 1 ] ].
		ms := (Time microsecondClockValue - start) / 1000.0.
		gcs := self fullGCs - gcs.
		scavenges := (Smalltalk vm parameterAt: 9) - scavenges.
		grown := (Smalltalk vm parameterAt: 1) - grown.
		3 timesRepeat: [ Smalltalk garbageCollect ].
		retained := self usedBytes - before ].
	self line: {
		'LARGE'. label. file size. aBackend. ms printShowingDecimalPlaces: 1.
		(file size / 1000.0 / ms) printShowingDecimalPlaces: 1.
		gcs. scavenges. 'grown=' , grown printString. 'retained=' , retained printString. 'records=' , count printString }.
	file ensureDelete.
	SimdCSVBackend clearTestHooks! !

!SCBench methodsFor: 'running'!
runAll

	self info.
	sizes do: [ :size | data do: [ :each | self runData: each size: size ] ].
	largeMB > 0 ifTrue: [
		(backends includes: 'plugin') ifTrue: [ pluginInfo ifNotNil: [ self runLarge: 'plugin' ] ].
		(backends includes: 'neo') ifTrue: [ self runLarge: 'neo' ] ].
	^ errors! !
