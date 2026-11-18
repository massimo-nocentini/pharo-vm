/* glue-text.c -- the text primitive of the CairoPlugin (src/prims/text.c:
 * primitiveShowString, with Amendment A1: the four string classes, UTF-8 by
 * the SimdUTF engine) against the fake VM of fakevm.c, a real cairo and a
 * real FreeType face (a TrueType font of the system, loaded with test-only
 * FreeType and cairo_ft calls).
 *
 * Each drawing case runs twice on two image surfaces prepared alike: once
 * through the primitive (a CairoFreetypeFontRenderer-shaped fake object
 * whose font, canvas and glyphExtents hold the real cairo objects), once
 * through the stock call sequence of renderCharacters:from:to: made
 * directly on cairo (glyphsOf:from:to: with the copy of the glyphs and
 * freeGlyphs: before the lock, then renderGlyphs:), with the UTF-8 of an
 * independent encoder of this file; then: the answer (true, or false when
 * cairo fails), the pixels (memcmp), the cairo state (matrix, source rgba,
 * font matrix, the scaled font, current point, status), the glyph extents
 * written into glyphExtents, the UTF-8 cairo received (a spy in the
 * plugin's table in front of cairo_scaled_font_text_to_glyphs: the bytes,
 * the length, x, y, the NULL glyph pointer and the character count it was
 * given) and the cairo calls, function by function (12, or 10 when cairo
 * fails).
 *
 *   T1 strings     ASCII, Latin-1 bytes >= 0x80, a ByteSymbol, ranges
 *                  inside a string, empty strings and empty ranges;
 *                  WideString and WideSymbol with BMP and astral characters
 *                  (U+00E9 U+03BB U+4E2D U+1F600); invalid characters
 *                  outside the range; pixel sizes as SmallInteger, Float,
 *                  boxed Float; glyphExtents as bytes and as an
 *                  ExternalAddress
 *   T2 buffers     UTF-8 of 1023, 1024 (the stack buffer), 1025 and 1200
 *                  bytes (malloc), from byte and word strings
 *   T3 cairo fails a cairo_t in an error state: false, glyphExtents
 *                  untouched, no glyph_extents and no glyph_free
 *   T4 encoder     cp_c_encode against the independent encoder: random
 *                  byte and word strings and ranges, the stack/malloc
 *                  choice; surrogates, values above U+10FFFF and the
 *                  leading-char bits refused
 *   T5 failures    invalid characters (U+D800, U+110000, leading-char
 *                  bits) in the range: #'bad argument' before any cairo
 *                  call; the receiver chain (font, canvas, handles,
 *                  pathTransform, a paint of another class, glyphExtents of
 *                  40 bytes, read-only, NULL), the arguments (not one of
 *                  the four classes, start/end out of range, a Fraction
 *                  pixel size), the optional lock/unlock functions missing
 *                  (#'unsupported operation'), a binding without the string
 *                  classes (29-element layout): the code, no cairo call,
 *                  nothing drawn
 *   T6 forwarders  at every level of every chain, to depth 3 (receiver,
 *                  font and its handle, canvas, its handle, its paint, its
 *                  pathTransform and that handle, glyphExtents and its
 *                  handle; the string, the pixel size, the class): the first
 *                  call fails before any cairo call, the VM's retry draws
 *                  the stock pixels
 *
 * Usage: glue-text [--libcairo PATH] [--font PATH] (run by
 * tests/run-c-tests.sh, lanes glue-text and glue-wasm64).  The font is
 * --font, else the first of a list of common system paths (DejaVu Sans,
 * Liberation Sans, Arial...).  Prints one line per failed check and
 * "glue-text ok: N checks"; exits 1 when a check failed.
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
static const char *fontPath;
static const char *const fontCandidates[] = {
	"/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
	"/usr/share/fonts/TTF/DejaVuSans.ttf",
	"/usr/share/fonts/dejavu/DejaVuSans.ttf",
	"/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
	"/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
	"/usr/share/fonts/liberation-sans/LiberationSans-Regular.ttf",
	"/System/Library/Fonts/Supplemental/Arial.ttf",
	"/Library/Fonts/Arial.ttf",
	"C:/Windows/Fonts/arial.ttf",
	NULL
};


/*** The test's own cairo and FreeType (never through the plugin's table,
     so never counted) ***/

#ifdef CP_STATIC_CAIRO

# define TC(name) cairo_##name
typedef cairo_font_face_t tc_font_face_t;
typedef cairo_font_options_t tc_font_options_t;
typedef FT_Library tf_library;
typedef FT_Face tf_face;
# define TF(name) FT_##name

#else

typedef struct _cairo_font_face tc_font_face_t;
typedef struct _cairo_font_options tc_font_options_t;
typedef void *tf_library;
typedef void *tf_face;

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
	X(void, restore, (cp_cairo_t *cr)) \
	X(void, get_matrix, (cp_cairo_t *cr, cp_matrix_t *m)) \
	X(cp_pattern_t *, get_source, (cp_cairo_t *cr)) \
	X(int, pattern_get_rgba, (cp_pattern_t *p, double *r, double *g, double *b, double *a)) \
	X(int, has_current_point, (cp_cairo_t *cr)) \
	X(void, get_font_matrix, (cp_cairo_t *cr, cp_matrix_t *m)) \
	X(cp_scaled_font_t *, get_scaled_font, (cp_cairo_t *cr)) \
	X(void, set_matrix, (cp_cairo_t *cr, const cp_matrix_t *m)) \
	X(void, set_source_rgba, (cp_cairo_t *cr, double r, double g, double b, double a)) \
	X(void, set_scaled_font, (cp_cairo_t *cr, const cp_scaled_font_t *sf)) \
	X(void, set_font_size, (cp_cairo_t *cr, double size)) \
	X(void, translate, (cp_cairo_t *cr, double x, double y)) \
	X(void, move_to, (cp_cairo_t *cr, double x, double y)) \
	X(void, paint, (cp_cairo_t *cr)) \
	X(int, scaled_font_text_to_glyphs, (cp_scaled_font_t *sf, double x, double y, const char *utf8, int len, \
		cp_glyph_t **glyphs, int *n, cp_text_cluster_t **clusters, int *nclusters, int *flags)) \
	X(void, scaled_font_glyph_extents, (cp_scaled_font_t *sf, const cp_glyph_t *g, int n, cp_text_extents_t *e)) \
	X(void, glyph_free, (cp_glyph_t *g)) \
	X(void, show_glyphs, (cp_cairo_t *cr, const cp_glyph_t *g, int n)) \
	X(void *, ft_scaled_font_lock_face, (cp_scaled_font_t *sf)) \
	X(void, ft_scaled_font_unlock_face, (cp_scaled_font_t *sf)) \
	X(tc_font_face_t *, ft_font_face_create_for_ft_face, (void *face, int flags)) \
	X(void, font_face_destroy, (tc_font_face_t *f)) \
	X(tc_font_options_t *, font_options_create, (void)) \
	X(void, font_options_destroy, (tc_font_options_t *o)) \
	X(cp_scaled_font_t *, scaled_font_create, (tc_font_face_t *f, const cp_matrix_t *fm, const cp_matrix_t *ctm, \
		const tc_font_options_t *o)) \
	X(int, scaled_font_status, (cp_scaled_font_t *sf)) \
	X(void, scaled_font_destroy, (cp_scaled_font_t *sf))

