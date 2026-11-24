/* gluetest.c -- the glue SimdCSVPlugin.c against the fake VM of fakevm.c,
 * linked with the engine (or with scstub.c).
 *
 * Built twice by glue.sh: with the glue as the shared library compiles it
 * (its primitives and *AccessorDepth variables are globals), and with
 * -DSQUEAK_BUILTIN_PLUGIN (GLUE_BUILTIN here), where the primitives are
 * static and found by name in SimdCSVPlugin_exports, as the Emscripten VM
 * finds them.  The checks:
 *
 *   G1  module    setInterpreter, the module name and functions, the
 *                 accessor depths (all 0), the rows of the builtin table
 *   G2  info      primitiveInfo: its 8 slots, and the failure of each of its
 *                 allocations
 *   G3  arity     every primitive with one argument less and one more, and
 *                 with none
 *   G4  failures  every check of the contract, alone and in pairs, where the
 *                 earlier one wins; nothing is allocated, stored or answered
 *                 by a failing call, and the destination keeps its contents
 *   G5  sources   the classes: every byte and word class, and those refused
 *   G6  scans     known answers of primitiveMeasure and primitiveIndex;
 *                 the slots of the destination outside of what was stored
 *   G7  rows      known answers: classes of the cells, types, field counts (0,
 *                 1, the cap), short types, the empty value, quoted empty
 *                 fields, numbers, errors, ranges of the index, the
 *                 destination full, budgets 0, 1 and -1
 *   G8  memory    the failure of EVERY allocation of a call, with
 *                 PrimErrNoMemory (an answer: MORE, or NO_ROOM with the need,
 *                 equal to what the successful call takes) and with
 *                 PrimErrUnsupported (a failure)
 *   G9  random    rows of random inputs against a second reader written here
 *                 from the definition of the automaton, through budgets,
 *                 small destinations and failing allocations; random indexes
 *                 (under AddressSanitizer)
 *   G10 calls     200,000 calls of random arguments
 *
 * Prints "gluetest: N checks, 0 failures" and exits 0, or 1 when a check
 * failed.
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fakevm.h"
#include "scEngine.h"

typedef sqInt (*Primitive)(void);

#ifdef GLUE_BUILTIN
extern void *SimdCSVPlugin_exports[][3];
#else
extern const char *getModuleName(void);
extern sqInt initialiseModule(void);
extern sqInt shutdownModule(void);
extern sqInt setInterpreter(struct VirtualMachine *);
extern sqInt primitiveIndex(void);
extern sqInt primitiveInfo(void);
extern sqInt primitiveMeasure(void);
extern sqInt primitiveRows(void);
extern signed char primitiveIndexAccessorDepth, primitiveInfoAccessorDepth, primitiveMeasureAccessorDepth,
	primitiveRowsAccessorDepth;
#endif

static long checks, failures;
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

static Primitive pInfo, pMeasure, pIndex, pRows;
static sqInt (*shutdown)(void);

#ifdef GLUE_BUILTIN
static void *
entry(const char *name)
{
	int i;

	for (i = 0; SimdCSVPlugin_exports[i][1]; i++) {
		if (strcmp(SimdCSVPlugin_exports[i][1], name) == 0) {
			return SimdCSVPlugin_exports[i][2];
		}
	}
	return NULL;
}
#endif


/*** Calls ***/

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

/* A failing call: it must answer, allocate and store nothing */
static sqInt
failureOf(Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = call(p, nargs, args);

	CHECK(!a.returned, "a failing call answered");
	CHECK(a.allocations == 0 && a.stores == 0, "a failing call made %d objects and %d stores", a.allocations, a.stores);
	return a.code;
}

static sqInt
intSlot(sqInt array, size_t i)
{
	sqInt v = fake_slot(array, i);

	CHECK(fake_is_int(v), "slot %zu is not a SmallInteger", i + 1);
	return fake_int_value(v);
}

/* Checks an answer of n SmallIntegers */
static void
checkInts(sqInt array, const char *what, int n, ...)
{
	va_list args;
	int i;

	CHECK(fake_is_object(array) && fake_class_of(array) == fakeArray && fake_size(array) == (size_t)n, "%s: the Array of %d", what, n);
	if (!fake_is_object(array) || fake_class_of(array) != fakeArray || fake_size(array) != (size_t)n) {
		return;
	}
	va_start(args, n);
	for (i = 0; i < n; i++) {
		long long want = va_arg(args, int), got = (long long)intSlot(array, (size_t)i);

		CHECK(got == want, "%s: slot %d is %lld, not %lld", what, i + 1, got, want);
	}
	va_end(args);
}

static sqInt
bytesOop(sqInt cls, const char *s)
{
	return fake_bytes(cls, s, strlen(s));
}

static int
stringIs(sqInt oop, const char *s)
{
	return fake_is_object(oop) && fake_class_of(oop) == fakeByteString && fake_size(oop) == strlen(s)
		&& memcmp(fake_body(oop), s, strlen(s)) == 0;
}

static int
floatIs(sqInt oop, double value)
{
	double v;

	if (!fake_is_float(oop)) {
		return 0;
	}
	v = fake_float_value(oop);
	return memcmp(&v, &value, 8) == 0 && ((oop & 7) == 4) == fake_is_small_float_value(value);
}

/* A dialect word */
static sqInt
word(uint32_t separator, uint32_t quote, uint32_t flags)
{
	return fake_int((sqInt)((uint64_t)separator | ((uint64_t)quote << 21) | ((uint64_t)flags << 29)));
}

static sqInt
comma(uint32_t flags)
{
	return word(',', '"', flags | SC_F_FINAL);
}

static sqInt
measure(sqInt source, sqInt from, sqInt to, sqInt dialect)
{
	sqInt a[4];

	a[0] = source;
	a[1] = fake_int(from);
	a[2] = fake_int(to);
	a[3] = dialect;
	return answerOf(pMeasure, 4, a);
}

/* The 7 arguments of primitiveIndex */
static void
indexArgs(sqInt *a, sqInt source, sqInt base, sqInt from, sqInt to, sqInt dialect, sqInt dest, sqInt destFrom)
{
	a[0] = source;
	a[1] = fake_int(base);
	a[2] = fake_int(from);
	a[3] = fake_int(to);
	a[4] = dialect;
	a[5] = dest;
	a[6] = fake_int(destFrom);
}

static sqInt
indexOf(sqInt source, sqInt base, sqInt from, sqInt to, sqInt dialect, sqInt dest, sqInt destFrom)
{
	sqInt a[7];

	indexArgs(a, source, base, from, to, dialect, dest, destFrom);
	return answerOf(pIndex, 7, a);
}

/* The 14 arguments of primitiveRows */
enum { R_SOURCE, R_BASE, R_TO, R_DIALECT, R_INDEX, R_FROM, R_LAST, R_TYPES, R_COUNT, R_DEST, R_DESTFROM, R_WIDE, R_EMPTY,
       R_BUDGET, R_ARGS };

static void
rowsArgs(sqInt *a, sqInt source, sqInt dialect, sqInt index, sqInt entries, sqInt types, sqInt fieldCount, sqInt dest)
{
	a[R_SOURCE] = source;
	a[R_BASE] = fake_int(1);
	a[R_TO] = fake_int((sqInt)fake_size(source));
	a[R_DIALECT] = dialect;
	a[R_INDEX] = index;
	a[R_FROM] = fake_int(1);
	a[R_LAST] = fake_int(entries);
	a[R_TYPES] = types;
	a[R_COUNT] = fake_int(fieldCount);
	a[R_DEST] = dest;
	a[R_DESTFROM] = fake_int(1);
	a[R_WIDE] = fakeWideString;
	a[R_EMPTY] = fakeNil;
	a[R_BUDGET] = fake_int(-1);
}

static FakeAnswer
callRows(const sqInt *a)
{
	FakeAnswer r;

	fake_allow_store(a[R_DEST]);
	r = call(pRows, R_ARGS, a);
	fake_allow_store(0);
	return r;
}

static sqInt
rowsOf(const sqInt *a)
{
	FakeAnswer r = callRows(a);

	CHECK(r.code == 0 && r.returned, "rows failed with %lld", (long long)r.code);
	return r.returned ? r.value : fakeNil;
}

/* The index of the source, a WordArray of 64 slots, and its entries */
static sqInt
indexed(sqInt source, sqInt dialect, sqInt *entries)
{
	sqInt index = fake_new(fakeWordArray, 64);
	sqInt answer = indexOf(source, 1, 1, (sqInt)fake_size(source), dialect, index, 1);

	CHECK(intSlot(answer, 0) == SC_OK, "indexed: status %lld", (long long)intSlot(answer, 0));
	*entries = intSlot(answer, 1);
	return index;
}

/* The rows of text with a dialect, into a new dest of 16 slots */
static sqInt
rowsOfText(const char *text, sqInt dialect, sqInt types, sqInt fieldCount, sqInt *dest)
{
	sqInt a[R_ARGS], entries, source = bytesOop(fakeByteArray, text);
	sqInt index = indexed(source, dialect, &entries);

	*dest = fake_new(fakeArray, 16);
	rowsArgs(a, source, dialect, index, entries, types, fieldCount, *dest);
	return rowsOf(a);
}

static int
isRow(sqInt row, size_t size)
{
	return fake_is_object(row) && fake_class_of(row) == fakeArray && fake_size(row) == size;
}

/* Whether the cell i of row is the string s */
static int
cellIs(sqInt row, size_t i, const char *s)
{
	return fake_is_object(row) && fake_class_of(row) == fakeArray && i < fake_size(row) && stringIs(fake_slot(row, i), s);
}


/*** G1 module ***/

