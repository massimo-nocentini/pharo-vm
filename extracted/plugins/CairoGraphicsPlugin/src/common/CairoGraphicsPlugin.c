
static char __buildInfo[] = "CairoGraphicsPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pango/pangocairo.h>

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
primitive_pango_layout_set_text(void)
{
	cairo_t *cr = readAddress(interpreterProxy->stackValue(1));
	char *text = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));

	int length = interpreterProxy->stSizeOf(interpreterProxy->stackValue(0));

	pango_layout_set_text(cr, text, length);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_pango_layout_get_pixel_extents(void)
{
	PangoLayout *pango = readAddress(interpreterProxy->stackValue(0));

	PangoRectangle ink, logical;

	pango_layout_get_pixel_extents(pango, &ink, &logical);

	sqInt qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
	interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->integerObjectOf(logical.width));
	interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->integerObjectOf(logical.height));

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, qPoint);
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
