/* enginetest.c -- the tests of the engine of the UtilsPlugin (upEngine.h),
 * without a VM.  run-c-tests.sh builds it with ../src/engine/upEngine.c and
 * knuthref.c, Knuth's rng-double.c compiled on its own (its main renamed by
 * the command line), the reference of the generator.
 *
 *   enginetest [--vectors]
 *
 * prints "<n> checks, 0 failures" and exits 0 when every check passed;
 * --vectors also prints the reference values of the Smalltalk tests (the
 * first fractions of some seeds, as hexadecimal bits).
 */

#define _POSIX_C_SOURCE 200809L

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "upEngine.h"

/* knuthref.c: rng-double.c as Knuth wrote it, with its globals */
extern double ran_u[];
extern double *ranf_arr_ptr;
extern void ranf_start(long seed);
extern double ranf_arr_cycle(void);
#define ranf_arr_next() (*ranf_arr_ptr >= 0 ? *ranf_arr_ptr++ : ranf_arr_cycle())

static long checks, failures;

static void
check(int ok, const char *what, long detail)
{
	checks++;
	if (!ok) {
		failures++;
		if (failures <= 20) {
			fprintf(stderr, "FAIL %s (%ld)\n", what, detail);
		}
	}
}

static uint64_t
bits(double d)
{
	uint64_t u;

	memcpy(&u, &d, sizeof u);
	return u;
}

static uint64_t rng = 0x9E3779B97F4A7C15ULL;

static uint64_t
next64(void)
{
	rng ^= rng << 13;
	rng ^= rng >> 7;
	rng ^= rng << 17;
	return rng;
}


/*** Knuth's generator ***/

/* The generator of the image, as the glue drives it: state, buffer and
 * cursor (1-based) */
typedef struct {
	double state[UP_KNUTH_KK];
	double buffer[UP_KNUTH_QUALITY];
	int cursor;
} Gen;

static void
genStart(Gen *g, int64_t seed)
{
	up_knuth_start(seed, g->state);
	g->buffer[0] = -1;
	g->cursor = 1;
}

static double
genNext(Gen *g)
{
	if (g->buffer[g->cursor - 1] < 0) {
		up_knuth_cycle(g->state, g->buffer);
		g->cursor = 1;
	}
	return g->buffer[g->cursor++ - 1];
}

static void
testKnuth(void)
{
	static const int64_t seeds[] = { 0, 1, 2, 310952, 314159, 0x3fffffff, 0x40000000, -1, -310952, 123456789012LL };
	static Gen a, b;
	char text[64];
	size_t s, t;
	int i;

	snprintf(text, sizeof text, "%.20f", up_knuth_selftest());
	check(strcmp(text, "0.36410514377569680455") == 0, "Knuth's check of ranf_array", 0);

	/* each seed against Knuth's ranf_arr_next, two generators interleaved */
	for (s = 0; s < sizeof seeds / sizeof seeds[0]; s++) {
		t = (s + 1) % (sizeof seeds / sizeof seeds[0]);
		genStart(&a, seeds[s]);
		genStart(&b, seeds[t]);
		ranf_start((long)seeds[s]);
		for (i = 0; i < 25000; i++) {
			double x = genNext(&a), y = ranf_arr_next();

			check(bits(x) == bits(y), "the sequence of a seed", (long)s * 100000 + i);
			check(x >= 0 && x < 1, "a fraction in [0, 1)", i);
			if (i % 7 == 0) {
				(void)genNext(&b);
			}
		}
		/* a generator seeded again starts over */
		genStart(&a, seeds[s]);
		ranf_start((long)seeds[s]);
		for (i = 0; i < 300; i++) {
			check(bits(genNext(&a)) == bits(ranf_arr_next()), "a seed again", i);
		}
	}
	/* only the low 30 bits of the seed count */
	genStart(&a, 5);
	genStart(&b, 5 + 0x40000000LL * 3);
	for (i = 0; i < 500; i++) {
		check(bits(genNext(&a)) == bits(genNext(&b)), "the low 30 bits of the seed", i);
	}
}

static void
printVectors(void)
{
	static const int64_t seeds[] = { 0, 310952, 314159, 0x3fffffff };
	static Gen g;
	size_t s;
	int i;

	for (s = 0; s < sizeof seeds / sizeof seeds[0]; s++) {
		genStart(&g, seeds[s]);
		printf("seed %lld:", (long long)seeds[s]);
		for (i = 1; i <= 205; i++) {
			double x = genNext(&g);

			if (i <= 3 || i == 100 || i == 101 || i == 205) {
				printf(" %d=16r%016llX", i, (unsigned long long)bits(x));
			}
		}
		printf("\n");
	}
	printf("selftest %.20f\n", up_knuth_selftest());
}


/*** Splitting ***/

/* The model of the inclusive split: the fields between delimiters, the
 * last one dropped when empty */
