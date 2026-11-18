/* glue-basic.c -- the one-for-one primitives of the CairoPlugin
 * (src/prims/basic.c, group #basic) against the fake VM of fakevm.c and a
 * real cairo: the bundled libcairo.so.2 natively (--libcairo), the static
 * pharo_cairo in the wasm64 build (glue-wasm64).
 *
 * This file #includes CairoPlugin.c (and so src/prims/basic.c): the
 * plugin's state (cp_layout, the counters) is then in reach, while the
 * primitives are looked up as the VM looks them up (fake_primitive: dlsym
 * natively, the rows of CairoPlugin_exports in a builtin plugin).  The test
 * reaches cairo itself through a table of its own (dlsym in the same
 * library natively, the functions themselves under CP_STATIC_CAIRO): the
 * reference calls and the state getters (cairo_get_matrix, ...).
 *
 *   B1  success     every primitive (a table of 58 cases, each building its
 *                   Athens-shaped receiver and arguments on a fresh cairo_t):
 *                   its answer (the receiver, a SmallInteger, the argument,
 *                   a Point), exactly one cairo call of its function, its
 *                   success counter, no protocol error
 *   B2  receivers   every invalid receiver chain of every primitive (nil, a
 *                   SmallInteger, bytes, too short, a nil, NULL, 4-byte or
 *                   ByteArray handle, an unfollowed forwarder, a bad matrix,
 *                   a read-only matrix where it is written): #'bad receiver'
 *                   or #'no modification', no cairo call, no allocation
 *   B3  arguments   every invalid argument class of every primitive (nil,
 *                   a Fraction, a LargeInteger, out of range, short,
 *                   read-only, NULL, an ExternalAddress where bytes are
 *                   expected, whose 8 bytes stay unchanged, a byte object
 *                   without NUL for the C strings): its code, no cairo call,
 *                   no allocation
 *   B4  forwarders  a forwarder at every position of every case's objects
 *                   (each argument, each slot, each slot of a slot, to 3
 *                   levels): the first call fails before any cairo call,
 *                   the VM's retry at the depth of primitives.txt succeeds
 *   B5  effects     twin cairo_ts: each primitive on one, the same cairo call
 *                   directly on the other; after every step the whole
 *                   gstate equal (matrix bit-exact, source, line width,
 *                   join, cap, dash, current point, path, fill rule,
 *                   operator, antialias, font matrix, font face, clip
 *                   extents, status) and the pixels equal at checkpoints;
 *                   the answers of getCurrentPoint, getScaledFont, copyPath,
 *                   getOperator
 *   B6  matrices    every matrix primitive on a 48-byte matrix (and on an
 *                   ExternalAddress handle) bit-identical to cairo's
 *                   cairo_matrix_* on the same values; invert's status
 *   B7  text        a real FreeType face (--font, default DejaVuSans; the
 *                   toy font when absent): lock/unlock, text_to_glyphs
 *                   (glyphs, count, status, the in/out pointer), glyph,
 *                   text and font extents (bytes and ExternalAddress
 *                   handles; 40 of 48 bytes written), glyph_free,
 *                   showGlyphs and textPath pixels
 *   B8  memory      the answers' allocations failing (getCurrentPoint,
 *                   getScaledFont, copyPath): #'insufficient object memory',
 *                   no cairo call except the pure getter's
 *   B9  optional    lock/unlock with the cairo_ft functions missing from the
 *                   binding: #'unsupported operation', no cairo call
 *
 * Usage: glue-basic [--libcairo PATH] [--font PATH]
 * Prints one line per failed check and "glue-basic ok: N checks"; exits 1
 * when a check failed.
 */

#include "CairoPlugin.c"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef __EMSCRIPTEN__
# include <dlfcn.h>
#endif

#include "fakevm.h"

#ifndef CP_PRIMITIVES_TXT
# define CP_PRIMITIVES_TXT "tests/primitives.txt"
#endif
/* the forwarder test's depth relative to primitives.txt (a sensitivity check:
   -DGB_DEPTH_DELTA=-1 must make it fail) */
#ifndef GB_DEPTH_DELTA
# define GB_DEPTH_DELTA 0
#endif

static int checks, failures;

#define CHECK(cond, ...) do { \
		checks++; \
		if (!(cond)) { \
			failures++; \
			if (failures <= 100) { \
				printf("FAILED %s:%d: ", __FILE__, __LINE__); \
				printf(__VA_ARGS__); \
				printf("\n"); \
			} \
		} \
	} while (0)

typedef sqInt (*Primitive)(void);

static const char *libcairo = "libcairo.so.2";
static const char *fontPath = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";
static void *cairoHandle;


/*** cairo for the test itself (reference calls and getters) ***/

/* cairo_path_t */
typedef struct { int status; void *data; int num_data; } TPath;

static struct {
	void *(*image_surface_create)(int format, int width, int height);
	void (*surface_destroy)(void *surface);
	void (*surface_flush)(void *surface);
	unsigned char *(*image_surface_get_data)(void *surface);
	int (*image_surface_get_stride)(void *surface);
	void *(*create)(void *surface);
	void (*destroy)(void *cr);
	int (*status)(void *cr);
	void (*get_matrix)(void *cr, cp_matrix_t *m);
	void *(*get_source)(void *cr);
	int (*pattern_get_type)(void *pattern);
	int (*pattern_get_rgba)(void *pattern, double *r, double *g, double *b, double *a);
	void *(*pattern_create_linear)(double x0, double y0, double x1, double y1);
	void (*pattern_add_color_stop_rgba)(void *pattern, double offset, double r, double g, double b, double a);
	void (*pattern_destroy)(void *pattern);
	double (*get_line_width)(void *cr);
	int (*get_line_join)(void *cr);
	int (*get_line_cap)(void *cr);
	int (*get_dash_count)(void *cr);
	void (*get_dash)(void *cr, double *dashes, double *offset);
	int (*has_current_point)(void *cr);
	void (*get_current_point)(void *cr, double *x, double *y);
	int (*get_fill_rule)(void *cr);
	int (*get_operator)(void *cr);
	int (*get_antialias)(void *cr);
	void (*get_font_matrix)(void *cr, cp_matrix_t *m);
	void *(*get_scaled_font)(void *cr);
	void *(*scaled_font_get_font_face)(void *font);
	int (*scaled_font_get_type)(void *font);
	void (*clip_extents)(void *cr, double *x1, double *y1, double *x2, double *y2);
	TPath *(*copy_path)(void *cr);
	void (*path_destroy)(TPath *path);
	void (*new_path)(void *cr);
	void (*fill)(void *cr);
	void (*fill_preserve)(void *cr);
	void (*stroke)(void *cr);
	void (*paint)(void *cr);
	void (*save)(void *cr);
	void (*restore)(void *cr);
	void (*clip)(void *cr);
	void (*reset_clip)(void *cr);
	void (*set_matrix)(void *cr, const cp_matrix_t *m);
	void (*set_source_rgba)(void *cr, double r, double g, double b, double a);
	void (*set_source)(void *cr, void *pattern);
	void (*set_dash)(void *cr, const double *dashes, int n, double offset);
	void (*rectangle)(void *cr, double x, double y, double w, double h);
	void (*set_line_width)(void *cr, double w);
	void (*set_line_join)(void *cr, int join);
	void (*set_line_cap)(void *cr, int cap);
	void (*append_path)(void *cr, const TPath *path);
	void (*set_antialias)(void *cr, int aa);
	void (*set_fill_rule)(void *cr, int rule);
	void (*set_scaled_font)(void *cr, const void *font);
	void (*show_glyphs)(void *cr, const cp_glyph_t *glyphs, int n);
	void (*set_font_size)(void *cr, double size);
	void (*move_to)(void *cr, double x, double y);
	void (*line_to)(void *cr, double x, double y);
	void (*curve_to)(void *cr, double x1, double y1, double x2, double y2, double x3, double y3);
	void (*close_path)(void *cr);
	void (*arc)(void *cr, double xc, double yc, double r, double a1, double a2);
	void (*arc_negative)(void *cr, double xc, double yc, double r, double a1, double a2);
	void (*set_operator)(void *cr, int op);
	void (*text_path)(void *cr, const char *utf8);
	void (*matrix_init)(cp_matrix_t *m, double xx, double yx, double xy, double yy, double x0, double y0);
	void (*matrix_init_identity)(cp_matrix_t *m);
	void (*matrix_translate)(cp_matrix_t *m, double tx, double ty);
	void (*matrix_scale)(cp_matrix_t *m, double sx, double sy);
	void (*matrix_rotate)(cp_matrix_t *m, double radians);
	void (*matrix_multiply)(cp_matrix_t *r, const cp_matrix_t *a, const cp_matrix_t *b);
	int (*matrix_invert)(cp_matrix_t *m);
	void (*matrix_transform_point)(const cp_matrix_t *m, double *x, double *y);
	void (*matrix_init_translate)(cp_matrix_t *m, double tx, double ty);
	void (*matrix_init_scale)(cp_matrix_t *m, double sx, double sy);
	void (*matrix_init_rotate)(cp_matrix_t *m, double radians);
	void (*scaled_font_glyph_extents)(void *font, const cp_glyph_t *glyphs, int n, cp_text_extents_t *e);
	void (*scaled_font_text_extents)(void *font, const char *utf8, cp_text_extents_t *e);
	void (*scaled_font_extents)(void *font, cp_font_extents_t *e);
	int (*scaled_font_text_to_glyphs)(void *font, double x, double y, const char *utf8, int len,
		cp_glyph_t **glyphs, int *n, void **clusters, int *nclusters, int *flags);
	cp_glyph_t *(*glyph_allocate)(int n);
	void (*glyph_free)(cp_glyph_t *glyphs);
	void *(*ft_font_face_create_for_ft_face)(void *face, int loadFlags);
	void *(*scaled_font_create)(void *face, const cp_matrix_t *fontMatrix, const cp_matrix_t *ctm, const void *options);
	void *(*font_options_create)(void);
	void (*font_options_destroy)(void *options);
	void *(*ft_scaled_font_lock_face)(void *font);
	void (*ft_scaled_font_unlock_face)(void *font);
	int (*FT_Init_FreeType)(void **library);
	int (*FT_New_Face)(void *library, const char *path, long index, void **face);
} R;

#ifdef CP_STATIC_CAIRO
# define R_LOAD(name) (R.name = (__typeof__(R.name))(void (*)(void))cairo_##name)
# define R_LOAD_FT(name) (R.name = (__typeof__(R.name))(void (*)(void))name)
#else
# define R_LOAD(name) (*(void **)&R.name = dlsym(cairoHandle, "cairo_" #name))
# define R_LOAD_FT(name) (*(void **)&R.name = dlsym(cairoHandle, #name))
#endif

static int
loadReference(void)
{
	R_LOAD(image_surface_create); R_LOAD(surface_destroy); R_LOAD(surface_flush);
	R_LOAD(image_surface_get_data); R_LOAD(image_surface_get_stride); R_LOAD(create); R_LOAD(destroy);
	R_LOAD(status); R_LOAD(get_matrix); R_LOAD(get_source); R_LOAD(pattern_get_type); R_LOAD(pattern_get_rgba);
	R_LOAD(pattern_create_linear); R_LOAD(pattern_add_color_stop_rgba); R_LOAD(pattern_destroy);
	R_LOAD(get_line_width); R_LOAD(get_line_join); R_LOAD(get_line_cap); R_LOAD(get_dash_count); R_LOAD(get_dash);
	R_LOAD(has_current_point); R_LOAD(get_current_point); R_LOAD(get_fill_rule); R_LOAD(get_operator);
	R_LOAD(get_antialias); R_LOAD(get_font_matrix); R_LOAD(get_scaled_font); R_LOAD(scaled_font_get_font_face);
	R_LOAD(scaled_font_get_type); R_LOAD(clip_extents); R_LOAD(copy_path); R_LOAD(path_destroy);
	R_LOAD(new_path); R_LOAD(fill); R_LOAD(fill_preserve); R_LOAD(stroke); R_LOAD(paint); R_LOAD(save);
	R_LOAD(restore); R_LOAD(clip); R_LOAD(reset_clip); R_LOAD(set_matrix); R_LOAD(set_source_rgba);
	R_LOAD(set_source); R_LOAD(set_dash); R_LOAD(rectangle); R_LOAD(set_line_width); R_LOAD(set_line_join);
	R_LOAD(set_line_cap); R_LOAD(append_path); R_LOAD(set_antialias); R_LOAD(set_fill_rule);
	R_LOAD(set_scaled_font); R_LOAD(show_glyphs); R_LOAD(set_font_size); R_LOAD(move_to); R_LOAD(line_to);
	R_LOAD(curve_to); R_LOAD(close_path); R_LOAD(arc); R_LOAD(arc_negative); R_LOAD(set_operator);
	R_LOAD(text_path); R_LOAD(matrix_init); R_LOAD(matrix_init_identity); R_LOAD(matrix_translate);
	R_LOAD(matrix_scale); R_LOAD(matrix_rotate); R_LOAD(matrix_multiply); R_LOAD(matrix_invert);
	R_LOAD(matrix_transform_point); R_LOAD(matrix_init_translate); R_LOAD(matrix_init_scale);
	R_LOAD(matrix_init_rotate); R_LOAD(scaled_font_glyph_extents); R_LOAD(scaled_font_text_extents);
	R_LOAD(scaled_font_extents); R_LOAD(scaled_font_text_to_glyphs); R_LOAD(glyph_allocate); R_LOAD(glyph_free);
	R_LOAD(ft_font_face_create_for_ft_face); R_LOAD(scaled_font_create); R_LOAD(font_options_create); R_LOAD(font_options_destroy);
	R_LOAD(ft_scaled_font_lock_face); R_LOAD(ft_scaled_font_unlock_face);
	R_LOAD_FT(FT_Init_FreeType); R_LOAD_FT(FT_New_Face);
	{
		void **p = (void **)&R;
		size_t i, n = sizeof R / sizeof(void *);
		for (i = 0; i + 2 < n; i++) {	/* the FreeType ones may be absent */
			if (!p[i]) {
				printf("FAILED: the reference cairo function %d is missing\n", (int)i);
				return 0;
			}
		}
	}
	return 1;
}


