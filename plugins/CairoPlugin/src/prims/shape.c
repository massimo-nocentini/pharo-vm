/* shape.c -- the shape-level primitives of the CairoPlugin (group #shape,
 * docs/CairoPlugin.md section "Shape-level primitives"): a Rectangle or an
 * AthensCairoPath painted with a Color, or with an AthensCairoStrokePaint of
 * a Color, in one primitive making exactly the cairo calls, in exactly the
 * order, of the stock Athens path, so that pixels, cairo state and the
 * per-function call counts are those of the FFI by construction.  What they
 * skip is Smalltalk work: the double dispatch, the asFloat sends, the Color
 * component sends, the argument Arrays and the boundary crossings.
 *
 * The stock sequences (Athens-Cairo, identical source in Pharo 12-15):
 *   AthensCairoCanvas>>draw = self setPathMatrix.
 *                             ^ shape paintFillsUsing: paint on: self
 *   Rectangle + Color: AthensCairoSurface>>fillRectangle:withSolidColor: =
 *     loadSolidColor: (currentCanvas setSourceR:g:b:a:, resetDash),
 *     currentCanvas newPath; rectangleX: left asFloat y: top asFloat
 *     width: width asFloat height: height asFloat; fill; answers the surface
 *   AthensCairoPath + Color: fillPath:withSolidColor: = loadSolidColor:,
 *     newPath; loadPath: aPath; fill; answers the surface
 *   Rectangle or path + AthensCairoStrokePaint: athensFillRectangle:on: /
 *     athensFillPath:on: = prepareForDrawingOn: (fillPaint
 *     loadOnCairoCanvas:, primSetLineWidth: width asFloat, primSetLineJoin:,
 *     primSetLineCap:, dashLenghts nil -> resetDash, else setDashesOn: =
 *     set_dash of a buffer of the lengths asFloat, dashLenghts size,
 *     dashOffset), newPath, rectangle or loadPath:, stroke; answer the paint
 *
 * The fill primitives draw on the receiver, and require it to be its
 * surface's currentCanvas (as stock, which fills through currentCanvas: nil
 * outside drawDuring:, and then the fallback raises as stock does); the
 * stroke primitives draw on the receiver, as stock.  The installed methods
 * (AthensCairoCanvas>>cairoPlugin..., docs/CairoPlugin.md) fall back to the
 * original draw (ffiDraw) when a primitive fails.
 *
 * Owner: WP-C.
 *
 * Not a translation unit: src/common/CairoPlugin.c #includes it (see
 * basic.c).  The dash buffer is the one malloc of this file (R1). */

/* The dashes of a stroke paint that fit on the stack; more are malloc'd */
#define CP_C_DASHES_ON_STACK 64
/* The most dashes accepted (more: the fallback) */
#define CP_C_DASHES_MAX 4096

/* What a primitive draws on: the cairo_t of the receiver canvas and the 6
   doubles of its pathTransform (setPathMatrix) */
typedef struct {
	cp_cairo_t *cr;
	cp_matrix_t *matrix;
} cp_c_canvas;

/* What an AthensCairoStrokePaint of a Color loads (prepareForDrawingOn:) */
typedef struct {
	double rgba[4];
	double width;
	int join, cap;
	int dashed;		/* 0: dashLenghts nil (resetDash) */
	int count;		/* dashLenghts size */
	double offset;		/* dashOffset */
	double *dashes;		/* count lengths: stack or malloc'd */
	double stack[CP_C_DASHES_ON_STACK];
} cp_c_stroke;

/* The receiver canvas (cr and pathTransform); with currentCanvas, also
   the identity rcvr.surface.currentCanvas == rcvr */
static int
cp_c_canvas_of(sqInt rcvr, int currentCanvas, cp_c_canvas *c)
{
	sqInt transform, surface, current;

	if (!cp_canvas_cr(rcvr, &c->cr)
	 || !cp_slot(rcvr, CP_L_CANVAS_PATH_TRANSFORM, &transform)
	 || !cp_matrix(transform, 0, &c->matrix)) {
		return 0;
	}
	if (currentCanvas
	 && (!cp_slot(rcvr, CP_L_CANVAS_SURFACE, &surface)
	  || !cp_slot(surface, CP_L_SURFACE_CURRENT_CANVAS, &current)
	  || current != rcvr)) {
		return 0;
	}
	return 1;
}

/* The fields of a stroke paint whose fillPaint is exactly of class
   colorClass, and its dash lengths (nil, or an Array of at most
   CP_C_DASHES_MAX numbers, then dashOffset a number); the lengths go into
   s->stack or a malloc'd buffer.  Answers 0 or a failure code:
   PrimErrBadArgument, PrimErrNoCMemory.  The caller frees with
   cp_c_stroke_free. */
