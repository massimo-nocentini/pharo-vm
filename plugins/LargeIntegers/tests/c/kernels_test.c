/* kernels_test.c: unit tests of the LargeIntegers multiply, divide and Montgomery kernels.
 *
 * Compiles src/common/LargeIntegersKernels.c into this program (#include, so that the
 * thresholds are variables and malloc can be made to fail) and compares liDigitMultiply()
 * and liDigitDiv() digit for digit with FROZEN copies of the 32-bit loops the plugin ran
 * before the kernels existed, and with independent oracles: a base-256 schoolbook
 * multiply, the reconstruction a = q*b + r with known q and r, and (mode `dump`)
 * Python's big integers (oracle.py).  Squares (the same digits as both operands, through
 * one pointer or a copy) take their own kernels, and have their own cases; so do
 * divisions that take Burnikel & Ziegler's recursion (LI_BZ_LIMBS, LI_BZ_QUOTIENT_LIMBS),
 * whose liDivRemBZ is also called directly, on limbs, with the contract of liDivRem.
 * liDigitMontgomery() is compared with a frozen copy of the plugin's Montgomery loop, for
 * the right inverse (the limb kernels) and for wrong ones and even moduli (the loop
 * again), and in mode `dump` with T = (a b + U m) / 2^32n, T - m when T >= m, from
 * Python's integers.
 *
 * Build variants (run-c-tests.sh): LI_LIMB_BITS=64 (the default on x86_64 and aarch64)
 * or 32, FAKE_BIGENDIAN (SQ_SWAP_4_BYTES_IF_BIGENDIAN swaps: digits are stored
 * byte-swapped, as on a big-endian host, in kernels and frozen loops alike), ASan+UBSan
 * or -O2.
 *
 * usage: kernels_test [scale]      run every test; scale multiplies the random counts
 *        kernels_test dump N       print N multiply, N divide and N Montgomery vectors for oracle.py
 * Exit status 0 when every check holds.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "config.h"
#include "sqConfig.h"
#include "memoryAccess.h"
/* LargeIntegersKernels.c includes sqConfig.h again, which the one of macOS refuses
   while SQ_CONFIG_DONE is defined (it only defines macros: a second pass is harmless) */
#undef SQ_CONFIG_DONE
#ifdef FAKE_BIGENDIAN
# undef SQ_SWAP_4_BYTES_IF_BIGENDIAN
# define SQ_SWAP_4_BYTES_IF_BIGENDIAN(x) SQ_SWAP_4_BYTES(x)
#endif

static int mallocFails;
static long mallocCalls;
static void *testMalloc(size_t n) { mallocCalls++; return mallocFails ? NULL : malloc(n); }
#define LI_MALLOC testMalloc
#define LI_FREE free
#define LI_TUNABLE 1
/* the add-backs of the Burnikel & Ziegler steps, [step][k - 1] (k = 3 counts the third and later) */
static long nAddBack[3][3];
#define LI_DIV_ADDBACK(step, k) (nAddBack[step][(k) < 3 ? (k) - 1 : 2]++)
#include "LargeIntegersKernels.c"

typedef uint32_t u32;
typedef uint64_t u64;
#define GET(p, i) ((u32) SQ_SWAP_4_BYTES_IF_BIGENDIAN((p)[i]))
#define PUT(p, i, v) ((p)[i] = SQ_SWAP_4_BYTES_IF_BIGENDIAN((u32) (v)))

static u64 rs = 0x0123456789ABCDEFull;
static u64 rnd(void) { rs ^= rs >> 12; rs ^= rs << 25; rs ^= rs >> 27; return rs * 0x2545F4914F6CDD1Dull; }
static long rndBelow(long n) { return n <= 0 ? 0 : (long) (rnd() % (u64) n); }

/* adversarial digit values */
static u32 patDigit(void)
{
	static const u32 pats[] = { 0, 1, 0xFFFFFFFFu, 0xFFFFFFFEu, 0x80000000u, 0x7FFFFFFFu, 0x80000001u, 0xFFFF0000u };
	unsigned k = rnd() % 12;
	return k < 8 ? pats[k] : (u32) rnd();
}
/* logical digits, stored through PUT */
static void fill(u32 *d, long n, int mode)
{
	for (long i = 0; i < n; i++) {
		u32 v;
		switch (mode % 5) {
		case 0: v = (u32) rnd(); break;
		case 1: v = patDigit(); break;
		case 2: v = 0xFFFFFFFFu; break;
		case 3: v = rnd() % 7 ? 0 : (u32) rnd(); break;      /* sparse */
		default: v = rnd() % 3 ? 0xFFFFFFFFu : patDigit(); break;
		}
		PUT(d, i, v);
	}
}
static u32 *dalloc(long n) { return malloc((n ? n : 1) * 4); }   /* exact size, for ASan */
static void garbage(u32 *d, long n) { for (long i = 0; i < n; i++) d[i] = (u32) rnd() | 0x5A5A5A5A; }

/* ---------------------------------------------------------------- frozen loops */

/* The two loops below are frozen copies of the C that VMMaker.oscog-eem.2495 generated
   for LargeIntegersPlugin>>cDigitMultiply:len:with:len:into:len: and
   cDigitDiv:len:rem:len:quo:len: (inlined in primDigitMultiplyNegative and
   primDigitDivNegative of src/common/LargeIntegers.c before the kernels were added),
   with sqInt locals as long.  liDigitMultiply32/liDigitDiv32 in LargeIntegersKernels.c
   hold the same loops; this file keeps its own copy so that a change there cannot also
   change the reference.  Do not edit them. */

/* cDigitMultiply:len:with:len:into:len:; pRes starts out all zero (the plugin zeroes it
   or allocates it) */
static void frozen_mul(unsigned int *pShort, long shortLen, unsigned int *pLong, long longLen, unsigned int *pProd, long resDigitLen)
{
	unsigned long long ab;
	unsigned int carry, digit;
	long i2, j, k, limitShort, limitLong;

	if ((shortLen == 1) && ((pShort[0]) == 0)) return;
	if ((longLen == 1) && ((pLong[0]) == 0)) return;
	limitShort = shortLen - 1;
	limitLong = longLen - 1;
	for (i2 = 0; i2 <= limitShort; i2 += 1) {
		if (((digit = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pShort[i2]))) != 0) {
			k = i2;
			carry = 0;
			for (j = 0; j <= limitLong; j += 1) {
				ab = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pLong[j]);
				ab = ((ab * digit) + carry) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pProd[k]));
				carry = ab >> 32;
				pProd[k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(ab & 0xFFFFFFFFU);
				k += 1;
			}
			if (k < resDigitLen) {
				pProd[k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(carry);
			}
		}
	}
}

/* cDigitDiv:len:rem:len:quo:len: */
static void frozen_div(unsigned int *pDiv, long divLen, unsigned int *pRem, long remLen, unsigned int *pQuo, long quoDigitLen)
{
	unsigned long long a, b, hi, lo, mul, q, r1r2, t;
	int cond;
	unsigned int dh, dnh, r3;
	long dl, ql, i, j, k, l;

	dl = divLen - 1;
	ql = quoDigitLen;
	dh = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[dl - 1]);
	if (dl == 1) dnh = 0;
	else dnh = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pDiv[dl - 2]);
	for (k = 1; k <= ql; k += 1) {
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
			if (j < 3) r3 = 0;
			else r3 = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRem[j - 3]);
			while (1) {
				if ((t < hi) || ((t == hi) && (r3 < lo))) {
					q -= 1;
					if (hi == 0) cond = 0;
					else {
						if (lo < dnh) { hi -= 1; lo = (lo + 0x100000000LL) - dnh; }
						else lo -= dnh;
						cond = hi >= dh;
					}
				}
				else cond = 0;
				if (!cond) break;
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
		pQuo[quoDigitLen - k] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(((unsigned int) q));
	}
}

/* A frozen copy of the C that Pharo Slang generated for
   LargeIntegersPlugin>>cDigitMontgomery:len:times:len:modulo:len:mInvModB:into: (inlined in
   primMontgomeryTimesModulo of src/common/LargeIntegers.c before liDigitMontgomery was
   added), with the cDigitCompare:with:len: it calls (cDigitComparewithlen) and sqInt
   locals as long.  liDigitMontgomery32 in LargeIntegersKernels.c holds the same loop.
   Do not edit them. */
static long frozen_compare(unsigned int *pFirst, unsigned int *pSecond, long len)
{
	unsigned int firstDigit, secondDigit;
	long ix;

	ix = len - 1;
	while (ix >= 0) {
		if (((secondDigit = pSecond[ix])) != ((firstDigit = pFirst[ix]))) {
			if ((SQ_SWAP_4_BYTES_IF_BIGENDIAN(secondDigit)) < (SQ_SWAP_4_BYTES_IF_BIGENDIAN(firstDigit))) return 1;
			else return -1;
		}
		ix -= 1;
	}
	return 0;
}

/* cDigitMontgomery:len:times:len:modulo:len:mInvModB:into:; pRes starts out all zero */
static void frozen_mont(unsigned int *pFirst, long firstDigitLen, unsigned int *pSecond, long secondDigitLen,
	unsigned int *pThird, long thirdDigitLen, unsigned int mInv, unsigned int *pRes)
{
	unsigned long long accum, accum2, accum3, u;
	unsigned int lastDigit;
	long i2, k, limit1, limit2, limit3;

	limit1 = firstDigitLen - 1;
	limit2 = secondDigitLen - 1;
	limit3 = thirdDigitLen - 1;
	lastDigit = 0;
	for (i2 = 0; i2 <= limit1; i2 += 1) {
		accum3 = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFirst[i2]);
		accum3 = (accum3 * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pSecond[0]))) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[0]));
		u = (accum3 * mInv) & 0xFFFFFFFFU;
		accum2 = u * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[0]));
		accum = (accum2 & 0xFFFFFFFFU) + (accum3 & 0xFFFFFFFFU);
		accum = ((accum >> 32) + (accum2 >> 32)) + (accum3 >> 32);
		for (k = 1; k <= limit2; k += 1) {
			accum3 = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pFirst[i2]);
			accum3 = (accum3 * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pSecond[k]))) + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[k]));
			accum2 = u * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[k]));
			accum = (accum + (accum2 & 0xFFFFFFFFU)) + (accum3 & 0xFFFFFFFFU);
			pRes[k - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
			accum = ((accum >> 32) + (accum2 >> 32)) + (accum3 >> 32);
		}
		for (k = secondDigitLen; k <= limit3; k += 1) {
			accum2 = u * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[k]));
			accum = (accum + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[k]))) + (accum2 & 0xFFFFFFFFU);
			pRes[k - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
			accum = (accum >> 32) + (accum2 >> 32);
		}
		accum += lastDigit;
		pRes[limit3] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
		lastDigit = accum >> 32;
	}
	for (i2 = firstDigitLen; i2 <= limit3; i2 += 1) {
		accum = SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[0]);
		u = (accum * mInv) & 0xFFFFFFFFU;
		accum += u * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[0]));
		accum = accum >> 32;
		for (k = 1; k <= limit3; k += 1) {
			accum2 = u * (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[k]));
			accum = (accum + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[k]))) + (accum2 & 0xFFFFFFFFU);
			pRes[k - 1] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
			accum = (accum >> 32) + (accum2 >> 32);
		}
		accum += lastDigit;
		pRes[limit3] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
		lastDigit = accum >> 32;
	}
	if (!((lastDigit == 0) && ((frozen_compare(pThird, pRes, thirdDigitLen)) == 1))) {
		accum = 0;
		for (i2 = 0; i2 <= limit3; i2 += 1) {
			accum = (accum + (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pRes[i2]))) - (SQ_SWAP_4_BYTES_IF_BIGENDIAN(pThird[i2]));
			pRes[i2] = SQ_SWAP_4_BYTES_IF_BIGENDIAN(accum & 0xFFFFFFFFU);
			accum = 0 - (accum >> 0x3F);
		}
	}
}

