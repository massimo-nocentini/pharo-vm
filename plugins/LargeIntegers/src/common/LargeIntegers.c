/* Generated from
	smalltalksrc/VMMaker/LargeIntegersPlugin.class.st
   by the Pharo Slang SmartSyntaxPluginCodeGenerator, then adapted to the
   conventions of the output of
	SmartSyntaxPluginCodeGenerator VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4
   that this file was first generated with; some functions the class has not
   changed since keep that output's text. Keep it equivalent to the class when
   changing either: see the class comment (C source).
   The multiply, divide and Montgomery kernels are hand-written in
   LargeIntegersKernels.c.
 */
static char __buildInfo[] = "LargeIntegersPlugin (Pharo Slang, adapted to VMMaker.oscog-eem.2495 conventions)";



#include "config.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* Default EXPORT macro that does nothing (see comment in sq.h): */
#define EXPORT(returnType) returnType

/* Do not include the entire sq.h file but just those parts needed. */
#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"	/*  The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions */

#define true 1
#define false 0
#define null 0  /* using 'null' because nil is predefined in Think C */
#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

#include "LargeIntegersKernels.h"
#include "memoryAccess.h"


/*** Constants ***/
#define LIStackDigits 128
#define PrimErrBadArgument 3
#define PrimErrBadNumArgs 5
#define PrimErrNoMemory 9


/*** Function Prototypes ***/
static sqInt anyBitOfDigitslenfromto(unsigned int *pDigits, sqInt digitLen, sqInt start, sqInt stopArg);
static sqInt byteSizeOfCSI(sqInt csi);
static sqInt cDigitComparewithlen(unsigned int *pFirst, unsigned int *pSecond, sqInt len);
static sqInt cDigitHighBitlen(unsigned int *pUint32, sqInt len);
static sqInt cDigitLshiftfromlentolen(sqInt shiftCount, unsigned int *pFrom, sqInt lenFrom, unsigned int *pTo, sqInt lenTo);
static unsigned int cDigitOfat(unsigned int *cPointer, sqInt zeroBasedDigitIndex);
static sqInt cDigitRshiftfromlentolen(sqInt shiftCount, unsigned int *pFrom, sqInt lenFrom, unsigned int *pTo, sqInt lenTo);
static sqInt cDigitSublenwithleninto(unsigned int *pWordSmall, sqInt smallLen, unsigned int *pWordLarge, sqInt largeLen, unsigned int *pWordRes);
static sqInt cHighBit32(unsigned int anUnsignedInt32);
static sqInt digitBitLogicwithopIndex(sqInt firstInteger, sqInt secondInteger, sqInt opIx);
EXPORT(const char*) getModuleName(void);
static sqInt isNormalized(sqInt aLargeInteger);
static sqInt largeIntgrowTo(sqInt aBytesObject, sqInt newByteLen);
static sqInt normalizedIntegerFromlennegative(unsigned int *pDigits, sqInt len, sqInt neg);
static sqInt normalizeNegative(sqInt aLargeNegativeInteger);
static sqInt normalizePositive(sqInt aLargePositiveInteger);
static unsigned int * pointerToDigitsOfsmallbyteSizeInto(sqInt anInteger, unsigned int *pSmall, sqInt *pByteSize);
EXPORT(sqInt) primAnyBitFromTo(void);
EXPORT(sqInt) primDigitAdd(void);
EXPORT(sqInt) primDigitBitAnd(void);
EXPORT(sqInt) primDigitBitOr(void);
EXPORT(sqInt) primDigitBitShiftMagnitude(void);
EXPORT(sqInt) primDigitBitXor(void);
EXPORT(sqInt) primDigitCompare(void);
EXPORT(sqInt) primDigitDivNegative(void);
EXPORT(sqInt) primDigitMultiplyNegative(void);
EXPORT(sqInt) primDigitSubtract(void);
EXPORT(sqInt) primGetModuleName(void);
EXPORT(sqInt) primMontgomeryDigitLength(void);
EXPORT(sqInt) primMontgomeryTimesModulo(void);
EXPORT(sqInt) primNormalizeNegative(void);
EXPORT(sqInt) primNormalizePositive(void);
EXPORT(sqInt) setInterpreter(struct VirtualMachine *anInterpreter);
static unsigned char unsafeByteOfLargeIntat(sqInt bytesObj, sqInt ix);


/*** Variables ***/
static const int  andOpIndex = 0;

#if defined(SQUEAK_BUILTIN_PLUGIN)

# define isIntegerObject(oop) ((oop) & 1)

extern sqInt classIndexOf(sqInt);
#	define LargeNegativeIntegerClassIndex 32
#	define LargePositiveIntegerClassIndex 33
#	if BytesPerOop == 4
#	  define isImmediate(oop) ((oop) & 3)
#	else
#	  define isImmediate(oop) ((oop) & 7)
#	endif
#	define isKindOfInteger(oop) (isImmediate(oop) ? isIntegerObject(oop) : (unsigned)(classIndexOf(oop) - LargeNegativeIntegerClassIndex) <= 1)
#	define isLargeIntegerObject(oop) (!isImmediate(oop) && (unsigned)(classIndexOf(oop) - LargeNegativeIntegerClassIndex) <= 1)
#	define isLargeNegativeIntegerObject(oop) (!isImmediate(oop) && classIndexOf(oop) == LargeNegativeIntegerClassIndex)
#	define isLargePositiveIntegerObject(oop) (!isImmediate(oop) && classIndexOf(oop) == LargePositiveIntegerClassIndex)
#endif /* defined(SQUEAK_BUILTIN_PLUGIN) */

#if !defined(isKindOfInteger)
# define isLargeNegativeIntegerObject(oop) (fetchClassOf(oop) == classLargeNegativeInteger())
# define isLargePositiveIntegerObject(oop) (fetchClassOf(oop) == classLargePositiveInteger())
# define isLargeIntegerObject(oop) (isLargeNegativeIntegerObject(oop) || isLargePositiveIntegerObject(oop))
# define isKindOfInteger(oop) (isIntegerObject(oop) || isLargeNegativeIntegerObject(oop) || isLargePositiveIntegerObject(oop))
#endif

#if !defined(SQUEAK_BUILTIN_PLUGIN)
static sqInt (*booleanValueOf)(sqInt obj);
static sqInt (*classArray)(void);
static sqInt (*classLargeNegativeInteger)(void);
static sqInt (*classLargePositiveInteger)(void);
static sqInt (*classString)(void);
static sqInt (*failed)(void);
static sqInt (*falseObject)(void);
static sqInt (*fetchClassOf)(sqInt oop);
static void * (*firstIndexableField)(sqInt oop);
static sqInt (*instantiateClassindexableSize)(sqInt classPointer, sqInt size);
static sqInt (*integerObjectOf)(sqInt value);
static sqInt (*integerValueOf)(sqInt oop);
static sqInt (*isBooleanObject)(sqInt oop);
static sqInt (*isIntegerObject)(sqInt objectPointer);
static sqInt (*methodArgumentCount)(void);
static void (*popthenPush)(sqInt nItems, sqInt oop);
static unsigned int (*positive32BitValueOf)(sqInt oop);
static sqInt (*primitiveFail)(void);
static sqInt (*primitiveFailFor)(sqInt reasonCode);
static sqInt (*slotSizeOf)(sqInt oop);
static sqInt (*stObjectatput)(sqInt array, sqInt index, sqInt value);
static sqInt (*stackIntegerValue)(sqInt offset);
static sqInt (*stackValue)(sqInt offset);
static sqInt (*success)(sqInt aBoolean);
static sqInt (*trueObject)(void);
#else /* !defined(SQUEAK_BUILTIN_PLUGIN) */
extern sqInt booleanValueOf(sqInt obj);
extern sqInt classArray(void);
extern sqInt classLargeNegativeInteger(void);
extern sqInt classLargePositiveInteger(void);
extern sqInt classString(void);
extern sqInt failed(void);
extern sqInt falseObject(void);
extern sqInt fetchClassOf(sqInt oop);
extern void * firstIndexableField(sqInt oop);
extern sqInt instantiateClassindexableSize(sqInt classPointer, sqInt size);
extern sqInt integerObjectOf(sqInt value);
extern sqInt integerValueOf(sqInt oop);
#if VM_PROXY_MAJOR > 1 || (VM_PROXY_MAJOR == 1 && VM_PROXY_MINOR >= 15)
extern sqInt isBooleanObject(sqInt oop);
#else
# define isBooleanObject(oop) 0
#endif
#if !defined(isIntegerObject)
extern sqInt isIntegerObject(sqInt objectPointer);
#endif
extern sqInt methodArgumentCount(void);
extern void popthenPush(sqInt nItems, sqInt oop);
extern unsigned int positive32BitValueOf(sqInt oop);
extern sqInt primitiveFail(void);
extern sqInt primitiveFailFor(sqInt reasonCode);
extern sqInt slotSizeOf(sqInt oop);
extern sqInt stObjectatput(sqInt array, sqInt index, sqInt value);
extern sqInt stackIntegerValue(sqInt offset);
extern sqInt stackValue(sqInt offset);
extern sqInt success(sqInt aBoolean);
extern sqInt trueObject(void);
extern
#endif
struct VirtualMachine* interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"LargeIntegers v2.1 VMMaker.oscog-eem.2495 (i)"
#else
	"LargeIntegers v2.1 VMMaker.oscog-eem.2495 (e)"
#endif
;
static const int  orOpIndex = 1;
static const int  xorOpIndex = 2;



/*	Tests for any magnitude bits in the interval from start to stopArg of the
	digitLen 32-bit digits at pDigits. */

	/* LargeIntegersPlugin>>#anyBitOfDigits:len:from:to: */
