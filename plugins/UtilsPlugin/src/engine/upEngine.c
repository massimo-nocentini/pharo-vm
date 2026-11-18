/* upEngine.c -- the engine of the UtilsPlugin (upEngine.h): fused
 * multiply-add, the splitting of bytes, the de Casteljau scheme and Knuth's
 * generator.  No VM header: the glue (src/common/UtilsPlugin.c) and the C
 * tests (tests/enginetest.c) call it.
 *
 * Compile with -ffp-contract=off: every rounding of up_bezier is the one
 * written here, so that the Smalltalk twin (UtilsPluginSmalltalk, with an
 * exact fma) answers the same bits.
 */

#include <math.h>
#include <stdio.h>		/* before rng-double.c, which includes it: outside its hidden visibility */
#include <string.h>

#include "upEngine.h"


/*** Fused multiply-add ***/

double
up_fma(double x, double y, double z)
{
	return fma(x, y, z);
}


/*** Splitting bytes ***/

void
up_byte_set(unsigned char set[256], const unsigned char *delimiters, size_t count)
{
	size_t i;

	memset(set, 0, 256);
	for (i = 0; i < count; i++) {
		set[delimiters[i]] = 1;
	}
}

void
up_split_init(up_splitter *it, const unsigned char *s, size_t n, const unsigned char set[256], int inclusive)
{
	it->s = s;
	it->set = set;
	it->n = n;
	it->pos = 0;
	it->inclusive = inclusive != 0;
	it->done = 0;
}

int
up_split_next(up_splitter *it, size_t *start, size_t *length)
{
	size_t end;

	if (it->done) {
		return 0;
	}
	if (!it->inclusive) {
		while (it->pos < it->n && it->set[it->s[it->pos]]) {
			it->pos++;
		}
		if (it->pos == it->n) {
			it->done = 1;
			return 0;
		}
	}
	end = it->pos;
	while (end < it->n && !it->set[it->s[end]]) {
		end++;
	}
	*start = it->pos;
	*length = end - it->pos;
	if (end == it->n) {
		/* the last field: dropped when empty (inclusive), never empty
		 * otherwise */
		it->done = 1;
		return *length > 0;
	}
	it->pos = end + 1;
	return 1;
}


/*** Bezier curves ***/

void
up_bezier(const double *xs, const double *ys, size_t n, double t, double *qx, double *qy)
{
	double h = 1.0, u = 1.0 - t, h1, x, y, px = xs[0], py = ys[0];
	size_t k;

	for (k = 1; k < n; k++) {
		h *= fma(t, (double)n, (-t) * (double)k);
		h /= fma(u, (double)k, h);
		x = xs[k] * h;
		y = ys[k] * h;
		h1 = 1.0 - h;
		px = fma(px, h1, x);
		py = fma(py, h1, y);
	}
	*qx = px;
	*qy = py;
}


/*** Knuth's generator ***/

/* rng-double.c, by D. E. Knuth, in the public domain "AS LONG AS YOU MAKE
 * ABSOLUTELY NO CHANGES": it is included as it is (src/knuth, the file of
 * https://www-cs-faculty.stanford.edu/~knuth/programs/rng-double.c, SHA256
 * e8bc0a86...8c46fed), and only renamed from outside, so that its globals
 * and its test program cannot clash with other names of the VM.  Its state
 * ran_u is a global, so each generator of the image keeps its own state in
 * a ByteArray, copied into ran_u before Knuth's functions run and out of it
 * after: the VM runs primitives on one thread. */
#define ran_u up_knuth_ran_u
#define ranf_array up_knuth_ranf_array
#define ranf_arr_buf up_knuth_ranf_arr_buf
#define ranf_arr_dummy up_knuth_ranf_arr_dummy
#define ranf_arr_started up_knuth_ranf_arr_started
#define ranf_arr_ptr up_knuth_ranf_arr_ptr
#define ranf_start up_knuth_ranf_start
#define ranf_arr_cycle up_knuth_ranf_arr_cycle
#define main up_knuth_rng_double_main
#if defined(__GNUC__) || defined(__clang__)
# pragma GCC visibility push(hidden)
#endif
#include "../knuth/rng-double.c"
#if defined(__GNUC__) || defined(__clang__)
# pragma GCC visibility pop
#endif
#undef main

_Static_assert(KK == UP_KNUTH_KK, "upEngine.h: Knuth's long lag");
_Static_assert(QUALITY == UP_KNUTH_QUALITY, "upEngine.h: Knuth's quality");
_Static_assert(sizeof(double) == 8, "upEngine.h: the states are arrays of 8-byte doubles");

void
up_knuth_start(int64_t seed, double state[UP_KNUTH_KK])
{
	ranf_start((long)seed);
	memcpy(state, ran_u, sizeof ran_u);
}

void
up_knuth_cycle(double state[UP_KNUTH_KK], double buffer[UP_KNUTH_QUALITY])
{
	memcpy(ran_u, state, sizeof ran_u);
	ranf_array(buffer, QUALITY);
	buffer[KK] = -1;
	memcpy(state, ran_u, sizeof ran_u);
}

double
up_knuth_selftest(void)
{
	static double a[2009];
	double saved[KK], result;
	int m;

	memcpy(saved, ran_u, sizeof ran_u);
	ranf_start(310952);
	for (m = 0; m < 2009; m++) {
		ranf_array(a, 1009);
	}
	result = ran_u[0];
	memcpy(ran_u, saved, sizeof ran_u);
	return result;
}
