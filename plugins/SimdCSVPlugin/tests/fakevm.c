/* fakevm.c -- a stand-in for the VM, for the tests of the glue: see
 * fakevm.h.  Only the functions of the proxy that the glue may call are
 * real; the ones it must never call record a protocol error; the others
 * are NULL. */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "fakevm.h"

#define MAGIC 0x46616b65u	/* "Fake" */
#define SMALLINTEGER_MAX (((sqInt)1 << 60) - 1)
#define SMALLINTEGER_MIN (-((sqInt)1 << 60))
#define FORWARDER_FORMAT 7

typedef struct {
	uint32_t magic;
	int format;		/* Spur's: 0, 1, 2, 4, 7, 9, 10, 12, 16, 24 */
	int immutable;
	size_t pretended;	/* what byteSizeOf answers, when not 0 */
	int madeInCall;		/* by instantiateClassindexableSize in this call */
	int old;		/* in old space */
	sqInt cls;
	size_t size;		/* slots (formats 1, 2, 4), elements (9, 10, 12), bytes (16, 24) */
	sqInt *slots;
	unsigned char *bytes;
	const char *name;	/* of a class */
} Obj;

sqInt fakeMetaclass, fakeUndefinedObject, fakeTrue, fakeFalse, fakeSmallInteger, fakeCharacter;
sqInt fakeSmallFloat, fakeBoxedFloat, fakeLargePositiveInteger, fakeLargeNegativeInteger, fakeFraction;
sqInt fakeArray, fakeArraySubclass, fakeWeakArray, fakeCompiledMethod;
sqInt fakeByteString, fakeByteSymbol, fakeWideString, fakeWideSymbol, fakeMyString;
sqInt fakeByteArray, fakeDoubleByteArray, fakeWordArray, fakeBitmap, fakeIntegerArray;
sqInt fakeFloat32Array, fakeFloat64Array, fakeDoubleWordArray;
sqInt fakeNil, fakeTrueObject, fakeFalseObject;

static struct VirtualMachine proxy;
static Obj **objects;
static size_t objectCount, objectCapacity;

/* The values of the SmallFloat64s */
static double *smallFloats;
static size_t smallFloatCount, smallFloatCapacity;

/* The state of a call */
static sqInt stack[16];
static int stackArgs;
static int inCall;
static sqInt primFailCode;
static FakeAnswer current;
static char lastError[256];

static sqInt storeTarget;

/* The allocator */
static long budget = -1;
static long eden = -1;
static sqInt budgetCode = PrimErrNoMemory;

static void
protocolError(const char *format, ...)
{
	va_list args;

	if (current.protocolErrors++ == 0) {
		va_start(args, format);
		vsnprintf(lastError, sizeof lastError, format, args);
		va_end(args);
	}
}

const char *
fake_last_error(void)
{
	return lastError;
}

static int
isImmediateOop(sqInt oop)
{
	return (oop & 7) != 0;
}

static Obj *
objectOf(sqInt oop, const char *who)
{
	Obj *o = (Obj *)oop;

	if (!oop || isImmediateOop(oop) || o->magic != MAGIC) {
		protocolError("%s: %#llx is not an object", who, (unsigned long long)oop);
		return NULL;
	}
	return o;
}

/* The object oop, or NULL (and a protocol error) when it is a forwarder,
 * whose body the VM never reads */
static Obj *
bodyOf(sqInt oop, const char *who)
{
	Obj *o = isImmediateOop(oop) ? NULL : objectOf(oop, who);

	if (o && o->format == FORWARDER_FORMAT) {
		protocolError("%s of a forwarder", who);
		return NULL;
	}
	return o;
}

/* The bytes of size elements of format */
static size_t
bytesOf(int format, size_t size)
{
	switch (format) {
	case 1: case 2: case 4: case 9: return size * 8;
	case 10: return size * 4;
	case 12: return size * 2;
	case 16: case 24: return size;
	default: return 0;
	}
}

