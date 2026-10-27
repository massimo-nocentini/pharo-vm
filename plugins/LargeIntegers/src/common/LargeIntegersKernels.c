/* LargeIntegersKernels.c: the multiply and divide kernels of the LargeIntegers plugin.
 *
 * LargeIntegers.c (generated from LargeIntegersPlugin.class.st by Slang, then adapted by
 * hand: see its header) calls liDigitMultiply()
 * and liDigitDiv() where the plugin used to run its schoolbook loops over 32-bit digits;
 * see LargeIntegersKernels.h for what each one computes.  The answers are digit for digit
 * those of the loops: quotient and remainder digits are unique, and so are the low resLen
 * digits of a product.
 *
 * Where the C compiler has 128-bit arithmetic on a 64-bit host, the kernels compute over
 * 64-bit limbs (two digits each), else over 32-bit limbs (one digit each):
 * - multiplication: schoolbook, and Karatsuba once the shorter operand has
 *   LI_KARATSUBA_LIMBS limbs (an unbalanced product is cut into pieces of the shorter
 *   operand's length); a short operand of one 64-bit limb streams over the digits;
 * - squaring, when both operands are the same digits (one pointer, as for x * x, or
 *   equal contents) of at least LI_SQR_BASECASE_LIMBS limbs: schoolbook squaring, each
 *   cross product once, and Karatsuba squaring (three half-size squares) from
 *   LI_SQR_KARATSUBA_LIMBS limbs on; 0.6 to 0.9 of the time of the product;
 * - division: Knuth's algorithm D, each quotient limb estimated with the Moller-Granlund
 *   2-by-1 division by a precomputed reciprocal of the top divisor limb, then corrected
 *   with the next divisor limb as Knuth does; a single-limb divisor takes a plain loop.
 *   Once the divisor and the quotient both have LI_BZ_LIMBS limbs, Burnikel & Ziegler's
 *   recursive division in GMP's divide-and-conquer shape (mpn_dcpi1_div_qr): the quotient
 *   in blocks of the divisor's length, top block first, each block's halves estimated
 *   from the top half of the divisor (recursively, down to algorithm D) and corrected
 *   with the product of the rest (Karatsuba) and at most two add-backs.  A quotient of
 *   LI_BZ_QUOTIENT_LIMBS limbs or more over a divisor at least that much longer is one
 *   such block: algorithm D on the top of the divisor, then the product of the rest.
 * The operands are copied into limbs in scratch memory (on the stack up to
 * LI_STACK_LIMBS limbs, else malloc'ed) and the results copied back as digits, so a
 * kernel reads and writes exactly the digits it is given.
 * The original 32-bit loops (liDigitMultiply32/liDigitDiv32) still run in place where
 * they are faster, for the smallest operands (LI_MUL_LOOP_DIGIT_PRODUCT,
 * LI_DIV_LOOP_QUOTIENT_DIGITS), and when the malloc fails: no new failure.
 *
 * Compile-time knobs, for tests and tuning: LI_LIMB_BITS (64 or 32), the thresholds
 * above, LI_STACK_LIMBS, LI_MALLOC/LI_FREE (the kernels never ask for more than a size_t
 * holds: the 32-bit loop runs instead), LI_TUNABLE (makes the thresholds variables),
 * LI_DIV_ADDBACK (counts the add-backs of the recursive division).
 */

#include "config.h"
#include <stdlib.h>
#include <string.h>

#include "sqConfig.h"
#include "sqPlatformSpecific.h"
#include "memoryAccess.h"	/* sqInt, SQ_SWAP_4_BYTES_IF_BIGENDIAN */

#include "LargeIntegersKernels.h"

#if !defined(LI_LIMB_BITS)
# if defined(__SIZEOF_INT128__) && !defined(__wasm__) && (defined(__x86_64__) || defined(__aarch64__))
#  define LI_LIMB_BITS 64
# else
#  define LI_LIMB_BITS 32
# endif
#endif

#if LI_LIMB_BITS == 64
typedef unsigned long long liLimb;
typedef unsigned __int128 liDLimb;
# define LI_DIGITS_PER_LIMB 2
#elif LI_LIMB_BITS == 32
typedef unsigned int liLimb;
typedef unsigned long long liDLimb;
# define LI_DIGITS_PER_LIMB 1
#else
# error "LI_LIMB_BITS must be 64 or 32"
#endif
#define LI_LB LI_LIMB_BITS

/* The thresholds were measured on an Intel Xeon Gold 6238R (x86_64) with clang 18 -O2;
   the 32-bit limb ones on the same host with LI_LIMB_BITS=32.
   Karatsuba from this many limbs of the shorter operand on, schoolbook below. */
#if !defined(LI_KARATSUBA_LIMBS)
# if LI_LIMB_BITS == 64
#  define LI_KARATSUBA_LIMBS 24
# else
#  define LI_KARATSUBA_LIMBS 32
# endif
#endif
#if LI_KARATSUBA_LIMBS < 2
# error "LI_KARATSUBA_LIMBS must be at least 2"
#endif
/* The 32-bit multiply loop for products of at most this many digit pairs (64-bit limbs; a
   one-limb short operand streams instead).  With 32-bit limbs, below Karatsuba. */
#if !defined(LI_MUL_LOOP_DIGIT_PRODUCT)
# define LI_MUL_LOOP_DIGIT_PRODUCT 25
#endif
/* A square (the two operands the same digits) of at least LI_SQR_BASECASE_LIMBS limbs
   takes the squaring kernels: schoolbook squaring, and Karatsuba squaring from
   LI_SQR_KARATSUBA_LIMBS limbs on.  Smaller squares are multiplied as any product.
   Schoolbook squaring does half the limb products of schoolbook multiplication, so
   Karatsuba overtakes it at a larger size.  With 32-bit limbs, the threshold of the
   wasm build (node 25, emcc -O2): below 11 digits the 32-bit loop is as fast, and
   cheaper than comparing two copies of the digits. */
#if !defined(LI_SQR_BASECASE_LIMBS)
# if LI_LIMB_BITS == 64
#  define LI_SQR_BASECASE_LIMBS 3
# else
#  define LI_SQR_BASECASE_LIMBS 11
# endif
#endif
#if !defined(LI_SQR_KARATSUBA_LIMBS)
# if LI_LIMB_BITS == 64
#  define LI_SQR_KARATSUBA_LIMBS 44
# else
#  define LI_SQR_KARATSUBA_LIMBS 48
# endif
#endif
#if LI_SQR_KARATSUBA_LIMBS < 2 || LI_SQR_BASECASE_LIMBS < 1
# error "LI_SQR_KARATSUBA_LIMBS must be at least 2, LI_SQR_BASECASE_LIMBS at least 1"
#endif
/* The 32-bit division loop for quotients of at most this many digits by divisors of fewer
   than LI_DIV_LOOP_DIVISOR_DIGITS digits: it needs no reciprocal and no copies. */
