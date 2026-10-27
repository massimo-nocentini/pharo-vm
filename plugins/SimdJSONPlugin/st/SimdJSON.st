"SimdJSON.st - a JSON reader for Pharo 15 whose parsing is done by simdjson, in the
SimdJSONPlugin of this VM (plugins/SimdJSONPlugin, docs/SimdJSON.md), with the API of
NeoJSONReader and no other dependency: NeoJSON need not be in the image, and when it is,
SimdJSONReader still has no NeoJSON class in its hierarchy and sends nothing to NeoJSON.

SimdJSONReader is a subclass of SimdJSONMapper, a port of NeoJSON's mapper and mapping
classes (the reading side).  It is strict RFC 8259: what simdjson refuses, NeoJSON's
leniencies included (01, 1., a control character in a string), raises
SimdJSONParseError.  File it in with:

	pharo --headless Pharo.image --no-default-preferences st --save --quit SimdJSON.st

Filing it in again is harmless; over an older SimdJSON.st whose SimdJSONReader was a
subclass of NeoJSONReader (its neo mode), the class is made again.

The primitive (primitiveRead) answers a whole value at a time: either Dictionaries and
Arrays built in C, when the reader is configured as NeoJSON's defaults, or a tree of
Arrays and SimdJSONFlatMaps (k1 v1 k2 v2 ...) that the reader turns into mapClass and
listClass instances, or walks as a cursor for the mappings.  Numbers whose token has an
exponent, and integers beyond 64 bits, come back as their text, which NeoJSON's rules
convert, so 1e2 is 100 and 1e-2 is 1/100.