# define TC_FIELD(ret, name, params) ret (*name) params;
static struct { TC_FUNCTIONS(TC_FIELD) } tc;
# undef TC_FIELD
# define TC(name) tc.name

static struct {
	int (*Init_FreeType)(tf_library *lib);
	int (*New_Face)(tf_library lib, const char *path, long index, tf_face *face);
	int (*Done_Face)(tf_face face);
	int (*Done_FreeType)(tf_library lib);
} tf;
# define TF(name) tf.name

static void *cairoHandle, *freetypeHandle;

static int
loadTestLibraries(void)
{
	char path[1024];
	const char *slash;

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
	/* the FreeType cairo was linked with: the one already loaded, else the
	   one next to libcairo */
	freetypeHandle = dlopen("libfreetype.so.6", RTLD_NOW | RTLD_LOCAL | RTLD_NOLOAD);
	if (!freetypeHandle && (slash = strrchr(libcairo, '/'))) {
		snprintf(path, sizeof path, "%.*s/libfreetype.so.6", (int)(slash - libcairo), libcairo);
		freetypeHandle = dlopen(path, RTLD_NOW | RTLD_LOCAL);
	}
	if (!freetypeHandle) {
		freetypeHandle = dlopen("libfreetype.so.6", RTLD_NOW | RTLD_LOCAL);
	}
	if (!freetypeHandle) {
		printf("FAILED: no libfreetype.so.6: %s\n", dlerror());
		return 0;
	}
	if (!(*(void **)&tf.Init_FreeType = dlsym(freetypeHandle, "FT_Init_FreeType"))
	 || !(*(void **)&tf.New_Face = dlsym(freetypeHandle, "FT_New_Face"))
	 || !(*(void **)&tf.Done_Face = dlsym(freetypeHandle, "FT_Done_Face"))
	 || !(*(void **)&tf.Done_FreeType = dlsym(freetypeHandle, "FT_Done_FreeType"))) {
		printf("FAILED: FreeType functions missing\n");
		return 0;
	}
	return 1;
}

#endif /* CP_STATIC_CAIRO */

#define W 96
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

static cp_fn_scaled_font_text_to_glyphs realTextToGlyphs;

/* What cairo_scaled_font_text_to_glyphs received from the plugin */
static struct {
	int calls;
	double x, y;
	int length, glyphsWasNull, numGlyphsIn;
	char bytes[8192];
} spy;

static cp_status_t
spyTextToGlyphs(cp_scaled_font_t *sf, double x, double y, const char *utf8, int length,
	cp_glyph_t **glyphs, int *n, cp_text_cluster_t **clusters, int *nclusters, cp_text_cluster_flags_t *flags)
{
	spy.calls++;
	spy.x = x;
	spy.y = y;
	spy.length = length;
	spy.glyphsWasNull = *glyphs == NULL;
	spy.numGlyphsIn = *n;
	if (length > 0) {
		memcpy(spy.bytes, utf8, (size_t)length < sizeof spy.bytes ? (size_t)length : sizeof spy.bytes);
	}
	return realTextToGlyphs(sf, x, y, utf8, length, glyphs, n, clusters, nclusters, flags);
}

/* Binds with the string classes (33 elements) or without (29); then the
   spy in front of text_to_glyphs */
static int
bind(int withClasses)
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
	args[2] = fake_array(fakeArray, oops, withClasses ? CP_LAYOUT_SIZE_TEXT : CP_LAYOUT_SIZE_BASE);
	a = fake_call(prim("primitiveBind"), fakeNil, 3, args);
	CHECK(a.returned && a.code == 0 && a.protocolErrors == 0, "bind: code %lld %s", (long long)a.code, fake_last_error());
	if (!(a.returned && a.code == 0)) {
		return 0;
	}
	/* the test's spy (the plugin calls through the table: CP_CALL still
	   counts the call) */
	realTextToGlyphs = cp_api.scaled_font_text_to_glyphs;
	cp_api.scaled_font_text_to_glyphs = spyTextToGlyphs;
	return 1;
}

static void
snapCounters(uint64_t calls[CP_FN_COUNT], uint64_t prims[CP_P_COUNT])
{
	memcpy(calls, cp_cairo_calls, sizeof cp_cairo_calls);
	memcpy(prims, cp_prim_calls, sizeof cp_prim_calls);
}

static void
checkCounters(const char *what, const uint64_t calls0[CP_FN_COUNT], const uint64_t prims0[CP_P_COUNT],
	const uint64_t expected[CP_FN_COUNT], uint64_t primDelta)
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
		uint64_t want = i == CP_P_ShowString ? primDelta : 0;
		if (cp_prim_calls[i] - prims0[i] != want) {
			CHECK(0, "%s: %s succeeded %llu times, expected %llu", what, cp_primitive_names[i],
				(unsigned long long)(cp_prim_calls[i] - prims0[i]), (unsigned long long)want);
			bad = 1;
		}
	}
	CHECK(!bad, "%s: counters", what);
}

/* The cairo calls of the stock drawString: (converted: 12; cairo failed: 10,
   the numbers of text.c's header and docs/CairoPlugin.md) */
static void
expectedCalls(int converted, uint64_t e[CP_FN_COUNT])
{
	uint64_t total = 0;
	int k;

	memset(e, 0, CP_FN_COUNT * sizeof e[0]);
	e[CP_FN_set_scaled_font] = 2;
	e[CP_FN_set_font_size] = 1;
	e[CP_FN_get_scaled_font] = 1;
	e[CP_FN_scaled_font_text_to_glyphs] = 1;
	e[CP_FN_ft_scaled_font_lock_face] = 1;
	e[CP_FN_set_matrix] = 1;
	e[CP_FN_set_source_rgba] = 1;
	e[CP_FN_show_glyphs] = 1;
	e[CP_FN_ft_scaled_font_unlock_face] = 1;
	if (converted) {
		e[CP_FN_scaled_font_glyph_extents] = 1;
		e[CP_FN_glyph_free] = 1;
	}
	for (k = 0; k < CP_FN_COUNT; k++) {
		total += e[k];
	}
	CHECK(total == (converted ? 12u : 10u), "expectedCalls: %llu calls", (unsigned long long)total);
}


