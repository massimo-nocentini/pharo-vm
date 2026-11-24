/* SimdCSVPlugin.c -- the VM side of the SimdCSVPlugin: the module functions
 * and the four primitives of the package SimdCSV (github.com/
 * massimo-nocentini/simdcsv.st), which call the engine
 * (src/engine/scEngine.cpp, with the kernels of simdcsv a9494a8) through
 * scEngine.h.  This is the only file of the plugin that includes VM headers,
 * and it includes nothing of simdcsv.  docs/SimdCSV.md, section "Primitive
 * contract", is the contract.
 *
 * Two steps: primitiveIndex writes the positions of the separators into a
 * word array of the caller, 32 bits a field; primitiveRows makes the rows of
 * a range of such an index, and trusts neither the index nor the source: the
 * cursor of the engine checks every entry against the source before anything
 * is read through it.
 *
 * Each primitive checks its arguments in the contract's order:
 *   1. their count                                       PrimErrBadNumArgs
 *   2. the source: bytes (no CompiledMethod) or words    PrimErrBadArgument
 *   3. the numbers: SmallIntegers                        PrimErrBadArgument
 *   4. the dialect word                                  PrimErrBadArgument
 *   5. the ranges: of the source, of the index and of the destination (when
 *      these are what 7. asks for, else 7. fails), the field count
 *                                                        PrimErrBadIndex
 *   6. the class of wide strings                         PrimErrBadArgument
 *   7. the index, the types, the destination: their classes, then that the
 *      destination is not the source                     PrimErrBadArgument
 *      and that it is not immutable                      PrimErrNoModification
 * and fails leaving the stack alone.  Bad data (an irregular quote, invalid
 * UTF-8, a wrong index) is never a failure: it is a status of the answer.
 *
 * Allocation and GC: objects are made only by
 * instantiateClassindexableSize, which never collects nor moves (when eden is
 * full, Spur schedules a scavenge and allocates in old space, which never
 * grows in it), so every oop and the addresses of the source and of the index
 * stay valid during a call: no remappable oops, no pinning; none is kept
 * after it.  Pointers are stored only by storePointerofObjectwithValue, into
 * objects the call made and into the destination of primitiveRows, which is
 * only written.  The answer Array is made first: PrimErrNoMemory is a failure
 * then, and only then.  When a row does not fit in object memory, the failure
 * is taken back, and the answer is MORE (with the rows made before it) or
 * NO_ROOM (with the bytes that this record needs); any other failure of an
 * allocation is the primitive's.  There is no C heap.
 *
 * Natively it is the shared library libSimdCSVPlugin.so, whose module
 * functions, primitives and primitiveXAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through SimdCSVPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).  No
 * primitive reads an object inside an argument: every accessor depth is 0.
 */

#include <stdint.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "scEngine.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

_Static_assert(sizeof(sqInt) == 8, "SimdCSVPlugin: the answers hold SmallIntegers above 2^30 (docs/SimdCSV.md)");

#define PLUGIN_VERSION "SimdCSVPlugin 1.0"
#ifndef PHARO_SIMDCSV_VERSION	/* cmake/simdcsv.cmake gives it */
# define PHARO_SIMDCSV_VERSION "a9494a8"
#endif
#ifdef __EMSCRIPTEN__
# define PLATFORM "emscripten"
#else
# define PLATFORM "native"
#endif

/* The dialect word: the separator, the quote, the flags */
#define SEPARATOR_BITS 21
#define QUOTE_BITS 8
#define FLAG_SHIFT (SEPARATOR_BITS + QUOTE_BITS)

/* The type codes of a column of primitiveRows */
enum { IGNORED, STRING, INTEGER, FLOAT, FLOAT_COMMA, TYPE_CODES };

/* Spur's instance specification of 32-bit words */
#define WORDS_INST_SPEC 10

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"SimdCSVPlugin (i)";
#else
	"SimdCSVPlugin (e)";
