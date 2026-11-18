/* bindtest.c -- the binding layer of the CairoPlugin (src/binding/cpCairo.c)
 * against a real libcairo, without a VM: the lane bind of
 * tests/run-c-tests.sh builds it with cpCairo.c (own types, as every native
 * build) and runs it on the libcairo.so.2 of a VM bundle (--libcairo).
 *
 *   B1  names      the name and flag tables: 57 functions, "cairo_" names,
 *                  54 required, 2 optional (the ft ones), 1 probe (create)
 *   B2  refusals   a NULL loader or table: CP_BIND_BAD_ARGUMENT, nothing
 *                  looked up; a loader that finds nothing: CP_BIND_MISSING,
 *                  "cairo_version"; a missing required function (each one in
 *                  turn): CP_BIND_MISSING naming it, the lookups stopped
 *                  there, the table untouched; the optional ones missing:
 *                  bound, NULL
 *   B3  bind       every function bound, each the dlsym of its name, the
 *                  version >= 1.16.0
 *   B4  matrices   the matrix functions through the table on known values
 *   B5  drawing    an ARGB32 image surface (test-only dlsym of
 *                  cairo_image_surface_*): a rectangle filled through the
 *                  table, read back pixel by pixel; paths (copy, append,
 *                  current point), state setters and getters, clip, dash,
 *                  paint, the operator
 *   B6  text       the toy font's scaled font: text_to_glyphs, the extents,
 *                  show_glyphs, glyph_free, text_path, the ft lock/unlock
 *
 * Usage: bindtest --libcairo PATH.  Prints one line per failed check and
 * "bindtest ok: N checks"; exits 1 when a check failed. */

#include <dlfcn.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "cpCairo.h"

static int checks, failures;

#define CHECK(cond, ...) do { \
		checks++; \
		if (!(cond)) { \
			failures++; \
			printf("FAILED %s:%d: ", __FILE__, __LINE__); \
			printf(__VA_ARGS__); \
			printf("\n"); \
		} \
	} while (0)

static void *library;
static int lookups;
static const char *hidden[4];

static void *
loader(const char *name, void *context)
{
	int i;

	lookups++;
	for (i = 0; i < 4; i++) {
		if (hidden[i] && strcmp(hidden[i], name) == 0) {
			return NULL;
		}
	}
	return dlsym(context, name);
}

static void *
nothing(const char *name, void *context)
{
	(void)name;
	(void)context;
	lookups++;
	return NULL;
}

/* test-only functions of cairo, not in the table */
typedef cp_surface_t *(*surface_create_fn)(int format, int width, int height);
typedef unsigned char *(*get_data_fn)(cp_surface_t *surface);
typedef int (*get_stride_fn)(cp_surface_t *surface);
typedef void (*destroy_fn)(void *object);
typedef int (*status_fn)(cp_cairo_t *cr);
typedef int (*has_point_fn)(cp_cairo_t *cr);
typedef double (*get_width_fn)(cp_cairo_t *cr);
typedef int (*dash_count_fn)(cp_cairo_t *cr);

static int
isTable(const cp_cairo_api *api, unsigned char byte)
{
	const unsigned char *p = (const unsigned char *)api;
	size_t i;

	for (i = 0; i < sizeof *api; i++) {
		if (p[i] != byte) {
			return 0;
		}
	}
	return 1;
}

static void
testNames(void)
{
	int i, req = 0, opt = 0, probe = 0;

	CHECK(CP_FN_COUNT == 57, "%d functions", (int)CP_FN_COUNT);
	for (i = 0; i < CP_FN_COUNT; i++) {
		CHECK(strncmp(cp_cairo_function_names[i], "cairo_", 6) == 0, "name %d: %s", i, cp_cairo_function_names[i]);
		req += cp_cairo_function_flags[i] == CP_REQ;
		opt += cp_cairo_function_flags[i] == CP_OPT;
		probe += cp_cairo_function_flags[i] == CP_PROBE;
	}
	CHECK(req == 54 && opt == 2 && probe == 1, "flags: %d required, %d optional, %d probes", req, opt, probe);
	CHECK(strcmp(cp_cairo_function_names[CP_FN_version], "cairo_version") == 0, "CP_FN_version");
	CHECK(strcmp(cp_cairo_function_names[CP_FN_create], "cairo_create") == 0 && cp_cairo_function_flags[CP_FN_create] == CP_PROBE,
		"CP_FN_create");
	CHECK(strcmp(cp_cairo_function_names[CP_FN_text_path], "cairo_text_path") == 0, "CP_FN_text_path");
	CHECK(cp_cairo_function_flags[CP_FN_ft_scaled_font_lock_face] == CP_OPT
		&& cp_cairo_function_flags[CP_FN_ft_scaled_font_unlock_face] == CP_OPT, "the ft functions are optional");
}

