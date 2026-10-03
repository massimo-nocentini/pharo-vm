/* webdisplay-unit.c -- unit tests of the frame code of WebDisplayPlugin
 *
 * Includes the plain C part of src/emscripten/plugins/WebDisplayPlugin.c
 * (WEBDISPLAY_UNIT_TEST leaves the plugin out), and checks it:
 *
 *  - Form pixels become opaque RGBA (alpha 255) on a known pattern;
 *  - a blit copies its rectangle only, clipped to the Form;
 *  - 64 dirty rectangles stay apart, the 65th merges them into their
 *    bounding box, which later ones grow until the frame is presented;
 *  - a new extent reallocates the frame and marks it all dirty;
 *  - 1-bit cursors become RGBA (black, white, transparent);
 *  - the event ring and the clipboard store.
 *
 * tests/wasm/lanes/55-webdisplay-unit.sh builds it with emcc -m64 and runs
 * it in node.  Prints one line per failed check and a summary; exits 1 when
 * a check failed.
 */

#include <stdio.h>

#define WEBDISPLAY_UNIT_TEST 1
#include "../../src/emscripten/plugins/WebDisplayPlugin.c"

static int checks, failures;

#define CHECK(condition) check((condition), #condition, __LINE__)

static void
check(int ok, const char *what, int line)
{
	checks += 1;
	if (!ok) {
		failures += 1;
		printf("FAIL webdisplay-unit.c:%d: %s\n", line, what);
	}
}

/* The RGBA bytes of pixel x, y of the frame. */
static const uint8_t *
framePixel(WebFrame *frame, int x, int y)
{
	return (const uint8_t *)(frame->pixels + (size_t)y * frame->width + x);
}

static int
pixelIs(WebFrame *frame, int x, int y, int r, int g, int b, int a)
{
	const uint8_t *p = framePixel(frame, x, y);

	return p[0] == r && p[1] == g && p[2] == b && p[3] == a;
}

static int
rectIs(const int *rect, int x, int y, int width, int height)
{
	return rect[0] == x && rect[1] == y && rect[2] == width && rect[3] == height;
}

static void
freeFrame(WebFrame *frame)
{
	free(frame->pixels);
	memset(frame, 0, sizeof(*frame));
}

static void
testPixels(void)
{
	WebFrame frame = { 0 };
	/* 0xAARRGGBB, alpha opaque, transparent and in between */
	uint32_t bits[4 * 2] = {
		0xFF112233, 0x00445566, 0x80FF0000, 0x0000FF00,
		0x000000FF, 0xFFFFFFFF, 0x00000000, 0x7F010203 };

	CHECK(webPixelFromFormPixel(0x00ABCDEF) == 0xFFEFCDAB);
	CHECK(webFrameBlit(&frame, bits, 4, 2, 0, 0, 4, 2));
	CHECK(frame.width == 4 && frame.height == 2);
	CHECK(pixelIs(&frame, 0, 0, 0x11, 0x22, 0x33, 255));
	CHECK(pixelIs(&frame, 1, 0, 0x44, 0x55, 0x66, 255));
	CHECK(pixelIs(&frame, 2, 0, 255, 0, 0, 255));
	CHECK(pixelIs(&frame, 3, 0, 0, 255, 0, 255));
	CHECK(pixelIs(&frame, 0, 1, 0, 0, 255, 255));
	CHECK(pixelIs(&frame, 1, 1, 255, 255, 255, 255));
	CHECK(pixelIs(&frame, 2, 1, 0, 0, 0, 255));
	CHECK(pixelIs(&frame, 3, 1, 1, 2, 3, 255));
	freeFrame(&frame);
}