/*** The plugin's state ***/

static Primitive
prim(const char *name)
{
	Primitive p = fake_primitive(name);

	if (!p) {
		printf("FAILED: no primitive %s\n", name);
		exit(1);
	}
	return p;
}

static int
primIndex(const char *name)
{
	int i;

	for (i = 0; i < CP_P_COUNT; i++) {
		if (strcmp(cp_primitive_names[i], name) == 0) {
			return i;
		}
	}
	printf("FAILED: no CP_P_ for %s\n", name);
	exit(1);
}

static uint64_t callsBefore[CP_FN_COUNT], primsBefore[CP_P_COUNT];

static void
mark(void)
{
	memcpy(callsBefore, cp_cairo_calls, sizeof callsBefore);
	memcpy(primsBefore, cp_prim_calls, sizeof primsBefore);
}

/* the cairo calls since mark() */
static uint64_t
callsSince(void)
{
	uint64_t n = 0;
	int i;

	for (i = 0; i < CP_FN_COUNT; i++) {
		n += cp_cairo_calls[i] - callsBefore[i];
	}
	return n;
}

/* exactly one cairo call since mark(), of function fn */
static int
onlyCall(int fn)
{
	return callsSince() == 1 && cp_cairo_calls[fn] == callsBefore[fn] + 1;
}

static int
primsSince(void)
{
	uint64_t n = 0;
	int i;

	for (i = 0; i < CP_P_COUNT; i++) {
		n += cp_prim_calls[i] - primsBefore[i];
	}
	return (int)n;
}

/* The P12-P15 layout (docs/CairoPlugin.md), with the string classes */
static const sqInt layoutValues[CP_LAYOUT_INTEGERS] = {
	1, 6, 7, 0, 1, 1, 0, 0, 0, 0, 1, 3, 4, 5, 6, 0, 2, 6, 0, 3, 0, 1, 0, 1, 20, 10, 1023, 255, 0
};

static int
bind(void)
{
	sqInt oops[CP_L_COUNT], args[3];
	void *probe;
	FakeAnswer a;
	int i;

	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		oops[i] = fake_int(layoutValues[i]);
	}
	oops[CP_L_CLASS_BYTE_STRING] = fakeByteString;
	oops[CP_L_CLASS_BYTE_SYMBOL] = fakeByteSymbol;
	oops[CP_L_CLASS_WIDE_STRING] = fakeWideString;
	oops[CP_L_CLASS_WIDE_SYMBOL] = fakeWideSymbol;
#ifdef CP_STATIC_CAIRO
	probe = (void *)cairo_create;
#else
	probe = dlsym(cairoHandle, "cairo_create");
#endif
	args[0] = fake_string(libcairo);
	args[1] = fake_external_address(probe);
	args[2] = fake_array(fakeArray, oops, CP_LAYOUT_SIZE_TEXT);
	fake_loader_reset();
	a = fake_call(prim("primitiveBind"), fakeNil, 3, args);
	CHECK(a.returned && a.code == 0 && cp_bound, "bind: code %lld %s", (long long)a.code, fake_last_error());
	return a.returned && cp_bound;
}


/*** The world: cairo objects, Athens-shaped fake objects ***/

#define SIZE 64

static void *surfS;		/* the scratch surface of the case table */
static void *fontFT;		/* a scaled font: FreeType when the font file loads, else the toy font */
static int haveFT;
static void *pattern;		/* a linear gradient */
static cp_matrix_t identity = { 1, 0, 0, 1, 0, 0 };

/* objects to free after a case: cairo_ts, paths, glyphs */
static void *trackedCr[64], *trackedPath[64], *trackedGlyphs[64];
static int nCr, nPath, nGlyphs;

static void *
trackCr(void *cr)
{
	if (nCr < 64) {
		trackedCr[nCr++] = cr;
	}
	return cr;
}

static void *
trackPath(void *path)
{
	if (path && nPath < 64) {
		trackedPath[nPath++] = path;
	}
	return path;
}

static void *
trackGlyphs(void *glyphs)
{
	if (glyphs && nGlyphs < 64) {
		trackedGlyphs[nGlyphs++] = glyphs;
	}
	return glyphs;
}

static void
freeTracked(void)
{
	while (nPath) {
		R.path_destroy((TPath *)trackedPath[--nPath]);
	}
	while (nGlyphs) {
		R.glyph_free((cp_glyph_t *)trackedGlyphs[--nGlyphs]);
	}
	while (nCr) {
		R.destroy(trackedCr[--nCr]);
	}
}

static sqInt
ea(void *p)
{
	return fake_external_address(p);
}

/* an AthensCairoMatrix whose handle is a 48-byte ByteArray of m */
static sqInt
newMatrix(const cp_matrix_t *m)
{
	sqInt obj = fake_new(fakeAthensCairoMatrix, 0);

	fake_set_slot(obj, 0, fake_bytes(fakeByteArray, m, sizeof *m));
	return obj;
}

static cp_matrix_t *
matrixOf(sqInt obj)
{
	return (cp_matrix_t *)fake_body(fake_slot(obj, 0));
}

/* an ExternalObject of class cls whose handle is an ExternalAddress of p */
static sqInt
newExtObj(sqInt cls, void *p)
{
	sqInt obj = fake_new(cls, 0);

	fake_set_slot(obj, 0, ea(p));
	return obj;
}

/* an AthensCairoCanvas on cr (and the surface object of surface), its
   pathTransform the matrix m */
static sqInt
newCanvasM(void *cr, void *surface, const cp_matrix_t *m)
{
	sqInt canvas = fake_new(fakeAthensCairoCanvas, 0);
	sqInt surfaceObj = fake_new(fakeAthensCairoSurface, 0);

	fake_set_slot(surfaceObj, 0, canvas);
	fake_set_slot(surfaceObj, 1, ea(surface));
	fake_set_slot(canvas, 0, fake_color(fake_int(0x3FFFFFFF), fake_int(255)));
	fake_set_slot(canvas, 1, surfaceObj);
	fake_set_slot(canvas, 6, ea(cr));
	fake_set_slot(canvas, 7, newMatrix(m));
	return canvas;
}

static sqInt
newCanvas(void *cr)
{
	static const cp_matrix_t m = { 2, 0.5, -0.25, 1.5, 3, 4 };

	return newCanvasM(cr, surfS, &m);
}

static sqInt
newBuilder(void *cr)
{
	sqInt builder = fake_new(fakeAthensCairoPathBuilder, 0);

	fake_set_slot(builder, 0, newCanvas(cr));
	return builder;
}

/* a fresh cairo_t on the scratch surface, freed after the case */
static void *
scratchCr(void)
{
	return trackCr(R.create(surfS));
}

static sqInt
doubles(const double *d, size_t n)
{
	return fake_bytes(fakeByteArray, d, n * sizeof(double));
}

/* glyphs of "Hi!" in a ByteArray (n glyphs) */
static sqInt
glyphBytes(int n)
{
	cp_glyph_t g[3] = { { 43, 2, 20 }, { 76, 14, 20 }, { 4, 20, 20 } };

	return fake_bytes(fakeByteArray, g, (size_t)n * sizeof(cp_glyph_t));
}


/*** The cases: one per primitive ***/

enum { ANS_RCVR, ANS_INT, ANS_ARG0, ANS_POINT };
enum { RK_NONE, RK_CANVAS, RK_PATH_MATRIX, RK_BUILDER, RK_PAINTMODE, RK_MATRIX_W, RK_MATRIX_R, RK_SURFACE, RK_FONT };

typedef struct {
	const char *name;
	int nargs;
	int fn;			/* CP_FN_ of its one cairo call */
	int answer;		/* ANS_ */
	int rk;			/* RK_: the receiver chain */
	void (*make)(sqInt *rcvr, sqInt *args);
	void (*after)(sqInt rcvr, sqInt *args);	/* frees what a successful call made */
} Case;

static void mkCanvas0(sqInt *r, sqInt *a) { (void)a; *r = newCanvas(scratchCr()); }
static void mkRestore(sqInt *r, sqInt *a) { void *cr = scratchCr(); (void)a; R.save(cr); *r = newCanvas(cr); }
static void mkSetMatrix(sqInt *r, sqInt *a)
{
	static const cp_matrix_t m = { 1, 0.25, 0, 1, 7, 8 };
	*r = newCanvas(scratchCr());
	a[0] = newMatrix(&m);
}
static void mkSetSourceRGBA(sqInt *r, sqInt *a)
{
	*r = newCanvas(scratchCr());
	a[0] = fake_float(0.25); a[1] = fake_int(1); a[2] = fake_boxed_float(0.5); a[3] = fake_float(0.75);
}
static void mkSetSource(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = newExtObj(fakeAthensCairoPatternPaint, pattern); }
static void mkNumber(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = fake_float(1.5); }
static void mkInt1(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = fake_int(1); }
static void mkSetDash(sqInt *r, sqInt *a)
{
	static const double d[3] = { 3, 1, 2 };
	*r = newCanvas(scratchCr());
	a[0] = doubles(d, 3); a[1] = fake_int(2); a[2] = fake_float(0.5);
}
static void mkRectangle(sqInt *r, sqInt *a)
{
	*r = newCanvas(scratchCr());
	a[0] = fake_int(3); a[1] = fake_float(4.5); a[2] = fake_int(20); a[3] = fake_float(10.25);
}
static void mkAppendPath(sqInt *r, sqInt *a)
{
	void *cr = scratchCr();
	R.rectangle(cr, 1, 2, 3, 4);
	*r = newCanvas(cr);
	a[0] = newExtObj(fakeAthensCairoPath, trackPath(R.copy_path(cr)));
}
static void mkCurrentPoint(sqInt *r, sqInt *a) { void *cr = scratchCr(); (void)a; R.move_to(cr, 3, 4.5); *r = newCanvas(cr); }
static void mkSetScaledFont(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = newExtObj(fakeCairoScaledFont, fontFT); }
static void mkShowGlyphs(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = glyphBytes(3); a[1] = fake_int(3); }
static void mkGetScaledFont(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = fake_new(fakeCairoScaledFont, 0); }
static void mkBuilder0(sqInt *r, sqInt *a) { void *cr = scratchCr(); (void)a; R.move_to(cr, 1, 1); *r = newBuilder(cr); }
static void mkBuilder2(sqInt *r, sqInt *a) { *r = newBuilder(scratchCr()); a[0] = fake_float(5.5); a[1] = fake_int(6); }
static void mkBuilder5(sqInt *r, sqInt *a)
{
	*r = newBuilder(scratchCr());
	a[0] = fake_int(30); a[1] = fake_float(31.5); a[2] = fake_int(10); a[3] = fake_float(0.0); a[4] = fake_float(2.5);
}
static void mkBuilder6(sqInt *r, sqInt *a)
{
	int i;
	*r = newBuilder(scratchCr());
	for (i = 0; i < 6; i++) {
		a[i] = i & 1 ? fake_int(i * 3) : fake_float(i * 2.5 + 0.25);
	}
}
static void mkCopyPath(sqInt *r, sqInt *a)
{
	void *cr = scratchCr();
	R.move_to(cr, 1, 2); R.line_to(cr, 10, 20);
	*r = newBuilder(cr);
	a[0] = fake_new(fakeAthensCairoPath, 0);
}
static void mkPathDestroy(sqInt *r, sqInt *a)
{
	void *cr = scratchCr();
	R.rectangle(cr, 1, 2, 3, 4);
	*r = fakeNil;	/* AthensCairoPath class: not read */
	a[0] = ea(R.copy_path(cr));
}
static const cp_matrix_t m0 = { 1.5, 0.25, -0.5, 2, 10, -3 };
static void mkMatrix0(sqInt *r, sqInt *a) { (void)a; *r = newMatrix(&m0); }
static void mkMatrix1(sqInt *r, sqInt *a) { *r = newMatrix(&m0); a[0] = fake_float(0.75); }
static void mkMatrix2(sqInt *r, sqInt *a) { *r = newMatrix(&m0); a[0] = fake_float(2.5); a[1] = fake_int(-3); }
static void mkMatrix6(sqInt *r, sqInt *a)
{
	int i;
	*r = newMatrix(&m0);
	for (i = 0; i < 6; i++) {
		a[i] = i & 1 ? fake_int(i + 1) : fake_float(i + 0.5);
	}
}
static void mkMatrixMultiply(sqInt *r, sqInt *a)
{
	static const cp_matrix_t m = { 0, 1, -1, 0, 5, 6 };
	*r = newMatrix(&m0);
	a[0] = newMatrix(&m);
}
static void mkTransformPoint(sqInt *r, sqInt *a)
{
	double x = 3, y = -4;
	*r = newMatrix(&m0);
	a[0] = doubles(&x, 1); a[1] = doubles(&y, 1);
}
static void mkPaintMode0(sqInt *r, sqInt *a)
{
	(void)a;
	*r = fake_new(fakeAthensCairoPaintMode, 0);
	fake_set_slot(*r, 0, newCanvas(scratchCr()));
}
static void mkPaintMode1(sqInt *r, sqInt *a) { mkPaintMode0(r, a); a[0] = fake_int(2); }
static void mkSurface(sqInt *r, sqInt *a)
{
	(void)a;
	*r = fake_new(fakeAthensCairoSurface, 0);
	fake_set_slot(*r, 1, ea(surfS));
}
static sqInt newFontObj(void) { return newExtObj(fakeCairoScaledFont, fontFT); }
static void mkFont0(sqInt *r, sqInt *a) { (void)a; *r = newFontObj(); }
static sqInt newExtents(sqInt cls, size_t n) { sqInt o = fake_new(cls, 0); fake_set_slot(o, 0, fake_new(fakeByteArray, n)); return o; }
static void mkGlyphExtents(sqInt *r, sqInt *a)
{
	*r = newFontObj();
	a[0] = glyphBytes(3); a[1] = fake_int(3); a[2] = newExtents(fakeCairoTextExtents, 48);
}
static void mkTextExtents(sqInt *r, sqInt *a)
{
	*r = newFontObj();
	a[0] = fake_string("Hello"); a[1] = newExtents(fakeCairoTextExtents, 48);
}
static void mkFontExtents(sqInt *r, sqInt *a) { *r = newFontObj(); a[0] = newExtents(fakeCairoFontExtents, 48); }
static void mkTextToGlyphs(sqInt *r, sqInt *a)
{
	int n = 5;
	*r = fake_new(fakeCairoFreetypeFontRenderer, 0);
	a[0] = fake_bytes(fakeByteArray, "Hello world", 11);
	a[1] = fake_int(5);
	a[2] = newFontObj();
	a[3] = ea(NULL);
	a[4] = fake_bytes(fakeByteArray, &n, 4);
	a[5] = fake_float(1.5);
	a[6] = fake_int(2);
}
static void mkGlyphFree(sqInt *r, sqInt *a)
{
	*r = fake_new(fakeCairoFreetypeFontRenderer, 0);
	a[0] = ea(R.glyph_allocate(2));
}
static void mkTextPath(sqInt *r, sqInt *a) { *r = newCanvas(scratchCr()); a[0] = fake_string("Hi"); }

