/* TimSortPlugin.c -- the VM side of the TimSortPlugin: the module functions
 * and the eight primitives of the package TimSort (the submodule
 * plugins/TimSortPlugin/timsort.st), which call the engine
 * (src/engine/tsEngine.c) through tsEngine.h and, natively, the resumable
 * sort (src/coro) through tsCoro.h.  This file never includes timsort.h, and
 * neither the engine nor the resumable sort includes a VM header.
 * docs/TimSort.md, section "Primitive contract", is the contract.
 *
 * Natively it is the shared library libTimSortPlugin.so, whose module
 * functions, primitives and primitiveXAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through TimSortPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).
 *
 * The primitives check their arguments in the contract's order (the
 * argument count; the kinds of the arguments; read-only; the range; the
 * rest), and a failing primitive leaves its arguments alone, except after
 * PrimErrNoCMemory, when the range sorted holds a permutation of its
 * elements.  The sorts of objects first classify every element of the
 * range (SmallInteger, Float that is not a NaN, normalized LargePositive
 * or LargeNegativeInteger, or an instance of one of the four string
 * classes the image passes) and fail with PrimErrBadArgument, before
 * touching anything, unless all of them are numbers or all of them are
 * strings: the image then sorts with its port of timsort.c,
 * TimSortSmalltalk, by Pharo's sortBlock [:a :b | a <= b], which gives
 * Pharo's order (and errors) for anything else.  A forwarder has no class (nil), so
 * it fails too; primitiveSortObjects and primitiveArgSort have an accessor
 * depth of 1, so that the VM follows the forwarders in the slots of their
 * array and retries once.
 *
 * Allocation and GC: the sorts allocate no object (only primitiveInfo
 * does, through instantiateClassindexableSize) and run no Smalltalk, so no
 * GC can move an object while they run: they permute the slots of the
 * Array (or of perm) in place through firstIndexableField, and timsort.c
 * holds oops in its C merge buffer (the engine's malloc; never here).  A
 * permutation of the slots of one object keeps the same references, so it
 * needs no store barrier (the remembered sets stay right), and SmallIntegers
 * need none.  SmallIntegers alone are sorted as their raw tagged oops (in
 * the order of their values) by the engine's typed sort of signed 64-bit
 * words; the other comparisons read both objects through the proxy at
 * every comparison: the numbers exactly (ts_number_compare: an Integer
 * against a Float as asExactFraction would, never the JIT's rounding beyond
 * 2^53), the strings by code point (ts_string_compare).
 *
 * The resumable sort (primitiveBlockSort*): natively, where tsCoro.h has a
 * stack switch, primitiveBlockSortNew opens a sort of 0 .. n-1 in the
 * table of tsCoro (ids, never pointers, so a stale id fails with
 * PrimErrNotFound), and every primitiveBlockSortNext resumes it with the
 * answer of the previous comparison and answers the next request,
 * (i << 32) | j, 0-based, or -1 done, -2 no memory, -3 aborted; n is at
 * most 2^28, so that every request is a SmallInteger.  Elsewhere (the
 * Emscripten VM) the rows stay in the table and the primitives fail with
 * PrimErrUnsupported after their type checks.
 */

#include <math.h>
#include <stdint.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "tsEngine.h"
#include "tsCoro.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

_Static_assert(sizeof(sqInt) == 8, "TimSortPlugin: the slots it sorts are 8-byte words (docs/TimSort.md)");
_Static_assert(sizeof(sqInt) == sizeof(int64_t), "TimSortPlugin: sqInt is the engine's int64_t");

#ifndef TS_TIMSORT_VERSION
# define TS_TIMSORT_VERSION "unknown"
#endif

/* Spur's class format: instance specification and number of named slots */
#define instSpecOf(format) (((format) >> 16) & 0x1F)
#define fixedFieldsOf(format) ((format) & 0xFFFF)

/* The value of a SmallInteger oop of 64-bit Spur, (value << 3) | 1, is
 * oop >> 3: the words of perm are such oops (ts_argsort_buffer's shift) */
#define SMALLINTEGER_SHIFT 3

