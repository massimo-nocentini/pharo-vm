/* SimdJSONPlugin.c -- the VM side of the SimdJSONPlugin: the module
 * functions, the five primitives of plugins/SimdJSONPlugin/st/SimdJSON.st,
 * the builder (sjBridge.h) through which src/cxx/sjEngine.cpp makes the
 * objects of a JSON value, and the walk of primitiveWrite, which writes the
 * JSON of a value.  This file never includes simdjson.h, and the
 * engine never includes a VM header.
 *
 * Natively it is the shared library libSimdJSONPlugin.so, whose module
 * functions, primitives and primitiveXAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through SimdJSONPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).
 *
 * Allocation: only instantiateClassindexableSize, which falls back to old
 * space when eden is full, plus the immediates of the SmallInteger and
 * SmallFloat64 ranges.  The proxy's constructors of 64-bit integers and
 * boxed floats allocate in eden only, and abort the VM once a big build has
 * filled it.  Spur never collects garbage inside an allocation of the proxy,
 * so the oops held in statics and in the malloc'd lists below stay valid for
 * the whole primitive.  Every pointer store goes through
 * storePointerofObjectwithValue: objects allocated after eden filled up are
 * old.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "sjBridge.h"
#include "sjText.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

#define SJ_PROTOCOL 2	/* 2: primitiveRead takes maxUnits, status 3 TOO_LARGE; primitiveWrite came later without a change of the read contract */
#define SJ_PLUGIN_VERSION "SimdJSONPlugin 1.0"
#ifndef SJ_SIMDJSON_VERSION
# define SJ_SIMDJSON_VERSION "5.0.5"	/* the release cmake/simdjson.cmake pins */
#endif

/* Spur's class format: instance specification and number of named slots */
#define instSpecOf(format) (((format) >> 16) & 0x1F)
#define fixedFieldsOf(format) ((format) & 0xFFFF)
#define arrayFormat 2
#define indexablePointersFormat 3
#define nonIndexablePointerFormat 1
#define firstLongFormat 10

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"SimdJSONPlugin (i)";
#else
	"SimdJSONPlugin (e)";