/*** The font ***/

static tf_library ftLibrary;
static tf_face ftFace;
static tc_font_face_t *fontFace;
static tc_font_options_t *fontOptions;
static cp_scaled_font_t *scaledFont;	/* 13 px, as a CairoScaledFont of Athens */

static int
loadFont(void)
{
	cp_matrix_t fm = { 13.0, 0.0, 0.0, 13.0, 0.0, 0.0 }, ctm = { 1.0, 0.0, 0.0, 1.0, 0.0, 0.0 };
	FILE *f;
	int i;

	if (!fontPath) {
		for (i = 0; fontCandidates[i]; i++) {
			if ((f = fopen(fontCandidates[i], "rb"))) {
				fclose(f);
				fontPath = fontCandidates[i];
				break;
			}
		}
	}
	if (!fontPath) {
		printf("FAILED: no TrueType font found (--font PATH)\n");
		return 0;
	}
	if (TF(Init_FreeType)(&ftLibrary) || TF(New_Face)(ftLibrary, fontPath, 0, &ftFace)) {
		printf("FAILED: FreeType cannot load %s\n", fontPath);
		return 0;
	}
	fontFace = TC(ft_font_face_create_for_ft_face)(ftFace, 0);
	fontOptions = TC(font_options_create)();
	scaledFont = TC(scaled_font_create)(fontFace, &fm, &ctm, fontOptions);
	if (TC(scaled_font_status)(scaledFont) != 0) {
		printf("FAILED: cairo cannot make a scaled font of %s\n", fontPath);
		return 0;
	}
	printf("-- font %s\n", fontPath);
	return 1;
}

static void
freeFont(void)
{
	TC(scaled_font_destroy)(scaledFont);
	TC(font_options_destroy)(fontOptions);
	TC(font_face_destroy)(fontFace);
	/* the face outlives cairo's font face, whose cache may keep it: done
	   last, after cairo let it go (cairo_debug_reset_static_data is not
	   exported by every build, so the library stays) */
	(void)ftLibrary;
}


/*** Surfaces, state ***/

typedef struct {
	cp_surface_t *surface;
	cp_cairo_t *cr;
} Ctx;

/* An ARGB32 surface, painted, with a state the drawing must change or keep
   (another font size, a transform, a current point); pre 3: a cairo_t in
   an error state (a restore without save) */
static Ctx
newCtx(int pre)
{
	Ctx c;

	c.surface = TC(image_surface_create)(0, W, H);	/* CAIRO_FORMAT_ARGB32 */
	c.cr = TC(create)(c.surface);
	TC(set_source_rgba)(c.cr, 0.95, 0.95, 0.9, 1.0);
	TC(paint)(c.cr);
	switch (pre % 4) {
	case 0:
		break;
	case 1:
		TC(set_font_size)(c.cr, 30.0);
		TC(translate)(c.cr, 2.0, 3.0);
		break;
	case 2:
		TC(set_scaled_font)(c.cr, scaledFont);
		TC(move_to)(c.cr, 5.0, 5.0);
		break;
	case 3:
		TC(restore)(c.cr);	/* CAIRO_STATUS_INVALID_RESTORE */
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
	int hasPoint;
	double fontMatrix[6];
	cp_scaled_font_t *scaled;
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
	s->hasPoint = TC(has_current_point)(cr);
	TC(get_font_matrix)(cr, &m);
	memcpy(s->fontMatrix, &m, sizeof m);
	s->scaled = TC(get_scaled_font)(cr);
}

static int
samePixels(Ctx *a, Ctx *b)
{
	TC(surface_flush)(a->surface);
	TC(surface_flush)(b->surface);
	return memcmp(TC(image_surface_get_data)(a->surface), TC(image_surface_get_data)(b->surface),
		(size_t)TC(image_surface_get_stride)(a->surface) * H) == 0;
}

static void
compareCtx(const char *what, Ctx *a, Ctx *b, int pre, int drawn)
{
	State sa, sb;
	Ctx fresh;

	CHECK(samePixels(a, b), "%s: pixels differ from the stock sequence's", what);
	stateOf(a->cr, &sa);
	stateOf(b->cr, &sb);
	CHECK(memcmp(&sa, &sb, sizeof sa) == 0, "%s: state differs (status %d/%d, matrix %d, rgba %d, point %d, font matrix %d, scaled font %d)",
		what, sa.status, sb.status, memcmp(sa.matrix, sb.matrix, sizeof sa.matrix) != 0,
		memcmp(sa.rgba, sb.rgba, sizeof sa.rgba) != 0, sa.hasPoint != sb.hasPoint,
		memcmp(sa.fontMatrix, sb.fontMatrix, sizeof sa.fontMatrix) != 0, sa.scaled != sb.scaled);
	fresh = newCtx(pre);
	CHECK(samePixels(a, &fresh) == !drawn, "%s: %s", what, drawn ? "nothing was drawn" : "something was drawn");
	freeCtx(&fresh);
}


/*** The independent UTF-8 encoder (the stock converter's rules: Latin-1
     bytes, UTF-32 words) ***/

/* The UTF-8 of units [start0, stop0) into out (room for 4 per unit):
   answers its length, or -1 for a surrogate or a value above U+10FFFF */
static long
refUtf8(const void *units, int words, size_t start0, size_t stop0, unsigned char *out)
{
	size_t i;
	long n = 0;

	for (i = start0; i < stop0; i++) {
		uint32_t c = words ? ((const uint32_t *)units)[i] : ((const unsigned char *)units)[i];
		if (c < 0x80) {
			out[n++] = (unsigned char)c;
		} else if (c < 0x800) {
			out[n++] = (unsigned char)(0xC0 | (c >> 6));
			out[n++] = (unsigned char)(0x80 | (c & 0x3F));
		} else if (c >= 0xD800 && c <= 0xDFFF) {
			return -1;
		} else if (c < 0x10000) {
			out[n++] = (unsigned char)(0xE0 | (c >> 12));
			out[n++] = (unsigned char)(0x80 | ((c >> 6) & 0x3F));
			out[n++] = (unsigned char)(0x80 | (c & 0x3F));
		} else if (c <= 0x10FFFF) {
			out[n++] = (unsigned char)(0xF0 | (c >> 18));
			out[n++] = (unsigned char)(0x80 | ((c >> 12) & 0x3F));
			out[n++] = (unsigned char)(0x80 | ((c >> 6) & 0x3F));
			out[n++] = (unsigned char)(0x80 | (c & 0x3F));
		} else {
			return -1;
		}
	}
	return n;
}


/*** Athens-shaped objects ***/

static void *mallocs[512];
static int nMallocs;

static void *
keep(void *p)
{
	if (nMallocs < 512) {
		mallocs[nMallocs++] = p;
	}
	return p;
}

/* An ExternalObject of class cls whose handle is h */
static sqInt
newExternal(sqInt cls, sqInt h)
{
	sqInt o = fake_new(cls, 0);

	fake_set_slot(o, SLOT(CP_L_EXTOBJ_HANDLE), h);
	return o;
}

/* The canvas of a renderer: handle cr, pathTransform the matrix (bytes),
   paint the Color */
static sqInt
newCanvas(Ctx *c, const cp_matrix_t *m, sqInt paint)
{
	sqInt canvas = fake_new(fakeAthensCairoCanvas, 0);
	sqInt surface = fake_new(fakeAthensCairoSurface, 0);

	fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_external_address(c->cr));
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM),
		newExternal(fakeAthensCairoMatrix, fake_bytes(fakeByteArray, m, sizeof *m)));
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), paint);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_SURFACE), surface);
	fake_set_slot(surface, SLOT(CP_L_SURFACE_CURRENT_CANVAS), canvas);
	return canvas;
}

