/* fakevm.h -- a stand-in for the VM, for the C tests of the CairoPlugin's
 * glue (tests/glue-*.c): a struct VirtualMachine whose functions work on
 * objects of this file (TimSortPlugin's tests/fakevm.h, extended with what
 * the CairoPlugin meets: ExternalAddresses, Athens-shaped objects,
 * forwarders and the VM's retry, the module loader, GC roots).
 *
 * An object is a malloc'd record, and its oop is the record's address
 * (8-aligned, so the tag bits are free).  Immediates are tagged as in 64-bit
 * Spur: a SmallInteger (value << 3) | 1, a Character (code << 3) | 2, a
 * SmallFloat64 (index << 3) | 4, where index is its place in a table of
 * doubles of this file.  Objects have one of Spur's formats: 0 (nil, true,
 * false), 1 (named slots: classes, Point, Color, the Athens objects), 2
 * (Array), 4 (WeakArray), 7 (a forwarder: its class is nil, reading its body
 * is a protocol error), 9 (64-bit), 10 (32-bit words: WideString,
 * BoxedFloat64), 12 (16-bit), 16 (bytes: ByteString, ByteArray,
 * ExternalAddress) and 24 (a CompiledMethod: isBytes, but not isWordsOrBytes,
 * and firstIndexableField answers NULL).  A class holds its superclass, its
 * method dictionary (nil) and its format word in its first three slots.
 * Bodies of non-byte objects are allocated at their exact size, so that
 * AddressSanitizer sees an access past the end; byte bodies are rounded up
 * to 8 bytes and zero-filled past their size, as Spur's are (cp_cstring reads
 * that padding, never further).
 *
 * fake_call runs a primitive with a receiver and arguments on a fake stack,
 * and records what it did: the failure code, the value it answered, the
 * allocations, the stores.  fake_call_retry does what the VM does when a
 * primitive fails: it follows the forwarders of the stack values to the
 * primitive's accessor depth (as followForwardedObjectFields:toDepth:, in
 * place) and, when it found one, calls the primitive again.
 *
 * Protocol errors (fake_last_error says the first): answering after
 * failing, failing after answering, neither; a store into a read-only
 * object, a forwarder or a non-pointers object; an oop that is not an object
 * of this file; reading the body or the size of a forwarder; floatValueOf of
 * a non-Float, integerValueOf of a non-SmallInteger; the eden-only
 * allocators (clone, stringForCString, signed64BitIntegerFor, ...;
 * floatObjectOf of a value that is not a SmallFloat64); a symbol lookup with
 * a NULL module.
 *
 * The module loader (natively): ioLoadModuleOfLength dlopens the name with
 * RTLD_NOW | RTLD_LOCAL (never RTLD_DEEPBIND, which AddressSanitizer
 * refuses), or answers NULL when told to; ioLoadSymbolOfLengthFromModule
 * dlsyms in that handle, but answers NULL for hidden names and the override
 * for overridden ones.  Both count their calls.  Under Emscripten there is
 * no loader (the plugin binds statically).
 */

#ifndef CP_FAKEVM_H
#define CP_FAKEVM_H

#include <stddef.h>
#include <stdint.h>

#include "sqConfig.h"
#include "virtualMachine.h"

/* The classes, built by fake_init */
extern sqInt fakeMetaclass, fakeUndefinedObject, fakeTrue, fakeFalse, fakeSmallInteger, fakeCharacter;
extern sqInt fakeSmallFloat, fakeBoxedFloat, fakeLargePositiveInteger, fakeLargeNegativeInteger, fakeFraction;
extern sqInt fakeArray, fakeWeakArray, fakeCompiledMethod;
extern sqInt fakeByteString, fakeByteSymbol, fakeWideString, fakeWideSymbol;
extern sqInt fakeByteArray, fakeWordArray, fakeExternalAddress, fakePoint, fakeColor, fakeRectangle;
/* Athens: the instance variables of Pharo 12-15 (docs/CairoPlugin.md, the
   layout table): AthensCairoCanvas 10 (paint surface shape fontRenderer
   paintMode paintCache handle pathTransform paintTransform currentClipRect),
   AthensCairoSurface 7 (currentCanvas handle context builder id
   ftFontRenderer session), AthensCairoPathBuilder 4 (context absolute
   endPoint lastControlPoint), AthensCairoPaintMode 1 (canvas),
   AthensCairoStrokePaint 7 (fillPaint width miterLimit capStyle joinStyle
   dashLenghts dashOffset), CairoFreetypeFontRenderer 7 (font utfConverter
   canvas advance originalFont fontExtents glyphExtents), and the
   ExternalObjects of 1 slot (handle) or 3 (CairoScaledFont: handle face
   freeTypeFont) */
extern sqInt fakeAthensCairoCanvas, fakeAthensCairoSurface, fakeAthensCairoPathBuilder;
extern sqInt fakeAthensCairoPaintMode, fakeAthensCairoStrokePaint, fakeCairoFreetypeFontRenderer;
extern sqInt fakeAthensCairoMatrix, fakeAthensCairoPath, fakeCairoScaledFont;
extern sqInt fakeCairoTextExtents, fakeCairoFontExtents, fakeAthensCairoPatternPaint;
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
/* A Float: a SmallFloat64 when in its range, else a BoxedFloat64 */
sqInt fake_float(double value);

