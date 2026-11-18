/* basic.c -- the one-for-one primitives of the CairoPlugin (group #basic,
 * docs/CairoPlugin.md section "One-for-one primitives"): each is the
 * patched Athens method, its receiver the Athens object and its arguments the
 * method's, and makes the cairo call of the FFI method it replaces, with the
 * same arguments and coercions:
 *   - a double argument is a SmallInteger or a Float (cp_number: the asFloat
 *     of the FFI); anything else (a Fraction, a LargeInteger) fails, and the
 *     patched method's fallback, the original FFI method, coerces it;
 *   - an int or enum argument is a SmallInteger in the range of int
 *     (cp_int), passed unchecked, as the FFI passes it;
 *   - a handle (a cairo_t, a surface, a pattern, a path, a scaled font) is
 *     the non-NULL address of the ExternalAddress in the handle slot of its
 *     object (cp_handle): NULL is the stale handle of a previous session;
 *   - a void* buffer is a byte object that is not an ExternalAddress
 *     (cp_bytes), of the size the call reads or writes; where the FFI also
 *     passes an ExternalAddress as the pointer it holds (glyphs, extents),
 *     the ExternalAddress is tested first;
 *   - a void callout answers the receiver, an int callout a SmallInteger.
 * Failure codes (docs/CairoPlugin.md, "Failure codes"): #'bad receiver' for
 * the receiver or a slot chain from it, #'bad argument' for an argument or a
 * chain from it, #'no modification' for an object to be written that is
 * read-only, #'unsupported operation' when unbound or when a needed optional
 * function is missing.  Every check comes before the first cairo call; the
 * only failure after one is the allocation of the answer of a pure getter
 * (primitiveGetCurrentPoint), whose retry repeats a call without effect.
 *
 * Owner: WP-B.
 *
 * Not a translation unit: src/common/CairoPlugin.c #includes it after its
 * helpers (cp_slot, cp_handle, cp_canvas_cr, cp_matrix, cp_number, cp_int,
 * cp_bytes, cp_cstring, cp_address*, cp_new_*), macros (CP_ARGS, CP_BOUND,
 * CP_ARG, CP_RECEIVER, CP_CALL, CP_HAVE, CP_PRIM_DONE) and state (cp_layout),
 * so it includes nothing, and every primitive is EXPORT(sqInt) name(void),
 * declared in CairoPlugin.c and listed in tests/primitives.txt. */


/*** Helpers of this file (cp_b_*): 1 or 0 unless they say otherwise ***/

/* Whether oop is an ExternalAddress (of any size and value) */
static int
cp_b_is_address(sqInt oop)
{
	return !interpreterProxy->isImmediate(oop)
		&& interpreterProxy->fetchClassOf(oop) == interpreterProxy->classExternalAddress();
}

/* d[0..n-1] = the n arguments first..first+n-1 of a primitive of nargs
   arguments, as numbers (cp_number) */
static int
cp_b_numbers(int nargs, int first, int n, double *d)
{
	int i;

	for (i = 0; i < n; i++) {
		if (!cp_number(CP_ARG(nargs, first + i), &d[i])) {
			return 0;
		}
	}
	return 1;
}

/* cr = the cairo_t of the AthensCairoPathBuilder builder (its context, an
   AthensCairoCanvas) */
static int
cp_b_builder_cr(sqInt builder, cp_cairo_t **cr)
{
	sqInt canvas;

	return cp_slot(builder, CP_L_BUILDER_CONTEXT, &canvas) && cp_canvas_cr(canvas, cr);
}

/* m = the matrix of the AthensCairoMatrix obj (cp_matrix); answers 0 or the
   failure code: notValid when it is not one, PrimErrNoModification when it
   is one, but read-only and writable is asked */
static sqInt
cp_b_matrix(sqInt obj, int writable, sqInt notValid, cp_matrix_t **m)
{
	if (cp_matrix(obj, writable, m)) {
		return 0;
	}
	return writable && cp_matrix(obj, 0, m) ? PrimErrNoModification : notValid;
}

/* p = the bytes of a byte object of at least min bytes that the call writes
   (cp_bytes, writable); answers 0, PrimErrBadArgument (not one: an
   ExternalAddress, too short, not bytes) or PrimErrNoModification (read-only) */
static sqInt
cp_b_out_bytes(sqInt oop, size_t min, void **p)
{
	if (!cp_bytes(oop, min, 0, p, NULL)) {
		return PrimErrBadArgument;
	}
	return cp_bytes(oop, min, 1, p, NULL) ? 0 : PrimErrNoModification;
}

/* g = the glyphs (cp_glyph_t) of an argument: an ExternalAddress (tested
   first: its address, trusted like the FFI; NULL only when n = 0, which
   cairo checks before it reads the glyphs: stock Athens passes the NULL
   glyphs of an empty string), else a byte object of at least n glyphs */
