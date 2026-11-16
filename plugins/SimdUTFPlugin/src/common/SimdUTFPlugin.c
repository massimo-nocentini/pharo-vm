/* SimdUTFPlugin.c -- the VM side of the SimdUTFPlugin: the module functions
 * and the seven primitives of the package SimdUTF (the submodule
 * plugins/SimdUTFPlugin/simdutf.st), which call the engine
 * (src/cxx/suEngine.cpp) through suEngine.h.  This file never includes
 * simdutf.h, and the engine never includes a VM header.  docs/SimdUTF.md,
 * section "Primitive contract", is the contract.
 *
 * Natively it is the shared library libSimdUTFPlugin.so, whose module
 * functions, primitives and primitiveXAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through SimdUTFPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).
 *
 * The primitives check their arguments in one order (the contract's
 * checks (1) to (7)): the argument count; the source is a byte object or a
 * word object; every number is a SmallInteger; the flags or options, and
 * that the kind of the source fits them; the range of the source; the
 * class argument; the destination.  A failing primitive leaves the stack
 * alone.  Invalid input is an answer, never a failure: a negative
 * SmallInteger, the status word -(status + 4*code + 4096*payload).
 *
 * Allocation: at most one object per call, at its exact size, after every
 * check and every validation that can fail (base64 decoding allocates
 * before it decodes: the size is exact for any input that decodes), only
 * through instantiateClassindexableSize (allocateResult), which falls back
 * to old space when eden is full; integers only through integerObjectOf.
 * When the allocation fails for want of memory the primitive does not fail
 * (the VM would only collect garbage and fail again): it answers
 * OVER_BUDGET with the bytes the object needs, and the image grows memory.
 * Spur never collects garbage inside an allocation of the proxy, so the
 * address of the source that the engine holds stays valid across it, and
 * the oops held across the allocations of primitiveInfo stay valid; every
 * pointer store goes through storePointerofObjectwithValue, since objects
 * allocated after eden filled up are old.  No malloc: nothing outlives a
 * call.
 */

#include <stdint.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "suEngine.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

_Static_assert(sizeof(sqInt) == 8, "SimdUTFPlugin: the status word needs 64-bit SmallIntegers (docs/SimdUTF.md)");

/* Spur's class format: instance specification and number of named slots */
#define instSpecOf(format) (((format) >> 16) & 0x1F)
#define fixedFieldsOf(format) ((format) & 0xFFFF)
#define firstLongFormat 10

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"SimdUTFPlugin (i)";
#else
	"SimdUTFPlugin (e)";
#endif

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitiveBase64Decode(void);
EXPORT(sqInt) primitiveBase64Encode(void);
EXPORT(sqInt) primitiveDecode(void);
EXPORT(sqInt) primitiveEncode(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveMeasure(void);
EXPORT(sqInt) primitiveValidate(void);
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
	su_init();
	return 1;
}