static Obj *
newObject(sqInt cls, int format, size_t size)
{
	Obj *o = calloc(1, sizeof *o);

	if (!o) {
		fprintf(stderr, "fakevm: out of memory\n");
		exit(2);
	}
	o->magic = MAGIC;
	o->format = format;
	o->cls = cls;
	o->size = size;
	if (format == 1 || format == 2 || format == 4) {
		size_t i;
		o->slots = malloc((size ? size : 1) * sizeof(sqInt));
		for (i = 0; i < size; i++) {
			o->slots[i] = fakeNil;
		}
	} else if (format >= 9) {
		/* the exact size, so that AddressSanitizer sees an access past
		   the end; malloc(0) is a unique address on glibc, as the VM's is
		   not NULL; 8-aligned, as Spur's bodies are */
		size_t bytes = bytesOf(format, size);
		o->bytes = malloc(bytes ? bytes : 1);
		if (bytes) {
			memset(o->bytes, 0, bytes);
		}
	}
	if (!o->slots && (format == 1 || format == 2 || format == 4)) {
		fprintf(stderr, "fakevm: out of memory\n");
		exit(2);
	}
	if (objectCount == objectCapacity) {
		objectCapacity = objectCapacity ? 2 * objectCapacity : 1024;
		objects = realloc(objects, objectCapacity * sizeof *objects);
		if (!objects) {
			fprintf(stderr, "fakevm: out of memory\n");
			exit(2);
		}
	}
	objects[objectCount++] = o;
	return o;
}

static void
freeObject(Obj *o)
{
	o->magic = 0;
	free(o->slots);
	free(o->bytes);
	free(o);
}

size_t
fake_mark(void)
{
	return objectCount;
}

void
fake_release(size_t mark)
{
	while (objectCount > mark) {
		freeObject(objects[--objectCount]);
	}
}

void
fake_free_all(void)
{
	fake_release(0);
	free(objects);
	objects = NULL;
	objectCapacity = 0;
	free(smallFloats);
	smallFloats = NULL;
	smallFloatCount = smallFloatCapacity = 0;
}


/*** Immediates ***/

sqInt
fake_int(sqInt value)
{
	return (sqInt)((usqInt)value << 3) | 1;
}

int
fake_is_int(sqInt oop)
{
	return (oop & 7) == 1;
}

sqInt
fake_int_value(sqInt oop)
{
	return oop >> 3;
}

sqInt
fake_character(uint32_t code)
{
	return (sqInt)((usqInt)code << 3) | 2;
}

sqInt
fake_small_float(double value)
{
	if (smallFloatCount == smallFloatCapacity) {
		smallFloatCapacity = smallFloatCapacity ? 2 * smallFloatCapacity : 1024;
		smallFloats = realloc(smallFloats, smallFloatCapacity * sizeof *smallFloats);
		if (!smallFloats) {
			fprintf(stderr, "fakevm: out of memory\n");
			exit(2);
		}
	}
	smallFloats[smallFloatCount] = value;
	return (sqInt)((usqInt)smallFloatCount++ << 3) | 4;
}


/*** Objects ***/

/* A class whose class is a new metaclass (or meta, for Metaclass itself) */
static sqInt
makeClass(const char *name, int instSpec, int fixedFields, sqInt meta)
{
	Obj *metaclass, *cls;

	if (!meta) {
		metaclass = newObject(fakeMetaclass, 1, 3);
		metaclass->slots[2] = fake_int((1 << 16) | 3);
		metaclass->name = name;
		meta = (sqInt)metaclass;
	}
	cls = newObject(meta, 1, 3);
	cls->slots[2] = fake_int(((sqInt)instSpec << 16) | fixedFields);
	cls->name = name;
	return (sqInt)cls;
}

sqInt
fake_class(const char *name, int instSpec, int fixedFields)
{
	return makeClass(name, instSpec, fixedFields, 0);
}

sqInt
fake_new(sqInt aClass, size_t size)
{
	Obj *c = objectOf(aClass, "fake_new");
	sqInt format;
	int instSpec;

	if (!c || c->format != 1 || c->size < 3 || !fake_is_int(c->slots[2])) {
		fprintf(stderr, "fakevm: fake_new of a non-class\n");
		exit(2);
	}
	format = fake_int_value(c->slots[2]);
	instSpec = (int)((format >> 16) & 0x1F);
	return (sqInt)newObject(aClass, instSpec,
		instSpec == 0 ? 0 : instSpec == 1 ? (size_t)(format & 0xFFFF) : size);
}

sqInt
fake_bytes(sqInt aClass, const void *bytes, size_t size)
{
	sqInt oop = fake_new(aClass, size);

	if (size) {
		memcpy(fake_body(oop), bytes, size);
	}
	return oop;
}