/* The resumable sort of the plugin: at most 2^28 elements, so that a
 * request (i << 32) | j is below 2^60, a SmallInteger */
#define BLOCK_SORT_MAX_SIZE ((sqInt)1 << 28)

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"TimSortPlugin (i)";
#else
	"TimSortPlugin (e)";
#endif

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitiveArgSort(void);
EXPORT(sqInt) primitiveBlockSortFree(void);
EXPORT(sqInt) primitiveBlockSortNew(void);
EXPORT(sqInt) primitiveBlockSortNext(void);
EXPORT(sqInt) primitiveBlockSortPermutation(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveSortBuffer(void);
EXPORT(sqInt) primitiveSortObjects(void);
EXPORT(sqInt) setInterpreter(struct VirtualMachine *anInterpreter);
EXPORT(sqInt) shutdownModule(void);


/*** The module ***/

EXPORT(const char*)
getModuleName(void)
{
	return moduleName;
}

EXPORT(sqInt)
initialiseModule(void)
{
	return 1;
}

EXPORT(sqInt)
shutdownModule(void)
{
	/* the live resumable sorts and their cached stacks */
#if TS_CORO_SUPPORTED
	ts_coro_close_all();
#endif
	return 1;
}

EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return interpreterProxy->majorVersion() == VM_PROXY_MAJOR
	    && interpreterProxy->minorVersion() >= VM_PROXY_MINOR;
}


/*** Arguments ***/

static sqInt
integerAt(int offset)
{
	return interpreterProxy->integerValueOf(interpreterProxy->stackValue(offset));
}

static int
isIntegerAt(int offset)
{
	return interpreterProxy->isIntegerObject(interpreterProxy->stackValue(offset));
}

/* 1 for true, 0 for false, -1 for anything else */
static int
booleanAt(int offset)
{
	sqInt oop = interpreterProxy->stackValue(offset);

	return oop == interpreterProxy->trueObject() ? 1 : oop == interpreterProxy->falseObject() ? 0 : -1;
}

/* Whether start..stop (1-based, inclusive; empty when start = stop + 1) is
 * a range of size elements */
static int
isRange(sqInt start, sqInt stop, sqInt size)
{
	return start >= 1 && start <= stop + 1 && stop <= size;
}

/* Spur's format word of aClass (its third slot), or -1 when aClass does not
 * look like a class (SimdJSONPlugin.c) */
static sqInt
classFormat(sqInt aClass)
{
	sqInt format;

	if (interpreterProxy->isImmediate(aClass)
	 || !interpreterProxy->isPointers(aClass)
	 || interpreterProxy->slotSizeOf(aClass) < 3) {
		return -1;
	}
	format = interpreterProxy->fetchPointerofObject(2, aClass);
	return interpreterProxy->isIntegerObject(format) ? interpreterProxy->integerValueOf(format) : -1;
}

/* A numeric buffer: its first element and its elements */
typedef struct {
	void *base;
	sqInt count;
} Buffer;

/* The instance specification of the class of a buffer of kind */
static int
instSpecOfKind(sqInt kind)
{
	switch (kind) {
	case TS_KIND_F64: case TS_KIND_I64: case TS_KIND_U64: return 9;
	case TS_KIND_F32: case TS_KIND_I32: case TS_KIND_U32: return 10;
	case TS_KIND_U16: return 12;
	default: return 16;
	}
}

/* Whether oop is a buffer of kind (a SmallInteger in [1, 8]), into *b: an
 * object of 64-, 32-, 16-bit or byte elements as the kind says (its class's
 * instance specification), that is neither a CompiledMethod (whose
 * firstIndexableField is not its bytes) nor a number: a BoxedFloat64 (2
 * words) or a LargeInteger (bytes) would have its representation sorted. */
