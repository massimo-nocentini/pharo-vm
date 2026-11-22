/* gluetest.c -- the glue CMarkPlugin.c against the fake VM of fakevm.c,
 * linked with the engine (src/engine) and cmark-gfm, built with
 * -DCM_TESTING.
 *
 * Built twice by run-c-tests.sh: with the glue as the shared library
 * compiles it (its primitives and *AccessorDepth variables are globals),
 * and with -DSQUEAK_BUILTIN_PLUGIN (GLUE_BUILTIN here), where the
 * primitives are static and found by name in CMarkPlugin_exports, as the
 * Emscripten VM finds them.  The checks:
 *
 *   G1  module    setInterpreter, the module name and functions, the
 *                 accessor depths (all 0), the rows of the builtin table
 *   G2  info      primitiveInfo: its 7 slots and their values, and
 *                 PrimErrNoMemory when any of its allocations fails
 *   G3  arity     every primitive with one argument less and one more
 *   G4  failures  every failure code, each check alone, then pairs where the
 *                 earlier check of the contract's order wins; PrimErrNotFound
 *                 (cm_test_drop_extension) only after every BadArgument
 *                 check; nothing answered after a failure
 *   G5  sources   ByteArray, ByteString, WideString, another words object
 *                 (read as UTF-32); ByteSymbol and Array refused
 *   G6  shapes    the Array of 9: a Bitmap of 16 * n words equal to the
 *                 engine's tape, the strings, the SmallInteger slots, nils
 *                 for statuses 2, 3, 4; sanitised only for a ByteArray with a
 *                 bad byte (then T's bytes); the empty document
 *   G7  budget    budget 0: OVER_BUDGET with the need of 1.5 computed here,
 *                 then budget -1: OK by reuse (stats reused + 1); the same
 *                 for primitiveRender
 *   G8  memory    PrimErrNoMemory at each allocation of a parse (and of a
 *                 render): the result kept, the retry answers it by reuse
 *   G9  release   primitiveRelease answers the kept bytes, then 0;
 *                 shutdownModule frees the kept result
 *   G10 render    the answer of every format, ascii true and false, width
 *
 * Every call is also checked by the fake VM (fakevm.h): no answer after a
 * failure, no failure after an answer, no store into an object the call did
 * not make, none of the proxy's eden-only allocators.
 *
 * Prints one line per failed check (the first 50) and "gluetest ok: N
 * checks"; exits 1 when a check failed.
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fakevm.h"
#include "cmEngine.h"

typedef sqInt (*Primitive)(void);

#ifdef GLUE_BUILTIN
extern void *CMarkPlugin_exports[][3];
#else
extern const char *getModuleName(void);
extern sqInt initialiseModule(void);
extern sqInt shutdownModule(void);
extern sqInt setInterpreter(struct VirtualMachine *);
extern sqInt primitiveInfo(void);
extern sqInt primitiveParse(void);
extern sqInt primitiveRelease(void);
extern sqInt primitiveRender(void);
extern signed char primitiveInfoAccessorDepth, primitiveParseAccessorDepth, primitiveReleaseAccessorDepth,
	primitiveRenderAccessorDepth;
#endif

static int checks, failures;
static const char *section = "";

static void
fail(const char *format, ...)
{
	va_list args;

	if (++failures <= 50) {
		va_start(args, format);
		printf("%s FAILED: ", section);
		vprintf(format, args);
		printf("\n");
		va_end(args);
	}
}

#define CHECK(condition, ...) do { checks++; if (!(condition)) fail(__VA_ARGS__); } while (0)

static Primitive pInfo, pParse, pRender, pRelease;
static sqInt (*shutdown)(void);

#ifdef GLUE_BUILTIN
static void *
entry(const char *name)
{
	int i;

	for (i = 0; CMarkPlugin_exports[i][1]; i++) {
		if (strcmp(CMarkPlugin_exports[i][1], name) == 0) {
			return CMarkPlugin_exports[i][2];
		}
	}
	return NULL;
}
#endif

/*** Calls ***/

/* A call that must answer: its value, with no protocol error */
static FakeAnswer
call(Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = fake_call(p, fakeNil, nargs, args);

	CHECK(a.protocolErrors == 0, "protocol error: %s", fake_last_error());
	return a;
}

static sqInt
answerOf(Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = call(p, nargs, args);

	CHECK(a.code == 0 && a.returned, "the call failed with %lld", (long long)a.code);
	return a.returned ? a.value : fakeNil;
}

static sqInt
failureOf(Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = call(p, nargs, args);

	CHECK(!a.returned, "a failing call answered");
	return a.code;
}

