/* WebSocketPlugin.c -- the WebSocket client connections of the image, by the WebSocket of the host (Emscripten only)
 *
 * A builtin plugin, written by hand in the form Slang generates, so that the
 * build's void primitive trampolines (cmake/emscripten/genPrimitiveTable.cmake)
 * apply to its exports table as they do to the generated plugins'.
 *
 * The sockets are in JavaScript, Module.webSocket (packaging/emscripten/host/
 * web-socket.js, which tells their states, their signals and their records).
 * A socket has an id, and the indexes of three external semaphores of the
 * image, state, read and write, which it signals between the slices; the
 * primitives only open sockets, look at them, take what came, send, close
 * and release them.  A message stays in the JavaScript heap until it is
 * copied into an object of the image, once.
 *
 * The package WebFetch-WebSocket of the image is their only user.  No
 * primitive waits, so all of them work inside an FFI callback too; what
 * waits there looks at primitiveMayWait of the WebHostPlugin first.  A
 * plugin of its own, not more primitives of the WebFetchPlugin: a host may
 * have fetch and no WebSocket, and an image on a VM without this plugin
 * still has fetch.
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
# error "WebSocketPlugin is a builtin plugin"
#endif

/* only in the generated interp.h, not in virtualMachine.h */
#ifndef PrimErrLimitExceeded
# define PrimErrLimitExceeded 15
#endif

#define WS_READY 1				/* of availability() */
#define WS_MAX_RECORD 65536		/* the largest open record */
#define WS_MAX_LIMIT (1 << 30)	/* the largest maxMessage and maxQueued */
#define WS_MAX_SEND (64 * 1024 * 1024)	/* the largest message sent */
#define WS_MAX_REASON 123		/* bytes of UTF-8 in a close frame */

extern int ioGetMaxExtSemTableSize(void);
extern sqInt isCompiledMethod(sqInt objOop);


/*** Module.webSocket ***
 *
 * A pointer reaches JavaScript as a BigInt in wasm64: Number() it.  HEAPU8 is
 * read in each body, since a memory growth renews it.  Nothing is thrown
 * through the interpreter.  Sizes of at most 2^31-1 are ints; the answers
 * that may be larger (a state, bufferedAmount) are doubles.
 */

EM_JS(int, js_ws_availability, (), {
	try {
		return Module['webSocket'] ? Module['webSocket'].availability() : 0;
	} catch (e) { err('webSocket.availability: ' + e); }
	return 0;
});

/* The id, 0 when too many are open, -1 when the copy of the record failed */
EM_JS(int, js_ws_open, (int stateIndex, int readIndex, int writeIndex, const char *record, int recordSize,
		int maxMessage, int maxQueued, int timeoutMs), {
	try {
		var from = Number(record);
		return Module['webSocket'].open(stateIndex, readIndex, writeIndex, HEAPU8.slice(from, from + recordSize),
			maxMessage, maxQueued, timeoutMs);
	} catch (e) { err('webSocket.open: ' + e); }
	return -1;
});

EM_JS(double, js_ws_state, (int id), {
	try {
		return Module['webSocket'] ? Module['webSocket'].state(id) : -1;
	} catch (e) { err('webSocket.state: ' + e); }
	return -1;
});

EM_JS(int, js_ws_info_size, (int id), {
	try {
		return Module['webSocket'].infoSize(id);
	} catch (e) { err('webSocket.infoSize: ' + e); }
	return -1;
});

EM_JS(int, js_ws_info, (int id, char *to, int size), {
	try {
		var start = Number(to);
		return Module['webSocket'].info(id, HEAPU8.subarray(start, start + size));
	} catch (e) { err('webSocket.info: ' + e); }
	return -1;
});

/* -1 when none is queued, else 2 * the size of the next message, + 1 for text */
EM_JS(double, js_ws_peek, (int id), {
	try {
		return Module['webSocket'].peek(id);
	} catch (e) { err('webSocket.peek: ' + e); }
	return -1;
});

EM_JS(int, js_ws_receive, (int id, char *to, int size), {
	try {
		var start = Number(to);
		return Module['webSocket'].receive(id, HEAPU8.subarray(start, start + size));
	} catch (e) { err('webSocket.receive: ' + e); }
	return -1;
});

EM_JS(double, js_ws_drop, (int id), {
	try {
		return Module['webSocket'].drop(id);
	} catch (e) { err('webSocket.drop: ' + e); }
	return -1;
});

/* bufferedAmount after, or -1 to -5 (web-socket.js) */
EM_JS(double, js_ws_send, (int id, const char *bytes, int size, int isText), {
	try {
		var start = Number(bytes);
		return Module['webSocket'].send(id, HEAPU8.subarray(start, start + size), !!isText);
	} catch (e) { err('webSocket.send: ' + e); }
	return -5;
});

