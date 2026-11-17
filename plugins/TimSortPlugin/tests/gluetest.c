/* gluetest.c -- the glue TimSortPlugin.c against the fake VM of fakevm.c,
 * linked with the engine (tsEngine.c), the resumable sort (src/coro) and
 * timsort.c.
 *
 * Built twice by run-c-tests.sh: with the glue as the shared library
 * compiles it (its primitives and *AccessorDepth variables are globals),
 * and with -DSQUEAK_BUILTIN_PLUGIN (GLUE_BUILTIN here), where the
 * primitives are static and found by name in TimSortPlugin_exports, as the
 * Emscripten VM finds them.  The engine's own order is checked by
 * enginetest.c: here the references are the engine's comparisons applied
 * to values read from the fake objects directly (not through the proxy),
 * with a stable insertion sort of their own.  The checks:
 *
 *   G1  module    setInterpreter, the module name and functions, the
 *                 AccessorDepths (1 for primitiveSortObjects and
 *                 primitiveArgSort, 0 for the others), the rows of the
 *                 builtin table
 *   G2  info      primitiveInfo: the 6 slots, and PrimErrNoMemory (the
 *                 VM's retry) when any of its 4 allocations fails
 *   G3  arity     every primitive with one argument less and one more
 *   G4  order     the failures of the contract's checks: each alone, then
 *                 every pair of them on different arguments, where the
 *                 earlier check must win; nothing allocated, nothing stored,
 *                 every argument untouched
 *   G5  objects   primitiveSortObjects on random Arrays of SmallIntegers
 *                 (the raw path), Floats (immediate and boxed, -0.0,
 *                 infinities), mixed numbers with LargeIntegers, and the
 *                 four string classes (wide units above 2^31, NULs), random
 *                 ranges, both directions: the slots equal the reference's
 *                 oop for oop, the others untouched; every element that
 *                 makes it decline (NaN, Character, nil, Fraction, a String
 *                 subclass, a forwarder, LargeIntegers that are not
 *                 normalized, mixed families, a CompiledMethod or a 16-bit
 *                 object of a whitelisted class) in the range or out of it;
 *                 Array subclasses, WeakArray, read-only, n < 2
 *   G6  buffers   primitiveSortBuffer: every kind on the classes of its
 *                 format against ts_sort_buffer on a copy of the range, the
 *                 rest untouched; every class against every kind that is
 *                 not its own, CompiledMethod, BoxedFloat64, LargeIntegers,
 *                 a forwarder
 *   G7  argsort   primitiveArgSort with Array keys of each family and with
 *                 buffer keys of each kind, against the reference; keys
 *                 untouched; perm being keys
 *   G8  coroutine the resumable sort: where it works (TS_CORO_SUPPORTED and
 *                 ts_coro_available()), sorts stepped to the end against the
 *                 stable reference (both directions, n up to 5,000, two
 *                 interleaved), stale ids (PrimErrNotFound, a second free
 *                 answers false, ids never reused), the table full
 *                 (PrimErrInappropriate) and freed, n = 2^28 (the largest:
 *                 PrimErrNoCMemory under AddressSanitizer, not
 *                 PrimErrBadArgument), ids from a base in [2^32, 2^58),
 *                 abort, free while a
 *                 request is pending, the permutation before the end, of the
 *                 wrong size, read-only, the live count of primitiveInfo,
 *                 shutdownModule; elsewhere, PrimErrUnsupported after the
 *                 type checks
 *
 * Every call is also checked by the fake VM: no answer after a failure, no
 * failure after an answer, no store into an object the call did not make,
 * none of the proxy's eden-only allocators, no read of a forwarder's body.
 *
 * Usage: gluetest [--seed N] [--rounds N] [--slow]   (3,000 random calls
 * per family by default, --slow 30,000.)  Prints one line per failed
 * check (the first 50) and "gluetest ok: N checks, 0 failures"; exits 1
 * when a check failed.
 */

#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fakevm.h"
#include "tsEngine.h"
#include "tsCoro.h"

typedef sqInt (*Primitive)(void);

/* Under AddressSanitizer an allocation above 1 GiB answers NULL, so that G8
   checks the largest resumable sort (2^28 elements, a permutation of 2 GiB)
   without the memory: PrimErrNoCMemory, after the one line "WARNING:
   AddressSanitizer failed to allocate 0x80000000 bytes" (the environment's
   ASAN_OPTIONS come after these) */
const char *__asan_default_options(void);
const char *
__asan_default_options(void)
{
	return "allocator_may_return_null=1:max_allocation_size_mb=1024";
}

#ifdef GLUE_BUILTIN
extern void *TimSortPlugin_exports[][3];
#else
extern const char *getModuleName(void);
extern sqInt initialiseModule(void);
extern sqInt shutdownModule(void);
extern sqInt setInterpreter(struct VirtualMachine *);
extern sqInt primitiveArgSort(void);
extern sqInt primitiveBlockSortFree(void);
extern sqInt primitiveBlockSortNew(void);
extern sqInt primitiveBlockSortNext(void);
extern sqInt primitiveBlockSortPermutation(void);
extern sqInt primitiveInfo(void);
extern sqInt primitiveSortBuffer(void);
extern sqInt primitiveSortObjects(void);
extern signed char primitiveArgSortAccessorDepth, primitiveBlockSortFreeAccessorDepth,
	primitiveBlockSortNewAccessorDepth, primitiveBlockSortNextAccessorDepth,
	primitiveBlockSortPermutationAccessorDepth, primitiveInfoAccessorDepth,
	primitiveSortBufferAccessorDepth, primitiveSortObjectsAccessorDepth;
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

/* xorshift64*, seeded */
static uint64_t seed = 0x54536c7565540a01ull;

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

static void *
xmalloc(size_t size)
{
	void *p = malloc(size ? size : 1);

	if (!p) {
		fprintf(stderr, "gluetest: out of memory\n");
		exit(2);
	}
	return p;
}


/*** The glue ***/

static Primitive pInfo, pSortObjects, pSortBuffer, pArgSort, pNew, pNext, pPermutation, pFree;
static sqInt (*shutdown)(void);

#ifdef GLUE_BUILTIN
static void *
entry(const char *name)
{
	int i;

	for (i = 0; TimSortPlugin_exports[i][1]; i++) {
		if (strcmp(TimSortPlugin_exports[i][1], name) == 0) {
			return TimSortPlugin_exports[i][2];
		}
	}
	return NULL;
}
#endif

