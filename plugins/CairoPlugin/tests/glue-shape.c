/* glue-shape.c -- the shape-level primitives of the CairoPlugin
 * (src/prims/shape.c: primitiveFillRectangleColor, primitiveStrokeRectangle,
 * primitiveFillPathColor, primitiveStrokePath) against the fake VM of
 * fakevm.c and a real cairo.
 *
 * Each case draws twice on two image surfaces prepared alike: once through
 * the primitive (Athens-shaped fake objects whose handles are the real
 * cairo_t, matrix and path), once through the stock call sequence of
 * AthensCairoCanvas>>draw made directly on cairo (the test's own function
 * table: dlsym natively, cairo itself under CP_STATIC_CAIRO); then the
 * pixels must be identical (memcmp), the cairo state identical (matrix,
 * source rgba, line width, join, cap, dashes and offset, current point,
 * fill rule, operator, font matrix, status: bit-exact), the answer the
 * stock one (the surface for fills, the paint for strokes) and the cairo
 * calls, counted by the plugin, those of the stock sequence, function by
 * function.
 *
 *   S1 fill rect     Integer, Float (immediate and boxed), mixed and
 *                    negative coordinates; the matrix as bytes and as an
 *                    ExternalAddress; transforms; alpha
 *   S2 stroke rect   dashes nil, empty, short, long (the malloc path),
 *                    Integer and Float lengths and offsets; joins, caps,
 *                    widths; the fillPaint class
 *   S3 fill path     a path of lines, curves, arcs and close
 *   S4 stroke path   the same path, dashed and not
 *   S5 failures      every invalid input of the contract (receiver chain:
 *                    handle nil/NULL/bytes, matrix of 47 bytes or NULL,
 *                    currentCanvas not the receiver; arguments: a Fraction,
 *                    nil, a bad Color, a fillPaint of another class, a bad
 *                    width/join/cap, dashes not an Array, a bad element, too
 *                    many, a bad offset, a NULL path): the code, no cairo
 *                    call, nothing drawn, nothing allocated
 *   S6 forwarders    a forwarder at every level of every chain (receiver,
 *                    its handle, pathTransform and its handle, surface and
 *                    its currentCanvas; rectangle, origin, a boxed x; the
 *                    paint, fillPaint, width, dashes and a boxed length;
 *                    path and its handle; the class): the first call fails
 *                    before any cairo call, the VM's retry (to the
 *                    primitive's accessor depth) draws the stock pixels
 *
 * Usage: glue-shape [--libcairo PATH] (run by tests/run-c-tests.sh, lanes
 * glue-shape and glue-wasm64).  Prints one line per failed check and
 * "glue-shape ok: N checks"; exits 1 when a check failed.
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

static int checks, failures;

#define CHECK(cond, ...) do { \
		checks++; \
		if (!(cond)) { \
			failures++; \
			if (failures <= 80) { \
				printf("FAILED %s:%d: ", __FILE__, __LINE__); \
				printf(__VA_ARGS__); \
				printf("\n"); \
			} \
		} \
	} while (0)

typedef sqInt (*Primitive)(void);

static const char *libcairo = "libcairo.so.2";


/*** The test's own cairo (never through the plugin's table, so never
     counted): dlsym of the same library natively, cairo itself under
     CP_STATIC_CAIRO ***/

#ifdef CP_STATIC_CAIRO

# define TC(name) cairo_##name

#else

/* X(ret, name, params) */
# define TC_FUNCTIONS(X) \
	X(cp_surface_t *, image_surface_create, (int format, int width, int height)) \
	X(unsigned char *, image_surface_get_data, (cp_surface_t *s)) \
	X(int, image_surface_get_stride, (cp_surface_t *s)) \
	X(void, surface_destroy, (cp_surface_t *s)) \
	X(void, surface_flush, (cp_surface_t *s)) \
	X(cp_cairo_t *, create, (cp_surface_t *s)) \
	X(void, destroy, (cp_cairo_t *cr)) \
	X(int, status, (cp_cairo_t *cr)) \
	X(void, get_matrix, (cp_cairo_t *cr, cp_matrix_t *m)) \
	X(cp_pattern_t *, get_source, (cp_cairo_t *cr)) \
	X(int, pattern_get_rgba, (cp_pattern_t *p, double *r, double *g, double *b, double *a)) \
	X(double, get_line_width, (cp_cairo_t *cr)) \
	X(int, get_line_join, (cp_cairo_t *cr)) \
	X(int, get_line_cap, (cp_cairo_t *cr)) \
	X(int, get_dash_count, (cp_cairo_t *cr)) \
	X(void, get_dash, (cp_cairo_t *cr, double *dashes, double *offset)) \
	X(int, has_current_point, (cp_cairo_t *cr)) \
	X(int, get_fill_rule, (cp_cairo_t *cr)) \
	X(int, get_operator, (cp_cairo_t *cr)) \
	X(void, get_font_matrix, (cp_cairo_t *cr, cp_matrix_t *m)) \
	X(void, set_matrix, (cp_cairo_t *cr, const cp_matrix_t *m)) \
	X(void, set_source_rgba, (cp_cairo_t *cr, double r, double g, double b, double a)) \
	X(void, set_dash, (cp_cairo_t *cr, const double *dashes, int n, double offset)) \
	X(void, set_line_width, (cp_cairo_t *cr, double w)) \
	X(void, set_line_join, (cp_cairo_t *cr, int join)) \
	X(void, set_line_cap, (cp_cairo_t *cr, int cap)) \
	X(void, set_fill_rule, (cp_cairo_t *cr, int rule)) \
	X(void, new_path, (cp_cairo_t *cr)) \
	X(void, move_to, (cp_cairo_t *cr, double x, double y)) \
	X(void, line_to, (cp_cairo_t *cr, double x, double y)) \
	X(void, curve_to, (cp_cairo_t *cr, double x1, double y1, double x2, double y2, double x3, double y3)) \
	X(void, arc, (cp_cairo_t *cr, double xc, double yc, double r, double a1, double a2)) \
	X(void, close_path, (cp_cairo_t *cr)) \
	X(void, rectangle, (cp_cairo_t *cr, double x, double y, double w, double h)) \
	X(cp_path_t *, copy_path, (cp_cairo_t *cr)) \
	X(void, append_path, (cp_cairo_t *cr, const cp_path_t *p)) \
	X(void, path_destroy, (cp_path_t *p)) \
	X(void, fill, (cp_cairo_t *cr)) \
	X(void, stroke, (cp_cairo_t *cr)) \
	X(void, paint, (cp_cairo_t *cr)) \
	X(void, translate, (cp_cairo_t *cr, double x, double y)) \
	X(void, scale, (cp_cairo_t *cr, double x, double y)) \
	X(void, rotate, (cp_cairo_t *cr, double a))

