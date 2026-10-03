/* WebDisplayPlugin.c -- the display of the Pharo world in a web page (Emscripten only)
 *
 * A builtin plugin, written by hand in the form Slang generates, so that the
 * build's void primitive trampolines (cmake/emscripten/genPrimitiveTable.cmake)
 * apply to its exports table as they do to the generated plugins'.
 *
 * The VM runs in a Web Worker and the page owns the canvas.  The image's
 * OSWebDriver (packaging/emscripten/st/OSWindow-Web) reaches the page only
 * through the primitives below, and the page reaches the VM only through the
 * webdisplay_* exports, which it calls between slices:
 *
 *  - the image blits each damaged rectangle of its 32-bit Form into the
 *    frame, an RGBA copy of that Form, and presents the frame once per
 *    Morphic cycle: the dirty rectangles go to the page, and the slice ends,
 *    so that the canvas shows them as soon as the worker gets its thread back;
 *  - the page pushes its input as records of 8 integers into a ring, and
 *    each push signals the image's input semaphore;
 *  - title, cursor, clipboard and focus requests go to the page as well, and
 *    the size of the canvas and the text pasted into the page come back.
 *
 * The page's side is Module.webDisplay, an object the host gives the VM
 * (display-worker.js on the world page).  Without it, in the node command
 * line VM and on the Console page, primitiveWebDisplayIsAvailable answers
 * false, so the image never uses the others.  Its methods are called inside
 * a slice and must not call back into the VM:
 *
 *	open(width, height, title)	the world's window is opening
 *	present(pixels, width, height, rects, count)
 *		paint the frame: pixels is the byte address in HEAPU8 of its
 *		width * height RGBA pixels, row by row, and rects the byte
 *		address of count dirty rectangles, 4 int32 each (x, y, width,
 *		height) from HEAP32[rects >> 2].  Both are valid only during
 *		the call.
 *	setTitle(title)
 *	setCursor(rgba, width, height, hotX, hotY)
 *		rgba is a Uint8ClampedArray (a copy) of width * height RGBA pixels
 *	setClipboard(text)	the image copied text
 *	focus()			move the keyboard focus to the canvas
 *
 * Methods the object lacks are not called, and an exception is reported on
 * stderr rather than thrown through the interpreter.
 */

/*** The frame, the event ring and the clipboard ***
 *
 * Plain C, which needs nothing of the VM: tests/wasm/webdisplay-unit.c
 * includes this file with WEBDISPLAY_UNIT_TEST defined, which leaves out
 * the plugin after this part.
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define WEB_MAX_EXTENT	16384	/* the largest frame and canvas side */
#define WEB_MAX_CURSOR	256	/* the largest cursor side */
#define WEB_MAX_DIRTY	64	/* dirty rectangles kept apart before they merge */
#define WEB_EVENT_SIZE	8	/* integers in an event record */
#define WEB_EVENT_RING	256	/* event records queued for the image */

/* RGBA pixels as little-endian words: 0xAABBGGRR */
#define WEB_OPAQUE_BLACK	0xFF000000u
#define WEB_OPAQUE_WHITE	0xFFFFFFFFu
#define WEB_TRANSPARENT		0x00000000u

/* The frame: the RGBA copy of the Form the image blits, which has the size
 * of the canvas, and the rectangles that changed since it was last presented.
 */
typedef struct {
	uint32_t *pixels;	/* width * height RGBA pixels, row by row */
	int width, height;
	int dirty[WEB_MAX_DIRTY][4];	/* x, y, width, height */
	int dirtyCount;
	int dirtyMerged;	/* dirty[0] bounds every change since the list overflowed */
} WebFrame;

/* The page's input, oldest first. */
typedef struct {
	int32_t records[WEB_EVENT_RING][WEB_EVENT_SIZE];
	int first, count;
} WebEventRing;

/* The clipboard's text, UTF-8: the latest of the text pasted into the page
 * and the text copied in the image.
 */
typedef struct {
	char *bytes;
	int size;	/* -1 until there is a text */
	int capacity;
} WebClipboard;


