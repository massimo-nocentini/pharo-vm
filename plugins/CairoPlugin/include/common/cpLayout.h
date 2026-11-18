/* cpLayout.h -- the layout table of the CairoPlugin (docs/CairoPlugin.md,
 * "The layout table"): the slot indices of the Athens, Color, Rectangle and
 * Point objects the primitives read, and the class constants of Color, that
 * the image computes by name (CairoPluginLayout of the package
 * VM-Plugins-Primitives-Cairo) and passes to primitiveBind.
 *
 * The table is an Array of 29 or 33 elements; position p (1-based, as in
 * Smalltalk) is CP_L_<NAME> = p - 1 here.  Positions 1-29 are SmallIntegers:
 * 0-based slot indices ((cls allInstVarNames indexOf: #name) - 1) or
 * constants.  Positions 30-33 (Amendment A1) are the four string classes the
 * text primitive takes (ByteString, ByteSymbol, WideString, WideSymbol),
 * compared by identity with the class of its string argument: a 29-element
 * table binds too, and then the text primitive fails (the image keeps the
 * FFI path for text).
 *
 * primitiveBind checks: size 29 or 33; element 1 = CP_LAYOUT_VERSION;
 * elements 2-24 and 29 in 0..CP_LAYOUT_SLOT_MAX; 25-26 in
 * 0..CP_LAYOUT_SHIFT_MAX; 27-28 in 1..CP_LAYOUT_CONSTANT_MAX; 30-31 classes
 * of byte instances (instSpec 16-23), 32-33 classes of 32-bit word instances
 * (instSpec 10-11); else PrimErrBadIndex. */

#ifndef CP_LAYOUT_H
#define CP_LAYOUT_H

enum {
	CP_L_LAYOUT_VERSION = 0,	/*  1 constant 1 */
	CP_L_CANVAS_HANDLE,		/*  2 AthensCairoCanvas #handle (6) */
	CP_L_CANVAS_PATH_TRANSFORM,	/*  3 AthensCairoCanvas #pathTransform (7) */
	CP_L_CANVAS_PAINT,		/*  4 AthensCairoCanvas #paint (0) */
	CP_L_CANVAS_SURFACE,		/*  5 AthensCairoCanvas #surface (1) */
	CP_L_SURFACE_HANDLE,		/*  6 AthensCairoSurface #handle (1) */
	CP_L_BUILDER_CONTEXT,		/*  7 AthensCairoPathBuilder #context (0) */
	CP_L_EXTOBJ_HANDLE,		/*  8 ExternalObject #handle (0) */
	CP_L_PAINTMODE_CANVAS,		/*  9 AthensCairoPaintMode #canvas (0) */
	CP_L_STROKE_FILL_PAINT,		/* 10 AthensCairoStrokePaint #fillPaint (0) */
	CP_L_STROKE_WIDTH,		/* 11 #width (1) */
	CP_L_STROKE_CAP,		/* 12 #capStyle (3) */
	CP_L_STROKE_JOIN,		/* 13 #joinStyle (4) */
	CP_L_STROKE_DASHES,		/* 14 #dashLenghts (sic) (5) */
	CP_L_STROKE_DASH_OFFSET,	/* 15 #dashOffset (6) */
	CP_L_RENDERER_FONT,		/* 16 CairoFreetypeFontRenderer #font (0) */
	CP_L_RENDERER_CANVAS,		/* 17 #canvas (2) */
	CP_L_RENDERER_GLYPH_EXTENTS,	/* 18 #glyphExtents (6) */
	CP_L_COLOR_RGB,			/* 19 Color #rgb (0) */
	CP_L_COLOR_ALPHA,		/* 20 Color #alpha (3) */
	CP_L_RECT_ORIGIN,		/* 21 Rectangle #origin (0) */
	CP_L_RECT_CORNER,		/* 22 Rectangle #corner (1) */
	CP_L_POINT_X,			/* 23 Point #x (0) */
	CP_L_POINT_Y,			/* 24 Point #y (1) */
	CP_L_COLOR_RED_SHIFT,		/* 25 Color classPool at: #RedShift (20) */
	CP_L_COLOR_GREEN_SHIFT,		/* 26 #GreenShift (10) */
	CP_L_COLOR_COMPONENT_MASK,	/* 27 #ComponentMask (1023) */
	CP_L_COLOR_ALPHA_MAX,		/* 28 constant 255 */
	CP_L_SURFACE_CURRENT_CANVAS,	/* 29 AthensCairoSurface #currentCanvas (0) */
	/* Amendment A1: the classes of the text primitive's string */
	CP_L_CLASS_BYTE_STRING,		/* 30 ByteString */
	CP_L_CLASS_BYTE_SYMBOL,		/* 31 ByteSymbol */
	CP_L_CLASS_WIDE_STRING,		/* 32 WideString */
	CP_L_CLASS_WIDE_SYMBOL,		/* 33 WideSymbol */
	CP_L_COUNT
};

#define CP_LAYOUT_VERSION 1
#define CP_LAYOUT_INTEGERS 29		/* the SmallInteger positions 1-29 */
#define CP_LAYOUT_SIZE_BASE 29		/* a table without the string classes */
#define CP_LAYOUT_SIZE_TEXT 33		/* with them */
#define CP_LAYOUT_STRING_CLASSES 4
#define CP_LAYOUT_SLOT_MAX 254
#define CP_LAYOUT_SHIFT_MAX 62
#define CP_LAYOUT_CONSTANT_MAX (1L << 30)

#endif /* CP_LAYOUT_H */