static void
testClipping(void)
{
	WebFrame frame = { 0 };
	uint32_t bits[4 * 3];

	for (int i = 0; i < 4 * 3; i++)
		bits[i] = 0x00FF0000;
	CHECK(webFrameBlit(&frame, bits, 4, 3, 1, 1, 3, 2));
	webFrameClearDirty(&frame);
	/* only the rectangle was copied: the rest is still opaque black */
	CHECK(pixelIs(&frame, 1, 1, 255, 0, 0, 255) && pixelIs(&frame, 2, 1, 255, 0, 0, 255));
	CHECK(pixelIs(&frame, 0, 1, 0, 0, 0, 255) && pixelIs(&frame, 3, 1, 0, 0, 0, 255));
	CHECK(pixelIs(&frame, 1, 0, 0, 0, 0, 255) && pixelIs(&frame, 1, 2, 0, 0, 0, 255));

	for (int i = 0; i < 4 * 3; i++)
		bits[i] = 0x000000FF;
	/* beyond every side of the Form */
	CHECK(webFrameBlit(&frame, bits, 4, 3, -5, -7, 2, 100));
	CHECK(frame.dirtyCount == 1 && rectIs(frame.dirty[0], 0, 0, 2, 3));
	CHECK(pixelIs(&frame, 0, 0, 0, 0, 255, 255) && pixelIs(&frame, 1, 2, 0, 0, 255, 255));
	CHECK(pixelIs(&frame, 2, 1, 255, 0, 0, 255) && pixelIs(&frame, 3, 2, 0, 0, 0, 255));
	CHECK(webFrameBlit(&frame, bits, 4, 3, 3, 2, 1000000, 1000000));
	CHECK(frame.dirtyCount == 2 && rectIs(frame.dirty[1], 3, 2, 1, 1));
	CHECK(pixelIs(&frame, 3, 2, 0, 0, 255, 255));

	/* outside the Form, or empty: nothing is copied or dirty */
	webFrameClearDirty(&frame);
	for (int i = 0; i < 4 * 3; i++)
		bits[i] = 0x0000FF00;
	CHECK(webFrameBlit(&frame, bits, 4, 3, 4, 0, 8, 3));
	CHECK(webFrameBlit(&frame, bits, 4, 3, 0, -3, 4, 0));
	CHECK(webFrameBlit(&frame, bits, 4, 3, 3, 1, 1, 2));
	CHECK(webFrameBlit(&frame, bits, 4, 3, 2, 2, 2, 3));
	CHECK(frame.dirtyCount == 0);
	for (int y = 0; y < 3; y++)
		for (int x = 0; x < 4; x++)
			CHECK(!pixelIs(&frame, x, y, 0, 255, 0, 255));
	freeFrame(&frame);
}

static void
testDirtyMerge(void)
{
	WebFrame frame = { 0 };
	uint32_t bits[100 * 80] = { 0 };

	CHECK(webFrameSetExtent(&frame, 100, 80));
	webFrameClearDirty(&frame);
	/* 64 one-pixel rectangles on the diagonal from 10@5 stay apart */
	for (int i = 0; i < WEB_MAX_DIRTY; i++)
		CHECK(webFrameBlit(&frame, bits, 100, 80, 10 + i, 5 + i, 11 + i, 6 + i));
	CHECK(frame.dirtyCount == 64 && !frame.dirtyMerged);
	CHECK(rectIs(frame.dirty[0], 10, 5, 1, 1) && rectIs(frame.dirty[63], 73, 68, 1, 1));
	/* the 65th, inside them, merges all 65 into their bounding box */
	CHECK(webFrameBlit(&frame, bits, 100, 80, 20, 20, 22, 22));
	CHECK(frame.dirtyCount == 1 && frame.dirtyMerged);
	CHECK(rectIs(frame.dirty[0], 10, 5, 64, 64));
	/* a later one grows it, also one before it */
	CHECK(webFrameBlit(&frame, bits, 100, 80, 2, 70, 90, 75));
	CHECK(frame.dirtyCount == 1 && rectIs(frame.dirty[0], 2, 5, 88, 70));
	CHECK(webFrameBlit(&frame, bits, 100, 80, 0, 0, 1, 1));
	CHECK(frame.dirtyCount == 1 && rectIs(frame.dirty[0], 0, 0, 90, 75));
	/* presenting starts a new list */
	webFrameClearDirty(&frame);
	CHECK(frame.dirtyCount == 0 && !frame.dirtyMerged);
	CHECK(webFrameBlit(&frame, bits, 100, 80, 50, 50, 60, 55));
	CHECK(webFrameBlit(&frame, bits, 100, 80, 1, 2, 3, 4));
	CHECK(frame.dirtyCount == 2 && rectIs(frame.dirty[0], 50, 50, 10, 5) && rectIs(frame.dirty[1], 1, 2, 2, 2));
	freeFrame(&frame);
}