static const unsigned char extentsPattern[48] = {
	0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5,
	0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5,
	0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5
};

/* A CairoFreetypeFontRenderer: font (a CairoScaledFont of scaledFont),
   canvas, glyphExtents (a CairoTextExtents of 48 bytes, or of an
   ExternalAddress of a malloc'd 48 bytes), the rest nil */
static sqInt
newRenderer(sqInt canvas, int extentsAsAddress)
{
	sqInt r = fake_new(fakeCairoFreetypeFontRenderer, 0);
	sqInt handle;

	if (extentsAsAddress) {
		void *p = keep(malloc(48));
		memcpy(p, extentsPattern, 48);
		handle = fake_external_address(p);
	} else {
		handle = fake_bytes(fakeByteArray, extentsPattern, 48);
	}
	fake_set_slot(r, SLOT(CP_L_RENDERER_FONT), newExternal(fakeCairoScaledFont, fake_external_address(scaledFont)));
	fake_set_slot(r, SLOT(CP_L_RENDERER_CANVAS), canvas);
	fake_set_slot(r, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, handle));
	return r;
}

/* The 48 bytes of glyphExtents of a renderer */
static void *
extentsOf(sqInt renderer)
{
	sqInt h = fake_follow(fake_slot(fake_follow(fake_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS))),
		SLOT(CP_L_EXTOBJ_HANDLE)));

	return fake_class_of(h) == fakeExternalAddress ? fake_address_value(h) : fake_body(h);
}

static sqInt
newColor(sqInt r, sqInt g, sqInt b, sqInt alpha)
{
	return fake_color(fake_int((r << 20) | (g << 10) | b), fake_int(alpha));
}

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

/* A string of one of the four classes: bytes or words */
static sqInt
byteString(sqInt cls, const char *s, size_t n)
{
	return fake_bytes(cls, s, n);
}

static sqInt
wideString(sqInt cls, const uint32_t *w, size_t n)
{
	return fake_words(cls, w, n);
}

static const cp_matrix_t baseline = { 1.0, 0.0, 0.0, 1.0, 2.0, 22.0 };	/* translated to a baseline */


/*** The stock sequence (renderCharacters:from:to:), made directly ***/

/* Answers whether cairo converted the text; ext receives the extents */
static int
stockShowString(cp_cairo_t *cr, const cp_matrix_t *m, sqInt color, const unsigned char *utf8, int utf8Length,
	int count, double pixelSize, void *ext)
{
	cp_glyph_t *glyphs = NULL, *copy;
	cp_scaled_font_t *scaled;
	double rgba[4];
	int n = count, status;
	static cp_glyph_t empty[1];

	/* glyphsOf:from:to: */
	TC(set_scaled_font)(cr, scaledFont);
	TC(set_font_size)(cr, pixelSize);
	scaled = TC(get_scaled_font)(cr);
	status = TC(scaled_font_text_to_glyphs)(scaled, 0.0, 0.0, (const char *)utf8, utf8Length, &glyphs, &n, NULL, NULL, NULL);
	if (status != 0) {
		copy = empty;	/* CairoGlyphsArray new: 0 */
		n = 0;
	} else {
		TC(scaled_font_glyph_extents)(scaledFont, glyphs, n, (cp_text_extents_t *)ext);
		copy = malloc((size_t)(n ? n : 1) * sizeof(cp_glyph_t));
		if (n) {
			memcpy(copy, glyphs, (size_t)n * sizeof(cp_glyph_t));	/* LibC memCopy */
		}
		TC(glyph_free)(glyphs);		/* freeGlyphs: */
	}
	/* renderGlyphs: */
	colorComponents(color, rgba);
	TC(ft_scaled_font_lock_face)(scaledFont);
	TC(set_matrix)(cr, m);
	TC(set_scaled_font)(cr, scaledFont);
	TC(set_source_rgba)(cr, rgba[0], rgba[1], rgba[2], rgba[3]);
	TC(show_glyphs)(cr, copy, n);
	TC(ft_scaled_font_unlock_face)(scaledFont);
	if (copy != empty) {
		free(copy);
	}
	return status == 0;
}


/*** T1-T3 drawing cases ***/

typedef void (*Mangle)(sqInt renderer, sqInt *args);
static int forwarderCase;

/* Draws characters start..end of string (whose units are units, words
   when wide) through the primitive and through the stock sequence; with
   mangle, puts forwarders first and expects the VM's retry */