EM_JS(double, js_ws_buffered, (int id), {
	try {
		return Module['webSocket'].buffered(id);
	} catch (e) { err('webSocket.buffered: ' + e); }
	return -1;
});

EM_JS(int, js_ws_close, (int id, int code, const char *reason, int size), {
	try {
		var start = Number(reason);
		return Module['webSocket'].close(id, code, start ? HEAPU8.slice(start, start + size) : new Uint8Array(0));
	} catch (e) { err('webSocket.close: ' + e); }
	return -1;
});

EM_JS(int, js_ws_release, (int id), {
	try {
		return Module['webSocket'] ? Module['webSocket'].release(id) : 0;
	} catch (e) { err('webSocket.release: ' + e); }
	return 0;
});


/*** Function Prototypes ***/
static const char *getModuleName(void);
static sqInt isByteArray(sqInt oop);
static int isSemaphoreIndex(sqInt index);
static int socketIdAt(sqInt stackIndex);
static int knownSocketAt(sqInt stackIndex);
static void primitiveWebSocketAvailability(void);
static void primitiveWebSocketBuffered(void);
static void primitiveWebSocketClose(void);
static void primitiveWebSocketDrop(void);
static void primitiveWebSocketInfo(void);
static void primitiveWebSocketOpen(void);
static void primitiveWebSocketReceive(void);
static void primitiveWebSocketRelease(void);
static void primitiveWebSocketSend(void);
static void primitiveWebSocketState(void);
static sqInt setInterpreter(struct VirtualMachine *anInterpreter);


/*** Variables ***/
static struct VirtualMachine *interpreterProxy;
static const char *moduleName = "WebSocketPlugin (i)";


/*	The module name is used to check, once a module is loaded, that it is the
	module that was asked for. */

static const char *
getModuleName(void)
{
	return moduleName;
}


/*	Whether oop is a ByteArray.  So is a CompiledMethod in the image, whose
	bytes are not to be read here. */

static sqInt
isByteArray(sqInt oop)
{
	return interpreterProxy->isKindOfClass(oop, interpreterProxy->classByteArray())
	 && !isCompiledMethod(oop);
}


/*	Whether index is one of the table of external semaphores, as the
	WebFetchPlugin takes them. */

static int
isSemaphoreIndex(sqInt index)
{
	return index >= 1 && index <= ioGetMaxExtSemTableSize();
}


/*	The id of a socket on the stack, 0 for a SmallInteger that is none (0,
	negative, 2^31 and up).  Fails for another object. */

static int
socketIdAt(sqInt stackIndex)
{
	sqInt id;

	id = interpreterProxy->stackIntegerValue(stackIndex);
	if (interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return 0;
	}
	return id >= 1 && id <= INT_MAX ? (int)id : 0;
}


/*	The id of a socket that the host has, on the stack; fails with 'bad
	argument' for no SmallInteger, and 'not found' for no such socket. */

static int
knownSocketAt(sqInt stackIndex)
{
	int id;

	id = socketIdAt(stackIndex);
	if (interpreterProxy->failed()) {
		return 0;
	}
	if (!id || js_ws_state(id) < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return 0;
	}
	return id;
}


/*	Answer 0 when the host has no WebSocket, 1 when sockets may open, 2 when
	the network is off (host-events.js). */

static void
primitiveWebSocketAvailability(void)
{
	interpreterProxy->popthenPush(1, interpreterProxy->integerObjectOf(js_ws_availability()));
}


/*	primitiveWebSocketBuffered: id

	Answer the bytes sent that the host has not handed to the network yet. */

static void
primitiveWebSocketBuffered(void)
{
	int id;
	double buffered;

	id = socketIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	buffered = id ? js_ws_buffered(id) : -1;
	if (buffered < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf((sqInt)buffered));
}


/*	primitiveWebSocketClose: id code: code reason: reason

	Start the closing handshake: code is 1000 or in 3000..4999 (what the
	WebSocket of the host takes from a client), reason nil or a ByteArray of
	at most 123 bytes of UTF-8.  Answer true when it started, false when the
	socket was closing or over already. */