static void
checkModule(struct VirtualMachine *vm)
{
	section = "G1 module";
#ifdef GLUE_BUILTIN
	{
		static const char *names[] = { "getModuleName", "initialiseModule", "primitiveIndex", "primitiveInfo",
			"primitiveMeasure", "primitiveRows", "setInterpreter", "shutdownModule" };
		static const int depths[] = { -1, -1, 0, 0, 0, 0, -1, -1 };
		const char *(*moduleName)(void) = (const char *(*)(void))entry("getModuleName");
		sqInt (*init)(void) = (sqInt (*)(void))entry("initialiseModule");
		sqInt (*set)(struct VirtualMachine *) = (sqInt (*)(struct VirtualMachine *))entry("setInterpreter");
		size_t i;

		shutdown = (sqInt (*)(void))entry("shutdownModule");
		for (i = 0; SimdCSVPlugin_exports[i][1]; i++) {
			const char *name = SimdCSVPlugin_exports[i][1];
			CHECK(strcmp(SimdCSVPlugin_exports[i][0], "SimdCSVPlugin") == 0, "row %zu: module", i);
			CHECK(i < 8 && strcmp(name, names[i]) == 0 && SimdCSVPlugin_exports[i][2], "row %zu: %s", i, name);
			if (i < 8 && depths[i] >= 0) {
				CHECK(name[strlen(name) + 1] == depths[i], "row %zu: depth", i);
			}
		}
		CHECK(i == 8 && SimdCSVPlugin_exports[i][0] == NULL && SimdCSVPlugin_exports[i][2] == NULL, "%zu rows", i);
		CHECK(set && set(vm) == 1, "setInterpreter");
		CHECK(moduleName && strcmp(moduleName(), "SimdCSVPlugin (i)") == 0, "getModuleName");
		CHECK(init && init() == 1, "initialiseModule");
		pInfo = (Primitive)entry("primitiveInfo");
		pMeasure = (Primitive)entry("primitiveMeasure");
		pIndex = (Primitive)entry("primitiveIndex");
		pRows = (Primitive)entry("primitiveRows");
	}
#else
	CHECK(setInterpreter(vm) == 1, "setInterpreter");
	CHECK(strcmp(getModuleName(), "SimdCSVPlugin (e)") == 0, "getModuleName");
	CHECK(initialiseModule() == 1, "initialiseModule");
	CHECK(primitiveIndexAccessorDepth == 0 && primitiveInfoAccessorDepth == 0
	      && primitiveMeasureAccessorDepth == 0 && primitiveRowsAccessorDepth == 0, "accessor depths");
	shutdown = shutdownModule;
	pInfo = primitiveInfo;
	pMeasure = primitiveMeasure;
	pIndex = primitiveIndex;
	pRows = primitiveRows;
#endif
	CHECK(pInfo && pMeasure && pIndex && pRows && shutdown, "the primitives");
}


/*** G2 info ***/

static void
checkInfo(void)
{
	sqInt info, kernels;
	FakeAnswer a;
	size_t i;
	int k, total, n = sc_kernel_count();

	section = "G2 info";
	a = call(pInfo, 0, NULL);
	CHECK(a.returned, "info answers");
	info = a.value;
	total = a.allocations;
	CHECK(isRow(info, 8), "8 slots");
	CHECK(intSlot(info, 0) == 1, "protocol");
	CHECK(stringIs(fake_slot(info, 1), "SimdCSVPlugin 1.0"), "plugin version");
	CHECK(stringIs(fake_slot(info, 2), "a9494a8"), "simdcsv version");
	CHECK(stringIs(fake_slot(info, 3), sc_kernel()), "kernel");
	CHECK(stringIs(fake_slot(info, 4), "native"), "platform");
	kernels = fake_slot(info, 5);
	CHECK(n >= 1 && isRow(kernels, (size_t)n), "%d kernels", n);
	for (i = 0; isRow(kernels, (size_t)n) && i < (size_t)n; i++) {
		CHECK(stringIs(fake_slot(kernels, i), sc_kernel_name((int)i)), "kernel %zu", i + 1);
	}
	CHECK(sc_forced_note()[0] ? stringIs(fake_slot(info, 6), sc_forced_note()) : fake_slot(info, 6) == fakeNil, "forced note");
	CHECK(intSlot(info, 7) == 4294967294LL, "max span");
	for (k = 0; k <= total; k++) {
		fake_set_budget(k, PrimErrNoMemory);
		a = call(pInfo, 0, NULL);
		CHECK(a.returned == (k == total), "allocation %d: answered %d", k, a.returned);
		CHECK(a.returned || a.code == PrimErrNoMemory, "allocation %d: code %lld", k, (long long)a.code);
		fake_set_budget(k, PrimErrUnsupported);
		a = call(pInfo, 0, NULL);
		CHECK(a.returned || a.code == PrimErrUnsupported, "allocation %d: code %lld", k, (long long)a.code);
	}
	fake_set_budget(-1, PrimErrNoMemory);
}


/*** G3 arity ***/

static void
checkArity(void)
{
	sqInt a[15];
	int i;

	section = "G3 arity";
	for (i = 0; i < 15; i++) {
		a[i] = fake_int(1);
	}
	CHECK(failureOf(pInfo, 1, a) == PrimErrBadNumArgs, "info with 1");
	CHECK(failureOf(pMeasure, 0, a) == PrimErrBadNumArgs, "measure with 0");
	CHECK(failureOf(pMeasure, 3, a) == PrimErrBadNumArgs, "measure with 3");
	CHECK(failureOf(pMeasure, 5, a) == PrimErrBadNumArgs, "measure with 5");
	CHECK(failureOf(pIndex, 0, a) == PrimErrBadNumArgs, "index with 0");
	CHECK(failureOf(pIndex, 6, a) == PrimErrBadNumArgs, "index with 6");
	CHECK(failureOf(pIndex, 8, a) == PrimErrBadNumArgs, "index with 8");
	CHECK(failureOf(pRows, 0, a) == PrimErrBadNumArgs, "rows with 0");
	CHECK(failureOf(pRows, 13, a) == PrimErrBadNumArgs, "rows with 13");
	CHECK(failureOf(pRows, 15, a) == PrimErrBadNumArgs, "rows with 15");
}


/*** G4 failures ***/

/* A bad argument: its index, its value, the stage of its check, the code */
typedef struct {
	int index;
	sqInt value;
	int stage;
	sqInt code;
	const char *what;
} Bad;

#define MAX_BAD 128
static Bad bad[MAX_BAD];
static int bads;

static void
addBad(int index, sqInt value, int stage, sqInt code, const char *what)
{
	if (bads == MAX_BAD) {
		printf("gluetest: MAX_BAD\n");
		exit(2);
	}
	bad[bads].index = index;
	bad[bads].value = value;
	bad[bads].stage = stage;
	bad[bads].code = code;
	bad[bads].what = what;
	bads++;
}

/* Every bad argument alone, and every pair of two different arguments, where
 * the earlier stage wins; dest is the index of the destination, or -1 */
static void
checkBads(Primitive p, const char *name, int nargs, const sqInt *good, int dest)
{
	int i, j;
	sqInt b[R_ARGS];
	size_t k;

	for (i = 0; i < bads; i++) {
		memcpy(b, good, (size_t)nargs * sizeof b[0]);
		b[bad[i].index] = bad[i].value;
		CHECK(failureOf(p, nargs, b) == bad[i].code, "%s, %s", name, bad[i].what);
		for (j = 0; j < bads; j++) {
			Bad *first;
			sqInt c[R_ARGS], code;

			if (bad[j].index == bad[i].index) {
				continue;
			}
			first = bad[j].stage < bad[i].stage ? &bad[j] : &bad[i];
			memcpy(c, b, (size_t)nargs * sizeof c[0]);
			c[bad[j].index] = bad[j].value;
			code = failureOf(p, nargs, c);
			CHECK(code == first->code || (bad[i].stage == bad[j].stage && code == bad[j].code), "%s, %s and %s: %lld", name,
				bad[i].what, bad[j].what, (long long)code);
		}
	}
	for (k = 0; dest >= 0 && k < fake_body_bytes(good[dest]); k++) {
		CHECK(((unsigned char *)fake_body(good[dest]))[k] == 0xA5, "%s: the destination was written", name);
	}
}

/* The bad sources (argument 0), numbers (the arguments of the list, ended by
 * -1) and dialects (argument dialect) that the three primitives share */
static void
addCommon(const int *numbers, int dialect)
{
	static const struct { uint32_t separator, quote, flags; const char *what; } dialects[] = {
		{ ',', ',', 0, "separator = quote" }, { '\n', '"', 0, "separator LF" }, { '\r', '"', 0, "separator CR" },
		{ '\n', '"', SC_F_NO_QUOTE, "separator LF, no quote" },
		{ ',', '\n', 0, "quote LF" }, { ',', '\r', 0, "quote CR" }, { ',', '\r', SC_F_NO_QUOTE, "quote CR, no quote" },
		{ ',', '"', 512, "flag 512" }, { ',', '"', 0x8000, "flag 0x8000" }, { ',', '"', 0x10000, "bit 45" },
		{ 128, '"', 0, "separator 128 of UTF-8" }, { ',', 128, 0, "quote 128 of UTF-8" }, { 256, '"', 0, "separator 256 of UTF-8" },
		{ 256, '"', SC_F_LATIN1, "separator 256 of Latin-1" }, { 0x1FFFFF, '"', SC_F_LATIN1, "separator 0x1FFFFF of Latin-1" },
	};
	size_t i;

	addBad(0, fake_int(7), 2, PrimErrBadArgument, "source SmallInteger");
	addBad(0, fake_character('a'), 2, PrimErrBadArgument, "source Character");
	addBad(0, fake_small_float(1.5), 2, PrimErrBadArgument, "source SmallFloat64");
	addBad(0, fakeNil, 2, PrimErrBadArgument, "source nil");
	addBad(0, fake_new(fakeArray, 4), 2, PrimErrBadArgument, "source Array");
	addBad(0, fake_compiled_method(64), 2, PrimErrBadArgument, "source CompiledMethod");
	addBad(0, fake_compiled_method(0), 2, PrimErrBadArgument, "source empty CompiledMethod");
	addBad(0, fake_new(fakeDoubleByteArray, 32), 2, PrimErrBadArgument, "source DoubleByteArray");
	addBad(0, fake_new(fakeFloat64Array, 32), 2, PrimErrBadArgument, "source Float64Array");
	addBad(0, fake_forwarder(), 2, PrimErrBadArgument, "source forwarder");
	addBad(0, fakeByteString, 2, PrimErrBadArgument, "source a class");
	for (; *numbers >= 0; numbers++) {
		addBad(*numbers, fakeNil, 3, PrimErrBadArgument, "a number nil");
		addBad(*numbers, fake_small_float(1.0), 3, PrimErrBadArgument, "a number 1.0");
	}
	for (i = 0; i < sizeof dialects / sizeof dialects[0]; i++) {
		addBad(dialect, word(dialects[i].separator, dialects[i].quote, dialects[i].flags), 4, PrimErrBadArgument, dialects[i].what);
	}
	addBad(dialect, fake_int(-1), 4, PrimErrBadArgument, "dialect -1");
	addBad(dialect, fake_int(((sqInt)1 << 60) - 1), 4, PrimErrBadArgument, "dialect 2^60 - 1");
	addBad(dialect, fake_int((sqInt)1 << 45), 4, PrimErrBadArgument, "dialect 2^45");
}