static sqInt
bytesOop(sqInt cls, const char *s)
{
	return fake_bytes(cls, s, strlen(s));
}

/* The 6 arguments of primitiveParse */
static void
parseArgs(sqInt *a, sqInt source, sqInt options, sqInt extensions, sqInt maxDepth, sqInt budget, sqInt cBudget)
{
	a[0] = source;
	a[1] = fake_int(options);
	a[2] = fake_int(extensions);
	a[3] = fake_int(maxDepth);
	a[4] = fake_int(budget);
	a[5] = fake_int(cBudget);
}

/* The 7 arguments of primitiveRender */
static void
renderArgs(sqInt *a, sqInt source, sqInt format, sqInt options, sqInt extensions, sqInt width, sqInt budget,
           sqInt cBudget)
{
	a[0] = source;
	a[1] = fake_int(format);
	a[2] = fake_int(options);
	a[3] = fake_int(extensions);
	a[4] = fake_int(width);
	a[5] = fake_int(budget);
	a[6] = fake_int(cBudget);
}

static sqInt
intSlot(sqInt array, size_t i)
{
	sqInt v = fake_slot(array, i);

	CHECK(fake_is_int(v), "slot %zu is not a SmallInteger", i + 1);
	return fake_int_value(v);
}

/* Slot i (0-based) of primitiveInfo's stats */
static sqInt
statistic(size_t i)
{
	sqInt info = answerOf(pInfo, 0, NULL);

	return intSlot(fake_slot(info, 6), i);
}

static int
stringIs(sqInt oop, const char *s)
{
	return fake_is_object(oop) && fake_class_of(oop) == fakeByteString && fake_size(oop) == strlen(s)
		&& memcmp(fake_body(oop), s, strlen(s)) == 0;
}


/*** The SimdUTF formulas of 1.5, written again ***/

static uint64_t
objectBytes(uint64_t slots)
{
	if (slots == 0) {
		slots = 1;
	}
	return 8 + (slots >= 255 ? 8 : 0) + 8 * slots;
}


/*** G1 module ***/

static void
checkModule(struct VirtualMachine *vm)
{
	section = "G1 module";
#ifdef GLUE_BUILTIN
	{
		static const char *names[] = { "getModuleName", "initialiseModule", "primitiveInfo", "primitiveParse",
			"primitiveRelease", "primitiveRender", "setInterpreter", "shutdownModule" };
		static const int depths[] = { -1, -1, 0, 0, 0, 0, -1, -1 };
		const char *(*moduleName)(void) = (const char *(*)(void))entry("getModuleName");
		sqInt (*init)(void) = (sqInt (*)(void))entry("initialiseModule");
		sqInt (*set)(struct VirtualMachine *) = (sqInt (*)(struct VirtualMachine *))entry("setInterpreter");
		size_t i;

		shutdown = (sqInt (*)(void))entry("shutdownModule");
		for (i = 0; CMarkPlugin_exports[i][1]; i++) {
			const char *name = CMarkPlugin_exports[i][1];
			CHECK(strcmp(CMarkPlugin_exports[i][0], "CMarkPlugin") == 0, "row %zu: module", i);
			CHECK(i < 8 && strcmp(name, names[i]) == 0 && CMarkPlugin_exports[i][2], "row %zu: %s", i, name);
			if (i < 8 && depths[i] >= 0) {
				CHECK(name[strlen(name) + 1] == depths[i], "row %zu: depth", i);
			}
		}
		CHECK(i == 8 && CMarkPlugin_exports[i][0] == NULL && CMarkPlugin_exports[i][2] == NULL, "%zu rows", i);
		CHECK(set && set(vm) == 1, "setInterpreter");
		CHECK(moduleName && strcmp(moduleName(), "CMarkPlugin (i)") == 0, "getModuleName");
		CHECK(init && init() == 1, "initialiseModule");
		pInfo = (Primitive)entry("primitiveInfo");
		pParse = (Primitive)entry("primitiveParse");
		pRender = (Primitive)entry("primitiveRender");
		pRelease = (Primitive)entry("primitiveRelease");
	}
#else
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
	CHECK(strcmp(getModuleName(), "CMarkPlugin (e)") == 0, "getModuleName");
	CHECK(initialiseModule() == 1, "initialiseModule");
	CHECK(primitiveInfoAccessorDepth == 0 && primitiveParseAccessorDepth == 0
	      && primitiveReleaseAccessorDepth == 0 && primitiveRenderAccessorDepth == 0, "accessor depths");
	shutdown = shutdownModule;
	pInfo = primitiveInfo;
	pParse = primitiveParse;
	pRender = primitiveRender;
	pRelease = primitiveRelease;
#endif
	CHECK(pInfo && pParse && pRender && pRelease && shutdown, "the primitives");
}