/* ---------------------------------------------------------------- independent oracles */

/* r[0 .. an + bn) := a * b, base 256 over the logical digits */
static void ref_mul(const u32 *a, long an, const u32 *b, long bn, u32 *r)
{
	long xn = an * 4, yn = bn * 4;
	uint8_t *x = malloc(xn + 1), *y = malloc(yn + 1);
	uint32_t *acc = calloc(xn + yn + 1, sizeof *acc);
	for (long i = 0; i < xn; i++) x[i] = (uint8_t) (GET(a, i / 4) >> (8 * (i % 4)));
	for (long i = 0; i < yn; i++) y[i] = (uint8_t) (GET(b, i / 4) >> (8 * (i % 4)));
	for (long i = 0; i < xn; i++) {
		uint32_t c = 0;
		for (long j = 0; j < yn; j++) { uint32_t t = acc[i + j] + (uint32_t) x[i] * y[j] + c; acc[i + j] = t & 255; c = t >> 8; }
		for (long k = i + yn; c; k++) { uint32_t t = acc[k] + c; acc[k] = t & 255; c = t >> 8; }
	}
	for (long i = 0; i < an + bn; i++) {
		u32 v = 0;
		for (int k = 3; k >= 0; k--) v = (v << 8) | acc[4 * i + k];
		PUT(r, i, v);
	}
	free(x); free(y); free(acc);
}

static int fails;
static long nMul, nDiv, nFallbackMul, nFallbackDiv;
#define CHECK(c, ...) do { if (!(c)) { if (fails++ < 30) { printf("FAIL: " __VA_ARGS__); printf("\n"); } } } while (0)

/* ---------------------------------------------------------------- multiply */

/* One multiply case: shortLen/longLen digits (either order), resLen digits.
   frozen into zeros vs kernel into garbage; optionally the base-256 oracle. */
static long pluginResLen(long sn, long ln);
static void mulCase(long sn, long ln, long resLen, int modeS, int modeL, int square, int oracle)
{
	u32 *s, *l;
	if (square && ln != sn) { ln = sn; resLen = pluginResLen(sn, ln); }
	s = dalloc(sn);
	l = square ? s : dalloc(ln);
	fill(s, sn, modeS);
	if (!square) fill(l, ln, modeL);
	u32 *r1 = dalloc(resLen), *r2 = dalloc(resLen);
	memset(r1, 0, resLen * 4);
	garbage(r2, resLen);
	frozen_mul(s, sn, l, ln, r1, resLen);
	liDigitMultiply(s, sn, l, ln, r2, resLen);
	CHECK(!memcmp(r1, r2, resLen * 4), "mul %ldx%ld res %ld modes %d/%d sq %d thr %ld", sn, ln, resLen, modeS, modeL, square, (long) liKaratsubaLimbs);
	if (oracle) {
		u32 *r3 = dalloc(sn + ln);
		ref_mul(s, sn, l, ln, r3);
		/* the low resLen digits of the exact product (resLen <= sn + ln here) */
		CHECK(!memcmp(r1, r3, resLen * 4), "frozen mul != base-256 oracle %ldx%ld res %ld", sn, ln, resLen);
		free(r3);
	}
	free(s); if (!square) free(l); free(r1); free(r2);
	nMul++;
}

/* resLen as the plugin computes it from byte sizes: ceil((sBytes + lBytes) / 4), i.e.
   sn + ln or sn + ln - 1; also exercises the dropped top carry of a partial word */
static long pluginResLen(long sn, long ln)
{
	if (sn == 0 || ln == 0) return sn + ln;
	return sn + ln - (long) (rnd() & 1);
}

/* the random draws in a fixed order: as arguments of one call they would be made in
   the compiler's order, and gcc and clang would run different cases */
static void mulRandomCase(long sn, long ln, int oracle)
{
	long resLen = pluginResLen(sn, ln);
	int sMode = (int) rnd(), lMode = (int) rnd();
	int square = rnd() % 16 == 0;
	mulCase(sn, ln, resLen, sMode, lMode, square, oracle);
}

static void testMulSmall(long count)
{
	long thr0 = liKaratsubaLimbs;
	for (long c = 0; c < count; c++) {
		long sn = rndBelow(13), ln = rndBelow(13);
		mulRandomCase(sn, ln, c < 20000);
	}
	/* Karatsuba forced on small operands: thresholds 2..9 limbs */
	for (long c = 0; c < count; c++) {
		static const long thr[] = { 2, 3, 4, 5, 6, 9 };
		liKaratsubaLimbs = thr[rnd() % 6];
		long sn = rndBelow(90), ln = rnd() % 3 ? sn + rndBelow(20) - 10 : rndBelow(200);
		if (ln < 0) ln = 0;
		mulRandomCase(sn, ln, c < 3000);
	}
	liKaratsubaLimbs = thr0;
	printf("mul small + forced Karatsuba: %ld cases\n", 2 * count);
}

/* sizes straddling the production threshold and the unbalanced split */
static void testMulThreshold(long reps)
{
	long T = LI_KARATSUBA_LIMBS, D = LI_DIGITS_PER_LIMB, n = 0;
	for (long bl = T - 3; bl <= T + 3; bl++) {
		long als[] = { bl, bl + 1, 2 * bl - 2, 2 * bl - 1, 2 * bl, 2 * bl + 1, 3 * bl, 3 * bl + 1, 5 * bl - 1 };
		for (unsigned i = 0; i < sizeof als / sizeof *als; i++)
			for (long r = 0; r < reps; r++)
				for (int ds = 0; ds < D; ds++)
					for (int dl = 0; dl < D; dl++) {
						long sn = bl * D - ds, ln = als[i] * D - dl;
						long resLen = pluginResLen(sn, ln);
						int sMode = (int) rnd(), lMode = (int) rnd();
						mulCase(sn, ln, resLen, sMode, lMode, 0, 0);
						n++;
					}
		mulCase(bl * D, bl * D, 2 * bl * D, 2, 2, 1, 0);   /* all-ones square */
	}
	printf("mul threshold straddle (T=%ld limbs): %ld cases\n", T, n);
}

static void testMulLarge(void)
{
	static const long sizes[][2] = { { 255, 257 }, { 500, 500 }, { 511, 1023 }, { 1000, 3001 }, { 2047, 2048 },
		{ 2048, 2048 }, { 4096, 4096 }, { 4095, 8193 }, { 3, 9000 }, { 64, 9000 }, { 129, 8191 }, { 6000, 6001 } };
	for (unsigned i = 0; i < sizeof sizes / sizeof *sizes; i++)
		for (int mode = 0; mode < 5; mode++)
			mulCase(sizes[i][0], sizes[i][1], pluginResLen(sizes[i][0], sizes[i][1]), mode, mode + 1 + (int) i, mode == 4, 0);
	printf("mul large: %ld sizes x 5 patterns\n", (long) (sizeof sizes / sizeof *sizes));
}

/* the scratch malloc fails: the 32-bit loop runs instead */
static void testMulNoMemory(void)
{
	long before = mallocCalls;
	mallocFails = 1;
	mulCase(700, 900, 1600, 0, 1, 0, 0);
	mulCase(1200, 1200, 2399, 2, 2, 1, 0);   /* a square: its limbs alone outgrow the stack */
	mulCase(40, 5000, 5040, 0, 0, 0, 0);
	mallocFails = 0;
	CHECK(mallocCalls - before == 3, "the large multiplies did not ask for scratch memory (%ld)", mallocCalls - before);
	nFallbackMul += 3;
	printf("mul with failing malloc: 3 cases\n");
}

/* ---------------------------------------------------------------- squares */

static long nSqr;

/* One square: the n digits of x by themselves, through one pointer (x * x, how the plugin
   passes x squared) or through a copy of x (equal digits at two places), into resLen
   digits.  top: the top digit as filled (0) or 0xFFFFFFFF, 0x80000000, 1; lead: that many
   zero digits above it (an unnormalized operand).  Frozen loop into zeros vs kernel into
   garbage, operands untouched; optionally the base-256 oracle. */
static void sqrCase(long n, long resLen, int mode, int top, long lead, int copy, int oracle)
{
	static const u32 tops[] = { 0, 0xFFFFFFFFu, 0x80000000u, 1 };
	u32 *x = dalloc(n), *y = copy ? dalloc(n) : x, *x0 = dalloc(n);
	fill(x, n, mode);
	if (top && n > lead) PUT(x, n - 1 - lead, tops[top]);
	for (long i = n - lead < 0 ? 0 : n - lead; i < n; i++) PUT(x, i, 0);
	if (copy) memcpy(y, x, n * 4);
	memcpy(x0, x, n * 4);
	u32 *r1 = dalloc(resLen), *r2 = dalloc(resLen);
	memset(r1, 0, resLen * 4);
	garbage(r2, resLen);
	frozen_mul(x0, n, x0, n, r1, resLen);
	liDigitMultiply(x, n, y, n, r2, resLen);
	CHECK(!memcmp(r1, r2, resLen * 4), "sqr %ld res %ld mode %d top %d lead %ld copy %d thr %ld/%ld", n, resLen, mode, top, lead, copy,
		(long) liSqrBasecaseLimbs, (long) liSqrKaratsubaLimbs);
	CHECK(!memcmp(x, x0, n * 4) && !memcmp(y, x0, n * 4), "sqr %ld changed its operand", n);
	if (oracle) {
		u32 *r3 = dalloc(2 * n);
		ref_mul(x0, n, x0, n, r3);
		CHECK(!memcmp(r1, r3, resLen * 4), "frozen square != base-256 oracle %ld res %ld", n, resLen);
		free(r3);
	}
	free(x); if (copy) free(y); free(x0); free(r1); free(r2);
	nMul++;
	nSqr++;
}