static void
primitiveWebSocketClose(void)
{
	int id, answer, reasonSize;
	sqInt code, reason;
	char *reasonBytes;

	id = socketIdAt(2);
	code = interpreterProxy->stackIntegerValue(1);
	reason = interpreterProxy->stackValue(0);
	if (interpreterProxy->failed()
	 || !(code == 1000 || (code >= 3000 && code <= 4999))
	 || (reason != interpreterProxy->nilObject() && !isByteArray(reason))) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	reasonBytes = NULL;
	reasonSize = 0;
	if (reason != interpreterProxy->nilObject()) {
		if (interpreterProxy->byteSizeOf(reason) > WS_MAX_REASON) {
			interpreterProxy->primitiveFailFor(PrimErrBadArgument);
			return;
		}
		reasonSize = (int)interpreterProxy->byteSizeOf(reason);
		reasonBytes = interpreterProxy->firstIndexableField(reason);
	}
	answer = id ? js_ws_close(id, (int)code, reasonSize ? reasonBytes : NULL, reasonSize) : -1;
	if (answer < 0) {
		interpreterProxy->primitiveFailFor(answer == -1 ? PrimErrNotFound : PrimErrBadArgument);
		return;
	}
	interpreterProxy->popthenPush(4, answer ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
}


/*	primitiveWebSocketDrop: id

	Drop the next message without copying it, which fails the socket with
	tooLarge unless it is over: for one that the image cannot hold.  Answer
	its size, nil when none is queued. */

static void
primitiveWebSocketDrop(void)
{
	int id;
	double size;

	id = knownSocketAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	size = js_ws_drop(id);
	interpreterProxy->popthenPush(2, size < 0
		? interpreterProxy->nilObject()
		: interpreterProxy->integerObjectOf((sqInt)size));
}


/*	primitiveWebSocketInfo: id

	Answer the info record (web-socket.js) as a new ByteArray; nil in state
	1, while there is none. */

static void
primitiveWebSocketInfo(void)
{
	int id, size;
	sqInt info;

	id = knownSocketAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	size = js_ws_info_size(id);
	if (size < 0) {
		interpreterProxy->popthenPush(2, interpreterProxy->nilObject());
		return;
	}
	info = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), size);
	if (!info) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	js_ws_info(id, interpreterProxy->firstIndexableField(info), size);
	interpreterProxy->popthenPush(2, info);
}


/*	primitiveWebSocketOpen: stateIndex read: readIndex write: writeIndex
		record: record maxMessage: maxMessage maxQueued: maxQueued
		timeout: timeoutMs

	Open a socket: the three indexes are of distinct external semaphores,
	record is the open record (a ByteArray: the URL, then one subprotocol
	per line), maxMessage and maxQueued the largest message and the most
	bytes queued unread (1..2^30), timeoutMs the time to the open (0 none).
	Answer the id; the socket is made, and its record checked, after the
	primitive. */

static void
primitiveWebSocketOpen(void)
{
	int id;
	sqInt stateIndex, readIndex, writeIndex, record, recordSize, maxMessage, maxQueued, timeoutMs;

	stateIndex = interpreterProxy->stackIntegerValue(6);
	readIndex = interpreterProxy->stackIntegerValue(5);
	writeIndex = interpreterProxy->stackIntegerValue(4);
	record = interpreterProxy->stackValue(3);
	maxMessage = interpreterProxy->stackIntegerValue(2);
	maxQueued = interpreterProxy->stackIntegerValue(1);
	timeoutMs = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed()
	 || !isSemaphoreIndex(stateIndex) || !isSemaphoreIndex(readIndex) || !isSemaphoreIndex(writeIndex)
	 || stateIndex == readIndex || stateIndex == writeIndex || readIndex == writeIndex
	 || !isByteArray(record)
	 || maxMessage < 1 || maxMessage > WS_MAX_LIMIT
	 || maxQueued < 1 || maxQueued > WS_MAX_LIMIT
	 || timeoutMs < 0 || timeoutMs > INT_MAX) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	recordSize = interpreterProxy->byteSizeOf(record);
	if (recordSize < 1 || recordSize > WS_MAX_RECORD) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (js_ws_availability() != WS_READY) {
		interpreterProxy->primitiveFailFor(PrimErrUnsupported);
		return;
	}
	id = js_ws_open((int)stateIndex, (int)readIndex, (int)writeIndex, interpreterProxy->firstIndexableField(record),
		(int)recordSize, (int)maxMessage, (int)maxQueued, (int)timeoutMs);
	if (id < 1) {
		interpreterProxy->primitiveFailFor(id ? PrimErrNoCMemory : PrimErrLimitExceeded);
		return;
	}
	interpreterProxy->popthenPush(8, interpreterProxy->integerObjectOf(id));
}


/*	primitiveWebSocketReceive: id

	Answer the next message as a new Array {isText. bytes}, true or false
	and a ByteArray, which leaves the queue; nil when none is queued.  The
	type and the bytes come in one call: no slice can end between learning
	of a message and taking it.  Without the memory for it the message stays
	queued. */

