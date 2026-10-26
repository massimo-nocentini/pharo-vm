"The differential driver: 86,417 primitive calls, all pairs over 94 operands (SmallIntegers,
 normalized LargeIntegers of 1 to 48 bytes with adversarial digits, unnormalized and empty
 ones) with add, sub, and, or, xor, compare, multiply and divide; shifts, anyBit, both
 normalizes; big operands up to 8000 digits; Montgomery; wrong types.  Output: one line
 per call, index, op, operands, '=>', answer.  The calls that made the plugin before the
 kernels write past an object (li01) or read uninitialised memory (li06) print SKIP-LI0x
 instead; after 'runUB: true' they run and print UB-LI0x OK when the answer is right.
 run-image-tests.sh puts lib.st in front and sends runDiff."!

!LIH methodsFor: 'diff'!
pow2: n cls: cls
	| x |
	x := cls new: n // 8 + 1.
	x basicAt: n // 8 + 1 put: (1 bitShift: n \\ 8).
	^x
!
smallInts
	^{0. 1. -1. 7. -255. 256. 65535. -65536. 16rFFFFFF. 16r1000000. 16rFFFFFFFF. -16r100000000.
	  16rFFFFFFFFFF. 1 bitShift: 56. SmallInteger maxVal. SmallInteger minVal.
	  SmallInteger maxVal - 1. SmallInteger minVal + 1},
	  ((1 to: 4) collect: [:i | (self next32 bitShift: (self below: 28)) * ((self below: 2) * 2 - 1)])