static int
bufferOf(sqInt oop, sqInt kind, Buffer *b)
{
	sqInt format, cls;

	if (kind < TS_KIND_FIRST || kind > TS_KIND_LAST
	 || interpreterProxy->isImmediate(oop)
	 || !interpreterProxy->isWordsOrBytes(oop)
	 || interpreterProxy->isFloatObject(oop)) {
		return 0;
	}
	cls = interpreterProxy->fetchClassOf(oop);
	if (cls == interpreterProxy->classLargePositiveInteger()
	 || cls == interpreterProxy->classLargeNegativeInteger()) {
		return 0;
	}
	format = classFormat(cls);
	if (format < 0 || instSpecOf(format) != instSpecOfKind(kind) || fixedFieldsOf(format) != 0) {
		return 0;
	}
	b->count = interpreterProxy->byteSizeOf(oop) / (sqInt)TS_KIND_SIZE(kind);
	b->base = interpreterProxy->firstIndexableField(oop);
	return b->count == 0 || b->base != NULL;
}

/* The four string classes {ByteString. ByteSymbol. WideString. WideSymbol},
 * an Array the image passes, at offset into classes: 0 when it is not an
 * Array of four classes */
static int
stringClassesAt(int offset, sqInt classes[4])
{
	sqInt oop = interpreterProxy->stackValue(offset);
	int i;

	if (!interpreterProxy->isArray(oop) || interpreterProxy->slotSizeOf(oop) != 4) {
		return 0;
	}
	for (i = 0; i < 4; i++) {
		classes[i] = interpreterProxy->fetchPointerofObject(i, oop);
		if (classFormat(classes[i]) < 0) {
			return 0;
		}
	}
	return 1;
}


/*** Objects: the pre-pass and the comparisons ***/

/* The comparisons of objects: their arg is the Objects of the call */
typedef struct Objects Objects;
struct Objects {
	sqInt classes[4];		/* the string classes */
	sqInt largePositive, largeNegative;
	ts_less_fn less;		/* of two elements */
	const sqInt *keys;		/* primitiveArgSort: the slots of the Array of keys */
};

/* Two SmallIntegers: their tagged oops are in the order of their values */
static int
lessSmallIntegers(int64_t a, int64_t b, void *arg)
{
	(void)arg;
	return a < b;
}

/* Two Floats, neither a NaN */
static int
lessFloats(int64_t a, int64_t b, void *arg)
{
	(void)arg;
	return interpreterProxy->floatValueOf((sqInt)a) < interpreterProxy->floatValueOf((sqInt)b);
}

static void
numberOf(sqInt oop, const Objects *o, ts_number *x)
{
	if (interpreterProxy->isIntegerObject(oop)) {
		x->type = TS_NUMBER_INTEGER;
		x->integer = interpreterProxy->integerValueOf(oop);
	} else if (interpreterProxy->isFloatObject(oop)) {
		x->type = TS_NUMBER_FLOAT;
		x->real = interpreterProxy->floatValueOf(oop);
	} else {
		x->type = TS_NUMBER_LARGE;
		x->negative = interpreterProxy->fetchClassOf(oop) == o->largeNegative;
		x->digits = interpreterProxy->firstIndexableField(oop);
		x->size = (size_t)interpreterProxy->byteSizeOf(oop);
	}
}

/* Two numbers, exactly */
static int
lessNumbers(int64_t a, int64_t b, void *arg)
{
	ts_number x, y;

	numberOf((sqInt)a, arg, &x);
	numberOf((sqInt)b, arg, &y);
	return ts_number_compare(&x, &y) < 0;
}

static void
stringOf(sqInt oop, ts_string *s)
{
	s->wide = interpreterProxy->isWords(oop) != 0;
	s->size = (size_t)interpreterProxy->byteSizeOf(oop) / (s->wide ? 4 : 1);
	s->units = interpreterProxy->firstIndexableField(oop);
}

/* Two strings, by code point */
static int
lessStrings(int64_t a, int64_t b, void *arg)
{
	ts_string x, y;

	(void)arg;
	stringOf((sqInt)a, &x);
	stringOf((sqInt)b, &y);
	return ts_string_compare(&x, &y) < 0;
}

/* primitiveArgSort: two words of perm, SmallIntegers of 1-based indices of
 * the keys, by their keys */
static int
lessKeys(int64_t a, int64_t b, void *arg)
{
	const Objects *o = arg;

	return o->less(o->keys[(a >> SMALLINTEGER_SHIFT) - 1], o->keys[(b >> SMALLINTEGER_SHIFT) - 1], arg);
}