static void
textCase(const char *what, sqInt string, const void *units, int words, sqInt startOop, sqInt endOop,
	sqInt pixelSize, int pre, int extentsAsAddress, Mangle mangle)
{
	sqInt start = fake_int_value(startOop), end = fake_int_value(endOop);
	Ctx a = newCtx(pre), b = newCtx(pre);
	sqInt color = newColor(40, 300, 900, pre == 1 ? 255 : 201);	/* opaque, translucent */
	sqInt canvas = newCanvas(&a, &baseline, color);
	sqInt renderer = newRenderer(canvas, extentsAsAddress);
	sqInt args[5];
	unsigned char *utf8 = malloc((size_t)(4 * (end - start + 1) + 1));
	long utf8Length = refUtf8(units, words, (size_t)(start - 1), (size_t)end, utf8);
	unsigned char extB[48];
	int converted, errored = pre % 4 == 3, drawn;
	uint64_t calls0[CP_FN_COUNT], prims0[CP_P_COUNT], e[CP_FN_COUNT];
	FakeAnswer r;
	char label[200];

	snprintf(label, sizeof label, "%s (pre %d%s)", what, pre, extentsAsAddress ? ", extents an ExternalAddress" : "");
	CHECK(utf8Length >= 0, "%s: the reference encoder refuses the string", label);
	args[0] = string;
	args[1] = startOop;
	args[2] = endOop;
	args[3] = pixelSize;
	args[4] = fakeColor;
	if (mangle) {
		mangle(renderer, args);
	}
	memset(&spy, 0, sizeof spy);
	snapCounters(calls0, prims0);
	if (mangle) {
		r = fake_call_retry(prim("primitiveShowString"), fake_accessor_depth("primitiveShowString"),
			forwarderCase == 0 ? fake_forwarder_to(renderer) : renderer, 5, args);
		CHECK(r.retried && r.firstCode != 0, "%s: the forwarder was not met (first code %lld)", label, (long long)r.firstCode);
	} else {
		r = fake_call(prim("primitiveShowString"), renderer, 5, args);
	}
	CHECK(r.returned && r.code == 0 && r.protocolErrors == 0, "%s: code %lld %s", label, (long long)r.code, fake_last_error());
	CHECK(r.allocations == 0 && r.stores == 0, "%s: %d allocations, %d stores", label, r.allocations, r.stores);
	memcpy(extB, extentsPattern, 48);
	converted = stockShowString(b.cr, &baseline, color, utf8, (int)utf8Length, (int)(end - start + 1),
		asFloat(fake_follow(pixelSize)), extB);
	CHECK(converted == !errored, "%s: the stock sequence %s", label, converted ? "converted" : "did not convert");
	CHECK(r.value == (converted ? fakeTrueObject : fakeFalseObject), "%s: answers %s, the stock sequence %s", label,
		r.value == fakeTrueObject ? "true" : r.value == fakeFalseObject ? "false" : "?", converted ? "converted" : "did not");
	/* what cairo received */
	CHECK(spy.calls == 1, "%s: text_to_glyphs called %d times", label, spy.calls);
	CHECK(spy.length == utf8Length && (utf8Length == 0
		|| ((size_t)utf8Length <= sizeof spy.bytes && memcmp(spy.bytes, utf8, (size_t)utf8Length) == 0)),
		"%s: UTF-8 of %d bytes, the independent encoder's %ld bytes differ", label, spy.length, utf8Length);
	CHECK(spy.x == 0.0 && spy.y == 0.0 && spy.glyphsWasNull && spy.numGlyphsIn == (int)(end - start + 1),
		"%s: text_to_glyphs arguments x %g y %g glyphs %s n %d", label, spy.x, spy.y, spy.glyphsWasNull ? "NULL" : "set", spy.numGlyphsIn);
	/* the extents: the stock's, or untouched */
	CHECK(memcmp(extentsOf(renderer), converted ? extB : extentsPattern, 48) == 0, "%s: glyphExtents differ", label);
	expectedCalls(converted, e);
	checkCounters(label, calls0, prims0, e, 1);
	drawn = converted && end >= start;
	compareCtx(label, &a, &b, pre, drawn);
	free(utf8);
	freeCtx(&a);
	freeCtx(&b);
}

static void
testStrings(void)
{
	static const char latin1[] = "caf\xe9 \xfc\xdf \xa9\xff \xc0\xd7";
	static const uint32_t wide[] = { 'H', 0xE9, 0x3BB, 0x4E2D, 0x1F600, '!', 'z' };
	static const uint32_t wideBadOutside[] = { 0xD800, 'o', 'k', 0x110000 };
	int pre;

	for (pre = 0; pre < 4; pre++) {
		textCase("ASCII", byteString(fakeByteString, "Hello, world", 12), "Hello, world", 0,
			fake_int(1), fake_int(12), fake_int(14), pre, pre & 1, NULL);
		textCase("Latin-1", byteString(fakeByteString, latin1, strlen(latin1)), latin1, 0,
			fake_int(1), fake_int((sqInt)strlen(latin1)), fake_float(13.5), pre, 0, NULL);
		textCase("ByteSymbol", byteString(fakeByteSymbol, "#sym\xe8ol", 8), "#sym\xe8ol", 0,
			fake_int(1), fake_int(8), fake_int(12), pre, 0, NULL);
		textCase("range 3..7", byteString(fakeByteString, "abcdefghij", 10), "abcdefghij", 0,
			fake_int(3), fake_int(7), fake_boxed_float(1e1), pre, 1, NULL);
		textCase("WideString", wideString(fakeWideString, wide, 7), wide, 1,
			fake_int(1), fake_int(7), fake_int(16), pre, 0, NULL);
		textCase("WideSymbol", wideString(fakeWideSymbol, wide, 7), wide, 1,
			fake_int(1), fake_int(7), fake_int(11), pre, 1, NULL);
		textCase("WideString 2..5", wideString(fakeWideString, wide, 7), wide, 1,
			fake_int(2), fake_int(5), fake_float(15.25), pre, 0, NULL);
		textCase("invalid outside the range", wideString(fakeWideString, wideBadOutside, 4), wideBadOutside, 1,
			fake_int(2), fake_int(3), fake_int(14), pre, 0, NULL);
	}
	/* empty strings and ranges: cairo converts nothing, draws nothing */
	textCase("empty", byteString(fakeByteString, "", 0), "", 0, fake_int(1), fake_int(0), fake_int(14), 0, 0, NULL);
	textCase("empty WideString", wideString(fakeWideString, wide, 0), wide, 1, fake_int(1), fake_int(0), fake_int(14), 1, 0, NULL);
	textCase("empty range 2..1", byteString(fakeByteString, "abc", 3), "abc", 0, fake_int(2), fake_int(1), fake_int(14), 2, 1, NULL);
	textCase("empty range 4..3", byteString(fakeByteString, "abc", 3), "abc", 0, fake_int(4), fake_int(3), fake_int(14), 0, 0, NULL);
	textCase("empty range errored", byteString(fakeByteString, "abc", 3), "abc", 0, fake_int(1), fake_int(0), fake_int(14), 3, 0, NULL);
}