!
largePool
	| pool |
	pool := OrderedCollection new.
	#(1 2 3 4 5 7 8 9 12 13 15 16 17 20 24 25 28 32 33 36 40 44 48) do: [:n |
		pool add: (self large: LargePositiveInteger bytes: n mode: (self pick: #(#rnd #pat))).
		pool add: (self large: LargeNegativeInteger bytes: n mode: (self pick: #(#rnd #pat)))].
	#(4 8 9 16 17 32) do: [:n | pool add: (self large: LargePositiveInteger bytes: n mode: #ff)].
	pool add: (self large: LargeNegativeInteger bytes: 8 mode: #ff).
	pool add: (self pow2: 64 cls: LargePositiveInteger).
	pool add: (self pow2: 32 cls: LargePositiveInteger).
	pool add: (self pow2: 60 cls: LargePositiveInteger).
	pool add: (self pow2: 60 cls: LargeNegativeInteger).
	pool add: (self pow2: 95 cls: LargePositiveInteger).
	pool add: (self pow2: 127 cls: LargeNegativeInteger).
	^pool
!
unnormPool
	^{self unnormalized: LargePositiveInteger value: 1 lead: 1.
	  self unnormalized: LargeNegativeInteger value: 5 lead: 3.
	  self unnormalized: LargePositiveInteger value: 16rABCDEF lead: 4.
	  self unnormalized: LargePositiveInteger value: 16r12345678 lead: 5.
	  self unnormalized: LargeNegativeInteger value: 16r1122334455 lead: 8.
	  self unnormalized: LargePositiveInteger value: 7 lead: 12.
	  self unnormalized: LargePositiveInteger value: 16rFFFFFFFFFFFFFFF lead: 1.
	  LargePositiveInteger new: 0.
	  LargeNegativeInteger new: 0.
	  LargePositiveInteger new: 1.
	  LargePositiveInteger new: 4.
	  LargeNegativeInteger new: 8.
	  LargePositiveInteger new: 9}
!
pairOps: a with: b
	self add: a to: b.
	self sub: a with: b.
	self and: a with: b.
	self or: a with: b.
	self xor: a with: b.
	self cmp: a with: b.
	self mul: a by: b neg: (self below: 2) = 0.
	self div: a by: b neg: false.
	self div: a by: b neg: true
!
singleOps: a shifts: shifts
	shifts do: [:s | self shift: a by: s].
	#(#(1 1) #(1 8) #(1 32) #(5 40) #(33 64) #(31 33) #(1 1000) #(64 1) #(0 3) #(3 0) #(100 100))
		do: [:ft | self anyBit: a from: ft first to: ft last].
	self normPos: a.
	self normNeg: a
!
bigOperand: digits cls: cls
	^self large: cls bytes: digits * 4 - (self below: 4) mode: (self pick: #(#rnd #rnd #pat))
!
runPairs: all
	self note: 'pairs over ', all size printString, ' operands'.
	all do: [:a | all do: [:b | self pairOps: a with: b]]
!
runSingles: all
	self note: 'singles'.
	all do: [:a | self singleOps: a shifts: #(0 1 5 7 8 31 32 33 63 64 95 200 -1 -5 -8 -31 -32 -33 -64 -200 -100000)].
	"big left shifts of a few small operands"
	(all first: 30) do: [:a | self shift: a by: 100000 + (self below: 64)]
!
runBig: smallSample
	| bigs |
	self note: 'big operands'.
	bigs := OrderedCollection new.
	#(50 64 100 128 129 255 256 500 1000 2000 4000 8000) do: [:d |
		| x y h |
		x := self bigOperand: d cls: LargePositiveInteger.
		y := self bigOperand: d cls: LargeNegativeInteger.
		h := self bigOperand: d // 2 + 1 cls: (self pick: {LargePositiveInteger. LargeNegativeInteger}).
		bigs add: x.
		smallSample do: [:s | self pairOps: x with: s. self pairOps: s with: y].
		self pairOps: x with: y.
		self pairOps: y with: h.
		self pairOps: h with: x.
		self mul: x by: x neg: false.
		self div: (x liMul: h neg: false) by: h neg: false.
		{x. y. h} do: [:a | self singleOps: a shifts: #(1 31 32 33 -1 -33 -1000)]].
	"unbalanced multiplies and divisions"
	self mul: bigs last by: (bigs at: 3) neg: true.
	self div: bigs last by: (bigs at: 3) neg: true.
	self div: bigs last by: (bigs at: 9) neg: false
!
runMontgomery
	| m a b inv |
	self note: 'montgomery'.
	#(1 2 3 4 5 6 7 8 9 10 11 12 16 32 64) do: [:k |
		3 timesRepeat:
			[m := self large: LargePositiveInteger bytes: k * 4 mode: #rnd.
			 m basicAt: 1 put: ((m basicAt: 1) bitOr: 1).
			 inv := self montInv: (self lowDigit: m).
			 a := self large: LargePositiveInteger bytes: (self below: k * 4) + 1 mode: #rnd.
			 b := self large: LargePositiveInteger bytes: (self below: k * 4) + 1 mode: #pat.
			 self mont: a times: b mod: m inv: inv.
			 self mont: b times: a mod: m inv: inv.
			 self mont: a times: a mod: m inv: inv.
			 self mont: m times: b mod: m inv: inv.
			 self mont: a times: b mod: m inv: self next32.
			 self mont: 0 times: b mod: m inv: inv.
			 self mont: 1 times: 1 mod: m inv: inv.
			 self mont: (LargePositiveInteger new: 0) times: b mod: m inv: inv.
			 self mont: (LargePositiveInteger new: 0) times: (LargePositiveInteger new: 0) mod: m inv: inv.
			 self mont: a times: (LargePositiveInteger new: 0) mod: m inv: inv.
			 self mont: (LargeNegativeInteger new: k * 4 + 4) times: b mod: m inv: inv.
			 self mont: a times: b mod: m inv: -1.
			 self mont: a times: b mod: m inv: (self pow2: 32 cls: LargePositiveInteger).
			 self mont: a times: b mod: m inv: (self pow2: 31 cls: LargePositiveInteger).
			 self mont: a times: b mod: m inv: nil.
			 self mont: (self large: LargePositiveInteger bytes: k * 4 + 5 mode: #rnd) times: b mod: m inv: inv]].
	"SmallInteger modulus and operands"
	{3. 7. 65537. 16rFFFFFFFB. 16r1FFFFFFFFF. SmallInteger maxVal} do: [:mv |
		inv := self montInv: (mv bitAnd: 16rFFFFFFFF).
		self mont: 5 times: 6 mod: mv inv: inv.
		self mont: mv - 1 times: mv - 2 mod: mv inv: inv.
		self mont: (self fromSmall: 12345 cls: LargePositiveInteger) times: 2 mod: mv inv: inv].
	self mont: 5 times: 6 mod: (LargePositiveInteger new: 0) inv: 1.
	self mont: (LargePositiveInteger new: 0) times: (LargePositiveInteger new: 0) mod: (LargePositiveInteger new: 0) inv: 1
!
runWrongTypes
	| bads ok |
	self note: 'wrong types'.
	bads := {nil. 1.5. 'x'. ByteArray new: 4. true. Object new. $a. #(1 2)}.
	ok := self large: LargePositiveInteger bytes: 9 mode: #rnd.
	bads do: [:x |
		self pairOps: ok with: x.
		self pairOps: x with: 3.
		self pairOps: x with: ok.
		self shift: ok by: x.
		self call: 'shiftRcv' rcv: x args: #() do: [x liShift: 3].
		self anyBit: ok from: x to: 3.
		self anyBit: ok from: 3 to: x.
		self call: 'anyBitRcv' rcv: x args: #() do: [x liAnyBitFrom: 1 to: 3].
		self mul: ok by: 3 neg: x.
		self div: ok by: 3 neg: x.
		self mont: x times: 3 mod: 7 inv: 1.
		self mont: 3 times: x mod: 7 inv: 1.
		self mont: 3 times: 5 mod: x inv: 1.
		self normPos: x.
		self normNeg: x.
		self call: 'moduleName' rcv: x args: #() do: [x liModuleName].
		self call: 'montLen' rcv: x args: #() do: [x liMontLen]].
	self shift: ok by: (self pow2: 70 cls: LargePositiveInteger).
	self shift: ok by: (self pow2: 70 cls: LargeNegativeInteger).
	self anyBit: ok from: (self pow2: 70 cls: LargePositiveInteger) to: 3.
	self anyBit: ok from: 1 to: SmallInteger maxVal.
	self shift: 0 by: 5.
	self shift: 0 by: -5.
	self shift: SmallInteger minVal by: 3.
	self shift: SmallInteger minVal by: -3.
	{0. -5. ok. LargePositiveInteger new: 0} do: [:r |
		self call: 'moduleName' rcv: r args: #() do: [r liModuleName].
		self call: 'montLen' rcv: r args: #() do: [r liMontLen]]
!
runDiff
	| small large unnorm all |
	small := self smallInts.
	large := self largePool.
	unnorm := self unnormPool.
	all := small, large asArray, unnorm.
	self runPairs: all.
	self runSingles: all.
	self runBig: {7. SmallInteger maxVal. -1. large at: 5. large at: 41. unnorm at: 8. unnorm at: 4}.
	self runMontgomery.
	self runWrongTypes.
	self note: 'done ', count printString, ' calls'.
	Stdio stdout flush
! !

