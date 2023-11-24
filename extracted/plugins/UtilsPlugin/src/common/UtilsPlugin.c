
static char __buildInfo[] = "UtilsPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <lua.h>
#include <lualib.h>
#include <lauxlib.h>

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

#include "UtilsPlugin.h"

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
	"UtilsPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"UtilsPlugin VMMaker.oscog-eem.2495 (e)"
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
primitive_decasteljau(void)
{
	sqInt designpoints = interpreterProxy->stackValue(2);
	sqInt controlpoints = interpreterProxy->stackValue(1);
	sqInt parameterDomain = interpreterProxy->stackValue(0);

	sqInt qPoint, hPoint;

	sqInt designpoints_length = interpreterProxy->stSizeOf(designpoints);
	sqInt domain_length = interpreterProxy->stSizeOf(parameterDomain);

	// printf("design point size %d, parameter domain size %d\n", designpoints_length, domain_length);

	//  sqInt aSequenceableOfPoints = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), designpoints_length);

	double h, u, t, x, y, h1, qx, qy;

	for (int j = 1; j <= domain_length; j++)
	{
		// memcpy(interpreterProxy->firstIndexableField(aSequenceableOfPoints),
		// 	   interpreterProxy->firstIndexableField(designpoints),
		// 	   sizeof(sqInt) * designpoints_length);

		t = interpreterProxy->floatValueOf(interpreterProxy->stObjectat(parameterDomain, j));

		h = 1.0;
		u = h - t;

		qPoint = interpreterProxy->stObjectat(designpoints, 1);
		qx = interpreterProxy->fetchFloatofObject(0, qPoint);
		qy = interpreterProxy->fetchFloatofObject(1, qPoint);

		// printf("design point (%f, %f) for param %f (%dith)\n", qx, qy, t, j);

		for (int k = 1; k < designpoints_length; k++)
		{
			h *= fma(t, (double)designpoints_length, (-t) * (double)k);
			h /= fma(u, k, h);

			hPoint = interpreterProxy->stObjectat(designpoints, k + 1);

			x = interpreterProxy->fetchFloatofObject(0, hPoint) * h;
			y = interpreterProxy->fetchFloatofObject(1, hPoint) * h;

			h1 = 1.0 - h;
			qx = fma(qx, h1, x);
			qy = fma(qy, h1, y);

			// interpreterProxy->storePointerofObjectwithValue(0, interpreterProxy->floatObjectOf(x), qPoint);
			// interpreterProxy->storePointerofObjectwithValue(1, interpreterProxy->floatObjectOf(y), qPoint);

			// qPoint = interpreterProxy->makePointwithxValueyValue(interpreterProxy->floatObjectOf(x), interpreterProxy->floatObjectOf(y));
			// printf("intermediate (%f, %f) for param %f (%dith)\n", qx, qy, t, j);
		}

		qPoint = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classPoint(), 0);
		interpreterProxy->storePointerofObjectwithValue(0, qPoint, interpreterProxy->floatObjectOf(qx));
		interpreterProxy->storePointerofObjectwithValue(1, qPoint, interpreterProxy->floatObjectOf(qy));

		interpreterProxy->stObjectatput(controlpoints, j, qPoint);

		// printf("finished (%f, %f) for param %f (%dith)\n", qx, qy, t, j);
	}

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
	}

	// printf("finished\n");

	return null;
}

EXPORT(sqInt)
primitive_strtok_r(void)
{
	sqInt oop;
	int length;

	char *orig = (char *)(interpreterProxy->firstIndexableField(interpreterProxy->stackValue(2))); // the receiver, indeed.
	char *delimiters = (char *)(interpreterProxy->firstIndexableField(interpreterProxy->stackValue(1)));
	sqInt include_empty_lines = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	length = strlen(orig);
	char *str = (char *)malloc(sizeof(char) * length + 1);

	char *del = strcpy(str, orig); // take a reference to the start of `str` in order to free it later; btw, `str` will be moved forward.

	// auxiliary pointers for tokens and followers.
	char *pch = NULL;
	char *ptr = str;

	int lines = 0;

	sqInt *buffer = (sqInt *)malloc(sizeof(sqInt) * length);

	while ((pch = strtok_r(str, delimiters, &str)) != NULL)
	{
		while (include_empty_lines && ptr != pch)
		{
			oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), 0);
			// strcpy(interpreterProxy->firstIndexableField(oop), "");
			*((char *)(interpreterProxy->firstIndexableField(oop))) = '\0';
			buffer[lines] = oop;

			lines++;
			ptr += 1;
		}

		length = strlen(pch);
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), length);
		strcpy(interpreterProxy->firstIndexableField(oop), pch);
		buffer[lines] = oop;

		lines++;
		ptr = str;
	}

	while (include_empty_lines && *ptr != '\0')
	{
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), 0);
		// strcpy(interpreterProxy->firstIndexableField(oop), "");
		*((char *)(interpreterProxy->firstIndexableField(oop))) = '\0';
		buffer[lines] = oop;

		lines++;
		ptr += 1;
	}

	if (lines == 1)
	{
		length = strlen(str);
		oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), length);
		strcpy(interpreterProxy->firstIndexableField(oop), str);
		buffer[lines] = oop;
		lines++;
	}

	oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), lines);

	memcpy(interpreterProxy->firstIndexableField(oop), buffer, sizeof(sqInt) * lines);

	free(del);
	free(buffer);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, oop);
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
