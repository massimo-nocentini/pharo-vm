/* text.c -- the text primitive of the CairoPlugin (group #text,
 * docs/CairoPlugin.md section "Text primitive", and Amendment A1: the four
 * string classes, UTF-8 by the SimdUTF engine, su_encode of suEngine.h, which
 * CairoPlugin.c includes): the whole drawString: of a
 * CairoFreetypeFontRenderer in one primitive.
 *
 * The stock path (Athens-Cairo, identical source in Pharo 12-15):
 * renderCharacters:from:to: = glyphsOf:from:to:, renderGlyphs:, ^ advance,
 * where
 *   glyphsOf: = the UTF-8 of the characters (CairoUTF8Converter),
 *     canvas setScaledFont: font; setFontSize: originalFont pixelSize,
 *     text_to_glyphs(canvas getScaledFont, 0.0, 0.0, utf8, utf8Len, &ptr
 *     (NULL), &n (the character count), NULL, NULL, NULL), on failure an
 *     empty CairoGlyphsArray (advance unchanged), else font
 *     getExtentsOfGlyphs: ptr ofLength: n into: glyphExtents, advance :=
 *     glyphExtents width @ glyphExtents height, a copy of the glyphs (LibC
 *     memCopy, not cairo), kerning when the face has it, freeGlyphs: ptr;
 *   renderGlyphs: = font lock, canvas setPathMatrix; setScaledFont: font,
 *     canvas paint loadOnCairoCanvas: canvas (a Color: set_source_rgba),
 *     canvas showGlyphs: glyphs size: n, font unlock.
 * The primitive makes the same 12 cairo calls (10 when text_to_glyphs
 * fails), with one change of order: cairo's glyph array is shown directly
 * and freed after show_glyphs instead of being copied and freed before the
 * lock; glyph_free touches no cairo_t state, so pixels and state are the
 * stock ones.  The patched renderCharacters:from:to: calls it only when the
 * face has no kerning and the canvas paint is a Color, and sets advance from
 * glyphExtents when it answers true.
 *
 * The string is an instance of exactly one of the four string classes the
 * image passes at bind (layout positions 30-33: ByteString, ByteSymbol:
 * Latin-1 bytes; WideString, WideSymbol: UTF-32 words), encoded to UTF-8 by
 * su_encode into a 1024-byte stack buffer, or a malloc of the exact size.  A
 * character simdutf refuses (a surrogate, a value above U+10FFFF: the
 * leading-char bits of a Pharo WideString) fails the primitive before any
 * cairo call, so that the stock converter runs and the image sees the stock
 * behaviour.
 *
 * Owner: WP-C.
 *
 * Not a translation unit: src/common/CairoPlugin.c #includes it (see
 * basic.c).  The UTF-8 buffer is the one malloc of this file (R1). */

/* The UTF-8 that fits on the stack; more is malloc'd */
#define CP_C_UTF8_ON_STACK 1024

/* The destination of su_encode in allocate mode: the stack buffer, or a
   malloc of the exact size; at = where the bytes went */
typedef struct {
	char *at;
	char *heap;
	int tooLong;		/* more than INT_MAX bytes (cairo takes an int) */
	int noMemory;		/* the malloc failed */
	char stack[CP_C_UTF8_ON_STACK];
} cp_c_utf8;

/* The allocate callback of the su_sink: called at most once, after the
   validation (su_encode answers SU_RC_NO_ROOM when it answers NULL) */
static void *
cp_c_utf8_allocate(void *ctx, int kind, size_t units)
{
	cp_c_utf8 *u = (cp_c_utf8 *)ctx;

	(void)kind;	/* SU_KIND_BYTES */
	if (units > (size_t)INT_MAX) {
		u->tooLong = 1;
		return NULL;
	}
	if (units <= sizeof u->stack) {
		return u->at = u->stack;
	}
	if (!(u->heap = (char *)malloc(units))) {
		u->noMemory = 1;
		return NULL;
	}
	return u->at = u->heap;
}