#if !defined(LI_DIV_LOOP_QUOTIENT_DIGITS)
# if LI_LIMB_BITS == 64
#  define LI_DIV_LOOP_QUOTIENT_DIGITS 1
# else
#  define LI_DIV_LOOP_QUOTIENT_DIGITS 2
# endif
#endif
#if !defined(LI_DIV_LOOP_DIVISOR_DIGITS)
# define LI_DIV_LOOP_DIVISOR_DIGITS 24
#endif
/* Burnikel & Ziegler's recursive division when the divisor and the quotient both have at
   least this many limbs, and in its recursion down to this many; Knuth's algorithm D below. */
#if !defined(LI_BZ_LIMBS)
# if LI_LIMB_BITS == 64
#  define LI_BZ_LIMBS 48
# else
#  define LI_BZ_LIMBS 32
# endif
#endif
#if LI_BZ_LIMBS < 2
# error "LI_BZ_LIMBS must be at least 2"
#endif
/* ... and when the divisor has LI_BZ_LIMBS limbs and the quotient fewer, but at least this
   many, with at least this many more limbs in the divisor: algorithm D on the top of the
   divisor, then the product of the quotient with the rest of the divisor.  With 64-bit
   limbs that product takes Karatsuba from here; with 32-bit limbs even its schoolbook is
   faster than the multiply-and-subtract rows of algorithm D. */
#if !defined(LI_BZ_QUOTIENT_LIMBS)
# if LI_LIMB_BITS == 64
#  define LI_BZ_QUOTIENT_LIMBS 24
# else
#  define LI_BZ_QUOTIENT_LIMBS 12
# endif
#endif
#if LI_BZ_QUOTIENT_LIMBS < 1
# error "LI_BZ_QUOTIENT_LIMBS must be at least 1"
#endif

#if defined(LI_TUNABLE)
sqInt liKaratsubaLimbs = LI_KARATSUBA_LIMBS;
sqInt liMulLoopDigitProduct = LI_MUL_LOOP_DIGIT_PRODUCT;
sqInt liSqrBasecaseLimbs = LI_SQR_BASECASE_LIMBS;
sqInt liSqrKaratsubaLimbs = LI_SQR_KARATSUBA_LIMBS;
sqInt liDivLoopQuotientDigits = LI_DIV_LOOP_QUOTIENT_DIGITS;
sqInt liBZLimbs = LI_BZ_LIMBS;
sqInt liBZQuotientLimbs = LI_BZ_QUOTIENT_LIMBS;
#else
# define liKaratsubaLimbs LI_KARATSUBA_LIMBS
# define liMulLoopDigitProduct LI_MUL_LOOP_DIGIT_PRODUCT
# define liSqrBasecaseLimbs LI_SQR_BASECASE_LIMBS
# define liSqrKaratsubaLimbs LI_SQR_KARATSUBA_LIMBS
# define liDivLoopQuotientDigits LI_DIV_LOOP_QUOTIENT_DIGITS
# define liBZLimbs LI_BZ_LIMBS
# define liBZQuotientLimbs LI_BZ_QUOTIENT_LIMBS
#endif

/* Scratch limbs on the C stack; more are malloc'ed. */
#if !defined(LI_STACK_LIMBS)
# define LI_STACK_LIMBS (4096 / LI_DIGITS_PER_LIMB / 4)
#endif
#if !defined(LI_MALLOC)
# define LI_MALLOC malloc
# define LI_FREE free
#endif

#define DIGIT(x) ((unsigned int) SQ_SWAP_4_BYTES_IF_BIGENDIAN(x))

static sqInt
liLimbsFor(sqInt digits)
{
	return (digits + LI_DIGITS_PER_LIMB - 1) / LI_DIGITS_PER_LIMB;
}

/* Answer need scratch limbs: stackLimbs (LI_STACK_LIMBS of them) when they are enough, else
   malloc'ed ones, or 0 when the malloc fails or need limbs are more bytes than a size_t
   holds (with a 32-bit size_t, operands of a few hundred MB). */
static liLimb *
liScratch(sqInt need, liLimb *stackLimbs)
{
	if (need <= LI_STACK_LIMBS) {
		return stackLimbs;
	}
	if ((unsigned long long) need > (unsigned long long) (((size_t) -1) / sizeof(liLimb))) {
		return 0;
	}
	return (liLimb *) LI_MALLOC((size_t) need * sizeof(liLimb));
}


/*** The original 32-bit loops: for the smallest operands, and the fallback ***/

/* cDigitMultiply:len:with:len:into:len:, into result digits it zeroes first */
static void
liDigitMultiply32(unsigned int *pWordShort, sqInt shortLen, unsigned int *pWordLong, sqInt longLen,
		unsigned int *pWordRes, sqInt resLen)
{
	unsigned long long ab;
	unsigned int carry;
	unsigned int digit;
	sqInt i, j, k;

	for (i = 0; i < resLen; i += 1) {
		pWordRes[i] = 0;
	}
	if ((shortLen == 1) && ((pWordShort[0]) == 0)) {
		return;
	}
	if ((longLen == 1) && ((pWordLong[0]) == 0)) {
		return;
	}
	for (i = 0; i < shortLen; i += 1) {
		if (((digit = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordShort[i]))) != 0) {
			k = i;
			carry = 0;
			for (j = 0; j < longLen; j += 1) {
				ab = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordLong[j]);
				ab = ((ab * digit) + carry) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pWordRes[k]));
				carry = ab >> 32;
				pWordRes[k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(ab & 0xFFFFFFFFU);
				k += 1;
			}
			if (k < resLen) {
				pWordRes[k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(carry);
			}
		}
	}
}