EXPORT(sqInt)
shutdownModule(void)
{
	/* nothing to release: the plugin keeps no state */
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

/* A source or a destination: its units are bytes, or 32-bit words when
 * words is 1 */
typedef struct {
	sqInt oop;
	int words;
	size_t units;
	void *base;
} Units;

/* Whether oop is a byte object or (when wordsTaken) a word object, into
 * *u.  isBytes is true of CompiledMethods too, whose bytecodes
 * firstIndexableField does not answer: they are refused. */
static int
unitsOf(sqInt oop, int bytesTaken, int wordsTaken, Units *u)
{
	u->oop = oop;
	if (interpreterProxy->isBytes(oop)) {
		u->words = 0;
		u->units = (size_t)interpreterProxy->byteSizeOf(oop);
	} else if (interpreterProxy->isWords(oop)) {
		u->words = 1;
		u->units = (size_t)interpreterProxy->byteSizeOf(oop) / 4;
	} else {
		return 0;
	}
	if (u->words ? !wordsTaken : !bytesTaken) {
		return 0;
	}
	u->base = interpreterProxy->firstIndexableField(oop);
	return u->units == 0 || u->base != NULL;
}

/* Whether the stack values at the given offsets are all SmallIntegers */
static int
areSmallIntegers(const int *offsets, int count)
{
	int i;

	for (i = 0; i < count; i++) {
		if (!interpreterProxy->isIntegerObject(interpreterProxy->stackValue(offsets[i]))) {
			return 0;
		}
	}
	return 1;
}

static sqInt
integerAt(int offset)
{
	return interpreterProxy->integerValueOf(interpreterProxy->stackValue(offset));
}

/* Whether start..stop (1-based, inclusive; empty when start = stop + 1) is
 * a range of the units of the source */
static int
isRange(sqInt start, sqInt stop, const Units *source)
{
	return start >= 1 && start <= stop + 1 && stop <= (sqInt)source->units;
}

/* The encodings of transcoding: UTF-8, UTF-16LE, UTF-16BE */
static int
isUnicode(sqInt encoding)
{
	return encoding == SU_ENC_UTF8 || encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

/* Spur's format word of aClass (its third slot), or -1 when aClass does not
 * look like a class (SimdJSONPlugin.c) */
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

/* Whether aClass can be the class of a decoded wide string: a class (its
 * class is a metaclass, so not an Array posing as one), of 32-bit words
 * without named slots, and not BoxedFloat64, whose instances
 * instantiateClassindexableSize makes of 2 words only. */
static int
isWideStringClass(sqInt aClass)
{
	sqInt format = classFormat(aClass);

	return format >= 0
		&& instSpecOf(format) == firstLongFormat
		&& fixedFieldsOf(format) == 0
		&& interpreterProxy->fetchClassOf(interpreterProxy->fetchClassOf(aClass))
			== interpreterProxy->fetchClassOf(interpreterProxy->fetchClassOf(interpreterProxy->classString()))
		&& aClass != interpreterProxy->classFloat();
}

/* Check (7), the destination at the stack offset destOffset with its index
 * just above it: nil (allocate mode: *into = 0), or a byte object (when
 * bytesTaken) or a word object (when wordsTaken), not the source, mutable,
 * with an index from 1 to its units + 1, and an even index - 1 when
 * evenIndex (a UTF-16 target).  Answers 0 or the PrimErr code. */
static sqInt
destinationAt(int destOffset, const Units *source, int bytesTaken, int wordsTaken, int evenIndex,
	Units *dest, sqInt *index, int *into)
{
	sqInt oop = interpreterProxy->stackValue(destOffset);

	*into = 0;
	if (oop == interpreterProxy->nilObject()) {
		return 0;
	}
	if (!unitsOf(oop, bytesTaken, wordsTaken, dest) || oop == source->oop) {
		return PrimErrBadArgument;
	}
	if (interpreterProxy->isOopImmutable(oop)) {
		return PrimErrNoModification;
	}
	*index = integerAt(destOffset - 1);
	if (*index < 1 || *index > (sqInt)dest->units + 1) {
		return PrimErrBadIndex;
	}
	if (evenIndex && (*index - 1) % 2 != 0) {
		return PrimErrBadIndex;
	}
	*into = 1;
	return 0;
}

static su_input
inputOf(const Units *source, sqInt start, sqInt stop)
{
	su_input in;

	in.base = source->base;
	in.units = source->units;
	in.words = source->words;
	in.start0 = (size_t)start - 1;
	in.stop0 = (size_t)stop;
	return in;
}


/*** Results ***/

/* The bytes of a Spur object of n slots of 64 bits, header included
 * (sjText.c): an overflow header from 255 slots, and a slot at least */
static uint64_t
spurPointers(uint64_t slots)
{
	return 8 + (slots >= 255 ? 8 : 0) + 8 * (slots ? slots : 1);
}

static uint64_t
spurBytes(uint64_t bytes)
{
	return spurPointers((bytes + 7) / 8);
}

static uint64_t
spurWords32(uint64_t words)
{
	return spurPointers((words + 1) / 2);
}

/* The state of the one allocation of a call in allocate mode */
typedef struct {
	sqInt wideClass;	/* the class of SU_KIND_UTF32 results (primitiveDecode) */
	sqInt oop;		/* the result, once allocated */
	uint64_t need;		/* OVER_BUDGET: the bytes of the object that did not fit */
	int badArgument;	/* the allocation failed for another reason than memory */
} Allocation;

/* The allocator of the engine's sinks: a ByteArray, a ByteString or an
 * instance of the wide string class of units units, and the address of its
 * first unit; NULL when it failed. */
static void *
allocateResult(void *ctx, int kind, size_t units)
{
	static uint64_t none;	/* the address of the first of 0 units */
	Allocation *a = ctx;
	sqInt cls = kind == SU_KIND_UTF32 ? a->wideClass
		: kind == SU_KIND_LATIN1 ? interpreterProxy->classString() : interpreterProxy->classByteArray();

	if (!(a->oop = interpreterProxy->instantiateClassindexableSize(cls, (sqInt)units))) {
		if (interpreterProxy->primitiveFailureCode() != PrimErrNoMemory) {
			/* PrimErrBadReceiver (a class of BoxedFloat64's shape),
			   PrimErrUnsupported (more than 2^40 slots), an error of the
			   class table: a bad argument, never OVER_BUDGET */
			a->badArgument = 1;
			return NULL;
		}
		interpreterProxy->primitiveFailFor(0);	/* OVER_BUDGET is an answer, not a failure */
		a->need = kind == SU_KIND_UTF32 ? spurWords32(units) : spurBytes(units);
		return NULL;
	}
	return units ? interpreterProxy->firstIndexableField(a->oop) : (void *)&none;
}

/* The sink of a call: the destination's room from index on (into), or the
 * one allocation of a */
static su_sink
sinkOf(int into, const Units *dest, sqInt index, int kind, Allocation *a)
{
	static uint64_t none;	/* the room of an empty destination */
	su_sink sink;
	size_t unitBytes = dest->words ? 4 : 1;

	memset(&sink, 0, sizeof sink);
	if (into) {
		sink.base = dest->base ? (char *)dest->base + ((size_t)index - 1) * unitBytes : (void *)&none;
		sink.capacity = dest->units - ((size_t)index - 1);
		sink.kind = kind;
	} else {
		sink.ctx = a;
		sink.allocate = allocateResult;
	}
	return sink;
}

/* The status word -(status + 4*code + 4096*payload), a negative SmallInteger
 * on a 64-bit VM: payload < 2^48 */
static sqInt
answerStatus(int status, int code, uint64_t payload)
{
	return interpreterProxy->methodReturnValue(
		interpreterProxy->integerObjectOf(-(sqInt)((uint64_t)status + 4 * (uint64_t)code + 4096 * payload)));
}

/* The answer of a primitive for the engine's rc and *out.  value is what
 * success answers (the units, a measure); in allocate mode (into = 0, a
 * given) the object allocated instead. */
static sqInt
answer(int rc, const su_outcome *out, int into, const Allocation *a, sqInt value)
{
	switch (rc) {
	case SU_RC_DONE:
		if (out->status == SU_ERROR) {
			return answerStatus(SU_ERROR, out->code, (uint64_t)out->position0 + 1);
		}
		if (a && !into) {
			return a->oop
				? interpreterProxy->methodReturnValue(a->oop)
				: interpreterProxy->primitiveFailFor(PrimErrBadArgument);	/* never */
		}
		return interpreterProxy->methodReturnValue(interpreterProxy->integerObjectOf(value));
	case SU_RC_NO_ROOM:
		if (into) {
			return answerStatus(SU_TOO_LARGE, 0, out->units);
		}
		if (!a || a->badArgument || !a->need) {
			return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		}
		return answerStatus(SU_OVER_BUDGET, 0, a->need);
	case SU_RC_BAD_INDEX:	/* never: the checks above are the engine's */
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	default:
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
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

/*	SimdUTFPrimitives class>>primInfo
	<primitive: 'primitiveInfo' module: 'SimdUTFPlugin' error: ec>
	Answer { protocol. pluginVersion. simdutfVersion. kernel. platform.
	kernels. forced }: the active kernel after the guard of
	SIMDUTF_FORCE_IMPLEMENTATION, 'native' or 'emscripten', an Array of the
	kernels compiled in that the CPU supports, and nil, the honoured value
	of SIMDUTF_FORCE_IMPLEMENTATION or 'refused <value>' (63 bytes at
	most: of a longer value, the first 55). */
EXPORT(sqInt)
primitiveInfo(void)
{
	su_info info;
	sqInt result, kernels, oop;
	int i;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	su_get_info(&info);
	info.simdutf[sizeof info.simdutf - 1] = 0;
	info.implementation[sizeof info.implementation - 1] = 0;
	info.forced[sizeof info.forced - 1] = 0;
	if (info.count < 0 || info.count > (int)(sizeof info.implementations / sizeof info.implementations[0])) {
		info.count = 0;
	}
	if (!(result = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 7))
	 || !(kernels = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), info.count))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(SU_PROTOCOL));
	if (!(oop = byteString(SU_PLUGIN_VERSION))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(1, result, oop);
	if (!(oop = byteString(info.simdutf))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(2, result, oop);
	if (!(oop = byteString(info.implementation))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(3, result, oop);
	if (!(oop = byteString(info.emscripten ? "emscripten" : "native"))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(4, result, oop);
	for (i = 0; i < info.count; i++) {
		info.implementations[i][sizeof info.implementations[i] - 1] = 0;
		if (!(oop = byteString(info.implementations[i]))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(i, kernels, oop);
	}
	interpreterProxy->storePointerofObjectwithValue(5, result, kernels);
	if (info.forced[0]) {
		if (!(oop = byteString(info.forced))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(6, result, oop);
	}	/* else slot 7 stays nil */
	return interpreterProxy->methodReturnValue(result);
}


/*** The conversions ***/

/*	SimdUTFPrimitives class>>encode: aString from: start to: stop flags: flags into: destOrNil at: index
	<primitive: 'primitiveEncode' module: 'SimdUTFPlugin' error: ec>
	Encode aString[start..stop] (a byte object, read as Latin-1, or a word
	object, read as UTF-32) in UTF-8, UTF-16LE or UTF-16BE (flags: the
	encoding, plus SU_F_BOM to write a BOM first).  Answer a new ByteArray,
	or the bytes written into the byte object destOrNil from index on, or
	a status word: ERROR 5 or 6 at a word that is not a Unicode scalar
	value, TOO_LARGE, OVER_BUDGET. */
EXPORT(sqInt)
primitiveEncode(void)
{
	static const int numbers[] = { 4, 3, 2, 0 };
	Units source, dest;
	sqInt start, stop, flags, index = 1, error;
	int into, rc;
	Allocation a = { 0, 0, 0, 0 };
	su_input in;
	su_sink sink;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 6) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(5), 1, 1, &source)
	 || !areSmallIntegers(numbers, 4)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(4);
	stop = integerAt(3);
	flags = integerAt(2);
	if ((flags & ~(sqInt)(SU_F_ENCODING | SU_F_BOM)) != 0 || !isUnicode(flags & SU_F_ENCODING)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	if ((error = destinationAt(1, &source, 1, 0, (flags & SU_F_ENCODING) != SU_ENC_UTF8, &dest, &index, &into))) {
		return interpreterProxy->primitiveFailFor(error);
	}
	in = inputOf(&source, start, stop);
	sink = sinkOf(into, &dest, index, SU_KIND_BYTES, &a);
	memset(&out, 0, sizeof out);
	rc = su_encode(&in, (unsigned)flags, &sink, &out);
	return answer(rc, &out, into, &a, (sqInt)out.units);
}

/*	SimdUTFPrimitives class>>decode: bytes from: start to: stop flags: flags wideStringClass: wideClass into: destOrNil at: index
	<primitive: 'primitiveDecode' module: 'SimdUTFPlugin' error: ec>
	Decode bytes[start..stop] (a byte object) from UTF-8, UTF-16LE or
	UTF-16BE (flags: the encoding, plus SU_F_BOM to skip one leading BOM,
	plus SU_F_WIDE or SU_F_LATIN1 to force the class of the result).
	Answer a new ByteString when every character is at most U+00FF, else a
	new instance of wideClass, or the characters written into destOrNil
	from index on (Latin-1 into a byte object, UTF-32 into a word object),
	or a status word: ERROR 1-6 at the first byte of an invalid sequence,
	1001 at stop for an odd UTF-16 length, 1002 at the first character
	above U+00FF when Latin-1 is required, TOO_LARGE, OVER_BUDGET. */
EXPORT(sqInt)
primitiveDecode(void)
{
	static const int numbers[] = { 5, 4, 3, 0 };
	Units source, dest;
	sqInt start, stop, flags, wideClass, index = 1, error;
	int into, rc;
	Allocation a = { 0, 0, 0, 0 };
	su_input in;
	su_sink sink;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 7) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(6), 1, 1, &source)
	 || !areSmallIntegers(numbers, 4)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(5);
	stop = integerAt(4);
	flags = integerAt(3);
	if ((flags & ~(sqInt)(SU_F_ENCODING | SU_F_BOM | SU_F_WIDE | SU_F_LATIN1)) != 0
	 || (flags & (SU_F_WIDE | SU_F_LATIN1)) == (SU_F_WIDE | SU_F_LATIN1)
	 || !isUnicode(flags & SU_F_ENCODING)
	 || source.words) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)
	 || ((flags & SU_F_ENCODING) != SU_ENC_UTF8 && (start - 1) % 2 != 0)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	wideClass = interpreterProxy->stackValue(2);
	if (!isWideStringClass(wideClass)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if ((error = destinationAt(1, &source, !(flags & SU_F_WIDE), !(flags & SU_F_LATIN1), 0, &dest, &index, &into))) {
		return interpreterProxy->primitiveFailFor(error);
	}
	a.wideClass = wideClass;
	in = inputOf(&source, start, stop);
	sink = sinkOf(into, &dest, index, dest.words ? SU_KIND_UTF32 : SU_KIND_LATIN1, &a);
	memset(&out, 0, sizeof out);
	rc = su_decode(&in, (unsigned)flags, &sink, &out);
	return answer(rc, &out, into, &a, (sqInt)out.units);
}

/*	SimdUTFPrimitives class>>measure: source from: start to: stop flags: flags
	<primitive: 'primitiveMeasure' module: 'SimdUTFPlugin' error: ec>
	Answer what source[start..stop] makes, without making it, or ERROR:
	SU_OP_ENCODED (plus an encoding and SU_F_BOM, a byte or word source):
	the bytes primitiveEncode writes; SU_OP_DECODED (plus an encoding and
	SU_F_BOM, a byte source): characters * 2 + 1 when one is above U+00FF;
	SU_OP_BASE64 (alone, a byte source): the bytes primitiveBase64Decode
	makes of valid input, without validating it. */
EXPORT(sqInt)
primitiveMeasure(void)
{
	static const int numbers[] = { 2, 1, 0 };
	Units source;
	sqInt start, stop, flags, op, encoding;
	int ok, rc;
	su_input in;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 4) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(3), 1, 1, &source)
	 || !areSmallIntegers(numbers, 3)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(2);
	stop = integerAt(1);
	flags = integerAt(0);
	op = flags & SU_F_OP;
	encoding = flags & SU_F_ENCODING;
	if (op == SU_OP_BASE64) {
		ok = flags == SU_OP_BASE64 && !source.words;
	} else {
		ok = (op == SU_OP_ENCODED || op == SU_OP_DECODED)
			&& (flags & ~(sqInt)(SU_F_OP | SU_F_ENCODING | SU_F_BOM)) == 0
			&& isUnicode(encoding)
			&& (op == SU_OP_ENCODED || !source.words);
	}
	if (!ok) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)
	 || (op == SU_OP_DECODED && encoding != SU_ENC_UTF8 && (start - 1) % 2 != 0)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	in = inputOf(&source, start, stop);
	memset(&out, 0, sizeof out);
	rc = su_measure(&in, (unsigned)flags, &out);
	return answer(rc, &out, 1, NULL,
		op == SU_OP_DECODED ? 2 * (sqInt)out.units + (out.kind == SU_KIND_UTF32) : (sqInt)out.units);
}

/*	SimdUTFPrimitives class>>validate: source from: start to: stop flags: flags
	<primitive: 'primitiveValidate' module: 'SimdUTFPlugin' error: ec>
	Answer 0 when source[start..stop] is valid, else ERROR at the first
	invalid unit: SU_OP_VALID plus SU_ENC_UTF8, SU_ENC_UTF16LE or
	SU_ENC_UTF16BE (a byte source), or SU_ENC_UTF32 (a word source, each
	word a Unicode scalar value); SU_OP_ASCII alone (either source): code 5
	at the first unit above 0x7F. */
EXPORT(sqInt)
primitiveValidate(void)
{
	static const int numbers[] = { 2, 1, 0 };
	Units source;
	sqInt start, stop, flags;
	int rc;
	su_input in;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 4) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(3), 1, 1, &source)
	 || !areSmallIntegers(numbers, 3)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(2);
	stop = integerAt(1);
	flags = integerAt(0);
	if (!(flags == SU_OP_ASCII
	   || (flags == (SU_OP_VALID | SU_ENC_UTF32) && source.words)
	   || (isUnicode(flags) && !source.words))) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)
	 || ((flags == SU_ENC_UTF16LE || flags == SU_ENC_UTF16BE) && (start - 1) % 2 != 0)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	in = inputOf(&source, start, stop);
	memset(&out, 0, sizeof out);
	rc = su_validate(&in, (unsigned)flags, &out);
	return answer(rc, &out, 1, NULL, 0);
}