#endif

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitiveErrorMessage(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveRead(void);
EXPORT(sqInt) primitiveRelease(void);
EXPORT(sqInt) primitiveWrite(void);
EXPORT(sqInt) setInterpreter(struct VirtualMachine *anInterpreter);
EXPORT(sqInt) shutdownModule(void);


/*** The state of one primitiveRead ***/

/* A number text stored at the 1-based slot index1 of container.  A later
 * duplicate key may replace it in a Dictionary: fillResult keeps only the
 * entries whose slot still holds their text. */
typedef struct { sj_oop container; sqInt index1; sj_oop text; } Fixup;

static sqInt objectClass;		/* SimdJSONFlatMap, or Dictionary with dictMode */
static sqInt associationClass;
static sqInt wideStringClass;
static int dictMode;
static int symbolKeys;
static sqInt hashSeed;
static int tallyIndex, arrayIndex, keyIndex, valueIndex;	/* 0-based slots */

/* The number text answered last and not stored yet: the slot it lands in
 * becomes a fixup, (container, 1-based index for instVarAt:). */
static sj_oop lastText;
static Fixup *fixups;
static size_t fixupCount, fixupCapacity;

/* With symbolKeys: the distinct keys in first-seen order */
static sj_keyset *keySet;
static sj_oop *keyStrings;
static size_t keyCount, keyCapacity;

static void
freeCallState(void)
{
	free(fixups);
	fixups = NULL;
	fixupCount = fixupCapacity = 0;
	free(keyStrings);
	keyStrings = NULL;
	keyCount = keyCapacity = 0;
	if (keySet) {
		sj_keyset_free(keySet);
		keySet = NULL;
	}
}

/* An allocation of the heap failed: the VM has set PrimErrNoMemory (or the
 * code of a bad request), and retries the primitive after a scavenge and
 * after a full GC. */
static sj_oop
noMemory(void)
{
	if (!interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
	}
	return 0;
}

static sj_oop
noCMemory(void)
{
	interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
	return 0;
}

static sj_oop
instantiate(sqInt aClass, size_t size)
{
	sqInt oop = interpreterProxy->instantiateClassindexableSize(aClass, (sqInt)size);

	return oop ? oop : noMemory();
}

static sj_oop
byteString(const char *bytes, size_t length)
{
	sj_oop oop = instantiate(interpreterProxy->classString(), length);

	if (oop && length) {
		memcpy(interpreterProxy->firstIndexableField(oop), bytes, length);
	}
	return oop;
}

static int
addFixup(sj_oop container, sqInt index1, sj_oop text)
{
	if (fixupCount == fixupCapacity) {
		size_t capacity = fixupCapacity ? 2 * fixupCapacity : 64;
		Fixup *grown = realloc(fixups, capacity * sizeof(Fixup));

		if (!grown) {
			noCMemory();
			return -1;
		}
		fixups = grown;
		fixupCapacity = capacity;
	}
	fixups[fixupCount].container = container;
	fixups[fixupCount].index1 = index1;
	fixups[fixupCount].text = text;
	fixupCount++;
	return 0;
}

/* Drop the fixups whose slot no longer holds their text: one pass, in
 * order, so that a document of many duplicate keys stays linear. */
static void
dropReplacedFixups(void)
{
	size_t i, kept = 0;

	for (i = 0; i < fixupCount; i++) {
		if (interpreterProxy->fetchPointerofObject(fixups[i].index1 - 1, fixups[i].container) == fixups[i].text) {
			fixups[kept++] = fixups[i];
		}
	}
	fixupCount = kept;
}

/* Store value at the 0-based slot of container, and record a fixup when it
 * is the number text answered last. */
static int
store(sj_oop container, size_t slot0, sj_oop value)
{
	interpreterProxy->storePointerofObjectwithValue((sqInt)slot0, container, value);
	if (lastText && value == lastText) {
		lastText = 0;
		return addFixup(container, (sqInt)slot0 + 1, value);
	}
	return 0;
}


/*** The builder (C side of sjBridge.h) ***/

/* A LargeInteger of 8 little-endian bytes: a magnitude outside the
 * SmallInteger range needs all of them, so it is normalized. */
static sj_oop
largeInteger(uint64_t magnitude, int negative)
{
	sj_oop oop = instantiate(negative
			? interpreterProxy->classLargeNegativeInteger()
			: interpreterProxy->classLargePositiveInteger(), 8);
	unsigned char *bytes;
	int i;

	if (!oop) {
		return 0;
	}
	bytes = interpreterProxy->firstIndexableField(oop);
	for (i = 0; i < 8; i++) {
		bytes[i] = (unsigned char)(magnitude >> (8 * i));
	}
	return oop;
}

static sj_oop
bInt64(int64_t value)
{
	if (value >= MinSmallInteger && value <= MaxSmallInteger) {
		return interpreterProxy->integerObjectOf((sqInt)value);
	}
	return value < 0 ? largeInteger(0 - (uint64_t)value, 1) : largeInteger((uint64_t)value, 0);
}

static sj_oop
bUInt64(uint64_t value)
{
	if (value <= (uint64_t)MaxSmallInteger) {
		return interpreterProxy->integerObjectOf((sqInt)value);
	}
	return largeInteger(value, 0);
}

static sj_oop
bFloat64(double value)
{
	sj_oop oop;

	if (sj_is_small_float(value)) {
		return interpreterProxy->floatObjectOf(value);	/* an immediate: no allocation */
	}
	/* classFloat() is BoxedFloat64, two 32-bit slots */
	oop = instantiate(interpreterProxy->classFloat(), 2);
	if (oop) {
		memcpy(interpreterProxy->firstIndexableField(oop), &value, sizeof value);
	}
	return oop;
}

static sj_oop
bNumberText(const char *ascii, size_t length)
{
	sj_oop oop = byteString(ascii, length);

	sj_note_number_text();
	lastText = oop;
	return oop;
}

/* UTF-8 validated by simdjson, into the narrowest string: a ByteString of
 * the bytes when they are ASCII, a ByteString of Latin-1 characters when
 * every code point fits in a byte, else an instance of wideStringClass. */
static sj_oop
bString(const char *utf8, size_t length)
{
	const unsigned char *s = (const unsigned char *)utf8;
	size_t i, chars;
	uint32_t maxCodePoint;
	sj_oop oop;

	for (i = 0; i < length && s[i] < 0x80; i++) {
	}
	if (i == length) {
		return byteString(utf8, length);
	}
	sj_utf8_scan(utf8, length, &chars, &maxCodePoint);
	if (maxCodePoint <= 0xFF) {
		oop = instantiate(interpreterProxy->classString(), chars);
		if (oop) {
			sj_utf8_to_latin1(utf8, length, interpreterProxy->firstIndexableField(oop));
		}
		return oop;
	}
	oop = instantiate(wideStringClass, chars);
	if (oop) {
		sj_utf8_to_utf32(utf8, length, interpreterProxy->firstIndexableField(oop));
	}
	return oop;
}

/* A fresh String per occurrence, or with symbolKeys the 1-based index of
 * the key among the distinct keys of this call, which the image interns. */
static sj_oop
bKey(const char *utf8, size_t length)
{
	int64_t index;
	int isNew = 0;
	sj_oop string;

	if (!symbolKeys) {
		return bString(utf8, length);
	}
	index = sj_keyset_index(keySet, utf8, length, &isNew);
	if (index < 0) {
		return noCMemory();
	}
	if (isNew) {
		if (!(string = bString(utf8, length))) {
			return 0;
		}
		if (keyCount == keyCapacity) {
			size_t capacity = keyCapacity ? 2 * keyCapacity : 64;
			sj_oop *grown = realloc(keyStrings, capacity * sizeof(sj_oop));

			if (!grown) {
				return noCMemory();
			}
			keyStrings = grown;
			keyCapacity = capacity;
		}
		keyStrings[keyCount++] = string;
	}
	return interpreterProxy->integerObjectOf((sqInt)index);
}

static sj_oop
bArray(size_t size)
{
	return instantiate(interpreterProxy->classArray(), size);	/* filled with nil */
}

static int
bArrayPut(sj_oop array, size_t index0, sj_oop value)
{
	return store(array, index0, value);
}

/* With dictMode an empty Dictionary whose array has room for npairs keys
 * (sj_dict_capacity), else a SimdJSONFlatMap of 2 x npairs slots. */
static sj_oop
bObject(size_t npairs)
{
	sj_oop array, dictionary;

	if (!dictMode) {
		return instantiate(objectClass, 2 * npairs);
	}
	if (!(array = instantiate(interpreterProxy->classArray(), sj_dict_capacity(npairs)))
	 || !(dictionary = instantiate(objectClass, 0))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(tallyIndex, dictionary, interpreterProxy->integerObjectOf(0));
	interpreterProxy->storePointerofObjectwithValue(arrayIndex, dictionary, array);
	return dictionary;
}

/* One step of String>>hash: the code point, then hashMultiply */
static uint64_t
hashStep(uint64_t hash, uint32_t codePoint)
{
	uint64_t low;

	hash += codePoint;
	low = hash & 16383;
	return (0x260D * low + (((0x260D * (hash >> 14) + 0x0065 * low) & 16383) * 16384)) & 0xFFFFFFF;
}

/* String>>hash of the image: ByteString class>>stringHash:initialHash:,
 * seeded with ByteString hash, over the code points of the key. */
static sqInt
keyHash(sj_oop key)
{
	uint64_t hash = (uint64_t)hashSeed & 0xFFFFFFF;
	size_t i, n;

	if (interpreterProxy->isBytes(key)) {
		const unsigned char *s = interpreterProxy->firstIndexableField(key);

		n = (size_t)interpreterProxy->byteSizeOf(key);
		for (i = 0; i < n; i++) {
			hash = hashStep(hash, s[i]);
		}
	} else {
		const uint32_t *s = interpreterProxy->firstIndexableField(key);

		n = (size_t)interpreterProxy->byteSizeOf(key) / 4;
		for (i = 0; i < n; i++) {
			hash = hashStep(hash, s[i]);
		}
	}
	return (sqInt)hash;
}

/* Keys are strings of the narrowest class, so equal keys have the same
 * format and the same bytes. */
static int
keysEqual(sj_oop a, sj_oop b)
{
	size_t n;

	if (a == b) {
		return 1;
	}
	if (interpreterProxy->isBytes(a) != interpreterProxy->isBytes(b)) {
		return 0;
	}
	n = (size_t)interpreterProxy->byteSizeOf(a);
	return n == (size_t)interpreterProxy->byteSizeOf(b)
		&& memcmp(interpreterProxy->firstIndexableField(a), interpreterProxy->firstIndexableField(b), n) == 0;
}

/* Dictionary>>at:put: on a Dictionary whose array never needs to grow: the
 * probe of scanFor: (hash \\ size, linear, wrapping), a new Association in
 * the first empty slot, or the last value wins on a duplicate key.  The
 * fixup of the value it replaces is dropped by fillResult, or
 * {"a":1e5,"a":"1e5"} would turn the string into a number. */
static int
dictionaryPut(sj_oop dictionary, sj_oop key, sj_oop value)
{
	sqInt array = interpreterProxy->fetchPointerofObject(arrayIndex, dictionary);
	sqInt nilOop = interpreterProxy->nilObject();
	size_t size = (size_t)interpreterProxy->slotSizeOf(array);
	size_t index = (size_t)keyHash(key) % size;
	sqInt element, association, tally;

	for (;;) {
		element = interpreterProxy->fetchPointerofObject((sqInt)index, array);
		if (element == nilOop) {
			if (!(association = instantiate(associationClass, 0))) {
				return -1;
			}
			interpreterProxy->storePointerofObjectwithValue(keyIndex, association, key);
			interpreterProxy->storePointerofObjectwithValue((sqInt)index, array, association);
			tally = interpreterProxy->fetchPointerofObject(tallyIndex, dictionary);
			interpreterProxy->storePointerofObjectwithValue(tallyIndex, dictionary,
				interpreterProxy->integerObjectOf(interpreterProxy->integerValueOf(tally) + 1));
			return store(association, valueIndex, value);
		}
		if (keysEqual(interpreterProxy->fetchPointerofObject(keyIndex, element), key)) {
			return store(element, valueIndex, value);
		}
		index = (index + 1) % size;
	}
}

static int
bObjectPut(sj_oop object, size_t pair0, sj_oop key, sj_oop value)
{
	if (dictMode) {
		return dictionaryPut(object, key, value);
	}
	interpreterProxy->storePointerofObjectwithValue((sqInt)(2 * pair0), object, key);
	return store(object, 2 * pair0 + 1, value);
}

static sj_builder builder = {
	0, 0, 0,
	bInt64, bUInt64, bFloat64, bNumberText, bString, bKey,
	bArray, bArrayPut, bObject, bObjectPut
};


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
	sj_release(1);
	return 1;
}

EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return interpreterProxy->majorVersion() == VM_PROXY_MAJOR
	    && interpreterProxy->minorVersion() >= VM_PROXY_MINOR;
}