static sqInt
anyBitOfDigitslenfromto(unsigned int *pDigits, sqInt digitLen, sqInt start, sqInt stopArg)
{
	unsigned int digit;
	usqInt firstDigitIx;
	unsigned int firstMask;
	usqInt ix;
	usqInt lastDigitIx;
	unsigned int lastMask;
	sqInt stop;

	/* missing DebugCode */;
	if ((start < 1) || (stopArg < 1)) {
		return primitiveFail();
	}
	stop = ((stopArg < (cDigitHighBitlen(pDigits, digitLen))) ? stopArg : cDigitHighBitlen(pDigits, digitLen));
	if (start > stop) {
		return 0;
	}
	firstDigitIx = ((start - 1) / 32) + 1;
	lastDigitIx = ((stop - 1) / 32) + 1;

	/* Note asUnsignedLong required to avoid ULLL suffix bug */
	firstMask = ((unsigned long) 0xFFFFFFFFU) << ((start - 1) & 0x1F);
	lastMask = ((usqInt) 0xFFFFFFFFU) >> (0x1F - ((stop - 1) & 0x1F));
	if (firstDigitIx == lastDigitIx) {
		digit = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[firstDigitIx - 1]);
		return (digit & (firstMask & lastMask)) != 0;
	}
	if (((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[firstDigitIx - 1])) & firstMask) != 0) {
		return 1;
	}
	for (ix = firstDigitIx + 1; ix < lastDigitIx; ix += 1) {
		if ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[ix - 1])) != 0) {
			return 1;
		}
	}
	if (((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[lastDigitIx - 1])) & lastMask) != 0) {
		return 1;
	}
	return 0;
}


/*	Answer the number of bytes required to represent the value of a
	C-SmallInteger. 
 */

	/* LargeIntegersPlugin>>#byteSizeOfCSI: */
static sqInt
byteSizeOfCSI(sqInt csi)
{
	if (csi >= 0) {
		if (csi < 256) {
			return 1;
		}
		if (csi < 65536) {
			return 2;
		}
		if (csi < 0x1000000) {
			return 3;
		}
		
#    if BytesPerOop == 4
		return 4;
#    else /* BytesPerOop == 4 */
		if (csi < 0x100000000LL) {
			return 4;
		}
		if (csi < 0x10000000000LL) {
			return 5;
		}
		if (csi < 0x1000000000000LL) {
			return 6;
		}
		if (csi < 0x100000000000000LL) {
			return 7;
		}
		return 8;
#    endif /* BytesPerOop == 4 */
	}
	if (csi > -256) {
		return 1;
	}
	if (csi > -65536) {
		return 2;
	}
	if (csi > -16777216) {
		return 3;
	}
	
#  if BytesPerOop == 4
	return 4;
#  else /* BytesPerOop == 4 */
	if (csi > -4294967296LL) {
		return 4;
	}
	if (csi > -1099511627776LL) {
		return 5;
	}
	if (csi > -281474976710656LL) {
		return 6;
	}
	if (csi > -72057594037927936LL) {
		return 7;
	}
	return 8;
#  endif /* BytesPerOop == 4 */
	return 0;
}


/*	pWordRes len = longLen; returns over.. */

/*	Precondition: pFirst len = pSecond len. */

	/* LargeIntegersPlugin>>#cDigitCompare:with:len: */
static sqInt
cDigitComparewithlen(unsigned int *pFirst, unsigned int *pSecond, sqInt len)
{
	unsigned int firstDigit;
	sqInt ix;
	unsigned int secondDigit;

	ix = len - 1;
	while (ix >= 0) {
		if (((secondDigit = pSecond[ix])) != ((firstDigit = pFirst[ix]))) {
			if ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(secondDigit)) < (SQ_SWAP_4_BYTES_IF_BIGENDIAN(firstDigit))) {
				return 1;
			}
			else {
				return -1;
			}
		}
		ix -= 1;
	}
	return 0;
}

/*	Answer the index (in bits) of the high order bit of the receiver, or zero
	if the 
	receiver is zero. This method is allowed (and needed) for 
	LargeNegativeIntegers as well, since Squeak's LargeIntegers are
	sign/magnitude. Work with 32 bits digits. */

	/* LargeIntegersPlugin>>#cDigitHighBit:len: */
static sqInt
cDigitHighBitlen(unsigned int *pUint32, sqInt len)
{
	unsigned int lastDigit;
	sqInt realLength;

	realLength = len;
	do {
		if (realLength == 0) {
			return 0;
		}
	} while(((lastDigit = cDigitOfat(pUint32, (realLength -= 1)))) == 0);
	return (cHighBit32(lastDigit)) + (32 * realLength);
}


/*	C indexed! */

	/* LargeIntegersPlugin>>#cDigitLshift:from:len:to:len: */
static sqInt
cDigitLshiftfromlentolen(sqInt shiftCount, unsigned int *pFrom, sqInt lenFrom, unsigned int *pTo, sqInt lenTo)
{
	sqInt bitShift;
	unsigned int carry;
	unsigned int digit;
	sqInt digitShift;
	sqInt i;
	sqInt i2;
	sqInt limit;
	sqInt rshift;

	digitShift = shiftCount / 32;
	bitShift = shiftCount % 32;
	limit = digitShift - 1;
	for (i = 0; i <= limit; i += 1) {
		pTo[i] = 0;
	}
	if (bitShift == 0) {
		/* begin cDigitCopyFrom:to:len: */
		for (i2 = 0; i2 < (((lenTo - 1) - digitShift) + 1); i2 += 1) {
			(pTo + digitShift)[i2] = pFrom[i2];
		}
		return 0;
	}
	rshift = 32 - bitShift;
	carry = 0;
	limit = lenFrom - 1;
	for (i = 0; i <= limit; i += 1) {
		digit = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFrom[i]);
		pTo[i + digitShift] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((carry | (((usqInt) digit) << bitShift)) & 0xFFFFFFFFU);
		carry = ((usqInt) digit) >> rshift;
	}
	if (!(carry == 0)) {
		pTo[lenTo - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(carry);
	}
	return 0;
}


	/* LargeIntegersPlugin>>#cDigitOf:at: */
static unsigned int
cDigitOfat(unsigned int *cPointer, sqInt zeroBasedDigitIndex)
{
	return SQ_SWAP_4_BYTES_IF_BIGENDIAN((cPointer[zeroBasedDigitIndex]));
}

/*	pWordRes len = longLen.
	NOTE: we don't bother with endianness here, those bit opes are
	endian-neutral 
 */


/*	C indexed! */

	/* LargeIntegersPlugin>>#cDigitRshift:from:len:to:len: */
static sqInt
cDigitRshiftfromlentolen(sqInt shiftCount, unsigned int *pFrom, sqInt lenFrom, unsigned int *pTo, sqInt lenTo)
{
	sqInt bitShift;
	unsigned int carry;
	unsigned int digit;
	sqInt digitShift;
	sqInt i;
	sqInt j;
	sqInt leftShift;
	sqInt limit;
	sqInt start;

	digitShift = shiftCount / 32;
	bitShift = shiftCount % 32;
	if (bitShift == 0) {
		/* begin cDigitReplace:from:to:with:startingAt: */
		/* begin cDigitCopyFrom:to:len: */
		for (i = 0; i < ((lenTo - 1) + 1); i += 1) {
			pTo[i] = (pFrom + digitShift)[i];
		}
		return 0;
	}
	leftShift = 32 - bitShift;
	carry = ((usqInt) (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFrom[digitShift]))) >> bitShift;
	start = digitShift + 1;
	limit = lenFrom - 1;
	for (j = start; j <= limit; j += 1) {
		/* begin cDigitOf:at: */
		digit = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFrom[j]);
		/* begin cDigitOf:at:put: */
		pTo[j - start] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((carry | (((usqInt) digit) << leftShift)) & 0xFFFFFFFFU);
		carry = ((usqInt) digit) >> bitShift;
	}
	if (!(carry == 0)) {
		/* begin cDigitOf:at:put: */
		pTo[lenTo - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(carry);
	}
	return 0;
}


	/* LargeIntegersPlugin>>#cDigitSub:len:with:len:into: */
static sqInt
cDigitSublenwithleninto(unsigned int *pWordSmall, sqInt smallLen, unsigned int *pWordLarge, sqInt largeLen, unsigned int *pWordRes)
{
	sqInt i;
	unsigned long long z;

	z = 0;
	for (i = 0; i < smallLen; i += 1) {
		z = (z + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordLarge[i]))) - (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordSmall[i]));
		/* begin cDigitOf:at:put: */
		pWordRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) (z & 0xFFFFFFFFU)));
		z = 0 - (z >> 0x3F);
	}
	for (i = smallLen; i < largeLen; i += 1) {
		z += SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordLarge[i]);
		/* begin cDigitOf:at:put: */
		pWordRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) (z & 0xFFFFFFFFU)));
		z = 0 - (z >> 0x3F);
	}
	return 0;
}


/*	Answer the index of the high order bit of the argument, or zero if the 
	argument is zero. */

	/* LargeIntegersPlugin>>#cHighBit32: */
static sqInt
cHighBit32(unsigned int anUnsignedInt32)
{
	sqInt bitNo;
	unsigned int shifted;

	shifted = anUnsignedInt32;
	bitNo = 0;
	if (!(shifted < (1U << 16))) {
		shifted = ((usqInt) shifted) >> 16;
		bitNo += 16;
	}
	if (!(shifted < (1U << 8))) {
		shifted = ((usqInt) shifted) >> 8;
		bitNo += 8;
	}
	if (!(shifted < (1U << 4))) {
		shifted = ((usqInt) shifted) >> 4;
		bitNo += 4;
	}
	if (!(shifted < (1U << 2))) {
		shifted = ((usqInt) shifted) >> 2;
		bitNo += 2;
	}
	if (!(shifted < (1U << 1))) {
		shifted = ((usqInt) shifted) >> 1;
		bitNo += 1;
	}
	return bitNo + shifted;
}