static void
cp_c_utf8_free(cp_c_utf8 *u)
{
	free(u->heap);
	u->heap = NULL;
	u->at = NULL;
}

/* The UTF-8 of the units start0 .. stop0 - 1 of a string (base, units,
   words as cp_string answers them) into u: *length bytes from u->at.
   Answers 0, or the failure code: PrimErrBadArgument (a character simdutf
   refuses, more than INT_MAX bytes, a request outside su_encode's
   contract), PrimErrNoCMemory (the malloc failed).  The caller frees with
   cp_c_utf8_free, also after a failure. */
static sqInt
cp_c_encode(const void *base, size_t units, int words, size_t start0, size_t stop0,
	cp_c_utf8 *u, int *length)
{
	su_input in;
	su_sink sink;
	su_outcome out;
	int rc;

	u->at = u->heap = NULL;
	u->tooLong = u->noMemory = 0;
	in.base = base;
	in.units = units;
	in.words = words;
	in.start0 = start0;
	in.stop0 = stop0;
	sink.base = NULL;		/* allocate mode: one pass, the exact size */
	sink.capacity = 0;
	sink.kind = SU_KIND_BYTES;
	sink.ctx = u;
	sink.allocate = cp_c_utf8_allocate;
	rc = su_encode(&in, SU_ENC_UTF8, &sink, &out);
	if (rc == SU_RC_DONE && out.status == SU_OK && u->at && out.units <= (size_t)INT_MAX) {
		*length = (int)out.units;
		return 0;
	}
	return rc == SU_RC_NO_ROOM && u->noMemory ? PrimErrNoCMemory : PrimErrBadArgument;
}


/*** primitiveShowString ***/

/*	CairoFreetypeFontRenderer>>cairoPluginShowString: aString from: start to: end pixelSize: pixelSize colorClass: aClass
	<primitive: 'primitiveShowString' module: 'CairoPlugin'>
	^ nil
	The drawString: of the characters start to end of aString (a
	ByteString, ByteSymbol, WideString or WideSymbol) with the receiver's
	font on its canvas, whose paint must be exactly of class aClass (Color).
	Answer true when cairo converted the text to glyphs (glyphExtents then
	holds their extents: the caller sets advance), false otherwise (nothing
	drawn, as stock). */
