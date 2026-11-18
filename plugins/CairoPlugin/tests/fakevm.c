/* fakevm.c -- a stand-in for the VM, for the C tests of the CairoPlugin's
 * glue: see fakevm.h.  Only the functions of the proxy that the glue may
 * call are real; the ones it must never call record a protocol error; the
 * others are NULL. */

#ifndef _GNU_SOURCE
# define _GNU_SOURCE		/* RTLD_DEFAULT */
#endif
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifndef __EMSCRIPTEN__
# include <dlfcn.h>
#endif

#include "fakevm.h"

#define MAGIC 0x46616b65u	/* "Fake" */
#define SMALLINTEGER_MAX (((sqInt)1 << 60) - 1)
#define SMALLINTEGER_MIN (-((sqInt)1 << 60))
#define FORWARDER_FORMAT 7

typedef struct {
	uint32_t magic;
	int format;		/* Spur's: 0, 1, 2, 4, 7, 9, 10, 12, 16, 24 */
	int immutable;
	int madeInCall;		/* by instantiateClassindexableSize in this call */
	sqInt cls;
	size_t size;		/* slots (formats 1, 2, 4), elements (9, 10, 12), bytes (16, 24) */
	sqInt *slots;
	unsigned char *bytes;
	const char *name;	/* of a class */
	sqInt forwardee;	/* of a forwarder */
} Obj;

sqInt fakeMetaclass, fakeUndefinedObject, fakeTrue, fakeFalse, fakeSmallInteger, fakeCharacter;
sqInt fakeSmallFloat, fakeBoxedFloat, fakeLargePositiveInteger, fakeLargeNegativeInteger, fakeFraction;
sqInt fakeArray, fakeWeakArray, fakeCompiledMethod;
sqInt fakeByteString, fakeByteSymbol, fakeWideString, fakeWideSymbol;
sqInt fakeByteArray, fakeWordArray, fakeExternalAddress, fakePoint, fakeColor, fakeRectangle;
sqInt fakeAthensCairoCanvas, fakeAthensCairoSurface, fakeAthensCairoPathBuilder;
sqInt fakeAthensCairoPaintMode, fakeAthensCairoStrokePaint, fakeCairoFreetypeFontRenderer;
sqInt fakeAthensCairoMatrix, fakeAthensCairoPath, fakeCairoScaledFont;
sqInt fakeCairoTextExtents, fakeCairoFontExtents, fakeAthensCairoPatternPaint;
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

/* The allocator */
static long budget = -1;
static sqInt budgetCode = PrimErrNoMemory;

/* GC roots */
#define MAX_ROOTS 64
static sqInt *roots[MAX_ROOTS];
static int rootCount;

/* The module loader */
#define MAX_HIDDEN 16
static int loaderFailModules;
static char loaderHidden[MAX_HIDDEN][64];
static int loaderHiddenCount;
static char loaderOverrideName[64];
static void *loaderOverrideAddress;
static int loaderModuleCalls, loaderSymbolCalls, loaderNullModuleCalls;
static char loaderLastModule[256];

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

static void
outOfMemory(void)
{
	fprintf(stderr, "fakevm: out of memory\n");
	exit(2);
}

static Obj *
newObject(sqInt cls, int format, size_t size)
{
	Obj *o = calloc(1, sizeof *o);

	if (!o) {
		outOfMemory();
	}
	o->magic = MAGIC;
	o->format = format;
	o->cls = cls;
	o->size = size;
	if (format == 1 || format == 2 || format == 4) {
		size_t i;
		o->slots = malloc((size ? size : 1) * sizeof(sqInt));
		if (!o->slots) {
			outOfMemory();
		}
		for (i = 0; i < size; i++) {
			o->slots[i] = fakeNil;
		}
	} else if (format >= 9) {
		/* words: the exact size, so that AddressSanitizer sees an access
		   past the end; bytes: rounded up to 8 and zero-filled, as Spur's
		   bodies (cp_cstring reads that padding); 8-aligned (malloc) */
		size_t bytes = bytesOf(format, size);
		size_t room = format >= 16 ? (bytes + 7) & ~(size_t)7 : bytes;
		o->bytes = calloc(room ? room : 1, 1);
		if (!o->bytes) {
			outOfMemory();
		}
	}
	if (objectCount == objectCapacity) {
		objectCapacity = objectCapacity ? 2 * objectCapacity : 1024;
		objects = realloc(objects, objectCapacity * sizeof *objects);
		if (!objects) {
			outOfMemory();
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
			outOfMemory();
		}
	}
	smallFloats[smallFloatCount] = value;
	return (sqInt)((usqInt)smallFloatCount++ << 3) | 4;
}