/*	A pixel of a 32-bit Form, 0xAARRGGBB, as an opaque RGBA pixel: Morphic
	leaves the alpha of many pixels at 0, which the canvas would not show. */

static inline uint32_t
webPixelFromFormPixel(uint32_t pixel)
{
	return WEB_OPAQUE_BLACK | (pixel & 0xFF) << 16 | (pixel & 0xFF00) | (pixel >> 16 & 0xFF);
}

static void
webFrameClearDirty(WebFrame *frame)
{
	frame->dirtyCount = 0;
	frame->dirtyMerged = 0;
}

static void
webFrameMarkAllDirty(WebFrame *frame)
{
	int *all = frame->dirty[0];

	all[0] = 0;
	all[1] = 0;
	all[2] = frame->width;
	all[3] = frame->height;
	frame->dirtyCount = 1;
	frame->dirtyMerged = 1;
}

/*	Add a rectangle, which is not empty and lies in the frame, to the dirty
	list.  Past WEB_MAX_DIRTY rectangles the list merges into its bounding
	box, which then grows with every later rectangle until the frame is
	presented. */

static void
webFrameAddDirty(WebFrame *frame, int x, int y, int width, int height)
{
	int left = x, top = y, right = x + width, bottom = y + height;
	int *box;

	if (!frame->dirtyMerged && frame->dirtyCount < WEB_MAX_DIRTY) {
		box = frame->dirty[frame->dirtyCount++];
		box[0] = x;
		box[1] = y;
		box[2] = width;
		box[3] = height;
		return;
	}
	for (int i = 0; i < frame->dirtyCount; i++) {
		int *rect = frame->dirty[i];

		if (rect[0] < left) left = rect[0];
		if (rect[1] < top) top = rect[1];
		if (rect[0] + rect[2] > right) right = rect[0] + rect[2];
		if (rect[1] + rect[3] > bottom) bottom = rect[1] + rect[3];
	}
	box = frame->dirty[0];
	box[0] = left;
	box[1] = top;
	box[2] = right - left;
	box[3] = bottom - top;
	frame->dirtyCount = 1;
	frame->dirtyMerged = 1;
}

/*	Give the frame the extent of the Form, 1 to WEB_MAX_EXTENT pixels each
	way.  A new extent reallocates the frame, keeps what it shared with the
	old one, makes the rest opaque black and marks it all dirty: the page
	cleared the canvas when it resized it.  Answer 0 when out of memory, the
	frame unchanged. */

static int
webFrameSetExtent(WebFrame *frame, int width, int height)
{
	uint32_t *pixels;
	size_t count = (size_t)width * height;

	if (frame->pixels && frame->width == width && frame->height == height)
		return 1;
	if (!(pixels = malloc(count * sizeof(uint32_t))))
		return 0;
	for (size_t i = 0; i < count; i++)
		pixels[i] = WEB_OPAQUE_BLACK;
	if (frame->pixels) {
		int rows = frame->height < height ? frame->height : height;
		int columns = frame->width < width ? frame->width : width;

		for (int y = 0; y < rows; y++)
			memcpy(pixels + (size_t)y * width, frame->pixels + (size_t)y * frame->width,
				columns * sizeof(uint32_t));
		free(frame->pixels);
	}
	frame->pixels = pixels;
	frame->width = width;
	frame->height = height;
	webFrameMarkAllDirty(frame);
	return 1;
}

/*	Copy the columns left to right - 1 of the rows top to bottom - 1 of a
	32-bit Form of width x height pixels into the frame, as RGBA, and mark
	them dirty.  The rectangle is clipped to the Form, and may end up empty.
	Answer 0 when the frame could not take the Form's extent. */

static int
webFrameBlit(WebFrame *frame, const uint32_t *bits, int width, int height,
	long left, long top, long right, long bottom)
{
	if (!webFrameSetExtent(frame, width, height))
		return 0;
	if (left < 0) left = 0;
	if (top < 0) top = 0;
	if (right > width) right = width;
	if (bottom > height) bottom = height;
	if (left >= right || top >= bottom)
		return 1;
	for (long y = top; y < bottom; y++) {
		const uint32_t *from = bits + (size_t)y * width;
		uint32_t *to = frame->pixels + (size_t)y * width;

		for (long x = left; x < right; x++)
			to[x] = webPixelFromFormPixel(from[x]);
	}
	/* clipped to the frame, so an int each */
	webFrameAddDirty(frame, (int)left, (int)top, (int)(right - left), (int)(bottom - top));
	return 1;
}