/*** G2 info ***/

static void
checkInfo(void)
{
	static const char *const names[] = { "table", "strikethrough", "autolink", "tagfilter", "tasklist" };
	sqInt info, ext, limits, stats;
	int k;
	size_t i;

	section = "G2 info";
	info = answerOf(pInfo, 0, NULL);
	CHECK(fake_class_of(info) == fakeArray && fake_size(info) == 7, "7 slots");
	CHECK(intSlot(info, 0) == 1, "protocol");
	CHECK(stringIs(fake_slot(info, 1), "CMarkPlugin 1.0"), "plugin version");
	CHECK(stringIs(fake_slot(info, 2), "0.29.0.gfm.14"), "cmark-gfm version");
	CHECK(stringIs(fake_slot(info, 3), "native"), "platform");
	ext = fake_slot(info, 4);
	CHECK(fake_class_of(ext) == fakeArray && fake_size(ext) == 5, "5 extensions");
	for (i = 0; i < 5 && i < fake_size(ext); i++) {
		CHECK(stringIs(fake_slot(ext, i), names[i]), "extension %zu", i + 1);
	}
	limits = fake_slot(info, 5);
	CHECK(fake_size(limits) == 5 && intSlot(limits, 0) == 268435456 && intSlot(limits, 1) == 268435456
	      && intSlot(limits, 2) == (sqInt)4294967296LL && intSlot(limits, 3) == 16
	      && intSlot(limits, 4) == (sqInt)4294967296LL, "limits");
	stats = fake_slot(info, 6);
	CHECK(fake_size(stats) == 8, "8 stats");
	for (i = 0; i < 8; i++) {
		CHECK(intSlot(stats, i) >= 0, "stat %zu", i + 1);
	}
	/* a missing extension shortens the Array */
	cm_test_drop_extension(4);
	info = answerOf(pInfo, 0, NULL);
	ext = fake_slot(info, 4);
	CHECK(fake_size(ext) == 4 && stringIs(fake_slot(ext, 2), "tagfilter"), "a dropped extension");
	cm_test_drop_extension(-1);
	/* PrimErrNoMemory at each of its allocations */
	/* 12 allocations: the Arrays of 7, 5 names, 5 limits, 8 stats, the 3
	   strings and the 5 names */
	for (k = 0; k <= 12; k++) {
		FakeAnswer a;
		fake_set_budget(k, PrimErrNoMemory);
		a = call(pInfo, 0, NULL);
		CHECK(a.returned == (k == 12), "allocation %d: answered %d", k, a.returned);
		CHECK(a.returned || a.code == PrimErrNoMemory, "allocation %d: code %lld", k, (long long)a.code);
	}
	fake_set_budget(-1, PrimErrNoMemory);
}


/*** G3 arity, G4 failures ***/

static void
checkArity(void)
{
	sqInt a[8];
	int i;

	section = "G3 arity";
	for (i = 0; i < 8; i++) {
		a[i] = fake_int(0);
	}
	a[0] = bytesOop(fakeByteArray, "# x");
	CHECK(failureOf(pInfo, 1, a) == PrimErrBadNumArgs, "info with 1");
	CHECK(failureOf(pRelease, 1, a) == PrimErrBadNumArgs, "release with 1");
	CHECK(failureOf(pParse, 5, a) == PrimErrBadNumArgs, "parse with 5");
	CHECK(failureOf(pParse, 7, a) == PrimErrBadNumArgs, "parse with 7");
	CHECK(failureOf(pRender, 6, a) == PrimErrBadNumArgs, "render with 6");
	CHECK(failureOf(pRender, 8, a) == PrimErrBadNumArgs, "render with 8");
}

/* One bad argument of a parse (index, value) */
typedef struct {
	int index;
	sqInt value;
	const char *what;
} Bad;

