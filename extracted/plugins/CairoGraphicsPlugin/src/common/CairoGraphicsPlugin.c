
static char __buildInfo[] = "CairoGraphicsPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pango/pangocairo.h>
#include <cairo.h>

/* Default EXPORT macro that does nothing (see comment in sq.h): */
#define EXPORT(returnType) returnType

#include "pharovm/pharo.h"
// #include "sqConfig.h"			/* Configuration options */
// #include "sqVirtualMachine.h"	/*  The virtual machine proxy definition */
// #include "sqPlatformSpecific.h" /* Platform specific definitions */
// #include "sqMemoryAccess.h"

#define true 1
#define false 0
#define null 0 /* using 'null' because nil is predefined in Think C */
#ifdef SQUEAK_BUILTIN_PLUGIN
#undef EXPORT
#define EXPORT(returnType) static returnType
#endif

#include "CairoGraphicsPlugin.h"

/*** Function Prototypes ***/
EXPORT(const char *)
getModuleName(void);
EXPORT(sqInt)
initialiseModule(void);
// EXPORT(sqInt)
// primitiveCompileThenFormat(void);
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter);
static sqInt sqAssert(sqInt aBool);

struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"CairoGraphicsPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"CairoGraphicsPlugin VMMaker.oscog-eem.2495 (e)"
#endif
	;

/*	Note: This is hardcoded so it can be run from Squeak.
	The module name is used for validating a module *after*
	it is loaded to check if it does really contain the module
	we're thinking it contains. This is important! */

/* InterpreterPlugin>>#getModuleName */
EXPORT(const char *)
getModuleName(void)
{
	return moduleName;
}

EXPORT(sqInt)
initialiseModule(void)
{
	return true;
}

void writeAddress(sqInt anExternalAddress, void *value)
{
	if (!interpreterProxy->isKindOfClass(anExternalAddress, interpreterProxy->classExternalAddress()))
	{
		interpreterProxy->primitiveFail();
		return;
	}

	*((void **)interpreterProxy->firstIndexableField(anExternalAddress)) = value;
}

sqInt newExternalAddress()
{
	return interpreterProxy->instantiateClassindexableSize(interpreterProxy->classExternalAddress(), sizeof(void *));
}

char *checked_cStringOrNullFor(sqInt oop, int *should_free)
{
	char *str = (char *)interpreterProxy->firstIndexableField(oop);
	sqInt size = interpreterProxy->stSizeOf(oop);

	*should_free = str[size] != '\0';

	return *should_free ? interpreterProxy->cStringOrNullFor(oop) : str;
}

