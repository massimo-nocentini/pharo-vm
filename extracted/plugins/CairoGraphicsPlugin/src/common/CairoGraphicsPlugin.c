
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