/*** The primitives ***/

/* Spur's format word of aClass (its third slot), or -1 when aClass does not
 * look like a class: instantiateClassindexableSize must not see it. */
static sqInt
classFormat(sqInt aClass)
{
	sqInt format;

	if (!interpreterProxy->isPointers(aClass)
	 || interpreterProxy->slotSizeOf(aClass) < 3) {
		return -1;
	}
	format = interpreterProxy->fetchPointerofObject(2, aClass);
	return interpreterProxy->isIntegerObject(format) ? interpreterProxy->integerValueOf(format) : -1;
}

static int
isIntegerAtLeast(sqInt oop, sqInt minimum)
{
	return interpreterProxy->isIntegerObject(oop) && interpreterProxy->integerValueOf(oop) >= minimum;
}

/* The arguments of primitiveRead that depend on the mode, into the statics
 * of the builder; answers a PrimErr code or 0. */
static sqInt
readClassArguments(void)
{
	sqInt format, layout, assocFormat;

	objectClass = interpreterProxy->stackValue(4);
	associationClass = interpreterProxy->stackValue(3);
	wideStringClass = interpreterProxy->stackValue(2);
	format = classFormat(wideStringClass);
	if (format < 0 || instSpecOf(format) != firstLongFormat || fixedFieldsOf(format) != 0) {
		return PrimErrBadArgument;
	}
	format = classFormat(objectClass);
	if (format < 0) {
		return PrimErrBadArgument;
	}
	if (!dictMode) {
		return (instSpecOf(format) == arrayFormat || instSpecOf(format) == indexablePointersFormat)
			&& fixedFieldsOf(format) == 0
			? 0 : PrimErrBadArgument;
	}
	assocFormat = classFormat(associationClass);
	if (assocFormat < 0
	 || !interpreterProxy->isIntegerObject(interpreterProxy->stackValue(1))
	 || !isIntegerAtLeast(interpreterProxy->stackValue(0), 0)) {
		return PrimErrBadArgument;
	}
	hashSeed = interpreterProxy->integerValueOf(interpreterProxy->stackValue(1));
	layout = interpreterProxy->integerValueOf(interpreterProxy->stackValue(0));
	tallyIndex = (int)(layout & 15);
	arrayIndex = (int)((layout >> 4) & 15);
	keyIndex = (int)((layout >> 8) & 15);
	valueIndex = (int)((layout >> 12) & 15);
	return layout <= 0xFFFF
		&& tallyIndex != arrayIndex && keyIndex != valueIndex
		&& instSpecOf(format) == nonIndexablePointerFormat
		&& fixedFieldsOf(format) > (tallyIndex > arrayIndex ? tallyIndex : arrayIndex)
		&& instSpecOf(assocFormat) == nonIndexablePointerFormat
		&& fixedFieldsOf(assocFormat) > (keyIndex > valueIndex ? keyIndex : valueIndex)
		? 0 : PrimErrBadArgument;
}

/* The 6-slot answer of a call that returned SJ_RC_DONE, written into result:
 * { status. value. nextIndex. fixups. keys. code }.  Answers 0 when an
 * allocation failed. */