static void
checkModule(struct VirtualMachine *vm)
{
#ifdef GLUE_BUILTIN
	/* the rows, with the accessor depth after the name's NUL */
	static const char *names[] = { "getModuleName", "initialiseModule", "primitiveArgSort",
		"primitiveBlockSortFree", "primitiveBlockSortNew", "primitiveBlockSortNext",
		"primitiveBlockSortPermutation", "primitiveInfo", "primitiveSortBuffer", "primitiveSortObjects",
		"setInterpreter", "shutdownModule" };
	static const int depths[] = { -1, -1, 1, 0, 0, 0, 0, 0, 0, 1, -1, -1 };
	const char *(*moduleName)(void) = (const char *(*)(void))entry("getModuleName");
	sqInt (*init)(void) = (sqInt (*)(void))entry("initialiseModule");
	sqInt (*set)(struct VirtualMachine *) = (sqInt (*)(struct VirtualMachine *))entry("setInterpreter");
	const char *expected = "TimSortPlugin (i)";
	size_t i, rows = 0;

	shutdown = (sqInt (*)(void))entry("shutdownModule");
	for (i = 0; TimSortPlugin_exports[i][1]; i++) {
		const char *name = TimSortPlugin_exports[i][1];
		rows++;
		CHECK(strcmp(TimSortPlugin_exports[i][0], "TimSortPlugin") == 0, "exports row %zu: module %s",
			i, (const char *)TimSortPlugin_exports[i][0]);
		CHECK(i < sizeof names / sizeof names[0] && strcmp(name, names[i]) == 0 && TimSortPlugin_exports[i][2],
			"exports row %zu: %s", i, name);
		if (i < sizeof names / sizeof names[0] && depths[i] >= 0) {
			CHECK(name[strlen(name) + 1] == depths[i], "exports row %s: depth %d", name, name[strlen(name) + 1]);
		}
	}
	CHECK(rows == sizeof names / sizeof names[0], "exports: %zu rows", rows);
	CHECK(TimSortPlugin_exports[rows][0] == NULL && TimSortPlugin_exports[rows][2] == NULL, "exports: no NULL row");
	pInfo = (Primitive)entry("primitiveInfo");
	pSortObjects = (Primitive)entry("primitiveSortObjects");
	pSortBuffer = (Primitive)entry("primitiveSortBuffer");
	pArgSort = (Primitive)entry("primitiveArgSort");
	pNew = (Primitive)entry("primitiveBlockSortNew");
	pNext = (Primitive)entry("primitiveBlockSortNext");
	pPermutation = (Primitive)entry("primitiveBlockSortPermutation");
	pFree = (Primitive)entry("primitiveBlockSortFree");
	if (!(moduleName && init && shutdown && set && pInfo && pSortObjects && pSortBuffer && pArgSort
	      && pNew && pNext && pPermutation && pFree)) {
		printf("G1 FAILED: an entry of TimSortPlugin_exports is missing\n");
		exit(1);
	}
#else
	const char *(*moduleName)(void) = getModuleName;
	sqInt (*init)(void) = initialiseModule;
	sqInt (*set)(struct VirtualMachine *) = setInterpreter;
	const char *expected = "TimSortPlugin (e)";

	shutdown = shutdownModule;
	pInfo = primitiveInfo;
	pSortObjects = primitiveSortObjects;
	pSortBuffer = primitiveSortBuffer;
	pArgSort = primitiveArgSort;
	pNew = primitiveBlockSortNew;
	pNext = primitiveBlockSortNext;
	pPermutation = primitiveBlockSortPermutation;
	pFree = primitiveBlockSortFree;
	CHECK(primitiveSortObjectsAccessorDepth == 1 && primitiveArgSortAccessorDepth == 1,
		"the AccessorDepths of the sorts of objects are not 1");
	CHECK(primitiveBlockSortFreeAccessorDepth == 0 && primitiveBlockSortNewAccessorDepth == 0
		&& primitiveBlockSortNextAccessorDepth == 0 && primitiveBlockSortPermutationAccessorDepth == 0
		&& primitiveInfoAccessorDepth == 0 && primitiveSortBufferAccessorDepth == 0, "an AccessorDepth is not 0");
#endif
	section = "G1";
	CHECK(set(vm) == 1, "setInterpreter refused the proxy");
	CHECK(strcmp(moduleName(), expected) == 0, "getModuleName: %s", moduleName());
	CHECK(init() == 1, "initialiseModule");
}


/*** Calls and answers ***/

static sqInt
I(sqInt value)
{
	return fake_int(value);
}

static const char *
show(const FakeAnswer *a)
{
	static char text[160];

	if (a->returned) {
		snprintf(text, sizeof text, "answered %s%lld", fake_is_int(a->value) ? "" : "an object ",
			(long long)(fake_is_int(a->value) ? fake_int_value(a->value) : 0));
	} else {
		snprintf(text, sizeof text, "failed with %lld", (long long)a->code);
	}
	return text;
}

/* Calls a primitive (the receiver: nil, as the class TimSortPrimitives
   would be) and checks the fake VM saw no protocol error */
static FakeAnswer
call(const char *what, Primitive p, int nargs, const sqInt *args)
{
	FakeAnswer a = fake_call(p, fakeNil, nargs, args);

	CHECK(a.protocolErrors == 0, "%s: protocol error: %s", what, fake_last_error());
	return a;
}

/* The bodies of the object arguments of a call, to tell that a call left
   them alone */
typedef struct {
	int count;
	sqInt oops[8];
	unsigned char *bytes[8];
	size_t sizes[8];
} Snapshot;

static void
take(Snapshot *s, int nargs, const sqInt *args)
{
	int k;

	s->count = nargs;
	for (k = 0; k < nargs; k++) {
		s->oops[k] = args[k];
		s->bytes[k] = NULL;
		s->sizes[k] = 0;
		if (fake_is_object(args[k]) && fake_body(args[k])) {
			s->sizes[k] = fake_body_bytes(args[k]);
			s->bytes[k] = xmalloc(s->sizes[k]);
			memcpy(s->bytes[k], fake_body(args[k]), s->sizes[k]);
		}
	}
}

/* Whether the arguments are as taken; frees the snapshot */
static int
unchanged(Snapshot *s)
{
	int k, same = 1;

	for (k = 0; k < s->count; k++) {
		if (s->bytes[k]) {
			same &= fake_body_bytes(s->oops[k]) == s->sizes[k]
				&& memcmp(fake_body(s->oops[k]), s->bytes[k], s->sizes[k]) == 0;
			free(s->bytes[k]);
		}
	}
	return same;
}

/* Calls a primitive that must fail with code, allocate and store nothing,
   and leave its arguments alone */
static void
expectFailure(const char *what, Primitive p, int nargs, const sqInt *args, sqInt code)
{
	Snapshot s;
	FakeAnswer a;

	take(&s, nargs, args);
	a = call(what, p, nargs, args);
	CHECK(!a.returned && a.code == code, "%s: %s, not failed with %lld", what, show(&a), (long long)code);
	CHECK(a.allocations == 0 && a.stores == 0, "%s: %d allocations, %d stores", what, a.allocations, a.stores);
	CHECK(unchanged(&s), "%s: an argument changed", what);
}

/* Calls a primitive that must answer value, allocating and storing
   nothing */
static FakeAnswer
expectAnswer(const char *what, Primitive p, int nargs, const sqInt *args, sqInt value)
{
	FakeAnswer a = call(what, p, nargs, args);

	CHECK(a.returned && a.code == 0 && a.value == value, "%s: %s", what, show(&a));
	CHECK(a.allocations == 0 && a.stores == 0, "%s: %d allocations, %d stores", what, a.allocations, a.stores);
	return a;
}


/*** Elements and references ***/

static sqInt stringClasses;	/* {ByteString. ByteSymbol. WideString. WideSymbol} */

static sqInt
newStringClasses(void)
{
	sqInt classes[4];

	classes[0] = fakeByteString;
	classes[1] = fakeByteSymbol;
	classes[2] = fakeWideString;
	classes[3] = fakeWideSymbol;
	return fake_array(fakeArray, classes, 4);
}

/* A Float as Spur stores it: immediate for 0 and finite magnitudes in
   [2^-126, 2^128), boxed otherwise */
static sqInt
newFloat(double d)
{
	double m = fabs(d);

	if (d == 0 || (m >= ldexp(1.0, -126) && m < ldexp(1.0, 128))) {
		return below(8) ? fake_small_float(d) : fake_boxed_float(d);	/* some boxed anyway */
	}
	return fake_boxed_float(d);
}

/* A normalized LargeInteger of random magnitude above 2^60 (or exactly
   one of the edges) */
static sqInt
newLarge(int negative)
{
	uint8_t digits[24];
	size_t size, k;

	switch (below(4)) {
	case 0:		/* 2^60 (positive) or 2^60 + 1 (negative): the first outside SmallInteger */
		memset(digits, 0, 8);
		digits[7] = 0x10;
		digits[0] = negative ? 1 : 0;
		return fake_large(digits, 8, negative);
	case 1:		/* 2^64 */
		memset(digits, 0, 9);
		digits[8] = 1;
		return fake_large(digits, 9, negative);
	default:
		size = 8 + below(14);
		for (k = 0; k < size; k++) {
			digits[k] = (uint8_t)next();
		}
		digits[size - 1] |= size == 8 ? 0x20 : 1;	/* above 2^61 when 8 bytes */
		return fake_large(digits, size, negative);
	}
}

static sqInt
newString(int wide, size_t size, sqInt cls)
{
	static const uint8_t byteUnits[] = { 0, 'A', 'a', 'b', 0x7F, 0xE9, 0xFF };
	static const uint32_t wideUnits[] = { 0, 'A', 'a', 'b', 0xFF, 0x3B1, 0x7FFFFFFF, 0x80000000u, 0xFFFFFFFFu };
	size_t k;
	sqInt oop = fake_new(cls, size);

	for (k = 0; k < size; k++) {
		if (wide) {
			((uint32_t *)fake_body(oop))[k] = wideUnits[below(sizeof wideUnits / sizeof wideUnits[0])];
		} else {
			((uint8_t *)fake_body(oop))[k] = byteUnits[below(sizeof byteUnits / sizeof byteUnits[0])];
		}
	}
	return oop;
}