/* x * y with y equal to x but for one digit (lowest, highest or a middle one): a product,
   not a square */
static void nearSquareCase(long n, int mode)
{
	u32 *x = dalloc(n), *y = dalloc(n), *r1 = dalloc(2 * n), *r2 = dalloc(2 * n);
	long k = rnd() % 3 == 0 ? 0 : rnd() % 2 ? n - 1 : rndBelow(n);
	fill(x, n, mode);
	memcpy(y, x, n * 4);
	PUT(y, k, GET(y, k) ^ (1u << (rnd() % 32)));
	memset(r1, 0, 2 * n * 4);
	garbage(r2, 2 * n);
	frozen_mul(x, n, y, n, r1, 2 * n);
	liDigitMultiply(x, n, y, n, r2, 2 * n);
	CHECK(!memcmp(r1, r2, 2 * n * 4), "near-square %ld digit %ld mode %d", n, k, mode);
	free(x); free(y); free(r1); free(r2);
	nMul++;
}

/* random squares up to 200 digits, Karatsuba squaring forced at thresholds 2..9 limbs,
   squaring from 1..4 limbs on */
static void testSqrSmall(long count)
{
	long b0 = liSqrBasecaseLimbs, k0 = liSqrKaratsubaLimbs;
	for (long c = 0; c < count; c++) {
		static const long kthr[] = { 2, 3, 4, 5, 6, 9 }, bthr[] = { 1, 2, 3, 4 };
		long n = c % 4 ? rndBelow(200) + 1 : rndBelow(16) + 1;
		int mode = (int) rnd(), top = (int) (rnd() % 4), copy = (int) (rnd() % 2);
		long lead = rnd() % 4 ? 0 : rndBelow(3);
		long resLen = 2 * n - (long) (rnd() & 1);
		liSqrKaratsubaLimbs = kthr[rnd() % 6];
		liSqrBasecaseLimbs = c % 3 ? bthr[rnd() % 4] : b0;
		sqrCase(n, resLen, mode, top, lead, copy, c < 3000);
		if (c % 8 == 0) nearSquareCase(n, mode);
	}
	liSqrBasecaseLimbs = b0;
	liSqrKaratsubaLimbs = k0;
	printf("sqr small + forced Karatsuba squaring: %ld cases\n", count);
}

/* sizes straddling the production thresholds (squaring, Karatsuba squaring, one and two
   levels of it, the product's Karatsuba), every partial-limb offset, both resLen */
static void testSqrThreshold(long reps)
{
	long D = LI_DIGITS_PER_LIMB, n = 0;
	long centres[] = { LI_SQR_BASECASE_LIMBS, LI_SQR_KARATSUBA_LIMBS, 2 * LI_SQR_KARATSUBA_LIMBS, 4 * LI_SQR_KARATSUBA_LIMBS,
		LI_KARATSUBA_LIMBS };
	for (unsigned c = 0; c < sizeof centres / sizeof *centres; c++)
		for (long l = centres[c] - 3; l <= centres[c] + 3; l++) {
			if (l < 1) continue;
			for (long r = 0; r < reps; r++)
				for (int ds = 0; ds < D; ds++)
					for (int top = 0; top < 4; top++) {
						long d = l * D - ds;
						int mode = (int) rnd(), copy = (int) (rnd() % 2);
						sqrCase(d, 2 * d - (top & 1), mode, top, 0, copy, r == 0 && c < 2);
						sqrCase(d, 2 * d, mode, top, 1 + (long) (rnd() % 2), !copy, 0);
						n += 2;
					}
			nearSquareCase(l * D, (int) rnd());
		}
	printf("sqr threshold straddle (squaring from %d, Karatsuba from %d limbs): %ld cases\n",
		LI_SQR_BASECASE_LIMBS, LI_SQR_KARATSUBA_LIMBS, n);
}

static void testSqrLarge(void)
{
	static const long sizes[] = { 255, 256, 257, 500, 511, 1000, 1023, 1024, 1025, 2047, 2048, 3001, 4096, 6001, 10000 };
	for (unsigned i = 0; i < sizeof sizes / sizeof *sizes; i++)
		for (int mode = 0; mode < 5; mode++)
			sqrCase(sizes[i], 2 * sizes[i] - (mode & 1), mode, mode % 4, mode == 3, mode & 1, sizes[i] <= 1024 && mode < 2);
	printf("sqr large: %ld sizes x 5 patterns\n", (long) (sizeof sizes / sizeof *sizes));
}

/* the scratch malloc fails: the 32-bit loop runs instead, for both kinds of square */
static void testSqrNoMemory(void)
{
	long before = mallocCalls;
	mallocFails = 1;
	sqrCase(1500, 3000, 0, 1, 0, 0, 0);
	sqrCase(1501, 3001, 4, 2, 1, 1, 0);
	mallocFails = 0;
	CHECK(mallocCalls - before == 2, "the large squares did not ask for scratch memory (%ld)", mallocCalls - before);
	nFallbackMul += 2;
	printf("sqr with failing malloc: 2 cases\n");
}

/* ---------------------------------------------------------------- divide */

static int clz32(u32 x) { return x ? __builtin_clz(x) : 32; }

/* Divide the logical digits a (an, normalized) by b (bn, normalized) as the plugin does:
   shift both by d into bn + 1 and an + 1 digits, quoLen = an - bn + 1. Compare frozen and
   kernel on all of pQuo and pRem (and pDiv untouched). Answer 0 if the shape is skipped. */
static int divCase(const u32 *a, long an, const u32 *b, long bn, u32 *qOut, u32 *rOut)
{
	if (bn < 1 || an < bn || GET(b, bn - 1) == 0) return 0;
	int d = clz32(GET(b, bn - 1));
	long divLen = bn + 1, remLen = an + 1, quoLen = an - bn + 1;
	u32 *div1 = dalloc(divLen), *div2 = dalloc(divLen), *rem1 = dalloc(remLen), *rem2 = dalloc(remLen);
	u32 *quo1 = dalloc(quoLen), *quo2 = dalloc(quoLen);
	u32 carry = 0;
	for (long i = 0; i < bn; i++) { u32 v = GET(b, i); PUT(div1, i, d ? (v << d) | carry : v); carry = d ? v >> (32 - d) : 0; }
	PUT(div1, bn, carry);   /* zero */
	carry = 0;
	for (long i = 0; i < an; i++) { u32 v = GET(a, i); PUT(rem1, i, d ? (v << d) | carry : v); carry = d ? v >> (32 - d) : 0; }
	PUT(rem1, an, carry);
	memcpy(div2, div1, divLen * 4);
	memcpy(rem2, rem1, remLen * 4);
	garbage(quo1, quoLen);
	memcpy(quo2, quo1, quoLen * 4);
	frozen_div(div1, divLen, rem1, remLen, quo1, quoLen);
	liDigitDiv(div2, divLen, rem2, remLen, quo2, quoLen);
	CHECK(!memcmp(quo1, quo2, quoLen * 4), "div quotient %ld/%ld", an, bn);
	CHECK(!memcmp(rem1, rem2, remLen * 4), "div remainder %ld/%ld", an, bn);
	CHECK(!memcmp(div1, div2, divLen * 4), "div changed the divisor %ld/%ld", an, bn);
	if (qOut) memcpy(qOut, quo1, quoLen * 4);
	if (rOut) for (long i = 0; i < bn; i++) {
		u32 lo = GET(rem1, i), hi = GET(rem1, i + 1);
		PUT(rOut, i, d ? (lo >> d) | (hi << (32 - d)) : lo);
	}
	free(div1); free(div2); free(rem1); free(rem2); free(quo1); free(quo2);
	nDiv++;
	return 1;
}

static void normalizeTop(u32 *x, long n) { if (n && GET(x, n - 1) == 0) PUT(x, n - 1, 1 + (u32) rndBelow(0xFFFFFFFE)); }

/* random a and b */
static void testDivRandom(long count, long maxB, long maxQ)
{
	for (long c = 0; c < count; c++) {
		long bn = 1 + rndBelow(maxB), an = bn + rndBelow(maxQ);
		u32 *a = dalloc(an), *b = dalloc(bn);
		fill(a, an, (int) rnd()); fill(b, bn, (int) rnd());
		normalizeTop(a, an); normalizeTop(b, bn);
		if (rnd() % 8 == 0) {
			/* top digits of a equal those of b: the r1 = dh shortcut */
			long k = 1 + rndBelow(bn);
			for (long i = 0; i < k; i++) PUT(a, an - 1 - i, GET(b, bn - 1 - i));
		}
		divCase(a, an, b, bn, NULL, NULL);
		free(a); free(b);
	}
}