/*	Turn a cursor, two 1-bit Forms of width x height pixels, into RGBA
	pixels: black where both bits and mask are set, white where only the
	mask is, transparent elsewhere.  Each row of a 1-bit Form is a whole
	number of 32-bit words, its leftmost pixel in the most significant bit. */

static void
webCursorToRGBA(const uint32_t *bits, const uint32_t *mask, int width, int height, uint32_t *rgba)
{
	int wordsPerRow = (width + 31) / 32;

	for (int y = 0; y < height; y++)
		for (int x = 0; x < width; x++) {
			int word = y * wordsPerRow + x / 32;
			uint32_t bit = 0x80000000u >> (x % 32);

			*rgba++ = !(mask[word] & bit) ? WEB_TRANSPARENT
				: bits[word] & bit ? WEB_OPAQUE_BLACK : WEB_OPAQUE_WHITE;
		}
}

/*	Queue an event record; answer 0 when the ring is full. */

static int
webEventPush(WebEventRing *ring, const int32_t *record)
{
	if (ring->count == WEB_EVENT_RING)
		return 0;
	memcpy(ring->records[(ring->first + ring->count) % WEB_EVENT_RING], record, sizeof(ring->records[0]));
	ring->count += 1;
	return 1;
}

/*	Take the oldest event record; answer 0 when the ring is empty. */

static int
webEventPop(WebEventRing *ring, int32_t *record)
{
	if (!ring->count)
		return 0;
	memcpy(record, ring->records[ring->first], sizeof(ring->records[0]));
	ring->first = (ring->first + 1) % WEB_EVENT_RING;
	ring->count -= 1;
	return 1;
}

/*	Answer the clipboard's buffer, grown to hold size bytes, or NULL.  The
	page writes its pasted text there and commits it at once, so the text
	the buffer held is no longer the clipboard's. */

static char *
webClipboardBuffer(WebClipboard *clipboard, int size)
{
	char *bytes;

	if (size < 0)
		return NULL;
	if (!clipboard->bytes || size > clipboard->capacity) {
		if (!(bytes = realloc(clipboard->bytes, size ? size : 1)))
			return NULL;
		clipboard->bytes = bytes;
		clipboard->capacity = size;
	}
	return clipboard->bytes;
}

/*	The first size bytes of the buffer are the clipboard's text now. */

static void
webClipboardCommit(WebClipboard *clipboard, int size)
{
	if (clipboard->bytes && size >= 0 && size <= clipboard->capacity)
		clipboard->size = size;
}

/*	Make a copy of bytes the clipboard's text; answer 0 when out of memory. */

static int
webClipboardSet(WebClipboard *clipboard, const char *bytes, int size)
{
	char *buffer = webClipboardBuffer(clipboard, size);

	if (!buffer)
		return 0;
	memcpy(buffer, bytes, size);
	webClipboardCommit(clipboard, size);
	return 1;
}


#if !defined(WEBDISPLAY_UNIT_TEST)

/*** The plugin ***/

#include "sqConfig.h"			/* Configuration options */
#include "virtualMachine.h"		/*  The virtual machine proxy definition */
#include "sqPlatformSpecific.h"	/* Platform specific definitions */

#include <limits.h>

#define true 1
#define false 0
#define null 0

#include <emscripten.h>

#if !defined(SQUEAK_BUILTIN_PLUGIN)
# error "WebDisplayPlugin is a builtin plugin"
#endif


/*** Function Prototypes ***/
static const char *getModuleName(void);
static void primitiveWebBlit(void);
static void primitiveWebCanvasExtent(void);
static void primitiveWebClipboardText(void);
static void primitiveWebDisplayIsAvailable(void);
static void primitiveWebFocus(void);
static void primitiveWebNextEvent(void);
static void primitiveWebOpenCanvas(void);
static void primitiveWebPresent(void);
static void primitiveWebSetClipboardText(void);
static void primitiveWebSetCursor(void);
static void primitiveWebSetInputSemaphore(void);
static void primitiveWebSetTitle(void);
static sqInt setInterpreter(struct VirtualMachine *anInterpreter);


