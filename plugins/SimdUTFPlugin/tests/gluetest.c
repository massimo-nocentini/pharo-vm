/* gluetest.c -- U10: the glue SimdUTFPlugin.c against the fake VM of
 * fakevm.c, linked with the engine (suEngine.cpp) and simdutf.
 *
 * Built twice by run-c-tests.sh: with the glue as the shared library
 * compiles it (its primitives and *AccessorDepth variables are globals),
 * and with -DSQUEAK_BUILTIN_PLUGIN (GLUE_BUILTIN here), where the
 * primitives are static and found by name in SimdUTFPlugin_exports, as the
 * Emscripten VM finds them.  The checks:
 *
 *   module     setInterpreter, the module name and functions, the
 *              AccessorDepths (0), the rows of the builtin table
 *   info       primitiveInfo: the 7 slots against su_get_info, and
 *              PrimErrNoMemory (the VM's retry) when any of its
 *              allocations fails
 *   arity      every primitive with one argument less and one more
 *   order      the failures of the contract's checks (1)-(7): each alone,
 *              then every pair of them on different arguments, where the
 *              earlier check must win; nothing allocated
 *   flags      every flag, option, mode, line length and line end value
 *              from -2 to 300 (and far ones) against the contract's sets,
 *              on byte and word sources
 *   answers    crafted calls: results and their classes, positions in the
 *              status words (absolute, 1-based, beyond 2^18), into mode,
 *              TOO_LARGE leaving the destination untouched, the class
 *              argument, the BOM, 1001, 1002
 *   random     random calls of the six conversion primitives (sources of
 *              every kind with invalid input, ranges, flags, destinations
 *              and indexes): each answer equals the one derived from the
 *              engine called directly; at most one allocation, none on
 *              invalid UTF input
 *   budget     a refused allocation: PrimErrNoMemory gives OVER_BUDGET with
 *              the Spur bytes of the object (around the 255-slot header)
 *              and no failure code; PrimErrBadReceiver and
 *              PrimErrUnsupported give PrimErrBadArgument
 *
 * Every call is also checked by the fake VM: no answer after a failure, no
 * failure after an answer, no store into an object the call did not make,
 * none of the proxy's eden-only allocators.
 *
 * Usage: gluetest [--seed N] [--rounds N]   (200,000 random calls by
 * default.)  Prints one line per failed check (the first 50), what the
 * random calls answered, and "U10 ok: N checks, 0 failures"; exits 1 when
 * a check failed.
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fakevm.h"
#include "suEngine.h"

typedef sqInt (*Primitive)(void);

#ifdef GLUE_BUILTIN
extern void *SimdUTFPlugin_exports[][3];
#else
extern const char *getModuleName(void);
extern sqInt initialiseModule(void);
extern sqInt shutdownModule(void);
extern sqInt setInterpreter(struct VirtualMachine *);
extern sqInt primitiveBase64Decode(void);
extern sqInt primitiveBase64Encode(void);
extern sqInt primitiveDecode(void);
extern sqInt primitiveEncode(void);
extern sqInt primitiveInfo(void);
extern sqInt primitiveMeasure(void);
extern sqInt primitiveValidate(void);
extern signed char primitiveBase64DecodeAccessorDepth, primitiveBase64EncodeAccessorDepth,
	primitiveDecodeAccessorDepth, primitiveEncodeAccessorDepth, primitiveInfoAccessorDepth,
	primitiveMeasureAccessorDepth, primitiveValidateAccessorDepth;
#endif

static long checks, failures;

static void
fail(const char *format, ...)
{
	va_list args;

	if (++failures <= 50) {
		va_start(args, format);
		printf("U10 FAILED: ");
		vprintf(format, args);
		printf("\n");
		va_end(args);
	}
}

#define CHECK(condition, ...) do { checks++; if (!(condition)) fail(__VA_ARGS__); } while (0)

/* xorshift64*, seeded */
static uint64_t seed = 0x5375474c55450a01ull;

static uint64_t
next(void)
{
	seed ^= seed >> 12;
	seed ^= seed << 25;
	seed ^= seed >> 27;
	return seed * 0x2545F4914F6CDD1Dull;
}

static size_t
below(size_t n)
{
	return n ? (size_t)(next() % n) : 0;
}


/*** The glue ***/

static Primitive pEncode, pDecode, pMeasure, pValidate, pBase64Encode, pBase64Decode, pInfo;

#ifdef GLUE_BUILTIN
static void *
entry(const char *name)
{
	int i;

	for (i = 0; SimdUTFPlugin_exports[i][1]; i++) {
		if (strcmp(SimdUTFPlugin_exports[i][1], name) == 0) {
			return SimdUTFPlugin_exports[i][2];
		}
	}
	return NULL;
}
#endif

static void
checkModule(struct VirtualMachine *vm)
{
#ifdef GLUE_BUILTIN
	static const char *names[] = { "getModuleName", "initialiseModule", "primitiveBase64Decode",
		"primitiveBase64Encode", "primitiveDecode", "primitiveEncode", "primitiveInfo",
		"primitiveMeasure", "primitiveValidate", "setInterpreter", "shutdownModule" };
	const char *(*moduleName)(void) = (const char *(*)(void))entry("getModuleName");
	sqInt (*init)(void) = (sqInt (*)(void))entry("initialiseModule");
	sqInt (*shutdown)(void) = (sqInt (*)(void))entry("shutdownModule");
	sqInt (*set)(struct VirtualMachine *) = (sqInt (*)(struct VirtualMachine *))entry("setInterpreter");
	const char *expected = "SimdUTFPlugin (i)";
	size_t i, rows = 0;

	for (i = 0; SimdUTFPlugin_exports[i][1]; i++) {
		rows++;
		CHECK(strcmp(SimdUTFPlugin_exports[i][0], "SimdUTFPlugin") == 0, "exports row %zu: module %s",
			i, (const char *)SimdUTFPlugin_exports[i][0]);
		CHECK(i < sizeof names / sizeof names[0] && strcmp(SimdUTFPlugin_exports[i][1], names[i]) == 0
			&& SimdUTFPlugin_exports[i][2], "exports row %zu: %s", i, (const char *)SimdUTFPlugin_exports[i][1]);
	}
	CHECK(rows == sizeof names / sizeof names[0], "exports: %zu rows", rows);
	CHECK(SimdUTFPlugin_exports[rows][0] == NULL && SimdUTFPlugin_exports[rows][2] == NULL, "exports: no NULL row");
	pEncode = (Primitive)entry("primitiveEncode");
	pDecode = (Primitive)entry("primitiveDecode");
	pMeasure = (Primitive)entry("primitiveMeasure");
	pValidate = (Primitive)entry("primitiveValidate");
	pBase64Encode = (Primitive)entry("primitiveBase64Encode");
	pBase64Decode = (Primitive)entry("primitiveBase64Decode");
	pInfo = (Primitive)entry("primitiveInfo");
	if (!(moduleName && init && shutdown && set && pEncode && pDecode && pMeasure && pValidate
	      && pBase64Encode && pBase64Decode && pInfo)) {
		printf("U10 FAILED: an entry of SimdUTFPlugin_exports is missing\n");
		exit(1);
	}
#else
	const char *(*moduleName)(void) = getModuleName;
	sqInt (*init)(void) = initialiseModule;
	sqInt (*shutdown)(void) = shutdownModule;
	sqInt (*set)(struct VirtualMachine *) = setInterpreter;
	const char *expected = "SimdUTFPlugin (e)";

	pEncode = primitiveEncode;
	pDecode = primitiveDecode;
	pMeasure = primitiveMeasure;
	pValidate = primitiveValidate;
	pBase64Encode = primitiveBase64Encode;
	pBase64Decode = primitiveBase64Decode;
	pInfo = primitiveInfo;
	CHECK(primitiveBase64DecodeAccessorDepth == 0 && primitiveBase64EncodeAccessorDepth == 0
		&& primitiveDecodeAccessorDepth == 0 && primitiveEncodeAccessorDepth == 0
		&& primitiveInfoAccessorDepth == 0 && primitiveMeasureAccessorDepth == 0
		&& primitiveValidateAccessorDepth == 0, "an AccessorDepth is not 0");
#endif
	CHECK(set(vm) == 1, "setInterpreter refused the proxy");
	CHECK(strcmp(moduleName(), expected) == 0, "getModuleName: %s", moduleName());
	CHECK(init() == 1, "initialiseModule");
	CHECK(shutdown() == 1, "shutdownModule");
}


/*** Calls and answers ***/

static sqInt
I(sqInt value)
{
	return fake_int(value);
}

static sqInt
statusWord(int status, int code, uint64_t payload)
{
	return -(sqInt)((uint64_t)status + 4 * (uint64_t)code + 4096 * payload);
}

/* The Spur bytes of an object with a body of n bytes: a header of 8 bytes,
   another 8 from 255 slots of 8 bytes, and one slot at least */
static uint64_t
spurNeed(uint64_t bodyBytes)
{
	uint64_t slots = (bodyBytes + 7) / 8;

	return 8 + (slots >= 255 ? 8 : 0) + 8 * (slots ? slots : 1);
}