/* cDigitDiv:len:rem:len:quo:len: */
static void
liDigitDiv32(unsigned int *pDiv, sqInt divLen, unsigned int *pRem, sqInt remLen, unsigned int *pQuo, sqInt quoLen)
{
	unsigned long long a, b, hi, lo, mul, q, r1r2, t;
	int cond;
	unsigned int dh, dnh, r3;
	sqInt dl, i, j, k, l;

	dl = divLen - 1;
	dh = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[dl - 1]);
	dnh = (dl == 1) ? 0 : SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[dl - 2]);
	for (k = 1; k <= quoLen; k += 1) {
		j = (remLen + 1) - k;
		if ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[j - 1])) == dh) {
			q = 0xFFFFFFFFU;
		}
		else {
			r1r2 = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[j - 1]);
			r1r2 = (r1r2 << 32) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[j - 2]));
			t = r1r2 % dh;
			q = r1r2 / dh;
			mul = q * dnh;
			hi = mul >> 32;
			lo = mul & 0xFFFFFFFFU;
			r3 = (j < 3) ? 0 : SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[j - 3]);
			while (1) {
				if ((t < hi) || ((t == hi) && (r3 < lo))) {
					q -= 1;
					if (hi == 0) {
						cond = 0;
					}
					else {
						if (lo < dnh) {
							hi -= 1;
							lo = (lo + 0x100000000LL) - dnh;
						}
						else {
							lo -= dnh;
						}
						cond = hi >= dh;
					}
				}
				else {
					cond = 0;
				}
				if (!cond)
					break;
				hi -= dh;
			}
		}
		l = j - dl;
		a = 0;
		for (i = 1; i <= divLen; i += 1) {
			hi = (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[i - 1])) * (q >> 32);
			lo = (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[i - 1])) * (q & 0xFFFFFFFFU);
			b = ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[l - 1])) - a) - (lo & 0xFFFFFFFFU);
			pRem[l - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(b & 0xFFFFFFFFU);
			b = (b >> 32) | ((0 - (b >> 0x3F)) & 0xFFFFFFFF00000000ULL);
			a = (hi + (lo >> 32)) - b;
			l += 1;
		}
		if (a > 0) {
			q -= 1;
			l = j - dl;
			a = 0;
			for (i = 1; i <= divLen; i += 1) {
				a = ((a >> 32) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[l - 1]))) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[i - 1]));
				pRem[l - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(a & 0xFFFFFFFFU);
				l += 1;
			}
		}
		pQuo[quoLen - k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) q));
	}
}


/*** Digits <-> limbs ***/

/* dst[0 .. liLimbsFor(skip + digits)) := skip zero digits followed by the digits at src
   (skip is 0 or 1; 1 only with 64-bit limbs). */
static void
liLoad(liLimb *dst, const unsigned int *src, sqInt digits, sqInt skip)
{
	sqInt i;
#if LI_LIMB_BITS == 64
	sqInt n = skip + digits;
	if (skip == 0) {
		for (i = 0; i + 1 < n; i += 2) {
			dst[i / 2] = (liLimb) DIGIT(src[i]) | ((liLimb) DIGIT(src[i + 1]) << 32);
		}
		if (n & 1) {
			dst[n / 2] = DIGIT(src[n - 1]);
		}
	}
	else {
		dst[0] = n > 1 ? (liLimb) DIGIT(src[0]) << 32 : 0;
		for (i = 2; i + 1 < n; i += 2) {
			dst[i / 2] = (liLimb) DIGIT(src[i - 1]) | ((liLimb) DIGIT(src[i]) << 32);
		}
		if ((n & 1) && n > 1) {
			dst[n / 2] = DIGIT(src[n - 2]);
		}
	}
#else
	(void) skip;
	for (i = 0; i < digits; i += 1) {
		dst[i] = DIGIT(src[i]);
	}
#endif
}

/* dst[0 .. digits) := the digits skip, skip + 1, ... of the srcLimbs limbs at src, and
   zeros past their end. */
static void
liStore(unsigned int *dst, sqInt digits, const liLimb *src, sqInt srcLimbs, sqInt skip)
{
	sqInt i, have;

	have = srcLimbs * LI_DIGITS_PER_LIMB - skip;
	if (have > digits) {
		have = digits;
	}
	i = 0;
#if LI_LIMB_BITS == 64
	if (skip == 0) {
		for (; i + 1 < have; i += 2) {
			dst[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) src[i / 2]);
			dst[i + 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) (src[i / 2] >> 32));
		}
		if (i < have) {
			dst[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) src[i / 2]);
			i += 1;
		}
	}
	else if (have > 0) {
		dst[0] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) (src[0] >> 32));
		for (i = 1; i + 1 < have; i += 2) {
			dst[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) src[(i + 1) / 2]);
			dst[i + 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) (src[(i + 1) / 2] >> 32));
		}
		if (i < have) {
			dst[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) src[(i + 1) / 2]);
			i += 1;
		}
	}
#else
	for (; i < have; i += 1) {
		dst[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(src[i + skip]);
	}
#endif
	for (; i < digits; i += 1) {
		dst[i] = 0;
	}
}


/*** Limb vector primitives ***/

/* r[0 .. n) := a * b, answer the high limb */
static liLimb
liMul1(liLimb *r, const liLimb *a, sqInt n, liLimb b)
{
	liLimb carry = 0;
	liDLimb t;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		t = (liDLimb) a[i] * b + carry;
		r[i] = (liLimb) t;
		carry = (liLimb) (t >> LI_LB);
	}
	return carry;
}

/* r[0 .. n) += a * b, answer the carry limb */
static liLimb
liAddMul1(liLimb *r, const liLimb *a, sqInt n, liLimb b)
{
	liLimb carry = 0;
	liDLimb t;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		t = (liDLimb) a[i] * b + r[i] + carry;
		r[i] = (liLimb) t;
		carry = (liLimb) (t >> LI_LB);
	}
	return carry;
}

/* r[0 .. n) -= a * b, answer the borrow limb */
static liLimb
liSubMul1(liLimb *r, const liLimb *a, sqInt n, liLimb b)
{
	liLimb carry = 0, pl, ri;
	liDLimb p;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		p = (liDLimb) a[i] * b + carry;
		pl = (liLimb) p;
		ri = r[i];
		carry = (liLimb) (p >> LI_LB) + (ri < pl);
		r[i] = ri - pl;
	}
	return carry;
}

/* r[0 .. n) := a + b, answer the carry (r may be a or b) */
static liLimb
liAddN(liLimb *r, const liLimb *a, const liLimb *b, sqInt n)
{
	liLimb c = 0, s, t;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		s = a[i] + c;
		c = s < c;
		t = s + b[i];
		c += t < s;
		r[i] = t;
	}
	return c;
}

/* r[0 .. n) := a - b, answer the borrow (r may be a or b) */
static liLimb
liSubN(liLimb *r, const liLimb *a, const liLimb *b, sqInt n)
{
	liLimb c = 0, ai, bi;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		ai = a[i];
		bi = b[i] + c;
		c = bi < c;
		c += ai < bi;
		r[i] = ai - bi;
	}
	return c;
}

/* r[0 .. n) := a[0 .. n) + c, answer the carry (r may be a) */
static liLimb
liAdd1(liLimb *r, const liLimb *a, sqInt n, liLimb c)
{
	sqInt i;

	for (i = 0; i < n; i += 1) {
		r[i] = a[i] + c;
		c = r[i] < c;
	}
	return c;
}


/*** Multiplication ***/

static void liMul(liLimb *r, const liLimb *a, sqInt an, const liLimb *b, sqInt bn, liLimb *scratch);