# define TC_FIELD(ret, name, params) ret (*name) params;
static struct { TC_FUNCTIONS(TC_FIELD) } tc;
# undef TC_FIELD
# define TC(name) tc.name

static void *cairoHandle;

static int
loadTestCairo(void)
{
	if (!(cairoHandle = dlopen(libcairo, RTLD_NOW | RTLD_LOCAL))) {
		printf("FAILED: dlopen %s: %s\n", libcairo, dlerror());
		return 0;
	}
# define TC_LOAD(ret, name, params) \
	if (!(*(void **)&tc.name = dlsym(cairoHandle, "cairo_" #name))) { \
		printf("FAILED: no cairo_%s in %s\n", #name, libcairo); \
		return 0; \
	}
	TC_FUNCTIONS(TC_LOAD)
# undef TC_LOAD
	return 1;
}

#endif /* CP_STATIC_CAIRO */

#define W 40
#define H 32


/*** The plugin ***/

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

/* the layout of Pharo 12-15 (docs/CairoPlugin.md); SLOT(CP_L_x) is the
   0-based slot index of the position CP_L_x */
static const sqInt layoutValues[CP_LAYOUT_INTEGERS] = {
	1, 6, 7, 0, 1, 1, 0, 0, 0, 0, 1, 3, 4, 5, 6, 0, 2, 6, 0, 3, 0, 1, 0, 1, 20, 10, 1023, 255, 0
};
#define SLOT(L) ((size_t)layoutValues[L])

static int
bind(void)
{
	sqInt oops[CP_L_COUNT], args[3];
	FakeAnswer a;
	void *create;
	int i;

	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		oops[i] = fake_int(layoutValues[i]);
	}
	oops[CP_L_CLASS_BYTE_STRING] = fakeByteString;
	oops[CP_L_CLASS_BYTE_SYMBOL] = fakeByteSymbol;
	oops[CP_L_CLASS_WIDE_STRING] = fakeWideString;
	oops[CP_L_CLASS_WIDE_SYMBOL] = fakeWideSymbol;
#ifdef CP_STATIC_CAIRO
	create = (void *)cairo_create;
#else
	create = dlsym(cairoHandle, "cairo_create");
#endif
	args[0] = fake_string(libcairo);
	args[1] = fake_external_address(create);
	args[2] = fake_array(fakeArray, oops, CP_LAYOUT_SIZE_TEXT);
	a = fake_call(prim("primitiveBind"), fakeNil, 3, args);
	CHECK(a.returned && a.code == 0 && a.protocolErrors == 0, "bind: code %lld %s", (long long)a.code, fake_last_error());
	return a.returned && a.code == 0;
}

static void
snapCounters(uint64_t calls[CP_FN_COUNT], uint64_t prims[CP_P_COUNT])
{
	memcpy(calls, cp_cairo_calls, sizeof cp_cairo_calls);
	memcpy(prims, cp_prim_calls, sizeof cp_prim_calls);
}

/* The cairo calls since calls0 equal expected (CP_FN_COUNT counts), and the
   primitive's success counter moved by primDelta, every other one by 0 */
static void
checkCounters(const char *what, const uint64_t calls0[CP_FN_COUNT], const uint64_t prims0[CP_P_COUNT],
	const uint64_t expected[CP_FN_COUNT], int primIndex, uint64_t primDelta)
{
	int i, bad = 0;

	for (i = 0; i < CP_FN_COUNT; i++) {
		if (cp_cairo_calls[i] - calls0[i] != expected[i]) {
			CHECK(0, "%s: %s called %llu times, expected %llu", what, cp_cairo_function_names[i],
				(unsigned long long)(cp_cairo_calls[i] - calls0[i]), (unsigned long long)expected[i]);
			bad = 1;
		}
	}
	for (i = 0; i < CP_P_COUNT; i++) {
		uint64_t want = i == primIndex ? primDelta : 0;
		if (cp_prim_calls[i] - prims0[i] != want) {
			CHECK(0, "%s: %s succeeded %llu times, expected %llu", what, cp_primitive_names[i],
				(unsigned long long)(cp_prim_calls[i] - prims0[i]), (unsigned long long)want);
			bad = 1;
		}
	}
	CHECK(!bad, "%s: counters", what);
}


/*** Surfaces, state ***/

typedef struct {
	cp_surface_t *surface;
	cp_cairo_t *cr;
} Ctx;

/* An ARGB32 surface prepared in one of a few states that the drawing
   must change or keep: a background, a transform, line attributes, a dash,
   a current path, a fill rule */
static Ctx
newCtx(int pre)
{
	Ctx c;
	static const double dash[2] = { 3.0, 1.5 };

	c.surface = TC(image_surface_create)(0, W, H);	/* CAIRO_FORMAT_ARGB32 */
	c.cr = TC(create)(c.surface);
	TC(set_source_rgba)(c.cr, 0.9, 0.95, 1.0, 1.0);
	TC(paint)(c.cr);
	switch (pre % 4) {
	case 0:
		break;
	case 1:
		TC(translate)(c.cr, 3.0, -2.0);
		TC(set_line_width)(c.cr, 5.0);
		TC(set_dash)(c.cr, dash, 2, 0.5);
		TC(move_to)(c.cr, 1.0, 1.0);
		TC(line_to)(c.cr, 30.0, 20.0);
		break;
	case 2:
		TC(scale)(c.cr, 1.5, 0.75);
		TC(set_line_join)(c.cr, 2);
		TC(set_line_cap)(c.cr, 1);
		TC(set_fill_rule)(c.cr, 1);
		TC(rectangle)(c.cr, 2.0, 2.0, 5.0, 5.0);
		break;
	case 3:
		TC(rotate)(c.cr, 0.3);
		TC(set_dash)(c.cr, dash, 1, 0.0);
		break;
	}
	return c;
}

static void
freeCtx(Ctx *c)
{
	TC(destroy)(c->cr);
	TC(surface_destroy)(c->surface);
}

typedef struct {
	int status;
	double matrix[6];
	int sourceStatus;
	double rgba[4];
	double lineWidth;
	int join, cap;
	int dashCount;
	double dashes[128];
	double dashOffset;
	int hasPoint;
	int fillRule, op;
	double fontMatrix[6];
} State;

static void
stateOf(cp_cairo_t *cr, State *s)
{
	cp_matrix_t m;

	memset(s, 0, sizeof *s);
	s->status = TC(status)(cr);
	TC(get_matrix)(cr, &m);
	memcpy(s->matrix, &m, sizeof m);
	s->sourceStatus = TC(pattern_get_rgba)(TC(get_source)(cr), &s->rgba[0], &s->rgba[1], &s->rgba[2], &s->rgba[3]);
	s->lineWidth = TC(get_line_width)(cr);
	s->join = TC(get_line_join)(cr);
	s->cap = TC(get_line_cap)(cr);
	s->dashCount = TC(get_dash_count)(cr);
	if (s->dashCount <= 128) {
		TC(get_dash)(cr, s->dashes, &s->dashOffset);
	}
	s->hasPoint = TC(has_current_point)(cr);
	s->fillRule = TC(get_fill_rule)(cr);
	s->op = TC(get_operator)(cr);
	TC(get_font_matrix)(cr, &m);
	memcpy(s->fontMatrix, &m, sizeof m);
}

static int
samePixels(Ctx *a, Ctx *b)
{
	TC(surface_flush)(a->surface);
	TC(surface_flush)(b->surface);
	return memcmp(TC(image_surface_get_data)(a->surface), TC(image_surface_get_data)(b->surface),
		(size_t)TC(image_surface_get_stride)(a->surface) * H) == 0;
}

/* a (the primitive's) and b (the stock sequence's) have the same pixels and
   the same state, and a differs from a fresh surface of the same
   preparation (something was drawn) when drawn is 1, equals it when 0
   (-1: either) */
static void
compareCtx(const char *what, Ctx *a, Ctx *b, int pre, int drawn)
{
	State sa, sb;
	Ctx fresh;

	CHECK(samePixels(a, b), "%s: pixels differ from the stock sequence's", what);
	stateOf(a->cr, &sa);
	stateOf(b->cr, &sb);
	CHECK(sa.status == 0, "%s: cairo status %d", what, sa.status);
	CHECK(memcmp(&sa, &sb, sizeof sa) == 0, "%s: state differs (matrix %d, rgba %d, width %d, join %d, cap %d, dash %d/%d, point %d, rule %d, op %d)",
		what, memcmp(sa.matrix, sb.matrix, sizeof sa.matrix) != 0, memcmp(sa.rgba, sb.rgba, sizeof sa.rgba) != 0,
		sa.lineWidth != sb.lineWidth, sa.join != sb.join, sa.cap != sb.cap, sa.dashCount, sb.dashCount,
		sa.hasPoint != sb.hasPoint, sa.fillRule != sb.fillRule, sa.op != sb.op);
	CHECK(!sa.hasPoint, "%s: a current point is left", what);
	if (drawn >= 0) {
		fresh = newCtx(pre);
		CHECK(samePixels(a, &fresh) == !drawn, "%s: %s", what, drawn ? "nothing was drawn" : "something was drawn");
		freeCtx(&fresh);
	}
}


/*** Athens-shaped objects ***/

/* An AthensCairoMatrix whose handle is a 48-byte ByteArray, or an
   ExternalAddress of a malloc'd copy (freed at the end) */
static void *matrixCopies[256];
static int nMatrixCopies;

static sqInt
newMatrix(const cp_matrix_t *m, int asAddress)
{
	sqInt handle, obj;

	if (asAddress && nMatrixCopies < 256) {
		cp_matrix_t *copy = malloc(sizeof *copy);
		*copy = *m;
		matrixCopies[nMatrixCopies++] = copy;
		handle = fake_external_address(copy);
	} else {
		handle = fake_bytes(fakeByteArray, m, sizeof *m);
	}
	obj = fake_new(fakeAthensCairoMatrix, 0);
	fake_set_slot(obj, SLOT(CP_L_EXTOBJ_HANDLE), handle);
	return obj;
}

/* An AthensCairoCanvas of cr with that pathTransform, its surface's
   currentCanvas the canvas */
static sqInt
newCanvas(Ctx *c, sqInt matrix)
{
	sqInt canvas = fake_new(fakeAthensCairoCanvas, 0);
	sqInt surface = fake_new(fakeAthensCairoSurface, 0);

	fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_external_address(c->cr));
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), matrix);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_SURFACE), surface);
	fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), canvas);
	fake_set_slot(surface, SLOT(CP_L_SURFACE_HANDLE), fake_external_address(c->surface));
	return canvas;
}