/*** Variables ***/
static struct VirtualMachine *interpreterProxy;
static const char *moduleName = "WebDisplayPlugin (i)";
static sqInt inputSemaphoreIndex = 0;
static WebFrame frame;
static WebEventRing events;
static WebClipboard clipboard = { NULL, -1, 0 };
static int canvasWidth = -1;	/* -1 until the page reports its canvas */
static int canvasHeight = -1;


/*** Module.webDisplay ***
 *
 * A pointer reaches JavaScript as a BigInt in wasm64: Number() it.
 */
EM_JS_DEPS(webDisplay, "$UTF8ToString");

EM_JS(int, js_display_available, (), {
	return Module['webDisplay'] ? 1 : 0;
});

EM_JS(void, js_display_open, (int width, int height, const char *title, int titleSize), {
	var display = Module['webDisplay'];
	try {
		if (display && display['open'])
			display['open'](width, height, UTF8ToString(Number(title), titleSize, true));
	} catch (e) { err('webDisplay.open: ' + e); }
});

EM_JS(void, js_display_present, (const uint32_t *pixels, int width, int height, const int *rects, int count), {
	var display = Module['webDisplay'];
	try {
		if (display && display['present'])
			display['present'](Number(pixels), width, height, Number(rects), count);
	} catch (e) { err('webDisplay.present: ' + e); }
});

EM_JS(void, js_display_set_title, (const char *title, int titleSize), {
	var display = Module['webDisplay'];
	try {
		if (display && display['setTitle'])
			display['setTitle'](UTF8ToString(Number(title), titleSize, true));
	} catch (e) { err('webDisplay.setTitle: ' + e); }
});

EM_JS(void, js_display_set_cursor, (const uint32_t *rgba, int width, int height, int hotX, int hotY), {
	var display = Module['webDisplay'];
	try {
		if (display && display['setCursor']) {
			var start = Number(rgba);
			display['setCursor'](new Uint8ClampedArray(HEAPU8.subarray(start, start + width * height * 4)),
				width, height, hotX, hotY);
		}
	} catch (e) { err('webDisplay.setCursor: ' + e); }
});

EM_JS(void, js_display_set_clipboard, (const char *text, int textSize), {
	var display = Module['webDisplay'];
	try {
		if (display && display['setClipboard'])
			display['setClipboard'](UTF8ToString(Number(text), textSize, true));
	} catch (e) { err('webDisplay.setClipboard: ' + e); }
});

EM_JS(void, js_display_focus, (), {
	var display = Module['webDisplay'];
	try {
		if (display && display['focus'])
			display['focus']();
	} catch (e) { err('webDisplay.focus: ' + e); }
});


/*	The module name is used to check, once a module is loaded, that it is the
	module that was asked for. */

static const char *
getModuleName(void)
{
	return moduleName;
}

/*	Fetch the x and y of the Point oop, which must be SmallIntegers between
	min and max; answer whether they are. */

static sqInt
fetchPoint(sqInt oop, sqInt min, sqInt max, int *x, int *y)
{
	sqInt px, py;

	if (!interpreterProxy->isKindOfClass(oop, interpreterProxy->classPoint())) {
		return false;
	}
	px = interpreterProxy->fetchIntegerofObject(0, oop);
	py = interpreterProxy->fetchIntegerofObject(1, oop);
	if (interpreterProxy->failed()
	 || px < min || px > max || py < min || py > max) {
		return false;
	}
	*x = (int)px;
	*y = (int)py;
	return true;
}

/*	Fetch the bytes and the size of the bytes object oop, whose size the
	page's side takes as an int; answer whether it is one of at most
	INT_MAX bytes. */