sqInt
fake_words(sqInt aClass, const uint32_t *words, size_t size)
{
	sqInt oop = fake_new(aClass, size);

	if (size) {
		memcpy(fake_body(oop), words, size * 4);
	}
	return oop;
}

sqInt
fake_array(sqInt aClass, const sqInt *oops, size_t size)
{
	sqInt oop = fake_new(aClass, size);

	if (size) {
		memcpy(((Obj *)oop)->slots, oops, size * sizeof(sqInt));
	}
	return oop;
}

sqInt
fake_boxed_float(double value)
{
	Obj *o = newObject(fakeBoxedFloat, 10, 2);

	memcpy(o->bytes, &value, 8);
	return (sqInt)o;
}

sqInt
fake_large(const uint8_t *digits, size_t size, int negative)
{
	return fake_bytes(negative ? fakeLargeNegativeInteger : fakeLargePositiveInteger, digits, size);
}

sqInt
fake_compiled_method(size_t size)
{
	return (sqInt)newObject(fakeCompiledMethod, 24, size);
}

sqInt
fake_forwarder(void)
{
	return (sqInt)newObject(fakeNil, FORWARDER_FORMAT, 0);
}

int
fake_is_object(sqInt oop)
{
	return oop && !isImmediateOop(oop) && ((Obj *)oop)->magic == MAGIC;
}

sqInt
fake_class_of(sqInt oop)
{
	switch (oop & 7) {
	case 1: return fakeSmallInteger;
	case 2: return fakeCharacter;
	case 4: return fakeSmallFloat;
	case 0: return ((Obj *)oop)->cls;
	default: return fakeNil;
	}
}

const char *
fake_class_name(sqInt aClass)
{
	return fake_is_object(aClass) && ((Obj *)aClass)->name ? ((Obj *)aClass)->name : "?";
}

int
fake_format(sqInt oop)
{
	return ((Obj *)oop)->format;
}

int
fake_is_float(sqInt oop)
{
	return (oop & 7) == 4 || (fake_is_object(oop) && ((Obj *)oop)->cls == fakeBoxedFloat);
}

double
fake_float_value(sqInt oop)
{
	double d;

	if ((oop & 7) == 4) {
		return smallFloats[(usqInt)oop >> 3];
	}
	memcpy(&d, ((Obj *)oop)->bytes, 8);
	return d;
}

size_t
fake_size(sqInt oop)
{
	return ((Obj *)oop)->size;
}

size_t
fake_body_bytes(sqInt oop)
{
	Obj *o = (Obj *)oop;

	return bytesOf(o->format, o->size);
}

void *
fake_body(sqInt oop)
{
	Obj *o = (Obj *)oop;

	return o->slots ? (void *)o->slots : (void *)o->bytes;
}

sqInt
fake_slot(sqInt oop, size_t index0)
{
	Obj *o = (Obj *)oop;

	return fake_is_object(oop) && o->slots && index0 < o->size ? o->slots[index0] : fakeNil;
}

void
fake_set_slot(sqInt oop, size_t index0, sqInt value)
{
	((Obj *)oop)->slots[index0] = value;
}

void
fake_set_immutable(sqInt oop, int immutable)
{
	((Obj *)oop)->immutable = immutable;
}

void
fake_pretend_bytes(sqInt oop, size_t bytes)
{
	((Obj *)oop)->pretended = bytes;
}

void
fake_set_budget(long allowed, sqInt failCode)
{
	budget = allowed;
	budgetCode = failCode;
}

void
fake_set_eden(long allowed)
{
	eden = allowed;
}

void
fake_allow_store(sqInt oop)
{
	storeTarget = oop;
}

int
fake_is_small_float_value(double value)
{
	double magnitude = value < 0 ? -value : value;

	/* the exponents 897 to 1151 whole, and of 896 all but the power of two
	   (whose rotated bits are those of a zero) */
	return value == 0 || (magnitude > 0x1p-127 && magnitude < 0x1p129);
}

uint64_t
fake_spur_bytes(sqInt oop)
{
	uint64_t slots = (bytesOf(((Obj *)oop)->format, ((Obj *)oop)->size) + 7) / 8;

	return 8 * ((slots ? slots : 1) + (slots >= 255 ? 2 : 1));
}


/*** The proxy ***/

static sqInt fMinorVersion(void) { return VM_PROXY_MINOR; }
static sqInt fMajorVersion(void) { return VM_PROXY_MAJOR; }