enum { SMALLINTEGERS, FLOATS, NUMBERS, STRINGS, FAMILIES };
static const char *familyNames[] = { "SmallIntegers", "Floats", "numbers", "strings" };

static sqInt
newElement(int family)
{
	static const sqInt edges[] = { ((sqInt)1 << 60) - 1, -((sqInt)1 << 60), 0, -1, 1 };
	static const double reals[] = { 0.0, -0.0, 1.0, -1.5, 0.5, 1e300, -1e300, 1e-310, INFINITY, -INFINITY, 3.0 };

	switch (family) {
	case SMALLINTEGERS:
		return I(below(5) == 0 ? edges[below(5)] : (sqInt)below(20) - 10);
	case FLOATS:
		return newFloat(below(3) == 0 ? reals[below(sizeof reals / sizeof reals[0])] : (double)((int)below(20) - 10) / 4);
	case NUMBERS:
		switch (below(4)) {
		case 0: return newElement(SMALLINTEGERS);
		case 1: return newElement(FLOATS);
		case 2: return newLarge((int)below(2));
		default: return newFloat(ldexp((double)((int)below(9) - 4), 60 + (int)below(8)));
		}
	default: {
		int wide = (int)below(2);
		size_t size = below(5);
		return newString(wide, size, wide ? (below(2) ? fakeWideString : fakeWideSymbol)
			: (below(2) ? fakeByteString : fakeByteSymbol));
	}
	}
}

/* The values of elements, read from the fake objects */
static void
numberOf(sqInt oop, ts_number *x)
{
	memset(x, 0, sizeof *x);
	if (fake_is_int(oop)) {
		x->type = TS_NUMBER_INTEGER;
		x->integer = fake_int_value(oop);
	} else if (fake_is_float(oop)) {
		x->type = TS_NUMBER_FLOAT;
		x->real = fake_float_value(oop);
	} else {
		x->type = TS_NUMBER_LARGE;
		x->negative = fake_class_of(oop) == fakeLargeNegativeInteger;
		x->digits = fake_body(oop);
		x->size = fake_size(oop);
	}
}

static void
stringOf(sqInt oop, ts_string *s)
{
	s->wide = fake_format(oop) == 10;
	s->size = fake_size(oop);
	s->units = fake_body(oop);
}

/* a before b in the reference order of objects */
static int
referenceLess(sqInt a, sqInt b, int strings)
{
	if (strings) {
		ts_string x, y;
		stringOf(a, &x);
		stringOf(b, &y);
		return ts_string_compare(&x, &y) < 0;
	} else {
		ts_number x, y;
		numberOf(a, &x);
		numberOf(b, &y);
		return ts_number_compare(&x, &y) < 0;
	}
}

/* Sorts the n oops of keys stably (insertion), or the n 1-based indices of
   order by keys[index - 1] when order is not NULL */
static void
referenceSort(sqInt *keys, sqInt *order, size_t n, int descending, int strings)
{
	size_t i, j;

	for (i = 1; i < n; i++) {
		sqInt x = order ? order[i] : keys[i];
		for (j = i; j > 0; j--) {
			sqInt y = order ? order[j - 1] : keys[j - 1];
			sqInt kx = order ? keys[x - 1] : x, ky = order ? keys[y - 1] : y;
			if (!(descending ? referenceLess(ky, kx, strings) : referenceLess(kx, ky, strings))) {
				break;
			}
			if (order) {
				order[j] = y;
			} else {
				keys[j] = y;
			}
		}
		if (order) {
			order[j] = x;
		} else {
			keys[j] = x;
		}
	}
}


/*** G2: primitiveInfo ***/

static int coroutines;		/* primitiveInfo's slot 5 */

static int
isByteString(sqInt oop, const char *text)
{
	return fake_is_object(oop) && fake_class_of(oop) == fakeByteString && fake_size(oop) == strlen(text)
		&& memcmp(fake_body(oop), text, strlen(text)) == 0;
}

static sqInt
liveSorts(void)
{
	FakeAnswer a = call("info", pInfo, 0, NULL);

	return a.returned ? fake_int_value(fake_slot(a.value, 5)) : -1;
}

static void
checkInfo(void)
{
	size_t mark = fake_mark();
	FakeAnswer a;
	long k;

	section = "G2";
	a = call("info", pInfo, 0, NULL);
	CHECK(a.returned && fake_is_object(a.value) && fake_class_of(a.value) == fakeArray && fake_size(a.value) == 6,
		"info: %s", show(&a));
	if (a.returned) {
		sqInt slot5 = fake_slot(a.value, 4);
		CHECK(fake_slot(a.value, 0) == I(TS_PROTOCOL), "info: protocol");
		CHECK(isByteString(fake_slot(a.value, 1), "TimSortPlugin 1.0"), "info: the plugin's version");
		CHECK(isByteString(fake_slot(a.value, 2), "timsort.c " TS_TIMSORT_VERSION), "info: the version of timsort.c");
		CHECK(isByteString(fake_slot(a.value, 3), "native"), "info: the platform");
		CHECK(slot5 == fakeTrueObject || slot5 == fakeFalseObject, "info: slot 5 is not a Boolean");
		coroutines = slot5 == fakeTrueObject;
#if !TS_CORO_SUPPORTED
		CHECK(!coroutines, "info: a resumable sort without TS_CORO_SUPPORTED");
#else
		CHECK(coroutines == (ts_coro_available() != 0), "info: slot 5 is not ts_coro_available()");
#endif
		CHECK(fake_slot(a.value, 5) == I(0), "info: live sorts");
		CHECK(a.allocations == 4 && a.allocated == 4, "info: %d allocations", a.allocations);
	}
	/* each allocation refused in turn: PrimErrNoMemory, then the VM retries */
	for (k = 0; k < 4; k++) {
		fake_set_budget(k, PrimErrNoMemory);
		a = call("info", pInfo, 0, NULL);
		fake_set_budget(-1, PrimErrNoMemory);
		CHECK(!a.returned && a.code == PrimErrNoMemory, "info with %ld allocations: %s", k, show(&a));
	}
	fake_release(mark);
}


/*** G3: arity ***/

static void
checkArity(void)
{
	struct { const char *name; Primitive p; int nargs; } prims[] = {
		{ "info", pInfo, 0 }, { "sortObjects", pSortObjects, 5 }, { "sortBuffer", pSortBuffer, 5 },
		{ "argSort", pArgSort, 7 }, { "blockSortNew", pNew, 2 }, { "blockSortNext", pNext, 2 },
		{ "blockSortPermutation", pPermutation, 2 }, { "blockSortFree", pFree, 1 } };
	sqInt args[9];
	char what[80];
	size_t i;
	int k;

	section = "G3";
	for (k = 0; k < 9; k++) {
		args[k] = I(1);
	}
	for (i = 0; i < sizeof prims / sizeof prims[0]; i++) {
		if (prims[i].nargs > 0) {
			snprintf(what, sizeof what, "%s with %d arguments", prims[i].name, prims[i].nargs - 1);
			expectFailure(what, prims[i].p, prims[i].nargs - 1, args, PrimErrBadNumArgs);
		}
		snprintf(what, sizeof what, "%s with %d arguments", prims[i].name, prims[i].nargs + 1);
		expectFailure(what, prims[i].p, prims[i].nargs + 1, args, PrimErrBadNumArgs);
	}
}


/*** G4: the order of the checks ***/

typedef struct {
	const char *name;
	int arg;		/* the argument it replaces */
	sqInt value;
	int step;		/* the contract's check: the earlier wins */
	sqInt code;
} Fault;

/* Each fault alone, then each pair on different arguments */
static void
checkFaults(const char *prim, Primitive p, int nargs, const sqInt *valid, const Fault *faults, size_t count)
{
	sqInt args[8];
	char what[200];
	size_t i, j;

	for (i = 0; i < count; i++) {
		memcpy(args, valid, (size_t)nargs * sizeof *args);
		args[faults[i].arg] = faults[i].value;
		snprintf(what, sizeof what, "%s: %s", prim, faults[i].name);
		expectFailure(what, p, nargs, args, faults[i].code);
		for (j = 0; j < count; j++) {
			const Fault *first = faults[i].step <= faults[j].step ? &faults[i] : &faults[j];
			if (i == j || faults[i].arg == faults[j].arg) {
				continue;
			}
			memcpy(args, valid, (size_t)nargs * sizeof *args);
			args[faults[i].arg] = faults[i].value;
			args[faults[j].arg] = faults[j].value;
			snprintf(what, sizeof what, "%s: %s and %s", prim, faults[i].name, faults[j].name);
			expectFailure(what, p, nargs, args, first->code);
		}
	}
}

