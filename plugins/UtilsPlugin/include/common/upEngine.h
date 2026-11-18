/* upEngine.h -- the engine of the UtilsPlugin: what its primitives compute,
 * without any VM header (src/engine/upEngine.c).  The glue
 * (src/common/UtilsPlugin.c) checks the arguments, reads the objects and
 * makes the results; the image's package VM-Plugins-Primitives (the
 * submodule plugins/UtilsPlugin/vm-plugins-primitives.st) has a Smalltalk
 * twin of every function here, which answers the same bits.
 * docs/UtilsPlugin.md is the contract.
 */

#ifndef UP_ENGINE_H
#define UP_ENGINE_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*** Fused multiply-add ***/

/* x * y + z with one rounding: C99's fma */
double up_fma(double x, double y, double z);


/*** Splitting bytes (primitive_strtok_r) ***/

/* The delimiters as a set of bytes: set[b] is 1 when b is a delimiter */
void up_byte_set(unsigned char set[256], const unsigned char *delimiters, size_t count);

/* An iteration over the pieces of the bytes s[0 .. n-1]:
 *   inclusive = 0: the maximal runs of non-delimiters, in order (no empty
 *                  piece), as strtok_r(3) finds them;
 *   inclusive = 1: the fields between delimiters (every delimiter ends a
 *                  field, so two adjacent delimiters make an empty one),
 *                  but the last field when it is empty: "a\n" has the one
 *                  piece "a", "a\n\n" has "a" and "", "" has none, "\n" has
 *                  "".  With the line ends as delimiters, these are the
 *                  lines of the text.
 * A NUL byte is an ordinary byte. */
typedef struct {
	const unsigned char *s;
	const unsigned char *set;
	size_t n;
	size_t pos;
	int inclusive;
	int done;
} up_splitter;

void up_split_init(up_splitter *it, const unsigned char *s, size_t n, const unsigned char set[256], int inclusive);

/* The next piece, s[*start .. *start + *length - 1]: answers 1, or 0 at
 * the end */
int up_split_next(up_splitter *it, size_t *start, size_t *length);


/*** Bezier curves (primitive_decasteljau) ***/

/* The point at t of the Bezier curve of the n >= 1 control points
 * (xs[k], ys[k]), by the de Casteljau scheme of the UtilsPlugin of
 * pharo-10: Q = P0, then for k = 1 .. n-1
 *     h := h * t(n-k) / (k(1-t) + h * t(n-k)),  Q := (1-h) Q + h Pk
 * with h = 1 at first, each product-sum fused (fma) in this order.  t = 0
 * answers P0, t = 1 answers P(n-1); outside [0, 1] it extrapolates. */
void up_bezier(const double *xs, const double *ys, size_t n, double t, double *qx, double *qy);


/*** Knuth's lagged Fibonacci generator of fractions (primitive_randomknuth_*) ***/

/* ranf_array and ranf_start of Knuth's rng-double.c (TAOCP 3.6), which
 * src/knuth/rng-double.c holds unchanged: the long lag, the quality of the
 * buffer, and the bytes of the two arrays of a generator's state */
#define UP_KNUTH_KK 100
#define UP_KNUTH_QUALITY 1009
#define UP_KNUTH_STATE_BYTES (UP_KNUTH_KK * 8)
#define UP_KNUTH_BUFFER_BYTES (UP_KNUTH_QUALITY * 8)

/* ranf_start(seed) into the state ran_u (only the low 30 bits of seed
 * count, as Knuth's seed & 0x3fffffff) */
void up_knuth_start(int64_t seed, double ran_u[UP_KNUTH_KK]);

/* ranf_arr_cycle: ranf_array(buffer, QUALITY) with the state ran_u, then
 * buffer[KK] = -1, the end of the KK fractions of the cycle (buffer[0 ..
 * KK-1], each in [0, 1)) */
void up_knuth_cycle(double ran_u[UP_KNUTH_KK], double buffer[UP_KNUTH_QUALITY]);

/* Knuth's own check (the main of rng-double.c, without printing): ran_u[0]
 * after ranf_start(310952) and 2009 calls of ranf_array(a, 1009); it is
 * 0.36410514377569680455 */
double up_knuth_selftest(void);

#ifdef __cplusplus
}
#endif

#endif /* UP_ENGINE_H */
