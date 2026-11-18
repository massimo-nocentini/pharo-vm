/* glue-admin.c -- the module, the admin primitives and the helpers of the
 * CairoPlugin (src/common/CairoPlugin.c) against the fake VM of fakevm.c.
 *
 * This file #includes CairoPlugin.c (and so the files of src/prims): the plugin's
 * static helpers and state (cp_slot ... cp_new_point, cp_layout, cp_api, the
 * counters) are then in reach of the checks, while the primitives are still
 * looked up as the VM looks them up (fake_primitive: dlsym natively, the rows
 * of CairoPlugin_exports in a builtin plugin).  Linked with fakevm.c,
 * src/binding/cpCairo.c and the SimdUTF engine (su_init at bind).  Built by
 * tests/run-c-tests.sh twice natively (shared and -DSQUEAK_BUILTIN_PLUGIN,
 * clang -O1 ASan and UBSan; the bundled libcairo.so.2, --libcairo) and once
 * for wasm64 (glue-wasm64: builtin, the static bind of pharo_cairo).
 *
 *   A1  module     setInterpreter, the module name, shutdownModule
 *   A2  table      tests/primitives.txt: every primitive found by name, its
 *                  accessor depth, the order of CP_P_* and of the names
 *   A3  arity      every primitive with one argument less and one more:
 *                  #'bad number of arguments', no cairo call, no allocation
 *   A4  unbound    every drawing primitive: #'unsupported operation' before
 *                  any cairo call; primitiveInfo's 7 elements
 *   A5  bind args  every bad argument of primitiveBind (path, probe, layout
 *                  size and each position's range): its code, the loader
 *                  never called, the binding unchanged
 *   A6  bind       nil and NULL probes, an unloadable library, a missing
 *                  required function (named by primitiveInfo), missing
 *                  optional ones (listed), an old cairo, another probe: the
 *                  code, the plugin left unbound, no lookup with a NULL
 *                  module; the good bind, with 29 and 33 elements
 *   A7  state      the layout, the string classes as GC roots (nil after
 *                  unbind, removed by shutdownModule), cp_string
 *   A8  admin      names (57 functions, 69 primitives), counters, reset
 *   A9  forwarders a forwarder at each level of primitiveBind's arguments
 *                  (depth 1): the first call fails before any load, the
 *                  VM's retry binds
 *   A10 memory     every allocation of the admin primitives failing in turn
 *   A11 helpers    cp_slot, cp_address*, cp_bytes (no ExternalAddress, no
 *                  CompiledMethod), cp_cstring (the NUL and Spur's padding),
 *                  cp_matrix, cp_number, cp_int, cp_color (bit-identical to
 *                  Color>>red ...), cp_rect, cp_new_float, cp_new_address,
 *                  cp_new_point, cp_writable_slot
 *
 * Usage: glue-admin [--libcairo PATH] [--primitives tests/primitives.txt]
 * Prints one line per failed check and "glue-admin ok: N checks"; exits 1
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

static FakePrimitiveRow rows[128];
static int nrows;
static const char *libcairo = "libcairo.so.2";
static void *cairoHandle;	/* the test's own dlopen of the same library */
static void *probeCreate, *probePaint;

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

static uint64_t
cairoCalls(void)
{
	uint64_t n = 0;
	int i;

	for (i = 0; i < CP_FN_COUNT; i++) {
		n += cp_cairo_calls[i];
	}
	return n;
}

static int
isString(sqInt oop, const char *s)
{
	return fake_is_object(oop) && fake_class_of(oop) == fakeByteString
		&& fake_size(oop) == strlen(s) && memcmp(fake_body(oop), s, strlen(s)) == 0;
}


/*** The layout and the bind ***/

/* The P12-P15 values (docs/CairoPlugin.md) */
static const sqInt layoutValues[CP_LAYOUT_INTEGERS] = {
	1, 6, 7, 0, 1, 1, 0, 0, 0, 0, 1, 3, 4, 5, 6, 0, 2, 6, 0, 3, 0, 1, 0, 1, 20, 10, 1023, 255, 0
};

static sqInt
newLayout(int withClasses)
{
	sqInt oops[CP_L_COUNT];
	int i;

	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		oops[i] = fake_int(layoutValues[i]);
	}
	oops[CP_L_CLASS_BYTE_STRING] = fakeByteString;
	oops[CP_L_CLASS_BYTE_SYMBOL] = fakeByteSymbol;
	oops[CP_L_CLASS_WIDE_STRING] = fakeWideString;
	oops[CP_L_CLASS_WIDE_SYMBOL] = fakeWideSymbol;
	return fake_array(fakeArray, oops, withClasses ? CP_LAYOUT_SIZE_TEXT : CP_LAYOUT_SIZE_BASE);
}

static sqInt
probeAddress(void *p)
{
	return fake_external_address(p);
}

static FakeAnswer
bindWith(sqInt path, sqInt probe, sqInt layout)
{
	sqInt args[3] = { path, probe, layout };

	return fake_call(prim("primitiveBind"), fakeNil, 3, args);
}

static FakeAnswer
bindGood(int withClasses)
{
	return bindWith(fake_string(libcairo), probeAddress(probeCreate), newLayout(withClasses));
}

static sqInt
info(void)
{
	FakeAnswer a = fake_call(prim("primitiveInfo"), fakeNil, 0, NULL);

	CHECK(a.returned && a.protocolErrors == 0, "primitiveInfo: code %lld %s", (long long)a.code, fake_last_error());
	return a.value;
}

/* the cairo version answered by the info, or -1 when unbound (nil) */
static sqInt
boundVersion(void)
{
	sqInt v = fake_slot(info(), 3);

	return v == fakeNil ? -1 : fake_int_value(v);
}

static int
fakeOldVersion(void)
{
	return 11400;
}


/*** A1 module, A2 table ***/

