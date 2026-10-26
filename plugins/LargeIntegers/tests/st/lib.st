"LargeIntegers image tests: the shared library.
 run-image-tests.sh and bench.sh put it in front of every driver (chunk format: a bang
 inside a string or comment must be doubled).
 Every primitive is called DIRECTLY through an Object>>li... method whose only
 fallback is answering an LIFail with the primitive error code, so image fallback
 code cannot mask a failure.  Operands are built byte-by-byte with basicAt:put:
 and a pure-SmallInteger xorshift32 PRNG, so building them never calls the plugin."

Object subclass: #LIFail
	instanceVariableNames: 'code'
	classVariableNames: ''
	package: 'LIHarness'!

!LIFail methodsFor: 'li'!
code
	^code
!
code: c
	code := c
! !

Object subclass: #LIH
	instanceVariableNames: 'seed out count runUB'
	classVariableNames: ''
	package: 'LIHarness'!

!Object methodsFor: 'li-harness'!
liAnyBitFrom: a to: b
	<primitive: 'primAnyBitFromTo' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liAdd: x
	<primitive: 'primDigitAdd' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liAnd: x
	<primitive: 'primDigitBitAnd' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liOr: x
	<primitive: 'primDigitBitOr' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liXor: x
	<primitive: 'primDigitBitXor' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liShift: n
	<primitive: 'primDigitBitShiftMagnitude' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liCompare: x
	<primitive: 'primDigitCompare' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liDiv: x neg: b
	<primitive: 'primDigitDivNegative' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liMul: x neg: b
	<primitive: 'primDigitMultiplyNegative' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liSub: x
	<primitive: 'primDigitSubtract' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liModuleName
	<primitive: 'primGetModuleName' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liMontLen
	<primitive: 'primMontgomeryDigitLength' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liMont: b mod: m inv: i
	<primitive: 'primMontgomeryTimesModulo' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liNormNeg
	<primitive: 'primNormalizeNegative' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liNormPos
	<primitive: 'primNormalizePositive' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
! !

!LIH methodsFor: 'prng'!
seed: s
	seed := s
!
next32
	"xorshift32, SmallInteger arithmetic only"
	seed := seed bitXor: ((seed bitShift: 13) bitAnd: 16rFFFFFFFF).
	seed := seed bitXor: (seed bitShift: -17).
	seed := seed bitXor: ((seed bitShift: 5) bitAnd: 16rFFFFFFFF).
	^seed
!
below: n
	^self next32 \\ n
!
pick: aCollection
	^aCollection at: (self below: aCollection size) + 1