/* An Array of n SmallIntegers n .. 1 */
static sqInt
countdown(size_t n)
{
	sqInt oop = fake_new(fakeArray, n);
	size_t k;

	for (k = 0; k < n; k++) {
		fake_set_slot(oop, k, I((sqInt)(n - k)));
	}
	return oop;
}

static void
checkOrders(void)
{
	size_t mark = fake_mark();
	sqInt array = countdown(10), readOnly = countdown(10), withNil = countdown(10), forwarder = fake_forwarder();
	sqInt three = fake_array(fakeArray, (sqInt[]){ fakeByteString, fakeByteSymbol, fakeWideString }, 3);
	sqInt notClasses = fake_array(fakeArray, (sqInt[]){ fakeByteString, fakeByteSymbol, fakeWideString, I(3) }, 4);
	sqInt nilClass = fake_array(fakeArray, (sqInt[]){ fakeByteString, fakeNil, fakeWideString, fakeWideSymbol }, 4);
	sqInt weak = fake_new(fakeWeakArray, 10), byteArray = fake_new(fakeByteArray, 10);
	sqInt float64 = fake_new(fakeFloat64Array, 10), readOnly64 = fake_new(fakeFloat64Array, 10);
	sqInt keys = countdown(10), perm = fake_new(fakeArray, 8), readOnlyPerm = fake_new(fakeArray, 8);
	sqInt perm7 = fake_new(fakeArray, 7), large = fake_large((const uint8_t *)"\1\0\0\0\0\0\0\0\1", 9, 0);
	sqInt boxed = fake_boxed_float(1.0), method = fake_compiled_method(16), doubleBytes = fake_new(fakeDoubleByteArray, 10);
	size_t k;

	section = "G4";
	fake_set_immutable(readOnly, 1);
	fake_set_immutable(readOnly64, 1);
	fake_set_immutable(readOnlyPerm, 1);
	fake_set_slot(withNil, 4, fakeNil);
	for (k = 0; k < 10; k++) {
		double d = (double)(10 - k);
		memcpy((char *)fake_body(float64) + 8 * k, &d, 8);
	}
	{
		sqInt valid[] = { array, I(2), I(9), fakeFalseObject, stringClasses };
		Fault faults[] = {
			{ "nil array", 0, fakeNil, 1, PrimErrBadArgument },
			{ "SmallInteger array", 0, I(3), 1, PrimErrBadArgument },
			{ "WeakArray", 0, weak, 1, PrimErrBadArgument },
			{ "ByteArray", 0, byteArray, 1, PrimErrBadArgument },
			{ "forwarded array", 0, forwarder, 1, PrimErrBadArgument },
			{ "nil start", 1, fakeNil, 2, PrimErrBadArgument },
			{ "Float start", 1, fake_small_float(2.0), 2, PrimErrBadArgument },
			{ "LargeInteger stop", 2, large, 2, PrimErrBadArgument },
			{ "nil descending", 3, fakeNil, 2, PrimErrBadArgument },
			{ "1 as descending", 3, I(1), 2, PrimErrBadArgument },
			{ "nil classes", 4, fakeNil, 2, PrimErrBadArgument },
			{ "3 classes", 4, three, 2, PrimErrBadArgument },
			{ "a class that is 3", 4, notClasses, 2, PrimErrBadArgument },
			{ "a class that is nil", 4, nilClass, 2, PrimErrBadArgument },
			{ "read-only", 0, readOnly, 3, PrimErrNoModification },
			{ "start 0", 1, I(0), 4, PrimErrBadIndex },
			{ "stop 11", 2, I(11), 4, PrimErrBadIndex },
			{ "start 11", 1, I(11), 4, PrimErrBadIndex },
			{ "nil in the range", 0, withNil, 5, PrimErrBadArgument },
		};
		expectAnswer("sortObjects valid", pSortObjects, 5, valid, array);
		checkFaults("sortObjects", pSortObjects, 5, valid, faults, sizeof faults / sizeof faults[0]);
	}
	{
		sqInt valid[] = { float64, I(TS_KIND_F64), I(2), I(9), fakeFalseObject };
		Fault faults[] = {
			{ "kind nil", 1, fakeNil, 1, PrimErrBadArgument },
			{ "kind 0", 1, I(0), 1, PrimErrBadArgument },
			{ "kind 9", 1, I(9), 1, PrimErrBadArgument },
			{ "kind f32", 1, I(TS_KIND_F32), 1, PrimErrBadArgument },
			{ "kind u8", 1, I(TS_KIND_U8), 1, PrimErrBadArgument },
			{ "an Array", 0, array, 1, PrimErrBadArgument },
			{ "a DoubleByteArray", 0, doubleBytes, 1, PrimErrBadArgument },
			{ "a CompiledMethod", 0, method, 1, PrimErrBadArgument },
			{ "a BoxedFloat64", 0, boxed, 1, PrimErrBadArgument },
			{ "a forwarder", 0, forwarder, 1, PrimErrBadArgument },
			{ "a SmallInteger", 0, I(3), 1, PrimErrBadArgument },
			{ "nil start", 2, fakeNil, 1, PrimErrBadArgument },
			{ "nil stop", 3, fakeNil, 1, PrimErrBadArgument },
			{ "nil descending", 4, fakeNil, 1, PrimErrBadArgument },
			{ "read-only", 0, readOnly64, 2, PrimErrNoModification },
			{ "start 0", 2, I(0), 3, PrimErrBadIndex },
			{ "stop 11", 3, I(11), 3, PrimErrBadIndex },
			{ "start 11", 2, I(11), 3, PrimErrBadIndex },
		};
		expectAnswer("sortBuffer valid", pSortBuffer, 5, valid, float64);
		checkFaults("sortBuffer", pSortBuffer, 5, valid, faults, sizeof faults / sizeof faults[0]);
	}
	{
		sqInt valid[] = { keys, fakeNil, perm, I(2), I(9), fakeTrueObject, stringClasses };
		Fault faults[] = {
			{ "ByteArray keys", 0, byteArray, 2, PrimErrBadArgument },
			{ "forwarded keys", 0, forwarder, 2, PrimErrBadArgument },
			{ "kind 0", 1, I(0), 2, PrimErrBadArgument },
			{ "kind 9", 1, I(9), 2, PrimErrBadArgument },
			{ "kind f64 for an Array", 1, I(TS_KIND_F64), 2, PrimErrBadArgument },
			{ "kind true", 1, fakeTrueObject, 2, PrimErrBadArgument },
			{ "nil perm", 2, fakeNil, 3, PrimErrBadArgument },
			{ "ByteArray perm", 2, byteArray, 3, PrimErrBadArgument },
			{ "WeakArray perm", 2, weak, 3, PrimErrBadArgument },
			{ "nil start", 3, fakeNil, 3, PrimErrBadArgument },
			{ "nil stop", 4, fakeNil, 3, PrimErrBadArgument },
			{ "nil descending", 5, fakeNil, 3, PrimErrBadArgument },
			{ "nil classes", 6, fakeNil, 3, PrimErrBadArgument },
			{ "3 classes", 6, three, 3, PrimErrBadArgument },
			{ "read-only perm", 2, readOnlyPerm, 4, PrimErrNoModification },
			{ "start 0", 3, I(0), 5, PrimErrBadIndex },
			{ "stop 11", 4, I(11), 5, PrimErrBadIndex },
			{ "perm of 7", 2, perm7, 6, PrimErrBadArgument },
			{ "nil in the keys", 0, withNil, 7, PrimErrBadArgument },
		};
		expectAnswer("argSort valid", pArgSort, 7, valid, perm);
		checkFaults("argSort", pArgSort, 7, valid, faults, sizeof faults / sizeof faults[0]);
	}
	{
		/* buffer keys: the kind decides */
		sqInt valid[] = { float64, I(TS_KIND_F64), perm, I(2), I(9), fakeFalseObject, stringClasses };
		Fault faults[] = {
			{ "kind nil for a buffer", 1, fakeNil, 2, PrimErrBadArgument },
			{ "kind f32", 1, I(TS_KIND_F32), 2, PrimErrBadArgument },
			{ "ByteArray keys", 0, byteArray, 2, PrimErrBadArgument },
			{ "nil perm", 2, fakeNil, 3, PrimErrBadArgument },
			{ "read-only perm", 2, readOnlyPerm, 4, PrimErrNoModification },
			{ "stop 11", 4, I(11), 5, PrimErrBadIndex },
			{ "perm of 7", 2, perm7, 6, PrimErrBadArgument },
		};
		expectAnswer("argSort of a buffer valid", pArgSort, 7, valid, perm);
		checkFaults("argSort of a buffer", pArgSort, 7, valid, faults, sizeof faults / sizeof faults[0]);
	}
	/* perm being the keys */
	{
		sqInt both = countdown(8);
		expectFailure("argSort into its keys", pArgSort, 7,
			(sqInt[]){ both, fakeNil, both, I(1), I(8), fakeFalseObject, stringClasses }, PrimErrBadArgument);
	}
	fake_release(mark);
}