static FakeAnswer
call(Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = fake_call(p, fakeNil, nargs, args);

	checks++;
	if (a.protocolErrors) {
		fail("protocol: %s", fake_last_error());
	}
	return a;
}

static const char *
show(const FakeAnswer *a)
{
	static char text[160];

	if (!a->returned) {
		snprintf(text, sizeof text, "failed %lld", (long long)a->code);
	} else if (fake_is_int(a->value)) {
		snprintf(text, sizeof text, "answered %lld", (long long)fake_int_value(a->value));
	} else {
		snprintf(text, sizeof text, "answered a %s of %zu", fake_class_name(fake_class_of(a->value)),
			fake_size(a->value));
	}
	return text;
}

static void
expectFailure(const char *what, const FakeAnswer *a, sqInt code)
{
	CHECK(!a->returned && a->code == code && a->allocated == 0, "%s: %s, expected failure %lld",
		what, show(a), (long long)code);
}

static void
expectInteger(const char *what, const FakeAnswer *a, sqInt value)
{
	CHECK(a->returned && fake_is_int(a->value) && fake_int_value(a->value) == value,
		"%s: %s, expected %lld", what, show(a), (long long)value);
}

static void
expectObject(const char *what, const FakeAnswer *a, sqInt cls, const void *units, size_t size)
{
	size_t bytes;

	if (!a->returned || fake_is_int(a->value) || fake_class_of(a->value) != cls || fake_size(a->value) != size) {
		fail("%s: %s, expected a %s of %zu", what, show(a), fake_class_name(cls), size);
		return;
	}
	bytes = fake_format(a->value) == 10 ? 4 * size : size;
	CHECK(bytes == 0 || memcmp(fake_body(a->value), units, bytes) == 0, "%s: other units", what);
}

static sqInt
byteString(const char *text)
{
	return fake_bytes(fakeByteString, text, strlen(text));
}

static sqInt
byteArray(const void *bytes, size_t n)
{
	return fake_bytes(fakeByteArray, bytes, n);
}

static sqInt
wideString(const uint32_t *words, size_t n)
{
	return fake_words(fakeWideString, words, n);
}

static sqInt
filled(sqInt cls, size_t size, int value)
{
	sqInt oop = fake_new(cls, size);

	memset(fake_body(oop), value, fake_format(oop) == 10 ? 4 * size : size);
	return oop;
}

static int
isFilled(sqInt oop, int value)
{
	size_t n = fake_format(oop) == 10 ? 4 * fake_size(oop) : fake_size(oop), i;
	const unsigned char *p = fake_body(oop);

	for (i = 0; i < n; i++) {
		if (p[i] != value) {
			return 0;
		}
	}
	return 1;
}

/* UTF-8 and UTF-16 of a code point (written here, not by simdutf) */
static size_t
putUtf8(unsigned char *p, uint32_t c)
{
	if (c < 0x80) {
		p[0] = (unsigned char)c;
		return 1;
	}
	if (c < 0x800) {
		p[0] = (unsigned char)(0xC0 | c >> 6);
		p[1] = (unsigned char)(0x80 | (c & 0x3F));
		return 2;
	}
	if (c < 0x10000) {
		p[0] = (unsigned char)(0xE0 | c >> 12);
		p[1] = (unsigned char)(0x80 | ((c >> 6) & 0x3F));
		p[2] = (unsigned char)(0x80 | (c & 0x3F));
		return 3;
	}
	p[0] = (unsigned char)(0xF0 | c >> 18);
	p[1] = (unsigned char)(0x80 | ((c >> 12) & 0x3F));
	p[2] = (unsigned char)(0x80 | ((c >> 6) & 0x3F));
	p[3] = (unsigned char)(0x80 | (c & 0x3F));
	return 4;
}

static size_t
putUnit16(unsigned char *p, unsigned unit, int bigEndian)
{
	p[bigEndian ? 0 : 1] = (unsigned char)(unit >> 8);
	p[bigEndian ? 1 : 0] = (unsigned char)(unit & 0xFF);
	return 2;
}

static size_t
putUtf16(unsigned char *p, uint32_t c, int bigEndian)
{
	if (c < 0x10000) {
		return putUnit16(p, c, bigEndian);
	}
	c -= 0x10000;
	putUnit16(p, 0xD800 + (c >> 10), bigEndian);
	return 2 + putUnit16(p + 2, 0xDC00 + (c & 0x3FF), bigEndian);
}


/*** primitiveInfo ***/

static int
isByteString(sqInt oop, const char *text)
{
	return fake_is_object(oop) && fake_class_of(oop) == fakeByteString && fake_size(oop) == strlen(text)
		&& memcmp(fake_body(oop), text, strlen(text)) == 0;
}

static void
checkInfo(void)
{
	su_info info;
	FakeAnswer a;
	sqInt r, kernels;
	int i, allocations;

	su_get_info(&info);
	a = call(pInfo, 0, NULL);
	if (!a.returned || fake_is_int(a.value) || fake_class_of(a.value) != fakeArray || fake_size(a.value) != 7) {
		fail("primitiveInfo: %s, expected an Array of 7", show(&a));
		return;
	}
	r = a.value;
	CHECK(fake_slot(r, 0) == I(SU_PROTOCOL), "info slot 1: not the protocol");
	CHECK(isByteString(fake_slot(r, 1), "SimdUTFPlugin 1.0"), "info slot 2: not the plugin version");
	CHECK(isByteString(fake_slot(r, 2), "9.4.0"), "info slot 3: not 9.4.0");
	CHECK(isByteString(fake_slot(r, 3), info.implementation), "info slot 4: not the kernel %s", info.implementation);
	CHECK(isByteString(fake_slot(r, 4), "native"), "info slot 5: not 'native'");
	kernels = fake_slot(r, 5);
	if (!fake_is_object(kernels) || fake_class_of(kernels) != fakeArray || fake_size(kernels) != (size_t)info.count) {
		fail("info slot 6: not an Array of %d kernels", info.count);
	} else {
		for (i = 0; i < info.count; i++) {
			CHECK(isByteString(fake_slot(kernels, (size_t)i), info.implementations[i]), "info slot 6: kernel %d", i);
		}
	}
	CHECK(info.forced[0] ? isByteString(fake_slot(r, 6), info.forced) : fake_slot(r, 6) == fakeNil,
		"info slot 7: not the forced kernel '%s'", info.forced);
	CHECK(info.ready && strcmp(info.implementation, "unsupported") != 0, "info: no kernel");
	allocations = a.allocated;
	CHECK(allocations == 6 + info.count + (info.forced[0] != 0), "info: %d allocations", allocations);
	/* each allocation refused in turn: the VM's failure, its retry */
	for (i = 0; i < allocations; i++) {
		fake_set_budget(i, PrimErrNoMemory);
		a = call(pInfo, 0, NULL);
		CHECK(!a.returned && a.code == PrimErrNoMemory, "info with %d allocations: %s", i, show(&a));
	}
	fake_set_budget(-1, PrimErrNoMemory);
	a = call(pInfo, 1, (sqInt[]){ fakeNil });
	expectFailure("info with an argument", &a, PrimErrBadNumArgs);
}


/*** Arity ***/

static void
checkArity(void)
{
	static const struct { const char *name; Primitive *p; int nargs; } prims[] = {
		{ "encode", &pEncode, 6 }, { "decode", &pDecode, 7 }, { "measure", &pMeasure, 4 },
		{ "validate", &pValidate, 4 }, { "base64Encode", &pBase64Encode, 8 },
		{ "base64Decode", &pBase64Decode, 7 } };
	sqInt args[10];
	size_t i;
	int k, n;
	FakeAnswer a;
	char what[64];

	for (k = 0; k < 10; k++) {
		args[k] = I(1);
	}
	for (i = 0; i < sizeof prims / sizeof prims[0]; i++) {
		for (n = 0; n <= 9; n++) {
			if (n == prims[i].nargs) {
				continue;
			}
			a = call(*prims[i].p, n, args);
			snprintf(what, sizeof what, "%s with %d arguments", prims[i].name, n);
			expectFailure(what, &a, PrimErrBadNumArgs);
		}
	}
}


/*** The order of the checks ***/

/* A bad request: up to three arguments (0-based) replaced */
typedef struct {
	const char *what;
	int count;
	int slot[3];
	sqInt value[3];
	sqInt code;
} Fault;

static int
disjoint(const Fault *f, const Fault *g)
{
	int i, j;

	for (i = 0; i < f->count; i++) {
		for (j = 0; j < g->count; j++) {
			if (f->slot[i] == g->slot[j]) {
				return 0;
			}
		}
	}
	return 1;
}

static void
apply(sqInt *args, const Fault *f)
{
	int i;

	for (i = 0; i < f->count; i++) {
		args[f->slot[i]] = f->value[i];
	}
}

/* faults are in the order of the checks: each alone fails with its code,
   each pair with the code of the earlier one */