/* The scratch limbs liMul(r, a, an, b, bn, scratch) uses; an >= bn >= 1. */
static sqInt
liMulScratch(sqInt an, sqInt bn)
{
	sqInt h, need, other;

	if (bn < liKaratsubaLimbs) {
		return 0;
	}
	if (bn <= (an + 1) / 2) {
		need = liMulScratch(bn, bn);
		if (an % bn != 0 && (other = liMulScratch(bn, an % bn)) > need) {
			need = other;
		}
		return 2 * bn + need;
	}
	h = (an + 1) / 2;
	need = liMulScratch(h, h);
	if ((other = liMulScratch(an - h, bn - h)) > need) {
		need = other;
	}
	if (2 * h + 1 > need) {
		need = 2 * h + 1;
	}
	return 4 * h + need;
}

/* r[0 .. an + bn) := a * b by schoolbook; an >= bn >= 1 */
static void
liMulBasecase(liLimb *r, const liLimb *a, sqInt an, const liLimb *b, sqInt bn)
{
	sqInt i;

	r[an] = liMul1(r, a, an, b[0]);
	for (i = 1; i < bn; i += 1) {
		r[an + i] = liAddMul1(r + i, a, an, b[i]);
	}
}

/* d[0 .. xn) := |x - y|, where y has yn <= xn limbs; answer whether x < y */
static int
liAbsDiff(liLimb *d, const liLimb *x, sqInt xn, const liLimb *y, sqInt yn)
{
	sqInt i;
	int less;

	for (i = xn - 1; i >= yn && x[i] == 0; i -= 1) ;
	if (i >= yn) {
		less = 0;
	}
	else {
		while (i >= 0 && x[i] == y[i]) {
			i -= 1;
		}
		less = i >= 0 && x[i] < y[i];
	}
	if (less) {
		/* then x[yn .. xn) is zero */
		liSubN(d, y, x, yn);
		for (i = yn; i < xn; i += 1) {
			d[i] = 0;
		}
	}
	else {
		liLimb c = liSubN(d, x, y, yn);
		for (i = yn; i < xn; i += 1) {
			d[i] = x[i] - c;
			c = x[i] < c;
		}
	}
	return less;
}

/* Karatsuba, bn > (an + 1) / 2: with h = (an + 1) / 2, a = a1 B^h + a0, b = b1 B^h + b0,
   a * b = z2 B^2h + (z0 + z2 - (a0 - a1)(b0 - b1)) B^h + z0, z0 = a0 b0, z2 = a1 b1 */
static void
liMulKaratsuba(liLimb *r, const liLimb *a, sqInt an, const liLimb *b, sqInt bn, liLimb *scratch)
{
	sqInt h = (an + 1) / 2, a1n = an - h, b1n = bn - h, z2n = a1n + b1n, tn;
	liLimb *da = scratch, *db = scratch + h, *zm = scratch + 2 * h, *t = scratch + 4 * h;
	liLimb c;
	int negA, negB;

	negA = liAbsDiff(da, a, h, a + h, a1n);
	negB = liAbsDiff(db, b, h, b + h, b1n);
	liMul(zm, da, h, db, h, scratch + 4 * h);
	liMul(r, a, h, b, h, scratch + 4 * h);
	liMul(r + 2 * h, a + h, a1n, b + h, b1n, scratch + 4 * h);

	/* t[0 .. 2h] := z0 + z2 -+ |a0 - a1| |b0 - b1|, the middle coefficient a0 b1 + a1 b0 */
	c = liAddN(t, r, r + 2 * h, z2n);
	t[2 * h] = liAdd1(t + z2n, r + z2n, 2 * h - z2n, c);
	if (negA == negB) {
		t[2 * h] -= liSubN(t, t, zm, 2 * h);
	}
	else {
		t[2 * h] += liAddN(t, t, zm, 2 * h);
	}

	/* r[h ..) += t; the product fits r, so the carry dies inside it (and when r has only
	   2h limbs from h on, t[2h] is zero) */
	tn = an + bn - h < 2 * h + 1 ? an + bn - h : 2 * h + 1;
	c = liAddN(r + h, r + h, t, tn);
	liAdd1(r + h + tn, r + h + tn, an + bn - h - tn, c);
}

/* bn <= (an + 1) / 2: the product of a, cut into pieces of bn limbs, with b */
static void
liMulUnbalanced(liLimb *r, const liLimb *a, sqInt an, const liLimb *b, sqInt bn, liLimb *scratch)
{
	liLimb *t = scratch, c;
	sqInt off, cn;

	liMul(r, a, bn, b, bn, scratch + 2 * bn);
	for (off = bn; off < an; off += bn) {
		cn = an - off < bn ? an - off : bn;
		if (cn == bn) {
			liMul(t, a + off, bn, b, bn, scratch + 2 * bn);
		}
		else {
			liMul(t, b, bn, a + off, cn, scratch + 2 * bn);
		}
		/* r[off .. off + bn) holds the top of the previous piece, r[off + bn ..) is new */
		c = liAddN(r + off, r + off, t, bn);
		liAdd1(r + off + bn, t + bn, cn, c);
	}
}

/* r[0 .. an + bn) := a * b; an >= bn >= 1; r overlaps neither a, b nor the
   liMulScratch(an, bn) limbs at scratch */
static void
liMul(liLimb *r, const liLimb *a, sqInt an, const liLimb *b, sqInt bn, liLimb *scratch)
{
	if (bn < liKaratsubaLimbs) {
		liMulBasecase(r, a, an, b, bn);
	}
	else if (bn <= (an + 1) / 2) {
		liMulUnbalanced(r, a, an, b, bn, scratch);
	}
	else {
		liMulKaratsuba(r, a, an, b, bn, scratch);
	}
}

/* The scratch limbs liSqr(r, a, n, scratch) uses; n >= 1. */
static sqInt
liSqrScratch(sqInt n)
{
	sqInt h, need, other;

	if (n < liSqrKaratsubaLimbs) {
		return 0;
	}
	h = (n + 1) / 2;
	need = liSqrScratch(h);
	if ((other = liSqrScratch(n - h)) > need) {
		need = other;
	}
	if (2 * h + 1 > need) {
		need = 2 * h + 1;
	}
	return 3 * h + need;
}

/* r[0 .. 2n) := a^2 by schoolbook, each cross product once: the sum of a_i a_j B^(i+j)
   over i < j, doubled by a one-bit shift, plus the squares a_i^2 B^2i on the diagonal;
   n (n + 1) / 2 limb products instead of n^2.  Menezes, van Oorschot & Vanstone,
   "Handbook of Applied Cryptography" (1996), algorithm 14.16, doubles each cross product
   as it goes; summing them first and doubling once, as GMP's mpn_sqr_basecase does, needs
   no three-limb accumulator.  n >= 1 */