static void
testResize(void)
{
	WebFrame frame = { 0 };
	uint32_t small[3 * 2], large[5 * 4];
	uint32_t *pixels;

	for (int i = 0; i < 3 * 2; i++)
		small[i] = 0x00FF0000;
	for (int i = 0; i < 5 * 4; i++)
		large[i] = 0x0000FF00;
	/* the first blit allocates the frame, all dirty */
	CHECK(webFrameBlit(&frame, small, 3, 2, 1, 0, 2, 1));
	CHECK(frame.pixels && frame.width == 3 && frame.height == 2);
	CHECK(frame.dirtyCount == 1 && frame.dirtyMerged && rectIs(frame.dirty[0], 0, 0, 3, 2));
	CHECK(pixelIs(&frame, 0, 0, 0, 0, 0, 255) && pixelIs(&frame, 1, 0, 255, 0, 0, 255));
	CHECK(webFrameBlit(&frame, small, 3, 2, 0, 0, 3, 2));
	webFrameClearDirty(&frame);

	/* the same extent keeps the frame, and the dirty list */
	pixels = frame.pixels;
	CHECK(webFrameSetExtent(&frame, 3, 2));
	CHECK(frame.pixels == pixels && frame.dirtyCount == 0);
	CHECK(webFrameBlit(&frame, small, 3, 2, 0, 0, 1, 1));
	CHECK(frame.pixels == pixels && frame.dirtyCount == 1 && !frame.dirtyMerged);

	/* a larger Form: a new frame, all dirty, which a small blit cannot shrink */
	CHECK(webFrameBlit(&frame, large, 5, 4, 4, 3, 5, 4));
	CHECK(frame.width == 5 && frame.height == 4);
	CHECK(frame.dirtyCount == 1 && frame.dirtyMerged && rectIs(frame.dirty[0], 0, 0, 5, 4));
	/* it keeps the old pixels, and the rest is opaque black */
	CHECK(pixelIs(&frame, 0, 0, 255, 0, 0, 255) && pixelIs(&frame, 2, 1, 255, 0, 0, 255));
	CHECK(pixelIs(&frame, 3, 0, 0, 0, 0, 255) && pixelIs(&frame, 0, 2, 0, 0, 0, 255));
	CHECK(pixelIs(&frame, 4, 3, 0, 255, 0, 255));
	webFrameClearDirty(&frame);

	/* a smaller one */
	CHECK(webFrameBlit(&frame, small, 2, 1, 0, 0, 0, 0));
	CHECK(frame.width == 2 && frame.height == 1);
	CHECK(frame.dirtyCount == 1 && rectIs(frame.dirty[0], 0, 0, 2, 1));
	CHECK(pixelIs(&frame, 0, 0, 255, 0, 0, 255) && pixelIs(&frame, 1, 0, 255, 0, 0, 255));
	freeFrame(&frame);
}

static void
testCursor(void)
{
	/* 40 x 2 pixels: rows of 2 words.  Row 0 pairs bits and mask:
	   pixel 0 both, 1 mask only, 2 bits only, 3 neither, and in the second
	   word pixel 32 both, 33 mask only, 39 both.  Row 1: mask only, all. */
	uint32_t bits[2 * 2] = { 0xA0000000, 0x81000000, 0x00000000, 0x00000000 };
	uint32_t mask[2 * 2] = { 0xC0000000, 0xC1000000, 0xFFFFFFFF, 0xFF000000 };
	uint32_t rgba[40 * 2];
	const uint8_t *p = (const uint8_t *)rgba;

	webCursorToRGBA(bits, mask, 40, 2, rgba);
	CHECK(p[0] == 0 && p[1] == 0 && p[2] == 0 && p[3] == 255);
	CHECK(p[4] == 255 && p[5] == 255 && p[6] == 255 && p[7] == 255);
	CHECK(rgba[0] == WEB_OPAQUE_BLACK && rgba[1] == WEB_OPAQUE_WHITE);
	CHECK(rgba[2] == WEB_TRANSPARENT && rgba[3] == WEB_TRANSPARENT);
	for (int x = 4; x < 32; x++)
		CHECK(rgba[x] == WEB_TRANSPARENT);
	CHECK(rgba[32] == WEB_OPAQUE_BLACK && rgba[33] == WEB_OPAQUE_WHITE);
	CHECK(rgba[38] == WEB_TRANSPARENT && rgba[39] == WEB_OPAQUE_BLACK);
	for (int x = 0; x < 40; x++)
		CHECK(rgba[40 + x] == WEB_OPAQUE_WHITE);

	/* 16 x 16, one word a row, as Cursor normal: the mask's frame of a black pixel */
	{
		uint32_t bits16[16] = { 0 }, mask16[16] = { 0 }, rgba16[16 * 16];

		bits16[5] = 0x00100000;	/* pixel 11 */
		mask16[5] = 0x00380000;	/* pixels 10 to 12 */
		webCursorToRGBA(bits16, mask16, 16, 16, rgba16);
		CHECK(rgba16[5 * 16 + 11] == WEB_OPAQUE_BLACK);
		CHECK(rgba16[5 * 16 + 10] == WEB_OPAQUE_WHITE && rgba16[5 * 16 + 12] == WEB_OPAQUE_WHITE);
		CHECK(rgba16[5 * 16 + 9] == WEB_TRANSPARENT && rgba16[5 * 16 + 13] == WEB_TRANSPARENT);
		CHECK(rgba16[4 * 16 + 11] == WEB_TRANSPARENT && rgba16[6 * 16 + 11] == WEB_TRANSPARENT);
	}
}