/* a = q * b + r with adversarial q, b, r: every quotient digit known, reconstruction checked */
static void testDivReconstruct(long count, long maxB, long maxQ)
{
	for (long c = 0; c < count; c++) {
		long bn = 1 + rndBelow(maxB), qn = 1 + rndBelow(maxQ), an = bn + qn;
		u32 *b = dalloc(bn), *q0 = dalloc(qn), *r0 = dalloc(bn), *a = dalloc(an + 1);
		fill(b, bn, (int) rnd()); fill(q0, qn, (int) rnd());
		normalizeTop(b, bn);
		switch (rnd() % 4) {   /* r0 < b */
		case 0: memset(r0, 0, bn * 4); break;                       /* exact division */
		case 1: memcpy(r0, b, bn * 4); for (long i = 0; i < bn; i++) { if (GET(r0, i)) { PUT(r0, i, GET(r0, i) - 1); break; } PUT(r0, i, 0xFFFFFFFFu); } break;   /* b - 1 */
		default: memcpy(r0, b, bn * 4); PUT(r0, bn - 1, GET(r0, bn - 1) - 1 - (u32) rndBelow(GET(r0, bn - 1))); fill(r0, bn - 1, (int) rnd()); break;
		}
		/* a = q0 * b + r0 */
		memset(a, 0, (an + 1) * 4);
		frozen_mul(q0, qn, b, bn, a, an);
		u64 cy = 0;
		for (long i = 0; i < an; i++) { cy += (u64) GET(a, i) + (i < bn ? GET(r0, i) : 0); PUT(a, i, (u32) cy); cy >>= 32; }
		PUT(a, an, (u32) cy);
		long aan = an + 1;
		while (aan > 0 && GET(a, aan - 1) == 0) aan--;
		if (aan < bn) { free(b); free(q0); free(r0); free(a); continue; }
		long quoLen = aan - bn + 1;
		u32 *q = dalloc(quoLen), *r = dalloc(bn);
		divCase(a, aan, b, bn, q, r);
		for (long i = 0; i < quoLen || i < qn; i++) {
			u32 got = i < quoLen ? GET(q, i) : 0, want = i < qn ? GET(q0, i) : 0;
			if (got != want) { CHECK(0, "reconstruct quotient digit %ld of %ld/%ld", i, aan, bn); break; }
		}
		CHECK(!memcmp(r, r0, bn * 4), "reconstruct remainder %ld/%ld", aan, bn);
		free(b); free(q0); free(r0); free(a); free(q); free(r);
	}
}

static void testDivLarge(void)
{
	static const long shapes[][2] = { { 600, 300 }, { 1024, 512 }, { 1025, 513 }, { 4096, 2048 }, { 4097, 1 }, { 4097, 2 },
		{ 5000, 3 }, { 3000, 2999 }, { 9000, 4500 }, { 8191, 129 }, { 2000, 1000 } };
	for (unsigned i = 0; i < sizeof shapes / sizeof *shapes; i++)
		for (int mode = 0; mode < 5; mode++) {
			long an = shapes[i][0], bn = shapes[i][1];
			u32 *a = dalloc(an), *b = dalloc(bn);
			fill(a, an, mode); fill(b, bn, mode + 2);
			normalizeTop(a, an); normalizeTop(b, bn);
			divCase(a, an, b, bn, NULL, NULL);
			free(a); free(b);
		}
	testDivReconstruct(40, 700, 700);
	printf("div large: %ld shapes x 5 patterns + 40 reconstructions\n", (long) (sizeof shapes / sizeof *shapes));
}

static void testDivNoMemory(void)
{
	long before = mallocCalls;
	mallocFails = 1;
	testDivReconstruct(1, 1, 1);   /* small: on the stack, malloc not called */
	/* (unless the build makes LI_STACK_LIMBS tiny: run-c-tests.sh's heap builds) */
	CHECK(LI_STACK_LIMBS < 16 || mallocCalls == before, "a small division asked for scratch memory");
	before = mallocCalls;
	long an = 3000, bn = 1100;
	u32 *a = dalloc(an), *b = dalloc(bn);
	fill(a, an, 0); fill(b, bn, 1); normalizeTop(a, an); normalizeTop(b, bn);
	divCase(a, an, b, bn, NULL, NULL);
	fill(b, 7, 0); normalizeTop(b, 7);
	divCase(a, an, b, 7, NULL, NULL);
	mallocFails = 0;
	CHECK(mallocCalls - before == 2, "the large divisions did not ask for scratch memory (%ld)", mallocCalls - before);
	nFallbackDiv += 2;
	free(a); free(b);
	printf("div with failing malloc: 2 cases\n");
}

/* shapes the plugin never prepares run the 32-bit loop: still identical */
static void testDivOffContract(void)
{
	u32 div[3], rem[5], quo[3], div2[3], rem2[5], quo2[3];
	/* divisor top bit clear */
	PUT(div, 0, 5); PUT(div, 1, 0x40000000u); PUT(div, 2, 0);
	PUT(rem, 0, 1); PUT(rem, 1, 2); PUT(rem, 2, 3); PUT(rem, 3, 4); PUT(rem, 4, 0);
	memcpy(div2, div, sizeof div); memcpy(rem2, rem, sizeof rem); memset(quo, 0, sizeof quo); memset(quo2, 0, sizeof quo2);
	frozen_div(div, 3, rem, 5, quo, 3);
	liDigitDiv(div2, 3, rem2, 5, quo2, 3);
	CHECK(!memcmp(quo, quo2, sizeof quo) && !memcmp(rem, rem2, sizeof rem), "off-contract division");
	nDiv++;
}

/* liReciprocal (Moller-Granlund algorithm 2 with 64-bit limbs) == the exact division */
static void testReciprocal(long count)
{
	liLimb top = (liLimb) 1 << (LI_LB - 1), ones = ~(liLimb) 0;
	liLimb edges[] = { top, top + 1, ones, ones - 1, top | (top >> 1), top | (ones >> (LI_LB / 2)), top | (top >> (LI_LB / 2)), ones << 8 };
	long n = 0, bad = 0;
	for (long i = 0; i < count + (long) (sizeof edges / sizeof *edges); i++) {
		liLimb d = i < (long) (sizeof edges / sizeof *edges) ? edges[i] : (liLimb) rnd() | top;
		if (i & 1) d = (d & ~(((liLimb) 1 << (rnd() % (LI_LB - 1))) - 1)) | top;   /* trailing zeros */
		if (i % 7 == 0) d |= ((liLimb) 1 << (rnd() % (LI_LB - 1))) - 1;            /* trailing ones */
		liLimb want = (liLimb) (((((liDLimb) ~d) << LI_LB) | ones) / d);
		n++;
		if (liReciprocal(d) != want && bad++ < 5) printf("FAIL: reciprocal of %llx\n", (unsigned long long) d);
	}
	fails += bad > 0;
	printf("reciprocal: %ld divisors, %ld wrong\n", n, bad);
}

/* ---------------------------------------------------------------- divide: Burnikel & Ziegler */

/* Divisors that make the estimates from the top half of a divisor too large: a top digit
   of 0x80000000 over zeros and ones, all ones, ones or zeros in the low half */
static void bzDivisor(u32 *b, long bn, int kind)
{
	long i;
	switch (kind % 7) {
	case 0: for (i = 0; i < bn; i++) PUT(b, i, 0); break;
	case 1: case 2: for (i = 0; i < bn; i++) PUT(b, i, 0xFFFFFFFFu); break;
	case 3: for (i = 0; i < bn; i++) PUT(b, i, i < bn / 2 ? 0xFFFFFFFFu : 0); break;
	case 4: fill(b, bn, (int) rnd()); for (i = 0; i < bn / 2; i++) PUT(b, i, 0xFFFFFFFFu); break;
	case 5: fill(b, bn, (int) rnd()); for (i = 0; i < bn / 2; i++) PUT(b, i, 0); break;
	default: fill(b, bn, (int) rnd()); break;
	}
	if (kind % 7 < 4 && kind % 7 != 2) PUT(b, bn - 1, 0x80000000u);
	if (kind % 7 >= 4) PUT(b, bn - 1, GET(b, bn - 1) | 0x80000000u);
	normalizeTop(b, bn);
}

/* Quotients with the largest digits: all ones, ones over random digits or zeros */
static void bzQuotient(u32 *q, long qn, int kind)
{
	long i;
	switch (kind % 5) {
	case 0: for (i = 0; i < qn; i++) PUT(q, i, 0xFFFFFFFFu); break;
	case 1: fill(q, qn, 0); for (i = qn / 2; i < qn; i++) PUT(q, i, 0xFFFFFFFFu); break;
	case 2: for (i = 0; i < qn; i++) PUT(q, i, i < qn / 2 ? 0 : 0xFFFFFFFFu); break;
	case 3: for (i = 0; i < qn; i++) PUT(q, i, 0xFFFFFFFFu - (u32) rndBelow(3)); break;
	default: fill(q, qn, (int) rnd()); break;
	}
}

/* a = q0 * b + r0 with r0 = b - 1, 0, b - 1 - small, or random below b: divide a by b,
   compare with the frozen loop (divCase) and with q0 and r0 */
static void bzReconstruct(const u32 *b, long bn, const u32 *q0, long qn, int rKind)
{
	long an = bn + qn, i;
	u32 *r0 = dalloc(bn), *a = dalloc(an + 1);
	u64 cy;
	memcpy(r0, b, bn * 4);
	switch (rKind % 4) {
	case 0:   /* b - 1 */
		for (i = 0; i < bn; i++) { if (GET(r0, i)) { PUT(r0, i, GET(r0, i) - 1); break; } PUT(r0, i, 0xFFFFFFFFu); }
		break;
	case 1: memset(r0, 0, bn * 4); break;
	case 2:   /* b - 1 - small, or 0 when b is smaller */
		cy = 1 + (u64) rndBelow(1000);
		for (i = 0; i < bn; i++) { u64 d = GET(r0, i); PUT(r0, i, (u32) (d - cy)); cy = d < cy; }
		if (cy) memset(r0, 0, bn * 4);
		break;
	default:
		fill(r0, bn - 1, (int) rnd());
		PUT(r0, bn - 1, (u32) rndBelow(GET(b, bn - 1)));
		break;
	}
	memset(a, 0, (an + 1) * 4);
	frozen_mul((unsigned int *) q0, qn, (unsigned int *) b, bn, a, an);
	cy = 0;
	for (i = 0; i < an; i++) { cy += (u64) GET(a, i) + (i < bn ? GET(r0, i) : 0); PUT(a, i, (u32) cy); cy >>= 32; }
	PUT(a, an, (u32) cy);
	long aan = an + 1;
	while (aan > 0 && GET(a, aan - 1) == 0) aan--;
	if (aan >= bn) {
		long quoLen = aan - bn + 1;
		u32 *q = dalloc(quoLen), *r = dalloc(bn);
		divCase(a, aan, b, bn, q, r);
		for (i = 0; i < quoLen || i < qn; i++) {
			u32 got = i < quoLen ? GET(q, i) : 0, want = i < qn ? GET(q0, i) : 0;
			if (got != want) { CHECK(0, "BZ reconstruct quotient digit %ld of %ld/%ld (thr %ld)", i, aan, bn, (long) liBZLimbs); break; }
		}
		CHECK(!memcmp(r, r0, bn * 4), "BZ reconstruct remainder %ld/%ld (thr %ld)", aan, bn, (long) liBZLimbs);
		free(q); free(r);
	}
	free(r0); free(a);
}