static sqInt
fillResult(sqInt result, const sj_outcome *out, sqInt start)
{
	sqInt fixupArray = interpreterProxy->nilObject();
	sqInt keyArray = interpreterProxy->nilObject();
	sqInt value = interpreterProxy->nilObject();
	sqInt next = interpreterProxy->integerObjectOf(start);
	sqInt code;
	size_t i;

	if (out->status == SJ_OK) {
		value = out->value;
		if (dictMode) {
			dropReplacedFixups();
		}
		if (lastText && value == lastText && addFixup(result, 2, value) < 0) {	/* a number text at the root */
			return 0;
		}
		next = interpreterProxy->integerObjectOf((sqInt)out->next0 + 1);
		if (fixupCount) {
			if (!(fixupArray = instantiate(interpreterProxy->classArray(), 2 * fixupCount))) {
				return 0;
			}
			for (i = 0; i < fixupCount; i++) {
				interpreterProxy->storePointerofObjectwithValue((sqInt)(2 * i), fixupArray, fixups[i].container);
				interpreterProxy->storePointerofObjectwithValue((sqInt)(2 * i + 1), fixupArray,
					interpreterProxy->integerObjectOf(fixups[i].index1));
			}
		}
		if (symbolKeys) {
			if (!(keyArray = instantiate(interpreterProxy->classArray(), keyCount))) {
				return 0;
			}
			for (i = 0; i < keyCount; i++) {
				interpreterProxy->storePointerofObjectwithValue((sqInt)i, keyArray, keyStrings[i]);
			}
		}
	}
	if (!(code = bInt64(out->status == SJ_OVER_BUDGET || out->status == SJ_TOO_LARGE ? out->need : out->code))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(out->status));
	interpreterProxy->storePointerofObjectwithValue(1, result, value);
	interpreterProxy->storePointerofObjectwithValue(2, result, next);
	interpreterProxy->storePointerofObjectwithValue(3, result, fixupArray);
	interpreterProxy->storePointerofObjectwithValue(4, result, keyArray);
	interpreterProxy->storePointerofObjectwithValue(5, result, code);
	return result;
}

/*	SimdJSONReader class>>primRead: source from: start to: stop flags: flags
		budget: budget maxUnits: maxUnits objectClass: objClass
		associationClass: assocClassOrNil wideStringClass: wideClass
		hashSeed: seed dictLayout: layout
	<primitive: 'primitiveRead' module: 'SimdJSONPlugin' error: ec>
	Read the JSON value at source[start..stop] (a ByteArray of UTF-8, a
	ByteString or ByteSymbol of Latin-1, a WideString of code points) and
	answer { status. value. nextIndex. fixups. keys. code }.  A JSON error, a
	refused budget and a value over maxUnits are statuses, not failures.
	The primitive fails only on bad arguments, PrimErrNoMemory during the
	build (the VM retries it after GCs, and the engine's parse cache makes
	those calls cost one parse) or PrimErrNoCMemory.  Every argument is read
	again on every call: the GCs of the retries may move the source.  The
	whole contract, statuses and flags included, is in docs/SimdJSON.md,
	section "Primitive contract". */
