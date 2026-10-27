"SimdJSON-Tests.st - the SUnit tests of SimdJSON.st, in the package SimdJSON-Tests

They need no NeoJSON, and pass whether NeoJSON is in the image or not:
SimdJSONReader has none of its classes in its hierarchy, and is strict
RFC 8259 (NeoJSON's leniencies raise SimdJSONParseError).
SimdJSON-NeoTests.st compares the reader with NeoJSON itself.

	SimdJSONReaderTests   the reader API: values, numbers, strings, classes,
	                      sources, stream positions, NDJSON, errors
	SimdJSONNativeTests   the Dictionaries built by the plugin against the
	                      tree path, their self-test and the kill switch
	SimdJSONMemoryTests   the budget of primitiveRead, the exact need, and
	                      the grow of the reader
	SimdJSONMappingTests  nextAs: and the mapper protocol of NeoJSON
	SimdJSONWriterTests   SimdJSONWriter: the texts of NeoJSONWriter (numbers,
	                      strings, escapes, pretty printing), streams, bytes,
	                      forwarders, the objects the plugin leaves to the
	                      image (holes), mappings, writers of their own, the
	                      streaming protocol and errors
	SimdJSONSlowTests     a 16 MB document of 2M boxed floats, and a cap of
	                      old space (OutOfMemory, the VM lives on); skipped
	                      unless the environment has SJ_SLOW

File it in after SimdJSON.st, then run the package:

	pharo --headless X.image --no-default-preferences st --save --quit SimdJSON.st
	pharo --headless X.image --no-default-preferences st --save --quit SimdJSON-Tests.st
	pharo --headless X.image --no-default-preferences test --fail-on-failure SimdJSON-Tests

(tests/run-st-tests.sh does all of it on a copy of an image).  The file is
ASCII, and every method has a methodsFor: section of its own (Pharo 15)."

"Classes.  A class that this DoIt creates is named through the globals"
| g |
g := Smalltalk globals.
(TestCase << #SimdJSONTestCase
	slots: { #savedMaxInputUnits };
	tag: 'Support'; package: 'SimdJSON-Tests') install.
(Object << #SimdJSONTestObject
	slots: { #id. #name. #points. #stamp. #tags };
	tag: 'Support'; package: 'SimdJSON-Tests') install.
(Object << #SimdJSONTestStream
	slots: { #stream. #closed };
	tag: 'Support'; package: 'SimdJSON-Tests') install.
(Object << #SimdJSONTestOwnWriter
	slots: { #value };
	tag: 'Support'; package: 'SimdJSON-Tests') install.
(Object << #SimdJSONTestFloatPrinter tag: 'Support'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONReaderTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONNativeTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONMemoryTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONMappingTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONSlowTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
((g at: #SimdJSONTestCase) << #SimdJSONWriterTests tag: 'Tests'; package: 'SimdJSON-Tests') install.
"The methods of an older SimdJSON-Tests.st that are gone"
{ #SimdJSONTestCase -> #( #isNeo #parseErrorClass #mappingNotFoundClass ).
  #SimdJSONReaderTests -> #( #testMode #testObjectFromString ) } do: [ :each |
	each value do: [ :selector |
		((g at: each key) includesSelector: selector) ifTrue: [ (g at: each key) removeSelector: selector ] ] ].
!

!SimdJSONTestStream class methodsFor: 'instance creation'!
on: aString
	"A character stream that is not a ReadStream, with the protocol of NeoJSONMockStream:
	 the reader reads it to its end"

	^ self new setStream: aString readStream! !

!SimdJSONTestStream methodsFor: 'initialization'!
setStream: aReadStream

	stream := aReadStream.
	closed := false! !

!SimdJSONTestStream methodsFor: 'accessing'!
next

	^ stream next! !

!SimdJSONTestStream methodsFor: 'accessing'!
peek

	^ stream peek! !

!SimdJSONTestStream methodsFor: 'accessing'!
peekFor: aCharacter

	^ stream peekFor: aCharacter! !

!SimdJSONTestStream methodsFor: 'accessing'!
collectionSpecies

	^ String! !

!SimdJSONTestStream methodsFor: 'testing'!
atEnd

	^ stream atEnd! !

!SimdJSONTestStream methodsFor: 'testing'!
isClosed

	^ closed! !

!SimdJSONTestStream methodsFor: 'initialize-release'!
close

	closed := true! !

!SimdJSONTestCase class methodsFor: 'testing'!
isAbstract

	^ self == SimdJSONTestCase! !

!SimdJSONTestCase methodsFor: 'running'!
defaultTimeLimit
	"The interpreter of WebAssembly is slow"

	^ 1 minute! !

!SimdJSONTestCase methodsFor: 'running'!
setUp
	"The first read of a session runs the native self-test, which reads too: run it before
	 the test counts anything"

	super setUp.
	savedMaxInputUnits := SimdJSONReader maxInputUnits.
	SimdJSONReader nativeDictionariesWork! !

!SimdJSONTestCase methodsFor: 'running'!
tearDown
	"Undo what a test may have changed in the class of the reader"

	SimdJSONReader
		maxInputUnits: savedMaxInputUnits;
		nativeDictionaries: true.
	#( #budget #pluginMissing #nativeSelfTestFails ) do: [ :each |
		SimdJSONReader testHook: each value: nil ].
	super tearDown! !

!SimdJSONTestCase methodsFor: 'accessing'!
statistic: aSymbol

	^ SimdJSONReader statistics at: aSymbol! !

!SimdJSONTestCase methodsFor: 'accessing'!
lf

	^ String with: Character lf! !

!SimdJSONTestCase methodsFor: 'accessing'!
ff
	"Form feed: white space for NeoJSON (isSeparator), not for RFC 8259"

	^ String with: (Character value: 12)! !

!SimdJSONTestCase methodsFor: 'helpers'!
count: aSymbol during: aBlock
	"How much the statistic aSymbol of SimdJSONReader grew while aBlock ran"

	| before |
	before := self statistic: aSymbol.
	aBlock value.
	^ (self statistic: aSymbol) - before! !

!SimdJSONTestCase methodsFor: 'helpers'!
read: aString

	^ SimdJSONReader fromString: aString! !

!SimdJSONTestCase methodsFor: 'helpers'!
reader: aString

	^ SimdJSONReader on: aString readStream! !

!SimdJSONTestCase methodsFor: 'helpers'!
strictErrorOf: aString
	"The SimdJSONParseError that the reader raises on aString, or nil"

	^ [ (self reader: aString) next. nil ]
		on: SimdJSONParseError
		do: [ :error | error return: error ]! !

!SimdJSONTestCase methodsFor: 'helpers'!
string: codePoints
	"A String of codePoints, of the narrowest class, as the reader answers it: the file
	 stays ASCII"

	| string |
	string := (codePoints allSatisfy: [ :each | each < 256 ])
		ifTrue: [ ByteString new: codePoints size ]
		ifFalse: [ WideString new: codePoints size ].
	codePoints doWithIndex: [ :each :index | string at: index put: (Character value: each) ].
	^ string! !

!SimdJSONTestCase methodsFor: 'helpers'!
prim: source flags: flags budget: budget
	"One call of primitiveRead on all of source, in tree mode"

	^ self prim: source from: 1 to: source size flags: flags budget: budget maxUnits: -1! !

!SimdJSONTestCase methodsFor: 'helpers'!
prim: source from: start to: stop flags: flags budget: budget maxUnits: maxUnits
	"One call of primitiveRead on source from start to stop, in tree mode"

	^ SimdJSONReader
		primRead: source from: start to: stop flags: flags budget: budget maxUnits: maxUnits
		objectClass: SimdJSONFlatMap associationClass: nil wideStringClass: WideString
		hashSeed: ByteString hash dictLayout: 0! !

!SimdJSONTestCase methodsFor: 'helpers'!
primDict: source budget: budget
	"One call of primitiveRead on all of source that builds Dictionaries"

	^ SimdJSONReader
		primRead: source from: 1 to: source size flags: 4 budget: budget maxUnits: -1
		objectClass: Dictionary associationClass: Association wideStringClass: WideString
		hashSeed: ByteString hash dictLayout: SimdJSONReader dictLayout! !

!SimdJSONTestCase methodsFor: 'helpers'!
probeJSON
	"The input of the per-session self-test of the Dictionaries built in C
	 (SimdJSONReader class>>nativeSelfTestJSON): a duplicate key, non ASCII keys, empty
	 containers, number text, and 300 keys (the overflow header)"

	^ String streamContents: [ :stream |
		stream nextPutAll: '{"a":1,"\u00e9":2,"\u20ac":3,"a":4,"e":{},"l":[],"n":1e2,"o":{'.
		0 to: 299 do: [ :i |
			i > 0 ifTrue: [ stream nextPut: $, ].
			stream nextPutAll: '"k'; print: i; nextPutAll: '":'; print: i ].
		stream nextPutAll: '}}' ]! !

!SimdJSONReaderTests methodsFor: 'tests - plugin'!
testStandalone
	"No NeoJSON class in the hierarchy of the reader and of its errors, and none named by a
	 method of the package SimdJSON, whether NeoJSON is in the image or not"

	| names |
	self assert: SimdJSONReader superclass equals: SimdJSONMapper.
	self assert: SimdJSONMapper superclass equals: Object.
	self assert: SimdJSONParseError superclass equals: Error.
	self assert: SimdJSONMappingNotFound superclass equals: Error.
	names := Set new.
	SimdJSONReader package definedClasses do: [ :class |
		class methods , class class methods do: [ :method |
			method literals do: [ :literal |
				(literal isVariableBinding and: [ literal key beginsWith: 'Neo' ])
					ifTrue: [ names add: literal key ].
				(literal isSymbol and: [ literal beginsWith: 'NeoJSON' ])
					ifTrue: [ names add: literal ] ] ] ].
	self assert: names isEmpty description: [ 'NeoJSON named: ' , names asArray printString ]! !

!SimdJSONReaderTests methodsFor: 'tests - plugin'!
testPluginInfo

	| info |
	self assert: SimdJSONReader pluginAvailable.
	info := SimdJSONReader primInfo.
	self assert: info size equals: 8.
	self assert: info first equals: SimdJSONReader protocol.
	self assert: (info at: 2) equals: 'SimdJSONPlugin 1.0'.
	self assert: (info at: 3) equals: '5.0.5'.
	self assert: ((info at: 6) isInteger and: [ (info at: 7) isInteger ]).
	self assert: (info at: 8) size equals: SimdJSONReader pluginStatisticsKeys size.
	self assert: ((info at: 8) allSatisfy: [ :each | each isInteger ]).
	self assert: SimdJSONReader simdjsonVersion equals: '5.0.5'.
	self assert: (#( 'native' 'emscripten' ) includes: SimdJSONReader platform).
	self assert: SimdJSONReader implementationName notEmpty.
	SimdJSONReader platform = 'emscripten'
		ifTrue: [
			self assert: SimdJSONReader implementationName equals: 'fallback'.
			self assert: savedMaxInputUnits equals: 16 * 1024 * 1024 ]
		ifFalse: [ self assert: savedMaxInputUnits isNil ]! !

!SimdJSONReaderTests methodsFor: 'tests - plugin'!
testPluginMissing
	"Without the module, reading signals SimdJSONResourceError"

	SimdJSONReader testHook: #pluginMissing value: true.
	self
		should: [ self read: '[1]' ]
		raise: SimdJSONResourceError
		withExceptionDo: [ :error | self assert: error messageText equals: 'SimdJSONPlugin not available' ]! !

!SimdJSONReaderTests methodsFor: 'tests - values'!
testShapes

	| value |
	value := self read: '{"a":[1,2.5,"x",null,true,false],"b":{}}'.
	self assert: value class equals: Dictionary.
	self assert: value size equals: 2.
	self assert: (value at: 'a') class equals: Array.
	self assert: (value at: 'a') equals: { 1. 2.5. 'x'. nil. true. false }.
	self assert: (value at: 'b') class equals: Dictionary.
	self assert: (value at: 'b') isEmpty! !

!SimdJSONReaderTests methodsFor: 'tests - values'!
testEmptyContainers

	| value |
	self assert: (self read: '{}') equals: Dictionary new.
	self assert: (self read: '{}') class equals: Dictionary.
	self assert: (self read: '[]') equals: #().
	self assert: (self read: '[]') class equals: Array.
	value := self read: '[{},[],{"a":[]},[{}]]'.
	self assert: value first class equals: Dictionary.
	self assert: value second equals: #().
	self assert: ((value at: 3) at: 'a') equals: #().
	self assert: (value at: 4) first class equals: Dictionary.
	value := (self reader: '[{},[]]')
		mapClass: OrderedDictionary;
		listClass: OrderedCollection;
		next.
	self assert: value class equals: OrderedCollection.
	self assert: value first class equals: OrderedDictionary.
	self assert: value second class equals: OrderedCollection! !

!SimdJSONReaderTests methodsFor: 'tests - values'!
testTopLevelScalars

	self assert: (self read: '123') equals: 123.
	self assert: (self read: '"abc"') equals: 'abc'.
	self assert: (self read: 'true') equals: true.
	self assert: (self read: 'false') equals: false.
	self assert: (self read: 'null') isNil.
	self assert: (self read: '  7  ') equals: 7! !

!SimdJSONReaderTests methodsFor: 'tests - values'!
testDuplicateKeys
	"The last value wins, as with at:put:, and keeps the place of the first in an
	 OrderedDictionary"

	| value |
	value := self read: '{"a":1,"a":2}'.
	self assert: value size equals: 1.
	self assert: (value at: 'a') equals: 2.
	value := (self reader: '{"a":1,"a":2}') nativeDictionaries: false; next.
	self assert: value size equals: 1.
	self assert: (value at: 'a') equals: 2.
	value := (self reader: '{"a":1,"b":2,"a":3}') mapClass: OrderedDictionary; next.
	self assert: value keys asArray equals: #( 'a' 'b' ).
	self assert: (value at: 'a') equals: 3! !

!SimdJSONReaderTests methodsFor: 'tests - values'!
testDeepNesting

	| depth value |
	depth := 1000.
	value := self read: (String new: depth withAll: $[) , (String new: depth withAll: $]).
	depth - 1 timesRepeat: [
		self assert: value size equals: 1.
		value := value first ].
	self assert: value equals: #()! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testIntegers
	"SmallIntegers up to 2^60, LargeIntegers beyond, of any size"

	{ 0. 123. -123. SmallInteger maxVal. SmallInteger maxVal + 1. SmallInteger minVal.
	  SmallInteger minVal - 1. (2 raisedTo: 63) - 1. (2 raisedTo: 63) negated.
	  2 raisedTo: 63. (2 raisedTo: 64) - 1. 2 raisedTo: 64. ((2 raisedTo: 63) + 1) negated.
	  123456789012345678901234567890. -123456789012345678901234567890 } do: [ :each |
		| value |
		value := self read: each printString.
		self assert: value equals: each.
		self assert: value class equals: each class ].
	self assert: (self read: '-0') equals: 0.
	self assert: (self read: '-0') class equals: SmallInteger.
	self
		assert: (self read: '[18446744073709551616,-9223372036854775809,1]')
		equals: { 2 raisedTo: 64. ((2 raisedTo: 63) + 1) negated. 1 }! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testFloats

	{ '1.5' -> 1.5. '0.1' -> 0.1. '-2.25' -> -2.25. '123456.789' -> 123456.789.
	  '0.30000000000000004' -> 0.30000000000000004.
	  '3.141592653589793238462643383279' -> Float pi } do: [ :each |
		| value |
		value := self read: each key.
		self assert: value equals: each value.
		self assert: value class equals: each value class ].
	self assert: (self read: '-0.0') equals: Float negativeZero.
	self assert: (self read: '-0.0') signBit equals: 1.
	self assert: (self read: '0.0') signBit equals: 0! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testExponents
	"Exact numbers, as NeoJSON reads them: an exponent without a fraction gives an Integer
	 or a Fraction"

	{ '1e2' -> 100. '1E2' -> 100. '1e+2' -> 100. '1E+2' -> 100. '-1e2' -> -100. '0e0' -> 0.
	  '1e-2' -> (1 / 100). '-25e-1' -> (-5 / 2). '1e308' -> (10 raisedTo: 308).
	  '1.5e2' -> 150.0. '2.5E-3' -> 0.0025. '1.0e-400' -> 0.0. '1.5e300' -> 1.5e300.
	  '1.7976931348623157e308' -> Float fmax.
	  '2.2250738585072014e-308' -> 2.2250738585072014e-308 } do: [ :each |
		| value |
		value := self read: each key.
		self assert: value equals: each value.
		self assert: value class equals: each value class ].
	self
		should: [ self read: '1e-1074' ]
		raise: SimdJSONParseError
		withExceptionDo: [ :error |
			self assert: error messageText equals: 'number exponent too small' ]! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testExactNumbersOff
	"Every number with an exponent is a Float, built by the plugin"

	| value |
	value := (self reader: '[1e2,1e-2,1.5e300,7,1.5]') exactNumbers: false; next.
	self assert: value first equals: 100.0.
	self assert: value first class equals: SmallFloat64.
	self assert: value second equals: 0.01.
	self assert: (value at: 3) equals: 1.5e300.
	self assert: (value at: 3) class equals: BoxedFloat64.
	self assert: (value at: 4) equals: 7.
	self assert: (value at: 5) equals: 1.5.
	self assert: ((self reader: '1e2') exactNumbers: false; next) equals: 100.0.
	self
		assert: (self count: #numberTexts during: [
			(self reader: '{"a":[1e2,2E-1]}') exactNumbers: false; next ])
		equals: 0! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testNumberTextInContainers
	"Number text in an Array, a map (native or tree), a nested map and at the root"

	| json check |
	json := '{"a":1e2,"b":[1e-2,{"c":1E1}],"d":-2E+1}'.
	check := [ :map |
		self assert: (map at: 'a') equals: 100.
		self assert: (map at: 'b') first equals: 1 / 100.
		self assert: ((map at: 'b') second at: 'c') equals: 10.
		self assert: (map at: 'd') equals: -20 ].
	check value: (self read: json).
	check value: ((self reader: json) nativeDictionaries: false; next).
	check value: ((self reader: json) mapClass: OrderedDictionary; next).
	check value: ((self reader: json) propertyNamesAsSymbols: true; next).
	self assert: (self read: '[[1e2]]') equals: #( #( 100 ) ).
	self assert: (self count: #numberTexts during: [ self read: json ]) equals: 4! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testManyDuplicateKeys
	"Many number texts, then many duplicates of a key whose value is a String: every number
	 text is converted once, the last value wins"

	| n json value |
	n := 2000.
	json := String streamContents: [ :stream |
		stream nextPut: $[.
		n timesRepeat: [ stream nextPutAll: '1e1,' ].
		stream nextPut: ${.
		1 to: n do: [ :i |
			i > 1 ifTrue: [ stream nextPut: $, ].
			stream nextPutAll: '"a":"1"' ].
		stream nextPutAll: ',"b":2e1,"b":"x","c":3e1}]' ].
	#( true false ) do: [ :native |
		value := (self reader: json) nativeDictionaries: native; next.
		self assert: value size equals: n + 1.
		self assert: ((1 to: n) allSatisfy: [ :i | (value at: i) = 10 ]).
		self assert: (value last at: 'a') equals: '1'.
		self assert: (value last at: 'b') equals: 'x'.
		self assert: (value last at: 'c') equals: 30 ]! !

!SimdJSONReaderTests methodsFor: 'tests - numbers'!
testNumberTextAndDuplicateKeys
	"The number text of a value that a duplicate key replaced is not converted"

	#( true false ) do: [ :native |
		| reader |
		reader := [ :json | (self reader: json) nativeDictionaries: native; next ].
		self assert: ((reader value: '{"a":1e5,"a":"1e5"}') at: 'a') equals: '1e5'.
		self assert: ((reader value: '{"a":"1e5","a":1e5}') at: 'a') equals: 100000.
		self assert: ((reader value: '{"a":1e5,"b":2,"a":3}') at: 'a') equals: 3.
		self assert: ((reader value: '{"a":1e5,"b":2,"a":3}') at: 'b') equals: 2 ]! !

!SimdJSONReaderTests methodsFor: 'tests - strings'!
testStrings

	| value |
	self assert: (self read: '"abc"') class equals: ByteString.
	value := self read: '"\u00e9"'.
	self assert: value equals: (self string: #( 16rE9 )).
	self assert: value class equals: ByteString.
	value := self read: '"\u20ac"'.
	self assert: value equals: (self string: #( 16r20AC )).
	self assert: value class equals: WideString.
	value := self read: '"\ud834\udd1e"'.
	self assert: value size equals: 1.
	self assert: value first asInteger equals: 16r1D11E.
	value := self read: '"a\u0000b"'.
	self assert: value size equals: 3.
	self assert: value second asInteger equals: 0.
	self
		assert: (self read: '"a\"b\\c\/d\b\f\n\r\t"')
		equals: 'a"b\c/d' , (self string: #( 8 12 10 13 9 )).
	value := self read: '["a","\u20ac","b","\u00e9\u20ac"]'.
	self assert: (value collect: [ :each | each class ]) equals: { ByteString. WideString. ByteString. WideString }.
	self assert: value last equals: (self string: #( 16rE9 16r20AC ))! !

!SimdJSONReaderTests methodsFor: 'tests - strings'!
testStringsInSources
	"Characters that are not ASCII in a ByteString (Latin-1), a WideString and UTF-8 bytes"

	| value |
	value := self read: '"' , (self string: #( 16rE9 )) , '"'.
	self assert: value equals: (self string: #( 16rE9 )).
	self assert: value class equals: ByteString.
	value := self read: '"' , (self string: #( 16r20AC 16r1D11E )) , '"'.
	self assert: value equals: (self string: #( 16r20AC 16r1D11E )).
	value := SimdJSONReader fromBytes: ('{"' , (self string: #( 16rE9 )) , '":"' , (self string: #( 16r20AC )) , '"}') utf8Encoded.
	self assert: (value at: (self string: #( 16rE9 ))) equals: (self string: #( 16r20AC ))! !

!SimdJSONReaderTests methodsFor: 'tests - strings'!
testKeys

	| value |
	value := self read: '{"a":1,"\u00e9":2,"\u20ac":3}'.
	self assert: (value at: 'a') equals: 1.
	self assert: (value at: (self string: #( 16rE9 ))) equals: 2.
	self assert: (value at: (self string: #( 16r20AC ))) equals: 3.
	self assert: (value keys detect: [ :each | each = 'a' ]) class equals: ByteString.
	self
		assert: (value keys detect: [ :each | each = (self string: #( 16r20AC )) ]) class
		equals: WideString! !

!SimdJSONReaderTests methodsFor: 'tests - classes'!
testMapClassOrderedDictionary

	| value |
	value := (self reader: '{"b":1,"a":{"y":2,"x":3},"c":[{"z":4}]}') mapClass: OrderedDictionary; next.
	self assert: value class equals: OrderedDictionary.
	self assert: value keys asArray equals: #( 'b' 'a' 'c' ).
	self assert: (value at: 'a') class equals: OrderedDictionary.
	self assert: (value at: 'a') keys asArray equals: #( 'y' 'x' ).
	self assert: (value at: 'c') class equals: Array.
	self assert: (value at: 'c') first class equals: OrderedDictionary! !

!SimdJSONReaderTests methodsFor: 'tests - classes'!
testMapClassSmallDictionary

	| value |
	value := (self reader: '{"b":1,"a":{"c":2}}') mapClass: SmallDictionary; next.
	self assert: value class equals: SmallDictionary.
	self assert: (value at: 'b') equals: 1.
	self assert: (value at: 'a') class equals: SmallDictionary.
	self assert: ((value at: 'a') at: 'c') equals: 2! !

!SimdJSONReaderTests methodsFor: 'tests - classes'!
testListClassOrderedCollection

	| value |
	value := (self reader: '[1,[2,[3]],{"a":[4]}]') listClass: OrderedCollection; next.
	self assert: value class equals: OrderedCollection.
	self assert: value second class equals: OrderedCollection.
	self assert: value second second class equals: OrderedCollection.
	self assert: ((value at: 3) at: 'a') class equals: OrderedCollection.
	self assert: value asArray first equals: 1.
	self assert: ((value at: 3) at: 'a') asArray equals: #( 4 )! !

!SimdJSONReaderTests methodsFor: 'tests - classes'!
testPropertyNamesAsSymbols

	| value nonAscii |
	value := (self reader: '{"abc":1,"n":{"abc":2},"l":[{"abc":3}],"abc":4}')
		propertyNamesAsSymbols: true;
		next.
	self assert: (value keys allSatisfy: [ :each | each isSymbol ]).
	self assert: (value keys detect: [ :each | each = #abc ]) == #abc.
	self assert: (value at: #abc) equals: 4.
	self assert: ((value at: #n) keys detect: [ :each | each = #abc ]) == #abc.
	self assert: (((value at: #l) first) keys detect: [ :each | each = #abc ]) == #abc.
	nonAscii := (self reader: '{"\u00e9t\u00e9":1,"\u20ac":2}') propertyNamesAsSymbols: true; next.
	self assert: (nonAscii keys allSatisfy: [ :each | each isSymbol ]).
	self assert: (nonAscii at: (self string: #( 16rE9 116 16rE9 )) asSymbol) equals: 1.
	self assert: (nonAscii at: (self string: #( 16r20AC )) asSymbol) equals: 2.
	value := (self reader: '{"b":1,"a":2,"b":3}')
		propertyNamesAsSymbols: true;
		mapClass: OrderedDictionary;
		next.
	self assert: value keys asArray equals: #( #b #a ).
	self assert: (value keys allSatisfy: [ :each | each isSymbol ]).
	self assert: (value at: #b) equals: 3! !

!SimdJSONReaderTests methodsFor: 'tests - sources'!
testSources

	| stream reader |
	self
		assert: (self read: '[1,"' , (self string: #( 16r20AC )) , '"]')
		equals: { 1. self string: #( 16r20AC ) }.
	self assert: (SimdJSONReader on: #'[1,2]' readStream) next equals: #( 1 2 ).
	self assert: (SimdJSONReader fromBytes: '{"a":[1,2]}' asByteArray) equals: (Dictionary new at: 'a' put: #( 1 2 ); yourself).
	reader := SimdJSONReader onBytes: '[1] [2]' asByteArray.
	self assert: reader next equals: #( 1 ).
	self assert: reader next equals: #( 2 ).
	self assert: reader atEnd.
	stream := ReadStream on: 'xx[1,2]yy' from: 3 to: 7.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: #( 1 2 ).
	self assert: reader atEnd.
	self assert: stream position equals: 7! !

!SimdJSONReaderTests methodsFor: 'tests - sources'!
testOtherStreams
	"Streams that are not a ReadStream on a String or a ByteArray are read to their end
	 first"

	| stream reader |
	stream := ReadWriteStream on: String new.
	stream nextPutAll: '[1] {"a":2} 3'; reset.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: #( 1 ).
	self assert: (reader next at: 'a') equals: 2.
	self assert: reader next equals: 3.
	self assert: reader atEnd.
	stream := ZnCharacterReadStream on: ('["' , (self string: #( 16r20AC )) , '"]') utf8Encoded readStream.
	self assert: (SimdJSONReader on: stream) next equals: { self string: #( 16r20AC ) }! !

!SimdJSONReaderTests methodsFor: 'tests - sources'!
testExternalReadsBetweenCalls
	"The reader takes the position of the stream before each value"

	| stream reader |
	stream := '[1] [2] [3]' readStream.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: #( 1 ).
	self assert: stream position equals: 4.
	self assert: (stream next: 4) equals: '[2] '.
	self assert: reader next equals: #( 3 ).
	self assert: reader atEnd.
	stream position: 0.
	self deny: reader atEnd.
	self assert: reader next equals: #( 1 )! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testPositions
	"After a value, the stream is after the white space that follows it"

	| stream reader euro |
	stream := '[1]  [2] tail' readStream.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: #( 1 ).
	self assert: stream position equals: 5.
	self assert: reader next equals: #( 2 ).
	self assert: stream position equals: 9.
	self assert: stream upToEnd equals: 'tail'.
	euro := self string: #( 16r20AC ).
	{ euro. self string: #( 16rE9 ) } do: [ :char |
		stream := ('"' , char , '"  [2] tail') readStream.
		reader := SimdJSONReader on: stream.
		self assert: reader next equals: char.
		self assert: stream position equals: 5.
		self assert: reader next equals: #( 2 ).
		self assert: stream upToEnd equals: 'tail' ].
	stream := ('"' , euro , '"  [2] tail') utf8Encoded readStream.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: euro.
	self assert: stream position equals: 7! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testAtEnd
	"Nothing is skipped before the first value, as in NeoJSON"

	| reader |
	reader := self reader: '[1]   '.
	self deny: reader atEnd.
	reader next.
	self assert: reader atEnd.
	self deny: (self reader: '   ') atEnd.
	self assert: (self reader: '') atEnd.
	reader := self reader: '1 2'.
	self assert: reader next equals: 1.
	self deny: reader atEnd.
	self assert: reader next equals: 2.
	self assert: reader atEnd! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testTrailingContent
	"next reads one value and leaves what follows, as NeoJSON"

	self assert: (self read: '1 2') equals: 1.
	self assert: (self read: '[1] x') equals: #( 1 ).
	self assert: (self read: '[1]x') equals: #( 1 ).
	self assert: ((self read: '{"a":1} {"b":2}') at: 'a') equals: 1.
	self assert: (self read: '"a" "b"') equals: 'a'.
	self assert: (self read: 'true false') equals: true! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testFormFeed
	"Form feed separates values, before, between and after them"

	self assert: (self read: self ff , '[1]' , self ff) equals: #( 1 ).
	self assert: (self reader: '[1]' , self ff , '[2]') upToEnd equals: #( #( 1 ) #( 2 ) ).
	self assert: ((self reader: '[1]' , self ff) next; atEnd)! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testUpToEnd

	| values reader |
	values := (self reader: '{"a":1}' , self lf , '{"a":2}' , self lf , '3') upToEnd.
	self assert: values class equals: Array.
	self assert: values size equals: 3.
	self assert: (values first at: 'a') equals: 1.
	self assert: (values second at: 'a') equals: 2.
	self assert: values third equals: 3.
	values := (self reader: '[1] [2]') listClass: OrderedCollection; upToEnd.
	self assert: values class equals: Array.
	self assert: values first class equals: OrderedCollection.
	reader := self reader: '1 2 3'.
	self assert: reader next equals: 1.
	self assert: reader upToEnd equals: #( 2 3 ).
	self assert: (self reader: '') upToEnd equals: #()! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testNextLoop

	| json reader values |
	json := String streamContents: [ :stream |
		1 to: 200 do: [ :i |
			stream nextPutAll: '{"id":'; print: i; nextPutAll: ',"v":['; print: i; nextPutAll: ',1e1]}'; nextPut: Character lf ] ].
	reader := self reader: json.
	values := OrderedCollection new.
	[ reader atEnd ] whileFalse: [ values add: reader next ].
	self assert: values size equals: 200.
	self assert: (values last at: 'id') equals: 200.
	self assert: (values last at: 'v') equals: #( 200 10 ).
	self assert: (self reader: json) upToEnd equals: values asArray! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testMultipleValues
	"NeoJSONExamplesTests>>testMultiInstances"

	| json reader |
	json := '{"a":1}' , self lf , '{"a":2}'.
	reader := self reader: json.
	self assert: (reader next at: 'a') equals: 1.
	self deny: reader atEnd.
	self assert: (reader next at: 'a') equals: 2.
	self assert: reader atEnd! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testFailIfNotAtEnd

	| reader |
	reader := self reader: '[1] x'.
	self assert: reader next equals: #( 1 ).
	self
		should: [ reader failIfNotAtEnd ]
		raise: SimdJSONParseError
		withExceptionDo: [ :error | self assert: error messageText equals: 'extraneous input detected' ].
	reader := self reader: '[1]  '.
	reader next.
	self shouldnt: [ reader failIfNotAtEnd ] raise: Error! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testNextAsNilAndClose

	| reader |
	reader := self reader: '[1] [2]'.
	self assert: (reader nextAs: nil) equals: #( 1 ).
	self assert: reader next equals: #( 2 ).
	reader close! !

!SimdJSONReaderTests methodsFor: 'tests - streams'!
testCloseClosesTheStream
	"close closes the stream given to on:, also when the reader read it into a stream of
	 its own"

	| stream reader |
	stream := SimdJSONTestStream on: '[1,2] 3'.
	reader := SimdJSONReader on: stream.
	self assert: reader next equals: #( 1 2 ).
	self deny: stream isClosed.
	reader close.
	self assert: stream isClosed! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testErrors

	#( '[1,' '[1,2' '{"a":1' '{"a" 1}' '[1 2]' '{1:2}' '[1,]' '[,1]' '{"a":1,}' 'nul' 'tru'
	   '"open' '{a:1}' '.5' '+1' '-' '' '   ' '[}' '"\x"' '"\uD83D"' ) do: [ :each |
		self should: [ self read: each ] raise: SimdJSONParseError ]! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testLenientInputs
	"NeoJSON accepts these, simdjson does not: the reader is strict RFC 8259"

	#( '01' '-01' '00' '1.' '[1.]' '"\uDE00"' '123abc' '0x10' '1e400' '1.0e309' ) ,
	{ '"a' , self lf , 'b"'. '[' , self ff , '1]' } do: [ :each |
		self should: [ self read: each ] raise: SimdJSONParseError ]! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testStrict
	"simdjson's error is a SimdJSONParseError, with its code, the position of the value and
	 its message"

	| error |
	error := self strictErrorOf: '01'.
	self assert: error notNil.
	self assert: error code equals: 9.
	self assert: error position equals: 1.
	self assert: error messageText equals: 'simdjson: ' , (SimdJSONReader errorMessageFor: 9).
	self assert: (error messageText beginsWith: 'simdjson: NUMBER_ERROR').
	self assert: (self read: '[1]') equals: #( 1 ).
	self assert: (self reader: '1 2') next equals: 1! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testErrorCodes

	self assert: (self strictErrorOf: '[1,') code isInteger.
	self assert: (self strictErrorOf: '   ') code equals: 13.
	self assert: (self strictErrorOf: '') code equals: 13.
	self assert: (self strictErrorOf: (self string: #( 16rFEFF )) , '[1]') code equals: 1001.
	self
		assert: ([ (SimdJSONReader on: (#[ 16rEF 16rBB 16rBF ] , '[1]' asByteArray) readStream) next. nil ]
			on: SimdJSONParseError
			do: [ :error | error return: error code ])
		equals: 1001.
	self
		assert: (self strictErrorOf: (String new: 1025 withAll: $[) , (String new: 1025 withAll: $])) code
		equals: 4.
	self assert: (self strictErrorOf: (String new: 1024 withAll: $[) , (String new: 1024 withAll: $])) isNil! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testErrorMessages

	self assert: ((SimdJSONReader errorMessageFor: 9) beginsWith: 'NUMBER_ERROR').
	self assert: ((SimdJSONReader errorMessageFor: 13) beginsWith: 'EMPTY').
	self assert: (SimdJSONReader errorMessageFor: 1001) notEmpty.
	self assert: (SimdJSONReader errorMessageFor: 1002) notEmpty.
	self assert: ((SimdJSONReader primErrorMessage: 0) beginsWith: 'SUCCESS').
	self assert: (SimdJSONReader primErrorMessage: 1001) equals: (SimdJSONReader errorMessageFor: 1001).
	self assert: (SimdJSONReader primErrorMessage: 100000) isNil.
	self assert: (SimdJSONReader primErrorMessage: -1) isNil! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testMaxInputUnits
	"Above the cap, the reader refuses the value"

	SimdJSONReader maxInputUnits: 8.
	self assert: (self read: '[1]') equals: #( 1 ).
	self
		assert: (self count: #tooLarge during: [
			self should: [ self read: '[1,2,3,4,5]' ] raise: SimdJSONResourceError ])
		equals: 1.
	SimdJSONReader maxInputUnits: nil.
	self assert: (self read: '[1,2,3,4,5]') equals: #( 1 2 3 4 5 )! !

!SimdJSONReaderTests methodsFor: 'tests - errors'!
testMaxInputUnitsPerValue
	"The cap applies to each value: a sequence of small values longer than the cap is read
	 by the plugin"

	| json reader |
	json := String streamContents: [ :stream |
		1 to: 100 do: [ :i | stream print: i; nextPut: Character lf ] ].
	SimdJSONReader maxInputUnits: 8.
	self
		assert: (self count: #tooLarge during: [
			self assert: (self reader: json) upToEnd equals: (1 to: 100) asArray ])
		equals: 0.
	reader := self reader: '[1,2] [1,2,3,4,5] [3]'.
	self assert: reader next equals: #( 1 2 ).
	self should: [ reader next ] raise: SimdJSONResourceError! !

!SimdJSONReaderTests methodsFor: 'tests - statistics'!
testStatistics

	SimdJSONReader resetStatistics.
	self assert: SimdJSONReader statistics keys asSet equals: SimdJSONReader statisticsKeys asSet.
	#( #reads #fastValues #overBudget #grows #numberTexts #nativeDictionaries #tooLarge ) do: [ :each |
		self assert: (self statistic: each) equals: 0 ].
	self read: '[1e2,{"a":1}]'.
	self assert: (self statistic: #reads) >= 1.
	self assert: (self statistic: #fastValues) >= 1.
	self assert: (self statistic: #numberTexts) equals: 1! !

!SimdJSONReaderTests methodsFor: 'tests - primitive'!
testPrimitiveResult
	"The 6 slots: status, value, nextIndex, fixups, keys, code"

	| result value |
	result := self prim: '[1e2,{"k":"v"}]' flags: 2 budget: -1.
	self assert: result size equals: 6.
	self assert: result first equals: 0.
	value := result at: 2.
	self assert: value first equals: '1e2'.
	self assert: value second class equals: SimdJSONFlatMap.
	self assert: value second size equals: 2.
	self assert: (value second at: 1) equals: 'k'.
	self assert: (value second at: 2) equals: 'v'.
	self assert: (result at: 3) equals: 16.
	self assert: (result at: 4) size equals: 2.
	self assert: (result at: 4) first == value.
	self assert: (result at: 4) second equals: 1.
	self assert: (result at: 5) isNil.
	self assert: (result at: 6) equals: 0.
	result := self prim: '1e2' flags: 2 budget: -1.
	self assert: (result at: 2) equals: '1e2'.
	self assert: (result at: 4) first == result.
	self assert: (result at: 4) second equals: 2.
	result := self prim: '1e2' flags: 0 budget: -1.
	self assert: (result at: 2) equals: 100.0.
	self assert: (result at: 4) isNil! !

!SimdJSONReaderTests methodsFor: 'tests - primitive'!
testPrimitiveKeysAndNext

	| result |
	result := self prim: '[{"a":1,"b":2},{"a":3}]' flags: 8 budget: -1.
	self assert: (result at: 5) equals: #( 'a' 'b' ).
	self assert: ((result at: 2) first at: 1) equals: 1.
	self assert: ((result at: 2) first at: 3) equals: 2.
	self assert: ((result at: 2) second at: 1) equals: 1.
	self assert: ((result at: 2) second at: 2) equals: 3.
	result := self prim: '[1] [2]' from: 1 to: 7 flags: 1 budget: -1 maxUnits: -1.
	self assert: (result at: 2) equals: #( 1 ).
	self assert: (result at: 3) equals: 5.
	result := self prim: '[1] [2]' from: 5 to: 7 flags: 1 budget: -1 maxUnits: -1.
	self assert: (result at: 2) equals: #( 2 ).
	self assert: (result at: 3) equals: 8.
	result := self primDict: '{"k":[1,{"j":null}]}' budget: -1.
	self assert: (result at: 2) class equals: Dictionary.
	self assert: (((result at: 2) at: 'k') second at: 'j') isNil! !

!SimdJSONReaderTests methodsFor: 'tests - primitive'!
testPrimitiveErrorsAndFailures
	"A parse error is a status; only bad arguments fail the primitive"

	| result |
	result := self prim: '[1,' flags: 0 budget: -1.
	self assert: result first equals: 2.
	self assert: (result at: 2) isNil.
	self assert: (result at: 3) equals: 1.
	self assert: (result at: 6) > 0.
	result := self prim: '{}' flags: 12 budget: -1.
	self assert: (result isKindOf: SimdJSONFailure).
	self assert: result ec equals: #'bad argument'.
	result := self prim: '[1]' from: 10 to: 3 flags: 0 budget: -1 maxUnits: -1.
	self assert: (result isKindOf: SimdJSONFailure).
	self assert: result ec equals: #'bad index'.
	result := self prim: Object new from: 1 to: 0 flags: 0 budget: -1 maxUnits: -1.
	self assert: (result isKindOf: SimdJSONFailure).
	self assert: result ec equals: #'bad argument'.
	result := self prim: '[1]' from: 1 to: 3 flags: 0 budget: -1 maxUnits: -2.
	self assert: (result isKindOf: SimdJSONFailure).
	self assert: result ec equals: #'bad argument'.
	"isBytes is true of a CompiledMethod, whose bytecodes the VM gives no pointer to"
	result := self prim: Object >> #printString from: 1 to: 5 flags: 0 budget: -1 maxUnits: -1.
	self assert: (result isKindOf: SimdJSONFailure).
	self assert: result ec equals: #'bad argument'! !

!SimdJSONReaderTests methodsFor: 'tests - primitive'!
testPrimitiveMaxUnits
	"A whole rest over maxUnits gives way to the value, and a value over it is a status"

	| result |
	result := self prim: '[1,2] [3,4]' from: 1 to: 11 flags: 0 budget: -1 maxUnits: 5.
	self assert: result first equals: 0.
	self assert: (result at: 2) equals: #( 1 2 ).
	self assert: (result at: 3) equals: 7.
	result := self prim: '[1,2] [3,4]' from: 7 to: 11 flags: 1 budget: -1 maxUnits: 5.
	self assert: (result at: 2) equals: #( 3 4 ).
	self assert: (result at: 3) equals: 12.
	#( 0 1 ) do: [ :flags |
		result := self prim: '[1,2] [3,4]' from: 1 to: 11 flags: flags budget: -1 maxUnits: 4.
		self assert: result first equals: 3.
		self assert: (result at: 2) isNil.
		self assert: (result at: 3) equals: 1.
		self assert: (result at: 6) equals: 5 ]! !

!SimdJSONReaderTests methodsFor: 'tests - primitive'!
testPrimitiveNextAfterRefusedBudget
	"A refused budget keeps the parse of [1] for the retry: a read of more text in NEXT mode
	 that reuses it still answers the index of the next value"

	| result |
	result := self prim: '[1] [2]' from: 1 to: 3 flags: 0 budget: 0 maxUnits: -1.
	self assert: result first equals: 1.
	result := self prim: '[1] [2]' from: 1 to: 7 flags: 1 budget: -1 maxUnits: -1.
	self assert: (result at: 2) equals: #( 1 ).
	self assert: (result at: 3) equals: 5! !

!SimdJSONNativeTests methodsFor: 'helpers'!
tree: aString

	^ (self reader: aString) nativeDictionaries: false; next! !

!SimdJSONNativeTests methodsFor: 'tests'!
testSelfTestPasses

	self assert: SimdJSONReader runNativeSelfTest isNil.
	self assert: SimdJSONReader nativeDictionariesWork! !

!SimdJSONNativeTests methodsFor: 'tests'!
testNativeIsUsed

	self assert: (self count: #nativeDictionaries during: [ self read: '{"a":1}' ]) > 0.
	self assert: (self count: #nativeDictionaries during: [ self tree: '{"a":1}' ]) equals: 0! !

!SimdJSONNativeTests methodsFor: 'tests'!
testNativeEqualsTree

	| native tree |
	native := self read: self probeJSON.
	tree := self tree: self probeJSON.
	self assert: native class equals: Dictionary.
	self assert: native equals: tree.
	self assert: native size equals: 7.
	self assert: (native at: 'a') equals: 4.
	self assert: (native at: 'n') equals: 100.
	tree keysAndValuesDo: [ :key :value |
		self assert: (native includesKey: key).
		self assert: (native at: key) equals: value ].
	self assert: (native at: 'o') size equals: 300.
	0 to: 299 do: [ :i |
		self assert: ((native at: 'o') at: 'k' , i printString) equals: i ]! !

!SimdJSONNativeTests methodsFor: 'tests'!
testNativeMutation
	"A native Dictionary grows, removes and sorts as any other (step 3 of the self-test)"

	| native tree empty |
	native := self read: self probeJSON.
	tree := self tree: self probeJSON.
	1 to: 50 do: [ :i |
		native at: 'new' , i printString put: i.
		tree at: 'new' , i printString put: i ].
	self assert: native equals: tree.
	native removeKey: 'a'.
	tree removeKey: 'a'.
	self assert: native equals: tree.
	self deny: (native includesKey: 'a').
	self assert: native keys asSet equals: tree keys asSet.
	self assert: native keysSortedSafely equals: tree keysSortedSafely.
	(native at: 'o') removeKey: 'k7'.
	(native at: 'o') at: 'k7' put: 7.
	self assert: (native at: 'o') equals: (tree at: 'o').
	empty := self read: '{}'.
	empty at: 'x' put: 1.
	self assert: (empty at: 'x') equals: 1.
	(native at: 'e') at: 'x' put: 1.
	self assert: ((native at: 'e') at: 'x') equals: 1! !

!SimdJSONNativeTests methodsFor: 'tests'!
testNativeKeys

	| native euro astral |
	euro := self string: #( 16r20AC ).
	astral := self string: #( 16r1D11E ).
	native := self read: '{"\u20ac":1,"\ud834\udd1e":2,"\u00e9":3,"a":4}'.
	self assert: (native at: euro) equals: 1.
	self assert: (native at: astral) equals: 2.
	self assert: (native at: (self string: #( 16rE9 ))) equals: 3.
	self assert: (native includesKey: euro).
	self assert: (native keys detect: [ :each | each = 'a' ]) class equals: ByteString.
	self assert: (native keys detect: [ :each | each = euro ]) class equals: WideString.
	self assert: native equals: (self tree: '{"\u20ac":1,"\ud834\udd1e":2,"\u00e9":3,"a":4}')! !

!SimdJSONNativeTests methodsFor: 'tests'!
testNativeLarge

	| json |
	json := String streamContents: [ :stream |
		stream nextPut: $[.
		1 to: 2000 do: [ :i |
			i > 1 ifTrue: [ stream nextPut: $, ].
			stream nextPut: ${.
			1 to: 20 do: [ :k |
				k > 1 ifTrue: [ stream nextPut: $, ].
				stream nextPutAll: '"key'; print: k; nextPutAll: '":'; print: i * k ].
			stream nextPut: $} ].
		stream nextPut: $] ].
	self assert: (self read: json) equals: (self tree: json)! !

!SimdJSONNativeTests methodsFor: 'tests'!
testSelfTestFailure
	"A failed self-test turns the native path off for the session; the results stay equal"

	| native |
	SimdJSONReader testHook: #nativeSelfTestFails value: true.
	self assert: SimdJSONReader runNativeSelfTest notNil.
	self deny: SimdJSONReader nativeDictionariesWork.
	self
		assert: (self count: #nativeDictionaries during: [ native := self read: self probeJSON ])
		equals: 0.
	self assert: native equals: (self tree: self probeJSON).
	SimdJSONReader testHook: #nativeSelfTestFails value: nil.
	self assert: SimdJSONReader nativeDictionariesWork! !

!SimdJSONNativeTests methodsFor: 'tests'!
testKillSwitch

	| value |
	SimdJSONReader nativeDictionaries: false.
	self deny: SimdJSONReader nativeDictionariesWork.
	self
		assert: (self count: #nativeDictionaries during: [ value := self read: self probeJSON ])
		equals: 0.
	self assert: value equals: (self tree: self probeJSON).
	SimdJSONReader nativeDictionaries: true.
	self assert: SimdJSONReader nativeDictionariesWork.
	self assert: (self count: #nativeDictionaries during: [ self read: '{}' ]) > 0! !

!SimdJSONNativeTests methodsFor: 'tests'!
testOnlyForDictionaryAndArray
	"Other classes, Symbol keys, and a reader that said no take the tree path"

	{ [ :reader | reader mapClass: OrderedDictionary; yourself ].
	  [ :reader | reader listClass: OrderedCollection; yourself ].
	  [ :reader | reader propertyNamesAsSymbols: true; yourself ].
	  [ :reader | reader nativeDictionaries: false; yourself ] } do: [ :configure |
		self
			assert: (self count: #nativeDictionaries during: [
				(configure value: (self reader: '{"a":[1]}')) next ])
			equals: 0 ]! !

!SimdJSONMemoryTests methodsFor: 'helpers'!
heapBytesOf: anObject
	"The bytes of anObject and of everything it holds that primitiveRead allocates: the
	 need that the plugin measures against the byte bound 32 x B + 4096 (docs/SimdJSON.md,
	 Memory and limits), counted by the image"

	| bytes |
	(anObject isNil or: [ anObject == true or: [ anObject == false ] ]) ifTrue: [ ^ 0 ].
	anObject class isImmediateClass ifTrue: [ ^ 0 ].
	bytes := anObject sizeInMemory.
	anObject class == Dictionary ifTrue: [
		bytes := bytes + (anObject instVarNamed: #array) sizeInMemory.
		anObject associationsDo: [ :each |
			bytes := bytes + each sizeInMemory + (self heapBytesOf: each key) + (self heapBytesOf: each value) ].
		^ bytes ].
	(anObject class == Array or: [ anObject class == SimdJSONFlatMap ]) ifTrue: [
		1 to: anObject basicSize do: [ :i | bytes := bytes + (self heapBytesOf: (anObject basicAt: i)) ] ].
	^ bytes! !

!SimdJSONMemoryTests methodsFor: 'helpers'!
needOf: aBlock
	"aBlock answers a result of primitiveRead for a budget: the need it reports with budget 0,
	 and the bytes of the result it builds without a budget"

	| refused built |
	refused := aBlock value: 0.
	self assert: refused first equals: 1.
	built := aBlock value: -1.
	self assert: built first equals: 0.
	^ { refused at: 6.
		built sizeInMemory + (self heapBytesOf: (built at: 2))
			+ ((built at: 4) ifNil: [ 0 ] ifNotNil: [ :fixups | fixups sizeInMemory ])
			+ ((built at: 5) ifNil: [ 0 ] ifNotNil: [ :keys | self heapBytesOf: keys ]) }! !

!SimdJSONMemoryTests methodsFor: 'tests'!
testPrimitiveBudget
	"A budget below the need is refused before anything is allocated, and the need is
	 exact"

	| result need |
	result := self prim: '[1,2,3]' flags: 0 budget: 0.
	self assert: result first equals: 1.
	self assert: (result at: 2) isNil.
	self assert: (result at: 3) equals: 1.
	need := result at: 6.
	result := self prim: '[1,2,3]' flags: 0 budget: need - 1.
	self assert: result first equals: 1.
	self assert: (result at: 6) equals: need.
	result := self prim: '[1,2,3]' flags: 0 budget: need.
	self assert: result first equals: 0.
	self assert: (result at: 2) equals: #( 1 2 3 ).
	self assert: need equals: result sizeInMemory + (result at: 2) sizeInMemory! !

!SimdJSONMemoryTests methodsFor: 'tests'!
testNeedIsExact
	"Tree, Symbol keys and number text, then Dictionaries.  A Dictionary allocates the key
	 and the value of a duplicate key too, which the last value replaces: its need is more
	 than what stays reachable"

	| pair |
	{ '[1,2.5,"x",null,true,[],{}]'. '{"a":{"b":[1e2,"\u20ac",-1.5e300]},"a":7}'.
	  '[18446744073709551616,9223372036854775808,-2E-2]'. self probeJSON } do: [ :json |
		#( 0 2 8 10 ) do: [ :flags |
			pair := self needOf: [ :budget | self prim: json flags: flags budget: budget ].
			self assert: pair first equals: pair last ] ].
	{ '{"a":{"b":[1e2,"\u20ac",-1.5e300]},"c":7}'. '[{},{"x":{"y":[]}},"\u00e9"]'.
	  self probeJSON copyReplaceAll: '"a":4' with: '"d":4' } do: [ :json |
		pair := self needOf: [ :budget | self primDict: json budget: budget ].
		self assert: pair first equals: pair last ].
	pair := self needOf: [ :budget | self primDict: self probeJSON budget: budget ].
	self assert: pair first > pair last! !

!SimdJSONMemoryTests methodsFor: 'tests'!
testOverBudgetThenGrow
	"With a budget of 0 the plugin refuses, the reader grows old space and reads again: no
	 build ever fails"

	| before after value |
	SimdJSONReader nativeDictionariesWork; resetStatistics.
	before := SimdJSONReader primInfo last.
	SimdJSONReader testHook: #budget value: 0.
	value := self read: '{"a":[1,2,3],"b":"x"}'.
	after := SimdJSONReader primInfo last.
	self assert: (value at: 'a') equals: #( 1 2 3 ).
	self assert: (value at: 'b') equals: 'x'.
	self assert: (self statistic: #overBudget) >= 1.
	self assert: (self statistic: #grows) >= 1.
	self assert: (after at: 6) - (before at: 6) >= 1.
	self assert: (after at: 5) equals: (before at: 5)! !

!SimdJSONMemoryTests methodsFor: 'tests'!
testLargeInput
	"Above 64 Ki units the budget is the free old space where it is checked
	 (budgetChecked); natively the build runs without one.  Either way no build fails"

	| json before value |
	json := String streamContents: [ :stream |
		stream nextPut: $[.
		1 to: 10000 do: [ :i |
			i > 1 ifTrue: [ stream nextPut: $, ].
			stream nextPutAll: '{"k":'; print: i; nextPutAll: ',"s":"abcdefgh","f":1.5e300}' ].
		stream nextPut: $] ].
	self assert: json size > 65536.
	before := SimdJSONReader primInfo last.
	value := self read: json.
	self assert: value size equals: 10000.
	self assert: (value last at: 'k') equals: 10000.
	self assert: (value last at: 'f') equals: 1.5e300.
	self assert: (SimdJSONReader primInfo last at: 5) equals: (before at: 5)! !

!SimdJSONMemoryTests methodsFor: 'tests'!
testReleaseParser

	| info |
	SimdJSONReader releaseParser.
	info := SimdJSONReader primInfo.
	self assert: (info at: 6) equals: 0.
	self assert: (info at: 7) equals: 0.
	self assert: (self read: '{"a":[1,2]}') equals: (Dictionary new at: 'a' put: #( 1 2 ); yourself).
	self assert: (SimdJSONReader primInfo at: 6) > 0! !

!SimdJSONMappingTests methodsFor: 'helpers'!
pointJSON

	^ '{"x":1,"y":2}'! !

!SimdJSONMappingTests methodsFor: 'tests'!
testPoint

	self assert: ((self reader: self pointJSON) mapInstVarsFor: Point; nextAs: Point) equals: 1 @ 2.
	self
		assert: ((self reader: '{"x":1e2,"y":-1e-1}') mapInstVarsFor: Point; nextAs: Point)
		equals: 100 @ (-1 / 10).
	self
		assert: ((self reader: self pointJSON) propertyNamesAsSymbols: true; mapInstVarsFor: Point; nextAs: Point)
		equals: 1 @ 2! !

!SimdJSONMappingTests methodsFor: 'tests'!
testUnknownKeys
	"An unknown key is skipped, whatever its value, and the stream goes on after the object"

	| reader |
	reader := self reader: '{"x":1,"z":{"deep":[1,2,{"a":3}],"n":1e5},"y":2} [3]'.
	reader mapInstVarsFor: Point.
	self assert: (reader nextAs: Point) equals: 1 @ 2.
	self assert: reader next equals: #( 3 ).
	self assert: reader atEnd! !

!SimdJSONMappingTests methodsFor: 'tests'!
testRetryAfterError
	"An error of a typed read leaves the reader and the stream before the value, before the
	 handler runs: try the mapping, else read the value as it is"

	| reader value |
	reader := (self reader: '[1,2] 3') mapInstVarsFor: Point; yourself.
	value := [ reader nextAs: Point ] on: Error do: [ :error | reader next ].
	self assert: value equals: #( 1 2 ).
	self assert: reader next equals: 3.
	reader := (self reader: '[1,2] 3') mapInstVarsFor: Point; yourself.
	value := [ reader nextAs: Point ] on: Error do: [ :error | error return: nil ].
	self assert: value isNil.
	self assert: reader next equals: #( 1 2 ).
	reader := (self reader: '{"a":1} 3') mapInstVarsFor: Point; yourself.
	value := [ reader nextListAs: Point ] on: Error do: [ :error | reader next ].
	self assert: (value at: 'a') equals: 1.
	self assert: reader next equals: 3.
	self assert: reader atEnd! !

!SimdJSONMappingTests methodsFor: 'tests'!
testNextListAs

	| value |
	value := (self reader: '[{"x":1,"y":2},{"x":3,"y":4}]') mapInstVarsFor: Point; nextListAs: Point.
	self assert: value equals: { 1 @ 2. 3 @ 4 }.
	self assert: value class equals: Array.
	value := (self reader: '[{"x":1,"y":2}]') listClass: OrderedCollection; mapInstVarsFor: Point; nextListAs: Point.
	self assert: value class equals: OrderedCollection.
	self assert: value first equals: 1 @ 2! !

!SimdJSONMappingTests methodsFor: 'tests'!
testNestedSchemas

	| reader |
	reader := self reader: '{"origin":{"x":1,"y":2},"corner":{"x":3,"y":4}}'.
	reader mapInstVarsFor: Point.
	reader for: Rectangle do: [ :mapping |
		(mapping mapInstVar: #origin) valueSchema: Point.
		(mapping mapInstVar: #corner) valueSchema: Point ].
	self assert: (reader nextAs: Rectangle) equals: (1 @ 2 corner: 3 @ 4)! !

!SimdJSONMappingTests methodsFor: 'tests'!
testAllowNil

	| reader |
	reader := self reader: 'null [{"x":1,"y":2},null] true'.
	reader mapInstVarsFor: Point.
	reader for: Point do: [ :mapping | mapping allowNil ].
	self assert: (reader nextAs: Point) isNil.
	self assert: (reader nextListAs: Point) equals: { 1 @ 2. nil }.
	self
		should: [ reader nextAs: Point ]
		raise: SimdJSONParseError
		withExceptionDo: [ :error | self assert: error messageText equals: 'Unexpected boolean constant' ].
	self
		should: [ (self reader: 'null') mapInstVarsFor: Point; nextAs: Point ]
		raise: SimdJSONParseError
		withExceptionDo: [ :error | self assert: error messageText equals: '{ expected' ]! !

!SimdJSONMappingTests methodsFor: 'tests'!
testDecoder

	self
		assert: ((self reader: '"42"') for: #Number customDo: [ :mapping | mapping decoder: [ :string | string asNumber ] ]; nextAs: #Number)
		equals: 42.
	self
		assert: ((self reader: '[1,2,3]') for: #Sum customDo: [ :mapping | mapping decoder: [ :list | list sum ] ]; nextAs: #Sum)
		equals: 6.
	self
		assert: ((self reader: '[1,{"a":2}]') for: #Plain customDo: [ :mapping | ]; nextAs: #Plain)
		equals: { 1. Dictionary new at: 'a' put: 2; yourself }! !

!SimdJSONMappingTests methodsFor: 'tests'!
testLists

	| reader |
	reader := self reader: '[{"x":1,"y":2}] [1,2,3] [{"x":3,"y":4}]'.
	reader mapInstVarsFor: Point.
	reader for: #Points customDo: [ :mapping | mapping listOfElementSchema: Point ].
	reader for: #Numbers customDo: [ :mapping | mapping listOfType: OrderedCollection ].
	reader for: #OrderedPoints customDo: [ :mapping | mapping listOfType: OrderedCollection andElementSchema: Point ].
	self assert: (reader nextAs: #Points) equals: { 1 @ 2 }.
	self assert: (reader nextAs: #Numbers) equals: (OrderedCollection withAll: #( 1 2 3 )).
	self assert: (reader nextAs: #OrderedPoints) equals: (OrderedCollection with: 3 @ 4)! !

!SimdJSONMappingTests methodsFor: 'tests'!
testMapWithValueSchema

	| json value |
	json := '{"b":{"x":1,"y":2},"a":{"x":3,"y":4}}'.
	value := (self reader: json)
		mapInstVarsFor: Point;
		for: #PointMap customDo: [ :mapping | mapping mapWithValueSchema: Point ];
		nextAs: #PointMap.
	self assert: value class equals: Dictionary.
	self assert: (value at: 'b') equals: 1 @ 2.
	self assert: (value at: 'a') equals: 3 @ 4.
	value := (self reader: json)
		mapClass: OrderedDictionary;
		mapInstVarsFor: Point;
		for: #PointMap customDo: [ :mapping | mapping mapWithValueSchema: Point ];
		nextAs: #PointMap.
	self assert: value class equals: OrderedDictionary.
	self assert: value keys asArray equals: #( 'b' 'a' )! !

!SimdJSONMappingTests methodsFor: 'tests'!
testReaderBlocks
	"User reader blocks with the parse protocol of NeoJSONReader"

	| pairs constants |
	pairs := (self reader: '{"b":1,"a":[2],"c":{"x":3,"y":4}}')
		mapInstVarsFor: Point;
		for: #Pairs customDo: [ :mapping |
			mapping reader: [ :reader |
				| out |
				out := OrderedCollection new.
				reader parseMapKeysDo: [ :key |
					out add: key -> (key = 'c' ifTrue: [ reader nextAs: Point ] ifFalse: [ reader next ]) ].
				out asArray ] ];
		nextAs: #Pairs.
	self assert: pairs equals: { 'b' -> 1. 'a' -> #( 2 ). 'c' -> (3 @ 4) }.
	constants := (self reader: '[true,1,null,"x",false]')
		for: #Constants customDo: [ :mapping |
			mapping reader: [ :reader |
				| out |
				out := OrderedCollection new.
				reader parseListDo: [
					| value |
					value := #none.
					reader parseConstantDo: [ :constant | value := constant ].
					value == #none ifTrue: [ value := reader next ].
					out add: value ].
				out asArray ] ];
		nextAs: #Constants.
	self assert: constants equals: { true. 1. nil. 'x'. false }! !

!SimdJSONMappingTests methodsFor: 'tests'!
testClassSideMapping
	"SimdJSONTestObject class>>neoJsonMapping:, through fromString:as: and fromBytes:as:"

	| json object |
	json := '{"id":7,"name":"seven","points":[{"x":1,"y":2},{"x":3,"y":4}],"created-at":"1234","tags":["a","b"],"extra":[1]}'.
	{ SimdJSONReader fromString: json as: SimdJSONTestObject.
	  SimdJSONReader fromBytes: json asByteArray as: SimdJSONTestObject } do: [ :each |
		object := each.
		self assert: object class equals: SimdJSONTestObject.
		self assert: object id equals: 7.
		self assert: object name equals: 'seven'.
		self assert: object points equals: { 1 @ 2. 3 @ 4 }.
		self assert: object stamp equals: 1234.
		self assert: object tags equals: (OrderedCollection withAll: #( 'a' 'b' )) ]! !

!SimdJSONMappingTests methodsFor: 'tests'!
testAccessorsAndNames

	| object |
	object := (self reader: '{"id":7,"name":"n"}')
		for: SimdJSONTestObject do: [ :mapping | mapping mapAccessors: #( #id #name ) ];
		nextAs: SimdJSONTestObject.
	self assert: object id equals: 7.
	self assert: object name equals: 'n'.
	self
		assert: ((self reader: '{"X":1,"Y":2}')
			for: Point do: [ :mapping | mapping mapInstVar: #x to: 'X'; mapInstVar: #y to: 'Y' ];
			nextAs: Point)
		equals: 1 @ 2.
	self
		assert: ((self reader: '{"both":5}')
			for: Point do: [ :mapping |
				mapping mapProperty: 'both' getter: [ :point | point x ] setter: [ :point :value | point setX: value setY: value ] ];
			nextAs: Point)
		equals: 5 @ 5! !

!SimdJSONMappingTests methodsFor: 'tests'!
testMappingNotFound
	"The mapping is looked up before anything is parsed"

	self
		should: [ (self reader: self pointJSON) nextAs: Point ]
		raise: SimdJSONMappingNotFound
		withExceptionDo: [ :error |
			self assert: error messageText equals: 'No mapping found for Point in SimdJSONReader' ].
	self should: [ (self reader: self pointJSON) nextAs: #Unknown ] raise: SimdJSONMappingNotFound.
	self should: [ SimdJSONReader fromString: '[1,' as: #Unknown ] raise: SimdJSONMappingNotFound.
	self
		should: [ (self reader: '{}') for: Point do: [ :mapping | mapping mapInstVar: #zz ] ]
		raise: Error! !

!SimdJSONMappingTests methodsFor: 'tests'!
testMappingLenientInput

	| reader |
	reader := (self reader: '{"x":01,"y":2}') mapInstVarsFor: Point; yourself.
	self should: [ reader nextAs: Point ] raise: SimdJSONParseError! !

!SimdJSONMappingTests methodsFor: 'tests'!
testSeveralValues

	| reader |
	reader := self reader: '{"x":1,"y":2} {"x":3,"y":4}'.
	reader mapInstVarsFor: Point.
	self assert: (reader nextAs: Point) equals: 1 @ 2.
	self deny: reader atEnd.
	self assert: (reader nextAs: Point) equals: 3 @ 4.
	self assert: reader atEnd! !

!SimdJSONSlowTests methodsFor: 'running'!
defaultTimeLimit

	^ 10 minutes! !

!SimdJSONSlowTests methodsFor: 'running'!
setUp

	super setUp.
	(OSEnvironment current at: 'SJ_SLOW' ifAbsent: [ nil ]) ifNil: [ self skip: 'set SJ_SLOW to run' ]! !

!SimdJSONSlowTests methodsFor: 'helpers'!
floatsJSON: count
	"[1.5e300,...]: 8 bytes, and one BoxedFloat64 of 16 bytes, an element"

	^ String new: count * 8 + 1 streamContents: [ :stream |
		stream nextPut: $[.
		1 to: count do: [ :i |
			i > 1 ifTrue: [ stream nextPut: $, ].
			stream nextPutAll: '1.5e300' ].
		stream nextPut: $] ]! !

!SimdJSONSlowTests methodsFor: 'tests'!
testManyBoxedFloats
	"2M boxes (48 MB of objects): the plugin allocates them where the image grew old space,
	 never in eden only"

	| value |
	value := (self reader: (self floatsJSON: 2000000)) exactNumbers: false; next.
	self assert: value size equals: 2000000.
	self assert: value first equals: 1.5e300.
	self assert: value first class equals: BoxedFloat64.
	self assert: value last equals: 1.5e300.
	value := self read: (self floatsJSON: 200000).
	self assert: value size equals: 200000.
	self assert: value last equals: 1.5e300! !

!SimdJSONSlowTests methodsFor: 'tests'!
testOutOfMemoryAtCap
	"Old space capped 8 MB above its size: a clean OutOfMemory, and the VM goes on"

	| json cap result |
	json := self floatsJSON: 2000000.
	Smalltalk garbageCollect.
	cap := Smalltalk vm parameterAt: 67.
	[ Smalltalk vm parameterAt: 67 put: (Smalltalk vm parameterAt: 1) + (8 * 1024 * 1024).
	  result := [ (self reader: json) exactNumbers: false; next ]
		on: OutOfMemory
		do: [ :error | error return: #outOfMemory ] ]
		ensure: [ Smalltalk vm parameterAt: 67 put: cap ].
	self assert: result equals: #outOfMemory.
	json := result := nil.
	SimdJSONReader releaseParser.
	Smalltalk garbageCollect.
	self assert: (self read: '{"a":[1,2.5,"x",null,true]}') size equals: 1! !

!SimdJSONSlowTests methodsFor: 'tests'!
testWriteOutOfMemoryAtCap
	"An output of 2 million Floats that old space, capped, cannot hold: the primitive answers
	 what it needs, growing fails, and the writer signals OutOfMemory; uncapped, it writes"

	| floats cap result |
	floats := (1 to: 2000000) collect: [ :i | i / 7.0 ].
	Smalltalk garbageCollect.
	cap := Smalltalk vm parameterAt: 67.
	[ Smalltalk vm parameterAt: 67 put: (Smalltalk vm parameterAt: 1) + (8 * 1024 * 1024).
	  result := [ SimdJSONWriter toBytes: floats ]
		on: OutOfMemory
		do: [ :error | error return: #outOfMemory ] ]
		ensure: [ Smalltalk vm parameterAt: 67 put: cap ].
	self assert: result equals: #outOfMemory.
	result := SimdJSONWriter toBytes: floats.
	self assert: (result copyFrom: 1 to: 21) asString equals: '[0.14285714285714285,'.
	self assert: (SimdJSONReader fromBytes: result) size equals: 2000000! !

!SimdJSONTestObject methodsFor: 'accessing'!
id

	^ id! !

!SimdJSONTestObject methodsFor: 'accessing'!
id: anObject

	id := anObject! !

!SimdJSONTestObject methodsFor: 'accessing'!
name

	^ name! !

!SimdJSONTestObject methodsFor: 'accessing'!
name: anObject

	name := anObject! !

!SimdJSONTestObject methodsFor: 'accessing'!
points

	^ points! !

!SimdJSONTestObject methodsFor: 'accessing'!
stamp

	^ stamp! !

!SimdJSONTestObject methodsFor: 'accessing'!
tags

	^ tags! !

!SimdJSONTestObject class methodsFor: 'mapping'!
neoJsonMapping: mapper
	"The class-side hook that the mapper of NeoJSON, and its port, resolve"

	mapper for: self do: [ :mapping |
		mapping mapInstVars: #( #id #name ).
		(mapping mapInstVar: #points) valueSchema: #SimdJSONTestPoints.
		(mapping mapInstVar: #stamp to: 'created-at') valueSchema: #SimdJSONTestStamp.
		(mapping mapInstVar: #tags) valueSchema: #SimdJSONTestTags ].
	mapper for: #SimdJSONTestPoints customDo: [ :mapping | mapping listOfElementSchema: Point ].
	mapper mapAllInstVarsFor: Point.
	mapper for: #SimdJSONTestStamp customDo: [ :mapping | mapping decoder: [ :string | string asNumber ] ].
	mapper for: #SimdJSONTestTags customDo: [ :mapping | mapping listOfType: OrderedCollection ]! !

!SimdJSONWriterTests methodsFor: 'helpers'!
write: anObject

	^ SimdJSONWriter toString: anObject! !

!SimdJSONWriterTests methodsFor: 'helpers'!
writeError: aBlock
	"The SimdJSONWriteError that aBlock signals, or nil"

	^ [ aBlock value. nil ]
		on: SimdJSONWriteError
		do: [ :error | error return: error ]! !

!SimdJSONWriterTests methodsFor: 'helpers'!
float: hi low: lo
	"A BoxedFloat64 of the bits hi (the sign, the exponent and the top of the mantissa) and lo"

	^ (Float new: 2)
		basicAt: 1 put: hi;
		basicAt: 2 put: lo;
		yourself! !

!SimdJSONWriterTests methodsFor: 'helpers'!
mapOf: aDictionary
	"The JSON of a Dictionary of String keys and Integer values, in the order of
	 keysAndValuesDo:, as NeoJSONWriter writes it"

	^ String streamContents: [ :stream |
		| first |
		first := true.
		stream nextPut: ${.
		aDictionary keysAndValuesDo: [ :key :value |
			first ifFalse: [ stream nextPut: $, ].
			first := false.
			stream nextPut: $"; nextPutAll: key; nextPutAll: '":'; print: value ].
		stream nextPut: $} ]! !

!SimdJSONWriterTests methodsFor: 'tests - values'!
testLiterals

	self assert: (self write: nil) equals: 'null'.
	self assert: (self write: true) equals: 'true'.
	self assert: (self write: false) equals: 'false'.
	{ 0. 1. -1. 42. SmallInteger maxVal. SmallInteger minVal. SmallInteger maxVal + 1. SmallInteger minVal - 1.
	  2 raisedTo: 63. (2 raisedTo: 63) negated. (2 raisedTo: 64) - 1. 2 raisedTo: 64. (2 raisedTo: 64) negated.
	  (2 raisedTo: 200) - 1. (10 raisedTo: 50) negated. 10 raisedTo: 18. 10 raisedTo: 27. (10 raisedTo: 36) + 1 }
		do: [ :each | self assert: (self write: each) equals: each printString ]! !

!SimdJSONWriterTests methodsFor: 'tests - values'!
testFloats
	"Float>>printOn:, as NeoJSONWriter writes them"

	{ 0.0. -0.0. 1.0. -1.0. 100.0. 0.1. 1.5. 123456.789. 1.0e15. 1.0e16. 1.0e20. 1.0e22. 123456789012345.0.
	  1234567890123456.0. 1.0e-4. 1.0e-5. 1.0e-7. 2.5e-3. Float fminDenormalized. Float fmin. Float fmax.
	  Float pi. Float e negated. 1.0e100. 1.0e-100. 1.7976931348623157e308. 4.9e-324 }
		do: [ :each | self assert: (self write: each) equals: each printString ]! !

!SimdJSONWriterTests methodsFor: 'tests - values'!
testRandomFloats
	"20000 doubles of random bits, boxed and as the VM normalizes them, and 20000 decimals"

	| random float |
	random := Random seed: 20261027.
	20000 timesRepeat: [
		float := self float: (random nextInteger: 16r100000000) - 1 low: (random nextInteger: 16r100000000) - 1.
		float isFinite ifTrue: [
			self assert: (self write: float) equals: float printString.
			self assert: (self write: { float * 1 }) equals: '[' , float printString , ']' ] ].
	20000 timesRepeat: [
		float := (random nextInteger: 100000000) / ((10 raisedTo: (random nextInteger: 12)) / 1.0).
		self assert: (self write: float) equals: float printString ]! !

!SimdJSONWriterTests methodsFor: 'tests - values'!
testNonFinite
	"JSON has no NaN or infinity: an error, or null with nonFiniteAsNull:"

	{ Float nan. Float infinity. Float negativeInfinity } do: [ :each |
		| error |
		error := self writeError: [ self write: { 1. each } ].
		self assert: error notNil.
		self assert: error code equals: 2.
		self assert: error culprit isNaN equals: each isNaN.
		self assert: (SimdJSONWriter new nonFiniteAsNull: true; stringFor: { 1. each }) equals: '[1,null]' ]! !

!SimdJSONWriterTests methodsFor: 'tests - strings'!
testStrings

	| controls |
	self assert: (self write: '') equals: '""'.
	self assert: (self write: 'abc') equals: '"abc"'.
	self assert: (self write: 'a"b\c/d') equals: '"a\"b\\c/d"'.
	self assert: (self write: #foo) equals: '"foo"'.
	controls := String new: 32.
	1 to: 32 do: [ :i | controls at: i put: (Character value: i - 1) ].
	self
		assert: (self write: controls)
		equals: '"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007\b\t\n\u000B\f\r\u000E\u000F\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017\u0018\u0019\u001A\u001B\u001C\u001D\u001E\u001F"'.
	self
		assert: (self write: (self string: #( 127 233 255 )))
		equals: '"' , (self string: #( 127 233 255 )) , '"'.
	self
		assert: (self write: (self string: #( 97 8364 16r1D11E 16rD800 )))
		equals: '"' , (self string: #( 97 8364 16r1D11E 16rD800 )) , '"'.
	self assert: (self write: (self string: #( 8364 ))) class equals: WideString.
	self assert: (self write: (self string: #( 8364 )) asSymbol) equals: '"' , (self string: #( 8364 )) , '"'! !

!SimdJSONWriterTests methodsFor: 'tests - strings'!
testAsciiOnly

	| writer |
	writer := SimdJSONWriter new asciiOnly: true; yourself.
	self
		assert: (writer stringFor: (self string: #( 97 126 127 233 8364 16r1D11E 16rD800 34 10 )))
		equals: '"a~\u007F\u00E9\u20AC\uD834\uDD1E\uD800\"\n"'.
	self assert: (writer stringFor: (self string: #( 233 ))) class equals: ByteString! !

!SimdJSONWriterTests methodsFor: 'tests - strings'!
testCodePointOutOfRange

	| string error |
	string := self string: #( 97 16r10FFFF ).
	self assert: (self write: string) equals: '"' , string , '"'.
	string := WideString new: 1.
	string at: 1 put: (Character value: 16r110000).
	error := self writeError: [ self write: { string } ].
	self assert: error code equals: 4.
	self assert: error culprit equals: string.
	self assert: error messageText equals: 'Character Unicode code point outside encoder range'! !

!SimdJSONWriterTests methodsFor: 'tests - structures'!
testStructures

	| collection |
	self assert: (self write: #()) equals: '[]'.
	self assert: (self write: Dictionary new) equals: '{}'.
	self assert: (self write: OrderedCollection new) equals: '[]'.
	self assert: (self write: #( 1 #( 2 #() ) 'x' #y nil )) equals: '[1,[2,[]],"x","y",null]'.
	collection := OrderedCollection withAll: #( 1 2 3 ).
	collection removeFirst.
	self assert: (self write: collection) equals: '[2,3]'.
	collection := OrderedCollection new.
	1 to: 20 do: [ :i | collection addFirst: i ].
	self assert: (self write: collection) equals: '[20,19,18,17,16,15,14,13,12,11,10,9,8,7,6,5,4,3,2,1]'.
	self
		assert: (self write: { Dictionary new at: 'a' put: (OrderedCollection with: #( 1 ) with: Dictionary new); yourself })
		equals: '[{"a":[[1],{}]}]'! !

!SimdJSONWriterTests methodsFor: 'tests - structures'!
testDictionaryOrder
	"The order of keysAndValuesDo:, through growth and removals"

	| dictionary |
	dictionary := Dictionary new.
	1 to: 300 do: [ :i | dictionary at: 'k' , i printString put: i ].
	1 to: 300 by: 7 do: [ :i | dictionary removeKey: 'k' , i printString ].
	self assert: (self write: dictionary) equals: (self mapOf: dictionary).
	dictionary := Dictionary new.
	dictionary at: #sym put: 1; at: (self string: #( 8364 )) put: 2.
	self assert: (SimdJSONReader fromString: (self write: dictionary)) equals:
		(Dictionary new at: 'sym' put: 1; at: (self string: #( 8364 )) put: 2; yourself)! !

!SimdJSONWriterTests methodsFor: 'tests - structures'!
testPretty

	| writer tab lf |
	tab := String with: Character tab.
	lf := String with: Character lf.
	writer := SimdJSONWriter new prettyPrint: true; newLine: lf; yourself.
	self assert: (writer stringFor: #()) equals: '[ ]'.
	self assert: (writer stringFor: Dictionary new) equals: '{ }'.
	self assert: (writer stringFor: 1) equals: '1'.
	self
		assert: (writer stringFor: { 1. Dictionary new at: 'a' put: #( 2 ); yourself. #() })
		equals: '[' , lf , tab , '1,' , lf , tab , '{' , lf , tab , tab , '"a" : [' , lf , tab , tab , tab , '2'
			, lf , tab , tab , ']' , lf , tab , '},' , lf , tab , '[ ]' , lf , ']'.
	writer newLine: (String with: Character cr with: Character lf).
	self
		assert: (writer stringFor: #( 1 ))
		equals: '[' , (String with: Character cr with: Character lf) , tab , '1' , (String with: Character cr with: Character lf) , ']'.
	self
		assert: (SimdJSONWriter toStringPretty: #( 1 ))
		equals: '[' , OSPlatform current lineEnding , tab , '1' , OSPlatform current lineEnding , ']'! !

!SimdJSONWriterTests methodsFor: 'tests - structures'!
testForwarders
	"A ByteString that gets a wide character becomes a WideString through a forwarder"

	| string dictionary |
	string := 'abc' copy.
	string at: 2 put: (Character value: 8364).
	self assert: (self write: { string }) equals: '["' , string , '"]'.
	string := 'key' copy.
	dictionary := Dictionary new.
	dictionary at: string put: 1.
	string at: 1 put: (Character value: 8364).
	self assert: (self write: dictionary) equals: '{"' , string , '":1}'! !

!SimdJSONWriterTests methodsFor: 'tests - output'!
testBytes

	| value |
	value := { 'a'. self string: #( 233 8364 16r1D11E ). 1.5 }.
	self assert: (SimdJSONWriter toBytes: value) class equals: ByteArray.
	self assert: (SimdJSONWriter toBytes: value) equals: (self write: value) utf8Encoded.
	self assert: (SimdJSONWriter toBytes: #()) equals: '[]' asByteArray! !

!SimdJSONWriterTests methodsFor: 'tests - output'!
testStreams

	| stream writer |
	stream := WriteStream on: String new.
	writer := SimdJSONWriter on: stream.
	writer nextPut: #( 1 2 ).
	writer nextPut: (self string: #( 8364 )).
	self assert: stream contents equals: '[1,2]"' , (self string: #( 8364 )) , '"'.
	stream := WriteStream on: ByteArray new.
	(SimdJSONWriter on: stream) nextPut: { self string: #( 233 ) }.
	self assert: stream contents equals: ('["' , (self string: #( 233 )) , '"]') utf8Encoded! !

!SimdJSONWriterTests methodsFor: 'tests - output'!
testRoundTrip

	| value |
	value := SimdJSONReader fromString: self probeJSON.
	self assert: (SimdJSONReader fromString: (self write: value)) equals: value.
	self assert: (SimdJSONReader fromString: (SimdJSONWriter toStringPretty: value)) equals: value.
	self assert: (SimdJSONReader fromBytes: (SimdJSONWriter toBytes: value)) equals: value.
	value := { (2 raisedTo: 100) negated. 0.1. -0.0. 'a"\' , (self string: #( 0 31 233 8364 16r1D11E )). nil. true }.
	self assert: (SimdJSONReader fromString: (self write: value)) equals: value.
	self assert: (SimdJSONReader fromString: (SimdJSONWriter new asciiOnly: true; stringFor: value)) equals: value! !

!SimdJSONWriterTests methodsFor: 'tests - errors'!
testErrors

	| error cycle |
	error := self writeError: [ self write: (Dictionary new at: 1 put: 2; yourself) ].
	self assert: error code equals: 1.
	self assert: error culprit equals: 1.
	self assert: error messageText equals: 'JSON key names in objects must be strings'.
	cycle := Array new: 1.
	cycle at: 1 put: cycle.
	error := self writeError: [ self write: cycle ].
	self assert: error code equals: 3.
	self assert: (error messageText includesSubstring: 'a cycle')! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testOtherObjects
	"What the plugin leaves to the image, written as NeoJSONWriter writes it"

	self assert: (self write: 1 / 4) equals: '0.25'.
	self assert: (self write: { 1 / 3. 3 / 2. 2 }) equals: '[0.3333333333333333,1.5,2]'.
	self assert: (self write: (3 / 2 asScaledDecimal: 2)) equals: '1.5'.
	self assert: (self write: (Set with: 7)) equals: '[7]'.
	self assert: (self write: (1 to: 3)) equals: '[1,2,3]'.
	self assert: (self write: #[ 1 2 ]) equals: '[1,2]'.
	self assert: (self write: (Bag with: 'x')) equals: '["x"]'.
	self
		assert: (self write: (OrderedDictionary new at: 'b' put: 1; at: 'a' put: 2; yourself))
		equals: '{"b":1,"a":2}'.
	self assert: (self write: (IdentityDictionary new at: #k put: 1 / 2; yourself)) equals: '{"k":0.5}'.
	self assert: (self write: (SmallDictionary new at: 'k' put: nil; yourself)) equals: '{"k":null}'.
	{ $a. Object new. 1 @ 2 } do: [ :each |
		self should: [ self write: { 1. each } ] raise: SimdJSONMappingNotFound ]! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testHolesLikePlainValues
	"An object the image writes in the middle of the plugin's output gives the same text as
	its plain equivalent, at every level, in every configuration and in bytes"

	| wide holed plain |
	wide := self string: #( 233 8364 16r1D11E ).
	holed := Dictionary new
		at: 'a' put: (Set with: (Dictionary new at: 'b' put: 1 / 2; at: wide put: (OrderedDictionary new at: 'c' put: { 3 / 4 }; yourself); yourself));
		at: 'd' put: { wide. 1 / 8. #( 1 2 ) asOrderedCollection. (1 to: 2) };
		yourself.
	plain := Dictionary new
		at: 'a' put: { Dictionary new at: 'b' put: 0.5; at: wide put: (Dictionary new at: 'c' put: #( 0.75 ); yourself); yourself };
		at: 'd' put: { wide. 0.125. #( 1 2 ) asOrderedCollection. #( 1 2 ) };
		yourself.
	{ [ :w | w ]. [ :w | w prettyPrint: true; newLine: (String with: Character lf) ]. [ :w | w asciiOnly: true ].
	  [ :w | w prettyPrint: true; asciiOnly: true; newLine: (String with: Character cr with: Character lf) ] } do: [ :configure |
		| writer |
		writer := SimdJSONWriter new.
		configure value: writer.
		self assert: (writer stringFor: holed) equals: (writer stringFor: plain).
		self assert: (writer bytesFor: holed) equals: (writer bytesFor: plain).
		self assert: (writer bytesFor: holed) equals: (writer stringFor: plain) utf8Encoded ].
	self assert: (SimdJSONReader fromString: (self write: holed)) equals: (SimdJSONReader fromString: (self write: plain))! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testPrettyHoles

	| writer tab lf |
	tab := String with: Character tab.
	lf := String with: Character lf.
	writer := SimdJSONWriter new prettyPrint: true; newLine: lf; yourself.
	self
		assert: (writer stringFor: (Dictionary new at: 'a' put: (Set with: (1 / 2)); yourself))
		equals: '{' , lf , tab , '"a" : [' , lf , tab , tab , '0.5' , lf , tab , ']' , lf , '}'.
	self assert: (writer stringFor: Set new) equals: '[ ]'.
	self assert: (writer stringFor: { OrderedDictionary new }) equals: '[' , lf , tab , '{ }' , lf , ']'! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testMappings

	| writer object |
	writer := SimdJSONWriter new.
	writer mapInstVarsFor: Point.
	self assert: (writer stringFor: 1 @ 2) equals: '{"x":1,"y":2}'.
	self assert: (writer stringFor: { 1 @ 2. nil }) equals: '[{"x":1,"y":2},null]'.
	object := SimdJSONReader
		fromString: '{"id":7,"name":"seven","points":[{"x":1,"y":2},{"x":3,"y":4}],"created-at":"1234","tags":["a","b"]}'
		as: SimdJSONTestObject.
	self
		assert: (SimdJSONWriter toString: object)
		equals: '{"id":7,"name":"seven","points":[{"x":1,"y":2},{"x":3,"y":4}],"created-at":1234,"tags":["a","b"]}'.
	object name: nil.
	self assert: ((SimdJSONWriter toString: object) includesSubstring: '"name"') not.
	self assert: ((SimdJSONWriter new writeNil: true; stringFor: object) includesSubstring: '"name":null').
	self
		should: [ SimdJSONWriter toString: { 1 @ 2 } ]
		raise: SimdJSONMappingNotFound
		withExceptionDo: [ :error | self assert: error messageText equals: 'No mapping found for Point in SimdJSONWriter' ]! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testCustomMappings

	| stream writer |
	stream := WriteStream on: String new.
	writer := SimdJSONWriter on: stream.
	writer for: #Pair customDo: [ :mapping | mapping encoder: [ :point | { point x. point y } ] ].
	writer for: #Pairs customDo: [ :mapping | mapping listOfElementSchema: #Pair ].
	writer for: #Counts customDo: [ :mapping | mapping mapWithValueSchema: #Pair ].
	writer nextPut: (3 @ 4) as: #Pair.
	writer nextPut: { 1 @ 2. 5 @ 6 } as: #Pairs.
	writer nextPut: (Dictionary new at: 'p' put: 7 @ 8; yourself) as: #Counts.
	writer nextPut: nil as: #Pair.
	self assert: stream contents equals: '[3,4][[1,2],[5,6]]{"p":[7,8]}null'! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testOwnWriters
	"A class that writes itself with neoJsonOn:, NeoJSON's protocol"

	| own |
	own := SimdJSONTestOwnWriter new value: 3.
	self assert: (self write: own) equals: '{"own":3}'.
	self assert: (self write: { own. own }) equals: '[{"own":3},{"own":3}]'.
	self
		assert: (SimdJSONWriter new prettyPrint: true; newLine: (String with: Character lf); stringFor: { own })
		equals: '[' , (String with: Character lf with: Character tab) , '{' , (String with: Character lf) , (String new: 2 withAll: Character tab)
			, '"own" : 3' , (String with: Character lf with: Character tab) , '}' , (String with: Character lf) , ']'! !

!SimdJSONWriterTests methodsFor: 'tests - objects'!
testFloatPrinter

	| writer |
	writer := SimdJSONWriter new floatPrinter: SimdJSONTestFloatPrinter new; yourself.
	self assert: (writer stringFor: { 1.0. 2.5. 1 / 3. 7. 'x' }) equals: '[1.00,2.50,0.33,7,"x"]'.
	self assert: (writer stringFor: 0.125) equals: '0.13'! !

!SimdJSONWriterTests methodsFor: 'tests - output'!
testStreamingProtocol

	| stream writer lf |
	lf := String with: Character lf.
	stream := WriteStream on: String new.
	writer := SimdJSONWriter on: stream.
	writer writeMapStreamingDo: [ :map |
		map writeKey: 'a' value: 1.
		map writeKey: #b value: #( 1 2 ).
		map writeKey: 'c' value: (1 / 2) ].
	writer writeListStreamingDo: [ :list |
		list writeElement: 'x'.
		list writeElement: (Set with: true) ].
	writer writeNull; writeBoolean: false; writeInteger: (2 raisedTo: 70); writeFloat: 0.5; writeString: #sym.
	self assert: stream contents equals: '{"a":1,"b":[1,2],"c":0.5}["x",[true]]nullfalse11805916207174113034240.5"sym"'.
	stream := WriteStream on: String new.
	(SimdJSONWriter on: stream) newLine: lf; nextPutAll: { 1. 'x'. Set new. Dictionary new }.
	self assert: stream contents equals: '1' , lf , '"x"' , lf , '[]' , lf , '{}' , lf.
	stream := WriteStream on: ByteArray new.
	(SimdJSONWriter on: stream) nextPut: { 1 / 2. self string: #( 233 8364 ) }.
	self assert: stream contents equals: ('[0.5,"' , (self string: #( 233 8364 )) , '"]') utf8Encoded! !

!SimdJSONWriterTests methodsFor: 'tests - errors'!
testErrorsInTheImage
	"The errors of the objects that the image writes, as those of the plugin"

	| cycle error |
	error := self writeError: [ self write: (OrderedDictionary new at: 1 put: 2; yourself) ].
	self assert: error code equals: 1.
	self assert: error messageText equals: 'JSON key names in objects must be strings'.
	error := self writeError: [ self write: (Set with: Float nan) ].
	self assert: error code equals: 2.
	self assert: (SimdJSONWriter new nonFiniteAsNull: true; stringFor: (Set with: Float infinity)) equals: '[null]'.
	cycle := IdentitySet new.
	cycle add: cycle.
	error := self writeError: [ self write: cycle ].
	self assert: error code equals: 3.
	cycle := OrderedCollection new.
	cycle add: (Set with: cycle).
	error := self writeError: [ self write: cycle ].
	self assert: error code equals: 3! !

!SimdJSONWriterTests methodsFor: 'tests - errors'!
testPluginMissing

	SimdJSONReader testHook: #pluginMissing value: true.
	self should: [ self write: #( 1 ) ] raise: SimdJSONResourceError! !

!SimdJSONTestOwnWriter methodsFor: 'accessing'!
value: anObject

	value := anObject! !

!SimdJSONTestOwnWriter methodsFor: 'writing'!
neoJsonOn: aWriter
	"NeoJSON's protocol of an object that writes itself"

	aWriter writeMapStreamingDo: [ :map | map writeKey: 'own' value: value ]! !

!SimdJSONTestFloatPrinter methodsFor: 'printing'!
printFloat: aFloat on: aStream

	aStream nextPutAll: (aFloat printShowingDecimalPlaces: 2)! !