static void
testRefusals(void)
{
	cp_cairo_api api;
	const char *missing;
	int i, rc;

	memset(&api, 0xAB, sizeof api);
	lookups = 0;
	missing = "unchanged";
	CHECK(cp_cairo_bind(NULL, library, &api, &missing) == CP_BIND_BAD_ARGUMENT, "NULL loader");
	CHECK(cp_cairo_bind(loader, library, NULL, &missing) == CP_BIND_BAD_ARGUMENT, "NULL table");
	CHECK(lookups == 0 && isTable(&api, 0xAB) && strcmp(missing, "unchanged") == 0, "a refusal looked up or wrote");
	rc = cp_cairo_bind(nothing, library, &api, &missing);
	CHECK(rc == CP_BIND_MISSING && strcmp(missing, "cairo_version") == 0 && lookups == 1 && isTable(&api, 0xAB),
		"a loader that finds nothing: %d %s, %d lookups", rc, missing, lookups);
	CHECK(cp_cairo_bind(nothing, library, &api, NULL) == CP_BIND_MISSING, "missing may be NULL");
	/* each required function missing in turn */
	for (i = 0; i < CP_FN_COUNT; i++) {
		if (cp_cairo_function_flags[i] == CP_OPT) {
			continue;
		}
		hidden[0] = cp_cairo_function_names[i];
		lookups = 0;
		missing = NULL;
		memset(&api, 0xAB, sizeof api);
		rc = cp_cairo_bind(loader, library, &api, &missing);
		CHECK(rc == CP_BIND_MISSING && missing && strcmp(missing, cp_cairo_function_names[i]) == 0
			&& lookups == i + 1 && isTable(&api, 0xAB), "%s missing: %d %s, %d lookups", cp_cairo_function_names[i],
			rc, missing ? missing : "-", lookups);
	}
	/* the optional ones missing */
	hidden[0] = "cairo_ft_scaled_font_lock_face";
	hidden[1] = "cairo_ft_scaled_font_unlock_face";
	rc = cp_cairo_bind(loader, library, &api, &missing);
	CHECK(rc == CP_BIND_OK && !api.ft_scaled_font_lock_face && !api.ft_scaled_font_unlock_face && api.new_path,
		"the optional functions missing: %d", rc);
	hidden[0] = hidden[1] = NULL;
}