/* one adversarial division: a quotient of qn digits by a divisor of bn digits */
static void bzAdversarial(long bn, long qn)
{
	u32 *b = dalloc(bn), *q0 = dalloc(qn);
	int bKind = (int) rnd(), qKind = (int) rnd(), rKind = (int) rnd();
	bzDivisor(b, bn, bKind);
	bzQuotient(q0, qn, qKind);
	bzReconstruct(b, bn, q0, qn, rKind);
	free(b); free(q0);
}

/* one random division of an by bn digits, the top digits of a sometimes those of b */
static void bzRandom(long bn, long an)
{
	u32 *a = dalloc(an), *b = dalloc(bn);
	fill(a, an, (int) rnd()); fill(b, bn, (int) rnd());
	normalizeTop(a, an); normalizeTop(b, bn);
	if (rnd() % 4 == 0) {
		long k = 1 + rndBelow(bn);
		for (long i = 0; i < k; i++) PUT(a, an - 1 - i, GET(b, bn - 1 - i));
	}
	divCase(a, an, b, bn, NULL, NULL);
	free(a); free(b);
}

/* divisors of LI_BZ_LIMBS - 1 .. 5 x LI_BZ_LIMBS + 3 limbs (odd sizes among them),
   quotients of 1, 2, vn - 1, vn, vn + 1, 3vn + 5 and 10vn limbs, every partial-limb
   offset of both; random and adversarial */
static void testDivBZThreshold(long reps)
{
	long T = LI_BZ_LIMBS, D = LI_DIGITS_PER_LIMB, n = 0;
	long vls[] = { T - 1, T, T + 1, 2 * T - 1, 2 * T, 2 * T + 1, 3 * T + 2, 5 * T + 3 };
	for (unsigned i = 0; i < sizeof vls / sizeof *vls; i++) {
		long vl = vls[i];
		long qls[] = { 1, 2, vl - 1, vl, vl + 1, 3 * vl + 5, 10 * vl };
		for (unsigned j = 0; j < sizeof qls / sizeof *qls; j++)
			for (long r = 0; r < reps; r++)
				for (int ds = 0; ds < D; ds++)
					for (int dq = 0; dq < D; dq++) {
						long bn = vl * D - ds, qn = qls[j] * D - dq;
						bzRandom(bn, bn + qn - 1);
						bzAdversarial(bn, qn);
						n += 2;
					}
	}
	printf("div BZ threshold straddle (T=%ld limbs): %ld cases\n", T, n);
}

/* short quotients around LI_BZ_QUOTIENT_LIMBS (Tq) over divisors of 2Tq - 1 .. 3T limbs,
   and divisors Tq - 1, Tq and Tq + 1 limbs longer than the quotient, every partial-limb
   offset of both; random and adversarial */
static void testDivBZShortQuotient(long reps)
{
	long T = LI_BZ_LIMBS, Tq = LI_BZ_QUOTIENT_LIMBS, D = LI_DIGITS_PER_LIMB, n = 0;
	long vls[] = { 2 * Tq - 1, 2 * Tq, 2 * Tq + 1, T, T + 1, 3 * T };
	for (unsigned i = 0; i < sizeof vls / sizeof *vls; i++) {
		long vl = vls[i];
		long qls[] = { Tq - 1, Tq, Tq + 1, vl - Tq - 1, vl - Tq, vl - Tq + 1 };
		for (unsigned j = 0; j < sizeof qls / sizeof *qls; j++) {
			if (qls[j] < 1) continue;
			for (long r = 0; r < reps; r++)
				for (int ds = 0; ds < D; ds++)
					for (int dq = 0; dq < D; dq++) {
						long bn = vl * D - ds, qn = qls[j] * D - dq;
						bzRandom(bn, bn + qn - 1);
						bzAdversarial(bn, qn);
						n += 2;
					}
		}
	}
	printf("div BZ short quotients (Tq=%ld limbs): %ld cases\n", Tq, n);
}

/* the recursion at thresholds 2..9 limbs, sometimes with Karatsuba at 2..6 limbs, on small
   random and adversarial operands: every split, odd halves, top blocks of 1 .. vn limbs */
static void testDivBZForced(long count)
{
	long thr0 = liBZLimbs, qthr0 = liBZQuotientLimbs, kara0 = liKaratsubaLimbs;
	for (long c = 0; c < count; c++) {
		static const long thr[] = { 2, 3, 4, 5, 6, 9 }, kara[] = { 2, 3, 4, 6 };
		liBZLimbs = thr[rnd() % 6];
		liBZQuotientLimbs = rnd() % 2 ? qthr0 : 1 + rndBelow(6);
		liKaratsubaLimbs = rnd() % 3 ? kara0 : kara[rnd() % 4];
		long bn = 1 + rndBelow(70);
		long qn = 1 + (rnd() % 4 ? rndBelow(2 * bn + 4) : rndBelow(12 * bn));
		if (c & 1) bzAdversarial(bn, qn);
		else bzRandom(bn, bn + qn - 1);
	}
	liBZLimbs = thr0;
	liBZQuotientLimbs = qthr0;
	liKaratsubaLimbs = kara0;
	printf("div BZ forced at 2..9 limbs (short quotients from 1..6): %ld cases\n", count);
}

/* operands up to 20000 digits, at the production thresholds */
static void testDivBZLarge(void)
{
	static const long shapes[][2] = { { 16385, 8192 }, { 8193, 4096 }, { 20000, 2000 }, { 12000, 7001 }, { 5000, 4900 }, { 9999, 333 } };
	for (unsigned i = 0; i < sizeof shapes / sizeof *shapes; i++) {
		long an = shapes[i][0], bn = shapes[i][1];
		bzRandom(bn, an);
		bzAdversarial(bn, an - bn + 1);
	}
	printf("div BZ large: %ld shapes, random and adversarial\n", (long) (sizeof shapes / sizeof *shapes));
}

/* the scratch of the recursion alone sends a division to malloc (Knuth's algorithm D
   would run on the stack): when the malloc fails, the 32-bit loop answers */
static void testDivBZNoMemory(void)
{
	long D = LI_DIGITS_PER_LIMB, vl = (long) LI_STACK_LIMBS / 4 - 2, before;
	if (vl < LI_BZ_LIMBS) vl = LI_BZ_LIMBS;   /* a tiny LI_STACK_LIMBS: on the heap anyway */
	before = mallocCalls;
	mallocFails = 1;
	bzAdversarial(vl * D, vl * D);
	bzRandom(vl * D, 2 * vl * D);
	bzRandom(2 * LI_BZ_LIMBS * D, 22 * LI_BZ_LIMBS * D);
	mallocFails = 0;
	CHECK(mallocCalls - before == 3, "the BZ divisions did not ask for scratch memory (%ld)", mallocCalls - before);
	nFallbackDiv += 3;
	printf("div BZ with failing malloc: 3 cases\n");
}

/* limbs at their exact size, for ASan */
static liLimb *lalloc(long n) { return malloc((n ? n : 1) * sizeof(liLimb)); }
static void limbFill(liLimb *x, long n, int mode)
{
	for (long i = 0; i < n; i++) {
		switch (mode % 4) {
		case 0: x[i] = (liLimb) rnd(); break;
		case 1: x[i] = ~(liLimb) 0; break;
		case 2: x[i] = rnd() % 3 ? 0 : (liLimb) rnd(); break;
		default: x[i] = rnd() % 2 ? ~(liLimb) 0 : (liLimb) patDigit() << (LI_LB - 32); break;
		}
	}
}

/* liDivRemBZ called directly with liDivRem's contract, the top vn limbs of u below v, where
   liDigitDiv has the top limb of u below v's: dividends whose top limbs equal the divisor's
   down to a lower limb, so that the top block's quotient (qb < vn limbs) reaches B^qb and
   is corrected, which liDigitDiv never needs.  Against Knuth's algorithm D (liDivRem) and
   the reconstruction u = q v + r with r < v; thresholds forced or at production. */
static void testDivBZDirect(long count)
{
	long thr0 = liBZLimbs, kara0 = liKaratsubaLimbs, nTop = 0;
	for (long c = 0; c < count; c++) {
		static const long thr[] = { 2, 3, 4, 5, 6, 9 }, kara[] = { 2, 3, 4, 6 };
		int prod = c % 8 == 0;
		liBZLimbs = prod ? thr0 : thr[rnd() % 6];
		liKaratsubaLimbs = prod || rnd() % 3 ? kara0 : kara[rnd() % 4];
		long vn = prod ? thr0 + rndBelow(3 * thr0) : 1 + rndBelow(40);
		long qn = 1 + (rnd() % 2 ? rndBelow(vn) : rndBelow(3 * vn + 3)), un = vn + qn, i, p;
		long qb = qn % vn == 0 ? vn : qn % vn;
		liLimb *v = lalloc(vn), *u = lalloc(un), *u0 = lalloc(un), *u1 = lalloc(un), *q = lalloc(qn), *q1 = lalloc(qn);
		liLimb *t = lalloc(un), *scratch = lalloc(liDivRemBZScratch(qn, vn)), cy;
		limbFill(v, vn, (int) rnd());
		v[vn - 1] |= (liLimb) 1 << (LI_LB - 1);
		limbFill(u, qn, (int) rnd());
		/* the top vn limbs of u: v's above limb p, below v's at p (the lowest nonzero limb of
		   v at or above the chosen one), anything under it; p = 0 most often */
		p = rnd() % 2 ? 0 : rndBelow(vn);
		while (v[p] == 0) p++;
		for (i = vn - 1; i > p; i--) u[qn + i] = v[i];
		u[qn + p] = (liLimb) rnd() % v[p];
		limbFill(u + qn, p, (int) rnd());
		if (rnd() % 4 == 0) {
			/* liDigitDiv's contract instead: the top limb below v's */
			limbFill(u + qn, vn, (int) rnd());
			u[un - 1] = (liLimb) rnd() % v[vn - 1];
		}
		memcpy(u0, u, un * sizeof(liLimb));
		memcpy(u1, u, un * sizeof(liLimb));
		if (qb < vn && liCmp(u + un - qb, v + vn - qb, qb) >= 0) nTop++;
		liDivRemBZ(q, u, un, v, vn, scratch);
		liDivRem(q1, u1, un, v, vn);
		CHECK(!memcmp(q, q1, qn * sizeof(liLimb)) && !memcmp(u, u1, un * sizeof(liLimb)),
			"liDivRemBZ against liDivRem %ld/%ld limbs (thr %ld)", un, vn, (long) liBZLimbs);
		for (i = vn; i < un && u[i] == 0; i++) ;
		CHECK(i == un && liCmp(u, v, vn) < 0, "liDivRemBZ remainder %ld/%ld limbs (thr %ld)", un, vn, (long) liBZLimbs);
		if (qn >= vn) liMulBasecase(t, q, qn, v, vn); else liMulBasecase(t, v, vn, q, qn);
		cy = liAddN(t, t, u, vn);
		cy = liAdd1(t + vn, t + vn, qn, cy);
		CHECK(cy == 0 && !memcmp(t, u0, un * sizeof(liLimb)), "liDivRemBZ q v + r %ld/%ld limbs (thr %ld)", un, vn, (long) liBZLimbs);
		free(v); free(u); free(u0); free(u1); free(q); free(q1); free(t); free(scratch);
		nDiv++;
	}
	liBZLimbs = thr0;
	liKaratsubaLimbs = kara0;
	CHECK(nTop > count / 20, "too few top blocks whose quotient reached B^qb (%ld)", nTop);
	printf("div liDivRemBZ direct: %ld cases, %ld top blocks with a quotient of B^qb or more\n", count, nTop);
}