static size_t
modelInclusive(const unsigned char *s, size_t n, const unsigned char *set, size_t *starts, size_t *lengths)
{
	size_t count = 0, start = 0, i;

	for (i = 0; i <= n; i++) {
		if (i == n || set[s[i]]) {
			if (i < n || i > start) {
				starts[count] = start;
				lengths[count] = i - start;
				count++;
			}
			start = i + 1;
		}
	}
	return count;
}

static size_t
modelExclusive(const unsigned char *s, size_t n, const unsigned char *set, size_t *starts, size_t *lengths)
{
	size_t count = 0, i = 0, start;

	while (i < n) {
		while (i < n && set[s[i]]) {
			i++;
		}
		if (i == n) {
			break;
		}
		start = i;
		while (i < n && !set[s[i]]) {
			i++;
		}
		starts[count] = start;
		lengths[count] = i - start;
		count++;
	}
	return count;
}

static size_t
engineSplit(const unsigned char *s, size_t n, const unsigned char *set, int inclusive, size_t *starts, size_t *lengths)
{
	up_splitter it;
	size_t count = 0;

	up_split_init(&it, s, n, set, inclusive);
	while (up_split_next(&it, &starts[count], &lengths[count])) {
		count++;
	}
	return count;
}

/* The pieces of s split by delims, as "[a][][b]" */
static void
render(const char *s, size_t n, const char *delims, int inclusive, char *out)
{
	unsigned char set[256];
	size_t starts[64], lengths[64], count, i;

	up_byte_set(set, (const unsigned char *)delims, strlen(delims));
	count = engineSplit((const unsigned char *)s, n, set, inclusive, starts, lengths);
	*out = 0;
	for (i = 0; i < count; i++) {
		strcat(out, "[");
		strncat(out, s + starts[i], lengths[i]);
		strcat(out, "]");
	}
}

static void
testSplit(void)
{
	static const struct { const char *s, *d; int inclusive; const char *pieces; } cases[] = {
		{ "", "\n", 0, "" },
		{ "", "\n", 1, "" },
		{ "abc", " ", 0, "[abc]" },
		{ "abc", " ", 1, "[abc]" },
		{ "\n", "\n", 0, "" },
		{ "\n", "\n", 1, "[]" },
		{ "\n\n", "\n", 1, "[][]" },
		{ "a\n", "\n", 1, "[a]" },
		{ "a\n\n", "\n", 1, "[a][]" },
		{ "\na", "\n", 1, "[][a]" },
		{ "a\n\nb", "\n", 1, "[a][][b]" },
		{ "a\n\nb", "\n", 0, "[a][b]" },
		{ "hello\nworld", "\r\n", 0, "[hello][world]" },
		{ "a\r\nb", "\r\n", 1, "[a][][b]" },
		{ "  a  b ", " ", 0, "[a][b]" },
		{ "  a  b ", " ", 1, "[][][a][][b]" },
		{ "a,b;c", ",;", 0, "[a][b][c]" },
		{ "abc", "", 0, "[abc]" },
		{ "abc", "", 1, "[abc]" },
	};
	unsigned char s[40], set[256], copy[41], delims[4];
	size_t a1[41], b1[41], a2[41], b2[41], n, nd, count, model, i, k;
	char out[256], *save, *token;
	int round, inclusive, same;

	for (i = 0; i < sizeof cases / sizeof cases[0]; i++) {
		render(cases[i].s, strlen(cases[i].s), cases[i].d, cases[i].inclusive, out);
		check(strcmp(out, cases[i].pieces) == 0, cases[i].s, (long)i);
		if (strcmp(out, cases[i].pieces) != 0) {
			fprintf(stderr, "  '%s' by '%s' %d: %s, not %s\n", cases[i].s, cases[i].d, cases[i].inclusive, out, cases[i].pieces);
		}
	}
	/* NUL is an ordinary byte */
	{
		size_t st[8], ln[8];

		up_byte_set(set, (const unsigned char *)"\n", 1);
		count = engineSplit((const unsigned char *)"a\0b\nc", 5, set, 0, st, ln);
		check(count == 2 && ln[0] == 3 && ln[1] == 1, "NUL inside a piece: lengths", (long)count);
		up_byte_set(set, (const unsigned char *)"\0", 1);
		count = engineSplit((const unsigned char *)"a\0b\0", 4, set, 1, st, ln);
		check(count == 2 && ln[0] == 1 && ln[1] == 1, "NUL as a delimiter", (long)count);
	}

	/* random strings over a few bytes, against the models, and the
	 * exclusive split against strtok_r(3) when there is no NUL */
	for (round = 0; round < 200000; round++) {
		static const unsigned char alphabet[] = { 'a', 'b', '\n', '\r', ' ', 0 };
		n = next64() % 24;
		for (i = 0; i < n; i++) {
			s[i] = alphabet[next64() % sizeof alphabet];
		}
		nd = next64() % 4;
		for (i = 0; i < nd; i++) {
			delims[i] = alphabet[2 + next64() % 4];
		}
		up_byte_set(set, delims, nd);
		inclusive = (int)(next64() & 1);
		count = engineSplit(s, n, set, inclusive, a1, b1);
		model = inclusive ? modelInclusive(s, n, set, a2, b2) : modelExclusive(s, n, set, a2, b2);
		same = count == model;
		for (k = 0; same && k < count; k++) {
			same = a1[k] == a2[k] && b1[k] == b2[k];
		}
		check(same, inclusive ? "inclusive against the model" : "exclusive against the model", round);
		if (!inclusive && memchr(s, 0, n) == NULL && memchr(delims, 0, nd) == NULL) {
			char d[5];

			memcpy(copy, s, n);
			copy[n] = 0;
			memcpy(d, delims, nd);
			d[nd] = 0;
			k = 0;
			same = 1;
			for (token = strtok_r((char *)copy, d, &save); token; token = strtok_r(NULL, d, &save)) {
				same = same && k < count && (size_t)(token - (char *)copy) == a1[k] && strlen(token) == b1[k];
				k++;
			}
			check(same && k == count, "exclusive against strtok_r", round);
		}
	}
}


