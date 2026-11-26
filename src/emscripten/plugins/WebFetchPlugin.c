/* WebFetchPlugin.c -- the HTTP requests of the image, by the fetch of the host (Emscripten only)
 *
 * A builtin plugin, written by hand in the form Slang generates, so that the
 * build's void primitive trampolines (cmake/emscripten/genPrimitiveTable.cmake)
 * apply to its exports table as they do to the generated plugins'.
 *
 * The requests are in JavaScript, Module.webFetch (packaging/emscripten/host/
 * web-fetch.js, which tells their states and their records).  A request has an
 * id, and the index of an external semaphore of the image, which it signals
 * between the slices; the primitives only start requests, look at them, copy
 * what came and close them.  The body of an answer stays in the JavaScript
 * heap until it is copied into an object of the image, once.
 *
 * The package WebFetch of the image is their only user.  No primitive waits,
 * so all of them work inside an FFI callback too; what waits there looks at
 * primitiveMayWait of the WebHostPlugin first.
 */

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/*  The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions */

#define true 1
#define false 0
#define null 0

#include <limits.h>
#include <emscripten.h>

#if !defined(SQUEAK_BUILTIN_PLUGIN)
# error "WebFetchPlugin is a builtin plugin"
#endif

/* only in the generated interp.h, not in virtualMachine.h */
#ifndef PrimErrLimitExceeded
# define PrimErrLimitExceeded 15
#endif

#define FETCH_READY 1			/* of availability() */
#define FETCH_MAX_HEAD 1048576	/* the largest request record */

extern int ioGetMaxExtSemTableSize(void);
extern sqInt isCompiledMethod(sqInt objOop);


/*** Module.webFetch ***
 *
 * A pointer reaches JavaScript as a BigInt in wasm64: Number() it.  HEAPU8 is
 * read in each body, since a memory growth renews it.  Nothing is thrown
 * through the interpreter.  Sizes and counts of at most 2^31-1 are ints.
 */
EM_JS_DEPS(webFetch, "$UTF8ToString");

EM_JS(int, js_fetch_availability, (), {
	try {
		return Module['webFetch'] ? Module['webFetch'].availability() : 0;
	} catch (e) { err('webFetch.availability: ' + e); }
	return 0;
});

/* The id, 0 when too many are open, -1 when the copy of the body failed */
EM_JS(int, js_fetch_start, (int semaphoreIndex, const char *head, int headSize,
		const char *body, int bodySize, int window, int timeoutMs, int flags), {
	try {
		var from = Number(body);
		return Module['webFetch'].start(semaphoreIndex, UTF8ToString(Number(head), headSize, true),
			from ? HEAPU8.slice(from, from + bodySize) : null, window, timeoutMs, flags);
	} catch (e) { err('webFetch.start: ' + e); }
	return -1;
});

EM_JS(double, js_fetch_state, (int id), {
	try {
		return Module['webFetch'] ? Module['webFetch'].state(id) : -1;
	} catch (e) { err('webFetch.state: ' + e); }
	return -1;
});

EM_JS(int, js_fetch_head_size, (int id), {
	try {
		return Module['webFetch'].headSize(id);
	} catch (e) { err('webFetch.headSize: ' + e); }
	return -1;
});

EM_JS(int, js_fetch_head, (int id, char *to, int size), {
	try {
		var start = Number(to);
		return Module['webFetch'].head(id, HEAPU8.subarray(start, start + size));
	} catch (e) { err('webFetch.head: ' + e); }
	return -1;
});

/* The bytes copied, or -1 for an unknown id */
EM_JS(int, js_fetch_read, (int id, char *to, int count), {
	try {
		var start = Number(to);
		return Module['webFetch'] ? Module['webFetch'].read(id, HEAPU8.subarray(start, start + count)) : -1;
	} catch (e) { err('webFetch.read: ' + e); }
	return -1;
});

EM_JS(int, js_fetch_close, (int id), {
	try {
		return Module['webFetch'] ? Module['webFetch'].close(id) : 0;
	} catch (e) { err('webFetch.close: ' + e); }
	return 0;
});


/*** Function Prototypes ***/
static const char *getModuleName(void);
static sqInt isByteArray(sqInt oop);
static int requestIdAt(sqInt stackIndex);
static void primitiveFetchAvailability(void);
static void primitiveFetchClose(void);
static void primitiveFetchHead(void);
static void primitiveFetchRead(void);
static void primitiveFetchStart(void);
static void primitiveFetchState(void);
static sqInt setInterpreter(struct VirtualMachine *anInterpreter);


/*** Variables ***/
static struct VirtualMachine *interpreterProxy;
static const char *moduleName = "WebFetchPlugin (i)";


/*	The module name is used to check, once a module is loaded, that it is the
	module that was asked for. */

static const char *
getModuleName(void)
{
	return moduleName;
}


/*	Whether oop is a ByteArray.  So is a CompiledMethod in the image, whose
	bytes are not to be read or written here. */

static sqInt
isByteArray(sqInt oop)
{
	return interpreterProxy->isKindOfClass(oop, interpreterProxy->classByteArray())
	 && !isCompiledMethod(oop);
}


/*	The id of a request on the stack, 0 for a SmallInteger that is none.
	Fails for another object. */

static int
requestIdAt(sqInt stackIndex)
{
	sqInt id;

	id = interpreterProxy->stackIntegerValue(stackIndex);
	if (interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return 0;
	}
	return id >= 1 && id <= INT_MAX ? (int)id : 0;
}


/*	Answer 0 when the host has no fetch, 1 when requests may start, 2 when
	the network is off (host-events.js). */