/* the scratch never asks for more bytes than a size_t holds, and stays on the stack up to
   LI_STACK_LIMBS limbs */
static void testScratchLimit(void)
{
	liLimb stackLimbs[1], *p;
	unsigned long long most = (unsigned long long) (((size_t) -1) / sizeof(liLimb));
	long before = mallocCalls;
	CHECK(liScratch(LI_STACK_LIMBS, stackLimbs) == stackLimbs, "LI_STACK_LIMBS limbs not on the stack");
	p = liScratch(LI_STACK_LIMBS + 1, stackLimbs);
	CHECK(p != 0 && p != stackLimbs && mallocCalls == before + 1, "LI_STACK_LIMBS + 1 limbs not malloc'ed");
	if (p != stackLimbs) free(p);
	before = mallocCalls;
	if (sizeof(sqInt) == 8 && most < 0x7FFFFFFFFFFFFFFFull) {
		CHECK(liScratch((sqInt) (most + 1), stackLimbs) == 0 && liScratch((sqInt) 0x7FFFFFFFFFFFFFFFull, stackLimbs) == 0,
			"a scratch larger than a size_t holds did not answer 0");
	}
	CHECK(mallocCalls == before, "a scratch larger than a size_t holds called malloc");
	printf("scratch: stack, heap and over-size_t limits\n");
}

/* every add-back loop of the recursion ran, some twice, none three times; restart the counts */
static void checkAddBacks(const char *what)
{
	printf("div BZ add-backs, %s [step][once, twice, more]: %ld %ld %ld / %ld %ld %ld / %ld %ld %ld\n", what,
		nAddBack[0][0], nAddBack[0][1], nAddBack[0][2], nAddBack[1][0], nAddBack[1][1], nAddBack[1][2],
		nAddBack[2][0], nAddBack[2][1], nAddBack[2][2]);
	CHECK(nAddBack[0][0] > 0 && nAddBack[1][0] > 0 && nAddBack[2][0] > 0, "an add-back loop never ran");
	CHECK(nAddBack[0][1] + nAddBack[1][1] + nAddBack[2][1] > 0, "no step needed two add-backs");
	CHECK(nAddBack[0][2] + nAddBack[1][2] + nAddBack[2][2] == 0, "a step needed more than two add-backs");
	memset(nAddBack, 0, sizeof nAddBack);
}

/* ---------------------------------------------------------------- Montgomery */

static long nMont, nMontSqr, nMontWrong;

/* -1/m0 mod 2^32 for an odd m0, as the image computes mInv */
static u32 montInv(u32 m0)
{
	u32 x = 1;
	for (int i = 0; i < 5; i++) x *= 2 - m0 * x;
	return 0 - x;
}

/* An odd modulus of n digits: 0 random, 1 all ones (2^32n - 1), 2 2^k - 1 with k bits in
   the top digit, 3 top digit FFFFFFFF, 4 top digit 0 (unnormalized: the plugin takes the
   digit count from the byte size), 5 top digit 1, 6 adversarial digits, 7 2^(32n - 32) + 1
   (all zero but the ends) */
static void montModulus(u32 *m, long n, int kind)
{
	kind %= 8;
	switch (kind) {
	case 1: case 2: fill(m, n, 2); break;
	case 6: fill(m, n, 1); break;
	case 7: memset(m, 0, n * 4); PUT(m, n - 1, 1); break;
	default: fill(m, n, 0); break;
	}
	if (kind == 2) PUT(m, n - 1, (u32) (0xFFFFFFFFu >> rndBelow(32)));
	if (kind == 3) PUT(m, n - 1, 0xFFFFFFFFu);
	if (kind == 4 && n > 1) PUT(m, n - 1, 0);
	if (kind == 5 && n > 1) PUT(m, n - 1, 1);
	PUT(m, 0, GET(m, 0) | 1);
}

/* One Montgomery product, as the plugin calls it: a (fl digits), b (sl digits) and m (n
   digits), the kernel against the frozen loop, both into n zero digits, the operands
   untouched.  aMode/bMode: fill() modes, or 5 for a copy of m (fl = n), 6 for all ones,
   7 for 1.  share: 0 two operands, 1 b is a (one pointer, as x * x), 2 b a copy of a.
   inv: 0 the right inverse; 1 a random one, 2 the right one with one bit flipped, 3 zero,
   4 an even m (with the inverse of m + 1): the loop's own answer.  Answer the result in
   res (n digits) when it is not NULL. */
static void montCase(long n, long fl, long sl, int aMode, int bMode, int mKind, int share, int inv, u32 *res)
{
	u32 *m = dalloc(n), *a, *b, *r1 = dalloc(n), *r2 = dalloc(n), *a0, *b0, *m0, mInv;

	if (aMode == 5) fl = n;
	if (fl > n) fl = n;
	if (sl > n) sl = n;
	if (share) sl = fl;
	if (fl > 0 && sl == 0) sl = 1;
	montModulus(m, n, mKind);
	a = dalloc(fl);
	if (aMode == 5) memcpy(a, m, n * 4);
	else if (aMode == 6) fill(a, fl, 2);
	else if (aMode == 7) { memset(a, 0, fl * 4); if (fl) PUT(a, 0, 1); }
	else fill(a, fl, aMode);
	if (share == 1) b = a;
	else {
		b = dalloc(sl);
		if (share == 2) memcpy(b, a, fl * 4);
		else if (bMode == 6) fill(b, sl, 2);
		else if (bMode == 7) { memset(b, 0, sl * 4); if (sl) PUT(b, 0, 1); }
		else fill(b, sl, bMode % 5);
	}
	mInv = montInv(GET(m, 0));
	switch (inv) {
	case 1: mInv = (u32) rnd(); break;
	case 2: mInv ^= 1u << rndBelow(32); break;
	case 3: mInv = 0; break;
	case 4: PUT(m, 0, GET(m, 0) + 1); mInv = montInv(GET(m, 0) + 1); break;
	}
	a0 = dalloc(fl); b0 = dalloc(sl); m0 = dalloc(n);
	memcpy(a0, a, fl * 4); memcpy(b0, b, sl * 4); memcpy(m0, m, n * 4);
	memset(r1, 0, n * 4);
	memset(r2, 0, n * 4);
	frozen_mont(a0, fl, b0, sl, m0, n, mInv, r1);
	liDigitMontgomery(a, fl, b, sl, m, n, mInv, r2);
	CHECK(!memcmp(r1, r2, n * 4), "mont %ld digits, %ld x %ld, modes %d/%d, modulus %d, share %d, inverse %d, thresholds %ld/%ld/%ld",
		n, fl, sl, aMode, bMode, mKind, share, inv, (long) liMontLoopDigits, (long) liMontMulLimbs, (long) liMontSqrLimbs);
	CHECK(!memcmp(a, a0, fl * 4) && !memcmp(b, b0, sl * 4) && !memcmp(m, m0, n * 4), "mont %ld changed an operand", n);
	if (res) memcpy(res, r1, n * 4);
	nMont++;
	nMontSqr += share != 0;
	nMontWrong += inv != 0 && GET(m, 0) * mInv != 0xFFFFFFFFu;
	free(m); free(a); if (share != 1) free(b); free(r1); free(r2); free(a0); free(b0); free(m0);
}

/* the random draws of a case in a fixed order (see mulRandomCase); n digits, operands
   mostly of n digits */
static void montRandomCase(long n, int wrong)
{
	long fl = rnd() % 4 ? n : rndBelow(n + 1);
	long sl = rnd() % 4 ? n : 1 + rndBelow(n);
	int aMode = (int) (rnd() % 8), bMode = (int) (rnd() % 8), mKind = (int) rnd();
	int share = rnd() % 3 ? 0 : 1 + (int) (rnd() % 2);
	int inv = wrong ? 1 + (int) (rnd() % 4) : 0;
	montCase(n, fl, sl, aMode, bMode, mKind, share, inv, NULL);
}

static void montForceThresholds(void)
{
	static const long loops[] = { 1, 2, 3, 5 }, muls[] = { 1, 2, 3, 4, 6, 9 }, sqrs[] = { 1, 2, 3, 5 }, kara[] = { 2, 3, 4, 6 };
	liMontLoopDigits = loops[rnd() % 4];
	liMontMulLimbs = muls[rnd() % 6];
	liMontSqrLimbs = sqrs[rnd() % 4];
	liKaratsubaLimbs = rnd() % 2 ? kara[rnd() % 4] : LI_KARATSUBA_LIMBS;
	liSqrKaratsubaLimbs = rnd() % 2 ? kara[rnd() % 4] : LI_SQR_KARATSUBA_LIMBS;
	liSqrBasecaseLimbs = rnd() % 2 ? 1 + rndBelow(3) : LI_SQR_BASECASE_LIMBS;
}

static void montProductionThresholds(void)
{
	liMontLoopDigits = LI_MONT_LOOP_DIGITS;
	liMontMulLimbs = LI_MONT_MUL_LIMBS;
	liMontSqrLimbs = LI_MONT_SQR_LIMBS;
	liKaratsubaLimbs = LI_KARATSUBA_LIMBS;
	liSqrKaratsubaLimbs = LI_SQR_KARATSUBA_LIMBS;
	liSqrBasecaseLimbs = LI_SQR_BASECASE_LIMBS;
}