static void
testBuffers(void)
{
	char *latin;
	uint32_t *words;
	size_t i;

	latin = malloc(1200);
	words = malloc(1200 * sizeof(uint32_t));
	for (i = 0; i < 1200; i++) {
		latin[i] = (char)(i % 3 == 0 ? 'a' + i % 26 : 0xE0 + i % 32);
	}
	/* 512 x 2 bytes = 1024 (the stack buffer); + 1 ASCII = 1025 (malloc) */
	memset(latin, (char)0xE9, 600);
	textCase("1024 UTF-8 bytes", byteString(fakeByteString, latin, 600), latin, 0, fake_int(1), fake_int(512), fake_int(14), 0, 0, NULL);
	latin[512] = 'x';
	textCase("1025 UTF-8 bytes", byteString(fakeByteString, latin, 600), latin, 0, fake_int(1), fake_int(513), fake_int(14), 1, 1, NULL);
	textCase("1023 UTF-8 bytes", byteString(fakeByteString, latin, 600), latin, 0, fake_int(2), fake_int(513), fake_int(14), 2, 0, NULL);
	textCase("1200 UTF-8 bytes", byteString(fakeByteString, latin, 600), latin, 0, fake_int(1), fake_int(600), fake_int(14), 0, 0, NULL);
	/* words: 341 x 3 + 1 = 1024, + 1 = 1025; 400 x 3 = 1200 */
	for (i = 0; i < 1200; i++) {
		words[i] = 0x4E2D + (uint32_t)(i % 50);
	}
	words[341] = 'a';
	words[342] = 'b';
	textCase("1024 UTF-8 bytes from words", wideString(fakeWideString, words, 1200), words, 1, fake_int(1), fake_int(342), fake_int(14), 0, 0, NULL);
	textCase("1025 UTF-8 bytes from words", wideString(fakeWideString, words, 1200), words, 1, fake_int(1), fake_int(343), fake_int(14), 1, 0, NULL);
	words[700] = 0x1F600;
	textCase("long WideString with astral", wideString(fakeWideSymbol, words, 1200), words, 1, fake_int(300), fake_int(1200), fake_int(14), 2, 1, NULL);
	textCase("1200 UTF-8 bytes errored", wideString(fakeWideString, words, 1200), words, 1, fake_int(343), fake_int(742), fake_int(14), 3, 0, NULL);
	free(latin);
	free(words);
}


/*** T4 the encoder ***/

static uint64_t rngState = 0x9E3779B97F4A7C15ULL;

static uint32_t
rnd(void)
{
	rngState ^= rngState << 13;
	rngState ^= rngState >> 7;
	rngState ^= rngState << 17;
	return (uint32_t)(rngState >> 11);
}

static uint32_t
randomCharacter(int words)
{
	uint32_t c;

	if (!words) {
		return rnd() % 256;
	}
	switch (rnd() % 5) {
	case 0: return rnd() % 0x80;
	case 1: return 0x80 + rnd() % 0x780;
	case 2:
		do {
			c = 0x800 + rnd() % 0xF800;
		} while (c >= 0xD800 && c <= 0xDFFF);
		return c;
	case 3: return 0x10000 + rnd() % 0x100000;
	default: return rnd() % 256;
	}
}

static void
testEncoder(void)
{
	unsigned char *ref = malloc(4 * 3000 + 4);
	unsigned char *bytes = malloc(3000);
	uint32_t *words = malloc(3000 * sizeof(uint32_t));
	cp_c_utf8 u;
	int k, length, bad = 0, heapUsed = 0, stackUsed = 0;
	size_t i, n, start0, stop0;
	long refLength;
	sqInt code;

	for (k = 0; k < 3000; k++) {
		int wide = k & 1;
		n = (size_t)(k % 7 == 0 ? rnd() % 3000 : rnd() % 600);
		for (i = 0; i < n; i++) {
			uint32_t c = randomCharacter(wide);
			if (wide) {
				words[i] = c;
			} else {
				bytes[i] = (unsigned char)c;
			}
		}
		start0 = n ? rnd() % (n + 1) : 0;
		stop0 = start0 + (n - start0 ? rnd() % (n - start0 + 1) : 0);
		refLength = refUtf8(wide ? (const void *)words : (const void *)bytes, wide, start0, stop0, ref);
		code = cp_c_encode(wide ? (const void *)words : (const void *)bytes, n, wide, start0, stop0, &u, &length);
		if (code != 0 || refLength < 0 || length != refLength || (length && memcmp(u.at, ref, (size_t)length) != 0)
		 || (u.at == u.heap) != (length > CP_C_UTF8_ON_STACK)) {
			if (bad++ < 5) {
				CHECK(0, "encoder: string %d (%s, %zu units, %zu..%zu): code %lld, %d bytes, reference %ld",
					k, wide ? "words" : "bytes", n, start0, stop0, (long long)code, length, refLength);
			}
		}
		if (u.at && u.at == u.heap) {
			heapUsed++;
		} else {
			stackUsed++;
		}
		cp_c_utf8_free(&u);
	}
	CHECK(bad == 0, "encoder: %d of 3000 random strings differ", bad);
	CHECK(heapUsed > 50 && stackUsed > 50, "encoder: heap %d, stack %d", heapUsed, stackUsed);
	/* refused: a surrogate, above U+10FFFF, the leading-char bits */
	{
		static const uint32_t refused[] = { 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0x110000, 0x3FFFFF, (1u << 22) | 0x41, 0xFFFFFFFFu };
		for (k = 0; k < (int)(sizeof refused / sizeof refused[0]); k++) {
			uint32_t s[3] = { 'a', refused[k], 'b' };
			code = cp_c_encode(s, 3, 1, 0, 3, &u, &length);
			CHECK(code == PrimErrBadArgument, "encoder: U+%X accepted (code %lld)", refused[k], (long long)code);
			cp_c_utf8_free(&u);
			code = cp_c_encode(s, 3, 1, 2, 3, &u, &length);
			CHECK(code == 0 && length == 1 && u.at[0] == 'b', "encoder: U+%X outside the range refused", refused[k]);
			cp_c_utf8_free(&u);
		}
	}
	/* the stack buffer holds 1024 bytes, not one more */
	memset(bytes, 0xE9, 600);
	code = cp_c_encode(bytes, 600, 0, 0, 512, &u, &length);
	CHECK(code == 0 && length == 1024 && u.at == u.stack && !u.heap, "encoder: 1024 bytes not on the stack");
	cp_c_utf8_free(&u);
	code = cp_c_encode(bytes, 600, 0, 0, 513, &u, &length);
	CHECK(code == 0 && length == 1026 && u.at == u.heap && u.heap, "encoder: 1026 bytes not malloc'd");
	cp_c_utf8_free(&u);
	CHECK(!u.heap && !u.at, "encoder: cp_c_utf8_free");
	/* more than INT_MAX bytes (cairo takes an int length): refused by the
	   allocate callback before any malloc */
	memset(&u, 0, sizeof u);
	CHECK(cp_c_utf8_allocate(&u, SU_KIND_BYTES, (size_t)INT_MAX + 1) == NULL && u.tooLong && !u.heap && !u.noMemory,
		"encoder: more than INT_MAX bytes accepted");
	CHECK(cp_c_utf8_allocate(&u, SU_KIND_BYTES, 0) == u.stack, "encoder: 0 bytes not on the stack");
	/* the extremes of the valid set */
	{
		static const uint32_t edge[] = { 0, 0x7F, 0x80, 0x7FF, 0x800, 0xD7FF, 0xE000, 0xFFFD, 0xFFFF, 0x10000, 0x10FFFF };
		refLength = refUtf8(edge, 1, 0, 11, ref);
		code = cp_c_encode(edge, 11, 1, 0, 11, &u, &length);
		CHECK(code == 0 && length == refLength && memcmp(u.at, ref, (size_t)length) == 0, "encoder: the edges");
		cp_c_utf8_free(&u);
	}
	/* outside the contract: a range past the end (the primitive checks first) */
	code = cp_c_encode(bytes, 3, 0, 2, 4, &u, &length);
	CHECK(code == PrimErrBadArgument, "encoder: a range past the end (code %lld)", (long long)code);
	cp_c_utf8_free(&u);
	free(ref);
	free(bytes);
	free(words);
}