static sqInt
newColor(sqInt r, sqInt g, sqInt b, sqInt alpha)
{
	return fake_color(fake_int((r << 20) | (g << 10) | b), fake_int(alpha));
}

/* Color>>red ... alpha */
static void
colorComponents(sqInt color, double rgba[4])
{
	sqInt rgb = fake_int_value(fake_slot(color, 0));

	rgba[0] = (double)((rgb >> 20) & 1023) / 1023.0;
	rgba[1] = (double)((rgb >> 10) & 1023) / 1023.0;
	rgba[2] = (double)(rgb & 1023) / 1023.0;
	rgba[3] = (double)fake_int_value(fake_slot(color, 3)) / 255.0;
}

static double
asFloat(sqInt oop)
{
	return fake_is_int(oop) ? (double)fake_int_value(oop) : fake_float_value(oop);
}

/* left top width height asFloat of a Rectangle of Points */
static void
rectComponents(sqInt rect, double xywh[4])
{
	sqInt o = fake_follow(fake_slot(rect, 0)), c = fake_follow(fake_slot(rect, 1));
	sqInt ox = fake_follow(fake_slot(o, 0)), oy = fake_follow(fake_slot(o, 1));
	sqInt cx = fake_follow(fake_slot(c, 0)), cy = fake_follow(fake_slot(c, 1));

	xywh[0] = asFloat(ox);
	xywh[1] = asFloat(oy);
	xywh[2] = fake_is_int(ox) && fake_is_int(cx) ? (double)(fake_int_value(cx) - fake_int_value(ox)) : asFloat(cx) - asFloat(ox);
	xywh[3] = fake_is_int(oy) && fake_is_int(cy) ? (double)(fake_int_value(cy) - fake_int_value(oy)) : asFloat(cy) - asFloat(oy);
}

/* An AthensCairoStrokePaint: fillPaint width miterLimit capStyle joinStyle
   dashLenghts dashOffset */
static sqInt
newStroke(sqInt fill, sqInt width, sqInt cap, sqInt join, sqInt dashes, sqInt offset)
{
	sqInt p = fake_new(fakeAthensCairoStrokePaint, 0);

	fake_set_slot(p, SLOT(CP_L_STROKE_FILL_PAINT), fill);
	fake_set_slot(p, SLOT(CP_L_STROKE_WIDTH), width);
	fake_set_slot(p, 2, fake_int(10));
	fake_set_slot(p, SLOT(CP_L_STROKE_CAP), cap);
	fake_set_slot(p, SLOT(CP_L_STROKE_JOIN), join);
	fake_set_slot(p, SLOT(CP_L_STROKE_DASHES), dashes);
	fake_set_slot(p, SLOT(CP_L_STROKE_DASH_OFFSET), offset);
	return p;
}