static void
liSqrBasecase(liLimb *r, const liLimb *a, sqInt n)
{
	liLimb s = 0, c = 0, x0, x1;
	liDLimb p, t;
	sqInt i;

	/* r[1 .. 2n - 1) := the cross products, row i adding a_i a[i + 1 .. n) from limb 2i + 1 */
	r[0] = 0;
	r[2 * n - 1] = 0;
	if (n > 1) {
		r[n] = liMul1(r + 1, a + 1, n - 1, a[0]);
		for (i = 1; i < n - 1; i += 1) {
			r[n + i] = liAddMul1(r + 2 * i + 1, a + i + 1, n - i - 1, a[i]);
		}
	}
	/* r := 2 r + the diagonal, two limbs at a time: s is the bit shifted out of the limb
	   below, c the carry of the sum; both end at zero, as a^2 fits 2n limbs */
	for (i = 0; i < n; i += 1) {
		p = (liDLimb) a[i] * a[i];
		x0 = r[2 * i];
		x1 = r[2 * i + 1];
		t = (liDLimb) ((x0 << 1) | s) + (liLimb) p + c;
		r[2 * i] = (liLimb) t;
		t = (liDLimb) ((x1 << 1) | (x0 >> (LI_LB - 1))) + (liLimb) (p >> LI_LB) + (liLimb) (t >> LI_LB);
		r[2 * i + 1] = (liLimb) t;
		c = (liLimb) (t >> LI_LB);
		s = x1 >> (LI_LB - 1);
	}
}

static void liSqr(liLimb *r, const liLimb *a, sqInt n, liLimb *scratch);

/* Karatsuba squaring, n >= 2: with h = (n + 1) / 2 and a = a1 B^h + a0,
   a^2 = a1^2 B^2h + (a0^2 + a1^2 - (a0 - a1)^2) B^h + a0^2, three squares of at most h
   limbs; the middle coefficient 2 a0 a1 is never negative */
static void
liSqrKaratsuba(liLimb *r, const liLimb *a, sqInt n, liLimb *scratch)
{
	sqInt h = (n + 1) / 2, a1n = n - h, z2n = 2 * a1n, tn;
	liLimb *d = scratch, *zm = scratch + h, *t = scratch + 3 * h;
	liLimb c;

	liAbsDiff(d, a, h, a + h, a1n);
	liSqr(zm, d, h, scratch + 3 * h);
	liSqr(r, a, h, scratch + 3 * h);
	liSqr(r + 2 * h, a + h, a1n, scratch + 3 * h);

	/* t[0 .. 2h] := z0 + z2 - (a0 - a1)^2 */
	c = liAddN(t, r, r + 2 * h, z2n);
	t[2 * h] = liAdd1(t + z2n, r + z2n, 2 * h - z2n, c);
	t[2 * h] -= liSubN(t, t, zm, 2 * h);

	/* r[h ..) += t, as in liMulKaratsuba */
	tn = 2 * n - h < 2 * h + 1 ? 2 * n - h : 2 * h + 1;
	c = liAddN(r + h, r + h, t, tn);
	liAdd1(r + h + tn, r + h + tn, 2 * n - h - tn, c);
}

/* r[0 .. 2n) := a^2; n >= 1; r overlaps neither a nor the liSqrScratch(n) limbs at
   scratch.  Below liSqrBasecaseLimbs (inside Karatsuba only when the tests force its
   threshold down) the general schoolbook. */
static void
liSqr(liLimb *r, const liLimb *a, sqInt n, liLimb *scratch)
{
	if (n < liSqrBasecaseLimbs) {
		liMulBasecase(r, a, n, a, n);
	}
	else if (n < liSqrKaratsubaLimbs) {
		liSqrBasecase(r, a, n);
	}
	else {
		liSqrKaratsuba(r, a, n, scratch);
	}
}

#if LI_LIMB_BITS == 64
/* pRes[0 .. resLen) := the low resLen digits of the product of the aDigits digits at pA
   with the limb m: the short operand of most multiplications, with no scratch memory */
static void
liMulDigitsBy1(unsigned int *pRes, sqInt resLen, const unsigned int *pA, sqInt aDigits, liLimb m)
{
	liLimb carry = 0, x;
	liDLimb t;
	sqInt i, n;

	/* digits of a at or above resLen only reach product digits at or above resLen */
	n = aDigits < resLen ? aDigits : resLen;
	for (i = 0; i + 1 < n; i += 2) {
		x = (liLimb) DIGIT(pA[i]) | ((liLimb) DIGIT(pA[i + 1]) << 32);
		t = (liDLimb) x * m + carry;
		pRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) t);
		pRes[i + 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) ((liLimb) t >> 32));
		carry = (liLimb) (t >> 64);
	}
	if (i < n) {
		t = (liDLimb) DIGIT(pA[i]) * m + carry;
		pRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) t);
		carry = (liLimb) (t >> 32);	/* the product of n digits and a limb has n + 2 digits */
		i += 1;
	}
	if (i < resLen) {
		pRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) carry);
		i += 1;
	}
	if (i < resLen) {
		pRes[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((unsigned int) (carry >> 32));
		i += 1;
	}
	for (; i < resLen; i += 1) {
		pRes[i] = 0;
	}
}
#endif

/* liDigitMultiply over limbs in scratch memory; aDigits >= bDigits > 0.  square: the
   digits at pB are those at pA, loaded once and squared. */
static void
liMulDigits(unsigned int *pA, sqInt aDigits, unsigned int *pB, sqInt bDigits, unsigned int *pRes, sqInt resLen,
		int square)
{
	liLimb stackLimbs[LI_STACK_LIMBS];
	liLimb *limbs, *a, *b, *r;
	sqInt an, bn, need;

	an = liLimbsFor(aDigits);
	bn = liLimbsFor(bDigits);
	need = square ? an + (an + an) + liSqrScratch(an) : an + bn + (an + bn) + liMulScratch(an, bn);
	limbs = liScratch(need, stackLimbs);
	if (limbs == 0) {
		liDigitMultiply32(pB, bDigits, pA, aDigits, pRes, resLen);
		return;
	}
	a = limbs;
	liLoad(a, pA, aDigits, 0);
	if (square) {
		r = a + an;
		liSqr(r, a, an, r + an + an);
	}
	else {
		b = a + an;
		r = b + bn;
		liLoad(b, pB, bDigits, 0);
		liMul(r, a, an, b, bn, r + an + bn);
	}
	liStore(pRes, resLen, r, an + bn, 0);
	if (limbs != stackLimbs) {
		LI_FREE(limbs);
	}
}