/*	Answer a new LargeInteger of the class anInteger has or converts to, with
	its magnitude, the byteLen bytes at pDigits, shifted left by shiftCount
	bits. See #pointerToDigitsOf:small:byteSizeInto:. */
/*	Does not normalize: the answer is a LargeInteger even when it would fit a
	SmallInteger, and one of 1 byte for a zero magnitude. */

/*	Answer the sum of the magnitudes of the firstDigitLen and secondDigitLen
	32-bit digits at pFirst and pSecond, with the sign neg. */
/*	Normalizes, except when the sum carries out of the longer operand: then
	the answer is a LargeInteger of 4 * longDigitLen + 1 bytes, even when it
	would fit a SmallInteger. */

/*	Bit logic here is only implemented for positive integers or Zero;
	if rec or arg is negative, it fails. */

	/* LargeIntegersPlugin>>#digitBitLogic:with:opIndex: */
static sqInt
digitBitLogicwithopIndex(sqInt firstInteger, sqInt secondInteger, sqInt opIx)
{
	sqInt firstLen;
	unsigned int firstSmall[2];
	sqInt i;
	sqInt limit;
	sqInt longLen;
	unsigned int *pFirst;
	unsigned int *pLong;
	unsigned int *pRes;
	unsigned int *pSecond;
	unsigned int *pShort;
	unsigned int resBuffer[LIStackDigits];
	sqInt result;
	sqInt secondLen;
	unsigned int secondSmall[2];
	sqInt shortLen;

	firstLen = 0;
	secondLen = 0;
	pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
	pSecond = pointerToDigitsOfsmallbyteSizeInto(secondInteger, secondSmall, &secondLen);
	if (pFirst == firstSmall) {
		if ((integerValueOf(firstInteger)) < 0) {
			return primitiveFail();
		}
	}
	else {
		if (!(isLargePositiveIntegerObject(firstInteger))) {
			return primitiveFail();
		}
	}
	if (pSecond == secondSmall) {
		if ((integerValueOf(secondInteger)) < 0) {
			return primitiveFail();
		}
	}
	else {
		if (!(isLargePositiveIntegerObject(secondInteger))) {
			return primitiveFail();
		}
	}
	longLen = ((firstLen < secondLen) ? secondLen : firstLen);
	if (((longLen + 3) / 4) <= LIStackDigits) {
		result = null;
		pRes = resBuffer;
	}
	else {
		result = instantiateClassindexableSize(classLargePositiveInteger(), longLen);
		if (!result) {
			return primitiveFailFor(PrimErrNoMemory);
		}
		pRes = ((unsigned int *) (firstIndexableField(result)));
		if (isIntegerObject(firstInteger)) {
			pFirst = firstSmall;
		}
		else {
			/* begin pointerToFirstDigitOfLargeInt: */
			pFirst = ((unsigned int *) (firstIndexableField(firstInteger)));
		}
		if (isIntegerObject(secondInteger)) {
			pSecond = secondSmall;
		}
		else {
			/* begin pointerToFirstDigitOfLargeInt: */
			pSecond = ((unsigned int *) (firstIndexableField(secondInteger)));
		}
	}
	if (firstLen < secondLen) {
		shortLen = firstLen;
		pShort = pFirst;
		pLong = pSecond;
	}
	else {
		shortLen = secondLen;
		pShort = pSecond;
		pLong = pFirst;
	}
	/* begin cDigitOp:short:len:long:len:into: */
	limit = ((shortLen + 3) / 4) - 1;
	if (opIx == andOpIndex) {
		for (i = 0; i <= limit; i += 1) {
			pRes[i] = (pShort[i]) & (pLong[i]);
		}
		limit = ((longLen + 3) / 4) - 1;
		for (i = (shortLen + 3) / 4; i <= limit; i += 1) {
			pRes[i] = 0;
		}
		goto l2;
	}
	if (opIx == orOpIndex) {
		for (i = 0; i <= limit; i += 1) {
			pRes[i] = (pShort[i]) | (pLong[i]);
		}
		limit = ((longLen + 3) / 4) - 1;
		for (i = (shortLen + 3) / 4; i <= limit; i += 1) {
			pRes[i] = pLong[i];
		}
		goto l2;
	}
	if (opIx == xorOpIndex) {
		for (i = 0; i <= limit; i += 1) {
			pRes[i] = (pShort[i]) ^ (pLong[i]);
		}
		limit = ((longLen + 3) / 4) - 1;
		for (i = (shortLen + 3) / 4; i <= limit; i += 1) {
			pRes[i] = pLong[i];
		}
		goto l2;
	}
	primitiveFail();
	l2:
	;
	if (failed()) {
		return 0;
	}
	/* begin normalized:digits:len:negative: */
	if (!result) {
		return normalizedIntegerFromlennegative(pRes, (longLen + 3) / 4, 0);
	}
	return normalizePositive(result);
}


/*	Compare the magnitude of firstInteger with that of secondInteger. 
	Return a code of 1, 0, -1 for firstInteger >, = , < secondInteger */

/*	Answer {quotient. remainder} of the magnitudes of firstInteger, the
	firstLen bytes at pFirst, and of the secondLen bytes at pSecond. */
/*	Does not normalize: the quotient is a LargeInteger of quoDigitLen 32-bit
	digits with the sign neg, the remainder a LargeInteger with the sign of
	firstInteger and no leading zero byte (no byte at all when it is zero).
	When the dividend has fewer digits than the divisor, answer {0.
	firstInteger}, with a SmallInteger firstInteger converted to a
	LargeInteger. */
/*	Division by zero has to be checked in caller. */

/*	The lengths are in 32-bit digits. Normalizes. */

/*	The lengths are in bytes. Normalizes. */

/*	Answer the value of a 32 bits digit in a C-SmallInteger. */
/*	ST indexed! */

/*	Argument has to be aLargeInteger! */

/*	Answer the number of 32-bits fields of a C-SmallInteger. This value is 
	the same as the largest legal subscript. */

/*	answer number of 32 bits digits of a Large Integer */

/*	The lengths are in 32-bit digits, firstNeg is the sign of the first
	operand. Normalizes. */

/*	Answer a new LargeInteger with the sign neg and the magnitude of the
	first a 32-bit digits at pFrom shifted right by shiftCount bits. */
/*	Does not normalize, but the answer has no leading zero byte: it has no
	byte at all when all bits are lost. */

/*	Answer the normalized Integer with the sign neg and the magnitude of the
	digitLen 32-bit digits at pFrom shifted right by shiftCount bits. */


/*	Note: This is hardcoded so it can be run from Squeak.
	The module name is used for validating a module *after*
	it is loaded to check if it does really contain the module
	we're thinking it contains. This is important! */

	/* InterpreterPlugin>>#getModuleName */
EXPORT(const char*)
getModuleName(void)
{
	return moduleName;
}

/*	Check for leading zero of LargeInteger */

	/* LargeIntegersPlugin>>#isNormalized: */
static sqInt
isNormalized(sqInt aLargeInteger)
{
	sqInt len;

	/* begin byteSizeOfLargeInt: */
	len = slotSizeOf(aLargeInteger);
	if (len == 0) {
		return 0;
	}
	if ((unsafeByteOfLargeIntat(aLargeInteger, len)) == 0) {
		return 0;
	}
	return 1;
}


/*	Attention: this method invalidates all oop's! Only newBytes is valid at
	return. 
 */
/*	Does not normalize. */

	/* LargeIntegersPlugin>>#largeInt:growTo: */
static sqInt
largeIntgrowTo(sqInt aBytesObject, sqInt newByteLen)
{
	sqInt copyLen;
	sqInt i;
	sqInt newBytes;
	sqInt newDigitLen;
	sqInt oldDigitLen;
	unsigned int *pFrom;
	unsigned int *pTo;

	newBytes = instantiateClassindexableSize(fetchClassOf(aBytesObject), newByteLen);
	if (!(newBytes)) {
		primitiveFailFor(PrimErrNoMemory);
		return null;
	}

	newDigitLen = (newByteLen + 3) / 4;
	/* begin digitSizeOfLargeInt: */
	oldDigitLen = ((slotSizeOf(aBytesObject)) + 3) / 4;
	copyLen = (oldDigitLen < newDigitLen
		? oldDigitLen
		: newDigitLen);
	/* begin cDigitCopyFrom:to:len: */
	pFrom = ((unsigned int *) (firstIndexableField(aBytesObject)));
	pTo = ((unsigned int *) (firstIndexableField(newBytes)));
	for (i = 0; i < copyLen; i += 1) {
		pTo[i] = (pFrom[i]);
	}
	return newBytes;
}


/*	Answer the normalized Integer with the sign neg and the magnitude of the
	len 32-bit digits at pDigits: a SmallInteger if it fits, else a new
	LargeInteger of the significant bytes only.
	That is what #normalizePositive: and #normalizeNegative: answer for a new
	LargeInteger holding these digits, without allocating that LargeInteger
	first. */

	/* LargeIntegersPlugin>>#normalizedIntegerFrom:len:negative: */