/* random sizes up to 300 digits (most up to 40): the right inverse, at the production
   thresholds and forced ones (CIOS from 1 digit, REDC from 1 limb, Karatsuba inside it);
   a quarter of them with a wrong inverse or an even modulus */
static void testMontRandom(long count)
{
	for (long c = 0; c < count; c++) {
		long n = c % 5 ? 1 + rndBelow(40) : 1 + rndBelow(300);
		if (c % 2) montForceThresholds();
		else montProductionThresholds();
		montRandomCase(n, c % 4 == 3);
	}
	montProductionThresholds();
	printf("mont random + forced thresholds: %ld cases\n", count);
}

/* every size from 1 to 300 digits, odd and even, with each path's thresholds */
static void testMontSizes(void)
{
	long n0 = nMont;
	for (long n = 1; n <= 300; n++)
		for (int k = 0; k < 4; k++) {
			montProductionThresholds();
			if (k == 1) { liMontLoopDigits = 1; liMontMulLimbs = liMontSqrLimbs = 1 << 30; }   /* CIOS */
			if (k == 2) { liMontLoopDigits = 1; liMontMulLimbs = liMontSqrLimbs = 1; }         /* REDC */
			/* one draw per statement (see mulRandomCase) */
			int aMode = (int) (rnd() % 5), bMode = (int) (rnd() % 5), mKind = (int) rnd();
			montCase(n, n, n, aMode, bMode, mKind, k == 3, 0, NULL);
			if (k == 2) {
				int share = 1 + (int) (rnd() % 2);
				montCase(n, n, n, aMode, 0, mKind, share, 0, NULL);
			}
		}
	montProductionThresholds();
	printf("mont sizes 1..300 digits, loop / CIOS / REDC / square: %ld cases\n", nMont - n0);
}

/* sizes straddling the production thresholds, every partial-limb offset */
static void testMontThreshold(long reps)
{
	long D = LI_DIGITS_PER_LIMB, n0 = nMont;
	long centres[] = { LI_MONT_SQR_LIMBS, LI_MONT_MUL_LIMBS, LI_SQR_KARATSUBA_LIMBS, LI_KARATSUBA_LIMBS, 2 * LI_KARATSUBA_LIMBS };
	montProductionThresholds();
	for (long d = LI_MONT_LOOP_DIGITS - 3; d <= LI_MONT_LOOP_DIGITS + 3; d++)
		for (long r = 0; r < 4 * reps; r++)
			if (d >= 1) montRandomCase(d, 0);
	for (unsigned c = 0; c < sizeof centres / sizeof *centres; c++)
		for (long l = centres[c] - 3; l <= centres[c] + 3; l++)
			for (long r = 0; r < reps; r++)
				for (int ds = 0; ds < D; ds++) {
					long n = l * D - ds;
					if (n < 1) continue;
					int aMode = (int) (rnd() % 8), bMode = (int) (rnd() % 8), mKind = (int) rnd();
					int share = 1 + (int) (rnd() % 2);
					long fl = 1 + rndBelow(n), sl = 1 + rndBelow(n);
					montCase(n, n, n, aMode, bMode, mKind, 0, 0, NULL);
					montCase(n, n, n, aMode % 7, 0, mKind + 1, share, 0, NULL);
					montCase(n, fl, sl, aMode % 5, bMode % 5, mKind + 2, 0, 0, NULL);
				}
	printf("mont threshold straddle (loop below %d digits, square REDC from %d, REDC from %d limbs): %ld cases\n",
		LI_MONT_LOOP_DIGITS, LI_MONT_SQR_LIMBS, LI_MONT_MUL_LIMBS, nMont - n0);
}

/* moduli 2^k - 1 and with a top digit FFFFFFFF, 1 or 0; operands all ones (T at or above
   2^32n: the loop's lastDigit), m itself (T = m: the subtraction leaves 0), 1 (taking a
   value out of Montgomery form), 0 and short; squares of all ones */
static void testMontPatterns(void)
{
	long n0 = nMont;
	montProductionThresholds();
	static const long sizes[] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17, 19, 20, 21, 23, 24, 25, 31, 32, 33,
		47, 48, 49, 63, 64, 65, 71, 72, 73, 95, 96, 97, 127, 128, 129, 143, 144, 145, 199, 255, 256, 257, 300 };
	for (unsigned i = 0; i < sizeof sizes / sizeof *sizes; i++) {
		long n = sizes[i];
		for (int mk = 0; mk < 8; mk++) {
			u32 r[300];
			montCase(n, n, n, 6, 6, mk, 0, 0, NULL);          /* all ones */
			montCase(n, n, n, 6, 0, mk, 1, 0, NULL);          /* all ones squared */
			montCase(n, n, n, 5, 0, mk, 0, 0, r);             /* m b: T = m */
			for (long k = 0; k < n; k++) CHECK(GET(r, k) == 0, "mont m * b, modulus %d, %ld digits: not 0", mk, n);
			montCase(n, n, 1, 0, 7, mk, 0, 0, NULL);          /* x * 1 */
			montCase(n, 0, 0, 0, 0, mk, 0, 0, NULL);          /* empty operands */
			long fl = 1 + rndBelow(n), sl = 1 + rndBelow(n);
			montCase(n, 0, sl, 0, 0, mk, 0, 0, NULL);
			montCase(n, fl, sl, 6, 6, mk, 0, 0, NULL);
			montCase(n, n, n, 3, 3, mk, 0, 0, NULL);          /* sparse digits */
		}
	}
	printf("mont patterns: %ld cases\n", nMont - n0);
}

/* a wrong inverse or an even modulus: the loop's answer, at every threshold */
static void testMontWrongInverse(long count)
{
	long n0 = nMont;
	for (long c = 0; c < count; c++) {
		long n = c % 4 ? 1 + rndBelow(40) : 1 + rndBelow(300);
		if (c % 2) montForceThresholds();
		else montProductionThresholds();
		montRandomCase(n, 1);
	}
	montProductionThresholds();
	printf("mont wrong inverse or even modulus: %ld cases\n", nMont - n0);
}

static void testMontLarge(void)
{
	static const long sizes[] = { 513, 1000, 1001, 2000, 2001, 5000, 5001 };
	long n0 = nMont;
	montProductionThresholds();
	for (unsigned i = 0; i < sizeof sizes / sizeof *sizes; i++) {
		long n = sizes[i];
		montCase(n, n, n, 0, 0, (int) i, 0, 0, NULL);
		montCase(n, n, n, (int) i % 5, 0, (int) i + 1, 1, 0, NULL);
		montCase(n, n, n, 6, 0, 3, 2, 0, NULL);
		if (n < 3000) {
			montCase(n, n / 3, n, 4, 1, (int) i + 2, 0, 0, NULL);
			montCase(n, n, n, 0, 0, (int) i, 0, 2, NULL);
		}
	}
	printf("mont large: %ld cases up to %ld digits\n", nMont - n0, sizes[sizeof sizes / sizeof *sizes - 1]);
}

/* the scratch malloc fails: the loop runs instead; small ones stay on the stack */
static void testMontNoMemory(void)
{
	long before = mallocCalls;
	montProductionThresholds();
	mallocFails = 1;
	montCase(8, 8, 8, 0, 0, 0, 0, 0, NULL);
	CHECK(LI_STACK_LIMBS < 16 || mallocCalls == before, "a small Montgomery product asked for scratch memory");
	before = mallocCalls;
	montCase(700, 700, 700, 0, 1, 0, 0, 0, NULL);          /* REDC */
	montCase(1200, 1200, 1200, 2, 0, 3, 1, 0, NULL);       /* REDC, square */
	liMontMulLimbs = liMontSqrLimbs = 1 << 30;
	montCase(701, 701, 333, 0, 1, 0, 0, 0, NULL);          /* CIOS */
	montProductionThresholds();
	mallocFails = 0;
	CHECK(mallocCalls - before == 3, "the large Montgomery products did not ask for scratch memory (%ld)", mallocCalls - before);
	printf("mont with failing malloc: 3 cases\n");
}

/* one operand shorter than the other, in either order (the kernel scans the longer one),
   at the production thresholds: of 1 or 2 digits, and of about half the modulus, where the
   product then REDC takes over from CIOS (from LI_MONT_SQR_LIMBS limbs, for a shorter
   operand of at most half the modulus's limbs, rounded up); odd and even digit counts;
   operands all ones; and a wrong inverse on the same shapes */
static void testMontShort(void)
{
	long D = LI_DIGITS_PER_LIMB, n0 = nMont;
	long centres[] = { LI_MONT_LOOP_DIGITS, LI_MONT_SQR_LIMBS * D, LI_MONT_MUL_LIMBS * D, 2 * LI_MONT_MUL_LIMBS * D + 1 };
	montProductionThresholds();
	for (unsigned c = 0; c < sizeof centres / sizeof *centres; c++)
		for (long n = centres[c] - 3; n <= centres[c] + 3; n++) {
			long limbs = (n + D - 1) / D;
			long shorts[] = { 1, 2, (limbs / 2 - 1) * D, limbs / 2 * D, (limbs + 1) / 2 * D, (limbs + 1) / 2 * D + 1,
				(limbs / 2 + 1) * D + 1, n - 1 };
			if (n < 1) continue;
			for (unsigned k = 0; k < sizeof shorts / sizeof *shorts; k++) {
				long sl = shorts[k];
				if (sl < 1 || sl > n) continue;
				int aMode = (int) (rnd() % 5), bMode = (int) (rnd() % 5), mKind = (int) rnd();
				montCase(n, n, sl, aMode, bMode, mKind, 0, 0, NULL);
				montCase(n, sl, n, aMode, bMode, mKind + 1, 0, 0, NULL);
				montCase(n, sl, n, 6, 6, mKind + 2, 0, 0, NULL);
				montCase(n, n, sl, 6, 6, mKind + 3, 0, 0, NULL);
				montCase(n, sl, sl, aMode, 6, mKind + 4, 0, 0, NULL);
				montCase(n, sl, n - (n > sl), bMode, aMode, mKind + 5, 0, 0, NULL);
				montCase(n, sl, n, aMode, bMode, mKind + 6, 0, 2, NULL);
			}
		}
	printf("mont short operands, either order: %ld cases\n", nMont - n0);
}

/* ---------------------------------------------------------------- dump for oracle.py */

static void hex(const u32 *d, long n)
{
	long i = n - 1;
	while (i > 0 && GET(d, i) == 0) i--;
	if (n == 0) { printf(" 0"); return; }
	printf(" %x", GET(d, i));
	for (i--; i >= 0; i--) printf("%08x", GET(d, i));
}