/* Whether the LargeInteger oop of the class largePositive or largeNegative
 * is normalized: bytes, no high zero byte, and outside SmallInteger's range
 * (above 2^60 - 1, or below -2^60).  Pharo orders the others inconsistently
 * (a 5 in 8 bytes sorts after 7): the image sorts them itself. */
static int
isNormalizedLarge(sqInt oop, int negative)
{
	const uint8_t *digits;
	sqInt size;
	uint64_t magnitude = 0;
	int i;

	if (!interpreterProxy->isBytes(oop)) {
		return 0;
	}
	size = interpreterProxy->byteSizeOf(oop);
	digits = interpreterProxy->firstIndexableField(oop);
	if (size < 1 || !digits || digits[size - 1] == 0) {
		return 0;
	}
	if (size > 8) {
		return 1;
	}
	for (i = (int)size; i-- > 0;) {
		magnitude = (magnitude << 8) | digits[i];
	}
	return negative ? magnitude > ((uint64_t)1 << 60) : magnitude >= ((uint64_t)1 << 60);
}

/* The pre-pass: classifies each of the n slots, and sets o->less to the
 * comparison of the family they all belong to; 0 (nothing set) when an
 * element is in no family, or the families are mixed. */
static int
classify(const sqInt *slots, sqInt n, Objects *o)
{
	sqInt i;
	int smallIntegers = 0, floats = 0, larges = 0, strings = 0;

	o->largePositive = interpreterProxy->classLargePositiveInteger();
	o->largeNegative = interpreterProxy->classLargeNegativeInteger();
	for (i = 0; i < n; i++) {
		sqInt e = slots[i], cls;
		if (interpreterProxy->isIntegerObject(e)) {
			smallIntegers = 1;
			continue;
		}
		if (interpreterProxy->isFloatObject(e)) {
			if (isnan(interpreterProxy->floatValueOf(e))) {
				return 0;
			}
			floats = 1;
			continue;
		}
		if (interpreterProxy->isImmediate(e)) {
			return 0;	/* a Character */
		}
		cls = interpreterProxy->fetchClassOf(e);	/* nil for a forwarder */
		if (cls == o->largePositive || cls == o->largeNegative) {
			if (!isNormalizedLarge(e, cls == o->largeNegative)) {
				return 0;
			}
			larges = 1;
			continue;
		}
		if (cls == o->classes[0] || cls == o->classes[1] || cls == o->classes[2] || cls == o->classes[3]) {
			/* 32-bit words, or bytes that are not a CompiledMethod's */
			if (!interpreterProxy->isWords(e)
			 && !(interpreterProxy->isBytes(e) && interpreterProxy->isWordsOrBytes(e))) {
				return 0;
			}
			if (interpreterProxy->byteSizeOf(e) > 0 && !interpreterProxy->firstIndexableField(e)) {
				return 0;
			}
			strings = 1;
			continue;
		}
		return 0;
	}
	if (strings && (smallIntegers || floats || larges)) {
		return 0;
	}
	o->less = strings ? lessStrings
		: !floats && !larges ? lessSmallIntegers
		: !smallIntegers && !larges ? lessFloats
		: lessNumbers;
	return 1;
}

/* The PrimErr code of an engine's status (never TS_OK) */
static sqInt
failureOf(int status)
{
	return status == TS_NO_MEMORY ? PrimErrNoCMemory : PrimErrBadArgument;
}


/*** primitiveInfo ***/

/* A new ByteString of the bytes of a C string, or 0 when the allocation
 * failed: the VM has set the failure code (PrimErrNoMemory), and retries
 * the primitive after a scavenge and after a full GC. */
static sqInt
byteString(const char *bytes)
{
	size_t length = strlen(bytes);
	sqInt oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), (sqInt)length);

	if (oop && length) {
		memcpy(interpreterProxy->firstIndexableField(oop), bytes, length);
	}
	return oop;
}