static void
testModule(struct VirtualMachine *vm)
{
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
#ifdef SQUEAK_BUILTIN_PLUGIN
	CHECK(strcmp(getModuleName(), "CairoPlugin (i)") == 0, "module name %s", getModuleName());
#else
	CHECK(strcmp(getModuleName(), "CairoPlugin (e)") == 0, "module name %s", getModuleName());
#endif
	CHECK(initialiseModule() == 1, "initialiseModule");
}

static void
testTable(void)
{
	int i;

	nrows = fake_read_primitives(CP_PRIMITIVES_TXT, rows, 128);
	CHECK(nrows == 69, "primitives.txt: %d rows (%s)", nrows, CP_PRIMITIVES_TXT);
	CHECK(CP_P_COUNT == 69 && nrows == CP_P_COUNT, "CP_P_COUNT %d", (int)CP_P_COUNT);
	CHECK(CP_FN_COUNT == 57, "CP_FN_COUNT %d", (int)CP_FN_COUNT);
	for (i = 0; i < nrows; i++) {
		CHECK(fake_primitive(rows[i].name) != NULL, "%s not found", rows[i].name);
		CHECK(fake_accessor_depth(rows[i].name) == rows[i].depth, "%s: depth %d, primitives.txt %d",
			rows[i].name, fake_accessor_depth(rows[i].name), rows[i].depth);
		CHECK(i < CP_P_COUNT && strcmp(cp_primitive_names[i], rows[i].name) == 0,
			"CP_P_ %d is %s, primitives.txt %s", i, i < CP_P_COUNT ? cp_primitive_names[i] : "-", rows[i].name);
	}
#ifdef SQUEAK_BUILTIN_PLUGIN
	for (i = 0; CairoPlugin_exports[i][1]; i++) {
		CHECK(strcmp((const char *)CairoPlugin_exports[i][0], "CairoPlugin") == 0, "row %d: module", i);
	}
	CHECK(i == 4 + 69, "CairoPlugin_exports: %d rows", i);
	CHECK(CairoPlugin_exports[i][0] == NULL && CairoPlugin_exports[i][2] == NULL, "exports: the NULL row");
#endif
	CHECK(fake_primitive("primitiveNoSuchThing") == NULL, "a missing primitive is found");
}


/*** A3 arity, A4 unbound ***/

static void
testArity(void)
{
	sqInt args[16];
	int i, k, n;

	for (k = 0; k < 16; k++) {
		args[k] = fakeNil;
	}
	for (i = 0; i < nrows; i++) {
		Primitive p = prim(rows[i].name);
		uint64_t before = cairoCalls();
		for (n = rows[i].nargs - 1; n <= rows[i].nargs + 1; n += 2) {
			FakeAnswer a;
			if (n < 0) {
				continue;
			}
			a = fake_call(p, fakeNil, n, args);
			CHECK(a.code == PrimErrBadNumArgs && a.allocations == 0 && a.protocolErrors == 0,
				"%s with %d arguments: code %lld, %d allocations, %s", rows[i].name, n,
				(long long)a.code, a.allocations, fake_last_error());
		}
		CHECK(cairoCalls() == before, "%s: a cairo call on a bad arity", rows[i].name);
	}
}

static void
testUnbound(void)
{
	sqInt args[16], v;
	int i, k;

	for (k = 0; k < 16; k++) {
		args[k] = fakeNil;
	}
	for (i = 0; i < nrows; i++) {
		FakeAnswer a;
		if (strcmp(rows[i].group, "admin") == 0) {
			continue;
		}
		a = fake_call(prim(rows[i].name), fakeNil, rows[i].nargs, args);
		CHECK(a.code == PrimErrUnsupported && a.allocations == 0 && a.protocolErrors == 0,
			"%s unbound: code %lld %s", rows[i].name, (long long)a.code, fake_last_error());
	}
	CHECK(cairoCalls() == 0, "cairo calls while unbound");
	v = info();
	CHECK(fake_class_of(v) == fakeArray && fake_size(v) == 7, "info: not 7 elements");
	CHECK(fake_slot(v, 0) == fake_int(1), "info 1: protocol");
	CHECK(isString(fake_slot(v, 1), "CairoPlugin 1.0"), "info 2: version");
#ifdef __EMSCRIPTEN__
	CHECK(isString(fake_slot(v, 2), "emscripten"), "info 3: platform");
#else
	CHECK(isString(fake_slot(v, 2), "native"), "info 3: platform");
#endif
	CHECK(fake_slot(v, 3) == fakeNil, "info 4: bound");
#ifdef CP_STATIC_CAIRO
	CHECK(isString(fake_slot(v, 4), "static"), "info 5: binding");
#else
	CHECK(isString(fake_slot(v, 4), "loader"), "info 5: binding");
#endif
	CHECK(fake_slot(v, 5) == fakeNil, "info 6: missing");
	CHECK(fake_class_of(fake_slot(v, 6)) == fakeArray && fake_size(fake_slot(v, 6)) == 0, "info 7: optional");
}


/*** A5 bind arguments ***/

/* bindWith, expecting code, the loader untouched and the binding as it was */
static void
expectBadBind(const char *what, sqInt path, sqInt probe, sqInt layout, sqInt code)
{
	sqInt versionBefore = boundVersion();
	FakeAnswer a;

	fake_loader_reset();
	a = bindWith(path, probe, layout);
	CHECK(a.code == code && a.protocolErrors == 0, "bind (%s): code %lld, expected %lld %s", what,
		(long long)a.code, (long long)code, fake_last_error());
	CHECK(fake_loader_module_calls() == 0 && fake_loader_symbol_calls() == 0, "bind (%s): the loader was called", what);
	CHECK(boundVersion() == versionBefore, "bind (%s): the binding changed", what);
}

