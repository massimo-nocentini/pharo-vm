/* LargeIntegersKernels.h: the multiply and divide kernels of the LargeIntegers plugin.
 *
 * Hand-written C (src/common/LargeIntegersKernels.c) called by LargeIntegers.c, which is
 * generated from LargeIntegersPlugin.class.st by Slang and then adapted by hand (see the
 * header of LargeIntegers.c).  Both work on the plugin's 32-bit digits: little-endian
 * digits of a LargeInteger's bytes, each one stored through SQ_SWAP_4_BYTES_IF_BIGENDIAN.
 * Include after the VM headers that define sqInt.
 */
#ifndef LARGE_INTEGERS_KERNELS_H
#define LARGE_INTEGERS_KERNELS_H

/* Slang spells a send of liDigitMultiply:len:with:len:into:len: and of
   liDigitDiv:len:rem:len:quo:len: (LargeIntegersPlugin) as these function names. */
#define liDigitMultiplylenwithlenintolen liDigitMultiply
#define liDigitDivlenremlenquolen liDigitDiv

/* Not exported from the plugin library, where the VM would find them as primitives. */
#if defined(__GNUC__) && !defined(_WIN32)
# define LI_KERNEL __attribute__((visibility("hidden")))
#else
# define LI_KERNEL
#endif

/* Store into the resLen digits at pWordRes the low resLen digits of the product of the
   shortLen digits at pWordShort and the longLen digits at pWordLong.  Every result digit
   is written, whatever it held before.  The operands may be the same digits; the result
   must not overlap them.  This is what cDigitMultiply:len:with:len:into:len: computes
   into digits that start out all zero.  Never fails: when scratch memory cannot be
   allocated, the 32-bit schoolbook loop runs instead. */
LI_KERNEL void liDigitMultiply(unsigned int *pWordShort, sqInt shortLen, unsigned int *pWordLong, sqInt longLen,
				unsigned int *pWordRes, sqInt resLen);

/* Knuth's algorithm D, as cDigitDiv:len:rem:len:quo:len: runs it.  pDiv holds divLen
   digits: the divisor shifted left until the top bit of its digit divLen - 2 is set, and
   a zero digit divLen - 1.  pRem holds the remLen digits of the dividend shifted left by
   the same count.  Stores the quoLen = remLen - divLen + 1 digits of the quotient at
   pQuo, and leaves the shifted remainder in the low divLen - 1 digits of pRem, with
   zeros above.  Never fails: other operand shapes, and a failed scratch allocation, run
   the 32-bit loop of cDigitDiv:len:rem:len:quo:len: instead. */
LI_KERNEL void liDigitDiv(unsigned int *pDiv, sqInt divLen, unsigned int *pRem, sqInt remLen,
				unsigned int *pQuo, sqInt quoLen);

#endif /* LARGE_INTEGERS_KERNELS_H */