static void afterCopyPath(sqInt r, sqInt *a) { (void)r; trackPath(fake_address_value(fake_slot(fake_follow(a[0]), 0))); }
static void afterTextToGlyphs(sqInt r, sqInt *a) { (void)r; trackGlyphs(fake_address_value(fake_follow(a[3]))); }

#define C(name, nargs, fn, ans, rk, mk, after) \
	{ "primitive" #name, nargs, CP_FN_##fn, ans, rk, mk, after }

static const Case cases[] = {
	C(NewPath, 0, new_path, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(Fill, 0, fill, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(FillPreserve, 0, fill_preserve, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(Stroke, 0, stroke, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(Paint, 0, paint, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(Save, 0, save, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(Restore, 0, restore, ANS_RCVR, RK_CANVAS, mkRestore, NULL),
	C(Clip, 0, clip, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(ResetClip, 0, reset_clip, ANS_RCVR, RK_CANVAS, mkCanvas0, NULL),
	C(SetPathMatrix, 0, set_matrix, ANS_RCVR, RK_PATH_MATRIX, mkCanvas0, NULL),
	C(SetMatrix, 1, set_matrix, ANS_RCVR, RK_CANVAS, mkSetMatrix, NULL),
	C(SetSourceRGBA, 4, set_source_rgba, ANS_RCVR, RK_CANVAS, mkSetSourceRGBA, NULL),
	C(SetSource, 1, set_source, ANS_RCVR, RK_CANVAS, mkSetSource, NULL),
	C(ResetDash, 1, set_dash, ANS_RCVR, RK_CANVAS, mkNumber, NULL),
	C(SetDash, 3, set_dash, ANS_RCVR, RK_CANVAS, mkSetDash, NULL),
	C(Rectangle, 4, rectangle, ANS_RCVR, RK_CANVAS, mkRectangle, NULL),
	C(SetLineWidth, 1, set_line_width, ANS_RCVR, RK_CANVAS, mkNumber, NULL),
	C(SetLineJoin, 1, set_line_join, ANS_RCVR, RK_CANVAS, mkInt1, NULL),
	C(SetLineCap, 1, set_line_cap, ANS_RCVR, RK_CANVAS, mkInt1, NULL),
	C(AppendPath, 1, append_path, ANS_RCVR, RK_CANVAS, mkAppendPath, NULL),
	C(GetCurrentPoint, 0, get_current_point, ANS_POINT, RK_CANVAS, mkCurrentPoint, NULL),
	C(SetAntialias, 1, set_antialias, ANS_RCVR, RK_CANVAS, mkInt1, NULL),
	C(SetFillRule, 1, set_fill_rule, ANS_RCVR, RK_CANVAS, mkInt1, NULL),
	C(SetScaledFont, 1, set_scaled_font, ANS_RCVR, RK_CANVAS, mkSetScaledFont, NULL),
	C(ShowGlyphs, 2, show_glyphs, ANS_RCVR, RK_CANVAS, mkShowGlyphs, NULL),
	C(GetScaledFont, 1, get_scaled_font, ANS_ARG0, RK_CANVAS, mkGetScaledFont, NULL),
	C(SetFontSize, 1, set_font_size, ANS_RCVR, RK_CANVAS, mkNumber, NULL),
	C(BuilderMoveTo, 2, move_to, ANS_RCVR, RK_BUILDER, mkBuilder2, NULL),
	C(BuilderLineTo, 2, line_to, ANS_RCVR, RK_BUILDER, mkBuilder2, NULL),
	C(BuilderCurveTo, 6, curve_to, ANS_RCVR, RK_BUILDER, mkBuilder6, NULL),
	C(BuilderClosePath, 0, close_path, ANS_RCVR, RK_BUILDER, mkBuilder0, NULL),
	C(BuilderNewPath, 0, new_path, ANS_RCVR, RK_BUILDER, mkBuilder0, NULL),
	C(BuilderCopyPath, 1, copy_path, ANS_ARG0, RK_BUILDER, mkCopyPath, afterCopyPath),
	C(BuilderArc, 5, arc, ANS_RCVR, RK_BUILDER, mkBuilder5, NULL),
	C(BuilderArcNegative, 5, arc_negative, ANS_RCVR, RK_BUILDER, mkBuilder5, NULL),
	C(PathDestroy, 1, path_destroy, ANS_RCVR, RK_NONE, mkPathDestroy, NULL),
	C(MatrixInit, 6, matrix_init, ANS_RCVR, RK_MATRIX_W, mkMatrix6, NULL),
	C(MatrixInitIdentity, 0, matrix_init_identity, ANS_RCVR, RK_MATRIX_W, mkMatrix0, NULL),
	C(MatrixTranslate, 2, matrix_translate, ANS_RCVR, RK_MATRIX_W, mkMatrix2, NULL),
	C(MatrixScale, 2, matrix_scale, ANS_RCVR, RK_MATRIX_W, mkMatrix2, NULL),
	C(MatrixRotate, 1, matrix_rotate, ANS_RCVR, RK_MATRIX_W, mkMatrix1, NULL),
	C(MatrixMultiply, 1, matrix_multiply, ANS_RCVR, RK_MATRIX_W, mkMatrixMultiply, NULL),
	C(MatrixInvert, 0, matrix_invert, ANS_INT, RK_MATRIX_W, mkMatrix0, NULL),
	C(MatrixTransformPoint, 2, matrix_transform_point, ANS_RCVR, RK_MATRIX_R, mkTransformPoint, NULL),
	C(MatrixInitTranslate, 2, matrix_init_translate, ANS_RCVR, RK_MATRIX_W, mkMatrix2, NULL),
	C(MatrixInitScale, 2, matrix_init_scale, ANS_RCVR, RK_MATRIX_W, mkMatrix2, NULL),
	C(MatrixInitRotate, 1, matrix_init_rotate, ANS_RCVR, RK_MATRIX_W, mkMatrix1, NULL),
	C(GetOperator, 0, get_operator, ANS_INT, RK_PAINTMODE, mkPaintMode0, NULL),
	C(SetOperator, 1, set_operator, ANS_RCVR, RK_PAINTMODE, mkPaintMode1, NULL),
	C(SurfaceFlush, 0, surface_flush, ANS_RCVR, RK_SURFACE, mkSurface, NULL),
	C(ScaledFontLockFace, 0, ft_scaled_font_lock_face, ANS_RCVR, RK_FONT, mkFont0, NULL),
	C(ScaledFontUnlockFace, 0, ft_scaled_font_unlock_face, ANS_RCVR, RK_FONT, mkFont0, NULL),
	C(ScaledFontGlyphExtents, 3, scaled_font_glyph_extents, ANS_RCVR, RK_FONT, mkGlyphExtents, NULL),
	C(ScaledFontTextExtents, 2, scaled_font_text_extents, ANS_RCVR, RK_FONT, mkTextExtents, NULL),
	C(ScaledFontExtents, 1, scaled_font_extents, ANS_INT, RK_FONT, mkFontExtents, NULL),
	C(TextToGlyphs, 7, scaled_font_text_to_glyphs, ANS_INT, RK_NONE, mkTextToGlyphs, afterTextToGlyphs),
	C(GlyphFree, 1, glyph_free, ANS_RCVR, RK_NONE, mkGlyphFree, NULL),
	C(TextPath, 1, text_path, ANS_RCVR, RK_CANVAS, mkTextPath, NULL),
};
#define NCASES ((int)(sizeof cases / sizeof cases[0]))

static FakeAnswer
runCase(const Case *c, sqInt rcvr, sqInt *args)
{
	return fake_call(prim(c->name), rcvr, c->nargs, args);
}


static void dropArgs(const Case *c, sqInt *args);


/*** B1 success ***/

static void
checkAnswer(const Case *c, const char *what, sqInt rcvr, sqInt *args, FakeAnswer a)
{
	switch (c->answer) {
	case ANS_RCVR:
		CHECK(a.value == rcvr, "%s %s: not the receiver", c->name, what);
		break;
	case ANS_INT:
		CHECK(fake_is_int(a.value), "%s %s: not a SmallInteger", c->name, what);
		break;
	case ANS_ARG0:
		CHECK(a.value == fake_follow(args[0]), "%s %s: not the argument", c->name, what);
		CHECK(fake_class_of(fake_slot(a.value, 0)) == fakeExternalAddress, "%s %s: no ExternalAddress handle", c->name, what);
		break;
	case ANS_POINT:
		CHECK(fake_is_object(a.value) && fake_class_of(a.value) == fakePoint
			&& fake_is_float(fake_slot(a.value, 0)) && fake_is_float(fake_slot(a.value, 1)),
			"%s %s: not a Point of Floats", c->name, what);
		break;
	}
}

static void
testSuccess(void)
{
	sqInt rcvr, args[8];
	FakeAnswer a;
	int i;

	CHECK(NCASES == 58, "%d cases, not 58", NCASES);
	for (i = 0; i < NCASES; i++) {
		const Case *c = &cases[i];
		int p = primIndex(c->name);
		c->make(&rcvr, args);
		mark();
		a = runCase(c, rcvr, args);
		CHECK(a.returned && a.code == 0 && a.protocolErrors == 0, "%s: code %lld %s", c->name, (long long)a.code, fake_last_error());
		CHECK(onlyCall(c->fn), "%s: %llu cairo calls (not one of %s)", c->name, (unsigned long long)callsSince(),
			cp_cairo_function_names[c->fn]);
		CHECK(cp_prim_calls[p] == primsBefore[p] + 1 && primsSince() == 1, "%s: not counted", c->name);
		if (a.returned) {
			checkAnswer(c, "", rcvr, args, a);
			if (c->after) {
				c->after(rcvr, args);
			}
		}
		/* the stores: only those of the answers' handles, into the argument */
		CHECK(a.stores == (c->answer == ANS_ARG0 ? 1 : c->answer == ANS_POINT ? 2 : 0), "%s: %d stores", c->name, a.stores);
		freeTracked();
	}
	/* every #basic primitive of primitives.txt has a case */
	{
		FakePrimitiveRow rows[128];
		int n = fake_read_primitives(CP_PRIMITIVES_TXT, rows, 128), k, found, basic = 0;
		for (k = 0; k < n; k++) {
			if (strcmp(rows[k].group, "basic") != 0) {
				continue;
			}
			basic++;
			found = 0;
			for (i = 0; i < NCASES; i++) {
				if (strcmp(cases[i].name, rows[k].name) == 0) {
					found = 1;
					CHECK(cases[i].nargs == rows[k].nargs, "%s: %d arguments, primitives.txt %d", rows[k].name,
						cases[i].nargs, rows[k].nargs);
				}
			}
			CHECK(found, "%s has no case", rows[k].name);
		}
		CHECK(basic == 58, "primitives.txt: %d #basic primitives", basic);
	}
}


/*** B2 receivers, B3 arguments ***/

/* c made, then rcvr/args replaced by the caller: the call must fail with
   code, no cairo call, no allocation, no store, no success counted */
static void
expectFail(const Case *c, const char *what, sqInt code, sqInt rcvr, sqInt *args)
{
	FakeAnswer a;

	mark();
	a = runCase(c, rcvr, args);
	CHECK(a.code == code && !a.returned && a.protocolErrors == 0, "%s (%s): code %lld, expected %lld %s", c->name, what,
		(long long)a.code, (long long)code, fake_last_error());
	CHECK(callsSince() == 0, "%s (%s): %llu cairo calls", c->name, what, (unsigned long long)callsSince());
	CHECK(a.allocations == 0 && a.stores == 0, "%s (%s): %d allocations, %d stores", c->name, what, a.allocations, a.stores);
	CHECK(primsSince() == 0, "%s (%s): counted", c->name, what);
}

/* the receiver variants of a chain kind: v-th variant (answers 0 when none) */
static int
badReceiver(const Case *c, int v, sqInt *rcvr, sqInt *code, const char **what)
{
	sqInt r = *rcvr, inner;
	static const cp_matrix_t m = { 1, 0, 0, 1, 0, 0 };

	*code = PrimErrBadReceiver;
	switch (v) {
	case 0: *what = "nil"; *rcvr = fakeNil; return 1;
	case 1: *what = "SmallInteger"; *rcvr = fake_int(6); return 1;
	case 2: *what = "a ByteArray"; *rcvr = fake_new(fakeByteArray, 80); return 1;
	case 3: *what = "an unfollowed forwarder"; *rcvr = fake_forwarder_to(r); return 1;
	}
	v -= 4;
	switch (c->rk) {
	case RK_CANVAS:
	case RK_PATH_MATRIX:
		switch (v) {
		case 0: *what = "handle nil"; fake_set_slot(r, 6, fakeNil); return 1;
		case 1: *what = "handle NULL"; fake_set_slot(r, 6, ea(NULL)); return 1;
		case 2: *what = "handle a ByteArray"; fake_set_slot(r, 6, fake_bytes(fakeByteArray, fake_body(fake_slot(r, 6)), 8)); return 1;
		case 3: *what = "handle 4 bytes"; fake_set_slot(r, 6, fake_bytes(fakeExternalAddress, fake_body(fake_slot(r, 6)), 4)); return 1;
		case 4: *what = "handle 16 bytes"; fake_set_slot(r, 6, fake_new(fakeExternalAddress, 16)); return 1;
		case 5: *what = "handle a forwarder"; fake_set_slot(r, 6, fake_forwarder_to(fake_slot(r, 6))); return 1;
		case 6: *what = "too short"; *rcvr = fake_new(fakeAthensCairoPathBuilder, 0); fake_set_slot(*rcvr, 1, fake_slot(r, 6)); return 1;
		case 7: *what = "an Array of 6"; *rcvr = fake_new(fakeArray, 6); return 1;
		}
		if (c->rk != RK_PATH_MATRIX) {
			return 0;
		}
		switch (v - 8) {
		case 0: *what = "pathTransform nil"; fake_set_slot(r, 7, fakeNil); return 1;
		case 1: *what = "pathTransform 47 bytes"; fake_set_slot(fake_slot(r, 7), 0, fake_bytes(fakeByteArray, &m, 47)); return 1;
		case 2: *what = "pathTransform NULL"; fake_set_slot(fake_slot(r, 7), 0, ea(NULL)); return 1;
		case 3: *what = "pathTransform handle nil"; fake_set_slot(fake_slot(r, 7), 0, fakeNil); return 1;
		case 4: *what = "pathTransform a forwarder"; fake_set_slot(r, 7, fake_forwarder_to(fake_slot(r, 7))); return 1;
		case 5: *what = "pathTransform a ByteArray"; fake_set_slot(r, 7, fake_bytes(fakeByteArray, &m, 48)); return 1;
		}
		return 0;
	case RK_BUILDER:
		inner = fake_slot(r, 0);
		switch (v) {
		case 0: *what = "context nil"; fake_set_slot(r, 0, fakeNil); return 1;
		case 1: *what = "context handle NULL"; fake_set_slot(inner, 6, ea(NULL)); return 1;
		case 2: *what = "context handle nil"; fake_set_slot(inner, 6, fakeNil); return 1;
		case 3: *what = "context a forwarder"; fake_set_slot(r, 0, fake_forwarder_to(inner)); return 1;
		case 4: *what = "context handle a forwarder"; fake_set_slot(inner, 6, fake_forwarder_to(fake_slot(inner, 6))); return 1;
		case 5: *what = "context a SmallInteger"; fake_set_slot(r, 0, fake_int(0)); return 1;
		case 6: *what = "builder of no slot"; *rcvr = fake_new(fakeArray, 0); return 1;
		}
		return 0;
	case RK_PAINTMODE:
		inner = fake_slot(r, 0);
		switch (v) {
		case 0: *what = "canvas nil"; fake_set_slot(r, 0, fakeNil); return 1;
		case 1: *what = "canvas handle NULL"; fake_set_slot(inner, 6, ea(NULL)); return 1;
		case 2: *what = "canvas a forwarder"; fake_set_slot(r, 0, fake_forwarder_to(inner)); return 1;
		case 3: *what = "paint mode of no slot"; *rcvr = fake_new(fakeArray, 0); return 1;
		}
		return 0;
	case RK_MATRIX_W:
	case RK_MATRIX_R:
		switch (v) {
		case 0: *what = "handle nil"; fake_set_slot(r, 0, fakeNil); return 1;
		case 1: *what = "47 bytes"; fake_set_slot(r, 0, fake_bytes(fakeByteArray, matrixOf(r), 47)); return 1;
		case 2: *what = "49 bytes"; fake_set_slot(r, 0, fake_new(fakeByteArray, 49)); return 1;
		case 3: *what = "NULL"; fake_set_slot(r, 0, ea(NULL)); return 1;
		case 4: *what = "12 words"; fake_set_slot(r, 0, fake_new(fakeWordArray, 12)); return 1;
		case 5: *what = "a CompiledMethod"; fake_set_slot(r, 0, fake_compiled_method(48)); return 1;
		case 6: *what = "handle a forwarder"; fake_set_slot(r, 0, fake_forwarder_to(fake_slot(r, 0))); return 1;
		case 7: *what = "of no slot"; *rcvr = fake_new(fakeArray, 0); return 1;
		case 8:
			if (c->rk == RK_MATRIX_R) {
				return 0;
			}
			*what = "read-only bytes"; fake_set_immutable(fake_slot(r, 0), 1); *code = PrimErrNoModification; return 1;
		}
		return 0;
	case RK_SURFACE:
		switch (v) {
		case 0: *what = "handle nil"; fake_set_slot(r, 1, fakeNil); return 1;
		case 1: *what = "handle NULL"; fake_set_slot(r, 1, ea(NULL)); return 1;
		case 2: *what = "too short"; *rcvr = fake_new(fakeAthensCairoMatrix, 0); fake_set_slot(*rcvr, 0, fake_slot(r, 1)); return 1;
		}
		return 0;
	case RK_FONT:
		switch (v) {
		case 0: *what = "handle nil"; fake_set_slot(r, 0, fakeNil); return 1;
		case 1: *what = "handle NULL"; fake_set_slot(r, 0, ea(NULL)); return 1;
		case 2: *what = "handle a ByteArray"; fake_set_slot(r, 0, fake_bytes(fakeByteArray, fake_body(fake_slot(r, 0)), 8)); return 1;
		case 3: *what = "of no slot"; *rcvr = fake_new(fakeArray, 0); return 1;
		}
		return 0;
	}
	return 0;
}

static void
testReceivers(void)
{
	sqInt rcvr, args[8], code;
	const char *what;
	int i, v, n;

	for (i = 0; i < NCASES; i++) {
		const Case *c = &cases[i];
		if (c->rk == RK_NONE) {
			continue;
		}
		for (v = 0, n = 0;; v++) {
			c->make(&rcvr, args);
			dropArgs(c, args);
			if (!badReceiver(c, v, &rcvr, &code, &what)) {
				freeTracked();
				break;
			}
			expectFail(c, what, code, rcvr, args);
			freeTracked();
			n++;
		}
		CHECK(n >= 7, "%s: %d receiver variants", c->name, n);
	}
	/* the receiver of primTransformX:Y: is only read: a read-only one works */
	{
		const Case *c = NULL;
		FakeAnswer a;
		for (i = 0; i < NCASES; i++) {
			if (strcmp(cases[i].name, "primitiveMatrixTransformPoint") == 0) {
				c = &cases[i];
			}
		}
		c->make(&rcvr, args);
		fake_set_immutable(fake_slot(rcvr, 0), 1);
		a = runCase(c, rcvr, args);
		CHECK(a.returned && a.code == 0, "primitiveMatrixTransformPoint: a read-only receiver %lld", (long long)a.code);
	}
}

/* frees what a case's arguments hold that only a successful call frees (the
   path of primDestroyPath:, the glyphs of freeGlyphs:), when it is not called */
static void
dropArgs(const Case *c, sqInt *args)
{
	if (c->make == mkPathDestroy) {
		trackPath(fake_address_value(args[0]));
	} else if (c->make == mkGlyphFree) {
		trackGlyphs(fake_address_value(args[0]));
	}
}

static const Case *
caseNamed(const char *name)
{
	int i;

	for (i = 0; i < NCASES; i++) {
		if (strcmp(cases[i].name + strlen("primitive"), name) == 0) {
			return &cases[i];
		}
	}
	printf("FAILED: no case %s\n", name);
	exit(1);
}

/* the case `name` with its argument i replaced by bad */
static void
argFail(const char *name, int i, sqInt bad, sqInt code, const char *what)
{
	const Case *c = caseNamed(name);
	sqInt rcvr, args[8];

	c->make(&rcvr, args);
	dropArgs(c, args);
	args[i] = bad;
	expectFail(c, what, code, rcvr, args);
	freeTracked();
}

/* An ExternalAddress where bytes are expected: fails, its 8 bytes unchanged */
static void
argFailAddress(const char *name, int i, sqInt code)
{
	void *p = (void *)(uintptr_t)0x123458;
	sqInt e = ea(p);

	argFail(name, i, e, code, "an ExternalAddress for bytes");
	CHECK(fake_address_value(e) == p && fake_size(e) == 8, "%s: the ExternalAddress changed", name);
}

/* the bad number classes of a number argument */
static void
numberFails(const char *name, int i)
{
	argFail(name, i, fakeNil, PrimErrBadArgument, "nil number");
	argFail(name, i, fake_new(fakeFraction, 0), PrimErrBadArgument, "a Fraction");
	argFail(name, i, fake_bytes(fakeLargePositiveInteger, "\1\0\0\0\0\0\0\0\1", 9), PrimErrBadArgument, "a LargeInteger");
	argFail(name, i, fake_string("1.0"), PrimErrBadArgument, "a String");
	argFail(name, i, fake_forwarder_to(fake_boxed_float(1.0)), PrimErrBadArgument, "a forwarder (no retry)");
}

static void
intFails(const char *name, int i)
{
	argFail(name, i, fakeNil, PrimErrBadArgument, "nil int");
	argFail(name, i, fake_float(1.0), PrimErrBadArgument, "a Float int");
	argFail(name, i, fake_int((sqInt)INT_MAX + 1), PrimErrBadArgument, "INT_MAX + 1");
	argFail(name, i, fake_int((sqInt)INT_MIN - 1), PrimErrBadArgument, "INT_MIN - 1");
}

/* the handle argument of an ExternalObject of class cls */
static void
handleArgFails(const char *name, int i, sqInt cls)
{
	argFail(name, i, fakeNil, PrimErrBadArgument, "nil");
	argFail(name, i, newExtObj(cls, NULL), PrimErrBadArgument, "a NULL handle");
	{
		sqInt o = fake_new(cls, 0);
		argFail(name, i, o, PrimErrBadArgument, "a nil handle");
		o = fake_new(cls, 0);
		fake_set_slot(o, 0, fake_new(fakeByteArray, 8));
		argFail(name, i, o, PrimErrBadArgument, "a ByteArray handle");
	}
	argFail(name, i, fake_new(fakeArray, 0), PrimErrBadArgument, "of no slot");
	argFail(name, i, ea((void *)(uintptr_t)0x1000), PrimErrBadArgument, "an ExternalAddress itself");
	argFail(name, i, fake_forwarder_to(newExtObj(cls, (void *)(uintptr_t)0x1000)), PrimErrBadArgument, "a forwarder (no retry)");
}

static void
matrixArgFails(const char *name, int i)
{
	sqInt o;
	static const cp_matrix_t m = { 1, 0, 0, 1, 0, 0 };

	argFail(name, i, fakeNil, PrimErrBadArgument, "nil matrix");
	o = fake_new(fakeAthensCairoMatrix, 0);
	argFail(name, i, o, PrimErrBadArgument, "a nil handle");
	o = fake_new(fakeAthensCairoMatrix, 0);
	fake_set_slot(o, 0, fake_bytes(fakeByteArray, &m, 40));
	argFail(name, i, o, PrimErrBadArgument, "40 bytes");
	o = fake_new(fakeAthensCairoMatrix, 0);
	fake_set_slot(o, 0, ea(NULL));
	argFail(name, i, o, PrimErrBadArgument, "a NULL handle");
	argFail(name, i, fake_bytes(fakeByteArray, &m, 48), PrimErrBadArgument, "the ByteArray itself");
}

/* the extents argument (an object whose handle is written, min bytes) */
static void
extentsArgFails(const char *name, int i, size_t min)
{
	sqInt o;

	argFail(name, i, fakeNil, PrimErrBadArgument, "nil extents");
	o = fake_new(fakeCairoTextExtents, 0);
	argFail(name, i, o, PrimErrBadArgument, "a nil handle");
	o = fake_new(fakeCairoTextExtents, 0);
	fake_set_slot(o, 0, fake_new(fakeByteArray, min - 1));
	argFail(name, i, o, PrimErrBadArgument, "too short");
	o = fake_new(fakeCairoTextExtents, 0);
	fake_set_slot(o, 0, ea(NULL));
	argFail(name, i, o, PrimErrBadArgument, "a NULL handle");
	o = fake_new(fakeCairoTextExtents, 0);
	fake_set_slot(o, 0, fake_new(fakeWordArray, 12));
	argFail(name, i, o, PrimErrBadArgument, "words");
	o = fake_new(fakeCairoTextExtents, 0);
	fake_set_slot(o, 0, fake_new(fakeByteArray, 48));
	fake_set_immutable(fake_slot(o, 0), 1);
	argFail(name, i, o, PrimErrNoModification, "read-only bytes");
	argFail(name, i, fake_new(fakeByteArray, 48), PrimErrBadArgument, "the ByteArray itself");
}

static void
testArguments(void)
{
	static const double d3[3] = { 1, 2, 3 };
	static const unsigned char noNul[8] = { 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h' };
	sqInt o;
	int i;

	/* canvas */
	matrixArgFails("SetMatrix", 0);
	for (i = 0; i < 4; i++) {
		numberFails("SetSourceRGBA", i);
		numberFails("Rectangle", i);
	}
	handleArgFails("SetSource", 0, fakeAthensCairoPatternPaint);
	numberFails("ResetDash", 0);
	argFailAddress("SetDash", 0, PrimErrBadArgument);
	argFail("SetDash", 0, fakeNil, PrimErrBadArgument, "nil dashes");
	argFail("SetDash", 0, fake_words(fakeWordArray, (const uint32_t *)d3, 6), PrimErrBadArgument, "words");
	argFail("SetDash", 1, fake_int(4), PrimErrBadArgument, "4 dashes in 24 bytes");
	argFail("SetDash", 1, fake_int(-1), PrimErrBadArgument, "-1 dashes");
	intFails("SetDash", 1);
	numberFails("SetDash", 2);
	{
		/* the limits: 3 dashes in 24 bytes, 0 in 0 bytes */
		const Case *c = caseNamed("SetDash");
		sqInt rcvr, args[8];
		FakeAnswer a;
		c->make(&rcvr, args);
		args[1] = fake_int(3);
		a = runCase(c, rcvr, args);
		CHECK(a.returned, "SetDash: 3 dashes in 24 bytes");
		args[0] = fake_new(fakeByteArray, 0);
		args[1] = fake_int(0);
		a = runCase(c, rcvr, args);
		CHECK(a.returned, "SetDash: 0 dashes in 0 bytes");
		args[0] = doubles(d3, 3);
		fake_set_immutable(args[0], 1);
		args[1] = fake_int(3);
		a = runCase(c, rcvr, args);
		CHECK(a.returned, "SetDash: read-only dashes are only read");
		freeTracked();
	}
	numberFails("SetLineWidth", 0);
	intFails("SetLineJoin", 0);
	intFails("SetLineCap", 0);
	handleArgFails("AppendPath", 0, fakeAthensCairoPath);
	intFails("SetAntialias", 0);
	intFails("SetFillRule", 0);
	handleArgFails("SetScaledFont", 0, fakeCairoScaledFont);
	/* showGlyphs: the glyphs are an ExternalAddress (tested first) or bytes */
	argFail("ShowGlyphs", 0, ea(NULL), PrimErrBadArgument, "a NULL ExternalAddress");
	{
		/* a NULL ExternalAddress with 0 glyphs: the empty string of stock Athens */
		const Case *c = caseNamed("ShowGlyphs");
		sqInt rcvr, args[8];
		FakeAnswer a;
		c->make(&rcvr, args);
		args[0] = ea(NULL);
		args[1] = fake_int(0);
		a = runCase(c, rcvr, args);
		CHECK(a.returned, "ShowGlyphs: NULL glyphs with n = 0");
		freeTracked();
		c = caseNamed("ScaledFontGlyphExtents");
		c->make(&rcvr, args);
		args[0] = ea(NULL);
		args[1] = fake_int(0);
		a = runCase(c, rcvr, args);
		CHECK(a.returned, "ScaledFontGlyphExtents: NULL glyphs with n = 0");
		freeTracked();
	}
	argFail("ShowGlyphs", 0, glyphBytes(2), PrimErrBadArgument, "2 glyphs for 3");
	argFail("ShowGlyphs", 0, fakeNil, PrimErrBadArgument, "nil glyphs");
	argFail("ShowGlyphs", 0, fake_new(fakeWordArray, 18), PrimErrBadArgument, "words");
	argFail("ShowGlyphs", 1, fake_int(-1), PrimErrBadArgument, "-1 glyphs");
	intFails("ShowGlyphs", 1);
	/* getScaledFont: the target */
	argFail("GetScaledFont", 0, fakeNil, PrimErrBadArgument, "nil target");
	argFail("GetScaledFont", 0, fake_int(1), PrimErrBadArgument, "a SmallInteger target");
	argFail("GetScaledFont", 0, fake_new(fakeArray, 0), PrimErrBadArgument, "a target of no slot");
	argFail("GetScaledFont", 0, fake_new(fakeByteArray, 8), PrimErrBadArgument, "a bytes target");
	o = fake_new(fakeCairoScaledFont, 0);
	fake_set_immutable(o, 1);
	argFail("GetScaledFont", 0, o, PrimErrNoModification, "a read-only target");
	argFail("GetScaledFont", 0, fake_forwarder_to(fake_new(fakeCairoScaledFont, 0)), PrimErrBadArgument, "a forwarder (no retry)");
	/* slot 0 must be nil (basicNew): never replace a live handle or slot 0 of another object */
	argFail("GetScaledFont", 0, newExtObj(fakeCairoScaledFont, (void *)(uintptr_t)0x1000), PrimErrBadArgument, "a font that has a handle");
	argFail("GetScaledFont", 0, newExtObj(fakeCairoScaledFont, NULL), PrimErrBadArgument, "a font whose handle is NULL");
	o = fake_new(fakeCairoScaledFont, 0);
	fake_set_slot(o, 0, fake_int(42));
	argFail("GetScaledFont", 0, o, PrimErrBadArgument, "slot 0 a SmallInteger (a married Context's sender)");
	o = fake_new(fakeArray, 2);
	fake_set_slot(o, 0, fake_int(42));
	fake_set_slot(o, 1, fake_int(43));
	argFail("GetScaledFont", 0, o, PrimErrBadArgument, "an Array #(42 43)");
	CHECK(fake_slot(o, 0) == fake_int(42) && fake_slot(o, 1) == fake_int(43), "GetScaledFont: the Array changed");
	o = fake_new(fakeCairoScaledFont, 0);
	fake_set_slot(o, 0, fake_int(42));
	fake_set_immutable(o, 1);
	argFail("GetScaledFont", 0, o, PrimErrBadArgument, "slot 0 not nil and read-only");
	numberFails("SetFontSize", 0);
	/* builder */
	for (i = 0; i < 2; i++) {
		numberFails("BuilderMoveTo", i);
		numberFails("BuilderLineTo", i);
	}
	for (i = 0; i < 6; i++) {
		numberFails("BuilderCurveTo", i);
	}
	for (i = 0; i < 5; i++) {
		numberFails("BuilderArc", i);
		numberFails("BuilderArcNegative", i);
	}
	argFail("BuilderCopyPath", 0, fakeNil, PrimErrBadArgument, "nil target");
	argFail("BuilderCopyPath", 0, fake_new(fakeArray, 0), PrimErrBadArgument, "a target of no slot");
	o = fake_new(fakeAthensCairoPath, 0);
	fake_set_immutable(o, 1);
	argFail("BuilderCopyPath", 0, o, PrimErrNoModification, "a read-only target");
	argFail("BuilderCopyPath", 0, newExtObj(fakeAthensCairoPath, (void *)(uintptr_t)0x1000), PrimErrBadArgument, "a path that has a handle");
	o = fake_new(fakeArray, 1);
	fake_set_slot(o, 0, fake_int(7));
	argFail("BuilderCopyPath", 0, o, PrimErrBadArgument, "an Array #(7)");
	CHECK(fake_slot(o, 0) == fake_int(7), "BuilderCopyPath: the Array changed");
	/* path destroy */
	argFail("PathDestroy", 0, ea(NULL), PrimErrBadArgument, "NULL");
	argFail("PathDestroy", 0, fakeNil, PrimErrBadArgument, "nil");
	argFail("PathDestroy", 0, fake_int(5), PrimErrBadArgument, "a SmallInteger");
	argFail("PathDestroy", 0, fake_new(fakeByteArray, 8), PrimErrBadArgument, "a ByteArray");
	argFail("PathDestroy", 0, newExtObj(fakeAthensCairoPath, (void *)(uintptr_t)0x1000), PrimErrBadArgument, "an AthensCairoPath");
	/* matrices */
	for (i = 0; i < 6; i++) {
		numberFails("MatrixInit", i);
	}
	for (i = 0; i < 2; i++) {
		numberFails("MatrixTranslate", i);
		numberFails("MatrixScale", i);
		numberFails("MatrixInitTranslate", i);
		numberFails("MatrixInitScale", i);
	}
	numberFails("MatrixRotate", 0);
	numberFails("MatrixInitRotate", 0);
	matrixArgFails("MatrixMultiply", 0);
	for (i = 0; i < 2; i++) {
		double x = 1;
		argFailAddress("MatrixTransformPoint", i, PrimErrBadArgument);
		argFail("MatrixTransformPoint", i, fake_new(fakeByteArray, 7), PrimErrBadArgument, "7 bytes");
		argFail("MatrixTransformPoint", i, fakeNil, PrimErrBadArgument, "nil");
		argFail("MatrixTransformPoint", i, fake_float(1.0), PrimErrBadArgument, "a Float");
		argFail("MatrixTransformPoint", i, fake_boxed_float(1.0), PrimErrBadArgument, "a BoxedFloat64 (words)");
		o = doubles(&x, 1);
		fake_set_immutable(o, 1);
		argFail("MatrixTransformPoint", i, o, PrimErrNoModification, "read-only");
	}
	/* paint mode, fonts */
	intFails("SetOperator", 0);
	for (i = 0; i < 1; i++) {
		argFail("ScaledFontGlyphExtents", 0, ea(NULL), PrimErrBadArgument, "NULL glyphs");
		argFail("ScaledFontGlyphExtents", 0, glyphBytes(2), PrimErrBadArgument, "2 glyphs for 3");
		argFail("ScaledFontGlyphExtents", 0, fakeNil, PrimErrBadArgument, "nil glyphs");
		argFail("ScaledFontGlyphExtents", 1, fake_int(-1), PrimErrBadArgument, "-1 glyphs");
		intFails("ScaledFontGlyphExtents", 1);
		extentsArgFails("ScaledFontGlyphExtents", 2, 48);
	}
	argFailAddress("ScaledFontTextExtents", 0, PrimErrBadArgument);
	argFail("ScaledFontTextExtents", 0, fake_bytes(fakeByteString, noNul, 8), PrimErrBadArgument, "no NUL");
	argFail("ScaledFontTextExtents", 0, fake_string(""), PrimErrBadArgument, "empty");
	argFail("ScaledFontTextExtents", 0, fakeNil, PrimErrBadArgument, "nil");
	argFail("ScaledFontTextExtents", 0, fake_words(fakeWideString, (const uint32_t *)"abcd", 1), PrimErrBadArgument, "a WideString");
	extentsArgFails("ScaledFontTextExtents", 1, 48);
	extentsArgFails("ScaledFontExtents", 0, 40);
	/* text to glyphs */
	argFailAddress("TextToGlyphs", 0, PrimErrBadArgument);
	argFail("TextToGlyphs", 0, fakeNil, PrimErrBadArgument, "nil utf8");
	argFail("TextToGlyphs", 1, fake_int(12), PrimErrBadArgument, "len past the bytes");
	argFail("TextToGlyphs", 1, fake_int(-1), PrimErrBadArgument, "len -1");
	intFails("TextToGlyphs", 1);
	handleArgFails("TextToGlyphs", 2, fakeCairoScaledFont);
	argFail("TextToGlyphs", 3, fake_new(fakeByteArray, 8), PrimErrBadArgument, "glyphs a ByteArray");
	argFail("TextToGlyphs", 3, fakeNil, PrimErrBadArgument, "glyphs nil");
	argFail("TextToGlyphs", 3, fake_bytes(fakeExternalAddress, "abcd", 4), PrimErrBadArgument, "glyphs 4 bytes");
	o = ea(NULL);
	fake_set_immutable(o, 1);
	argFail("TextToGlyphs", 3, o, PrimErrNoModification, "glyphs read-only");
	argFailAddress("TextToGlyphs", 4, PrimErrBadArgument);
	argFail("TextToGlyphs", 4, fake_new(fakeByteArray, 3), PrimErrBadArgument, "numGlyphs 3 bytes");
	argFail("TextToGlyphs", 4, fakeNil, PrimErrBadArgument, "numGlyphs nil");
	o = fake_new(fakeByteArray, 4);
	fake_set_immutable(o, 1);
	argFail("TextToGlyphs", 4, o, PrimErrNoModification, "numGlyphs read-only");
	numberFails("TextToGlyphs", 5);
	numberFails("TextToGlyphs", 6);
	/* glyph free: an ExternalAddress (NULL allowed) */
	argFail("GlyphFree", 0, fakeNil, PrimErrBadArgument, "nil");
	argFail("GlyphFree", 0, fake_new(fakeByteArray, 8), PrimErrBadArgument, "a ByteArray");
	argFail("GlyphFree", 0, fake_bytes(fakeExternalAddress, "abcd", 4), PrimErrBadArgument, "4 bytes");
	/* text path: a C string within the object */
	argFailAddress("TextPath", 0, PrimErrBadArgument);
	argFail("TextPath", 0, fake_bytes(fakeByteString, noNul, 8), PrimErrBadArgument, "no NUL");
	{
		unsigned char sixteen[16];
		memset(sixteen, 'x', sizeof sixteen);
		argFail("TextPath", 0, fake_bytes(fakeByteArray, sixteen, 16), PrimErrBadArgument, "16 bytes, no NUL");
	}
	argFail("TextPath", 0, fake_string(""), PrimErrBadArgument, "empty");
	argFail("TextPath", 0, fakeNil, PrimErrBadArgument, "nil");
	argFail("TextPath", 0, fake_int(65), PrimErrBadArgument, "a SmallInteger");
	argFail("TextPath", 0, fake_compiled_method(16), PrimErrBadArgument, "a CompiledMethod");
}


/*** B4 forwarders ***/

/* A position: the stack value k (1.. the arguments; the receiver is never a
   forwarder at a primitive's entry), then up to 3 slot indices */
typedef struct { int k; int n; int slots[3]; } Pos;

static int
forwardable(sqInt oop)
{
	return !fake_is_int(oop) && (oop & 7) == 0 && fake_is_object(oop) && !fake_is_forwarder(oop)
		&& fake_format(oop) != 0 && fake_class_of(fake_class_of(oop)) != fakeMetaclass;
}

static void
collect(sqInt oop, Pos *at, Pos *out, int *n, int max)
{
	size_t i;

	if (*n >= max) {
		return;
	}
	out[(*n)++] = *at;
	if (at->n == 3 || fake_format(oop) < 1 || fake_format(oop) > 5) {
		return;
	}
	for (i = 0; i < fake_size(oop); i++) {
		sqInt field = fake_slot(oop, i);
		if (forwardable(field)) {
			Pos next = *at;
			next.slots[next.n++] = (int)i;
			collect(field, &next, out, n, max);
		}
	}
}

static void
testForwarders(void)
{
	sqInt rcvr, args[8], stackv[9];
	Pos pos[256];
	FakeAnswer a;
	int i, k, n, p, j, tested = 0;

	for (i = 0; i < NCASES; i++) {
		const Case *c = &cases[i];
		int depth = fake_accessor_depth(c->name) + GB_DEPTH_DELTA;
		/* the positions, on one instance */
		c->make(&rcvr, args);
		n = 0;
		for (k = 0; k <= c->nargs; k++) {
			sqInt v = k == 0 ? rcvr : args[k - 1];
			if (!forwardable(v)) {
				continue;
			}
			if (k == 0) {
				/* the receiver's slots only */
				size_t s;
				if (fake_format(v) < 1 || fake_format(v) > 5) {
					continue;
				}
				for (s = 0; s < fake_size(v); s++) {
					if (forwardable(fake_slot(v, s))) {
						Pos at = { 0, 1, { (int)s, 0, 0 } };
						collect(fake_slot(v, s), &at, pos, &n, 256);
					}
				}
			} else {
				Pos at = { k, 0, { 0, 0, 0 } };
				collect(v, &at, pos, &n, 256);
			}
		}
		dropArgs(c, args);
		freeTracked();
		for (p = 0; p < n; p++) {
			sqInt holder, target;
			uint64_t baseline;
			c->make(&rcvr, args);
			stackv[0] = rcvr;
			for (k = 0; k < c->nargs; k++) {
				stackv[k + 1] = args[k];
			}
			if (pos[p].n == 0) {
				stackv[pos[p].k] = fake_forwarder_to(stackv[pos[p].k]);
			} else {
				holder = stackv[pos[p].k];
				for (j = 0; j + 1 < pos[p].n; j++) {
					holder = fake_slot(holder, (size_t)pos[p].slots[j]);
				}
				target = fake_slot(holder, (size_t)pos[p].slots[pos[p].n - 1]);
				fake_set_slot(holder, (size_t)pos[p].slots[pos[p].n - 1], fake_forwarder_to(target));
			}
			mark();
			a = fake_call_retry(prim(c->name), depth, stackv[0], c->nargs, stackv + 1);
			baseline = 1;
			CHECK(a.returned && a.code == 0 && a.protocolErrors == 0,
				"%s, forwarder at stack %d level %d: code %lld (first %lld, retried %d) %s", c->name, pos[p].k,
				pos[p].n, (long long)a.code, (long long)a.firstCode, a.retried, fake_last_error());
			CHECK(callsSince() == baseline && onlyCall(c->fn), "%s, forwarder at stack %d level %d: %llu cairo calls",
				c->name, pos[p].k, pos[p].n, (unsigned long long)callsSince());
			CHECK(!a.retried || a.firstCode != 0, "%s: retried without a failure", c->name);
			if (a.returned) {
				for (k = 0; k < c->nargs; k++) {
					args[k] = stackv[k + 1];
				}
				checkAnswer(c, "after a forwarder", fake_follow(stackv[0]), args, a);
				if (c->after) {
					c->after(stackv[0], args);
				}
			}
			freeTracked();
			tested++;
		}
		CHECK(n >= 1 || c->nargs == 0, "%s: no forwarder position", c->name);
	}
	CHECK(tested > 300, "only %d forwarder positions", tested);
	printf("-- glue-basic: %d forwarder positions\n", tested);
}


/*** B5 effects: twin cairo_ts ***/

static void *surfA, *surfB, *crA, *crB;
static sqInt canvasA;

static int
sameDoubles(const double *a, const double *b, int n)
{
	return memcmp(a, b, (size_t)n * sizeof(double)) == 0;
}

static int samePath(const TPath *a, const TPath *b);

/* the whole gstate of crA and crB equal */
static void
sameState(const char *what)
{
	cp_matrix_t ma, mb;
	double a[8], b[8], da[16], db[16];
	void *pa, *pb;
	TPath *qa, *qb;
	int ta, tb, na, nb;

	CHECK(R.status(crA) == 0 && R.status(crB) == 0, "%s: status %d %d", what, R.status(crA), R.status(crB));
	R.get_matrix(crA, &ma);
	R.get_matrix(crB, &mb);
	CHECK(memcmp(&ma, &mb, sizeof ma) == 0, "%s: matrix", what);
	pa = R.get_source(crA);
	pb = R.get_source(crB);
	ta = R.pattern_get_type(pa);
	tb = R.pattern_get_type(pb);
	CHECK(ta == tb, "%s: source type %d %d", what, ta, tb);
	if (ta == 0 && tb == 0) {
		R.pattern_get_rgba(pa, &a[0], &a[1], &a[2], &a[3]);
		R.pattern_get_rgba(pb, &b[0], &b[1], &b[2], &b[3]);
		CHECK(sameDoubles(a, b, 4), "%s: source rgba %g %g %g %g / %g %g %g %g", what, a[0], a[1], a[2], a[3], b[0], b[1], b[2], b[3]);
	} else {
		CHECK(pa == pb, "%s: source pattern", what);
	}
	a[0] = R.get_line_width(crA);
	b[0] = R.get_line_width(crB);
	CHECK(a[0] == b[0], "%s: line width %g %g", what, a[0], b[0]);
	CHECK(R.get_line_join(crA) == R.get_line_join(crB), "%s: line join", what);
	CHECK(R.get_line_cap(crA) == R.get_line_cap(crB), "%s: line cap", what);
	na = R.get_dash_count(crA);
	nb = R.get_dash_count(crB);
	CHECK(na == nb && na <= 16, "%s: dash count %d %d", what, na, nb);
	if (na == nb && na <= 16) {
		R.get_dash(crA, da, &a[0]);
		R.get_dash(crB, db, &b[0]);
		CHECK(sameDoubles(da, db, na) && a[0] == b[0], "%s: dashes", what);
	}
	CHECK(R.has_current_point(crA) == R.has_current_point(crB), "%s: has current point", what);
	R.get_current_point(crA, &a[0], &a[1]);
	R.get_current_point(crB, &b[0], &b[1]);
	CHECK(sameDoubles(a, b, 2), "%s: current point", what);
	CHECK(R.get_fill_rule(crA) == R.get_fill_rule(crB), "%s: fill rule", what);
	CHECK(R.get_operator(crA) == R.get_operator(crB), "%s: operator", what);
	CHECK(R.get_antialias(crA) == R.get_antialias(crB), "%s: antialias", what);
	R.get_font_matrix(crA, &ma);
	R.get_font_matrix(crB, &mb);
	CHECK(memcmp(&ma, &mb, sizeof ma) == 0, "%s: font matrix", what);
	CHECK(R.scaled_font_get_font_face(R.get_scaled_font(crA)) == R.scaled_font_get_font_face(R.get_scaled_font(crB)),
		"%s: font face", what);
	R.clip_extents(crA, &a[0], &a[1], &a[2], &a[3]);
	R.clip_extents(crB, &b[0], &b[1], &b[2], &b[3]);
	CHECK(sameDoubles(a, b, 4), "%s: clip extents", what);
	qa = R.copy_path(crA);
	qb = R.copy_path(crB);
	CHECK(samePath(qa, qb), "%s: path (%d, %d data)", what, qa->num_data, qb->num_data);
	R.path_destroy(qa);
	R.path_destroy(qb);
}

/* the same path data: each element a header (type, length: its first 8
   bytes; the other 8 are the union's padding) followed by length - 1 points */
static int
samePath(const TPath *a, const TPath *b)
{
	const unsigned char *da = (const unsigned char *)a->data, *db = (const unsigned char *)b->data;
	int i, len;

	if (a->status != b->status || a->num_data != b->num_data) {
		return 0;
	}
	for (i = 0; i < a->num_data; i += len) {
		len = ((const int *)(da + (size_t)i * 16))[1];
		if (memcmp(da + (size_t)i * 16, db + (size_t)i * 16, 8) != 0 || len < 1 || i + len > a->num_data
		 || (len > 1 && memcmp(da + (size_t)(i + 1) * 16, db + (size_t)(i + 1) * 16, (size_t)(len - 1) * 16) != 0)) {
			return 0;
		}
	}
	return 1;
}

static void
samePixels(const char *what)
{
	unsigned char *a, *b;

	R.surface_flush(surfA);
	R.surface_flush(surfB);
	a = R.image_surface_get_data(surfA);
	b = R.image_surface_get_data(surfB);
	CHECK(memcmp(a, b, (size_t)R.image_surface_get_stride(surfA) * SIZE) == 0, "%s: pixels differ", what);
}

static int
nonBlank(void *surface)
{
	unsigned char *d = R.image_surface_get_data(surface);
	size_t i, n = (size_t)R.image_surface_get_stride(surface) * SIZE;

	R.surface_flush(surface);
	for (i = 0; i < n; i++) {
		if (d[i]) {
			return 1;
		}
	}
	return 0;
}

/* calls primitive name on crA's objects (one cairo call of fn), checks it
   answered, then the caller makes the same call on crB */
static FakeAnswer
onA(const char *name, int fn, sqInt rcvr, int nargs, const sqInt *args)
{
	FakeAnswer a;

	mark();
	a = fake_call(prim(name), rcvr, nargs, args);
	CHECK(a.returned && a.code == 0 && a.protocolErrors == 0, "%s on A: code %lld %s", name, (long long)a.code, fake_last_error());
	CHECK(onlyCall(fn), "%s on A: %llu cairo calls", name, (unsigned long long)callsSince());
	return a;
}

#define A0(name, fn) onA("primitive" #name, CP_FN_##fn, canvasA, 0, NULL)
#define A1(name, fn, x) do { sqInt a_[1] = { x }; onA("primitive" #name, CP_FN_##fn, canvasA, 1, a_); } while (0)

static sqInt
builderA(void)
{
	sqInt b = fake_new(fakeAthensCairoPathBuilder, 0);

	fake_set_slot(b, 0, canvasA);
	return b;
}

static void
testEffects(void)
{
	static const cp_matrix_t pathM = { 1.25, 0.125, -0.25, 0.75, 5, 3 };
	sqInt args[8], b, o;
	FakeAnswer a;
	double x, y;

	surfA = R.image_surface_create(0, SIZE, SIZE);
	surfB = R.image_surface_create(0, SIZE, SIZE);
	crA = R.create(surfA);
	crB = R.create(surfB);
	canvasA = newCanvasM(crA, surfA, &pathM);
	sameState("start");

	/* the source, the line attributes, the dashes, the rules */
	args[0] = fake_float(0.25); args[1] = fake_int(1); args[2] = fake_boxed_float(0.1); args[3] = fake_float(0.75);
	onA("primitiveSetSourceRGBA", CP_FN_set_source_rgba, canvasA, 4, args);
	R.set_source_rgba(crB, 0.25, 1, 0.1, 0.75);
	sameState("setSourceR:g:b:a:");
	A1(SetLineWidth, set_line_width, fake_float(2.5)); R.set_line_width(crB, 2.5); sameState("line width");
	A1(SetLineWidth, set_line_width, fake_int(3)); R.set_line_width(crB, 3); sameState("line width 3");
	A1(SetLineJoin, set_line_join, fake_int(1)); R.set_line_join(crB, 1); sameState("line join");
	A1(SetLineCap, set_line_cap, fake_int(2)); R.set_line_cap(crB, 2); sameState("line cap");
	{
		static const double d[3] = { 4, 2.5, 1 };
		args[0] = doubles(d, 3); args[1] = fake_int(3); args[2] = fake_float(1.5);
		onA("primitiveSetDash", CP_FN_set_dash, canvasA, 3, args);
		R.set_dash(crB, d, 3, 1.5);
		sameState("dashes");
		args[1] = fake_int(2);
		onA("primitiveSetDash", CP_FN_set_dash, canvasA, 3, args);
		R.set_dash(crB, d, 2, 1.5);
		sameState("2 of 3 dashes");
	}
	A1(ResetDash, set_dash, fake_float(0.0)); R.set_dash(crB, NULL, 0, 0.0); sameState("reset dashes");
	A1(SetAntialias, set_antialias, fake_int(1)); R.set_antialias(crB, 1); sameState("antialias");
	A1(SetFillRule, set_fill_rule, fake_int(1)); R.set_fill_rule(crB, 1); sameState("fill rule");
	A1(SetFontSize, set_font_size, fake_float(13.5)); R.set_font_size(crB, 13.5); sameState("font size");
	/* the operator, through the paint mode */
	o = fake_new(fakeAthensCairoPaintMode, 0);
	fake_set_slot(o, 0, canvasA);
	a = onA("primitiveGetOperator", CP_FN_get_operator, o, 0, NULL);
	CHECK(a.value == fake_int(R.get_operator(crB)), "getOperator: %lld", (long long)fake_int_value(a.value));
	args[0] = fake_int(1);
	onA("primitiveSetOperator", CP_FN_set_operator, o, 1, args);
	R.set_operator(crB, 1);
	sameState("operator");
	a = onA("primitiveGetOperator", CP_FN_get_operator, o, 0, NULL);
	CHECK(a.value == fake_int(1), "getOperator after set: %lld", (long long)fake_int_value(a.value));
	args[0] = fake_int(2);
	onA("primitiveSetOperator", CP_FN_set_operator, o, 1, args);
	R.set_operator(crB, 2);
	sameState("operator over");
	/* the matrices */
	A0(SetPathMatrix, set_matrix); R.set_matrix(crB, &pathM); sameState("setPathMatrix");
	{
		static const cp_matrix_t m = { 0.5, 0.25, 0.125, 2, -1, 7 };
		cp_matrix_t cm = m;
		A1(SetMatrix, set_matrix, newMatrix(&m)); R.set_matrix(crB, &m); sameState("setPathMatrix:");
		o = newMatrix(&identity);
		fake_set_slot(o, 0, ea(&cm));
		A1(SetMatrix, set_matrix, o); R.set_matrix(crB, &m); sameState("setPathMatrix: an ExternalAddress handle");
	}
	A0(SetPathMatrix, set_matrix); R.set_matrix(crB, &pathM); sameState("setPathMatrix again");
	/* a rectangle, filled */
	args[0] = fake_int(2); args[1] = fake_float(3.5); args[2] = fake_int(20); args[3] = fake_boxed_float(1e-310 + 12.25);
	onA("primitiveRectangle", CP_FN_rectangle, canvasA, 4, args);
	R.rectangle(crB, 2, 3.5, 20, 1e-310 + 12.25);
	sameState("rectangle");
	A0(FillPreserve, fill_preserve); R.fill_preserve(crB); sameState("fillPreserve");
	A0(Fill, fill); R.fill(crB); sameState("fill");
	CHECK(nonBlank(surfA), "nothing drawn");
	samePixels("rectangle fill");
	/* a path built through the builder, stroked */
	b = builderA();
	args[0] = fake_int(5); args[1] = fake_float(6.5);
	onA("primitiveBuilderMoveTo", CP_FN_move_to, b, 2, args); R.move_to(crB, 5, 6.5); sameState("moveTo");
	args[0] = fake_float(40.25); args[1] = fake_int(9);
	onA("primitiveBuilderLineTo", CP_FN_line_to, b, 2, args); R.line_to(crB, 40.25, 9); sameState("lineTo");
	args[0] = fake_int(45); args[1] = fake_int(20); args[2] = fake_float(30.5); args[3] = fake_float(35);
	args[4] = fake_int(10); args[5] = fake_float(25.75);
	onA("primitiveBuilderCurveTo", CP_FN_curve_to, b, 6, args); R.curve_to(crB, 45, 20, 30.5, 35, 10, 25.75); sameState("curveTo");
	a = onA("primitiveGetCurrentPoint", CP_FN_get_current_point, canvasA, 0, NULL);
	R.get_current_point(crB, &x, &y);
	CHECK(a.returned && fake_class_of(a.value) == fakePoint && fake_float_value(fake_slot(a.value, 0)) == x
		&& fake_float_value(fake_slot(a.value, 1)) == y, "getCurrentPoint: not %g @ %g", x, y);
	onA("primitiveBuilderClosePath", CP_FN_close_path, b, 0, NULL); R.close_path(crB); sameState("closePath");
	/* copyPath, appended (loadPath:) after a newPath */
	o = fake_new(fakeAthensCairoPath, 0);
	args[0] = o;
	a = onA("primitiveBuilderCopyPath", CP_FN_copy_path, b, 1, args);
	CHECK(a.value == o && fake_class_of(fake_slot(o, 0)) == fakeExternalAddress && fake_address_value(fake_slot(o, 0)) != NULL,
		"copyPath: the answer");
	{
		TPath *mine = (TPath *)fake_address_value(fake_slot(o, 0)), *ref = R.copy_path(crB);
		CHECK(mine && mine->status == 0 && ref->num_data > 4 && samePath(mine, ref), "copyPath: the path");
		onA("primitiveBuilderNewPath", CP_FN_new_path, b, 0, NULL); R.new_path(crB); sameState("builder newPath");
		A1(AppendPath, append_path, o); R.append_path(crB, ref); sameState("loadPath:");
		A0(Stroke, stroke); R.stroke(crB); sameState("stroke");
		samePixels("path stroke");
		/* primDestroyPath: */
		args[0] = fake_slot(o, 0);
		onA("primitivePathDestroy", CP_FN_path_destroy, fakeNil, 1, args);
		CHECK(fake_address_value(fake_slot(o, 0)) == mine, "primDestroyPath: nulled the address");
		R.path_destroy(ref);
	}
	/* arcs, with clips and save/restore */
	A0(Save, save); R.save(crB); sameState("save");
	args[0] = fake_int(32); args[1] = fake_int(32); args[2] = fake_float(20.5); args[3] = fake_int(0); args[4] = fake_float(4.0);
	onA("primitiveBuilderArc", CP_FN_arc, b, 5, args); R.arc(crB, 32, 32, 20.5, 0, 4.0); sameState("arc");
	A0(Clip, clip); R.clip(crB); sameState("clip");
	args[0] = fake_float(0.0); args[1] = fake_float(0.5); args[2] = fake_int(1); args[3] = fake_float(0.5);
	onA("primitiveSetSourceRGBA", CP_FN_set_source_rgba, canvasA, 4, args); R.set_source_rgba(crB, 0, 0.5, 1, 0.5);
	A0(Paint, paint); R.paint(crB); sameState("paint");
	samePixels("clipped paint");
	A0(ResetClip, reset_clip); R.reset_clip(crB); sameState("resetClip");
	args[0] = fake_int(20); args[1] = fake_int(40); args[2] = fake_int(10); args[3] = fake_float(1.0); args[4] = fake_float(-2.0);
	onA("primitiveBuilderArcNegative", CP_FN_arc_negative, b, 5, args); R.arc_negative(crB, 20, 40, 10, 1.0, -2.0); sameState("arcNegative");
	A0(Fill, fill); R.fill(crB); sameState("fill arcs");
	A0(Restore, restore); R.restore(crB); sameState("restore");
	samePixels("arcs");
	/* a pattern source */
	A1(SetSource, set_source, newExtObj(fakeAthensCairoPatternPaint, pattern)); R.set_source(crB, pattern); sameState("setSource:");
	args[0] = fake_int(0); args[1] = fake_int(40); args[2] = fake_int(64); args[3] = fake_int(20);
	onA("primitiveRectangle", CP_FN_rectangle, canvasA, 4, args); R.rectangle(crB, 0, 40, 64, 20);
	A0(Fill, fill); R.fill(crB); sameState("gradient fill");
	samePixels("gradient fill");
	A0(NewPath, new_path); R.new_path(crB); sameState("newPath");
	/* text: the scaled font, glyphs and a text path */
	A1(SetScaledFont, set_scaled_font, newExtObj(fakeCairoScaledFont, fontFT)); R.set_scaled_font(crB, fontFT);
	sameState("setScaledFont:");
	args[0] = fake_new(fakeCairoScaledFont, 0);
	a = onA("primitiveGetScaledFont", CP_FN_get_scaled_font, canvasA, 1, args);
	CHECK(a.value == args[0] && fake_address_value(fake_slot(args[0], 0)) == R.get_scaled_font(crA),
		"getScaledFont: the handle");
	CHECK(a.allocations == 1 && a.stores == 1 && a.foreignStores == 1, "getScaledFont: %d allocations, %d stores",
		a.allocations, a.stores);
	{
		cp_glyph_t g[3] = { { 43, 2, 20 }, { 76, 14, 20 }, { 4, 20, 20 } };
		args[0] = fake_bytes(fakeByteArray, g, sizeof g); args[1] = fake_int(3);
		onA("primitiveShowGlyphs", CP_FN_show_glyphs, canvasA, 2, args); R.show_glyphs(crB, g, 3);
		sameState("showGlyphs: bytes");
		args[0] = ea(g); args[1] = fake_int(2);
		onA("primitiveShowGlyphs", CP_FN_show_glyphs, canvasA, 2, args); R.show_glyphs(crB, g, 2);
		sameState("showGlyphs: an ExternalAddress");
		args[1] = fake_int(0);
		onA("primitiveShowGlyphs", CP_FN_show_glyphs, canvasA, 2, args); R.show_glyphs(crB, g, 0);
		sameState("showGlyphs: none");
		samePixels("showGlyphs");
	}
	A1(TextPath, text_path, fake_string("Ab")); R.text_path(crB, "Ab"); sameState("textPath:");
	A1(TextPath, text_path, fake_bytes(fakeByteArray, "xy\0zz", 5)); R.text_path(crB, "xy"); sameState("textPath: NUL inside");
	A0(Fill, fill); R.fill(crB); sameState("fill text path");
	samePixels("text path");
	/* the surface */
	o = fake_new(fakeAthensCairoSurface, 0);
	fake_set_slot(o, 1, ea(surfA));
	a = onA("primitiveSurfaceFlush", CP_FN_surface_flush, o, 0, NULL);
	CHECK(a.value == o, "flush: the receiver");
	sameState("end");
	CHECK(R.status(crA) == 0, "crA status %d", R.status(crA));
	R.destroy(crA);
	R.destroy(crB);
	R.surface_destroy(surfA);
	R.surface_destroy(surfB);
}


/*** B6 matrices ***/

/* the primitive name on a matrix m0 (args), its bytes against op on a copy */
static void
matrixCase(const char *name, int fn, int nargs, const sqInt *args, const cp_matrix_t *expected, int byAddress)
{
	sqInt rcvr;
	cp_matrix_t cm = m0;
	FakeAnswer a;

	rcvr = newMatrix(&m0);
	if (byAddress) {
		fake_set_slot(rcvr, 0, ea(&cm));
	}
	mark();
	a = fake_call(prim(name), rcvr, nargs, args);
	CHECK(a.returned && a.code == 0 && onlyCall(fn), "%s: code %lld", name, (long long)a.code);
	CHECK(memcmp(byAddress ? &cm : matrixOf(rcvr), expected, sizeof *expected) == 0, "%s%s: not cairo's matrix", name,
		byAddress ? " (ExternalAddress)" : "");
}

static void
testMatrices(void)
{
	static const cp_matrix_t other = { 0.5, -1, 2, 0.25, 3, -7 };
	cp_matrix_t m, o;
	sqInt args[6];
	int byAddress, i;

	for (byAddress = 0; byAddress < 2; byAddress++) {
		for (i = 0; i < 6; i++) {
			args[i] = i & 1 ? fake_int(i * 2 - 3) : fake_float(i * 0.75 + 0.1);
		}
		R.matrix_init(&m, 0.75 * 2 + 0.1, 5 * 2 - 3, 0.75 * 4 + 0.1, 3 * 2 - 3, 0.1, 1 * 2 - 3);	/* sx shy shx sy x y */
		matrixCase("primitiveMatrixInit", CP_FN_matrix_init, 6, args, &m, byAddress);
		R.matrix_init_identity(&m);
		matrixCase("primitiveMatrixInitIdentity", CP_FN_matrix_init_identity, 0, args, &m, byAddress);
		args[0] = fake_float(2.5); args[1] = fake_int(-3);
		m = m0; R.matrix_translate(&m, 2.5, -3);
		matrixCase("primitiveMatrixTranslate", CP_FN_matrix_translate, 2, args, &m, byAddress);
		m = m0; R.matrix_scale(&m, 2.5, -3);
		matrixCase("primitiveMatrixScale", CP_FN_matrix_scale, 2, args, &m, byAddress);
		R.matrix_init_translate(&m, 2.5, -3);
		matrixCase("primitiveMatrixInitTranslate", CP_FN_matrix_init_translate, 2, args, &m, byAddress);
		R.matrix_init_scale(&m, 2.5, -3);
		matrixCase("primitiveMatrixInitScale", CP_FN_matrix_init_scale, 2, args, &m, byAddress);
		args[0] = fake_float(0.7);
		m = m0; R.matrix_rotate(&m, 0.7);
		matrixCase("primitiveMatrixRotate", CP_FN_matrix_rotate, 1, args, &m, byAddress);
		R.matrix_init_rotate(&m, 0.7);
		matrixCase("primitiveMatrixInitRotate", CP_FN_matrix_init_rotate, 1, args, &m, byAddress);
		args[0] = newMatrix(&other);
		m = m0; R.matrix_multiply(&m, &other, &m);
		matrixCase("primitiveMatrixMultiply", CP_FN_matrix_multiply, 1, args, &m, byAddress);
		o = other;
		args[0] = newMatrix(&identity);
		fake_set_slot(args[0], 0, ea(&o));
		matrixCase("primitiveMatrixMultiply", CP_FN_matrix_multiply, 1, args, &m, byAddress);
		m = m0; R.matrix_invert(&m);
		matrixCase("primitiveMatrixInvert", CP_FN_matrix_invert, 0, args, &m, byAddress);
	}
	/* the receiver multiplied by itself */
	{
		sqInt rcvr = newMatrix(&m0);
		m = m0;
		R.matrix_multiply(&m, &m, &m);
		args[0] = rcvr;
		CHECK(fake_call(prim("primitiveMatrixMultiply"), rcvr, 1, args).returned
			&& memcmp(matrixOf(rcvr), &m, sizeof m) == 0, "primMultiplyBy: self");
	}
	/* invert: its status */
	{
		static const cp_matrix_t singular = { 0, 0, 0, 0, 1, 1 };
		sqInt rcvr = newMatrix(&singular);
		FakeAnswer a;
		m = singular;
		i = R.matrix_invert(&m);
		a = fake_call(prim("primitiveMatrixInvert"), rcvr, 0, NULL);
		CHECK(a.returned && a.value == fake_int(i) && i != 0, "invert of a singular matrix: %lld, cairo %d",
			(long long)fake_int_value(a.value), i);
		rcvr = newMatrix(&m0);
		a = fake_call(prim("primitiveMatrixInvert"), rcvr, 0, NULL);
		CHECK(a.returned && a.value == fake_int(0), "invert: status 0");
	}
	/* transform a point in place */
	{
		double x = 3.25, y = -4, rx = 3.25, ry = -4;
		sqInt rcvr = newMatrix(&m0);
		FakeAnswer a;
		args[0] = doubles(&x, 1);
		args[1] = fake_bytes(fakeByteArray, &y, 8);
		R.matrix_transform_point(&m0, &rx, &ry);
		mark();
		a = fake_call(prim("primitiveMatrixTransformPoint"), rcvr, 2, args);
		CHECK(a.returned && a.value == rcvr && onlyCall(CP_FN_matrix_transform_point), "transformPoint");
		CHECK(memcmp(fake_body(args[0]), &rx, 8) == 0 && memcmp(fake_body(args[1]), &ry, 8) == 0, "transformPoint: x, y");
		CHECK(memcmp(matrixOf(rcvr), &m0, sizeof m0) == 0, "transformPoint: the matrix changed");
		/* 16-byte buffers: only the first double */
		{
			double xx[2] = { 1, 99 }, yy[2] = { 2, 98 };
			rx = 1; ry = 2;
			R.matrix_transform_point(&m0, &rx, &ry);
			args[0] = doubles(xx, 2);
			args[1] = doubles(yy, 2);
			a = fake_call(prim("primitiveMatrixTransformPoint"), rcvr, 2, args);
			CHECK(a.returned && ((double *)fake_body(args[0]))[0] == rx && ((double *)fake_body(args[0]))[1] == 99
				&& ((double *)fake_body(args[1]))[0] == ry && ((double *)fake_body(args[1]))[1] == 98, "transformPoint: 16 bytes");
		}
	}
}


/*** B7 text ***/

static void
testText(void)
{
	sqInt font = newFontObj(), args[8], o;
	FakeAnswer a;
	cp_glyph_t *ref = NULL, *mine;
	int nref = 5, st, n;
	cp_text_extents_t te, mte;
	cp_font_extents_t fe;

	/* lock and unlock */
	mark();
	a = fake_call(prim("primitiveScaledFontLockFace"), font, 0, NULL);
	CHECK(a.returned && a.value == font && onlyCall(CP_FN_ft_scaled_font_lock_face), "lock");
	mark();
	a = fake_call(prim("primitiveScaledFontUnlockFace"), font, 0, NULL);
	CHECK(a.returned && a.value == font && onlyCall(CP_FN_ft_scaled_font_unlock_face), "unlock");
	/* text to glyphs */
	st = R.scaled_font_text_to_glyphs(fontFT, 1.5, 2, "Hello world", 5, &ref, &nref, NULL, NULL, NULL);
	CHECK(st == 0 && nref == 5 && ref, "the reference text_to_glyphs: %d, %d glyphs", st, nref);
	n = 5;
	args[0] = fake_bytes(fakeByteArray, "Hello world", 11); args[1] = fake_int(5); args[2] = font; args[3] = ea(NULL);
	args[4] = fake_bytes(fakeByteArray, &n, 4); args[5] = fake_float(1.5); args[6] = fake_int(2);
	mark();
	a = fake_call(prim("primitiveTextToGlyphs"), fake_new(fakeCairoFreetypeFontRenderer, 0), 7, args);
	CHECK(a.returned && a.value == fake_int(st) && onlyCall(CP_FN_scaled_font_text_to_glyphs), "convertString: code %lld",
		(long long)a.code);
	mine = (cp_glyph_t *)fake_address_value(args[3]);
	memcpy(&n, fake_body(args[4]), 4);
	CHECK(mine && n == nref && memcmp(mine, ref, (size_t)n * sizeof *ref) == 0, "convertString: the glyphs (%d)", n);
	/* glyph extents of those glyphs: through an ExternalAddress, into bytes and through an ExternalAddress */
	R.scaled_font_glyph_extents(fontFT, ref, nref, &te);
	o = newExtents(fakeCairoTextExtents, 48);
	args[0] = args[3]; args[1] = fake_int(n); args[2] = o;
	mark();
	a = fake_call(prim("primitiveScaledFontGlyphExtents"), font, 3, args);
	CHECK(a.returned && a.value == font && onlyCall(CP_FN_scaled_font_glyph_extents), "glyph extents");
	CHECK(memcmp(fake_body(fake_slot(o, 0)), &te, sizeof te) == 0 && te.x_advance > 0, "glyph extents: the values");
	memset(&mte, 0, sizeof mte);
	fake_set_slot(o, 0, ea(&mte));
	args[0] = fake_bytes(fakeByteArray, ref, (size_t)nref * sizeof *ref);
	a = fake_call(prim("primitiveScaledFontGlyphExtents"), font, 3, args);
	CHECK(a.returned && memcmp(&mte, &te, sizeof te) == 0, "glyph extents: bytes glyphs into an ExternalAddress");
	/* the glyphs freed (primitiveGlyphFree), the address not nulled */
	o = args[0];
	args[0] = ea(mine);
	mark();
	a = fake_call(prim("primitiveGlyphFree"), fakeNil, 1, args);
	CHECK(a.returned && onlyCall(CP_FN_glyph_free) && fake_address_value(args[0]) == mine, "freeGlyphs:");
	args[0] = ea(NULL);
	a = fake_call(prim("primitiveGlyphFree"), fakeNil, 1, args);
	CHECK(a.returned, "freeGlyphs: NULL");
	R.glyph_free(ref);
	/* the in/out pointer: cairo writes into the caller's buffer when it is big enough */
	{
		cp_glyph_t *buffer = R.glyph_allocate(16), *before;
		n = 16;
		args[0] = fake_string("abc"); args[1] = fake_int(3); args[2] = font; args[3] = ea(buffer);
		args[4] = fake_bytes(fakeByteArray, &n, 4); args[5] = fake_int(0); args[6] = fake_int(0);
		before = buffer;
		a = fake_call(prim("primitiveTextToGlyphs"), fakeNil, 7, args);
		memcpy(&n, fake_body(args[4]), 4);
		CHECK(a.returned && a.value == fake_int(0) && n == 3 && fake_address_value(args[3]) == before,
			"convertString: into a given buffer (%d)", n);
		R.glyph_free(buffer);
		/* len 0 of an empty ByteArray */
		n = 0;
		args[0] = fake_new(fakeByteArray, 0); args[1] = fake_int(0); args[3] = ea(NULL);
		args[4] = fake_bytes(fakeByteArray, &n, 4);
		a = fake_call(prim("primitiveTextToGlyphs"), fakeNil, 7, args);
		memcpy(&n, fake_body(args[4]), 4);
		CHECK(a.returned && a.value == fake_int(0) && n == 0, "convertString: empty");
		R.glyph_free((cp_glyph_t *)fake_address_value(args[3]));
	}
	/* text extents */
	R.scaled_font_text_extents(fontFT, "Hello", &te);
	o = newExtents(fakeCairoTextExtents, 48);
	args[0] = fake_string("Hello"); args[1] = o;
	mark();
	a = fake_call(prim("primitiveScaledFontTextExtents"), font, 2, args);
	CHECK(a.returned && a.value == font && onlyCall(CP_FN_scaled_font_text_extents)
		&& memcmp(fake_body(fake_slot(o, 0)), &te, sizeof te) == 0, "text extents");
	R.scaled_font_text_extents(fontFT, "He", &te);
	args[0] = fake_bytes(fakeByteArray, "He\0llo", 6);
	fake_set_slot(o, 0, ea(&mte));
	a = fake_call(prim("primitiveScaledFontTextExtents"), font, 2, args);
	CHECK(a.returned && memcmp(&mte, &te, sizeof te) == 0, "text extents: NUL inside, an ExternalAddress handle");
	/* font extents: 40 bytes written into 48 */
	R.scaled_font_extents(fontFT, &fe);
	o = newExtents(fakeCairoFontExtents, 48);
	memset(fake_body(fake_slot(o, 0)), 0xAB, 48);
	args[0] = o;
	mark();
	a = fake_call(prim("primitiveScaledFontExtents"), font, 1, args);
	CHECK(a.returned && a.value == fake_int(0) && onlyCall(CP_FN_scaled_font_extents)
		&& memcmp(fake_body(fake_slot(o, 0)), &fe, sizeof fe) == 0 && fe.ascent > 0
		&& ((unsigned char *)fake_body(fake_slot(o, 0)))[40] == 0xAB && ((unsigned char *)fake_body(fake_slot(o, 0)))[47] == 0xAB,
		"font extents");
	o = newExtents(fakeCairoFontExtents, 40);
	args[0] = o;
	a = fake_call(prim("primitiveScaledFontExtents"), font, 1, args);
	CHECK(a.returned && memcmp(fake_body(fake_slot(o, 0)), &fe, sizeof fe) == 0, "font extents: 40 bytes");
	{
		cp_font_extents_t mfe;
		fake_set_slot(o, 0, ea(&mfe));
		a = fake_call(prim("primitiveScaledFontExtents"), font, 1, args);
		CHECK(a.returned && memcmp(&mfe, &fe, sizeof fe) == 0, "font extents: an ExternalAddress handle");
	}
	/* invalid UTF-8: cairo's error status, answered (last: it may leave the font in error) */
	n = 2;
	args[0] = fake_bytes(fakeByteArray, "\xff\xfe", 2); args[1] = fake_int(2); args[2] = font; args[3] = ea(NULL);
	args[4] = fake_bytes(fakeByteArray, &n, 4); args[5] = fake_int(0); args[6] = fake_int(0);
	a = fake_call(prim("primitiveTextToGlyphs"), fakeNil, 7, args);
	CHECK(a.returned && fake_is_int(a.value) && a.value != fake_int(0), "convertString: invalid UTF-8 status");
	R.glyph_free((cp_glyph_t *)fake_address_value(args[3]));
}


/*** B8 memory ***/

static void
testMemory(void)
{
	static const char *names[] = { "GetCurrentPoint", "GetScaledFont", "BuilderCopyPath" };
	sqInt rcvr, args[8];
	FakeAnswer a;
	int k, n;

	for (k = 0; k < 3; k++) {
		const Case *c = caseNamed(names[k]);
		for (n = 0; n < 8; n++) {
			c->make(&rcvr, args);
			fake_set_budget(n, PrimErrNoMemory);
			mark();
			a = runCase(c, rcvr, args);
			fake_set_budget(-1, PrimErrNoMemory);
			if (a.returned) {
				if (c->after) {
					c->after(rcvr, args);
				}
				freeTracked();
				break;
			}
			CHECK(a.code == PrimErrNoMemory && a.protocolErrors == 0, "%s with %d allocations: code %lld %s",
				c->name, n, (long long)a.code, fake_last_error());
			/* only the pure getter may call cairo before its answer's allocation */
			CHECK(callsSince() == (uint64_t)(k == 0) && primsSince() == 0, "%s with %d allocations: %llu cairo calls",
				c->name, n, (unsigned long long)callsSince());
			freeTracked();
		}
		CHECK(n > 0 && n < 8, "%s: %d allocations", c->name, n);
	}
}


/*** B9 optional functions ***/

static void
testOptional(void)
{
	cp_cairo_api saved = cp_api;
	sqInt font = newFontObj();
	FakeAnswer a;

	cp_api.ft_scaled_font_lock_face = NULL;
	cp_api.ft_scaled_font_unlock_face = NULL;
	mark();
	a = fake_call(prim("primitiveScaledFontLockFace"), font, 0, NULL);
	CHECK(a.code == PrimErrUnsupported && callsSince() == 0, "lock without cairo_ft: %lld", (long long)a.code);
	a = fake_call(prim("primitiveScaledFontUnlockFace"), font, 0, NULL);
	CHECK(a.code == PrimErrUnsupported && callsSince() == 0, "unlock without cairo_ft: %lld", (long long)a.code);
	cp_api = saved;
	/* unbound: unsupported, nothing called */
	{
		sqInt rcvr, args[8];
		const Case *c = caseNamed("NewPath");
		fake_call(prim("primitiveUnbind"), fakeNil, 0, NULL);
		c->make(&rcvr, args);
		expectFail(c, "unbound", PrimErrUnsupported, rcvr, args);
		freeTracked();
		bind();
	}
}


/*** main ***/

static void
setUpWorld(void)
{
	void *cr;
	const char *why;

	surfS = R.image_surface_create(0, SIZE, SIZE);
	pattern = R.pattern_create_linear(0, 0, 64, 64);
	R.pattern_add_color_stop_rgba(pattern, 0, 1, 0, 0, 1);
	R.pattern_add_color_stop_rgba(pattern, 1, 0, 0, 1, 0.5);
	haveFT = 0;
	why = "no FreeType functions";
	if (R.FT_Init_FreeType && R.FT_New_Face) {
		void *library = NULL, *face = NULL;
		why = "FT_Init_FreeType or FT_New_Face failed";
		if (R.FT_Init_FreeType(&library) == 0 && R.FT_New_Face(library, fontPath, 0, &face) == 0) {
			why = "not an FT scaled font";
			void *fontFace = R.ft_font_face_create_for_ft_face(face, 0);
			cp_matrix_t fm, ctm;
			R.matrix_init_scale(&fm, 12, 12);
			R.matrix_init_identity(&ctm);
			void *options = R.font_options_create();
			fontFT = R.scaled_font_create(fontFace, &fm, &ctm, options);
			R.font_options_destroy(options);
			haveFT = R.scaled_font_get_type(fontFT) == 1;	/* CAIRO_FONT_TYPE_FT */
		}
	}
	if (!haveFT) {
		cr = R.create(surfS);
		R.set_font_size(cr, 12);
		fontFT = R.get_scaled_font(cr);
		R.destroy(cr);
	}
	if (haveFT) {
		printf("-- glue-basic: the font %s\n", fontPath);
	} else {
		printf("-- glue-basic: the toy font (%s, %s)\n", why, fontPath);
	}
}

int
main(int argc, char **argv)
{
	struct VirtualMachine *vm;
	int i;

	setvbuf(stdout, NULL, _IOLBF, 0);
	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--libcairo") == 0 && i + 1 < argc) {
			libcairo = argv[++i];
		} else if (strcmp(argv[i], "--font") == 0 && i + 1 < argc) {
			fontPath = argv[++i];
		}
	}
	vm = fake_init();
#ifndef CP_STATIC_CAIRO
	cairoHandle = dlopen(libcairo, RTLD_NOW | RTLD_LOCAL);
	if (!cairoHandle) {
		printf("FAILED: dlopen %s: %s\n", libcairo, dlerror());
		return 1;
	}
#endif
	if (!loadReference()) {
		return 1;
	}
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
	if (!bind()) {
		printf("glue-basic FAILED: no bind\n");
		return 1;
	}
	setUpWorld();
	testSuccess();
	testReceivers();
	testArguments();
	testForwarders();
	testEffects();
	testMatrices();
	testText();
	testMemory();
	testOptional();
	shutdownModule();
	fake_free_all();
	if (failures) {
		printf("glue-basic FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("glue-basic ok: %d checks\n", checks);
	return 0;
}