static sqInt
fetchBytes(sqInt oop, char **bytes, int *size)
{
	sqInt byteSize;

	if (!interpreterProxy->isBytes(oop)) {
		return false;
	}
	byteSize = interpreterProxy->byteSizeOf(oop);
	if (byteSize > INT_MAX) {
		return false;
	}
	*bytes = interpreterProxy->firstIndexableField(oop);
	*size = (int)byteSize;
	return true;
}


/*	Arguments: bits, width, height, depth, left, top, right, bottom.  Copy
	the rectangle left@top corner: right@bottom of the 32-bit Form of
	width x height pixels whose bits are bits into the frame.  The frame
	takes the extent of the Form, and the rectangle is clipped to it.
	Answer the receiver. */

static void
primitiveWebBlit(void)
{
	sqInt bits, width, height, depth, left, top, right, bottom;

	bits = interpreterProxy->stackValue(7);
	width = interpreterProxy->stackIntegerValue(6);
	height = interpreterProxy->stackIntegerValue(5);
	depth = interpreterProxy->stackIntegerValue(4);
	left = interpreterProxy->stackIntegerValue(3);
	top = interpreterProxy->stackIntegerValue(2);
	right = interpreterProxy->stackIntegerValue(1);
	bottom = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed()) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (depth != 32
	 || width < 1 || width > WEB_MAX_EXTENT || height < 1 || height > WEB_MAX_EXTENT
	 || !interpreterProxy->isWords(bits)
	 || interpreterProxy->byteSizeOf(bits) < width * height * 4) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (!webFrameBlit(&frame, interpreterProxy->firstIndexableField(bits), (int)width, (int)height,
			left, top, right, bottom)) {
		interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
		return;
	}
	interpreterProxy->pop(8);
}


/*	Answer the extent of the page's canvas as a Point, or nil before the
	page reported it. */

static void
primitiveWebCanvasExtent(void)
{
	sqInt extent;

	if (canvasWidth < 0) {
		interpreterProxy->popthenPush(1, interpreterProxy->nilObject());
		return;
	}
	extent = interpreterProxy->makePointwithxValueyValue(canvasWidth, canvasHeight);
	if (!extent) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	interpreterProxy->popthenPush(1, extent);
}


/*	Answer the clipboard's text as a UTF-8 ByteArray, or nil when there is
	none yet.  Reading it leaves it there. */

static void
primitiveWebClipboardText(void)
{
	sqInt text;

	if (clipboard.size < 0) {
		interpreterProxy->popthenPush(1, interpreterProxy->nilObject());
		return;
	}
	text = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), clipboard.size);
	if (!text) {
		interpreterProxy->primitiveFailFor(PrimErrNoMemory);
		return;
	}
	memcpy(interpreterProxy->firstIndexableField(text), clipboard.bytes, clipboard.size);
	interpreterProxy->popthenPush(1, text);
}


/*	Answer whether the page provides a display. */

static void
primitiveWebDisplayIsAvailable(void)
{
	interpreterProxy->popthenPush(1, js_display_available()
		? interpreterProxy->trueObject()
		: interpreterProxy->falseObject());
}


/*	Ask the page to move the keyboard focus to the canvas.  Answer the
	receiver. */

static void
primitiveWebFocus(void)
{
	js_display_focus();
}


/*	Argument: an IntegerArray of at least 8 elements.  Move the oldest event
	record of the page into it and answer true, or answer false when there
	is none. */

static void
primitiveWebNextEvent(void)
{
	sqInt record;

	record = interpreterProxy->stackValue(0);
	if (!interpreterProxy->isWords(record)
	 || interpreterProxy->stSizeOf(record) < WEB_EVENT_SIZE) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (interpreterProxy->isOopImmutable(record)) {
		interpreterProxy->primitiveFailFor(PrimErrNoModification);
		return;
	}
	interpreterProxy->popthenPush(2, webEventPop(&events, interpreterProxy->firstIndexableField(record))
		? interpreterProxy->trueObject()
		: interpreterProxy->falseObject());
}


/*	Arguments: the extent (a Point) and the title (UTF-8 bytes) of the
	world's window.  Tell the page that it opens.  Answer the receiver. */

