/* UtilsPlugin.c -- the VM side of the UtilsPlugin: the module functions and
 * the primitives of the package VM-Plugins-Primitives (the submodule
 * plugins/UtilsPlugin/vm-plugins-primitives.st), which call the engine
 * (src/engine/upEngine.c) through upEngine.h.  The primitives, their names
 * and the order of their arguments are those of the UtilsPlugin of the
 * pharo-10 branch, whose image code they keep serving:
 *
 *   primitive_info              the protocol, the version, the platform
 *   primitive_fma               Float>>primitiveTimes:plus:
 *   primitive_strtok_r          String>>strtok:inclusive:
 *   primitive_decasteljau       UtilsPlugin class>>primDeCasteljau:into:at:
 *   primitive_randomknuth_init  UtilsPlugin class>>primKnuthState
 *   primitive_randomknuth_start KnuthRandom>>primStart
 *   primitive_randomknuth_next  KnuthRandom>>primNext:
 *
 * docs/UtilsPlugin.md, section "Primitive contract", is the contract.
 *
 * Natively it is the shared library libUtilsPlugin.so, whose module
 * functions, primitives and primitive_xAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through UtilsPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).
 *
 * The primitives check all their arguments (the argument count, the kinds,
 * read-only, the sizes) before they write anything, and fail with a code
 * (PrimErrBadReceiver, PrimErrBadArgument, PrimErrBadNumArgs,
 * PrimErrNoModification, PrimErrNoMemory, PrimErrNoCMemory): the image then
 * runs its Smalltalk twin of the primitive (UtilsPluginSmalltalk), which
 * answers the same result or signals the error.  A forwarder is not of the
 * kind expected, so it fails too, and the accessor depths make the VM follow
 * the forwarders and retry once.
 *
 * Allocation: only instantiateClassindexableSize, which falls back to old
 * space when eden is full, plus the immediates (SmallIntegers, and the
 * SmallFloat64s of 64-bit VMs): the proxy's floatObjectOf allocates boxed
 * floats in eden only, and aborts the VM when eden is full.  Spur never
 * collects garbage inside an allocation of the proxy, so the oops and the
 * pointers to their bytes stay valid for the whole primitive.  Every pointer
 * store into an object goes through storePointerofObjectwithValue.  When an
 * allocation fails the VM has set PrimErrNoMemory, and retries the primitive
 * after a scavenge and after a full GC: primitive_decasteljau may then have
 * stored some of its points.
 */

#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "upEngine.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

#define UP_PROTOCOL 1
#define UP_PLUGIN_VERSION "UtilsPlugin 1.0"

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"UtilsPlugin (i)";
#else
	"UtilsPlugin (e)";
#endif

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitive_decasteljau(void);
EXPORT(sqInt) primitive_fma(void);
EXPORT(sqInt) primitive_info(void);
EXPORT(sqInt) primitive_randomknuth_init(void);
EXPORT(sqInt) primitive_randomknuth_next(void);
EXPORT(sqInt) primitive_randomknuth_start(void);
EXPORT(sqInt) primitive_strtok_r(void);
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
	return 1;
}

EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return interpreterProxy->majorVersion() == VM_PROXY_MAJOR
	    && interpreterProxy->minorVersion() >= VM_PROXY_MINOR;
}


/*** Objects ***/

static sqInt
fail(sqInt code)
{
	return interpreterProxy->primitiveFailFor(code);
}

/* An allocation, or 0 when it failed: the VM has set PrimErrNoMemory */
static sqInt
instantiate(sqInt aClass, sqInt size)
{
	sqInt oop = interpreterProxy->instantiateClassindexableSize(aClass, size);

	if (!oop && !interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
	}
	return oop;
}

static sqInt
byteString(const char *bytes, size_t length)
{
	sqInt oop = instantiate(interpreterProxy->classString(), (sqInt)length);

	if (oop && length) {
		memcpy(interpreterProxy->firstIndexableField(oop), bytes, length);
	}
	return oop;
}

/* Whether d is in the range of the SmallFloat64 immediates of 64-bit Spur
 * (as the VM's isSmallFloatValue: the exponents of 2^-127 .. 2^128, and
 * +0.0) */
static int
isSmallFloat(double d)
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