The classes SimdJSONMapper, SimdJSONMapping, SimdJSONObjectMapping, SimdJSONCustomMapping,
SimdJSONPropertyMapping and SimdJSONMappingNotFound, and SimdJSONNumberParser, are ports
of NeoJSON (https://github.com/svenvc/NeoJSON), under its MIT licence:

	Copyright (C) 2012 Sven Van Caekenberghe

	Permission is hereby granted, free of charge, to any person obtaining a copy of this
	software and associated documentation files (the 'Software'), to deal in the Software
	without restriction, including without limitation the rights to use, copy, modify,
	merge, publish, distribute, sublicense, and/or sell copies of the Software, and to
	permit persons to whom the Software is furnished to do so, subject to the following
	conditions:

	The above copyright notice and this permission notice shall be included in all copies
	or substantial portions of the Software.

	THE SOFTWARE IS PROVIDED 'AS IS', WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED,
	INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
	PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
	HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
	CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE
	OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

The file is in the chunk format that Pharo 15 reads: every class in the first chunk, one
method per section of methods, ASCII only, and no method names a NeoJSON class."
!
"SimdJSON: classes.  Every class goes through the globals: Pharo 15 compiles the whole
 DoIt first, and a literal reference to a class this DoIt creates is an undeclared
 variable.  A SimdJSONReader of an older SimdJSON.st (a NeoJSONReader in neo mode, or with
 the slots of its fallback) is removed first, so that none of its methods is recompiled
 against the slots of this one"
| g |
g := Smalltalk globals.
(g at: #SimdJSONReader ifAbsent: [ nil ]) ifNotNil: [ :old |
	(old superclass name == #SimdJSONMapper and: [ (old slotNames includes: #delegating) not ])
		ifFalse: [ old removeFromSystem ] ].
(Object << #SimdJSONFlatMap layout: VariableLayout; tag: 'Core'; package: 'SimdJSON') install.
(Object << #SimdJSONFailure slots: { #ec }; tag: 'Core'; package: 'SimdJSON') install.
(Object << #SimdJSONNumberParser slots: { #readStream }; tag: 'Core'; package: 'SimdJSON') install.
(Error << #SimdJSONParseError slots: { #code. #position }; tag: 'Errors'; package: 'SimdJSON') install.
(Error << #SimdJSONResourceError slots: { #code }; tag: 'Errors'; package: 'SimdJSON') install.
(Error << #SimdJSONMappingNotFound slots: { #object. #collection }; tag: 'Errors'; package: 'SimdJSON') install.
(Object << #SimdJSONMapper slots: { #mappings }; tag: 'Mapping'; package: 'SimdJSON') install.
(Object << #SimdJSONMapping tag: 'Mapping'; package: 'SimdJSON') install.
((g at: #SimdJSONMapping) << #SimdJSONObjectMapping slots: { #subjectClass. #properties. #allowNil };
	tag: 'Mapping'; package: 'SimdJSON') install.
((g at: #SimdJSONMapping) << #SimdJSONCustomMapping slots: { #schemaName. #reader. #writer };
	tag: 'Mapping'; package: 'SimdJSON') install.
(Object << #SimdJSONPropertyMapping slots: { #propertyName. #valueSchema. #getter. #setter };
	tag: 'Mapping'; package: 'SimdJSON') install.
((g at: #SimdJSONMapper) << #SimdJSONReader
	slots: #( #readStream #mapClass #listClass #propertyNamesAsSymbols #source #streamKind #index
		#stop #depth #pending #hasPending #topStart #multi #exactNumbers #nativeDictionaries
		#keyTable #originalStream );
	sharedVariables: { #Statistics. #SessionToken. #PluginInfo. #NativeVerdict. #NativeEnabled.
		#MaxInputUnits. #TestHooks };
	tag: 'Core'; package: 'SimdJSON') install.
Transcript show: 'SimdJSON: SimdJSONReader loaded, a subclass of ', (g at: #SimdJSONReader) superclass name; cr.
!

!SimdJSONFailure class methodsFor: 'instance creation'!
on: errorCode
	"The error code of a failed primitive of the plugin (nil when the plugin is missing)"

	^ self new
		ec: errorCode;
		yourself
! !

!SimdJSONFailure methodsFor: 'accessing'!
ec
	^ ec
! !

!SimdJSONFailure methodsFor: 'accessing'!
ec: errorCode
	ec := errorCode
! !

!SimdJSONFailure methodsFor: 'testing'!
isNoMemory
	"An object allocation failed, after the VM's own retries"

	^ ec == #'insufficient object memory'
! !

!SimdJSONFailure methodsFor: 'testing'!
isNoCMemory
	^ ec == #'insufficient C memory'
! !

!SimdJSONFailure methodsFor: 'testing'!
isUnavailable
	"The module could not be loaded, or it has no such primitive"

	^ ec isNil or: [ ec == #'not found' ]
! !

!SimdJSONNumberParser class methodsFor: 'parsing'!
parse: aString
	"Convert the text of a JSON number token with NeoJSON's rules: 1e2 is 100, 1e-2 is 1/100"

	^ self new
		on: aString readStream;
		parseNumber
! !

!SimdJSONNumberParser methodsFor: 'initialize-release'!
on: aReadStream
	readStream := aReadStream
! !

!SimdJSONNumberParser methodsFor: 'private'!
error: text
	SimdJSONParseError new signal: text
! !

!SimdJSONNumberParser methodsFor: 'parsing'!
parseNumber
	| negated number isFloat |
	negated := readStream peekFor: $-.
	number := self parseNumberInteger.
	isFloat := (readStream peekFor: $.)
		ifTrue: [ number := number + self parseNumberFraction. true ]
		ifFalse: [ false ].
	((readStream peekFor: $e) or: [ readStream peekFor: $E ])
		ifTrue: [ number := number * self parseNumberExponent ].
	isFloat ifTrue: [ number := number asFloat ].
	negated
		ifTrue: [ number := number negated ].
	^ number
! !

!SimdJSONNumberParser methodsFor: 'parsing'!
parseNumberExponent
	| number negated |
	number := 0.
	(negated := readStream peekFor: $-)
		ifFalse: [ readStream peekFor: $+ ].
	(readStream atEnd or: [ readStream peek isDigit not ])
		ifTrue: [ self error: 'number exponent expected' ].
	[ readStream atEnd not and: [ readStream peek isDigit ] ]
		whileTrue: [ number := 10 * number + readStream next digitValue ].
	negated
		ifTrue: [ number := number negated ].
	number positive
		ifTrue: [ number > Float emax ifTrue: [ self error: 'number exponent too large' ] ]
		ifFalse: [ number < Float emin ifTrue: [ self error: 'number exponent too small' ] ].
	^ 10 raisedTo: number
! !

!SimdJSONNumberParser methodsFor: 'parsing'!
parseNumberFraction
	| number power |
	number := 0.
	power := 1.
	[ readStream atEnd not and: [ readStream peek isDigit ] ]
		whileTrue: [
			number := 10 * number + readStream next digitValue.
			power := power * 10 ].
	^ number / power
! !

!SimdJSONNumberParser methodsFor: 'parsing'!
parseNumberInteger
	| number |
	number := nil.
	[ readStream atEnd not and: [ readStream peek isDigit ] ]
		whileTrue: [
			number := 10 * (number ifNil: [ 0 ]) + readStream next digitValue ].
	number ifNil: [ self error: 'Integer digit expected' ].
	^ number
! !

!SimdJSONParseError methodsFor: 'accessing'!
code
	"The simdjson error code (or the plugin's: 1001 BOM, 1002 number token check), or nil"

	^ code
! !

!SimdJSONParseError methodsFor: 'accessing'!
code: anInteger
	code := anInteger
! !

!SimdJSONParseError methodsFor: 'accessing'!
position
	"The index of the source unit (character, or byte of a ByteArray) where the value starts"

	^ position
! !

!SimdJSONParseError methodsFor: 'accessing'!
position: anInteger
	position := anInteger
! !

!SimdJSONResourceError methodsFor: 'accessing'!
code
	"The error code of the primitive, or #tooLarge"

	^ code
! !

!SimdJSONResourceError methodsFor: 'accessing'!
code: anObject
	code := anObject
! !

!SimdJSONMappingNotFound class methodsFor: 'instance creation'!
signalFor: anObject
	^ self new
		object: anObject;
		signal
! !

!SimdJSONMappingNotFound class methodsFor: 'instance creation'!
signalFor: anObject in: aCollection
	^ self new
		object: anObject;
		collection: aCollection;
		signal
! !

!SimdJSONMappingNotFound methodsFor: 'accessing'!
collection
	^ collection
! !

!SimdJSONMappingNotFound methodsFor: 'accessing'!
collection: aCollection
	collection := aCollection
! !

!SimdJSONMappingNotFound methodsFor: 'accessing'!
messageText
	^ self standardMessageText
! !

!SimdJSONMappingNotFound methodsFor: 'accessing'!
object
	^ object
! !

!SimdJSONMappingNotFound methodsFor: 'accessing'!
object: anObject
	object := anObject
! !

!SimdJSONMappingNotFound methodsFor: 'private'!
standardMessageText
	^ String streamContents: [ :stream |
		stream << 'No mapping found for '.
		stream print: self object.
		stream << ' in '.
		stream print: self collection class ]
! !

!SimdJSONMapper methodsFor: 'private'!
customMappingFor: schemaName
	^ self mappings
		at: schemaName
		ifAbsentPut: [
			SimdJSONCustomMapping new
				schemaName: schemaName;
				yourself ]
! !

!SimdJSONMapper methodsFor: 'mapping'!
for: schemaName customDo: block
	"Execute the code in block to further customize the mapping for schemaName.
	Create and add a new custom mapping for schemaName if necessary."

	| mapping |
	mapping := self customMappingFor: schemaName.
	block value: mapping.
	^ mapping
! !

!SimdJSONMapper methodsFor: 'mapping'!
for: smalltalkClass do: block
	"Execute the code in block to further customize the mapping for schemaName.
	Create and add a new standard object mapping for schemaName if necessary."

	| mapping |
	mapping := self objectMappingFor: smalltalkClass.
	block value: mapping.
	^ mapping
! !

!SimdJSONMapper methodsFor: 'convenience'!
mapAllInstVarsFor: smalltalkClass
	"Map the instance variables of smalltalkClass and all its super classes"

	| mapping |
	mapping := self objectMappingFor: smalltalkClass.
	mapping mapAllInstVars.
	^ mapping
! !

!SimdJSONMapper methodsFor: 'convenience'!
mapInstVarsFor: smalltalkClass
	"Map the instance variables of smalltalkClass"

	| mapping |
	mapping := self objectMappingFor: smalltalkClass.
	mapping mapInstVars.
	^ mapping
! !

!SimdJSONMapper methodsFor: 'accessing'!
mappingFor: schema
	^ self
		mappingFor: schema
		ifAbsent: [ SimdJSONMappingNotFound signalFor: schema in: self ]
! !

!SimdJSONMapper methodsFor: 'accessing'!
mappingFor: schema ifAbsent: block
	(self mappings includesKey: schema)
		ifFalse: [ self resolveMappingFor: schema ].
	^ self mappings
		at: schema
		ifAbsent: block
! !

!SimdJSONMapper methodsFor: 'private'!
mappings
	^ mappings
		ifNil: [ mappings := Dictionary new ]
! !

!SimdJSONMapper methodsFor: 'private'!
objectMappingFor: smalltalkClass
	^ self mappings
		at: smalltalkClass
		ifAbsentPut: [
			SimdJSONObjectMapping new
				subjectClass: smalltalkClass;
				yourself ]
! !

!SimdJSONMapper methodsFor: 'private'!
resolveMappingFor: class
	"The class-side hook of NeoJSON, neoJsonMapping: mapper, sends only the mapper protocol"

	(class respondsTo: #neoJsonMapping:)
		ifTrue: [
			class perform: #neoJsonMapping: with: self ]
! !

!SimdJSONMapping methodsFor: 'accessing'!
identifier
	self subclassResponsibility
! !

!SimdJSONMapping methodsFor: 'printing'!
printOn: stream
	super printOn: stream.
	stream nextPut: $(; print: self identifier; nextPut: $)
! !

!SimdJSONMapping methodsFor: 'parsing'!
readFrom: jsonReader
	self subclassResponsibility
! !

!SimdJSONObjectMapping methodsFor: 'initialize-release'!
allowNil
	"Set that I will allow nil/null values when I read/expect an object"

	allowNil := true
! !

!SimdJSONObjectMapping methodsFor: 'accessing'!
identifier
	^ subjectClass
! !

!SimdJSONObjectMapping methodsFor: 'initialize-release'!
initialize
	super initialize.
	properties := OrderedCollection new.
	allowNil := false
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapAccessor: accessor
	^ self mapAccessor: accessor to: accessor
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapAccessor: accessor mutator: mutator to: propertyName
	^ self
		mapProperty: propertyName
		getter: [ :object | object perform: accessor ]
		setter: [ :object :value | object perform: mutator with: value ]
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapAccessor: accessor to: propertyName
	^ self
		mapProperty: propertyName
		getter: [ :object | object perform: accessor ]
		setter: [ :object :value | object perform: accessor asMutator with: value ]
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapAccessors: accessors
	^ accessors collect: [ :each | self mapAccessor: each ]
! !

!SimdJSONObjectMapping methodsFor: 'convenience'!
mapAllInstVars
	^ self mapInstVars: subjectClass allInstVarNames
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapInstVar: instVarName
	^ self mapInstVar: instVarName to: instVarName
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapInstVar: instVarName to: propertyName
	| index |
	index := (subjectClass allInstVarNames collect: [ :each | each asSymbol ])
		indexOf: instVarName asSymbol
		ifAbsent: [ self error: 'Inst var not found: ', instVarName printString ].
	^ self
		mapProperty: propertyName asString
		getter: [ :object | object instVarAt: index ]
		setter: [ :object :value | object instVarAt: index put: value ]
! !

!SimdJSONObjectMapping methodsFor: 'convenience'!
mapInstVars
	^ self mapInstVars: subjectClass instVarNames
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapInstVars: instVarNames
	^ instVarNames collect: [ :each | self mapInstVar: each ]
! !

!SimdJSONObjectMapping methodsFor: 'mapping'!
mapProperty: propertyName getter: readBlock setter: writeBlock
	| propertyMapping |
	propertyMapping := SimdJSONPropertyMapping new.
	propertyMapping
		propertyName: propertyName;
		getter: readBlock;
		setter: writeBlock.
	properties removeAllSuchThat: [ :each | each propertyName = propertyMapping propertyName ].
	properties addLast: propertyMapping.
	^ propertyMapping
! !

!SimdJSONObjectMapping methodsFor: 'accessing'!
propertyNamed: propertyName ifAbsent: block
	| symbolPropertyName |
	symbolPropertyName := propertyName asSymbol.
	^ properties detect: [ :each | each propertyName = symbolPropertyName ] ifNone: block
! !

!SimdJSONObjectMapping methodsFor: 'parsing'!
readFrom: jsonReader
	"NeoJSON's, except that the value of an unknown key is skipped, not read"

	| anObject |
	anObject := subjectClass new.
	allowNil ifTrue: [
		jsonReader parseConstantDo: [ :value |
			^ value ifNotNil: [ jsonReader error: 'Unexpected boolean constant' ] ] ].
	jsonReader parseMapKeysDo: [ :key |
		(self propertyNamed: key ifAbsent: [ nil ])
			ifNil: [ jsonReader skipValue ]
			ifNotNil: [ :mapping |
				mapping readObject: anObject from: jsonReader ] ].
	^ anObject
! !

!SimdJSONObjectMapping methodsFor: 'accessing'!
subjectClass: smalltalkClass
	subjectClass := smalltalkClass
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
decoder: block
	"Set a decoder block that accepts the next object representation read
	and converts it into the actual object to be returned."

	self reader: [ :jsonReader | | representation |
		representation := jsonReader next.
		block value: representation ]
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
encoder: block
	"Kept for the mappings written for NeoJSON; this port has no writer"

	self writer: [ :jsonWriter :anObject | | representation |
		representation := block value: anObject.
		jsonWriter nextPut: representation ]
! !

!SimdJSONCustomMapping methodsFor: 'accessing'!
identifier
	^ schemaName
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
listOfElementSchema: elementSchema
	"Set the reader to a block that will create an instance of the
	standard listClass while reading a list of elements of type elementSchema."

	self reader: [ :jsonReader |
		jsonReader listClass streamContents: [ :stream |
			jsonReader parseListDo: [ | element |
				element := jsonReader nextAs: elementSchema.
				stream nextPut: element ] ] ].
	self writer: [ :jsonWriter :list |
		jsonWriter writeListStreamingDo: [ :listWriter |
			list do: [ :each |
				listWriter writeElement: each as: elementSchema ] ] ]
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
listOfType: collectionClass
	"Set the reader to a block that will create an instance of collectionClass
	using #streamContents: while reading a list of elements."

	self reader: [ :jsonReader |
		collectionClass streamContents: [ :stream |
			jsonReader parseListDo: [ | element |
				element := jsonReader next.
				stream nextPut: element ] ] ].
	self writer: [ :jsonWriter :list |
		jsonWriter writeListStreamingDo: [ :listWriter |
			list do: [ :each |
				listWriter writeElement: each ] ] ]
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
listOfType: collectionClass andElementSchema: elementSchema
	"Set the reader to a block that will create an instance of collectionClass
	using #streamContents: while reading a list of elements of type elementSchema."

	self reader: [ :jsonReader |
		collectionClass streamContents: [ :stream |
			jsonReader parseListDo: [ | element |
				element := jsonReader nextAs: elementSchema.
				stream nextPut: element ] ] ].
	self writer: [ :jsonWriter :list |
		jsonWriter writeListStreamingDo: [ :listWriter |
			list do: [ :each |
				listWriter writeElement: each as: elementSchema ] ] ]
! !

!SimdJSONCustomMapping methodsFor: 'mapping'!
mapWithValueSchema: valueSchema
	"Set the reader to a block that will create an instance of the
	standard mapClass while reading values of type valueSchema,
	with standard keys"

	self reader: [ :jsonReader | | map |
		map := jsonReader mapClass new.
		jsonReader parseMapKeysDo: [ :key | | value |
				value := jsonReader nextAs: valueSchema.
				map at: key put: value ].
		map ].
	self writer: [ :jsonWriter :map |
		jsonWriter writeMapStreamingDo: [ :mapWriter |
			map keysAndValuesDo: [ :key :value |
				mapWriter writeKey: key value: value as: valueSchema ] ] ]
! !

!SimdJSONCustomMapping methodsFor: 'parsing'!
readFrom: jsonReader
	^ reader
		ifNil: [ jsonReader next ]
		ifNotNil: [ reader value: jsonReader ]
! !

!SimdJSONCustomMapping methodsFor: 'accessing'!
reader: block
	"Set the reader to block, a 1 argument block that when given
	a jsonReader as argument reads the object that I represent.
	When there is no reader block, the default #next will be used."

	reader := block
! !

!SimdJSONCustomMapping methodsFor: 'accessing'!
schemaName: schema
	schemaName := schema
! !

!SimdJSONCustomMapping methodsFor: 'accessing'!
writer: block
	"Stored and never used: this port only reads"

	writer := block
! !

!SimdJSONPropertyMapping methodsFor: 'accessing'!
getter: block
	getter := block
! !

!SimdJSONPropertyMapping methodsFor: 'printing'!
printOn: stream
	super printOn: stream.
	stream nextPut: $(; print: propertyName; nextPut: $)
! !

!SimdJSONPropertyMapping methodsFor: 'accessing'!
propertyName
	^ propertyName
! !

!SimdJSONPropertyMapping methodsFor: 'accessing'!
propertyName: symbolOrString
	propertyName := symbolOrString asSymbol
! !

!SimdJSONPropertyMapping methodsFor: 'parsing'!
readObject: anObject from: jsonReader
	| value |
	value := jsonReader nextAs: valueSchema.
	setter value: anObject value: value
! !

!SimdJSONPropertyMapping methodsFor: 'accessing'!
setter: block
	setter := block
! !

!SimdJSONPropertyMapping methodsFor: 'accessing'!
valueSchema: schemaName
	valueSchema := schemaName
! !

!SimdJSONReader class methodsFor: 'instance creation'!
on: aReadStream
	"A ReadStream on a String or a ByteArray (UTF-8) is read by the plugin; any other
	stream that answers #next #atEnd (or #upToEnd) is read to its end here (on: waits for
	the end of a socket stream, and what is read from that stream afterwards is not seen
	by the reader); close closes it"

	^ self new
		on: aReadStream;
		yourself
! !

!SimdJSONReader class methodsFor: 'instance creation'!
onBytes: aByteArray
	"UTF-8 bytes; the positions are then byte indexes"

	^ self on: aByteArray readStream
! !

!SimdJSONReader class methodsFor: 'convenience'!
fromString: aString
	^ (self on: aString readStream) next
! !

!SimdJSONReader class methodsFor: 'convenience'!
fromString: aString as: schema
	^ (self on: aString readStream) nextAs: schema
! !

!SimdJSONReader class methodsFor: 'convenience'!
fromBytes: aByteArray
	^ (self onBytes: aByteArray) next
! !

!SimdJSONReader class methodsFor: 'convenience'!
fromBytes: aByteArray as: schema
	^ (self onBytes: aByteArray) nextAs: schema
! !

!SimdJSONReader class methodsFor: 'plugin'!
sessionMemo
	"What the plugin and the image answered in this session: { info (#unknown until
	asked). dictLayout. the messages of the error codes }; a new session, or a test hook,
	forgets it"

	SessionToken == Smalltalk session
		ifFalse: [
			SessionToken := Smalltalk session.
			PluginInfo := { #unknown. nil. Dictionary new }.
			NativeVerdict := nil ].
	^ PluginInfo
! !

!SimdJSONReader class methodsFor: 'plugin'!
pluginInfo
	"The answer of primitiveInfo, { protocol. plugin version. simdjson version.
	implementation. platform. parser capacity. cached bytes. stats }, or nil"

	| memo |
	memo := self sessionMemo.
	(memo at: 1) == #unknown ifTrue: [
		memo at: 1 put: ((self testHook: #pluginMissing) == true
			ifFalse: [
				self primInfo ifNotNil: [ :info |
					(info isArray and: [ info first = self protocol ]) ifTrue: [ info ] ] ]) ].
	^ memo at: 1
! !

!SimdJSONReader class methodsFor: 'plugin'!
protocol
	"The version of the primitives' contract that this file speaks; a plugin that answers
	another one in primitiveInfo is not used"

	^ 2
! !

!SimdJSONReader class methodsFor: 'plugin'!
pluginAvailable
	^ self pluginInfo notNil
! !

!SimdJSONReader class methodsFor: 'plugin'!
implementationName
	"The simdjson kernel: haswell, westmere, arm64, fallback..."

	^ self pluginInfo ifNotNil: [ :info | info at: 4 ]
! !

!SimdJSONReader class methodsFor: 'plugin'!
simdjsonVersion
	^ self pluginInfo ifNotNil: [ :info | info at: 3 ]
! !

!SimdJSONReader class methodsFor: 'plugin'!
platform
	"'native' or 'emscripten'"

	^ self pluginInfo ifNotNil: [ :info | info at: 5 ]
! !

!SimdJSONReader class methodsFor: 'plugin'!
pluginStatisticsKeys
	"The names of the counters of the plugin, the last slot of pluginInfo, in their order
	(SJ_NUM_STATS of sjBridge.h); parseNanos and buildNanos are cumulative nanoseconds"

	^ #( #calls #parses #cacheHits #builds #buildFailures #overBudget #measures
		#numberTexts #gateHits #enumerations #parseNanos #buildNanos )
! !

!SimdJSONReader class methodsFor: 'plugin'!
releaseParser
	"Free the memory of the parser of the plugin; the next read allocates it again"

	^ self primRelease: true
! !

!SimdJSONReader class methodsFor: 'plugin'!
errorMessageFor: code
	^ (self sessionMemo at: 3)
		at: code
		ifAbsentPut: [ (self primErrorMessage: code) ifNil: [ 'error ', code printString ] ]
! !

!SimdJSONReader class methodsFor: 'configuration'!
maxInputUnits
	"The largest value, in units of the source (characters, or bytes of UTF-8), that the
	plugin parses: 16 Mi on WebAssembly, where the memory a parser takes is never given
	back; none natively.  It applies to each value, so a stream of many small values (NDJSON)
	of any size is read by the plugin"

	MaxInputUnits == #none ifTrue: [ ^ nil ].
	^ MaxInputUnits ifNil: [
		self platform = 'emscripten' ifTrue: [ 16 * 1024 * 1024 ] ]
! !

!SimdJSONReader class methodsFor: 'configuration'!
maxInputUnits: anIntegerOrNil
	"nil removes the cap"

	MaxInputUnits := anIntegerOrNil ifNil: [ #none ]
! !

!SimdJSONReader class methodsFor: 'configuration'!
budgetChecked
	"Whether the plugin is given a budget, the free old space, and measures a large value
	before it builds it: on WebAssembly, and on a VM whose old space has a maximum.
	Elsewhere old space grows on demand: a shortage fails the primitive, the VM collects
	garbage and calls it again on the same parse, and read: grows memory (recover:)"

	^ self platform = 'emscripten'
		or: [ ((Smalltalk vm parameterAt: 67) ifNil: [ 0 ]) > 0 ]
! !

!SimdJSONReader class methodsFor: 'configuration'!
nativeDictionaries: aBoolean
	"false: never let the plugin build Dictionaries (the kill switch of the fast path).  The
	setting stays, through image saves too, until it is set back to true"

	NativeEnabled := aBoolean
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
nativeDictionariesWork
	| verdict |
	NativeEnabled == false ifTrue: [ ^ false ].
	(NativeVerdict == #passed and: [ SessionToken == Smalltalk session ]) ifTrue: [ ^ true ].
	self pluginAvailable ifFalse: [ ^ false ].
	NativeVerdict ifNil: [
		verdict := self runNativeSelfTest.
		verdict ifNotNil: [
			Transcript show: 'SimdJSON: Dictionaries are built in Smalltalk in this session: ', verdict; cr ].
		NativeVerdict := verdict ifNil: [ #passed ] ].
	^ NativeVerdict == #passed
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
dictLayout
	"The 0-based slots of Dictionary tally and array, and of Association key and value,
	4 bits each, for the Dictionaries the plugin builds"

	| memo |
	memo := self sessionMemo.
	^ (memo at: 2) ifNil: [
		memo at: 2 put: ((Dictionary allInstVarNames indexOf: #tally) - 1)
			+ (((Dictionary allInstVarNames indexOf: #array) - 1) << 4)
			+ (((Association allInstVarNames indexOf: #key) - 1) << 8)
			+ (((Association allInstVarNames indexOf: #value) - 1) << 12) ]
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
runNativeSelfTest
	"Answer nil when the Dictionaries that the plugin builds behave as the image's own
	(slots, String>>hash, the probing of Dictionary>>scanFor:), else what failed"

	| hooks statistics |
	(self testHook: #nativeSelfTestFails) == true
		ifTrue: [ ^ 'forced by the test hook #nativeSelfTestFails' ].
	hooks := TestHooks.
	statistics := Statistics.
	TestHooks := Statistics := nil.
	^ [ [ self nativeSelfTestPrecheck ifNil: [ self nativeSelfTestProbe ] ]
			on: Error
			do: [ :error | error class name, ': ', error messageText asString ] ]
		ensure: [
			TestHooks := hooks.
			Statistics := statistics ]
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
nativeSelfTestPrecheck
	| wide |
	((Dictionary allInstVarNames includesAll: #(tally array))
		and: [ (Association allInstVarNames includesAll: #(key value))
		and: [ Dictionary isVariable not and: [ Association isVariable not ] ] ])
			ifFalse: [ ^ 'the slots of Dictionary or Association' ].
	'abc' hash = (ByteString stringHash: 'abc' initialHash: ByteString hash)
		ifFalse: [ ^ 'ByteString>>hash' ].
	wide := String with: (Character value: 300).
	wide hash = (wide class stringHash: wide initialHash: ByteString hash)
		ifFalse: [ ^ 'WideString>>hash' ].
	#abc hash = 'abc' hash
		ifFalse: [ ^ 'Symbol>>hash' ].
	^ nil
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
nativeSelfTestJSON
	"Latin-1 and wide keys, a duplicate, empty containers, a number text, and an object of
	300 keys, whose array has the overflow header"

	^ String streamContents: [ :stream |
		stream
			nextPutAll: '{"a":1,"'; nextPut: (Character value: 233);
			nextPutAll: '":2,"'; nextPut: (Character value: 8364);
			nextPutAll: '":3,"a":4,"e":{},"l":[],"n":1e2,"o":{'.
		0 to: 299 do: [ :i |
			i > 0 ifTrue: [ stream nextPut: $, ].
			stream nextPutAll: '"k'; print: i; nextPutAll: '":'; print: i ].
		stream nextPutAll: '}}' ]
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
nativeSelfTestProbe
	"The same JSON built by the plugin and through the tree path, compared, looked up and
	changed alike"

	| json native tree |
	json := self nativeSelfTestJSON.
	native := self nativeSelfTestRead: json.
	tree := (self on: json readStream)
		nativeDictionaries: false;
		next.
	native class == Dictionary ifFalse: [ ^ 'no Dictionary built' ].
	(native = tree and: [ tree = native ]) ifFalse: [ ^ 'differs from the tree path' ].
	(native at: 'a') = 4 ifFalse: [ ^ 'the last of duplicate keys' ].
	{ tree -> native. (tree at: 'o') -> (native at: 'o') } do: [ :pair |
		pair key keysAndValuesDo: [ :key :value |
			((pair value includesKey: key) and: [ (pair value at: key) = value ])
				ifFalse: [ ^ 'the lookup of a key' ] ] ].
	1 to: 50 do: [ :i |
		native at: 'm', i printString put: i.
		tree at: 'm', i printString put: i ].
	(native = tree and: [ tree = native ]) ifFalse: [ ^ 'growth' ].
	native removeKey: 'a'.
	tree removeKey: 'a'.
	(native = tree and: [ tree = native and: [ native keys asSet = tree keys asSet ] ])
		ifFalse: [ ^ 'removeKey:' ].
	(native at: 'e') at: 'x' put: 1.
	(tree at: 'e') at: 'x' put: 1.
	(((native at: 'e') at: 'x') = 1 and: [ native = tree and: [ tree = native ] ])
		ifFalse: [ ^ 'an empty Dictionary' ].
	^ nil
! !

!SimdJSONReader class methodsFor: 'native dictionaries'!
nativeSelfTestRead: json
	| result |
	result := self
		primRead: json from: 1 to: json size flags: self flagExact + self flagDict budget: -1
		maxUnits: -1 objectClass: Dictionary associationClass: Association wideStringClass: WideString
		hashSeed: ByteString hash dictLayout: self dictLayout.
	result class == SimdJSONFailure
		ifTrue: [ ^ self error: 'primitiveRead failed: ', result ec printString ].
	(result at: 1) = 0
		ifFalse: [ ^ self error: 'primitiveRead answered status ', (result at: 1) printString ].
	^ self new finish: result
! !

!SimdJSONReader class methodsFor: 'statistics'!
statistics
	"The counters, in a new Dictionary: reads (primitive calls), fastValues (values answered by
	simdjson), overBudget, grows, numberTexts, nativeDictionaries (values built as
	Dictionaries in C), tooLarge"

	| statistics |
	Statistics ifNil: [ self resetStatistics ].
	statistics := Dictionary new.
	self statisticsKeys with: Statistics do: [ :key :count | statistics at: key put: count ].
	^ statistics
! !

!SimdJSONReader class methodsFor: 'statistics'!
resetStatistics
	Statistics := Array new: self statisticsKeys size withAll: 0
! !

!SimdJSONReader class methodsFor: 'statistics'!
statisticsKeys
	^ #(reads fastValues overBudget grows numberTexts nativeDictionaries tooLarge)
! !

!SimdJSONReader class methodsFor: 'statistics'!
count: aSymbol
	self count: aSymbol by: 1
! !

!SimdJSONReader class methodsFor: 'statistics'!
count: aSymbol by: anInteger
	| i |
	Statistics ifNil: [ self resetStatistics ].
	i := self statisticsKeys identityIndexOf: aSymbol.
	Statistics at: i put: (Statistics at: i) + anInteger
! !

!SimdJSONReader class methodsFor: 'testing'!
testHook: aSymbol value: anObject
	"Test-only overrides, nil removes one: #budget (the budget of every read),
	#pluginMissing (true: as if the module were absent), #nativeSelfTestFails (true)"

	TestHooks ifNil: [ TestHooks := Dictionary new ].
	anObject
		ifNil: [ TestHooks removeKey: aSymbol ifAbsent: [ ] ]
		ifNotNil: [ TestHooks at: aSymbol put: anObject ].
	TestHooks isEmpty ifTrue: [ TestHooks := nil ].
	SessionToken := nil
! !

!SimdJSONReader class methodsFor: 'testing'!
testHook: aSymbol
	^ TestHooks ifNotNil: [ :hooks | hooks at: aSymbol ifAbsent: [ nil ] ]
! !

!SimdJSONReader class methodsFor: 'primitives'!
flagNext
	"primRead: flag: scan for the end of the value, and parse only it (else the whole rest
	is parsed first, the fast path of fromString:)"

	^ 1
! !

!SimdJSONReader class methodsFor: 'primitives'!
flagExact
	"primRead: flag: numbers with an exponent, and integers beyond 64 bits, as their text"

	^ 2
! !

!SimdJSONReader class methodsFor: 'primitives'!
flagDict
	"primRead: flag: build Dictionaries, Associations and Arrays, not a tree"

	^ 4
! !

!SimdJSONReader class methodsFor: 'primitives'!
flagSymbolKeys
	"primRead: flag: the keys of the tree are indexes in an Array of the distinct keys"

	^ 8
! !

!SimdJSONReader class methodsFor: 'primitives'!
primRead: source from: start to: stop flags: flags budget: budget maxUnits: maxUnits
		objectClass: objClass associationClass: assocClassOrNil wideStringClass: wideClass
		hashSeed: seed dictLayout: layout
	"The JSON value at start in source (a String, or UTF-8 bytes), as
	{ status. value. nextIndex. fixups. keys. code }; flags are the sum of flagNext,
	flagExact, flagDict and flagSymbolKeys.  docs/SimdJSON.md, Primitive contract, has the
	whole contract"

	<primitive: 'primitiveRead' module: 'SimdJSONPlugin' error: ec>
	^ SimdJSONFailure on: ec
! !

!SimdJSONReader class methodsFor: 'primitives'!
primInfo
	<primitive: 'primitiveInfo' module: 'SimdJSONPlugin' error: ec>
	^ nil
! !

!SimdJSONReader class methodsFor: 'primitives'!
primErrorMessage: code
	<primitive: 'primitiveErrorMessage' module: 'SimdJSONPlugin' error: ec>
	^ nil
! !

!SimdJSONReader class methodsFor: 'primitives'!
primRelease: freeParser
	<primitive: 'primitiveRelease' module: 'SimdJSONPlugin' error: ec>
	^ nil
! !

!SimdJSONReader methodsFor: 'initialize-release'!
initialize
	super initialize.
	propertyNamesAsSymbols ifNil: [ propertyNamesAsSymbols := false ].
	depth := 0.
	multi := false.
	exactNumbers := true.
	nativeDictionaries := true.
	hasPending := false
! !

!SimdJSONReader methodsFor: 'initialize-release'!
on: aReadStream
	readStream := originalStream := aReadStream.
	self classifyStream
! !

!SimdJSONReader methodsFor: 'initialize-release'!
close
	"Close the stream given to on:, as NeoJSON does, also when I read it into a stream of
	my own"

	originalStream ifNotNil: [ originalStream close ].
	(readStream notNil and: [ readStream ~~ originalStream ]) ifTrue: [ readStream close ].
	originalStream := readStream := nil
! !

!SimdJSONReader methodsFor: 'initialize-release'!
mapClass: aClass
	mapClass := aClass
! !

!SimdJSONReader methodsFor: 'initialize-release'!
listClass: aClass
	listClass := aClass
! !

!SimdJSONReader methodsFor: 'initialize-release'!
propertyNamesAsSymbols: aBoolean
	propertyNamesAsSymbols := aBoolean
! !

!SimdJSONReader methodsFor: 'initialize-release'!
exactNumbers: aBoolean
	"false: every number with a fraction or an exponent is a Float (1e2 is 100.0), faster"

	exactNumbers := aBoolean
! !

!SimdJSONReader methodsFor: 'initialize-release'!
nativeDictionaries: aBoolean
	"false: Dictionaries are built by the image, with NeoJSON's capacity and order"

	nativeDictionaries := aBoolean
! !

!SimdJSONReader methodsFor: 'accessing'!
mapClass
	^ mapClass ifNil: [ Dictionary ]
! !

!SimdJSONReader methodsFor: 'accessing'!
listClass
	^ listClass ifNil: [ Array ]
! !

!SimdJSONReader methodsFor: 'accessing'!
next
	"One JSON value.  The white space after it is consumed; anything else after it is left"

	| r value native |
	depth > 0 ifTrue: [ ^ self materialize: self takeValue ].
	self syncIn.
	native := self useNativeDictionary.
	r := self read: native not.
	(r at: 1) = 2 ifTrue: [ ^ self signalError: (r at: 6) at: index ].
	value := self finish: r.
	native
		ifTrue: [ self class count: #nativeDictionaries ]
		ifFalse: [ value := self materialize: value ].
	self class count: #fastValues.
	index := r at: 3.
	multi := index <= stop.
	self syncOut: index.
	^ value
! !

!SimdJSONReader methodsFor: 'accessing'!
nextAs: schema
	| mapping |
	schema ifNil: [ ^ self next ].
	mapping := self mappingFor: schema.
	depth > 0 ifTrue: [ ^ mapping readFrom: self ].
	^ self topLevel: [ mapping readFrom: self ]
! !

!SimdJSONReader methodsFor: 'accessing'!
nextListAs: schema
	^ self listClass streamContents: [ :stream |
		self parseListDo: [ stream nextPut: (self nextAs: schema) ] ]
! !

!SimdJSONReader methodsFor: 'accessing'!
upToEnd
	"Every value left, NDJSON or JSON Lines, in an Array"

	^ Array streamContents: [ :stream |
		[ self atEnd ] whileFalse: [ stream nextPut: self next ] ]
! !

!SimdJSONReader methodsFor: 'testing'!
atEnd
	depth = 0 ifTrue: [ self syncIn ].
	^ index > stop
! !

!SimdJSONReader methodsFor: 'parsing'!
failIfNotAtEnd
	self atEnd
		ifFalse: [ self error: 'extraneous input detected' ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parseValue
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseValue ] ].
	^ self materialize: self takeValue
! !

!SimdJSONReader methodsFor: 'parsing'!
parseMap
	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseMap ] ].
	node := self takeValue.
	node class == SimdJSONFlatMap ifFalse: [ ^ self error: '{ expected' ].
	^ self materialize: node
! !

!SimdJSONReader methodsFor: 'parsing'!
parseList
	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseList ] ].
	node := self takeValue.
	node class == Array ifFalse: [ ^ self error: '[ expected' ].
	^ self materialize: node
! !

!SimdJSONReader methodsFor: 'parsing'!
parseString
	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseString ] ].
	node := self takeValue.
	node isString ifFalse: [ ^ self error: '" expected' ].
	^ node
! !

!SimdJSONReader methodsFor: 'parsing'!
parseNumber
	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseNumber ] ].
	node := self takeValue.
	node isNumber ifFalse: [ ^ self error: 'Integer digit expected' ].
	^ node
! !

!SimdJSONReader methodsFor: 'parsing'!
parseConstantDo: block
	"Evaluate block with the value when it is true, false or null, else consume nothing"

	depth = 0 ifTrue: [ ^ self topLevel: [ self parseConstantDo: block ] ].
	(hasPending == true and: [ pending isNil or: [ pending == true or: [ pending == false ] ] ])
		ifTrue: [ block value: self takeValue ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parseMapKeysDo: block
	"Evaluate block with each key, the value armed for the reads of the block; a value the
	block does not read is skipped"

	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseMapKeysDo: block ] ].
	node := self takeValue.
	node class == SimdJSONFlatMap ifFalse: [ ^ self error: '{ expected' ].
	1 to: node size by: 2 do: [ :i |
		pending := node at: i + 1.
		hasPending := true.
		block value: (self keyFrom: (node at: i)).
		hasPending := false ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parseMapKeysAndValuesDo: block
	self parseMapKeysDo: [ :key |
		block value: key value: self parseValue ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parseMapDo: block
	"NeoJSON's per entry protocol: block reads the key with parsePropertyName, then the value"

	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseMapDo: block ] ].
	node := self takeValue.
	node class == SimdJSONFlatMap ifFalse: [ ^ self error: '{ expected' ].
	1 to: node size by: 2 do: [ :i |
		pending := (node at: i) -> (node at: i + 1).
		hasPending := #entry.
		block value.
		hasPending := false ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parsePropertyName
	| entry |
	hasPending == #entry ifFalse: [ ^ self error: 'property name expected' ].
	entry := pending.
	pending := entry value.
	hasPending := true.
	^ self keyFrom: entry key
! !

!SimdJSONReader methodsFor: 'parsing'!
parseListDo: block
	"Evaluate block for each element, armed for the reads of the block"

	| node |
	depth = 0 ifTrue: [ ^ self topLevel: [ self parseListDo: block ] ].
	node := self takeValue.
	node class == Array ifFalse: [ ^ self error: '[ expected' ].
	1 to: node size do: [ :i |
		pending := node at: i.
		hasPending := true.
		block value.
		hasPending := false ]
! !

!SimdJSONReader methodsFor: 'parsing'!
parseListElementsDo: block
	self parseListDo: [
		block value: self parseValue ]
! !

!SimdJSONReader methodsFor: 'parsing'!
skipValue
	"Drop the next value without building it (the object mapping, for unknown keys)"

	depth = 0 ifTrue: [ ^ self next ].
	self takeValue
! !

!SimdJSONReader methodsFor: 'private'!
consumeWhitespace
	"NeoJSON's; the plugin consumes the white space around a value itself"
! !

!SimdJSONReader methodsFor: 'private'!
expectChar: character
	"NeoJSON's; simdjson checks the structure of the whole value"
! !

!SimdJSONReader methodsFor: 'private'!
error: text
	SimdJSONParseError new signal: text
! !

!SimdJSONReader methodsFor: 'private'!
classifyStream
	"#string: a ReadStream on a String, read by the plugin in characters; #bytes: on a
	ByteArray, in bytes of UTF-8; anything else is read to its end now, into a ReadStream
	of my own: originalStream keeps the one given, which close closes"

	| contents |
	depth := 0.
	multi := false.
	hasPending := false.
	readStream class == ReadStream
		ifTrue: [
			contents := readStream originalContents.
			contents isString ifTrue: [ ^ self useSource: contents kind: #string ].
			contents class == ByteArray ifTrue: [ ^ self useSource: contents kind: #bytes ] ].
	contents := (readStream respondsTo: #upToEnd)
		ifTrue: [ readStream upToEnd ]
		ifFalse: [
			String streamContents: [ :stream |
				[ readStream atEnd ] whileFalse: [ stream nextPut: readStream next ] ] ].
	(contents isString or: [ contents class == ByteArray ])
		ifFalse: [ contents := String withAll: contents ].
	readStream := ReadStream on: contents.
	self useSource: contents kind: (contents isString ifTrue: [ #string ] ifFalse: [ #bytes ])
! !

!SimdJSONReader methodsFor: 'private'!
useSource: aStringOrByteArray kind: aSymbol
	source := aStringOrByteArray.
	streamKind := aSymbol.
	stop := readStream readLimit
! !

!SimdJSONReader methodsFor: 'private'!
syncIn
	"The stream holds the position: someone else may have read it since"

	index := readStream position + 1
! !

!SimdJSONReader methodsFor: 'private'!
syncOut: nextIndex
	readStream position: nextIndex - 1
! !

!SimdJSONReader methodsFor: 'private'!
tooLarge: units
	"The plugin refused a value of units over the cap (maxInputUnits)"

	self class count: #tooLarge.
	^ SimdJSONResourceError new
		code: #tooLarge;
		signal: 'a value of ', units printString, ' units exceeds SimdJSONReader maxInputUnits; raise it', (self class platform = 'emscripten'
			ifTrue: [ ', and start the VM with PHARO_WASM_OLD_SPACE_BASE=0x40000000 for large documents' ]
			ifFalse: [ '' ])
! !

!SimdJSONReader methodsFor: 'private'!
useNativeDictionary
	^ nativeDictionaries
		and: [ self mapClass == Dictionary
		and: [ self listClass == Array
		and: [ propertyNamesAsSymbols ~~ true
		and: [ self class nativeDictionariesWork ] ] ] ]
! !

!SimdJSONReader methodsFor: 'private'!
flagsTree: wantTree
	"The flags of primRead:: NEXT after a first value (the end of the next one is scanned
	for), EXACT_NUMBERS, and DICT unless wantTree (the caller checked useNativeDictionary),
	else SYMBOL_KEYS with propertyNamesAsSymbols"

	| class flags |
	class := self class.
	flags := multi ifTrue: [ class flagNext ] ifFalse: [ 0 ].
	exactNumbers ifTrue: [ flags := flags + class flagExact ].
	wantTree
		ifFalse: [ flags := flags + class flagDict ]
		ifTrue: [ propertyNamesAsSymbols == true ifTrue: [ flags := flags + class flagSymbolKeys ] ].
	^ flags
! !

!SimdJSONReader methodsFor: 'private'!
prim: flags budget: budget
	| dict |
	(self class testHook: #pluginMissing) == true ifTrue: [ ^ SimdJSONFailure on: nil ].
	dict := flags anyMask: self class flagDict.
	self class count: #reads.
	^ self class
		primRead: source from: index to: stop flags: flags budget: budget
		maxUnits: (self class maxInputUnits ifNil: [ -1 ])
		objectClass: (dict ifTrue: [ Dictionary ] ifFalse: [ SimdJSONFlatMap ])
		associationClass: (dict ifTrue: [ Association ] ifFalse: [ nil ])
		wideStringClass: WideString
		hashSeed: ByteString hash
		dictLayout: (dict ifTrue: [ self class dictLayout ] ifFalse: [ 0 ])
! !

!SimdJSONReader methodsFor: 'private'!
read: wantTree
	"One call of the primitive at index, as a tree or built in C (wantTree false: the caller
	checked useNativeDictionary), with status OK or ERROR.  Where budgetChecked, the budget is checked before anything is built: a refusal
	says how many bytes the value needs, and the memory is grown by that first"

	| flags budget r |
	flags := self flagsTree: wantTree.
	budget := (self class testHook: #budget) ifNil: [
		(stop - index + 1 > 65536 and: [ self class budgetChecked ])
			ifTrue: [ Smalltalk vm parameterAt: 54 ]
			ifFalse: [ -1 ] ].
	r := self prim: flags budget: budget.
	r class == SimdJSONFailure ifTrue: [ r := self recover: r flags: flags ].
	(r at: 1) = 3 ifTrue: [ ^ self tooLarge: (r at: 6) ].
	(r at: 1) = 1 ifTrue: [
		self class count: #overBudget.
		self grow: ((r at: 6) - (Smalltalk vm parameterAt: 54) + 1048576 max: 1048576).
		r := self prim: flags budget: (Smalltalk vm parameterAt: 54).
		r class == SimdJSONFailure ifTrue: [ r := self recover: r flags: flags ].
		(r at: 1) = 1 ifTrue: [ ^ OutOfMemory signal ] ].
	^ r
! !

!SimdJSONReader methodsFor: 'private'!
recover: failure flags: flags
	"The VM already retried 5 times, on the parse the plugin keeps.  A no-memory failure
	here means fragmentation: ask the exact need (budget 0, the same parse), grow by all
	of it, try once more"

	| measure r |
	failure isUnavailable ifTrue: [ ^ self pluginMissing ].
	failure isNoCMemory ifTrue: [ ^ self cMemoryFailure ].
	failure isNoMemory ifFalse: [ ^ self primitiveFailed ].
	measure := self prim: flags budget: 0.
	measure class == SimdJSONFailure ifTrue: [ ^ OutOfMemory signal ].
	(measure at: 1) = 1 ifFalse: [ ^ measure ].
	self grow: (measure at: 6) + 1048576.
	r := self prim: flags budget: -1.
	r class == SimdJSONFailure ifTrue: [ ^ OutOfMemory signal ].
	^ r
! !

!SimdJSONReader methodsFor: 'private'!
pluginMissing
	^ SimdJSONResourceError new
		code: #'not found';
		signal: 'SimdJSONPlugin not available'
! !

!SimdJSONReader methodsFor: 'private'!
cMemoryFailure
	^ SimdJSONResourceError new
		code: #'insufficient C memory';
		signal: 'SimdJSONPlugin: out of C memory'
! !

!SimdJSONReader methodsFor: 'private'!
grow: byteCount
	self class count: #grows.
	Smalltalk growMemoryByAtLeast: byteCount
! !

!SimdJSONReader methodsFor: 'private'!
finish: r
	"Intern the keys, turn the number texts into numbers, before anything else reads the
	value; answer the value"

	keyTable := (r at: 5) ifNotNil: [ :keys | keys collect: [ :each | each asSymbol ] ].
	(r at: 4) ifNotNil: [ :fixups |
		fixups pairsDo: [ :object :slot |
			object instVarAt: slot put: (self numberFromText: (object instVarAt: slot)) ].
		self class count: #numberTexts by: fixups size // 2 ].
	^ r at: 2
! !

!SimdJSONReader methodsFor: 'private'!
numberFromText: aString
	"With NeoJSON's rules, and its errors as SimdJSONParseErrors"

	^ SimdJSONNumberParser parse: aString
! !

!SimdJSONReader methodsFor: 'private'!
keyFrom: aKey
	"A key of the tree: a String, or the index of a Symbol in keyTable"

	^ aKey isInteger
		ifTrue: [ keyTable at: aKey ]
		ifFalse: [ aKey ]
! !

!SimdJSONReader methodsFor: 'private'!
materialize: node
	"A node of the tree as NeoJSON would have read it, with mapClass and listClass"

	^ self materialize: node map: self mapClass list: self listClass
! !

!SimdJSONReader methodsFor: 'private'!
materialize: node map: aMapClass list: aListClass
	"The children of an Array are materialized in place, then copied once into aListClass"

	| map |
	node class == SimdJSONFlatMap ifTrue: [
		map := aMapClass new.
		1 to: node size by: 2 do: [ :i |
			map
				at: (self keyFrom: (node at: i))
				put: (self materialize: (node at: i + 1) map: aMapClass list: aListClass) ].
		^ map ].
	node class == Array ifTrue: [
		1 to: node size do: [ :i |
			node at: i put: (self materialize: (node at: i) map: aMapClass list: aListClass) ].
		^ aListClass == Array
			ifTrue: [ node ]
			ifFalse: [ aListClass withAll: node ] ].
	^ node
! !

!SimdJSONReader methodsFor: 'private'!
takeValue
	hasPending == true ifFalse: [ ^ self error: 'value expected' ].
	hasPending := false.
	^ pending
! !

!SimdJSONReader methodsFor: 'private'!
topLevel: cursorBlock
	"Read the value at the position as a tree, then evaluate cursorBlock with it pending.
	When nothing took it, the stream goes back before it, as NeoJSON reads nothing.  An
	error of cursorBlock puts the reader and the stream back before the value before any
	handler runs, so that a handler can read it again (try nextAs:, else next); a handler
	that resumes the error gets the reader back where cursorBlock was"

	| r root restored |
	self syncIn.
	topStart := index.
	r := self read: true.
	(r at: 1) = 2 ifTrue: [ ^ self signalError: (r at: 6) at: index ].
	root := pending := self finish: r.
	hasPending := true.
	self class count: #fastValues.
	index := r at: 3.
	multi := index <= stop.
	depth := depth + 1.
	restored := false.
	^ [ [ cursorBlock value ]
			on: Error
			do: [ :error | | saved resumption |
				restored ifTrue: [ error pass ].
				saved := { pending. hasPending. index }.
				restored := true.
				depth := depth - 1.
				hasPending := false.
				pending := nil.
				index := topStart.
				self syncOut: index.
				resumption := error outer.
				restored := false.
				depth := depth + 1.
				pending := saved first.
				hasPending := saved second.
				index := saved third.
				error resume: resumption ] ]
		ensure: [
			restored ifFalse: [
				depth := depth - 1.
				(hasPending == true and: [ pending == root ]) ifTrue: [ index := topStart ].
				hasPending := false.
				pending := nil.
				self syncOut: index ] ]
! !

!SimdJSONReader methodsFor: 'private'!
signalError: code at: unitIndex
	^ SimdJSONParseError new
		code: code;
		position: unitIndex;
		signal: 'simdjson: ', (self class errorMessageFor: code)
! !