static sqInt
marked(sqInt cls, size_t size)
{
	sqInt oop = fake_new(cls, size);

	memset(fake_body(oop), 0xA5, fake_body_bytes(oop));
	return oop;
}

static sqInt
immutable(sqInt oop)
{
	fake_set_immutable(oop, 1);
	return oop;
}

static void
checkFailures(void)
{
	static const int measureNumbers[] = { 1, 2, 3, -1 }, indexNumbers[] = { 1, 2, 3, 4, 6, -1 };
	static const int rowsNumbers[] = { R_BASE, R_TO, R_DIALECT, R_FROM, R_LAST, R_COUNT, R_DESTFROM, R_BUDGET, -1 };
	sqInt source = bytesOop(fakeByteArray, "a,b\n1,\"x,y\"\n"), wide = fake_new(fakeWideString, 12);
	sqInt a[R_ARGS], index, types, entries;
	sqInt stores;

	section = "G4 failures";

	/* measure: 1..12 */
	bads = 0;
	addCommon(measureNumbers, 3);
	addBad(1, fake_int(0), 5, PrimErrBadIndex, "from 0");
	addBad(1, fake_int(-1), 5, PrimErrBadIndex, "from -1");
	addBad(1, fake_int(14), 5, PrimErrBadIndex, "from to + 2");
	addBad(1, fake_int(-((sqInt)1 << 60)), 5, PrimErrBadIndex, "from SmallInteger minVal");
	addBad(2, fake_int(13), 5, PrimErrBadIndex, "to size + 1");
	addBad(2, fake_int(-1), 5, PrimErrBadIndex, "to -1");
	addBad(2, fake_int(((sqInt)1 << 60) - 1), 5, PrimErrBadIndex, "to SmallInteger maxVal");
	a[0] = source;
	a[1] = fake_int(1);
	a[2] = fake_int(12);
	a[3] = comma(0);
	checkInts(answerOf(pMeasure, 4, a), "the good measure", 7, SC_OK, 2, 4, 13, 0, 0, 0);
	checkBads(pMeasure, "measure", 4, a, -1);

	/* index */
	bads = 0;
	addCommon(indexNumbers, 4);
	addBad(1, fake_int(0), 5, PrimErrBadIndex, "base 0");
	addBad(1, fake_int(2), 5, PrimErrBadIndex, "base > from");
	addBad(2, fake_int(0), 5, PrimErrBadIndex, "from 0");
	addBad(2, fake_int(14), 5, PrimErrBadIndex, "from to + 2");
	addBad(3, fake_int(13), 5, PrimErrBadIndex, "to size + 1");
	addBad(3, fake_int(-1), 5, PrimErrBadIndex, "to -1");
	addBad(6, fake_int(-1), 5, PrimErrBadIndex, "destFrom -1");
	addBad(6, fake_int(10), 5, PrimErrBadIndex, "destFrom size + 2");
	addBad(6, fake_int(((sqInt)1 << 60) - 1), 5, PrimErrBadIndex, "destFrom SmallInteger maxVal");
	/* (the pairs of a destFrom with a destination of another class: the
	   class is asked first, stage 4.5) */
	addBad(5, marked(fakeByteArray, 32), 7, PrimErrBadArgument, "dest ByteArray");
	addBad(5, immutable(marked(fakeWordArray, 8)), 8, PrimErrNoModification, "dest immutable");
	indexArgs(a, source, 1, 1, 12, comma(0), marked(fakeWordArray, 8), 1);
	stores = intSlot(answerOf(pIndex, 7, a), 1);
	CHECK(stores == 4, "the good index");
	a[5] = marked(fakeWordArray, 8);
	{
		int i;

		/* with a dest of another class, the range of destFrom is not asked:
		   its class fails */
		for (i = 0; i < bads; i++) {
			if (bad[i].index == 6 && bad[i].code == PrimErrBadIndex) {
				sqInt b[R_ARGS];

				memcpy(b, a, sizeof b);
				b[6] = bad[i].value;
				b[5] = marked(fakeByteArray, 32);
				CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, %s and dest ByteArray", bad[i].what);
				b[5] = immutable(marked(fakeWordArray, 8));
				CHECK(failureOf(pIndex, 7, b) == PrimErrBadIndex, "index, %s and dest immutable", bad[i].what);
				bad[i--] = bad[--bads];
			}
		}
		addBad(6, fake_int(10), 5, PrimErrBadIndex, "destFrom size + 2");
		for (i = 0; i < bads; i++) {	/* (it stays, without the ByteArray) */
			if (bad[i].index == 5 && bad[i].stage == 7) {
				bad[i] = bad[--bads];
				break;
			}
		}
	}
	checkBads(pIndex, "index", 7, a, 5);
	/* the classes of dest, with good ranges */
	{
		sqInt b[7], dests[12];
		size_t i, n = 0;

		memcpy(b, a, sizeof b);
		dests[n++] = fakeNil;
		dests[n++] = fake_int(1);
		dests[n++] = fake_new(fakeArray, 8);
		dests[n++] = fake_new(fakeByteArray, 32);
		dests[n++] = fake_new(fakeByteString, 32);
		dests[n++] = fake_new(fakeDoubleByteArray, 16);
		dests[n++] = fake_new(fakeFloat64Array, 8);
		dests[n++] = fake_compiled_method(32);
		dests[n++] = fake_forwarder();
		dests[n++] = source;
		for (i = 0; i < n; i++) {
			b[5] = dests[i];
			CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, dest %zu", i);
			b[4] = word(',', ',', 0);
			b[0] = fakeNil;
			CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, dest %zu, dialect, source", i);
			b[4] = a[4];
			b[0] = a[0];
			b[3] = fake_int(13);
			CHECK(failureOf(pIndex, 7, b) == PrimErrBadIndex, "index, dest %zu and to", i);
			b[3] = a[3];
		}
		/* dest = source, words: not written */
		indexArgs(b, wide, 1, 1, 12, comma(0), wide, 1);
		CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, dest = source");
		fake_set_immutable(wide, 1);
		CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, dest = source, immutable");
		fake_set_immutable(wide, 0);
		b[0] = wide;
		b[4] = comma(SC_F_LATIN1);
		b[5] = a[5];
		CHECK(failureOf(pIndex, 7, b) == PrimErrBadArgument, "index, words and LATIN1");
		b[4] = word(0x1FFFFF, 255, 0);
		CHECK(intSlot(answerOf(pIndex, 7, b), 0) == SC_OK, "index, words, separator 0x1FFFFF, quote 255");
		/* an immutable source or index is only read */
		indexArgs(b, immutable(bytesOop(fakeByteString, "a,b")), 1, 1, 3, comma(0), fake_new(fakeWordArray, 8), 1);
		CHECK(intSlot(answerOf(pIndex, 7, b), 1) == 2, "index, immutable source");
	}

	/* rows */
	index = indexed(source, comma(0), &entries);
	types = fake_bytes(fakeByteArray, "\1\2", 2);
	bads = 0;
	addCommon(rowsNumbers, R_DIALECT);
	addBad(R_BASE, fake_int(0), 5, PrimErrBadIndex, "base 0");
	addBad(R_BASE, fake_int(14), 5, PrimErrBadIndex, "base to + 2");
	addBad(R_TO, fake_int(13), 5, PrimErrBadIndex, "to size + 1");
	addBad(R_TO, fake_int(-1), 5, PrimErrBadIndex, "to -1");
	addBad(R_FROM, fake_int(0), 5, PrimErrBadIndex, "indexFrom 0");
	addBad(R_FROM, fake_int(6), 5, PrimErrBadIndex, "indexFrom indexTo + 2");
	addBad(R_LAST, fake_int(65), 5, PrimErrBadIndex, "indexTo size + 1");
	addBad(R_LAST, fake_int(-1), 5, PrimErrBadIndex, "indexTo -1");
	addBad(R_COUNT, fake_int(-1), 5, PrimErrBadIndex, "fieldCount -1");
	addBad(R_COUNT, fake_int(SC_MAX_FIELD_COUNT + 1), 5, PrimErrBadIndex, "fieldCount the cap + 1");
	addBad(R_DESTFROM, fake_int(0), 5, PrimErrBadIndex, "destFrom 0");
	addBad(R_DESTFROM, fake_int(6), 5, PrimErrBadIndex, "destFrom size + 2");
	addBad(R_WIDE, fakeNil, 6, PrimErrBadArgument, "wideClass nil");
	addBad(R_WIDE, fake_int(1), 6, PrimErrBadArgument, "wideClass 1");
	addBad(R_WIDE, fakeByteString, 6, PrimErrBadArgument, "wideClass ByteString");
	addBad(R_WIDE, fakeBoxedFloat, 6, PrimErrBadArgument, "wideClass BoxedFloat64");
	addBad(R_WIDE, fakeArray, 6, PrimErrBadArgument, "wideClass Array");
	addBad(R_WIDE, fakeFloat64Array, 6, PrimErrBadArgument, "wideClass Float64Array");
	addBad(R_WIDE, fake_class("Words with a name", 10, 1), 6, PrimErrBadArgument, "wideClass with a named slot");
	addBad(R_WIDE, fake_forwarder(), 6, PrimErrBadArgument, "wideClass forwarder");
	{
		sqInt posing = fake_class("posing", 1, 3), oop = fake_new(posing, 0);

		fake_set_slot(oop, 2, fake_int((sqInt)10 << 16));
		addBad(R_WIDE, oop, 6, PrimErrBadArgument, "wideClass an object posing as a class");
	}
	addBad(R_TYPES, fake_bytes(fakeByteArray, "\1", 1), 7, PrimErrBadArgument, "types of 1");
	addBad(R_TYPES, fake_bytes(fakeByteArray, "\1\1\1", 3), 7, PrimErrBadArgument, "types of 3");
	addBad(R_TYPES, fake_bytes(fakeByteArray, "\1\5", 2), 7, PrimErrBadArgument, "type code 5");
	addBad(R_TYPES, fake_bytes(fakeByteArray, "\1\377", 2), 7, PrimErrBadArgument, "type code 255");
	addBad(R_TYPES, fake_bytes(fakeByteString, "\1\1", 2), 7, PrimErrBadArgument, "types ByteString");
	addBad(R_TYPES, fake_int(1), 7, PrimErrBadArgument, "types 1");
	addBad(R_TYPES, fake_new(fakeArray, 2), 7, PrimErrBadArgument, "types Array");
	addBad(R_TYPES, fake_forwarder(), 7, PrimErrBadArgument, "types forwarder");
	addBad(R_DEST, immutable(fake_new(fakeArray, 4)), 8, PrimErrNoModification, "dest immutable");
	rowsArgs(a, source, comma(0), index, entries, types, 2, fake_new(fakeArray, 4));
	checkInts(rowsOf(a), "the good rows", 9, SC_OK, 2, 5, 2, 0, 0, 0, 0, 0);
	a[R_DEST] = fake_new(fakeArray, 4);
	checkBads(pRows, "rows", R_ARGS, a, -1);
	CHECK(fake_slot(a[R_DEST], 0) == fakeNil, "rows: the destination was written");
	{
		sqInt b[R_ARGS], objects[12];
		size_t i, n = 0;

		objects[n++] = fakeNil;
		objects[n++] = fake_int(1);
		objects[n++] = fake_new(fakeByteArray, 32);
		objects[n++] = fake_new(fakeByteString, 32);
		objects[n++] = fake_new(fakeDoubleByteArray, 16);
		objects[n++] = fake_new(fakeFloat64Array, 8);
		objects[n++] = fake_compiled_method(32);
		objects[n++] = fake_forwarder();
		objects[n++] = source;
		for (i = 0; i < n; i++) {
			memcpy(b, a, sizeof b);
			b[R_INDEX] = objects[i];
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, index %zu", i);
			b[R_FROM] = fake_int(0);
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, index %zu, indexFrom 0", i);
			b[R_COUNT] = fake_int(-1);
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadIndex, "rows, index %zu, fieldCount -1", i);
			memcpy(b, a, sizeof b);
			b[R_DEST] = objects[i];
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, dest %zu", i);
			b[R_DESTFROM] = fake_int(0);
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, dest %zu, destFrom 0", i);
			b[R_TO] = fake_int(13);
			CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadIndex, "rows, dest %zu and to", i);
		}
		memcpy(b, a, sizeof b);
		b[R_INDEX] = fake_new(fakeArray, 64);
		CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, index Array");
		b[R_INDEX] = a[R_INDEX];
		b[R_DEST] = a[R_INDEX];
		CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, dest = index");
		b[R_DEST] = fake_new(fakeWeakArray, 4);
		CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, dest WeakArray");
		b[R_DEST] = fakeByteString;
		CHECK(failureOf(pRows, R_ARGS, b) == PrimErrBadArgument, "rows, dest a class");
		/* a subclass of Array, emptyValue = dest, an immutable index: fine */
		b[R_DEST] = fake_new(fakeArraySubclass, 4);
		b[R_EMPTY] = b[R_DEST];
		fake_set_immutable(b[R_INDEX], 1);
		CHECK(intSlot(rowsOf(b), 1) == 2, "rows, dest of a subclass of Array");
		fake_set_immutable(b[R_INDEX], 0);
		/* index = source, words: read only */
		rowsArgs(b, wide, comma(0), wide, 0, fakeNil, 0, fake_new(fakeArray, 4));
		checkInts(rowsOf(b), "rows, index = source", 9, SC_OK, 0, 1, 0, 0, 0, 0, 0, 0);
		b[R_LAST] = fake_int(1);
		b[R_FROM] = fake_int(1);
		b[R_TO] = fake_int(0);
		checkInts(rowsOf(b), "rows, an entry of nothing", 9, SC_ERROR, 0, 1, 0, SC_E_BAD_INDEX, 1, 1, 0, 0);
	}
}