static void
primitiveWebSocketReceive(void)
{
	int id, size, isText;
	double descriptor;
	sqInt bytes, message;

	id = knownSocketAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	descriptor = js_ws_peek(id);
	if (descriptor < 0) {
		interpreterProxy->popthenPush(2, interpreterProxy->nilObject());
		return;
	}
	size = (int)(descriptor / 2);
	isText = (int)descriptor & 1;
	bytes = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), size);
	if (!bytes) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	interpreterProxy->pushRemappableOop(bytes);
	message = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 2);
	bytes = interpreterProxy->popRemappableOop();
	if (!message) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	/* nothing of JavaScript ran since the peek: the same message */
	if (js_ws_receive(id, interpreterProxy->firstIndexableField(bytes), size) != size) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	interpreterProxy->storePointerofObjectwithValue(0, message,
		isText ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(1, message, bytes);
	interpreterProxy->popthenPush(2, message);
}


/*	primitiveWebSocketRelease: id

	Forget the socket: its semaphores are not signalled any more, and its
	WebSocket, unless closed, is closed with 1000.  Answer whether it was
	known. */

static void
primitiveWebSocketRelease(void)
{
	int id;
	sqInt wasKnown;

	id = socketIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	wasKnown = id && js_ws_release(id);
	interpreterProxy->popthenPush(2, wasKnown ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
}


/*	primitiveWebSocketSend: id bytes: aByteArray isText: aBoolean

	Send one message of at most 64 MiB, as text (the bytes are UTF-8) or as
	binary.  Answer bufferedAmount after it.  'resource limit exceeded' says
	that the send buffer is full and nothing was sent: wait for the write
	semaphore. */

static void
primitiveWebSocketSend(void)
{
	int id;
	sqInt bytes, isText, size;
	double answer;

	id = socketIdAt(2);
	bytes = interpreterProxy->stackValue(1);
	isText = interpreterProxy->stackValue(0);
	if (interpreterProxy->failed()
	 || !isByteArray(bytes)
	 || (isText != interpreterProxy->trueObject() && isText != interpreterProxy->falseObject())) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	size = interpreterProxy->byteSizeOf(bytes);
	if (size > WS_MAX_SEND) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	answer = id
		? js_ws_send(id, interpreterProxy->firstIndexableField(bytes), (int)size, isText == interpreterProxy->trueObject())
		: -1;
	if (answer < 0) {
		interpreterProxy->primitiveFailFor(
			answer == -1 ? PrimErrNotFound
			: answer == -2 ? PrimErrInappropriate
			: answer == -3 ? PrimErrLimitExceeded
			: answer == -4 ? PrimErrBadArgument
			: PrimErrNoCMemory);
		return;
	}
	interpreterProxy->popthenPush(4, interpreterProxy->integerObjectOf((sqInt)answer));
}


/*	primitiveWebSocketState: id

	Answer the state, 1 to 5 (web-socket.js), + 8 * the number of messages
	queued. */

static void
primitiveWebSocketState(void)
{
	int id;
	double state;

	id = socketIdAt(0);
	if (interpreterProxy->failed()) {
		return;
	}
	state = id ? js_ws_state(id) : -1;
	if (state < 0) {
		interpreterProxy->primitiveFailFor(PrimErrNotFound);
		return;
	}
	interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf((sqInt)state));
}

static sqInt
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR))
	 && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));
}


static char _m[] = "WebSocketPlugin";
void* WebSocketPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "primitiveWebSocketAvailability\000\377", (void*)primitiveWebSocketAvailability},
	{(void*)_m, "primitiveWebSocketBuffered\000\377", (void*)primitiveWebSocketBuffered},
	{(void*)_m, "primitiveWebSocketClose\000\000", (void*)primitiveWebSocketClose},
	{(void*)_m, "primitiveWebSocketDrop\000\377", (void*)primitiveWebSocketDrop},
	{(void*)_m, "primitiveWebSocketInfo\000\377", (void*)primitiveWebSocketInfo},
	{(void*)_m, "primitiveWebSocketOpen\000\000", (void*)primitiveWebSocketOpen},
	{(void*)_m, "primitiveWebSocketReceive\000\377", (void*)primitiveWebSocketReceive},
	{(void*)_m, "primitiveWebSocketRelease\000\377", (void*)primitiveWebSocketRelease},
	{(void*)_m, "primitiveWebSocketSend\000\000", (void*)primitiveWebSocketSend},
	{(void*)_m, "primitiveWebSocketState\000\377", (void*)primitiveWebSocketState},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{NULL, NULL, NULL}
};
