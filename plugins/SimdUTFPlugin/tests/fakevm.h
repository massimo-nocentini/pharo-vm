/* fakevm.h -- a stand-in for the VM, for the tests of the glue
 * (SimdUTFPlugin.c) in gluetest.c: a struct VirtualMachine whose functions
 * work on objects of this file.
 *
 * An object is a malloc'd record, and its oop is the record's address
 * (8-aligned, so the tag bits are free); a SmallInteger is tagged as in
 * 64-bit Spur, (value << 3) | 1.  Objects have one of Spur's formats: 0
 * (nil), 1 (named slots: classes and metaclasses), 2 (Array), 10 (32-bit
 * words), 16 (bytes) and 24 (a CompiledMethod: isBytes, but
 * firstIndexableField answers NULL).  A class holds its superclass, its
 * method dictionary (nil) and its format word in its first three slots,
 * and its class is a metaclass, whose class is Metaclass, as in Pharo.
 * Bodies are allocated at their exact size, so that AddressSanitizer sees
 * a write past the end.
 *
 * fake_call runs a primitive with a receiver and arguments on a fake stack,
 * and records what it did: the failure code, the value it answered, the
 * allocations it asked for and how many succeeded.  The allocator has a
 * budget: when it is spent, instantiateClassindexableSize fails with the
 * code given (PrimErrNoMemory, or another to test the glue's handling).
 * The functions the glue must never call (eden-only allocators, stack
 * manipulation, ...) record a protocol error, and so do: answering after
 * failing, a store into an object other than one primitiveInfo made, an
 * oop that is not an object of this file.
 */

#ifndef FAKEVM_H
#define FAKEVM_H

#include <stddef.h>
#include <stdint.h>

#include "sqConfig.h"
#include "virtualMachine.h"

/* The classes, built by fake_init */
extern sqInt fakeMetaclass, fakeUndefinedObject, fakeSmallInteger;
extern sqInt fakeArray, fakeByteArray, fakeByteString, fakeByteSymbol, fakeWideString, fakeWideSymbol;
extern sqInt fakeWordArray, fakeFloat, fakeCompiledMethod, fakeOtherWide, fakeFixedWide;
extern sqInt fakeNil;

/* The proxy; fake_init builds the classes and fills it */
struct VirtualMachine *fake_init(void);
/* Frees every object made since fake_init (the classes too) */
void fake_free_all(void);
/* The objects made so far, and the freeing of those made after a mark */
size_t fake_mark(void);
void fake_release(size_t mark);

/* SmallIntegers */
sqInt fake_int(sqInt value);
int fake_is_int(sqInt oop);
sqInt fake_int_value(sqInt oop);

/* A new class of the given Spur instance specification and named slots */
sqInt fake_class(const char *name, int instSpec, int fixedFields);
/* A new instance of aClass with size indexable units, zero-filled (nil in
   the slots of an Array); outside a primitive: not counted */
sqInt fake_new(sqInt aClass, size_t size);
/* Instances holding the given units */
sqInt fake_bytes(sqInt aClass, const void *bytes, size_t size);
sqInt fake_words(sqInt aClass, const uint32_t *words, size_t size);
/* A CompiledMethod of size bytes (firstIndexableField answers NULL) */
sqInt fake_compiled_method(size_t size);

int fake_is_object(sqInt oop);
sqInt fake_class_of(sqInt oop);
const char *fake_class_name(sqInt aClass);
int fake_format(sqInt oop);
/* Indexable units: slots, words or bytes */
size_t fake_size(sqInt oop);
/* The first byte of the body of a word or byte object, or the slots */
void *fake_body(sqInt oop);
sqInt fake_slot(sqInt oop, size_t index0);
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

#endif /* FAKEVM_H */