/*** G5 sources ***/

static void
checkSources(void)
{
	sqInt byteClasses[4], wordClasses[5], a[R_ARGS];
	size_t i;

	section = "G5 sources";
	byteClasses[0] = fakeByteArray;
	byteClasses[1] = fakeByteString;
	byteClasses[2] = fakeByteSymbol;
	byteClasses[3] = fakeMyString;
	wordClasses[0] = fakeWideString;
	wordClasses[1] = fakeWideSymbol;
	wordClasses[2] = fakeWordArray;
	wordClasses[3] = fakeBitmap;
	wordClasses[4] = fakeIntegerArray;
	for (i = 0; i < 4; i++) {
		sqInt source = bytesOop(byteClasses[i], "x,y\n"), index, entries, dest = fake_new(fakeArray, 2);

		checkInts(measure(source, 1, 4, comma(0)), "bytes", 7, SC_OK, 1, 2, 5, 0, 0, 0);
		index = indexed(source, comma(0), &entries);
		rowsArgs(a, source, comma(0), index, entries, fakeNil, 0, dest);
		checkInts(rowsOf(a), "rows of bytes", 9, SC_OK, 1, 3, 0, 0, 0, 0, 0, 0);
		CHECK(cellIs(fake_slot(dest, 0), 1, "y"), "cell of bytes");
		checkInts(measure(fake_new(byteClasses[i], 0), 1, 0, comma(0)), "no bytes", 7, SC_OK, 0, 0, 1, 0, 0, 0);
	}
	for (i = 0; i < 5; i++) {
		static const uint32_t units[] = { 0x3B1, 0x1F600, 'e', 0xFFFFFFFFu, 0xE9, '\n' };
		sqInt source = fake_words(wordClasses[i], units, 6), index, entries, dest = fake_new(fakeArray, 2), row;

		checkInts(measure(source, 1, 6, word(0x1F600, '"', SC_F_FINAL)), "words", 7, SC_OK, 1, 2, 7, 0, 0, 0);
		index = indexed(source, word('e', '"', SC_F_FINAL), &entries);
		rowsArgs(a, source, word('e', '"', SC_F_FINAL), index, entries, fakeNil, 0, dest);
		checkInts(rowsOf(a), "rows of words", 9, SC_OK, 1, 3, 0, 0, 0, 0, 0, 0);
		row = fake_slot(dest, 0);
		CHECK(isRow(row, 2) && fake_class_of(fake_slot(row, 0)) == fakeWideString && fake_size(fake_slot(row, 0)) == 2
			&& memcmp(fake_body(fake_slot(row, 0)), units, 8) == 0, "cell of words");
		checkInts(measure(fake_new(wordClasses[i], 0), 1, 0, comma(0)), "no words", 7, SC_OK, 0, 0, 1, 0, 0, 0);
	}
	/* the narrowest class, and another class of wide strings */
	{
		static const uint32_t units[] = { 0xE9, ',', 0x100 };
		sqInt source = fake_words(fakeWideString, units, 3), index, entries, dest = fake_new(fakeArray, 2), row;

		index = indexed(source, comma(0), &entries);
		rowsArgs(a, source, comma(0), index, entries, fakeNil, 0, dest);
		a[R_WIDE] = fakeWideSymbol;
		rowsOf(a);
		row = fake_slot(dest, 0);
		CHECK(cellIs(row, 0, "\xE9"), "a Latin-1 cell of words");
		CHECK(fake_class_of(fake_slot(row, 1)) == fakeWideSymbol, "the class of wide strings");
	}
	/* the separator and the quote above 127 are Latin-1's only */
	{
		sqInt source = fake_bytes(fakeByteString, "a\xE9" "\xFFz\xFF", 5), dest, index, entries;

		index = indexed(source, word(0xE9, 0xFF, SC_F_FINAL | SC_F_LATIN1), &entries);
		CHECK(entries == 2, "separator and quote above 127");
		dest = fake_new(fakeArray, 1);
		rowsArgs(a, source, word(0xE9, 0xFF, SC_F_FINAL | SC_F_LATIN1), index, entries, fakeNil, 0, dest);
		rowsOf(a);
		CHECK(cellIs(fake_slot(dest, 0), 1, "z"), "quote above 127");
	}
}


/*** G6 scans ***/

