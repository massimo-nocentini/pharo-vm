/* protocheck-wrong.h -- the negative test of CP_CHECK_PROTOTYPES: the lane
 * protocheck of tests/run-c-tests.sh compiles src/binding/cpCairo.c in the
 * native source-stack mode (-DCP_HAVE_CAIRO_H -DCP_CAIRO_TYPES
 * -DCP_CHECK_PROTOTYPES) with -include of this file, which makes cairo.h's
 * cairo_set_dash, as cpCairo.c sees it, a function whose num_dashes is a
 * double instead of an int.  The compile must fail with
 * "cp_fn_set_dash differs from cairo.h" and with no other error: that shows
 * that the check of cpCairo.c compares every prototype of cpCairo.h with
 * cairo.h's.
 *
 * Nothing else includes this file. */

#ifndef CP_PROTOCHECK_WRONG_H
#define CP_PROTOCHECK_WRONG_H

#include "cpCairo.h"	/* cairo.h, with the real cairo_set_dash */

void cairo_set_dash_cp_wrong(cairo_t *cr, const double *dashes, double num_dashes, double offset);
#define cairo_set_dash cairo_set_dash_cp_wrong

#endif /* CP_PROTOCHECK_WRONG_H */