/*	TimSortPrimitives class>>primInfo
	<primitive: 'primitiveInfo' module: 'TimSortPlugin' error: ec>
	Answer { protocol. pluginVersion. 'timsort.c <commit>'. platform.
	coroutineSupport. liveCoroutines }: 'native' or 'emscripten', true when
	the resumable sort (primitiveBlockSort*) works on this VM, and the number
	of live resumable sorts. */
EXPORT(sqInt)
primitiveInfo(void)
{
	sqInt result, oop;
	int coroutines = 0;
	int64_t live = 0;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
#if TS_CORO_SUPPORTED
	coroutines = ts_coro_available() != 0;
	live = ts_coro_live();
#endif
	if (!(result = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 6))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(TS_PROTOCOL));
	if (!(oop = byteString(TS_PLUGIN_VERSION))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(1, result, oop);
	if (!(oop = byteString("timsort.c " TS_TIMSORT_VERSION))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(2, result, oop);
#ifdef __EMSCRIPTEN__
	oop = byteString("emscripten");
#else
	oop = byteString("native");
#endif
	if (!oop) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(3, result, oop);
	interpreterProxy->storePointerofObjectwithValue(4, result,
		coroutines ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(5, result, interpreterProxy->integerObjectOf((sqInt)live));
	return interpreterProxy->methodReturnValue(result);
}


/*** The sorts ***/

/*	TimSortPrimitives class>>primSortObjects: anArray from: start to: stop
		descending: aBoolean stringClasses: classes
	<primitive: 'primitiveSortObjects' module: 'TimSortPlugin' error: ec>
	Sort anArray[start..stop] in place, stably, ascending (as Pharo's
	sort: [:a :b | a <= b]) or descending (sort: [:a :b | a >= b]), when its
	elements are all numbers (SmallInteger, Float but NaN, normalized
	LargeIntegers; compared exactly) or all strings (instances of the four
	classes; compared by code point).  Answer anArray.  Fail with
	#'bad argument' for anything else (the image then sorts with its port,
	TimSortSmalltalk, by [:a :b | a <= b] or [:a :b | a >= b]),
	#'no modification' when anArray is read-only, #'bad index' for a range
	outside it, #'insufficient C memory' when timsort.c's merge buffer could
	not be allocated (the range then holds a permutation of its elements). */
EXPORT(sqInt)
primitiveSortObjects(void)
{
	sqInt array, start, stop, *slots;
	int descending, status;
	Objects o;

	if (interpreterProxy->methodArgumentCount() != 5) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	array = interpreterProxy->stackValue(4);
	if (!interpreterProxy->isArray(array)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	memset(&o, 0, sizeof o);
	if (!isIntegerAt(3) || !isIntegerAt(2) || (descending = booleanAt(1)) < 0 || !stringClassesAt(0, o.classes)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (interpreterProxy->isOopImmutable(array)) {
		return interpreterProxy->primitiveFailFor(PrimErrNoModification);
	}
	start = integerAt(3);
	stop = integerAt(2);
	if (!isRange(start, stop, interpreterProxy->slotSizeOf(array))) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	if (stop - start + 1 < 2) {
		return interpreterProxy->methodReturnValue(array);
	}
	slots = (sqInt *)interpreterProxy->firstIndexableField(array) + (start - 1);
	if (!classify(slots, stop - start + 1, &o)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	/* SmallIntegers: their oops as signed 64-bit integers, the engine's
	   typed sort (one comparison call less per comparison) */
	status = o.less == lessSmallIntegers
		? ts_sort_buffer(slots, stop - start + 1, TS_KIND_I64, descending)
		: ts_sort_words((int64_t *)slots, stop - start + 1, descending, o.less, &o);
	if (status != TS_OK) {
		return interpreterProxy->primitiveFailFor(failureOf(status));
	}
	return interpreterProxy->methodReturnValue(array);
}

/*	TimSortPrimitives class>>primSortBuffer: aBuffer kind: kind from: start
		to: stop descending: aBoolean
	<primitive: 'primitiveSortBuffer' module: 'TimSortPlugin' error: ec>
	Sort the elements start..stop of aBuffer in place, stably, by value:
	kind (TimSort kindFloat64 ...) says their type, and must be the one of
	aBuffer's class (Float64Array 1, Float32Array 2, DoubleWordArray 4,
	IntegerArray 5, WordArray and Bitmap 6, DoubleByteArray 7, ByteArray 8).
	Floats: every NaN after every number (before, descending), -0.0 = 0.0.
	Answer aBuffer.  Fail with #'bad argument' for a kind that is not
	aBuffer's, #'no modification' when aBuffer is read-only (Pharo's own
	Float32Array and Float64Array do not check it), #'bad index' for a range
	outside it, #'insufficient C memory' when a buffer could not be
	allocated (the elements are untouched, or permuted for the kinds of 8
	bytes). */
EXPORT(sqInt)
primitiveSortBuffer(void)
{
	sqInt buffer, kind, start, stop;
	size_t size;
	int descending, status;
	Buffer b;

	if (interpreterProxy->methodArgumentCount() != 5) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!isIntegerAt(3)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	kind = integerAt(3);
	buffer = interpreterProxy->stackValue(4);
	if (!bufferOf(buffer, kind, &b)
	 || !isIntegerAt(2) || !isIntegerAt(1) || (descending = booleanAt(0)) < 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (interpreterProxy->isOopImmutable(buffer)) {
		return interpreterProxy->primitiveFailFor(PrimErrNoModification);
	}
	start = integerAt(2);
	stop = integerAt(1);
	if (!isRange(start, stop, b.count)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	size = TS_KIND_SIZE(kind);
	status = ts_sort_buffer((char *)b.base + (size_t)(start - 1) * size, stop - start + 1, (int)kind, descending);
	if (status != TS_OK) {
		return interpreterProxy->primitiveFailFor(failureOf(status));
	}
	return interpreterProxy->methodReturnValue(buffer);
}

/*	TimSortPrimitives class>>primArgSort: keys kind: kindOrNil into: perm
		from: start to: stop descending: aBoolean stringClasses: classes
	<primitive: 'primitiveArgSort' module: 'TimSortPlugin' error: ec>
	Fill perm (an Array of stop - start + 1 slots) with start .. stop and
	sort it, stably, by keys at those indices: keys an Array (kindOrNil nil;
	its elements classified as primSortObjects: does) or a buffer of
	kindOrNil (as primSortBuffer: does).  Position k of the sorted range
	takes the element at perm at: k.  Answer perm.  Fail with
	#'bad argument' for keys of no family, a kind that is not the keys', or
	a perm of another size (or perm being keys), #'no modification' when
	perm is read-only, #'bad index' for a range outside keys,
	#'insufficient C memory' (perm then holds a permutation of
	start .. stop). */
EXPORT(sqInt)
primitiveArgSort(void)
{
	sqInt keys, kind, perm, start, stop, n, k, count, *slots;
	int descending, status;
	Buffer b = { NULL, 0 };
	Objects o;

	if (interpreterProxy->methodArgumentCount() != 7) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	keys = interpreterProxy->stackValue(6);
	kind = interpreterProxy->stackValue(5);
	if (kind == interpreterProxy->nilObject()) {
		kind = 0;
		if (!interpreterProxy->isArray(keys)) {
			return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		}
		count = interpreterProxy->slotSizeOf(keys);
	} else {
		if (!interpreterProxy->isIntegerObject(kind)
		 || !bufferOf(keys, kind = interpreterProxy->integerValueOf(kind), &b)) {
			return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		}
		count = b.count;
	}
	perm = interpreterProxy->stackValue(4);
	memset(&o, 0, sizeof o);
	if (!interpreterProxy->isArray(perm)
	 || !isIntegerAt(3) || !isIntegerAt(2) || (descending = booleanAt(1)) < 0 || !stringClassesAt(0, o.classes)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (interpreterProxy->isOopImmutable(perm)) {
		return interpreterProxy->primitiveFailFor(PrimErrNoModification);
	}
	start = integerAt(3);
	stop = integerAt(2);
	if (!isRange(start, stop, count)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	n = stop - start + 1;
	if (interpreterProxy->slotSizeOf(perm) != n || perm == keys) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!kind && n > 1) {
		o.keys = interpreterProxy->firstIndexableField(keys);
		if (!classify(o.keys + (start - 1), n, &o)) {
			return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		}
	}
	slots = interpreterProxy->firstIndexableField(perm);
	for (k = 0; k < n; k++) {
		slots[k] = interpreterProxy->integerObjectOf(start + k);
	}
	/* keys of SmallIntegers: their oops as signed 64-bit keys */
	status = kind ? ts_argsort_buffer(b.base, count, (int)kind, (int64_t *)slots, n, SMALLINTEGER_SHIFT, 1, descending)
		: n > 1 && o.less == lessSmallIntegers
		? ts_argsort_buffer(o.keys, count, TS_KIND_I64, (int64_t *)slots, n, SMALLINTEGER_SHIFT, 1, descending)
		: ts_sort_words((int64_t *)slots, n, descending, lessKeys, &o);
	if (status != TS_OK) {
		return interpreterProxy->primitiveFailFor(failureOf(status));
	}
	return interpreterProxy->methodReturnValue(perm);
}


/*** The resumable sort ***/

/* Whether the resumable sort works on this VM */
static int
blockSortAvailable(void)
{
#if TS_CORO_SUPPORTED
	return ts_coro_available() != 0;
#else
	return 0;
#endif
}

/*	TimSortPrimitives class>>primBlockSortNew: n descending: aBoolean
	<primitive: 'primitiveBlockSortNew' module: 'TimSortPlugin' error: ec>
	Open a resumable sort of the indices 0 .. n-1 (n at most 2^28) and
	answer its id, a SmallInteger >= 2^32.  Fail with #'bad argument' for
	another n, #'unsupported operation' where the VM has no resumable sort
	(the Emscripten VM), #inappropriate when 64 sorts are live,
	#'insufficient C memory' when its stack or its permutation could not be
	allocated. */
EXPORT(sqInt)
primitiveBlockSortNew(void)
{
	sqInt n;
	int descending;

	if (interpreterProxy->methodArgumentCount() != 2) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!isIntegerAt(1) || (descending = booleanAt(0)) < 0
	 || (n = integerAt(1)) < 0 || n > BLOCK_SORT_MAX_SIZE) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!blockSortAvailable()) {
		return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
	}
#if TS_CORO_SUPPORTED
	{
		int status = TS_OK;
		int64_t id = ts_coro_open(n, descending, &status);
		if (id <= 0) {
			return interpreterProxy->primitiveFailFor(status == TS_LIMIT ? PrimErrInappropriate
				: status == TS_NO_MEMORY ? PrimErrNoCMemory
				: status == TS_UNSUPPORTED ? PrimErrUnsupported
				: PrimErrBadArgument);
		}
		return interpreterProxy->methodReturnValue(interpreterProxy->integerObjectOf((sqInt)id));
	}
#else
	(void)n;
	(void)descending;
	return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
#endif
}

/*	TimSortPrimitives class>>primBlockSortNext: id answer: anInteger
	<primitive: 'primitiveBlockSortNext' module: 'TimSortPlugin' error: ec>
	Resume the sort id with the answer to its last request (1: element i
	comes before element j, 0: it does not, -1: abort the sort; the first
	call passes 0, which is ignored) and answer its next request,
	(i << 32) | j with 0-based i and j, or -1 when it is done, -2 when
	timsort.c's merge buffer could not be allocated, -3 when it was
	aborted.  Fail with #'bad argument' unless both are SmallIntegers,
	#'unsupported operation', #'not found' for an id that is not live. */
EXPORT(sqInt)
primitiveBlockSortNext(void)
{
	if (interpreterProxy->methodArgumentCount() != 2) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!isIntegerAt(1) || !isIntegerAt(0)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!blockSortAvailable()) {
		return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
	}
#if TS_CORO_SUPPORTED
	{
		ts_coro *h = ts_coro_lookup(integerAt(1));
		if (!h) {
			return interpreterProxy->primitiveFailFor(PrimErrNotFound);
		}
		return interpreterProxy->methodReturnValue(
			interpreterProxy->integerObjectOf((sqInt)ts_coro_next(h, integerAt(0))));
	}
#else
	return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
#endif
}

/*	TimSortPrimitives class>>primBlockSortPermutation: id into: perm
	<primitive: 'primitiveBlockSortPermutation' module: 'TimSortPlugin' error: ec>
	Fill perm (an Array of the sort's n slots) with the sorted order of the
	finished sort id, 1-based: position k takes the element at perm at: k.
	Answer perm.  Fail with #'bad argument', #'unsupported operation',
	#'not found' for an id that is not live, #'bad argument' for a perm of
	another size, #'no modification' when perm is read-only,
	#inappropriate before the sort answered -1 (done). */
EXPORT(sqInt)
primitiveBlockSortPermutation(void)
{
	sqInt perm;

	if (interpreterProxy->methodArgumentCount() != 2) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	perm = interpreterProxy->stackValue(0);
	if (!isIntegerAt(1) || !interpreterProxy->isArray(perm)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!blockSortAvailable()) {
		return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
	}
#if TS_CORO_SUPPORTED
	{
		ts_coro *h = ts_coro_lookup(integerAt(1));
		const int64_t *permutation;
		sqInt *slots, k, n;
		if (!h) {
			return interpreterProxy->primitiveFailFor(PrimErrNotFound);
		}
		n = (sqInt)ts_coro_size(h);
		if (interpreterProxy->slotSizeOf(perm) != n) {
			return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		}
		if (interpreterProxy->isOopImmutable(perm)) {
			return interpreterProxy->primitiveFailFor(PrimErrNoModification);
		}
		if (ts_coro_result(h) != TS_DONE) {
			return interpreterProxy->primitiveFailFor(PrimErrInappropriate);
		}
		permutation = ts_coro_permutation(h);
		slots = interpreterProxy->firstIndexableField(perm);
		for (k = 0; k < n; k++) {
			slots[k] = interpreterProxy->integerObjectOf((sqInt)permutation[k] + 1);
		}
		return interpreterProxy->methodReturnValue(perm);
	}
#else
	return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
#endif
}

/*	TimSortPrimitives class>>primBlockSortFree: id
	<primitive: 'primitiveBlockSortFree' module: 'TimSortPlugin' error: ec>
	Free the sort id, in any state (a pending one is aborted first).
	Answer true, or false when id was not live: a second free does not
	fail, so that ensure: blocks may free twice.  Fail with
	#'bad argument', #'unsupported operation'. */
EXPORT(sqInt)
primitiveBlockSortFree(void)
{
	if (interpreterProxy->methodArgumentCount() != 1) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!isIntegerAt(0)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!blockSortAvailable()) {
		return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
	}
#if TS_CORO_SUPPORTED
	return interpreterProxy->methodReturnValue(ts_coro_close(integerAt(0)) == TS_OK
		? interpreterProxy->trueObject() : interpreterProxy->falseObject());
#else
	return interpreterProxy->primitiveFailFor(PrimErrUnsupported);
#endif
}

#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "TimSortPlugin";
void* TimSortPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveArgSort\000\001", (void*)primitiveArgSort},
	{(void*)_m, "primitiveBlockSortFree\000\000", (void*)primitiveBlockSortFree},
	{(void*)_m, "primitiveBlockSortNew\000\000", (void*)primitiveBlockSortNew},
	{(void*)_m, "primitiveBlockSortNext\000\000", (void*)primitiveBlockSortNext},
	{(void*)_m, "primitiveBlockSortPermutation\000\000", (void*)primitiveBlockSortPermutation},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveSortBuffer\000\000", (void*)primitiveSortBuffer},
	{(void*)_m, "primitiveSortObjects\000\001", (void*)primitiveSortObjects},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveArgSortAccessorDepth = 1;
EXPORT(signed char) primitiveBlockSortFreeAccessorDepth = 0;
EXPORT(signed char) primitiveBlockSortNewAccessorDepth = 0;
EXPORT(signed char) primitiveBlockSortNextAccessorDepth = 0;
EXPORT(signed char) primitiveBlockSortPermutationAccessorDepth = 0;
EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveSortBufferAccessorDepth = 0;
EXPORT(signed char) primitiveSortObjectsAccessorDepth = 1;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
