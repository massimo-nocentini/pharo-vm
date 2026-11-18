/* CairoPlugin.c -- the VM side of the CairoPlugin: Athens drawing through
 * cairo without the FFI's callout, for the package VM-Plugins-Primitives-Cairo
 * of the submodule plugins/UtilsPlugin/vm-plugins-primitives.st, which
 * installs plugin-first versions of the hot Athens methods (each falls back to
 * its original FFI method when its primitive fails).  docs/CairoPlugin.md is
 * the contract; tests/primitives.txt the list of the primitives.
 *
 * This file holds the module functions, the helpers every primitive uses
 * (validation, allocation, the call and success counters), the admin
 * primitives (info, bind, unbind, counters, names, reset), and, through
 * #include, the drawing primitives of the directory src/prims (basic.c: the one-for-one
 * primitives; shape.c: the shape-level ones; text.c: drawString:), so that in
 * a builtin plugin every primitive is static in this one translation unit;
 * the files of src/prims are never compiled on their own.  Then the exports table and
 * the *AccessorDepth variables of all of them.
 *
 * cairo is reached through the function table of cpCairo.h (cp_api), filled
 * by primitiveBind: natively from the library path the image's FFI loads
 * (CairoLibrary uniqueInstance libraryName), through the VM's own module
 * loader (ioLoadModuleOfLength, ioLoadSymbolOfLengthFromModule: the code path
 * and module cache of the FFI's primitiveLoadSymbolFromModule, hence the same
 * libcairo); under Emscripten (CP_STATIC_CAIRO) statically from pharo_cairo.
 * Until a bind every drawing primitive fails with PrimErrUnsupported, and the
 * image's FFI methods run.
 *
 * Natively it is the shared library libCairoPlugin.so, whose module
 * functions, primitives and *AccessorDepth variables the VM finds with dlsym.
 * In the Emscripten VM it is a builtin plugin (SQUEAK_BUILTIN_PLUGIN): every
 * function is static and the VM finds them through CairoPlugin_exports, whose
 * rows cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).
 *
 * Rules for every primitive (docs/CairoPlugin.md; the R lane of
 * tests/run-c-tests.sh checks the static ones):
 *   1. order: argument count (CP_ARGS), bound (CP_BOUND), receiver chain,
 *      arguments, allocation of the answer when possible, cairo calls,
 *      answer; no failure after the first cairo call, except the allocation
 *      of the answer of a pure getter (the VM's retry repeats a call without
 *      side effect);
 *   2. every cairo call is CP_CALL(name)(args), which counts it; every
 *      successful primitive does CP_PRIM_DONE(Name) just before answering;
 *   3. answers mirror the FFI methods: a void callout answers the receiver
 *      (cp_return_receiver), an int callout a SmallInteger;
 *   4. no printing, no exit, no global state but the table, the layout, the
 *      counters and the bind record;
 *   5. pointers are validated (the helpers below); enum values, coordinates
 *      and sizes are passed to cairo as the FFI passes them.
 *
 * Validation helpers answer 1 or 0 and never set the failure code: the
 * caller fails with the code of its contract (cp_fail).  Allocation helpers
 * answer the new object, or 0 when the VM failed the allocation (it has set
 * PrimErrNoMemory, and retries the primitive after a GC).
 *
 * Allocation: only cp_instantiate (instantiateClassindexableSize, which falls
 * back to old space when eden is full), and the SmallFloat64 immediates of
 * cp_new_float (floatObjectOf, which never allocates for them): the proxy's
 * eden-only allocators (floatObjectOf of a boxed value, stringForCString,
 * clone, signed64BitIntegerFor) abort the VM when eden is full.  Spur moves
 * nothing during a primitive's allocation, so oops and body pointers stay
 * valid for the whole primitive.  Every pointer store goes through
 * storePointerofObjectwithValue (remembered-set aware); an object that will
 * be written is validated (pointers, size, not read-only) before any
 * allocation and before the first cairo call.
 *
 * Forwarders: a forwarder is never of the kind expected (not pointers, not
 * bytes, not of class ExternalAddress), so a primitive meeting one fails; the
 * VM then follows the forwarders of the stack values to the primitive's
 * accessor depth and calls it again. */

#include <limits.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>		/* malloc and free of src/prims/shape.c and text.c */
#include <string.h>

/* first: cairo-ft.h (Emscripten) includes <stdio.h>, which the
   "#define fseek fseeko" of sqPlatformSpecific.h would break */
#include "cpCairo.h"
#include "cpLayout.h"
#include "suEngine.h"			/* the SimdUTF engine: UTF-8 of the text primitive */

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

#if defined(__GNUC__) || defined(__clang__)
# define CP_HELPER static __attribute__((unused))
#else
# define CP_HELPER static
#endif

#define CP_PROTOCOL 1
#define CP_PLUGIN_VERSION "CairoPlugin 1.0"
#ifdef __EMSCRIPTEN__
# define CP_PLATFORM "emscripten"
#else
# define CP_PLATFORM "native"
#endif
#ifdef CP_STATIC_CAIRO
# define CP_BINDING "static"
#else
# define CP_BINDING "loader"
#endif
#define CP_SMALLINTEGER_MAX ((((sqInt)1) << 60) - 1)

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"CairoPlugin (i)";
#else
	"CairoPlugin (e)";
#endif