static void
primitiveFetchAvailability(void)
{
	interpreterProxy->popthenPush(1, interpreterProxy->integerObjectOf(js_fetch_availability()));
}


/*	primitiveFetchClose: id

	Abort the request unless it is over, and forget it: its semaphore is not
	signalled any more.  Answer whether it was open. */

static void
primitiveFetchClose(void)
{
	int id;
	sqInt wasOpen;

	id = requestIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	wasOpen = id && js_fetch_close(id);
	interpreterProxy->popthenPush(2, wasOpen ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
}


/*	primitiveFetchHead: id

	Answer the record of the answer, or of the error, as a new ByteArray; nil
	while there is none. */

static void
primitiveFetchHead(void)
{
	int id, size;
	sqInt head;

	id = requestIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	if (!id || js_fetch_state(id) < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	size = js_fetch_head_size(id);
	if (size < 0) {
		interpreterProxy->popthenPush(2, interpreterProxy->nilObject());
		return;
	}
	head = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), size);
	if (!head) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	js_fetch_head(id, interpreterProxy->firstIndexableField(head), size);
	interpreterProxy->popthenPush(2, head);
}


/*	primitiveFetchRead: id into: aByteArray startingAt: start count: count

	Copy at most count bytes of the body that are ready into aByteArray from
	the index start on.  Answer how many. */

static void
primitiveFetchRead(void)
{
	int id, copied;
	sqInt bytes, start, count;

	id = requestIdAt(3);
	bytes = interpreterProxy->stackValue(2);
	start = interpreterProxy->stackIntegerValue(1);
	count = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed() || !isByteArray(bytes)) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (interpreterProxy->isOopImmutable(bytes)) {
		interpreterProxy->primitiveFailFor(PrimErrNoModification);
		return;
	}
	if (start < 1 || count < 0 || start - 1 + count > interpreterProxy->byteSizeOf(bytes)) {
		interpreterProxy->primitiveFailFor(PrimErrBadIndex);
		return;
	}
	if (count > INT_MAX) {
		count = INT_MAX;
	}
	copied = id ? js_fetch_read(id, (char *)interpreterProxy->firstIndexableField(bytes) + start - 1, (int)count) : -1;
	if (copied < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	interpreterProxy->popthenPush(5, interpreterProxy->integerObjectOf(copied));
}


/*	primitiveFetchStart: semaphoreIndex head: head body: body window: window
		timeout: timeoutMs flags: flags

	Start a request: head is its record, a ByteArray, body nil or a
	ByteArray, which is copied; window is the size at which the body of the
	answer waits to be read, timeoutMs 0 is none; flags 1 does not follow
	redirects, flags 2 is the cache mode no-store, and 3 is both.  Answer the
	id of the request. */

static void
primitiveFetchStart(void)
{
	char *bodyBytes;
	int id, availability;
	sqInt semaphoreIndex, head, headSize, body, bodySize, window, timeoutMs, flags;

	semaphoreIndex = interpreterProxy->stackIntegerValue(5);
	head = interpreterProxy->stackValue(4);
	body = interpreterProxy->stackValue(3);
	window = interpreterProxy->stackIntegerValue(2);
	timeoutMs = interpreterProxy->stackIntegerValue(1);
	flags = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed()
	 || semaphoreIndex < 1 || semaphoreIndex > ioGetMaxExtSemTableSize()
	 || !isByteArray(head)
	 || (body != interpreterProxy->nilObject() && !isByteArray(body))
	 || window < 1 || window > INT_MAX
	 || timeoutMs < 0 || timeoutMs > INT_MAX
	 || flags < 0 || flags > 3) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	headSize = interpreterProxy->byteSizeOf(head);
	bodyBytes = NULL;
	bodySize = 0;
	if (body != interpreterProxy->nilObject()) {
		bodyBytes = interpreterProxy->firstIndexableField(body);
		bodySize = interpreterProxy->byteSizeOf(body);
	}
	if (headSize < 1 || headSize > FETCH_MAX_HEAD || bodySize > INT_MAX) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	availability = js_fetch_availability();
	if (availability != FETCH_READY) {
		interpreterProxy->primitiveFailFor(PrimErrUnsupported);
		return;
	}
	id = js_fetch_start((int)semaphoreIndex, interpreterProxy->firstIndexableField(head), (int)headSize,
		bodyBytes, (int)bodySize, (int)window, (int)timeoutMs, (int)flags);
	if (id < 1) {
		interpreterProxy->primitiveFailFor(id ? PrimErrNoCMemory : PrimErrLimitExceeded);
		return;
	}
	interpreterProxy->popthenPush(7, interpreterProxy->integerObjectOf(id));
}


/*	primitiveFetchState: id

	Answer the state, 1 to 4 (web-fetch.js), + 8 * the bytes of the body that
	are ready. */

static void
primitiveFetchState(void)
{
	int id;
	double stateAndSize;

	id = requestIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	stateAndSize = id ? js_fetch_state(id) : -1;
	if (stateAndSize < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf((sqInt)stateAndSize));
}

static sqInt
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR))
	 && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));
}


static char _m[] = "WebFetchPlugin";
void* WebFetchPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "primitiveFetchAvailability\000\377", (void*)primitiveFetchAvailability},
	{(void*)_m, "primitiveFetchClose\000\377", (void*)primitiveFetchClose},
	{(void*)_m, "primitiveFetchHead\000\377", (void*)primitiveFetchHead},
	{(void*)_m, "primitiveFetchRead\000\000", (void*)primitiveFetchRead},
	{(void*)_m, "primitiveFetchStart\000\000", (void*)primitiveFetchStart},
	{(void*)_m, "primitiveFetchState\000\377", (void*)primitiveFetchState},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{NULL, NULL, NULL}
};