static void
checkOrder(const char *name, Primitive p, int nargs, const sqInt *base, const Fault *faults, int count)
{
	sqInt args[10];
	FakeAnswer a;
	char what[200];
	int i, j;

	memcpy(args, base, (size_t)nargs * sizeof args[0]);
	a = call(p, nargs, args);
	CHECK(a.returned, "%s: the base request %s", name, show(&a));
	for (i = 0; i < count; i++) {
		memcpy(args, base, (size_t)nargs * sizeof args[0]);
		apply(args, &faults[i]);
		a = call(p, nargs, args);
		snprintf(what, sizeof what, "%s, %s", name, faults[i].what);
		expectFailure(what, &a, faults[i].code);
		CHECK(a.allocations == 0, "%s: %d allocations", what, a.allocations);
		for (j = i + 1; j < count; j++) {
			if (!disjoint(&faults[i], &faults[j])) {
				continue;
			}
			memcpy(args, base, (size_t)nargs * sizeof args[0]);
			apply(args, &faults[j]);
			apply(args, &faults[i]);
			a = call(p, nargs, args);
			snprintf(what, sizeof what, "%s, %s and %s", name, faults[i].what, faults[j].what);
			expectFailure(what, &a, faults[i].code);
		}
	}
}

#define F1(text, s, v, c) { text, 1, { s }, { v }, c }
#define F2(text, s, v, t, w, c) { text, 2, { s, t }, { v, w }, c }

