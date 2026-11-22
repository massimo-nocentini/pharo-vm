/* CMarkPlugin.c -- the VM side of the CMarkPlugin: the module functions and
 * the four primitives of the package CMark (the submodule
 * plugins/CMarkPlugin/cmark-gfm.st), which call the engine
 * (src/engine/cmEngine.c, cmText.c, cmTrack.c: cmark-gfm 0.29.0.gfm.14)
 * through cmEngine.h.  This is the only file of the plugin that includes VM
 * headers, and it never includes a cmark header.  docs/CMark.md, section
 * "Primitive contract", is the contract; section "Tree format" describes the
 * tape that primitiveParse answers.
 *
 * Each primitive checks its arguments in the contract's order (the argument
 * count; the source's kind; that every number is a SmallInteger; the option
 * bits; the extension bits; maxDepth; width; budget; cBudget; format), then
 * that every extension of the mask was found (PrimErrNotFound), and fails
 * leaving the stack alone.  Markdown itself is never an error: any bytes
 * parse, and the limits (the budget of object memory, the input's size, the
 * depth, the C limits) are statuses of the answer, so that they never start
 * the VM's retries.
 *
 * Allocation and GC: the engine works on C memory only; this file allocates
 * the answer only through instantiateClassindexableSize, which never runs a
 * GC (when eden is full, Spur schedules a scavenge and allocates in old
 * space), so the source's bytes and every oop stay valid during the call:
 * no remappable oops, no pinning.  A parse answers at most four objects (the
 * Array of 9, a Bitmap, a ByteArray, the sanitised ByteArray), whose exact
 * size in object memory is known before the first allocation: with a budget
 * (>= 0) under that need, the answer is OVER_BUDGET with the need, and the
 * engine keeps its result for the next call.  When an allocation answers 0,
 * the primitive fails with PrimErrNoMemory and keeps the result too: the VM
 * collects garbage and calls again, and the engine answers from the kept
 * result without parsing again.  Every pointer store goes through
 * storePointerofObjectwithValue, into the answer Arrays only.
 *
 * Natively it is the shared library libCMarkPlugin.so, whose module
 * functions, primitives and primitiveXAccessorDepth variables the VM finds
 * with dlsym.  In the Emscripten VM it is a builtin plugin
 * (SQUEAK_BUILTIN_PLUGIN): every function is static and the VM finds them
 * through CMarkPlugin_exports, whose rows
 * cmake/emscripten/genPrimitiveTable.cmake parses line by line (keep the
 * layout: a tab, then {(void*)_m, "name\000\ddd", (void*)fn},).  No
 * primitive reads objects inside its arguments: every accessor depth is 0.
 */

#include <stdint.h>
#include <string.h>

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/* The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions (EXPORT) */

#include "cmEngine.h"

#ifdef SQUEAK_BUILTIN_PLUGIN
# undef EXPORT
# define EXPORT(returnType) static returnType
#endif

_Static_assert(sizeof(sqInt) == 8, "CMarkPlugin: the answers hold SmallIntegers above 2^30 (docs/CMark.md)");

#define PLUGIN_VERSION "CMarkPlugin 1.0"
#ifdef __EMSCRIPTEN__
# define PLATFORM "emscripten"
#else
# define PLATFORM "native"
#endif

/* The widest width of the text formats */
#define MAX_WIDTH 10000

/* static in both modes: the VM itself defines a global interpreterProxy */
static struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"CMarkPlugin (i)";
#else
	"CMarkPlugin (e)";
#endif

/* The OVER_BUDGET answers (the engine never sees the budget) */
static uint64_t overBudget;

EXPORT(const char*) getModuleName(void);
EXPORT(sqInt) initialiseModule(void);
EXPORT(sqInt) primitiveInfo(void);
EXPORT(sqInt) primitiveParse(void);
EXPORT(sqInt) primitiveRelease(void);
EXPORT(sqInt) primitiveRender(void);
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
	/* registration of the extensions, once per process (a missing one only
	   makes its bit fail with PrimErrNotFound) */
	cm_init();
	return 1;
}

