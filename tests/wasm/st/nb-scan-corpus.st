"nb-scan-corpus.st - the tokens that Pharo's scanner finds in the methods of the image,
the reference of the Smalltalk scanner of the notebook (packaging/emscripten/web/
notebook-st.js, checked by tests/wasm/notebook-lib.test.mjs)

	pharo --headless Pharo.image --no-default-preferences st --no-source --quit nb-scan-corpus.st

Writes one line per method, a JSON array

	[name, source, tokens]

where name is the print string of the method ('Object>>#printOn:') and source its
source, and tokens are [kind, start, stop, value], start and stop being the 1-based
positions in the source of the first and the last character of the token (code
points, not UTF-16 units):

	comment     a comment (no value), as the scanner reports the comments before a
	            token, and those before the end
	string, symbol, char, literal
	            a literal (#true, #false and #nil are literals); a symbol is #foo,
	            #at:put:, #+ or #'a b', but also a bare word of a literal array
	number      a number literal, with its sign when the scanner reads one
	litarray    the #( or #[ of a literal or a byte array
	special     ( ) [ ] { } . ; ^ | and the like, the value being the character
	keyword, ident, binary, assign
	            the others; and any other token by the name of its class without
	            its prefix

The scanner is RBScanner in Pharo 12 and OCScanner in Pharo 15.  The methods are those
of every class and trait of the image, both sides, in the order of their print strings,
of which NB_SCAN_METHODS are written, evenly spread: 5000 when the environment variable
is unset, and all of them when it is 0.  A method whose source the
scanner cannot read without an error, or with an error token, is left out.

The lines go to the file NB_SCAN_OUT names, else to stdout, where only the lines
that begin with [ are the corpus (the image may write notices of its own there).  A
last line on stderr says how many methods were written, of how many, and how long it
took."

| environment max outName scannerClass kindOf methods count written skipped time write |
environment := Smalltalk globals at: #OSEnvironment.
max := ((environment current at: 'NB_SCAN_METHODS' ifAbsent: [ '5000' ]) ifEmpty: [ '5000' ]) asNumber.
outName := environment current at: 'NB_SCAN_OUT' ifAbsent: [ '' ].
scannerClass := Smalltalk globals at: #OCScanner ifAbsent: [ Smalltalk globals at: #RBScanner ].
kindOf := [ :token | | name value |
	name := token class name allButFirst: 2.
	name = 'LiteralToken'
		ifTrue: [
			value := token value.
			value isSymbol
				ifTrue: [ 'symbol' ]
				ifFalse: [
					value isString
						ifTrue: [ 'string' ]
						ifFalse: [ value isCharacter ifTrue: [ 'char' ] ifFalse: [ 'literal' ] ] ] ]
		ifFalse: [
			(Dictionary newFrom: {
				'NumberLiteralToken' -> 'number'.
				'LiteralArrayToken' -> 'litarray'.
				'SpecialCharacterToken' -> 'special'.
				'KeywordToken' -> 'keyword'.
				'IdentifierToken' -> 'ident'.
				'BinarySelectorToken' -> 'binary'.
				'AssignmentToken' -> 'assign' }) at: name ifAbsent: [ name ] ] ].
time := Time millisecondsToRun: [
	methods := OrderedCollection new.
	Smalltalk globals allClassesAndTraits do: [ :each |
		{ each. each classSide } do: [ :behavior | methods addAll: behavior methods ] ].
	methods := (methods collect: [ :each | each printString -> each ]) asArray
		sort: [ :a :b | a key <= b key ].
	count := max = 0 ifTrue: [ methods size ] ifFalse: [ max min: methods size ].
	written := skipped := 0.
	write := [ :out |
		0 to: count - 1 do: [ :k | | method source tokens line |
			method := methods at: k * methods size // count + 1.
			line := [
				source := method value sourceCode.
				tokens := OrderedCollection new.
				(scannerClass on: source readStream) in: [ :scanner | | token |
					[	token := scanner next.
						token comments ifNotNil: [ :comments |
							comments do: [ :comment | tokens add: { 'comment'. comment first. comment last } ] ].
						token isEOF ] whileFalse: [
							(token class name endsWith: 'ErrorToken') ifTrue: [ Error signal: 'an error token' ].
							tokens add: {
								kindOf value: token.
								token start.
								token stop.
								(token class name endsWith: 'SpecialCharacterToken')
									ifTrue: [ token value asString ]
									ifFalse: [ '' ] } ] ].
				(STONJSON toString: { method key. source. tokens asArray }) utf8Encoded ]
					on: Error
					do: [ :error | error return: nil ].
			line
				ifNil: [ skipped := skipped + 1 ]
				ifNotNil: [
					out nextPutAll: line; nextPut: 10.
					written := written + 1 ] ] ].
	outName isEmpty
		ifTrue: [ write value: Stdio stdout. Stdio stdout flush ]
		ifFalse: [
			outName asFileReference ensureDelete.
			outName asFileReference binaryWriteStreamDo: [ :out | write value: out ] ] ].
Stdio stderr
	nextPutAll: ('nb-scan-corpus: {1} methods written ({2} left out) of {3}, by {4}, in {5} ms' format: {
		written printString. skipped printString. methods size printString. scannerClass name. time printString })
		asByteArray;
	nextPut: 10;
	flush