/*** The primitives (tests/primitives.txt, in its order) ***/

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) setInterpreter(struct VirtualMachine *anInterpreter);
EXPORT(sqInt) shutdownModule(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveBind(void);
EXPORT(sqInt) primitiveUnbind(void);
EXPORT(sqInt) primitiveCounters(void);
EXPORT(sqInt) primitiveFunctionNames(void);
EXPORT(sqInt) primitiveResetCounters(void);
EXPORT(sqInt) primitiveNewPath(void);
EXPORT(sqInt) primitiveFill(void);
EXPORT(sqInt) primitiveFillPreserve(void);
EXPORT(sqInt) primitiveStroke(void);
EXPORT(sqInt) primitivePaint(void);
EXPORT(sqInt) primitiveSave(void);
EXPORT(sqInt) primitiveRestore(void);
EXPORT(sqInt) primitiveClip(void);
EXPORT(sqInt) primitiveResetClip(void);
EXPORT(sqInt) primitiveSetPathMatrix(void);
EXPORT(sqInt) primitiveSetMatrix(void);
EXPORT(sqInt) primitiveSetSourceRGBA(void);
EXPORT(sqInt) primitiveSetSource(void);
EXPORT(sqInt) primitiveResetDash(void);
EXPORT(sqInt) primitiveSetDash(void);
EXPORT(sqInt) primitiveRectangle(void);
EXPORT(sqInt) primitiveSetLineWidth(void);
EXPORT(sqInt) primitiveSetLineJoin(void);
EXPORT(sqInt) primitiveSetLineCap(void);
EXPORT(sqInt) primitiveAppendPath(void);
EXPORT(sqInt) primitiveGetCurrentPoint(void);
EXPORT(sqInt) primitiveSetAntialias(void);
EXPORT(sqInt) primitiveSetFillRule(void);
EXPORT(sqInt) primitiveSetScaledFont(void);
EXPORT(sqInt) primitiveShowGlyphs(void);
EXPORT(sqInt) primitiveGetScaledFont(void);
EXPORT(sqInt) primitiveSetFontSize(void);
EXPORT(sqInt) primitiveBuilderMoveTo(void);
EXPORT(sqInt) primitiveBuilderLineTo(void);
EXPORT(sqInt) primitiveBuilderCurveTo(void);
EXPORT(sqInt) primitiveBuilderClosePath(void);
EXPORT(sqInt) primitiveBuilderNewPath(void);
EXPORT(sqInt) primitiveBuilderCopyPath(void);
EXPORT(sqInt) primitiveBuilderArc(void);
EXPORT(sqInt) primitiveBuilderArcNegative(void);
EXPORT(sqInt) primitivePathDestroy(void);
EXPORT(sqInt) primitiveMatrixInit(void);
EXPORT(sqInt) primitiveMatrixInitIdentity(void);
EXPORT(sqInt) primitiveMatrixTranslate(void);
EXPORT(sqInt) primitiveMatrixScale(void);
EXPORT(sqInt) primitiveMatrixRotate(void);
EXPORT(sqInt) primitiveMatrixMultiply(void);
EXPORT(sqInt) primitiveMatrixInvert(void);
EXPORT(sqInt) primitiveMatrixTransformPoint(void);
EXPORT(sqInt) primitiveMatrixInitTranslate(void);
EXPORT(sqInt) primitiveMatrixInitScale(void);
EXPORT(sqInt) primitiveMatrixInitRotate(void);
EXPORT(sqInt) primitiveGetOperator(void);
EXPORT(sqInt) primitiveSetOperator(void);
EXPORT(sqInt) primitiveSurfaceFlush(void);
EXPORT(sqInt) primitiveScaledFontLockFace(void);
EXPORT(sqInt) primitiveScaledFontUnlockFace(void);
EXPORT(sqInt) primitiveScaledFontGlyphExtents(void);
EXPORT(sqInt) primitiveScaledFontTextExtents(void);
EXPORT(sqInt) primitiveScaledFontExtents(void);
EXPORT(sqInt) primitiveTextToGlyphs(void);
EXPORT(sqInt) primitiveGlyphFree(void);
EXPORT(sqInt) primitiveTextPath(void);
EXPORT(sqInt) primitiveFillRectangleColor(void);
EXPORT(sqInt) primitiveStrokeRectangle(void);
EXPORT(sqInt) primitiveFillPathColor(void);
EXPORT(sqInt) primitiveStrokePath(void);
EXPORT(sqInt) primitiveShowString(void);

/* CP_P_<Name>: the index of each primitive (its success counter, its name in
   primitiveFunctionNames); P(Name) for primitive<Name> */
#define CP_PRIMITIVES(P) \
	P(Info) \
	P(Bind) \
	P(Unbind) \
	P(Counters) \
	P(FunctionNames) \
	P(ResetCounters) \
	P(NewPath) \
	P(Fill) \
	P(FillPreserve) \
	P(Stroke) \
	P(Paint) \
	P(Save) \
	P(Restore) \
	P(Clip) \
	P(ResetClip) \
	P(SetPathMatrix) \
	P(SetMatrix) \
	P(SetSourceRGBA) \
	P(SetSource) \
	P(ResetDash) \
	P(SetDash) \
	P(Rectangle) \
	P(SetLineWidth) \
	P(SetLineJoin) \
	P(SetLineCap) \
	P(AppendPath) \
	P(GetCurrentPoint) \
	P(SetAntialias) \
	P(SetFillRule) \
	P(SetScaledFont) \
	P(ShowGlyphs) \
	P(GetScaledFont) \
	P(SetFontSize) \
	P(BuilderMoveTo) \
	P(BuilderLineTo) \
	P(BuilderCurveTo) \
	P(BuilderClosePath) \
	P(BuilderNewPath) \
	P(BuilderCopyPath) \
	P(BuilderArc) \
	P(BuilderArcNegative) \
	P(PathDestroy) \
	P(MatrixInit) \
	P(MatrixInitIdentity) \
	P(MatrixTranslate) \
	P(MatrixScale) \
	P(MatrixRotate) \
	P(MatrixMultiply) \
	P(MatrixInvert) \
	P(MatrixTransformPoint) \
	P(MatrixInitTranslate) \
	P(MatrixInitScale) \
	P(MatrixInitRotate) \
	P(GetOperator) \
	P(SetOperator) \
	P(SurfaceFlush) \
	P(ScaledFontLockFace) \
	P(ScaledFontUnlockFace) \
	P(ScaledFontGlyphExtents) \
	P(ScaledFontTextExtents) \
	P(ScaledFontExtents) \
	P(TextToGlyphs) \
	P(GlyphFree) \
	P(TextPath) \
	P(FillRectangleColor) \
	P(StrokeRectangle) \
	P(FillPathColor) \
	P(StrokePath) \
	P(ShowString)

#define CP_X_PRIM_ENUM(Name) CP_P_##Name,
enum { CP_PRIMITIVES(CP_X_PRIM_ENUM) CP_P_COUNT };
#undef CP_X_PRIM_ENUM

#define CP_X_PRIM_NAME(Name) "primitive" #Name,
static const char *const cp_primitive_names[CP_P_COUNT] = { CP_PRIMITIVES(CP_X_PRIM_NAME) };
#undef CP_X_PRIM_NAME


/*** State: the binding, the layout, the counters ***/

static cp_cairo_api cp_api;		/* the bound functions; all NULL when unbound */
static int cp_bound;			/* 1 after a successful primitiveBind */
static int cp_version;			/* cairo_version() of the binding */
static sqInt cp_layout[CP_L_COUNT];	/* positions 1-29 of the layout table (0-based CP_L_*) */
/* Amendment A1: the four string classes of the text primitive (layout
   positions 30-33), GC roots (addGCRoot) from the first bind that has them,
   nil otherwise; cp_have_string_classes when the binding has them */
static sqInt cp_string_classes[CP_LAYOUT_STRING_CLASSES];
static int cp_roots_registered;
static int cp_have_string_classes;
static const char *cp_missing;		/* the missing required function of the last bind, or NULL */
static char cp_library[256];		/* the library path of the binding (recorded only, on Emscripten) */
static uint64_t cp_cairo_calls[CP_FN_COUNT];	/* cairo calls, by function (CP_CALL) */
static uint64_t cp_prim_calls[CP_P_COUNT];	/* successful primitive calls (CP_PRIM_DONE) */

/* The call of a cairo function through the table, counted:
   CP_CALL(new_path)(cr).  The only way the primitives call cairo. */
#define CP_CALL(name) (cp_cairo_calls[CP_FN_##name]++, cp_api.name)
/* Whether a CP_OPT function is bound: CP_HAVE(ft_scaled_font_lock_face) */
#define CP_HAVE(name) (cp_api.name != NULL)
/* A successful primitive, counted just before it answers */
#define CP_PRIM_DONE(Name) (cp_prim_calls[CP_P_##Name]++)

/* The prelude of every primitive: CP_ARGS(n) fails with PrimErrBadNumArgs
   unless it has n arguments, CP_BOUND() with PrimErrUnsupported unless
   bound */
#define CP_ARGS(n) do { \
		if (interpreterProxy->methodArgumentCount() != (n)) { \
			return cp_fail(PrimErrBadNumArgs); \
		} \
	} while (0)
