"The kernel differential driver: 6,297 calls of primDigitMultiplyNegative and
 primDigitDivNegative over the sizes where the C kernels change algorithm (one-limb
 streaming, the 32-bit loops for the smallest products and quotients, schoolbook over
 64-bit limbs, Karatsuba from 24 limbs = 192 bytes, the one-limb divisor, quotients of one
 digit, divisors around 24 digits), and big operands up to 40000 bytes.  Same output as
 diff.st.  run-image-tests.sh puts lib.st in front and sends runKernels."!

!LIH methodsFor: 'kernels'!
opnd: bytes
	"a random operand of about this many bytes, of either sign, now and then a SmallInteger"
	| cls |
	(bytes <= 7 and: [(self below: 4) = 0]) ifTrue:
		[^((self next32 bitShift: (self below: 26)) + 1) * ((self below: 2) * 2 - 1)].
	cls := (self below: 2) = 0 ifTrue: [LargePositiveInteger] ifFalse: [LargeNegativeInteger].
	^self large: cls bytes: (bytes max: 1) mode: (self pick: #(#rnd #rnd #pat #pat #ff))
!
topOf: b over: lowBytes
	"a LargePositiveInteger whose top bytes are those of b, above lowBytes random bytes:
	 the top dividend digits equal the divisor's (the r1 = dh shortcut of Knuth D)"
	| x n |
	n := b basicSize.
	x := LargePositiveInteger new: n + lowBytes.
	1 to: lowBytes do: [:i | x basicAt: i put: (self below: 256)].
	1 to: n do: [:i | x basicAt: lowBytes + i put: (b basicAt: i)].
	^x
!
runMulKernels
	| a b |
	self note: 'multiply: random sizes'.
	1 to: 2500 do: [:k |
		| m |
		m := self pick: #(12 24 64 64 200 400 1200).
		a := self opnd: 1 + (self below: m).
		b := (self below: 10) = 0 ifTrue: [a] ifFalse: [self opnd: 1 + (self below: m)].
		self mul: a by: b neg: (self below: 2) = 0].
	self note: 'multiply: Karatsuba threshold straddle (192 bytes = 24 limbs)'.
	#(176 184 188 189 190 191 192 193 194 196 200 208 216) do: [:n |
		#(1 2 3 5 6) do: [:f |
			a := self opnd: n.
			b := self opnd: n * f // 2 + (self below: 4) - 2.
			self mul: a by: b neg: false.
			self mul: b by: a neg: true].
		a := self large: LargePositiveInteger bytes: n mode: #ff.
		self mul: a by: a neg: false].
	self note: 'multiply: big'.
	#(#(1000 1000) #(2047 2049) #(4096 4096) #(4000 12000) #(8 30000) #(100 20000) #(16000 16001) #(40000 40000)) do: [:p |
		a := self opnd: p first.
		b := self opnd: p last.
		self mul: a by: b neg: false.
		self mul: b by: b neg: true]
!
runDivKernels
	| a b q |
	self note: 'divide: random sizes'.
	1 to: 2500 do: [:k |
		| m |
		m := self pick: #(8 16 48 120 400).
		b := self opnd: 1 + (self below: m).
		a := self opnd: b basicSize + (self below: (self pick: #(4 8 64 400))).
		self div: a by: b neg: (self below: 2) = 0].
	self note: 'divide: one quotient digit, divisors around 24 digits'.
	#(4 8 12 40 80 92 93 94 95 96 97 98 100 104 128 400) do: [:n |
		3 timesRepeat:
			[b := self opnd: n.
			 a := self opnd: n.
			 self div: a by: b neg: false.
			 a := self opnd: n + 1 + (self below: 4).
			 self div: a by: b neg: true]].
	self note: 'divide: top dividend digits = top divisor digits'.
	1 to: 400 do: [:k |
		b := self large: LargePositiveInteger bytes: 4 * (1 + (self below: 40)) mode: (self pick: #(#rnd #pat #ff)).
		a := self topOf: b over: 4 * (self below: 30) + (self below: 4).
		self div: a by: b neg: false].
	self note: 'divide: exact (a * b / b), and by SmallIntegers'.
	1 to: 300 do: [:k |
		a := self opnd: 1 + (self below: 300).
		b := self opnd: 1 + (self below: 300).
		q := a liMul: b neg: false.
		(q isKindOf: LIFail) ifFalse: [self div: q by: b neg: false. self div: q by: a neg: true]].
	#(10 1000000000 16r7FFFFFFF 16rFFFFFFFF 1152921504606846975 3 -7) do: [:d |
		#(5 9 33 100 1000) do: [:n | self div: (self opnd: n) by: d neg: false]].
	self note: 'divide: big'.
	#(#(2000 1000) #(4100 2050) #(8000 8) #(8000 4) #(16000 7000) #(20000 9999) #(30000 100)) do: [:p |
		a := self opnd: p first.
		b := self opnd: p last.
		self div: a by: b neg: false]
!
runKernels
	self runMulKernels.
	self runDivKernels.
	self note: 'done ', count printString, ' calls'.
	Stdio stdout flush
! !