/*** G5: primitiveSortObjects ***/

static void
checkSortObjectsOnce(int family, size_t n, sqInt cls)
{
	sqInt array = fake_new(cls, n), *original = xmalloc(n * sizeof(sqInt)), *expected = xmalloc(n * sizeof(sqInt));
	size_t k, start = 1 + below(n + 1), stop;
	int descending = (int)below(2);
	char what[160];

	stop = start - 1 + below(n - start + 2);	/* start - 1 .. n */
	for (k = 0; k < n; k++) {
		fake_set_slot(array, k, newElement(family));
	}
	memcpy(original, fake_body(array), n * sizeof(sqInt));
	memcpy(expected, original, n * sizeof(sqInt));
	referenceSort(expected + start - 1, NULL, stop + 1 - start, descending, family == STRINGS);
	snprintf(what, sizeof what, "sortObjects of %zu %s, %zu to %zu%s", n, familyNames[family], start, stop,
		descending ? " descending" : "");
	expectAnswer(what, pSortObjects, 5, (sqInt[]){ array, I((sqInt)start), I((sqInt)stop),
		descending ? fakeTrueObject : fakeFalseObject, stringClasses }, array);
	CHECK(n == 0 || memcmp(fake_body(array), expected, n * sizeof(sqInt)) == 0, "%s: not the reference's order", what);
	free(original);
	free(expected);
}

/* The elements that make primitiveSortObjects decline, for a family */
static sqInt
newDecliner(int family, int which, const char **name)
{
	static uint8_t five[8] = { 5 }, maxVal[8] = { 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F };
	static uint8_t minVal[8] = { 0, 0, 0, 0, 0, 0, 0, 0x10 }, highZero[10] = { 1, 2, 3, 4, 5, 6, 7, 8, 9, 0 };

	switch (which) {
	case 0: *name = "a NaN"; return fake_boxed_float(NAN);
	case 1: *name = "a Character"; return fake_character('a');
	case 2: *name = "nil"; return fakeNil;
	case 3: *name = "a Fraction"; return fake_new(fakeFraction, 0);
	case 4: *name = "a String subclass"; return fake_bytes(fakeMyString, "abc", 3);
	case 5: *name = "a forwarder"; return fake_forwarder();
	case 6: *name = "5 in 8 bytes"; return fake_large(five, 8, 0);
	case 7: *name = "SmallInteger maxVal as a LargePositiveInteger"; return fake_large(maxVal, 8, 0);
	case 8: *name = "SmallInteger minVal as a LargeNegativeInteger"; return fake_large(minVal, 8, 1);
	case 9: *name = "a high zero byte"; return fake_large(highZero, 10, (int)below(2));
	case 10: *name = "a LargeInteger of no bytes"; return fake_large(five, 0, 0);
	case 11: *name = "an Array"; return fake_new(fakeArray, 2);
	case 12: *name = "a ByteArray"; return fake_bytes(fakeByteArray, "abc", 3);
	case 13: *name = "true"; return fakeTrueObject;
	default:
		*name = "the other family";
		return newElement(family == STRINGS ? (int)below(3) : STRINGS);
	}
}
#define DECLINERS 15

static void
checkDeclines(void)
{
	size_t mark = fake_mark();
	char what[200];
	int family, which;

	for (family = 0; family < FAMILIES; family++) {
		for (which = 0; which < DECLINERS; which++) {
			size_t n = 2 + below(40), k, at = below(n);
			sqInt array = fake_new(fakeArray, n), *slots, *copy;
			const char *name;
			for (k = 0; k < n; k++) {
				fake_set_slot(array, k, newElement(family));
			}
			fake_set_slot(array, at, newDecliner(family, which, &name));
			snprintf(what, sizeof what, "sortObjects of %s with %s", familyNames[family], name);
			expectFailure(what, pSortObjects, 5, (sqInt[]){ array, I(1), I((sqInt)n), fakeFalseObject,
				stringClasses }, PrimErrBadArgument);
			/* out of the range, it is never looked at */
			slots = fake_body(array);
			copy = xmalloc(n * sizeof *copy);
			memcpy(copy, slots, n * sizeof *copy);
			if (at > 1) {
				snprintf(what, sizeof what, "sortObjects of %s with %s after the range", familyNames[family], name);
				expectAnswer(what, pSortObjects, 5, (sqInt[]){ array, I(1), I((sqInt)at), fakeTrueObject,
					stringClasses }, array);
				CHECK(slots[at] == copy[at], "%s: the element after the range moved", what);
				for (k = at + 1; k < n; k++) {
					CHECK(slots[k] == copy[k], "%s: an element after the range moved", what);
				}
			}
			free(copy);
		}
	}
	/* whitelisted classes of other shapes */
	{
		sqInt odd = fake_array(fakeArray, (sqInt[]){ fakeByteString, fakeCompiledMethod, fakeDoubleByteArray,
			fakeWideSymbol }, 4);
		sqInt array = fake_array(fakeArray, (sqInt[]){ fake_bytes(fakeByteString, "b", 1),
			fake_compiled_method(8) }, 2);
		expectFailure("sortObjects of a whitelisted CompiledMethod", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(2), fakeFalseObject, odd }, PrimErrBadArgument);
		array = fake_array(fakeArray, (sqInt[]){ fake_bytes(fakeByteString, "b", 1),
			fake_new(fakeDoubleByteArray, 2) }, 2);
		expectFailure("sortObjects of a whitelisted 16-bit object", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(2), fakeFalseObject, odd }, PrimErrBadArgument);
	}
	/* n < 2: nothing is looked at */
	{
		sqInt array = fake_array(fakeArray, (sqInt[]){ fakeNil, fake_character('x'), fake_forwarder() }, 3);
		expectAnswer("sortObjects of {nil}", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(1), fakeFalseObject, stringClasses }, array);
		expectAnswer("sortObjects of nothing", pSortObjects, 5,
			(sqInt[]){ array, I(4), I(3), fakeFalseObject, stringClasses }, array);
		expectAnswer("sortObjects of nothing at the start", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(0), fakeTrueObject, stringClasses }, array);
		expectFailure("sortObjects of {nil. $x}", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(2), fakeFalseObject, stringClasses }, PrimErrBadArgument);
	}
	/* read-only, even when there is nothing to sort */
	{
		sqInt array = countdown(3);
		fake_set_immutable(array, 1);
		expectFailure("sortObjects of a read-only Array", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(0), fakeFalseObject, stringClasses }, PrimErrNoModification);
	}
	fake_release(mark);
}

static void
checkSortObjects(long rounds)
{
	static const size_t big[] = { 255, 256, 257, 600, 2000 };
	long r;
	int family;
	size_t i;

	section = "G5";
	for (family = 0; family < FAMILIES; family++) {
		size_t mark = fake_mark();
		for (r = 0; r < rounds; r++) {
			checkSortObjectsOnce(family, below(4) ? below(12) : below(70), below(10) ? fakeArray : fakeArraySubclass);
			if (r % 1000 == 999) {
				fake_release(mark);
			}
		}
		for (i = 0; i < sizeof big / sizeof big[0]; i++) {
			checkSortObjectsOnce(family, big[i], fakeArray);
		}
		fake_release(mark);
	}
	/* by hand: the exact order beyond 2^53, which the JIT gets wrong */
	{
		size_t mark = fake_mark();
		uint8_t two60[8] = { 0, 0, 0, 0, 0, 0, 0, 0x10 };
		sqInt maxVal = I(((sqInt)1 << 60) - 1), f = fake_boxed_float(ldexp(1.0, 60)), large = fake_large(two60, 8, 0);
		sqInt array = fake_array(fakeArray, (sqInt[]){ large, f, maxVal }, 3);
		expectAnswer("sortObjects of {2^60. 2^60 asFloat. 2^60-1}", pSortObjects, 5,
			(sqInt[]){ array, I(1), I(3), fakeFalseObject, stringClasses }, array);
		CHECK(fake_slot(array, 0) == maxVal && fake_slot(array, 1) == large && fake_slot(array, 2) == f,
			"{2^60. 2^60 asFloat. 2^60-1}: not {2^60-1. 2^60. 2^60 asFloat}");
		fake_release(mark);
	}
	checkDeclines();
}


