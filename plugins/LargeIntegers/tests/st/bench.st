"The benchmark of bench.sh: ns per direct call of primDigitMultiplyNegative and
 primDigitDivNegative by operand size (best of 3 runs, each calibrated to about 40 ms; the
 'empty' row is the cost of the block loop itself), the small operands where the kernels
 dispatch between the 32-bit loops and the limb code, and end-to-end image expressions (ms
 for the slow ones).  One row per line: '<label> ns: <value>' or '<label> ms: <value>'.
 bench.sh puts lib.st in front and sends benchKernels, benchSmall and benchImage."!

!LIH methodsFor: 'bench'!
nsPerCall: aBlock
	| n t best |
	n := 1.
	[t := self usToRun: aBlock times: n. t < 4000] whileTrue: [n := n * 4].
	n := (n * 40000 / (t max: 1)) ceiling max: 1.
	best := nil.
	3 timesRepeat: [t := self usToRun: aBlock times: n. best := best ifNil: [t] ifNotNil: [best min: t]].
	^best * 1000 / n
!
usToRun: aBlock times: n
	| t0 |
	t0 := Time microsecondClockValue.
	1 to: n do: [:k | aBlock value].
	^Time microsecondClockValue - t0
!
row: label ns: ns
	Stdio stdout nextPutAll: label, ' ns: ', (ns asFloat printShowingDecimalPlaces: 1); lf; flush
!
rowMs: label ns: ns
	Stdio stdout nextPutAll: label, ' ms: ', (ns asFloat / 1000000 printShowingDecimalPlaces: 3); lf; flush
!
benchKernels
	| a b c ab ac si |
	self row: 'empty' ns: (self nsPerCall: [nil]).
	si := 16r0FFFFFFFFFFFFFFF - 12345.
	#(64 96 128 192 256 384 512 768 1024 1536 2048 3072 4096 8192 16384 32768 65536 262144) do: [:bits |
		a := self large: LargePositiveInteger bytes: bits // 8 mode: #rnd.
		b := self large: LargePositiveInteger bytes: bits // 8 mode: #rnd.
		c := self large: LargePositiveInteger bytes: bits // 8 - 1 mode: #rnd.
		ab := (a liMul: b neg: false) liAdd: c.
		self row: 'mul ', bits printString, 'x', bits printString, 'b' ns: (self nsPerCall: [a liMul: b neg: false]).
		self row: 'sqr ', bits printString, 'b (x * x)' ns: (self nsPerCall: [a liMul: a neg: false]).
		ac := LargePositiveInteger new: a basicSize.
		1 to: a basicSize do: [:i | ac basicAt: i put: (a basicAt: i)].
		self row: 'sqr ', bits printString, 'b (x * copy of x)' ns: (self nsPerCall: [a liMul: ac neg: false]).
		self row: 'div ', (2 * bits) printString, '/', bits printString, 'b' ns: (self nsPerCall: [ab liDiv: b neg: false]).
		self row: 'div ', bits printString, '/', bits printString, 'b' ns: (self nsPerCall: [a liDiv: c neg: false]).
		self row: 'mul ', bits printString, 'b x SI' ns: (self nsPerCall: [a liMul: si neg: false]).
		self row: 'div ', bits printString, 'b / SI' ns: (self nsPerCall: [a liDiv: 1000000007 neg: false])].
	#(#(256 4096) #(1024 65536) #(4096 65536)) do: [:p |
		a := self large: LargePositiveInteger bytes: p first // 8 mode: #rnd.
		b := self large: LargePositiveInteger bytes: p last // 8 mode: #rnd.
		self row: 'mul ', p first printString, 'x', p last printString, 'b' ns: (self nsPerCall: [a liMul: b neg: false]).
		self row: 'div ', p last printString, '/', p first printString, 'b' ns: (self nsPerCall: [b liDiv: a neg: false])].
	"short quotients over long divisors: #(divisor bits, quotient bits)"
	#(#(6144 1536) #(16384 2048) #(65536 1536) #(65536 2560)) do: [:p |
		a := self large: LargePositiveInteger bytes: p first // 8 mode: #rnd.
		b := self large: LargePositiveInteger bytes: p first + p last // 8 mode: #rnd.
		self row: 'div ', (p first + p last) printString, '/', p first printString, 'b' ns: (self nsPerCall: [b liDiv: a neg: false])].
	self row: 'mul SI x SI' ns: (self nsPerCall: [si liMul: si neg: false]).
	self row: 'div SI / SI' ns: (self nsPerCall: [si liDiv: 1000003 neg: false])