EXPORT(sqInt)
primitive_pango_cairo_create_layout(void)
{
	cairo_t *cr = readAddress(interpreterProxy->stackValue(0));

	PangoLayout *layout = pango_cairo_create_layout(cr);

	sqInt oop = newExternalAddress();

	writeAddress(oop, layout);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_cairo_show_layout(void)
{
	cairo_t *cr = readAddress(interpreterProxy->stackValue(1));
	PangoLayout *layout = readAddress(interpreterProxy->stackValue(0));

	pango_cairo_show_layout(cr, layout);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_set_text(void)
{
	PangoLayout *layout = readAddress(interpreterProxy->stackValue(1));
	char *text = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));

	int length = interpreterProxy->stSizeOf(interpreterProxy->stackValue(0));

	pango_layout_set_text(layout, text, length);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_set_markup(void)
{
	PangoLayout *layout = readAddress(interpreterProxy->stackValue(1));
	sqInt oop = interpreterProxy->stackValue(0);

	char *text = interpreterProxy->firstIndexableField(oop);

	int length = interpreterProxy->stSizeOf(oop);

	pango_layout_set_markup(layout, text, length);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_set_width(void)
{
	PangoLayout *layout = readAddress(interpreterProxy->stackValue(1));
	int width = interpreterProxy->stackIntegerValue(0);

	pango_layout_set_width(layout, width);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_g_object_unref(void)
{
	void *ptr = readAddress(interpreterProxy->stackValue(0));

	g_object_unref(ptr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_attr_list_new(void)
{

	PangoAttrList *attrs = pango_attr_list_new();

	sqInt oop = newExternalAddress();

	writeAddress(oop, attrs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_attr_list_from_string(void)
{
	int free_str;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &free_str);
	PangoAttrList *attrs = pango_attr_list_from_string(str);

	if (free_str)
		free(str);

	sqInt oop = newExternalAddress();

	writeAddress(oop, attrs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_image_surface_create(void)
{

	sqInt width = interpreterProxy->stackIntegerValue(1);
	sqInt height = interpreterProxy->stackIntegerValue(0);

	cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, width, height);

	sqInt oop = newExternalAddress();

	writeAddress(oop, surface);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_surface_destroy(void)
{
	cairo_surface_t *surface = readAddress(interpreterProxy->stackValue(0));

	cairo_surface_destroy(surface);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_create(void)
{
	cairo_surface_t *surface = readAddress(interpreterProxy->stackValue(0));

	cairo_t *cr = cairo_create(surface);

	sqInt oop = newExternalAddress();

	writeAddress(oop, cr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_destroy(void)
{
	cairo_t *cr = readAddress(interpreterProxy->stackValue(0));

	cairo_destroy(cr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_get_pixel_extents(void)
{

	PangoLayout *pango = readAddress(interpreterProxy->stackValue(0));
	// PangoLayout *pango = pango_cairo_create_layout(cr);

	PangoRectangle ink, logical;

	pango_layout_get_pixel_extents(pango, &ink, &logical);

	sqInt oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 2);

	sqInt qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
	interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->integerObjectOf(logical.x));
	interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->integerObjectOf(logical.y));

	interpreterProxy->stObjectatput(oop, 1, qPoint);

	qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
	interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->integerObjectOf(logical.width));
	interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->integerObjectOf(logical.height));

	interpreterProxy->stObjectatput(oop, 2, qPoint);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_get_pixel_size(void)
{

	PangoLayout *pango = readAddress(interpreterProxy->stackValue(0));

	int w, h;

	pango_layout_get_pixel_size(pango, &w, &h);

	sqInt qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
	interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->integerObjectOf(w));
	interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->integerObjectOf(h));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, qPoint);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_get_ink_pixel_size(void)
{

	PangoLayout *pango = readAddress(interpreterProxy->stackValue(0));

	PangoRectangle ink, logical;

	pango_layout_get_pixel_extents(pango, &ink, &logical);

	sqInt qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
	interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->integerObjectOf(logical.width));
	interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->integerObjectOf(ink.height + ((logical.height - ink.height) * 0.618)));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, qPoint);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_set_attributes(void)
{
	PangoLayout *layout = readAddress(interpreterProxy->stackValue(1));
	PangoAttrList *attrs = readAddress(interpreterProxy->stackValue(0));

	pango_layout_set_attributes(layout, attrs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_attr_list_unref(void)
{
	PangoAttrList *attrs = readAddress(interpreterProxy->stackValue(0));

	pango_attr_list_unref(attrs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_show_text(void)
{
	int free_str;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(1), &free_str);

	cairo_t *cr = readAddress(interpreterProxy->stackValue(0));

	cairo_show_text(cr, str);

	if (free_str)
		free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_text_path(void)
{
	cairo_t *cr = readAddress(interpreterProxy->stackValue(1));

	int free_str;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &free_str);

	cairo_text_path(cr, str);

	if (free_str)
		free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_scaled_font_extents(void)
{

	cairo_scaled_font_t *scaled_font = readAddress(interpreterProxy->stackValue(1));
	sqInt extentObject = interpreterProxy->stackValue(0);

	cairo_font_extents_t extents;
	cairo_scaled_font_extents(scaled_font, &extents);

	interpreterProxy->storePointerofObjectwithValue(1, extentObject, interpreterProxy->floatObjectOf(extents.ascent));
	interpreterProxy->storePointerofObjectwithValue(2, extentObject, interpreterProxy->floatObjectOf(extents.descent));
	interpreterProxy->storePointerofObjectwithValue(3, extentObject, interpreterProxy->floatObjectOf(0.0));
	interpreterProxy->storePointerofObjectwithValue(4, extentObject, interpreterProxy->floatObjectOf(extents.height));
	interpreterProxy->storePointerofObjectwithValue(5, extentObject, interpreterProxy->floatObjectOf(extents.max_x_advance));
	interpreterProxy->storePointerofObjectwithValue(6, extentObject, interpreterProxy->floatObjectOf(extents.max_y_advance));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_scaled_font_text_to_glyphs(void)
{

	char *strUtf8 = interpreterProxy->arrayValueOf(interpreterProxy->stackValue(6));
	int len = interpreterProxy->stackIntegerValue(5);
	cairo_scaled_font_t *scaled_font = readAddress(interpreterProxy->stackValue(4));
	sqInt glyphsExternalAddress = interpreterProxy->stackValue(3);
	int *numGlyphs = interpreterProxy->arrayValueOf(interpreterProxy->stackValue(2));
	double x = interpreterProxy->stackFloatValue(1);
	double y = interpreterProxy->stackFloatValue(0);

	cairo_glyph_t *glyphs = NULL;
	cairo_status_t status = cairo_scaled_font_text_to_glyphs(scaled_font, x, y, strUtf8, len, &glyphs, numGlyphs, NULL, NULL, NULL);

	writeAddress(glyphsExternalAddress, glyphs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(8, interpreterProxy->integerObjectOf(status));
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_memcpy_glyphs(void)
{

	const void *src = readAddress(interpreterProxy->stackValue(2));
	void *dest = interpreterProxy->arrayValueOf(interpreterProxy->stackValue(1));
	int n = interpreterProxy->stackIntegerValue(0);

	memcpy(dest, src, n);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_scaled_font_glyph_extents(void)
{

	cairo_scaled_font_t *scaled_font = readAddress(interpreterProxy->stackValue(3));
	cairo_glyph_t *glyphs = readAddress(interpreterProxy->stackValue(2));
	int n = interpreterProxy->stackIntegerValue(1);
	sqInt extentObject = interpreterProxy->stackValue(0);

	cairo_text_extents_t extents;

	cairo_scaled_font_glyph_extents(scaled_font, glyphs, n, &extents);

	interpreterProxy->storePointerofObjectwithValue(1, extentObject, interpreterProxy->floatObjectOf(extents.x_bearing));
	interpreterProxy->storePointerofObjectwithValue(2, extentObject, interpreterProxy->floatObjectOf(extents.y_bearing));
	interpreterProxy->storePointerofObjectwithValue(3, extentObject, interpreterProxy->floatObjectOf(extents.width));
	interpreterProxy->storePointerofObjectwithValue(4, extentObject, interpreterProxy->floatObjectOf(extents.height));
	interpreterProxy->storePointerofObjectwithValue(5, extentObject, interpreterProxy->floatObjectOf(extents.x_advance));
	interpreterProxy->storePointerofObjectwithValue(6, extentObject, interpreterProxy->floatObjectOf(extents.y_advance));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_scaled_font_glyph_extents_bytearray(void)
{

	cairo_scaled_font_t *scaled_font = readAddress(interpreterProxy->stackValue(3));
	cairo_glyph_t *glyphs = interpreterProxy->arrayValueOf(interpreterProxy->stackValue(2));
	int n = interpreterProxy->stackIntegerValue(1);
	sqInt extentObject = interpreterProxy->stackValue(0);

	cairo_text_extents_t extents;

	cairo_scaled_font_glyph_extents(scaled_font, glyphs, n, &extents);

	interpreterProxy->storePointerofObjectwithValue(1, extentObject, interpreterProxy->floatObjectOf(extents.x_bearing));
	interpreterProxy->storePointerofObjectwithValue(2, extentObject, interpreterProxy->floatObjectOf(extents.y_bearing));
	interpreterProxy->storePointerofObjectwithValue(3, extentObject, interpreterProxy->floatObjectOf(extents.width));
	interpreterProxy->storePointerofObjectwithValue(4, extentObject, interpreterProxy->floatObjectOf(extents.height));
	interpreterProxy->storePointerofObjectwithValue(5, extentObject, interpreterProxy->floatObjectOf(extents.x_advance));
	interpreterProxy->storePointerofObjectwithValue(6, extentObject, interpreterProxy->floatObjectOf(extents.y_advance));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_glyph_free(void)
{

	cairo_glyph_t *glyphs = readAddress(interpreterProxy->stackValue(0));

	cairo_glyph_free(glyphs);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_cairo_scaled_font_text_extents(void)
{

	cairo_scaled_font_t *scaled_font = readAddress(interpreterProxy->stackValue(2));
	int should_free;
	char *utf8 = checked_cStringOrNullFor(interpreterProxy->stackValue(1), &should_free);
	sqInt extentObject = interpreterProxy->stackValue(0);

	cairo_text_extents_t extents;

	cairo_scaled_font_text_extents(scaled_font, utf8, &extents);

	if (should_free)
		free(utf8);

	interpreterProxy->storePointerofObjectwithValue(1, extentObject, interpreterProxy->floatObjectOf(extents.x_bearing));
	interpreterProxy->storePointerofObjectwithValue(2, extentObject, interpreterProxy->floatObjectOf(extents.y_bearing));
	interpreterProxy->storePointerofObjectwithValue(3, extentObject, interpreterProxy->floatObjectOf(extents.width));
	interpreterProxy->storePointerofObjectwithValue(4, extentObject, interpreterProxy->floatObjectOf(extents.height));
	interpreterProxy->storePointerofObjectwithValue(5, extentObject, interpreterProxy->floatObjectOf(extents.x_advance));
	interpreterProxy->storePointerofObjectwithValue(6, extentObject, interpreterProxy->floatObjectOf(extents.y_advance));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_g_unichar_to_utf8(void)
{

	sqInt oop = interpreterProxy->stackValue(1); //	get the receiver.
	gunichar c = interpreterProxy->characterValueOf(oop);

	sqInt include_null_char = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	gchar buf[33];

	gint written = g_unichar_to_utf8(c, buf);

	if (include_null_char)
		buf[written++] = '\0';

	sqInt array = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), written);

	memcpy(interpreterProxy->arrayValueOf(array), buf, written);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, array);
	}

	return null;
}

EXPORT(sqInt)
primitive_string_to_utf8(void)
{

	sqInt oop = interpreterProxy->stackValue(2); //	get the receiver, which is a string.`
	sqInt include_null_char = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(1));
	sqInt allocateByteArray = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	sqInt size = interpreterProxy->stSizeOf(oop);

	char *final = malloc(32 * size + (include_null_char ? 1 : 0));
	int count = 0;

	gchar buf[32];
	gint written;

	sqInt c;

	for (int i = 1; i <= size; i++)
	{
		c = interpreterProxy->characterValueOf(interpreterProxy->stObjectat(oop, i));

		written = g_unichar_to_utf8(c, buf);

		memcpy(final + count, buf, written);
		count += written;
	}

	if (include_null_char)
		final[count++] = '\0';

	sqInt array;

	if (allocateByteArray)
	{
		array = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classByteArray(), count);
	}
	else
	{
		array = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), count);
	}

	memcpy(interpreterProxy->firstIndexableField(array), final, count);

	free(final);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, array);
	}

	return null;
}

EXPORT(sqInt)
primitive_g_markup_escape_text(void)
{

	sqInt oop = interpreterProxy->stackValue(0); //	get the receiver, which is a string.
	sqInt size = interpreterProxy->stSizeOf(oop);

	gchar *escaped = g_markup_escape_text(interpreterProxy->firstIndexableField(oop), size);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, interpreterProxy->stringForCString(escaped));
		g_free(escaped);
	}

	return null;
}

EXPORT(sqInt)
primitive_fix_empty_lines_for_pango(void)
{

	sqInt oop = interpreterProxy->stackValue(1); //	get the receiver, which is a string.
	char splitter = interpreterProxy->characterValueOf(interpreterProxy->stackValue(0));

	sqInt size = interpreterProxy->stSizeOf(oop);

	GString *str = g_string_sized_new((size << 1) | 1); // the max case is where we have a string full of newlines.

	char *orig = interpreterProxy->firstIndexableField(oop);

	for (int i = 0; i < size; i++)
	{
		if (orig[i] == splitter && (i > 0 ? orig[i - 1] == splitter : true))
			str = g_string_append_c(str, ' ');

		str = g_string_append_c(str, orig[i]);
	}

	if (size > 0 && orig[size - 1] == splitter)
		str = g_string_append_c(str, ' ');

	int len = str->len;

	gchar *steal = g_string_free(str, false);
	sqInt fixed = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), len);
	memcpy(interpreterProxy->firstIndexableField(fixed), steal, len);
	g_free(steal);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, fixed);
	}

	return null;
}

EXPORT(sqInt)
primitive_replace_tabs_with_spaces(void)
{

	sqInt oop = interpreterProxy->stackValue(1); //	get the receiver, which is a string.
	sqInt times = interpreterProxy->stackIntegerValue(0);

	sqInt size = interpreterProxy->stSizeOf(oop);

	GString *str = g_string_append_len(g_string_sized_new(size), interpreterProxy->firstIndexableField(oop), size);

	char rep[times + 1];
	for (int i = 0; i < times; i++)
		rep[i] = ' ';
	rep[times] = '\0';

	g_string_replace(str, "\t", rep, 0);

	int len = str->len;

	gchar *steal = g_string_free(str, false);
	sqInt fixed = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), len);
	memcpy(interpreterProxy->firstIndexableField(fixed), steal, len);
	g_free(steal);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, fixed);
	}

	return null;
}

/*	Note: This is coded so that it can be run in Squeak. */

/* InterpreterPlugin>>#setInterpreter: */
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter)
{
	sqInt ok;

	interpreterProxy = anInterpreter;
	ok = ((interpreterProxy->majorVersion()) == (VM_PROXY_MAJOR)) && ((interpreterProxy->minorVersion()) >= (VM_PROXY_MINOR));

	return ok;
}

/* SmartSyntaxInterpreterPlugin>>#sqAssert: */
static sqInt
sqAssert(sqInt aBool)
{
	/* missing DebugCode */;
	return aBool;
}