/* A Float of the value d, or 0 when the allocation failed */
static sqInt
newFloat(double d)
{
	sqInt oop;

	if (isSmallFloat(d)) {
		return interpreterProxy->floatObjectOf(d);	/* an immediate: no allocation */
	}
	/* classFloat() is BoxedFloat64: two 32-bit slots, in the platform's order */
	if ((oop = instantiate(interpreterProxy->classFloat(), 2))) {
		memcpy(interpreterProxy->firstIndexableField(oop), &d, sizeof d);
	}
	return oop;
}

/* The value of a Float or a SmallInteger as a double (as asFloat): 1, or 0
 * for anything else */
static int
numberValue(sqInt oop, double *value)
{
	if (interpreterProxy->isIntegerObject(oop)) {
		*value = (double)interpreterProxy->integerValueOf(oop);
		return 1;
	}
	if (interpreterProxy->isFloatObject(oop)) {
		*value = interpreterProxy->floatValueOf(oop);
		return 1;
	}
	return 0;
}

static int
isBooleanAt(int offset, int *value)
{
	sqInt oop = interpreterProxy->stackValue(offset);

	if (oop == interpreterProxy->trueObject()) {
		*value = 1;
		return 1;
	}
	if (oop == interpreterProxy->falseObject()) {
		*value = 0;
		return 1;
	}
	return 0;
}


/*** primitive_info ***/

/*	UtilsPlugin class>>primInfo
	<primitive: 'primitive_info' module: 'UtilsPlugin' error: ec>
	Answer { protocol. 'UtilsPlugin <version>'. platform } where platform is
	'native' or 'emscripten'. */