static void
checkOrders(void)
{
	static const uint32_t text[] = { 'a', 0xE9, 0x20AC, 0x1F600 };
	static const unsigned char utf8[] = { 'a', 0xC3, 0xA9, 0xE2, 0x82, 0xAC };
	size_t mark = fake_mark();
	sqInt array = fake_new(fakeArray, 3);
	sqInt method = fake_compiled_method(16);
	sqInt wide = wideString(text, 4);
	sqInt bytes = byteArray(utf8, 6);
	sqInt base64 = byteString("TWFuTWFu");
	sqInt posing = fake_new(fakeArray, 3);
	sqInt metaWide = fake_class_of(fakeWideString);
	sqInt dest16 = fake_new(fakeByteArray, 16);
	sqInt destWide = fake_new(fakeWideString, 8);
	sqInt destBytes = fake_new(fakeByteString, 8);
	sqInt readOnlyBytes = fake_new(fakeByteArray, 16);
	sqInt readOnlyWide = fake_new(fakeWideString, 8);
	sqInt readOnlyString = fake_new(fakeByteString, 8);
	sqInt destB64 = fake_new(fakeByteString, 40);

	((sqInt *)fake_body(posing))[2] = I(10 << 16);	/* an Array posing as a class of words */
	fake_set_immutable(readOnlyBytes, 1);
	fake_set_immutable(readOnlyWide, 1);
	fake_set_immutable(readOnlyString, 1);

	{	/* encode: aString start stop flags dest index */
		sqInt base[] = { wide, I(1), I(4), I(0), dest16, I(1) };
		Fault faults[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a SmallInteger source", 0, I(5), PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("flags nil", 3, fakeNil, PrimErrBadArgument),
			F2("index nil without destination", 4, fakeNil, 5, fakeNil, PrimErrBadArgument),
			F1("index nil", 5, fakeNil, PrimErrBadArgument),
			F1("flags 1", 3, I(1), PrimErrBadArgument),
			F1("flags 2", 3, I(2), PrimErrBadArgument),
			F1("flags 16", 3, I(16), PrimErrBadArgument),
			F1("flags -1", 3, I(-1), PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 5", 2, I(5), PrimErrBadIndex),
			F2("start 3 stop 1", 1, I(3), 2, I(1), PrimErrBadIndex),
			F1("a WideString destination", 4, destWide, PrimErrBadArgument),
			F1("an Array destination", 4, array, PrimErrBadArgument),
			F1("the source as destination", 4, wide, PrimErrBadArgument),
			F1("a read-only destination", 4, readOnlyBytes, PrimErrNoModification),
			F1("index 0", 5, I(0), PrimErrBadIndex),
			F1("index 18", 5, I(18), PrimErrBadIndex),
			F2("UTF-16 at index 2", 3, I(SU_ENC_UTF16LE), 5, I(2), PrimErrBadIndex),
		};
		checkOrder("encode", pEncode, 6, base, faults, (int)(sizeof faults / sizeof faults[0]));
	}
	{	/* decode: bytes start stop flags wideClass dest index, into a word or a byte object */
		sqInt base[] = { bytes, I(1), I(6), I(0), fakeWideString, destWide, I(1) };
		sqInt baseBytes[] = { bytes, I(1), I(6), I(0), fakeWideString, destBytes, I(1) };
		Fault common[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a SmallInteger source", 0, I(7), PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("flags nil", 3, fakeNil, PrimErrBadArgument),
			F1("index nil", 6, fakeNil, PrimErrBadArgument),
			F1("flags 1", 3, I(1), PrimErrBadArgument),
			F1("flags 2", 3, I(2), PrimErrBadArgument),
			F1("flags 48", 3, I(48), PrimErrBadArgument),
			F1("flags 64", 3, I(64), PrimErrBadArgument),
			F1("flags -1", 3, I(-1), PrimErrBadArgument),
			F1("a WideString source", 0, wide, PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 7", 2, I(7), PrimErrBadIndex),
			F2("UTF-16 from 2", 3, I(SU_ENC_UTF16BE), 1, I(2), PrimErrBadIndex),
			F1("an Array posing as class", 4, posing, PrimErrBadArgument),
			F1("BoxedFloat64 as class", 4, fakeFloat, PrimErrBadArgument),
			F1("a metaclass as class", 4, metaWide, PrimErrBadArgument),
			F1("ByteString as class", 4, fakeByteString, PrimErrBadArgument),
			F1("a class with a named slot", 4, fakeFixedWide, PrimErrBadArgument),
			F1("nil as class", 4, fakeNil, PrimErrBadArgument),
			F1("3 as class", 4, I(3), PrimErrBadArgument),
			F1("an Array destination", 5, array, PrimErrBadArgument),
		};
		Fault toWords[sizeof common / sizeof common[0] + 5], toBytes[sizeof common / sizeof common[0] + 5];
		Fault words[] = {
			F1("Latin-1 into words", 3, I(SU_F_LATIN1), PrimErrBadArgument),
			F1("the source as destination", 5, bytes, PrimErrBadArgument),
			F1("a read-only destination", 5, readOnlyWide, PrimErrNoModification),
			F1("index 0", 6, I(0), PrimErrBadIndex),
			F1("index 10", 6, I(10), PrimErrBadIndex),
		};
		Fault bytesInto[] = {
			F1("wide into bytes", 3, I(SU_F_WIDE), PrimErrBadArgument),
			F1("the source as destination", 5, bytes, PrimErrBadArgument),
			F1("a read-only destination", 5, readOnlyString, PrimErrNoModification),
			F1("index 0", 6, I(0), PrimErrBadIndex),
			F1("index 10", 6, I(10), PrimErrBadIndex),
		};
		size_t n = sizeof common / sizeof common[0];
		memcpy(toWords, common, sizeof common);
		memcpy(toWords + n, words, sizeof words);
		memcpy(toBytes, common, sizeof common);
		memcpy(toBytes + n, bytesInto, sizeof bytesInto);
		checkOrder("decode into words", pDecode, 7, base, toWords, (int)(n + 5));
		checkOrder("decode into bytes", pDecode, 7, baseBytes, toBytes, (int)(n + 5));
	}
	{	/* measure: source start stop flags */
		sqInt base[] = { bytes, I(1), I(6), I(SU_OP_DECODED) };
		Fault faults[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("flags nil", 3, fakeNil, PrimErrBadArgument),
			F1("flags 65", 3, I(65), PrimErrBadArgument),
			F1("flags 80", 3, I(80), PrimErrBadArgument),
			F1("flags 129", 3, I(129), PrimErrBadArgument),
			F1("flags 192", 3, I(192), PrimErrBadArgument),
			F1("flags 256", 3, I(256), PrimErrBadArgument),
			F1("flags 16", 3, I(16), PrimErrBadArgument),
			F1("a WideString source", 0, wide, PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 7", 2, I(7), PrimErrBadIndex),
			F2("UTF-16 from 2", 3, I(SU_OP_DECODED | SU_ENC_UTF16LE), 1, I(2), PrimErrBadIndex),
		};
		checkOrder("measure", pMeasure, 4, base, faults, (int)(sizeof faults / sizeof faults[0]));
	}
	{	/* validate: source start stop flags */
		sqInt base[] = { bytes, I(1), I(6), I(SU_ENC_UTF8) };
		Fault faults[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("flags nil", 3, fakeNil, PrimErrBadArgument),
			F1("flags 1", 3, I(1), PrimErrBadArgument),
			F1("flags 8", 3, I(8), PrimErrBadArgument),
			F1("flags 65", 3, I(65), PrimErrBadArgument),
			F1("a WideString source", 0, wide, PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 7", 2, I(7), PrimErrBadIndex),
			F2("UTF-16 from 2", 3, I(SU_ENC_UTF16BE), 1, I(2), PrimErrBadIndex),
		};
		checkOrder("validate", pValidate, 4, base, faults, (int)(sizeof faults / sizeof faults[0]));
	}
	{	/* base64Encode: bytes start stop options lineLength lineEnd dest index */
		sqInt base[] = { bytes, I(1), I(6), I(0), I(76), I(SU_EOL_CRLF), destB64, I(1) };
		Fault faults[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("options nil", 3, fakeNil, PrimErrBadArgument),
			F1("lineLength nil", 4, fakeNil, PrimErrBadArgument),
			F1("lineEnd nil", 5, fakeNil, PrimErrBadArgument),
			F1("index nil", 7, fakeNil, PrimErrBadArgument),
			F1("options 4", 3, I(4), PrimErrBadArgument),
			F1("options -1", 3, I(-1), PrimErrBadArgument),
			F1("lineLength 6", 4, I(6), PrimErrBadArgument),
			F1("lineLength -4", 4, I(-4), PrimErrBadArgument),
			F1("lineLength 2^30", 4, I((sqInt)1 << 30), PrimErrBadArgument),
			F1("lineEnd 3", 5, I(3), PrimErrBadArgument),
			F1("lineEnd -1", 5, I(-1), PrimErrBadArgument),
			F1("a WideString source", 0, wide, PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 7", 2, I(7), PrimErrBadIndex),
			F1("a WideString destination", 6, destWide, PrimErrBadArgument),
			F1("the source as destination", 6, bytes, PrimErrBadArgument),
			F1("a read-only destination", 6, readOnlyString, PrimErrNoModification),
			F1("index 0", 7, I(0), PrimErrBadIndex),
			F1("index 42", 7, I(42), PrimErrBadIndex),
		};
		checkOrder("base64Encode", pBase64Encode, 8, base, faults, (int)(sizeof faults / sizeof faults[0]));
	}
	{	/* base64Decode: source start stop options mode dest index */
		sqInt base[] = { base64, I(1), I(8), I(0), I(0), dest16, I(1) };
		Fault faults[] = {
			F1("an Array source", 0, array, PrimErrBadArgument),
			F1("a CompiledMethod source", 0, method, PrimErrBadArgument),
			F1("start nil", 1, fakeNil, PrimErrBadArgument),
			F1("stop nil", 2, fakeNil, PrimErrBadArgument),
			F1("options nil", 3, fakeNil, PrimErrBadArgument),
			F1("mode nil", 4, fakeNil, PrimErrBadArgument),
			F1("index nil", 6, fakeNil, PrimErrBadArgument),
			F1("options 2", 3, I(2), PrimErrBadArgument),
			F1("options 3", 3, I(3), PrimErrBadArgument),
			F1("options 9", 3, I(9), PrimErrBadArgument),
			F1("mode 2", 4, I(2), PrimErrBadArgument),
			F1("mode -1", 4, I(-1), PrimErrBadArgument),
			F1("a WideString source", 0, wide, PrimErrBadArgument),
			F1("start 0", 1, I(0), PrimErrBadIndex),
			F1("stop 9", 2, I(9), PrimErrBadIndex),
			F1("a WideString destination", 5, destWide, PrimErrBadArgument),
			F1("the source as destination", 5, base64, PrimErrBadArgument),
			F1("a read-only destination", 5, readOnlyBytes, PrimErrNoModification),
			F1("index 0", 6, I(0), PrimErrBadIndex),
			F1("index 18", 6, I(18), PrimErrBadIndex),
		};
		checkOrder("base64Decode", pBase64Decode, 7, base, faults, (int)(sizeof faults / sizeof faults[0]));
	}
	fake_release(mark);
}


/*** Every value of the flags ***/

static int
isUnicodeEncoding(sqInt e)
{
	return e == SU_ENC_UTF8 || e == SU_ENC_UTF16LE || e == SU_ENC_UTF16BE;
}

static int
encodeFlagsOk(sqInt f)
{
	return (f & ~(sqInt)15) == 0 && isUnicodeEncoding(f & 7);
}

static int
decodeFlagsOk(sqInt f, int words, sqInt dest)
{
	if (words || (f & ~(sqInt)63) != 0 || (f & 48) == 48 || !isUnicodeEncoding(f & 7)) {
		return 0;
	}
	if (dest == fakeNil) {
		return 1;
	}
	return fake_format(dest) == 10 ? !(f & SU_F_LATIN1) : !(f & SU_F_WIDE);
}

static int
measureFlagsOk(sqInt f, int words)
{
	sqInt op = f & 192;

	if (op == 128) {
		return f == 128 && !words;
	}
	return (op == 0 || op == 64) && (f & ~(sqInt)0xCF) == 0 && isUnicodeEncoding(f & 7) && (op == 0 || !words);
}

static int
validateFlagsOk(sqInt f, int words)
{
	return f == 64 || (f == 2 && words) || (isUnicodeEncoding(f) && f >= 0 && !words);
}

static void
checkFlagValue(const char *name, Primitive p, int nargs, sqInt *args, int slot, sqInt f, int ok)
{
	FakeAnswer a;

	args[slot] = I(f);
	a = call(p, nargs, args);
	CHECK(ok ? a.returned : !a.returned && a.code == PrimErrBadArgument,
		"%s with %lld: %s, expected %s", name, (long long)f, show(&a), ok ? "an answer" : "bad argument");
}

static void
checkFlags(void)
{
	static const sqInt far[] = { 1 << 20, (sqInt)1 << 40, -((sqInt)1 << 40), ((sqInt)1 << 59), 256 + 3, 1024 };
	static const sqInt lineLengths[] = { -4, -1, 0, 1, 2, 3, 4, 6, 8, 76, 1000, ((sqInt)1 << 30) - 4,
		(sqInt)1 << 30, ((sqInt)1 << 30) + 4, (sqInt)1 << 40 };
	static const uint32_t text[] = { 'a', 0xE9, 'b', 'c' };
	size_t mark = fake_mark();
	sqInt bytes = byteString("abcd"), words = wideString(text, 4);
	sqInt destWide = fake_new(fakeWideString, 8), destBytes = fake_new(fakeByteString, 8);
	sqInt f;
	size_t i, k;
	int w;
	char name[80];

	for (f = -2; f <= 300 + (sqInt)(sizeof far / sizeof far[0]); f++) {
		sqInt v = f <= 300 ? f : far[f - 301];
		for (w = 0; w < 2; w++) {
			sqInt source = w ? words : bytes;
			sqInt e[] = { source, I(1), I(4), 0, fakeNil, I(1) };
			snprintf(name, sizeof name, "encode of %s", w ? "words" : "bytes");
			checkFlagValue(name, pEncode, 6, e, 3, v, encodeFlagsOk(v));
			{
				sqInt m[] = { source, I(1), I(4), 0 };
				snprintf(name, sizeof name, "measure of %s", w ? "words" : "bytes");
				checkFlagValue(name, pMeasure, 4, m, 3, v, measureFlagsOk(v, w));
				memcpy(m, (sqInt[]){ source, I(1), I(4), 0 }, sizeof m);
				snprintf(name, sizeof name, "validate of %s", w ? "words" : "bytes");
				checkFlagValue(name, pValidate, 4, m, 3, v, validateFlagsOk(v, w));
			}
			for (k = 0; k < 3; k++) {
				sqInt dest = k == 0 ? fakeNil : k == 1 ? destWide : destBytes;
				sqInt d[] = { source, I(1), I(4), 0, fakeWideString, dest, I(1) };
				snprintf(name, sizeof name, "decode of %s into %s", w ? "words" : "bytes",
					k == 0 ? "nil" : k == 1 ? "words" : "bytes");
				checkFlagValue(name, pDecode, 7, d, 3, v, decodeFlagsOk(v, w, dest));
			}
		}
	}
	/* base64: options x line lengths x line ends; options x modes */
	for (f = -2; f <= 10; f++) {
		for (i = 0; i < sizeof lineLengths / sizeof lineLengths[0]; i++) {
			for (k = 0; k < 6; k++) {
				sqInt lineEnd = (sqInt)k - 2;
				sqInt ll = lineLengths[i];
				int ok = f >= 0 && f <= 3 && lineEnd >= 0 && lineEnd <= 2
					&& ll >= 0 && ll % 4 == 0 && ll < ((sqInt)1 << 30);
				FakeAnswer a;
				for (w = 0; w < 2; w++) {
					sqInt b[] = { w ? words : bytes, I(1), I(4), I(f), I(ll), I(lineEnd), fakeNil, I(1) };
					a = call(pBase64Encode, 8, b);
					CHECK((ok && !w) ? a.returned : !a.returned && a.code == PrimErrBadArgument,
						"base64Encode of %s, options %lld lineLength %lld lineEnd %lld: %s",
						w ? "words" : "bytes", (long long)f, (long long)ll, (long long)lineEnd, show(&a));
				}
			}
		}
		for (k = 0; k < 6; k++) {
			sqInt mode = (sqInt)k - 2;
			int ok = (f == 0 || f == 1 || f == 8) && (mode == 0 || mode == 1);
			FakeAnswer a;
			for (w = 0; w < 2; w++) {
				sqInt b[] = { w ? words : bytes, I(1), I(4), I(f), I(mode), fakeNil, I(1) };
				a = call(pBase64Decode, 7, b);
				CHECK((ok && !w) ? a.returned : !a.returned && a.code == PrimErrBadArgument,
					"base64Decode of %s, options %lld mode %lld: %s",
					w ? "words" : "bytes", (long long)f, (long long)mode, show(&a));
			}
		}
	}
	fake_release(mark);
}


/*** Crafted answers ***/

static void
checkAnswers(void)
{
	static const uint32_t text[] = { 'a', 0xE9, 0x20AC, 0x1F600 };
	static const unsigned char utf8[] = { 'a', 0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x9F, 0x98, 0x80 };
	static const unsigned char utf16be[] = { 0, 'a', 0, 0xE9, 0x20, 0xAC, 0xD8, 0x3D, 0xDE, 0x00 };
	static const unsigned char bomUtf8[] = { 0xEF, 0xBB, 0xBF, 'a', 0xC3, 0xA9, 0xE2, 0x82, 0xAC, 0xF0, 0x9F, 0x98, 0x80 };
	static const unsigned char latin1[] = { 'a', 0xE9 };
	size_t mark = fake_mark();
	sqInt wide = wideString(text, 4);
	sqInt u8 = byteArray(utf8, sizeof utf8);
	FakeAnswer a;
	sqInt dest;

	/* encode */
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF8), fakeNil, I(1) });
	expectObject("encode to UTF-8", &a, fakeByteArray, utf8, sizeof utf8);
	CHECK(a.allocations == 1 && a.stores == 0, "encode: %d allocations, %d stores", a.allocations, a.stores);
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF8 | SU_F_BOM), fakeNil, I(1) });
	expectObject("encode to UTF-8 with a BOM", &a, fakeByteArray, bomUtf8, sizeof bomUtf8);
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF16BE), fakeNil, I(1) });
	expectObject("encode to UTF-16BE", &a, fakeByteArray, utf16be, sizeof utf16be);
	a = call(pEncode, 6, (sqInt[]){ wide, I(2), I(3), I(SU_ENC_UTF8), fakeNil, I(1) });
	expectObject("encode of 2 to 3", &a, fakeByteArray, utf8 + 1, 5);
	a = call(pEncode, 6, (sqInt[]){ wide, I(3), I(2), I(SU_ENC_UTF8), fakeNil, I(1) });
	expectObject("encode of an empty range", &a, fakeByteArray, "", 0);
	a = call(pEncode, 6, (sqInt[]){ byteString("a\xE9"), I(1), I(2), I(SU_ENC_UTF8), fakeNil, I(1) });
	expectObject("encode of a ByteString", &a, fakeByteArray, utf8, 3);
	a = call(pEncode, 6, (sqInt[]){ fake_bytes(fakeByteSymbol, "a\xE9", 2), I(1), I(2), I(SU_ENC_UTF8), fakeNil, I(1) });
	expectObject("encode of a ByteSymbol", &a, fakeByteArray, utf8, 3);
	{
		uint32_t bad[] = { 'a', 'b', 0xD800, 'c' };
		a = call(pEncode, 6, (sqInt[]){ fake_words(fakeWordArray, bad, 4), I(2), I(4), I(SU_ENC_UTF8), fakeNil, I(1) });
		expectInteger("encode of a surrogate", &a, statusWord(SU_ERROR, SU_E_SURROGATE, 3));
		CHECK(a.allocations == 0, "encode of a surrogate: %d allocations", a.allocations);
		bad[2] = 0x110000;
		a = call(pEncode, 6, (sqInt[]){ wideString(bad, 4), I(1), I(4), I(SU_ENC_UTF16LE), fakeNil, I(1) });
		expectInteger("encode of 0x110000", &a, statusWord(SU_ERROR, SU_E_TOO_LARGE, 3));
	}
	/* encode into: written from index on, nothing else touched */
	dest = filled(fakeByteArray, 16, 0xEE);
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF8), dest, I(4) });
	expectInteger("encode into", &a, 10);
	CHECK(a.allocations == 0, "encode into: %d allocations", a.allocations);
	{
		unsigned char want[16];
		memset(want, 0xEE, sizeof want);
		memcpy(want + 3, utf8, sizeof utf8);
		CHECK(memcmp(fake_body(dest), want, 16) == 0, "encode into: other bytes");
	}
	dest = filled(fakeByteString, 12, 0xEE);
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF8), dest, I(4) });
	expectInteger("encode into a room 1 short", &a, statusWord(SU_TOO_LARGE, 0, 10));
	CHECK(isFilled(dest, 0xEE), "encode into a room 1 short: written");
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF8), dest, I(13) });
	expectInteger("encode into at the end", &a, statusWord(SU_TOO_LARGE, 0, 10));
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF16LE), dest, I(5) });
	expectInteger("encode UTF-16 into at 5", &a, statusWord(SU_TOO_LARGE, 0, 10));
	a = call(pEncode, 6, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF16LE), dest, I(3) });
	expectInteger("encode UTF-16 into at 3", &a, 10);
	a = call(pEncode, 6, (sqInt[]){ wide, I(3), I(2), I(SU_ENC_UTF8), dest, I(13) });
	expectInteger("encode of nothing into at the end", &a, 0);

	/* decode */
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(3), I(SU_ENC_UTF8), fakeWideString, fakeNil, I(1) });
	expectObject("decode to a ByteString", &a, fakeByteString, latin1, 2);
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8), fakeOtherWide, fakeNil, I(1) });
	expectObject("decode to the class argument", &a, fakeOtherWide, text, 4);
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(3), I(SU_ENC_UTF8 | SU_F_WIDE), fakeWideString, fakeNil, I(1) });
	expectObject("decode forced wide", &a, fakeWideString, text, 2);
	a = call(pDecode, 7, (sqInt[]){ byteArray(bomUtf8, sizeof bomUtf8), I(1), I(13), I(SU_ENC_UTF8 | SU_F_BOM),
		fakeWideString, fakeNil, I(1) });
	expectObject("decode skipping a BOM", &a, fakeWideString, text, 4);
	a = call(pDecode, 7, (sqInt[]){ byteArray(utf16be, sizeof utf16be), I(1), I(10), I(SU_ENC_UTF16BE),
		fakeWideString, fakeNil, I(1) });
	expectObject("decode of UTF-16BE", &a, fakeWideString, text, 4);
	a = call(pDecode, 7, (sqInt[]){ byteArray(utf16be, sizeof utf16be), I(3), I(9), I(SU_ENC_UTF16BE),
		fakeWideString, fakeNil, I(1) });
	expectInteger("decode of an odd UTF-16 length", &a, statusWord(SU_ERROR, SU_E_ODD_UTF16_LENGTH, 9));
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8 | SU_F_LATIN1), fakeWideString, fakeNil, I(1) });
	expectInteger("decode forced Latin-1", &a, statusWord(SU_ERROR, SU_E_NOT_LATIN1, 4));
	CHECK(a.allocations == 0, "decode forced Latin-1: %d allocations", a.allocations);
	{
		unsigned char bad[] = { 'x', 'y', 0xE2, 0x82, 0xAC, 0xC0, 0x80, 'z' };
		a = call(pDecode, 7, (sqInt[]){ byteArray(bad, sizeof bad), I(2), I(8), I(SU_ENC_UTF8), fakeWideString,
			fakeNil, I(1) });
		expectInteger("decode of an overlong", &a, statusWord(SU_ERROR, SU_E_OVERLONG, 6));
		CHECK(a.allocations == 0, "decode of an overlong: %d allocations", a.allocations);
	}
	/* decode into a WideString and a ByteString */
	dest = filled(fakeWideString, 6, 0xEE);
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8), fakeWideString, dest, I(2) });
	expectInteger("decode into words", &a, 4);
	CHECK(memcmp((uint32_t *)fake_body(dest) + 1, text, 16) == 0
		&& ((uint32_t *)fake_body(dest))[0] == 0xEEEEEEEE && ((uint32_t *)fake_body(dest))[5] == 0xEEEEEEEE,
		"decode into words: other words");
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8), fakeWideString, dest, I(4) });
	expectInteger("decode into words 1 short", &a, statusWord(SU_TOO_LARGE, 0, 4));
	dest = filled(fakeByteString, 4, 0xEE);
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8), fakeWideString, dest, I(1) });
	expectInteger("decode of wide text into bytes", &a, statusWord(SU_ERROR, SU_E_NOT_LATIN1, 4));
	CHECK(isFilled(dest, 0xEE), "decode of wide text into bytes: written");
	a = call(pDecode, 7, (sqInt[]){ u8, I(1), I(3), I(SU_ENC_UTF8), fakeWideString, dest, I(3) });
	expectInteger("decode into bytes", &a, 2);
	CHECK(memcmp((unsigned char *)fake_body(dest) + 2, latin1, 2) == 0, "decode into bytes: other bytes");

	/* measure and validate */
	a = call(pMeasure, 4, (sqInt[]){ wide, I(1), I(4), I(SU_OP_ENCODED | SU_ENC_UTF16LE | SU_F_BOM) });
	expectInteger("measure of UTF-16LE with a BOM", &a, 12);
	a = call(pMeasure, 4, (sqInt[]){ u8, I(1), I(10), I(SU_OP_DECODED | SU_ENC_UTF8) });
	expectInteger("measure of wide text", &a, 4 * 2 + 1);
	a = call(pMeasure, 4, (sqInt[]){ u8, I(1), I(3), I(SU_OP_DECODED | SU_ENC_UTF8) });
	expectInteger("measure of Latin-1 text", &a, 2 * 2);
	a = call(pMeasure, 4, (sqInt[]){ byteString(" TW Fu\nTQ==\r\n"), I(1), I(13), I(SU_OP_BASE64) });
	expectInteger("measure of base64", &a, 4);
	a = call(pMeasure, 4, (sqInt[]){ u8, I(1), I(9), I(SU_OP_DECODED | SU_ENC_UTF8) });
	expectInteger("measure of a cut sequence", &a, statusWord(SU_ERROR, SU_E_TOO_SHORT, 7));
	a = call(pValidate, 4, (sqInt[]){ u8, I(1), I(10), I(SU_ENC_UTF8) });
	expectInteger("validate", &a, 0);
	a = call(pValidate, 4, (sqInt[]){ u8, I(1), I(10), I(SU_OP_ASCII) });
	expectInteger("validate ASCII", &a, statusWord(SU_ERROR, SU_E_TOO_LARGE, 2));
	a = call(pValidate, 4, (sqInt[]){ wide, I(1), I(4), I(SU_OP_ASCII) });
	expectInteger("validate ASCII of words", &a, statusWord(SU_ERROR, SU_E_TOO_LARGE, 2));
	a = call(pValidate, 4, (sqInt[]){ wide, I(1), I(4), I(SU_ENC_UTF32) });
	expectInteger("validate of words", &a, 0);
	{
		/* a position beyond 2^18: the status word needs 64-bit SmallIntegers */
		size_t n = 300000;
		sqInt big = fake_new(fakeByteArray, n);
		memset(fake_body(big), 'a', n);
		((unsigned char *)fake_body(big))[n - 1] = 0xFF;
		a = call(pValidate, 4, (sqInt[]){ big, I(1), I((sqInt)n), I(SU_ENC_UTF8) });
		expectInteger("validate of 300000 bytes", &a, statusWord(SU_ERROR, SU_E_HEADER_BITS, n));
		a = call(pDecode, 7, (sqInt[]){ big, I(2), I((sqInt)n - 1), I(SU_ENC_UTF8), fakeWideString,
			fake_new(fakeByteArray, 5), I(1) });
		expectInteger("decode of 299998 bytes into 5", &a, statusWord(SU_TOO_LARGE, 0, n - 2));
	}

	/* base64 */
	a = call(pBase64Encode, 8, (sqInt[]){ byteString("Man"), I(1), I(3), I(0), I(0), I(0), fakeNil, I(1) });
	expectObject("base64Encode", &a, fakeByteString, "TWFu", 4);
	a = call(pBase64Encode, 8, (sqInt[]){ byteString("ManMa"), I(1), I(5), I(0), I(4), I(SU_EOL_CRLF), fakeNil, I(1) });
	expectObject("base64Encode in lines", &a, fakeByteString, "TWFu\r\nTWE=\r\n", 12);
	a = call(pBase64Encode, 8, (sqInt[]){ byteString("ManMa"), I(1), I(5), I(1), I(4), I(SU_EOL_LF), fakeNil, I(1) });
	expectObject("base64Encode URL in lines", &a, fakeByteString, "TWFu\nTWE\n", 9);
	dest = filled(fakeByteArray, 8, 0xEE);
	a = call(pBase64Encode, 8, (sqInt[]){ byteString("Man"), I(1), I(3), I(0), I(0), I(0), dest, I(5) });
	expectInteger("base64Encode into", &a, 4);
	CHECK(memcmp((char *)fake_body(dest) + 4, "TWFu", 4) == 0 && ((unsigned char *)fake_body(dest))[3] == 0xEE,
		"base64Encode into: other bytes");
	a = call(pBase64Encode, 8, (sqInt[]){ byteString("Man"), I(1), I(3), I(0), I(0), I(0), dest, I(6) });
	expectInteger("base64Encode into 1 short", &a, statusWord(SU_TOO_LARGE, 0, 4));
	a = call(pBase64Decode, 7, (sqInt[]){ byteString(" TW Fu\nTQ==\r\n"), I(1), I(13), I(0), I(1), fakeNil, I(1) });
	expectObject("base64Decode", &a, fakeByteArray, "ManM", 4);
	a = call(pBase64Decode, 7, (sqInt[]){ byteString("xxTW*u"), I(3), I(6), I(0), I(0), fakeNil, I(1) });
	expectInteger("base64Decode of a bad character", &a, statusWord(SU_ERROR, SU_E_INVALID_BASE64_CHARACTER, 5));
	a = call(pBase64Decode, 7, (sqInt[]){ byteString("TWFuTQ \n"), I(1), I(8), I(0), I(1), fakeNil, I(1) });
	expectInteger("base64Decode strict without padding", &a, statusWord(SU_ERROR, SU_E_BASE64_INPUT_REMAINDER, 6));
	dest = filled(fakeByteString, 6, 0xEE);
	a = call(pBase64Decode, 7, (sqInt[]){ byteString("TWFuTQ=="), I(1), I(8), I(0), I(0), dest, I(3) });
	expectInteger("base64Decode into", &a, 4);
	CHECK(memcmp((char *)fake_body(dest) + 2, "ManM", 4) == 0, "base64Decode into: other bytes");
	a = call(pBase64Decode, 7, (sqInt[]){ byteString("TWFuTQ=="), I(1), I(8), I(0), I(0), dest, I(4) });
	expectInteger("base64Decode into 1 short", &a, statusWord(SU_TOO_LARGE, 0, 4));
	fake_release(mark);
}