static sqInt
fStackValue(sqInt offset)
{
	if (!inCall || offset < 0 || offset > stackArgs) {
		protocolError("stackValue(%lld) outside the %d arguments", (long long)offset, stackArgs);
		return fakeNil;
	}
	return stack[stackArgs - offset];
}

static sqInt fMethodArgumentCount(void) { return stackArgs; }

static sqInt
fByteSizeOf(sqInt oop)
{
	Obj *o;

	if (isImmediateOop(oop) || !(o = bodyOf(oop, "byteSizeOf"))) {
		return 0;
	}
	return (sqInt)(o->pretended ? o->pretended : bytesOf(o->format, o->size));
}

static sqInt
fFetchClassOf(sqInt oop)
{
	if (isImmediateOop(oop)) {
		return fake_class_of(oop);
	}
	return objectOf(oop, "fetchClassOf") ? ((Obj *)oop)->cls : fakeNil;
}

static sqInt
fFetchPointerofObject(sqInt index, sqInt oop)
{
	Obj *o = bodyOf(oop, "fetchPointerofObject");

	if (oop == storeTarget) {
		protocolError("fetchPointerofObject of the destination");
	}
	if (!o || (o->format != 1 && o->format != 2 && o->format != 4) || index < 0 || (size_t)index >= o->size) {
		protocolError("fetchPointerofObject(%lld) of a %s", (long long)index, o ? fake_class_name(o->cls) : "non-object");
		return fakeNil;
	}
	return o->slots[index];
}

static void *
fFirstIndexableField(sqInt oop)
{
	Obj *o = bodyOf(oop, "firstIndexableField");

	if (!o) {
		return NULL;
	}
	switch (o->format) {
	case 2: case 4: return o->slots;
	case 9: case 10: case 12: case 16: return o->bytes;
	default: return NULL;	/* not indexable, or a CompiledMethod */
	}
}

static sqInt
fSlotSizeOf(sqInt oop)
{
	Obj *o = bodyOf(oop, "slotSizeOf");

	if (!o) {
		return 0;
	}
	switch (o->format) {
	case 1: case 2: case 4: case 9: return (sqInt)o->size;
	case 10: return (sqInt)((o->size + 1) / 2);
	case 12: return (sqInt)((o->size + 3) / 4);
	case 16: case 24: return (sqInt)((o->size + 7) / 8);
	default: return 0;
	}
}

static sqInt
fStorePointerofObjectwithValue(sqInt index, sqInt oop, sqInt value)
{
	Obj *o = bodyOf(oop, "storePointerofObjectwithValue");

	current.stores++;
	if (!o || (o->format != 1 && o->format != 2) || index < 0 || (size_t)index >= o->size) {
		protocolError("storePointerofObjectwithValue(%lld) into a %s", (long long)index,
			o ? fake_class_name(o->cls) : "non-object");
		return value;
	}
	if (!o->madeInCall && oop != storeTarget) {
		protocolError("storePointerofObjectwithValue into an object the call did not make");
	}
	if (o->immutable) {
		protocolError("storePointerofObjectwithValue into an immutable object");
	}
	if (!isImmediateOop(value)) {
		objectOf(value, "storePointerofObjectwithValue (value)");
	}
	o->slots[index] = value;
	return value;
}

static int
formatOf(sqInt oop, const char *who)
{
	Obj *o = isImmediateOop(oop) ? NULL : objectOf(oop, who);

	return o ? o->format : -1;
}

static sqInt fIsBytes(sqInt oop) { return formatOf(oop, "isBytes") >= 16; }
static sqInt fIsWords(sqInt oop) { int f = formatOf(oop, "isWords"); return f == 10 || f == 11; }
static sqInt fIsWordsOrBytes(sqInt oop) { int f = formatOf(oop, "isWordsOrBytes"); return f >= 9 && f < 24; }
static sqInt fIsPointers(sqInt oop) { int f = formatOf(oop, "isPointers"); return f >= 0 && f <= 5; }
static sqInt fIsArray(sqInt oop) { return formatOf(oop, "isArray") == 2; }
static sqInt fIsIntegerObject(sqInt oop) { return fake_is_int(oop); }
static sqInt fIsImmediate(sqInt oop) { return isImmediateOop(oop); }

static sqInt
fIsFloatObject(sqInt oop)
{
	if (!isImmediateOop(oop)) {
		objectOf(oop, "isFloatObject");
	}
	return fake_is_float(oop);
}