static void
checkScans(void)
{
	sqInt source, dest, answer, a[7];
	uint32_t *w;
	const char *text = "a,b\n\"c\nd\",e\r\nf";
	int i;

	section = "G6 scans";
	source = bytesOop(fakeByteArray, text);	/* 14 bytes */
	checkInts(measure(source, 1, 14, comma(SC_F_LINES)), "measure", 7, SC_OK, 3, 5, 15, 3, 0, 0);
	checkInts(measure(source, 1, 14, comma(0)), "measure, no lines", 7, SC_OK, 3, 5, 15, 0, 0, 0);
	checkInts(measure(source, 1, 14, word(',', '"', 0)), "measure, not final", 7, SC_OK, 2, 4, 14, 0, 0, 0);
	checkInts(measure(source, 5, 13, word(',', '"', 0)), "measure, LF last", 7, SC_OK, 1, 2, 14, 0, 0, 0);
	checkInts(measure(source, 5, 12, word(',', '"', 0)), "measure, CR last", 7, SC_OK, 0, 0, 5, 0, 0, 0);
	checkInts(measure(source, 15, 14, comma(0)), "measure, nothing", 7, SC_OK, 0, 0, 15, 0, 0, 0);
	checkInts(measure(bytesOop(fakeByteArray, "x\na\"b"), 1, 5, comma(SC_F_STRICT)), "measure, strict", 7, SC_ERROR, 1, 1, 3, 0,
		SC_E_UNEXPECTED_QUOTE, 4);
	checkInts(measure(bytesOop(fakeByteArray, "\"x"), 1, 2, comma(SC_F_STRICT)), "measure, strict", 7, SC_ERROR, 0, 0, 1, 0,
		SC_E_UNTERMINATED_QUOTE, 1);
	checkInts(measure(bytesOop(fakeByteArray, "\"x\"y"), 1, 4, comma(SC_F_STRICT)), "measure, strict", 7, SC_ERROR, 0, 0, 1, 0,
		SC_E_TEXT_AFTER_QUOTE, 4);

	dest = marked(fakeWordArray, 100);
	w = fake_body(dest);
	checkInts(indexOf(source, 1, 1, 14, comma(0), dest, 3), "index", 9, SC_OK, 5, 3, 15, 0, 1, 2, 0, 0);
	CHECK(w[0] == 0xA5A5A5A5u && w[1] == 0xA5A5A5A5u, "the slots before destFrom");
	CHECK(w[2] == 1 && w[3] == 3 && w[4] == 9 && w[5] == 11 && w[6] == 14, "the entries");
	/* from a base: rebased */
	checkInts(indexOf(source, 3, 5, 14, comma(SC_F_LINES), dest, 1), "index from 5, base 3", 9, SC_OK, 3, 2, 15, 2, 1, 2, 0, 0);
	CHECK(w[0] == 7 && w[1] == 9 && w[2] == 12, "the entries from base 3");
	/* MORE: complete records only, nothing outside of the capacity */
	for (i = 0; i <= 5; i++) {
		sqInt small = marked(fakeBitmap, (size_t)i);
		static const int stored[] = { 0, 0, 2, 2, 4, 5 }, records[] = { 0, 0, 1, 1, 2, 3 }, next[] = { 1, 1, 5, 5, 14, 15 };

		answer = indexOf(source, 1, 1, 14, comma(0), small, 1);
		CHECK(intSlot(answer, 0) == (i < 5 ? SC_MORE : SC_OK) && intSlot(answer, 1) == stored[i] && intSlot(answer, 2) == records[i]
			&& intSlot(answer, 3) == next[i], "capacity %d", i);
		CHECK(intSlot(answer, 5) == (records[i] == 0 ? 0 : i < 5 ? 2 : 1) && intSlot(answer, 6) == (records[i] == 0 ? 0 : 2),
			"capacity %d: min and max", i);
		CHECK(intSlot(answer, 7) == 0 && intSlot(answer, 8) == 0, "capacity %d: code", i);
	}
	/* destFrom = size + 1: no capacity */
	checkInts(indexOf(source, 1, 1, 14, comma(0), dest, 101), "index, no slot", 9, SC_MORE, 0, 0, 1, 0, 0, 0, 0, 0);
	checkInts(indexOf(source, 15, 15, 14, comma(0), dest, 101), "index, no slot, nothing", 9, SC_OK, 0, 0, 15, 0, 0, 0, 0, 0);
	checkInts(indexOf(bytesOop(fakeByteString, "x\n\"y\"z"), 1, 1, 6, comma(SC_F_STRICT), dest, 1), "index, strict", 9, SC_ERROR, 1, 1,
		3, 0, 1, 1, SC_E_TEXT_AFTER_QUOTE, 6);
	/* the answer Array */
	for (i = 0; i < 2; i++) {
		sqInt code = i ? PrimErrUnsupported : PrimErrNoMemory;
		FakeAnswer r;

		fake_set_budget(0, code);
		a[0] = source;
		a[1] = fake_int(1);
		a[2] = fake_int(14);
		a[3] = comma(0);
		r = call(pMeasure, 4, a);
		CHECK(!r.returned && r.code == code && r.allocations == 1, "measure without memory");
		dest = marked(fakeWordArray, 100);
		indexArgs(a, source, 1, 1, 14, comma(0), dest, 1);
		r = call(pIndex, 7, a);
		CHECK(!r.returned && r.code == code && r.allocations == 1 && *(uint32_t *)fake_body(dest) == 0xA5A5A5A5u, "index without memory");
		fake_set_budget(-1, PrimErrNoMemory);
		r = call(pMeasure, 4, a);
		(void)r;
	}
}


/*** G7 rows ***/