/*** Random calls against the engine ***/

typedef struct {
	void *units;
	int kind;
	size_t size;
	int calls;
} Made;

static void *
makeResult(void *ctx, int kind, size_t units)
{
	Made *m = ctx;

	m->calls++;
	m->kind = kind;
	m->size = units;
	m->units = calloc(units ? (kind == SU_KIND_UTF32 ? 4 * units : units) : 1, 1);
	return m->units;
}

/* Random text of n code points of one of five families, as UTF-8, UTF-16
   or words, sometimes with an error in it */
static uint32_t
randomCodePoint(int family)
{
	switch (family) {
	case 0: return 0x20 + (uint32_t)below(0x5F);
	case 1: return below(4) ? 0x20 + (uint32_t)below(0x5F) : 0xA0 + (uint32_t)below(0x60);
	case 2: return below(3) ? 0x20 + (uint32_t)below(0x5F) : 0x100 + (uint32_t)below(0xD700);
	case 3: return 0x10000 + (uint32_t)below(0x100000);
	default: {
		uint32_t c = (uint32_t)below(0x110000);
		return c >= 0xD800 && c < 0xE000 ? c - 0x800 : c;
	}
	}
}

static size_t
randomLength(void)
{
	switch (below(10)) {
	case 0: return below(4);
	case 1: return 1000 + below(5000);
	default: return below(300);
	}
}