static double
fFloatValueOf(sqInt oop)
{
	if (!fake_is_float(oop)) {
		protocolError("floatValueOf of a non-Float");
		primFailCode = PrimErrBadArgument;
		return 0;
	}
	return fake_float_value(oop);
}

static sqInt
fIsOopImmutable(sqInt oop)
{
	return isImmediateOop(oop) || (objectOf(oop, "isOopImmutable") && ((Obj *)oop)->immutable);
}

static sqInt
fIsYoung(sqInt oop)
{
	return !isImmediateOop(oop) && objectOf(oop, "isYoung") && !((Obj *)oop)->old;
}

static sqInt
fIntegerObjectOf(sqInt value)
{
	if (value < SMALLINTEGER_MIN || value > SMALLINTEGER_MAX) {
		protocolError("integerObjectOf(%lld): not a SmallInteger", (long long)value);
	}
	return fake_int(value);
}

static sqInt
fIntegerValueOf(sqInt oop)
{
	if (!fake_is_int(oop)) {
		protocolError("integerValueOf of a non-SmallInteger");
	}
	return fake_int_value(oop);
}

static sqInt fNilObject(void) { return fakeNil; }
static sqInt fTrueObject(void) { return fakeTrueObject; }
static sqInt fFalseObject(void) { return fakeFalseObject; }
static sqInt fClassArray(void) { return fakeArray; }
static sqInt fClassByteArray(void) { return fakeByteArray; }
static sqInt fClassBitmap(void) { return fakeBitmap; }
static sqInt fClassFloat(void) { return fakeBoxedFloat; }
static sqInt fClassString(void) { return fakeByteString; }
static sqInt fClassLargePositiveInteger(void) { return fakeLargePositiveInteger; }
static sqInt fClassLargeNegativeInteger(void) { return fakeLargeNegativeInteger; }

/* As the VM's: a new zero-filled instance, or 0 and primFailCode set */
static sqInt
fInstantiateClassindexableSize(sqInt aClass, sqInt size)
{
	Obj *c = isImmediateOop(aClass) ? NULL : objectOf(aClass, "instantiateClassindexableSize");
	Obj *o;
	sqInt format;
	int instSpec, fixed;

	current.allocations++;
	if (!inCall) {
		protocolError("instantiateClassindexableSize outside a call");
	}
	if (!c || c->format != 1 || c->size < 3 || !fake_is_int(c->slots[2])) {
		primFailCode = PrimErrBadReceiver;	/* not a class: an error of the class table */
		return 0;
	}
	if (budget == 0) {
		primFailCode = budgetCode;
		return 0;
	}
	format = fake_int_value(c->slots[2]);
	instSpec = (int)((format >> 16) & 0x1F);
	fixed = (int)(format & 0xFFFF);
	if (size < 0 || fixed != 0 || (instSpec != 2 && instSpec != 10 && instSpec != 16)) {
		primFailCode = PrimErrBadReceiver;
		return 0;
	}
	o = newObject(aClass, instSpec, (size_t)size);
	if (budget > 0) {
		budget--;
	}
	o->madeInCall = 1;
	o->old = eden >= 0 && current.allocated >= eden;
	current.allocated++;
	current.bytes += fake_spur_bytes((sqInt)o);
	if (current.allocated == 1) {
		current.firstBytes = current.bytes;
	}
	return (sqInt)o;
}

static sqInt fFailed(void) { return primFailCode != 0; }

static sqInt
fPrimitiveFailFor(sqInt code)
{
	return primFailCode = code;
}

static sqInt fPrimitiveFailureCode(void) { return primFailCode; }

static sqInt
fMethodReturnValue(sqInt oop)
{
	if (primFailCode) {
		protocolError("methodReturnValue while failing with %lld", (long long)primFailCode);
	}
	if (current.returned) {
		protocolError("methodReturnValue twice");
	}
	if (!isImmediateOop(oop)) {
		objectOf(oop, "methodReturnValue");
	}
	current.returned = 1;
	current.value = oop;
	return 0;
}

static sqInt
fFloatObjectOf(double value)
{
	if (!fake_is_small_float_value(value)) {
		protocolError("floatObjectOf(%g): not a SmallFloat64", value);
	}
	return fake_small_float(value);
}