/*** G6: primitiveSortBuffer ***/

static const sqInt *bufferClasses[] = { &fakeFloat64Array, &fakeDoubleWordArray, &fakeFloat32Array,
	&fakeIntegerArray, &fakeWordArray, &fakeBitmap, &fakeDoubleByteArray, &fakeByteArray, &fakeByteString };
#define BUFFER_CLASSES (sizeof bufferClasses / sizeof bufferClasses[0])

/* The instance specification of the class of a buffer of kind */
static int
specOfKind(int kind)
{
	return TS_KIND_SIZE(kind) == 8 ? 9 : TS_KIND_SIZE(kind) == 4 ? 10 : TS_KIND_SIZE(kind) == 2 ? 12 : 16;
}

static int
specOfClass(sqInt cls)
{
	return (int)((fake_int_value(fake_slot(cls, 2)) >> 16) & 0x1F);
}

static void
fillRandom(void *base, size_t bytes)
{
	size_t k;

	for (k = 0; k < bytes; k++) {
		((uint8_t *)base)[k] = below(3) ? (uint8_t)below(3) : (uint8_t)next();
	}
}

static void
checkSortBuffers(long rounds)
{
	size_t mark = fake_mark();
	char what[160];
	long r;
	int kind;
	size_t c;

	section = "G6";
	for (r = 0; r < rounds; r++) {
		size_t n = below(4) ? below(20) : below(2000), start = 1 + below(n + 1), stop, size;
		sqInt cls, buffer;
		unsigned char *expected;
		int descending = (int)below(2);
		kind = TS_KIND_FIRST + (int)below(TS_KIND_LAST);
		size = TS_KIND_SIZE(kind);
		do {
			cls = *bufferClasses[below(BUFFER_CLASSES)];
		} while (specOfClass(cls) != specOfKind(kind));
		stop = start - 1 + below(n - start + 2);
		buffer = fake_new(cls, n);
		fillRandom(fake_body(buffer), n * size);
		expected = xmalloc(n * size);
		memcpy(expected, fake_body(buffer), n * size);
		CHECK(ts_sort_buffer(expected + (start - 1) * size, (int64_t)(stop + 1 - start), kind, descending) == TS_OK,
			"ts_sort_buffer");
		snprintf(what, sizeof what, "sortBuffer of %zu of kind %d in a %s, %zu to %zu%s", n, kind,
			fake_class_name(cls), start, stop, descending ? " descending" : "");
		expectAnswer(what, pSortBuffer, 5, (sqInt[]){ buffer, I(kind), I((sqInt)start), I((sqInt)stop),
			descending ? fakeTrueObject : fakeFalseObject }, buffer);
		CHECK(n == 0 || memcmp(fake_body(buffer), expected, n * size) == 0, "%s: not the engine's order", what);
		free(expected);
		if (r % 1000 == 999) {
			fake_release(mark);
		}
	}
	/* every class against every kind */
	for (c = 0; c < BUFFER_CLASSES; c++) {
		sqInt cls = *bufferClasses[c], buffer = fake_new(cls, 6);
		fillRandom(fake_body(buffer), fake_body_bytes(buffer));
		for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
			sqInt args[] = { buffer, I(kind), I(1), I(6), fakeFalseObject };
			snprintf(what, sizeof what, "sortBuffer of a %s as kind %d", fake_class_name(cls), kind);
			if (specOfClass(cls) == specOfKind(kind)) {
				expectAnswer(what, pSortBuffer, 5, args, buffer);
			} else {
				expectFailure(what, pSortBuffer, 5, args, PrimErrBadArgument);
			}
		}
	}
	/* numbers and other objects are not buffers */
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		uint8_t digits[9] = { 1, 2, 3, 4, 5, 6, 7, 8, 9 };
		sqInt others[] = { fake_boxed_float(2.5), fake_large(digits, 9, 0), fake_large(digits, 9, 1),
			fake_compiled_method(16), fake_forwarder(), fake_new(fakeArray, 2), fake_new(fakeWeakArray, 2),
			fake_bytes(fakeMyString, "abcdef", 6), I(5), fake_character('a'), fake_small_float(1.0), fakeNil };
		for (c = 0; c < sizeof others / sizeof others[0]; c++) {
			sqInt args[] = { others[c], I(kind), I(1), I(1), fakeFalseObject };
			snprintf(what, sizeof what, "sortBuffer of a %s as kind %d", fake_class_name(fake_class_of(others[c])), kind);
			if (fake_class_of(others[c]) == fakeMyString && kind == TS_KIND_U8) {
				expectAnswer(what, pSortBuffer, 5, args, others[c]);	/* bytes are bytes */
			} else {
				expectFailure(what, pSortBuffer, 5, args, PrimErrBadArgument);
			}
		}
	}
	fake_release(mark);
}


/*** G7: primitiveArgSort ***/

static void
checkArgSortOnce(int family, int kind, size_t n)
{
	size_t start = 1 + below(n + 1), stop = start - 1 + below(n - start + 2), m = stop + 1 - start, k;
	sqInt keys, perm = fake_new(fakeArray, m), *order = xmalloc((m ? m : 1) * sizeof(sqInt));
	unsigned char *snapshot;
	size_t keyBytes;
	int descending = (int)below(2);
	char what[160];

	if (kind) {
		sqInt cls;
		do {
			cls = *bufferClasses[below(BUFFER_CLASSES)];
		} while (specOfClass(cls) != specOfKind(kind));
		keys = fake_new(cls, n);
		fillRandom(fake_body(keys), n * TS_KIND_SIZE(kind));
	} else {
		keys = fake_new(fakeArray, n);
		for (k = 0; k < n; k++) {
			fake_set_slot(keys, k, newElement(family));
		}
	}
	keyBytes = fake_body_bytes(keys);
	snapshot = xmalloc(keyBytes);
	memcpy(snapshot, fake_body(keys), keyBytes);
	for (k = 0; k < m; k++) {
		order[k] = (sqInt)(start + k);
		fake_set_slot(perm, k, below(2) ? fakeNil : fake_character('p'));	/* overwritten */
	}
	if (kind) {
		int64_t *indices = xmalloc((m ? m : 1) * sizeof *indices);
		for (k = 0; k < m; k++) {
			indices[k] = (int64_t)(start - 1 + k);
		}
		CHECK(ts_argsort_buffer(fake_body(keys), (int64_t)n, kind, indices, (int64_t)m, 0, 0, descending) == TS_OK,
			"ts_argsort_buffer");
		for (k = 0; k < m; k++) {
			order[k] = (sqInt)indices[k] + 1;
		}
		free(indices);
	} else {
		referenceSort(fake_body(keys), order, m, descending, family == STRINGS);
	}
	snprintf(what, sizeof what, "argSort of %zu %s, %zu to %zu%s", n, kind ? "buffer keys" : familyNames[family],
		start, stop, descending ? " descending" : "");
	expectAnswer(what, pArgSort, 7, (sqInt[]){ keys, kind ? I(kind) : fakeNil, perm, I((sqInt)start), I((sqInt)stop),
		descending ? fakeTrueObject : fakeFalseObject, stringClasses }, perm);
	for (k = 0; k < m; k++) {
		if (fake_slot(perm, k) != I(order[k])) {
			fail("%s: perm[%zu] is not %lld", what, k + 1, (long long)order[k]);
			break;
		}
	}
	checks++;
	CHECK(memcmp(fake_body(keys), snapshot, keyBytes) == 0, "%s: the keys changed", what);
	free(snapshot);
	free(order);
}