!
benchSmall
	| si l h m |
	si := 16r0FFFFFFFFFFFFFFF - 12345.
	self row: 'div SI(2d) / 1000003' ns: (self nsPerCall: [si liDiv: 1000003 neg: false]).
	self row: 'div SI(2d) / SI(2d)' ns: (self nsPerCall: [si liDiv: 16r123456789AB neg: false]).
	#(8 16 24 32 48 64 96 128) do: [:bytes |
		l := self large: LargePositiveInteger bytes: bytes mode: #rnd.
		h := self large: LargePositiveInteger bytes: bytes // 2 mode: #rnd.
		m := self large: LargePositiveInteger bytes: bytes - 1 mode: #rnd.
		self row: 'div L', bytes printString, ' / 1000003' ns: (self nsPerCall: [l liDiv: 1000003 neg: false]).
		self row: 'div L', bytes printString, ' / L', (bytes // 2) printString ns: (self nsPerCall: [l liDiv: h neg: false]).
		self row: 'div L', bytes printString, ' / L', (bytes - 1) printString ns: (self nsPerCall: [l liDiv: m neg: false]).
		self row: 'mul L', bytes printString, ' x SI' ns: (self nsPerCall: [l liMul: si neg: false]).
		self row: 'mul L', bytes printString, ' x L', bytes printString ns: (self nsPerCall: [l liMul: l neg: false])].
	l := self large: LargePositiveInteger bytes: 16 mode: #rnd.
	self row: 'add L16 + SI' ns: (self nsPerCall: [l liAdd: 12345]).
	self row: 'add SI + SI (carry)' ns: (self nsPerCall: [16rFFFFFFFF liAdd: 1]).
	self row: 'sub L16 - L16' ns: (self nsPerCall: [l liSub: l]).
	self row: 'and L16 & SI' ns: (self nsPerCall: [l liAnd: 255]).
	self row: 'shift SI << 40' ns: (self nsPerCall: [7 liShift: 40]).
	self row: 'shift L16 >> 13' ns: (self nsPerCall: [l liShift: -13]).
	self row: 'cmp L16 L16' ns: (self nsPerCall: [l liCompare: l]).
	self row: 'normalize L16 (no allocation)' ns: (self nsPerCall: [l liNormPos])
!
benchImage
	| x p q r |
	x := (2 raisedTo: 62) + 12345.
	p := (3 raisedTo: 2000) + 1.
	q := (7 raisedTo: 700) + 3.
	r := (11 raisedTo: 9000) - 1.
	self row: 'image x * x' ns: (self nsPerCall: [x * x]).
	self row: 'image (3^2000) * (7^700)' ns: (self nsPerCall: [p * q]).
	self row: 'image (11^9000) // (7^700)' ns: (self nsPerCall: [r // q]).
	self row: 'image (11^9000) \\ (3^2000)' ns: (self nsPerCall: [r \\ p]).
	self row: 'image (11^9000) gcd: (3^2000 + 1)' ns: (self nsPerCall: [r gcd: p]).
	self rowMs: 'image 1000 factorial' ns: (self nsPerCall: [1000 factorial]).
	self rowMs: 'image 5000 factorial' ns: (self nsPerCall: [5000 factorial]).
	self rowMs: 'image (2 raisedTo: 100000) printString' ns: (self nsPerCall: [(2 raisedTo: 100000) printString]).
	self rowMs: 'image (3 raisedTo: 100000) printString' ns: (self nsPerCall: [(3 raisedTo: 100000) printString]).
	self rowMs: 'image (3 raisedTo: 200000) squared' ns: (self nsPerCall: [(3 raisedTo: 200000) squared]).
	self rowMs: 'image 10 squarings of 3^200000' ns: (self nsPerCall: [| y | y := 3 raisedTo: 200000. 10 timesRepeat: [y * y]]).
	self rowMs: 'image (7 raisedTo: 300000) // (3 raisedTo: 200000)' ns: (self nsPerCall: [(7 raisedTo: 300000) // (3 raisedTo: 200000)])
! !