static void
checkFailures(void)
{
	sqInt good = bytesOop(fakeByteArray, "- [x] t\n");
	sqInt a[7], b[7];
	Bad parseBad[] = {
		{ 0, 0, "source SmallInteger" }, { 0, 0, "source Array" }, { 0, 0, "source ByteSymbol" },
		{ 0, 0, "source nil" }, { 0, 0, "source MyString" },
		{ 1, 0, "options nil" }, { 1, 0, "options 1" }, { 1, 0, "options 1 << 18" }, { 1, 0, "options -1" },
		{ 1, 0, "options 1 << 5" },
		{ 2, 0, "extensions 32" }, { 2, 0, "extensions -1" }, { 2, 0, "extensions true" },
		{ 3, 0, "maxDepth -1" }, { 3, 0, "maxDepth 1.5" },
		{ 4, 0, "budget -2" }, { 4, 0, "budget nil" },
		{ 5, 0, "cBudget -2" }, { 5, 0, "cBudget nil" },
	};
	size_t i, j;

	section = "G4 failures";
	parseBad[0].value = fake_int(7);
	parseBad[1].value = fake_array(fakeArray, NULL, 0);
	parseBad[2].value = bytesOop(fakeByteSymbol, "# x");
	parseBad[3].value = fakeNil;
	parseBad[4].value = bytesOop(fakeMyString, "# x");
	parseBad[5].value = fakeNil;
	parseBad[6].value = fake_int(1);
	parseBad[7].value = fake_int((sqInt)1 << 18);
	parseBad[8].value = fake_int(-1);
	parseBad[9].value = fake_int((sqInt)1 << 5);
	parseBad[10].value = fake_int(32);
	parseBad[11].value = fake_int(-1);
	parseBad[12].value = fakeTrueObject;
	parseBad[13].value = fake_int(-1);
	parseBad[14].value = fake_small_float(1.5);
	parseBad[15].value = fake_int(-2);
	parseBad[16].value = fakeNil;
	parseBad[17].value = fake_int(-2);
	parseBad[18].value = fakeNil;
	parseArgs(a, good, 0, 31, 0, -1, -1);
	CHECK(fake_int_value(fake_slot(answerOf(pParse, 6, a), 0)) == 0, "the good parse");
	for (i = 0; i < sizeof parseBad / sizeof parseBad[0]; i++) {
		FakeAnswer r;
		memcpy(b, a, sizeof b);
		b[parseBad[i].index] = parseBad[i].value;
		r = call(pParse, 6, b);
		CHECK(!r.returned && r.code == PrimErrBadArgument && r.allocations == 0 && r.stores == 0,
			"parse, %s: code %lld", parseBad[i].what, (long long)r.code);
		/* with the tasklist missing, the bad argument still wins */
		cm_test_drop_extension(16);
		r = call(pParse, 6, b);
		CHECK(!r.returned && r.code == PrimErrBadArgument, "parse, %s, tasklist missing: code %lld",
			parseBad[i].what, (long long)r.code);
		cm_test_drop_extension(-1);
		/* every pair is BadArgument too */
		for (j = i + 1; j < sizeof parseBad / sizeof parseBad[0]; j++) {
			if (parseBad[j].index != parseBad[i].index) {
				sqInt c[7];
				memcpy(c, b, sizeof c);
				c[parseBad[j].index] = parseBad[j].value;
				CHECK(failureOf(pParse, 6, c) == PrimErrBadArgument, "parse, %s and %s", parseBad[i].what,
					parseBad[j].what);
			}
		}
	}
	/* the options allowed */
	parseArgs(b, good, 0x3FF1E, 31, 0, -1, -1);
	CHECK(fake_int_value(fake_slot(answerOf(pParse, 6, b), 0)) == 0, "options 0x3FF1E");
	/* render: width and format */
	renderArgs(a, good, 1, 0, 31, 0, -1, -1);
	CHECK(fake_int_value(fake_slot(answerOf(pRender, 7, a), 0)) == 0, "the good render");
	{
		static const struct { int index; sqInt value; const char *what; } renderBad[] = {
			{ 1, 0, "format 0" }, { 1, 7, "format 7" }, { 4, -1, "width -1" }, { 4, 10001, "width 10001" },
			{ 2, 1, "options 1" }, { 3, 32, "extensions 32" }, { 5, -2, "budget -2" }, { 6, -2, "cBudget -2" },
		};
		for (i = 0; i < sizeof renderBad / sizeof renderBad[0]; i++) {
			memcpy(b, a, sizeof b);
			b[renderBad[i].index] = fake_int(renderBad[i].value);
			CHECK(failureOf(pRender, 7, b) == PrimErrBadArgument, "render, %s", renderBad[i].what);
		}
		memcpy(b, a, sizeof b);
		b[4] = fake_int(10000);
		CHECK(fake_int_value(fake_slot(answerOf(pRender, 7, b), 0)) == 0, "width 10000");
		memcpy(b, a, sizeof b);
		b[0] = fake_int(1);
		b[1] = fake_int(0);
		CHECK(failureOf(pRender, 7, b) == PrimErrBadArgument, "render, bad source and format");
	}
	/* PrimErrNotFound: a missing extension, only for its bit */
	cm_test_drop_extension(16);
	parseArgs(b, good, 0, 16, 0, -1, -1);
	{
		FakeAnswer r = call(pParse, 6, b);
		CHECK(!r.returned && r.code == PrimErrNotFound && r.allocations == 0, "tasklist missing: %lld",
			(long long)r.code);
	}
	parseArgs(b, good, 0, 15, 0, -1, -1);
	CHECK(fake_int_value(fake_slot(answerOf(pParse, 6, b), 0)) == 0, "tasklist missing, the other bits");
	renderArgs(b, good, 1, 0, 31, 0, -1, -1);
	CHECK(failureOf(pRender, 7, b) == PrimErrNotFound, "render, tasklist missing");
	cm_test_drop_extension(-1);
	parseArgs(b, good, 0, 16, 0, -1, -1);
	CHECK(fake_int_value(fake_slot(answerOf(pParse, 6, b), 0)) == 0, "tasklist restored");
}