/* What the glue must never call */
static sqInt fClone(sqInt oop) { (void)oop; protocolError("clone"); return 0; }
static sqInt fSigned64BitIntegerFor(sqLong v) { (void)v; protocolError("signed64BitIntegerFor"); return 0; }
static sqInt fPositive64BitIntegerFor(usqLong v) { (void)v; protocolError("positive64BitIntegerFor"); return 0; }
static sqInt fPositive32BitIntegerFor(unsigned int v) { (void)v; protocolError("positive32BitIntegerFor"); return 0; }
static sqInt fSigned32BitIntegerFor(sqInt v) { (void)v; protocolError("signed32BitIntegerFor"); return 0; }
static sqInt fStringForCString(const char *s) { (void)s; protocolError("stringForCString"); return 0; }
static sqInt fMethodReturnString(char *s) { (void)s; protocolError("methodReturnString"); return 0; }
static sqInt fMethodReturnInteger(sqInt v) { (void)v; protocolError("methodReturnInteger"); return 0; }
static sqInt fMethodReturnBool(sqInt v) { (void)v; protocolError("methodReturnBool"); return 0; }
static sqInt fPop(sqInt n) { (void)n; protocolError("pop"); return 0; }
static void fPush(sqInt oop) { (void)oop; protocolError("push"); }
static void fPopthenPush(sqInt n, sqInt oop) { (void)n; (void)oop; protocolError("popthenPush"); }
static sqInt fPushInteger(sqInt v) { (void)v; protocolError("pushInteger"); return 0; }
static sqInt fPrimitiveFail(void) { protocolError("primitiveFail"); return 0; }
static sqInt fSuccess(sqInt b) { (void)b; protocolError("success"); return 0; }