static sqInt
newDashes(const double *lengths, int n, int asIntegers)
{
	sqInt *oops = malloc((size_t)(n ? n : 1) * sizeof(sqInt)), a;
	int i;

	for (i = 0; i < n; i++) {
		oops[i] = asIntegers ? fake_int((sqInt)lengths[i]) : fake_float(lengths[i]);
	}
	a = fake_array(fakeArray, oops, (size_t)n);
	free(oops);
	return a;
}

static sqInt
newPathObject(cp_path_t *path)
{
	sqInt p = fake_new(fakeAthensCairoPath, 0);

	fake_set_slot(p, SLOT(CP_L_EXTOBJ_HANDLE), fake_external_address(path));
	return p;
}


/*** The stock sequences (AthensCairoCanvas>>draw), made directly ***/

static void
stockFillHead(cp_cairo_t *cr, const cp_matrix_t *m, sqInt color)
{
	double rgba[4];

	colorComponents(color, rgba);
	TC(set_matrix)(cr, m);					/* setPathMatrix */
	TC(set_source_rgba)(cr, rgba[0], rgba[1], rgba[2], rgba[3]);	/* loadSolidColor: */
	TC(set_dash)(cr, NULL, 0, 0.0);				/* resetDash */
	TC(new_path)(cr);
}

static void
stockFillRect(cp_cairo_t *cr, const cp_matrix_t *m, sqInt rect, sqInt color)
{
	double xywh[4];

	rectComponents(rect, xywh);
	stockFillHead(cr, m, color);
	TC(rectangle)(cr, xywh[0], xywh[1], xywh[2], xywh[3]);
	TC(fill)(cr);
}

static void
stockFillPath(cp_cairo_t *cr, const cp_matrix_t *m, cp_path_t *path, sqInt color)
{
	stockFillHead(cr, m, color);
	TC(append_path)(cr, path);	/* loadPath: */
	TC(fill)(cr);
}

/* setPathMatrix, prepareForDrawingOn:, newPath */
static void
stockStrokeHead(cp_cairo_t *cr, const cp_matrix_t *m, sqInt paint)
{
	double rgba[4], *buf;
	sqInt dashes = fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_DASHES)));
	size_t i, n;

	colorComponents(fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_FILL_PAINT))), rgba);
	TC(set_matrix)(cr, m);
	TC(set_source_rgba)(cr, rgba[0], rgba[1], rgba[2], rgba[3]);
	TC(set_line_width)(cr, asFloat(fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_WIDTH)))));
	TC(set_line_join)(cr, (int)fake_int_value(fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_JOIN)))));
	TC(set_line_cap)(cr, (int)fake_int_value(fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_CAP)))));
	if (dashes == fakeNil) {
		TC(set_dash)(cr, NULL, 0, 0.0);
	} else {
		/* setDashesOn: a ByteArray of 8 * size bytes */
		n = fake_size(dashes);
		buf = malloc(n ? n * sizeof(double) : 1);
		for (i = 0; i < n; i++) {
			buf[i] = asFloat(fake_follow(fake_slot(dashes, i)));
		}
		TC(set_dash)(cr, buf, (int)n, asFloat(fake_follow(fake_slot(paint, SLOT(CP_L_STROKE_DASH_OFFSET)))));
		free(buf);
	}
	TC(new_path)(cr);
}

static void
stockStrokeRect(cp_cairo_t *cr, const cp_matrix_t *m, sqInt rect, sqInt paint)
{
	double xywh[4];

	rectComponents(rect, xywh);
	stockStrokeHead(cr, m, paint);
	TC(rectangle)(cr, xywh[0], xywh[1], xywh[2], xywh[3]);
	TC(stroke)(cr);
}

static void
stockStrokePath(cp_cairo_t *cr, const cp_matrix_t *m, cp_path_t *path, sqInt paint)
{
	stockStrokeHead(cr, m, paint);
	TC(append_path)(cr, path);
	TC(stroke)(cr);
}


/*** The cases ***/

enum { FILL_RECT, STROKE_RECT, FILL_PATH, STROKE_PATH };
static const char *const kindNames[] = { "fill rect", "stroke rect", "fill path", "stroke path" };
static const char *const primNames[] = {
	"primitiveFillRectangleColor", "primitiveStrokeRectangle", "primitiveFillPathColor", "primitiveStrokePath"
};
static const int primIndices[] = { CP_P_FillRectangleColor, CP_P_StrokeRectangle, CP_P_FillPathColor, CP_P_StrokePath };

static cp_path_t *testPath;	/* lines, curves, arcs, close */
static int expectDrawn = 1;	/* drawCase: the drawing changes pixels */

/* The cairo calls of the stock sequence of kind */
static void
expectedCalls(int kind, uint64_t e[CP_FN_COUNT])
{
	memset(e, 0, CP_FN_COUNT * sizeof e[0]);
	e[CP_FN_set_matrix] = 1;
	e[CP_FN_set_source_rgba] = 1;
	e[CP_FN_set_dash] = 1;
	e[CP_FN_new_path] = 1;
	if (kind == STROKE_RECT || kind == STROKE_PATH) {
		e[CP_FN_set_line_width] = 1;
		e[CP_FN_set_line_join] = 1;
		e[CP_FN_set_line_cap] = 1;
		e[CP_FN_stroke] = 1;
	} else {
		e[CP_FN_fill] = 1;
	}
	if (kind == FILL_RECT || kind == STROKE_RECT) {
		e[CP_FN_rectangle] = 1;
	} else {
		e[CP_FN_append_path] = 1;
	}
}

/* One drawing: the primitive of kind on a canvas of a fresh surface (pre),
   with the shape and paint (a Color for fills, a stroke paint for
   strokes), with forwarder (0: none; else a function that puts forwarders
   into the objects before the call: then the first call must fail before
   any cairo call and the VM's retry draw); against the stock sequence */
typedef void (*Mangle)(sqInt canvas, sqInt *args);