/*** G5 sources, G6 shapes ***/

/* Parses source and answers the Array, checking its shape against the
   engine's own parse of the same source */
static sqInt
parseChecked(const char *what, sqInt source, const cm_source *src, int options, int extensions)
{
	sqInt a[6], answer, tape, strings;
	cm_tree t;
	uint64_t code;

	parseArgs(a, source, options, extensions, 0, -1, -1);
	answer = answerOf(pParse, 6, a);
	if (!fake_is_object(answer) || fake_size(answer) != 9) {
		CHECK(0, "%s: not an Array of 9", what);
		return answer;
	}
	CHECK(cm_parse(src, options, extensions, 0, -1, &t, &code) == CM_OK, "%s: engine", what);
	tape = fake_slot(answer, 2);
	strings = fake_slot(answer, 3);
	CHECK(intSlot(answer, 0) == 0 && intSlot(answer, 1) == 0, "%s: status", what);
	CHECK(fake_class_of(tape) == fakeBitmap && fake_format(tape) == 10 && fake_size(tape) == t.nodes * 16
	      && memcmp(fake_body(tape), t.tape, t.nodes * 64) == 0, "%s: the tape", what);
	CHECK(fake_class_of(strings) == fakeByteArray && fake_size(strings) == t.stringBytes
	      && (t.stringBytes == 0 || memcmp(fake_body(strings), t.strings, t.stringBytes) == 0), "%s: the strings", what);
	CHECK(intSlot(answer, 4) == (sqInt)t.nodes && intSlot(answer, 5) == (sqInt)t.depth && intSlot(answer, 6) == 16
	      && intSlot(answer, 7) == (sqInt)t.sourceChars, "%s: the numbers", what);
	if (t.t.differs) {
		sqInt s = fake_slot(answer, 8);
		CHECK(fake_class_of(s) == fakeByteArray && fake_size(s) == t.t.textBytes
		      && memcmp(fake_body(s), t.t.text, t.t.textBytes) == 0, "%s: sanitised", what);
	} else {
		CHECK(fake_slot(answer, 8) == fakeNil, "%s: sanitised is not nil", what);
	}
	cm_tree_free(&t);
	return answer;
}