/*** T5 failures ***/

/* The primitive fails with code: no cairo call, no allocation or store,
   nothing drawn, the extents untouched */
static void
failCase(const char *what, sqInt renderer, Ctx *c, sqInt string, sqInt start, sqInt end, sqInt pixelSize, sqInt cls, sqInt code)
{
	sqInt args[5] = { string, start, end, pixelSize, cls };
	uint64_t calls0[CP_FN_COUNT], prims0[CP_P_COUNT], none[CP_FN_COUNT];
	FakeAnswer r;
	Ctx fresh = newCtx(0);

	memset(&spy, 0, sizeof spy);
	snapCounters(calls0, prims0);
	r = fake_call(prim("primitiveShowString"), renderer, 5, args);
	CHECK(r.code == code && !r.returned && r.protocolErrors == 0, "%s: code %lld, expected %lld %s", what,
		(long long)r.code, (long long)code, fake_last_error());
	CHECK(r.allocations == 0 && r.stores == 0, "%s: %d allocations, %d stores", what, r.allocations, r.stores);
	memset(none, 0, sizeof none);
	checkCounters(what, calls0, prims0, none, 0);
	CHECK(spy.calls == 0, "%s: cairo was called", what);
	CHECK(samePixels(c, &fresh), "%s: something was drawn", what);
	freeCtx(&fresh);
}

static void
testFailures(void)
{
	static const uint32_t surrogate[] = { 'a', 0xD800, 'b' }, high[] = { 'a', 0x110000 }, leading[] = { (1u << 22) | 0x41, 'b' };
	static const uint32_t lowSurrogate[] = { 0xDFFF };
	Ctx c = newCtx(0);
	sqInt color = newColor(1, 2, 3, 255);
	sqInt canvas = newCanvas(&c, &baseline, color);
	sqInt renderer = newRenderer(canvas, 0);
	sqInt good = fake_string("abc"), px = fake_int(14), saved, inner, bad, other;
	cp_fn_ft_scaled_font_lock_face lock;
	cp_fn_ft_scaled_font_unlock_face unlock;
	int k;

	/* invalid characters in the range: before any cairo call */
	failCase("U+D800", renderer, &c, wideString(fakeWideString, surrogate, 3), fake_int(1), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("U+D800 alone", renderer, &c, wideString(fakeWideString, surrogate, 3), fake_int(2), fake_int(2), px, fakeColor, PrimErrBadArgument);
	failCase("U+DFFF WideSymbol", renderer, &c, wideString(fakeWideSymbol, lowSurrogate, 1), fake_int(1), fake_int(1), px, fakeColor, PrimErrBadArgument);
	failCase("U+110000", renderer, &c, wideString(fakeWideString, high, 2), fake_int(1), fake_int(2), px, fakeColor, PrimErrBadArgument);
	failCase("leading-char bits", renderer, &c, wideString(fakeWideString, leading, 2), fake_int(1), fake_int(2), px, fakeColor, PrimErrBadArgument);
	/* the arguments */
	failCase("string nil", renderer, &c, fakeNil, fake_int(1), fake_int(0), px, fakeColor, PrimErrBadArgument);
	failCase("string a ByteArray", renderer, &c, fake_bytes(fakeByteArray, "abc", 3), fake_int(1), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("string a WordArray", renderer, &c, fake_words(fakeWordArray, (const uint32_t *)"abcd", 1), fake_int(1), fake_int(1), px, fakeColor, PrimErrBadArgument);
	failCase("string an Array", renderer, &c, fake_new(fakeArray, 3), fake_int(1), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("string a SmallInteger", renderer, &c, fake_int(3), fake_int(1), fake_int(1), px, fakeColor, PrimErrBadArgument);
	failCase("string a ByteString subclass", renderer, &c, fake_bytes(fake_class("ByteStringSubclass", 16, 0), "abc", 3),
		fake_int(1), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("start 0", renderer, &c, good, fake_int(0), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("start -5", renderer, &c, good, fake_int(-5), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("end 4", renderer, &c, good, fake_int(1), fake_int(4), px, fakeColor, PrimErrBadArgument);
	failCase("start 5 end 3", renderer, &c, good, fake_int(5), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("start 3 end 1", renderer, &c, good, fake_int(3), fake_int(1), px, fakeColor, PrimErrBadArgument);
	failCase("end -1", renderer, &c, good, fake_int(1), fake_int(-1), px, fakeColor, PrimErrBadArgument);
	failCase("start a Float", renderer, &c, good, fake_float(1.0), fake_int(3), px, fakeColor, PrimErrBadArgument);
	failCase("end nil", renderer, &c, good, fake_int(1), fakeNil, px, fakeColor, PrimErrBadArgument);
	failCase("end huge", renderer, &c, good, fake_int(1), fake_int((sqInt)1 << 40), px, fakeColor, PrimErrBadArgument);
	failCase("pixelSize a Fraction", renderer, &c, good, fake_int(1), fake_int(3), fake_new(fakeFraction, 0), fakeColor, PrimErrBadArgument);
	failCase("pixelSize nil", renderer, &c, good, fake_int(1), fake_int(3), fakeNil, fakeColor, PrimErrBadArgument);
	/* the receiver chain */
	failCase("receiver nil", fakeNil, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	failCase("receiver a canvas", canvas, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	other = fake_class("TranslucentColor", 1, 4);
	failCase("colorClass another class", renderer, &c, good, fake_int(1), fake_int(3), px, other, PrimErrBadReceiver);
	failCase("colorClass nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeNil, PrimErrBadReceiver);
	failCase("colorClass 3", renderer, &c, good, fake_int(1), fake_int(3), px, fake_int(3), PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), fake_array(other, (sqInt[]){ fake_int(5), fakeNil, fakeNil, fake_int(255) }, 4));
	failCase("paint of another class", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), fake_new(fakeAthensCairoPatternPaint, 0));
	failCase("paint a pattern", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), fakeNil);
	failCase("paint nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), newColor(1, 1, 1, 256));
	failCase("paint alpha 256", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), color);
	/* font */
	saved = fake_slot(renderer, SLOT(CP_L_RENDERER_FONT));
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_FONT), fakeNil);
	failCase("font nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_FONT), newExternal(fakeCairoScaledFont, fake_external_address(NULL)));
	failCase("font NULL", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_FONT), newExternal(fakeCairoScaledFont, fake_bytes(fakeByteArray, &scaledFont, 8)));
	failCase("font handle a ByteArray", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_FONT), saved);
	/* canvas */
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_CANVAS), fakeNil);
	failCase("canvas nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_CANVAS), canvas);
	saved = fake_slot(canvas, SLOT(CP_L_CANVAS_HANDLE));
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_external_address(NULL));
	failCase("canvas handle NULL", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), saved);
	saved = fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM));
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), newExternal(fakeAthensCairoMatrix, fake_bytes(fakeByteArray, &baseline, 40)));
	failCase("pathTransform of 40 bytes", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), fakeNil);
	failCase("pathTransform nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM), saved);
	/* glyphExtents */
	saved = fake_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS));
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), fakeNil);
	failCase("glyphExtents nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, fake_bytes(fakeByteArray, extentsPattern, 40)));
	failCase("glyphExtents of 40 bytes", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, fake_external_address(NULL)));
	failCase("glyphExtents NULL", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, fakeNil));
	failCase("glyphExtents handle nil", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	bad = fake_bytes(fakeByteArray, extentsPattern, 48);
	fake_set_immutable(bad, 1);
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, bad));
	failCase("glyphExtents read-only", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	inner = fake_bytes(fakeByteString, extentsPattern, 48);	/* bytes of another class: accepted as stock (void*) */
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), newExternal(fakeCairoTextExtents, fake_compiled_method(48)));
	failCase("glyphExtents a CompiledMethod", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadReceiver);
	(void)inner;
	fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), saved);
	/* the optional functions missing */
	lock = cp_api.ft_scaled_font_lock_face;
	unlock = cp_api.ft_scaled_font_unlock_face;
	for (k = 0; k < 2; k++) {
		if (k == 0) {
			cp_api.ft_scaled_font_lock_face = NULL;
		} else {
			cp_api.ft_scaled_font_unlock_face = NULL;
		}
		failCase(k ? "no unlock_face" : "no lock_face", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrUnsupported);
		cp_api.ft_scaled_font_lock_face = lock;
		cp_api.ft_scaled_font_unlock_face = unlock;
	}
	/* a binding without the string classes: no string is accepted */
	if (bind(0)) {
		failCase("29-element binding", renderer, &c, good, fake_int(1), fake_int(3), px, fakeColor, PrimErrBadArgument);
		failCase("29-element binding, WideString", renderer, &c, wideString(fakeWideString, high, 1), fake_int(1), fake_int(1),
			px, fakeColor, PrimErrBadArgument);
	}
	bind(1);
	/* and still good afterwards */
	{
		sqInt args[5] = { good, fake_int(1), fake_int(3), px, fakeColor };
		FakeAnswer r = fake_call(prim("primitiveShowString"), renderer, 5, args);
		CHECK(r.returned && r.value == fakeTrueObject, "after the failures: code %lld", (long long)r.code);
	}
	freeCtx(&c);
}