static void
drawCase(const char *what, int kind, int pre, const cp_matrix_t *m, int matrixAsAddress,
	sqInt shape, sqInt paint, Mangle mangle)
{
	Ctx a = newCtx(pre), b = newCtx(pre);
	sqInt canvas = newCanvas(&a, newMatrix(m, matrixAsAddress));
	sqInt surface = fake_slot(canvas, SLOT(CP_L_CANVAS_SURFACE));
	sqInt args[3];
	int nargs = kind == STROKE_RECT || kind == STROKE_PATH ? 3 : 2;
	uint64_t calls0[CP_FN_COUNT], prims0[CP_P_COUNT], e[CP_FN_COUNT];
	FakeAnswer r;
	char label[200];

	snprintf(label, sizeof label, "%s (%s, pre %d)", what, kindNames[kind], pre);
	args[0] = shape;
	args[1] = paint;
	args[2] = fakeColor;
	if (mangle) {
		mangle(canvas, args);
	}
	snapCounters(calls0, prims0);
	if (mangle) {
		r = fake_call_retry(prim(primNames[kind]), fake_accessor_depth(primNames[kind]), canvas, nargs, args);
		CHECK(r.retried && r.firstCode != 0, "%s: the forwarder was not met (first code %lld)", label, (long long)r.firstCode);
	} else {
		r = fake_call(prim(primNames[kind]), canvas, nargs, args);
	}
	CHECK(r.returned && r.code == 0 && r.protocolErrors == 0, "%s: code %lld %s", label, (long long)r.code, fake_last_error());
	CHECK(r.allocations == 0 && r.stores == 0, "%s: %d allocations, %d stores", label, r.allocations, r.stores);
	if (kind == FILL_RECT || kind == FILL_PATH) {
		CHECK(r.value == surface, "%s: does not answer the surface", label);
	} else {
		CHECK(r.value == fake_follow(paint), "%s: does not answer the paint", label);
	}
	/* calls: one stock sequence, whatever the forwarders did first */
	expectedCalls(kind, e);
	checkCounters(label, calls0, prims0, e, primIndices[kind], 1);
	switch (kind) {
	case FILL_RECT: stockFillRect(b.cr, m, shape, paint); break;
	case STROKE_RECT: stockStrokeRect(b.cr, m, shape, paint); break;
	case FILL_PATH: stockFillPath(b.cr, m, testPath, paint); break;
	case STROKE_PATH: stockStrokePath(b.cr, m, testPath, paint); break;
	}
	compareCtx(label, &a, &b, pre, expectDrawn);
	freeCtx(&a);
	freeCtx(&b);
}

static cp_matrix_t
matrixOf(double xx, double yx, double xy, double yy, double x0, double y0)
{
	cp_matrix_t m;

	m.xx = xx;
	m.yx = yx;
	m.xy = xy;
	m.yy = yy;
	m.x0 = x0;
	m.y0 = y0;
	return m;
}

static sqInt
rect(sqInt ox, sqInt oy, sqInt cx, sqInt cy)
{
	return fake_rectangle(fake_point(ox, oy), fake_point(cx, cy));
}

static const cp_matrix_t *
identity(void)
{
	static cp_matrix_t m = { 1, 0, 0, 1, 0, 0 };

	return &m;
}

static void
makeTestPath(void)
{
	Ctx c = newCtx(0);

	TC(new_path)(c.cr);
	TC(move_to)(c.cr, 4.0, 4.0);
	TC(line_to)(c.cr, 30.5, 6.25);
	TC(curve_to)(c.cr, 36.0, 12.0, 20.0, 28.0, 10.0, 22.0);
	TC(close_path)(c.cr);
	TC(arc)(c.cr, 24.0, 18.0, 6.0, 0.0, 4.5);
	TC(line_to)(c.cr, 8.0, 30.0);
	testPath = TC(copy_path)(c.cr);
	freeCtx(&c);
}


/*** S1 fill rect ***/

static void
testFillRect(void)
{
	cp_matrix_t rot = matrixOf(cos(0.4), sin(0.4), -sin(0.4), cos(0.4), 6.0, -3.0);
	cp_matrix_t odd = matrixOf(1.25, 0.1, -0.2, 0.8, 2.5, 1.0);
	sqInt red = newColor(1023, 0, 0, 255), translucent = newColor(17, 600, 1000, 128);
	int pre;

	for (pre = 0; pre < 4; pre++) {
		drawCase("integers", FILL_RECT, pre, identity(), 0, rect(fake_int(3), fake_int(4), fake_int(30), fake_int(25)), red, NULL);
		drawCase("floats", FILL_RECT, pre, &odd, 0,
			rect(fake_float(2.5), fake_float(3.25), fake_float(29.75), fake_boxed_float(20.125)), translucent, NULL);
		drawCase("mixed", FILL_RECT, pre, &rot, 1,
			rect(fake_int(5), fake_float(1.5), fake_float(22.3), fake_int(18)), translucent, NULL);
		drawCase("negative extent", FILL_RECT, pre, identity(), 1,
			rect(fake_int(30), fake_int(25), fake_int(4), fake_int(2)), red, NULL);
	}
	expectDrawn = 0;
	drawCase("alpha 0", FILL_RECT, 1, identity(), 0, rect(fake_int(0), fake_int(0), fake_int(40), fake_int(32)),
		newColor(500, 500, 500, 0), NULL);
	expectDrawn = 1;
	/* beyond cairo's fixed point: 1.17.4 and 1.18.6 fill the band, 1.18.4
	   nothing; the same as stock either way */
	expectDrawn = -1;
	drawCase("huge integers", FILL_RECT, 0, identity(), 0,
		rect(fake_int(-((sqInt)1 << 40)), fake_int(1), fake_int(((sqInt)1 << 40) + 7), fake_int(9)), red, NULL);
	expectDrawn = 1;
}


/*** S2 stroke rect, S3 fill path, S4 stroke path ***/