static sqInt
normalizedIntegerFromlennegative(unsigned int *pDigits, sqInt len, sqInt neg)
{
	sqInt byteLen;
	sqInt digitLen;
	sqInt i;
	usqInt limit;
	unsigned int *pTo;
	sqInt result;
	int sLen;
	usqInt val;
	usqInt val2;

	digitLen = len;
	while ((digitLen != 0) && ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[digitLen - 1])) == 0)) {
		digitLen -= 1;
	}
	if (digitLen == 0) {
		return integerObjectOf(0);
	}
	/* begin cDigitOf:at: */
	val = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[digitLen - 1]);
	sLen = ((MaxSmallInteger > 0x3FFFFFFF)
		 ? 2
		 : 1);
	if (digitLen <= sLen) {
		val2 = val;
		if (digitLen > 1) {
			val2 = (((unsigned long long) val2) << 32) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDigits[0]));
		}
		if (neg) {
			limit = 0 - MinSmallInteger;
			if (val2 <= limit) {
				return integerObjectOf(0 - val2);
			}
		}
		else {
			limit = MaxSmallInteger;
			if (val2 <= limit) {
				return integerObjectOf(val2);
			}
		}
	}
	byteLen = digitLen * 4;
	if (val <= 0xFFFF) {
		byteLen -= 2;
	}
	else {
		val = ((usqInt) val) >> 16;
	}
	if (val <= 0xFF) {
		byteLen -= 1;
	}
	/* begin createLargeIntegerNeg:byteLength: */
	result = instantiateClassindexableSize(((neg)
		 ? classLargeNegativeInteger()
		 : classLargePositiveInteger()), byteLen);
	if (!result) {
		primitiveFailFor(PrimErrNoMemory);
		return null;
	}
	/* begin cDigitCopyFrom:to:len: */
	pTo = ((unsigned int *) (firstIndexableField(result)));
	for (i = 0; i < digitLen; i += 1) {
		pTo[i] = pDigits[i];
	}
	return result;
}


/*	Check for leading zeroes and return shortened copy if so. */
/*	First establish len = significant length. */

	/* LargeIntegersPlugin>>#normalizeNegative: */
static sqInt
normalizeNegative(sqInt aLargeNegativeInteger)
{
	sqInt byteLen;
	unsigned int *cPointer;
	sqInt digitLen;
	usqInt minVal;
	sqInt oldByteLen;
	int sLen;
	usqInt val;
	usqInt val2;

	/* begin digitSizeOfLargeInt: */
	digitLen = ((slotSizeOf(aLargeNegativeInteger)) + 3) / 4;
	while ((digitLen != 0)
	 && ((cDigitOfat(((unsigned int *) (firstIndexableField(aLargeNegativeInteger))), digitLen - 1)) == 0)) {
		digitLen -= 1;
	}
	if (digitLen == 0) {
		return integerObjectOf(0);
	}
	/* begin cDigitOf:at: */
	cPointer = ((unsigned int *) (firstIndexableField(aLargeNegativeInteger)));
	val = SQ_SWAP_4_BYTES_IF_BIGENDIAN((cPointer[digitLen - 1]));

	/* SmallInteger minVal digitLength */
	sLen = ((MinSmallInteger) < -1073741824LL
		? 2
		: 1);
	if (digitLen <= sLen) {
		minVal = 0 - (MinSmallInteger);
		val2 = val;
		if (digitLen > 1) {
			val2 = (val2 << 32) + (cDigitOfat(((unsigned int *) (firstIndexableField(aLargeNegativeInteger))), 1 - 1));
		}
		if (val2 <= minVal) {
			return integerObjectOf((0 - val2));
		}
	}
	byteLen = digitLen * 4;
	if (val <= 0xFFFF) {
		byteLen -= 2;
	}
	else {
		val = ((usqInt) val) >> 16;
	}
	if (val <= 0xFF) {
		byteLen -= 1;
	}
	/* begin byteSizeOfLargeInt: */
	oldByteLen = slotSizeOf(aLargeNegativeInteger);
	if (byteLen < oldByteLen) {
		return largeIntgrowTo(aLargeNegativeInteger, byteLen);
	}
	else {
		return aLargeNegativeInteger;
	}
}


/*	Check for leading zeroes and return shortened copy if so. */
/*	First establish len = significant length. */

	/* LargeIntegersPlugin>>#normalizePositive: */
static sqInt
normalizePositive(sqInt aLargePositiveInteger)
{
	sqInt byteLen;
	unsigned int *cPointer;
	sqInt digitLen;
	usqInt maxVal;
	sqInt oldByteLen;
	int sLen;
	usqInt val;
	usqInt val2;

	/* begin digitSizeOfLargeInt: */
	digitLen = ((slotSizeOf(aLargePositiveInteger)) + 3) / 4;
	while ((digitLen != 0)
	 && ((cDigitOfat(((unsigned int *) (firstIndexableField(aLargePositiveInteger))), digitLen - 1)) == 0)) {
		digitLen -= 1;
	}
	if (digitLen == 0) {
		return integerObjectOf(0);
	}
	/* begin cDigitOf:at: */
	cPointer = ((unsigned int *) (firstIndexableField(aLargePositiveInteger)));
	val = SQ_SWAP_4_BYTES_IF_BIGENDIAN((cPointer[digitLen - 1]));

	/* SmallInteger maxVal digitLength */
	sLen = ((MaxSmallInteger) > 0x3FFFFFFF
		? 2
		: 1);
	if (digitLen <= sLen) {
		maxVal = MaxSmallInteger;
		val2 = val;
		if (digitLen > 1) {

			/* Note: asUnsignedLongLong is not necessary because this branch is for 64 bits only.
			   but we want to avoid a C Compiler warning on 32 bits */
			val2 = ((((unsigned long long)val2)) << 32) + (cDigitOfat(((unsigned int *) (firstIndexableField(aLargePositiveInteger))), 1 - 1));
		}
		if (val2 <= maxVal) {
			return integerObjectOf(val2);
		}
	}
	byteLen = digitLen * 4;
	if (val <= 0xFFFF) {
		byteLen -= 2;
	}
	else {
		val = ((usqInt) val) >> 16;
	}
	if (val <= 0xFF) {
		byteLen -= 1;
	}
	/* begin byteSizeOfLargeInt: */
	oldByteLen = slotSizeOf(aLargePositiveInteger);
	if (byteLen < oldByteLen) {
		return largeIntgrowTo(aLargePositiveInteger, byteLen);
	}
	else {
		return aLargePositiveInteger;
	}
}


/*	Answer a pointer to the 32-bit digits of anInteger, a SmallInteger or a
	LargeInteger, and store its byte size at pByteSize.
	A SmallInteger is not converted to a scratch LargeInteger: its magnitude
	is stored into the two 32-bit digits at pSmall, as
	#createLargeFromSmallInteger: would store it into that LargeInteger, the
	answer is pSmall and the byte size is that of that LargeInteger.
	Spur's #instantiateClass:indexableSize: does not move objects: it never
	collects garbage synchronously (it schedules a scavenge and falls back to
	old space). So the pointer stays valid across the allocations of a
	primitive, and the C relies on it: Slang emits nothing for #remapOop:in:,
	see InterpreterPlugin>>#remapOop:in:. The #remapOop:in: blocks around
	allocations, and fetching the pointer again with #pointerToDigitsOf:small:
	after one, only state which oops and pointers are live across an
	allocation; they would not by themselves make the C safe with an object
	memory that moves objects when it allocates. */

	/* LargeIntegersPlugin>>#pointerToDigitsOf:small:byteSizeInto: */
static unsigned int *
pointerToDigitsOfsmallbyteSizeInto(sqInt anInteger, unsigned int *pSmall, sqInt *pByteSize)
{
	unsigned long long mag;
	sqInt val;

	if (!(isIntegerObject(anInteger))) {
		pByteSize[0] = slotSizeOf(anInteger);
		return ((unsigned int *) (firstIndexableField(anInteger)));
	}
	val = integerValueOf(anInteger);
	mag = ((val < 0)
		 ? 0 - val
		 : val);
	/* begin cDigitOf:at:put: */
	pSmall[0] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) (mag & 0xFFFFFFFFU)));
	/* begin cDigitOf:at:put: */
	pSmall[1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) ((mag >> 32) & 0xFFFFFFFFU)));
	pByteSize[0] = byteSizeOfCSI(val);
	return pSmall;
}


	/* LargeIntegersPlugin>>#primAnyBitFrom:to: */