static void
primitiveWebOpenCanvas(void)
{
	sqInt extent;
	int width, height, titleSize;
	char *title;

	extent = interpreterProxy->stackValue(1);
	if (!fetchPoint(extent, 0, WEB_MAX_EXTENT, &width, &height)
	 || !fetchBytes(interpreterProxy->stackValue(0), &title, &titleSize)) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	js_display_open(width, height, title, titleSize);
	interpreterProxy->pop(2);
}


/*	Send the rectangles blitted since the last present to the page, and end
	the slice at the next event check, so that the canvas shows them.
	Answer the receiver. */

static void
primitiveWebPresent(void)
{
	if (!frame.pixels || !frame.dirtyCount) {
		return;
	}
	js_display_present(frame.pixels, frame.width, frame.height, &frame.dirty[0][0], frame.dirtyCount);
	webFrameClearDirty(&frame);
	emscriptenRequestYield();
}


/*	Argument: UTF-8 bytes.  They are the clipboard's text now, and the page
	copies them to the system clipboard (which it may not be allowed to do).
	Answer the receiver. */

static void
primitiveWebSetClipboardText(void)
{
	char *text;
	int textSize;

	if (!fetchBytes(interpreterProxy->stackValue(0), &text, &textSize)) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (!webClipboardSet(&clipboard, text, textSize)) {
		interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
		return;
	}
	js_display_set_clipboard(clipboard.bytes, clipboard.size);
	interpreterProxy->pop(1);
}


/*	Arguments: bits and maskBits, the words of two 1-bit Forms of the same
	extent (a Point), and offset (a Point), the Form offset: the hot spot
	negated.  Give the canvas that cursor.  Answer the receiver. */

static void
primitiveWebSetCursor(void)
{
	sqInt bits, mask, extent, offset, bytes;
	int width, height, offsetX, offsetY, hotX, hotY;
	uint32_t *rgba;

	bits = interpreterProxy->stackValue(3);
	mask = interpreterProxy->stackValue(2);
	extent = interpreterProxy->stackValue(1);
	offset = interpreterProxy->stackValue(0);
	if (!fetchPoint(extent, 1, WEB_MAX_CURSOR, &width, &height)
	 || !fetchPoint(offset, -WEB_MAX_EXTENT, WEB_MAX_EXTENT, &offsetX, &offsetY)) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	bytes = (width + 31) / 32 * 4 * height;
	if (!interpreterProxy->isWords(bits) || interpreterProxy->byteSizeOf(bits) < bytes
	 || !interpreterProxy->isWords(mask) || interpreterProxy->byteSizeOf(mask) < bytes) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	if (!(rgba = malloc((size_t)width * height * sizeof(uint32_t)))) {
		interpreterProxy->primitiveFailFor(PrimErrNoCMemory);
		return;
	}
	webCursorToRGBA(interpreterProxy->firstIndexableField(bits),
		interpreterProxy->firstIndexableField(mask), width, height, rgba);
	/* CSS ignores a hot spot outside the image */
	hotX = -offsetX < 0 ? 0 : -offsetX >= width ? width - 1 : -offsetX;
	hotY = -offsetY < 0 ? 0 : -offsetY >= height ? height - 1 : -offsetY;
	js_display_set_cursor(rgba, width, height, hotX, hotY);
	free(rgba);
	interpreterProxy->pop(4);
}


/*	Remember the index of the external semaphore that webdisplay_push_event()
	signals; 0 forgets it.  signalSemaphoreWithIndex() narrows the index to
	an int, so a larger one would signal another semaphore.  Answer the
	receiver. */

static void
primitiveWebSetInputSemaphore(void)
{
	sqInt semaphoreIndex;

	semaphoreIndex = interpreterProxy->stackIntegerValue(0);
	if (interpreterProxy->failed()
	 || semaphoreIndex < 0 || semaphoreIndex > INT_MAX) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	inputSemaphoreIndex = semaphoreIndex;
	interpreterProxy->pop(1);
}


/*	Argument: UTF-8 bytes, the title of the world's window.  Answer the
	receiver. */