EXPORT(sqInt)
shutdownModule(void)
{
	cm_release_kept();
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

static int
isIntegerAt(int offset)
{
	return interpreterProxy->isIntegerObject(interpreterProxy->stackValue(offset));
}

static sqInt
integerAt(int offset)
{
	return interpreterProxy->integerValueOf(interpreterProxy->stackValue(offset));
}

/* The source at offset, by exact class or format: a ByteArray (UTF-8), a
 * ByteString (Latin-1), any words object (UTF-32: a WideString, or a
 * WideSymbol, a WordArray, a Bitmap...); 0 for anything else */
static int
sourceAt(int offset, cm_source *source)
{
	sqInt oop = interpreterProxy->stackValue(offset);
	sqInt cls = interpreterProxy->fetchClassOf(oop);

	if (cls == interpreterProxy->classByteArray()) {
		source->kind = CM_SRC_UTF8;
		source->count = (size_t)interpreterProxy->byteSizeOf(oop);
	} else if (cls == interpreterProxy->classString()) {
		source->kind = CM_SRC_LATIN1;
		source->count = (size_t)interpreterProxy->byteSizeOf(oop);
	} else if (interpreterProxy->isWords(oop)) {
		source->kind = CM_SRC_UTF32;
		source->count = (size_t)interpreterProxy->byteSizeOf(oop) / 4;
	} else {
		return 0;
	}
	source->units = interpreterProxy->firstIndexableField(oop);
	return source->count == 0 || source->units != NULL;
}

/* The arguments of primitiveParse and primitiveRender */
typedef struct {
	cm_source source;
	sqInt options, extensions, maxDepth, width, budget, cBudget, format;
} Arguments;

/* Reads and checks the arguments of a primitive of nargs arguments: the
 * source at the bottom, then format (render only), options, extensions,
 * maxDepth (parse) or width (render), budget and cBudget; answers 0, or the
 * failure code */
static sqInt
argumentsOf(int render, Arguments *a)
{
	int nargs = render ? 7 : 6, i;

	if (interpreterProxy->methodArgumentCount() != nargs) {
		return PrimErrBadNumArgs;
	}
	if (!sourceAt(nargs - 1, &a->source)) {
		return PrimErrBadArgument;
	}
	for (i = 0; i < nargs - 1; i++) {
		if (!isIntegerAt(i)) {
			return PrimErrBadArgument;
		}
	}
	a->cBudget = integerAt(0);
	a->budget = integerAt(1);
	if (render) {
		a->width = integerAt(2);
		a->extensions = integerAt(3);
		a->options = integerAt(4);
		a->format = integerAt(5);
		a->maxDepth = 0;
	} else {
		a->maxDepth = integerAt(2);
		a->extensions = integerAt(3);
		a->options = integerAt(4);
		a->width = 0;
		a->format = CM_FMT_HTML;
	}
	if (a->options < 0 || (a->options & ~(sqInt)CM_OPT_ALLOWED) != 0
	 || a->extensions < 0 || a->extensions > CM_EXT_ALL
	 || a->maxDepth < 0
	 || a->width < 0 || a->width > MAX_WIDTH
	 || a->budget < -1
	 || a->cBudget < -1
	 || a->format < CM_FMT_HTML || a->format > CM_FMT_MAN) {
		return PrimErrBadArgument;
	}
	cm_init();
	if (!cm_extensions_present((int)a->extensions)) {
		return PrimErrNotFound;
	}
	return 0;
}

static uint32_t
maxDepthOf(const Arguments *a)
{
	return a->maxDepth > (sqInt)UINT32_MAX ? UINT32_MAX : (uint32_t)a->maxDepth;
}


/*** Answers ***/

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

/* A new instance of cls with size units, or 0 (then the primitive fails
 * with PrimErrNoMemory) */
static sqInt
instantiate(sqInt cls, size_t size)
{
	return interpreterProxy->instantiateClassindexableSize(cls, (sqInt)size);
}

/* A new non-pointer object of cls of units elements of unitSize bytes,
 * holding the units at bytes (no copy when there is none: memcpy's
 * arguments must not be NULL) */
static sqInt
unitsObject(sqInt cls, const void *bytes, size_t units, size_t unitSize)
{
	sqInt oop = instantiate(cls, units);

	if (oop && units) {
		memcpy(interpreterProxy->firstIndexableField(oop), bytes, units * unitSize);
	}
	return oop;
}

static sqInt
bytesObject(sqInt cls, const void *bytes, size_t size)
{
	return unitsObject(cls, bytes, size, 1);
}

static sqInt
stringObject(const char *s)
{
	return bytesObject(interpreterProxy->classString(), s, s ? strlen(s) : 0);
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

static sqInt
failNoMemory(void)
{
	return interpreterProxy->primitiveFailFor(PrimErrNoMemory);
}

/* The 9 slots of a parse answer without its objects */
static sqInt
parseAnswer(int status, uint64_t code, const cm_tree *tree, size_t nodes)
{
	sqInt answer = instantiate(interpreterProxy->classArray(), 9);

	if (!answer) {
		return 0;
	}
	putInteger(answer, 0, (uint64_t)status);
	putInteger(answer, 1, code);
	putInteger(answer, 4, nodes);
	putInteger(answer, 5, tree->depth);
	putInteger(answer, 6, CM_REC_WORDS);
	putInteger(answer, 7, tree->sourceChars);
	return answer;
}


/*** The primitives ***/

/*	CMarkPrimitives class>>primInfo
	<primitive: 'primitiveInfo' module: 'CMarkPlugin' error: ec>
	Answer { protocol. 'CMarkPlugin 1.0'. cmark-gfm's version. platform.
	extensions. limits. stats }: the names of the extensions found, in bit
	order; { maxInputBytes. maxBlockBytes. defaultCBudget. recordWords.
	keepCap }; { parses. renders. reused. overBudget. tooLarge. tooDeep.
	cLimit. kept }. */
EXPORT(sqInt)
primitiveInfo(void)
{
	sqInt answer, names, limits, statistics, oop;
	const cm_stats *s;
	int i, n;

	if (interpreterProxy->methodArgumentCount() != 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	cm_init();
	n = cm_extension_count();
	s = cm_get_stats();
	if (!(answer = instantiate(interpreterProxy->classArray(), 7))
	 || !(names = instantiate(interpreterProxy->classArray(), (size_t)n))
	 || !(limits = instantiate(interpreterProxy->classArray(), 5))
	 || !(statistics = instantiate(interpreterProxy->classArray(), 8))) {
		return failNoMemory();
	}
	putInteger(answer, 0, CM_PROTOCOL);
	if (!(oop = stringObject(PLUGIN_VERSION))) {
		return failNoMemory();
	}
	put(answer, 1, oop);
	if (!(oop = stringObject(cm_version()))) {
		return failNoMemory();
	}
	put(answer, 2, oop);
	if (!(oop = stringObject(PLATFORM))) {
		return failNoMemory();
	}
	put(answer, 3, oop);
	for (i = 0; i < n; i++) {
		if (!(oop = stringObject(cm_extension_name(i)))) {
			return failNoMemory();
		}
		put(names, i, oop);
	}
	put(answer, 4, names);
	putInteger(limits, 0, CM_MAX_INPUT_BYTES);
	putInteger(limits, 1, CM_MAX_BLOCK_BYTES);
	putInteger(limits, 2, cm_default_c_budget());
	putInteger(limits, 3, CM_REC_WORDS);
	putInteger(limits, 4, cm_keep_cap());
	put(answer, 5, limits);
	putInteger(statistics, 0, s->parses);
	putInteger(statistics, 1, s->renders);
	putInteger(statistics, 2, s->reused);
	putInteger(statistics, 3, overBudget);
	putInteger(statistics, 4, s->tooLarge);
	putInteger(statistics, 5, s->tooDeep);
	putInteger(statistics, 6, s->cLimit);
	putInteger(statistics, 7, cm_kept_bytes());
	put(answer, 6, statistics);
	return interpreterProxy->methodReturnValue(answer);
}

/*	CMarkPrimitives class>>primParse: source options: o extensions: e
		maxDepth: d budget: b cBudget: c
	<primitive: 'primitiveParse' module: 'CMarkPlugin' error: ec>
	Answer { status. code. tape. strings. nodeCount. depth. recordWords.
	sourceChars. sanitised } (docs/CMark.md, "Tree format").  Fail with
	#'bad number of arguments', #'bad argument', #'not found' (an extension
	of the mask is missing), #'insufficient object memory'. */
EXPORT(sqInt)
primitiveParse(void)
{
	Arguments a;
	cm_tree tree;
	uint64_t code, need;
	sqInt failure, answer, tape, strings, sanitised = 0;
	int status;

	if ((failure = argumentsOf(0, &a))) {
		return interpreterProxy->primitiveFailFor(failure);
	}
	status = cm_parse(&a.source, (int)a.options, (int)a.extensions, maxDepthOf(&a), a.cBudget, &tree, &code);
	if (status != CM_OK) {
		answer = parseAnswer(status, code, &tree, 0);
		return answer ? interpreterProxy->methodReturnValue(answer) : failNoMemory();
	}
	need = spurPointers(9) + spurWords32((uint64_t)tree.nodes * CM_REC_WORDS) + spurBytes(tree.stringBytes)
		+ (tree.t.differs ? spurBytes(tree.t.textBytes) : 0);
	if (a.budget >= 0 && need > (uint64_t)a.budget) {
		answer = parseAnswer(CM_OVER_BUDGET, need, &tree, 0);
		cm_keep_tree((int)a.options, (int)a.extensions, maxDepthOf(&a), &tree);
		if (!answer) {
			return failNoMemory();
		}
		overBudget++;
		return interpreterProxy->methodReturnValue(answer);
	}
	if (!(answer = parseAnswer(CM_OK, 0, &tree, tree.nodes))
	 || !(tape = unitsObject(interpreterProxy->classBitmap(), tree.tape, tree.nodes * CM_REC_WORDS,
			sizeof(uint32_t)))
	 || !(strings = bytesObject(interpreterProxy->classByteArray(), tree.strings, tree.stringBytes))
	 || (tree.t.differs
	     && !(sanitised = bytesObject(interpreterProxy->classByteArray(), tree.t.text, tree.t.textBytes)))) {
		cm_keep_tree((int)a.options, (int)a.extensions, maxDepthOf(&a), &tree);
		return failNoMemory();
	}
	put(answer, 2, tape);
	put(answer, 3, strings);
	if (sanitised) {
		put(answer, 8, sanitised);
	}
	cm_tree_free(&tree);
	return interpreterProxy->methodReturnValue(answer);
}

/*	CMarkPrimitives class>>primRender: source format: f options: o
		extensions: e width: w budget: b cBudget: c
	<primitive: 'primitiveRender' module: 'CMarkPlugin' error: ec>
	Answer { status. code. bytes. ascii. byteSize }: bytes, the output in
	UTF-8 (format 1 html, 2 xml, 3 commonmark, 4 plaintext, 5 latex, 6 man),
	nil unless the status is 0.  Fail as primitiveParse. */
EXPORT(sqInt)
primitiveRender(void)
{
	Arguments a;
	cm_output output;
	uint64_t code, need;
	sqInt failure, answer, bytes;
	int status;

	if ((failure = argumentsOf(1, &a))) {
		return interpreterProxy->primitiveFailFor(failure);
	}
	status = cm_render(&a.source, (int)a.format, (int)a.options, (int)a.extensions, (int)a.width,
		a.cBudget, &output, &code);
	if (status == CM_OK) {
		need = spurPointers(5) + spurBytes(output.size);
		if (a.budget >= 0 && need > (uint64_t)a.budget) {
			status = CM_OVER_BUDGET;
			code = need;
		}
	}
	if (!(answer = instantiate(interpreterProxy->classArray(), 5))) {
		cm_keep_output((int)a.format, (int)a.options, (int)a.extensions, (int)a.width, &output);
		return failNoMemory();
	}
	putInteger(answer, 0, (uint64_t)status);
	putInteger(answer, 1, code);
	/* ascii and byteSize are known for an output, also over the budget */
	put(answer, 3, (status == CM_OK || status == CM_OVER_BUDGET) && output.ascii
		? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	putInteger(answer, 4, status == CM_OK || status == CM_OVER_BUDGET ? output.size : 0);
	if (status == CM_OVER_BUDGET) {
		cm_keep_output((int)a.format, (int)a.options, (int)a.extensions, (int)a.width, &output);
		overBudget++;
		return interpreterProxy->methodReturnValue(answer);
	}
	if (status == CM_OK) {
		if (!(bytes = bytesObject(interpreterProxy->classByteArray(), output.bytes, output.size))) {
			cm_keep_output((int)a.format, (int)a.options, (int)a.extensions, (int)a.width, &output);
			return failNoMemory();
		}
		put(answer, 2, bytes);
		cm_output_free(&output);
	}
	return interpreterProxy->methodReturnValue(answer);
}

/*	CMarkPrimitives class>>primRelease
	<primitive: 'primitiveRelease' module: 'CMarkPlugin' error: ec>
	Free the kept result, and answer the bytes of C memory it took (0 when
	nothing was kept). */
EXPORT(sqInt)
primitiveRelease(void)
{
	if (interpreterProxy->methodArgumentCount() != 0) {
		return interpreterProxy->primitiveFailFor(PrimErrBadNumArgs);
	}
	return interpreterProxy->methodReturnValue(interpreterProxy->integerObjectOf((sqInt)cm_release_kept()));
}

#ifdef SQUEAK_BUILTIN_PLUGIN

static char _m[] = "CMarkPlugin";
void* CMarkPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "initialiseModule", (void*)initialiseModule},
	{(void*)_m, "primitiveInfo\000\000", (void*)primitiveInfo},
	{(void*)_m, "primitiveParse\000\000", (void*)primitiveParse},
	{(void*)_m, "primitiveRelease\000\000", (void*)primitiveRelease},
	{(void*)_m, "primitiveRender\000\000", (void*)primitiveRender},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{(void*)_m, "shutdownModule", (void*)shutdownModule},
	{NULL, NULL, NULL}
};

#else /* ifdef SQUEAK_BUILTIN_PLUGIN */

EXPORT(signed char) primitiveInfoAccessorDepth = 0;
EXPORT(signed char) primitiveParseAccessorDepth = 0;
EXPORT(signed char) primitiveReleaseAccessorDepth = 0;
EXPORT(signed char) primitiveRenderAccessorDepth = 0;

#endif /* ifdef SQUEAK_BUILTIN_PLUGIN */