EXPORT(sqInt)
primAnyBitFromTo(void)
{
	sqInt byteLen;
	sqInt from;
	sqInt fromOop;
	sqInt integer;
	unsigned int *pDigits;
	unsigned int small[2];
	sqInt to;
	sqInt toOop;
	sqInt _return_value;

	fromOop = stackValue(1);
	toOop = stackValue(0);
	if (!((methodArgumentCount()) == 2)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!((isIntegerObject(fromOop))
		 && (isIntegerObject(toOop)))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	from = integerValueOf(fromOop);
	to = integerValueOf(toOop);
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(2)));
	integer = stackValue(2);
	if (failed()) {
		return null;
	}
	pDigits = pointerToDigitsOfsmallbyteSizeInto(integer, small, &byteLen);
	if (!(failed())) {
		_return_value = ((anyBitOfDigitslenfromto(pDigits, (byteLen + 3) / 4, from, to)) ? trueObject() : falseObject());
		if (!(failed())) {
			popthenPush(3, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primDigitAdd: */
EXPORT(sqInt)
primDigitAdd(void)
{
	unsigned long long accum;
	unsigned int *cPointer;
	sqInt firstDigitLen;
	sqInt firstInteger;
	sqInt firstLen;
	unsigned int firstSmall[2];
	sqInt i;
	sqInt i2;
	sqInt longDigitLen;
	sqInt neg;
	sqInt newSum;
	unsigned int over;
	unsigned int *pFirst;
	unsigned int *pLong;
	unsigned int *pSecond;
	unsigned int *pShort;
	unsigned int *pSum;
	unsigned int *pTo;
	sqInt secondDigitLen;
	sqInt secondInteger;
	sqInt secondLen;
	unsigned int secondSmall[2];
	sqInt shortDigitLen;
	sqInt sum;
	unsigned int sumBuffer[LIStackDigits];
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		/* begin digitAddLarge:with: */
		pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
		pSecond = pointerToDigitsOfsmallbyteSizeInto(secondInteger, secondSmall, &secondLen);
		firstDigitLen = (firstLen + 3) / 4;
		secondDigitLen = (secondLen + 3) / 4;
		neg = ((isIntegerObject(firstInteger))
			 ? (integerValueOf(firstInteger)) < 0
			 : isLargeNegativeIntegerObject(firstInteger));
		longDigitLen = ((firstDigitLen < secondDigitLen) ? secondDigitLen : firstDigitLen);
		if (longDigitLen <= LIStackDigits) {
			sum = null;
			pSum = sumBuffer;
		}
		else {
			/* begin createLargeIntegerNeg:digitLength: */
			/* begin createLargeIntegerNeg:byteLength: */
			sum = instantiateClassindexableSize(((neg)
				 ? classLargeNegativeInteger()
				 : classLargePositiveInteger()), longDigitLen * 4);
			if (!sum) {
				_return_value = primitiveFailFor(PrimErrNoMemory);
				goto l9;
			}
			pSum = ((unsigned int *) (firstIndexableField(sum)));
			if (isIntegerObject(firstInteger)) {
				pFirst = firstSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pFirst = ((unsigned int *) (firstIndexableField(firstInteger)));
			}
			if (isIntegerObject(secondInteger)) {
				pSecond = secondSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pSecond = ((unsigned int *) (firstIndexableField(secondInteger)));
			}
		}
		if (firstDigitLen <= secondDigitLen) {
			pShort = pFirst;
			shortDigitLen = firstDigitLen;
			pLong = pSecond;
		}
		else {
			pShort = pSecond;
			shortDigitLen = secondDigitLen;
			pLong = pFirst;
		}
		/* begin cDigitAdd:len:with:len:into: */
		accum = 0;
		for (i = 0; i < shortDigitLen; i += 1) {
			accum = ((accum >> 32) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pShort[i]))) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pLong[i]));
			pSum[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
		}
		for (i = shortDigitLen; i < longDigitLen; i += 1) {
			accum = (accum >> 32) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pLong[i]));
			pSum[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
		}
		over = accum >> 32;
		if (!(over > 0)) {
			/* begin normalized:digits:len:negative: */
			if (!sum) {
				_return_value = normalizedIntegerFromlennegative(pSum, longDigitLen, neg);
				goto l9;
			}
			_return_value = ((neg)
				 ? normalizeNegative(sum)
				 : normalizePositive(sum));
			goto l9;
		}
		/* begin createLargeIntegerNeg:byteLength: */
		newSum = instantiateClassindexableSize(((neg)
			 ? classLargeNegativeInteger()
			 : classLargePositiveInteger()), (longDigitLen * 4) + 1);
		if (!newSum) {
			_return_value = primitiveFailFor(PrimErrNoMemory);
			goto l9;
		}
		if (sum) {
			pSum = ((unsigned int *) (firstIndexableField(sum)));
		}
		/* begin cDigitCopyFrom:to:len: */
		pTo = ((unsigned int *) (firstIndexableField(newSum)));
		for (i2 = 0; i2 < longDigitLen; i2 += 1) {
			pTo[i2] = pSum[i2];
		}
		/* begin cDigitOf:at:put: */
		cPointer = ((unsigned int *) (firstIndexableField(newSum)));
		cPointer[longDigitLen] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(over);
		_return_value = newSum;
		l9:
		;
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}


/*	Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails. */

	/* LargeIntegersPlugin>>#primDigitBitAnd: */
EXPORT(sqInt)
primDigitBitAnd(void)
{
	sqInt firstInteger;
	sqInt secondInteger;
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		_return_value = digitBitLogicwithopIndex(firstInteger, secondInteger, andOpIndex);
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}


/*	Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails. */

	/* LargeIntegersPlugin>>#primDigitBitOr: */
EXPORT(sqInt)
primDigitBitOr(void)
{
	sqInt firstInteger;
	sqInt secondInteger;
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		_return_value = digitBitLogicwithopIndex(firstInteger, secondInteger, orOpIndex);
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primDigitBitShiftMagnitude: */
EXPORT(sqInt)
primDigitBitShiftMagnitude(void)
{
	sqInt anInteger;
	sqInt byteLen;
	sqInt highBit;
	sqInt largeClass;
	unsigned int lastDigit;
	sqInt neg;
	sqInt newBitLen;
	sqInt newByteLen;
	sqInt newDigitLen;
	sqInt newDigitLen2;
	sqInt newOop;
	sqInt newOop2;
	sqInt oldBitLen;
	unsigned int *pDigits;
	sqInt realLength;
	unsigned int resBuffer[LIStackDigits];
	sqInt shiftCount;
	sqInt shiftCountOop;
	unsigned int small[2];
	sqInt _return_value;

	shiftCountOop = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isIntegerObject(shiftCountOop))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	shiftCount = integerValueOf(shiftCountOop);
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	anInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	pDigits = pointerToDigitsOfsmallbyteSizeInto(anInteger, small, &byteLen);
	if (shiftCount >= 0) {
		/* begin digit:digits:small:len:Lshift: */
		if (pDigits == small) {
			largeClass = (((integerValueOf(anInteger)) < 0)
				 ? classLargeNegativeInteger()
				 : classLargePositiveInteger());
		}
		else {
			largeClass = fetchClassOf(anInteger);
		}
		if (((highBit = cDigitHighBitlen(pDigits, (byteLen + 3) / 4))) == 0) {
			_return_value = instantiateClassindexableSize(largeClass, 1);
			goto l2;
		}
		newByteLen = ((highBit + shiftCount) + 7) / 8;
		newOop = instantiateClassindexableSize(largeClass, newByteLen);
		if (!newOop) {
			primitiveFailFor(PrimErrNoMemory);
			_return_value = null;
			goto l2;
		}
		newDigitLen = (newByteLen + 3) / 4;
		cDigitLshiftfromlentolen(shiftCount, ((pDigits == small)
			 ? small
			 : ((unsigned int *) (firstIndexableField(anInteger)))), (highBit + 0x1F) / 32, ((unsigned int *) (firstIndexableField(newOop))), newDigitLen);
		_return_value = newOop;
		l2:
		;
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
		return null;
	}
	else {
		/* begin digit:digits:small:len:Rshift: */
		neg = ((pDigits == small)
			 ? (integerValueOf(anInteger)) < 0
			 : isLargeNegativeIntegerObject(anInteger));
		/* begin cDigitHighBit:len: */
		realLength = (byteLen + 3) / 4;
		do {
			if (realLength == 0) {
				oldBitLen = 0;
				goto l3;
			}
		} while (((lastDigit = cDigitOfat(pDigits, (realLength -= 1)))) == 0);
		oldBitLen = (cHighBit32(lastDigit)) + (32 * realLength);
		l3:
		;
		newBitLen = oldBitLen - (0 - shiftCount);
		if (newBitLen <= 0) {
			_return_value = integerObjectOf(0);
			goto l5;
		}
		newDigitLen2 = (newBitLen + 0x1F) / 32;
		if (newDigitLen2 <= LIStackDigits) {
			cDigitRshiftfromlentolen(0 - shiftCount, pDigits, (oldBitLen + 0x1F) / 32, resBuffer, newDigitLen2);
			_return_value = normalizedIntegerFromlennegative(resBuffer, newDigitLen2, neg);
			goto l5;
		}
		/* begin createLargeIntegerNeg:byteLength: */
		newOop2 = instantiateClassindexableSize(((neg)
			 ? classLargeNegativeInteger()
			 : classLargePositiveInteger()), (newBitLen + 7) / 8);
		if (!newOop2) {
			_return_value = primitiveFailFor(PrimErrNoMemory);
			goto l5;
		}
		cDigitRshiftfromlentolen(0 - shiftCount, ((pDigits == small)
			 ? small
			 : ((unsigned int *) (firstIndexableField(anInteger)))), (oldBitLen + 0x1F) / 32, ((unsigned int *) (firstIndexableField(newOop2))), newDigitLen2);
		_return_value = newOop2;
		l5:
		;
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
		return null;
	}
}


/*	Bit logic here is only implemented for positive integers or Zero; if rec 
	or arg is negative, it fails. */

	/* LargeIntegersPlugin>>#primDigitBitXor: */
EXPORT(sqInt)
primDigitBitXor(void)
{
	sqInt firstInteger;
	sqInt secondInteger;
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		_return_value = digitBitLogicwithopIndex(firstInteger, secondInteger, xorOpIndex);
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}


/*	Compare the magnitude of self with that of arg. 
	Answer a code of 1, 0, -1 for self >, = , < arg */

	/* LargeIntegersPlugin>>#primDigitCompare: */