static void
checkArgSort(long rounds)
{
	long r;
	int family, kind;

	section = "G7";
	for (family = 0; family < FAMILIES; family++) {
		size_t mark = fake_mark();
		for (r = 0; r < rounds / 2; r++) {
			checkArgSortOnce(family, 0, below(4) ? below(12) : below(300));
			if (r % 500 == 499) {
				fake_release(mark);
			}
		}
		fake_release(mark);
	}
	for (kind = TS_KIND_FIRST; kind <= TS_KIND_LAST; kind++) {
		size_t mark = fake_mark();
		for (r = 0; r < rounds / 4; r++) {
			checkArgSortOnce(0, kind, below(4) ? below(12) : below(2000));
		}
		fake_release(mark);
	}
}


/*** G8: the resumable sort ***/

/* Steps the sort id of the keys (SmallIntegers, ascending order by <) to
   its end, answering each request as the image does for the block
   [:a :b | a <= b]: lt(i, j) = (keys[j] <= keys[i]) not; answers the last
   answer of primitiveBlockSortNext, and the requests in *requests */
static sqInt
stepSort(const char *what, sqInt id, const sqInt *keys, size_t n, long abortAt, long *requests)
{
	sqInt answer = 0;
	long count = 0;

	for (;;) {
		FakeAnswer a = call(what, pNext, 2, (sqInt[]){ I(id), I(answer) });
		sqInt r, i, j;
		if (!a.returned || !fake_is_int(a.value)) {
			fail("%s: next %s", what, show(&a));
			return -100;
		}
		r = fake_int_value(a.value);
		if (r < 0) {
			if (requests) {
				*requests = count;
			}
			return r;
		}
		i = r >> 32;
		j = r & 0xFFFFFFFF;
		if (i < 0 || (size_t)i >= n || j < 0 || (size_t)j >= n || i == j) {
			fail("%s: the request %lld asks for %lld and %lld of %zu", what, (long long)r, (long long)i, (long long)j, n);
			return -100;
		}
		count++;
		answer = abortAt && count == abortAt ? -1 : !(keys[j] <= keys[i]);
	}
}

static sqInt
newSort(const char *what, size_t n, int descending)
{
	FakeAnswer a = call(what, pNew, 2, (sqInt[]){ I((sqInt)n), descending ? fakeTrueObject : fakeFalseObject });

	CHECK(a.returned && fake_is_int(a.value) && fake_int_value(a.value) >= (sqInt)1 << 32
		&& fake_int_value(a.value) < (sqInt)1 << 59, "%s: new %s (ids from a base in [2^32, 2^58))", what, show(&a));
	return a.returned ? fake_int_value(a.value) : 0;
}

/* The stable order of the keys, as 1-based indices */
static void
referenceOrder(const sqInt *keys, size_t n, int descending, sqInt *order)
{
	size_t i, j;

	for (i = 0; i < n; i++) {
		sqInt x = (sqInt)i + 1;
		for (j = i; j > 0; j--) {
			sqInt kx = keys[x - 1], ky = keys[order[j - 1] - 1];
			if (!(descending ? ky < kx : kx < ky)) {
				break;
			}
			order[j] = order[j - 1];
		}
		order[j] = x;
	}
}

static void
checkResumableSort(size_t n, int descending)
{
	sqInt *keys = xmalloc((n ? n : 1) * sizeof *keys), *order = xmalloc((n ? n : 1) * sizeof *order);
	sqInt id, perm = fake_new(fakeArray, n), last;
	char what[120];
	long requests = 0;
	size_t k;

	for (k = 0; k < n; k++) {
		keys[k] = (sqInt)below(n / 3 + 1);
	}
	referenceOrder(keys, n, descending, order);
	snprintf(what, sizeof what, "blockSort of %zu%s", n, descending ? " descending" : "");
	id = newSort(what, n, descending);
	last = stepSort(what, id, keys, n, 0, &requests);
	CHECK(last == TS_DONE, "%s: ended with %lld", what, (long long)last);
	CHECK(n >= 2 || requests == 0, "%s: %ld requests", what, requests);
	expectAnswer(what, pPermutation, 2, (sqInt[]){ I(id), perm }, perm);
	for (k = 0; k < n; k++) {
		if (fake_slot(perm, k) != I(order[k])) {
			fail("%s: perm[%zu] is not %lld", what, k + 1, (long long)order[k]);
			break;
		}
	}
	checks++;
	/* finished: it keeps answering done */
	expectAnswer(what, pNext, 2, (sqInt[]){ I(id), I(1) }, I(TS_DONE));
	expectAnswer(what, pFree, 1, (sqInt[]){ I(id) }, fakeTrueObject);
	free(keys);
	free(order);
}