/*** T6 forwarders ***/

static void
mangleForwarder(sqInt renderer, sqInt *args)
{
	sqInt canvas = fake_slot(renderer, SLOT(CP_L_RENDERER_CANVAS)), t, x;

	switch (forwarderCase) {
	case 0: break;	/* the receiver: textCase passes a forwarder */
	case 1: x = fake_slot(renderer, SLOT(CP_L_RENDERER_FONT));
		fake_set_slot(renderer, SLOT(CP_L_RENDERER_FONT), fake_forwarder_to(x)); break;
	case 2: x = fake_slot(renderer, SLOT(CP_L_RENDERER_FONT));
		fake_set_slot(x, SLOT(CP_L_EXTOBJ_HANDLE), fake_forwarder_to(fake_slot(x, SLOT(CP_L_EXTOBJ_HANDLE)))); break;
	case 3: fake_set_slot(renderer, SLOT(CP_L_RENDERER_CANVAS), fake_forwarder_to(canvas)); break;
	case 4: fake_set_slot(canvas, SLOT(CP_L_CANVAS_HANDLE), fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_HANDLE)))); break;
	case 5: fake_set_slot(canvas, SLOT(CP_L_CANVAS_PAINT), fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_PAINT)))); break;
	case 6: fake_set_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM),
			fake_forwarder_to(fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM)))); break;
	case 7: t = fake_slot(canvas, SLOT(CP_L_CANVAS_PATH_TRANSFORM));	/* depth 3 */
		fake_set_slot(t, SLOT(CP_L_EXTOBJ_HANDLE), fake_forwarder_to(fake_slot(t, SLOT(CP_L_EXTOBJ_HANDLE)))); break;
	case 8: x = fake_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS));
		fake_set_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS), fake_forwarder_to(x)); break;
	case 9: x = fake_slot(renderer, SLOT(CP_L_RENDERER_GLYPH_EXTENTS));
		fake_set_slot(x, SLOT(CP_L_EXTOBJ_HANDLE), fake_forwarder_to(fake_slot(x, SLOT(CP_L_EXTOBJ_HANDLE)))); break;
	case 10: args[0] = fake_forwarder_to(args[0]); break;	/* the string */
	case 11: args[3] = fake_forwarder_to(args[3]); break;	/* the boxed pixel size */
	case 12: args[4] = fake_forwarder_to(args[4]); break;	/* the class */
	}
}

static void
testForwarders(void)
{
	static const uint32_t wide[] = { 'F', 0xE9, 0x3BB, 0x4E2D, 0x1F600, 'w' };
	char what[64];
	int k;

	CHECK(fake_accessor_depth("primitiveShowString") == 3, "accessor depth %d", fake_accessor_depth("primitiveShowString"));
	for (k = 0; k <= 12; k++) {
		forwarderCase = k;
		snprintf(what, sizeof what, "forwarder %d", k);
		textCase(what, wideString(fakeWideString, wide, 6), wide, 1, fake_int(1), fake_int(6),
			fake_boxed_float(12.5), k % 3, k == 9 ? 0 : k & 1, mangleForwarder);
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
	if (!loadTestLibraries()) {
		return 1;
	}
#endif
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
	if (!loadFont() || !bind(1)) {
		printf("glue-text FAILED: no font or no bind\n");
		return 1;
	}
	testStrings();
	testBuffers();
	testEncoder();
	testFailures();
	testForwarders();
	shutdownModule();
	freeFont();
	for (i = 0; i < nMallocs; i++) {
		free(mallocs[i]);
	}
	fake_free_all();
	if (failures) {
		printf("glue-text FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("glue-text ok: %d checks\n", checks);
	return 0;
}