!
patternDigit
	"adversarial 32-bit digit patterns, or a random digit"
	| k |
	k := self below: 10.
	k < 6 ifTrue: [^#(0 1 16rFFFFFFFF 16rFFFFFFFE 16r80000000 16r7FFFFFFF) at: k + 1].
	^self next32
! !

!LIH methodsFor: 'operands'!
large: cls bytes: n mode: mode
	"mode: #rnd random digits, #pat adversarial digit patterns, #ff all ones,
	 #zero all zero bytes.  The top byte is forced non zero for #rnd/#pat/#ff
	 (so the object is byte-normalized) when n > 0."
	| x d |
	x := cls new: n.
	1 to: n by: 4 do: [:i |
		d := mode == #rnd
			ifTrue: [self next32]
			ifFalse: [mode == #pat
				ifTrue: [self patternDigit]
				ifFalse: [mode == #ff ifTrue: [16rFFFFFFFF] ifFalse: [0]]].
		0 to: 3 do: [:k |
			i + k <= n ifTrue: [x basicAt: i + k put: ((d bitShift: k * -8) bitAnd: 255)]]].
	(mode ~~ #zero and: [n > 0 and: [(x basicAt: n) = 0]]) ifTrue:
		[x basicAt: n put: 1 + (self below: 255)].
	^x
!
unnormalized: cls value: v lead: z
	"a LargeInteger of class cls holding |v| (a SmallInteger) followed by z zero bytes"
	| m n x |
	m := v abs.
	n := 0.
	[m > 0] whileTrue: [n := n + 1. m := m bitShift: -8].
	x := cls new: n + z.
	m := v abs.
	1 to: n do: [:i | x basicAt: i put: (m bitAnd: 255). m := m bitShift: -8].
	^x
!
fromSmall: v cls: cls
	"byte-exact LargeInteger of class cls holding |v| (no leading zero bytes)"
	^self unnormalized: cls value: v lead: 0
!
highBitOfBytes: x
	| n |
	n := x basicSize.
	[n > 0 and: [(x basicAt: n) = 0]] whileTrue: [n := n - 1].
	n = 0 ifTrue: [^0].
	^n - 1 * 8 + (x basicAt: n) highBit
!
lshiftOverflows: x by: s
	"LI-01: true when the baseline digit:Lshift: writes past the 8-byte-rounded
	 body of the new object (receiver with leading zero digits, unaligned shift)"
	| h newByteLen cap |
	x class == SmallInteger ifTrue: [^false].
	s class == SmallInteger ifFalse: [^false].
	(s < 0 or: [s \\ 32 = 0]) ifTrue: [^false].
	h := self highBitOfBytes: x.
	h = 0 ifTrue: [^false].
	newByteLen := h + s + 7 // 8.
	cap := newByteLen + 7 // 8 * 2.
	^(x basicSize + 3 // 4) + (s // 32) > cap
!
montInv: m0
	"-(m0^-1) mod 2^32 for odd m0 < 2^32, SmallInteger arithmetic only"
	| x |
	x := 1.
	5 timesRepeat: [x := self mul32: x by: (2 - (self mul32: m0 by: x)) \\ 16r100000000].
	^(16r100000000 - x) \\ 16r100000000
!
mul32: a by: b
	"(a * b) mod 2^32 without leaving SmallInteger range"
	| alo ahi blo bhi |
	alo := a bitAnd: 16rFFFF. ahi := a bitShift: -16.
	blo := b bitAnd: 16rFFFF. bhi := b bitShift: -16.
	^((((ahi * blo) + (alo * bhi)) bitAnd: 16rFFFF) bitShift: 16) + (alo * blo) \\ 16r100000000
!
lowDigit: x
	| d |
	x class == SmallInteger ifTrue: [^x bitAnd: 16rFFFFFFFF]. "positive SmallIntegers only"
	d := 0.
	(x basicSize min: 4) to: 1 by: -1 do: [:i | d := (d bitShift: 8) + (x basicAt: i)].
	^d
! !

!LIH methodsFor: 'output'!
hex: x from: a to: b
	| s hx |
	hx := '0123456789abcdef'.
	s := WriteStream on: (String new: (b - a + 1) * 2).
	a to: b do: [:i | | v |
		v := x basicAt: i.
		s nextPut: (hx at: (v bitShift: -4) + 1); nextPut: (hx at: (v bitAnd: 15) + 1)].
	^s contents
!
fnv: x seed: h0
	| h |
	h := h0.
	1 to: x basicSize do: [:i | h := ((h bitXor: (x basicAt: i)) * 16777619) bitAnd: 16rFFFFFFFF].
	^h
!
bytes: x
	"exact bytes for small objects, a 64-bit content hash plus both ends for big ones"
	| n |
	n := x basicSize.
	n <= 64 ifTrue: [^self hex: x from: 1 to: n].
	^(self hex: x from: 1 to: 16), '..', (self hex: x from: n - 15 to: n),
		' h', ((self fnv: x seed: 16r811C9DC5) printStringBase: 16),
		'.', ((self fnv: x seed: 16r12345677) printStringBase: 16)
!
opd: x
	"short operand description"
	x class == SmallInteger ifTrue: [^'i', x printString].
	x class == LargePositiveInteger ifTrue: [^'P', x basicSize printString, ':', (self short: x)].
	x class == LargeNegativeInteger ifTrue: [^'N', x basicSize printString, ':', (self short: x)].
	^x class name
!
short: x
	x basicSize <= 16 ifTrue: [^self hex: x from: 1 to: x basicSize].
	^(self fnv: x seed: 16r811C9DC5) printStringBase: 16
!
val: r rcv: rcv
	| id |
	id := (r == rcv and: [r class ~~ SmallInteger]) ifTrue: ['ID '] ifFalse: [''].
	r class == SmallInteger ifTrue: [^'SI ', r printString].
	r class == LargePositiveInteger ifTrue: [^id, 'LPI ', r basicSize printString, ' ', (self bytes: r)].
	r class == LargeNegativeInteger ifTrue: [^id, 'LNI ', r basicSize printString, ' ', (self bytes: r)].
	r class == Array ifTrue: [^'Array', r size printString, '{',
		(r inject: '' into: [:acc :e | acc, (self val: e rcv: rcv), ' ; ']), '}'].
	(r == true or: [r == false]) ifTrue: [^r printString].
	r isString ifTrue: [^'S ', r printString].
	^id, r class name
!
res: r rcv: rcv
	(r isKindOf: LIFail) ifTrue: [^'FAIL ', r code printString].
	^self val: r rcv: rcv
!
emit: label args: anArray result: r rcv: rcv
	| s |
	count := count + 1.
	s := WriteStream on: (String new: 200).
	s print: count; space; nextPutAll: label.
	anArray do: [:a | s space; nextPutAll: (self opd: a)].
	s nextPutAll: ' => '; nextPutAll: (self res: r rcv: rcv).
	Stdio stdout nextPutAll: s contents; lf.
	count \\ 500 = 0 ifTrue: [Stdio stdout flush]
!
lbl: x
	x class == SmallInteger ifTrue: [^x printString].
	^self opd: x
!
note: aString
	Stdio stdout nextPutAll: '# ', aString; lf; flush
!
initialize
	count := 0.
	seed := 16r2545F491
! !

!LIH methodsFor: 'calls'!
call: label rcv: r args: args do: aBlock
	"aBlock answers the primitive result"
	| res |
	res := aBlock value.
	self emit: label args: {r}, args result: res rcv: r.
	^res
!
add: a to: b
	^self call: 'add' rcv: a args: {b} do: [a liAdd: b]
!
sub: a with: b
	^self call: 'sub' rcv: a args: {b} do: [a liSub: b]
!
and: a with: b
	^self call: 'and' rcv: a args: {b} do: [a liAnd: b]
!
or: a with: b
	^self call: 'or' rcv: a args: {b} do: [a liOr: b]
!
xor: a with: b
	^self call: 'xor' rcv: a args: {b} do: [a liXor: b]
!
cmp: a with: b
	^self call: 'cmp' rcv: a args: {b} do: [a liCompare: b]
!
mul: a by: b neg: f
	^self call: 'mul', (f == true ifTrue: ['-'] ifFalse: ['+']) rcv: a args: {b} do: [a liMul: b neg: f]
!
div: a by: b neg: f
	^self call: 'div', (f == true ifTrue: ['-'] ifFalse: ['+']) rcv: a args: {b} do: [a liDiv: b neg: f]
!
shift: a by: n
	(self lshiftOverflows: a by: n) ifTrue:
		[runUB == true ifTrue: [^self ubShift: a by: n].
		 count := count + 1.
		 ^Stdio stdout nextPutAll: count printString, ' shift ', (self lbl: n), ' ', (self opd: a), ' => SKIP-LI01'; lf].
	^self call: 'shift ', (self lbl: n) rcv: a args: #() do: [a liShift: n]
!
anyBit: a from: f to: t
	^self call: 'anyBit ', (self lbl: f), ',', (self lbl: t) rcv: a args: #() do: [a liAnyBitFrom: f to: t]
!
normPos: a
	^self call: 'normPos' rcv: a args: #() do: [a liNormPos]
!
normNeg: a
	^self call: 'normNeg' rcv: a args: #() do: [a liNormNeg]
!
mont: a times: b mod: m inv: i
	(((m isKindOf: Integer) and: [(a isKindOf: Integer) and: [b isKindOf: Integer]]) and:
		[(self digitLen: a) > 0 and: [(self digitLen: b) = 0 and: [(self digitLen: a) <= (self digitLen: m)]]])
		ifTrue:
			[runUB == true ifTrue: [^self ubMont: a times: b mod: m inv: i].
			 count := count + 1.
			 ^Stdio stdout nextPutAll: count printString, ' mont ', (self opd: a), ' ', (self opd: b), ' => SKIP-LI06'; lf].
	^self call: 'mont' rcv: a args: {b. m. i} do: [a liMont: b mod: m inv: i]
!
runUB: aBoolean
	runUB := aBoolean
!
normalizedCopy: x
	| n y |
	n := x basicSize.
	[n > 0 and: [(x basicAt: n) = 0]] whileTrue: [n := n - 1].
	y := x class new: n.
	1 to: n do: [:i | y basicAt: i put: (x basicAt: i)].
	^y
!
ubShift: a by: n
	"LI-01 shape on the new VM: must answer exactly what the normalized copy answers"
	| r r2 ok |
	r := a liShift: n.
	r2 := (self normalizedCopy: a) liShift: n.
	ok := (self res: r rcv: a) = (self res: r2 rcv: nil).
	count := count + 1.
	Stdio stdout nextPutAll: count printString, ' shift ', (self lbl: n), ' ', (self opd: a), ' => UB-LI01 ',
		(ok ifTrue: ['OK '] ifFalse: ['MISMATCH ']), (self res: r rcv: a); lf
!
ubMont: a times: b mod: m inv: i
	"LI-06 shape on the new VM: must fail with the generic code"
	| r |
	r := a liMont: b mod: m inv: i.
	count := count + 1.
	Stdio stdout nextPutAll: count printString, ' mont ', (self opd: a), ' ', (self opd: b), ' => UB-LI06 ',
		(((r isKindOf: LIFail) and: [r code isNil]) ifTrue: ['OK '] ifFalse: ['MISMATCH ']), (self res: r rcv: a); lf
!
digitLen: x
	x class == SmallInteger ifTrue: [^(x between: -16rFFFFFFFF and: 16rFFFFFFFF) ifTrue: [1] ifFalse: [2]].
	^x basicSize + 3 // 4
! !