#define CP_BOUND() do { \
		if (!cp_bound) { \
			return cp_fail(PrimErrUnsupported); \
		} \
	} while (0)
/* The stack: CP_ARG(i) is the argument i (0-based, the first one first) of a
   primitive of n arguments, CP_RECEIVER(n) its receiver */
#define CP_ARG(n, i) (interpreterProxy->stackValue((n) - 1 - (i)))
#define CP_RECEIVER(n) (interpreterProxy->stackValue(n))


/*** The module ***/

static void cp_unbind(void);

EXPORT(const char*)
getModuleName(void)
{
	return moduleName;
}

EXPORT(sqInt)
initialiseModule(void)
{
	return 1;	/* binds nothing: the image binds at its start (CairoPlugin class>>startUp:) */
}

EXPORT(sqInt)
shutdownModule(void)
{
	int k;

	if (interpreterProxy) {
		cp_unbind();
		if (cp_roots_registered) {
			for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
				interpreterProxy->removeGCRoot(&cp_string_classes[k]);
			}
			cp_roots_registered = 0;
		}
	}
	return 1;
}

EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return interpreterProxy->majorVersion() == VM_PROXY_MAJOR
	    && interpreterProxy->minorVersion() >= VM_PROXY_MINOR;
}


/*** Failing and answering ***/

static sqInt
cp_fail(sqInt code)
{
	return interpreterProxy->primitiveFailFor(code);
}

CP_HELPER sqInt
cp_return(sqInt oop)
{
	return interpreterProxy->methodReturnValue(oop);
}

/* the answer of a void callout */
CP_HELPER sqInt
cp_return_receiver(void)
{
	return interpreterProxy->methodReturnReceiver();
}

/* a SmallInteger answer (an int of cairo always is one) */
CP_HELPER sqInt
cp_return_int(sqInt value)
{
	return interpreterProxy->methodReturnValue(interpreterProxy->integerObjectOf(value));
}


/*** Validation helpers: 1 or 0, never a failure code ***/

/* out = the slot cp_layout[L] of oop, a pointers object with that slot */
CP_HELPER int
cp_slot(sqInt oop, int L, sqInt *out)
{
	sqInt index = cp_layout[L];

	if (interpreterProxy->isImmediate(oop) || !interpreterProxy->isPointers(oop)
	 || interpreterProxy->slotSizeOf(oop) <= index) {
		return 0;
	}
	*out = interpreterProxy->fetchPointerofObject(index, oop);
	return 1;
}

/* oop is a pointers object with the slot cp_layout[L], and not read-only:
   one the primitive may store into (storePointerofObjectwithValue) */
CP_HELPER int
cp_writable_slot(sqInt oop, int L)
{
	return !interpreterProxy->isImmediate(oop) && interpreterProxy->isPointers(oop)
		&& interpreterProxy->slotSizeOf(oop) > cp_layout[L]
		&& !interpreterProxy->isOopImmutable(oop);
}

/* ptr = the address of an ExternalAddress (exactly that class: it has no
   subclasses) of pointer size; NULL allowed */
CP_HELPER int
cp_address_or_null(sqInt oop, void **ptr)
{
	if (interpreterProxy->isImmediate(oop)
	 || interpreterProxy->fetchClassOf(oop) != interpreterProxy->classExternalAddress()
	 || interpreterProxy->byteSizeOf(oop) != (sqInt)sizeof(void *)) {
		return 0;
	}
	memcpy(ptr, interpreterProxy->firstIndexableField(oop), sizeof(void *));
	return 1;
}

/* the same, NULL refused: the stale-handle guard (every ExternalAddress of a
   previous session is NULL at image start) */
CP_HELPER int
cp_address(sqInt oop, void **ptr)
{
	return cp_address_or_null(oop, ptr) && *ptr != NULL;
}

/* body = the sizeof(void *) bytes of an ExternalAddress (NULL allowed) that
   is not read-only: an in/out pointer the primitive writes (exactly
   sizeof(void *) bytes, with memcpy) */
CP_HELPER int
cp_address_out(sqInt oop, void **body)
{
	void *ignored;

	if (!cp_address_or_null(oop, &ignored) || interpreterProxy->isOopImmutable(oop)) {
		return 0;
	}
	*body = interpreterProxy->firstIndexableField(oop);
	return 1;
}

/* ptr = the non-NULL address of the ExternalAddress in the slot L of obj */
CP_HELPER int
cp_handle(sqInt obj, int L, void **ptr)
{
	sqInt handle;

	return cp_slot(obj, L, &handle) && cp_address(handle, ptr);
}

/* cr = the cairo_t of an AthensCairoCanvas (its handle) */
CP_HELPER int
cp_canvas_cr(sqInt canvas, cp_cairo_t **cr)
{
	void *ptr;

	if (!cp_handle(canvas, CP_L_CANVAS_HANDLE, &ptr)) {
		return 0;
	}
	*cr = (cp_cairo_t *)ptr;
	return 1;
}

/* p, size = the bytes of a byte object (isBytes, not a CompiledMethod) that
   is not an ExternalAddress (whose 8 bytes are a pointer, not data), of at
   least min bytes, not read-only when writable.  The only isBytes of the
   plugin (R7). */
CP_HELPER int
cp_bytes(sqInt oop, size_t min, int writable, void **p, size_t *size)
{
	size_t n;

	if (interpreterProxy->isImmediate(oop) || !interpreterProxy->isBytes(oop)
	 || !interpreterProxy->isWordsOrBytes(oop)
	 || interpreterProxy->fetchClassOf(oop) == interpreterProxy->classExternalAddress()) {
		return 0;
	}
	n = (size_t)interpreterProxy->byteSizeOf(oop);
	if (n < min || (writable && interpreterProxy->isOopImmutable(oop))) {
		return 0;
	}
	*p = interpreterProxy->firstIndexableField(oop);
	if (size) {
		*size = n;
	}
	return 1;
}

/* p = the bytes of a byte object (cp_bytes) holding a NUL: inside its bytes,
   or in the zero padding Spur puts after them up to the 8-byte-rounded body
   size (never past the body): the C string the FFI's void* passes.  No NUL
   (a size multiple of 8 and no NUL) fails: the FFI would read past the
   object */
CP_HELPER int
cp_cstring(sqInt oop, const char **p)
{
	void *body;
	size_t n;

	if (!cp_bytes(oop, 0, 0, &body, &n)) {
		return 0;
	}
	if ((n > 0 && memchr(body, 0, n)) || ((n & 7) != 0 && ((const char *)body)[n] == 0)) {
		*p = (const char *)body;
		return 1;
	}
	return 0;
}

/* m = the 6 doubles (xx yx xy yy x0 y0) of an AthensCairoMatrix: its handle
   is first tested as an ExternalAddress (non-NULL; trusted like the FFI,
   writable or not), else it must be a byte object of exactly 48 bytes, not
   read-only when writable */