static void
testStrokes(void)
{
	static const double three[3] = { 4.0, 2.0, 1.0 };
	static const double ints[2] = { 5.0, 3.0 };
	double many[100];
	static double many4096[4096];
	cp_matrix_t odd = matrixOf(1.25, 0.1, -0.2, 0.8, 2.5, 1.0);
	sqInt blue = newColor(0, 300, 1023, 200);
	sqInt r = rect(fake_int(5), fake_int(4), fake_float(33.5), fake_int(27));
	int i, pre, kind;

	for (i = 0; i < 100; i++) {
		many[i] = 0.5 + (i % 7) * 0.75;
	}
	for (i = 0; i < 4096; i++) {
		many4096[i] = 1 + i % 4;
	}
	for (kind = STROKE_RECT; kind <= STROKE_PATH; kind += 2) {
		sqInt shape = kind == STROKE_RECT ? r : newPathObject(testPath);
		for (pre = 0; pre < 4; pre++) {
			drawCase("dashes nil", kind, pre, identity(), 0,
				shape, newStroke(blue, fake_int(3), fake_int(1), fake_int(2), fakeNil, fakeNil), NULL);
			drawCase("dashes empty", kind, pre, &odd, 0,
				shape, newStroke(blue, fake_float(2.5), fake_int(2), fake_int(1), newDashes(NULL, 0, 0), fake_float(1.5)), NULL);
			drawCase("dashes floats", kind, pre, &odd, 1,
				shape, newStroke(blue, fake_boxed_float(1.75), fake_int(0), fake_int(0), newDashes(three, 3, 0), fake_float(0.5)), NULL);
			drawCase("dashes integers", kind, pre, identity(), 0,
				shape, newStroke(blue, fake_int(2), fake_int(2), fake_int(2), newDashes(ints, 2, 1), fake_int(3)), NULL);
		}
		drawCase("dashes 100 (malloc)", kind, 1, &odd, 0,
			shape, newStroke(blue, fake_int(1), fake_int(0), fake_int(1), newDashes(many, 100, 0), fake_float(0.25)), NULL);
		drawCase("dashes 64 (stack)", kind, 2, &odd, 0,
			shape, newStroke(blue, fake_int(1), fake_int(1), fake_int(0), newDashes(many, 64, 0), fake_int(0)), NULL);
		drawCase("dashes 65", kind, 3, &odd, 0,
			shape, newStroke(blue, fake_int(1), fake_int(1), fake_int(0), newDashes(many, 65, 0), fake_int(1)), NULL);
		drawCase("dashes 4096 (the most)", kind, 1, identity(), 0,
			shape, newStroke(blue, fake_int(1), fake_int(0), fake_int(0), newDashes(many4096, 4096, 1), fake_int(2)), NULL);
		drawCase("nil dashes, nil offset", kind, 0, identity(), 0,
			shape, newStroke(blue, fake_int(4), fake_int(1), fake_int(1), fakeNil, fakeNil), NULL);
	}
	for (pre = 0; pre < 4; pre++) {
		drawCase("path", FILL_PATH, pre, identity(), 0, newPathObject(testPath), blue, NULL);
		drawCase("path, transformed", FILL_PATH, pre, &odd, 1, newPathObject(testPath), newColor(1, 2, 3, 255), NULL);
	}
}


/*** S5 failures ***/

/* The primitive of kind fails with code: no cairo call, nothing drawn,
   nothing allocated or stored, the cairo state untouched */
static void
failCase(const char *what, int kind, sqInt canvas, Ctx *c, sqInt shape, sqInt paint, sqInt cls, sqInt code)
{
	sqInt args[3];
	int nargs = kind == STROKE_RECT || kind == STROKE_PATH ? 3 : 2;
	uint64_t calls0[CP_FN_COUNT], prims0[CP_P_COUNT], none[CP_FN_COUNT];
	FakeAnswer r;
	Ctx fresh = newCtx(1);
	State s0, s1;
	char label[200];

	snprintf(label, sizeof label, "%s (%s)", what, kindNames[kind]);
	args[0] = shape;
	args[1] = paint;
	args[2] = cls;
	stateOf(c->cr, &s0);
	snapCounters(calls0, prims0);
	r = fake_call(prim(primNames[kind]), canvas, nargs, args);
	CHECK(r.code == code && !r.returned && r.protocolErrors == 0, "%s: code %lld, expected %lld %s", label,
		(long long)r.code, (long long)code, fake_last_error());
	CHECK(r.allocations == 0 && r.stores == 0, "%s: %d allocations, %d stores", label, r.allocations, r.stores);
	memset(none, 0, sizeof none);
	checkCounters(label, calls0, prims0, none, 0, 0);
	stateOf(c->cr, &s1);
	CHECK(memcmp(&s0, &s1, sizeof s0) == 0, "%s: the cairo state changed", label);
	CHECK(samePixels(c, &fresh), "%s: something was drawn", label);
	freeCtx(&fresh);
}