static int
cp_b_glyphs(sqInt oop, int n, const cp_glyph_t **g)
{
	void *p;

	if (cp_b_is_address(oop)) {
		if (!cp_address_or_null(oop, &p) || (p == NULL && n != 0)) {
			return 0;
		}
	} else if (n < 0 || !cp_bytes(oop, (size_t)n * sizeof(cp_glyph_t), 0, &p, NULL)) {
		return 0;
	}
	*g = (const cp_glyph_t *)p;
	return 1;
}

/* p = where cairo writes the extents of a CairoTextExtents or
   CairoFontExtents (size bytes): the handle of the object (its slot
   EXTOBJ_HANDLE), a non-NULL ExternalAddress (tested first: its address,
   trusted like the FFI), else a byte object of at least size bytes, not
   read-only; answers 0, PrimErrBadArgument or PrimErrNoModification */
static sqInt
cp_b_extents(sqInt obj, size_t size, void **p)
{
	sqInt handle;

	if (!cp_slot(obj, CP_L_EXTOBJ_HANDLE, &handle)) {
		return PrimErrBadArgument;
	}
	if (cp_b_is_address(handle)) {
		return cp_address(handle, p) ? 0 : PrimErrBadArgument;
	}
	return cp_b_out_bytes(handle, size, p);
}

/* The object (a fresh CairoScaledFont or AthensCairoPath basicNew) whose
   handle slot primitiveGetScaledFont and primitiveBuilderCopyPath set: a
   pointers object with that slot, the slot nil (as basicNew leaves it), not
   read-only; answers 0, PrimErrBadArgument or PrimErrNoModification.  The nil
   test keeps the primitives from replacing a live handle (a font's, or a
   path's that would then leak) or slot 0 of any other object: an Array's
   first element, a class's superclass, a married Context's sender (a
   SmallInteger frame pointer; storing there crashes the VM) */
static sqInt
cp_b_handle_target(sqInt oop)
{
	sqInt handle;

	if (!cp_slot(oop, CP_L_EXTOBJ_HANDLE, &handle)
	 || handle != interpreterProxy->nilObject()) {
		return PrimErrBadArgument;
	}
	return cp_writable_slot(oop, CP_L_EXTOBJ_HANDLE) ? 0 : PrimErrNoModification;
}

/* Stores ptr into the ExternalAddress ea (made by cp_new_address) and ea into
   the handle slot of target (validated by cp_b_handle_target) */
static void
cp_b_store_handle(sqInt target, sqInt ea, void *ptr)
{
	memcpy(interpreterProxy->firstIndexableField(ea), &ptr, sizeof(void *));
	interpreterProxy->storePointerofObjectwithValue(cp_layout[CP_L_EXTOBJ_HANDLE], target, ea);
}


/*** AthensCairoCanvas: the cairo_t of the receiver ***/

/*	AthensCairoCanvas>>newPath
	<primitive: 'primitiveNewPath' module: 'CairoPlugin'>
	cairo_new_path(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveNewPath(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(new_path)(cr);
	CP_PRIM_DONE(NewPath);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>fill: cairo_fill(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveFill(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(fill)(cr);
	CP_PRIM_DONE(Fill);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>fillPreserve: cairo_fill_preserve(cr).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveFillPreserve(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(fill_preserve)(cr);
	CP_PRIM_DONE(FillPreserve);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>stroke: cairo_stroke(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveStroke(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(stroke)(cr);
	CP_PRIM_DONE(Stroke);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primPaint: cairo_paint(cr).  Answer the receiver. */
