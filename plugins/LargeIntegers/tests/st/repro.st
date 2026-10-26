"The crash and undefined-behaviour repros of the LargeIntegers plugin, one per method;
 run-image-tests.sh puts lib.st in front and runs each in a VM of its own, since the plugin
 before the kernels crashes on li01, and compares the output with expected/repros.txt.
   li01  digitLshift sized the result from the significant bits but shifted every digit of
         the receiver: an unnormalized receiver wrote past the new object (a segmentation
         fault on the 64,000,000-byte receiver);
   li05  primDigitSubtract of two empty operands read one word below both (the answer
         cannot show it: 0 either way; the C harness shows the read);
   li06  primMontgomeryTimesModulo read slot 0 of an empty second operand, which Spur
         never initialises: answers made of stale eden bytes, now a primitive failure;
   li07  primitives installed at the wrong arity popped their own arity and overwrote the
         caller's slots: now PrimErrBadNumArgs with the slots intact;
   arity every primitive at every wrong argument count 0..4 from a method without temps
         (the slots above the arguments are the caller's frame), 3000 times each (JIT),
         with a LargePositiveInteger and with an Object receiver: the plugin before the
         kernels answered garbage or dereferenced frame words (a segmentation fault for
         Montgomery with no argument); every answer must be #'bad number of arguments'."!

!Object methodsFor: 'li-bad-arity'!
liAddNoArg
	<primitive: 'primDigitAdd' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liAdd: x extra: y
	<primitive: 'primDigitAdd' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liNormPosArg: x
	<primitive: 'primNormalizePositive' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liMontOne: x
	<primitive: 'primMontgomeryTimesModulo' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liModuleNameArg: x
	<primitive: 'primGetModuleName' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liMontLenArg: x
	<primitive: 'primMontgomeryDigitLength' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liDivOne: x
	<primitive: 'primDigitDivNegative' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
!
liShiftNoArg
	<primitive: 'primDigitBitShiftMagnitude' module: 'LargeIntegers' error: ec>
	^LIFail new code: ec
! !

!LIH methodsFor: 'repro'!
li01
	| x r |
	"1000 zero digits on top of 1: the overflow lands in free eden space"
	x := LargePositiveInteger new: 4000.
	x basicAt: 1 put: 1.
	r := x liShift: 1.
	self note: 'mid  shift 1 => ', (self res: r rcv: x).
	r := x liShift: 33.
	self note: 'mid  shift 33 => ', (self res: r rcv: x).
	x := LargeNegativeInteger new: 4001.
	x basicAt: 1 put: 16rFF; basicAt: 2 put: 16r80.
	r := x liShift: 7.
	self note: 'midN shift 7 => ', (self res: r rcv: x).
	"16M zero digits on top of 1: past the end of eden"
	x := LargePositiveInteger new: 64000000.
	x basicAt: 1 put: 1.
	r := x liShift: 1.
	self note: 'big  shift 1 => ', (self res: r rcv: x)
!
li05
	| z1 z2 |
	{LargePositiveInteger. LargeNegativeInteger} do: [:c1 |
		{LargePositiveInteger. LargeNegativeInteger} do: [:c2 |
			z1 := c1 new: 0. z2 := c2 new: 0.
			self note: c1 name, ' new: 0 - ', c2 name, ' new: 0 => ', (self res: (z1 liSub: z2) rcv: z1)]]
!
li06
	| a m inv junk e r |
	m := self large: LargePositiveInteger bytes: 8 mode: #rnd.
	m basicAt: 1 put: ((m basicAt: 1) bitOr: 1).
	inv := self montInv: (self lowDigit: m).
	a := self large: LargePositiveInteger bytes: 4 mode: #rnd.
	1 to: 8 do: [:i |
		"fill eden (several scavenges' worth) with garbage ByteArrays holding a known byte,
		 so the never-initialised slot of the 0-byte object lands on stale bytes"
		1 to: 300000 do: [:k | junk := ByteArray new: 64 withAll: (i * 37) \\ 256].
		junk := nil.
		"a 0-byte LargePositiveInteger made by the plugin itself: the remainder of an exact
		 division (the image's new: 0 zeroes the slot, instantiateClass:indexableSize: does not)"
		e := (a liDiv: a neg: false) last.
		e basicSize = 0 ifFalse: [self error: 'expected an empty remainder'].
		r := a liMont: e mod: m inv: inv.
		self note: 'try ', i printString, ': ', (self opd: a), ' * P0 mod ', (self opd: m), ' => ', (self res: r rcv: a)]
!
show: label array: arr
	self note: label, ' => ', (arr collect: [:e | (e isKindOf: LIFail)
		ifTrue: ['FAIL ', e code printString]
		ifFalse: [e class == SmallInteger ifTrue: [e printString] ifFalse: [e class name]]]) printString
!
li07
	| x p |
	x := self fromSmall: 16r123456789 cls: LargePositiveInteger.
	p := self large: LargePositiveInteger bytes: 12 mode: #rnd.
	self show: '{5. x liAddNoArg}' array: {5. x liAddNoArg}.
	self show: '{7. x liAdd: 1 extra: 2. 9}' array: {7. x liAdd: 1 extra: 2. 9}.
	self show: '{7. 3 liNormPosArg: p. 9}' array: {7. 3 liNormPosArg: p. 9}.
	self show: '{7. x liMontOne: 5. 9}' array: {7. x liMontOne: 5. 9}.
	self show: '{7. x liModuleNameArg: 1. 9}' array: {7. x liModuleNameArg: 1. 9}.
	self show: '{7. x liMontLenArg: 1. 9}' array: {7. x liMontLenArg: 1. 9}.
	self show: '{7. x liDivOne: true. 9}' array: {7. x liDivOne: true. 9}.
	self show: '{5. x liShiftNoArg}' array: {5. x liShiftNoArg}
!
arity
	| prims big shapes wrong |
	prims := #(#(primAnyBitFromTo 2) #(primDigitAdd 1) #(primDigitBitAnd 1) #(primDigitBitOr 1)
		#(primDigitBitShiftMagnitude 1) #(primDigitBitXor 1) #(primDigitCompare 1)
		#(primDigitDivNegative 2) #(primDigitMultiplyNegative 2) #(primDigitSubtract 1)
		#(primGetModuleName 0) #(primMontgomeryDigitLength 0) #(primMontgomeryTimesModulo 3)
		#(primNormalizeNegative 0) #(primNormalizePositive 0)).
	big := (2 raisedTo: 100) + 12345.
	shapes := wrong := 0.
	#(#LargePositiveInteger #Object) do: [:clsName | | cls |
		cls := Smalltalk at: clsName.
		prims do: [:pa | | prim n |
			prim := pa first. n := pa second.
			0 to: 4 do: [:k | n = k ifFalse: [ | sel callSel callArgs bag rcv |
				sel := 'liar', prim, k printString.
				sel := k = 0
					ifTrue: [sel]
					ifFalse: [(1 to: k) inject: '' into: [:s :i | s, sel, 'x', i printString, ': a', i printString, ' ']].
				cls compile: sel, '
	<primitive: ''', prim, ''' module: ''LargeIntegers'' error: ec>
	^ec'.
				callArgs := k = 0
					ifTrue: ['liar', prim, '0']
					ifFalse: [(1 to: k) inject: '' into: [:s :i | s, 'liar', prim, k printString, 'x', i printString, ': 7 ']].
				callSel := 'callLiar', prim, k printString.
				cls compile: callSel, '
	^self ', callArgs.
				rcv := clsName = #Object ifTrue: [Object new] ifFalse: [big].
				bag := Bag new.
				1 to: 3000 do: [:i | bag add: (rcv perform: callSel asSymbol) printString].
				shapes := shapes + 1.
				(bag size = 3000 and: [(bag occurrencesOf: '#''bad number of arguments''') = 3000]) ifFalse:
					[wrong := wrong + 1.
					 self note: prim, ' with ', k printString, ' arguments, ', clsName, ' receiver => ',
						bag sortedCounts printString]]]]].
	self note: 'arity: ', shapes printString, ' shapes, ', wrong printString, ' with another answer'
! !