/* A new class of the given Spur instance specification and named slots */
sqInt fake_class(const char *name, int instSpec, int fixedFields);
/* A new instance of aClass with size indexable units, zero-filled (nil in
   the slots); outside a primitive: not counted */
sqInt fake_new(sqInt aClass, size_t size);
/* Instances holding the given units */
sqInt fake_bytes(sqInt aClass, const void *bytes, size_t size);
sqInt fake_words(sqInt aClass, const uint32_t *words, size_t size);
/* A ByteString of the C string s */
sqInt fake_string(const char *s);
/* An Array (or an instance of another class of format 1, 2 or 4) of the
   oops: for format 1, the named slots */
sqInt fake_array(sqInt aClass, const sqInt *oops, size_t size);
/* A BoxedFloat64 */
sqInt fake_boxed_float(double value);
/* An ExternalAddress holding pointer */
sqInt fake_external_address(void *pointer);
/* A Point x @ y, a Color of rgb and alpha (SmallIntegers), a Rectangle */
sqInt fake_point(sqInt x, sqInt y);
sqInt fake_color(sqInt rgb, sqInt alpha);
sqInt fake_rectangle(sqInt origin, sqInt corner);
/* A CompiledMethod of size bytes (firstIndexableField answers NULL) */
sqInt fake_compiled_method(size_t size);

/* Forwarders: an object whose class is nil (Spur's class index 8) and that
   forwards to target (fake_forwarder: to nil) */
sqInt fake_forwarder(void);
sqInt fake_forwarder_to(sqInt target);
int fake_is_forwarder(sqInt oop);
/* The object a chain of forwarders ends at (oop itself when not one) */
sqInt fake_follow(sqInt oop);

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
/* The pointer an ExternalAddress holds */
void *fake_address_value(sqInt oop);

/* The allocator: allowed allocations (-1: no limit) before
   instantiateClassindexableSize fails with failCode */
void fake_set_budget(long allowed, sqInt failCode);

typedef struct {
	sqInt code;		/* primFailCode after the call: 0 or a PrimErr code */
	int returned;		/* methodReturnValue (or methodReturnReceiver) was called */
	sqInt value;		/* its argument (the receiver for methodReturnReceiver) */
	int allocations;	/* calls of instantiateClassindexableSize */
	int allocated;		/* of which succeeded */
	int stores;		/* calls of storePointerofObjectwithValue */
	int foreignStores;	/* of which into an object the call did not make */
	int protocolErrors;	/* see above; fake_last_error says the first */
	/* fake_call_retry: */
	int retried;		/* the first call failed, forwarders were followed, called again */
	sqInt firstCode;	/* the failure code of the first call (0 when it did not fail) */
} FakeAnswer;

/* Runs primitive with receiver and nargs arguments (the first argument
   first) and answers what it did */
FakeAnswer fake_call(sqInt (*primitive)(void), sqInt receiver, int nargs, const sqInt *args);
/* The same, then, when it failed and depth >= 0, as the VM: follows the
   forwarders of the receiver and arguments (and, with depth > 0, of their
   slots to that depth, in place), and calls again when one was found */
FakeAnswer fake_call_retry(sqInt (*primitive)(void), int depth, sqInt receiver, int nargs, const sqInt *args);
const char *fake_last_error(void);

/* GC roots (addGCRoot / removeGCRoot): how many are registered, and the
   value of the i-th */
int fake_gc_root_count(void);
sqInt fake_gc_root_value(int i);

/* The module loader of ioLoadModuleOfLength / ioLoadSymbolOfLengthFromModule
   (natively; no-ops under Emscripten) */
void fake_loader_reset(void);			/* defaults, counters zeroed, nothing hidden */
void fake_loader_fail_modules(int fail);	/* ioLoadModuleOfLength answers NULL */
void fake_loader_hide(const char *name);	/* lookups of name answer NULL (up to 16 names) */
void fake_loader_override(const char *name, void *address);	/* lookups of name answer address (one) */
int fake_loader_module_calls(void);
int fake_loader_symbol_calls(void);
int fake_loader_null_module_calls(void);	/* lookups with a NULL module (a protocol error too) */
const char *fake_loader_last_module(void);	/* the last name ioLoadModuleOfLength got */

/* The plugin's primitives and depth variables, as the VM finds them: in a
   builtin plugin (SQUEAK_BUILTIN_PLUGIN) the rows of CairoPlugin_exports;
   otherwise dlsym(RTLD_DEFAULT) (link the test with -rdynamic).  NULL / -1
   when absent. */
sqInt (*fake_primitive(const char *name))(void);
int fake_accessor_depth(const char *name);

/* tests/primitives.txt: one row per primitive */
typedef struct {
	char name[64];
	int depth;
	char group[16];
	char file[64];
	int nargs;
} FakePrimitiveRow;
/* Reads up to max rows of path; answers how many, or -1 when it cannot */
int fake_read_primitives(const char *path, FakePrimitiveRow *rows, int max);

#endif /* CP_FAKEVM_H */