void
liDigitMultiply(unsigned int *pWordShort, sqInt shortLen, unsigned int *pWordLong, sqInt longLen,
		unsigned int *pWordRes, sqInt resLen)
{
	unsigned int *pA, *pB;
	sqInt aDigits, bDigits;
	int square;

	if (shortLen <= longLen) {
		pA = pWordLong; aDigits = longLen; pB = pWordShort; bDigits = shortLen;
	}
	else {
		pA = pWordShort; aDigits = shortLen; pB = pWordLong; bDigits = longLen;
	}
#if LI_LIMB_BITS == 64
	if (bDigits <= 2) {
		liMulDigitsBy1(pWordRes, resLen, pA, aDigits,
			bDigits == 0 ? 0 : bDigits == 1 ? (liLimb) DIGIT(pB[0]) : (liLimb) DIGIT(pB[0]) | ((liLimb) DIGIT(pB[1]) << 32));
		return;
	}
	if (aDigits * bDigits <= liMulLoopDigitProduct) {
		liDigitMultiply32(pWordShort, shortLen, pWordLong, longLen, pWordRes, resLen);
		return;
	}
#endif
	/* x * x passes the same digits twice; equal digits at two places square too.  Unequal
	   operands nearly always differ in their lowest digit, which is compared first, so
	   that they pay no call; otherwise memcmp reads at most the 2 x aDigits digits, a few
	   ns at worst where the square saves at least 10% of a product of 3 limbs. */
	square = aDigits == bDigits && liLimbsFor(aDigits) >= liSqrBasecaseLimbs
		&& (pA == pB
			|| (pA[0] == pB[0] && memcmp(pA + 1, pB + 1, (size_t) (aDigits - 1) * sizeof(unsigned int)) == 0));
#if LI_LIMB_BITS == 32
	/* below Karatsuba, the 32-bit loop needs no scratch and is as fast as schoolbook */
	if (!square && bDigits < liKaratsubaLimbs) {
		liDigitMultiply32(pWordShort, shortLen, pWordLong, longLen, pWordRes, resLen);
		return;
	}
#endif
	liMulDigits(pA, aDigits, pB, bDigits, pWordRes, resLen, square);
}


/*** Division ***/

#if LI_LIMB_BITS == 64
/* floor((2^19 - 3 * 2^8) / d9) for d9 = 256 .. 511 */
static const unsigned short liReciprocalTable[256] = {
	2045, 2037, 2029, 2021, 2013, 2005, 1998, 1990, 1983, 1975, 1968, 1960, 1953, 1946, 1938, 1931,
	1924, 1917, 1910, 1903, 1896, 1889, 1883, 1876, 1869, 1863, 1856, 1849, 1843, 1836, 1830, 1824,
	1817, 1811, 1805, 1799, 1792, 1786, 1780, 1774, 1768, 1762, 1756, 1750, 1745, 1739, 1733, 1727,
	1722, 1716, 1710, 1705, 1699, 1694, 1688, 1683, 1677, 1672, 1667, 1661, 1656, 1651, 1646, 1641,
	1636, 1630, 1625, 1620, 1615, 1610, 1605, 1600, 1596, 1591, 1586, 1581, 1576, 1572, 1567, 1562,
	1558, 1553, 1548, 1544, 1539, 1535, 1530, 1526, 1521, 1517, 1513, 1508, 1504, 1500, 1495, 1491,
	1487, 1483, 1478, 1474, 1470, 1466, 1462, 1458, 1454, 1450, 1446, 1442, 1438, 1434, 1430, 1426,
	1422, 1418, 1414, 1411, 1407, 1403, 1399, 1396, 1392, 1388, 1384, 1381, 1377, 1374, 1370, 1366,
	1363, 1359, 1356, 1352, 1349, 1345, 1342, 1338, 1335, 1332, 1328, 1325, 1322, 1318, 1315, 1312,
	1308, 1305, 1302, 1299, 1295, 1292, 1289, 1286, 1283, 1280, 1276, 1273, 1270, 1267, 1264, 1261,
	1258, 1255, 1252, 1249, 1246, 1243, 1240, 1237, 1234, 1231, 1228, 1226, 1223, 1220, 1217, 1214,
	1211, 1209, 1206, 1203, 1200, 1197, 1195, 1192, 1189, 1187, 1184, 1181, 1179, 1176, 1173, 1171,
	1168, 1165, 1163, 1160, 1158, 1155, 1153, 1150, 1148, 1145, 1143, 1140, 1138, 1135, 1133, 1130,
	1128, 1125, 1123, 1121, 1118, 1116, 1113, 1111, 1109, 1106, 1104, 1102, 1099, 1097, 1095, 1092,
	1090, 1088, 1086, 1083, 1081, 1079, 1077, 1074, 1072, 1070, 1068, 1066, 1064, 1061, 1059, 1057,
	1055, 1053, 1051, 1049, 1047, 1044, 1042, 1040, 1038, 1036, 1034, 1032, 1030, 1028, 1026, 1024
};

/* floor((B^2 - 1) / d) - B for a normalized d (top bit set), B = 2^64, with no division:
   Moller & Granlund, "Improved division by invariant integers", IEEE Trans. Computers 60
   (2011), algorithm 2 (a table lookup and three Newton steps).  A 128-bit division calls a
   runtime routine that costs five times as much. */
static liLimb
liReciprocal(liLimb d)
{
	liLimb d0 = d & 1, d40 = (d >> 24) + 1, d63 = (d >> 1) + d0, v0, v1, v2, v3, e;

	v0 = liReciprocalTable[(d >> 55) - 256];
	v1 = (v0 << 11) - ((v0 * v0 * d40) >> 40) - 1;
	v2 = (v1 << 13) + ((v1 * ((1ULL << 60) - v1 * d40)) >> 47);
	e = ((v2 >> 1) & (0 - d0)) - v2 * d63;
	v3 = (v2 << 31) + (liLimb) (((liDLimb) v2 * e) >> 65);
	return v3 - (liLimb) ((((liDLimb) v3 * d) + d) >> 64) - d;
}
#else
/* floor((B^2 - 1) / d) - B for a normalized d (top bit set), B = 2^32 */
static liLimb
liReciprocal(liLimb d)
{
	return (liLimb) (((((liDLimb) ~d) << LI_LB) | (liLimb) ~(liLimb) 0) / d);
}
#endif

/* Answer floor((nh B + nl) / d) and store the remainder at *pr; nh < d, d normalized,
   di = liReciprocal(d).  Moller & Granlund, "Improved division by invariant integers",
   IEEE Trans. Computers 60 (2011), algorithm 4. */
static liLimb
liDiv2by1(liLimb *pr, liLimb nh, liLimb nl, liLimb d, liLimb di)
{
	liDLimb p = (liDLimb) nh * di + ((((liDLimb) nh) << LI_LB) | nl);
	liLimb q = (liLimb) (p >> LI_LB) + 1, q0 = (liLimb) p, r;

	r = nl - q * d;
	if (r > q0) {
		q -= 1;
		r += d;
	}
	if (r >= d) {
		q += 1;
		r -= d;
	}
	*pr = r;
	return q;
}