EXPORT(sqInt)
primitivePaint(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(paint)(cr);
	CP_PRIM_DONE(Paint);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSaveState: cairo_save(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveSave(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(save)(cr);
	CP_PRIM_DONE(Save);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primRestoreState: cairo_restore(cr).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveRestore(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(restore)(cr);
	CP_PRIM_DONE(Restore);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primClip: cairo_clip(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveClip(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(clip)(cr);
	CP_PRIM_DONE(Clip);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>resetClip: cairo_reset_clip(cr).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveResetClip(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(reset_clip)(cr);
	CP_PRIM_DONE(ResetClip);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>setPathMatrix
	cairo_set_matrix(cr, the matrix of the receiver's pathTransform).  Answer
	the receiver. */
EXPORT(sqInt)
primitiveSetPathMatrix(void)
{
	cp_cairo_t *cr;
	cp_matrix_t *m;
	sqInt rcvr, transform;

	CP_ARGS(0);
	CP_BOUND();
	rcvr = CP_RECEIVER(0);
	if (!cp_canvas_cr(rcvr, &cr) || !cp_slot(rcvr, CP_L_CANVAS_PATH_TRANSFORM, &transform)
	 || !cp_matrix(transform, 0, &m)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(set_matrix)(cr, m);
	CP_PRIM_DONE(SetPathMatrix);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>setPathMatrix: anAthensCairoMatrix
	cairo_set_matrix(cr, its matrix).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetMatrix(void)
{
	cp_cairo_t *cr;
	cp_matrix_t *m;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_matrix(CP_ARG(1, 0), 0, &m)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_matrix)(cr, m);
	CP_PRIM_DONE(SetMatrix);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetSourceR: red g: green b: blue a: alpha
	cairo_set_source_rgba(cr, red, green, blue, alpha).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveSetSourceRGBA(void)
{
	cp_cairo_t *cr;
	double c[4];

	CP_ARGS(4);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(4), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(4, 0, 4, c)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_source_rgba)(cr, c[0], c[1], c[2], c[3]);
	CP_PRIM_DONE(SetSourceRGBA);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetSource: aPattern
	cairo_set_source(cr, the pattern of aPattern's handle).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveSetSource(void)
{
	cp_cairo_t *cr;
	void *pattern;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_handle(CP_ARG(1, 0), CP_L_EXTOBJ_HANDLE, &pattern)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_source)(cr, (cp_pattern_t *)pattern);
	CP_PRIM_DONE(SetSource);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primResetDashes: anOffset
	cairo_set_dash(cr, NULL, 0, anOffset).  Answer the receiver. */
EXPORT(sqInt)
primitiveResetDash(void)
{
	cp_cairo_t *cr;
	double offset;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_number(CP_ARG(1, 0), &offset)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_dash)(cr, NULL, 0, offset);
	CP_PRIM_DONE(ResetDash);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetDashesLengths: aByteArray count: n offset: anOffset
	cairo_set_dash(cr, the n doubles of aByteArray, n, anOffset): a byte
	object of at least 8n bytes, not an ExternalAddress (whose bytes are a
	pointer: the FFI then passes that pointer, so it falls back to the FFI);
	0 <= n.  Answer the receiver. */
EXPORT(sqInt)
primitiveSetDash(void)
{
	cp_cairo_t *cr;
	void *dashes;
	size_t size;
	int n;
	double offset;

	CP_ARGS(3);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(3), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_bytes(CP_ARG(3, 0), 0, 0, &dashes, &size) || !cp_int(CP_ARG(3, 1), &n)
	 || n < 0 || (size_t)n > size / sizeof(double) || !cp_number(CP_ARG(3, 2), &offset)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_dash)(cr, (const double *)dashes, n, offset);
	CP_PRIM_DONE(SetDash);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primRectangleX: x y: y width: w height: h
	cairo_rectangle(cr, x, y, w, h).  Answer the receiver. */
EXPORT(sqInt)
primitiveRectangle(void)
{
	cp_cairo_t *cr;
	double d[4];

	CP_ARGS(4);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(4), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(4, 0, 4, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(rectangle)(cr, d[0], d[1], d[2], d[3]);
	CP_PRIM_DONE(Rectangle);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetLineWidth: width
	cairo_set_line_width(cr, width).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetLineWidth(void)
{
	cp_cairo_t *cr;
	double width;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_number(CP_ARG(1, 0), &width)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_line_width)(cr, width);
	CP_PRIM_DONE(SetLineWidth);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetLineJoin: joinStyle
	cairo_set_line_join(cr, joinStyle).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetLineJoin(void)
{
	cp_cairo_t *cr;
	int join;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(1, 0), &join)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_line_join)(cr, (cp_line_join_t)join);
	CP_PRIM_DONE(SetLineJoin);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>primSetLineCap: capStyle
	cairo_set_line_cap(cr, capStyle).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetLineCap(void)
{
	cp_cairo_t *cr;
	int cap;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(1, 0), &cap)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_line_cap)(cr, (cp_line_cap_t)cap);
	CP_PRIM_DONE(SetLineCap);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>loadPath: anAthensCairoPath
	cairo_append_path(cr, the path of its handle).  Answer the receiver. */
EXPORT(sqInt)
primitiveAppendPath(void)
{
	cp_cairo_t *cr;
	void *path;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_handle(CP_ARG(1, 0), CP_L_EXTOBJ_HANDLE, &path)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(append_path)(cr, (const cp_path_t *)path);
	CP_PRIM_DONE(AppendPath);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>getCurrentPoint
	cairo_get_current_point(cr, &x, &y).  Answer x @ y, a Point of two Floats
	(the (x doubleAt: 1) @ (y doubleAt: 1) of the stock method).  A pure
	getter: the allocation of the answer may fail after the call
	(#'insufficient object memory': the VM retries after a GC). */
EXPORT(sqInt)
primitiveGetCurrentPoint(void)
{
	cp_cairo_t *cr;
	double x = 0.0, y = 0.0;
	sqInt point;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(get_current_point)(cr, &x, &y);
	if (!(point = cp_new_point(x, y))) {
		return 0;	/* the failure code is set */
	}
	CP_PRIM_DONE(GetCurrentPoint);
	return cp_return(point);
}

/*	AthensCairoCanvas>>setAA: antiAlias
	cairo_set_antialias(cr, antiAlias).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetAntialias(void)
{
	cp_cairo_t *cr;
	int antialias;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(1, 0), &antialias)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_antialias)(cr, (cp_antialias_t)antialias);
	CP_PRIM_DONE(SetAntialias);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>setFillRule: aCairoFillRule
	cairo_set_fill_rule(cr, aCairoFillRule).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetFillRule(void)
{
	cp_cairo_t *cr;
	int rule;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(1, 0), &rule)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_fill_rule)(cr, (cp_fill_rule_t)rule);
	CP_PRIM_DONE(SetFillRule);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>setScaledFont: aCairoScaledFont
	cairo_set_scaled_font(cr, the scaled font of its handle).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveSetScaledFont(void)
{
	cp_cairo_t *cr;
	void *font;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_handle(CP_ARG(1, 0), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_scaled_font)(cr, (const cp_scaled_font_t *)font);
	CP_PRIM_DONE(SetScaledFont);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>showGlyphs: glyphs size: n
	cairo_show_glyphs(cr, glyphs, n): glyphs an ExternalAddress (tested
	first; NULL only when n = 0), else a byte object of at least n glyphs
	(24 bytes each); 0 <= n.  Answer the receiver. */
EXPORT(sqInt)
primitiveShowGlyphs(void)
{
	cp_cairo_t *cr;
	const cp_glyph_t *glyphs;
	int n;

	CP_ARGS(2);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(2), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(2, 1), &n) || n < 0 || !cp_b_glyphs(CP_ARG(2, 0), n, &glyphs)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(show_glyphs)(cr, glyphs, n);
	CP_PRIM_DONE(ShowGlyphs);
	return cp_return_receiver();
}

/*	AthensCairoCanvas>>cairoPluginGetScaledFontInto: aCairoScaledFont
	(the patched getScaledFont passes CairoScaledFont basicNew)
	Store a new ExternalAddress of cairo_get_scaled_font(cr) into the handle
	slot of aCairoScaledFont and answer it: the uFFI's `CairoScaledFont
	basicNew instVarAt: 1 put: result` (cairo owns the font: borrowed, not
	referenced).  The ExternalAddress is allocated before the call. */
EXPORT(sqInt)
primitiveGetScaledFont(void)
{
	cp_cairo_t *cr;
	cp_scaled_font_t *font;
	sqInt target, ea, code;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	target = CP_ARG(1, 0);
	if ((code = cp_b_handle_target(target))) {
		return cp_fail(code);
	}
	if (!(ea = cp_new_address(NULL))) {
		return 0;	/* PrimErrNoMemory is set */
	}
	font = CP_CALL(get_scaled_font)(cr);
	cp_b_store_handle(target, ea, (void *)font);
	CP_PRIM_DONE(GetScaledFont);
	return cp_return(target);
}

/*	AthensCairoCanvas>>primSetFontSize: aSize
	cairo_set_font_size(cr, aSize).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetFontSize(void)
{
	cp_cairo_t *cr;
	double size;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_number(CP_ARG(1, 0), &size)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_font_size)(cr, size);
	CP_PRIM_DONE(SetFontSize);
	return cp_return_receiver();
}


/*** AthensCairoPathBuilder: the cairo_t of the receiver's context ***/

/*	AthensCairoPathBuilder>>moveToX: x Y: y
	cairo_move_to(cr, x, y).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderMoveTo(void)
{
	cp_cairo_t *cr;
	double d[2];

	CP_ARGS(2);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(2), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(move_to)(cr, d[0], d[1]);
	CP_PRIM_DONE(BuilderMoveTo);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>lineToX: x Y: y
	cairo_line_to(cr, x, y).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderLineTo(void)
{
	cp_cairo_t *cr;
	double d[2];

	CP_ARGS(2);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(2), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(line_to)(cr, d[0], d[1]);
	CP_PRIM_DONE(BuilderLineTo);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>primCurveViaX: x1 Y: y1 viaX: x2 Y: y2 toX: x3 Y: y3
	cairo_curve_to(cr, x1, y1, x2, y2, x3, y3).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderCurveTo(void)
{
	cp_cairo_t *cr;
	double d[6];

	CP_ARGS(6);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(6), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(6, 0, 6, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(curve_to)(cr, d[0], d[1], d[2], d[3], d[4], d[5]);
	CP_PRIM_DONE(BuilderCurveTo);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>closePath
	cairo_close_path(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderClosePath(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(close_path)(cr);
	CP_PRIM_DONE(BuilderClosePath);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>newPath
	cairo_new_path(cr).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderNewPath(void)
{
	cp_cairo_t *cr;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(0), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(new_path)(cr);
	CP_PRIM_DONE(BuilderNewPath);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>cairoPluginCopyPathInto: anAthensCairoPath
	(the patched copyPath passes AthensCairoPath basicNew)
	Store a new ExternalAddress of cairo_copy_path(cr) into the handle slot of
	anAthensCairoPath and answer it (the patched copyPath answers it;
	createPath: then sends it initialize, as stock).  The ExternalAddress is
	allocated before the call. */
EXPORT(sqInt)
primitiveBuilderCopyPath(void)
{
	cp_cairo_t *cr;
	cp_path_t *path;
	sqInt target, ea, code;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	target = CP_ARG(1, 0);
	if ((code = cp_b_handle_target(target))) {
		return cp_fail(code);
	}
	if (!(ea = cp_new_address(NULL))) {
		return 0;	/* PrimErrNoMemory is set */
	}
	path = CP_CALL(copy_path)(cr);
	cp_b_store_handle(target, ea, (void *)path);
	CP_PRIM_DONE(BuilderCopyPath);
	return cp_return(target);
}

/*	AthensCairoPathBuilder>>arcCenterX: xc centerY: yc radius: r startAngle: a1 endAngle: a2
	cairo_arc(cr, xc, yc, r, a1, a2).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderArc(void)
{
	cp_cairo_t *cr;
	double d[5];

	CP_ARGS(5);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(5), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(5, 0, 5, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(arc)(cr, d[0], d[1], d[2], d[3], d[4]);
	CP_PRIM_DONE(BuilderArc);
	return cp_return_receiver();
}

/*	AthensCairoPathBuilder>>arcNegativeCenterX: xc centerY: yc radius: r startAngle: a1 endAngle: a2
	cairo_arc_negative(cr, xc, yc, r, a1, a2).  Answer the receiver. */
EXPORT(sqInt)
primitiveBuilderArcNegative(void)
{
	cp_cairo_t *cr;
	double d[5];

	CP_ARGS(5);
	CP_BOUND();
	if (!cp_b_builder_cr(CP_RECEIVER(5), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_b_numbers(5, 0, 5, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(arc_negative)(cr, d[0], d[1], d[2], d[3], d[4]);
	CP_PRIM_DONE(BuilderArcNegative);
	return cp_return_receiver();
}


/*** AthensCairoPath class ***/

/*	AthensCairoPath class>>primDestroyPath: anExternalAddress
	cairo_path_destroy(its address), which must not be NULL (the FFI copy
	then runs, as stock).  The address is not nulled (stock does not).
	Answer the receiver. */
EXPORT(sqInt)
primitivePathDestroy(void)
{
	void *path;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_address(CP_ARG(1, 0), &path)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(path_destroy)((cp_path_t *)path);
	CP_PRIM_DONE(PathDestroy);
	return cp_return_receiver();
}


/*** AthensCairoMatrix: the 6 doubles of the receiver (cp_matrix) ***/

/*	AthensCairoMatrix>>initx: x y: y sx: sx sy: sy shx: shx shy: shy
	cairo_matrix_init(m, sx, shy, shx, sy, x, y): the FFI method's order.
	Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixInit(void)
{
	cp_matrix_t *m;
	double d[6];
	sqInt code;

	CP_ARGS(6);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(6), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(6, 0, 6, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	/* d: x y sx sy shx shy */
	CP_CALL(matrix_init)(m, d[2], d[5], d[4], d[3], d[0], d[1]);
	CP_PRIM_DONE(MatrixInit);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primLoadIdentity
	cairo_matrix_init_identity(m).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixInitIdentity(void)
{
	cp_matrix_t *m;
	sqInt code;

	CP_ARGS(0);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(0), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	CP_CALL(matrix_init_identity)(m);
	CP_PRIM_DONE(MatrixInitIdentity);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primTranslateX: px Y: py
	cairo_matrix_translate(m, px, py).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixTranslate(void)
{
	cp_matrix_t *m;
	double d[2];
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(2), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_translate)(m, d[0], d[1]);
	CP_PRIM_DONE(MatrixTranslate);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primScaleX: fx Y: fy
	cairo_matrix_scale(m, fx, fy).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixScale(void)
{
	cp_matrix_t *m;
	double d[2];
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(2), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_scale)(m, d[0], d[1]);
	CP_PRIM_DONE(MatrixScale);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primRotateByRadians: angle
	cairo_matrix_rotate(m, angle).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixRotate(void)
{
	cp_matrix_t *m;
	double angle;
	sqInt code;

	CP_ARGS(1);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(1), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_number(CP_ARG(1, 0), &angle)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_rotate)(m, angle);
	CP_PRIM_DONE(MatrixRotate);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primMultiplyBy: anAthensCairoMatrix
	cairo_matrix_multiply(m, its matrix, m) (the FFI's order: the result is
	the receiver; cairo allows the aliasing).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixMultiply(void)
{
	cp_matrix_t *m, *other;
	sqInt code;

	CP_ARGS(1);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(1), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_matrix(CP_ARG(1, 0), 0, &other)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_multiply)(m, other, m);
	CP_PRIM_DONE(MatrixMultiply);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>invert
	Answer the status (a SmallInteger) of cairo_matrix_invert(m). */
EXPORT(sqInt)
primitiveMatrixInvert(void)
{
	cp_matrix_t *m;
	cp_status_t status;
	sqInt code;

	CP_ARGS(0);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(0), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	status = CP_CALL(matrix_invert)(m);
	CP_PRIM_DONE(MatrixInvert);
	return cp_return_int((sqInt)status);
}

/*	AthensCairoMatrix>>primTransformX: x Y: y
	cairo_matrix_transform_point(m, x, y): x and y byte objects of at least 8
	bytes (a double each, transformed in place), not read-only, not
	ExternalAddresses (whose 8 bytes are a pointer: the FFI then passes that
	pointer, so it falls back to the FFI); the receiver is only read.  Answer
	the receiver. */
EXPORT(sqInt)
primitiveMatrixTransformPoint(void)
{
	cp_matrix_t *m;
	void *x, *y;
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if (!cp_matrix(CP_RECEIVER(2), 0, &m)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if ((code = cp_b_out_bytes(CP_ARG(2, 0), sizeof(double), &x))
	 || (code = cp_b_out_bytes(CP_ARG(2, 1), sizeof(double), &y))) {
		return cp_fail(code);
	}
	CP_CALL(matrix_transform_point)(m, (double *)x, (double *)y);
	CP_PRIM_DONE(MatrixTransformPoint);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primSetTranslateX: x Y: y
	cairo_matrix_init_translate(m, x, y).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixInitTranslate(void)
{
	cp_matrix_t *m;
	double d[2];
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(2), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_init_translate)(m, d[0], d[1]);
	CP_PRIM_DONE(MatrixInitTranslate);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primSetScaleX: x Y: y
	cairo_matrix_init_scale(m, x, y).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixInitScale(void)
{
	cp_matrix_t *m;
	double d[2];
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(2), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(2, 0, 2, d)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_init_scale)(m, d[0], d[1]);
	CP_PRIM_DONE(MatrixInitScale);
	return cp_return_receiver();
}

/*	AthensCairoMatrix>>primSetRotationInRadians: radians
	cairo_matrix_init_rotate(m, radians).  Answer the receiver. */
EXPORT(sqInt)
primitiveMatrixInitRotate(void)
{
	cp_matrix_t *m;
	double radians;
	sqInt code;

	CP_ARGS(1);
	CP_BOUND();
	if ((code = cp_b_matrix(CP_RECEIVER(1), 1, PrimErrBadReceiver, &m))) {
		return cp_fail(code);
	}
	if (!cp_number(CP_ARG(1, 0), &radians)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(matrix_init_rotate)(m, radians);
	CP_PRIM_DONE(MatrixInitRotate);
	return cp_return_receiver();
}


/*** AthensCairoPaintMode: the cairo_t of the receiver's canvas ***/

/*	AthensCairoPaintMode>>getOperator
	Answer cairo_get_operator(cr), a SmallInteger. */
EXPORT(sqInt)
primitiveGetOperator(void)
{
	cp_cairo_t *cr;
	sqInt canvas;
	cp_operator_t op;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_slot(CP_RECEIVER(0), CP_L_PAINTMODE_CANVAS, &canvas) || !cp_canvas_cr(canvas, &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	op = CP_CALL(get_operator)(cr);
	CP_PRIM_DONE(GetOperator);
	return cp_return_int((sqInt)op);
}

/*	AthensCairoPaintMode>>setOperator: aCairoOperatorT
	cairo_set_operator(cr, aCairoOperatorT).  Answer the receiver. */
EXPORT(sqInt)
primitiveSetOperator(void)
{
	cp_cairo_t *cr;
	sqInt canvas;
	int op;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_slot(CP_RECEIVER(1), CP_L_PAINTMODE_CANVAS, &canvas) || !cp_canvas_cr(canvas, &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(1, 0), &op)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(set_operator)(cr, (cp_operator_t)op);
	CP_PRIM_DONE(SetOperator);
	return cp_return_receiver();
}


/*** AthensCairoSurface (and its PDF, SVG, Export subclasses) ***/

/*	AthensCairoSurface>>flush
	cairo_surface_flush(the surface of the receiver's handle).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveSurfaceFlush(void)
{
	void *surface;

	CP_ARGS(0);
	CP_BOUND();
	if (!cp_handle(CP_RECEIVER(0), CP_L_SURFACE_HANDLE, &surface)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(surface_flush)((cp_surface_t *)surface);
	CP_PRIM_DONE(SurfaceFlush);
	return cp_return_receiver();
}


/*** CairoScaledFont: the scaled font of the receiver's handle ***/

/*	CairoScaledFont>>lock
	cairo_ft_scaled_font_lock_face(font) (optional: #'unsupported operation'
	when the binding lacks it); its FT_Face is ignored (the FFI declares the
	call void).  Answer the receiver. */
EXPORT(sqInt)
primitiveScaledFontLockFace(void)
{
	void *font;

	CP_ARGS(0);
	CP_BOUND();
	if (!CP_HAVE(ft_scaled_font_lock_face)) {
		return cp_fail(PrimErrUnsupported);
	}
	if (!cp_handle(CP_RECEIVER(0), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadReceiver);
	}
	(void)CP_CALL(ft_scaled_font_lock_face)((cp_scaled_font_t *)font);
	CP_PRIM_DONE(ScaledFontLockFace);
	return cp_return_receiver();
}

/*	CairoScaledFont>>unlock
	cairo_ft_scaled_font_unlock_face(font) (optional, as lock).  Answer the
	receiver. */
EXPORT(sqInt)
primitiveScaledFontUnlockFace(void)
{
	void *font;

	CP_ARGS(0);
	CP_BOUND();
	if (!CP_HAVE(ft_scaled_font_unlock_face)) {
		return cp_fail(PrimErrUnsupported);
	}
	if (!cp_handle(CP_RECEIVER(0), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadReceiver);
	}
	CP_CALL(ft_scaled_font_unlock_face)((cp_scaled_font_t *)font);
	CP_PRIM_DONE(ScaledFontUnlockFace);
	return cp_return_receiver();
}

/*	CairoScaledFont>>getExtentsOfGlyphs: glyphs ofLength: n into: aCairoTextExtents
	cairo_scaled_font_glyph_extents(font, glyphs, n, extents): glyphs as
	primitiveShowGlyphs'; the extents where aCairoTextExtents' handle points
	(a non-NULL ExternalAddress, tested first) or in it (a byte object of at
	least 48 bytes, not read-only).  Answer the receiver. */
EXPORT(sqInt)
primitiveScaledFontGlyphExtents(void)
{
	void *font, *extents;
	const cp_glyph_t *glyphs;
	int n;
	sqInt code;

	CP_ARGS(3);
	CP_BOUND();
	if (!cp_handle(CP_RECEIVER(3), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_int(CP_ARG(3, 1), &n) || n < 0 || !cp_b_glyphs(CP_ARG(3, 0), n, &glyphs)) {
		return cp_fail(PrimErrBadArgument);
	}
	if ((code = cp_b_extents(CP_ARG(3, 2), sizeof(cp_text_extents_t), &extents))) {
		return cp_fail(code);
	}
	CP_CALL(scaled_font_glyph_extents)((cp_scaled_font_t *)font, glyphs, n, (cp_text_extents_t *)extents);
	CP_PRIM_DONE(ScaledFontGlyphExtents);
	return cp_return_receiver();
}

/*	CairoScaledFont>>getExtentsOf: utf8 into: aCairoTextExtents
	cairo_scaled_font_text_extents(font, utf8, extents): utf8 a byte object
	holding a NUL within its body (cp_cstring: never read past it; an
	ExternalAddress or a body without NUL falls back to the FFI); the extents
	as primitiveScaledFontGlyphExtents'.  Answer the receiver. */
EXPORT(sqInt)
primitiveScaledFontTextExtents(void)
{
	void *font, *extents;
	const char *utf8;
	sqInt code;

	CP_ARGS(2);
	CP_BOUND();
	if (!cp_handle(CP_RECEIVER(2), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_cstring(CP_ARG(2, 0), &utf8)) {
		return cp_fail(PrimErrBadArgument);
	}
	if ((code = cp_b_extents(CP_ARG(2, 1), sizeof(cp_text_extents_t), &extents))) {
		return cp_fail(code);
	}
	CP_CALL(scaled_font_text_extents)((cp_scaled_font_t *)font, utf8, (cp_text_extents_t *)extents);
	CP_PRIM_DONE(ScaledFontTextExtents);
	return cp_return_receiver();
}

/*	CairoScaledFont>>getExtents: aCairoFontExtents
	cairo_scaled_font_extents(font, extents): where aCairoFontExtents' handle
	points (a non-NULL ExternalAddress, tested first) or in it (a byte object
	of at least 40 bytes, cairo's size; Athens' is 48, not read-only).  Answer
	0 (the FFI declares an int for this void function: its answer is
	undefined, and the callers ignore it). */
EXPORT(sqInt)
primitiveScaledFontExtents(void)
{
	void *font, *extents;
	sqInt code;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_handle(CP_RECEIVER(1), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if ((code = cp_b_extents(CP_ARG(1, 0), sizeof(cp_font_extents_t), &extents))) {
		return cp_fail(code);
	}
	CP_CALL(scaled_font_extents)((cp_scaled_font_t *)font, (cp_font_extents_t *)extents);
	CP_PRIM_DONE(ScaledFontExtents);
	return cp_return_int(0);
}


/*** CairoFreetypeFontRenderer and CairoFontMetricsProvider (the receiver is
     not read) ***/

/*	convertString: utf8 len: len ofFont: aCairoScaledFont toGlyphs: anExternalAddress
		numGlyphs: aByteArray x: x y: y
	cairo_scaled_font_text_to_glyphs(font, x, y, utf8, len, &glyphs,
	&numGlyphs, NULL, NULL, NULL), glyphs and numGlyphs in/out: utf8 a byte
	object (not an ExternalAddress) of at least len bytes, 0 <= len; the
	glyphs pointer the 8 bytes of anExternalAddress (NULL allowed, not
	read-only: cairo's in/out pointer); numGlyphs the int32 in the first 4
	bytes of aByteArray (a byte object, not an ExternalAddress, not
	read-only); x, y numbers.  Answer the status, a SmallInteger. */
EXPORT(sqInt)
primitiveTextToGlyphs(void)
{
	void *utf8, *font, *glyphsBody, *countBody, *ignored;
	size_t size;
	int len, count;
	double xy[2];
	cp_glyph_t *glyphs;
	cp_status_t status;
	sqInt glyphsOop, code;

	CP_ARGS(7);
	CP_BOUND();
	if (!cp_bytes(CP_ARG(7, 0), 0, 0, &utf8, &size) || !cp_int(CP_ARG(7, 1), &len)
	 || len < 0 || (size_t)len > size || !cp_handle(CP_ARG(7, 2), CP_L_EXTOBJ_HANDLE, &font)) {
		return cp_fail(PrimErrBadArgument);
	}
	glyphsOop = CP_ARG(7, 3);
	if (!cp_address_or_null(glyphsOop, &ignored)) {
		return cp_fail(PrimErrBadArgument);
	}
	if (!cp_address_out(glyphsOop, &glyphsBody)) {
		return cp_fail(PrimErrNoModification);
	}
	if ((code = cp_b_out_bytes(CP_ARG(7, 4), sizeof(int), &countBody))) {
		return cp_fail(code);
	}
	if (!cp_b_numbers(7, 5, 2, xy)) {
		return cp_fail(PrimErrBadArgument);
	}
	memcpy(&glyphs, glyphsBody, sizeof glyphs);
	memcpy(&count, countBody, sizeof count);
	status = CP_CALL(scaled_font_text_to_glyphs)((cp_scaled_font_t *)font, xy[0], xy[1], (const char *)utf8, len,
		&glyphs, &count, NULL, NULL, NULL);
	memcpy(glyphsBody, &glyphs, sizeof glyphs);
	memcpy(countBody, &count, sizeof count);
	CP_PRIM_DONE(TextToGlyphs);
	return cp_return_int((sqInt)status);
}

/*	freeGlyphs: anExternalAddress
	cairo_glyph_free(its address; NULL allowed, as cairo).  The address is not
	nulled (stock does not).  Answer the receiver. */
EXPORT(sqInt)
primitiveGlyphFree(void)
{
	void *glyphs;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_address_or_null(CP_ARG(1, 0), &glyphs)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(glyph_free)((cp_glyph_t *)glyphs);
	CP_PRIM_DONE(GlyphFree);
	return cp_return_receiver();
}


/*** AthensCairoCanvas, Roassal's bordered labels ***/

/*	AthensCairoCanvas>>textPath: anUTF8EncodedString
	cairo_text_path(cr, its bytes): a byte object holding a NUL within its
	body (cp_cstring: the bytes the FFI's void* passes; an ExternalAddress or
	a body without NUL falls back to the FFI).  Answer the receiver. */
EXPORT(sqInt)
primitiveTextPath(void)
{
	cp_cairo_t *cr;
	const char *utf8;

	CP_ARGS(1);
	CP_BOUND();
	if (!cp_canvas_cr(CP_RECEIVER(1), &cr)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_cstring(CP_ARG(1, 0), &utf8)) {
		return cp_fail(PrimErrBadArgument);
	}
	CP_CALL(text_path)(cr, utf8);
	CP_PRIM_DONE(TextPath);
	return cp_return_receiver();
}
