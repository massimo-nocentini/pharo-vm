"Load the CMarkPlugin of the VM, as the CI jobs do with the VMMaker image, which has no CMark
package: this file compiles its own primitive methods (docs/CMark.md, Primitive contract).  Asks
primitiveInfo (protocol 1, cmark-gfm 0.29.0.gfm.14, the five extensions), and checks known answers:
- primitiveParse of '# Hi' (options 0, extensions 31, maxDepth 0, budget -1, cBudget -1): status 0,
  an Array of 9 whose slot 9 is nil, a Bitmap tape of 3 records of 16 words: the document
  (16r8001), the heading (16r8009, level 1, at 1:1-1:4, characters 1-4) and the text (16rC001,
  its str1 'Hi' flagged ASCII, at 1:3-1:4, characters 3-4);
- primitiveParse of a table: a record 16r8100;
- primitiveRender of '# Hi' as HTML: '<h1>Hi</h1>' and a LF, flagged ASCII;
- primitiveParse of the ByteArray #[35 32 72 255]: slot 9 holds the sanitised bytes
  #[35 32 72 16rEF 16rBF 16rBD] (the invalid byte became U+FFFD).
Then the limits, so that every native CI job runs the setjmp/longjmp landing of the engine:
- '# Hi' with a C budget of 512 bytes: status 4 (C_LIMIT), code 1 (the budget);
- 1200 '>' and ' x' with maxDepth 100: status 3 (TOO_DEEP), code 101;
- '# Hi' again: the same tape, word for word, as the first parse;
- a paragraph of inlines with every extension, at C budgets from 512 bytes up by 16 until it
  parses: each call answers status 0 or 4 (code 1), and after each status 4, 'www:a~b~*c*' with no
  extension has the tape it had before (the landing restored cmark-gfm's special characters of
  the inline phase, which the strikethrough and autolink extensions set).
Prints

	CMarkPlugin <platform> 0.29.0.gfm.14 <protocol> ok
	CMarkPlugin limits ok