static void
primitiveWebSetTitle(void)
{
	char *title;
	int titleSize;

	if (!fetchBytes(interpreterProxy->stackValue(0), &title, &titleSize)) {
		interpreterProxy->primitiveFailFor(PrimErrBadArgument);
		return;
	}
	js_display_set_title(title, titleSize);
	interpreterProxy->pop(1);
}

static sqInt
setInterpreter(struct VirtualMachine *anInterpreter)
{
	interpreterProxy = anInterpreter;
	return ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR))
	 && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));
}


/*** The page's side ***
 *
 * Called by the host between slices.  They take and answer only int and
 * double, so that no pointer crosses to JavaScript as a BigInt in wasm64.
 */

/*	Queue an event record of the page (see OSWebDriver for its layout) and
	signal the image's input semaphore.  Answer false when the ring is full:
	the page keeps the event and pushes it again after the next slice.  The
	image may not have loaded the plugin yet, nor set its semaphore: the
	records then wait for it. */

EMSCRIPTEN_KEEPALIVE int
webdisplay_push_event(int type, int timestamp, int x, int y, int a, int modifiers, int c, int d)
{
	int32_t record[WEB_EVENT_SIZE] = { type, timestamp, x, y, a, modifiers, c, d };

	if (!webEventPush(&events, record)) {
		return false;
	}
	if (interpreterProxy && inputSemaphoreIndex) {
		interpreterProxy->signalSemaphoreWithIndex(inputSemaphoreIndex);
	}
	return true;
}

/*	The page's canvas has width x height pixels now. */

EMSCRIPTEN_KEEPALIVE void
webdisplay_set_extent(int width, int height)
{
	canvasWidth = width < 0 ? 0 : width > WEB_MAX_EXTENT ? WEB_MAX_EXTENT : width;
	canvasHeight = height < 0 ? 0 : height > WEB_MAX_EXTENT ? WEB_MAX_EXTENT : height;
}

/*	Answer the address of a buffer for size bytes of pasted UTF-8 text, or 0.
	The page writes the text there, then commits it with
	webdisplay_clipboard_commit(), before it pushes the keys that paste. */

EMSCRIPTEN_KEEPALIVE double
webdisplay_clipboard_buffer(int size)
{
	return (double)(uintptr_t)webClipboardBuffer(&clipboard, size);
}

/*	The size bytes the page wrote into the clipboard's buffer are the
	clipboard's text now. */

EMSCRIPTEN_KEEPALIVE void
webdisplay_clipboard_commit(int size)
{
	webClipboardCommit(&clipboard, size);
}


static char _m[] = "WebDisplayPlugin";
void* WebDisplayPlugin_exports[][3] = {
	{(void*)_m, "getModuleName", (void*)getModuleName},
	{(void*)_m, "primitiveWebBlit\000\000", (void*)primitiveWebBlit},
	{(void*)_m, "primitiveWebCanvasExtent\000\377", (void*)primitiveWebCanvasExtent},
	{(void*)_m, "primitiveWebClipboardText\000\377", (void*)primitiveWebClipboardText},
	{(void*)_m, "primitiveWebDisplayIsAvailable\000\377", (void*)primitiveWebDisplayIsAvailable},
	{(void*)_m, "primitiveWebFocus\000\377", (void*)primitiveWebFocus},
	{(void*)_m, "primitiveWebNextEvent\000\000", (void*)primitiveWebNextEvent},
	{(void*)_m, "primitiveWebOpenCanvas\000\000", (void*)primitiveWebOpenCanvas},
	{(void*)_m, "primitiveWebPresent\000\377", (void*)primitiveWebPresent},
	{(void*)_m, "primitiveWebSetClipboardText\000\000", (void*)primitiveWebSetClipboardText},
	{(void*)_m, "primitiveWebSetCursor\000\000", (void*)primitiveWebSetCursor},
	{(void*)_m, "primitiveWebSetInputSemaphore\000\377", (void*)primitiveWebSetInputSemaphore},
	{(void*)_m, "primitiveWebSetTitle\000\000", (void*)primitiveWebSetTitle},
	{(void*)_m, "setInterpreter", (void*)setInterpreter},
	{NULL, NULL, NULL}
};

#endif /* !WEBDISPLAY_UNIT_TEST */