/*	SimdUTFPrimitives class>>base64Encode: bytes from: start to: stop options: options lineLength: lineLength lineEnd: lineEnd into: destOrNil at: index
	<primitive: 'primitiveBase64Encode' module: 'SimdUTFPlugin' error: ec>
	Encode bytes[start..stop] (a byte object) in base64: options 0 standard
	padded, 1 URL unpadded, 2 standard unpadded, 3 URL padded; lineLength 0
	(one line) or a multiple of 4 below 2^30, with a line end (0 LF, 1 CR,
	2 CRLF) after each group of 4 characters that completes a line, the
	last one included (ZnBase64Encoder's rule).  Answer a new ByteString, or
	the characters written into the byte object destOrNil from index on, or
	a status word: TOO_LARGE, OVER_BUDGET. */
EXPORT(sqInt)
primitiveBase64Encode(void)
{
	static const int numbers[] = { 6, 5, 4, 3, 2, 0 };
	Units source, dest;
	sqInt start, stop, options, lineLength, lineEnd, index = 1, error;
	int into, rc;
	Allocation a = { 0, 0, 0, 0 };
	su_input in;
	su_sink sink;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 8) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(7), 1, 1, &source)
	 || !areSmallIntegers(numbers, 6)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(6);
	stop = integerAt(5);
	options = integerAt(4);
	lineLength = integerAt(3);
	lineEnd = integerAt(2);
	if (options < SU_B64_STANDARD || options > SU_B64_URL_PADDED
	 || lineLength < 0 || lineLength % 4 != 0 || lineLength >= (sqInt)SU_MAX_LINE_LENGTH
	 || lineEnd < SU_EOL_LF || lineEnd > SU_EOL_CRLF
	 || source.words) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	if ((error = destinationAt(1, &source, 1, 0, 0, &dest, &index, &into))) {
		return interpreterProxy->primitiveFailFor(error);
	}
	in = inputOf(&source, start, stop);
	sink = sinkOf(into, &dest, index, SU_KIND_LATIN1, &a);
	memset(&out, 0, sizeof out);
	rc = su_base64_encode(&in, (unsigned)options, (size_t)lineLength, (unsigned)lineEnd, &sink, &out);
	return answer(rc, &out, into, &a, (sqInt)out.units);
}