static void
checkSources(void)
{
	static const uint32_t wide[] = { '#', ' ', 0xE9, 0x1F600, '\n', 'x' };
	static const char bad[] = "# a\xFF" "b\0c\n";
	cm_source src;
	sqInt answer;

	section = "G5 sources";
	src.kind = CM_SRC_UTF8;
	src.units = "# H\xC3\xA9 *x*\n";
	src.count = strlen(src.units);
	answer = parseChecked("ByteArray", bytesOop(fakeByteArray, src.units), &src, 0, 31);
	CHECK(intSlot(answer, 7) == 9, "ByteArray: sourceChars");
	src.kind = CM_SRC_LATIN1;
	src.units = "# H\xE9 *x*\n";
	src.count = strlen(src.units);
	answer = parseChecked("ByteString", bytesOop(fakeByteString, src.units), &src, 0, 31);
	CHECK(intSlot(answer, 7) == 9, "ByteString: sourceChars");
	src.kind = CM_SRC_UTF32;
	src.units = wide;
	src.count = 6;
	answer = parseChecked("WideString", fake_words(fakeWideString, wide, 6), &src, 0, 31);
	CHECK(intSlot(answer, 7) == 6, "WideString: sourceChars");
	parseChecked("WordArray", fake_words(fakeWordArray, wide, 6), &src, 0, 31);
	parseChecked("WideSymbol", fake_words(fakeWideSymbol, wide, 6), &src, 0, 31);
	src.kind = CM_SRC_UTF8;
	src.units = bad;
	src.count = sizeof bad - 1;
	section = "G6 shapes";
	answer = parseChecked("bad bytes", fake_bytes(fakeByteArray, bad, sizeof bad - 1), &src, 0, 31);
	CHECK(fake_slot(answer, 8) != fakeNil && fake_size(fake_slot(answer, 8)) == sizeof bad - 1 + 4,
		"bad bytes: the sanitised bytes");
	/* a Latin-1 String with NUL: T differs, but sanitised is for ByteArrays */
	src.kind = CM_SRC_LATIN1;
	src.count = sizeof bad - 1;
	parseChecked("Latin-1 with NUL", fake_bytes(fakeByteString, bad, sizeof bad - 1), &src, 0, 31);
	src.kind = CM_SRC_UTF8;
	src.units = "";
	src.count = 0;
	answer = parseChecked("empty ByteArray", fake_bytes(fakeByteArray, "", 0), &src, 0, 31);
	CHECK(fake_size(fake_slot(answer, 3)) == 0 && intSlot(answer, 4) == 1, "empty: one record, no string");
	src.kind = CM_SRC_LATIN1;
	parseChecked("empty ByteString", fake_bytes(fakeByteString, "", 0), &src, 0, 31);
	src.kind = CM_SRC_UTF32;
	src.units = wide;
	parseChecked("empty WideString", fake_words(fakeWideString, wide, 0), &src, 0, 31);
}

static void
checkStatuses(void)
{
	sqInt a[6], answer, deep;
	char text[1300];

	section = "G6 shapes";
	/* status 3 */
	memset(text, '>', 1200);
	memcpy(text + 1200, " x", 3);
	deep = bytesOop(fakeByteArray, text);
	parseArgs(a, deep, 0, 0, 100, -1, -1);
	answer = answerOf(pParse, 6, a);
	CHECK(intSlot(answer, 0) == 3 && intSlot(answer, 1) == 101 && intSlot(answer, 5) == 101, "status 3");
	CHECK(intSlot(answer, 7) == 1202, "status 3: sourceChars");
	/* status 4 */
	parseArgs(a, bytesOop(fakeByteArray, "# Hi"), 0, 31, 0, -1, 4096);
	answer = answerOf(pParse, 6, a);
	CHECK(intSlot(answer, 0) == 4 && intSlot(answer, 1) == 1, "status 4, code 1");
	CHECK(intSlot(answer, 4) == 0 && intSlot(answer, 6) == 16, "status 4: the numbers");
	/* status 2 */
	cm_test_limits(10, 0);
	parseArgs(a, bytesOop(fakeByteArray, "0123456789a"), 0, 31, 0, -1, -1);
	answer = answerOf(pParse, 6, a);
	CHECK(intSlot(answer, 0) == 2 && intSlot(answer, 1) == 11, "status 2, code 11");
	cm_test_limits(0, 0);
	/* the nils of a status other than 0 */
	CHECK(fake_slot(answer, 2) == fakeNil && fake_slot(answer, 3) == fakeNil && fake_slot(answer, 8) == fakeNil,
		"status 2: nils");
	/* render statuses */
	{
		sqInt r[7];
		renderArgs(r, bytesOop(fakeByteArray, "# Hi"), 3, 0, 31, 0, -1, 512);
		answer = answerOf(pRender, 7, r);
		CHECK(fake_size(answer) == 5 && intSlot(answer, 0) == 4 && fake_slot(answer, 2) == fakeNil
		      && fake_slot(answer, 3) == fakeFalseObject && intSlot(answer, 4) == 0, "render status 4");
	}
}


/*** G7 budget, G8 memory, G9 release ***/