EXPORT(sqInt)
primitiveShowString(void)
{
	sqInt rcvr, fontObj, canvas, transform, paint, extObj, extHandle;
	sqInt string, startOop, endOop, start, end;
	void *font, *ext;
	cp_cairo_t *cr;
	cp_matrix_t *matrix;
	cp_scaled_font_t *sf, *scaled;
	cp_glyph_t *glyphs;
	cp_status_t status;
	const void *base;
	size_t units;
	int words, utf8Length, n;
	double rgba[4], pixelSize;
	cp_c_utf8 utf8;
	sqInt code;

	CP_ARGS(5);
	CP_BOUND();
	if (!CP_HAVE(ft_scaled_font_lock_face) || !CP_HAVE(ft_scaled_font_unlock_face)) {
		return cp_fail(PrimErrUnsupported);
	}

	/* the receiver: font (a CairoScaledFont), canvas (its cr, its
	   pathTransform, its paint of class aClass), glyphExtents (an
	   ExternalAddress, tested first, or 48 writable bytes) */
	rcvr = CP_RECEIVER(5);
	if (!cp_slot(rcvr, CP_L_RENDERER_FONT, &fontObj) || !cp_handle(fontObj, CP_L_EXTOBJ_HANDLE, &font)
	 || !cp_slot(rcvr, CP_L_RENDERER_CANVAS, &canvas) || !cp_canvas_cr(canvas, &cr)
	 || !cp_slot(canvas, CP_L_CANVAS_PATH_TRANSFORM, &transform) || !cp_matrix(transform, 0, &matrix)
	 || !cp_slot(canvas, CP_L_CANVAS_PAINT, &paint) || interpreterProxy->isImmediate(paint)
	 || interpreterProxy->fetchClassOf(paint) != CP_ARG(5, 4) || !cp_color(paint, rgba)
	 || !cp_slot(rcvr, CP_L_RENDERER_GLYPH_EXTENTS, &extObj) || !cp_slot(extObj, CP_L_EXTOBJ_HANDLE, &extHandle)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!interpreterProxy->isImmediate(extHandle)
	 && interpreterProxy->fetchClassOf(extHandle) == interpreterProxy->classExternalAddress()
		? !cp_address(extHandle, &ext)
		: !cp_bytes(extHandle, sizeof(cp_text_extents_t), 1, &ext, NULL)) {
		return cp_fail(PrimErrBadReceiver);
	}
	sf = (cp_scaled_font_t *)font;

	/* the arguments: the string, 1 <= start <= end + 1, end <= its size, the
	   pixel size */
	string = CP_ARG(5, 0);
	startOop = CP_ARG(5, 1);
	endOop = CP_ARG(5, 2);
	if (!cp_string(string, &base, &units, &words)
	 || !interpreterProxy->isIntegerObject(startOop) || !interpreterProxy->isIntegerObject(endOop)) {
		return cp_fail(PrimErrBadArgument);
	}
	start = interpreterProxy->integerValueOf(startOop);
	end = interpreterProxy->integerValueOf(endOop);
	if (start < 1 || end < start - 1 || end < 0 || (size_t)end > units
	 || end - start + 1 > (sqInt)INT_MAX || !cp_number(CP_ARG(5, 3), &pixelSize)) {
		return cp_fail(PrimErrBadArgument);
	}

	/* the UTF-8, before any cairo call */
	if ((code = cp_c_encode(base, units, words, (size_t)(start - 1), (size_t)end, &utf8, &utf8Length))) {
		cp_c_utf8_free(&utf8);
		return cp_fail(code);
	}

	/* glyphsOf:from:to: */
	CP_CALL(set_scaled_font)(cr, sf);
	CP_CALL(set_font_size)(cr, pixelSize);
	scaled = CP_CALL(get_scaled_font)(cr);
	glyphs = NULL;
	n = (int)(end - start + 1);
	status = CP_CALL(scaled_font_text_to_glyphs)(scaled, 0.0, 0.0, utf8.at, utf8Length,
		&glyphs, &n, NULL, NULL, NULL);
	if (status == CP_STATUS_SUCCESS) {
		/* n may be 0 with glyphs NULL: cairo answers before reading them */
		CP_CALL(scaled_font_glyph_extents)(sf, glyphs, n, (cp_text_extents_t *)ext);
	} else {
		glyphs = NULL;	/* stock shows an empty CairoGlyphsArray */
		n = 0;
	}
	/* renderGlyphs: (show_glyphs of 0 glyphs answers before reading them) */
	CP_CALL(ft_scaled_font_lock_face)(sf);
	CP_CALL(set_matrix)(cr, matrix);
	CP_CALL(set_scaled_font)(cr, sf);
	CP_CALL(set_source_rgba)(cr, rgba[0], rgba[1], rgba[2], rgba[3]);
	CP_CALL(show_glyphs)(cr, glyphs, n);
	CP_CALL(ft_scaled_font_unlock_face)(sf);
	/* stock's freeGlyphs:, after the show instead of before the lock */
	if (status == CP_STATUS_SUCCESS) {
		CP_CALL(glyph_free)(glyphs);
	}
	cp_c_utf8_free(&utf8);
	CP_PRIM_DONE(ShowString);
	return cp_return(status == CP_STATUS_SUCCESS
		? interpreterProxy->trueObject() : interpreterProxy->falseObject());
}
