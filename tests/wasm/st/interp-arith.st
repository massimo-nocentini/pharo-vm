"Lane 77-interpreter: the arithmetic and the comparisons of the interpreter.

	pharo --headless Pharo.image --no-default-preferences st --quit interp-arith.st

 Every operator of the special selectors, on every pair of a list of numbers
 (SmallIntegers at the edges of 8, 32, 53 and 61 bits, LargeIntegers, floats
 with and without a SmallFloat, NaN and the infinities, fractions) and other
 objects, as a bytecode whose operands are on the stack or just pushed,
 alone and followed by a jump, is compared with the same message sent by
 perform:, which runs the primitive: a line 'DIFF ...' for each difference.
 Then one line per operator with a checksum of its results, which must be
 the same on every VM.  (The checksum leaves out what a JIT answers
 otherwise: the comparisons of a SmallInteger that no float is equal to
 with a float, made by the JIT in doubles, and the shifts by 61 or more.)"

| out vals number describe holder checksum ops kinds |
out := [ :s | Stdio stdout nextPutAll: s; lf; flush ].
vals := {
	0. 1. -1. 2. -2. 3. 7. -7. 8. 60. 61. -60. -61. -62. 64. 255. 256. 1000. -1000.
	(1 bitShift: 31). (1 bitShift: 31) - 1. (1 bitShift: 31) negated. (1 bitShift: 32). (1 bitShift: 32) + 7. -5000000000.
	(1 bitShift: 52). (1 bitShift: 53) + 1. ((1 bitShift: 53) + 1) negated. (1 bitShift: 59).
	SmallInteger maxVal. SmallInteger maxVal - 1. SmallInteger minVal. SmallInteger minVal + 1.
	SmallInteger maxVal + 1. SmallInteger minVal - 1. (1 bitShift: 70). (1 bitShift: 64) negated.
	0.0. 0.0 negated. 1.0. -1.0. 0.5. 2.5. 1.0e100. 1.0e-100. 1.0e300. 1.0e-308. 5.0e-324.
	Float nan. Float infinity. Float infinity negated. 9007199254740992.0. 1.0e18. 1.0e39. 1.0e-39.
	3.0e-39. 2.350988701644575e-38. 6.8e38. 1/3. 3/2. nil. true. false. $a. $b. #a. Object }.
describe := [ :r |
	r isFloat
		ifTrue: [ r isNaN ifTrue: [ 'nan' ] ifFalse: [ r asIEEE64BitWord printString , r class name ] ]
		ifFalse: [ (r isNumber or: [ r isSymbol or: [ r isCharacter or: [ r isNil or: [ r isKindOf: Boolean ] ] ] ])
			ifTrue: [ r printString , r class name ]
			ifFalse: [ 'a' , r class name ] ] ].
checksum := [ :h :s | s inject: h into: [ :acc :ch | (acc * 33 + ch asInteger) bitAnd: 16rFFFFFFFFF ] ].
"which results every VM must agree on"
number := [ :selector :a :b | | inexact |
	inexact := [ :i :f | (i isKindOf: SmallInteger) and: [ f isFloat and: [ i abs > 9007199254740992 ] ] ].
	((inexact value: a value: b) or: [ (inexact value: b value: a) or: [ selector = #bitShift: and: [ b isInteger and: [ b abs > 60 ] ] ] ]) not ].
(Object << #InterpHolder slots: { #value }; package: 'Interp-Test') install.
holder := (Smalltalk at: #InterpHolder) new.
"the kinds of a method that sends the operator: its source, and what it answers, given the answer r of perform: (nil for r itself)"
kinds := {
	{ '{M} ^ a {S} b'. nil }.
	{ '{M} ^ (#(true) basicAt: 1) ifTrue: [ a {S} (b instVarAt: 1) ]'. nil }.
	{ '{M} | x y | x := a. y := b. ^ x {S} y'. nil }.
	{ '{M} | r | r := a {S} b. ^ r'. nil }.
	{ '{M} ^ [ :x :y | x {S} y ] value: a value: b'. nil }.
	{ '{M} ^ (a {S} b) ifTrue: [ 1 ] ifFalse: [ 2 ]'. [ :r | r ifTrue: [ 1 ] ifFalse: [ 2 ] ] }.
	{ '{M} ^ (a {S} (b instVarAt: 1)) ifFalse: [ 1 ] ifTrue: [ 2 ]'. [ :r | r ifTrue: [ 2 ] ifFalse: [ 1 ] ] }.
	{ '{M} (a {S} b) ifTrue: [ ^ #yes ]. ^ #no'. [ :r | r ifTrue: [ #yes ] ifFalse: [ #no ] ] }.
	{ '{M} | n | n := 0. [ (a {S} b) and: [ n < 2 ] ] whileTrue: [ n := n + 1 ]. ^ n'. [ :r | r ifTrue: [ 2 ] ifFalse: [ 0 ] ] }.
	{ '{M} | n | n := 0. [ (a {S} b) or: [ n = 1 ] ] whileFalse: [ n := n + 1 ]. ^ n'. [ :r | r ifTrue: [ 0 ] ifFalse: [ 1 ] ] } }.
ops := #(+ - * / // \\ < > <= >= = ~= == ~~ bitAnd: bitOr: bitShift:).
ops do: [ :selector | | methods h |
	methods := kinds withIndexCollect: [ :kind :k |
		| source |
		source := kind first copyReplaceAll: '{S}' with: selector.
		source := source copyReplaceAll: '{M}' with: ('k' , k printString , 'A: a b: b').
		holder class compile: source.
		('k' , k printString , 'A:b:') asSymbol ].
	h := 0.
	vals do: [ :a |
		(vals reject: [ :b | selector = #bitShift: and: [ b isNumber and: [ b > 200 ] ] ]) do: [ :b | | expected boxed |
			expected := [ a perform: selector with: b ] on: Error do: [ :e | e class name asSymbol ].
			boxed := Association key: b value: nil.
			methods withIndexDo: [ :m :k | | r convert wanted skip |
				convert := (kinds at: k) second.
				"a jump on what is not a Boolean is a NonBooleanReceiver: not compared"
				skip := convert isNotNil and: [ (expected isKindOf: Boolean) not ].
				skip ifFalse: [
					wanted := convert ifNil: [ expected ] ifNotNil: [ convert value: expected ].
					r := [ holder perform: m with: a with: ((k = 2 or: [ k = 7 ]) ifTrue: [ boxed ] ifFalse: [ b ]) ]
						on: Error do: [ :e | e class name asSymbol ].
					(describe value: r) = (describe value: wanted) ifFalse: [
						out value: ('DIFF {1} {2} {3}: {4}, not {5} (kind {6})' format: {
							(describe value: a). selector. (describe value: b). (describe value: r). (describe value: wanted). k }) ] ] ].
			(number value: selector value: a value: b) ifTrue: [
				h := checksum value: h value: (describe value: expected).
				h := checksum value: h value: (describe value:
					([ holder perform: methods first with: a with: b ] on: Error do: [ :e | e class name asSymbol ])) ] ] ].
	out value: selector , ' ' , h printString ].

"constants next to the operator, and runs of operations"
#( 'a: a ^ a + 1' 'a: a ^ 1 + a' 'a: a ^ a - 1' 'a: a ^ a * 2' 'a: a | x | x := a. x := x + 1. ^ x'
	'a: a ^ a + a + a' 'a: a ^ a * a - a' 'a: a ^ a + 1 + 1 - 1 - 1' 'a: a ^ (a - 1) * (a + 1)' 'a: a ^ a < 0 | (a > 10)'
	'a: a ^ { a + 1. a - 1. a < 1. a = 0 } size' 'a: a | s | s := a. 1 to: 5 do: [ :i | s := s + i ]. ^ s'
	'a: a | s | s := a. 10 to: 1 by: -2 do: [ :i | s := s * i ]. ^ s' 'a: a ^ a + 1 = (1 + a)' 'a: a ^ (a bitAnd: 7) + (a bitOr: 1)'
	'a: a ^ (a bitShift: 1) - (a bitShift: -1)' 'a: a ^ (a // 3) * 3 + (a \\ 3) - a' ) do: [ :source | | h |
	holder class compile: source.
	h := 0.
	vals do: [ :a | (a isNumber and: [ a isFraction not ]) ifTrue: [
		h := checksum value: h value: (describe value: ([ holder perform: #a: with: a ] on: Error do: [ :e | e class name asSymbol ])) ] ].
	out value: source , ' ' , h printString ].

"sums that pass from SmallInteger to LargeInteger and back, and floats that leave the SmallFloats"
out value: ([ | s | s := SmallInteger maxVal - 5. 10 timesRepeat: [ s := s + 1 ]. 10 timesRepeat: [ s := s - 1 ]. s = (SmallInteger maxVal - 5) and: [ s class == SmallInteger ] ] value) printString.
out value: ([ | s | s := SmallInteger minVal + 5. 10 timesRepeat: [ s := s - 1 ]. s = (SmallInteger minVal - 5) and: [ (s class == SmallInteger) not ] ] value) printString.
out value: ([ | f | f := 1.0. 400 timesRepeat: [ f := f * 8.0 ]. f ] value) printString.
out value: ([ | f | f := 1.0. 400 timesRepeat: [ f := f / 8.0 ]. f ] value) printString.
out value: (300 factorial printString size -> (300 factorial \\ 1000007)) printString.
out value: ((1 to: 2000) inject: 0.0 into: [ :a :b | a + (b / 7) - (b * 0.125) ]) printString.
out value: 'end'.
Smalltalk exitSuccess!
