/* cpCairo.h -- the cairo the CairoPlugin calls: its types, the X-macro list of
 * the 57 functions it uses, the function table filled at bind time, and the
 * binding API (src/binding/cpCairo.c).  docs/CairoPlugin.md is the contract.
 *
 * The plugin never links cairo natively: it resolves every function at bind
 * time through the VM's module loader, with the library path the image's FFI
 * uses, so that both call the same cairo.  Three build modes:
 *
 *   1. source-stack native (CP_HAVE_CAIRO_H CP_CAIRO_TYPES
 *      CP_CHECK_PROTOTYPES): the VM bundles the cairo stack
 *      (cmake/cairoStack.cmake, cmake/cairo-stack/pins.txt), whose headers
 *      the plugin is compiled with: the cp_ types are cairo's, and
 *      src/binding/cpCairo.c checks at compile time that every cp_fn_ type
 *      below is the type of the cairo function (an unevaluated GNU C check:
 *      no reference to cairo is emitted); the binding is at run time, as
 *      in mode 3;
 *   2. Emscripten (CP_HAVE_CAIRO_H CP_CAIRO_TYPES CP_STATIC_CAIRO): a
 *      builtin plugin linked with pharo_cairo; the table is filled
 *      statically (cp_cairo_bind_static), which type-checks every
 *      assignment against cairo.h;
 *   3. own types (no switch): no cairo header at build time
 *      (PHARO_CAIRO_STACK=prebuilt, whose bundles ship only the shared
 *      libraries, or a build without the stack): the declarations below,
 *      with the struct tags of cairo.h (pointer-compatible) and the layouts
 *      of its value structs; tests/protocheck-own.c checks them against
 *      cairo.h field by field and function by function.
 *
 * The switches:
 *   CP_HAVE_CAIRO_H      include cairo.h (and cairo-ft.h, unless
 *                        CP_PROTOCHECK_NO_FT): modes 1 and 2, and the
 *                        protocheck and targets lanes of tests/run-c-tests.sh
 *   CP_CAIRO_TYPES       the cp_ types are typedefs of cairo's (needs
 *                        CP_HAVE_CAIRO_H): modes 1 and 2
 *   CP_CHECK_PROTOTYPES  the compile-time prototype check of cpCairo.c (needs
 *                        CP_CAIRO_TYPES and GNU C: __typeof__,
 *                        __builtin_types_compatible_p): mode 1
 *   CP_STATIC_CAIRO      cp_cairo_bind_static exists: mode 2
 *
 * Names never clash with cairo.h: types cp_*_t, constants CP_*, table fields
 * named after the cairo function without its cairo_ prefix.
 *
 * Freestanding: only <stddef.h> (offsetof, size_t) when cairo.h is not
 * included, so that the targets lane can check this file and cpCairo.c with
 * clang --target=<every shipped ABI> -ffreestanding. */

#ifndef CP_CAIRO_H
#define CP_CAIRO_H

#include <stddef.h>

#if defined(CP_CAIRO_TYPES) && !defined(CP_HAVE_CAIRO_H)
# error "CP_CAIRO_TYPES needs CP_HAVE_CAIRO_H"
#endif
#if defined(CP_CHECK_PROTOTYPES) && !defined(CP_CAIRO_TYPES)
# error "CP_CHECK_PROTOTYPES needs CP_CAIRO_TYPES"
#endif
#if defined(CP_CHECK_PROTOTYPES) && !defined(__GNUC__)
# error "CP_CHECK_PROTOTYPES needs GNU C (__typeof__, __builtin_types_compatible_p)"
#endif

#ifdef CP_HAVE_CAIRO_H
# include <cairo.h>
# ifndef CP_PROTOCHECK_NO_FT
#  include <cairo-ft.h>
# endif
#endif

/* The binding layer's extern symbols are hidden from the plugin's dynamic
 * symbol table (nm -D lists only the module functions, the primitives and
 * their depth variables); Windows exports only what EXPORT() marks */
#if defined(_WIN32) || defined(__CYGWIN__)
# define CP_HIDDEN
#elif defined(__GNUC__) || defined(__clang__)
# define CP_HIDDEN __attribute__((visibility("hidden")))
#else
# define CP_HIDDEN
#endif