/* As the VM's isSmallFloatValue (64-bit Spur) */
static int
isSmallFloatValue(double d)
{
	uint64_t raw, exponent;

	memcpy(&raw, &d, sizeof raw);
	exponent = (raw >> 52) & 0x7FF;
	return exponent > 896
		? exponent <= 0xFF + 896
		: (raw & ((1ULL << 52) - 1)) == 0 ? exponent == 0 : exponent == 896;
}

sqInt
fake_float(double value)
{
	return isSmallFloatValue(value) ? fake_small_float(value) : fake_boxed_float(value);
}


/*** Objects ***/

/* A class whose class is a new metaclass */
static sqInt
makeClass(const char *name, int instSpec, int fixedFields)
{
	Obj *metaclass, *cls;

	metaclass = newObject(fakeMetaclass, 1, 3);
	metaclass->slots[2] = fake_int((1 << 16) | 3);
	metaclass->name = name;
	cls = newObject((sqInt)metaclass, 1, 3);
	cls->slots[2] = fake_int(((sqInt)instSpec << 16) | fixedFields);
	cls->name = name;
	return (sqInt)cls;
}

sqInt
fake_class(const char *name, int instSpec, int fixedFields)
{
	return makeClass(name, instSpec, fixedFields);
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
fake_string(const char *s)
{
	return fake_bytes(fakeByteString, s, strlen(s));
}

sqInt
fake_array(sqInt aClass, const sqInt *oops, size_t size)
{
	sqInt oop = fake_new(aClass, size);
	Obj *o = (Obj *)oop;

	if (size > o->size) {
		fprintf(stderr, "fakevm: fake_array of %zu oops into %zu slots\n", size, o->size);
		exit(2);
	}
	if (size) {
		memcpy(o->slots, oops, size * sizeof(sqInt));
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
fake_external_address(void *pointer)
{
	return fake_bytes(fakeExternalAddress, &pointer, sizeof pointer);
}

sqInt
fake_point(sqInt x, sqInt y)
{
	sqInt oops[2] = { x, y };

	return fake_array(fakePoint, oops, 2);
}

sqInt
fake_color(sqInt rgb, sqInt alpha)
{
	/* rgb cachedDepth cachedBitPattern alpha */
	sqInt oops[4] = { rgb, fakeNil, fakeNil, alpha };

	return fake_array(fakeColor, oops, 4);
}

sqInt
fake_rectangle(sqInt origin, sqInt corner)
{
	sqInt oops[2] = { origin, corner };

	return fake_array(fakeRectangle, oops, 2);
}

sqInt
fake_compiled_method(size_t size)
{
	return (sqInt)newObject(fakeCompiledMethod, 24, size);
}

sqInt
fake_forwarder(void)
{
	return fake_forwarder_to(fakeNil);
}

sqInt
fake_forwarder_to(sqInt target)
{
	Obj *o = newObject(fakeNil, FORWARDER_FORMAT, 0);

	o->forwardee = target;
	return (sqInt)o;
}

int
fake_is_forwarder(sqInt oop)
{
	return fake_is_object(oop) && ((Obj *)oop)->format == FORWARDER_FORMAT;
}

sqInt
fake_follow(sqInt oop)
{
	while (fake_is_forwarder(oop)) {
		oop = ((Obj *)oop)->forwardee;
	}
	return oop;
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
	return ((Obj *)oop)->slots[index0];
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

void *
fake_address_value(sqInt oop)
{
	void *p;

	memcpy(&p, ((Obj *)oop)->bytes, sizeof p);
	return p;
}

void
fake_set_budget(long allowed, sqInt failCode)
{
	budget = allowed;
	budgetCode = failCode;
}

int
fake_gc_root_count(void)
{
	return rootCount;
}

sqInt
fake_gc_root_value(int i)
{
	return i >= 0 && i < rootCount ? *roots[i] : 0;
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
	return (sqInt)bytesOf(o->format, o->size);
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
	if (o->immutable) {
		protocolError("storePointerofObjectwithValue into a read-only %s", fake_class_name(o->cls));
	}
	if (!o->madeInCall) {
		current.foreignStores++;
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

/* Only for the SmallFloat64s, which it does not allocate */
static sqInt
fFloatObjectOf(double d)
{
	if (!isSmallFloatValue(d)) {
		protocolError("floatObjectOf(%g): an eden-only allocation", d);
	}
	return fake_small_float(d);
}

static sqInt fNilObject(void) { return fakeNil; }
static sqInt fTrueObject(void) { return fakeTrueObject; }
static sqInt fFalseObject(void) { return fakeFalseObject; }
static sqInt fClassArray(void) { return fakeArray; }
static sqInt fClassByteArray(void) { return fakeByteArray; }
static sqInt fClassFloat(void) { return fakeBoxedFloat; }
static sqInt fClassString(void) { return fakeByteString; }
static sqInt fClassPoint(void) { return fakePoint; }
static sqInt fClassExternalAddress(void) { return fakeExternalAddress; }
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
	if (size < 0 || (instSpec == 1 ? size != 0 : fixed != 0)
	 || (instSpec != 1 && instSpec != 2 && instSpec != 9 && instSpec != 10 && instSpec != 12 && instSpec != 16)) {
		primFailCode = PrimErrBadReceiver;
		return 0;
	}
	o = newObject(aClass, instSpec, instSpec == 1 ? (size_t)fixed : (size_t)size);
	if (budget > 0) {
		budget--;
	}
	o->madeInCall = 1;
	current.allocated++;
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
fMethodReturnReceiver(void)
{
	return fMethodReturnValue(stack[0]);
}

static sqInt
fAddGCRoot(sqInt *varLoc)
{
	if (rootCount >= MAX_ROOTS) {
		return 0;
	}
	roots[rootCount++] = varLoc;
	return 1;
}

static sqInt
fRemoveGCRoot(sqInt *varLoc)
{
	int i;

	for (i = 0; i < rootCount; i++) {
		if (roots[i] == varLoc) {
			roots[i] = roots[--rootCount];
			return 1;
		}
	}
	return 0;
}


/*** The module loader ***/

void
fake_loader_reset(void)
{
	loaderFailModules = 0;
	loaderHiddenCount = 0;
	loaderOverrideName[0] = 0;
	loaderOverrideAddress = NULL;
	loaderModuleCalls = loaderSymbolCalls = loaderNullModuleCalls = 0;
	loaderLastModule[0] = 0;
}

void
fake_loader_fail_modules(int fail)
{
	loaderFailModules = fail;
}

void
fake_loader_hide(const char *name)
{
	if (loaderHiddenCount < MAX_HIDDEN) {
		snprintf(loaderHidden[loaderHiddenCount++], sizeof loaderHidden[0], "%s", name);
	}
}

void
fake_loader_override(const char *name, void *address)
{
	snprintf(loaderOverrideName, sizeof loaderOverrideName, "%s", name);
	loaderOverrideAddress = address;
}

int fake_loader_module_calls(void) { return loaderModuleCalls; }
int fake_loader_symbol_calls(void) { return loaderSymbolCalls; }
int fake_loader_null_module_calls(void) { return loaderNullModuleCalls; }
const char *fake_loader_last_module(void) { return loaderLastModule; }

/* As the VM's ioLoadModuleOfLength: the name is an oop index
   (pointerForOop) of at most 255 bytes */
static void *
fIoLoadModuleOfLength(sqInt moduleNameIndex, sqInt moduleNameLength)
{
	char name[256];

	loaderModuleCalls++;
	if (moduleNameLength < 0 || moduleNameLength > 255) {
		return NULL;
	}
	memcpy(name, pointerForOop((usqInt)moduleNameIndex), (size_t)moduleNameLength);
	name[moduleNameLength] = 0;
	snprintf(loaderLastModule, sizeof loaderLastModule, "%s", name);
	if (loaderFailModules) {
		return NULL;
	}
#ifdef __EMSCRIPTEN__
	return NULL;
#else
	return dlopen(name, RTLD_NOW | RTLD_LOCAL);
#endif
}

static void *
fIoLoadSymbolOfLengthFromModule(sqInt fnIndex, sqInt fnLength, void *handle)
{
	char name[256];
	int i;

	loaderSymbolCalls++;
	if (fnLength < 0 || fnLength > 255) {
		return NULL;
	}
	memcpy(name, pointerForOop((usqInt)fnIndex), (size_t)fnLength);
	name[fnLength] = 0;
	if (!handle) {
		loaderNullModuleCalls++;
		protocolError("ioLoadSymbolOfLengthFromModule(%s) with a NULL module: a search of the whole process", name);
		return NULL;
	}
	for (i = 0; i < loaderHiddenCount; i++) {
		if (strcmp(loaderHidden[i], name) == 0) {
			return NULL;
		}
	}
	if (loaderOverrideName[0] && strcmp(loaderOverrideName, name) == 0) {
		return loaderOverrideAddress;
	}
#ifdef __EMSCRIPTEN__
	return NULL;
#else
	return dlsym(handle, name);
#endif
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

/* A class of fixed slots */
static sqInt
fixedClass(const char *name, int slots)
{
	return fake_class(name, 1, slots);
}

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
	fakeFraction = fixedClass("Fraction", 2);
	fakeArray = fake_class("Array", 2, 0);
	fakeWeakArray = fake_class("WeakArray", 4, 0);
	fakeCompiledMethod = fake_class("CompiledMethod", 24, 0);
	fakeByteString = fake_class("ByteString", 16, 0);
	fakeByteSymbol = fake_class("ByteSymbol", 16, 0);
	fakeWideString = fake_class("WideString", 10, 0);
	fakeWideSymbol = fake_class("WideSymbol", 10, 0);
	fakeByteArray = fake_class("ByteArray", 16, 0);
	fakeWordArray = fake_class("WordArray", 10, 0);
	fakeExternalAddress = fake_class("ExternalAddress", 16, 0);
	fakePoint = fixedClass("Point", 2);
	fakeColor = fixedClass("Color", 4);
	fakeRectangle = fixedClass("Rectangle", 2);
	fakeAthensCairoCanvas = fixedClass("AthensCairoCanvas", 10);
	fakeAthensCairoSurface = fixedClass("AthensCairoSurface", 7);
	fakeAthensCairoPathBuilder = fixedClass("AthensCairoPathBuilder", 4);
	fakeAthensCairoPaintMode = fixedClass("AthensCairoPaintMode", 1);
	fakeAthensCairoStrokePaint = fixedClass("AthensCairoStrokePaint", 7);
	fakeCairoFreetypeFontRenderer = fixedClass("CairoFreetypeFontRenderer", 7);
	fakeAthensCairoMatrix = fixedClass("AthensCairoMatrix", 1);
	fakeAthensCairoPath = fixedClass("AthensCairoPath", 1);
	fakeCairoScaledFont = fixedClass("CairoScaledFont", 3);
	fakeCairoTextExtents = fixedClass("CairoTextExtents", 1);
	fakeCairoFontExtents = fixedClass("CairoFontExtents", 1);
	fakeAthensCairoPatternPaint = fixedClass("AthensCairoPatternPaint", 1);

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
	proxy.classFloat = fClassFloat;
	proxy.classPoint = fClassPoint;
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
	proxy.classExternalAddress = fClassExternalAddress;
	proxy.ioLoadModuleOfLength = fIoLoadModuleOfLength;
	proxy.ioLoadSymbolOfLengthFromModule = fIoLoadSymbolOfLengthFromModule;
	proxy.positive64BitIntegerFor = fPositive64BitIntegerFor;
	proxy.signed64BitIntegerFor = fSigned64BitIntegerFor;
	proxy.isArray = fIsArray;
	proxy.addGCRoot = fAddGCRoot;
	proxy.removeGCRoot = fRemoveGCRoot;
	proxy.primitiveFailFor = fPrimitiveFailFor;
	proxy.isOopImmutable = fIsOopImmutable;
	proxy.methodReturnBool = fMethodReturnBool;
	proxy.methodReturnString = fMethodReturnString;
	proxy.methodReturnInteger = fMethodReturnInteger;
	proxy.methodReturnValue = fMethodReturnValue;
	proxy.primitiveFailureCode = fPrimitiveFailureCode;
	proxy.isImmediate = fIsImmediate;
	proxy.methodReturnReceiver = fMethodReturnReceiver;
	fake_loader_reset();
	return &proxy;
}


/*** Calls ***/

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

/* As followForwardedObjectFields:toDepth: of the VM: follows the forwarders
   in the slots of oop, and with depth > 0 those in the slots of its pointers
   slots, to depth - 1; answers whether it found one */
static int
followFields(sqInt oop, int depth)
{
	Obj *o = (Obj *)oop;
	int found = 0;
	size_t i;

	for (i = 0; i < o->size; i++) {
		sqInt field = o->slots[i];
		if (!isImmediateOop(field)) {
			if (fake_is_forwarder(field)) {
				found = 1;
				field = o->slots[i] = fake_follow(field);
			}
			if (depth > 0 && !isImmediateOop(field) && fake_is_object(field)
			 && ((Obj *)field)->format >= 1 && ((Obj *)field)->format <= 5
			 && followFields(field, depth - 1)) {
				found = 1;
			}
		}
	}
	return found;
}

FakeAnswer
fake_call_retry(sqInt (*primitive)(void), int depth, sqInt receiver, int nargs, const sqInt *args)
{
	FakeAnswer first, answer;
	sqInt values[16];
	int k, found = 0;

	first = fake_call(primitive, receiver, nargs, args);
	if (!first.code || depth < 0) {
		first.firstCode = first.code;
		return first;
	}
	/* as checkForAndFollowForwardedPrimitiveState: the receiver and the
	   arguments, then their fields to the depth */
	values[0] = receiver;
	for (k = 0; k < nargs; k++) {
		values[k + 1] = args[k];
	}
	for (k = 0; k <= nargs; k++) {
		sqInt oop = values[k];
		if (isImmediateOop(oop)) {
			continue;
		}
		if (fake_is_forwarder(oop)) {
			found = 1;
			oop = values[k] = fake_follow(oop);
		}
		if (depth > 0 && fake_is_object(oop) && ((Obj *)oop)->format >= 1 && ((Obj *)oop)->format <= 5
		 && followFields(oop, depth)) {
			found = 1;
		}
	}
	if (!found) {
		first.firstCode = first.code;
		return first;
	}
	answer = fake_call(primitive, values[0], nargs, values + 1);
	answer.retried = 1;
	answer.firstCode = first.code;
	answer.protocolErrors += first.protocolErrors;
	answer.allocations += first.allocations;
	answer.allocated += first.allocated;
	answer.stores += first.stores;
	answer.foreignStores += first.foreignStores;
	return answer;
}


#ifdef __EMSCRIPTEN__
/* The VM's: its sqPlatformSpecific.h makes exit() of the files that include
   it (the plugin, the glue tests) emscriptenExit() */
void emscriptenExit(int status) __attribute__((noreturn));
void
emscriptenExit(int status)
{
	fflush(stdout);
	fflush(stderr);
	(exit)(status);
}
#endif


/*** Lookups ***/

#ifdef SQUEAK_BUILTIN_PLUGIN
extern void *CairoPlugin_exports[][3];

/* The row of name, or -1 */
static int
exportsRow(const char *name)
{
	int i;

	for (i = 0; CairoPlugin_exports[i][1]; i++) {
		if (strcmp((const char *)CairoPlugin_exports[i][1], name) == 0) {
			return i;
		}
	}
	return -1;
}
#endif

sqInt
(*fake_primitive(const char *name))(void)
{
#ifdef SQUEAK_BUILTIN_PLUGIN
	int row = exportsRow(name);

	return row < 0 ? NULL : (sqInt (*)(void))CairoPlugin_exports[row][2];
#else
	return (sqInt (*)(void))dlsym(RTLD_DEFAULT, name);
#endif
}

int
fake_accessor_depth(const char *name)
{
#ifdef SQUEAK_BUILTIN_PLUGIN
	/* the depth is the byte after the name's NUL: "name\000\ddd" */
	int row = exportsRow(name);

	return row < 0 ? -1 : (signed char)((const char *)CairoPlugin_exports[row][1])[strlen(name) + 1];
#else
	char symbol[128];
	signed char *depth;

	snprintf(symbol, sizeof symbol, "%sAccessorDepth", name);
	depth = (signed char *)dlsym(RTLD_DEFAULT, symbol);
	return depth ? *depth : -1;
#endif
}

int
fake_read_primitives(const char *path, FakePrimitiveRow *rows, int max)
{
	FILE *f = fopen(path, "r");
	char line[256];
	int n = 0;

	if (!f) {
		return -1;
	}
	while (n < max && fgets(line, sizeof line, f)) {
		FakePrimitiveRow row;
		if (line[0] == '#' || line[0] == '\n') {
			continue;
		}
		if (sscanf(line, "%63s %d %15s %63s %d", row.name, &row.depth, row.group, row.file, &row.nargs) == 5) {
			rows[n++] = row;
		}
	}
	fclose(f);
	return n;
}