CP_HELPER int
cp_matrix(sqInt obj, int writable, cp_matrix_t **m)
{
	sqInt handle;
	void *p;
	size_t n;

	if (!cp_slot(obj, CP_L_EXTOBJ_HANDLE, &handle)) {
		return 0;
	}
	if (!interpreterProxy->isImmediate(handle)
	 && interpreterProxy->fetchClassOf(handle) == interpreterProxy->classExternalAddress()) {
		if (!cp_address(handle, &p)) {
			return 0;
		}
	} else if (!cp_bytes(handle, sizeof(cp_matrix_t), writable, &p, &n) || n != sizeof(cp_matrix_t)) {
		return 0;
	}
	*m = (cp_matrix_t *)p;
	return 1;
}

/* d = a SmallInteger or a Float as a double (as asFloat); anything else
   (a Fraction, a LargeInteger) fails: the fallback coerces it as the FFI does */
CP_HELPER int
cp_number(sqInt oop, double *d)
{
	if (interpreterProxy->isIntegerObject(oop)) {
		*d = (double)interpreterProxy->integerValueOf(oop);
		return 1;
	}
	if (interpreterProxy->isFloatObject(oop)) {
		*d = interpreterProxy->floatValueOf(oop);
		return 1;
	}
	return 0;
}

/* i = a SmallInteger in [INT_MIN, INT_MAX] */
CP_HELPER int
cp_int(sqInt oop, int *i)
{
	sqInt v;

	if (!interpreterProxy->isIntegerObject(oop)) {
		return 0;
	}
	v = interpreterProxy->integerValueOf(oop);
	if (v < INT_MIN || v > INT_MAX) {
		return 0;
	}
	*i = (int)v;
	return 1;
}

/* rgba = the components of a Color, as Color>>red, green, blue and alpha
   compute them (the same IEEE operations): the slot rgb a SmallInteger >= 0,
   alpha a SmallInteger in 0..255 */
CP_HELPER int
cp_color(sqInt oop, double rgba[4])
{
	sqInt rgbOop, alphaOop, rgb, alpha, mask;

	if (!cp_slot(oop, CP_L_COLOR_RGB, &rgbOop) || !cp_slot(oop, CP_L_COLOR_ALPHA, &alphaOop)
	 || !interpreterProxy->isIntegerObject(rgbOop) || !interpreterProxy->isIntegerObject(alphaOop)) {
		return 0;
	}
	rgb = interpreterProxy->integerValueOf(rgbOop);
	alpha = interpreterProxy->integerValueOf(alphaOop);
	if (rgb < 0 || alpha < 0 || alpha > cp_layout[CP_L_COLOR_ALPHA_MAX]) {
		return 0;
	}
	mask = cp_layout[CP_L_COLOR_COMPONENT_MASK];
	rgba[0] = (double)((rgb >> cp_layout[CP_L_COLOR_RED_SHIFT]) & mask) / (double)mask;
	rgba[1] = (double)((rgb >> cp_layout[CP_L_COLOR_GREEN_SHIFT]) & mask) / (double)mask;
	rgba[2] = (double)(rgb & mask) / (double)mask;
	rgba[3] = (double)alpha / (double)cp_layout[CP_L_COLOR_ALPHA_MAX];
	return 1;
}

/* x, y and their oops = the coordinates of a Point (numbers, cp_number) */
CP_HELPER int
cp_point(sqInt oop, sqInt *xOop, sqInt *yOop, double *x, double *y)
{
	return cp_slot(oop, CP_L_POINT_X, xOop) && cp_slot(oop, CP_L_POINT_Y, yOop)
		&& cp_number(*xOop, x) && cp_number(*yOop, y);
}

/* xywh = left, top, width, height of a Rectangle, as `aRectangle left
   asFloat` ... `height asFloat`: the width (height) of two SmallIntegers is
   subtracted exactly, then converted; otherwise both are converted and
   subtracted */
CP_HELPER int
cp_rect(sqInt oop, double xywh[4])
{
	sqInt origin, corner, ox, oy, cx, cy;
	double dox, doy, dcx, dcy;

	if (!cp_slot(oop, CP_L_RECT_ORIGIN, &origin) || !cp_slot(oop, CP_L_RECT_CORNER, &corner)
	 || !cp_point(origin, &ox, &oy, &dox, &doy) || !cp_point(corner, &cx, &cy, &dcx, &dcy)) {
		return 0;
	}
	xywh[0] = dox;
	xywh[1] = doy;
	xywh[2] = interpreterProxy->isIntegerObject(ox) && interpreterProxy->isIntegerObject(cx)
		? (double)(interpreterProxy->integerValueOf(cx) - interpreterProxy->integerValueOf(ox))
		: dcx - dox;
	xywh[3] = interpreterProxy->isIntegerObject(oy) && interpreterProxy->isIntegerObject(cy)
		? (double)(interpreterProxy->integerValueOf(cy) - interpreterProxy->integerValueOf(oy))
		: dcy - doy;
	return 1;
}

/* Amendment A1: base, units, words = the characters of an instance of
   exactly one of the four string classes of the layout (ByteString,
   ByteSymbol: Latin-1 bytes, words 0; WideString, WideSymbol: UTF-32 words,
   words 1), as su_input of suEngine.h takes them.  Fails when the binding
   has no string classes (a 29-element layout). */
CP_HELPER int
cp_string(sqInt oop, const void **base, size_t *units, int *words)
{
	sqInt cls;
	int k;

	if (!cp_have_string_classes || interpreterProxy->isImmediate(oop)) {
		return 0;
	}
	cls = interpreterProxy->fetchClassOf(oop);
	for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
		if (cls == cp_string_classes[k]) {
			/* the order of cpLayout.h: two byte classes, then two word ones;
			   the bind checked the classes' formats, the instance's own is
			   checked again so that base and units always cover its body */
			if (!interpreterProxy->isWordsOrBytes(oop)
			 || (interpreterProxy->isWords(oop) != 0) != (k >= 2)) {
				return 0;
			}
			*words = k >= 2;
			*units = (size_t)interpreterProxy->byteSizeOf(oop) / (*words ? 4 : 1);
			*base = interpreterProxy->firstIndexableField(oop);
			return 1;
		}
	}
	return 0;
}


/*** Allocation helpers: the new object, or 0 (PrimErrNoMemory set) ***/

/* The only call of instantiateClassindexableSize (R1) */
static sqInt
cp_instantiate(sqInt aClass, sqInt size)
{
	sqInt oop = interpreterProxy->instantiateClassindexableSize(aClass, size);

	if (!oop && !interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
	}
	return oop;
}

/* Whether d is in the range of the SmallFloat64 immediates of 64-bit Spur
   (as the VM's isSmallFloatValue: the exponents of 2^-127 .. 2^128, and
   +0.0) */
static int
cp_is_small_float(double d)
{
	uint64_t raw, exponent;

	if (sizeof(sqInt) != 8) {
		return 0;	/* 32-bit Spur has no immediate floats */
	}
	memcpy(&raw, &d, sizeof raw);
	exponent = (raw >> 52) & 0x7FF;
	return exponent > 896
		? exponent <= 0xFF + 896
		: (raw & ((1ULL << 52) - 1)) == 0 ? exponent == 0 : exponent == 896;
}

/* A Float of the value d: a SmallFloat64 (floatObjectOf allocates nothing
   for those) or a BoxedFloat64 */