int
main(int argc, char **argv)
{
	const char *path = "libcairo.so.2";
	cp_cairo_api api;
	const char *missing = NULL;
	int i, rc, version;

	setvbuf(stdout, NULL, _IOLBF, 0);
	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--libcairo") == 0 && i + 1 < argc) {
			path = argv[++i];
		}
	}
	library = dlopen(path, RTLD_NOW | RTLD_LOCAL);
	if (!library) {
		printf("FAILED: dlopen %s: %s\n", path, dlerror());
		return 1;
	}
	testNames();
	testRefusals();

	/* B3 */
	memset(&api, 0, sizeof api);
	rc = cp_cairo_bind(loader, library, &api, &missing);
	CHECK(rc == CP_BIND_OK, "bind: %d %s", rc, missing ? missing : "");
	{
		void **fields = (void **)&api;	/* (the table is CP_FN_COUNT function pointers) */
		CHECK(sizeof api == CP_FN_COUNT * sizeof(void *), "the table's size");
		for (i = 0; i < CP_FN_COUNT; i++) {
			CHECK(fields[i] && fields[i] == dlsym(library, cp_cairo_function_names[i]), "%s", cp_cairo_function_names[i]);
		}
	}
	version = api.version();
	CHECK(version >= CP_CAIRO_VERSION_MIN, "version %d", version);
	printf("bindtest: %s, cairo %d.%d.%d\n", path, version / 10000, version / 100 % 100, version % 100);

	/* B4 matrices */
	{
		cp_matrix_t m, n, r;
		double x, y;
		api.matrix_init(&m, 1, 2, 3, 4, 5, 6);
		CHECK(m.xx == 1 && m.yx == 2 && m.xy == 3 && m.yy == 4 && m.x0 == 5 && m.y0 == 6, "matrix_init");
		api.matrix_init_identity(&n);
		CHECK(n.xx == 1 && n.yx == 0 && n.xy == 0 && n.yy == 1 && n.x0 == 0 && n.y0 == 0, "matrix_init_identity");
		api.matrix_init_translate(&n, 7, 8);
		CHECK(n.x0 == 7 && n.y0 == 8 && n.xx == 1, "matrix_init_translate");
		api.matrix_init_scale(&n, 2, 3);
		CHECK(n.xx == 2 && n.yy == 3 && n.x0 == 0, "matrix_init_scale");
		api.matrix_init_rotate(&n, M_PI / 2);
		CHECK(fabs(n.xx) < 1e-15 && n.yx == 1 && n.xy == -1, "matrix_init_rotate");
		api.matrix_init_identity(&n);
		api.matrix_translate(&n, 1, 2);
		api.matrix_scale(&n, 2, 2);
		api.matrix_rotate(&n, 0);
		x = 1; y = 1;
		api.matrix_transform_point(&n, &x, &y);
		CHECK(x == 3 && y == 4, "translate, scale, transform_point: %g %g", x, y);
		api.matrix_multiply(&r, &m, &n);
		CHECK(r.xx == 2 && r.yx == 4 && r.x0 == 11 && r.y0 == 14, "matrix_multiply %g %g %g %g", r.xx, r.yx, r.x0, r.y0);
		CHECK(api.matrix_invert(&n) == CP_STATUS_SUCCESS && n.xx == 0.5 && n.x0 == -0.5, "matrix_invert");
		api.matrix_init(&m, 1, 2, 2, 4, 0, 0);
		CHECK(api.matrix_invert(&m) != CP_STATUS_SUCCESS, "matrix_invert of a singular matrix");
	}

	/* B5 drawing */
	{
		surface_create_fn surfaceCreate = (surface_create_fn)dlsym(library, "cairo_image_surface_create");
		get_data_fn getData = (get_data_fn)dlsym(library, "cairo_image_surface_get_data");
		get_stride_fn getStride = (get_stride_fn)dlsym(library, "cairo_image_surface_get_stride");
		destroy_fn destroy = (destroy_fn)dlsym(library, "cairo_destroy");
		destroy_fn surfaceDestroy = (destroy_fn)dlsym(library, "cairo_surface_destroy");
		status_fn status = (status_fn)dlsym(library, "cairo_status");
		has_point_fn hasPoint = (has_point_fn)dlsym(library, "cairo_has_current_point");
		get_width_fn getWidth = (get_width_fn)dlsym(library, "cairo_get_line_width");
		dash_count_fn dashCount = (dash_count_fn)dlsym(library, "cairo_get_dash_count");
		cp_surface_t *surface;
		cp_cairo_t *cr;
		cp_path_t *path;
		unsigned char *data;
		int stride, x, y, bad = 0;
		double px, py, dashes[2] = { 2, 1 };
		cp_matrix_t m;

		CHECK(surfaceCreate && getData && getStride && destroy && surfaceDestroy && status && hasPoint && getWidth && dashCount,
			"the test-only functions");
		surface = surfaceCreate(0 /* CAIRO_FORMAT_ARGB32 */, 16, 16);
		cr = api.create(surface);
		CHECK(api.get_operator(cr) == 2 /* CAIRO_OPERATOR_OVER */, "the default operator");
		api.set_operator(cr, 1 /* SOURCE */);
		CHECK(api.get_operator(cr) == 1, "set_operator");
		api.set_operator(cr, 2);
		api.save(cr);
		api.set_line_width(cr, 3.5);
		CHECK(getWidth(cr) == 3.5, "set_line_width");
		api.restore(cr);
		CHECK(getWidth(cr) == 2.0, "save and restore");
		api.set_dash(cr, dashes, 2, 0.5);
		CHECK(dashCount(cr) == 2, "set_dash");
		api.set_dash(cr, NULL, 0, 0.0);
		CHECK(dashCount(cr) == 0, "set_dash none");
		api.set_line_join(cr, 1);
		api.set_line_cap(cr, 1);
		api.set_fill_rule(cr, 1);
		api.set_antialias(cr, 1);
		api.matrix_init_identity(&m);
		api.set_matrix(cr, &m);
		/* a path, copied and appended */
		api.new_path(cr);
		api.move_to(cr, 1, 1);
		api.line_to(cr, 5, 1);
		api.curve_to(cr, 6, 2, 6, 3, 5, 4);
		api.arc(cr, 8, 8, 2, 0, M_PI);
		api.arc_negative(cr, 8, 8, 2, 0, M_PI);
		api.get_current_point(cr, &px, &py);
		CHECK(fabs(px - 6) < 1e-9 && fabs(py - 8) < 1e-9, "get_current_point %g %g", px, py);
		api.close_path(cr);
		path = api.copy_path(cr);
		api.new_path(cr);
		CHECK(!hasPoint(cr), "new_path");
		api.append_path(cr, path);
		CHECK(hasPoint(cr), "append_path");
		api.path_destroy(path);
		api.new_path(cr);
		/* the rectangle 2,2 4x4 in opaque red, read back */
		api.set_source_rgba(cr, 1, 0, 0, 1);
		api.rectangle(cr, 2, 2, 4, 4);
		api.fill_preserve(cr);
		api.fill(cr);
		api.surface_flush(surface);
		data = getData(surface);
		stride = getStride(surface);
		for (y = 0; y < 16; y++) {
			for (x = 0; x < 16; x++) {
				uint32_t pixel;
				memcpy(&pixel, data + y * stride + x * 4, 4);
				if (pixel != (x >= 2 && x < 6 && y >= 2 && y < 6 ? 0xFFFF0000u : 0u)) {
					bad++;
				}
			}
		}
		CHECK(bad == 0, "the rectangle: %d wrong pixels", bad);
		/* clip, paint, stroke */
		api.rectangle(cr, 10, 10, 2, 2);
		api.clip(cr);
		api.set_source_rgba(cr, 0, 0, 1, 1);
		api.paint(cr);
		api.reset_clip(cr);
		api.move_to(cr, 0, 15);
		api.line_to(cr, 15, 15);
		api.stroke(cr);
		api.surface_flush(surface);
		{
			uint32_t in, out;
			memcpy(&in, data + 10 * stride + 10 * 4, 4);
			memcpy(&out, data + 9 * stride + 9 * 4, 4);
			CHECK(in == 0xFF0000FFu && out == 0, "clip and paint: %08x %08x", in, out);
		}
		CHECK(status(cr) == 0, "status %d", status(cr));

		/* B6 text, with the toy font */
		{
			cp_scaled_font_t *sf;
			cp_glyph_t *glyphs = NULL;
			int nglyphs = 0, st;
			cp_text_extents_t te, ge;
			cp_font_extents_t fe;

			api.set_font_size(cr, 12);
			sf = api.get_scaled_font(cr);
			CHECK(sf != NULL, "get_scaled_font");
			api.set_scaled_font(cr, sf);
			st = api.scaled_font_text_to_glyphs(sf, 0, 0, "Hi \xc3\xa9", 5, &glyphs, &nglyphs, NULL, NULL, NULL);
			CHECK(st == CP_STATUS_SUCCESS && nglyphs == 4 && glyphs, "text_to_glyphs: %d, %d glyphs", st, nglyphs);
			if (st == CP_STATUS_SUCCESS) {
				api.scaled_font_glyph_extents(sf, glyphs, nglyphs, &ge);
				api.scaled_font_text_extents(sf, "Hi \xc3\xa9", &te);
				CHECK(ge.x_advance > 0 && fabs(ge.x_advance - te.x_advance) < 1e-9, "glyph and text extents %g %g",
					ge.x_advance, te.x_advance);
				api.show_glyphs(cr, glyphs, nglyphs);
				api.glyph_free(glyphs);
			}
			api.scaled_font_extents(sf, &fe);
			CHECK(fe.ascent > 0 && fe.height > 0, "font extents %g %g", fe.ascent, fe.height);
			api.new_path(cr);
			api.text_path(cr, "Hi");
			CHECK(hasPoint(cr), "text_path");
			api.new_path(cr);
			if (api.ft_scaled_font_lock_face) {
				cp_ft_face_t face = api.ft_scaled_font_lock_face(sf);
				CHECK(face != NULL, "ft_scaled_font_lock_face (a FreeType toy font)");
				api.ft_scaled_font_unlock_face(sf);
			}
			CHECK(status(cr) == 0, "status after the text %d", status(cr));
		}
		destroy(cr);
		surfaceDestroy(surface);
	}

	if (failures) {
		printf("bindtest FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("bindtest ok: %d checks\n", checks);
	return 0;
}