#ifdef __cplusplus
extern "C" {
#endif

/*** Types ***/

#ifdef CP_CAIRO_TYPES

typedef cairo_t			cp_cairo_t;
typedef cairo_surface_t		cp_surface_t;
typedef cairo_pattern_t		cp_pattern_t;
typedef cairo_scaled_font_t	cp_scaled_font_t;
typedef cairo_path_t		cp_path_t;
typedef cairo_matrix_t		cp_matrix_t;
typedef cairo_glyph_t		cp_glyph_t;
typedef cairo_text_extents_t	cp_text_extents_t;
typedef cairo_font_extents_t	cp_font_extents_t;
typedef cairo_text_cluster_t	cp_text_cluster_t;
typedef cairo_status_t		cp_status_t;
typedef cairo_line_join_t	cp_line_join_t;
typedef cairo_line_cap_t	cp_line_cap_t;
typedef cairo_fill_rule_t	cp_fill_rule_t;
typedef cairo_antialias_t	cp_antialias_t;
typedef cairo_operator_t	cp_operator_t;
typedef cairo_text_cluster_flags_t cp_text_cluster_flags_t;
# ifndef CP_PROTOCHECK_NO_FT
typedef FT_Face			cp_ft_face_t;
# else
typedef void *			cp_ft_face_t;
# endif

#else /* own types */

typedef struct _cairo		cp_cairo_t;		/* the struct tags of cairo.h */
typedef struct _cairo_surface	cp_surface_t;
typedef struct _cairo_pattern	cp_pattern_t;
typedef struct _cairo_scaled_font cp_scaled_font_t;
typedef struct cairo_path	cp_path_t;		/* opaque here: only pointers are passed */
typedef struct cp_matrix { double xx, yx, xy, yy, x0, y0; } cp_matrix_t;
typedef struct cp_glyph { unsigned long index; double x, y; } cp_glyph_t;
typedef struct cp_text_extents {
	double x_bearing, y_bearing, width, height, x_advance, y_advance;
} cp_text_extents_t;
typedef struct cp_font_extents {
	double ascent, descent, height, max_x_advance, max_y_advance;
} cp_font_extents_t;
typedef struct cp_text_cluster cp_text_cluster_t;	/* opaque: only NULL is passed */
typedef int cp_status_t, cp_line_join_t, cp_line_cap_t, cp_fill_rule_t, cp_antialias_t,
	cp_operator_t, cp_text_cluster_flags_t;
typedef void *cp_ft_face_t;				/* FT_Face */

#endif /* CP_CAIRO_TYPES */

#define CP_STATUS_SUCCESS 0
/* the oldest cairo the bind accepts (CAIRO_VERSION_ENCODE(1, 16, 0)).  The
   VM bundles 1.18.6 (cmake/cairo-stack/pins.txt), but the image's FFI may
   load another: a PHARO_CAIRO_STACK=prebuilt bundle (Linux aarch64 1.16.0),
   or a system cairo */
#define CP_CAIRO_VERSION_MIN 11600

/* The layouts the primitives rely on, in both modes (LP64 and LLP64: the
   glyph's unsigned long is 4 bytes on Windows, padded to 8) */
_Static_assert(sizeof(cp_matrix_t) == 48, "cp_matrix_t: 6 doubles");
_Static_assert(sizeof(cp_glyph_t) == 24, "cp_glyph_t: 24 bytes");
_Static_assert(offsetof(cp_glyph_t, x) == 8, "cp_glyph_t: x at 8");
_Static_assert(offsetof(cp_glyph_t, y) == 16, "cp_glyph_t: y at 16");
_Static_assert(sizeof(cp_text_extents_t) == 48, "cp_text_extents_t: 6 doubles");
_Static_assert(sizeof(cp_font_extents_t) == 40, "cp_font_extents_t: 5 doubles");
#ifdef CP_HAVE_CAIRO_H
_Static_assert(CP_STATUS_SUCCESS == CAIRO_STATUS_SUCCESS, "CP_STATUS_SUCCESS");
#endif


/*** The functions ***/

/* Flags: CP_REQ required at bind, CP_OPT optional (may stay NULL: the
   primitives that need one fail with PrimErrUnsupported), CP_PROBE resolved
   (required) but never called: the bind compares it with the image's probe */
#define CP_REQ   1
#define CP_OPT   2
#define CP_PROBE 3

/* X(ret, name, params, flags): name is the cairo function without its
   cairo_ prefix.  The order is the table's (CP_FN_<name>, the counters, the
   names answered by primitiveFunctionNames). */
#define CP_CAIRO_FUNCTIONS(X) \
	X(int, version, (void), CP_REQ) \
	X(cp_cairo_t *, create, (cp_surface_t *target), CP_PROBE) \
	X(void, new_path, (cp_cairo_t *cr), CP_REQ) \
	X(void, move_to, (cp_cairo_t *cr, double x, double y), CP_REQ) \
	X(void, line_to, (cp_cairo_t *cr, double x, double y), CP_REQ) \
	X(void, curve_to, (cp_cairo_t *cr, double x1, double y1, double x2, double y2, double x3, double y3), CP_REQ) \
	X(void, close_path, (cp_cairo_t *cr), CP_REQ) \
	X(void, arc, (cp_cairo_t *cr, double xc, double yc, double radius, double angle1, double angle2), CP_REQ) \
	X(void, arc_negative, (cp_cairo_t *cr, double xc, double yc, double radius, double angle1, double angle2), CP_REQ) \
	X(void, rectangle, (cp_cairo_t *cr, double x, double y, double width, double height), CP_REQ) \
	X(cp_path_t *, copy_path, (cp_cairo_t *cr), CP_REQ) \
	X(void, append_path, (cp_cairo_t *cr, const cp_path_t *path), CP_REQ) \
	X(void, path_destroy, (cp_path_t *path), CP_REQ) \
	X(void, get_current_point, (cp_cairo_t *cr, double *x, double *y), CP_REQ) \
	X(void, set_source_rgba, (cp_cairo_t *cr, double red, double green, double blue, double alpha), CP_REQ) \
	X(void, set_source, (cp_cairo_t *cr, cp_pattern_t *source), CP_REQ) \
	X(void, set_line_width, (cp_cairo_t *cr, double width), CP_REQ) \
	X(void, set_line_join, (cp_cairo_t *cr, cp_line_join_t line_join), CP_REQ) \
	X(void, set_line_cap, (cp_cairo_t *cr, cp_line_cap_t line_cap), CP_REQ) \
	X(void, set_dash, (cp_cairo_t *cr, const double *dashes, int num_dashes, double offset), CP_REQ) \
	X(void, set_matrix, (cp_cairo_t *cr, const cp_matrix_t *matrix), CP_REQ) \
	X(void, set_fill_rule, (cp_cairo_t *cr, cp_fill_rule_t fill_rule), CP_REQ) \
	X(void, set_antialias, (cp_cairo_t *cr, cp_antialias_t antialias), CP_REQ) \
	X(void, set_operator, (cp_cairo_t *cr, cp_operator_t op), CP_REQ) \
	X(cp_operator_t, get_operator, (cp_cairo_t *cr), CP_REQ) \
	X(void, fill, (cp_cairo_t *cr), CP_REQ) \
	X(void, fill_preserve, (cp_cairo_t *cr), CP_REQ) \
	X(void, stroke, (cp_cairo_t *cr), CP_REQ) \
	X(void, paint, (cp_cairo_t *cr), CP_REQ) \
	X(void, save, (cp_cairo_t *cr), CP_REQ) \
	X(void, restore, (cp_cairo_t *cr), CP_REQ) \
	X(void, clip, (cp_cairo_t *cr), CP_REQ) \
	X(void, reset_clip, (cp_cairo_t *cr), CP_REQ) \
	X(void, surface_flush, (cp_surface_t *surface), CP_REQ) \
	X(void, matrix_init, (cp_matrix_t *m, double xx, double yx, double xy, double yy, double x0, double y0), CP_REQ) \
	X(void, matrix_init_identity, (cp_matrix_t *m), CP_REQ) \
	X(void, matrix_init_translate, (cp_matrix_t *m, double tx, double ty), CP_REQ) \
	X(void, matrix_init_scale, (cp_matrix_t *m, double sx, double sy), CP_REQ) \
	X(void, matrix_init_rotate, (cp_matrix_t *m, double radians), CP_REQ) \
	X(void, matrix_translate, (cp_matrix_t *m, double tx, double ty), CP_REQ) \
	X(void, matrix_scale, (cp_matrix_t *m, double sx, double sy), CP_REQ) \
	X(void, matrix_rotate, (cp_matrix_t *m, double radians), CP_REQ) \
	X(void, matrix_multiply, (cp_matrix_t *result, const cp_matrix_t *a, const cp_matrix_t *b), CP_REQ) \
	X(cp_status_t, matrix_invert, (cp_matrix_t *m), CP_REQ) \
	X(void, matrix_transform_point, (const cp_matrix_t *m, double *x, double *y), CP_REQ) \
	X(void, set_scaled_font, (cp_cairo_t *cr, const cp_scaled_font_t *scaled_font), CP_REQ) \
	X(cp_scaled_font_t *, get_scaled_font, (cp_cairo_t *cr), CP_REQ) \
	X(void, set_font_size, (cp_cairo_t *cr, double size), CP_REQ) \
	X(void, show_glyphs, (cp_cairo_t *cr, const cp_glyph_t *glyphs, int num_glyphs), CP_REQ) \
	X(cp_status_t, scaled_font_text_to_glyphs, (cp_scaled_font_t *sf, double x, double y, \
		const char *utf8, int utf8_len, cp_glyph_t **glyphs, int *num_glyphs, \
		cp_text_cluster_t **clusters, int *num_clusters, cp_text_cluster_flags_t *cluster_flags), CP_REQ) \
	X(void, scaled_font_glyph_extents, (cp_scaled_font_t *sf, const cp_glyph_t *glyphs, int num_glyphs, \
		cp_text_extents_t *extents), CP_REQ) \
	X(void, scaled_font_text_extents, (cp_scaled_font_t *sf, const char *utf8, cp_text_extents_t *extents), CP_REQ) \
	X(void, scaled_font_extents, (cp_scaled_font_t *sf, cp_font_extents_t *extents), CP_REQ) \
	X(void, glyph_free, (cp_glyph_t *glyphs), CP_REQ) \
	X(void, text_path, (cp_cairo_t *cr, const char *utf8), CP_REQ) \
	X(cp_ft_face_t, ft_scaled_font_lock_face, (cp_scaled_font_t *sf), CP_OPT) \
	X(void, ft_scaled_font_unlock_face, (cp_scaled_font_t *sf), CP_OPT)

/* cp_fn_<name>: the pointer type of each function */
#define CP_X_TYPEDEF(ret, name, params, flags) typedef ret (*cp_fn_##name) params;
CP_CAIRO_FUNCTIONS(CP_X_TYPEDEF)
#undef CP_X_TYPEDEF

/* CP_FN_<name>: the index of each function; CP_FN_COUNT of them */
#define CP_X_ENUM(ret, name, params, flags) CP_FN_##name,
enum { CP_CAIRO_FUNCTIONS(CP_X_ENUM) CP_FN_COUNT };
#undef CP_X_ENUM

/* The table: one field per function, named after it */
#define CP_X_FIELD(ret, name, params, flags) cp_fn_##name name;
typedef struct cp_cairo_api { CP_CAIRO_FUNCTIONS(CP_X_FIELD) } cp_cairo_api;
#undef CP_X_FIELD


/*** Binding (src/binding/cpCairo.c, no VM header) ***/

/* The address of the function name ("cairo_new_path"), or NULL */
typedef void *(*cp_symbol_loader)(const char *name, void *context);

/* answers of cp_cairo_bind */
#define CP_BIND_OK 0
#define CP_BIND_MISSING 1		/* a CP_REQ or CP_PROBE function is missing: *missing names it */
#define CP_BIND_BAD_ARGUMENT 2		/* load or api is NULL: nothing was looked up */

/* "cairo_new_path", ... in table order (full C names) */
extern CP_HIDDEN const char *const cp_cairo_function_names[CP_FN_COUNT];
/* CP_REQ, CP_OPT or CP_PROBE, in table order */
extern CP_HIDDEN const unsigned char cp_cairo_function_flags[CP_FN_COUNT];

/* Fills *api from load(name, context), every function in table order;
   answers CP_BIND_OK, or CP_BIND_MISSING with *missing (when missing is not
   NULL) the first missing CP_REQ/CP_PROBE name (looking up stops there).
   CP_OPT functions may stay NULL.  Never writes *api partially on failure
   (fills a local copy first). */
CP_HIDDEN int cp_cairo_bind(cp_symbol_loader load, void *context, cp_cairo_api *api, const char **missing);

#ifdef CP_STATIC_CAIRO
/* api->name = cairo_name for every function; answers CP_BIND_OK */
CP_HIDDEN int cp_cairo_bind_static(cp_cairo_api *api);
#endif

#ifdef __cplusplus
}
#endif

#endif /* CP_CAIRO_H */