EXPORT(sqInt)
primDigitCompare(void)
{
	sqInt firstDigitLen;
	sqInt firstInteger;
	sqInt firstVal;
	sqInt secondDigitLen;
	sqInt secondInteger;
	sqInt secondVal;
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (isIntegerObject(firstInteger)) {
		if (isIntegerObject(secondInteger)) {
			firstVal = integerValueOf(firstInteger);

			/* Compute their magnitudes.  Since SmallIntegers are tagged they have
			   fewer bits than an integer on the platform; therefore in computing their
			   magnitude they cannot overflow. */
			secondVal = integerValueOf(secondInteger);
			if (firstVal < 0) {
				firstVal = 0 - firstVal;
			}
			if (secondVal < 0) {
				secondVal = 0 - secondVal;
			}
			if (!(failed())) {
				_return_value = (firstVal == secondVal
					? integerObjectOf(0)
					: (firstVal < secondVal
							? integerObjectOf(-1)
							: integerObjectOf(1)));
				if (!(failed())) {
					popthenPush(2, _return_value);
				}
			}
			return null;
		}
		if (!(failed())) {
			_return_value = integerObjectOf(-1);
			if (!(failed())) {
				popthenPush(2, _return_value);
			}
		}
		return null;
	}
	if (isIntegerObject(secondInteger)) {
		_return_value = integerObjectOf(1);
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
		return null;
	}
	if (!(failed())) {
		/* begin digitCompareLarge:with: */
		firstDigitLen = ((slotSizeOf(firstInteger)) + 3) / 4;
		secondDigitLen = ((slotSizeOf(secondInteger)) + 3) / 4;
		if (secondDigitLen != firstDigitLen) {
			if (secondDigitLen > firstDigitLen) {
				_return_value = integerObjectOf(-1);
				goto l1;
			}
			else {
				_return_value = integerObjectOf(1);
				goto l1;
			}
		}
		_return_value = integerObjectOf((cDigitComparewithlen(((unsigned int *) (firstIndexableField(firstInteger))), ((unsigned int *) (firstIndexableField(secondInteger))), firstDigitLen)));
	l1:	/* end digitCompareLarge:with: */;
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}


/*	Answer the result of dividing firstInteger by secondInteger. 
	Fail if parameters are not integers, not normalized or secondInteger is 
	zero. */

	/* LargeIntegersPlugin>>#primDigitDiv:negative: */
EXPORT(sqInt)
primDigitDivNegative(void)
{
	unsigned int aValue;
	sqInt byteSize;
	sqInt d;
	sqInt digitSize;
	unsigned int divBuffer[LIStackDigits];
	sqInt firstDigitLen;
	sqInt firstInteger;
	sqInt firstLarge;
	sqInt firstLen;
	sqInt firstNeg;
	unsigned int firstSmall[2];
	sqInt i;
	sqInt i2;
	sqInt ix;
	unsigned int lastDigit;
	sqInt neg;
	sqInt negOop;
	sqInt newBitLen;
	sqInt newByteLen;
	sqInt newDigitLen;
	sqInt newOop;
	sqInt oldBitLen;
	sqInt oldDigitLen;
	unsigned int *pDigit;
	unsigned int *pDiv;
	unsigned int *pFirst;
	unsigned int *pQuo;
	unsigned int *pRem;
	unsigned int *pSecond;
	sqInt quo;
	sqInt quoDigitLen;
	sqInt realLength;
	sqInt rem;
	unsigned int remBuffer[LIStackDigits];
	sqInt res;
	sqInt result;
	sqInt secondDigitLen;
	sqInt secondInteger;
	sqInt secondLen;
	unsigned int secondSmall[2];
	sqInt val;
	sqInt _return_value;

	secondInteger = stackValue(1);
	negOop = stackValue(0);
	if (!((methodArgumentCount()) == 2)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!((isKindOfInteger(secondInteger))
		 && (isBooleanObject(negOop)))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	neg = booleanValueOf(negOop);
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(2)));
	firstInteger = stackValue(2);
	if (failed()) {
		return null;
	}
	if (!(isIntegerObject(firstInteger))) {
		if (!(isNormalized(firstInteger))) {
			/* missing DebugCode */;
			primitiveFail();
			return null;
		}
	}
	if (isIntegerObject(secondInteger)) {
		if ((integerValueOf(secondInteger)) == 0) {
			primitiveFail();
			return null;
		}
	}
	else {
		if (!(isNormalized(secondInteger))) {
			/* missing DebugCode */;
			primitiveFail();
			return null;
		}
	}
	if (!(failed())) {
		/* begin digitDivLarge:with:negative: */
		pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
		pSecond = pointerToDigitsOfsmallbyteSizeInto(secondInteger, secondSmall, &secondLen);
		firstDigitLen = (firstLen + 3) / 4;
		secondDigitLen = (secondLen + 3) / 4;
		quoDigitLen = (firstDigitLen - secondDigitLen) + 1;
		if (quoDigitLen <= 0) {
			if (isIntegerObject(firstInteger)) {
				/* begin createLargeFromSmallInteger: */
				val = integerValueOf(firstInteger);
				byteSize = byteSizeOfCSI(val);
				res = instantiateClassindexableSize(((val < 0)
					 ? classLargeNegativeInteger()
					 : classLargePositiveInteger()), byteSize);
				if (!res) {
					primitiveFailFor(PrimErrNoMemory);
					firstLarge = null;
					goto l3;
				}
				pDigit = ((unsigned int *) (firstIndexableField(res)));
				digitSize = (byteSize + 3) / 4;
				for (ix = 1; ix <= digitSize; ix += 1) {
					/* begin cDigitOf:at:put: */
					aValue = ((unsigned int) (((usqInt) (((val < 0)
						 ? 0 - val
						 : val))) >> ((ix - 1) * 32)));
					pDigit[ix - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(aValue);
				}
				firstLarge = res;
				l3:
				;
				if (!firstLarge) {
					_return_value = null;
					goto l15;
				}
			}
			else {
				firstLarge = firstInteger;
			}
			result = instantiateClassindexableSize(classArray(), 2);
			if (result) {
				stObjectatput(result, 1, integerObjectOf(0));
				stObjectatput(result, 2, firstLarge);
			}
			_return_value = result;
			goto l15;
		}
		firstNeg = ((isIntegerObject(firstInteger))
			 ? (integerValueOf(firstInteger)) < 0
			 : isLargeNegativeIntegerObject(firstInteger));
		d = 32 - (cHighBit32(SQ_SWAP_4_BYTES_IF_BIGENDIAN(pSecond[secondDigitLen - 1])));
		/* begin digitBuffer:stack: */
		pDiv = (((secondDigitLen + 1) <= LIStackDigits)
			 ? divBuffer
			 : ((unsigned int *) (malloc((secondDigitLen + 1) * 4))));
		/* begin digitBuffer:stack: */
		pRem = (((firstDigitLen + 1) <= LIStackDigits)
			 ? remBuffer
			 : ((unsigned int *) (malloc((firstDigitLen + 1) * 4))));
		if ((pDiv == null) || (pRem == null)) {
			/* begin freeDigitBuffer:stack: */
			if (pDiv != divBuffer) {
				free(pDiv);
			}
			/* begin freeDigitBuffer:stack: */
			if (pRem != remBuffer) {
				free(pRem);
			}
			_return_value = primitiveFailFor(PrimErrNoMemory);
			goto l15;
		}
		/* begin cDigitLshift:from:len:into: */
		pDiv[secondDigitLen] = 0;
		if (d == 0) {
			/* begin cDigitCopyFrom:to:len: */
			for (i = 0; i < secondDigitLen; i += 1) {
				pDiv[i] = pSecond[i];
			}
		}
		else {
			cDigitLshiftfromlentolen(d, pSecond, secondDigitLen, pDiv, secondDigitLen + 1);
		}
		/* begin cDigitLshift:from:len:into: */
		pRem[firstDigitLen] = 0;
		if (d == 0) {
			/* begin cDigitCopyFrom:to:len: */
			for (i2 = 0; i2 < firstDigitLen; i2 += 1) {
				pRem[i2] = pFirst[i2];
			}
		}
		else {
			cDigitLshiftfromlentolen(d, pFirst, firstDigitLen, pRem, firstDigitLen + 1);
		}
		rem = null;
		/* begin createLargeIntegerNeg:digitLength: */
		/* begin createLargeIntegerNeg:byteLength: */
		quo = instantiateClassindexableSize(((neg)
			 ? classLargeNegativeInteger()
			 : classLargePositiveInteger()), quoDigitLen * 4);
		if (quo) {
			/* begin cDigitDiv:len:rem:len:quo:len: */
			pQuo = ((unsigned int *) (firstIndexableField(quo)));
			liDigitDivlenremlenquolen(pDiv, secondDigitLen + 1, pRem, firstDigitLen + 1, pQuo, quoDigitLen);
			/* begin digits:Rshift:lookfirst:negative: */
			/* begin cDigitHighBit:len: */
			realLength = secondDigitLen;
			do {
				if (realLength == 0) {
					oldBitLen = 0;
					goto l12;
				}
			} while (((lastDigit = cDigitOfat(pRem, (realLength -= 1)))) == 0);
			oldBitLen = (cHighBit32(lastDigit)) + (32 * realLength);
			l12:
			;
			oldDigitLen = (oldBitLen + 0x1F) / 32;
			newBitLen = oldBitLen - d;
			if (newBitLen <= 0) {
				/* begin createLargeIntegerNeg:byteLength: */
				rem = instantiateClassindexableSize(((firstNeg)
					 ? classLargeNegativeInteger()
					 : classLargePositiveInteger()), 0);
				goto l14;
			}
			newByteLen = (newBitLen + 7) / 8;
			newDigitLen = (newByteLen + 3) / 4;
			/* begin createLargeIntegerNeg:byteLength: */
			newOop = instantiateClassindexableSize(((firstNeg)
				 ? classLargeNegativeInteger()
				 : classLargePositiveInteger()), newByteLen);
			if (!newOop) {
				rem = null;
				goto l14;
			}
			cDigitRshiftfromlentolen(d, pRem, oldDigitLen, ((unsigned int *) (firstIndexableField(newOop))), newDigitLen);
			rem = newOop;
			l14:
			;
		}
		/* begin freeDigitBuffer:stack: */
		if (pDiv != divBuffer) {
			free(pDiv);
		}
		/* begin freeDigitBuffer:stack: */
		if (pRem != remBuffer) {
			free(pRem);
		}
		if ((quo == null) || (rem == null)) {
			_return_value = primitiveFailFor(PrimErrNoMemory);
			goto l15;
		}
		result = instantiateClassindexableSize(classArray(), 2);
		if (result) {
			stObjectatput(result, 1, quo);
			stObjectatput(result, 2, rem);
		}
		_return_value = result;
		l15:
		;
		if (!(failed())) {
			popthenPush(3, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primDigitMultiply:negative: */
EXPORT(sqInt)
primDigitMultiplyNegative(void)
{
	sqInt firstInteger;
	sqInt firstLen;
	unsigned int firstSmall[2];
	sqInt i;
	sqInt longLen;
	sqInt neg;
	sqInt negOop;
	unsigned int *pFirst;
	unsigned int *pLong;
	unsigned int *pProd;
	sqInt prod;
	unsigned int prodBuffer[LIStackDigits];
	unsigned int *pSecond;
	unsigned int *pShort;
	sqInt resDigitLen;
	sqInt secondInteger;
	sqInt secondLen;
	unsigned int secondSmall[2];
	sqInt shortLen;
	sqInt _return_value;

	secondInteger = stackValue(1);
	negOop = stackValue(0);
	if (!((methodArgumentCount()) == 2)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!((isKindOfInteger(secondInteger))
		 && (isBooleanObject(negOop)))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	neg = booleanValueOf(negOop);
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(2)));
	firstInteger = stackValue(2);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		/* begin digitMultiplyLarge:with:negative: */
		pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
		pSecond = pointerToDigitsOfsmallbyteSizeInto(secondInteger, secondSmall, &secondLen);
		resDigitLen = ((firstLen + secondLen) + 3) / 4;
		if (resDigitLen <= LIStackDigits) {
			prod = null;
			pProd = prodBuffer;
			/* begin cDigitZero:len: */
			for (i = 0; i < resDigitLen; i += 1) {
				pProd[i] = 0;
			}
		}
		else {
			/* begin createLargeIntegerNeg:byteLength: */
			prod = instantiateClassindexableSize(((neg)
				 ? classLargeNegativeInteger()
				 : classLargePositiveInteger()), firstLen + secondLen);
			if (!prod) {
				_return_value = primitiveFailFor(PrimErrNoMemory);
				goto l7;
			}
			pProd = ((unsigned int *) (firstIndexableField(prod)));
			if (isIntegerObject(firstInteger)) {
				pFirst = firstSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pFirst = ((unsigned int *) (firstIndexableField(firstInteger)));
			}
			if (isIntegerObject(secondInteger)) {
				pSecond = secondSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pSecond = ((unsigned int *) (firstIndexableField(secondInteger)));
			}
		}
		if (firstLen <= secondLen) {
			pShort = pFirst;
			shortLen = firstLen;
			pLong = pSecond;
			longLen = secondLen;
		}
		else {
			pShort = pSecond;
			shortLen = secondLen;
			pLong = pFirst;
			longLen = firstLen;
		}
		/* begin cDigitMultiply:len:with:len:into:len: */
		liDigitMultiplylenwithlenintolen(pShort, (shortLen + 3) / 4, pLong, (longLen + 3) / 4, pProd, resDigitLen);
		/* begin normalized:digits:len:negative: */
		if (!prod) {
			_return_value = normalizedIntegerFromlennegative(pProd, resDigitLen, neg);
			goto l7;
		}
		_return_value = ((neg)
			 ? normalizeNegative(prod)
			 : normalizePositive(prod));
		l7:
		;
		if (!(failed())) {
			popthenPush(3, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primDigitSubtract: */
EXPORT(sqInt)
primDigitSubtract(void)
{
	sqInt firstDigitLen;
	sqInt firstInteger;
	sqInt firstLen;
	sqInt firstNeg;
	unsigned int firstSmall[2];
	sqInt firstSmaller;
	sqInt largeDigitLen;
	sqInt neg;
	unsigned int *pFirst;
	unsigned int *pRes;
	unsigned int *pSecond;
	sqInt res;
	unsigned int resBuffer[LIStackDigits];
	sqInt secondDigitLen;
	sqInt secondInteger;
	sqInt secondLen;
	unsigned int secondSmall[2];
	sqInt smallerDigitLen;
	sqInt _return_value;

	secondInteger = stackValue(0);
	if (!((methodArgumentCount()) == 1)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!(isKindOfInteger(secondInteger))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(1)));
	firstInteger = stackValue(1);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		/* begin digitSubLarge:with: */
		pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
		pSecond = pointerToDigitsOfsmallbyteSizeInto(secondInteger, secondSmall, &secondLen);
		firstNeg = ((isIntegerObject(firstInteger))
			 ? (integerValueOf(firstInteger)) < 0
			 : isLargeNegativeIntegerObject(firstInteger));
		firstDigitLen = (firstLen + 3) / 4;
		secondDigitLen = (secondLen + 3) / 4;
		if (firstDigitLen == secondDigitLen) {
			while ((firstDigitLen > 1) && ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFirst[firstDigitLen - 1])) == (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pSecond[firstDigitLen - 1])))) {
				firstDigitLen -= 1;
			}
			secondDigitLen = firstDigitLen;
		}
		firstSmaller = (firstDigitLen < secondDigitLen) || ((firstDigitLen == secondDigitLen) && ((firstDigitLen > 0) && ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFirst[firstDigitLen - 1])) < (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pSecond[firstDigitLen - 1])))));
		if (firstSmaller) {
			largeDigitLen = secondDigitLen;
			smallerDigitLen = firstDigitLen;
			neg = firstNeg == 0;
		}
		else {
			largeDigitLen = firstDigitLen;
			smallerDigitLen = secondDigitLen;
			neg = firstNeg;
		}
		if (largeDigitLen <= LIStackDigits) {
			res = null;
			pRes = resBuffer;
		}
		else {
			/* begin createLargeIntegerNeg:digitLength: */
			/* begin createLargeIntegerNeg:byteLength: */
			res = instantiateClassindexableSize(((neg)
				 ? classLargeNegativeInteger()
				 : classLargePositiveInteger()), largeDigitLen * 4);
			if (!res) {
				_return_value = primitiveFailFor(PrimErrNoMemory);
				goto l5;
			}
			pRes = ((unsigned int *) (firstIndexableField(res)));
			if (isIntegerObject(firstInteger)) {
				pFirst = firstSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pFirst = ((unsigned int *) (firstIndexableField(firstInteger)));
			}
			if (isIntegerObject(secondInteger)) {
				pSecond = secondSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pSecond = ((unsigned int *) (firstIndexableField(secondInteger)));
			}
		}
		if (firstSmaller) {
			cDigitSublenwithleninto(pFirst, smallerDigitLen, pSecond, largeDigitLen, pRes);
		}
		else {
			cDigitSublenwithleninto(pSecond, smallerDigitLen, pFirst, largeDigitLen, pRes);
		}
		/* begin normalized:digits:len:negative: */
		if (!res) {
			_return_value = normalizedIntegerFromlennegative(pRes, largeDigitLen, neg);
			goto l5;
		}
		_return_value = ((neg)
			 ? normalizeNegative(res)
			 : normalizePositive(res));
		l5:
		;
		if (!(failed())) {
			popthenPush(2, _return_value);
		}
	}
	return null;
}