static void
testEventRing(void)
{
	static WebEventRing ring;
	int32_t record[WEB_EVENT_SIZE], out[WEB_EVENT_SIZE];
	int ok = 1;

	CHECK(!webEventPop(&ring, out));
	for (int i = 0; i < WEB_EVENT_RING; i++) {
		for (int j = 0; j < WEB_EVENT_SIZE; j++)
			record[j] = i * 10 + j - 5;
		ok = ok && webEventPush(&ring, record);
	}
	CHECK(ok);
	CHECK(!webEventPush(&ring, record));
	/* oldest first, with negative values */
	for (int i = 0; i < 100; i++) {
		ok = webEventPop(&ring, out);
		for (int j = 0; j < WEB_EVENT_SIZE; j++)
			ok = ok && out[j] == i * 10 + j - 5;
		if (!ok)
			break;
	}
	CHECK(ok);
	/* round the end of the ring */
	for (int i = 0; i < 100; i++) {
		for (int j = 0; j < WEB_EVENT_SIZE; j++)
			record[j] = -(i + 1) * 100 - j;
		ok = ok && webEventPush(&ring, record);
	}
	CHECK(ok && !webEventPush(&ring, record));
	for (int i = 100; i < WEB_EVENT_RING; i++)
		ok = ok && webEventPop(&ring, out) && out[0] == i * 10 - 5 && out[7] == i * 10 + 2;
	CHECK(ok);
	for (int i = 0; i < 100; i++)
		ok = ok && webEventPop(&ring, out) && out[0] == -(i + 1) * 100 && out[7] == -(i + 1) * 100 - 7;
	CHECK(ok);
	CHECK(!webEventPop(&ring, out) && ring.count == 0);
}

static void
testClipboard(void)
{
	WebClipboard clipboard = { NULL, -1, 0 };
	char *buffer;

	/* no text until the page pastes or the image copies */
	CHECK(clipboard.size == -1);
	CHECK(webClipboardBuffer(&clipboard, -1) == NULL);
	webClipboardCommit(&clipboard, 0);
	CHECK(clipboard.size == -1);

	/* a paste: the page writes into the buffer, then commits */
	CHECK((buffer = webClipboardBuffer(&clipboard, 5)) != NULL);
	memcpy(buffer, "hello", 5);
	CHECK(clipboard.size == -1);
	webClipboardCommit(&clipboard, 5);
	CHECK(clipboard.size == 5 && !memcmp(clipboard.bytes, "hello", 5));
	/* a commit larger than the buffer is ignored */
	webClipboardCommit(&clipboard, 6);
	CHECK(clipboard.size == 5);

	/* a copy in the image replaces it, in the same store */
	CHECK(webClipboardSet(&clipboard, "copied \xc3\xa8 text", 14));
	CHECK(clipboard.size == 14 && !memcmp(clipboard.bytes, "copied \xc3\xa8 text", 14));
	/* and a later paste replaces the copy, even with fewer bytes */
	CHECK((buffer = webClipboardBuffer(&clipboard, 2)) != NULL);
	memcpy(buffer, "ok", 2);
	webClipboardCommit(&clipboard, 2);
	CHECK(clipboard.size == 2 && !memcmp(clipboard.bytes, "ok", 2));
	/* an empty text is a text */
	CHECK(webClipboardSet(&clipboard, "", 0));
	CHECK(clipboard.size == 0 && clipboard.bytes);
	free(clipboard.bytes);
}

int
main(void)
{
	testPixels();
	testClipping();
	testDirtyMerge();
	testResize();
	testCursor();
	testEventRing();
	testClipboard();
	printf("webdisplay-unit: %d checks, %d failed\n", checks, failures);
	return failures ? 1 : 0;
}
