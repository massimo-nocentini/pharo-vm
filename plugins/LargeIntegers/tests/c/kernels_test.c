/* kernels_test.c: unit tests of the LargeIntegers multiply and divide kernels.
 *
 * Compiles src/common/LargeIntegersKernels.c into this program (#include, so that the
 * thresholds are variables and malloc can be made to fail) and compares liDigitMultiply()
 * and liDigitDiv() digit for digit with FROZEN copies of the 32-bit loops the plugin ran
 * before the kernels existed, and with independent oracles: a base-256 schoolbook
 * multiply, the reconstruction a = q*b + r with known q and r, and (mode `dump`)
 * Python's big integers (oracle.py).
 *
 * Build variants (run-c-tests.sh): LI_LIMB_BITS=64 (the default on x86_64 and aarch64)
 * or 32, FAKE_BIGENDIAN (SQ_SWAP_4_BYTES_IF_BIGENDIAN swaps: digits are stored
 * byte-swapped, as on a big-endian host, in kernels and frozen loops alike), ASan+UBSan
 * or -O2.
 *
 * usage: kernels_test [scale]      run every test; scale multiplies the random counts
 *        kernels_test dump N       print N multiply and N divide vectors for oracle.py
 * Exit status 0 when every check holds.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "config.h"
#include "sqConfig.h"
#include "sqPlatformSpecific.h"
#include "memoryAccess.h"
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
	mulCase(300, 300, 599, 2, 2, 1, 0);
	mulCase(40, 5000, 5040, 0, 0, 0, 0);
	mallocFails = 0;
	CHECK(mallocCalls - before == 3, "the large multiplies did not ask for scratch memory (%ld)", mallocCalls - before);
	nFallbackMul += 3;
	printf("mul with failing malloc: 3 cases\n");
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
	}
}

int main(int argc, char **argv)
{
	if (argc > 2 && !strcmp(argv[1], "dump")) {
		dump(atol(argv[2]));
		return fails != 0;
	}
	long scale = argc > 1 ? atol(argv[1]) : 1;
	printf("LI_LIMB_BITS=%d LI_KARATSUBA_LIMBS=%d LI_MUL_LOOP_DIGIT_PRODUCT=%d LI_DIV_LOOP_QUOTIENT_DIGITS=%d LI_STACK_LIMBS=%d%s\n",
		LI_LIMB_BITS, LI_KARATSUBA_LIMBS, LI_MUL_LOOP_DIGIT_PRODUCT, LI_DIV_LOOP_QUOTIENT_DIGITS, (int) LI_STACK_LIMBS,
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
	testDivLarge();
	testDivNoMemory();
	testDivOffContract();
	printf("totals: %ld multiplies, %ld divisions (%ld + %ld through a failing malloc)\n", nMul, nDiv, nFallbackMul, nFallbackDiv);
	printf("%s: %d failures\n", fails ? "FAILED" : "OK", fails);
	return fails != 0;
}