/*	SimdUTFPrimitives class>>base64Decode: source from: start to: stop options: options lastChunk: mode into: destOrNil at: index
	<primitive: 'primitiveBase64Decode' module: 'SimdUTFPlugin' error: ec>
	Decode the base64 of source[start..stop] (a byte object), skipping ASCII
	white space: options 0 standard, 1 URL, 8 either alphabet; mode 0
	loose (padding optional), 1 strict (padding required, the pad bits
	zero).  Answer a new ByteArray, or the bytes written into the byte
	object destOrNil from index on, or a status word: TOO_LARGE (before
	decoding), ERROR 7 at an invalid character, 8 or 9 at the last
	character that is not white space, OVER_BUDGET.  After an ERROR in
	into mode the destination's bytes from index on, as many as the
	measure of the source, are unspecified. */
EXPORT(sqInt)
primitiveBase64Decode(void)
{
	static const int numbers[] = { 5, 4, 3, 2, 0 };
	Units source, dest;
	sqInt start, stop, options, mode, index = 1, error;
	int into, rc;
	Allocation a = { 0, 0, 0, 0 };
	su_input in;
	su_sink sink;
	su_outcome out;

	if (interpreterProxy->methodArgumentCount() != 7) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	if (!unitsOf(interpreterProxy->stackValue(6), 1, 1, &source)
	 || !areSmallIntegers(numbers, 5)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	start = integerAt(5);
	stop = integerAt(4);
	options = integerAt(3);
	mode = integerAt(2);
	if ((options != SU_B64_STANDARD && options != SU_B64_URL && options != SU_B64_ANY)
	 || (mode != SU_CHUNK_LOOSE && mode != SU_CHUNK_STRICT)
	 || source.words) {
		return interpreterProxy->primitiveFailFor(PrimErrBadArgument);
	}
	if (!isRange(start, stop, &source)) {
		return interpreterProxy->primitiveFailFor(PrimErrBadIndex);
	}
	if ((error = destinationAt(1, &source, 1, 0, 0, &dest, &index, &into))) {
		return interpreterProxy->primitiveFailFor(error);
	}
	in = inputOf(&source, start, stop);
	sink = sinkOf(into, &dest, index, SU_KIND_BYTES, &a);
	memset(&out, 0, sizeof out);
	rc = su_base64_decode(&in, (unsigned)options, (unsigned)mode, &sink, &out);
	return answer(rc, &out, into, &a, (sqInt)out.units);
}

#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "SimdUTFPlugin";
void* SimdUTFPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveBase64Decode\000\000", (void*)primitiveBase64Decode},
	{(void*)_m, "primitiveBase64Encode\000\000", (void*)primitiveBase64Encode},
	{(void*)_m, "primitiveDecode\000\000", (void*)primitiveDecode},
	{(void*)_m, "primitiveEncode\000\000", (void*)primitiveEncode},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveMeasure\000\000", (void*)primitiveMeasure},
	{(void*)_m, "primitiveValidate\000\000", (void*)primitiveValidate},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveBase64DecodeAccessorDepth = 0;
EXPORT(signed char) primitiveBase64EncodeAccessorDepth = 0;
EXPORT(signed char) primitiveDecodeAccessorDepth = 0;
EXPORT(signed char) primitiveEncodeAccessorDepth = 0;
EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveMeasureAccessorDepth = 0;
EXPORT(signed char) primitiveValidateAccessorDepth = 0;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