EXPORT(sqInt)
primitiveRead(void)
{
	sqInt source, start, stop, flags, budget, maxUnits, result, error;
	size_t units;
	int encoding, rc;
	sj_request request;
	sj_outcome out;

	if (interpreterProxy->methodArgumentCount() != 11) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	source = interpreterProxy->stackValue(10);
	if (interpreterProxy->fetchClassOf(source) == interpreterProxy->classByteArray()) {
		encoding = SJ_ENC_UTF8;
		units = (size_t)interpreterProxy->byteSizeOf(source);
	} else if (interpreterProxy->isBytes(source)) {
		encoding = SJ_ENC_LATIN1;
		units = (size_t)interpreterProxy->byteSizeOf(source);
	} else if (interpreterProxy->isWords(source)) {
		encoding = SJ_ENC_UTF32;
		units = (size_t)interpreterProxy->byteSizeOf(source) / 4;
	} else {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	/* isBytes is true of CompiledMethods too, whose bytecodes
	 * firstIndexableField does not answer */
	if (units > 0 && !interpreterProxy->firstIndexableField(source)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!interpreterProxy->isIntegerObject(interpreterProxy->stackValue(9))
	 || !interpreterProxy->isIntegerObject(interpreterProxy->stackValue(8))
	 || !isIntegerAtLeast(interpreterProxy->stackValue(7), 0)
	 || !isIntegerAtLeast(interpreterProxy->stackValue(6), -1)
	 || !isIntegerAtLeast(interpreterProxy->stackValue(5), -1)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = interpreterProxy->integerValueOf(interpreterProxy->stackValue(9));
	stop = interpreterProxy->integerValueOf(interpreterProxy->stackValue(8));
	flags = interpreterProxy->integerValueOf(interpreterProxy->stackValue(7));
	budget = interpreterProxy->integerValueOf(interpreterProxy->stackValue(6));
	maxUnits = interpreterProxy->integerValueOf(interpreterProxy->stackValue(5));
	if (start < 1 || start > stop + 1 || (size_t)stop > units) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	if (flags > (SJ_F_NEXT | SJ_F_EXACT | SJ_F_DICT | SJ_F_SYMKEYS)
	 || (flags & (SJ_F_DICT | SJ_F_SYMKEYS)) == (SJ_F_DICT | SJ_F_SYMKEYS)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	dictMode = (flags & SJ_F_DICT) != 0;
	symbolKeys = (flags & SJ_F_SYMKEYS) != 0;
	if ((error = readClassArguments())) {
		return interpreterProxy->primitiveFailFor(error);
	}

	if (!(result = instantiate(interpreterProxy->classArray(), 6))) {
		return 0;
	}
	builder.nilOop = interpreterProxy->nilObject();
	builder.trueOop = interpreterProxy->trueObject();
	builder.falseOop = interpreterProxy->falseObject();
	lastText = 0;
	if (symbolKeys && !(keySet = sj_keyset_new(64))) {
		return interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
	}
	request.in.base = interpreterProxy->firstIndexableField(source);
	request.in.units = units;
	request.in.encoding = encoding;
	request.in.start0 = (size_t)start - 1;
	request.in.stop0 = (size_t)stop;
	request.flags = (unsigned)flags;
	request.budget = budget;
	request.maxUnits = maxUnits;
	memset(&out, 0, sizeof out);
	rc = sj_read(&request, &builder, &out);
	if (rc == SJ_RC_DONE) {
		result = fillResult(result, &out, start);
	} else if (rc == SJ_RC_NO_C_MEMORY) {
		interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
	} else {
		noMemory();	/* a builder call failed, and set the code */
	}
	freeCallState();
	lastText = 0;
	if (rc != SJ_RC_DONE || !result) {
		return 0;	/* leave the stack alone */
	}
	return interpreterProxy->methodReturnValue(result);
}

/* Store oop at the 0-based index of array, unless an allocation failed */
static int
put(sqInt array, size_t index0, sj_oop oop)
{
	if (!oop) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue((sqInt)index0, array, oop);
	return 1;
}

/*	SimdJSONReader class>>primInfo
	<primitive: 'primitiveInfo' module: 'SimdJSONPlugin' error: ec>
	Answer { protocol. pluginVersion. simdjsonVersion. implementation.
	platform. parserCapacity. cachedBytes. stats }, where stats holds the
	SJ_NUM_STATS counters of sj_info. */
EXPORT(sqInt)
primitiveInfo(void)
{
	sj_info info;
	sqInt result, stats;
	size_t i;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	memset(&info, 0, sizeof info);
	sj_get_info(&info);
	info.implementation[sizeof info.implementation - 1] = 0;
	if (!(result = instantiate(interpreterProxy->classArray(), 8))
	 || !(stats = instantiate(interpreterProxy->classArray(), SJ_NUM_STATS))) {
		return 0;
	}
	for (i = 0; i < SJ_NUM_STATS; i++) {
		if (!put(stats, i, bInt64(info.stats[i]))) {
			return 0;
		}
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(SJ_PROTOCOL));
	if (!put(result, 1, byteString(SJ_PLUGIN_VERSION, strlen(SJ_PLUGIN_VERSION)))
	 || !put(result, 2, byteString(SJ_SIMDJSON_VERSION, strlen(SJ_SIMDJSON_VERSION)))
	 || !put(result, 3, byteString(info.implementation, strlen(info.implementation)))
	 || !put(result, 4, info.emscripten ? byteString("emscripten", 10) : byteString("native", 6))
	 || !put(result, 5, bInt64(info.capacity))
	 || !put(result, 6, bInt64(info.cachedBytes))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(7, result, stats);
	return interpreterProxy->methodReturnValue(result);
}

/*	SimdJSONReader class>>primErrorMessage: code
	<primitive: 'primitiveErrorMessage' module: 'SimdJSONPlugin' error: ec>
	Answer the text of a simdjson error code, or of the plugin's codes 1001
	and 1002, or nil. */
EXPORT(sqInt)
primitiveErrorMessage(void)
{
	sqInt code, oop;
	const char *message = NULL;

	if (interpreterProxy->methodArgumentCount() != 1) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	code = interpreterProxy->stackValue(0);
	if (!interpreterProxy->isIntegerObject(code)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	code = interpreterProxy->integerValueOf(code);
	if (code >= INT32_MIN && code <= INT32_MAX) {
		message = sj_error_message((int)code);
	}
	if (!message) {
		return interpreterProxy->methodReturnValue(interpreterProxy->nilObject());
	}
	if (!(oop = byteString(message, strlen(message)))) {
		return 0;
	}
	return interpreterProxy->methodReturnValue(oop);
}

/*	SimdJSONReader class>>primRelease: freeParser
	<primitive: 'primitiveRelease' module: 'SimdJSONPlugin' error: ec>
	Drop the engine's parse cache; with true also delete the parser and free
	the input buffer.  Answer nil. */
EXPORT(sqInt)
primitiveRelease(void)
{
	sqInt freeParser;

	if (interpreterProxy->methodArgumentCount() != 1) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	freeParser = interpreterProxy->stackValue(0);
	if (freeParser != interpreterProxy->trueObject() && freeParser != interpreterProxy->falseObject()) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	sj_release(freeParser == interpreterProxy->trueObject());
	return interpreterProxy->methodReturnValue(interpreterProxy->nilObject());
}


/*** The walk of primitiveWrite ***/

/* An object the walk leaves to the image: its JSON goes at byte `at` of
 * the output, written at the indentation level `level`. */
typedef struct { sqInt object; size_t at; sqInt level; } Hole;

enum { SJW_PRETTY = 1, SJW_ASCII_ONLY = 2, SJW_BYTES = 4, SJW_NON_FINITE_NULL = 8 };
enum { SJW_OK = 0, SJW_OVER_BUDGET = 1, SJW_ERROR = 2, SJW_TOO_LARGE = 3 };
enum { SJW_ERR_KEY = 1, SJW_ERR_NON_FINITE = 2, SJW_ERR_DEPTH = 3, SJW_ERR_CODE_POINT = 4 };
enum { SJW_C_OOM = -1, SJW_FAILED = -2, SJW_OVER = -3 };	/* walk answers besides 0 */

static sj_out output;
static Hole *holes;
static size_t holeCount, holeCapacity;
static char *newLine;
static size_t newLineSize;
static int writeFlags, maxDepth, errorCode;
static int64_t maxBytes;
static sqInt culprit;
static sqInt byteSymbolClass, wideSymbolClass, dictionaryClass, writerAssociationClass, orderedCollectionClass;
static int ocArrayIndex, ocFirstIndex, ocLastIndex;

static void
freeWriteState(void)
{
	sj_out_free(&output);
	free(holes);
	holes = NULL;
	holeCount = holeCapacity = 0;
	free(newLine);
	newLine = NULL;
	newLineSize = 0;
}

static int
fail(int code, sqInt object)
{
	errorCode = code;
	culprit = object;
	return SJW_FAILED;
}

/* The appenders answer -1 on C OOM; maxBytes is checked once per value */
static int
emit(const char *s, size_t n)
{
	return sj_out_bytes(&output, s, n) ? SJW_C_OOM : 0;
}

static int
newlineIndent(sqInt level)
{
	sqInt i;

	if (!(writeFlags & SJW_PRETTY)) {
		return 0;
	}
	if (emit(newLine, newLineSize)) {
		return SJW_C_OOM;
	}
	for (i = 0; i < level; i++) {
		if (sj_out_char(&output, '\t')) {
			return SJW_C_OOM;
		}
	}
	return 0;
}

static int
addHole(sqInt object, sqInt level)
{
	if (holeCount == holeCapacity) {
		size_t capacity = holeCapacity ? 2 * holeCapacity : 16;
		Hole *grown = realloc(holes, capacity * sizeof(Hole));

		if (!grown) {
			return SJW_C_OOM;
		}
		holes = grown;
		holeCapacity = capacity;
	}
	holes[holeCount].object = object;
	holes[holeCount].at = output.n;
	holes[holeCount].level = level;
	holeCount++;
	return 0;
}

/* The object a reference stands for: become: leaves forwarders, as when a
 * ByteString that gets a wide character becomes a WideString, and the
 * proxy has no call for them.  A Spur forwarder has the class index 8 in
 * the low 22 bits of its 64-bit header and its referent in its first slot
 * (SpurMemoryManager>>followForwarded:). */
static sqInt
follow(sqInt oop)
{
	while (!interpreterProxy->isImmediate(oop) && (*(const uint64_t *)(uintptr_t)oop & 0x3FFFFF) == 8) {
		oop = interpreterProxy->fetchPointerofObject(0, oop);
	}
	return oop;
}

static int
isStringClass(sqInt aClass)
{
	return aClass == interpreterProxy->classString() || aClass == byteSymbolClass
		|| aClass == wideStringClass || aClass == wideSymbolClass;
}

static int
writeString(sqInt string, sqInt aClass)
{
	int rc;

	if (aClass == interpreterProxy->classString() || aClass == byteSymbolClass) {
		rc = sj_out_string(&output, interpreterProxy->firstIndexableField(string), SJ_ENC_LATIN1,
			(size_t)interpreterProxy->byteSizeOf(string), writeFlags & SJW_ASCII_ONLY);
	} else {
		rc = sj_out_string(&output, interpreterProxy->firstIndexableField(string), SJ_ENC_UTF32,
			(size_t)interpreterProxy->byteSizeOf(string) / 4, writeFlags & SJW_ASCII_ONLY);
	}
	return rc == -2 ? fail(SJW_ERR_CODE_POINT, string) : rc ? SJW_C_OOM : 0;
}

static int writeValue(sqInt value, sqInt level);

/* NeoJSONWriter>>writeListStreamingDo: and writeMapStreamingDo: with
 * NeoJSONStreamingWriter: each element after a newline and the indentation
 * of level + 1 (pretty), separated by commas, then a newline and the
 * indentation of level, or a space when there was none */
static int
closeContainer(size_t count, sqInt level, char close)
{
	int rc;

	if (count) {
		if ((rc = newlineIndent(level))) {
			return rc;
		}
	} else if ((writeFlags & SJW_PRETTY) && sj_out_char(&output, ' ')) {
		return SJW_C_OOM;
	}
	return sj_out_char(&output, close) ? SJW_C_OOM : 0;
}

static int
separate(size_t index, sqInt level)
{
	if (index && sj_out_char(&output, ',')) {
		return SJW_C_OOM;
	}
	return newlineIndent(level + 1);
}

static int
writeElements(sqInt array, sqInt first0, sqInt stop0, sqInt level)
{
	sqInt i;
	int rc;

	if (sj_out_char(&output, '[')) {
		return SJW_C_OOM;
	}
	for (i = first0; i < stop0; i++) {
		if ((rc = separate((size_t)(i - first0), level))
		 || (rc = writeValue(follow(interpreterProxy->fetchPointerofObject(i, array)), level + 1))) {
			return rc;
		}
	}
	return closeContainer((size_t)(stop0 - first0), level, ']');
}

/* A Dictionary whose array is an Array of nils and Associations, in the
 * order of keysAndValuesDo:; else the image writes it (answers 1). */
static int
writeDictionary(sqInt dictionary, sqInt level)
{
	sqInt array = follow(interpreterProxy->fetchPointerofObject(arrayIndex, dictionary));
	sqInt nilOop = interpreterProxy->nilObject();
	sqInt size, i, element, key;
	size_t count = 0;
	int rc;

	if (interpreterProxy->isImmediate(array) || interpreterProxy->fetchClassOf(array) != interpreterProxy->classArray()) {
		return 1;
	}
	size = interpreterProxy->slotSizeOf(array);
	for (i = 0; i < size; i++) {
		element = follow(interpreterProxy->fetchPointerofObject(i, array));
		if (element != nilOop
		 && (interpreterProxy->isImmediate(element) || interpreterProxy->fetchClassOf(element) != writerAssociationClass)) {
			return 1;
		}
	}
	if (sj_out_char(&output, '{')) {
		return SJW_C_OOM;
	}
	for (i = 0; i < size; i++) {
		element = follow(interpreterProxy->fetchPointerofObject(i, array));
		if (element == nilOop) {
			continue;
		}
		key = follow(interpreterProxy->fetchPointerofObject(keyIndex, element));
		if (interpreterProxy->isImmediate(key) || !isStringClass(interpreterProxy->fetchClassOf(key))) {
			return fail(SJW_ERR_KEY, key);
		}
		if ((rc = separate(count++, level))
		 || (rc = writeString(key, interpreterProxy->fetchClassOf(key)))
		 || ((writeFlags & SJW_PRETTY) && sj_out_char(&output, ' '))
		 || sj_out_char(&output, ':')
		 || ((writeFlags & SJW_PRETTY) && sj_out_char(&output, ' '))) {
			return rc ? rc : SJW_C_OOM;
		}
		if ((rc = writeValue(follow(interpreterProxy->fetchPointerofObject(valueIndex, element)), level + 1))) {
			return rc;
		}
	}
	return closeContainer(count, level, '}');
}

static int
writeOrderedCollection(sqInt collection, sqInt level)
{
	sqInt array = follow(interpreterProxy->fetchPointerofObject(ocArrayIndex, collection));
	sqInt first = interpreterProxy->fetchPointerofObject(ocFirstIndex, collection);
	sqInt last = interpreterProxy->fetchPointerofObject(ocLastIndex, collection);

	if (interpreterProxy->isImmediate(array) || interpreterProxy->fetchClassOf(array) != interpreterProxy->classArray()
	 || !interpreterProxy->isIntegerObject(first) || !interpreterProxy->isIntegerObject(last)) {
		return 1;
	}
	first = interpreterProxy->integerValueOf(first);
	last = interpreterProxy->integerValueOf(last);
	if (first < 1 || last < first - 1 || last > interpreterProxy->slotSizeOf(array)) {
		return 1;
	}
	return writeElements(array, first - 1, last, level);
}

/* NeoJSONWriter>>nextPut: on the values whose class the walk knows
 * exactly: nil, Booleans, Integers, Floats, the four string classes, Array,
 * OrderedCollection and Dictionary, through forwarders.  Any other object
 * (a Fraction, a Set, an instance of a subclass) is a hole. */
static int
writeValue(sqInt value, sqInt level)
{
	char text[SJ_DOUBLE_CHARS];
	sqInt aClass;
	size_t n;
	double d;
	int rc;

	if (maxBytes >= 0 && output.n > (uint64_t)maxBytes) {
		return SJW_OVER;
	}
	if (value == interpreterProxy->nilObject()) {
		return emit("null", 4);
	}
	if (value == interpreterProxy->trueObject()) {
		return emit("true", 4);
	}
	if (value == interpreterProxy->falseObject()) {
		return emit("false", 5);
	}
	if (interpreterProxy->isIntegerObject(value)) {
		return sj_out_int64(&output, interpreterProxy->integerValueOf(value)) ? SJW_C_OOM : 0;
	}
	if (interpreterProxy->isFloatObject(value)) {
		d = interpreterProxy->floatValueOf(value);
		if (d - d != 0) {	/* NaN or an infinity */
			return writeFlags & SJW_NON_FINITE_NULL ? emit("null", 4) : fail(SJW_ERR_NON_FINITE, value);
		}
		if (!(n = sj_print_double(d, text))) {
			return SJW_C_OOM;
		}
		return emit(text, n);
	}
	if (interpreterProxy->isImmediate(value)) {
		return addHole(value, level);
	}
	if (level > maxDepth) {
		return fail(SJW_ERR_DEPTH, value);
	}
	aClass = interpreterProxy->fetchClassOf(value);
	if (isStringClass(aClass)) {
		return writeString(value, aClass);
	}
	if (aClass == interpreterProxy->classLargePositiveInteger() || aClass == interpreterProxy->classLargeNegativeInteger()) {
		return sj_out_magnitude(&output, interpreterProxy->firstIndexableField(value),
			(size_t)interpreterProxy->byteSizeOf(value), aClass == interpreterProxy->classLargeNegativeInteger())
			? SJW_C_OOM : 0;
	}
	if (aClass == interpreterProxy->classArray()) {
		return writeElements(value, 0, interpreterProxy->slotSizeOf(value), level);
	}
	rc = 1;
	if (aClass == dictionaryClass) {
		rc = writeDictionary(value, level);
	} else if (aClass == orderedCollectionClass) {
		rc = writeOrderedCollection(value, level);
	}
	return rc == 1 ? addHole(value, level) : rc;
}

/* The classes argument of primitiveWrite, { ByteSymbol. WideString.
 * WideSymbol. Dictionary. Association. OrderedCollection }, and its layout:
 * the 4-bit 0-based slots of Dictionary tally and array, Association key
 * and value (dictLayout), then OrderedCollection array, firstIndex and
 * lastIndex. */
static sqInt
writeClassArguments(sqInt classes, sqInt layoutOop)
{
	sqInt format, layout, i;

	if (interpreterProxy->isImmediate(classes)
	 || interpreterProxy->fetchClassOf(classes) != interpreterProxy->classArray()
	 || interpreterProxy->slotSizeOf(classes) != 6
	 || !isIntegerAtLeast(layoutOop, 0)) {
		return PrimErrBadArgument;
	}
	for (i = 0; i < 6; i++) {
		if (classFormat(interpreterProxy->fetchPointerofObject(i, classes)) < 0) {
			return PrimErrBadArgument;
		}
	}
	byteSymbolClass = interpreterProxy->fetchPointerofObject(0, classes);
	wideStringClass = interpreterProxy->fetchPointerofObject(1, classes);
	wideSymbolClass = interpreterProxy->fetchPointerofObject(2, classes);
	dictionaryClass = interpreterProxy->fetchPointerofObject(3, classes);
	writerAssociationClass = interpreterProxy->fetchPointerofObject(4, classes);
	orderedCollectionClass = interpreterProxy->fetchPointerofObject(5, classes);
	layout = interpreterProxy->integerValueOf(layoutOop);
	tallyIndex = (int)(layout & 15);
	arrayIndex = (int)((layout >> 4) & 15);
	keyIndex = (int)((layout >> 8) & 15);
	valueIndex = (int)((layout >> 12) & 15);
	ocArrayIndex = (int)((layout >> 16) & 15);
	ocFirstIndex = (int)((layout >> 20) & 15);
	ocLastIndex = (int)((layout >> 24) & 15);
	if (layout > 0xFFFFFFF) {
		return PrimErrBadArgument;
	}
	format = classFormat(byteSymbolClass);
	if (instSpecOf(format) < 16 || fixedFieldsOf(format) != 0) {
		return PrimErrBadArgument;
	}
	format = classFormat(wideStringClass);
	if (instSpecOf(format) != firstLongFormat || fixedFieldsOf(format) != 0) {
		return PrimErrBadArgument;
	}
	format = classFormat(wideSymbolClass);
	if (instSpecOf(format) != firstLongFormat || fixedFieldsOf(format) != 0) {
		return PrimErrBadArgument;
	}
	format = classFormat(dictionaryClass);
	if (instSpecOf(format) != nonIndexablePointerFormat
	 || fixedFieldsOf(format) <= (tallyIndex > arrayIndex ? tallyIndex : arrayIndex)) {
		return PrimErrBadArgument;
	}
	format = classFormat(writerAssociationClass);
	if (instSpecOf(format) != nonIndexablePointerFormat
	 || fixedFieldsOf(format) <= (keyIndex > valueIndex ? keyIndex : valueIndex)) {
		return PrimErrBadArgument;
	}
	format = classFormat(orderedCollectionClass);
	if (instSpecOf(format) != nonIndexablePointerFormat
	 || fixedFieldsOf(format) <= ocArrayIndex || fixedFieldsOf(format) <= ocFirstIndex
	 || fixedFieldsOf(format) <= ocLastIndex) {
		return PrimErrBadArgument;
	}
	return 0;
}

/* The output: a ByteArray of the UTF-8 bytes, or the narrowest String of
 * their characters; then the holes, { position. object. level } flat, where
 * position counts the units of the output before the hole.  Answers 0 when
 * an allocation failed (the VM retries after a GC). */
static sqInt
writeOutput(void)
{
	sqInt oop;

	if (writeFlags & SJW_BYTES) {
		if ((oop = instantiate(interpreterProxy->classByteArray(), output.n)) && output.n) {
			memcpy(interpreterProxy->firstIndexableField(oop), output.p, output.n);
		}
		return oop;
	}
	return bString(output.p, output.n);
}

static sqInt
writeHoles(void)
{
	sqInt array;
	size_t i, units = 0, from = 0;

	if (!holeCount) {
		return interpreterProxy->nilObject();
	}
	if (!(array = instantiate(interpreterProxy->classArray(), 3 * holeCount))) {
		return 0;
	}
	for (i = 0; i < holeCount; i++) {
		units += writeFlags & SJW_BYTES ? holes[i].at - from : sj_utf8_chars(output.p + from, holes[i].at - from);
		from = holes[i].at;
		interpreterProxy->storePointerofObjectwithValue((sqInt)(3 * i), array, interpreterProxy->integerObjectOf((sqInt)units));
		interpreterProxy->storePointerofObjectwithValue((sqInt)(3 * i + 1), array, holes[i].object);
		interpreterProxy->storePointerofObjectwithValue((sqInt)(3 * i + 2), array, interpreterProxy->integerObjectOf(holes[i].level));
	}
	return array;
}


/*	SimdJSONWriter class>>primWrite: value flags: flags newLine: aString
		maxDepth: depth maxBytes: maxBytes classes: classes layout: layout
	<primitive: 'primitiveWrite' module: 'SimdJSONPlugin' error: ec>
	Write the JSON of value as NeoJSONWriter writes it, and answer { status.
	output. holes. code. culprit }.  The walk reads objects and allocates
	nothing; the output, a ByteArray of UTF-8 (flag BYTES) or the narrowest
	String, is allocated at the end.  Status OK answers it with the holes
	(nil, or { position. object. level } flat): the objects whose class the
	walk does not know, left for the image to write at those positions.
	ERROR answers a code and the culprit (1 a key that is not a string, 2 a
	NaN or an infinity, 3 a value deeper than depth, 4 a code point beyond
	U+10FFFF), OVER_BUDGET the bytes the output needs when its allocation
	failed, TOO_LARGE an output over maxBytes (-1 none).  The primitive fails
	only on bad arguments, PrimErrNoMemory on the small objects of the
	answer, and PrimErrNoCMemory. */
EXPORT(sqInt)
primitiveWrite(void)
{
	sqInt flags, depth, bytes, lineEnd, result, out, holeArray, error, need;
	int rc;

	if (interpreterProxy->methodArgumentCount() != 7) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	lineEnd = interpreterProxy->stackValue(4);
	if (!isIntegerAtLeast(interpreterProxy->stackValue(5), 0)
	 || !isIntegerAtLeast(interpreterProxy->stackValue(3), 0)
	 || !isIntegerAtLeast(interpreterProxy->stackValue(2), -1)
	 || interpreterProxy->isImmediate(lineEnd)
	 || interpreterProxy->fetchClassOf(lineEnd) != interpreterProxy->classString()) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	flags = interpreterProxy->integerValueOf(interpreterProxy->stackValue(5));
	depth = interpreterProxy->integerValueOf(interpreterProxy->stackValue(3));
	bytes = interpreterProxy->integerValueOf(interpreterProxy->stackValue(2));
	if (flags > (SJW_PRETTY | SJW_ASCII_ONLY | SJW_BYTES | SJW_NON_FINITE_NULL) || depth > 100000) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if ((error = writeClassArguments(interpreterProxy->stackValue(1), interpreterProxy->stackValue(0)))) {
		return interpreterProxy->primitiveFailFor(error);
	}
	if (!(result = instantiate(interpreterProxy->classArray(), 5))) {
		return 0;
	}
	writeFlags = (int)flags;
	maxDepth = (int)depth;
	maxBytes = bytes;
	errorCode = 0;
	culprit = interpreterProxy->nilObject();
	newLineSize = (size_t)interpreterProxy->byteSizeOf(lineEnd);
	if (!(newLine = malloc(newLineSize + 1))) {
		return interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
	}
	memcpy(newLine, interpreterProxy->firstIndexableField(lineEnd), newLineSize);

	rc = writeValue(follow(interpreterProxy->stackValue(6)), 0);
	if (rc == 0 && maxBytes >= 0 && output.n > (uint64_t)maxBytes) {
		rc = SJW_OVER;
	}
	if (rc == SJW_C_OOM) {
		freeWriteState();
		return interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
	}
	out = holeArray = interpreterProxy->nilObject();
	need = 0;
	if (rc == 0) {
		if (!(out = writeOutput())) {
			/* the output did not fit: the image grows memory by its size */
			interpreterProxy->primitiveFailFor(0);
			need = (sqInt)(writeFlags & SJW_BYTES ? sj_spur_bytes(output.n) : sj_spur_words32(output.n));
			out = interpreterProxy->nilObject();
			rc = SJW_OVER_BUDGET;
		} else if (!(holeArray = writeHoles())) {
			freeWriteState();
			return 0;
		}
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(
		rc == 0 ? SJW_OK : rc == SJW_FAILED ? SJW_ERROR : rc == SJW_OVER ? SJW_TOO_LARGE : SJW_OVER_BUDGET));
	interpreterProxy->storePointerofObjectwithValue(1, result, out);
	interpreterProxy->storePointerofObjectwithValue(2, result, holeArray);
	interpreterProxy->storePointerofObjectwithValue(3, result, interpreterProxy->integerObjectOf(
		rc == SJW_OVER_BUDGET ? need : errorCode));
	interpreterProxy->storePointerofObjectwithValue(4, result, culprit);
	freeWriteState();
	return interpreterProxy->methodReturnValue(result);
}

#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "SimdJSONPlugin";
void* SimdJSONPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveErrorMessage\000\000", (void*)primitiveErrorMessage},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveRead\000\000", (void*)primitiveRead},
	{(void*)_m, "primitiveRelease\000\000", (void*)primitiveRelease},
	{(void*)_m, "primitiveWrite\000\000", (void*)primitiveWrite},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveErrorMessageAccessorDepth = 0;
EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveReadAccessorDepth = 0;
EXPORT(signed char) primitiveReleaseAccessorDepth = 0;
EXPORT(signed char) primitiveWriteAccessorDepth = 0;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