/*** Bezier curves ***/

/* The textbook scheme: repeated linear interpolation */
static void
textbook(const double *xs, const double *ys, size_t n, double t, double *qx, double *qy)
{
	double x[32], y[32];
	size_t r, i;

	memcpy(x, xs, n * sizeof(double));
	memcpy(y, ys, n * sizeof(double));
	for (r = 1; r < n; r++) {
		for (i = 0; i < n - r; i++) {
			x[i] = (1 - t) * x[i] + t * x[i + 1];
			y[i] = (1 - t) * y[i] + t * y[i + 1];
		}
	}
	*qx = x[0];
	*qy = y[0];
}

static double
uniform(void)
{
	return (double)(next64() >> 11) / 9007199254740992.0;
}

static void
testBezier(void)
{
	double xs[32], ys[32], qx, qy, rx, ry, t, scale;
	size_t n, k;
	int round;

	/* one point: itself */
	xs[0] = 3.5;
	ys[0] = -2;
	up_bezier(xs, ys, 1, 0.25, &qx, &qy);
	check(qx == 3.5 && qy == -2, "one control point", 0);
	/* a segment */
	xs[1] = 7.5;
	ys[1] = 2;
	up_bezier(xs, ys, 2, 0.5, &qx, &qy);
	check(qx == 5.5 && qy == 0, "the middle of a segment", 0);
	/* NaN in, NaN out */
	up_bezier(xs, ys, 2, NAN, &qx, &qy);
	check(isnan(qx) && isnan(qy), "a NaN parameter", 0);

	for (round = 0; round < 100000; round++) {
		n = 1 + next64() % 16;
		scale = ldexp(1, (int)(next64() % 40) - 20);
		for (k = 0; k < n; k++) {
			xs[k] = (uniform() * 2 - 1) * scale;
			ys[k] = (uniform() * 2 - 1) * scale;
		}
		up_bezier(xs, ys, n, 0.0, &qx, &qy);
		check(bits(qx) == bits(xs[0]) && bits(qy) == bits(ys[0]), "t = 0 is the first point", round);
		up_bezier(xs, ys, n, 1.0, &qx, &qy);
		check(qx == xs[n - 1] && qy == ys[n - 1], "t = 1 is the last point", round);
		t = uniform();
		up_bezier(xs, ys, n, t, &qx, &qy);
		textbook(xs, ys, n, t, &rx, &ry);
		check(fabs(qx - rx) <= 1e-11 * scale && fabs(qy - ry) <= 1e-11 * scale, "against the textbook scheme", round);
	}
}


/*** fma ***/

static void
testFma(void)
{
	double tiny = ldexp(1, -60);

	check(up_fma(2, 3, 1) == 7, "2 * 3 + 1", 0);
	/* one rounding: (1 + 2^-60)(1 - 2^-60) - 1 = -2^-120, not 0 */
	check(up_fma(1 + ldexp(1, -52), 1 - ldexp(1, -53), -1) != 0, "one rounding", 0);
	check(up_fma(1 + tiny * 0, 1, -1) == 0 && !signbit(up_fma(1, 1, -1)), "an exact zero is +0", 0);
	check(signbit(up_fma(-0.0, 1, -0.0)), "-0 * 1 + -0 is -0", 0);
	check(isinf(up_fma(1e308, 10, -INFINITY)) && up_fma(1e308, 10, -INFINITY) < 0, "a finite overflow plus -inf", 0);
	check(isnan(up_fma(INFINITY, 0, 1)), "inf * 0", 0);
}


int
main(int argc, char **argv)
{
	if (argc > 1 && strcmp(argv[1], "--vectors") == 0) {
		printVectors();
		return 0;
	}
	testKnuth();
	testSplit();
	testBezier();
	testFma();
	printf("%ld checks, %ld failures\n", checks, failures);
	return failures != 0;
}