struct VirtualMachine *
fake_init(void)
{
	Obj *nilObject, *metaclass, *metaclassClass;

	/* nil first, so that the slots of the classes hold it */
	nilObject = newObject(0, 0, 0);
	fakeNil = (sqInt)nilObject;
	/* Metaclass is an instance of Metaclass class, an instance of Metaclass */
	metaclass = newObject(0, 1, 3);
	metaclass->slots[2] = fake_int((1 << 16) | 6);
	metaclass->name = "Metaclass";
	fakeMetaclass = (sqInt)metaclass;
	metaclassClass = newObject(fakeMetaclass, 1, 3);
	metaclassClass->slots[2] = fake_int((1 << 16) | 3);
	metaclassClass->name = "Metaclass class";
	metaclass->cls = (sqInt)metaclassClass;

	fakeUndefinedObject = fake_class("UndefinedObject", 0, 0);
	nilObject->cls = fakeUndefinedObject;
	fakeTrue = fake_class("True", 0, 0);
	fakeFalse = fake_class("False", 0, 0);
	fakeTrueObject = fake_new(fakeTrue, 0);
	fakeFalseObject = fake_new(fakeFalse, 0);
	fakeSmallInteger = fake_class("SmallInteger", 7, 0);
	fakeCharacter = fake_class("Character", 7, 0);
	fakeSmallFloat = fake_class("SmallFloat64", 7, 0);
	fakeBoxedFloat = fake_class("BoxedFloat64", 10, 0);
	fakeLargePositiveInteger = fake_class("LargePositiveInteger", 16, 0);
	fakeLargeNegativeInteger = fake_class("LargeNegativeInteger", 16, 0);
	fakeFraction = fake_class("Fraction", 1, 2);
	fakeArray = fake_class("Array", 2, 0);
	fakeArraySubclass = fake_class("WeakActionSequence", 2, 0);
	fakeWeakArray = fake_class("WeakArray", 4, 0);
	fakeCompiledMethod = fake_class("CompiledMethod", 24, 0);
	fakeByteString = fake_class("ByteString", 16, 0);
	fakeByteSymbol = fake_class("ByteSymbol", 16, 0);
	fakeWideString = fake_class("WideString", 10, 0);
	fakeWideSymbol = fake_class("WideSymbol", 10, 0);
	fakeMyString = fake_class("MyString", 16, 0);
	fakeByteArray = fake_class("ByteArray", 16, 0);
	fakeDoubleByteArray = fake_class("DoubleByteArray", 12, 0);
	fakeWordArray = fake_class("WordArray", 10, 0);
	fakeBitmap = fake_class("Bitmap", 10, 0);
	fakeIntegerArray = fake_class("IntegerArray", 10, 0);
	fakeFloat32Array = fake_class("Float32Array", 10, 0);
	fakeFloat64Array = fake_class("Float64Array", 9, 0);
	fakeDoubleWordArray = fake_class("DoubleWordArray", 9, 0);

	memset(&proxy, 0, sizeof proxy);
	proxy.minorVersion = fMinorVersion;
	proxy.majorVersion = fMajorVersion;
	proxy.pop = fPop;
	proxy.popthenPush = fPopthenPush;
	proxy.push = fPush;
	proxy.pushInteger = fPushInteger;
	proxy.stackValue = fStackValue;
	proxy.byteSizeOf = fByteSizeOf;
	proxy.fetchClassOf = fFetchClassOf;
	proxy.fetchPointerofObject = fFetchPointerofObject;
	proxy.firstIndexableField = fFirstIndexableField;
	proxy.methodArgumentCount = fMethodArgumentCount;
	proxy.slotSizeOf = fSlotSizeOf;
	proxy.storePointerofObjectwithValue = fStorePointerofObjectwithValue;
	proxy.isBytes = fIsBytes;
	proxy.isFloatObject = fIsFloatObject;
	proxy.isIntegerObject = fIsIntegerObject;
	proxy.isPointers = fIsPointers;
	proxy.isWords = fIsWords;
	proxy.isWordsOrBytes = fIsWordsOrBytes;
	proxy.floatObjectOf = fFloatObjectOf;
	proxy.floatValueOf = fFloatValueOf;
	proxy.integerObjectOf = fIntegerObjectOf;
	proxy.integerValueOf = fIntegerValueOf;
	proxy.positive32BitIntegerFor = fPositive32BitIntegerFor;
	proxy.falseObject = fFalseObject;
	proxy.nilObject = fNilObject;
	proxy.trueObject = fTrueObject;
	proxy.classArray = fClassArray;
	proxy.classByteArray = fClassByteArray;
	proxy.classBitmap = fClassBitmap;
	proxy.classFloat = fClassFloat;
	proxy.classLargePositiveInteger = fClassLargePositiveInteger;
	proxy.classString = fClassString;
	proxy.clone = fClone;
	proxy.instantiateClassindexableSize = fInstantiateClassindexableSize;
	proxy.failed = fFailed;
	proxy.primitiveFail = fPrimitiveFail;
	proxy.success = fSuccess;
	proxy.stringForCString = fStringForCString;
	proxy.classLargeNegativeInteger = fClassLargeNegativeInteger;
	proxy.signed32BitIntegerFor = fSigned32BitIntegerFor;
	proxy.positive64BitIntegerFor = fPositive64BitIntegerFor;
	proxy.signed64BitIntegerFor = fSigned64BitIntegerFor;
	proxy.isArray = fIsArray;
	proxy.primitiveFailFor = fPrimitiveFailFor;
	proxy.isOopImmutable = fIsOopImmutable;
	proxy.methodReturnBool = fMethodReturnBool;
	proxy.methodReturnString = fMethodReturnString;
	proxy.methodReturnInteger = fMethodReturnInteger;
	proxy.methodReturnValue = fMethodReturnValue;
	proxy.primitiveFailureCode = fPrimitiveFailureCode;
	proxy.isImmediate = fIsImmediate;
	proxy.isYoung = fIsYoung;
	return &proxy;
}

FakeAnswer
fake_call(sqInt (*primitive)(void), sqInt receiver, int nargs, const sqInt *args)
{
	FakeAnswer answer;
	size_t i;
	int k;

	if (nargs < 0 || nargs >= (int)(sizeof stack / sizeof stack[0])) {
		fprintf(stderr, "fakevm: %d arguments\n", nargs);
		exit(2);
	}
	memset(&current, 0, sizeof current);
	lastError[0] = 0;
	stack[0] = receiver;
	for (k = 0; k < nargs; k++) {
		stack[k + 1] = args[k];
	}
	stackArgs = nargs;
	primFailCode = 0;
	inCall = 1;
	primitive();
	inCall = 0;
	current.code = primFailCode;
	if (!current.returned && !primFailCode) {
		protocolError("neither answered nor failed");
	}
	if (current.returned && primFailCode) {
		protocolError("failed with %lld after answering", (long long)primFailCode);
	}
	for (i = 0; i < objectCount; i++) {
		objects[i]->madeInCall = 0;
	}
	answer = current;
	return answer;
}