CP_HELPER sqInt
cp_new_float(double d)
{
	sqInt oop;

	if (cp_is_small_float(d)) {
		return interpreterProxy->floatObjectOf(d);
	}
	/* classFloat() is BoxedFloat64: two 32-bit slots, in the platform's order */
	if ((oop = cp_instantiate(interpreterProxy->classFloat(), 2))) {
		memcpy(interpreterProxy->firstIndexableField(oop), &d, sizeof d);
	}
	return oop;
}

/* A new ExternalAddress holding ptr */
CP_HELPER sqInt
cp_new_address(void *ptr)
{
	sqInt oop = cp_instantiate(interpreterProxy->classExternalAddress(), (sqInt)sizeof(void *));

	if (oop) {
		memcpy(interpreterProxy->firstIndexableField(oop), &ptr, sizeof(void *));
	}
	return oop;
}

/* A new Point of two Floats, x @ y (the slots of the layout) */
CP_HELPER sqInt
cp_new_point(double x, double y)
{
	sqInt xOop, yOop, point;

	if (!(xOop = cp_new_float(x)) || !(yOop = cp_new_float(y))
	 || !(point = cp_instantiate(interpreterProxy->classPoint(), 0))) {
		return 0;
	}
	if (interpreterProxy->slotSizeOf(point) <= cp_layout[CP_L_POINT_X]
	 || interpreterProxy->slotSizeOf(point) <= cp_layout[CP_L_POINT_Y]) {
		cp_fail(PrimErrBadIndex);	/* a layout that does not fit Point: never with the image's */
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(cp_layout[CP_L_POINT_X], point, xOop);
	interpreterProxy->storePointerofObjectwithValue(cp_layout[CP_L_POINT_Y], point, yOop);
	return point;
}

/* A new ByteString of the C string s */
static sqInt
cp_new_string(const char *s)
{
	size_t n = strlen(s);
	sqInt oop = cp_instantiate(interpreterProxy->classString(), (sqInt)n);

	if (oop && n) {
		memcpy(interpreterProxy->firstIndexableField(oop), s, n);
	}
	return oop;
}

/* A new Array of n nils */
static sqInt
cp_new_array(sqInt n)
{
	return cp_instantiate(interpreterProxy->classArray(), n);
}


/*** Binding ***/

/* Clears the binding (the table, the layout, the string classes); the
   counters and the missing name stay */
static void
cp_unbind(void)
{
	int k;

	cp_bound = 0;
	cp_version = 0;
	memset(&cp_api, 0, sizeof cp_api);
	memset(cp_layout, 0, sizeof cp_layout);
	cp_have_string_classes = 0;
	if (cp_roots_registered) {
		for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
			cp_string_classes[k] = interpreterProxy->nilObject();
		}
	}
}

#ifndef CP_STATIC_CAIRO
/* The symbol loader of cp_cairo_bind: the VM's, in the module the bind
   loaded.  A NULL module would make the VM search the whole process (R9):
   primitiveBind never passes one, and this refuses it anyway. */
static void *
cp_vm_loader(const char *name, void *module)
{
	if (!module) {
		return NULL;
	}
	return interpreterProxy->ioLoadSymbolOfLengthFromModule(oopForPointer((void *)name),
		(sqInt)strlen(name), module);
}
#endif

/* Whether the function of index i (CP_FN_<name>) is bound */
static int
cp_function_bound(int i)
{
	switch (i) {
#define CP_X_BOUND(ret, name, params, flags) case CP_FN_##name: return CP_HAVE(name);
	CP_CAIRO_FUNCTIONS(CP_X_BOUND)
#undef CP_X_BOUND
	default:
		return 0;
	}
}

/* Whether value is acceptable at the 0-based layout position i (< 29) */
static int
cp_layout_value_ok(int i, sqInt value)
{
	switch (i) {
	case CP_L_LAYOUT_VERSION:
		return value == CP_LAYOUT_VERSION;
	case CP_L_COLOR_RED_SHIFT:
	case CP_L_COLOR_GREEN_SHIFT:
		return value >= 0 && value <= CP_LAYOUT_SHIFT_MAX;
	case CP_L_COLOR_COMPONENT_MASK:
	case CP_L_COLOR_ALPHA_MAX:
		return value >= 1 && value <= CP_LAYOUT_CONSTANT_MAX;
	default:
		return value >= 0 && value <= CP_LAYOUT_SLOT_MAX;
	}
}


/*** primitiveInfo ***/

/*	CairoPlugin class>>primInfo
	<primitive: 'primitiveInfo' module: 'CairoPlugin' error: ec>
	Answer { protocol. 'CairoPlugin 1.0'. 'native' or 'emscripten'. the cairo
	version (cairo_version(), e.g. 11704) or nil when unbound. 'loader' or
	'static'. the missing required function of the last bind (a String) or
	nil. an Array of the names of the optional functions the binding lacks
	(empty when all present or unbound) }. */