static void
checkRows(void)
{
	sqInt dest, row, types, a[R_ARGS], source, index, entries, marker = bytesOop(fakeByteString, "EMPTY");
	int i;

	section = "G7 rows";
	/* strings */
	checkInts(rowsOfText("a,\"b\"\"c\",\n\xC3\xA9,\xE2\x82\xAC,\"\"", comma(0), fakeNil, 0, &dest), "strings", 9, SC_OK, 2, 7, 0, 0, 0, 0, 0, 0);
	row = fake_slot(dest, 0);
	CHECK(isRow(row, 3) && cellIs(row, 0, "a") && cellIs(row, 1, "b\"c") && fake_slot(row, 2) == fakeNil, "row 1");
	row = fake_slot(dest, 1);
	CHECK(isRow(row, 3) && cellIs(row, 0, "\xE9") && fake_class_of(fake_slot(row, 1)) == fakeWideString
		&& fake_size(fake_slot(row, 1)) == 1 && *(uint32_t *)fake_body(fake_slot(row, 1)) == 0x20AC
		&& fake_slot(row, 2) == fakeNil, "row 2");
	CHECK(fake_slot(dest, 2) == fakeNil, "dest, slot 3");
	/* quoted empty fields, the empty value */
	source = bytesOop(fakeByteArray, "\"\",\n,\"\"\n");
	for (i = 0; i < 2; i++) {
		sqInt d = comma(i ? SC_F_KEEP_QUOTED_EMPTY : 0);

		index = indexed(source, d, &entries);
		dest = fake_new(fakeArray, 2);
		rowsArgs(a, source, d, index, entries, fakeNil, 3, dest);
		a[R_EMPTY] = marker;
		rowsOf(a);
		row = fake_slot(dest, 0);
		CHECK(isRow(row, 3) && (i ? cellIs(row, 0, "") : fake_slot(row, 0) == marker) && fake_slot(row, 1) == marker
			&& fake_slot(row, 2) == marker, "quoted empty %d", i);
		a[R_TYPES] = fake_bytes(fakeByteArray, "\2\3\4", 3);	/* typed: the empty value in both modes */
		checkInts(rowsOf(a), "typed empty", 9, SC_OK, 2, 5, 0, 0, 0, 0, 0, 0);
		row = fake_slot(dest, 0);
		CHECK(fake_slot(row, 0) == marker && fake_slot(row, 1) == marker && fake_slot(row, 2) == marker, "typed quoted empty %d", i);
	}
	/* types: ignored, string, integer, float, float with a comma; odd cells */
	types = fake_bytes(fakeByteArray, "\0\1\2\3\4", 5);
	checkInts(rowsOfText("x;s; -12 ;1.5;2,25\ny;;12a;1e300;1e300\nz;\xFF\n", word(';', '"', SC_F_FINAL | SC_F_LATIN1), types, 5, &dest),
		"typed", 9, SC_OK, 3, 13, 3, 0, 0, 0, 0, 0);
	row = fake_slot(dest, 0);
	CHECK(isRow(row, 4) && cellIs(row, 0, "s") && fake_slot(row, 1) == fake_int(-12) && floatIs(fake_slot(row, 2), 1.5)
		&& floatIs(fake_slot(row, 3), 2.25), "typed row 1");
	row = fake_slot(dest, 1);
	CHECK(isRow(row, 4) && fake_slot(row, 0) == fakeNil && cellIs(row, 1, "12a") && cellIs(row, 2, "1e300") && cellIs(row, 3, "1e300"),
		"typed row 2");
	row = fake_slot(dest, 2);
	CHECK(isRow(row, 4) && cellIs(row, 0, "\xFF") && fake_slot(row, 1) == fakeNil && fake_slot(row, 3) == fakeNil, "typed row 3");
	/* a SmallInteger of 18 digits; BoxedFloat64s; zeros */
	types = fake_bytes(fakeByteArray, "\2\2\3\3\3", 5);
	checkInts(rowsOfText("999999999999999999,-999999999999999999,1e-15,0,-0\n", comma(0), types, 5, &dest), "numbers", 9, SC_OK, 1, 6, 0,
		0, 0, 0, 0, 0);
	row = fake_slot(dest, 0);
	CHECK(fake_slot(row, 0) == fake_int(999999999999999999LL) && fake_slot(row, 1) == fake_int(-999999999999999999LL), "18 digits");
	CHECK(floatIs(fake_slot(row, 2), 1e-15) && floatIs(fake_slot(row, 3), 0.0) && floatIs(fake_slot(row, 4), -0.0), "floats");
	/* (a converted Float is at most 2^53 * 10^22 and at least 10^-22: always a
	   SmallFloat64 or a zero) */
	/* fieldCount: 0 (types shorter than the record; longer), 1, short and
	   long records, all ignored */
	types = fake_bytes(fakeByteArray, "\0\2", 2);
	checkInts(rowsOfText("a,1,c,d\ne\n\nf,2\n", comma(0), types, 0, &dest), "fieldCount 0", 9, SC_OK, 4, 9, 0, 0, 0, 0, 0, 0);
	CHECK(isRow(fake_slot(dest, 0), 3) && fake_slot(fake_slot(dest, 0), 0) == fake_int(1) && cellIs(fake_slot(dest, 0), 2, "d"), "wide");
	CHECK(isRow(fake_slot(dest, 1), 0) && isRow(fake_slot(dest, 2), 0) && isRow(fake_slot(dest, 3), 1), "narrow rows");
	checkInts(rowsOfText("a,b\nc,d\n\n", comma(0), fakeNil, 1, &dest), "fieldCount 1", 9, SC_OK, 3, 6, 0, 0, 0, 0, 0, 0);
	CHECK(isRow(fake_slot(dest, 1), 1) && cellIs(fake_slot(dest, 1), 0, "c") && isRow(fake_slot(dest, 2), 1)
		&& fake_slot(fake_slot(dest, 2), 0) == fakeNil, "fieldCount 1");
	checkInts(rowsOfText("a,b\nc\n", comma(0), fake_new(fakeByteArray, 2), 2, &dest), "all ignored", 9, SC_OK, 2, 4, 0, 0, 0, 0, 0, 0);
	CHECK(isRow(fake_slot(dest, 0), 0) && isRow(fake_slot(dest, 1), 0), "all ignored");
	checkInts(rowsOfText("a,b\nc\n", comma(0), fake_new(fakeByteArray, 0), 0, &dest), "no types", 9, SC_OK, 2, 4, 0, 0, 0, 0, 0, 0);
	CHECK(isRow(fake_slot(dest, 0), 2) && isRow(fake_slot(dest, 1), 1), "no types");
	{
		size_t mark = fake_mark();

		checkInts(rowsOfText("a\n", comma(0), fakeNil, SC_MAX_FIELD_COUNT, &dest), "the cap", 9, SC_OK, 1, 2, 0, 0, 0, 0, 0, 0);
		row = fake_slot(dest, 0);
		CHECK(isRow(row, SC_MAX_FIELD_COUNT) && cellIs(row, 0, "a") && fake_slot(row, SC_MAX_FIELD_COUNT - 1) == fakeNil, "the cap");
		fake_release(mark);
	}
	/* EXACT_COUNT, with a field count only */
	checkInts(rowsOfText("a,b\nc,d,e\nf\n", comma(SC_F_EXACT_COUNT), fakeNil, 2, &dest), "exact", 9, SC_ERROR, 1, 3, 0, SC_E_FIELD_COUNT, 5,
		2, 3, 0);
	CHECK(cellIs(fake_slot(dest, 0), 1, "b") && fake_slot(dest, 1) == fakeNil, "exact: the rows before");
	checkInts(rowsOfText("\n\na\n", comma(SC_F_EXACT_COUNT | SC_F_SKIP_BLANK), fakeNil, 2, &dest), "exact, blank lines", 9, SC_ERROR, 0, 1,
		0, SC_E_FIELD_COUNT, 3, 1, 1, 0);
	checkInts(rowsOfText("a,b\nc,d,e\nf\n", comma(SC_F_EXACT_COUNT), fakeNil, 0, &dest), "exact, no count", 9, SC_OK, 3, 7, 0, 0, 0, 0, 0, 0);
	/* invalid UTF-8: of a built field only */
	checkInts(rowsOfText("a,b\nc,\xC3\n", comma(0), fakeNil, 0, &dest), "invalid", 9, SC_ERROR, 1, 3, 0, SC_E_INVALID_UTF8, 7, 2, 2, 0);
	CHECK(isRow(fake_slot(dest, 0), 2) && fake_slot(dest, 1) == fakeNil, "invalid: the rows before");
	checkInts(rowsOfText("x1,b\nc,\xC3\n", comma(0), fake_bytes(fakeByteArray, "\2\2", 2), 2, &dest), "invalid, typed", 9, SC_ERROR, 1, 3, 2,
		SC_E_INVALID_UTF8, 8, 2, 2, 0);
	checkInts(rowsOfText("a,b\nc,\xC3\n", comma(0), fake_bytes(fakeByteArray, "\1\0", 2), 2, &dest), "invalid, ignored", 9, SC_OK, 2, 5, 0, 0,
		0, 0, 0, 0);
	checkInts(rowsOfText("a\nc,\xC3\n", comma(0), fakeNil, 1, &dest), "invalid, extra", 9, SC_OK, 2, 4, 0, 0, 0, 0, 0, 0);
	/* ranges of the index */
	source = bytesOop(fakeByteArray, "##a,b\r\nc,d\ne,f");
	index = fake_new(fakeWordArray, 10);
	checkInts(indexOf(source, 3, 3, 14, comma(0), index, 3), "index", 9, SC_OK, 6, 3, 15, 0, 2, 2, 0, 0);
	dest = fake_new(fakeArray, 4);
	rowsArgs(a, source, comma(0), index, 8, fakeNil, 0, dest);
	a[R_BASE] = fake_int(3);
	a[R_FROM] = fake_int(5);
	a[R_DESTFROM] = fake_int(2);
	checkInts(rowsOf(a), "from entry 5", 9, SC_OK, 2, 9, 0, 0, 0, 0, 0, 0);
	CHECK(fake_slot(dest, 0) == fakeNil && cellIs(fake_slot(dest, 1), 0, "c") && cellIs(fake_slot(dest, 2), 1, "f"), "from entry 5");
	a[R_FROM] = fake_int(9);
	checkInts(rowsOf(a), "an empty range", 9, SC_OK, 0, 9, 0, 0, 0, 0, 0, 0);
	a[R_FROM] = fake_int(6);
	checkInts(rowsOf(a), "in a record", 9, SC_ERROR, 0, 6, 0, SC_E_BAD_INDEX, 3, 1, 0, 0);
	a[R_FROM] = fake_int(5);
	a[R_LAST] = fake_int(7);
	checkInts(rowsOf(a), "a cut record", 9, SC_ERROR, 1, 7, 0, SC_E_BAD_INDEX, 12, 2, 0, 0);
	a[R_LAST] = fake_int(8);
	a[R_TO] = fake_int(13);
	checkInts(rowsOf(a), "a shorter range", 9, SC_ERROR, 1, 7, 0, SC_E_BAD_INDEX, 12, 2, 0, 0);
	a[R_TO] = fake_int(14);
	((uint32_t *)fake_body(index))[6] = 1;
	checkInts(rowsOf(a), "a decreasing index", 9, SC_ERROR, 1, 7, 0, SC_E_BAD_INDEX, 12, 2, 0, 0);
	((uint32_t *)fake_body(index))[6] = 0xFFFFFFFFu;
	checkInts(rowsOf(a), "a wild index", 9, SC_ERROR, 1, 7, 0, SC_E_BAD_INDEX, 12, 2, 0, 0);
	((uint32_t *)fake_body(index))[6] = 9;
	checkInts(rowsOf(a), "an entry that is no end", 9, SC_ERROR, 1, 7, 0, SC_E_BAD_INDEX, 12, 2, 0, 0);
	/* a destination that is full */
	source = bytesOop(fakeByteArray, "a\nb\nc\n");
	index = indexed(source, comma(0), &entries);
	for (i = 0; i <= 4; i++) {
		int n = i < 3 ? i : 3;

		dest = fake_new(fakeArray, 4);
		rowsArgs(a, source, comma(0), index, entries, fakeNil, 0, dest);
		a[R_DESTFROM] = fake_int(5 - i);
		checkInts(rowsOf(a), "dest full", 9, i < 3 ? SC_MORE : SC_OK, n, n + 1, 0, 0, 0, 0, 0, 0);
		CHECK(i == 0 || cellIs(fake_slot(dest, (size_t)(4 - i)), 0, "a"), "dest full: its first row");
	}
	/* budgets: a row of one cell takes 16 + 16 bytes */
	{
		static const int budgets[] = { -1, 0, 1, 31, 32, 33, 64, 65, 1000 }, rowsMade[] = { 3, 1, 1, 1, 1, 2, 2, 3, 3 };

		for (i = 0; i < 9; i++) {
			FakeAnswer r;

			dest = fake_new(fakeArray, 4);
			rowsArgs(a, source, comma(0), index, entries, fakeNil, 0, dest);
			a[R_BUDGET] = fake_int(budgets[i]);
			r = callRows(a);
			checkInts(r.value, "budget", 9, rowsMade[i] < 3 ? SC_MORE : SC_OK, rowsMade[i], rowsMade[i] + 1, 0, 0, 0, 0, 0, 0);
			CHECK(r.bytes - r.firstBytes == 32u * (unsigned)rowsMade[i], "budget %d: %llu bytes", budgets[i], (unsigned long long)r.bytes);
		}
	}
}


/*** G8 memory ***/

/* Every allocation of a call fails in its turn */
static void
checkEveryAllocation(const char *text, sqInt dialect, sqInt types, sqInt fieldCount, sqInt emptyValue)
{
	sqInt a[R_ARGS], source = bytesOop(fakeByteArray, text), dest, index, entries;
	int total, k, rowCount;
	FakeAnswer r, one;
	uint64_t need;

	index = indexed(source, dialect, &entries);
	dest = fake_new(fakeArray, 16);
	rowsArgs(a, source, dialect, index, entries, types, fieldCount, dest);
	a[R_EMPTY] = emptyValue;
	r = callRows(a);
	CHECK(r.returned && intSlot(r.value, 0) == SC_OK, "%s", text);
	total = r.allocations;
	rowCount = (int)intSlot(r.value, 1);
	a[R_BUDGET] = fake_int(0);	/* the first record alone */
	one = callRows(a);
	CHECK(one.returned && intSlot(one.value, 1) == 1, "%s: one record", text);
	need = one.bytes - one.firstBytes;
	a[R_BUDGET] = fake_int(-1);
	for (k = 0; k <= total; k++) {
		size_t i;

		dest = fake_new(fakeArray, 16);
		a[R_DEST] = dest;
		fake_set_budget(k, PrimErrUnsupported);
		r = callRows(a);
		CHECK(k == total ? r.returned : !r.returned && r.code == PrimErrUnsupported, "%s, Unsupported at %d: code %lld", text, k,
			(long long)r.code);
		dest = fake_new(fakeArray, 16);
		a[R_DEST] = dest;
		fake_set_budget(k, PrimErrNoMemory);
		r = callRows(a);
		fake_set_budget(-1, PrimErrNoMemory);
		if (k == 0) {
			CHECK(!r.returned && r.code == PrimErrNoMemory && r.stores == 0, "%s: no answer Array", text);
			continue;
		}
		CHECK(r.returned && r.code == 0, "%s, no memory at %d: code %lld", text, k, (long long)r.code);
		if (!r.returned) {
			continue;
		}
		if (k == total) {
			CHECK(intSlot(r.value, 0) == SC_OK && intSlot(r.value, 1) == rowCount, "%s: all", text);
		} else if (k < one.allocations) {
			checkInts(r.value, "no room", 9, SC_NO_ROOM, 0, 1, 0, 0, 0, 0, 0, (int)need);
			CHECK(need > 0, "%s: need", text);
		} else {
			sqInt made = intSlot(r.value, 1);

			CHECK(intSlot(r.value, 0) == SC_MORE && made >= 1 && made < rowCount, "%s, no memory at %d", text, k);
			CHECK(intSlot(r.value, 8) == 0 && intSlot(r.value, 4) == 0, "%s, no memory at %d: need and code", text, k);
			/* the rest, from there, with memory */
			a[R_FROM] = fake_int(intSlot(r.value, 2));
			a[R_DESTFROM] = fake_int(made + 1);
			r = callRows(a);
			CHECK(r.returned && intSlot(r.value, 0) == SC_OK && intSlot(r.value, 1) == rowCount - made, "%s, the rest after %d", text, k);
			a[R_FROM] = a[R_DESTFROM] = fake_int(1);
		}
		for (i = 0; i < 16; i++) {
			int full = k >= one.allocations && i < (size_t)rowCount;

			CHECK((fake_slot(dest, i) != fakeNil) == full, "%s, no memory at %d: dest %zu", text, k, i);
		}
	}
}