/* A byte or word source for the operation op (0 encode, 1 decode, 2
   measure, 3 validate, 4 base64 encode, 5 base64 decode) and its encoding */
static sqInt
randomSource(int words, int encoding, int base64)
{
	size_t n = randomLength(), i, at = 0;
	int family = (int)below(5);
	unsigned char *b = malloc(4 * n + 8);
	sqInt oop;

	if (words) {
		uint32_t *w = (uint32_t *)b;
		for (i = 0; i < n; i++) {
			w[i] = randomCodePoint(family);
		}
		if (n && !below(4)) {	/* an invalid word */
			static const uint32_t bad[] = { 0xD800, 0xDFFF, 0x110000, 0x3FFFFFFF, 0xFFFFFFFF, 0x80 };
			w[below(n)] = bad[below(6)];
		}
		oop = fake_words(below(2) ? fakeWideString : below(2) ? fakeWideSymbol : fakeWordArray, w, n);
		free(b);
		return oop;
	}
	if (base64) {
		static const char alphabet[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/-_= \n\r\t";
		for (i = 0; i < n; i++) {
			b[at++] = (unsigned char)alphabet[below(below(8) ? 64 : sizeof alphabet - 1)];
		}
		if (n && !below(5)) {
			b[below(n)] = (unsigned char)below(256);
		}
	} else if (encoding == SU_ENC_UTF8 || encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE) {
		for (i = 0; i < n; i++) {
			uint32_t c = randomCodePoint(family);
			at += encoding == SU_ENC_UTF8 ? putUtf8(b + at, c) : putUtf16(b + at, c, encoding == SU_ENC_UTF16BE);
		}
		if (at && !below(4)) {
			b[below(at)] = (unsigned char)below(256);
		}
		if (!below(20)) {
			b[at++] = (unsigned char)below(256);	/* an odd UTF-16 length */
		}
	} else {
		for (i = 0; i < n; i++) {
			b[at++] = (unsigned char)randomCodePoint(below(2));
		}
	}
	oop = fake_bytes(below(3) == 0 ? fakeByteArray : below(2) ? fakeByteString : fakeByteSymbol, b, at);
	free(b);
	return oop;
}

/* A random range of the units, from an odd start when even is set */
static void
randomRange(size_t units, int even, sqInt *start, sqInt *stop)
{
	size_t a = 1 + below(units + 1), b;

	if (below(3) == 0) {
		a = 1;
	}
	if (even && (a - 1) % 2) {
		a--;
	}
	b = a - 1 + below(units - (a - 1) + 1);
	if (below(3) == 0) {
		b = units;
	}
	*start = (sqInt)a;
	*stop = (sqInt)b;
}

/* nil, or a byte (or word) object of a random size, and an index into it */
static sqInt
randomDestination(int bytesTaken, int wordsTaken, size_t need, int evenIndex, sqInt *index)
{
	int words;
	size_t size, at;
	sqInt dest;

	*index = 1;
	if (below(2) || (!bytesTaken && !wordsTaken)) {
		return fakeNil;
	}
	words = !bytesTaken || (wordsTaken && below(2));
	size = need + below(8);
	if (below(3) == 0 && size > 0) {
		size = below(size);	/* too small, or just */
	}
	dest = filled(words ? (below(2) ? fakeWideString : fakeWordArray) : (below(2) ? fakeByteArray : fakeByteString),
		size, 0xA5);
	at = 1 + below(size + 1);
	if (evenIndex && (at - 1) % 2) {
		at--;
	}
	*index = (sqInt)at;
	return dest;
}

static int
sameUnits(sqInt oop, const void *units, size_t size, int kind)
{
	size_t bytes = kind == SU_KIND_UTF32 ? 4 * size : size;

	return fake_size(oop) == size && (bytes == 0 || memcmp(fake_body(oop), units, bytes) == 0);
}

/* The answer the primitive must give, from the engine called directly:
   rc and out of an allocate-mode call with made, or of an into-mode call
   on a copy of the destination */
/* What the random calls answered: new objects, numbers (units written, measures, 0
   for valid), ERROR, TOO_LARGE */
static long answeredObjects, answeredNumbers, answeredErrors, answeredTooLarge;

static void
compare(const char *what, const FakeAnswer *a, int rc, const su_outcome *out, const Made *made, sqInt dest,
	const void *copy, sqInt wideClass, int measureDecoded)
{
	sqInt expected;

	if (rc != SU_RC_DONE && rc != SU_RC_NO_ROOM) {
		fail("%s: the engine refused the request (%d)", what, rc);
		return;
	}
	CHECK(a->allocations <= 1, "%s: %d allocations", what, a->allocations);
	if (rc == SU_RC_DONE && out->status == SU_ERROR) {
		answeredErrors++;
		expectInteger(what, a, statusWord(SU_ERROR, out->code, (uint64_t)out->position0 + 1));
	} else if (rc == SU_RC_NO_ROOM) {
		if (dest == fakeNil) {
			fail("%s: no room in allocate mode", what);
			return;
		}
		answeredTooLarge++;
		expectInteger(what, a, statusWord(SU_TOO_LARGE, 0, out->units));
	} else if (dest != fakeNil || !made) {
		answeredNumbers++;
		expected = measureDecoded ? 2 * (sqInt)out->units + (out->kind == SU_KIND_UTF32) : (sqInt)out->units;
		expectInteger(what, a, expected);
	} else {
		answeredObjects++;
		sqInt cls = made->kind == SU_KIND_UTF32 ? wideClass
			: made->kind == SU_KIND_LATIN1 ? fakeByteString : fakeByteArray;
		if (!a->returned || fake_is_int(a->value) || fake_class_of(a->value) != cls
		    || !sameUnits(a->value, made->units, made->size, made->kind)) {
			fail("%s: %s, expected a %s of %zu", what, show(a), fake_class_name(cls), made->size);
		}
		CHECK(a->allocations == 1 && a->allocated == 1, "%s: %d allocations", what, a->allocations);
	}
	if (dest != fakeNil) {
		size_t bytes = fake_format(dest) == 10 ? 4 * fake_size(dest) : fake_size(dest);
		CHECK(a->allocations == 0, "%s: %d allocations into a destination", what, a->allocations);
		CHECK(bytes == 0 || memcmp(fake_body(dest), copy, bytes) == 0, "%s: the destination differs", what);
	}
}

static void
checkRandom(long rounds)
{
	long r;
	char what[160];

	for (r = 0; r < rounds; r++) {
		size_t mark = fake_mark();
		int op = (int)(r % 6);
		FakeAnswer a;
		su_input in;
		su_sink sink;
		su_outcome out;
		Made made;
		sqInt source, start, stop, dest = fakeNil, index = 1, wideClass = fakeWideString;
		void *copy = NULL;
		int rc = SU_RC_BAD_ARGUMENT, words, encoding, isDestWords = 0;
		unsigned flags = 0;
		size_t units, unitBytes;
		static const int encodings[] = { SU_ENC_UTF8, SU_ENC_UTF16LE, SU_ENC_UTF16BE };

		memset(&made, 0, sizeof made);
		memset(&out, 0, sizeof out);
		memset(&sink, 0, sizeof sink);
		encoding = encodings[below(3)];
		words = (op == 0 || op == 2 || op == 3) && below(2);
		if (op == 2 && words) {
			flags = SU_OP_ENCODED;
		} else if (op == 3 && words) {
			flags = below(2) ? SU_OP_ASCII : SU_OP_VALID | SU_ENC_UTF32;
		}
		source = randomSource(words, op == 0 ? -1 : encoding, op == 5 || (op == 2 && !words && below(3) == 0));
		units = fake_size(source);
		in.base = fake_body(source);
		in.units = units;
		in.words = words;
		switch (op) {
		case 0:	/* encode */
			flags = (unsigned)encoding | (below(4) == 0 ? SU_F_BOM : 0);
			randomRange(units, 0, &start, &stop);
			break;
		case 1:	/* decode */
			flags = (unsigned)encoding | (below(4) == 0 ? SU_F_BOM : 0)
				| (below(4) == 0 ? SU_F_WIDE : below(4) == 0 ? SU_F_LATIN1 : 0);
			wideClass = below(2) ? fakeWideString : fakeOtherWide;
			randomRange(units, encoding != SU_ENC_UTF8, &start, &stop);
			break;
		case 2:	/* measure */
			if (!words) {
				switch (below(3)) {
				case 0: flags = SU_OP_ENCODED | (unsigned)encoding; break;
				case 1: flags = SU_OP_DECODED | (unsigned)encoding; break;
				default: flags = SU_OP_BASE64; break;
				}
				if (flags != SU_OP_BASE64 && below(4) == 0) {
					flags |= SU_F_BOM;
				}
			} else {
				flags |= (unsigned)encoding | (below(4) == 0 ? SU_F_BOM : 0);
			}
			randomRange(units, (flags & SU_F_OP) == SU_OP_DECODED && encoding != SU_ENC_UTF8, &start, &stop);
			break;
		case 3:	/* validate */
			if (!words) {
				flags = below(4) == 0 ? SU_OP_ASCII : (unsigned)encoding;
			}
			randomRange(units, flags == SU_ENC_UTF16LE || flags == SU_ENC_UTF16BE, &start, &stop);
			break;
		default:
			randomRange(units, 0, &start, &stop);
			break;
		}
		in.start0 = (size_t)start - 1;
		in.stop0 = (size_t)stop;

		if (op == 2 || op == 3) {
			sqInt args[] = { source, I(start), I(stop), I((sqInt)flags) };
			rc = op == 2 ? su_measure(&in, flags, &out) : su_validate(&in, flags, &out);
			a = call(op == 2 ? pMeasure : pValidate, 4, args);
			snprintf(what, sizeof what, "random %s of %zu %s from %lld to %lld, flags %u",
				op == 2 ? "measure" : "validate", units, words ? "words" : "bytes",
				(long long)start, (long long)stop, flags);
			compare(what, &a, rc, &out, NULL, fakeNil, NULL, 0, op == 2 && (flags & SU_F_OP) == SU_OP_DECODED);
			fake_release(mark);
			continue;
		}

		/* the size of the result, for a destination of about that size */
		{
			su_outcome m;
			memset(&m, 0, sizeof m);
			if (op == 0) {
				su_measure(&in, (flags & ~(unsigned)SU_F_OP) | SU_OP_ENCODED, &m);
			} else if (op == 1) {
				su_measure(&in, (flags & (SU_F_ENCODING | SU_F_BOM)) | SU_OP_DECODED, &m);
			} else if (op == 5) {
				su_measure(&in, SU_OP_BASE64, &m);
			} else {
				m.units = su_base64_encoded_size(in.stop0 - in.start0, 0, 0, 0);
			}
			dest = randomDestination(op != 1 || !(flags & SU_F_WIDE), op == 1 && !(flags & SU_F_LATIN1),
				m.units, op == 0 && encoding != SU_ENC_UTF8, &index);
		}
		if (dest != fakeNil) {
			isDestWords = fake_format(dest) == 10;
			unitBytes = isDestWords ? 4 : 1;
			copy = malloc(fake_size(dest) * unitBytes + 1);
			memcpy(copy, fake_body(dest), fake_size(dest) * unitBytes);
			sink.base = (char *)copy + ((size_t)index - 1) * unitBytes;
			sink.capacity = fake_size(dest) - ((size_t)index - 1);
			sink.kind = op == 1 ? (isDestWords ? SU_KIND_UTF32 : SU_KIND_LATIN1)
				: op == 4 ? SU_KIND_LATIN1 : SU_KIND_BYTES;
		} else {
			sink.ctx = &made;
			sink.allocate = makeResult;
		}
		switch (op) {
		case 0: {
			sqInt args[] = { source, I(start), I(stop), I((sqInt)flags), dest, I(index) };
			rc = su_encode(&in, flags, &sink, &out);
			a = call(pEncode, 6, args);
			snprintf(what, sizeof what, "random encode of %zu %s from %lld to %lld, flags %u, into %zu at %lld",
				units, words ? "words" : "bytes", (long long)start, (long long)stop, flags,
				dest == fakeNil ? 0 : fake_size(dest), (long long)index);
			break;
		}
		case 1: {
			sqInt args[] = { source, I(start), I(stop), I((sqInt)flags), wideClass, dest, I(index) };
			rc = su_decode(&in, flags, &sink, &out);
			a = call(pDecode, 7, args);
			snprintf(what, sizeof what, "random decode of %zu bytes from %lld to %lld, flags %u, into %zu %s at %lld",
				units, (long long)start, (long long)stop, flags, dest == fakeNil ? 0 : fake_size(dest),
				isDestWords ? "words" : "bytes", (long long)index);
			break;
		}
		case 4: {
			unsigned options = (unsigned)below(4), lineEnd = (unsigned)below(3);
			static const size_t lineLengths[] = { 0, 4, 8, 12, 64, 76 };
			size_t ll = below(5) ? lineLengths[below(6)] : 4 * (1 + below(40));
			sqInt args[] = { source, I(start), I(stop), I(options), I((sqInt)ll), I(lineEnd), dest, I(index) };
			rc = su_base64_encode(&in, options, ll, lineEnd, &sink, &out);
			a = call(pBase64Encode, 8, args);
			snprintf(what, sizeof what, "random base64Encode of %zu bytes from %lld to %lld, %u %zu %u, into %zu at %lld",
				units, (long long)start, (long long)stop, options, ll, lineEnd,
				dest == fakeNil ? 0 : fake_size(dest), (long long)index);
			break;
		}
		default: {
			static const unsigned options[] = { SU_B64_STANDARD, SU_B64_URL, SU_B64_ANY };
			unsigned o = options[below(3)], mode = (unsigned)below(2);
			sqInt args[] = { source, I(start), I(stop), I(o), I(mode), dest, I(index) };
			rc = su_base64_decode(&in, o, mode, &sink, &out);
			a = call(pBase64Decode, 7, args);
			snprintf(what, sizeof what, "random base64Decode of %zu bytes from %lld to %lld, %u %u, into %zu at %lld",
				units, (long long)start, (long long)stop, o, mode,
				dest == fakeNil ? 0 : fake_size(dest), (long long)index);
			break;
		}
		}
		compare(what, &a, rc, &out, &made, dest, copy, wideClass, 0);
		if ((op == 0 || op == 1) && rc == SU_RC_DONE && out.status == SU_ERROR) {
			CHECK(a.allocations == 0, "%s: an allocation for invalid input", what);
		}
		free(made.units);
		free(copy);
		fake_release(mark);
	}
	printf("U10 random: %ld calls: %ld new objects, %ld numbers, %ld errors, %ld too large\n",
		rounds, answeredObjects, answeredNumbers, answeredErrors, answeredTooLarge);
}


/*** Refused allocations ***/

static void
checkBudget(void)
{
	static const size_t sizes[] = { 0, 1, 7, 8, 9, 2032, 2033, 2040, 2041, 5000 };
	/* the Spur bytes of byte objects of those sizes, worked out by hand */
	static const uint64_t needs[] = { 16, 16, 16, 16, 24, 2040, 2056, 2056, 2064, 5016 };
	static const sqInt codes[] = { PrimErrBadReceiver, PrimErrUnsupported, PrimErrGenericFailure };
	size_t i, k;
	char what[120];

	for (i = 0; i < sizeof sizes / sizeof sizes[0]; i++) {
		size_t mark = fake_mark(), n = sizes[i];
		sqInt ascii = fake_new(fakeByteString, n), wide = fake_new(fakeWideString, n);
		FakeAnswer a;
		uint32_t *w = fake_body(wide);
		size_t j;

		memset(fake_body(ascii), 'M', n);
		for (j = 0; j < n; j++) {
			w[j] = 0x20AC;
		}
		CHECK(spurNeed(n) == needs[i], "spurNeed(%zu)", n);
		for (k = 0; k <= sizeof codes / sizeof codes[0]; k++) {
			sqInt code = k == 0 ? PrimErrNoMemory : codes[k - 1];
			struct { const char *name; Primitive p; int nargs; sqInt args[8]; uint64_t body; } calls[] = {
				{ "encode", pEncode, 6, { ascii, I(1), I((sqInt)n), I(SU_ENC_UTF8), fakeNil, I(1) }, n },
				{ "decode", pDecode, 7, { ascii, I(1), I((sqInt)n), I(SU_ENC_UTF8), fakeWideString, fakeNil, I(1) }, n },
				{ "decode wide", pDecode, 7, { ascii, I(1), I((sqInt)n), I(SU_ENC_UTF8 | SU_F_WIDE), fakeWideString,
					fakeNil, I(1) }, 4 * n },
				{ "encode to UTF-16", pEncode, 6, { wide, I(1), I((sqInt)n), I(SU_ENC_UTF16LE), fakeNil, I(1) }, 2 * n },
				{ "base64Decode", pBase64Decode, 7, { ascii, I(1), I((sqInt)(n / 4 * 4)), I(0), I(0), fakeNil, I(1) },
					n / 4 * 3 },
				{ "base64Encode", pBase64Encode, 8, { ascii, I(1), I((sqInt)n), I(0), I(0), I(0), fakeNil, I(1) },
					(n + 2) / 3 * 4 },
			};
			size_t c;
			for (c = 0; c < sizeof calls / sizeof calls[0]; c++) {
				fake_set_budget(0, code);
				a = call(calls[c].p, calls[c].nargs, calls[c].args);
				fake_set_budget(-1, PrimErrNoMemory);
				snprintf(what, sizeof what, "%s of %zu with no memory (%lld)", calls[c].name, n, (long long)code);
				CHECK(a.allocations == 1 && a.allocated == 0, "%s: %d allocations", what, a.allocations);
				if (code == PrimErrNoMemory) {
					CHECK(a.code == 0, "%s: the failure code is %lld", what, (long long)a.code);
					expectInteger(what, &a, statusWord(SU_OVER_BUDGET, 0, spurNeed(calls[c].body)));
				} else {
					expectFailure(what, &a, PrimErrBadArgument);
				}
				/* and then it works */
				a = call(calls[c].p, calls[c].nargs, calls[c].args);
				CHECK(a.returned && !fake_is_int(a.value) && a.allocated == 1, "%s, then: %s", what, show(&a));
			}
		}
		fake_release(mark);
	}
	/* invalid input never allocates, even with no memory */
	{
		size_t mark = fake_mark();
		unsigned char bad[] = { 'a', 0xC3 };
		uint32_t surrogate[] = { 'a', 0xDC00 };
		FakeAnswer a;
		fake_set_budget(0, PrimErrNoMemory);
		a = call(pDecode, 7, (sqInt[]){ byteArray(bad, 2), I(1), I(2), I(SU_ENC_UTF8), fakeWideString, fakeNil, I(1) });
		expectInteger("decode of a cut sequence with no memory", &a, statusWord(SU_ERROR, SU_E_TOO_SHORT, 2));
		CHECK(a.allocations == 0, "decode of a cut sequence: %d allocations", a.allocations);
		a = call(pEncode, 6, (sqInt[]){ wideString(surrogate, 2), I(1), I(2), I(SU_ENC_UTF8), fakeNil, I(1) });
		expectInteger("encode of a surrogate with no memory", &a, statusWord(SU_ERROR, SU_E_SURROGATE, 2));
		CHECK(a.allocations == 0, "encode of a surrogate: %d allocations", a.allocations);
		fake_set_budget(-1, PrimErrNoMemory);
		fake_release(mark);
	}
}

int
main(int argc, char **argv)
{
	struct VirtualMachine *vm;
	long rounds = 200000;
	int i;

	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--seed") == 0 && i + 1 < argc) {
			seed = strtoull(argv[++i], NULL, 0) | 1;
		} else if (strcmp(argv[i], "--rounds") == 0 && i + 1 < argc) {
			rounds = strtol(argv[++i], NULL, 0);
		} else {
			fprintf(stderr, "usage: gluetest [--seed N] [--rounds N]\n");
			return 2;
		}
	}
	vm = fake_init();
	checkModule(vm);
	checkInfo();
	checkArity();
	checkOrders();
	checkFlags();
	checkAnswers();
	checkRandom(rounds);
	checkBudget();
	fake_free_all();
	if (failures) {
		printf("U10 FAILED: %ld checks, %ld failures\n", checks, failures);
		return 1;
	}
	printf("U10 ok: %ld checks, 0 failures\n", checks);
	return 0;
}
