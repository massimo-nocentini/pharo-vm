/* fakevm.h -- a stand-in for the VM, for the tests of the glue
 * (TimSortPlugin.c) in gluetest.c: a struct VirtualMachine whose functions
 * work on objects of this file (SimdUTFPlugin's tests/fakevm.h, extended
 * with the objects a sort meets).
 *
 * An object is a malloc'd record, and its oop is the record's address
 * (8-aligned, so the tag bits are free).  Immediates are tagged as in
 * 64-bit Spur: a SmallInteger (value << 3) | 1, a Character
 * (code << 3) | 2, a SmallFloat64 (index << 3) | 4, where index is its
 * place in a table of doubles of this file (the glue only ever reads it
 * through isFloatObject and floatValueOf).  Objects have one of Spur's
 * formats: 0 (nil, true, false), 1 (named slots: classes, metaclasses,
 * Fraction), 2 (Array and its subclasses), 4 (WeakArray), 7 (a forwarder:
 * its class is nil, and reading its body is a protocol error), 9 (64-bit
 * elements), 10 (32-bit words: WideString, BoxedFloat64 and the word
 * buffers), 12 (16-bit), 16 (bytes, LargeIntegers included) and 24 (a
 * CompiledMethod: isBytes, but firstIndexableField answers NULL).  A class
 * holds its superclass, its method dictionary (nil) and its format word in
 * its first three slots, and its class is a metaclass, whose class is
 * Metaclass, as in Pharo.  Bodies are allocated at their exact size, so
 * that AddressSanitizer sees an access past the end.
 *
 * fake_call runs a primitive with a receiver and arguments on a fake stack,
 * and records what it did: the failure code, the value it answered, the
 * allocations it asked for and how many succeeded, the stores.  The
 * allocator has a budget: when it is spent, instantiateClassindexableSize
 * fails with the code given (PrimErrNoMemory, or another).  The functions
 * the glue must never call (eden-only allocators, stack manipulation, ...)
 * record a protocol error, and so do: answering after failing, failing
 * after answering, neither, a store into an object other than one the call
 * made, an oop that is not an object of this file, reading the body or the
 * size of a forwarder, floatValueOf of a non-Float, integerValueOf of a
 * non-SmallInteger.
 */

#ifndef TS_FAKEVM_H
#define TS_FAKEVM_H

#include <stddef.h>
#include <stdint.h>

#include "sqConfig.h"
#include "virtualMachine.h"

/* The classes, built by fake_init */
extern sqInt fakeMetaclass, fakeUndefinedObject, fakeTrue, fakeFalse, fakeSmallInteger, fakeCharacter;
extern sqInt fakeSmallFloat, fakeBoxedFloat, fakeLargePositiveInteger, fakeLargeNegativeInteger, fakeFraction;
extern sqInt fakeArray, fakeArraySubclass, fakeWeakArray, fakeCompiledMethod;
extern sqInt fakeByteString, fakeByteSymbol, fakeWideString, fakeWideSymbol, fakeMyString;
extern sqInt fakeByteArray, fakeDoubleByteArray, fakeWordArray, fakeBitmap, fakeIntegerArray;
extern sqInt fakeFloat32Array, fakeFloat64Array, fakeDoubleWordArray;
/* nil, true, false */
extern sqInt fakeNil, fakeTrueObject, fakeFalseObject;

/* The proxy; fake_init builds the classes and fills it */
struct VirtualMachine *fake_init(void);
/* Frees every object made since fake_init (the classes too) */
void fake_free_all(void);
/* The objects made so far, and the freeing of those made after a mark */
size_t fake_mark(void);
void fake_release(size_t mark);

/* Immediates */
sqInt fake_int(sqInt value);
int fake_is_int(sqInt oop);
sqInt fake_int_value(sqInt oop);
sqInt fake_character(uint32_t code);
sqInt fake_small_float(double value);

/* A new class of the given Spur instance specification and named slots */
sqInt fake_class(const char *name, int instSpec, int fixedFields);
/* A new instance of aClass with size indexable units, zero-filled (nil in
   the slots of an Array); outside a primitive: not counted */
sqInt fake_new(sqInt aClass, size_t size);
/* Instances holding the given units */
sqInt fake_bytes(sqInt aClass, const void *bytes, size_t size);
sqInt fake_words(sqInt aClass, const uint32_t *words, size_t size);
/* An Array (or an instance of another class of format 2 or 4) of the oops */
sqInt fake_array(sqInt aClass, const sqInt *oops, size_t size);
/* A BoxedFloat64 */
sqInt fake_boxed_float(double value);
/* A LargePositiveInteger or LargeNegativeInteger of the bytes given (high
   zero bytes kept) */
sqInt fake_large(const uint8_t *digits, size_t size, int negative);
/* A CompiledMethod of size bytes (firstIndexableField answers NULL) */
sqInt fake_compiled_method(size_t size);
/* A forwarder: an object whose class is nil (Spur's class index 8) */
sqInt fake_forwarder(void);

int fake_is_object(sqInt oop);
sqInt fake_class_of(sqInt oop);
const char *fake_class_name(sqInt aClass);
int fake_format(sqInt oop);
/* Whether oop is a Float (immediate or boxed), and its value */
int fake_is_float(sqInt oop);
double fake_float_value(sqInt oop);
/* Indexable units: slots, elements or bytes */
size_t fake_size(sqInt oop);
/* The bytes of the body: size units of the format's width */
size_t fake_body_bytes(sqInt oop);
/* The first byte of the body of a non-pointer object, or the slots */
void *fake_body(sqInt oop);
sqInt fake_slot(sqInt oop, size_t index0);
void fake_set_slot(sqInt oop, size_t index0, sqInt value);
void fake_set_immutable(sqInt oop, int immutable);

/* The allocator: allowed allocations (-1: no limit) before
   instantiateClassindexableSize fails with failCode */
void fake_set_budget(long allowed, sqInt failCode);

typedef struct {
	sqInt code;		/* primFailCode after the call: 0 or a PrimErr code */
	int returned;		/* methodReturnValue was called */
	sqInt value;		/* its argument */
	int allocations;	/* calls of instantiateClassindexableSize */
	int allocated;		/* of which succeeded */
	int stores;		/* calls of storePointerofObjectwithValue */
	int protocolErrors;	/* see above; fake_last_error says the first */
} FakeAnswer;

/* Runs primitive with receiver and nargs arguments (the first argument
   first) and answers what it did */
FakeAnswer fake_call(sqInt (*primitive)(void), sqInt receiver, int nargs, const sqInt *args);
const char *fake_last_error(void);

#endif /* TS_FAKEVM_H */