static void
checkMemory(void)
{
	char longText[4096];
	int i;

	section = "G8 memory";
	checkEveryAllocation("a,bb,ccccccccc\nd\ne,\xE2\x82\xAC\n", comma(0), fakeNil, 0, fakeNil);
	checkEveryAllocation("\xE2\x82\xAC\xE2\x82\xAC\xE2\x82\xAC,\"a\"\"b\",\"\",\nd\n", comma(SC_F_KEEP_QUOTED_EMPTY), fakeNil, 0, fakeNil);
	checkEveryAllocation("\"\",\"\"\n", comma(0), fakeNil, 5, fakeTrueObject);
	checkEveryAllocation("12,x,1.5,1e400,\"\",3\n4,5,6,7,8,9\n", comma(SC_F_KEEP_QUOTED_EMPTY), fake_bytes(fakeByteArray, "\2\2\3\3\3\0", 6), 6,
		fakeNil);
	checkEveryAllocation("a\n\nb\n", comma(0), fakeNil, 300, fakeNil);
	/* an overflow header of a String and of a wide one */
	for (i = 0; i < 2500; i++) {
		longText[i] = 'x';
	}
	strcpy(longText + 2500, ",y\nz\n");
	checkEveryAllocation(longText, comma(0), fakeNil, 0, fakeNil);
	strcpy(longText + 2498, "\xC4\x80\ny\n");
	checkEveryAllocation(longText, comma(0), fakeNil, 0, fakeNil);
	/* a record that does not fit and is invalid too: the error, which no
	   memory cures */
	{
		sqInt a[R_ARGS], entries, source = bytesOop(fakeByteArray, "a,\xC3\n"), index = indexed(source, comma(0), &entries);
		FakeAnswer r;

		rowsArgs(a, source, comma(0), index, entries, fakeNil, 0, fake_new(fakeArray, 1));
		fake_set_budget(2, PrimErrNoMemory);
		r = callRows(a);
		fake_set_budget(-1, PrimErrNoMemory);
		checkInts(r.value, "no room, invalid", 9, SC_ERROR, 0, 1, 0, SC_E_INVALID_UTF8, 3, 1, 2, 0);
	}
}


/*** G9 random ***/

static uint64_t seed = 88172645463325252ULL;

static uint64_t
next64(void)
{
	seed ^= seed << 13;
	seed ^= seed >> 7;
	return seed ^= seed << 17;
}

static size_t
below(size_t n)
{
	return (size_t)((next64() >> 11) % n);
}

/* The second reader: the records of units[0, n) with FINAL, lenient */
#define MAX_UNITS 120
typedef struct {
	uint32_t text[MAX_UNITS + 1];	/* unquoted */
	int size, start, end, quoted, last, record;
} RefField;
static RefField ref[MAX_UNITS + 2];
static int refs, refRecords;

static void
reference(const uint32_t *u, int n, uint32_t separator, uint32_t quote, uint32_t flags)
{
	int i = 0;

	refs = refRecords = 0;
#define END_OF_LINE(c) ((c) == '\n' || ((c) == '\r' && !(flags & SC_F_LF_ONLY)))
#define IS_QUOTE(c) ((c) == quote && !(flags & SC_F_NO_QUOTE))
	while (i < n) {
		if ((flags & SC_F_SKIP_BLANK) && END_OF_LINE(u[i])) {
			i++;
			continue;
		}
		for (;;) {	/* a field from i */
			RefField *f = &ref[refs++];
			int in = 0;

			f->size = 0;
			f->start = i;
			f->quoted = i < n && IS_QUOTE(u[i]);
			f->record = refRecords;
			if (f->quoted) {
				in = 1;
				i++;
			}
			while (i < n && (in || !(u[i] == separator || END_OF_LINE(u[i])))) {
				if (in && IS_QUOTE(u[i])) {
					if (i + 1 < n && IS_QUOTE(u[i + 1])) {
						f->text[f->size++] = u[i];
						i += 2;
					} else {
						in = 0;
						i++;
					}
				} else {
					f->text[f->size++] = u[i++];
				}
			}
			f->end = i;
			f->last = i == n || END_OF_LINE(u[i]);
			if (i == n) {
				break;
			}
			i++;
			if (f->last) {
				if (u[i - 1] == '\r' && i < n && u[i] == '\n') {
					i++;
				}
				break;
			}
		}
		refRecords++;
	}
}

/* Whether cell is the text of f */
static int
isText(sqInt cell, const RefField *f)
{
	int i, wide = 0;

	if (!fake_is_object(cell) || fake_size(cell) != (size_t)f->size) {
		return 0;
	}
	for (i = 0; i < f->size; i++) {
		wide |= f->text[i] > 0xFF;
	}
	if (fake_class_of(cell) != (wide ? fakeWideString : fakeByteString)) {
		return 0;
	}
	for (i = 0; i < f->size; i++) {
		if ((wide ? ((uint32_t *)fake_body(cell))[i] : ((uint8_t *)fake_body(cell))[i]) != f->text[i]) {
			return 0;
		}
	}
	return 1;
}

/* Whether row is the row of the reference's record, and its odd cells added
 * to *odd */
static int
isReferenceRow(sqInt row, int record, sqInt types, int fieldCount, sqInt source, sqInt dialectOop, sqInt emptyValue, int *odd)
{
	uint64_t d = (uint64_t)fake_int_value(dialectOop);
	sc_dialect dialect;
	sc_input in;
	int i = 0, first, n = 0, column, slot = 0;

	dialect.separator = (uint32_t)(d & 0x1FFFFF);
	dialect.quote = (uint8_t)(d >> 21);
	dialect.flags = (uint32_t)(d >> 29);
	in.units = fake_body(source);
	in.wide = fake_format(source) == 10;
	in.from = 0;
	in.to = fake_size(source);
	while (i < refs && ref[i].record != record) {
		i++;
	}
	first = i;
	while (first + n < refs && ref[first + n].record == record) {
		n++;
	}
	for (column = 0; column < (fieldCount ? fieldCount : n); column++) {
		RefField *f = column < n ? &ref[first + column] : NULL;
		int type = types == fakeNil || (size_t)column >= fake_size(types) ? 1 : ((uint8_t *)fake_body(types))[column];
		sqInt cell;
		sc_field field;

		if (type == 0) {
			continue;
		}
		if ((size_t)slot >= fake_size(row)) {
			return 0;
		}
		cell = fake_slot(row, (size_t)slot++);
		if (!f || f->start == f->end) {
			if (cell != emptyValue) return 0;
			continue;
		}
		field.start = (uint64_t)f->start;
		field.end = (uint64_t)f->end;
		field.last = f->last;
		if (type > 1) {
			int64_t v;
			double x;

			if (type == 2 ? sc_to_integer(&in, &dialect, &field, &v) : sc_to_float(&in, &dialect, &field, type == 4, &x)) {
				if (type == 2 ? cell != fake_int((sqInt)v) : !floatIs(cell, x)) return 0;
				continue;
			}
		}
		if (f->size == 0 && !(type == 1 && (dialect.flags & SC_F_KEEP_QUOTED_EMPTY))) {
			if (cell != emptyValue) return 0;
			continue;
		}
		if (!isText(cell, f)) return 0;
		*odd += type > 1;
	}
	return fake_size(row) == (size_t)slot;
}