/*	If calling this primitive fails, then C module does not exist. */

	/* LargeIntegersPlugin>>#primGetModuleName */
EXPORT(sqInt)
primGetModuleName(void)
{
	sqInt strLen;
	sqInt strOop;

	if (!((methodArgumentCount()) == 0)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	/* missing DebugCode */;
	strLen = strlen(getModuleName());
	strOop = instantiateClassindexableSize(classString(), strLen);
	if (!strOop) {
		primitiveFailFor(PrimErrNoMemory);
		return null;
	}
	strncpy(firstIndexableField(strOop), getModuleName(), strLen);
	if (!(failed())) {
		popthenPush(1, strOop);
	}
	return null;
}

	/* LargeIntegersPlugin>>#primMontgomeryDigitLength */
EXPORT(sqInt)
primMontgomeryDigitLength(void)
{
	sqInt _return_value;

	if (!((methodArgumentCount()) == 0)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	/* missing DebugCode */;
	if (!(failed())) {
		_return_value = integerObjectOf(32);
		if (!(failed())) {
			popthenPush(1, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primMontgomeryTimes:modulo:mInvModB: */
EXPORT(sqInt)
primMontgomeryTimesModulo(void)
{
	sqInt firstDigitLen;
	sqInt firstInteger;
	sqInt firstLen;
	unsigned int firstSmall[2];
	sqInt i;
	unsigned int mInv;
	sqInt mInverseInteger;
	unsigned int *pFirst;
	unsigned int *pRes;
	sqInt prod;
	unsigned int *pSecond;
	unsigned int *pThird;
	unsigned int resBuffer[LIStackDigits];
	sqInt secondDigitLen;
	sqInt secondLen;
	sqInt secondOperandInteger;
	unsigned int secondSmall[2];
	sqInt thirdDigitLen;
	sqInt thirdLen;
	sqInt thirdModuloInteger;
	unsigned int thirdSmall[2];
	sqInt _return_value;

	secondOperandInteger = stackValue(2);
	thirdModuloInteger = stackValue(1);
	mInverseInteger = stackValue(0);
	if (!((methodArgumentCount()) == 3)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	if (!((isKindOfInteger(secondOperandInteger))
		 && ((isKindOfInteger(thirdModuloInteger))
		 && (isKindOfInteger(mInverseInteger))))) {
		primitiveFailFor(PrimErrBadArgument);
		return null;
	}
	/* missing DebugCode */;
	success(isKindOfInteger(stackValue(3)));
	firstInteger = stackValue(3);
	if (failed()) {
		return null;
	}
	mInv = positive32BitValueOf(mInverseInteger);
	if (!(failed())) {
		/* begin digitMontgomery:times:modulo:mInvModB: */
		pFirst = pointerToDigitsOfsmallbyteSizeInto(firstInteger, firstSmall, &firstLen);
		pSecond = pointerToDigitsOfsmallbyteSizeInto(secondOperandInteger, secondSmall, &secondLen);
		pThird = pointerToDigitsOfsmallbyteSizeInto(thirdModuloInteger, thirdSmall, &thirdLen);
		firstDigitLen = (firstLen + 3) / 4;
		secondDigitLen = (secondLen + 3) / 4;
		thirdDigitLen = (thirdLen + 3) / 4;
		if (!((firstDigitLen <= thirdDigitLen) && (secondDigitLen <= thirdDigitLen))) {
			_return_value = primitiveFail();
			goto l7;
		}
		if ((firstDigitLen > 0) && (secondDigitLen == 0)) {
			_return_value = primitiveFail();
			goto l7;
		}
		if (thirdDigitLen <= LIStackDigits) {
			prod = null;
			pRes = resBuffer;
			/* begin cDigitZero:len: */
			for (i = 0; i < thirdDigitLen; i += 1) {
				pRes[i] = 0;
			}
		}
		else {
			prod = instantiateClassindexableSize(classLargePositiveInteger(), thirdDigitLen * 4);
			if (!prod) {
				_return_value = primitiveFailFor(PrimErrNoMemory);
				goto l7;
			}
			pRes = ((unsigned int *) (firstIndexableField(prod)));
			if (isIntegerObject(firstInteger)) {
				pFirst = firstSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pFirst = ((unsigned int *) (firstIndexableField(firstInteger)));
			}
			if (isIntegerObject(secondOperandInteger)) {
				pSecond = secondSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pSecond = ((unsigned int *) (firstIndexableField(secondOperandInteger)));
			}
			if (isIntegerObject(thirdModuloInteger)) {
				pThird = thirdSmall;
			}
			else {
				/* begin pointerToFirstDigitOfLargeInt: */
				pThird = ((unsigned int *) (firstIndexableField(thirdModuloInteger)));
			}
		}
		/* begin cDigitMontgomery:len:times:len:modulo:len:mInvModB:into: */
		liDigitMontgomerylentimeslenmodulolenmInvModBinto(pFirst, firstDigitLen, pSecond, secondDigitLen, pThird, thirdDigitLen, mInv, pRes);
		/* begin normalized:digits:len:negative: */
		if (!prod) {
			_return_value = normalizedIntegerFromlennegative(pRes, thirdDigitLen, 0);
			goto l7;
		}
		_return_value = normalizePositive(prod);
		l7:
		;
		if (!(failed())) {
			popthenPush(4, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primNormalizeNegative */
EXPORT(sqInt)
primNormalizeNegative(void)
{
	sqInt rcvr;
	sqInt _return_value;

	if (!((methodArgumentCount()) == 0)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	/* missing DebugCode */;
	success(isLargeNegativeIntegerObject(stackValue(0)));
	rcvr = stackValue(0);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		_return_value = normalizeNegative(rcvr);
		if (!(failed())) {
			popthenPush(1, _return_value);
		}
	}
	return null;
}

	/* LargeIntegersPlugin>>#primNormalizePositive */
EXPORT(sqInt)
primNormalizePositive(void)
{
	sqInt rcvr;
	sqInt _return_value;

	if (!((methodArgumentCount()) == 0)) {
		primitiveFailFor(PrimErrBadNumArgs);
		return null;
	}
	/* missing DebugCode */;
	success(isLargePositiveIntegerObject(stackValue(0)));
	rcvr = stackValue(0);
	if (failed()) {
		return null;
	}
	if (!(failed())) {
		_return_value = normalizePositive(rcvr);
		if (!(failed())) {
			popthenPush(1, _return_value);
		}
	}
	return null;
}


/*	Note: This is coded so that it can be run in Squeak. */

	/* InterpreterPlugin>>#setInterpreter: */
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	sqInt ok;

	interpreterProxy = anInterpreter;
	ok = ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR))
	 && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));
	if (ok) {
		
#if !defined(SQUEAK_BUILTIN_PLUGIN)
		booleanValueOf = interpreterProxy->booleanValueOf;
		classArray = interpreterProxy->classArray;
		classLargeNegativeInteger = interpreterProxy->classLargeNegativeInteger;
		classLargePositiveInteger = interpreterProxy->classLargePositiveInteger;
		classString = interpreterProxy->classString;
		failed = interpreterProxy->failed;
		falseObject = interpreterProxy->falseObject;
		fetchClassOf = interpreterProxy->fetchClassOf;
		firstIndexableField = interpreterProxy->firstIndexableField;
		instantiateClassindexableSize = interpreterProxy->instantiateClassindexableSize;
		integerObjectOf = interpreterProxy->integerObjectOf;
		integerValueOf = interpreterProxy->integerValueOf;
#if VM_PROXY_MAJOR > 1 || (VM_PROXY_MAJOR == 1 && VM_PROXY_MINOR >= 15)
		isBooleanObject = interpreterProxy->isBooleanObject;
#else
#if !defined(isBooleanObject)
		isBooleanObject = 0;
#endif
#endif
		isIntegerObject = interpreterProxy->isIntegerObject;
		methodArgumentCount = interpreterProxy->methodArgumentCount;
		popthenPush = interpreterProxy->popthenPush;
		positive32BitValueOf = interpreterProxy->positive32BitValueOf;
		primitiveFail = interpreterProxy->primitiveFail;
		primitiveFailFor = interpreterProxy->primitiveFailFor;
		slotSizeOf = interpreterProxy->slotSizeOf;
		stObjectatput = interpreterProxy->stObjectatput;
		stackIntegerValue = interpreterProxy->stackIntegerValue;
		stackValue = interpreterProxy->stackValue;
		success = interpreterProxy->success;
		trueObject = interpreterProxy->trueObject;
#endif /* !defined(SQUEAK_BUILTIN_PLUGIN) */
	}
	return ok;
}

/*	Argument bytesObj must not be aSmallInteger! */

	/* LargeIntegersPlugin>>#unsafeByteOfLargeInt:at: */
static unsigned char
unsafeByteOfLargeIntat(sqInt bytesObj, sqInt ix)
{
	return (((unsigned char *) (firstIndexableField(bytesObj))))[ix - 1];
}


#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "LargeIntegers";
void* LargeIntegers_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "primAnyBitFromTo\000\002", (void*)primAnyBitFromTo},
	{(void*)_m, "primDigitAdd\000\001", (void*)primDigitAdd},
	{(void*)_m, "primDigitBitAnd\000\001", (void*)primDigitBitAnd},
	{(void*)_m, "primDigitBitOr\000\001", (void*)primDigitBitOr},
	{(void*)_m, "primDigitBitShiftMagnitude\000\002", (void*)primDigitBitShiftMagnitude},
	{(void*)_m, "primDigitBitXor\000\001", (void*)primDigitBitXor},
	{(void*)_m, "primDigitCompare\000\000", (void*)primDigitCompare},
	{(void*)_m, "primDigitDivNegative\000\001", (void*)primDigitDivNegative},
	{(void*)_m, "primDigitMultiplyNegative\000\001", (void*)primDigitMultiplyNegative},
	{(void*)_m, "primDigitSubtract\000\001", (void*)primDigitSubtract},
	{(void*)_m, "primGetModuleName\000\377", (void*)primGetModuleName},
	{(void*)_m, "primMontgomeryDigitLength\000\377", (void*)primMontgomeryDigitLength},
	{(void*)_m, "primMontgomeryTimesModulo\000\001", (void*)primMontgomeryTimesModulo},
	{(void*)_m, "primNormalizeNegative\000\001", (void*)primNormalizeNegative},
	{(void*)_m, "primNormalizePositive\000\001", (void*)primNormalizePositive},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{NULL, NULL, NULL}
};

#else /* ifdef SQ_BUILTIN_PLUGIN */

EXPORT(signed char) primAnyBitFromToAccessorDepth = 2;
EXPORT(signed char) primDigitAddAccessorDepth = 1;
EXPORT(signed char) primDigitBitAndAccessorDepth = 1;
EXPORT(signed char) primDigitBitOrAccessorDepth = 1;
EXPORT(signed char) primDigitBitShiftMagnitudeAccessorDepth = 2;
EXPORT(signed char) primDigitBitXorAccessorDepth = 1;
EXPORT(signed char) primDigitCompareAccessorDepth = 0;
EXPORT(signed char) primDigitDivNegativeAccessorDepth = 1;
EXPORT(signed char) primDigitMultiplyNegativeAccessorDepth = 1;
EXPORT(signed char) primDigitSubtractAccessorDepth = 1;
EXPORT(signed char) primMontgomeryTimesModuloAccessorDepth = 1;
EXPORT(signed char) primNormalizeNegativeAccessorDepth = 1;
EXPORT(signed char) primNormalizePositiveAccessorDepth = 1;

#endif /* ifdef SQ_BUILTIN_PLUGIN */