/* Knuth's algorithm D.  v has vn < un limbs and its top bit set; u has un limbs, its top
   vn limbs below v (as when u[un - 1] < v[vn - 1]), so that the quotient has un - vn
   limbs.  Store them at q, leave the remainder in u[0 .. vn) and zeros in u[vn .. un). */
static void
liDivRem(liLimb *q, liLimb *u, sqInt un, const liLimb *v, sqInt vn)
{
	liLimb v1 = v[vn - 1], v0, di = liReciprocal(v1), u2, u1, u0, qhat, rhat, borrow, r;
	sqInt j;

	if (vn == 1) {
		r = u[un - 1];
		u[un - 1] = 0;
		for (j = un - 2; j >= 0; j -= 1) {
			q[j] = liDiv2by1(&r, r, u[j], v1, di);
			u[j] = 0;
		}
		u[0] = r;
		return;
	}
	v0 = v[vn - 2];
	for (j = un - vn - 1; j >= 0; j -= 1) {
		u2 = u[j + vn];
		u1 = u[j + vn - 1];
		u0 = u[j + vn - 2];
		if (u2 >= v1) {
			/* u2 = v1: the quotient limb is B - 1 or B - 2 (v1 >= B / 2) */
			qhat = ~(liLimb) 0;
		}
		else {
			qhat = liDiv2by1(&rhat, u2, u1, v1, di);
			/* Knuth's test with the next divisor limb: afterwards qhat is exact or one too large */
			while ((liDLimb) qhat * v0 > ((((liDLimb) rhat) << LI_LB) | u0)) {
				qhat -= 1;
				rhat += v1;
				if (rhat < v1) {
					break;
				}
			}
		}
		borrow = liSubMul1(u + j, v, vn, qhat);
		if (u2 < borrow) {
			/* qhat was one too large: add v back */
			qhat -= 1;
			liAddN(u + j, u + j, v, vn);
		}
		u[j + vn] = 0;
		q[j] = qhat;
	}
}

/* Divide-and-conquer division: Burnikel & Ziegler, "Fast recursive division", MPI-I-98-1-022
   (1998), in the shape of GMP's mpn_dcpi1_div_qr_n and mpn_dcpi1_div_qr: the divisor is
   cut into uneven halves instead of being padded to a block size times a power of two,
   and Knuth's algorithm D divides below liBZLimbs.  Each half of a quotient is one D3n2n
   step: the top 2k limbs of the partial remainder divided (recursively: D2n1n) by the top
   k limbs of the divisor, then the product of that quotient with the rest of the divisor
   subtracted (liMul, Karatsuba), then the divisor added back while the remainder is
   negative.  The estimate from the top k limbs of a normalized divisor is never too small
   and at most 2 too large, so there are at most two add-backs: Knuth's theorem 4.3.1B with
   the base B^k, whose proof holds without its clamp of the estimate to B^k - 1 (the
   recursion does not clamp: a block's estimate may reach B^k), because the top k limbs of
   the partial remainder never exceed the divisor's.  With V = V1 B^m + V0 (V1 its top k
   limbs, B^k / 2 <= V1, V0 < B^m) and A = A1 B^m + A0 with A1 < (V1 + 1) B^k, the
   estimate floor(A1 / V1) exceeds floor(A / V) by less than 1 + A1 V0 / (V1 V) < 1 + B^k / V1
   <= 3.  The cost is a small multiple of a multiplication, against Knuth's quadratic loop. */

/* Tests count the add-backs: LI_DIV_ADDBACK(step, k) runs before the k-th one of a step */
#if !defined(LI_DIV_ADDBACK)
# define LI_DIV_ADDBACK(step, k) ((void) (k))
#endif

/* Answer -1, 0 or 1 as a[0 .. n) is below, equal to or above b[0 .. n) */
static int
liCmp(const liLimb *a, const liLimb *b, sqInt n)
{
	sqInt i;

	for (i = n - 1; i >= 0; i -= 1) {
		if (a[i] != b[i]) {
			return a[i] < b[i] ? -1 : 1;
		}
	}
	return 0;
}

/* r[0 .. n) := a[0 .. n) - c, answer the borrow (r may be a) */
static liLimb
liSub1(liLimb *r, const liLimb *a, sqInt n, liLimb c)
{
	liLimb ai;
	sqInt i;

	for (i = 0; i < n; i += 1) {
		ai = a[i];
		r[i] = ai - c;
		c = ai < c;
	}
	return c;
}

/* The scratch limbs liDivRem2n(q, u, v, n, scratch) uses */
static sqInt
liDivRem2nScratch(sqInt n)
{
	sqInt lo = n / 2, hi = n - lo, need, other;

	if (n < liBZLimbs) {
		return 0;
	}
	/* the product of a half quotient and a half divisor, and liMul's scratch above it;
	   the recursive divisions run before it, in the same limbs */
	need = n + liMulScratch(hi, lo);
	if ((other = liDivRem2nScratch(hi)) > need) {
		need = other;
	}
	if ((other = liDivRem2nScratch(lo)) > need) {
		need = other;
	}
	return need;
}

/* Divide the 2n limbs at u by the n limbs at v (top bit set): store the low n limbs of
   the quotient at q and answer its limb n (0 or 1), leave the remainder in u[0 .. n) and
   zeros in u[n .. 2n).  Burnikel & Ziegler's D2n1n (GMP's mpn_dcpi1_div_qr_n): with
   n = hi + lo, hi = lo or lo + 1, the quotient limbs q[lo .. n) from u[lo .. 2n), then
   q[0 .. lo) from the remainder and u[0 .. lo), each one D3n2n step.  Below liBZLimbs,
   Knuth's algorithm D, after subtracting v once if the top n limbs of u reach it. */
static liLimb
liDivRem2n(liLimb *q, liLimb *u, const liLimb *v, sqInt n, liLimb *scratch)
{
	sqInt lo = n / 2, hi = n - lo, k;
	liLimb qh, ql, cy, *t = scratch;

	if (n < liBZLimbs) {
		qh = liCmp(u + n, v, n) >= 0;
		if (qh) {
			liSubN(u + n, u + n, v, n);
		}
		liDivRem(q, u, 2 * n, v, n);
		return qh;
	}

	/* q[lo .. n) (and qh): u[2lo .. 2n) by v[lo .. n), leaving the remainder in
	   u[2lo .. n + lo); then u[lo .. n + lo) -= q[lo .. n) v[0 .. lo) */
	qh = liDivRem2n(q + lo, u + 2 * lo, v + lo, hi, scratch);
	liMul(t, q + lo, hi, v, lo, t + n);
	cy = liSubN(u + lo, u + lo, t, n);
	if (qh) {
		cy += liSubN(u + n, u + n, v, lo);
	}
	for (k = 1; cy != 0; k += 1) {
		LI_DIV_ADDBACK(0, k);
		qh -= liSub1(q + lo, q + lo, hi, 1);
		cy -= liAddN(u + lo, u + lo, v, n);
	}

	/* q[0 .. lo): u[hi .. n + lo) by v[hi .. n), leaving the remainder in u[hi .. n);
	   then u[0 .. n) -= q[0 .. lo) v[0 .. hi); the quotient is below B^lo */
	ql = liDivRem2n(q, u + hi, v + hi, lo, scratch);
	liMul(t, v, hi, q, lo, t + n);
	cy = liSubN(u, u, t, n);
	if (ql) {
		cy += liSubN(u + lo, u + lo, v, hi);
	}
	for (k = 1; cy != 0; k += 1) {
		LI_DIV_ADDBACK(1, k);
		liSub1(q, q, lo, 1);
		cy -= liAddN(u, u, v, n);
	}
	return qh;
}