static sqInt
cp_c_stroke_of(sqInt paint, sqInt colorClass, cp_c_stroke *s)
{
	sqInt fill, width, join, cap, dashes, offset, element;
	sqInt i, n;

	s->dashes = s->stack;
	s->count = 0;
	s->dashed = 0;
	s->offset = 0.0;
	if (interpreterProxy->isImmediate(colorClass) || !interpreterProxy->isPointers(colorClass)) {
		return PrimErrBadArgument;
	}
	if (!cp_slot(paint, CP_L_STROKE_FILL_PAINT, &fill)
	 || interpreterProxy->isImmediate(fill)
	 || interpreterProxy->fetchClassOf(fill) != colorClass
	 || !cp_color(fill, s->rgba)
	 || !cp_slot(paint, CP_L_STROKE_WIDTH, &width) || !cp_number(width, &s->width)
	 || !cp_slot(paint, CP_L_STROKE_JOIN, &join) || !cp_int(join, &s->join)
	 || !cp_slot(paint, CP_L_STROKE_CAP, &cap) || !cp_int(cap, &s->cap)
	 || !cp_slot(paint, CP_L_STROKE_DASHES, &dashes)) {
		return PrimErrBadArgument;
	}
	if (dashes == interpreterProxy->nilObject()) {
		return 0;
	}
	if (interpreterProxy->isImmediate(dashes)
	 || interpreterProxy->fetchClassOf(dashes) != interpreterProxy->classArray()
	 || (n = interpreterProxy->slotSizeOf(dashes)) > CP_C_DASHES_MAX
	 || !cp_slot(paint, CP_L_STROKE_DASH_OFFSET, &offset) || !cp_number(offset, &s->offset)) {
		return PrimErrBadArgument;
	}
	/* the numbers first: a bad element fails without a malloc */
	for (i = 0; i < n; i++) {
		element = interpreterProxy->fetchPointerofObject(i, dashes);
		if (!cp_number(element, &s->stack[0])) {
			return PrimErrBadArgument;
		}
	}
	if (n > CP_C_DASHES_ON_STACK
	 && !(s->dashes = malloc((size_t)n * sizeof(double)))) {
		s->dashes = s->stack;
		return PrimErrNoCMemory;
	}
	for (i = 0; i < n; i++) {
		cp_number(interpreterProxy->fetchPointerofObject(i, dashes), &s->dashes[i]);
	}
	s->dashed = 1;
	s->count = (int)n;
	return 0;
}

static void
cp_c_stroke_free(cp_c_stroke *s)
{
	if (s->dashes != s->stack) {
		free(s->dashes);
		s->dashes = s->stack;
	}
}

/* setPathMatrix, then loadSolidColor: (set_source_rgba, resetDash), then
   newPath: the head of a Color fill */
static void
cp_c_fill_head(const cp_c_canvas *c, const double rgba[4])
{
	CP_CALL(set_matrix)(c->cr, c->matrix);
	CP_CALL(set_source_rgba)(c->cr, rgba[0], rgba[1], rgba[2], rgba[3]);
	CP_CALL(set_dash)(c->cr, NULL, 0, 0.0);
	CP_CALL(new_path)(c->cr);
}

/* setPathMatrix, then prepareForDrawingOn:, then newPath: the head of a
   stroke */
static void
cp_c_stroke_head(const cp_c_canvas *c, const cp_c_stroke *s)
{
	CP_CALL(set_matrix)(c->cr, c->matrix);
	CP_CALL(set_source_rgba)(c->cr, s->rgba[0], s->rgba[1], s->rgba[2], s->rgba[3]);
	CP_CALL(set_line_width)(c->cr, s->width);
	CP_CALL(set_line_join)(c->cr, (cp_line_join_t)s->join);
	CP_CALL(set_line_cap)(c->cr, (cp_line_cap_t)s->cap);
	if (s->dashed) {
		CP_CALL(set_dash)(c->cr, s->dashes, s->count, s->offset);
	} else {
		CP_CALL(set_dash)(c->cr, NULL, 0, 0.0);
	}
	CP_CALL(new_path)(c->cr);
}


/*** primitiveFillRectangleColor ***/

/*	AthensCairoCanvas>>cairoPluginFillRectangle: aRectangle color: aColor
	<primitive: 'primitiveFillRectangleColor' module: 'CairoPlugin'>
	^ self ffiDraw
	The draw of a Rectangle with a Color (the receiver must be its surface's
	currentCanvas): set_matrix, set_source_rgba, set_dash(NULL, 0, 0.0),
	new_path, rectangle, fill.  Answer the receiver's surface. */
EXPORT(sqInt)
primitiveFillRectangleColor(void)
{
	sqInt rcvr, surface;
	cp_c_canvas c;
	double xywh[4], rgba[4];

	CP_ARGS(2);
	CP_BOUND();
	rcvr = CP_RECEIVER(2);
	if (!cp_c_canvas_of(rcvr, 1, &c) || !cp_slot(rcvr, CP_L_CANVAS_SURFACE, &surface)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_rect(CP_ARG(2, 0), xywh) || !cp_color(CP_ARG(2, 1), rgba)) {
		return cp_fail(PrimErrBadArgument);
	}
	cp_c_fill_head(&c, rgba);
	CP_CALL(rectangle)(c.cr, xywh[0], xywh[1], xywh[2], xywh[3]);
	CP_CALL(fill)(c.cr);
	CP_PRIM_DONE(FillRectangleColor);
	return cp_return(surface);
}