static void
checkBudget(void)
{
	static const char sample[] = "# T\xC3\xA9tle\n\nSome *text* with `code` and a [link](u \"t\").\n\n- [x] done\n";
	sqInt source = bytesOop(fakeByteArray, sample), bad = fake_bytes(fakeByteArray, "a\xFF" "b", 3);
	sqInt a[6], answer, r[7];
	cm_source src;
	cm_tree t;
	uint64_t code, need;
	sqInt reused;
	FakeAnswer f;
	int k;

	section = "G7 budget";
	src.kind = CM_SRC_UTF8;
	src.units = sample;
	src.count = sizeof sample - 1;
	cm_parse(&src, 0, 31, 0, -1, &t, &code);
	need = objectBytes(9) + objectBytes((t.nodes * 16 + 1) / 2) + objectBytes((t.stringBytes + 7) / 8);
	cm_tree_free(&t);
	parseArgs(a, source, 0, 31, 0, 0, -1);
	f = call(pParse, 6, a);
	CHECK(f.returned && f.allocations == 1, "budget 0: one allocation (%d)", f.allocations);
	answer = f.value;
	CHECK(intSlot(answer, 0) == 1 && (uint64_t)intSlot(answer, 1) == need, "budget 0: status %lld, need %lld (%llu)",
		(long long)intSlot(answer, 0), (long long)intSlot(answer, 1), (unsigned long long)need);
	CHECK(fake_slot(answer, 2) == fakeNil && fake_slot(answer, 3) == fakeNil && intSlot(answer, 4) == 0
	      && intSlot(answer, 6) == 16 && intSlot(answer, 7) == (sqInt)(sizeof sample - 2), "budget 0: the slots");
	CHECK(statistic(7) > 0, "budget 0: kept");
	reused = statistic(2);
	parseArgs(a, source, 0, 31, 0, (sqInt)need - 1, -1);
	CHECK(intSlot(answerOf(pParse, 6, a), 0) == 1 && statistic(2) == reused + 1, "need - 1: over budget, by reuse");
	parseArgs(a, source, 0, 31, 0, (sqInt)need, -1);
	CHECK(intSlot(answerOf(pParse, 6, a), 0) == 0 && statistic(2) == reused + 2, "need: OK, by reuse");
	CHECK(statistic(7) == 0, "OK: nothing kept");
	parseArgs(a, source, 0, 31, 0, 0, -1);
	answerOf(pParse, 6, a);
	parseArgs(a, source, 0, 31, 0, -1, -1);
	CHECK(intSlot(answerOf(pParse, 6, a), 0) == 0 && statistic(2) == reused + 3, "budget -1: OK by reuse");
	/* the need of a sanitised answer */
	src.units = "a\xFF" "b";
	src.count = 3;
	cm_parse(&src, 0, 31, 0, -1, &t, &code);
	need = objectBytes(9) + objectBytes((t.nodes * 16 + 1) / 2) + objectBytes((t.stringBytes + 7) / 8)
		+ objectBytes((t.t.textBytes + 7) / 8);
	cm_tree_free(&t);
	parseArgs(a, bad, 0, 31, 0, 0, -1);
	CHECK((uint64_t)intSlot(answerOf(pParse, 6, a), 1) == need, "sanitised: the need");
	parseArgs(a, bad, 0, 31, 0, (sqInt)need, -1);
	answer = answerOf(pParse, 6, a);
	CHECK(intSlot(answer, 0) == 0 && fake_slot(answer, 8) != fakeNil, "sanitised: OK at the need");
	/* another key frees the kept result */
	parseArgs(a, source, 0, 31, 0, 0, -1);
	answerOf(pParse, 6, a);
	parseArgs(a, source, 0, 31, 5, -1, -1);
	reused = statistic(2);
	CHECK(intSlot(answerOf(pParse, 6, a), 0) == 0 && statistic(2) == reused && statistic(7) == 0, "another key");
	/* render */
	renderArgs(r, source, 1, 0, 31, 0, 0, -1);
	answer = answerOf(pRender, 7, r);
	CHECK(intSlot(answer, 0) == 1 && fake_slot(answer, 2) == fakeNil && fake_slot(answer, 3) == fakeFalseObject
	      && intSlot(answer, 4) > 0, "render over budget");
	need = objectBytes(5) + objectBytes(((uint64_t)intSlot(answer, 4) + 7) / 8);
	CHECK((uint64_t)intSlot(answer, 1) == need, "render: the need");
	reused = statistic(2);
	renderArgs(r, source, 1, 0, 31, 0, -1, -1);
	answer = answerOf(pRender, 7, r);
	CHECK(intSlot(answer, 0) == 0 && statistic(2) == reused + 1 && fake_size(fake_slot(answer, 2)) == (size_t)intSlot(answer, 4),
		"render: OK by reuse");

	section = "G8 memory";
	for (k = 0; k < 4; k++) {
		sqInt s = k == 3 ? bad : source;
		parseArgs(a, s, 0, 31, 0, -1, -1);
		fake_set_budget(k, PrimErrNoMemory);
		f = call(pParse, 6, a);
		fake_set_budget(-1, PrimErrNoMemory);
		CHECK(!f.returned && f.code == PrimErrNoMemory, "allocation %d: code %lld", k, (long long)f.code);
		CHECK(statistic(7) > 0, "allocation %d: kept", k);
		reused = statistic(2);
		answer = answerOf(pParse, 6, a);
		CHECK(intSlot(answer, 0) == 0 && statistic(2) == reused + 1 && statistic(7) == 0, "allocation %d: the retry", k);
	}
	for (k = 0; k < 2; k++) {
		renderArgs(r, source, 2, 0, 31, 0, -1, -1);
		fake_set_budget(k, PrimErrNoMemory);
		f = call(pRender, 7, r);
		fake_set_budget(-1, PrimErrNoMemory);
		CHECK(!f.returned && f.code == PrimErrNoMemory && statistic(7) > 0, "render allocation %d", k);
		reused = statistic(2);
		CHECK(intSlot(answerOf(pRender, 7, r), 0) == 0 && statistic(2) == reused + 1, "render allocation %d: retry", k);
	}
	/* the over-budget answer itself cannot be allocated */
	parseArgs(a, source, 0, 31, 0, 0, -1);
	fake_set_budget(0, PrimErrNoMemory);
	f = call(pParse, 6, a);
	fake_set_budget(-1, PrimErrNoMemory);
	CHECK(!f.returned && f.code == PrimErrNoMemory && statistic(7) > 0, "over budget, no memory");

	section = "G9 release";
	{
		sqInt kept = statistic(7), freed;
		freed = fake_int_value(answerOf(pRelease, 0, NULL));
		CHECK(kept > 0 && freed == kept, "release: %lld of %lld", (long long)freed, (long long)kept);
		CHECK(fake_int_value(answerOf(pRelease, 0, NULL)) == 0, "release twice");
		parseArgs(a, source, 0, 31, 0, 0, -1);
		answerOf(pParse, 6, a);
		CHECK(statistic(7) > 0, "kept again");
		CHECK(shutdown() == 1 && statistic(7) == 0, "shutdownModule frees it");
	}
}