static void
testFailures(void)
{
	cp_matrix_t m = *identity();
	Ctx c = newCtx(1);
	sqInt canvas = newCanvas(&c, newMatrix(&m, 0));
	sqInt good = rect(fake_int(1), fake_int(2), fake_int(20), fake_int(20));
	sqInt red = newColor(1023, 0, 0, 255);
	sqInt path = newPathObject(testPath);
	sqInt stroke = newStroke(red, fake_int(2), fake_int(0), fake_int(0), fakeNil, fakeNil);
	sqInt surface = fake_slot(canvas, SLOT(CP_L_CANVAS_SURFACE)), saved, other, otherColor, bad;
	int kind;
	static const double two[2] = { 1.0, 2.0 };
	static double lots[4097];

	for (kind = 0; kind < 4097; kind++) {
		lots[kind] = 1.0 + kind % 3;
	}

	for (kind = FILL_RECT; kind <= STROKE_PATH; kind++) {
		sqInt shape = kind == FILL_RECT || kind == STROKE_RECT ? good : path;
		sqInt paint = kind == FILL_RECT || kind == FILL_PATH ? red : stroke;
		int isFill = kind == FILL_RECT || kind == FILL_PATH;

		/* the receiver chain */
		failCase("receiver nil", kind, fakeNil, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		failCase("receiver a Color", kind, red, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		saved = fake_slot(canvas, SLOT(CP_L_CANVAS_HANDLE));
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fakeNil);
		failCase("handle nil", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_external_address(NULL));
		failCase("handle NULL", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_bytes(fakeByteArray, &c.cr, sizeof c.cr));
		failCase("handle a ByteArray", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), saved);
		saved = fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM));
		bad = fake_new(fakeAthensCairoMatrix, 0);
		fake_set_slot(bad, 0, fake_bytes(fakeByteArray, &m, 47));
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), bad);
		failCase("matrix of 47 bytes", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(bad, 0, fake_external_address(NULL));
		failCase("matrix NULL", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), fakeNil);
		failCase("pathTransform nil", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
		fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), saved);
		/* currentCanvas: the fills only */
		if (isFill) {
			fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), fakeNil);
			failCase("currentCanvas nil (outside drawDuring:)", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
			other = newCanvas(&c, newMatrix(&m, 0));
			fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), other);
			failCase("currentCanvas another canvas", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
			fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), canvas);
			fake_set_slot(canvas, SLOT(CP_L_CANVAS_SURFACE), fakeNil);
			failCase("surface nil", kind, canvas, &c, shape, paint, fakeColor, PrimErrBadReceiver);
			fake_set_slot(canvas, SLOT(CP_L_CANVAS_SURFACE), surface);
		}
		/* the shape */
		if (kind == FILL_RECT || kind == STROKE_RECT) {
			failCase("rectangle nil", kind, canvas, &c, fakeNil, paint, fakeColor, PrimErrBadArgument);
			failCase("rectangle with a Fraction", kind, canvas, &c,
				rect(fake_new(fakeFraction, 0), fake_int(1), fake_int(9), fake_int(9)), paint, fakeColor, PrimErrBadArgument);
			failCase("rectangle with a nil corner", kind, canvas, &c,
				fake_rectangle(fake_point(fake_int(1), fake_int(1)), fakeNil), paint, fakeColor, PrimErrBadArgument);
			failCase("a Point for a rectangle", kind, canvas, &c, fake_point(fake_int(1), fake_int(1)), paint, fakeColor, PrimErrBadArgument);
		} else {
			failCase("path nil", kind, canvas, &c, fakeNil, paint, fakeColor, PrimErrBadArgument);
			failCase("path NULL", kind, canvas, &c, newPathObject(NULL), paint, fakeColor, PrimErrBadArgument);
			bad = fake_new(fakeAthensCairoPath, 0);
			fake_set_slot(bad, 0, fake_bytes(fakeByteArray, &testPath, sizeof testPath));
			failCase("path handle a ByteArray", kind, canvas, &c, bad, paint, fakeColor, PrimErrBadArgument);
		}
		/* the paint */
		if (isFill) {
			failCase("color nil", kind, canvas, &c, shape, fakeNil, fakeColor, PrimErrBadArgument);
			failCase("color alpha 256", kind, canvas, &c, shape, newColor(1, 1, 1, 256), fakeColor, PrimErrBadArgument);
			failCase("color alpha a Float", kind, canvas, &c, shape, fake_color(fake_int(5), fake_float(1.0)), fakeColor, PrimErrBadArgument);
			failCase("color a Point", kind, canvas, &c, shape, fake_point(fake_int(1), fake_int(1)), fakeColor, PrimErrBadArgument);
		} else {
			otherColor = fake_class("TranslucentColor", 1, 4);
			failCase("paint nil", kind, canvas, &c, shape, fakeNil, fakeColor, PrimErrBadArgument);
			failCase("paint a Color", kind, canvas, &c, shape, red, fakeColor, PrimErrBadArgument);
			failCase("fillPaint of another class", kind, canvas, &c, shape,
				newStroke(fake_array(otherColor, (sqInt[]){ fake_int(5), fakeNil, fakeNil, fake_int(255) }, 4),
					fake_int(2), fake_int(0), fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("fillPaint a pattern paint", kind, canvas, &c, shape,
				newStroke(fake_new(fakeAthensCairoPatternPaint, 0), fake_int(2), fake_int(0), fake_int(0), fakeNil, fakeNil),
				fakeColor, PrimErrBadArgument);
			failCase("colorClass nil", kind, canvas, &c, shape, stroke, fakeNil, PrimErrBadArgument);
			failCase("colorClass 3", kind, canvas, &c, shape, stroke, fake_int(3), PrimErrBadArgument);
			failCase("colorClass another class", kind, canvas, &c, shape, stroke, otherColor, PrimErrBadArgument);
			failCase("fillPaint alpha 300", kind, canvas, &c, shape,
				newStroke(newColor(1, 1, 1, 300), fake_int(2), fake_int(0), fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("width a Fraction", kind, canvas, &c, shape,
				newStroke(red, fake_new(fakeFraction, 0), fake_int(0), fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("width nil", kind, canvas, &c, shape,
				newStroke(red, fakeNil, fake_int(0), fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("join a Float", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_float(1.0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("cap nil", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fakeNil, fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("cap beyond int", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int((sqInt)1 << 40), fake_int(0), fakeNil, fakeNil), fakeColor, PrimErrBadArgument);
			failCase("dashes a ByteArray", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), fake_bytes(fakeByteArray, "ab", 2), fake_int(0)),
				fakeColor, PrimErrBadArgument);
			failCase("dashes an OrderedCollection", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), fake_new(fake_class("OrderedCollection", 1, 3), 0), fake_int(0)),
				fakeColor, PrimErrBadArgument);
			failCase("dashes with nil", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), fake_array(fakeArray, (sqInt[]){ fake_int(1), fakeNil }, 2), fake_int(0)),
				fakeColor, PrimErrBadArgument);
			failCase("dashes with a Fraction", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), fake_array(fakeArray, (sqInt[]){ fake_new(fakeFraction, 0) }, 1), fake_int(0)),
				fakeColor, PrimErrBadArgument);
			failCase("dashes 4097", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), newDashes(lots, 4097, 1), fake_int(0)), fakeColor, PrimErrBadArgument);
			failCase("dash offset nil", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), newDashes(two, 2, 0), fakeNil), fakeColor, PrimErrBadArgument);
			failCase("dash offset a Fraction", kind, canvas, &c, shape,
				newStroke(red, fake_int(1), fake_int(0), fake_int(0), newDashes(two, 2, 0), fake_new(fakeFraction, 0)), fakeColor, PrimErrBadArgument);
			bad = fake_new(fakeAthensCairoStrokePaint, 0);
			fake_set_slot(bad, 0, red);
			failCase("stroke paint of nils", kind, canvas, &c, shape, bad, fakeColor, PrimErrBadArgument);
		}
	}
	/* a stroke does not need the currentCanvas (stock strokes on the canvas
	   argument) */
	fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), fakeNil);
	{
		sqInt args[3] = { good, stroke, fakeColor };
		FakeAnswer r = fake_call(prim("primitiveStrokeRectangle"), canvas, 3, args);
		CHECK(r.returned && r.code == 0 && r.value == stroke, "stroke outside drawDuring:: code %lld", (long long)r.code);
		args[0] = path;
		r = fake_call(prim("primitiveStrokePath"), canvas, 3, args);
		CHECK(r.returned && r.code == 0 && r.value == stroke, "stroke path outside drawDuring:: code %lld", (long long)r.code);
	}
	fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), canvas);
	freeCtx(&c);
}


/*** S6 forwarders ***/

static int forwarderCase;