static void
checkCoroutines(void)
{
	size_t mark = fake_mark();
	sqInt perm = fake_new(fakeArray, 10), id, id2, ids[TS_CORO_TABLE_SIZE], first;
	sqInt keys[10] = { 5, 3, 9, 1, 3, 7, 0, 3, 8, 2 };
	sqInt big = ((sqInt)1 << 28) + 1;
	size_t k;

	section = "G8";
	/* the type checks come first, supported or not */
	expectFailure("new: nil n", pNew, 2, (sqInt[]){ fakeNil, fakeFalseObject }, PrimErrBadArgument);
	expectFailure("new: n -1", pNew, 2, (sqInt[]){ I(-1), fakeFalseObject }, PrimErrBadArgument);
	expectFailure("new: n 2^28 + 1", pNew, 2, (sqInt[]){ I(big), fakeFalseObject }, PrimErrBadArgument);
	expectFailure("new: nil descending", pNew, 2, (sqInt[]){ I(3), fakeNil }, PrimErrBadArgument);
	expectFailure("next: nil id", pNext, 2, (sqInt[]){ fakeNil, I(0) }, PrimErrBadArgument);
	expectFailure("next: nil answer", pNext, 2, (sqInt[]){ I(1), fakeNil }, PrimErrBadArgument);
	expectFailure("permutation: nil id", pPermutation, 2, (sqInt[]){ fakeNil, perm }, PrimErrBadArgument);
	expectFailure("permutation: a ByteArray", pPermutation, 2, (sqInt[]){ I(1), fake_new(fakeByteArray, 10) },
		PrimErrBadArgument);
	expectFailure("free: nil id", pFree, 1, (sqInt[]){ fakeNil }, PrimErrBadArgument);
	if (!coroutines) {
		expectFailure("new", pNew, 2, (sqInt[]){ I(3), fakeFalseObject }, PrimErrUnsupported);
		expectFailure("next", pNext, 2, (sqInt[]){ I(1), I(0) }, PrimErrUnsupported);
		expectFailure("permutation", pPermutation, 2, (sqInt[]){ I(1), perm }, PrimErrUnsupported);
		expectFailure("free", pFree, 1, (sqInt[]){ I(1) }, PrimErrUnsupported);
		printf("G8 coroutine: unsupported here (%s): PrimErrUnsupported after the type checks\n",
#if TS_CORO_SUPPORTED
			ts_coro_impl()
#else
			"no TS_CORO_SUPPORTED"
#endif
			);
		fake_release(mark);
		return;
	}
	/* sorts to the end */
	{
		static const size_t sizes[] = { 0, 1, 2, 3, 10, 64, 257, 1000, 5000 };
		for (k = 0; k < sizeof sizes / sizeof sizes[0]; k++) {
			checkResumableSort(sizes[k], 0);
			checkResumableSort(sizes[k], 1);
		}
	}
	CHECK(liveSorts() == 0, "live sorts after the sorts: %lld", (long long)liveSorts());
	/* n = 2^28, the largest: its permutation (2 GiB) is refused under
	   AddressSanitizer (__asan_default_options), which tells a refusal for
	   memory from one of the argument; without it, the sort opens */
	{
		FakeAnswer a = call("new: n 2^28", pNew, 2, (sqInt[]){ I(big - 1), fakeFalseObject });

		CHECK(a.returned ? fake_is_int(a.value) : a.code == PrimErrNoCMemory,
			"new: n 2^28 (the largest) %s, not a sort nor PrimErrNoCMemory", show(&a));
		if (a.returned) {
			expectAnswer("free of 2^28", pFree, 1, (sqInt[]){ a.value }, fakeTrueObject);
		}
	}
	/* stale ids, never reused */
	id = newSort("stale", 10, 0);
	expectAnswer("free", pFree, 1, (sqInt[]){ I(id) }, fakeTrueObject);
	expectAnswer("free again", pFree, 1, (sqInt[]){ I(id) }, fakeFalseObject);
	expectFailure("next of a stale id", pNext, 2, (sqInt[]){ I(id), I(0) }, PrimErrNotFound);
	expectFailure("permutation of a stale id", pPermutation, 2, (sqInt[]){ I(id), perm }, PrimErrNotFound);
	expectFailure("next of id 0", pNext, 2, (sqInt[]){ I(0), I(0) }, PrimErrNotFound);
	expectFailure("next of id -1", pNext, 2, (sqInt[]){ I(-1), I(0) }, PrimErrNotFound);
	expectAnswer("free of id 0", pFree, 1, (sqInt[]){ I(0) }, fakeFalseObject);
	id2 = newSort("after a free", 10, 0);
	CHECK(id2 > id, "ids reused: %lld after %lld", (long long)id2, (long long)id);
	/* the permutation before the end, and its checks in order */
	{
		sqInt readOnly = fake_new(fakeArray, 10), perm9 = fake_new(fakeArray, 9);
		FakeAnswer a;
		fake_set_immutable(readOnly, 1);
		expectFailure("permutation before the start", pPermutation, 2, (sqInt[]){ I(id2), perm }, PrimErrInappropriate);
		a = call("first request", pNext, 2, (sqInt[]){ I(id2), I(0) });
		CHECK(a.returned && fake_is_int(a.value) && fake_int_value(a.value) >= 0, "first request: %s", show(&a));
		expectFailure("permutation pending", pPermutation, 2, (sqInt[]){ I(id2), perm }, PrimErrInappropriate);
		expectFailure("permutation of 9 pending", pPermutation, 2, (sqInt[]){ I(id2), perm9 }, PrimErrBadArgument);
		expectFailure("permutation read-only pending", pPermutation, 2, (sqInt[]){ I(id2), readOnly },
			PrimErrNoModification);
		expectFailure("permutation of 9 of a stale id", pPermutation, 2, (sqInt[]){ I(id), perm9 }, PrimErrNotFound);
		CHECK(liveSorts() == 1, "live: %lld, not 1", (long long)liveSorts());
		/* free while a request is pending */
		expectAnswer("free pending", pFree, 1, (sqInt[]){ I(id2) }, fakeTrueObject);
		CHECK(liveSorts() == 0, "live after the free: %lld", (long long)liveSorts());
	}
	/* abort */
	{
		id = newSort("abort", 10, 0);
		CHECK(stepSort("abort", id, keys, 10, 3, NULL) == TS_ABORTED, "abort: not -3");
		expectAnswer("aborted, next", pNext, 2, (sqInt[]){ I(id), I(0) }, I(TS_ABORTED));
		expectFailure("aborted, permutation", pPermutation, 2, (sqInt[]){ I(id), perm }, PrimErrInappropriate);
		expectAnswer("aborted, free", pFree, 1, (sqInt[]){ I(id) }, fakeTrueObject);
	}
	/* two interleaved */
	{
		sqInt a1 = newSort("interleaved 1", 10, 0), a2 = newSort("interleaved 2", 10, 1), answer1 = 0, answer2 = 0;
		sqInt r1 = 0, r2 = 0, order[10], perm2 = fake_new(fakeArray, 10);
		while (r1 >= 0 || r2 >= 0) {
			if (r1 >= 0) {
				FakeAnswer a = call("interleaved 1", pNext, 2, (sqInt[]){ I(a1), I(answer1) });
				r1 = a.returned ? fake_int_value(a.value) : -100;
				if (r1 >= 0) {
					answer1 = !(keys[r1 & 0xFFFFFFFF] <= keys[r1 >> 32]);
				}
			}
			if (r2 >= 0) {
				FakeAnswer a = call("interleaved 2", pNext, 2, (sqInt[]){ I(a2), I(answer2) });
				r2 = a.returned ? fake_int_value(a.value) : -100;
				if (r2 >= 0) {
					/* descending: the same lt, timsort's reverse flag */
					answer2 = !(keys[r2 & 0xFFFFFFFF] <= keys[r2 >> 32]);
				}
			}
		}
		CHECK(r1 == TS_DONE && r2 == TS_DONE, "interleaved: %lld %lld", (long long)r1, (long long)r2);
		expectAnswer("interleaved 1", pPermutation, 2, (sqInt[]){ I(a1), perm }, perm);
		expectAnswer("interleaved 2", pPermutation, 2, (sqInt[]){ I(a2), perm2 }, perm2);
		referenceOrder(keys, 10, 0, order);
		for (k = 0; k < 10; k++) {
			CHECK(fake_slot(perm, k) == I(order[k]), "interleaved 1: perm[%zu]", k + 1);
		}
		/* stable descending: equal keys keep their order */
		referenceOrder(keys, 10, 1, order);
		for (k = 0; k < 10; k++) {
			CHECK(fake_slot(perm2, k) == I(order[k]), "interleaved 2: perm[%zu]", k + 1);
		}
		expectAnswer("interleaved 1", pFree, 1, (sqInt[]){ I(a1) }, fakeTrueObject);
		expectAnswer("interleaved 2", pFree, 1, (sqInt[]){ I(a2) }, fakeTrueObject);
	}
	/* the table full */
	first = newSort("table", 10, 0);
	ids[0] = first;
	for (k = 1; k < TS_CORO_TABLE_SIZE; k++) {
		ids[k] = newSort("table", 10, 0);
		CHECK(ids[k] > ids[k - 1], "table: ids not increasing");
	}
	CHECK(liveSorts() == TS_CORO_TABLE_SIZE, "table: live %lld", (long long)liveSorts());
	expectFailure("table full", pNew, 2, (sqInt[]){ I(10), fakeFalseObject }, PrimErrInappropriate);
	expectAnswer("table: free one", pFree, 1, (sqInt[]){ I(ids[5]) }, fakeTrueObject);
	ids[5] = newSort("table: one more", 10, 0);
	expectFailure("table full again", pNew, 2, (sqInt[]){ I(10), fakeFalseObject }, PrimErrInappropriate);
	/* a few started, then shutdownModule closes them all */
	for (k = 0; k < 4; k++) {
		call("started", pNext, 2, (sqInt[]){ I(ids[k]), I(0) });
	}
	CHECK(shutdown() == 1, "shutdownModule");
	CHECK(liveSorts() == 0, "live after shutdownModule: %lld", (long long)liveSorts());
	expectFailure("next after shutdownModule", pNext, 2, (sqInt[]){ I(first), I(0) }, PrimErrNotFound);
	id = newSort("after shutdownModule", 3, 0);
	for (k = 0; k < TS_CORO_TABLE_SIZE; k++) {
		CHECK(id > ids[k], "ids reused after shutdownModule");
	}
	expectAnswer("after shutdownModule", pFree, 1, (sqInt[]){ I(id) }, fakeTrueObject);
	printf("G8 coroutine: %s, sorts stepped to the end, stale ids, the table of %d, abort, shutdownModule\n",
#if TS_CORO_SUPPORTED
		ts_coro_impl(),
#else
		"?",
#endif
		TS_CORO_TABLE_SIZE);
	fake_release(mark);
}

int
main(int argc, char **argv)
{
	struct VirtualMachine *vm;
	long rounds = 3000;
	int i;

	for (i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--seed") == 0 && i + 1 < argc) {
			seed = strtoull(argv[++i], NULL, 0) | 1;
		} else if (strcmp(argv[i], "--rounds") == 0 && i + 1 < argc) {
			rounds = strtol(argv[++i], NULL, 0);
		} else if (strcmp(argv[i], "--slow") == 0) {
			rounds = 30000;
		} else {
			fprintf(stderr, "usage: gluetest [--seed N] [--rounds N] [--slow]\n");
			return 2;
		}
	}
	vm = fake_init();
	checkModule(vm);
	stringClasses = newStringClasses();
	checkInfo();
	checkArity();
	checkOrders();
	checkSortObjects(rounds);
	checkSortBuffers(rounds);
	checkArgSort(rounds);
	checkCoroutines();
	section = "G1";
	CHECK(shutdown() == 1, "shutdownModule");
	fake_free_all();
	if (failures) {
		printf("gluetest FAILED: %ld checks, %ld failures\n", checks, failures);
		return 1;
	}
	printf("gluetest ok: %ld checks, 0 failures\n", checks);
	return 0;
}
