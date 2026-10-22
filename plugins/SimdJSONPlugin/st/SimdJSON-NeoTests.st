"SimdJSON-NeoTests.st - the tests of SimdJSON.st against NeoJSON, in the package
SimdJSON-NeoTests

Neo mode only: NeoJSON with its tests (load-neojson.st) is in the image, and
SimdJSON.st was filed in after it.

	SimdJSONCompatTests        SimdJSONReader against NeoJSONReader, value for
	                           value: equal values with the same classes of
	                           numbers, strings and collections, the same error
	                           class and message, the same position of the
	                           stream afterwards.  The inputs are NeoJSON's
	                           semantics (its leniency and its errors), random
	                           number tokens, and, when the environment has
	                           SJ_CORPUS (the jsonexamples directory of
	                           simdjson-data), the corpus and the jsonchecker
	                           files next to it.  No RFC 8259 input may need
	                           the fallback.
	SimdJSONNeoTestGenerator   subclasses NeoJSON's reader tests as SJ<name>,
	                           with every method that reads JSON recompiled to
	                           read it with SimdJSONReader; the last chunk of
	                           this file runs it.  SimdJSONNeoTestGenerator
	                           report runs those classes and prints what passed
	                           and how many values NeoJSON read again.

	pharo --headless X.image --no-default-preferences st --save --quit SimdJSON-NeoTests.st
	pharo --headless X.image --no-default-preferences test --fail-on-failure SimdJSON-NeoTests
	pharo --headless X.image --no-default-preferences eval 'SimdJSONNeoTestGenerator report'

The file is ASCII; every method has a methodsFor: section of its own; NeoJSON
classes are named through Smalltalk globals."

