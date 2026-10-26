"
LargeIntegersPlugin provides functions for speeding up LargeInteger arithmetics. Usually it is part of your installation as 'LargeIntegers' plugin (a C-compiled binary).


Correctly installed?
----------------------
Probably you are just working with it.

To be really sure try
	100 factorial. ""to force plugin loading""
	SmalltalkImage current listLoadedModules.
Then you should see 'LargeIntegers' somewhere in the output strings. If this should not be the case, you probably have a problem.


Variables
-----------

Inst vars:
	andOpIndex			C constant
	orOpIndex			C constant
	xorOpIndex 			C constant
Used like an enum, in ST one would use symbols instead.

Class vars:
	LIStackDigits		C constant: the number of 32-bit digits of the scratch arrays on the C stack


Primitives
-----------

Every primitive first checks the number of arguments it was called with (interpreterProxy methodArgumentCount) and fails with PrimErrBadNumArgs when it is not the number of arguments of its selector, before it reads the receiver or validates an argument: with the wrong count, the stack slots it would read are not its receiver and arguments, and could be words of a frame header, which isKindOfInteger: must not dereference. That is why the arguments are declared #Oop to #primitive:parameters:receiver: and checked by hand after the count: the validation SmartSyntax generates for the other types runs before any statement of the method. The checks answer PrimErrBadArgument as that validation did, and in the same order (the arguments, then the receiver), so a call with the right count fails as before. The count check costs a proxy call (about 2.5 ns) on every call.


C source
---------

The checked-in LargeIntegers.c is generated from this class by Pharo Slang, then adapted to the conventions of the VMMaker.oscog-eem.2495 output it was first generated with: no '_' prefix on locals, asOop: translated to integerObjectOf()/trueObject()/falseObject(), and the argument checks of the primitives written as that generator wrote them (the Pharo Slang ones are not usable as is). Some functions this class has not changed since then keep that output's text. Keep the C equivalent to this class when changing either. The multiply and divide kernels are hand-written C in LargeIntegersKernels.c.


History
--------
v2.1

- fix: >>digitDivLarge:with:negative: now checks out of memory
- fix: >>digit:Lshift: shifts only the significant digits; the leading zero digits of an unnormalized receiver were written past the new object
- fix: >>createLargeFromSmallInteger: and >>primGetModuleName check their allocation
- fix: >>digitSubLarge:with: no longer reads below two empty operands, >>digitMontgomery:times:modulo:mInvModB: fails for an empty second operand instead of reading past it
- every primitive fails with PrimErrBadNumArgs when called with the wrong number of arguments, before it reads its receiver or validates its arguments (see Primitives above)
- SmallInteger operands are no longer converted to scratch LargeIntegers: their digits are read from C arrays (>>pointerToDigitsOf:small:byteSizeInto:)
- a division no longer allocates shifted and grown scratch copies of its operands: Knuth's algorithm D runs on C scratch memory, on the stack or malloc'ed (>>digitDivLarge:with:negative:, >>digitBuffer:stack:)
- results of up to LIStackDigits digits are computed on the stack and allocated once, at their normalized size, or not at all when they fit a SmallInteger (>>normalized:digits:len:negative:, >>normalizedIntegerFrom:len:negative:); larger ones are computed into a LargeInteger and normalized as before. The deliberately unnormalized answers (the carry case of >>digitAddLarge:with:, the left shift, both halves of a division) keep their sizes
- multiplication and division run C kernels, hand-written in LargeIntegersKernels.c (>>cDigitMultiply:len:with:len:into:len:, >>cDigitDiv:len:rem:len:quo:len:): over 64-bit limbs where the C compiler has 128-bit arithmetic, Karatsuba's algorithm for long products, Knuth's algorithm D with a precomputed reciprocal of the top divisor limb. They answer the digits of the 32-bit loops they replace (>>liDigitMultiply:len:with:len:into:len:, >>liDigitDiv:len:rem:len:quo:len:, kept for simulation)
(the version string is unchanged: no answer of a primitive changes for valid input)

v2.0

Rewrite primitives to use 32-bits digits instead of 8-bits digits. This can fasten many operation up to 2x and multiplication up to 7x.
Since Large Integers are still variable byte object, care is taken to allocate just enough bytes to contain integer value (not necessarily a multiple of 4 bytes).
Primitives will use the extra bytes beyond necessary bytes up to next multiple of 4, so it's mandatory that those extra bytes be correctly set to zero
At image side, digits are still 8-bits so that fallback code involve only SmallInteger.

Primitives assume that byte ordering (endianness) is allways little endian, because LargeIntegers remain regular byte Objects.
On (hypothetic) big endian machines, conversion is performed at fetch/store time thru usage of dedicated macro.

v1.5

- no code change at all compared to v1.4
- made to outsource testing code (LargeIntegersPluginTest) introduced in earlier versions
- updated class comment: reference to LargeIntegersPluginTest removed

v1.4

- no semantic change compared to v1.3
- >>cHighBit: improved (could be faster now)
- fix: class comment
- improved class comment
- >>flag: introduced to allow #flag: messages (does nothing)
- new: class>>buildCodeGeneratorUpTo: as hook for switching debugMode (default is not to change anything)
- removed: class>>new (obsolete)
- minor cleanup of source code layout

v1.3

- fix: >>primDigitDiv:negative: now checks if its Integer args are normalized; without this change the plugin crashes, if a division by zero through a non normalized - filled with zero bytes - arg occurs. This can happen through printing by the inspector windows after changing the bytes of a LargeInteger manually.

v1.2

- fix: >>anyBitOfBytes: aBytesOop from: start to: stopArg

v1.1

- >>primGetModuleName for checking the version of the plugin;

- >>primDigitBitShiftMagnitude and >>primAnyBitFrom:to: for supporting - not installing! - unification of shift semantics of negative Integers;

v1.0

- speeds up digitDiv:neg: at about 20%.
	In >>cCoreDigitDivDiv:len:rem:len:quo:len: the 'nibble' arithmetic is removed.

"
Class {
	#name : 'LargeIntegersPlugin',
	#superclass : 'SmartSyntaxInterpreterPlugin',
	#instVars : [
		'andOpIndex',
		'orOpIndex',
		'xorOpIndex'
	],
	#classVars : [
		'LIStackDigits'
	],
	#category : 'VMMaker-Plugins',
	#package : 'VMMaker',
	#tag : 'Plugins'
}