static void dump(long n)
{
	for (long c = 0; c < n; c++) {
		static const long sz[] = { 1, 2, 3, 5, 17, 47, 48, 49, 50, 96, 97, 300, 1000, 2500 };
		/* one draw per declarator: the operands of + are evaluated in no fixed order */
		long sn = sz[rnd() % 14];
		sn += rndBelow(3);
		long ln = sz[rnd() % 14];
		ln += rndBelow(3);
		long resLen = pluginResLen(sn, ln);
		u32 *s = dalloc(sn), *l = dalloc(ln), *r = dalloc(resLen);
		fill(s, sn, (int) rnd()); fill(l, ln, (int) rnd()); garbage(r, resLen);
		liDigitMultiply(s, sn, l, ln, r, resLen);
		printf("M"); hex(s, sn); hex(l, ln); printf(" %ld", resLen); hex(r, resLen); printf("\n");
		free(s); free(l); free(r);

		long bn = sz[rnd() % 14];
		bn += rndBelow(3);
		long an = bn + sz[rnd() % 14] - 1;
		u32 *a = dalloc(an), *b = dalloc(bn), *q = dalloc(an - bn + 1), *rr = dalloc(bn);
		fill(a, an, (int) rnd()); fill(b, bn, (int) rnd()); normalizeTop(a, an); normalizeTop(b, bn);
		divCase(a, an, b, bn, q, rr);
		printf("D"); hex(a, an); hex(b, bn); hex(q, an - bn + 1); hex(rr, bn); printf("\n");
		free(a); free(b); free(q); free(rr);

		/* a square, around the squaring thresholds of both limb sizes, through one pointer
		   or a copy */
		static const long sq[] = { 1, 2, 3, 5, 6, 7, 8, 9, 16, 47, 48, 49, 87, 88, 89, 95, 96, 97, 175, 176, 177, 300, 1000, 2500 };
		long xn = sq[rnd() % 24];
		xn += rndBelow(3);
		int copy = (int) (rnd() & 1);
		resLen = pluginResLen(xn, xn);
		u32 *x = dalloc(xn), *y = copy ? dalloc(xn) : x;
		r = dalloc(resLen);
		fill(x, xn, (int) rnd());
		if (copy) memcpy(y, x, xn * 4);
		garbage(r, resLen);
		liDigitMultiply(x, xn, y, xn, r, resLen);
		printf("M"); hex(x, xn); hex(y, xn); printf(" %ld", resLen); hex(r, resLen); printf("\n");
		free(x); if (copy) free(y); free(r);
	}
}

/* divisions over the Burnikel & Ziegler threshold, at the production one and forced */
static void dumpDivBZ(long n)
{
	long thr0 = liBZLimbs, qthr0 = liBZQuotientLimbs, D = LI_DIGITS_PER_LIMB;
	for (long c = 0; c < n; c++) {
		static const long thr[] = { 2, 3, 5 };
		liBZLimbs = c % 4 ? thr0 : thr[rnd() % 3];
		liBZQuotientLimbs = c % 4 ? qthr0 : 1 + rndBelow(4);
		long bn = liBZLimbs * D - 1 + rndBelow(4 * liBZLimbs * D);
		long qn = 1 + (rnd() % 2 ? rndBelow(2 * bn) : rndBelow(10 * bn));
		long an = bn + qn - 1;
		u32 *a = dalloc(an), *b = dalloc(bn), *q = dalloc(qn), *rr = dalloc(bn);
		if (c % 3) {
			fill(a, an, (int) rnd()); fill(b, bn, (int) rnd());
		}
		else {
			/* the largest quotient for this divisor: the add-backs */
			bzDivisor(b, bn, (int) rnd());
			for (long i = 0; i < an; i++) PUT(a, i, i < bn - 1 ? 0 : 0xFFFFFFFFu);
			PUT(a, an - 1, GET(b, bn - 1) - 1);
		}
		normalizeTop(a, an); normalizeTop(b, bn);
		divCase(a, an, b, bn, q, rr);
		printf("D"); hex(a, an); hex(b, bn); hex(q, qn); hex(rr, bn); printf("\n");
		free(a); free(b); free(q); free(rr);
	}
	liBZLimbs = thr0;
	liBZQuotientLimbs = qthr0;
}

/* Montgomery products with the right inverse, for oracle.py: around the thresholds of both
   limb sizes, up to 1000 digits, odd and even, squares, short operands, all-ones
   operands and special moduli */
static void dumpMont(long count)
{
	static const long sz[] = { 1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 19, 20, 21, 23, 24, 25, 47, 48, 49, 71, 72, 73,
		87, 88, 89, 143, 144, 145, 300, 1000 };
	for (long c = 0; c < count; c++) {
		long n = sz[rnd() % 32];
		if (c % 2) montForceThresholds();
		else montProductionThresholds();
		long fl = rnd() % 4 ? n : rndBelow(n + 1);
		long sl = rnd() % 4 ? n : 1 + rndBelow(n);
		int aMode = (int) (rnd() % 7), bMode = (int) (rnd() % 8), mKind = (int) rnd();
		int share = rnd() % 3 ? 0 : 1 + (int) (rnd() % 2);
		u32 *a = dalloc(n), *b = dalloc(n), *m = dalloc(n), *r = dalloc(n);
		if (share) sl = fl;
		if (fl > 0 && sl == 0) sl = 1;
		montModulus(m, n, mKind);
		if (aMode == 5) { fl = n; if (share) sl = n; memcpy(a, m, n * 4); }
		else fill(a, fl, aMode == 6 ? 2 : aMode);
		if (share == 1) memcpy(b, a, fl * 4);
		else if (share == 2) memcpy(b, a, fl * 4);
		else fill(b, sl, bMode == 6 ? 2 : bMode % 5);
		memset(r, 0, n * 4);
		liDigitMontgomery(a, fl, share == 1 ? a : b, sl, m, n, montInv(GET(m, 0)), r);
		printf("G"); hex(a, fl); hex(b, sl); hex(m, n); printf(" %ld", n); hex(r, n); printf("\n");
		free(a); free(b); free(m); free(r);
	}
	montProductionThresholds();
}

int main(int argc, char **argv)
{
	if (argc > 2 && !strcmp(argv[1], "dump")) {
		dump(atol(argv[2]));
		dumpDivBZ(atol(argv[2]) / 3);
		dumpMont(atol(argv[2]));
		return fails != 0;
	}
	long scale = argc > 1 ? atol(argv[1]) : 1;
	printf("LI_LIMB_BITS=%d LI_KARATSUBA_LIMBS=%d LI_MUL_LOOP_DIGIT_PRODUCT=%d LI_SQR_BASECASE_LIMBS=%d LI_SQR_KARATSUBA_LIMBS=%d LI_DIV_LOOP_QUOTIENT_DIGITS=%d LI_STACK_LIMBS=%d%s\n",
		LI_LIMB_BITS, LI_KARATSUBA_LIMBS, LI_MUL_LOOP_DIGIT_PRODUCT, LI_SQR_BASECASE_LIMBS, LI_SQR_KARATSUBA_LIMBS,
		LI_DIV_LOOP_QUOTIENT_DIGITS, (int) LI_STACK_LIMBS,
#ifdef FAKE_BIGENDIAN
		" FAKE_BIGENDIAN"
#else
		""
#endif
		);
	testReciprocal(3000000 * scale);
	/* every random suite twice: with the production dispatch to the 32-bit loops for the
	   smallest operands, and with that dispatch off, so that the limb code sees them too */
	for (int pass = 0; pass < 2; pass++) {
		liMulLoopDigitProduct = pass ? 0 : LI_MUL_LOOP_DIGIT_PRODUCT;
		liDivLoopQuotientDigits = pass ? 0 : LI_DIV_LOOP_QUOTIENT_DIGITS;
		printf("-- pass %d: dispatch to the 32-bit loops %s\n", pass, pass ? "OFF" : "on");
		testMulSmall(100000 * scale);
		testMulThreshold(2);
		testSqrSmall(20000 * scale);
		testSqrThreshold(1);
		testDivRandom(100000 * scale, 12, 12);
		testDivRandom(5000 * scale, 80, 80);
		testDivReconstruct(100000 * scale, 12, 12);
		testDivReconstruct(3000 * scale, 60, 60);
		printf("div random + reconstruction: %ld cases so far\n", nDiv);
	}
	liMulLoopDigitProduct = LI_MUL_LOOP_DIGIT_PRODUCT;
	liDivLoopQuotientDigits = LI_DIV_LOOP_QUOTIENT_DIGITS;
	testMulLarge();
	testMulNoMemory();
	testSqrLarge();
	testSqrNoMemory();
	testDivLarge();
	testDivNoMemory();
	testDivOffContract();
	printf("-- Burnikel & Ziegler: LI_BZ_LIMBS=%d LI_BZ_QUOTIENT_LIMBS=%d\n", LI_BZ_LIMBS, LI_BZ_QUOTIENT_LIMBS);
	memset(nAddBack, 0, sizeof nAddBack);
	testDivBZThreshold(2 * scale);
	testDivBZShortQuotient(2 * scale);
	testDivBZLarge();
	checkAddBacks("production thresholds");
	testDivBZForced(20000 * scale);
	checkAddBacks("forced thresholds");
	testDivBZDirect(10000 * scale);
	checkAddBacks("liDivRemBZ direct");
	testDivBZNoMemory();
	printf("-- Montgomery: LI_MONT_LOOP_DIGITS=%d LI_MONT_MUL_LIMBS=%d LI_MONT_SQR_LIMBS=%d\n", LI_MONT_LOOP_DIGITS, LI_MONT_MUL_LIMBS, LI_MONT_SQR_LIMBS);
	testMontSizes();
	testMontRandom(40000 * scale);
	testMontThreshold(scale);
	testMontPatterns();
	testMontWrongInverse(10000 * scale);
	testMontLarge();
	testMontNoMemory();
	testMontShort();
	testScratchLimit();
	printf("totals: %ld multiplies (%ld squares), %ld divisions (%ld + %ld through a failing malloc), %ld Montgomery (%ld squares, %ld with a wrong inverse or an even modulus)\n",
		nMul, nSqr, nDiv, nFallbackMul, nFallbackDiv, nMont, nMontSqr, nMontWrong);
	printf("%s: %d failures\n", fails ? "FAILED" : "OK", fails);
	return fails != 0;
}