and exits 0; else prints 'CMarkPlugin FAILED ...' or 'CMarkPlugin did not load' (which the
jobs' grep of 'CMarkPlugin native' does not match) and exits 1."

| lineEnd lf fail info parse render word words answer tape strings first offset length table html bytes plain special budget limited reached |
lineEnd := OSPlatform current lineEnding.
lf := String with: Character lf.
Object compile: 'cmarkCiInfo <primitive: ''primitiveInfo'' module: ''CMarkPlugin''> ^ nil' classified: 'probe'.
Object compile: 'cmarkCiParse: s options: o extensions: e maxDepth: d budget: b cBudget: c <primitive: ''primitiveParse'' module: ''CMarkPlugin'' error: ec> ^ ec' classified: 'probe'.
Object compile: 'cmarkCiRender: s format: f options: o extensions: e width: w budget: b cBudget: c <primitive: ''primitiveRender'' module: ''CMarkPlugin'' error: ec> ^ ec' classified: 'probe'.

fail := [ :message |
	Stdio stdout nextPutAll: 'CMarkPlugin FAILED '; nextPutAll: message; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
"an answer of primitiveParse, or a failure"
parse := [ :source :extensions :maxDepth :cBudget |
	| result |
	result := nil cmarkCiParse: source options: 0 extensions: extensions maxDepth: maxDepth budget: -1 cBudget: cBudget.
	(result isArray and: [ result size = 9 ]) ifFalse: [
		fail value: 'primitiveParse of ' , source printString , ' answers ' , result printString ].
	result ].
render := [ :source :format |
	| result |
	result := nil cmarkCiRender: source format: format options: 0 extensions: 31 width: 0 budget: -1 cBudget: -1.
	(result isArray and: [ result size = 5 ]) ifFalse: [
		fail value: 'primitiveRender of ' , source printString , ' answers ' , result printString ].
	result ].
"word k of record i of a tape"
word := [ :aTape :i :k | aTape at: i - 1 * 16 + k ].
"words 1 to 12 of record i: code parent flags startLine startColumn endLine endColumn
startIndex endIndex int1 int2 int3"
words := [ :aTape :i | (1 to: 12) collect: [ :k | word value: aTape value: i value: k ] ].

info := nil cmarkCiInfo.
info ifNil: [
	Stdio stdout nextPutAll: 'CMarkPlugin did not load'; nextPutAll: lineEnd; flush.
	Smalltalk exitFailure ].
(info isArray and: [ info size = 7 and: [ info first = 1 and: [ info second = 'CMarkPlugin 1.0'
	and: [ info third = '0.29.0.gfm.14' and: [ (#('native' 'emscripten') includes: info fourth)
	and: [ info fifth asArray = #('table' 'strikethrough' 'autolink' 'tagfilter' 'tasklist') ] ] ] ] ] ])
		ifFalse: [ fail value: 'primitiveInfo answers ' , info printString ].

"'# Hi': the document, the heading and its text"
answer := parse value: '# Hi' value: 31 value: 0 value: -1.
((answer first: 2) = #(0 0) and: [ (answer at: 5) = 3 and: [ (answer at: 7) = 16
	and: [ (answer at: 8) = 4 and: [ (answer at: 9) isNil ] ] ] ])
		ifFalse: [ fail value: 'primitiveParse of ''# Hi'' answers ' , answer printString ].
tape := answer at: 3.
strings := answer at: 4.
(tape class == Bitmap and: [ tape size = 48 and: [ strings class == ByteArray ] ])
	ifFalse: [ fail value: 'primitiveParse of ''# Hi'' answers a tape ' , tape printString , ' and strings ' , strings printString ].
"flags: HAS_POS (4) and nothing else of bits 2-9; STR1_ASCII (1) for the text (an empty or
absent string may have its ASCII bit or not)"
#( #(1 'document' #(16r8001 0 nil 1 1 1 4 1 4 0 0 0))
   #(2 'heading' #(16r8009 1 nil 1 1 1 4 1 4 1 0 0))
   #(3 'text' #(16rC001 2 nil 1 3 1 4 3 4 0 0 0)) ) do: [ :expected |
	| record |
	record := words value: tape value: expected first.
	(((1 to: 12) allSatisfy: [ :k | k = 3 or: [ (record at: k) = ((expected at: 3) at: k) ] ])
		and: [ ((record at: 3) bitAnd: 16r3FC) = 4 ])
			ifFalse: [ fail value: 'the ' , expected second , ' record is ' , record printString ] ].
((word value: tape value: 3 value: 3) bitAnd: 1) = 1
	ifFalse: [ fail value: 'the text of the heading is not flagged ASCII' ].
offset := word value: tape value: 3 value: 13.
length := word value: tape value: 3 value: 14.
(length = 2 and: [ offset + length <= strings size
	and: [ (strings copyFrom: offset + 1 to: offset + length) = 'Hi' asByteArray ] ])
		ifFalse: [ fail value: 'the text of the heading is at ' , offset printString , ' length ' , length printString , ' in ' , strings printString ].
first := tape asArray.

"a table"
table := parse value: '| a |' , lf , '|---|' , lf , '| b |' value: 31 value: 0 value: -1.
((table at: 1) = 0 and: [ ((1 to: (table at: 5)) collect: [ :i | word value: (table at: 3) value: i value: 1 ]) includes: 16r8100 ])
	ifFalse: [ fail value: 'primitiveParse of a table answers ' , table printString ].

"rendered as HTML"
html := render value: '# Hi' value: 1.
bytes := ('<h1>Hi</h1>' , lf) asByteArray.
(html first = 0 and: [ (html at: 2) = bytes size and: [ (html at: 3) = bytes
	and: [ (html at: 4) == true and: [ (html at: 5) = bytes size ] ] ] ])
		ifFalse: [ fail value: 'primitiveRender of ''# Hi'' answers ' , html printString ].

"invalid UTF-8: T is in slot 9"
answer := parse value: #[35 32 72 255] value: 31 value: 0 value: -1.
(answer first = 0 and: [ (answer at: 8) = 4 and: [ (answer at: 9) = #[35 32 72 16rEF 16rBF 16rBD] ] ])
	ifFalse: [ fail value: 'primitiveParse of #[35 32 72 255] answers ' , answer printString ].

Stdio stdout
	nextPutAll: 'CMarkPlugin '; nextPutAll: info fourth;
	nextPutAll: ' '; nextPutAll: info third;
	nextPutAll: ' '; nextPutAll: info first printString;
	nextPutAll: ' ok'; nextPutAll: lineEnd; flush.

"The limits.  The tape of the special characters first, before any status 4"
special := ((parse value: 'www:a~b~*c*' value: 0 value: 0 value: -1) at: 3) asArray.
answer := parse value: '# Hi' value: 31 value: 0 value: 512.
(answer first: 2) = #(4 1)
	ifFalse: [ fail value: 'primitiveParse of ''# Hi'' with a C budget of 512 answers ' , answer printString ].
answer := parse value: (String new: 1200 withAll: $>) , ' x' value: 31 value: 100 value: -1.
(answer first: 2) = #(3 101)
	ifFalse: [ fail value: 'primitiveParse of 1200 quotes with maxDepth 100 answers ' , (answer first: 2) printString ].
answer := parse value: '# Hi' value: 31 value: 0 value: -1.
(answer first = 0 and: [ (answer at: 3) asArray = first ])
	ifFalse: [ fail value: 'primitiveParse of ''# Hi'' after the limits answers ' , answer printString ].
plain := 'www:a~b~*c* and ~~d~~ www.e.f http://g.h <x@y.z> ~e~'.
budget := 512.
limited := 0.
reached := false.
[ reached or: [ budget > 65536 ] ] whileFalse: [
	answer := parse value: plain value: 31 value: 0 value: budget.
	answer first = 0
		ifTrue: [ reached := true ]
		ifFalse: [
			(answer first: 2) = #(4 1) ifFalse: [
				fail value: 'primitiveParse with a C budget of ' , budget printString , ' answers ' , (answer first: 2) printString ].
			limited := limited + 1.
			((parse value: 'www:a~b~*c*' value: 0 value: 0 value: -1) at: 3) asArray = special ifFalse: [
				fail value: 'after a status 4 at a C budget of ' , budget printString , ', www:a~b~*c* has another tape' ].
			budget := budget + 16 ] ].
(reached and: [ limited > 0 ])
	ifFalse: [ fail value: 'the C budget sweep: ' , limited printString , ' status 4, parsed ' , reached printString ].

Stdio stdout nextPutAll: 'CMarkPlugin limits ok'; nextPutAll: lineEnd; flush.
Smalltalk snapshot: false andQuit: true.