/* Puts the forwarder of forwarderCase into the canvas or the arguments */
static void
mangleForwarder(sqInt canvas, sqInt *args)
{
	sqInt t, o, d;

	switch (forwarderCase) {
	case 0:		/* the receiver itself (the caller passes it) */
		break;
	case 1: fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_HANDLE)))); break;
	case 2: fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM)))); break;
	case 3:
		t = fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM));
		fake_set_slot(t, SLOT(CP_L_EXTOBJ_HANDLE), fake_forwarder_to(fake_slot(t, SLOT(CP_L_EXTOBJ_HANDLE))));
		break;
	case 4: fake_set_slot(canvas, SLOT(CP_L_CANVAS_SURFACE), fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_SURFACE)))); break;
	case 5:
		t = fake_slot(canvas, SLOT(CP_L_CANVAS_SURFACE));
		fake_set_slot(t, SLOT(CP_L_SURFACE_CURRENT_CANVAS), fake_forwarder_to(canvas));
		break;
	case 6: args[0] = fake_forwarder_to(args[0]); break;		/* the shape */
	case 7: args[1] = fake_forwarder_to(args[1]); break;		/* the paint */
	case 8: args[2] = fake_forwarder_to(args[2]); break;		/* the class */
	case 9:		/* rectangle origin; path handle */
		fake_set_slot(args[0], 0, fake_forwarder_to(fake_slot(args[0], 0)));
		break;
	case 10:	/* rectangle corner's boxed y */
		o = fake_slot(args[0], 1);
		fake_set_slot(o, 1, fake_forwarder_to(fake_slot(o, 1)));
		break;
	case 11:	/* stroke: fillPaint */
		fake_set_slot(args[1], SLOT(CP_L_STROKE_FILL_PAINT), fake_forwarder_to(fake_slot(args[1], SLOT(CP_L_STROKE_FILL_PAINT))));
		break;
	case 12:	/* stroke: width */
		fake_set_slot(args[1], SLOT(CP_L_STROKE_WIDTH), fake_forwarder_to(fake_slot(args[1], SLOT(CP_L_STROKE_WIDTH))));
		break;
	case 13:	/* stroke: dashes */
		fake_set_slot(args[1], SLOT(CP_L_STROKE_DASHES), fake_forwarder_to(fake_slot(args[1], SLOT(CP_L_STROKE_DASHES))));
		break;
	case 14:	/* stroke: a boxed dash length */
		d = fake_slot(args[1], SLOT(CP_L_STROKE_DASHES));
		fake_set_slot(d, 1, fake_forwarder_to(fake_slot(d, 1)));
		break;
	case 15:	/* stroke: dashOffset */
		fake_set_slot(args[1], SLOT(CP_L_STROKE_DASH_OFFSET), fake_forwarder_to(fake_slot(args[1], SLOT(CP_L_STROKE_DASH_OFFSET))));
		break;
	}
}

static int
forwarderApplies(int kind, int k)
{
	int isFill = kind == FILL_RECT || kind == FILL_PATH, isRect = kind == FILL_RECT || kind == STROKE_RECT;

	switch (k) {
	case 4: case 5: return isFill;		/* the fills read the surface */
	case 8: case 11: case 12: case 13: case 14: case 15: return !isFill;
	case 10: return isRect;
	default: return 1;
	}
}

static void
testForwarders(void)
{
	static const double three[3] = { 4.0, 2.0, 1.0 };
	cp_matrix_t odd = matrixOf(1.25, 0.1, -0.2, 0.8, 2.5, 1.0);
	int kind, k;

	CHECK(fake_accessor_depth("primitiveFillRectangleColor") == 2 && fake_accessor_depth("primitiveStrokeRectangle") == 2
		&& fake_accessor_depth("primitiveFillPathColor") == 2 && fake_accessor_depth("primitiveStrokePath") == 2,
		"accessor depths are not 2");
	for (kind = FILL_RECT; kind <= STROKE_PATH; kind++) {
		for (k = 0; k <= 15; k++) {
			char what[64];
			sqInt shape, paint;
			int isFill = kind == FILL_RECT || kind == FILL_PATH;
			if (!forwarderApplies(kind, k)) {
				continue;
			}
			shape = kind == FILL_RECT || kind == STROKE_RECT
				? rect(fake_int(3), fake_boxed_float(2.5), fake_int(30), fake_boxed_float(27.75))
				: newPathObject(testPath);
			paint = isFill ? newColor(700, 20, 300, 230)
				: newStroke(newColor(5, 900, 5, 255), fake_boxed_float(2.25), fake_int(1), fake_int(1),
					fake_array(fakeArray, (sqInt[]){ fake_float(three[0]), fake_boxed_float(three[1]), fake_int(1) }, 3),
					fake_boxed_float(0.75));
			forwarderCase = k;
			snprintf(what, sizeof what, "forwarder %d", k);
			if (k == 0) {
				/* the receiver: drawCase builds it, so redo the call here */
				Ctx a = newCtx(2), b = newCtx(2);
				sqInt canvas = newCanvas(&a, newMatrix(&odd, 0));
				sqInt args[3] = { shape, paint, fakeColor };
				uint64_t calls0[CP_FN_COUNT], prims0[CP_P_COUNT], e[CP_FN_COUNT];
				FakeAnswer r;
				if (isFill) {
					/* the surface's currentCanvas is the object, not the forwarder */
					fake_set_slot(fake_slot(canvas, SLOT(CP_L_CANVAS_SURFACE)), SLOT(CP_L_SURFACE_CURRENT_CANVAS), canvas);
				}
				snapCounters(calls0, prims0);
				r = fake_call_retry(prim(primNames[kind]), fake_accessor_depth(primNames[kind]),
					fake_forwarder_to(canvas), isFill ? 2 : 3, args);
				CHECK(r.retried && r.firstCode == PrimErrBadReceiver && r.returned && r.code == 0,
					"forwarder receiver (%s): first %lld, code %lld", kindNames[kind], (long long)r.firstCode, (long long)r.code);
				expectedCalls(kind, e);
				checkCounters("forwarder receiver", calls0, prims0, e, primIndices[kind], 1);
				switch (kind) {
				case FILL_RECT: stockFillRect(b.cr, &odd, shape, paint); break;
				case STROKE_RECT: stockStrokeRect(b.cr, &odd, shape, paint); break;
				case FILL_PATH: stockFillPath(b.cr, &odd, testPath, paint); break;
				case STROKE_PATH: stockStrokePath(b.cr, &odd, testPath, paint); break;
				}
				compareCtx("forwarder receiver", &a, &b, 2, 1);
				freeCtx(&a);
				freeCtx(&b);
				continue;
			}
			drawCase(what, kind, k % 4, &odd, 0, shape, paint, mangleForwarder);
		}
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
		}
	}
	vm = fake_init();
#ifndef CP_STATIC_CAIRO
	if (!loadTestCairo()) {
		return 1;
	}
#endif
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
	if (!bind()) {
		printf("glue-shape FAILED: no bind\n");
		return 1;
	}
	makeTestPath();
	testFillRect();
	testStrokes();
	testFailures();
	testForwarders();
	TC(path_destroy)(testPath);
	for (i = 0; i < nMatrixCopies; i++) {
		free(matrixCopies[i]);
	}
	shutdownModule();
	fake_free_all();
	if (failures) {
		printf("glue-shape FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("glue-shape ok: %d checks\n", checks);
	return 0;
}