static void
checkRandom(long rounds)
{
	static const char alphabet[] = "a,;\"\n\r1 -.2e\"\",\n\n";
	static const char separators[] = ",;\t ";
	size_t mark = fake_mark();
	long round;

	section = "G9 random";
	for (round = 0; round < rounds; round++) {
		uint32_t units[MAX_UNITS];
		uint8_t bytes[MAX_UNITS];
		int n = (int)below(MAX_UNITS + 1), i, wide = below(3) == 0, fieldCount, made = 0, odd = 0, oddSeen = 0, calls = 0;
		uint32_t separator = (uint32_t)separators[below(4)], quote = below(8) ? '"' : 'a', flags = SC_F_FINAL;
		sqInt source, dialect, index, types = fakeNil, dest, a[R_ARGS], answer, entries, emptyValue;

		flags |= (uint32_t)(below(4) ? 0 : SC_F_LF_ONLY) | (uint32_t)(below(4) ? 0 : SC_F_SKIP_BLANK)
			| (uint32_t)(below(8) ? 0 : SC_F_NO_QUOTE) | (uint32_t)(below(2) ? 0 : SC_F_KEEP_QUOTED_EMPTY)
			| (uint32_t)(wide ? 0 : SC_F_LATIN1);
		for (i = 0; i < n; i++) {
			units[i] = below(10) ? (uint32_t)alphabet[below(sizeof alphabet - 1)] : (uint32_t)(wide ? below(0x300) : below(256));
			bytes[i] = (uint8_t)units[i];
		}
		source = wide ? fake_words(fakeWideString, units, (size_t)n) : fake_bytes(fakeByteArray, bytes, (size_t)n);
		dialect = word(separator, quote, flags);
		reference(units, n, separator, quote, flags);
		index = fake_new(fakeWordArray, (size_t)refs + below(90));
		answer = indexOf(source, 1, 1, n, dialect, index, 1);
		entries = intSlot(answer, 1);
		CHECK(intSlot(answer, 0) == SC_OK && entries == refs && intSlot(answer, 2) == refRecords, "round %ld: %lld entries, not %d", round,
			(long long)entries, refs);
		for (i = 0; i < refs && i < entries; i++) {
			CHECK(((uint32_t *)fake_body(index))[i] == (uint32_t)ref[i].end, "round %ld: entry %d", round, i);
		}
		fieldCount = below(2) ? 0 : 1 + (int)below(6);
		if (below(2)) {
			size_t size = fieldCount ? (size_t)fieldCount : below(6);
			types = fake_new(fakeByteArray, size);
			while (size) {
				((uint8_t *)fake_body(types))[--size] = (uint8_t)below(5);
			}
		}
		emptyValue = below(2) ? fakeNil : fake_int(0);
		dest = fake_new(fakeArray, (size_t)refRecords + 1);
		rowsArgs(a, source, dialect, index, entries, types, fieldCount, dest);
		a[R_EMPTY] = emptyValue;
		/* pieces: by budgets, by failing allocations, by small destinations */
		for (;;) {
			FakeAnswer r;
			sqInt status;

			if (++calls > 4 * refRecords + 10) {
				CHECK(0, "round %ld: no progress", round);
				break;
			}
			a[R_DESTFROM] = fake_int(made + 1);
			a[R_BUDGET] = fake_int((sqInt)below(400) - 1);
			fake_set_budget(below(3) ? -1 : 1 + (long)below(12), PrimErrNoMemory);
			r = callRows(a);
			fake_set_budget(-1, PrimErrNoMemory);
			CHECK(r.returned, "round %ld: code %lld", round, (long long)r.code);
			if (!r.returned) {
				break;
			}
			status = intSlot(r.value, 0);
			made += (int)intSlot(r.value, 1);
			odd += (int)intSlot(r.value, 3);
			a[R_FROM] = fake_int(intSlot(r.value, 2));
			if (status == SC_NO_ROOM) {
				uint64_t need = (uint64_t)intSlot(r.value, 8);

				CHECK(intSlot(r.value, 1) == 0, "round %ld: rows and NO_ROOM", round);
				a[R_BUDGET] = fake_int(0);
				r = callRows(a);
				CHECK(r.returned && r.bytes - r.firstBytes == need, "round %ld: need", round);
				if (!r.returned) {
					break;
				}
				status = intSlot(r.value, 0);
				made += (int)intSlot(r.value, 1);
				odd += (int)intSlot(r.value, 3);
				a[R_FROM] = fake_int(intSlot(r.value, 2));
			}
			if (status == SC_OK) {
				break;
			}
			CHECK(status == SC_MORE, "round %ld: status %lld", round, (long long)status);
			if (status != SC_MORE) {
				break;
			}
		}
		CHECK(made == refRecords && fake_int_value(a[R_FROM]) == entries + 1, "round %ld: %d rows of %d", round, made, refRecords);
		for (i = 0; i < made && i < refRecords; i++) {
			CHECK(isReferenceRow(fake_slot(dest, (size_t)i), i, types, fieldCount, source, dialect, emptyValue, &oddSeen),
				"round %ld: row %d", round, i + 1);
		}
		CHECK(odd == oddSeen, "round %ld: %d odd, not %d", round, odd, oddSeen);
		CHECK(fake_slot(dest, (size_t)refRecords) == fakeNil, "round %ld: the slot after", round);

		/* the same with a damaged index, from any entry, in any dialect: an
		   answer, and no access outside (AddressSanitizer) */
		for (i = 0; i < 4; i++) {
			size_t k, size = fake_size(index);
			FakeAnswer r;

			for (k = below(4); k < size; k += 1 + below(8)) {
				((uint32_t *)fake_body(index))[k] = below(4) ? (uint32_t)below((size_t)n + 2) : (uint32_t)next64();
			}
			a[R_DEST] = fake_new(fakeArray, below(6));
			a[R_DESTFROM] = fake_int(1);
			a[R_FROM] = fake_int(1 + (sqInt)below(size + 1));
			a[R_LAST] = fake_int(fake_int_value(a[R_FROM]) - 1 + (sqInt)below(size + 2 - (size_t)fake_int_value(a[R_FROM])));
			a[R_BASE] = fake_int(1 + (sqInt)below((size_t)n + 1));
			r = callRows(a);
			CHECK(r.returned, "round %ld: a damaged index, code %lld", round, (long long)r.code);
			if (r.returned) {
				sqInt status = intSlot(r.value, 0), code = intSlot(r.value, 4);

				CHECK(status == SC_OK || status == SC_MORE || (status == SC_ERROR && code == SC_E_BAD_INDEX), "round %ld: status %lld",
					round, (long long)status);
				CHECK(status != SC_ERROR || (intSlot(r.value, 5) >= fake_int_value(a[R_BASE]) && intSlot(r.value, 5) <= n + 1
					&& intSlot(r.value, 6) == intSlot(r.value, 1) + 1), "round %ld: the error", round);
			}
		}
		fake_release(mark);
	}
}


/*** G10 calls ***/

static void
checkCalls(long rounds)
{
	sqInt pool[40];
	size_t pooled = 0, mark;
	long round, answered = 0;
	sqInt source, index, entries;

	section = "G10 calls";
	source = bytesOop(fakeByteArray, "a,b\n1,\"x,y\"\n\n\xC3\xA9,\xE2\x82\xAC\n");
	index = indexed(source, comma(0), &entries);
	pool[pooled++] = source;
	pool[pooled++] = index;
	pool[pooled++] = fake_new(fakeArray, 8);
	pool[pooled++] = fake_new(fakeArray, 0);
	pool[pooled++] = immutable(fake_new(fakeArray, 3));
	pool[pooled++] = fake_new(fakeWordArray, 0);
	pool[pooled++] = immutable(fake_new(fakeWordArray, 3));
	pool[pooled++] = fake_new(fakeByteArray, 2);
	pool[pooled++] = fake_bytes(fakeByteArray, "\1\2\3\4\0", 5);
	pool[pooled++] = fake_new(fakeByteArray, 0);
	pool[pooled++] = bytesOop(fakeByteString, "x;y\n\xFF\n");
	pool[pooled++] = fake_new(fakeWideString, 5);
	pool[pooled++] = fake_new(fakeDoubleByteArray, 5);
	pool[pooled++] = fake_new(fakeFloat64Array, 5);
	pool[pooled++] = fake_boxed_float(1e300);
	pool[pooled++] = fake_compiled_method(20);
	pool[pooled++] = fake_forwarder();
	pool[pooled++] = fake_new(fakeWeakArray, 5);
	pool[pooled++] = fakeWideString;
	pool[pooled++] = fakeBoxedFloat;
	pool[pooled++] = fakeByteString;
	pool[pooled++] = fakeArray;
	pool[pooled++] = fakeNil;
	pool[pooled++] = fakeTrueObject;
	pool[pooled++] = fake_small_float(1.5);
	pool[pooled++] = fake_character('"');
	mark = fake_mark();
	for (round = 0; round < rounds; round++) {
		static const Primitive *const primitives[] = { &pInfo, &pMeasure, &pIndex, &pRows, &pRows, &pIndex };
		static const int arity[] = { 0, 4, 7, 14, 14, 7 };
		size_t which = below(6);
		int nargs = arity[which], i;
		sqInt a[15];
		FakeAnswer r;

		if (below(20) == 0) {
			nargs = (int)below(16);
		}
		/* mostly a good call, with a few arguments replaced */
		for (i = 0; i < 15; i++) {
			a[i] = pool[below(pooled)];
		}
		if (nargs == 4) {
			a[0] = pool[below(2) ? 0 : 10];
			a[1] = fake_int(1 + (sqInt)below(4));
			a[2] = fake_int((sqInt)below(8));
			a[3] = comma((uint32_t)below(1024));
		} else if (nargs == 7) {
			indexArgs(a, source, 1 + (sqInt)below(3), 1 + (sqInt)below(4), (sqInt)below(24), comma((uint32_t)below(1024)), pool[1],
				1 + (sqInt)below(66));
		} else if (nargs == 14) {
			rowsArgs(a, source, comma((uint32_t)below(1024)), index, (sqInt)below(12), below(2) ? fakeNil : pool[8], (sqInt)below(7),
				pool[2]);
			a[R_FROM] = fake_int(1 + (sqInt)below(4));
			a[R_BUDGET] = fake_int((sqInt)below(300) - 1);
		}
		for (i = 0; i < nargs; i++) {
			if (below(12) == 0) {
				switch (below(4)) {
				case 0: a[i] = fake_int((sqInt)below(80) - 4); break;
				case 1: a[i] = fake_int((sqInt)next64() >> 3); break;
				default: a[i] = pool[below(pooled)]; break;
				}
			}
		}
		if (nargs == 14 && fake_is_int(a[R_COUNT]) && fake_int_value(a[R_COUNT]) > 100000) {
			a[R_COUNT] = fake_int(100);	/* (a row of the cap takes too long here) */
		}
		if (nargs == 14 && a[R_WIDE] == a[R_DEST]) {
			a[R_WIDE] = fakeNil;	/* (whose format would be read from the destination) */
		}
		fake_set_budget(below(10) ? -1 : (long)below(10), below(4) ? PrimErrNoMemory : PrimErrUnsupported);
		/* a destination in the pool may be stored into */
		fake_allow_store(nargs == 14 && fake_is_object(a[R_DEST]) && fake_format(a[R_DEST]) == 2 ? a[R_DEST] : 0);
		r = call(*primitives[which], nargs, a);
		CHECK(r.returned != (r.code != 0), "round %ld", round);
		answered += r.returned;
		fake_allow_store(0);
		fake_set_budget(-1, PrimErrNoMemory);
		/* (the rows stored go away with the mark) */
		for (i = 0; i < 8; i++) {
			fake_set_slot(pool[2], (size_t)i, fakeNil);
		}
		fake_release(mark);
	}
	CHECK(answered > rounds / 10, "only %ld of %ld random calls answered", answered, rounds);
}

int
main(int argc, char **argv)
{
	struct VirtualMachine *vm = fake_init();
	long rounds = argc > 1 ? atol(argv[1]) : 200000;

	checkModule(vm);
	checkInfo();
	checkArity();
	checkFailures();
	checkSources();
	checkScans();
	checkRows();
	checkMemory();
	checkRandom(rounds / 10);
	checkCalls(rounds);
	section = "G1 module";
	CHECK(shutdown() == 1, "shutdownModule");
	fake_free_all();
	printf("gluetest: %ld checks, %ld failures\n", checks, failures);
	return failures ? 1 : 0;
}