/*** G10 render ***/

static void
checkRender(void)
{
	static const char *const formats[] = { "?", "<h1>Hi</h1>\n", "<?xml", "# Hi\n", "Hi\n", "\\section{Hi}\n",
		".SH\nHi\n" };
	sqInt r[7], answer, bytes;
	int f;

	section = "G10 render";
	for (f = 1; f <= 6; f++) {
		renderArgs(r, bytesOop(fakeByteArray, "# Hi"), f, 0, 31, 0, -1, -1);
		answer = answerOf(pRender, 7, r);
		bytes = fake_slot(answer, 2);
		CHECK(fake_size(answer) == 5 && intSlot(answer, 0) == 0 && fake_class_of(bytes) == fakeByteArray
		      && (size_t)intSlot(answer, 1) == fake_size(bytes) && (size_t)intSlot(answer, 4) == fake_size(bytes)
		      && fake_slot(answer, 3) == fakeTrueObject, "format %d: the answer", f);
		CHECK(fake_size(bytes) >= strlen(formats[f]) && memcmp(fake_body(bytes), formats[f], strlen(formats[f])) == 0,
			"format %d: %.*s", f, (int)fake_size(bytes), (const char *)fake_body(bytes));
	}
	renderArgs(r, bytesOop(fakeByteArray, "\xC3\xA9"), 1, 0, 0, 0, -1, -1);
	answer = answerOf(pRender, 7, r);
	CHECK(fake_slot(answer, 3) == fakeFalseObject && intSlot(answer, 4) == 10, "ascii false");
	renderArgs(r, fake_bytes(fakeByteArray, "", 0), 1, 0, 0, 0, -1, -1);
	answer = answerOf(pRender, 7, r);
	CHECK(intSlot(answer, 0) == 0 && fake_size(fake_slot(answer, 2)) == 0 && fake_slot(answer, 3) == fakeTrueObject,
		"the empty output");
	renderArgs(r, bytesOop(fakeByteArray, "one two three four five six seven\n"), 4, 0, 0, 10, -1, -1);
	answer = answerOf(pRender, 7, r);
	bytes = fake_slot(answer, 2);
	CHECK(memchr(fake_body(bytes), '\n', fake_size(bytes)) != (char *)fake_body(bytes) + fake_size(bytes) - 1,
		"width 10 wraps");
}

int
main(void)
{
	struct VirtualMachine *vm = fake_init();

	setvbuf(stdout, NULL, _IONBF, 0);
	checkModule(vm);
	checkInfo();
	checkArity();
	checkFailures();
	checkSources();
	checkStatuses();
	checkBudget();
	checkRender();
	cm_release_kept();
	fake_free_all();
	if (failures) {
		printf("gluetest FAILED: %d of %d checks\n", failures, checks);
		return 1;
	}
	printf("gluetest ok: %d checks\n", checks);
	return 0;
}