/*** primitiveStrokeRectangle ***/

/*	AthensCairoCanvas>>cairoPluginStrokeRectangle: aRectangle paint: aStrokePaint colorClass: aClass
	<primitive: 'primitiveStrokeRectangle' module: 'CairoPlugin'>
	^ self ffiDraw
	The draw of a Rectangle with an AthensCairoStrokePaint whose fillPaint is
	exactly of class aClass (Color): set_matrix, set_source_rgba,
	set_line_width, set_line_join, set_line_cap, set_dash, new_path,
	rectangle, stroke.  Answer the paint. */
EXPORT(sqInt)
primitiveStrokeRectangle(void)
{
	sqInt paint, code;
	cp_c_canvas c;
	cp_c_stroke s;
	double xywh[4];

	CP_ARGS(3);
	CP_BOUND();
	if (!cp_c_canvas_of(CP_RECEIVER(3), 0, &c)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_rect(CP_ARG(3, 0), xywh)) {
		return cp_fail(PrimErrBadArgument);
	}
	paint = CP_ARG(3, 1);
	if ((code = cp_c_stroke_of(paint, CP_ARG(3, 2), &s))) {
		cp_c_stroke_free(&s);
		return cp_fail(code);
	}
	cp_c_stroke_head(&c, &s);
	CP_CALL(rectangle)(c.cr, xywh[0], xywh[1], xywh[2], xywh[3]);
	CP_CALL(stroke)(c.cr);
	cp_c_stroke_free(&s);
	CP_PRIM_DONE(StrokeRectangle);
	return cp_return(paint);
}


/*** primitiveFillPathColor ***/

/*	AthensCairoCanvas>>cairoPluginFillPath: aPath color: aColor
	<primitive: 'primitiveFillPathColor' module: 'CairoPlugin'>
	^ self ffiDraw
	The draw of an AthensCairoPath with a Color (the receiver must be its
	surface's currentCanvas): set_matrix, set_source_rgba,
	set_dash(NULL, 0, 0.0), new_path, append_path, fill.  Answer the
	receiver's surface. */
EXPORT(sqInt)
primitiveFillPathColor(void)
{
	sqInt rcvr, surface;
	cp_c_canvas c;
	void *path;
	double rgba[4];

	CP_ARGS(2);
	CP_BOUND();
	rcvr = CP_RECEIVER(2);
	if (!cp_c_canvas_of(rcvr, 1, &c) || !cp_slot(rcvr, CP_L_CANVAS_SURFACE, &surface)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_handle(CP_ARG(2, 0), CP_L_EXTOBJ_HANDLE, &path) || !cp_color(CP_ARG(2, 1), rgba)) {
		return cp_fail(PrimErrBadArgument);
	}
	cp_c_fill_head(&c, rgba);
	CP_CALL(append_path)(c.cr, (const cp_path_t *)path);
	CP_CALL(fill)(c.cr);
	CP_PRIM_DONE(FillPathColor);
	return cp_return(surface);
}


/*** primitiveStrokePath ***/

/*	AthensCairoCanvas>>cairoPluginStrokePath: aPath paint: aStrokePaint colorClass: aClass
	<primitive: 'primitiveStrokePath' module: 'CairoPlugin'>
	^ self ffiDraw
	The draw of an AthensCairoPath with an AthensCairoStrokePaint whose
	fillPaint is exactly of class aClass (Color): set_matrix,
	set_source_rgba, set_line_width, set_line_join, set_line_cap, set_dash,
	new_path, append_path, stroke.  Answer the paint. */
EXPORT(sqInt)
primitiveStrokePath(void)
{
	sqInt paint, code;
	cp_c_canvas c;
	cp_c_stroke s;
	void *path;

	CP_ARGS(3);
	CP_BOUND();
	if (!cp_c_canvas_of(CP_RECEIVER(3), 0, &c)) {
		return cp_fail(PrimErrBadReceiver);
	}
	if (!cp_handle(CP_ARG(3, 0), CP_L_EXTOBJ_HANDLE, &path)) {
		return cp_fail(PrimErrBadArgument);
	}
	paint = CP_ARG(3, 1);
	if ((code = cp_c_stroke_of(paint, CP_ARG(3, 2), &s))) {
		cp_c_stroke_free(&s);
		return cp_fail(code);
	}
	cp_c_stroke_head(&c, &s);
	CP_CALL(append_path)(c.cr, (const cp_path_t *)path);
	CP_CALL(stroke)(c.cr);
	cp_c_stroke_free(&s);
	CP_PRIM_DONE(StrokePath);
	return cp_return(paint);
}