/* The scratch limbs liDivRemBZ(q, u, vn + qn, v, vn, scratch) uses */
static sqInt
liDivRemBZScratch(sqInt qn, sqInt vn)
{
	sqInt qb = qn % vn == 0 ? vn : qn % vn, big, need, other;

	need = liDivRem2nScratch(qb);
	if (qb < vn) {
		big = qb > vn - qb ? qb : vn - qb;
		if ((other = vn + liMulScratch(big, vn - big)) > need) {
			need = other;
		}
	}
	if (qn > qb && (other = liDivRem2nScratch(vn)) > need) {
		need = other;
	}
	return need;
}

/* Burnikel & Ziegler's division with the contract of liDivRem, and the
   liDivRemBZScratch(un - vn, vn) limbs at scratch.  The quotient is cut into blocks of
   vn limbs from the bottom and computed top block first (GMP's mpn_dcpi1_div_qr).  The
   top block has qb = 1 .. vn limbs: a D3n2n step with the top 2qb limbs of u and the
   top qb limbs of v (a quotient shorter than v needs no more of it for its estimate),
   then the rest of v.  Every other block divides 2vn limbs of the partial remainder by
   v (D2n1n), whose top vn limbs are the previous remainder, below v. */
static void
liDivRemBZ(liLimb *q, liLimb *u, sqInt un, const liLimb *v, sqInt vn, liLimb *scratch)
{
	sqInt qn = un - vn, qb, j, k;
	liLimb qh, cy;

	qb = qn % vn == 0 ? vn : qn % vn;
	j = qn - qb;
	/* q[j .. qn) (and qh): u[j + vn - qb .. un) by v[vn - qb .. vn), leaving the
	   remainder in u[j + vn - qb .. j + vn); then u[j .. j + vn) -= q[j .. qn) v[0 .. vn - qb) */
	qh = liDivRem2n(q + j, u + j + vn - qb, v + vn - qb, qb, scratch);
	if (qb < vn) {
		if (qb >= vn - qb) {
			liMul(scratch, q + j, qb, v, vn - qb, scratch + vn);
		}
		else {
			liMul(scratch, v, vn - qb, q + j, qb, scratch + vn);
		}
		cy = liSubN(u + j, u + j, scratch, vn);
		if (qh) {
			/* the top qb limbs of u reached those of v: possible with liDivRem's contract,
			   not through liDigitDiv, whose dividend's top limb is below the divisor's */
			cy += liSubN(u + j + qb, u + j + qb, v, vn - qb);
		}
		for (k = 1; cy != 0; k += 1) {
			LI_DIV_ADDBACK(2, k);
			qh -= liSub1(q + j, q + j, qb, 1);
			cy -= liAddN(u + j, u + j, v, vn);
		}
	}
	/* qh is 0 here: the top vn limbs of u were below v */
	for (j -= vn; j >= 0; j -= vn) {
		liDivRem2n(q + j, u + j, v, vn, scratch);
	}
}

void
liDigitDiv(unsigned int *pDiv, sqInt divLen, unsigned int *pRem, sqInt remLen, unsigned int *pQuo, sqInt quoLen)
{
	liLimb stackLimbs[LI_STACK_LIMBS];
	liLimb *limbs, *u, *v, *q;
	sqInt dl, skip, un, vn, need, bz;

	dl = divLen - 1;
	if (!(dl >= 1 && quoLen >= 1 && quoLen == remLen - divLen + 1
		 && pDiv[dl] == 0
		 && (DIGIT(pDiv[dl - 1]) & 0x80000000U) != 0
		 && DIGIT(pRem[remLen - 1]) < DIGIT(pDiv[dl - 1]))) {
		/* not the shape the plugin prepares: the loop it replaces answers whatever it answers */
		liDigitDiv32(pDiv, divLen, pRem, remLen, pQuo, quoLen);
		return;
	}
	if (quoLen <= liDivLoopQuotientDigits && dl < LI_DIV_LOOP_DIVISOR_DIGITS) {
		liDigitDiv32(pDiv, divLen, pRem, remLen, pQuo, quoLen);
		return;
	}
	/* With 64-bit limbs, an odd divisor digit count gets one more zero digit below both
	   operands, so that the top limb of the divisor has its top bit set: the quotient is
	   the same, the remainder is one digit higher.  The top limb of the dividend is below
	   that of the divisor, as its top digit is below the divisor's (checked above). */
	skip = LI_DIGITS_PER_LIMB == 2 ? dl & 1 : 0;
	vn = liLimbsFor(dl + skip);
	un = liLimbsFor(remLen + skip);
	need = vn + un + (un - vn);
	/* Burnikel & Ziegler when both the divisor and the quotient have liBZLimbs limbs, or the
	   divisor liBZLimbs and the quotient liBZQuotientLimbs, with the divisor at least
	   liBZQuotientLimbs longer */
	bz = vn >= liBZLimbs
		&& (un - vn >= liBZLimbs || (un - vn >= liBZQuotientLimbs && vn - (un - vn) >= liBZQuotientLimbs));
	if (bz) {
		need += liDivRemBZScratch(un - vn, vn);
	}
	limbs = liScratch(need, stackLimbs);
	if (limbs == 0) {
		liDigitDiv32(pDiv, divLen, pRem, remLen, pQuo, quoLen);
		return;
	}
	v = limbs;
	u = v + vn;
	q = u + un;
	liLoad(v, pDiv, dl, skip);
	liLoad(u, pRem, remLen, skip);
	if (bz) {
		liDivRemBZ(q, u, un, v, vn, q + (un - vn));
	}
	else {
		liDivRem(q, u, un, v, vn);
	}
	liStore(pQuo, quoLen, q, un - vn, 0);
	liStore(pRem, remLen, u, vn, skip);
	if (limbs != stackLimbs) {
		LI_FREE(limbs);
	}
}