{ #category : 'translation' }
LargeIntegersPlugin class >> declareCVarsIn: cg [ 
	cg var: 'andOpIndex' declareC: 'const int  andOpIndex = 0'.
	cg var: 'orOpIndex' declareC: 'const int  orOpIndex = 1'.
	cg var: 'xorOpIndex' declareC: 'const int  xorOpIndex = 2'.
	"the multiply and divide kernels, hand-written C in LargeIntegersKernels.c"
	cg addHeaderFile: '"LargeIntegersKernels.h"'
]

{ #category : 'class initialization' }
LargeIntegersPlugin class >> initialize [
	"LargeIntegersPlugin initialize"
	"Results of up to LIStackDigits 32-bit digits are computed in C arrays on the stack, see #normalized:digits:len:negative:, and so are the shifted operands of a division, which are malloc'ed when they need more, see #digitBuffer:stack:."
	LIStackDigits := 128
]

{ #category : 'translation' }
LargeIntegersPlugin class >> moduleName [
	^'LargeIntegers'
]

{ #category : 'translation' }
LargeIntegersPlugin class >> moduleNameAndVersion [
	"Answer the receiver's module name and version info that is used for the plugin's C code. The default is to append the code generation date, but any useful text is ok (keep it short)"

	^ self moduleName, ' ', self version
]

{ #category : 'translation' }
LargeIntegersPlugin class >> version [
	"Answer the receiver's version info as String."

	^ 'v2.1'
]

{ #category : 'util' }
LargeIntegersPlugin >> anyBitOfDigits: pDigits len: digitLen from: start to: stopArg [ 
	"Tests for any magnitude bits in the interval from start to stopArg of the digitLen 32-bit digits at pDigits."
	| stop firstDigitIx lastDigitIx firstMask lastMask |
	<var: #pDigits type: #'unsigned int *'>
	<var: #digit type: #'unsigned int'>
	<var: #firstMask type: #'unsigned int'>
	<var: #lastMask type: #'unsigned int'>
	<var: #firstDigitIx type: #usqInt>
	<var: #lastDigitIx type: #usqInt>
	<var: #ix type: #usqInt>
	self
		debugCode: [self msg: 'anyBitOfDigits: pDigits len: digitLen from: start to: stopArg'].
	(start < 1 or: [ stopArg < 1 ])
		ifTrue: [^ interpreterProxy primitiveFail].
	stop := stopArg min: (self cDigitHighBit: pDigits len: digitLen).
	start > stop
		ifTrue: [^ false].
	firstDigitIx := start - 1 // 32 + 1.
	lastDigitIx := stop - 1 // 32 + 1.
	firstMask := 16rFFFFFFFF asUnsignedLong << (start - 1 bitAnd: 31). "Note asUnsignedLong required to avoid ULLL suffix bug"
	lastMask := 16rFFFFFFFF >> (31 - (stop - 1 bitAnd: 31)).
	firstDigitIx = lastDigitIx
		ifTrue: [| digit | 
			digit := self cDigitOf: pDigits at: firstDigitIx - 1.
			^ (digit bitAnd: (firstMask bitAnd: lastMask))
				~= 0].
	((self cDigitOf: pDigits at: firstDigitIx - 1) bitAnd: firstMask)
			~= 0
		ifTrue: [^ true].
	firstDigitIx + 1
		to: lastDigitIx - 1
		do: [:ix | (self cDigitOf: pDigits at: ix - 1)
					~= 0
				ifTrue: [^ true]].
	((self cDigitOf: pDigits at: lastDigitIx - 1)  bitAnd: lastMask)
			~= 0
		ifTrue: [^ true].
	^ false
]

{ #category : 'util' }
LargeIntegersPlugin >> byteSizeOfCSI: csi [ 
	"Answer the number of bytes required to represent the value of a C-SmallInteger."
	csi >= 0 ifTrue:
		[csi < 256 ifTrue:
			[^1].
		 csi < 65536 ifTrue:
			[^2].
		 csi < 16777216 ifTrue:
			[^3].
		 self cppIf: interpreterProxy bytesPerOop = 4
			ifTrue:
				[^4]
			ifFalse:
				[csi < 4294967296 ifTrue:
					[^4].
				 csi < 1099511627776 ifTrue:
					[^5].
				 csi < 281474976710656 ifTrue:
					[^6].
				 csi < 72057594037927936 ifTrue:
					[^7].
				 ^8]].
	csi > -256 ifTrue:
		[^1].
	csi > -65536 ifTrue:
		[^2].
	csi > -16777216 ifTrue:
		[^3].
	self cppIf: interpreterProxy bytesPerOop = 4
		ifTrue:
			[^4]
		ifFalse:
			[csi > -4294967296 ifTrue:
				[^4].
			 csi > -1099511627776 ifTrue:
				[^5].
			 csi > -281474976710656 ifTrue:
				[^6].
			 csi > -72057594037927936 ifTrue:
				[^7].
			^8]
]

{ #category : 'util' }
LargeIntegersPlugin >> byteSizeOfLargeInt: bytesOop [ 
	"Answer the number of bytes required to represent a LargeInteger.
	Precondition: bytesOop is not a small integer."
	<inline: #always>
	^ interpreterProxy slotSizeOf: bytesOop
]

{ #category : 'C core util' }
LargeIntegersPlugin >> byteSwapped32IfBigEndian: anInteger [
	"This is only for simulation purposes"
	<doNotGenerate>
	^interpreterProxy byteSwapped32IfBigEndian: anInteger
]

{ #category : 'C core util' }
LargeIntegersPlugin >> byteSwapped64IfBigEndian: anInteger [
	"This is only for simulation purposes"
	<doNotGenerate>
	^interpreterProxy byteSwapped64IfBigEndian: anInteger
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitAdd: pWordShort len: shortLen with: pWordLong len: longLen into: pWordRes [ 
	"pWordRes len = longLen; returns over.."
	| accum |
	<returnTypeC: #'unsigned int'>
	<var: #pWordShort type: #'unsigned int *'>
	<var: #pWordLong type: #'unsigned int *'>
	<var: #pWordRes type: #'unsigned int *'>
	<var: #accum type: #'unsigned long long'>
	accum := 0.
	0 to: shortLen - 1 do: 
		[:i | 
		accum := (accum >> 32)
					+ (self cDigitOf: pWordShort at: i)
					+ (self cDigitOf: pWordLong at: i).
		self cDigitOf: pWordRes at: i put: (accum bitAnd: 16rFFFFFFFF)].
	shortLen to: longLen - 1 do: 
		[:i | 
		accum := (accum >> 32)
					+ (self cDigitOf: pWordLong at: i).
		self cDigitOf: pWordRes at: i put: (accum bitAnd: 16rFFFFFFFF)].
	^ accum >> 32
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitCompare: pFirst with: pSecond len: len [ 
	"Precondition: pFirst len = pSecond len."
	| secondDigit ix firstDigit |
	<var: #pFirst type: #'unsigned int *'>
	<var: #pSecond type: #'unsigned int *'>
	<var: #firstDigit type: #'unsigned int'>
	<var: #secondDigit type: #'unsigned int'>
	ix := len - 1.
	[ix >= 0]
		whileTrue: 
			[(secondDigit := pSecond at: ix) ~= (firstDigit := pFirst at: ix)
				ifTrue: [(self byteSwapped32IfBigEndian: secondDigit) < (self byteSwapped32IfBigEndian: firstDigit)
						ifTrue: [^ 1]
						ifFalse: [^ -1]].
			ix := ix - 1].
	^ 0
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitCopyFrom: pFrom to: pTo len: len [ 
	<var: #pFrom type: #'unsigned int *'>
	<var: #pTo type: #'unsigned int *'>

	"Note: don't care about endianness here, copy operation is endian neutral"
	0 to: len - 1 do: [:i | pTo at: i put: (pFrom at: i)].
	^ 0
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitDiv: pDiv len: divLen rem: pRem len: remLen quo: pQuo len: quoLen [ 
	"Knuth's algorithm D. pDiv holds divLen 32-bit digits: the divisor shifted left until the top bit of its digit divLen - 1 (ST indexed) is set, and a zero digit. pRem holds the remLen digits of the dividend shifted left by as many bits. Store the quoLen = remLen - divLen + 1 digits of the quotient at pQuo, and leave the shifted remainder in the low divLen - 1 digits of pRem, with zeros above.
	 The C kernel liDigitDiv() of LargeIntegersKernels.c computes them over 64-bit limbs where the C compiler has 128-bit arithmetic, else over 32-bit digits, estimating each quotient limb with a precomputed reciprocal of the top divisor limb. #liDigitDiv:len:rem:len:quo:len: is the loop it replaces."
	<var: #pDiv type: #'unsigned int *'>
	<var: #pRem type: #'unsigned int *'>
	<var: #pQuo type: #'unsigned int *'>
	self liDigitDiv: pDiv len: divLen rem: pRem len: remLen quo: pQuo len: quoLen.
	^0
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitHighBit: pUint32 len: len [ 
	"Answer the index (in bits) of the high order bit of the receiver, or zero if the    
	 receiver is zero. This method is allowed (and needed) for     
	LargeNegativeIntegers as well, since Squeak's LargeIntegers are sign/magnitude.
	Work with 32 bits digits."
	| realLength lastDigit |
	<var: #pUint32 type: #'unsigned int *'>
	<var: #lastDigit type: #'unsigned int'>
	realLength := len.
	[realLength = 0 ifTrue: [^0].
	(lastDigit := self cDigitOf: pUint32 at: (realLength := realLength - 1)) = 0]
		whileTrue.
	^  (self cHighBit32: lastDigit) + (32 * realLength)
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitLshift: shiftCount from: pFrom len: len into: pTo [ 
	"Store the len digits at pFrom shifted left by shiftCount bits, 0 <= shiftCount < 32, into the len + 1 digits at pTo."
	<var: #pTo type: #'unsigned int *'>
	<var: #pFrom type: #'unsigned int *'>
	"Note: 0 is endian neutral, use direct access"
	pTo at: len put: 0.
	shiftCount = 0
		ifTrue: [self cDigitCopyFrom: pFrom to: pTo len: len]
		ifFalse: [self cDigitLshift: shiftCount from: pFrom len: len to: pTo len: len + 1].
	^0
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitLshift: shiftCount from: pFrom len: lenFrom to: pTo len: lenTo [ 
	"C indexed!"
	| digitShift bitShift carry limit digit rshift |
	<var: #pTo type: #'unsigned int *'>
	<var: #pFrom type: #'unsigned int *'>
	<var: #carry type: #'unsigned int'>
	<var: #digit type: #'unsigned int'>
	digitShift := shiftCount // 32.
	bitShift := shiftCount \\ 32.
	
	limit := digitShift - 1.
	"Note: 0 is endian neutral, use direct access"
	0 to: limit do: [:i | pTo at: i put: 0].
	
	bitShift = 0 ifTrue: ["Fast version for digit-aligned shifts"
		"C indexed!"
		^ self
			cDigitReplace: pTo
			from: digitShift
			to: lenTo - 1
			with: pFrom
			startingAt: 0].
		
	"This implementation use at most 31 bits of carry.
	bitAnd: 16rFFFFFFFF is only for simulator, useless in C"
	rshift := 32 - bitShift.
	carry := 0.
	limit := lenFrom - 1.
	0 to: limit do: 
		[:i | 
		digit := self cDigitOf: pFrom at: i.
		self cDigitOf: pTo at: i + digitShift put: ((carry bitOr: digit << bitShift) bitAnd: 16rFFFFFFFF).
		carry := digit >> rshift].
	carry = 0 ifFalse: [self cDigitOf: pTo at: lenTo - 1 put: carry].
	^0
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitMontgomery: pFirst
				len: firstLen
				times: pSecond
				len: secondLen
				modulo: pThird
				len: thirdLen
				mInvModB: mInv
				into: pRes [
				
	| u limit1 limit2 limit3 accum accum2 accum3 lastDigit |
	<var: #pFirst type: #'unsigned int *'>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pThird type: #'unsigned int *'>
	<var: #mInv type: #'unsigned int'>
	<var: #pRes type: #'unsigned int *'>
	<var: #accum type: #'unsigned long long'>
	<var: #accum2 type: #'unsigned long long'>
	<var: #accum3 type: #'unsigned long long'>
	<var: #u type: #'unsigned long long '>
	<var: #lastDigit type: #'unsigned int '>
	limit1 := firstLen - 1.
	limit2 := secondLen - 1.
	limit3 := thirdLen - 1.
	lastDigit := 0.
	0 to: limit1 do: 
		[:i | 
		accum3 := self cDigitOf: pFirst at: i.
		accum3 := accum3*(self cDigitOf: pSecond at: 0) + (self cDigitOf: pRes at: 0).
		u := accum3 * mInv bitAnd: 16rFFFFFFFF.
		accum2 :=  u * (self cDigitOf: pThird at: 0).
		accum := (accum2 bitAnd: 16rFFFFFFFF) + (accum3 bitAnd: 16rFFFFFFFF).
		accum := (accum >> 32) + (accum2 >> 32) + (accum3 >> 32).
		1 to: limit2 do: [:k |
			accum3 := self cDigitOf: pFirst at: i.
			accum3 := accum3*(self cDigitOf: pSecond at: k) + (self cDigitOf: pRes at: k).
			accum2 :=  u * (self cDigitOf: pThird at: k).
			accum := accum + (accum2 bitAnd: 16rFFFFFFFF) + (accum3 bitAnd: 16rFFFFFFFF).
			self cDigitOf: pRes at: k-1 put: (accum bitAnd: 16rFFFFFFFF).
			accum := (accum >> 32) + (accum2 >> 32) + (accum3 >> 32)].
		secondLen to: limit3 do: [:k |
			accum2 :=  u * (self cDigitOf: pThird at: k).
			accum := accum + (self cDigitOf: pRes at: k) + (accum2 bitAnd: 16rFFFFFFFF).
			self cDigitOf: pRes at: k-1 put: (accum bitAnd: 16rFFFFFFFF).
			accum := (accum >> 32) + (accum2 >> 32)].
		accum := accum + lastDigit.
		self cDigitOf: pRes at: limit3 put: (accum bitAnd: 16rFFFFFFFF).
		lastDigit := accum >> 32].
	firstLen to: limit3 do: 
		[:i | 
		accum := (self cDigitOf: pRes at: 0).
		u := accum * mInv bitAnd: 16rFFFFFFFF.
		accum := accum + (u * (self cDigitOf: pThird at: 0)).
		accum := accum >> 32.
		1 to: limit3 do: [:k |
			accum2 :=  u * (self cDigitOf: pThird at: k).
			accum := accum + (self cDigitOf: pRes at: k) + (accum2 bitAnd: 16rFFFFFFFF).
			self cDigitOf: pRes at: k-1 put: (accum bitAnd: 16rFFFFFFFF).
			accum := (accum >> 32) + (accum2 >> 32)].
		accum := accum + lastDigit.
		self cDigitOf: pRes at: limit3 put: (accum bitAnd: 16rFFFFFFFF).
		lastDigit := accum >> 32].
	(lastDigit = 0 and: [(self cDigitCompare: pThird with: pRes len: thirdLen) = 1]) ifFalse: [
		"self cDigitSub: pThird len: thirdLen with: pRes len: thirdLen into: pRes"
		accum := 0.
		0 to: limit3 do: 
			[:i | 
			accum := accum + (self cDigitOf: pRes at: i) - (self cDigitOf: pThird at: i).
			self cDigitOf: pRes at: i put: (accum bitAnd: 16rFFFFFFFF).
			accum := 0 - (accum >> 63)]].
	^0
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitMultiply: pWordShort len: shortLen with: pWordLong len: longLen into: pWordRes len: resLen [
	"Store into the resLen 32-bit digits at pWordRes, which start out all zero, the low resLen digits of the product of the shortLen digits at pWordShort and the longLen digits at pWordLong.
	 The C kernel liDigitMultiply() of LargeIntegersKernels.c computes them over 64-bit limbs where the C compiler has 128-bit arithmetic, else over 32-bit digits, with Karatsuba's algorithm for long operands. #liDigitMultiply:len:with:len:into:len: is the schoolbook loop it replaces."
	<returnTypeC: #'unsigned int'>
	<var: #pWordShort type: #'unsigned int *'>
	<var: #pWordLong type: #'unsigned int *'>
	<var: #pWordRes type: #'unsigned int *'>
	self liDigitMultiply: pWordShort len: shortLen with: pWordLong len: longLen into: pWordRes len: resLen.
	^ 0
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitOf: cPointer at: zeroBasedDigitIndex [
	<inline: true>
	<returnTypeC: #'unsigned int'>
	<var: 'cPointer' type: #'unsigned int *'>
	^self byteSwapped32IfBigEndian: ((self cCode: [cPointer] inSmalltalk: [interpreterProxy cCoerce: cPointer to: #'unsigned int *']) at: zeroBasedDigitIndex)
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitOf: cPointer at: zeroBasedDigitIndex put: aValue [
	<inline: true>
	<returnTypeC: #'unsigned int'>
	<var: 'cPointer' type: #'unsigned int *'>
	<var: 'aValue' type: #'unsigned int'>
	^(self cCode: [cPointer] inSmalltalk: [interpreterProxy cCoerce: cPointer to: #'unsigned int *'])
		at: zeroBasedDigitIndex
		put: (self byteSwapped32IfBigEndian: aValue)
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitOp: opIndex short: pWordShort len: shortLen long: pWordLong len: longLen into: pWordRes [ 
	"pWordRes len = longLen.
	NOTE: we don't bother with endianness here, those bit opes are endian-neutral"
	| limit |
	<var: #pWordShort type: #'unsigned int *'>
	<var: #pWordLong type: #'unsigned int *'>
	<var: #pWordRes type: #'unsigned int *'>
	limit := shortLen - 1.
	opIndex = andOpIndex
		ifTrue: 
			[0 to: limit do: [:i | pWordRes at: i put: ((pWordShort at: i)
						bitAnd: (pWordLong at: i))].
			limit := longLen - 1.
			shortLen to: limit do: [:i | pWordRes at: i put: 0].
			^ 0].
	opIndex = orOpIndex
		ifTrue: 
			[0 to: limit do: [:i | pWordRes at: i put: ((pWordShort at: i)
						bitOr: (pWordLong at: i))].
			limit := longLen - 1.
			shortLen to: limit do: [:i | pWordRes at: i put: (pWordLong at: i)].
			^ 0].
	opIndex = xorOpIndex
		ifTrue: 
			[0 to: limit do: [:i | pWordRes at: i put: ((pWordShort at: i)
						bitXor: (pWordLong at: i))].
			limit := longLen - 1.
			shortLen to: limit do: [:i | pWordRes at: i put: (pWordLong at: i)].
			^ 0].
	^ interpreterProxy primitiveFail
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitReplace: pTo from: start to: stop with: pFrom startingAt: repStart [ 
	"C indexed!"
	<var: #pTo type: #'unsigned int *'>
	<var: #pFrom type: #'unsigned int *'>
	^ self
		cDigitCopyFrom: pFrom + repStart
		to: pTo + start
		len: stop - start + 1
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitRshift: shiftCount from: pFrom len: lenFrom to: pTo len: lenTo [ 
	| carry digit digitShift bitShift leftShift limit start |
	<var: #pTo type: #'unsigned int *'>
	<var: #pFrom type: #'unsigned int *'>
	<var: #carry type: #'unsigned int'>
	<var: #digit type: #'unsigned int'>
	digitShift := shiftCount // 32.
	bitShift := shiftCount \\ 32.
	bitShift = 0 ifTrue: ["Fast version for digit-aligned shifts"
		"C indexed!"
		^self
			cDigitReplace: pTo
			from: 0
			to: lenTo - 1
			with: pFrom
			startingAt: digitShift].
		
	"This implementation use at most 31 bits of carry.
	bitAnd: 16rFFFFFFFF is only for simulator, useless in C"
	leftShift := 32 - bitShift.
	carry := (self cDigitOf: pFrom at: digitShift) >> bitShift.
	start := digitShift + 1.
	limit := lenFrom - 1.
	start to: limit do: 
		[:j | 
		digit := self cDigitOf: pFrom at: j.
		self cDigitOf: pTo at: j - start put: ((carry bitOr: digit << leftShift) bitAnd: 16rFFFFFFFF).
		carry := digit >> bitShift].
	carry = 0 ifFalse: [self cDigitOf: pTo at: lenTo - 1 put: carry].
	^0
]

{ #category : 'C core' }
LargeIntegersPlugin >> cDigitSub: pWordSmall
		len: smallLen
		with: pWordLarge
		len: largeLen
		into: pWordRes [
	| z |
	<var: #pWordSmall type: #'unsigned int *'>
	<var: #pWordLarge type: #'unsigned int *'>
	<var: #pWordRes type: #'unsigned int *'>
	<var: #z type: #'unsigned long long'>

	z := 0.
	0 to: smallLen - 1 do: 
		[:i | 
		z := z + (self cDigitOf: pWordLarge at: i) - (self cDigitOf: pWordSmall at: i).
		self cDigitOf: pWordRes at: i put: (z bitAnd: 16rFFFFFFFF).
		z := 0 - (self cCode: [z >> 63] inSmalltalk: [z >> 63 bitAnd: 1])].
	smallLen to: largeLen - 1 do: 
		[:i | 
		z := z + (self cDigitOf: pWordLarge at: i) .
		self cDigitOf: pWordRes at: i put: (z bitAnd: 16rFFFFFFFF).
		z := 0 - (self cCode: [z >> 63] inSmalltalk: [z >> 63 bitAnd: 1])].
	^0
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cDigitZero: pTo len: len [ 
	<var: #pTo type: #'unsigned int *'>
	"Note: 0 is endian neutral, use direct access"
	0 to: len - 1 do: [:i | pTo at: i put: 0].
	^0
]

{ #category : 'C core util' }
LargeIntegersPlugin >> cHighBit32: anUnsignedInt32 [ 
	"Answer the index of the high order bit of the argument, or zero if the  
	argument is zero."
	| shifted bitNo |

	<var: #anUnsignedInt32 type: #'unsigned int'>
	<var: #shifted type: #'unsigned int'>
	shifted := anUnsignedInt32.
	bitNo := 0.
	shifted < (1 << 16)
		ifFalse: [shifted := shifted >> 16.
			bitNo := bitNo + 16].
	shifted < (1 << 8)
		ifFalse: [shifted := shifted >> 8.
			bitNo := bitNo + 8].
	shifted < (1 << 4)
		ifFalse: [shifted := shifted >> 4.
			bitNo := bitNo + 4].
	shifted < (1 << 2)
		ifFalse: [shifted := shifted >> 2.
			bitNo := bitNo + 2].
	shifted < (1 << 1)
		ifFalse: [shifted := shifted >> 1.
			bitNo := bitNo + 1].
	"shifted 0 or 1 now"
	^ bitNo + shifted
]

{ #category : 'oop util' }
LargeIntegersPlugin >> createLargeFromSmallInteger: anOop [ 
	"anOop has to be a SmallInteger!"
	| val res pDigit byteSize digitSize |
	<var: #pDigit type: #'unsigned int *'>
	val := interpreterProxy integerValueOf: anOop.
	byteSize := self byteSizeOfCSI: val.
	res := self createLargeIntegerNeg: val < 0 byteLength: byteSize.
	res ifNil: [interpreterProxy primitiveFailFor: PrimErrNoMemory. ^nil].
	pDigit := self pointerToFirstDigitOfLargeInt: res.
	digitSize := byteSize + 3 // 4.
	1 to: digitSize do: [:ix | self cDigitOf: pDigit at: ix - 1 put: (self digitOfCSI: val at: ix)].
	^ res
]

{ #category : 'util' }
LargeIntegersPlugin >> createLargeIntegerNeg: neg byteLength: byteLength [
	<inline: #always>
	^interpreterProxy
		instantiateClass: (neg
				ifTrue: [interpreterProxy classLargeNegativeInteger]
				ifFalse: [interpreterProxy classLargePositiveInteger])
		indexableSize: byteLength
]

{ #category : 'util' }
LargeIntegersPlugin >> createLargeIntegerNeg: neg digitLength: digitLength [
	<inline: #always>
	^self createLargeIntegerNeg: neg byteLength: digitLength * 4
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digit: anInteger digits: pDigits small: pSmall len: byteLen Lshift: shiftCount [ 
	"Answer a new LargeInteger of the class anInteger has or converts to, with its magnitude, the byteLen bytes at pDigits, shifted left by shiftCount bits. See #pointerToDigitsOf:small:byteSizeInto:."
	"Does not normalize: the answer is a LargeInteger even when it would fit a SmallInteger, and one of 1 byte for a zero magnitude."
	| newOop highBit newDigitLen newByteLen largeClass |
	<var: #pDigits type: #'unsigned int *'>
	<var: #pSmall type: #'unsigned int *'>
	"pDigits == pSmall tells a SmallInteger anInteger without another #isIntegerObject: call"
	largeClass := pDigits == pSmall
				ifTrue: [(interpreterProxy integerValueOf: anInteger) < 0
						ifTrue: [interpreterProxy classLargeNegativeInteger]
						ifFalse: [interpreterProxy classLargePositiveInteger]]
				ifFalse: [interpreterProxy fetchClassOf: anInteger].
	(highBit := self cDigitHighBit: pDigits len: byteLen + 3 // 4) = 0
		ifTrue: [^ interpreterProxy instantiateClass: largeClass indexableSize: 1].
	newByteLen := highBit + shiftCount + 7 // 8.
	self remapOop: anInteger in:
		[newOop := interpreterProxy instantiateClass: largeClass indexableSize: newByteLen].
	newOop ifNil:
		[interpreterProxy primitiveFailFor: PrimErrNoMemory.
		 ^nil].
	newDigitLen := newByteLen + 3 // 4.
	self
		cDigitLshift: shiftCount
		from: (pDigits == pSmall ifTrue: [pSmall] ifFalse: [self pointerToFirstDigitOfLargeInt: anInteger])
		len: highBit + 31 // 32 "only the significant digits: the leading zero digits of an unnormalized anInteger would be written past newOop"
		to: (self pointerToFirstDigitOfLargeInt: newOop)
		len: newDigitLen.
	^ newOop
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digit: anInteger digits: pDigits small: pSmall len: byteLen Rshift: shiftCount [ 
	"Answer the normalized Integer with the sign of anInteger and its magnitude, the byteLen bytes at pDigits, shifted right by shiftCount bits. See #pointerToDigitsOf:small:byteSizeInto:."
	| neg oldBitLen newBitLen newDigitLen newOop resBuffer |
	<var: #pDigits type: #'unsigned int *'>
	<var: #newOop type: #sqInt>
	<var: #pSmall type: #'unsigned int *'>
	<var: #resBuffer declareC: 'unsigned int resBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk: [resBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	"pDigits == pSmall tells a SmallInteger anInteger without another #isIntegerObject: call"
	neg := pDigits == pSmall
			ifTrue: [(interpreterProxy integerValueOf: anInteger) < 0]
			ifFalse: [interpreterProxy isLargeNegativeIntegerObject: anInteger].
	oldBitLen := self cDigitHighBit: pDigits len: byteLen + 3 // 4.
	newBitLen := oldBitLen - shiftCount.
	newBitLen <= 0 ifTrue: ["All bits lost"
		^ 0 asOop: SmallInteger].
	newDigitLen := newBitLen + 31 // 32.
	newDigitLen <= LIStackDigits ifTrue:
		["computed on the stack and allocated once, normalized"
		 self
			cDigitRshift: shiftCount
			from: pDigits
			len: oldBitLen + 31 // 32
			to: resBuffer
			len: newDigitLen.
		 ^ self normalizedIntegerFrom: resBuffer len: newDigitLen negative: neg].
	"too long for a SmallInteger, and newOop has no leading zero byte: it is normalized"
	self remapOop: anInteger in:
		[newOop := self createLargeIntegerNeg: neg byteLength: newBitLen + 7 // 8].
	newOop ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
	self
		cDigitRshift: shiftCount
		from: (pDigits == pSmall ifTrue: [pSmall] ifFalse: [self pointerToFirstDigitOfLargeInt: anInteger])
		len: oldBitLen + 31 // 32
		to: (self pointerToFirstDigitOfLargeInt: newOop)
		len: newDigitLen.
	^ newOop
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitAddLarge: firstInteger with: secondInteger [ 
	"Answer the sum of the magnitudes of firstInteger and secondInteger, SmallIntegers or LargeIntegers, with the sign of firstInteger."
	"Normalizes, except when the sum carries out of the longer operand: then the answer is a LargeInteger of 4 * longDigitLen + 1 bytes, even when it would fit a SmallInteger."
	| over firstLen secondLen firstDigitLen secondDigitLen pFirst pSecond shortDigitLen pShort longDigitLen pLong neg pSum sum newSum firstSmall secondSmall sumBuffer |
	<var: #over type: #'unsigned int'>
	<var: #sum type: #sqInt>
	<var: #newSum type: #sqInt>
	<var: #pFirst type: #'unsigned int *'>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pShort type: #'unsigned int *'>
	<var: #pLong type: #'unsigned int *'>
	<var: #pSum type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #sumBuffer declareC: 'unsigned int sumBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 sumBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	firstDigitLen := firstLen + 3 // 4.
	secondDigitLen := secondLen + 3 // 4.
	neg := self isNegativeInteger: firstInteger.
	longDigitLen := firstDigitLen max: secondDigitLen.
	"	sum := Integer new: len neg: firstInteger negative.
	 A sum that fits LIStackDigits digits is computed on the stack and allocated once, normalized."
	longDigitLen <= LIStackDigits
		ifTrue:
			[sum := nil.
			 pSum := sumBuffer]
		ifFalse:
			[self remapOop: #(firstInteger secondInteger) in: [sum := self createLargeIntegerNeg: neg digitLength: longDigitLen].
			 sum ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
			 pSum := self pointerToFirstDigitOfLargeInt: sum.
			 pFirst := self pointerToDigitsOf: firstInteger small: firstSmall.
			 pSecond := self pointerToDigitsOf: secondInteger small: secondSmall].
	firstDigitLen <= secondDigitLen
		ifTrue: 
			[pShort := pFirst.
			shortDigitLen := firstDigitLen.
			pLong := pSecond]
		ifFalse: 
			[pShort := pSecond.
			shortDigitLen := secondDigitLen.
			pLong := pFirst].
	over := self
				cDigitAdd: pShort
				len: shortDigitLen
				with: pLong
				len: longDigitLen
				into: pSum.
	over > 0 ifFalse:
		[^ self normalized: sum digits: pSum len: longDigitLen negative: neg].
	"sum := sum growby: 1."
	self remapOop: sum in: [newSum := self createLargeIntegerNeg: neg byteLength: longDigitLen * 4 + 1].
	newSum ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
	sum ifNotNil: [pSum := self pointerToFirstDigitOfLargeInt: sum].
	self
		cDigitCopyFrom: pSum
		to: (self pointerToFirstDigitOfLargeInt: newSum)
		len: longDigitLen.
	"C index!"
	self cDigitOf: (self pointerToFirstDigitOfLargeInt: newSum)
		at: longDigitLen put: over.
	^ newSum
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitBitLogic: firstInteger with: secondInteger opIndex: opIx [ 
	"Bit logic here is only implemented for positive integers or Zero;
	if rec or arg is negative, it fails."
	| firstLen secondLen pFirst pSecond shortLen pShort longLen pLong pRes result firstSmall secondSmall resBuffer |
	<var: #pFirst type: #'unsigned int *'>
	<var: #result type: #sqInt>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pShort type: #'unsigned int *'>
	<var: #pLong type: #'unsigned int *'>
	<var: #pRes type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #resBuffer declareC: 'unsigned int resBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 resBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	"a SmallInteger operand is not converted to a scratch LargeInteger"
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	"pFirst == firstSmall tells a SmallInteger firstInteger without another #isIntegerObject: call.
	 Every failure here is the generic one, so checking after reading the digits answers the same."
	pFirst == firstSmall
		ifTrue: 
			[(interpreterProxy integerValueOf: firstInteger) < 0 ifTrue:
				[^ interpreterProxy primitiveFail]]
		ifFalse: 
			[(interpreterProxy isLargePositiveIntegerObject: firstInteger) ifFalse: [^ interpreterProxy primitiveFail]].
	pSecond == secondSmall
		ifTrue: 
			[(interpreterProxy integerValueOf: secondInteger) < 0 ifTrue:
				[^ interpreterProxy primitiveFail]]
		ifFalse: 
			[(interpreterProxy isLargePositiveIntegerObject: secondInteger) ifFalse: [^ interpreterProxy primitiveFail]].
	longLen := firstLen max: secondLen.
	"a result that fits LIStackDigits digits is computed on the stack and allocated once, normalized"
	longLen + 3 // 4 <= LIStackDigits
		ifTrue:
			[result := nil.
			 pRes := resBuffer]
		ifFalse:
			[self remapOop: #(firstInteger secondInteger) in:
				[result := interpreterProxy instantiateClass: interpreterProxy classLargePositiveInteger indexableSize: longLen].
			 result ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
			 pRes := self pointerToFirstDigitOfLargeInt: result.
			 pFirst := self pointerToDigitsOf: firstInteger small: firstSmall.
			 pSecond := self pointerToDigitsOf: secondInteger small: secondSmall].
	firstLen < secondLen
		ifTrue: 
			[shortLen := firstLen.
			pShort := pFirst.
			pLong := pSecond]
		ifFalse: 
			[shortLen := secondLen.
			pShort := pSecond.
			pLong := pFirst].
	self
		cDigitOp: opIx
		short: pShort
		len: shortLen + 3 // 4
		long: pLong
		len: longLen + 3 // 4
		into: pRes.
	interpreterProxy failed ifTrue: [^ 0].
	^ self normalized: result digits: pRes len: longLen + 3 // 4 negative: false
]

{ #category : 'C core util' }
LargeIntegersPlugin >> digitBuffer: digitLen stack: stackBuffer [
	"Answer room for digitLen 32-bit digits: stackBuffer, an array of LIStackDigits digits in the caller's frame, when they fit, else malloc'ed memory, or nil when there is none. Release it with #freeDigitBuffer:stack:.
	 A division shifts its operands into such buffers, not into scratch LargeIntegers."
	<inline: #always>
	<returnTypeC: #'unsigned int *'>
	<var: #stackBuffer type: #'unsigned int *'>
	^ digitLen <= LIStackDigits
		ifTrue: [stackBuffer]
		ifFalse:
			[self
				cCode: [self cCoerceSimple: (self malloc: digitLen * 4) to: #'unsigned int *']
				inSmalltalk: [CArrayAccessor on: (WordArray new: digitLen)]]
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitCompareLarge: firstInteger with: secondInteger [ 
	"Compare the magnitude of firstInteger with that of secondInteger.      
	Return a code of 1, 0, -1 for firstInteger >, = , < secondInteger"
	| firstDigitLen secondDigitLen |
	firstDigitLen := self digitSizeOfLargeInt: firstInteger.
	secondDigitLen := self digitSizeOfLargeInt: secondInteger.
	secondDigitLen ~= firstDigitLen
		ifTrue: [secondDigitLen > firstDigitLen
				ifTrue: [^ -1 asOop: SmallInteger]
				ifFalse: [^ 1 asOop: SmallInteger]].
	^ (self
		cDigitCompare: (self pointerToFirstDigitOfLargeInt: firstInteger)
		with: (self pointerToFirstDigitOfLargeInt: secondInteger)
		len: firstDigitLen)
		asOop: SmallInteger
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitDivLarge: firstInteger with: secondInteger negative: neg [ 
	"Answer {quotient. remainder} of the magnitudes of firstInteger and secondInteger, SmallIntegers or normalized LargeIntegers."
	"Does not normalize: the quotient is a LargeInteger of quoDigitLen 32-bit digits with the sign neg, the remainder a LargeInteger with the sign of firstInteger and no leading zero byte (no byte at all when it is zero).
	 When the dividend has fewer digits than the divisor, answer {0. firstInteger}, with a SmallInteger firstInteger converted to a LargeInteger."
	"Division by zero has to be checked in caller."
	| firstLen secondLen firstDigitLen secondDigitLen quoDigitLen d firstNeg firstLarge pFirst pSecond pDiv pRem quo rem result firstSmall secondSmall divBuffer remBuffer |
	<var: #pFirst type: #'unsigned int *'>
	<var: #firstLarge type: #sqInt>
	<var: #quo type: #sqInt>
	<var: #rem type: #sqInt>
	<var: #result type: #sqInt>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pDiv type: #'unsigned int *'>
	<var: #pRem type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #divBuffer declareC: 'unsigned int divBuffer[LIStackDigits]'>
	<var: #remBuffer declareC: 'unsigned int remBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 divBuffer := CArrayAccessor on: (WordArray new: LIStackDigits).
		 remBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	firstDigitLen := firstLen + 3 // 4.
	secondDigitLen := secondLen + 3 // 4.
	quoDigitLen := firstDigitLen - secondDigitLen + 1.
	quoDigitLen <= 0
		ifTrue: 
			[(interpreterProxy isIntegerObject: firstInteger)
				ifTrue:
					[firstLarge := self createLargeFromSmallInteger: firstInteger.
					 firstLarge ifNil: [^nil]]
				ifFalse: [firstLarge := firstInteger].
			 self remapOop: firstLarge in: [result := interpreterProxy instantiateClass: interpreterProxy classArray indexableSize: 2].
			 result ifNotNil:
				[interpreterProxy stObject: result at: 1 put: (0 asOop: SmallInteger).
				 interpreterProxy stObject: result at: 2 put: firstLarge].
			^ result].
	firstNeg := self isNegativeInteger: firstInteger.
	"set rem and div to copies of firstInteger and secondInteger, respectively. 
	 However,  to facilitate use of Knuth's algorithm, multiply rem and div by 2
	 (that is, shift) until the high word of div is >=16r80000000.
	 Both copies are C scratch memory with one more digit, not LargeIntegers."
	d := 32 - (self cHighBit32: (self cDigitOf: pSecond at: secondDigitLen - 1)).
	pDiv := self digitBuffer: secondDigitLen + 1 stack: divBuffer.
	pRem := self digitBuffer: firstDigitLen + 1 stack: remBuffer.
	(pDiv isNil or: [pRem isNil]) ifTrue:
		[self freeDigitBuffer: pDiv stack: divBuffer.
		 self freeDigitBuffer: pRem stack: remBuffer.
		 ^interpreterProxy primitiveFailFor: PrimErrNoMemory].
	self cDigitLshift: d from: pSecond len: secondDigitLen into: pDiv.
	self cDigitLshift: d from: pFirst len: firstDigitLen into: pRem.
	rem := nil.
	quo := self createLargeIntegerNeg: neg digitLength: quoDigitLen.
	quo ifNotNil:
		[self
			cDigitDiv: pDiv
			len: secondDigitLen + 1
			rem: pRem
			len: firstDigitLen + 1
			quo: (self pointerToFirstDigitOfLargeInt: quo)
			len: quoDigitLen.
		"the remainder is in the low secondDigitLen digits of pRem, shifted left by d bits"
		self remapOop: quo
			in: [rem := self digits: pRem Rshift: d lookfirst: secondDigitLen negative: firstNeg]].
	self freeDigitBuffer: pDiv stack: divBuffer.
	self freeDigitBuffer: pRem stack: remBuffer.
	(quo isNil or: [rem isNil]) ifTrue: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
	"^ Array with: quo with: rem"
	self remapOop: #(quo rem)
		in: [result := interpreterProxy instantiateClass: interpreterProxy classArray indexableSize: 2].
	result ifNotNil:
		[interpreterProxy stObject: result at: 1 put: quo.
		 interpreterProxy stObject: result at: 2 put: rem].
	^result
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitMontgomery: firstInteger times: secondInteger modulo: thirdInteger mInvModB: mInv [
	"The operands are SmallIntegers or LargeIntegers. Normalizes."
	| firstLen secondLen thirdLen firstDigitLen secondDigitLen thirdDigitLen pFirst pSecond pThird pRes prod firstSmall secondSmall thirdSmall resBuffer |
	<var: #mInv type: #'unsigned int'>
	<var: #prod type: #sqInt>
	<var: #pFirst type: #'unsigned int *'>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pThird type: #'unsigned int *'>
	<var: #pRes type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #thirdSmall declareC: 'unsigned int thirdSmall[2]'>
	<var: #resBuffer declareC: 'unsigned int resBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 thirdSmall := CArrayAccessor on: (WordArray new: 2).
		 resBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	pThird := self pointerToDigitsOf: thirdInteger small: thirdSmall byteSizeInto: (self addressOf: thirdLen put: [:v | thirdLen := v]).
	firstDigitLen := firstLen + 3 // 4.
	secondDigitLen := secondLen + 3 // 4.
	thirdDigitLen := thirdLen + 3 // 4.
	(firstDigitLen <= thirdDigitLen and: [secondDigitLen <= thirdDigitLen]) ifFalse: [^interpreterProxy primitiveFail].
	"the multiplication loop reads the first digit of the second operand"
	(firstDigitLen > 0 and: [secondDigitLen = 0]) ifTrue: [^interpreterProxy primitiveFail].
	"a product that fits LIStackDigits digits is computed on the stack and allocated once, normalized"
	thirdDigitLen <= LIStackDigits
		ifTrue:
			[prod := nil.
			 pRes := resBuffer.
			 self cDigitZero: pRes len: thirdDigitLen]
		ifFalse:
			[self remapOop: #(firstInteger secondInteger thirdInteger)
				in: [prod := interpreterProxy instantiateClass: interpreterProxy classLargePositiveInteger indexableSize: thirdDigitLen * 4].
			 prod ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
			 pRes := self pointerToFirstDigitOfLargeInt: prod.
			 pFirst := self pointerToDigitsOf: firstInteger small: firstSmall.
			 pSecond := self pointerToDigitsOf: secondInteger small: secondSmall.
			 pThird := self pointerToDigitsOf: thirdInteger small: thirdSmall].
	self
		cDigitMontgomery: pFirst
		len: firstDigitLen
		times: pSecond
		len: secondDigitLen
		modulo: pThird
		len: thirdDigitLen
		mInvModB: mInv
		into: pRes.
	^ self normalized: prod digits: pRes len: thirdDigitLen negative: false
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitMultiplyLarge: firstInteger with: secondInteger negative: neg [ 
	"firstInteger and secondInteger are SmallIntegers or LargeIntegers. Normalizes."
	| firstLen secondLen pFirst pSecond pShort shortLen pLong longLen resDigitLen pProd prod firstSmall secondSmall prodBuffer |
	<var: #pFirst type: #'unsigned int *'>
	<var: #prod type: #sqInt>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pShort type: #'unsigned int *'>
	<var: #pLong type: #'unsigned int *'>
	<var: #pProd type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #prodBuffer declareC: 'unsigned int prodBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 prodBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	resDigitLen := firstLen + secondLen + 3 // 4.
	"a product that fits LIStackDigits digits is computed on the stack and allocated once, normalized"
	resDigitLen <= LIStackDigits
		ifTrue:
			["prod starts out all zero"
			 prod := nil.
			 pProd := prodBuffer.
			 self cDigitZero: pProd len: resDigitLen]
		ifFalse:
			[self remapOop: #(firstInteger secondInteger) in: [prod := self createLargeIntegerNeg: neg byteLength: firstLen + secondLen].
			 prod ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
			 pProd := self pointerToFirstDigitOfLargeInt: prod.
			 pFirst := self pointerToDigitsOf: firstInteger small: firstSmall.
			 pSecond := self pointerToDigitsOf: secondInteger small: secondSmall].
	firstLen <= secondLen
		ifTrue: 
			[pShort := pFirst.
			shortLen := firstLen.
			pLong := pSecond.
			longLen := secondLen]
		ifFalse: 
			[pShort := pSecond.
			shortLen := secondLen.
			pLong := pFirst.
			longLen := firstLen].
	self
		cDigitMultiply: pShort
		len: shortLen + 3 // 4
		with: pLong
		len: longLen + 3 // 4
		into: pProd
		len: resDigitLen.
	^ self normalized: prod digits: pProd len: resDigitLen negative: neg
]

{ #category : 'util' }
LargeIntegersPlugin >> digitOfCSI: csi at: ix [ 
	"Answer the value of a 32 bits digit in a C-SmallInteger."
	"ST indexed!"
	^(csi < 0
		ifTrue:  [0 - csi]
		ifFalse: [csi]) >> (ix - 1 * 32)
]

{ #category : 'util' }
LargeIntegersPlugin >> digitOfLargeInt: aBytesOop at: ix [ 
	"Argument has to be aLargeInteger!"
	ix > (self digitSizeOfLargeInt: aBytesOop)
		ifTrue: [^ 0]
		ifFalse: [^ self unsafeDigitOfLargeInt: aBytesOop at: ix]
]

{ #category : 'util' }
LargeIntegersPlugin >> digitSizeOfCSI: csi [ 
	"Answer the number of 32-bits fields of a C-SmallInteger. This value is 
	   the same as the largest legal subscript."
	^(interpreterProxy maxSmallInteger <= 16r3FFFFFFF)
		ifTrue: [1]
		ifFalse: [csi > 16rFFFFFFFF asIntegerPtr "conversion is not really needed here, but avoid generating a warning in 32bits, and harmless in 64bits"
			ifTrue: [2]
			ifFalse: [csi < -16rFFFFFFFF
				ifTrue: [2]
				ifFalse: [1]]]
]

{ #category : 'util' }
LargeIntegersPlugin >> digitSizeOfLargeInt: anOop [
	"answer number of 32 bits digits of a Large Integer"
	<inline: true>
	^(self byteSizeOfLargeInt: anOop) + 3 // 4
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digitSubLarge: firstInteger with: secondInteger [ 
	"firstInteger and secondInteger are SmallIntegers or LargeIntegers. Normalizes."
	| firstLen secondLen firstDigitLen secondDigitLen pFirst pSecond firstSmaller largeDigitLen smallerDigitLen neg firstNeg pRes res firstSmall secondSmall resBuffer |
	<var: #pFirst type: #'unsigned int *'>
	<var: #res type: #sqInt>
	<var: #pSecond type: #'unsigned int *'>
	<var: #pRes type: #'unsigned int *'>
	<var: #firstSmall declareC: 'unsigned int firstSmall[2]'>
	<var: #secondSmall declareC: 'unsigned int secondSmall[2]'>
	<var: #resBuffer declareC: 'unsigned int resBuffer[LIStackDigits]'>
	self cCode: '' inSmalltalk:
		[firstSmall := CArrayAccessor on: (WordArray new: 2).
		 secondSmall := CArrayAccessor on: (WordArray new: 2).
		 resBuffer := CArrayAccessor on: (WordArray new: LIStackDigits)].
	pFirst := self pointerToDigitsOf: firstInteger small: firstSmall byteSizeInto: (self addressOf: firstLen put: [:v | firstLen := v]).
	pSecond := self pointerToDigitsOf: secondInteger small: secondSmall byteSizeInto: (self addressOf: secondLen put: [:v | secondLen := v]).
	firstNeg := self isNegativeInteger: firstInteger.
	firstDigitLen := firstLen + 3 // 4.
	secondDigitLen := secondLen + 3 // 4.
	firstDigitLen = secondDigitLen ifTrue: 
		[[firstDigitLen > 1
		  and: [(self cDigitOf: pFirst at: firstDigitLen - 1) = (self cDigitOf: pSecond at: firstDigitLen - 1)]]
			whileTrue: [firstDigitLen := firstDigitLen - 1].
		secondDigitLen := firstDigitLen].
	firstSmaller := firstDigitLen < secondDigitLen
	 or: [firstDigitLen = secondDigitLen
		 and: [firstDigitLen > 0 "two empty magnitudes have no top digit to compare"
		 and: [(self cDigitOf: pFirst at: firstDigitLen - 1) < (self cDigitOf: pSecond at: firstDigitLen - 1)]]].
	firstSmaller
		ifTrue: 
			[largeDigitLen := secondDigitLen.
			smallerDigitLen := firstDigitLen.
			neg := firstNeg == false]
		ifFalse: 
			[largeDigitLen := firstDigitLen.
			smallerDigitLen := secondDigitLen.
			neg := firstNeg].
	"a difference that fits LIStackDigits digits is computed on the stack and allocated once, normalized"
	largeDigitLen <= LIStackDigits
		ifTrue:
			[res := nil.
			 pRes := resBuffer]
		ifFalse:
			[self remapOop: #(firstInteger secondInteger)
				in: [res := self createLargeIntegerNeg: neg digitLength: largeDigitLen].
			 res ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
			 pRes := self pointerToFirstDigitOfLargeInt: res.
			 pFirst := self pointerToDigitsOf: firstInteger small: firstSmall.
			 pSecond := self pointerToDigitsOf: secondInteger small: secondSmall].
	firstSmaller
		ifTrue:
			[self
				cDigitSub: pFirst
				len: smallerDigitLen
				with: pSecond
				len: largeDigitLen
				into: pRes]
		ifFalse:
			[self
				cDigitSub: pSecond
				len: smallerDigitLen
				with: pFirst
				len: largeDigitLen
				into: pRes].
	^ self normalized: res digits: pRes len: largeDigitLen negative: neg
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> digits: pFrom Rshift: shiftCount lookfirst: a negative: neg [ 
	"Answer a new LargeInteger with the sign neg and the magnitude of the first a 32-bit digits at pFrom shifted right by shiftCount bits."
	"Does not normalize, but the answer has no leading zero byte: it has no byte at all when all bits are lost."
	| newByteLen newOop oldBitLen newBitLen oldDigitLen newDigitLen |
	<var: #pFrom type: #'unsigned int *'>
	oldBitLen := self cDigitHighBit: pFrom len: a.
	oldDigitLen := oldBitLen + 31 // 32.
	newBitLen := oldBitLen - shiftCount.
	newBitLen <= 0 ifTrue: ["All bits lost"
		^ self createLargeIntegerNeg: neg byteLength: 0].
	newByteLen := newBitLen + 7 // 8.
	newDigitLen := newByteLen + 3 // 4.
	newOop := self createLargeIntegerNeg: neg byteLength: newByteLen.
	newOop ifNil: [^nil].
	self
		cDigitRshift: shiftCount
		from: pFrom
		len: oldDigitLen
		to: (self pointerToFirstDigitOfLargeInt: newOop)
		len: newDigitLen.
	^ newOop
]

{ #category : 'C core util' }
LargeIntegersPlugin >> freeDigitBuffer: pDigits stack: stackBuffer [
	"Release a buffer that #digitBuffer:stack: answered."
	<inline: #always>
	<var: #pDigits type: #'unsigned int *'>
	<var: #stackBuffer type: #'unsigned int *'>
	pDigits ~= stackBuffer ifTrue:
		[self cCode: [self free: pDigits] inSmalltalk: [nil]]
]

{ #category : 'ST initialize' }
LargeIntegersPlugin >> initialize [
	"Initializes ST constants; C's are set by class>>declareCVarsIn:."
	<returnTypeC: 'void'>
	self cCode: '"nothing to do here"'
		inSmalltalk: 
			[andOpIndex := 0.
			orOpIndex := 1.
			xorOpIndex := 2]
]

{ #category : 'util' }
LargeIntegersPlugin >> isNegativeInteger: anInteger [
	"Answer whether anInteger, a SmallInteger or a LargeInteger, is negative, i.e. whether it is or converts to a LargeNegativeInteger."
	<inline: #always>
	^ (interpreterProxy isIntegerObject: anInteger)
		ifTrue: [(interpreterProxy integerValueOf: anInteger) < 0]
		ifFalse: [interpreterProxy isLargeNegativeIntegerObject: anInteger]
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> isNormalized: aLargeInteger [ 
	| len |
	"Check for leading zero of LargeInteger"
	len := self byteSizeOfLargeInt: aLargeInteger.
	len = 0 ifTrue:
		[^ false].
	(self unsafeByteOfLargeInt: aLargeInteger at: len) = 0 ifTrue:
		[^ false].
	^true
]

{ #category : 'oop util' }
LargeIntegersPlugin >> largeInt: aBytesObject growTo: newByteLen [ 
	"Attention: this method invalidates all oop's! Only newBytes is valid at return."
	"Does not normalize."
	| newBytes oldDigitLen newDigitLen copyLen |
	self remapOop: aBytesObject in:
		[newBytes := interpreterProxy
						instantiateClass: (interpreterProxy fetchClassOf: aBytesObject)
						indexableSize: newByteLen.
		 newBytes ifNil:
			[interpreterProxy primitiveFailFor: PrimErrNoMemory. ^nil]].
	newDigitLen := newByteLen + 3 // 4.
	oldDigitLen := self digitSizeOfLargeInt: aBytesObject.
	copyLen := oldDigitLen < newDigitLen
					ifTrue: [oldDigitLen]
					ifFalse: [newDigitLen].
	self
		cDigitCopyFrom: (self pointerToFirstDigitOfLargeInt: aBytesObject)
		to: (self pointerToFirstDigitOfLargeInt: newBytes)
		len: copyLen.
	^newBytes
]

{ #category : 'C core' }
LargeIntegersPlugin >> liDigitDiv: pDiv len: divLen rem: pRem len: remLen quo: pQuo len: quoLen [
	"Knuth's algorithm D over 32-bit digits, which the C kernel liDigitDiv() of LargeIntegersKernels.c replaces, run in simulation. The kernel tests compare the two digit for digit, quotient and all of pRem."
	| dl ql dh dnh j t hi lo r3 l a b cond q r1r2 mul |
	<doNotGenerate>
	dl := divLen - 1.
	"Last actual byte of data (ST ix)"
	ql := quoLen.
	dh := self cDigitOf: pDiv at: dl - 1.
	dl = 1
		ifTrue: [dnh := 0]
		ifFalse: [dnh := self cDigitOf: pDiv at: dl - 2].
	1 to: ql do: 
		[:k | 
		"maintain quo*arg+rem=self"
		"Estimate rem/div by dividing the leading two unint32 of rem by dh."
		"The estimate is q = qhi*16r100000000+qlo, where qhi and qlo are uint32."
		j := remLen + 1 - k.
		"r1 := rem digitAt: j."
		(self cDigitOf: pRem at: j - 1)
			= dh
			ifTrue: [q := 16rFFFFFFFF]
			ifFalse: 
				["Compute q = (r1,r2)//dh, t = (r1,r2)\\dh.
				Note that r1,r2 are uint64, not uint32."
				"r2 := (rem digitAt: j - 2)."
				r1r2 := self cDigitOf: pRem at: j - 1.
				r1r2 := (r1r2 << 32) + (self cDigitOf: pRem at: j - 2).
				t := r1r2 \\ dh.
				q := r1r2 // dh.
				"Next compute (hi,lo) := q*dnh"
				mul := q * dnh.
				hi := mul >> 32.
				lo := mul bitAnd: 16rFFFFFFFF.
				"Correct overestimate of q.                
				Max of 2 iterations through loop -- see Knuth vol. 2"
				j < 3
					ifTrue: [r3 := 0]
					ifFalse: [r3 := self cDigitOf: pRem at: j - 3].
				
				[(t < hi
					or: [t = hi and: [r3 < lo]])
					ifTrue: 
						["i.e. (t,r3) < (hi,lo)"
						q := q - 1.
						hi = 0 "since hi is unsigned we must have this guard"
							ifTrue: [cond := false]
							ifFalse:
								[lo < dnh
									ifTrue: 
										[hi := hi - 1.
										lo := lo + 16r100000000 - dnh]
									ifFalse:
										[lo := lo - dnh].
								cond := hi >= dh]]
					ifFalse: [cond := false].
				cond]
					whileTrue: [hi := hi - dh]].
		"Subtract q*div from rem"
		l := j - dl.
		a := 0.
		1 to: divLen do: 
			[:i | 
			hi := (self cDigitOf: pDiv at: i - 1) * (q >> 32).
			lo := (self cDigitOf: pDiv at: i - 1) * (q bitAnd: 16rFFFFFFFF).
			b := (self cDigitOf: pRem at: l - 1) - a - (lo bitAnd: 16rFFFFFFFF).
			self cDigitOf: pRem at: l - 1 put: (b bitAnd: 16rFFFFFFFF).
			"simulate arithmetic shift (preserving sign of b)"
			b := b >> 32 bitOr: (0 - (b >> 63) bitAnd: 16rFFFFFFFF00000000).
			a := hi + (lo >> 32) - b.
			l := l + 1].
		a > 0
			ifTrue: 
				["Add div back into rem, decrease q by 1"
				q := q - 1.
				l := j - dl.
				a := 0.
				1 to: divLen do: 
					[:i | 
					a := (a >> 32)
								+ (self cDigitOf: pRem at: l - 1) + (self cDigitOf: pDiv at: i - 1).
					self cDigitOf: pRem at: l - 1 put: (a bitAnd: 16rFFFFFFFF).
					l := l + 1]].
		self cDigitOf: pQuo at: quoLen - k put: (self cCoerceSimple: q to: #'unsigned int')].
	^0
]

{ #category : 'C core' }
LargeIntegersPlugin >> liDigitMultiply: pWordShort len: shortLen with: pWordLong len: longLen into: pWordRes len: resLen [
	"The 32-bit schoolbook loop that the C kernel liDigitMultiply() of LargeIntegersKernels.c replaces, run in simulation. It adds the product into the resLen digits at pWordRes, which start out all zero; the kernel stores the same digits whatever they held. The kernel tests compare the two digit for digit."
	| limitLong digit k carry limitShort ab |
	<doNotGenerate>
	(shortLen = 1 and: [(pWordShort at: 0) = 0])
		ifTrue: [^ 0].
	(longLen = 1 and: [(pWordLong at: 0) = 0])
		ifTrue: [^ 0].
	"prod starts out all zero"
	limitShort := shortLen - 1.
	limitLong := longLen - 1.
	0 to: limitShort do: [:i | (digit := self cDigitOf: pWordShort at: i) ~= 0
			ifTrue: 
				[k := i.
				carry := 0.
				"Loop invariant: 0<=carry<=16rFFFFFFFF, k=i+j-1 (ST)"
				"-> Loop invariant: 0<=carry<=16rFFFFFFFF, k=i+j (C) (?)"
				0 to: limitLong do: 
					[:j | 
					ab := (self cDigitOf: pWordLong at: j).
					ab := ab * digit + carry + (self cDigitOf: pWordRes at: k).
					carry := ab >> 32.
					self cDigitOf: pWordRes at: k put: (ab bitAnd: 16rFFFFFFFF).
					k := k + 1].
				k < resLen ifTrue: [self cDigitOf: pWordRes at: k put: carry]]].
	^ 0
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> normalizeNegative: aLargeNegativeInteger [ 
	"Check for leading zeroes and return shortened copy if so."
	"First establish len = significant length."
	| val val2 sLen digitLen byteLen oldByteLen minVal |
	<var: #val type: #usqInt>
	<var: #val2 type: #usqInt>
	<var: #minVal type: #usqInt>
	digitLen := self digitSizeOfLargeInt: aLargeNegativeInteger.
	[digitLen ~= 0 and: [(self unsafeDigitOfLargeInt: aLargeNegativeInteger at: digitLen) = 0]]
		whileTrue: [digitLen := digitLen - 1].
	digitLen = 0 ifTrue: [^ 0 asOop: SmallInteger].
	"Now check if in SmallInteger range"
	val := self unsafeDigitOfLargeInt: aLargeNegativeInteger at: digitLen.
	sLen := interpreterProxy minSmallInteger < -16r40000000
				ifTrue: [2]
				ifFalse: [1]. "SmallInteger minVal digitLength"
	digitLen <= sLen
		ifTrue: 
			[minVal := 0 - interpreterProxy minSmallInteger.
			val2 := val.
			digitLen > 1 ifTrue: [val2 := val2 << 32 + (self unsafeDigitOfLargeInt: aLargeNegativeInteger at: 1)].
			val2 <= minVal
				ifTrue: [^0 -  val2 asOop: SmallInteger]].
	"Return self, or a shortened copy"
	byteLen := digitLen * 4.
	val <= 16rFFFF
		ifTrue: [byteLen := byteLen - 2]
		ifFalse: [val := val >> 16].
	val <= 16rFF
		ifTrue: [byteLen := byteLen - 1].
	oldByteLen := self byteSizeOfLargeInt: aLargeNegativeInteger.
	byteLen < oldByteLen
		ifTrue: [^ self largeInt: aLargeNegativeInteger growTo: byteLen]
		ifFalse: [^ aLargeNegativeInteger]
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> normalizePositive: aLargePositiveInteger [ 
	"Check for leading zeroes and return shortened copy if so."
	"First establish len = significant length."
	| val val2 sLen digitLen byteLen oldByteLen maxVal |
	<var: #val type: #usqInt>
	<var: #val2 type: #usqInt>
	<var: #maxVal type: #usqInt>
	digitLen := self digitSizeOfLargeInt: aLargePositiveInteger.
	[digitLen ~= 0 and: [(self unsafeDigitOfLargeInt: aLargePositiveInteger at: digitLen) = 0]]
		whileTrue: [digitLen := digitLen - 1].
	digitLen = 0 ifTrue: [^ 0 asOop: SmallInteger].
	"Now check if in SmallInteger range"
	val := self unsafeDigitOfLargeInt: aLargePositiveInteger at: digitLen.
	sLen := interpreterProxy maxSmallInteger > 16r3FFFFFFF
				ifTrue: [2]
				ifFalse: [1]. "SmallInteger maxVal digitLength"
	digitLen <= sLen
		ifTrue: 
			[maxVal := interpreterProxy maxSmallInteger.
			val2 := val.
			digitLen > 1 ifTrue:
				["Note: asUnsignedLongLong is not necessary because this branch is for 64 bits only.
				but we want to avoid a C Compiler warning on 32 bits"
				val2 := val2 asUnsignedLongLong << 32 + (self unsafeDigitOfLargeInt: aLargePositiveInteger at: 1)].
			val2 <= maxVal
				ifTrue: [^val2 asOop: SmallInteger]].
	"Return self, or a shortened copy"
	byteLen := digitLen * 4.
	val <= 16rFFFF
		ifTrue: [byteLen := byteLen - 2]
		ifFalse: [val := val >> 16].
	val <= 16rFF
		ifTrue: [byteLen := byteLen - 1].
	oldByteLen := self byteSizeOfLargeInt: aLargePositiveInteger.
	byteLen < oldByteLen
		ifTrue: [^ self largeInt: aLargePositiveInteger growTo: byteLen]
		ifFalse: [^ aLargePositiveInteger]
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> normalized: result digits: pDigits len: digitLen negative: neg [
	"Answer the normalized Integer whose digitLen 32-bit digits are at pDigits.
	 Either they are those of result, a new LargeInteger with the sign neg, normalized as the plugin always did, or result is nil and they are on the stack: the answer is then allocated once, at its normalized size, or not at all when it fits a SmallInteger."
	<inline: true>
	<var: #pDigits type: #'unsigned int *'>
	<var: #result type: #sqInt>
	result ifNil: [^ self normalizedIntegerFrom: pDigits len: digitLen negative: neg].
	^ neg
		ifTrue: [self normalizeNegative: result]
		ifFalse: [self normalizePositive: result]
]

{ #category : 'oop functions' }
LargeIntegersPlugin >> normalizedIntegerFrom: pDigits len: len negative: neg [ 
	"Answer the normalized Integer with the sign neg and the magnitude of the len 32-bit digits at pDigits: a SmallInteger if it fits, else a new LargeInteger of the significant bytes only.
	 That is what #normalizePositive: and #normalizeNegative: answer for a new LargeInteger holding these digits, without allocating that LargeInteger first."
	| val val2 sLen digitLen byteLen limit result |
	<var: #pDigits type: #'unsigned int *'>
	<var: #val type: #usqInt>
	<var: #val2 type: #usqInt>
	<var: #limit type: #usqInt>
	digitLen := len.
	[digitLen ~= 0 and: [(self cDigitOf: pDigits at: digitLen - 1) = 0]]
		whileTrue: [digitLen := digitLen - 1].
	digitLen = 0 ifTrue: [^ 0 asOop: SmallInteger].
	"Now check if in SmallInteger range"
	val := self cDigitOf: pDigits at: digitLen - 1.
	sLen := interpreterProxy maxSmallInteger > 16r3FFFFFFF
				ifTrue: [2]
				ifFalse: [1]. "SmallInteger maxVal digitLength"
	digitLen <= sLen
		ifTrue: 
			[val2 := val.
			digitLen > 1 ifTrue:
				["Note: asUnsignedLongLong is not necessary because this branch is for 64 bits only.
				but we want to avoid a C Compiler warning on 32 bits"
				val2 := val2 asUnsignedLongLong << 32 + (self cDigitOf: pDigits at: 0)].
			neg
				ifTrue:
					[limit := 0 - interpreterProxy minSmallInteger.
					 val2 <= limit
						ifTrue: [^0 -  val2 asOop: SmallInteger]]
				ifFalse:
					[limit := interpreterProxy maxSmallInteger.
					 val2 <= limit
						ifTrue: [^val2 asOop: SmallInteger]]].
	"Allocate the significant bytes only"
	byteLen := digitLen * 4.
	val <= 16rFFFF
		ifTrue: [byteLen := byteLen - 2]
		ifFalse: [val := val >> 16].
	val <= 16rFF
		ifTrue: [byteLen := byteLen - 1].
	result := self createLargeIntegerNeg: neg byteLength: byteLen.
	result ifNil:
		[interpreterProxy primitiveFailFor: PrimErrNoMemory.
		 ^nil].
	self
		cDigitCopyFrom: pDigits
		to: (self pointerToFirstDigitOfLargeInt: result)
		len: digitLen.
	^ result
]

{ #category : 'util' }
LargeIntegersPlugin >> pointerToDigitsOf: anInteger small: pSmall [
	"Answer a pointer to the 32-bit digits of anInteger: those of its object for a LargeInteger, pSmall, which #pointerToDigitsOf:small:byteSizeInto: has filled, for a SmallInteger.
	 Used to fetch the pointer again after an allocation, see #pointerToDigitsOf:small:byteSizeInto:."
	<inline: #always>
	<returnTypeC: #'unsigned int *'>
	<var: #pSmall type: #'unsigned int *'>
	^ (interpreterProxy isIntegerObject: anInteger)
		ifTrue: [pSmall]
		ifFalse: [self pointerToFirstDigitOfLargeInt: anInteger]
]

{ #category : 'util' }
LargeIntegersPlugin >> pointerToDigitsOf: anInteger small: pSmall byteSizeInto: pByteSize [
	"Answer a pointer to the 32-bit digits of anInteger, a SmallInteger or a LargeInteger, and store its byte size at pByteSize.
	 A SmallInteger is not converted to a scratch LargeInteger: its magnitude is stored into the two 32-bit digits at pSmall, as #createLargeFromSmallInteger: would store it into that LargeInteger, the answer is pSmall and the byte size is that of that LargeInteger.
	 Spur's #instantiateClass:indexableSize: does not move objects: it never collects garbage synchronously (it schedules a scavenge and falls back to old space). So the pointer stays valid across the allocations of a primitive, and the C relies on it: Slang emits nothing for #remapOop:in:, see InterpreterPlugin>>#remapOop:in:. The #remapOop:in: blocks around allocations, and fetching the pointer again with #pointerToDigitsOf:small: after one, only state which oops and pointers are live across an allocation; they would not by themselves make the C safe with an object memory that moves objects when it allocates."
	| val mag |
	<returnTypeC: #'unsigned int *'>
	<var: #pSmall type: #'unsigned int *'>
	<var: #pByteSize type: #'sqInt *'>
	<var: #mag type: #'unsigned long long'>
	(interpreterProxy isIntegerObject: anInteger) ifFalse:
		[pByteSize at: 0 put: (self byteSizeOfLargeInt: anInteger).
		 ^ self pointerToFirstDigitOfLargeInt: anInteger].
	val := interpreterProxy integerValueOf: anInteger.
	mag := val < 0
			ifTrue: [0 - val]
			ifFalse: [val].
	self cDigitOf: pSmall at: 0 put: (mag bitAnd: 16rFFFFFFFF).
	self cDigitOf: pSmall at: 1 put: (mag >> 32 bitAnd: 16rFFFFFFFF).
	pByteSize at: 0 put: (self byteSizeOfCSI: val).
	^ pSmall
]

{ #category : 'util' }
LargeIntegersPlugin >> pointerToFirstDigitOfLargeInt: bytesObject [
	<inline: #always>
	<returnTypeC: #'unsigned int *'>
	^interpreterProxy cCoerce: (interpreterProxy firstIndexableField: bytesObject) to: #'unsigned int *'
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primAnyBitFrom: fromOop to: toOop [ 
	| integer small byteLen pDigits from to |
	<var: #small declareC: 'unsigned int small[2]'>
	<var: #pDigits type: #'unsigned int *'>
	<var: #from type: #sqInt>
	<var: #to type: #sqInt>
	interpreterProxy methodArgumentCount = 2 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	((interpreterProxy isIntegerObject: fromOop)
	 and: [interpreterProxy isIntegerObject: toOop]) ifFalse:
		[^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	from := interpreterProxy integerValueOf: fromOop.
	to := interpreterProxy integerValueOf: toOop.
	self debugCode: [self msg: 'primAnyBitFrom: from to: to'].
	integer := self
				primitive: 'primAnyBitFromTo'
				parameters: #(#Oop #Oop )
				receiver: #Integer.
	self cCode: '' inSmalltalk: [small := CArrayAccessor on: (WordArray new: 2)].
	"a SmallInteger is not converted to a scratch LargeInteger"
	pDigits := self pointerToDigitsOf: integer small: small byteSizeInto: (self addressOf: byteLen put: [:v | byteLen := v]).
	^ (self
		anyBitOfDigits: pDigits
		len: byteLen + 3 // 4
		from: from
		to: to)
		asOop: Boolean
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitAdd: secondInteger [
	| firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitAdd: secondInteger'].
	firstInteger := self
				primitive: 'primDigitAdd'
				parameters: #(#Oop )
				receiver: #Integer.
	"SmallIntegers are not converted to scratch LargeIntegers, see #pointerToDigitsOf:small:byteSizeInto:"
	^ self digitAddLarge: firstInteger with: secondInteger
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitBitAnd: secondInteger [ 
	"Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails."
	| firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitBitAnd: secondInteger'].
	firstInteger := self
				primitive: 'primDigitBitAnd'
				parameters: #(#Oop )
				receiver: #Integer.
	^ self
		digitBitLogic: firstInteger
		with: secondInteger
		opIndex: andOpIndex
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitBitOr: secondInteger [ 
	"Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails."
	| firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitBitOr: secondInteger'].
	firstInteger := self
				primitive: 'primDigitBitOr'
				parameters: #(#Oop )
				receiver: #Integer.
	^ self
		digitBitLogic: firstInteger
		with: secondInteger
		opIndex: orOpIndex
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitBitShiftMagnitude: shiftCountOop [ 
	| anInteger small byteLen pDigits shiftCount |
	<var: #small declareC: 'unsigned int small[2]'>
	<var: #pDigits type: #'unsigned int *'>
	<var: #shiftCount type: #sqInt>
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isIntegerObject: shiftCountOop) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	shiftCount := interpreterProxy integerValueOf: shiftCountOop.
	self debugCode: [self msg: 'primDigitBitShiftMagnitude: shiftCount'].
	anInteger := self
				primitive: 'primDigitBitShiftMagnitude'
				parameters: #(#Oop )
				receiver: #Integer.
	self cCode: '' inSmalltalk: [small := CArrayAccessor on: (WordArray new: 2)].
	"a SmallInteger is not converted to a scratch LargeInteger"
	pDigits := self pointerToDigitsOf: anInteger small: small byteSizeInto: (self addressOf: byteLen put: [:v | byteLen := v]).
	shiftCount >= 0
		ifTrue: [^ self digit: anInteger digits: pDigits small: small len: byteLen Lshift: shiftCount]
		ifFalse: [^ self digit: anInteger digits: pDigits small: small len: byteLen Rshift: 0 - shiftCount]
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitBitXor: secondInteger [ 
	"Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails."
	| firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitBitXor: secondInteger'].
	firstInteger := self
				primitive: 'primDigitBitXor'
				parameters: #(#Oop )
				receiver: #Integer.
	^ self
		digitBitLogic: firstInteger
		with: secondInteger
		opIndex: xorOpIndex
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitCompare: secondInteger [ 
	"Compare the magnitude of self with that of arg.   
	 Answer a code of 1, 0, -1 for self >, = , < arg"	| firstVal secondVal firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitCompare: secondInteger'].
	firstInteger := self
					primitive: 'primDigitCompare'
					parameters: #(#Oop )
					receiver: #Integer.
	"shortcut: aSmallInteger has to be smaller in Magnitude than aLargeInteger"
	(interpreterProxy isIntegerObject: firstInteger) ifTrue:
		[(interpreterProxy isIntegerObject: secondInteger) ifTrue:
			[firstVal := interpreterProxy integerValueOf: firstInteger.
			 secondVal := interpreterProxy integerValueOf: secondInteger.
			 "Compute their magnitudes.  Since SmallIntegers are tagged they have
			  fewer bits than an integer on the platform; therefore in computing their
			  magnitude they cannot overflow."
			 firstVal < 0 ifTrue: [firstVal := 0 - firstVal].
			 secondVal < 0 ifTrue: [secondVal := 0 - secondVal].
			 ^firstVal = secondVal
				ifTrue: [0 asOop: SmallInteger]
				ifFalse:
					[firstVal < secondVal
						ifTrue: [-1 asOop: SmallInteger]
						ifFalse: [1 asOop: SmallInteger]]].
			^-1 asOop: SmallInteger]. "first < second"
	(interpreterProxy isIntegerObject: secondInteger) ifTrue:
		[^1 asOop: SmallInteger]. "first > second"
	^ self digitCompareLarge: firstInteger with: secondInteger
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitDiv: secondInteger negative: negOop [ 
	"Answer the result of dividing firstInteger by secondInteger. 
	Fail if parameters are not integers, not normalized or secondInteger is 
	zero. "
	| firstInteger neg |
	<var: #neg type: #sqInt>
	interpreterProxy methodArgumentCount = 2 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	((interpreterProxy isKindOfInteger: secondInteger)
	 and: [interpreterProxy isBooleanObject: negOop]) ifFalse:
		[^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	neg := interpreterProxy booleanValueOf: negOop.
	self debugCode: [self msg: 'primDigitDiv: secondInteger negative: neg'].
	firstInteger := self
				primitive: 'primDigitDivNegative'
				parameters: #(#Oop #Oop )
				receiver: #Integer.
	"Check for zerodivide. SmallIntegers are not converted to scratch LargeIntegers, see #pointerToDigitsOf:small:byteSizeInto:"
	(interpreterProxy isIntegerObject: firstInteger)
		ifFalse:
			["Avoid crashes in case of getting unnormalized args."
			(self isNormalized: firstInteger)
				ifFalse:
					[self debugCode:
						[self msg: 'ERROR in primDigitDiv: secondInteger negative: neg'.
						self msg: '------> receiver *not* normalized!'].
					^ interpreterProxy primitiveFail]].
	(interpreterProxy isIntegerObject: secondInteger)
		ifTrue: ["check for zerodivide"
			(interpreterProxy integerValueOf: secondInteger)
					= 0
				ifTrue: [^ interpreterProxy primitiveFail]]
		ifFalse:
			["Avoid crashes in case of getting unnormalized args."
			(self isNormalized: secondInteger)
				ifFalse:
					[self debugCode:
						[self msg: 'ERROR in primDigitDiv: secondInteger negative: neg'.
						self msg: '------> argument *not* normalized!'].
					^ interpreterProxy primitiveFail]].
	^ self
		digitDivLarge: firstInteger
		with: secondInteger
		negative: neg
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitMultiply: secondInteger negative: negOop [ 
	| firstInteger neg |
	<var: #neg type: #sqInt>
	interpreterProxy methodArgumentCount = 2 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	((interpreterProxy isKindOfInteger: secondInteger)
	 and: [interpreterProxy isBooleanObject: negOop]) ifFalse:
		[^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	neg := interpreterProxy booleanValueOf: negOop.
	self debugCode: [self msg: 'primDigitMultiply: secondInteger negative: neg'].
	firstInteger := self
				primitive: 'primDigitMultiplyNegative'
				parameters: #(#Oop #Oop )
				receiver: #Integer.
	"SmallIntegers are not converted to scratch LargeIntegers, see #pointerToDigitsOf:small:byteSizeInto:"
	^ self
		digitMultiplyLarge: firstInteger
		with: secondInteger
		negative: neg
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primDigitSubtract: secondInteger [ 
	| firstInteger |
	interpreterProxy methodArgumentCount = 1 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	(interpreterProxy isKindOfInteger: secondInteger) ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'primDigitSubtract: secondInteger'].
	firstInteger := self
				primitive: 'primDigitSubtract'
				parameters: #(#Oop )
				receiver: #Integer.
	"SmallIntegers are not converted to scratch LargeIntegers, see #pointerToDigitsOf:small:byteSizeInto:"
	^ self digitSubLarge: firstInteger with: secondInteger
]

{ #category : 'control & support primitives' }
LargeIntegersPlugin >> primGetModuleName [
	"If calling this primitive fails, then C module does not exist."
	| strLen strOop |
	interpreterProxy methodArgumentCount = 0 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	self debugCode: [self msg: 'primGetModuleName'].
	self
		primitive: 'primGetModuleName'
		parameters: #()
		receiver: #Oop.
	strLen := self strlen: self getModuleName.
	strOop := interpreterProxy instantiateClass: interpreterProxy classString indexableSize: strLen.
	strOop ifNil: [^interpreterProxy primitiveFailFor: PrimErrNoMemory].
	self strncpy: (interpreterProxy firstIndexableField: strOop)
		_: self getModuleName
		_: strLen.
	^strOop
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primMontgomeryDigitLength [
	interpreterProxy methodArgumentCount = 0 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	self debugCode: [self msg: 'primMontgomeryDigitLength'].
	self
				primitive: 'primMontgomeryDigitLength'
				parameters: #()
				receiver: #Integer.
	^interpreterProxy integerObjectOf: 32
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primMontgomeryTimes: secondOperandInteger modulo: thirdModuloInteger mInvModB: mInverseInteger [
	| firstInteger mInv |
	<var: #mInv type: #'unsigned int'>
	interpreterProxy methodArgumentCount = 3 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	((interpreterProxy isKindOfInteger: secondOperandInteger)
	 and: [(interpreterProxy isKindOfInteger: thirdModuloInteger)
	 and: [interpreterProxy isKindOfInteger: mInverseInteger]]) ifFalse:
		[^interpreterProxy primitiveFailFor: PrimErrBadArgument].
	self debugCode: [self msg: 'montgomeryTimes: secondOperandInteger modulo: thirdModuloInteger mInvModB: smallInverseInteger'].
	firstInteger := self
				primitive: 'primMontgomeryTimesModulo'
				parameters: #(#Oop #Oop #Oop )
				receiver: #Integer.
	 mInv := interpreterProxy positive32BitValueOf: mInverseInteger.
	"SmallIntegers are not converted to scratch LargeIntegers, see #pointerToDigitsOf:small:byteSizeInto:"
	^ self digitMontgomery: firstInteger times: secondOperandInteger modulo: thirdModuloInteger mInvModB: mInv
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primNormalizeNegative [
	| rcvr |
	interpreterProxy methodArgumentCount = 0 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	self debugCode: [self msg: 'primNormalizeNegative'].
	rcvr := self
				primitive: 'primNormalizeNegative'
				parameters: #()
				receiver: #LargeNegativeInteger.
	^ self normalizeNegative: rcvr
]

{ #category : 'Integer primitives' }
LargeIntegersPlugin >> primNormalizePositive [
	| rcvr |
	interpreterProxy methodArgumentCount = 0 ifFalse: [^interpreterProxy primitiveFailFor: PrimErrBadNumArgs].
	self debugCode: [self msg: 'primNormalizePositive'].
	rcvr := self
				primitive: 'primNormalizePositive'
				parameters: #()
				receiver: #LargePositiveInteger.
	^ self normalizePositive: rcvr
]

{ #category : 'util' }
LargeIntegersPlugin >> unsafeByteOfLargeInt: bytesObj at: ix [
	"Argument bytesObj must not be aSmallInteger!"
	<inline: true>
	<returnTypeC: #'unsigned char'>
	^(interpreterProxy cCoerce: (interpreterProxy firstIndexableField: bytesObj) to: #'unsigned char *') at: ix - 1
]

{ #category : 'util' }
LargeIntegersPlugin >> unsafeDigitOfLargeInt: anOop at: ix [
	"Argument must not be aSmallInteger!"
	<inline: #always>
	<returnTypeC: #'unsigned int'>
	^self cDigitOf: (self pointerToFirstDigitOfLargeInt: anOop) at: ix - 1
]