EXPORT(sqInt)
primitive_info(void)
{
	sqInt result, oop;
#ifdef __EMSCRIPTEN__
	static const char platform[] = "emscripten";
#else
	static const char platform[] = "native";
#endif

	if (interpreterProxy->methodArgumentCount() != 0) {
		return fail(PrimErrBadNumArgs);
	}
	if (!(result = instantiate(interpreterProxy->classArray(), 3))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, result, interpreterProxy->integerObjectOf(UP_PROTOCOL));
	if (!(oop = byteString(UP_PLUGIN_VERSION, strlen(UP_PLUGIN_VERSION)))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(1, result, oop);
	if (!(oop = byteString(platform, strlen(platform)))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(2, result, oop);
	return interpreterProxy->methodReturnValue(result);
}


/*** primitive_fma ***/

/*	Float>>primitiveTimes: y plus: z
	<primitive: 'primitive_fma' module: 'UtilsPlugin'>
	Answer self * y + z rounded once (C's fma).  Fail with #'bad receiver'
	unless the receiver is a Float, #'bad argument' unless y and z are
	Floats. */
EXPORT(sqInt)
primitive_fma(void)
{
	sqInt result;

	if (interpreterProxy->methodArgumentCount() != 2) {
		return fail(PrimErrBadNumArgs);
	}
	if (!interpreterProxy->isFloatObject(interpreterProxy->stackValue(2))) {
		return fail(PrimErrBadReceiver);
	}
	if (!interpreterProxy->isFloatObject(interpreterProxy->stackValue(1))
	 || !interpreterProxy->isFloatObject(interpreterProxy->stackValue(0))) {
		return fail(PrimErrBadArgument);
	}
	result = newFloat(up_fma(
		interpreterProxy->floatValueOf(interpreterProxy->stackValue(2)),
		interpreterProxy->floatValueOf(interpreterProxy->stackValue(1)),
		interpreterProxy->floatValueOf(interpreterProxy->stackValue(0))));
	return result ? interpreterProxy->methodReturnValue(result) : 0;
}


/*** primitive_strtok_r ***/

/*	String>>strtok: delimiters inclusive: aBoolean
	<primitive: 'primitive_strtok_r' module: 'UtilsPlugin'>
	Answer an Array of new ByteStrings, the pieces of the receiver between
	the characters of delimiters: with aBoolean false the maximal runs of
	non-delimiters (strtok_r(3)), with aBoolean true every field between two
	delimiters, empty ones included, but an empty last one (the lines of a
	text, when the delimiters are its line ends).  Every byte counts, NUL
	too.  Fail with #'bad receiver' unless the receiver is a byte string,
	#'bad argument' unless delimiters is a byte string (or ByteArray) and
	aBoolean a Boolean. */
EXPORT(sqInt)
primitive_strtok_r(void)
{
	sqInt receiver, delimiters, result, piece;
	unsigned char set[256];
	up_splitter it;
	size_t start, length, count, index;
	int inclusive;

	if (interpreterProxy->methodArgumentCount() != 2) {
		return fail(PrimErrBadNumArgs);
	}
	receiver = interpreterProxy->stackValue(2);
	delimiters = interpreterProxy->stackValue(1);
	if (interpreterProxy->isImmediate(receiver) || !interpreterProxy->isBytes(receiver)) {
		return fail(PrimErrBadReceiver);
	}
	if (interpreterProxy->isImmediate(delimiters) || !interpreterProxy->isBytes(delimiters)
	 || !isBooleanAt(0, &inclusive)) {
		return fail(PrimErrBadArgument);
	}
	up_byte_set(set, (const unsigned char *)interpreterProxy->firstIndexableField(delimiters),
		(size_t)interpreterProxy->byteSizeOf(delimiters));

	count = 0;
	up_split_init(&it, (const unsigned char *)interpreterProxy->firstIndexableField(receiver),
		(size_t)interpreterProxy->byteSizeOf(receiver), set, inclusive);
	while (up_split_next(&it, &start, &length)) {
		count++;
	}
	if (!(result = instantiate(interpreterProxy->classArray(), (sqInt)count))) {
		return 0;
	}
	index = 0;
	up_split_init(&it, (const unsigned char *)interpreterProxy->firstIndexableField(receiver),
		(size_t)interpreterProxy->byteSizeOf(receiver), set, inclusive);
	while (up_split_next(&it, &start, &length)) {
		if (!(piece = byteString((const char *)interpreterProxy->firstIndexableField(receiver) + start, length))) {
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue((sqInt)index++, result, piece);
	}
	return interpreterProxy->methodReturnValue(result);
}


/*** primitive_decasteljau ***/

/*	UtilsPlugin class>>primDeCasteljau: controlPoints into: curvePoints at: parameters
	<primitive: 'primitive_decasteljau' module: 'UtilsPlugin'>
	Store at curvePoints[j] a new Point, of Floats, the point at
	parameters[j] of the Bezier curve of controlPoints (up_bezier), for j in
	1 .. parameters size, and answer the receiver.  controlPoints is a
	non-empty Array of Points whose coordinates are Floats or SmallIntegers,
	parameters an Array of Floats or SmallIntegers, curvePoints a writable
	Array at least as large as parameters (controlPoints itself may be
	curvePoints: the control points are all read first).  Fail with
	#'bad argument' for anything else, #'no modification' when curvePoints is
	read-only, #'insufficient C memory' when the copy of the control points
	could not be allocated. */
EXPORT(sqInt)
primitive_decasteljau(void)
{
	sqInt controlPoints, curvePoints, parameters, point, oop;
	sqInt n, count, k, j;
	double *xs, t, qx, qy;

	if (interpreterProxy->methodArgumentCount() != 3) {
		return fail(PrimErrBadNumArgs);
	}
	controlPoints = interpreterProxy->stackValue(2);
	curvePoints = interpreterProxy->stackValue(1);
	parameters = interpreterProxy->stackValue(0);
	if (!interpreterProxy->isArray(controlPoints) || !interpreterProxy->isArray(curvePoints)
	 || !interpreterProxy->isArray(parameters)) {
		return fail(PrimErrBadArgument);
	}
	n = interpreterProxy->stSizeOf(controlPoints);
	count = interpreterProxy->stSizeOf(parameters);
	if (n < 1 || interpreterProxy->stSizeOf(curvePoints) < count) {
		return fail(PrimErrBadArgument);
	}
	for (k = 0; k < n; k++) {
		point = interpreterProxy->fetchPointerofObject(k, controlPoints);
		if (interpreterProxy->isImmediate(point)
		 || !interpreterProxy->isKindOfClass(point, interpreterProxy->classPoint())
		 || interpreterProxy->slotSizeOf(point) < 2
		 || !numberValue(interpreterProxy->fetchPointerofObject(0, point), &t)
		 || !numberValue(interpreterProxy->fetchPointerofObject(1, point), &t)) {
			return fail(PrimErrBadArgument);
		}
	}
	for (j = 0; j < count; j++) {
		if (!numberValue(interpreterProxy->fetchPointerofObject(j, parameters), &t)) {
			return fail(PrimErrBadArgument);
		}
	}
	if (interpreterProxy->isOopImmutable(curvePoints)) {
		return fail(PrimErrNoModification);
	}
	if (!(xs = malloc(2 * (size_t)n * sizeof(double)))) {
		return fail(PrimErrNoCMemory);
	}
	for (k = 0; k < n; k++) {
		point = interpreterProxy->fetchPointerofObject(k, controlPoints);
		numberValue(interpreterProxy->fetchPointerofObject(0, point), &xs[k]);
		numberValue(interpreterProxy->fetchPointerofObject(1, point), &xs[n + k]);
	}
	for (j = 0; j < count; j++) {
		numberValue(interpreterProxy->fetchPointerofObject(j, parameters), &t);
		up_bezier(xs, xs + n, (size_t)n, t, &qx, &qy);
		if (!(point = instantiate(interpreterProxy->classPoint(), 0))) {
			free(xs);
			return 0;
		}
		if (!(oop = newFloat(qx))) {
			free(xs);
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(0, point, oop);
		if (!(oop = newFloat(qy))) {
			free(xs);
			return 0;
		}
		interpreterProxy->storePointerofObjectwithValue(1, point, oop);
		interpreterProxy->storePointerofObjectwithValue(j, curvePoints, point);
	}
	free(xs);
	return interpreterProxy->methodReturnReceiver();
}


/*** Knuth's generator: primitive_randomknuth_* ***/

/* The state of a generator: { ByteArray of KK doubles (Knuth's ran_u).
 * ByteArray of QUALITY doubles (ranf_arr_buf). cursor }, where cursor is
 * the 1-based index of the next fraction of the buffer, at most KK + 1 (the
 * index of its -1, which starts a new cycle) */
#define STATE_SLOTS 3

static int
isWritableBytes(sqInt oop, sqInt size)
{
	return !interpreterProxy->isImmediate(oop)
		&& interpreterProxy->isBytes(oop)
		&& interpreterProxy->byteSizeOf(oop) == size
		&& !interpreterProxy->isOopImmutable(oop);
}

/* Whether state is a generator's state that the primitives may update: 1,
 * or 0 (and the failure code is set) */
static int
checkState(sqInt state)
{
	sqInt cursor;

	if (!interpreterProxy->isArray(state) || interpreterProxy->stSizeOf(state) != STATE_SLOTS
	 || !isWritableBytes(interpreterProxy->fetchPointerofObject(0, state), UP_KNUTH_STATE_BYTES)
	 || !isWritableBytes(interpreterProxy->fetchPointerofObject(1, state), UP_KNUTH_BUFFER_BYTES)) {
		fail(PrimErrBadArgument);
		return 0;
	}
	cursor = interpreterProxy->fetchPointerofObject(2, state);
	if (!interpreterProxy->isIntegerObject(cursor)
	 || interpreterProxy->integerValueOf(cursor) < 1
	 || interpreterProxy->integerValueOf(cursor) > UP_KNUTH_KK + 1) {
		fail(PrimErrBadArgument);
		return 0;
	}
	if (interpreterProxy->isOopImmutable(state)) {
		fail(PrimErrNoModification);
		return 0;
	}
	return 1;
}

static double *
stateArray(sqInt state, sqInt index)
{
	return (double *)interpreterProxy->firstIndexableField(interpreterProxy->fetchPointerofObject(index, state));
}

/* Seed the state (ranf_start), with an empty buffer: the next fraction
 * starts a new cycle */
static void
startState(sqInt state, int64_t seed)
{
	up_knuth_start(seed, stateArray(state, 0));
	stateArray(state, 1)[0] = -1;
	interpreterProxy->storePointerofObjectwithValue(2, state, interpreterProxy->integerObjectOf(1));
}

/*	UtilsPlugin class>>primKnuthState
	<primitive: 'primitive_randomknuth_init' module: 'UtilsPlugin'>
	Answer a new state, seeded with 314159 (the seed of Knuth's ranf_arr_cycle
	when it was never seeded). */
EXPORT(sqInt)
primitive_randomknuth_init(void)
{
	sqInt state, oop;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return fail(PrimErrBadNumArgs);
	}
	if (!(state = instantiate(interpreterProxy->classArray(), STATE_SLOTS))) {
		return 0;
	}
	if (!(oop = instantiate(interpreterProxy->classByteArray(), UP_KNUTH_STATE_BYTES))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(0, state, oop);
	if (!(oop = instantiate(interpreterProxy->classByteArray(), UP_KNUTH_BUFFER_BYTES))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(1, state, oop);
	startState(state, 314159);
	return interpreterProxy->methodReturnValue(state);
}

/*	KnuthRandom>>primStart
	<primitive: 'primitive_randomknuth_start' module: 'UtilsPlugin'>
	Seed the state, the first slot of the receiver, with the SmallInteger of
	its second slot (its low 30 bits, as Knuth's ranf_start), and answer the
	receiver.  Fail with #'bad receiver' unless the receiver has these two
	slots, #'bad argument' unless the state is one, #'no modification' when
	it is read-only. */
EXPORT(sqInt)
primitive_randomknuth_start(void)
{
	sqInt receiver, state, seed;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return fail(PrimErrBadNumArgs);
	}
	receiver = interpreterProxy->stackValue(0);
	if (interpreterProxy->isImmediate(receiver) || !interpreterProxy->isPointers(receiver)
	 || interpreterProxy->slotSizeOf(receiver) < 2
	 || !interpreterProxy->isIntegerObject(seed = interpreterProxy->fetchPointerofObject(1, receiver))) {
		return fail(PrimErrBadReceiver);
	}
	state = interpreterProxy->fetchPointerofObject(0, receiver);
	if (!checkState(state)) {
		return 0;
	}
	startState(state, (int64_t)interpreterProxy->integerValueOf(seed));
	return interpreterProxy->methodReturnReceiver();
}

/*	KnuthRandom>>primNext: state
	<primitive: 'primitive_randomknuth_next' module: 'UtilsPlugin'>
	Answer the next fraction of the state, a Float in [0, 1) (Knuth's
	ranf_arr_next), and advance it.  Fail with #'bad argument' unless state
	is one, #'no modification' when it is read-only. */
EXPORT(sqInt)
primitive_randomknuth_next(void)
{
	sqInt state, cursor, result;
	double *buffer, r;

	if (interpreterProxy->methodArgumentCount() != 1) {
		return fail(PrimErrBadNumArgs);
	}
	state = interpreterProxy->stackValue(0);
	if (!checkState(state)) {
		return 0;
	}
	buffer = stateArray(state, 1);
	cursor = interpreterProxy->integerValueOf(interpreterProxy->fetchPointerofObject(2, state));
	if (buffer[cursor - 1] < 0) {
		/* a new cycle, whose first fraction is next: the state stays
		 * consistent if the Float below cannot be allocated, and the VM's
		 * retry answers that fraction */
		up_knuth_cycle(stateArray(state, 0), buffer);
		cursor = 1;
		interpreterProxy->storePointerofObjectwithValue(2, state, interpreterProxy->integerObjectOf(cursor));
	}
	r = buffer[cursor - 1];
	/* a fraction in [0, 1): a SmallFloat64 on 64-bit VMs, but tiny ones */
	if (!(result = newFloat(r))) {
		return 0;
	}
	interpreterProxy->storePointerofObjectwithValue(2, state, interpreterProxy->integerObjectOf(cursor + 1));
	return interpreterProxy->methodReturnValue(result);
}


#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "UtilsPlugin";
void* UtilsPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitive_decasteljau\000\002", (void*)primitive_decasteljau},
	{(void*)_m, "primitive_fma\000\000", (void*)primitive_fma},
	{(void*)_m, "primitive_info\000\000", (void*)primitive_info},
	{(void*)_m, "primitive_randomknuth_init\000\000", (void*)primitive_randomknuth_init},
	{(void*)_m, "primitive_randomknuth_next\000\001", (void*)primitive_randomknuth_next},
	{(void*)_m, "primitive_randomknuth_start\000\002", (void*)primitive_randomknuth_start},
	{(void*)_m, "primitive_strtok_r\000\000", (void*)primitive_strtok_r},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitive_decasteljauAccessorDepth = 2;
EXPORT(signed char) primitive_fmaAccessorDepth = 0;
EXPORT(signed char) primitive_infoAccessorDepth = 0;
EXPORT(signed char) primitive_randomknuth_initAccessorDepth = 0;
EXPORT(signed char) primitive_randomknuth_nextAccessorDepth = 1;
EXPORT(signed char) primitive_randomknuth_startAccessorDepth = 2;
EXPORT(signed char) primitive_strtok_rAccessorDepth = 0;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