#endif

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitiveIndex(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveMeasure(void);
EXPORT(sqInt) primitiveRows(void);
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
	/* the choice of the kernel, once per process */
	sc_init();
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

/* A source, an index or a destination of words: units of 1 byte, or of 4
 * when wide is 1 */
typedef struct {
	sqInt oop;
	int wide;
	sqInt size;		/* in units */
	void *units;
} Units;

/* Whether the stack value at offset is a word object, into *u */
static int
wordsAt(int offset, Units *u)
{
	u->oop = interpreterProxy->stackValue(offset);
	u->wide = 1;
	if (!interpreterProxy->isWords(u->oop)) {
		return 0;
	}
	u->size = interpreterProxy->byteSizeOf(u->oop) / 4;
	u->units = interpreterProxy->firstIndexableField(u->oop);
	return u->size == 0 || u->units != NULL;
}

/* Whether the stack value at offset is a byte object or a word object, into
 * *u.  isBytes is true of CompiledMethods too, whose bytecodes
 * firstIndexableField does not answer: they are refused, empty ones too. */
static int
sourceAt(int offset, Units *u)
{
	if (wordsAt(offset, u)) {
		return 1;
	}
	if (!interpreterProxy->isBytes(u->oop)) {
		return 0;
	}
	u->wide = 0;
	u->size = interpreterProxy->byteSizeOf(u->oop);
	u->units = interpreterProxy->firstIndexableField(u->oop);
	return u->units != NULL;
}

static sqInt
integerAt(int offset)
{
	return interpreterProxy->integerValueOf(interpreterProxy->stackValue(offset));
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

static int
isLineEnd(sqInt unit)
{
	return unit == '\r' || unit == '\n';
}

/* Whether word is a dialect for source, into *d */
static int
dialectOf(sqInt word, const Units *source, sc_dialect *d)
{
	sqInt separator = word & (((sqInt)1 << SEPARATOR_BITS) - 1);
	sqInt quote = (word >> SEPARATOR_BITS) & (((sqInt)1 << QUOTE_BITS) - 1);
	sqInt flags = word >> FLAG_SHIFT;	/* with every bit above, the sign too */

	if ((flags & ~(sqInt)SC_F_ALL) != 0
	 || isLineEnd(separator)
	 || isLineEnd(quote)
	 || (separator == quote && !(flags & SC_F_NO_QUOTE))) {
		return 0;
	}
	if (source->wide) {
		if (flags & SC_F_LATIN1) {
			return 0;
		}
	} else if (separator > 255 || (!(flags & SC_F_LATIN1) && (separator > 127 || quote > 127))) {
		return 0;
	}
	d->separator = (uint32_t)separator;
	d->quote = (uint8_t)quote;
	d->flags = (uint32_t)flags;
	return 1;
}

/* Whether start..stop (1-based, inclusive; empty when start = stop + 1) is a
 * range of size units */
static int
isRange(sqInt start, sqInt stop, sqInt size)
{
	return start >= 1 && stop <= size && start - 1 <= stop;
}

/* The units start..stop of source */
static sc_input
inputOf(const Units *source, sqInt start, sqInt stop)
{
	sc_input in;

	in.units = source->units;
	in.wide = source->wide;
	in.from = (uint64_t)(start - 1);
	in.to = (uint64_t)stop;
	return in;
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

/* Whether aClass can be the class of wide strings (SimdUTFPlugin.c): a class
 * (its class is a metaclass, so not an Array posing as one), of 32-bit words
 * without named slots, and not BoxedFloat64, whose instances
 * instantiateClassindexableSize makes of 2 words only. */
static int
isWideStringClass(sqInt aClass)
{
	sqInt format = classFormat(aClass);

	return format >= 0
		&& ((format >> 16) & 0x1F) == WORDS_INST_SPEC
		&& (format & 0xFFFF) == 0
		&& interpreterProxy->fetchClassOf(interpreterProxy->fetchClassOf(aClass))
			== interpreterProxy->fetchClassOf(interpreterProxy->fetchClassOf(interpreterProxy->classString()))
		&& aClass != interpreterProxy->classFloat();
}

static sqInt
fail(sqInt code)
{
	return interpreterProxy->primitiveFailFor(code);
}


/*** Objects ***/

/* The bytes of a Spur object of n slots of 64 bits, header included
 * (SimdUTFPlugin.c): an overflow header from 255 slots, and a slot at least */
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

/* Whether d is an immediate SmallFloat64: -0.0, 0.0, or an exponent in the
 * middle range (SimdJSONPlugin's sj_is_small_float) */
static int
isSmallFloat(double d)
{
	uint64_t raw, exponent;

	memcpy(&raw, &d, sizeof raw);
	exponent = (raw >> 52) & 0x7FF;
	return exponent > 896
		? exponent <= 0xFF + 896
		: (raw & ((1ULL << 52) - 1)) == 0 ? exponent == 0 : exponent == 896;
}

/* A new instance of cls with size units, or 0 with the failure code set by
 * the VM */
static sqInt
instantiate(sqInt cls, uint64_t size)
{
	return interpreterProxy->instantiateClassindexableSize(cls, (sqInt)size);
}

/* The primitive fails after an instantiate that answered 0: with the VM's
 * code */
static sqInt
failNoObject(void)
{
	sqInt code = interpreterProxy->primitiveFailureCode();

	return fail(code ? code : PrimErrNoMemory);
}

static sqInt
stringObject(const char *s)
{
	size_t size = strlen(s);
	sqInt oop = instantiate(interpreterProxy->classString(), size);

	if (oop && size) {
		memcpy(interpreterProxy->firstIndexableField(oop), s, size);
	}
	return oop;
}

static void
put(sqInt array, sqInt index0, sqInt value)
{
	interpreterProxy->storePointerofObjectwithValue(index0, array, value);
}

static void
putInteger(sqInt array, sqInt index0, uint64_t value)
{
	put(array, index0, interpreterProxy->integerObjectOf((sqInt)value));
}

/* Stores a new ByteString of s at slot index0 of array, and answers whether
 * it could be made */
static int
putString(sqInt array, sqInt index0, const char *s)
{
	sqInt oop = stringObject(s);

	if (!oop) {
		return 0;
	}
	put(array, index0, oop);
	return 1;
}


/*** The primitives ***/

/*	SimdCSVPrimitives class>>primInfo
	<primitive: 'primitiveInfo' module: 'SimdCSVPlugin' error: ec>
	Answer { protocol. 'SimdCSVPlugin 1.0'. simdcsv's version. the kernel.
	the platform. the Array of the kernels of this machine, best first. nil,
	or what came of SIMDCSV_FORCE_IMPLEMENTATION: the kernel forced, or
	'refused <value>'. the widest range of an index }. */
EXPORT(sqInt)
primitiveInfo(void)
{
	sqInt answer, kernels;
	const char *note;
	int i, n;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return fail(PrimErrBadNumArgs);
	}
	n = sc_kernel_count();
	note = sc_forced_note();
	if (!(answer = instantiate(interpreterProxy->classArray(), 8))
	 || !putString(answer, 1, PLUGIN_VERSION)
	 || !putString(answer, 2, PHARO_SIMDCSV_VERSION)
	 || !putString(answer, 3, sc_kernel())
	 || !putString(answer, 4, PLATFORM)
	 || !(kernels = instantiate(interpreterProxy->classArray(), (uint64_t)n))) {
		return failNoObject();
	}
	put(answer, 5, kernels);
	for (i = 0; i < n; i++) {
		if (!putString(kernels, i, sc_kernel_name(i))) {
			return failNoObject();
		}
	}
	if (note[0] && !putString(answer, 6, note)) {
		return failNoObject();
	}
	putInteger(answer, 0, SC_PROTOCOL);
	putInteger(answer, 7, SC_MAX_SPAN);
	return interpreterProxy->methodReturnValue(answer);
}

/* The position of an answer: 1-based, 0 without an error */
static uint64_t
positionOf(const sc_scan *scan)
{
	return scan->code == SC_E_NONE ? 0 : scan->position + 1;
}

/*	SimdCSVPrimitives class>>measure: source from: start to: stop dialect: word
	<primitive: 'primitiveMeasure' module: 'SimdCSVPlugin' error: ec>
	Answer { status. records. fields. next. lines. code. position } of the
	complete records of source[start..stop], which starts at a record. */
EXPORT(sqInt)
primitiveMeasure(void)
{
	static const int numbers[] = { 2, 1, 0 };
	Units source;
	sqInt start, stop, answer;
	sc_dialect dialect;
	sc_input in;
	sc_scan scan;

	if (interpreterProxy->methodArgumentCount() != 4) {
		return fail(PrimErrBadNumArgs);
	}
	if (!sourceAt(3, &source) || !areSmallIntegers(numbers, 3) || !dialectOf(integerAt(0), &source, &dialect)) {
		return fail(PrimErrBadArgument);
	}
	start = integerAt(2);
	stop = integerAt(1);
	if (!isRange(start, stop, source.size)) {
		return fail(PrimErrBadIndex);
	}
	if (!(answer = instantiate(interpreterProxy->classArray(), 7))) {
		return failNoObject();
	}
	in = inputOf(&source, start, stop);
	memset(&scan, 0, sizeof scan);
	sc_measure(&in, &dialect, &scan);
	putInteger(answer, 0, (uint64_t)scan.status);
	putInteger(answer, 1, scan.records);
	putInteger(answer, 2, scan.stored);
	putInteger(answer, 3, scan.next + 1);
	putInteger(answer, 4, scan.lines);
	putInteger(answer, 5, (uint64_t)scan.code);
	putInteger(answer, 6, positionOf(&scan));
	return interpreterProxy->methodReturnValue(answer);
}

/*	SimdCSVPrimitives class>>index: source base: base from: start to: stop
		dialect: word into: dest from: destStart
	<primitive: 'primitiveIndex' module: 'SimdCSVPlugin' error: ec>
	Write the ends of the fields of the complete records of
	source[start..stop], which starts at a record, as 32-bit offsets from
	base, into the word object dest from its slot destStart on.  Answer
	{ status. stored. records. next. lines. minFields. maxFields. code.
	position }. */
EXPORT(sqInt)
primitiveIndex(void)
{
	static const int numbers[] = { 5, 4, 3, 2, 0 };
	Units source, dest;
	sqInt base, start, stop, destStart, answer;
	int destIsWords;
	sc_dialect dialect;
	sc_input in;
	sc_scan scan;

	if (interpreterProxy->methodArgumentCount() != 7) {
		return fail(PrimErrBadNumArgs);
	}
	if (!sourceAt(6, &source) || !areSmallIntegers(numbers, 5) || !dialectOf(integerAt(2), &source, &dialect)) {
		return fail(PrimErrBadArgument);
	}
	base = integerAt(5);
	start = integerAt(4);
	stop = integerAt(3);
	destStart = integerAt(0);
	destIsWords = wordsAt(1, &dest);
	if (base < 1 || base > start || !isRange(start, stop, source.size)
	 || (destIsWords && (destStart < 1 || destStart > dest.size + 1))) {
		return fail(PrimErrBadIndex);
	}
	if (!destIsWords || dest.oop == source.oop) {
		return fail(PrimErrBadArgument);
	}
	if (interpreterProxy->isOopImmutable(dest.oop)) {
		return fail(PrimErrNoModification);
	}
	if (!(answer = instantiate(interpreterProxy->classArray(), 9))) {
		return failNoObject();
	}
	in = inputOf(&source, start, stop);
	memset(&scan, 0, sizeof scan);
	sc_index(&in, &dialect, (uint64_t)(base - 1), (uint32_t *)dest.units + (destStart - 1),
		(uint64_t)(dest.size - destStart + 1), &scan);
	putInteger(answer, 0, (uint64_t)scan.status);
	putInteger(answer, 1, scan.stored);
	putInteger(answer, 2, scan.records);
	putInteger(answer, 3, scan.next + 1);
	putInteger(answer, 4, scan.lines);
	putInteger(answer, 5, scan.records ? scan.min_fields : 0);
	putInteger(answer, 6, scan.records ? scan.max_fields : 0);
	putInteger(answer, 7, (uint64_t)scan.code);
	putInteger(answer, 8, positionOf(&scan));
	return interpreterProxy->methodReturnValue(answer);
}


/*** The rows ***/

/* A call of primitiveRows: its arguments, checked, and what it made */
typedef struct {
	sc_input in;
	sc_dialect dialect;
	const unsigned char *types;	/* NULL: every column is a String */
	uint64_t typeCount;
	uint64_t fieldCount;		/* 0: rows as wide as their record */
	uint64_t width;			/* the slots of a row, when fieldCount is not 0 */
	sqInt wideClass;
	sqInt emptyValue;
	int emptyIsNil;
	uint64_t made;			/* bytes of the objects of the rows */
	uint64_t odd;
	/* of the record at hand */
	uint64_t rowOdd;
	uint64_t position;		/* of invalid UTF-8, 0-based */
} Rows;

/* What became of a cell or of a row */
enum { MADE, NO_OBJECT, INVALID };

/* The type code of the column, 0-based */
static int
typeOf(const Rows *rows, uint64_t column)
{
	return column < rows->typeCount ? rows->types[column] : STRING;
}

/* The columns among the first count ones that are not ignored */
static uint64_t
columnsKept(const Rows *rows, uint64_t count)
{
	uint64_t i, kept = count;

	for (i = 0; i < count && i < rows->typeCount; i++) {	/* (typeCount is 0 without types) */
		if (rows->types[i] == IGNORED) {
			kept--;
		}
	}
	return kept;
}

/* Whether the field is a number of the type given, into *integer or *number */
static int
isNumber(const Rows *rows, const sc_field *field, int type, int64_t *integer, double *number)
{
	if (type == INTEGER) {
		return sc_to_integer(&rows->in, &rows->dialect, field, integer);
	}
	return sc_to_float(&rows->in, &rows->dialect, field, type == FLOAT_COMMA, number);
}

/* The oop of the String text, or 0 */
static sqInt
textObject(Rows *rows, const sc_text *text)
{
	int wide = text->kind == SC_WIDE;
	sqInt oop = instantiate(wide ? rows->wideClass : interpreterProxy->classString(), text->units);

	if (!oop) {
		return 0;
	}
	rows->made += wide ? spurWords32(text->units) : spurBytes(text->units);
	if (text->units) {
		sc_text_copy(&rows->in, &rows->dialect, text, interpreterProxy->firstIndexableField(oop));
	}
	return oop;
}

/* Makes the cell of a field of the type given (not IGNORED) into *cell.
 * INVALID: rows->position is where its UTF-8 is invalid. */
static int
makeCell(Rows *rows, const sc_field *field, int type, sqInt *cell)
{
	sc_text text;
	int64_t integer;
	double number;

	if (field->start == field->end) {
		*cell = rows->emptyValue;
		return MADE;
	}
	if (type != STRING && isNumber(rows, field, type, &integer, &number)) {
		if (type == INTEGER) {
			*cell = interpreterProxy->integerObjectOf((sqInt)integer);
		} else if (isSmallFloat(number)) {
			*cell = interpreterProxy->floatObjectOf(number);	/* an immediate: no allocation */
		} else {
			/* classFloat() is BoxedFloat64, two 32-bit slots */
			if (!(*cell = instantiate(interpreterProxy->classFloat(), 2))) {
				return NO_OBJECT;
			}
			memcpy(interpreterProxy->firstIndexableField(*cell), &number, sizeof number);
			rows->made += spurWords32(2);
		}
		return MADE;
	}
	sc_text_measure(&rows->in, &rows->dialect, field, &text);
	if (text.code != SC_E_NONE) {
		rows->position = text.position;
		return INVALID;
	}
	if (text.kind == SC_EMPTY
	 || (text.kind == SC_EMPTY_QUOTED
	     && !(type == STRING && (rows->dialect.flags & SC_F_KEEP_QUOTED_EMPTY)))) {
		*cell = rows->emptyValue;
		return MADE;
	}
	if (!(*cell = textObject(rows, &text))) {
		return NO_OBJECT;
	}
	if (type != STRING) {
		rows->rowOdd++;
	}
	return MADE;
}

/* The bytes of the object that makeCell makes of the field, added to *need */
static int
measureCell(Rows *rows, const sc_field *field, int type, uint64_t *need)
{
	sc_text text;
	int64_t integer;
	double number;

	if (field->start == field->end) {
		return MADE;
	}
	if (type != STRING && isNumber(rows, field, type, &integer, &number)) {
		if (type != INTEGER && !isSmallFloat(number)) {
			*need += spurWords32(2);
		}
		return MADE;
	}
	sc_text_measure(&rows->in, &rows->dialect, field, &text);
	if (text.code != SC_E_NONE) {
		rows->position = text.position;
		return INVALID;
	}
	if (text.kind == SC_WIDE) {
		*need += spurWords32(text.units);
	} else if (text.kind == SC_BYTES) {
		*need += spurBytes(text.units);
	} else if (text.kind == SC_EMPTY_QUOTED && type == STRING
		&& (rows->dialect.flags & SC_F_KEEP_QUOTED_EMPTY)) {
		*need += spurBytes(0);
	}
	return MADE;
}

/* Makes the row of the record of count fields at the cursor, whose fields
 * the caller walked already, into *row; or, when row is NULL, adds the bytes
 * of its objects to *need.  *column: the column, 0-based, of an INVALID
 * cell. */
static int
makeRow(Rows *rows, sc_cursor cursor, uint64_t count, sqInt *row, uint64_t *need, uint64_t *column)
{
	uint64_t width = rows->fieldCount ? rows->width : columnsKept(rows, count);
	uint64_t columns = rows->fieldCount && rows->fieldCount < count ? rows->fieldCount : count;
	uint64_t i, slot = 0;
	sc_field field;

	if (row) {
		if (!(*row = instantiate(interpreterProxy->classArray(), width))) {
			return NO_OBJECT;
		}
		rows->made += spurPointers(width);
	} else {
		*need += spurPointers(width);
	}
	rows->rowOdd = 0;
	for (i = 0; i < columns; i++) {
		int type = typeOf(rows, i), outcome;
		sqInt cell = 0;

		if (sc_cursor_next(&cursor, &field)) {
			break;	/* (never: the caller walked the record) */
		}
		if (type == IGNORED) {
			continue;
		}
		*column = i;
		outcome = row ? makeCell(rows, &field, type, &cell) : measureCell(rows, &field, type, need);
		if (outcome != MADE) {
			return outcome;
		}
		if (row) {
			put(*row, (sqInt)slot, cell);
		}
		slot++;
	}
	if (row && !rows->emptyIsNil) {	/* (a new Array holds nils) */
		for (; slot < width; slot++) {
			put(*row, (sqInt)slot, rows->emptyValue);
		}
	}
	return MADE;
}

/*	SimdCSVPrimitives class>>rows: source base: base to: stop dialect: word
		index: index from: indexStart to: indexStop types: types
		fieldCount: n into: dest from: destStart wideStringClass: wideClass
		empty: emptyValue budget: budget
	<primitive: 'primitiveRows' module: 'SimdCSVPlugin' error: ec>
	Make the rows of the records whose fields are the entries
	indexStart..indexStop of the index, and store them in the Array dest from
	its slot destStart on.  Answer { status. rows. nextIndex. odd. code.
	position. row. field. need }. */
EXPORT(sqInt)
primitiveRows(void)
{
	static const int numbers[] = { 12, 11, 10, 8, 7, 5, 3, 0 };
	Units source, index;
	Rows rows;
	sqInt base, stop, indexStart, indexStop, fieldCount, destStart, budget;
	sqInt types, dest, answer, destSize = 0;
	int indexIsWords, destIsArray, i, status = SC_OK, code = SC_E_NONE;
	uint64_t capacity, stored = 0, next, position = 0, badRow = 0, badField = 0, need = 0;
	sc_cursor cursor;

	if (interpreterProxy->methodArgumentCount() != 14) {
		return fail(PrimErrBadNumArgs);
	}
	memset(&rows, 0, sizeof rows);
	if (!sourceAt(13, &source) || !areSmallIntegers(numbers, 8) || !dialectOf(integerAt(10), &source, &rows.dialect)) {
		return fail(PrimErrBadArgument);
	}
	base = integerAt(12);
	stop = integerAt(11);
	indexStart = integerAt(8);
	indexStop = integerAt(7);
	fieldCount = integerAt(5);
	destStart = integerAt(3);
	budget = integerAt(0);
	indexIsWords = wordsAt(9, &index);
	types = interpreterProxy->stackValue(6);
	dest = interpreterProxy->stackValue(4);
	destIsArray = (int)interpreterProxy->isArray(dest);
	if (destIsArray) {
		destSize = interpreterProxy->slotSizeOf(dest);
	}
	if (!isRange(base, stop, source.size)
	 || (indexIsWords && !isRange(indexStart, indexStop, index.size))
	 || fieldCount < 0 || fieldCount > SC_MAX_FIELD_COUNT
	 || (destIsArray && (destStart < 1 || destStart > destSize + 1))) {
		return fail(PrimErrBadIndex);
	}
	rows.wideClass = interpreterProxy->stackValue(2);
	if (!isWideStringClass(rows.wideClass) || !indexIsWords) {
		return fail(PrimErrBadArgument);
	}
	if (types != interpreterProxy->nilObject()) {
		uint64_t k;

		if (interpreterProxy->fetchClassOf(types) != interpreterProxy->classByteArray()) {
			return fail(PrimErrBadArgument);
		}
		rows.types = interpreterProxy->firstIndexableField(types);
		rows.typeCount = (uint64_t)interpreterProxy->byteSizeOf(types);
		if (fieldCount && rows.typeCount != (uint64_t)fieldCount) {
			return fail(PrimErrBadArgument);
		}
		for (k = 0; k < rows.typeCount; k++) {
			if (rows.types[k] >= TYPE_CODES) {
				return fail(PrimErrBadArgument);
			}
		}
	}
	if (!destIsArray) {	/* (an Array is not the source, bytes or words) */
		return fail(PrimErrBadArgument);
	}
	if (interpreterProxy->isOopImmutable(dest)) {
		return fail(PrimErrNoModification);
	}
	if (!(answer = instantiate(interpreterProxy->classArray(), 9))) {
		return failNoObject();
	}
	rows.in = inputOf(&source, base, stop);
	rows.fieldCount = (uint64_t)fieldCount;
	rows.width = columnsKept(&rows, rows.fieldCount);
	rows.emptyValue = interpreterProxy->stackValue(1);
	rows.emptyIsNil = rows.emptyValue == interpreterProxy->nilObject();
	capacity = (uint64_t)(destSize - destStart + 1);
	next = (uint64_t)(indexStart - 1);	/* 0-based: the first entry of the first record not stored */
	memset(&cursor, 0, sizeof cursor);

	if (rows.in.to - rows.in.from > SC_MAX_SPAN) {
		status = SC_TOO_LARGE;
	} else if (indexStart <= indexStop
		&& sc_cursor_open(&cursor, &rows.in, &rows.dialect, index.units, next, (uint64_t)indexStop)) {
		status = SC_ERROR;
		code = SC_E_BAD_INDEX;
		position = rows.in.from + 1;
		badRow = 1;
	}
	while (status == SC_OK && next < (uint64_t)indexStop) {
		sc_cursor record = cursor;
		sc_field field;
		uint64_t count = 0, start = 0, column = 0;
		sqInt row;
		int outcome;

		if (stored == capacity || (stored && budget >= 0 && rows.made >= (uint64_t)budget)) {
			status = SC_MORE;
			break;
		}
		/* pass 1: the fields of the record, each entry checked */
		do {
			if (sc_cursor_next(&cursor, &field)) {
				code = SC_E_BAD_INDEX;
				/* (before its first field: past the blank lines that the cursor skipped) */
				start = count ? start : cursor.pos;
				break;
			}
			if (count++ == 0) {
				start = field.start;
			}
		} while (!field.last);
		if (code == SC_E_NONE && fieldCount && count != (uint64_t)fieldCount
		 && (rows.dialect.flags & SC_F_EXACT_COUNT)) {
			code = SC_E_FIELD_COUNT;
			badField = count;
		}
		if (code != SC_E_NONE) {
			status = SC_ERROR;
			position = start + 1;
			badRow = stored + 1;
			break;
		}
		/* pass 2: the row and its cells */
		outcome = makeRow(&rows, record, count, &row, NULL, &column);
		if (outcome == NO_OBJECT) {
			sqInt failure = interpreterProxy->primitiveFailureCode();

			if (failure && failure != PrimErrNoMemory) {
				return fail(failure);
			}
			interpreterProxy->primitiveFailFor(0);
			if (stored) {
				status = SC_MORE;
				break;
			}
			status = SC_NO_ROOM;
			outcome = makeRow(&rows, record, count, NULL, &need, &column);
		}
		if (outcome == INVALID) {
			status = SC_ERROR;
			code = SC_E_INVALID_UTF8;
			position = rows.position + 1;
			badRow = stored + 1;
			badField = column + 1;
			need = 0;
		}
		if (status != SC_OK) {
			break;
		}
		put(dest, destStart - 1 + (sqInt)stored, row);
		stored++;
		rows.odd += rows.rowOdd;
		next = cursor.slot;
	}

	i = 0;
	putInteger(answer, i++, (uint64_t)status);
	putInteger(answer, i++, stored);
	putInteger(answer, i++, next + 1);
	putInteger(answer, i++, rows.odd);
	putInteger(answer, i++, (uint64_t)code);
	putInteger(answer, i++, position);
	putInteger(answer, i++, badRow);
	putInteger(answer, i++, badField);
	putInteger(answer, i++, need);
	return interpreterProxy->methodReturnValue(answer);
}

#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "SimdCSVPlugin";
void* SimdCSVPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveIndex\000\000", (void*)primitiveIndex},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveMeasure\000\000", (void*)primitiveMeasure},
	{(void*)_m, "primitiveRows\000\000", (void*)primitiveRows},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveIndexAccessorDepth = 0;
EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveMeasureAccessorDepth = 0;
EXPORT(signed char) primitiveRowsAccessorDepth = 0;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