static void
testBindArguments(void)
{
	char longPath[300];
	sqInt layout, good, probe;
	int i;

	probe = probeAddress(probeCreate);
	good = fake_string(libcairo);
	/* bound first: a bad argument must leave the binding */
#ifndef CP_STATIC_CAIRO
	{
		FakeAnswer a = bindGood(1);
		CHECK(a.returned && a.code == 0, "the first bind: code %lld %s", (long long)a.code, fake_last_error());
	}
#endif
	memset(longPath, 'a', sizeof longPath);
	expectBadBind("nil path", fakeNil, probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("SmallInteger path", fake_int(3), probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("ByteArray path", fake_bytes(fakeByteArray, "libcairo", 8), probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("ByteSymbol path", fake_bytes(fakeByteSymbol, "libcairo", 8), probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("empty path", fake_string(""), probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("256-byte path", fake_bytes(fakeByteString, longPath, 256), probe, newLayout(1), PrimErrBadArgument);
	expectBadBind("SmallInteger probe", good, fake_int(1), newLayout(1), PrimErrBadArgument);
	expectBadBind("ByteArray probe", good, fake_bytes(fakeByteArray, &probeCreate, 8), newLayout(1), PrimErrBadArgument);
	expectBadBind("4-byte probe", good, fake_bytes(fakeExternalAddress, &probeCreate, 4), newLayout(1), PrimErrBadArgument);
	expectBadBind("nil layout", good, probe, fakeNil, PrimErrBadArgument);
	expectBadBind("WordArray layout", good, probe, fake_new(fakeWordArray, 29), PrimErrBadArgument);
	{
		sqInt oops[34];
		for (i = 0; i < 34; i++) {
			oops[i] = i < CP_LAYOUT_INTEGERS ? fake_int(layoutValues[i]) : fakeByteString;
		}
		expectBadBind("28 elements", good, probe, fake_array(fakeArray, oops, 28), PrimErrBadIndex);
		expectBadBind("30 elements", good, probe, fake_array(fakeArray, oops, 30), PrimErrBadIndex);
		expectBadBind("34 elements", good, probe, fake_array(fakeArray, oops, 34), PrimErrBadIndex);
	}
	/* each position out of its range */
	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		sqInt bad[4];
		int nbad = 0, k;
		char what[64];
		switch (i) {
		case CP_L_LAYOUT_VERSION: bad[nbad++] = fake_int(0); bad[nbad++] = fake_int(2); break;
		case CP_L_COLOR_RED_SHIFT: case CP_L_COLOR_GREEN_SHIFT:
			bad[nbad++] = fake_int(-1); bad[nbad++] = fake_int(63); break;
		case CP_L_COLOR_COMPONENT_MASK: case CP_L_COLOR_ALPHA_MAX:
			bad[nbad++] = fake_int(0); bad[nbad++] = fake_int(((sqInt)1 << 30) + 1); break;
		default: bad[nbad++] = fake_int(-1); bad[nbad++] = fake_int(255); break;
		}
		bad[nbad++] = fakeNil;
		bad[nbad++] = fake_boxed_float(1.0);
		for (k = 0; k < nbad; k++) {
			layout = newLayout(1);
			fake_set_slot(layout, (size_t)i, bad[k]);
			snprintf(what, sizeof what, "layout position %d, bad value %d", i + 1, k);
			expectBadBind(what, good, probe, layout, PrimErrBadIndex);
		}
	}
	/* the limits themselves are accepted (checked by binding below) */
	for (i = CP_LAYOUT_INTEGERS; i < CP_L_COUNT; i++) {
		char what[64];
		layout = newLayout(1);
		fake_set_slot(layout, (size_t)i, fake_int(7));
		snprintf(what, sizeof what, "class position %d a SmallInteger", i + 1);
		expectBadBind(what, good, probe, layout, PrimErrBadIndex);
		layout = newLayout(1);
		fake_set_slot(layout, (size_t)i, fake_string("x"));
		snprintf(what, sizeof what, "class position %d a String", i + 1);
		expectBadBind(what, good, probe, layout, PrimErrBadIndex);
	}
	/* the string classes must be classes of byte (30-31) or 32-bit word
	   (32-33) instances: cp_string reads their instances as such, and a
	   class of pointers instances with named slots (instSpec 3) would make
	   it read past the end of the object */
	{
		sqInt notStringClasses[6];
		int nk = 0, k;

		notStringClasses[nk++] = fakeNil;
		notStringClasses[nk++] = fakeArray;
		notStringClasses[nk++] = fakePoint;
		notStringClasses[nk++] = fake_class("CPFmt3", 3, 4);
		notStringClasses[nk++] = fake_new(fakeArray, 3);	/* a pointers object of 3 slots, the third nil */
		notStringClasses[nk++] = fake_class("CPFmt9", 9, 0);	/* 64-bit words */
		for (i = CP_LAYOUT_INTEGERS; i < CP_L_COUNT; i++) {
			for (k = 0; k < nk; k++) {
				char what[64];
				layout = newLayout(1);
				fake_set_slot(layout, (size_t)i, notStringClasses[k]);
				snprintf(what, sizeof what, "class position %d, not a string class %d", i + 1, k);
				expectBadBind(what, good, probe, layout, PrimErrBadIndex);
			}
			/* a byte class where a word class is expected and the reverse */
			{
				char what[64];
				layout = newLayout(1);
				fake_set_slot(layout, (size_t)i, i < CP_LAYOUT_INTEGERS + 2 ? fakeWideString : fakeByteString);
				snprintf(what, sizeof what, "class position %d, the other width", i + 1);
				expectBadBind(what, good, probe, layout, PrimErrBadIndex);
			}
		}
	}
}


/*** A6 bind ***/

static void
expectFailedBind(const char *what, sqInt path, sqInt probe, sqInt code)
{
	FakeAnswer a = bindWith(path, probe, newLayout(1));

	CHECK(a.code == code, "bind (%s): code %lld, expected %lld %s", what, (long long)a.code, (long long)code, fake_last_error());
	CHECK(boundVersion() == -1, "bind (%s): left bound", what);
	CHECK(!cp_bound && !cp_have_string_classes, "bind (%s): state bound", what);
	CHECK(fake_loader_null_module_calls() == 0, "bind (%s): a lookup with a NULL module", what);
}

static void
testBind(void)
{
	FakeAnswer a;
	sqInt v, good = fake_string(libcairo);
	int i;

	/* nil and NULL probes: not found, nothing loaded */
	fake_loader_reset();
	expectFailedBind("nil probe", good, fakeNil, PrimErrNotFound);
	CHECK(fake_loader_module_calls() == 0 && fake_loader_symbol_calls() == 0, "nil probe: the loader was called");
	expectFailedBind("NULL probe", good, probeAddress(NULL), PrimErrNotFound);
	CHECK(fake_loader_module_calls() == 0 && fake_loader_symbol_calls() == 0, "NULL probe: the loader was called");
#ifndef CP_STATIC_CAIRO
	/* the module does not load: no symbol lookup at all (R9) */
	fake_loader_reset();
	fake_loader_fail_modules(1);
	expectFailedBind("unloadable", good, probeAddress(probeCreate), PrimErrNotFound);
	CHECK(fake_loader_module_calls() == 1 && fake_loader_symbol_calls() == 0,
		"unloadable: %d module and %d symbol calls", fake_loader_module_calls(), fake_loader_symbol_calls());
	CHECK(strcmp(fake_loader_last_module(), libcairo) == 0, "the module loaded is %s", fake_loader_last_module());
	fake_loader_reset();
	expectFailedBind("no such file", fake_string("/no/such/dir/libcairo.so.2"), probeAddress(probeCreate), PrimErrNotFound);
	/* a missing required function, named by primitiveInfo */
	fake_loader_reset();
	fake_loader_hide("cairo_rectangle");
	expectFailedBind("cairo_rectangle missing", good, probeAddress(probeCreate), PrimErrNotFound);
	CHECK(isString(fake_slot(info(), 5), "cairo_rectangle"), "info 6: not the missing cairo_rectangle");
	fake_loader_reset();
	fake_loader_hide("cairo_create");
	expectFailedBind("cairo_create missing", good, probeAddress(probeCreate), PrimErrNotFound);
	CHECK(isString(fake_slot(info(), 5), "cairo_create"), "info 6: not the missing cairo_create");
	fake_loader_reset();
	fake_loader_hide("cairo_version");
	expectFailedBind("cairo_version missing", good, probeAddress(probeCreate), PrimErrNotFound);
	CHECK(isString(fake_slot(info(), 5), "cairo_version"), "info 6: not the missing cairo_version");
	/* an old cairo */
	fake_loader_reset();
	fake_loader_override("cairo_version", (void *)fakeOldVersion);
	expectFailedBind("cairo 1.14", good, probeAddress(probeCreate), PrimErrUnsupported);
	CHECK(fake_slot(info(), 5) == fakeNil, "info 6: a missing function after an old cairo");
	/* the optional functions missing: bound, listed */
	fake_loader_reset();
	fake_loader_hide("cairo_ft_scaled_font_lock_face");
	fake_loader_hide("cairo_ft_scaled_font_unlock_face");
	a = bindGood(1);
	CHECK(a.returned && fake_is_int(a.value) && fake_int_value(a.value) >= CP_CAIRO_VERSION_MIN,
		"bind without the ft functions: code %lld %s", (long long)a.code, fake_last_error());
	v = fake_slot(info(), 6);
	CHECK(fake_size(v) == 2 && isString(fake_slot(v, 0), "cairo_ft_scaled_font_lock_face")
		&& isString(fake_slot(v, 1), "cairo_ft_scaled_font_unlock_face"), "info 7: the missing optional functions");
	CHECK(!CP_HAVE(ft_scaled_font_lock_face) && !CP_HAVE(ft_scaled_font_unlock_face) && CP_HAVE(new_path),
		"CP_HAVE without the ft functions");
	fake_loader_reset();
#endif
	/* another function as the probe: another libcairo */
	expectFailedBind("cairo_paint as probe", good, probeAddress(probePaint), PrimErrInappropriate);
	/* the good binds */
	fake_loader_reset();
	a = bindGood(0);
	CHECK(a.returned && a.protocolErrors == 0 && fake_is_int(a.value), "bind 29: code %lld %s", (long long)a.code, fake_last_error());
#ifdef CP_STATIC_CAIRO
	CHECK(fake_int_value(a.value) == 11804, "static bind: cairo %lld", (long long)fake_int_value(a.value));
#else
	CHECK(fake_int_value(a.value) >= CP_CAIRO_VERSION_MIN, "bind: cairo %lld", (long long)fake_int_value(a.value));
	CHECK(fake_loader_module_calls() == 1 && fake_loader_symbol_calls() == CP_FN_COUNT,
		"bind: %d module and %d symbol calls", fake_loader_module_calls(), fake_loader_symbol_calls());
#endif
	CHECK(cp_bound && !cp_have_string_classes, "bind 29: state");
	CHECK(boundVersion() == fake_int_value(a.value), "info 4: the version");
	v = info();
	CHECK(fake_slot(v, 5) == fakeNil && fake_size(fake_slot(v, 6)) == 0, "info 6, 7 after a good bind");
	for (i = 0; i < CP_FN_COUNT; i++) {
		CHECK(cp_function_bound(i), "%s not bound", cp_cairo_function_names[i]);
	}
	CHECK((void *)cp_api.create == probeCreate, "cp_api.create is not the probe");
	CHECK(cairoCalls() == 0, "the bind counted a cairo call");
	a = bindGood(1);
	CHECK(a.returned && cp_bound && cp_have_string_classes, "bind 33: code %lld", (long long)a.code);
	CHECK(fake_loader_null_module_calls() == 0, "a lookup with a NULL module");
}


/*** A7 state ***/

static void
testState(void)
{
	FakeAnswer a;
	int i;
	const void *base;
	size_t units;
	int words;
	uint32_t wide[3] = { 0x41, 0x3BB, 0x1F600 };
	sqInt wideString = fake_words(fakeWideString, wide, 3);

	a = bindGood(1);
	CHECK(a.returned, "bind 33");
	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		CHECK(cp_layout[i] == layoutValues[i], "cp_layout[%d] = %lld", i, (long long)cp_layout[i]);
	}
	CHECK(fake_gc_root_count() == 4, "%d GC roots", fake_gc_root_count());
	CHECK(fake_gc_root_value(0) == fakeByteString || fake_gc_root_value(3) == fakeByteString
		|| fake_gc_root_value(1) == fakeByteString || fake_gc_root_value(2) == fakeByteString, "the GC roots hold the classes");
	for (i = 0; i < 4; i++) {
		CHECK(*(&cp_string_classes[0] + i) == (i == 0 ? fakeByteString : i == 1 ? fakeByteSymbol : i == 2 ? fakeWideString : fakeWideSymbol),
			"cp_string_classes[%d]", i);
	}
	CHECK(cp_string(fake_string("abc"), &base, &units, &words) && units == 3 && words == 0
		&& memcmp(base, "abc", 3) == 0, "cp_string ByteString");
	CHECK(cp_string(fake_bytes(fakeByteSymbol, "ab", 2), &base, &units, &words) && units == 2 && words == 0, "cp_string ByteSymbol");
	CHECK(cp_string(wideString, &base, &units, &words) && units == 3 && words == 1
		&& ((const uint32_t *)base)[2] == 0x1F600, "cp_string WideString");
	CHECK(cp_string(fake_words(fakeWideSymbol, wide, 2), &base, &units, &words) && units == 2 && words == 1, "cp_string WideSymbol");
	CHECK(!cp_string(fake_bytes(fakeByteArray, "ab", 2), &base, &units, &words), "cp_string ByteArray");
	CHECK(!cp_string(fake_words(fakeWordArray, wide, 3), &base, &units, &words), "cp_string WordArray");
	CHECK(!cp_string(fake_int(3), &base, &units, &words), "cp_string SmallInteger");
	CHECK(!cp_string(fake_forwarder_to(fake_string("x")), &base, &units, &words), "cp_string forwarder");
	/* a 29-element bind: no string classes */
	a = bindGood(0);
	CHECK(a.returned && !cp_have_string_classes, "bind 29 after 33");
	CHECK(!cp_string(fake_string("abc"), &base, &units, &words), "cp_string without the classes");
	CHECK(fake_gc_root_count() == 4 && fake_gc_root_value(0) == fakeNil, "the GC roots after a 29-element bind");
	/* unbind */
	a = bindGood(1);
	a = fake_call(prim("primitiveUnbind"), fake_int(5), 0, NULL);
	CHECK(a.returned && a.value == fake_int(5), "unbind answers the receiver");
	CHECK(!cp_bound && boundVersion() == -1, "unbind: bound");
	for (i = 0; i < fake_gc_root_count(); i++) {
		CHECK(fake_gc_root_value(i) == fakeNil, "GC root %d after unbind", i);
	}
	CHECK(!cp_string(fake_string("abc"), &base, &units, &words), "cp_string after unbind");
	/* shutdownModule: unbound, roots removed */
	a = bindGood(1);
	CHECK(shutdownModule() == 1 && !cp_bound, "shutdownModule");
	CHECK(fake_gc_root_count() == 0, "%d GC roots after shutdownModule", fake_gc_root_count());
	a = bindGood(1);
	CHECK(a.returned && fake_gc_root_count() == 4, "bind after shutdownModule: %d roots", fake_gc_root_count());
}


/*** A8 admin ***/

static void
testAdmin(void)
{
	FakeAnswer a;
	sqInt v, counts;
	int i;

	a = fake_call(prim("primitiveFunctionNames"), fakeNil, 0, NULL);
	CHECK(a.returned && a.protocolErrors == 0, "names %s", fake_last_error());
	v = a.value;
	CHECK(fake_size(v) == 2 && fake_size(fake_slot(v, 0)) == 57 && fake_size(fake_slot(v, 1)) == 69, "names: sizes");
	for (i = 0; i < CP_FN_COUNT; i++) {
		CHECK(isString(fake_slot(fake_slot(v, 0), (size_t)i), cp_cairo_function_names[i]), "function name %d", i);
		CHECK(strncmp(cp_cairo_function_names[i], "cairo_", 6) == 0, "function name %d prefix", i);
	}
	CHECK(isString(fake_slot(fake_slot(v, 0), 0), "cairo_version") && isString(fake_slot(fake_slot(v, 0), 56), "cairo_ft_scaled_font_unlock_face"),
		"function names: first and last");
	for (i = 0; i < nrows; i++) {
		CHECK(isString(fake_slot(fake_slot(v, 1), (size_t)i), rows[i].name), "primitive name %d", i);
	}
	a = fake_call(prim("primitiveResetCounters"), fake_int(9), 0, NULL);
	CHECK(a.returned && a.value == fake_int(9), "reset answers the receiver");
	cp_cairo_calls[CP_FN_fill] = 5;
	info();
	info();
	a = fake_call(prim("primitiveCounters"), fakeNil, 0, NULL);
	CHECK(a.returned && a.protocolErrors == 0, "counters %s", fake_last_error());
	counts = a.value;
	CHECK(fake_size(counts) == 2 && fake_size(fake_slot(counts, 0)) == 57 && fake_size(fake_slot(counts, 1)) == 69, "counters: sizes");
	CHECK(fake_slot(fake_slot(counts, 0), CP_FN_fill) == fake_int(5), "counter of cairo_fill");
	CHECK(fake_slot(fake_slot(counts, 1), CP_P_Info) == fake_int(2), "counter of primitiveInfo: %lld",
		(long long)fake_int_value(fake_slot(fake_slot(counts, 1), CP_P_Info)));
	CHECK(fake_slot(fake_slot(counts, 1), CP_P_Counters) == fake_int(1), "counter of primitiveCounters");
	a = fake_call(prim("primitiveResetCounters"), fakeNil, 0, NULL);
	CHECK(cairoCalls() == 0 && cp_prim_calls[CP_P_Info] == 0, "reset");
}


/*** A9 forwarders ***/

static void
testForwarders(void)
{
	sqInt path, probe, layout;
	FakeAnswer a;
	int level;

	/* level 0: each stack value a forwarder; level 1: an element of the
	   layout (an integer position, a class position) a forwarder */
	for (level = 0; level < 6; level++) {
		path = fake_string(libcairo);
		probe = probeAddress(probeCreate);
		layout = newLayout(1);
		switch (level) {
		case 0: path = fake_forwarder_to(path); break;
		case 1: probe = fake_forwarder_to(probe); break;
		case 2: layout = fake_forwarder_to(layout); break;
		case 3: fake_set_slot(layout, 2, fake_forwarder_to(fake_int(7))); break;	/* (an immediate behind: odd, but followed) */
		case 4: fake_set_slot(layout, CP_L_CLASS_WIDE_STRING, fake_forwarder_to(fakeWideString)); break;
		case 5: fake_set_slot(layout, CP_L_CLASS_BYTE_STRING, fake_forwarder_to(fakeByteString)); break;
		}
		fake_loader_reset();
		{
			sqInt args[3] = { path, probe, layout };
			a = fake_call_retry(prim("primitiveBind"), fake_accessor_depth("primitiveBind"), fakeNil, 3, args);
		}
		CHECK(a.retried && a.firstCode != 0 && a.returned && a.protocolErrors == 0,
			"forwarder at level %d: retried %d, first code %lld, code %lld %s", level, a.retried,
			(long long)a.firstCode, (long long)a.code, fake_last_error());
#ifndef CP_STATIC_CAIRO
		CHECK(fake_loader_module_calls() == 1, "forwarder at level %d: %d module loads", level, fake_loader_module_calls());
#endif
		CHECK(cp_bound && cp_have_string_classes, "forwarder at level %d: not bound", level);
		CHECK(cp_string_classes[2] == fakeWideString && cp_string_classes[0] == fakeByteString,
			"forwarder at level %d: a class root holds a forwarder", level);
	}
}


/*** A10 memory ***/

static void
testMemory(void)
{
	static const char *names[] = { "primitiveInfo", "primitiveCounters", "primitiveFunctionNames" };
	int n, k;

	for (k = 0; k < 3; k++) {
		for (n = 0; n < 200; n++) {
			FakeAnswer a;
			fake_set_budget(n, PrimErrNoMemory);
			a = fake_call(prim(names[k]), fakeNil, 0, NULL);
			fake_set_budget(-1, PrimErrNoMemory);
			if (a.returned) {
				break;
			}
			CHECK(a.code == PrimErrNoMemory && a.protocolErrors == 0, "%s with %d allocations: code %lld %s",
				names[k], n, (long long)a.code, fake_last_error());
		}
		CHECK(n > 0 && n < 200, "%s: %d allocations", names[k], n);
	}
}


/*** A11 helpers ***/

static sqInt helperOop;
static double helperX, helperY;
static int helperKind;

/* A primitive that runs an allocation helper (allocation needs a call) */
static sqInt
helperPrimitive(void)
{
	sqInt oop = 0;

	switch (helperKind) {
	case 0: oop = cp_new_float(helperX); break;
	case 1: oop = cp_new_address((void *)(uintptr_t)0x1234); break;
	case 2: oop = cp_new_point(helperX, helperY); break;
	}
	return oop ? cp_return(oop) : 0;
}

static FakeAnswer
runHelper(int kind, double x, double y)
{
	helperKind = kind;
	helperX = x;
	helperY = y;
	return fake_call(helperPrimitive, fakeNil, 0, NULL);
}

static void
testHelpers(void)
{
	sqInt canvas, ea, ba, out, m;
	void *p;
	const char *s;
	size_t n;
	double d, rgba[4], xywh[4];
	int i;
	cp_matrix_t *mp, matrix = { 1, 2, 3, 4, 5, 6 };
	FakeAnswer a;
	static const unsigned char eightNoNul[8] = { 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h' };
	uint32_t word = 7;

	a = bindGood(1);
	CHECK(a.returned, "bind for the helpers");
	/* cp_slot */
	canvas = fake_new(fakeAthensCairoCanvas, 0);
	fake_set_slot(canvas, 6, fake_int(42));
	CHECK(cp_slot(canvas, CP_L_CANVAS_HANDLE, &out) && out == fake_int(42), "cp_slot");
	CHECK(!cp_slot(fake_new(fakePoint, 0), CP_L_CANVAS_HANDLE, &out), "cp_slot: too short");
	CHECK(!cp_slot(fake_int(1), CP_L_CANVAS_HANDLE, &out), "cp_slot: immediate");
	CHECK(!cp_slot(fake_string("0123456789abcdef"), CP_L_CANVAS_HANDLE, &out), "cp_slot: bytes");
	CHECK(!cp_slot(fake_forwarder_to(canvas), CP_L_CANVAS_HANDLE, &out), "cp_slot: forwarder");
	CHECK(cp_writable_slot(canvas, CP_L_CANVAS_HANDLE), "cp_writable_slot");
	fake_set_immutable(canvas, 1);
	CHECK(!cp_writable_slot(canvas, CP_L_CANVAS_HANDLE), "cp_writable_slot: read-only");
	fake_set_immutable(canvas, 0);
	/* cp_address and friends */
	ea = fake_external_address((void *)(uintptr_t)0x10);
	CHECK(cp_address(ea, &p) && p == (void *)(uintptr_t)0x10, "cp_address");
	CHECK(!cp_address(fake_external_address(NULL), &p), "cp_address: NULL");
	CHECK(cp_address_or_null(fake_external_address(NULL), &p) && p == NULL, "cp_address_or_null: NULL");
	CHECK(!cp_address(fake_bytes(fakeByteArray, &p, 8), &p), "cp_address: a ByteArray");
	CHECK(!cp_address(fake_bytes(fakeExternalAddress, &p, 4), &p), "cp_address: 4 bytes");
	CHECK(!cp_address(fakeNil, &p) && !cp_address(fake_int(8), &p), "cp_address: nil, SmallInteger");
	CHECK(!cp_address(fake_forwarder_to(ea), &p), "cp_address: forwarder");
	CHECK(cp_address_out(ea, &p) && p == fake_body(ea), "cp_address_out");
	CHECK(cp_address_out(fake_external_address(NULL), &p), "cp_address_out: NULL allowed");
	fake_set_immutable(ea, 1);
	CHECK(!cp_address_out(ea, &p), "cp_address_out: read-only");
	CHECK(cp_address(ea, &p), "cp_address: read-only is readable");
	fake_set_immutable(ea, 0);
	fake_set_slot(canvas, 6, ea);
	CHECK(cp_handle(canvas, CP_L_CANVAS_HANDLE, &p) && p == (void *)(uintptr_t)0x10, "cp_handle");
	{
		cp_cairo_t *cr;
		CHECK(cp_canvas_cr(canvas, &cr) && (void *)cr == (void *)(uintptr_t)0x10, "cp_canvas_cr");
		fake_set_slot(canvas, 6, fake_external_address(NULL));
		CHECK(!cp_canvas_cr(canvas, &cr), "cp_canvas_cr: NULL handle");
		fake_set_slot(canvas, 6, fakeNil);
		CHECK(!cp_canvas_cr(canvas, &cr), "cp_canvas_cr: nil handle");
	}
	/* cp_bytes */
	ba = fake_bytes(fakeByteArray, "abcdef", 6);
	CHECK(cp_bytes(ba, 6, 1, &p, &n) && n == 6 && p == fake_body(ba), "cp_bytes");
	CHECK(!cp_bytes(ba, 7, 0, &p, &n), "cp_bytes: min");
	CHECK(!cp_bytes(ea, 0, 0, &p, &n), "cp_bytes: an ExternalAddress");
	CHECK(!cp_bytes(fake_compiled_method(16), 0, 0, &p, &n), "cp_bytes: a CompiledMethod");
	CHECK(!cp_bytes(fake_words(fakeWordArray, &word, 1), 0, 0, &p, &n), "cp_bytes: words");
	CHECK(!cp_bytes(fake_forwarder_to(ba), 0, 0, &p, &n), "cp_bytes: forwarder");
	fake_set_immutable(ba, 1);
	CHECK(!cp_bytes(ba, 0, 1, &p, &n) && cp_bytes(ba, 0, 0, &p, &n), "cp_bytes: read-only");
	fake_set_immutable(ba, 0);
	/* cp_cstring: a NUL inside, in the padding, none */
	CHECK(cp_cstring(fake_bytes(fakeByteString, "ab\0cd", 5), &s) && strcmp(s, "ab") == 0, "cp_cstring: NUL inside");
	CHECK(cp_cstring(fake_string("hello"), &s) && strcmp(s, "hello") == 0, "cp_cstring: the padding");
	CHECK(cp_cstring(fake_bytes(fakeByteArray, "1234567", 7), &s) && strcmp(s, "1234567") == 0, "cp_cstring: 7 bytes");
	CHECK(!cp_cstring(fake_bytes(fakeByteString, eightNoNul, 8), &s), "cp_cstring: 8 bytes, no NUL");
	CHECK(!cp_cstring(fake_string(""), &s), "cp_cstring: empty");
	CHECK(cp_cstring(fake_bytes(fakeByteString, "12345678\0", 9), &s) && strlen(s) == 8, "cp_cstring: 9 bytes");
	CHECK(!cp_cstring(fake_external_address((void *)"x"), &s), "cp_cstring: an ExternalAddress");
	/* cp_matrix */
	m = fake_new(fakeAthensCairoMatrix, 0);
	fake_set_slot(m, 0, fake_bytes(fakeByteArray, &matrix, 48));
	CHECK(cp_matrix(m, 1, &mp) && mp->xx == 1 && mp->y0 == 6 && (void *)mp == fake_body(fake_slot(m, 0)), "cp_matrix: bytes");
	fake_set_immutable(fake_slot(m, 0), 1);
	CHECK(!cp_matrix(m, 1, &mp) && cp_matrix(m, 0, &mp), "cp_matrix: read-only bytes");
	fake_set_slot(m, 0, fake_bytes(fakeByteArray, &matrix, 47));
	CHECK(!cp_matrix(m, 0, &mp), "cp_matrix: 47 bytes");
	fake_set_slot(m, 0, fake_new(fakeByteArray, 49));
	CHECK(!cp_matrix(m, 0, &mp), "cp_matrix: 49 bytes");
	fake_set_slot(m, 0, fake_external_address(&matrix));
	CHECK(cp_matrix(m, 1, &mp) && mp == &matrix, "cp_matrix: an ExternalAddress");
	fake_set_immutable(fake_slot(m, 0), 1);
	CHECK(cp_matrix(m, 1, &mp) && mp == &matrix, "cp_matrix: a read-only ExternalAddress (as the FFI)");
	fake_set_slot(m, 0, fake_external_address(NULL));
	CHECK(!cp_matrix(m, 0, &mp), "cp_matrix: NULL");
	fake_set_slot(m, 0, fakeNil);
	CHECK(!cp_matrix(m, 0, &mp), "cp_matrix: nil");
	/* cp_number, cp_int */
	CHECK(cp_number(fake_int(-3), &d) && d == -3.0, "cp_number: SmallInteger");
	CHECK(cp_number(fake_int(((sqInt)1 << 60) - 1), &d) && d == (double)(((sqInt)1 << 60) - 1), "cp_number: big SmallInteger");
	CHECK(cp_number(fake_small_float(2.5), &d) && d == 2.5, "cp_number: SmallFloat64");
	CHECK(cp_number(fake_boxed_float(1e300), &d) && d == 1e300, "cp_number: BoxedFloat64");
	CHECK(!cp_number(fake_new(fakeFraction, 0), &d) && !cp_number(fakeNil, &d), "cp_number: Fraction, nil");
	CHECK(!cp_number(fake_bytes(fakeLargePositiveInteger, "\1\0\0\0\0\0\0\0\1", 9), &d), "cp_number: LargeInteger");
	CHECK(cp_int(fake_int(INT_MAX), &i) && i == INT_MAX && cp_int(fake_int(INT_MIN), &i) && i == INT_MIN, "cp_int");
	CHECK(!cp_int(fake_int((sqInt)INT_MAX + 1), &i) && !cp_int(fake_int((sqInt)INT_MIN - 1), &i), "cp_int: range");
	CHECK(!cp_int(fake_small_float(1.0), &i), "cp_int: Float");
	/* cp_color: as Color>>red ... alpha */
	{
		sqInt rgb = ((sqInt)1023 << 20) | ((sqInt)511 << 10) | 7;
		CHECK(cp_color(fake_color(fake_int(rgb), fake_int(128)), rgba), "cp_color");
		CHECK(rgba[0] == 1023.0 / 1023.0 && rgba[1] == 511.0 / 1023.0 && rgba[2] == 7.0 / 1023.0 && rgba[3] == 128.0 / 255.0,
			"cp_color: %.17g %.17g %.17g %.17g", rgba[0], rgba[1], rgba[2], rgba[3]);
		CHECK(!cp_color(fake_color(fake_int(rgb), fake_int(256)), rgba), "cp_color: alpha 256");
		CHECK(!cp_color(fake_color(fake_int(rgb), fake_int(-1)), rgba), "cp_color: alpha -1");
		CHECK(!cp_color(fake_color(fake_int(-1), fake_int(1)), rgba), "cp_color: rgb -1");
		CHECK(!cp_color(fake_color(fakeNil, fake_int(1)), rgba), "cp_color: rgb nil");
		CHECK(!cp_color(fake_color(fake_int(rgb), fake_small_float(1.0)), rgba), "cp_color: alpha Float");
		CHECK(!cp_color(fake_point(fake_int(1), fake_int(2)), rgba), "cp_color: a Point");
	}
	/* cp_rect */
	{
		sqInt big = ((sqInt)1 << 59);
		CHECK(cp_rect(fake_rectangle(fake_point(fake_int(1), fake_int(2)), fake_point(fake_int(11), fake_int(22))), xywh)
			&& xywh[0] == 1 && xywh[1] == 2 && xywh[2] == 10 && xywh[3] == 20, "cp_rect: integers");
		CHECK(cp_rect(fake_rectangle(fake_point(fake_int(-big), fake_int(0)), fake_point(fake_int(big - 1), fake_int(1))), xywh)
			&& xywh[2] == (double)(2 * big - 1), "cp_rect: exact integer width");
		CHECK(cp_rect(fake_rectangle(fake_point(fake_small_float(0.5), fake_int(2)), fake_point(fake_int(3), fake_boxed_float(1e300))), xywh)
			&& xywh[0] == 0.5 && xywh[2] == 2.5 && xywh[3] == 1e300 - 2.0, "cp_rect: mixed");
		CHECK(!cp_rect(fake_rectangle(fake_point(fake_new(fakeFraction, 0), fake_int(2)), fake_point(fake_int(3), fake_int(4))), xywh),
			"cp_rect: a Fraction");
		CHECK(!cp_rect(fake_rectangle(fakeNil, fake_point(fake_int(3), fake_int(4))), xywh), "cp_rect: nil origin");
	}
	/* the allocation helpers */
	a = runHelper(0, 1.5, 0);
	CHECK(a.returned && (a.value & 7) == 4 && fake_float_value(a.value) == 1.5 && a.allocations == 0, "cp_new_float: small");
	a = runHelper(0, 1e300, 0);
	CHECK(a.returned && fake_class_of(a.value) == fakeBoxedFloat && fake_float_value(a.value) == 1e300, "cp_new_float: boxed");
	a = runHelper(0, -0.0, 0);
	CHECK(a.returned && (a.value & 7) == 4 && signbit(fake_float_value(a.value)), "cp_new_float: -0.0 is a SmallFloat64");
	a = runHelper(1, 0, 0);
	CHECK(a.returned && fake_class_of(a.value) == fakeExternalAddress && fake_size(a.value) == 8
		&& fake_address_value(a.value) == (void *)(uintptr_t)0x1234, "cp_new_address");
	a = runHelper(2, 3.0, 1e300);
	CHECK(a.returned && a.protocolErrors == 0 && fake_class_of(a.value) == fakePoint
		&& fake_float_value(fake_slot(a.value, 0)) == 3.0 && fake_float_value(fake_slot(a.value, 1)) == 1e300, "cp_new_point %s", fake_last_error());
	fake_set_budget(0, PrimErrNoMemory);
	a = runHelper(2, 3.0, 4.0);
	fake_set_budget(-1, PrimErrNoMemory);
	CHECK(a.code == PrimErrNoMemory && a.protocolErrors == 0, "cp_new_point without memory");
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
#ifdef CP_STATIC_CAIRO
	probeCreate = (void *)cairo_create;
	probePaint = (void *)cairo_paint;
	(void)cairoHandle;
#else
	cairoHandle = dlopen(libcairo, RTLD_NOW | RTLD_LOCAL);
	if (!cairoHandle) {
		printf("FAILED: dlopen %s: %s\n", libcairo, dlerror());
		return 1;
	}
	probeCreate = dlsym(cairoHandle, "cairo_create");
	probePaint = dlsym(cairoHandle, "cairo_paint");
#endif
	testModule(vm);
	testTable();
	testArity();
	testUnbound();
	testBindArguments();
	testBind();
	testState();
	testAdmin();
	testForwarders();
	testMemory();
	testHelpers();
	shutdownModule();
	fake_free_all();
	if (failures) {
		printf("glue-admin FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("glue-admin ok: %d checks\n", checks);
	return 0;
}