"Classes"
(TestCase << #SimdJSONCompatTests tag: 'Compat'; package: 'SimdJSON-NeoTests') install.
(Object << #SimdJSONNeoTestGenerator tag: 'Generator'; package: 'SimdJSON-NeoTests') install.
!

!SimdJSONCompatTests methodsFor: 'running'!
defaultTimeLimit

	^ 10 minutes! !

!SimdJSONCompatTests methodsFor: 'running'!
setUp

	super setUp.
	SimdJSONReader mode == #neo
		ifFalse: [ self fail: 'file in SimdJSON.st again after NeoJSON' ].
	SimdJSONReader nativeDictionariesWork! !

!SimdJSONCompatTests methodsFor: 'accessing'!
neoReaderClass

	^ Smalltalk globals at: #NeoJSONReader! !

!SimdJSONCompatTests methodsFor: 'accessing'!
corpusDirectory
	"The jsonexamples directory of simdjson-data, from SJ_CORPUS, or nil"

	^ (OSEnvironment current at: 'SJ_CORPUS' ifAbsent: [ nil ]) ifNotNil: [ :path | path asFileReference ]! !

!SimdJSONCompatTests methodsFor: 'accessing'!
string: codePoints
	"A String of codePoints, of the narrowest class: the file stays ASCII"

	| string |
	string := (codePoints allSatisfy: [ :each | each < 256 ])
		ifTrue: [ ByteString new: codePoints size ]
		ifFalse: [ WideString new: codePoints size ].
	codePoints doWithIndex: [ :each :index | string at: index put: (Character value: each) ].
	^ string! !

!SimdJSONCompatTests methodsFor: 'accessing'!
semanticInputs
	"NeoJSON's semantics (numbers, strings, keys, white space), the lenient inputs that
	 NeoJSON accepts and simdjson refuses, and the stream cases (several values, trailing
	 content, atEnd): values, leniency and errors"

	| lf ff |
	lf := String with: Character lf.
	ff := String with: (Character value: 12).
	^ #( '{"a":1,"b":[1,2]}' '[1,2,3]' '{}' '[]' '123' '-123' '0' '-0' '-0.0' '0.0' '01' '00' '-01'
		'00123' '-0123' ' 123	' '12345678901234567890123456789' '-12345678901234567890123456789'
		'18446744073709551615' '18446744073709551616' '-9223372036854775808' '-9223372036854775809'
		'1.5' '0.1' '3.141592653589793238462643383279' '1e2' '1E2' '1e+2' '1e-2' '1.0e-2' '1.5e2' '2.5E-3'
		'123e6' '123e-6' '1e308' '1.0e308' '1.0e309' '1e1023' '1e1024' '1e-1074' '1.0e-1074' '1.0e-400'
		'4.9e-324' '1e-1022' '1e-1023' '1e400' '1.7976931348623157e308' '2.2250738585072014e-308'
		'1.' '.5' '-' '+1' ' - ' '1e' '1e+' '0x10' 'NaN' 'Infinity' '-Infinity'
		'"abc"' '"a\"b\\c\/d\b\f\n\r\t"' '"\ud83d\ude00"' '"\ud834\udd1e"' '"\uD83D"' '"\uDE00"'
		'"\udbff"' '"\x"' '"\u12"' '"\uAA"' '"unterminated' ' " ' '"\u00e9\u20ac"'
		'true' 'false' 'null' 'tru' 'nul' 'True' 'nil' 'falsE'
		'' '   ' '[1,]' '[,1]' '{"a":1,}' '{"a" 1}' '{a:1}' '{1:2}' '{"a":1,"a":2}' '[1 2]' '[1,2'
		'{"a":1' '[1,' '{9:true,}' '[}' '{"foo":}' '[1] , [2]' 'zero' '#' '@'
		'1 2' '[1] x' '{"a":1} {"b":2}' 'true false' '"a" "b"' '123abc' '[1]   ' 'true   ' '"x"   '
		'[1] [2' '[1]x' 'x' '  [1]  ' '[1,2,3]true' '{"foo":1}"extra"' '[]"extra"'
		'[1,[2,[3,[4]]]]' '{"x":{"y":{"z":null}}}' '{"b":{"y":1,"x":[{"k":2}]},"a":[1e2,-0.5]}' ) ,
	{ '"a' , lf , 'b"'. '"a' , (String with: (Character value: 0)) , 'b"'. '"a' , (String with: Character tab) , 'b"'.
	  ff , '[1]'. '[1]' , ff. '[1]' , ff , '[2]'. '[' , ff , '1]'. (self string: #( 160 )) , '[1]'.
	  '{"a":1}' , lf , '{"a":2}' , lf , '3'.
	  '{"a":[1,{"b":null}],"c":"' , (self string: #( 233 )) , '"}'.
	  (self string: #( 91 8364 93 )) , ' [2]'.
	  '"' , (self string: #( 233 )) , '" 7'.
	  '"' , (self string: #( 8364 )) , '"  [2] tail'.
	  '["' , (self string: #( 16r1D11E )) , '",{"' , (self string: #( 8364 )) , '":1}]'.
	  (self string: #( 16rFEFF )) , '[1]' }! !

!SimdJSONCompatTests methodsFor: 'accessing'!
validInputs
	"RFC 8259 values that simdjson reads itself: none of them may need NeoJSON again"

	^ #( '{"a":1,"b":[1,2]}' '[1,2,3]' '{}' '[]' '123' '-123' '0' '-0' '-0.0' '0.0'
		'12345678901234567890123456789' '18446744073709551616' '-9223372036854775809'
		'1.5' '0.1' '3.141592653589793238462643383279' '1e2' '1E2' '1e+2' '1e-2' '1.0e-2' '1.5e2' '2.5E-3'
		'123e6' '123e-6' '1e308' '1.0e308' '1.0e-400' '4.9e-324' '1e-1022'
		'"abc"' '"a\"b\\c\/d\b\f\n\r\t"' '"\ud83d\ude00"' '"\u00e9\u20ac"' 'true' 'false' 'null'
		'{"a":1,"a":2}' '[1,[2,[3,[4]]]]' '{"x":{"y":{"z":null}}}' '[1]   ' 'true   ' '  [1]  '
		'1 2' '[1] x' '{"a":1} {"b":2}' 'true false' '"a" "b"' '[1]x' )! !

!SimdJSONCompatTests methodsFor: 'accessing'!
operations
	"What is read from a reader, the stream cases: one value, three values or atEnd, all
	 the values, one value and nothing after it"

	^ {
		#next -> [ :reader | reader next ].
		#next3 -> [ :reader |
			(1 to: 3) collect: [ :i |
				reader atEnd
					ifTrue: [ #atEnd ]
					ifFalse: [ self outcomeOf: [ reader next ] ] ] ].
		#upToEnd -> [ :reader | reader upToEnd ].
		#failIfNotAtEnd -> [ :reader | | value | value := reader next. reader failIfNotAtEnd. value ] }! !

!SimdJSONCompatTests methodsFor: 'accessing'!
configurations
	"How the two readers are set up: NeoJSON's options, and the tree path of SimdJSONReader
	 against NeoJSON's defaults"

	^ {
		#defaults -> { [ :reader | reader ]. [ :reader | reader ] }.
		#tree -> { [ :reader | reader ]. [ :reader | reader nativeDictionaries: false; yourself ] }.
		#ordered -> (Array new: 2 withAll: [ :reader |
			reader mapClass: OrderedDictionary; listClass: OrderedCollection; yourself ]).
		#symbols -> (Array new: 2 withAll: [ :reader | reader propertyNamesAsSymbols: true; yourself ]).
		#small -> (Array new: 2 withAll: [ :reader | reader mapClass: SmallDictionary; yourself ]) }! !

!SimdJSONCompatTests methodsFor: 'comparing'!
outcomeOf: aBlock
	"{ #value. what aBlock answers }, or { #error. the class of its error. its message }"

	^ [ { #value. aBlock value } ]
		on: Error
		do: [ :error | error return: { #error. error class. error messageText } ]! !

!SimdJSONCompatTests methodsFor: 'comparing'!
same: a as: b
	"Equal, with the same classes of numbers, strings and collections all the way down; the
	 keys of an ordered map in the same order"

	a class == b class ifFalse: [ ^ false ].
	a isNumber ifTrue: [ ^ a = b and: [ a isFloat not or: [ a signBit = b signBit ] ] ].
	a isString ifTrue: [ ^ a = b ].
	a isDictionary ifTrue: [
		a size = b size ifFalse: [ ^ false ].
		a class == Dictionary
			ifTrue: [
				a associationsDo: [ :each |
					| other |
					other := b associationAt: each key ifAbsent: [ ^ false ].
					((self same: each key as: other key) and: [ self same: each value as: other value ])
						ifFalse: [ ^ false ] ] ]
			ifFalse: [
				(self same: a keys asArray as: b keys asArray) ifFalse: [ ^ false ].
				a keysAndValuesDo: [ :key :value |
					(self same: value as: (b at: key)) ifFalse: [ ^ false ] ] ].
		^ true ].
	(a isKindOf: SequenceableCollection) ifTrue: [
		a size = b size ifFalse: [ ^ false ].
		a with: b do: [ :x :y | (self same: x as: y) ifFalse: [ ^ false ] ].
		^ true ].
	^ a = b! !

!SimdJSONCompatTests methodsFor: 'comparing'!
differenceOn: input operation: operation configuration: configuration
	"nil, or what differs between NeoJSONReader and SimdJSONReader on input"

	| streams outcomes |
	streams := { input readStream. input readStream }.
	outcomes := (1 to: 2) collect: [ :i |
		self outcomeOf: [
			operation value value: ((configuration value at: i) value:
				(({ self neoReaderClass. SimdJSONReader } at: i) on: (streams at: i))) ] ].
	((self same: outcomes first as: outcomes last)
		and: [ streams first position = streams last position ])
		ifTrue: [ ^ nil ].
	^ String streamContents: [ :stream |
		stream
			print: input; space; nextPutAll: operation key; space; nextPutAll: configuration key;
			nextPutAll: ': neo '; print: outcomes first; nextPutAll: ' at '; print: streams first position;
			nextPutAll: ', sj '; print: outcomes last; nextPutAll: ' at '; print: streams last position ]! !

!SimdJSONCompatTests methodsFor: 'comparing'!
assertNoDifferences: differences

	self
		assert: differences isEmpty
		description: [
			String streamContents: [ :stream |
				stream print: differences size; nextPutAll: ' differences'.
				(differences first: (differences size min: 20)) do: [ :each |
					stream nextPut: Character lf; nextPutAll: each ] ] ]! !

!SimdJSONCompatTests methodsFor: 'helpers'!
log: aString
	"A line of the report on the Transcript"

	Transcript show: 'SimdJSON-NeoTests: ' , aString; cr! !

!SimdJSONCompatTests methodsFor: 'helpers'!
fallbacksDuring: aBlock

	| before |
	before := SimdJSONReader statistics at: #fallbacks.
	aBlock value.
	^ (SimdJSONReader statistics at: #fallbacks) - before! !

!SimdJSONCompatTests methodsFor: 'helpers'!
randomToken: random
	"A number token [-]d+[.d+][(e|E)[+-]d+], with mantissas up to 25 digits and exponents
	 around the edges of Float and of NeoJSON's limits"

	^ String streamContents: [ :stream |
		| digits exponent |
		(random nextInteger: 4) = 1 ifTrue: [ stream nextPut: $- ].
		digits := random nextInteger: 25.
		stream nextPut: (Character digitValue: (digits = 1
			ifTrue: [ (random nextInteger: 10) - 1 ]
			ifFalse: [ random nextInteger: 9 ])).
		digits - 1 timesRepeat: [ stream nextPut: (Character digitValue: (random nextInteger: 10) - 1) ].
		(random nextInteger: 2) = 1 ifTrue: [
			stream nextPut: $..
			(random nextInteger: 25) timesRepeat: [
				stream nextPut: (Character digitValue: (random nextInteger: 10) - 1) ] ].
		(random nextInteger: 2) = 1 ifTrue: [
			stream nextPut: ((random nextInteger: 2) = 1 ifTrue: [ $e ] ifFalse: [ $E ]).
			stream nextPutAll: (#( '' '+' '-' ) at: (random nextInteger: 3)).
			exponent := (random nextInteger: 2) = 1
				ifTrue: [ (#( 1022 1023 1074 308 324 ) at: (random nextInteger: 5)) + (random nextInteger: 7) - 4 ]
				ifFalse: [ (random nextInteger: 400) - 1 ].
			stream print: exponent ] ]! !

!SimdJSONCompatTests methodsFor: 'tests'!
testSemantics
	"Every input, read in every way, with every configuration"

	| differences |
	differences := OrderedCollection new.
	self semanticInputs do: [ :input |
		self operations do: [ :operation |
			self configurations do: [ :configuration |
				(self differenceOn: input operation: operation configuration: configuration)
					ifNotNil: [ :difference | differences add: difference ] ] ] ].
	self assertNoDifferences: differences! !

!SimdJSONCompatTests methodsFor: 'tests'!
testNoFallbackOnValidInput

	| fallbacks |
	fallbacks := OrderedCollection new.
	self validInputs do: [ :input |
		(self fallbacksDuring: [ [ SimdJSONReader fromString: input ] on: Error do: [ :error | error return: nil ] ]) = 0
			ifFalse: [ fallbacks add: input ] ].
	self assert: fallbacks isEmpty description: [ 'NeoJSON read again: ' , fallbacks asArray printString ]! !

!SimdJSONCompatTests methodsFor: 'tests'!
testPositions

	| inputs |
	inputs := { '[1]  [2] tail'. '"' , (self string: #( 8364 )) , '"  [2] tail'. '"' , (self string: #( 233 )) , '"  [2] tail'.
		'1e2 , 3'. '{"a":1}' , (String with: Character lf) , 'x' }.
	inputs do: [ :input |
		| streams readers |
		streams := { input readStream. input readStream }.
		readers := { self neoReaderClass on: streams first. SimdJSONReader on: streams last }.
		self assert: (self same: readers first next as: readers last next).
		self assert: streams first position equals: streams last position.
		self assert: (self same: (self outcomeOf: [ readers first next ]) as: (self outcomeOf: [ readers last next ])).
		self assert: streams first position equals: streams last position.
		self assert: streams first upToEnd equals: streams last upToEnd ]! !

!SimdJSONCompatTests methodsFor: 'tests'!
testMockStream
	"A stream with only next, peek, atEnd: NeoJSON reads it, one value at a time"

	| mock reader |
	mock := (Smalltalk globals at: #NeoJSONMockStream) on: '[1] {"a":2e1} 3' readStream.
	reader := SimdJSONReader on: mock.
	self assert: reader next equals: #( 1 ).
	self assert: (reader next at: 'a') equals: 20.
	self deny: reader atEnd.
	self assert: reader upToEnd equals: #( 3 )! !

!SimdJSONCompatTests methodsFor: 'tests'!
testRandomNumbers
	"S2-num: random number tokens, value and class, or error and message, as NeoJSON reads
	 them (SJ_NUM_TOKENS of them, 100000 by default)"

	| random count differences fallbacks |
	random := Random seed: 20261021.
	count := (OSEnvironment current at: 'SJ_NUM_TOKENS' ifAbsent: [ nil ])
		ifNil: [ 100000 ]
		ifNotNil: [ :string | string asNumber ].
	differences := OrderedCollection new.
	fallbacks := self fallbacksDuring: [
		count timesRepeat: [
			| token neo sj |
			token := self randomToken: random.
			neo := self outcomeOf: [ self neoReaderClass fromString: token ].
			sj := self outcomeOf: [ SimdJSONReader fromString: token ].
			(self same: neo as: sj) ifFalse: [
				differences add: token , ': neo ' , neo printString , ', sj ' , sj printString ] ] ].
	self log: 'random numbers: ' , count printString , ' tokens, ' , differences size printString , ' differences, '
		, fallbacks printString , ' read again by NeoJSON'.
	self assertNoDifferences: differences! !

!SimdJSONCompatTests methodsFor: 'tests'!
testCorpus
	"The 30 files of jsonexamples, as Strings and as UTF-8 bytes: no difference, no fallback"

	| directory differences files |
	directory := self corpusDirectory.
	(directory notNil and: [ directory isDirectory ]) ifFalse: [ ^ self skip: 'set SJ_CORPUS to run' ].
	files := (directory allChildrenMatching: '*.json') asSortedCollection: [ :a :b | a fullName <= b fullName ].
	self assert: files notEmpty.
	differences := OrderedCollection new.
	files do: [ :file |
		| text bytes neo fallbacks millis |
		text := file contents.
		bytes := file binaryReadStreamDo: [ :stream | stream upToEnd ].
		neo := self neoReaderClass fromString: text.
		millis := Time millisecondsToRun: [
			fallbacks := self fallbacksDuring: [
				(self same: neo as: (SimdJSONReader fromString: text))
					ifFalse: [ differences add: file basename , ' (String)' ].
				(self same: neo as: (SimdJSONReader fromBytes: bytes))
					ifFalse: [ differences add: file basename , ' (bytes)' ] ] ].
		fallbacks = 0 ifFalse: [ differences add: file basename , ': ' , fallbacks printString , ' fallbacks' ].
		self log: file basename , ' ' , bytes size printString , ' bytes, ' , fallbacks printString , ' fallbacks, '
			, millis printString , ' ms' ].
	self assertNoDifferences: differences! !

!SimdJSONCompatTests methodsFor: 'tests'!
testJsonChecker
	"The pass and fail files of simdjson-data's jsonchecker, next to SJ_CORPUS: the same
	 outcome as NeoJSON, a value then nothing after it, or the same error"

	| directory differences count |
	directory := self corpusDirectory.
	(directory notNil and: [ (directory parent / 'jsonchecker') isDirectory ])
		ifFalse: [ ^ self skip: 'set SJ_CORPUS to run' ].
	differences := OrderedCollection new.
	count := 0.
	((directory parent / 'jsonchecker') childrenMatching: '*.json') do: [ :file |
		([ file contents ] on: Error do: [ :error | error return: nil ]) ifNotNil: [ :text |
			count := count + 1.
			(self differenceOn: text operation: self operations last configuration: self configurations first)
				ifNotNil: [ :difference | differences add: file basename , ' ' , (difference first: (difference size min: 300)) ] ] ].
	self log: 'jsonchecker: ' , count printString , ' files, ' , differences size printString , ' differences'.
	self assert: count > 0.
	self assertNoDifferences: differences! !

!SimdJSONNeoTestGenerator class methodsFor: 'accessing'!
classNames
	"NeoJSON's test classes that read JSON"

	^ #( #NeoJSONReaderTests #NeoJSONMappingTests #NeoJSONExamplesTests #NeoJSONWriteReadTests
		#NeoJSONWriteReadMockStreamTests #NeoJSONWriteReadAsciiOnlyTests
		#NeoJSONWriteReadPrettyPrintedTests #NeoJSONObjectTests )! !

!SimdJSONNeoTestGenerator class methodsFor: 'accessing'!
generatedClasses

	^ self classNames
		collect: [ :each | Smalltalk globals at: ('SJ' , each) asSymbol ifAbsent: [ nil ] ]
		thenSelect: [ :each | each notNil ]! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
generate
	"Make SJ<name> for each of the classNames, and answer them"

	(self classNames allSatisfy: [ :each | Smalltalk globals includesKey: each ]) ifFalse: [
		Transcript show: 'SimdJSON-NeoTests: Neo-JSON-Tests is not loaded, nothing generated'; cr.
		^ #() ].
	^ self classNames collect: [ :each | self generateFor: (Smalltalk globals at: each) ]! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
generateFor: aClass
	"A subclass of aClass that inherits its tests and reads with SimdJSONReader"

	| class |
	class := (aClass << ('SJ' , aClass name) asSymbol
		tag: 'Generated';
		package: 'SimdJSON-NeoTests') install.
	class class compile: 'shouldInheritSelectors' , String cr , String tab , '^ true' classified: 'testing'.
	(self selectorsToRewriteIn: aClass) do: [ :each |
		class compile: (self rewrite: (aClass lookupSelector: each) sourceCode) classified: 'simdjson' ].
	^ class! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
selectorsToRewriteIn: aClass
	"The selectors of aClass and of its superclasses below TestCase whose method reads JSON
	 with NeoJSON: inherited methods too, so that each class reads with SimdJSONReader"

	| selectors |
	selectors := Set new.
	(aClass withAllSuperclasses copyUpTo: TestCase) do: [ :each | selectors addAll: each selectors ].
	^ (selectors select: [ :each |
		| source |
		source := (aClass lookupSelector: each) sourceCode.
		(self rewrite: source) ~= source ]) asArray sort! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
rewrite: source
	"NeoJSONObject fromString: and the identifier NeoJSONReader, for SimdJSONReader"

	^ self
		replaceIdentifier: 'NeoJSONReader'
		with: 'SimdJSONReader'
		in: (source copyReplaceAll: 'NeoJSONObject fromString:' with: 'SimdJSONReader objectFromString:')! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
replaceIdentifier: old with: new in: source
	"Each occurrence of old that is not a part of a longer identifier"

	^ String streamContents: [ :stream |
		| start index |
		start := 1.
		[ (index := source indexOfSubCollection: old startingAt: start) > 0 ] whileTrue: [
			stream
				nextPutAll: (source copyFrom: start to: index - 1);
				nextPutAll: ((self isIdentifierAt: index size: old size in: source) ifTrue: [ new ] ifFalse: [ old ]).
			start := index + old size ].
		stream nextPutAll: (source copyFrom: start to: source size) ]! !

!SimdJSONNeoTestGenerator class methodsFor: 'generating'!
isIdentifierAt: index size: size in: source

	| isPart |
	isPart := [ :position |
		(position between: 1 and: source size)
			and: [ (source at: position) isAlphaNumeric or: [ (source at: position) = $_ ] ] ].
	^ (isPart value: index - 1) not and: [ (isPart value: index + size) not ]! !

!SimdJSONNeoTestGenerator class methodsFor: 'reporting'!
report
	"Run the generated classes; print, for each, what passed and how many values NeoJSON
	 read again, then answer the total"

	| out run passed |
	out := Stdio stdout.
	run := passed := 0.
	self generatedClasses do: [ :class |
		| result |
		SimdJSONReader resetStatistics.
		result := class buildSuite run.
		out
			nextPutAll: class name; nextPutAll: ': ';
			print: result passedCount; nextPut: $/; print: result runCount;
			nextPutAll: ' passed, fallbacks ';
			print: (SimdJSONReader statistics at: #fallbacks); lf.
		result failures , result errors do: [ :each |
			out nextPutAll: '    failed: '; nextPutAll: each selector; lf ].
		run := run + result runCount.
		passed := passed + result passedCount ].
	out flush.
	^ 'SJ*: ' , passed printString , '/' , run printString , ' passed'! !

"Generate the SJ classes"
Transcript
	show: 'SimdJSON-NeoTests: generated ' , SimdJSONNeoTestGenerator generate size printString , ' classes';
	cr.
!