EXPORT(sqInt)
primitiveInfo(void)
{
	sqInt result, oop, missingOptional;
	sqInt nOptional = 0, k;
	int i;

	CP_ARGS(0);
	if (cp_bound) {
		for (i = 0; i < CP_FN_COUNT; i++) {
			if (cp_cairo_function_flags[i] == CP_OPT && !cp_function_bound(i)) {
				nOptional++;
			}
		}
	}
	if (!(result = cp_new_array(7)) || !(missingOptional = cp_new_array(nOptional))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(CP_PROTOCOL));
	if (!(oop = cp_new_string(CP_PLUGIN_VERSION))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(1, result, oop);
	if (!(oop = cp_new_string(CP_PLATFORM))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(2, result, oop);
	interpreterProxy->storePointerofObjectwithValue(3, result,
		cp_bound ? interpreterProxy->integerObjectOf(cp_version) : interpreterProxy->nilObject());
	if (!(oop = cp_new_string(CP_BINDING))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(4, result, oop);
	if (cp_missing) {
		if (!(oop = cp_new_string(cp_missing))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(5, result, oop);
	}
	for (i = 0, k = 0; k < nOptional && i < CP_FN_COUNT; i++) {
		if (cp_cairo_function_flags[i] == CP_OPT && !cp_function_bound(i)) {
			if (!(oop = cp_new_string(cp_cairo_function_names[i]))) {
				return 0;
			}
			interpreterProxy->storePointerofObjectwithValue(k++, missingOptional, oop);
		}
	}
	interpreterProxy->storePointerofObjectwithValue(6, result, missingOptional);
	CP_PRIM_DONE(Info);
	return cp_return(result);
}


/*** primitiveBind ***/

/* A string class of the layout (positions 30-33): a class (a pointers object
   whose third slot, the format, is a SmallInteger) of byte instances (instSpec
   16 to 23: ByteString, ByteSymbol) or, words, of 32-bit word instances
   (instSpec 10 or 11: WideString, WideSymbol).  cp_string reads every
   instance of these classes as Latin-1 bytes or UTF-32 words: a class of
   pointers instances (nil, Array, a class with named and indexable slots,
   whose byteSizeOf counts the named slots that firstIndexableField skips)
   would make it read past the end of the object. */
static int
cp_string_class_ok(sqInt cls, int words)
{
	sqInt format;
	sqInt spec;

	if (interpreterProxy->isImmediate(cls) || !interpreterProxy->isPointers(cls)
	 || interpreterProxy->slotSizeOf(cls) < 3) {
		return 0;	/* not a class (or a forwarder: the VM follows it and retries) */
	}
	format = interpreterProxy->fetchPointerofObject(2, cls);
	if (!interpreterProxy->isIntegerObject(format)) {
		return 0;
	}
	spec = (interpreterProxy->integerValueOf(format) >> 16) & 0x1F;
	return words ? spec == 10 || spec == 11 : spec >= 16 && spec <= 23;
}

/*	CairoPlugin class>>primBind: libraryPath probe: anExternalAddress layout: anArray
	<primitive: 'primitiveBind' module: 'CairoPlugin' error: ec>
	Bind cairo: natively the library at libraryPath (a ByteString of 1 to
	255 bytes: CairoLibrary uniqueInstance libraryName), loaded and searched
	through the VM's module loader, as the FFI does; on Emscripten the cairo
	linked in (the path is only recorded).  anExternalAddress is the probe,
	the address the FFI resolves for cairo_create, which must be the bound
	cairo_create; anArray the layout table (cpLayout.h).  Answer the version
	of cairo (cairo_version()).  Fails with #'bad argument' (the path, a
	probe that is neither nil nor an ExternalAddress, a layout that is not an
	Array) or #'bad index' (the layout's size or elements), changing
	nothing; then, leaving the plugin unbound: #'not found' (a nil or NULL
	probe, a library that does not load, a required function missing:
	primitiveInfo names it), #'inappropriate operation' (the probe is not the
	bound cairo_create: another libcairo), #'unsupported operation' (cairo
	older than 1.16.0). */
EXPORT(sqInt)
primitiveBind(void)
{
	sqInt path, probe, layout, size, value;
	sqInt values[CP_LAYOUT_INTEGERS], classes[CP_LAYOUT_STRING_CLASSES];
	void *pathBytes, *probePtr = NULL;
	size_t pathLength;
	cp_cairo_api api;
	int i, k, haveClasses, version;
#ifndef CP_STATIC_CAIRO
	void *module;
	const char *missing = NULL;
#endif

	CP_ARGS(3);
	path = interpreterProxy->stackValue(2);
	probe = interpreterProxy->stackValue(1);
	layout = interpreterProxy->stackValue(0);
	/* the arguments, before anything changes */
	if (interpreterProxy->isImmediate(path)
	 || interpreterProxy->fetchClassOf(path) != interpreterProxy->classString()
	 || !cp_bytes(path, 1, 0, &pathBytes, &pathLength) || pathLength > 255) {
		return cp_fail(PrimErrBadArgument);
	}
	if (probe != interpreterProxy->nilObject() && !cp_address_or_null(probe, &probePtr)) {
		return cp_fail(PrimErrBadArgument);
	}
	if (interpreterProxy->isImmediate(layout)
	 || interpreterProxy->fetchClassOf(layout) != interpreterProxy->classArray()) {
		return cp_fail(PrimErrBadArgument);
	}
	size = interpreterProxy->slotSizeOf(layout);
	if (size != CP_LAYOUT_SIZE_BASE && size != CP_LAYOUT_SIZE_TEXT) {
		return cp_fail(PrimErrBadIndex);
	}
	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		value = interpreterProxy->fetchPointerofObject(i, layout);
		if (!interpreterProxy->isIntegerObject(value)
		 || !cp_layout_value_ok(i, interpreterProxy->integerValueOf(value))) {
			return cp_fail(PrimErrBadIndex);
		}
		values[i] = interpreterProxy->integerValueOf(value);
	}
	haveClasses = size == CP_LAYOUT_SIZE_TEXT;
	for (k = 0; haveClasses && k < CP_LAYOUT_STRING_CLASSES; k++) {
		classes[k] = interpreterProxy->fetchPointerofObject(CP_LAYOUT_INTEGERS + k, layout);
		if (!cp_string_class_ok(classes[k], k >= 2)) {
			return cp_fail(PrimErrBadIndex);
		}
	}

	/* the bind itself: any failure from here on leaves the plugin unbound */
	cp_unbind();
	cp_missing = NULL;
	if (!probePtr) {
		return cp_fail(PrimErrNotFound);	/* the FFI cannot reach cairo either */
	}
#ifdef CP_STATIC_CAIRO
	cp_cairo_bind_static(&api);
#else
	module = interpreterProxy->ioLoadModuleOfLength(oopForPointer(pathBytes), (sqInt)pathLength);
	if (!module) {
		return cp_fail(PrimErrNotFound);	/* before any symbol lookup (R9) */
	}
	if (cp_cairo_bind(cp_vm_loader, module, &api, &missing) != CP_BIND_OK) {
		cp_missing = missing;
		return cp_fail(PrimErrNotFound);
	}
#endif
	if ((void *)api.create != probePtr) {
		return cp_fail(PrimErrInappropriate);	/* another libcairo than the FFI's */
	}
	version = api.version();
	if (version < CP_CAIRO_VERSION_MIN) {
		return cp_fail(PrimErrUnsupported);
	}
	su_init();		/* idempotent: the kernel of the text primitive's UTF-8 */

	/* commit */
	if (haveClasses && !cp_roots_registered) {
		for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
			cp_string_classes[k] = interpreterProxy->nilObject();
		}
		for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
			if (!interpreterProxy->addGCRoot(&cp_string_classes[k])) {
				while (--k >= 0) {
					interpreterProxy->removeGCRoot(&cp_string_classes[k]);
				}
				break;
			}
		}
		cp_roots_registered = k == CP_LAYOUT_STRING_CLASSES;
	}
	cp_api = api;
	for (i = 0; i < CP_LAYOUT_INTEGERS; i++) {
		cp_layout[i] = values[i];
	}
	if (haveClasses && cp_roots_registered) {
		for (k = 0; k < CP_LAYOUT_STRING_CLASSES; k++) {
			cp_string_classes[k] = classes[k];
		}
		cp_have_string_classes = 1;
	}
	memcpy(cp_library, pathBytes, pathLength);
	cp_library[pathLength] = 0;
	cp_version = version;
	cp_bound = 1;
	CP_PRIM_DONE(Bind);
	return cp_return_int(version);
}


/*** primitiveUnbind ***/

/*	CairoPlugin class>>primUnbind
	<primitive: 'primitiveUnbind' module: 'CairoPlugin' error: ec>
	Forget the binding: every drawing primitive fails with #'unsupported
	operation' until the next bind (the image's FFI methods run).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveUnbind(void)
{
	CP_ARGS(0);
	cp_unbind();
	CP_PRIM_DONE(Unbind);
	return cp_return_receiver();
}


/*** primitiveCounters ***/

/*	CairoPlugin class>>primCounters
	<primitive: 'primitiveCounters' module: 'CairoPlugin' error: ec>
	Answer { the cairo calls by function (an Array of SmallIntegers, in the
	order of primitiveFunctionNames' first Array). the successful calls by
	primitive (an Array of SmallIntegers, in the order of its second
	Array) }. */
EXPORT(sqInt)
primitiveCounters(void)
{
	sqInt result, calls, prims;
	int i;

	CP_ARGS(0);
	if (!(result = cp_new_array(2)) || !(calls = cp_new_array(CP_FN_COUNT))
	 || !(prims = cp_new_array(CP_P_COUNT))) {
		return 0;
	}
	for (i = 0; i < CP_FN_COUNT; i++) {
		interpreterProxy->storePointerofObjectwithValue(i, calls, interpreterProxy->integerObjectOf(
			cp_cairo_calls[i] > (uint64_t)CP_SMALLINTEGER_MAX ? CP_SMALLINTEGER_MAX : (sqInt)cp_cairo_calls[i]));
	}
	/* this call counts too, before it answers */
	CP_PRIM_DONE(Counters);
	for (i = 0; i < CP_P_COUNT; i++) {
		interpreterProxy->storePointerofObjectwithValue(i, prims, interpreterProxy->integerObjectOf(
			cp_prim_calls[i] > (uint64_t)CP_SMALLINTEGER_MAX ? CP_SMALLINTEGER_MAX : (sqInt)cp_prim_calls[i]));
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, calls);
	interpreterProxy->storePointerofObjectwithValue(1, result, prims);
	return cp_return(result);
}


/*** primitiveFunctionNames ***/

/*	CairoPlugin class>>primFunctionNames
	<primitive: 'primitiveFunctionNames' module: 'CairoPlugin' error: ec>
	Answer { the names of the cairo functions ('cairo_version'
	'cairo_create' 'cairo_new_path' ...: the order of the counters). the
	names of the primitives ('primitiveInfo' ...) }. */
EXPORT(sqInt)
primitiveFunctionNames(void)
{
	sqInt result, functions, prims, oop;
	int i;

	CP_ARGS(0);
	if (!(result = cp_new_array(2)) || !(functions = cp_new_array(CP_FN_COUNT))
	 || !(prims = cp_new_array(CP_P_COUNT))) {
		return 0;
	}
	for (i = 0; i < CP_FN_COUNT; i++) {
		if (!(oop = cp_new_string(cp_cairo_function_names[i]))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(i, functions, oop);
	}
	for (i = 0; i < CP_P_COUNT; i++) {
		if (!(oop = cp_new_string(cp_primitive_names[i]))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(i, prims, oop);
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, functions);
	interpreterProxy->storePointerofObjectwithValue(1, result, prims);
	CP_PRIM_DONE(FunctionNames);
	return cp_return(result);
}


/*** primitiveResetCounters ***/

/*	CairoPlugin class>>primResetCounters
	<primitive: 'primitiveResetCounters' module: 'CairoPlugin' error: ec>
	Zero both counters.  Answer the receiver. */
EXPORT(sqInt)
primitiveResetCounters(void)
{
	CP_ARGS(0);
	memset(cp_cairo_calls, 0, sizeof cp_cairo_calls);
	memset(cp_prim_calls, 0, sizeof cp_prim_calls);
	return cp_return_receiver();
}


/*** The drawing primitives (relative to this file: also from the
     Emscripten wrapper that #includes it) ***/

#include "../prims/basic.c"
#include "../prims/shape.c"
#include "../prims/text.c"


#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "CairoPlugin";
void* CairoPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveBind\000\001", (void*)primitiveBind},
	{(void*)_m, "primitiveUnbind\000\000", (void*)primitiveUnbind},
	{(void*)_m, "primitiveCounters\000\000", (void*)primitiveCounters},
	{(void*)_m, "primitiveFunctionNames\000\000", (void*)primitiveFunctionNames},
	{(void*)_m, "primitiveResetCounters\000\000", (void*)primitiveResetCounters},
	{(void*)_m, "primitiveNewPath\000\001", (void*)primitiveNewPath},
	{(void*)_m, "primitiveFill\000\001", (void*)primitiveFill},
	{(void*)_m, "primitiveFillPreserve\000\001", (void*)primitiveFillPreserve},
	{(void*)_m, "primitiveStroke\000\001", (void*)primitiveStroke},
	{(void*)_m, "primitivePaint\000\001", (void*)primitivePaint},
	{(void*)_m, "primitiveSave\000\001", (void*)primitiveSave},
	{(void*)_m, "primitiveRestore\000\001", (void*)primitiveRestore},
	{(void*)_m, "primitiveClip\000\001", (void*)primitiveClip},
	{(void*)_m, "primitiveResetClip\000\001", (void*)primitiveResetClip},
	{(void*)_m, "primitiveSetPathMatrix\000\002", (void*)primitiveSetPathMatrix},
	{(void*)_m, "primitiveSetMatrix\000\001", (void*)primitiveSetMatrix},
	{(void*)_m, "primitiveSetSourceRGBA\000\001", (void*)primitiveSetSourceRGBA},
	{(void*)_m, "primitiveSetSource\000\001", (void*)primitiveSetSource},
	{(void*)_m, "primitiveResetDash\000\001", (void*)primitiveResetDash},
	{(void*)_m, "primitiveSetDash\000\001", (void*)primitiveSetDash},
	{(void*)_m, "primitiveRectangle\000\001", (void*)primitiveRectangle},
	{(void*)_m, "primitiveSetLineWidth\000\001", (void*)primitiveSetLineWidth},
	{(void*)_m, "primitiveSetLineJoin\000\001", (void*)primitiveSetLineJoin},
	{(void*)_m, "primitiveSetLineCap\000\001", (void*)primitiveSetLineCap},
	{(void*)_m, "primitiveAppendPath\000\001", (void*)primitiveAppendPath},
	{(void*)_m, "primitiveGetCurrentPoint\000\001", (void*)primitiveGetCurrentPoint},
	{(void*)_m, "primitiveSetAntialias\000\001", (void*)primitiveSetAntialias},
	{(void*)_m, "primitiveSetFillRule\000\001", (void*)primitiveSetFillRule},
	{(void*)_m, "primitiveSetScaledFont\000\001", (void*)primitiveSetScaledFont},
	{(void*)_m, "primitiveShowGlyphs\000\001", (void*)primitiveShowGlyphs},
	{(void*)_m, "primitiveGetScaledFont\000\001", (void*)primitiveGetScaledFont},
	{(void*)_m, "primitiveSetFontSize\000\001", (void*)primitiveSetFontSize},
	{(void*)_m, "primitiveBuilderMoveTo\000\002", (void*)primitiveBuilderMoveTo},
	{(void*)_m, "primitiveBuilderLineTo\000\002", (void*)primitiveBuilderLineTo},
	{(void*)_m, "primitiveBuilderCurveTo\000\002", (void*)primitiveBuilderCurveTo},
	{(void*)_m, "primitiveBuilderClosePath\000\002", (void*)primitiveBuilderClosePath},
	{(void*)_m, "primitiveBuilderNewPath\000\002", (void*)primitiveBuilderNewPath},
	{(void*)_m, "primitiveBuilderCopyPath\000\002", (void*)primitiveBuilderCopyPath},
	{(void*)_m, "primitiveBuilderArc\000\002", (void*)primitiveBuilderArc},
	{(void*)_m, "primitiveBuilderArcNegative\000\002", (void*)primitiveBuilderArcNegative},
	{(void*)_m, "primitivePathDestroy\000\000", (void*)primitivePathDestroy},
	{(void*)_m, "primitiveMatrixInit\000\001", (void*)primitiveMatrixInit},
	{(void*)_m, "primitiveMatrixInitIdentity\000\001", (void*)primitiveMatrixInitIdentity},
	{(void*)_m, "primitiveMatrixTranslate\000\001", (void*)primitiveMatrixTranslate},
	{(void*)_m, "primitiveMatrixScale\000\001", (void*)primitiveMatrixScale},
	{(void*)_m, "primitiveMatrixRotate\000\001", (void*)primitiveMatrixRotate},
	{(void*)_m, "primitiveMatrixMultiply\000\001", (void*)primitiveMatrixMultiply},
	{(void*)_m, "primitiveMatrixInvert\000\001", (void*)primitiveMatrixInvert},
	{(void*)_m, "primitiveMatrixTransformPoint\000\001", (void*)primitiveMatrixTransformPoint},
	{(void*)_m, "primitiveMatrixInitTranslate\000\001", (void*)primitiveMatrixInitTranslate},
	{(void*)_m, "primitiveMatrixInitScale\000\001", (void*)primitiveMatrixInitScale},
	{(void*)_m, "primitiveMatrixInitRotate\000\001", (void*)primitiveMatrixInitRotate},
	{(void*)_m, "primitiveGetOperator\000\002", (void*)primitiveGetOperator},
	{(void*)_m, "primitiveSetOperator\000\002", (void*)primitiveSetOperator},
	{(void*)_m, "primitiveSurfaceFlush\000\001", (void*)primitiveSurfaceFlush},
	{(void*)_m, "primitiveScaledFontLockFace\000\001", (void*)primitiveScaledFontLockFace},
	{(void*)_m, "primitiveScaledFontUnlockFace\000\001", (void*)primitiveScaledFontUnlockFace},
	{(void*)_m, "primitiveScaledFontGlyphExtents\000\001", (void*)primitiveScaledFontGlyphExtents},
	{(void*)_m, "primitiveScaledFontTextExtents\000\001", (void*)primitiveScaledFontTextExtents},
	{(void*)_m, "primitiveScaledFontExtents\000\001", (void*)primitiveScaledFontExtents},
	{(void*)_m, "primitiveTextToGlyphs\000\001", (void*)primitiveTextToGlyphs},
	{(void*)_m, "primitiveGlyphFree\000\000", (void*)primitiveGlyphFree},
	{(void*)_m, "primitiveTextPath\000\001", (void*)primitiveTextPath},
	{(void*)_m, "primitiveFillRectangleColor\000\002", (void*)primitiveFillRectangleColor},
	{(void*)_m, "primitiveStrokeRectangle\000\002", (void*)primitiveStrokeRectangle},
	{(void*)_m, "primitiveFillPathColor\000\002", (void*)primitiveFillPathColor},
	{(void*)_m, "primitiveStrokePath\000\002", (void*)primitiveStrokePath},
	{(void*)_m, "primitiveShowString\000\003", (void*)primitiveShowString},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveBindAccessorDepth = 1;
EXPORT(signed char) primitiveUnbindAccessorDepth = 0;
EXPORT(signed char) primitiveCountersAccessorDepth = 0;
EXPORT(signed char) primitiveFunctionNamesAccessorDepth = 0;
EXPORT(signed char) primitiveResetCountersAccessorDepth = 0;
EXPORT(signed char) primitiveNewPathAccessorDepth = 1;
EXPORT(signed char) primitiveFillAccessorDepth = 1;
EXPORT(signed char) primitiveFillPreserveAccessorDepth = 1;
EXPORT(signed char) primitiveStrokeAccessorDepth = 1;
EXPORT(signed char) primitivePaintAccessorDepth = 1;
EXPORT(signed char) primitiveSaveAccessorDepth = 1;
EXPORT(signed char) primitiveRestoreAccessorDepth = 1;
EXPORT(signed char) primitiveClipAccessorDepth = 1;
EXPORT(signed char) primitiveResetClipAccessorDepth = 1;
EXPORT(signed char) primitiveSetPathMatrixAccessorDepth = 2;
EXPORT(signed char) primitiveSetMatrixAccessorDepth = 1;
EXPORT(signed char) primitiveSetSourceRGBAAccessorDepth = 1;
EXPORT(signed char) primitiveSetSourceAccessorDepth = 1;
EXPORT(signed char) primitiveResetDashAccessorDepth = 1;
EXPORT(signed char) primitiveSetDashAccessorDepth = 1;
EXPORT(signed char) primitiveRectangleAccessorDepth = 1;
EXPORT(signed char) primitiveSetLineWidthAccessorDepth = 1;
EXPORT(signed char) primitiveSetLineJoinAccessorDepth = 1;
EXPORT(signed char) primitiveSetLineCapAccessorDepth = 1;
EXPORT(signed char) primitiveAppendPathAccessorDepth = 1;
EXPORT(signed char) primitiveGetCurrentPointAccessorDepth = 1;
EXPORT(signed char) primitiveSetAntialiasAccessorDepth = 1;
EXPORT(signed char) primitiveSetFillRuleAccessorDepth = 1;
EXPORT(signed char) primitiveSetScaledFontAccessorDepth = 1;
EXPORT(signed char) primitiveShowGlyphsAccessorDepth = 1;
EXPORT(signed char) primitiveGetScaledFontAccessorDepth = 1;
EXPORT(signed char) primitiveSetFontSizeAccessorDepth = 1;
EXPORT(signed char) primitiveBuilderMoveToAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderLineToAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderCurveToAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderClosePathAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderNewPathAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderCopyPathAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderArcAccessorDepth = 2;
EXPORT(signed char) primitiveBuilderArcNegativeAccessorDepth = 2;
EXPORT(signed char) primitivePathDestroyAccessorDepth = 0;
EXPORT(signed char) primitiveMatrixInitAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixInitIdentityAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixTranslateAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixScaleAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixRotateAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixMultiplyAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixInvertAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixTransformPointAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixInitTranslateAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixInitScaleAccessorDepth = 1;
EXPORT(signed char) primitiveMatrixInitRotateAccessorDepth = 1;
EXPORT(signed char) primitiveGetOperatorAccessorDepth = 2;
EXPORT(signed char) primitiveSetOperatorAccessorDepth = 2;
EXPORT(signed char) primitiveSurfaceFlushAccessorDepth = 1;
EXPORT(signed char) primitiveScaledFontLockFaceAccessorDepth = 1;
EXPORT(signed char) primitiveScaledFontUnlockFaceAccessorDepth = 1;
EXPORT(signed char) primitiveScaledFontGlyphExtentsAccessorDepth = 1;
EXPORT(signed char) primitiveScaledFontTextExtentsAccessorDepth = 1;
EXPORT(signed char) primitiveScaledFontExtentsAccessorDepth = 1;
EXPORT(signed char) primitiveTextToGlyphsAccessorDepth = 1;
EXPORT(signed char) primitiveGlyphFreeAccessorDepth = 0;
EXPORT(signed char) primitiveTextPathAccessorDepth = 1;
EXPORT(signed char) primitiveFillRectangleColorAccessorDepth = 2;
EXPORT(signed char) primitiveStrokeRectangleAccessorDepth = 2;
EXPORT(signed char) primitiveFillPathColorAccessorDepth = 2;
EXPORT(signed char) primitiveStrokePathAccessorDepth = 2;
EXPORT(signed char) primitiveShowStringAccessorDepth = 3;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
