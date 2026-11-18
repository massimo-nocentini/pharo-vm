/* protocheck-own.c -- checks the own types and prototypes of cpCairo.h, the
 * ones every native build of the CairoPlugin uses (no cairo.h there),
 * against cairo.h and cairo-ft.h: the lane protocheck of
 * tests/run-c-tests.sh compiles it with the host clang (-fsyntax-only
 * -Werror=incompatible-function-pointer-types
 * -Werror=incompatible-pointer-types) against the cairo 1.18.4 headers, and
 * the lane targets, with -DCP_PROTOCHECK_NO_FT, for every shipped ABI
 * (clang --target=T -ffreestanding: cairo.h includes no system header).
 *
 *   - the value structs: the same size and, field by field, the same offset
 *     and size as cairo's (cp_glyph_t's index too: unsigned long, 4 bytes
 *     on LLP64);
 *   - every enum the own types map to int has the size of an int;
 *   - every function of the X-macro, its own prototype with the cp_ types
 *     replaced by cairo's (enums by the cairo enums), initializes a pointer
 *     to the cairo function: a different parameter count or scalar type
 *     (float/double, int/long, pointer level) is a compile error.
 *
 * Nothing runs: compiling is the check. */

#ifdef CP_CAIRO_TYPES
# error "protocheck-own checks the own types: compile it without CP_CAIRO_TYPES"
#endif
#ifndef CP_HAVE_CAIRO_H
# define CP_HAVE_CAIRO_H 1
#endif
#include "cpCairo.h"

#ifdef CP_PROTOCHECK_NO_FT
/* No cairo-ft.h (no FreeType headers for the cross targets): the two
   functions as cairo-ft.h declares them, with FT_Face, a pointer, as void * */
cairo_public void *cairo_ft_scaled_font_lock_face(cairo_scaled_font_t *scaled_font);
cairo_public void cairo_ft_scaled_font_unlock_face(cairo_scaled_font_t *scaled_font);
#endif

#define CP_SAME_FIELD(own, theirs, field) \
	_Static_assert(offsetof(own, field) == offsetof(theirs, field) \
		&& sizeof(((own *)0)->field) == sizeof(((theirs *)0)->field), #own "." #field " differs from " #theirs);

_Static_assert(sizeof(cp_matrix_t) == sizeof(cairo_matrix_t), "cp_matrix_t");
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, xx)
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, yx)
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, xy)
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, yy)
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, x0)
CP_SAME_FIELD(cp_matrix_t, cairo_matrix_t, y0)

_Static_assert(sizeof(cp_glyph_t) == sizeof(cairo_glyph_t), "cp_glyph_t");
_Static_assert(sizeof(((cp_glyph_t *)0)->index) == sizeof(unsigned long), "cp_glyph_t.index: unsigned long");
CP_SAME_FIELD(cp_glyph_t, cairo_glyph_t, index)
CP_SAME_FIELD(cp_glyph_t, cairo_glyph_t, x)
CP_SAME_FIELD(cp_glyph_t, cairo_glyph_t, y)

_Static_assert(sizeof(cp_text_extents_t) == sizeof(cairo_text_extents_t), "cp_text_extents_t");
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, x_bearing)
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, y_bearing)
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, width)
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, height)
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, x_advance)
CP_SAME_FIELD(cp_text_extents_t, cairo_text_extents_t, y_advance)

_Static_assert(sizeof(cp_font_extents_t) == sizeof(cairo_font_extents_t), "cp_font_extents_t");
CP_SAME_FIELD(cp_font_extents_t, cairo_font_extents_t, ascent)
CP_SAME_FIELD(cp_font_extents_t, cairo_font_extents_t, descent)
CP_SAME_FIELD(cp_font_extents_t, cairo_font_extents_t, height)
CP_SAME_FIELD(cp_font_extents_t, cairo_font_extents_t, max_x_advance)
CP_SAME_FIELD(cp_font_extents_t, cairo_font_extents_t, max_y_advance)

/* the enums the own types pass as int */
_Static_assert(sizeof(cairo_status_t) == sizeof(int) && sizeof(cp_status_t) == sizeof(int), "cairo_status_t");
_Static_assert(sizeof(cairo_line_join_t) == sizeof(int), "cairo_line_join_t");
_Static_assert(sizeof(cairo_line_cap_t) == sizeof(int), "cairo_line_cap_t");
_Static_assert(sizeof(cairo_fill_rule_t) == sizeof(int), "cairo_fill_rule_t");
_Static_assert(sizeof(cairo_antialias_t) == sizeof(int), "cairo_antialias_t");
_Static_assert(sizeof(cairo_operator_t) == sizeof(int), "cairo_operator_t");
_Static_assert(sizeof(cairo_text_cluster_flags_t) == sizeof(int), "cairo_text_cluster_flags_t");
_Static_assert(sizeof(cp_ft_face_t) == sizeof(void *), "cp_ft_face_t: a pointer");
_Static_assert(CP_STATUS_SUCCESS == CAIRO_STATUS_SUCCESS, "CP_STATUS_SUCCESS");
_Static_assert(CP_CAIRO_VERSION_MIN == CAIRO_VERSION_ENCODE(1, 16, 0), "CP_CAIRO_VERSION_MIN");

/* every cp_ type of the prototypes, as cairo's */
#define cp_cairo_t		cairo_t
#define cp_surface_t		cairo_surface_t
#define cp_pattern_t		cairo_pattern_t
#define cp_scaled_font_t	cairo_scaled_font_t
#define cp_path_t		cairo_path_t
#define cp_matrix_t		cairo_matrix_t
#define cp_glyph_t		cairo_glyph_t
#define cp_text_extents_t	cairo_text_extents_t
#define cp_font_extents_t	cairo_font_extents_t
#define cp_text_cluster_t	cairo_text_cluster_t
#define cp_status_t		cairo_status_t
#define cp_line_join_t		cairo_line_join_t
#define cp_line_cap_t		cairo_line_cap_t
#define cp_fill_rule_t		cairo_fill_rule_t
#define cp_antialias_t		cairo_antialias_t
#define cp_operator_t		cairo_operator_t
#define cp_text_cluster_flags_t	cairo_text_cluster_flags_t
#ifndef CP_PROTOCHECK_NO_FT
# define cp_ft_face_t		FT_Face
#endif

#define CP_X_CHECK(ret, name, params, flags) \
	static ret (*const cp_check_##name) params __attribute__((unused)) = cairo_##name;
CP_CAIRO_FUNCTIONS(CP_X_CHECK)
#undef CP_X_CHECK
