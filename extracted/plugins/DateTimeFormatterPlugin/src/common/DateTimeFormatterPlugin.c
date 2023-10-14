
static char __buildInfo[] = "DateTimeFormatterPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include "config.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <datetimeformatter.h>

/* Default EXPORT macro that does nothing (see comment in sq.h): */
#define EXPORT(returnType) returnType

/* Do not include the entire sq.h file but just those parts needed. */
#include "sqConfig.h"			/* Configuration options */
#include "sqVirtualMachine.h"	/*  The virtual machine proxy definition */
#include "sqPlatformSpecific.h" /* Platform specific definitions */

#define true 1
#define false 0
#define null 0 /* using 'null' because nil is predefined in Think C */
#ifdef SQUEAK_BUILTIN_PLUGIN
#undef EXPORT
#define EXPORT(returnType) static returnType
#endif

#include "DateTimeFormatterPlugin.h"
#include "sqMemoryAccess.h"

/*** Function Prototypes ***/
EXPORT(const char *)
getModuleName(void);
EXPORT(sqInt)
initialiseModule(void);
EXPORT(sqInt)
primitiveCompileThenFormat(void);
EXPORT(sqInt)
setInterpreter(struct VirtualMachine *anInterpreter);
static sqInt sqAssert(sqInt aBool);

struct VirtualMachine *interpreterProxy;
static const char *moduleName =
#ifdef SQUEAK_BUILTIN_PLUGIN
	"DateTimeFormatterPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"DateTimeFormatterPlugin VMMaker.oscog-eem.2495 (e)"
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

EXPORT(sqInt)
primitiveCompileThenFormat(void)
{
	buffer_t *B;
	char strbuffer[1025];
	int failed = 0;

	sqInt sq_value;

	sq_value = interpreterProxy->stackValue(5);
	char *pattern = (char *)interpreterProxy->firstIndexableField(sq_value);

	sqInt timer = interpreterProxy->stackIntegerValue(4);

	sq_value = interpreterProxy->stackValue(3);
	char *locale = (char *)interpreterProxy->firstIndexableField(sq_value);

	sqInt offset = interpreterProxy->stackIntegerValue(2);

	sq_value = interpreterProxy->stackValue(1);
	char *timezone = (char *)interpreterProxy->firstIndexableField(sq_value);

	sqInt local = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	// printf("PREPARING COMPILATION: (pattern %s) (timer %d) (locale %s) (offset %d) (timezone %s) (local %d)\n",
	// 	   pattern, timer, locale, offset, timezone, local);

	failed = dtf_compile(pattern, &B, strbuffer);
	// printf("FAILED: %d\n", failed);
	assert(!failed);
	failed = dtf_format(B, timer, locale, offset, timezone, local, strbuffer);
	// printf("FAILED: %d\n", failed);
	assert(!failed);

	free_buffer(B);

	int l = strlen(strbuffer);
	sqInt oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), l);

	strncpy(interpreterProxy->firstIndexableField(oop), strbuffer, l);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(7, oop);
	}

	return null;
}

/*

primitiveCountry(void)
{
	sqInt oop, array, idx;

	char *buffer = "Hello from C";

	idx = 0;
	array = interpreterProxy->stackValue(0);
	// interpreterProxy->checkFailed();

	idx = 1;
	oop = interpreterProxy->stObjectat(array, 1);

	char *string = (char *)interpreterProxy->firstIndexableField(oop);
	// interpreterProxy->checkFailed();
	printf("RECEIVED %d: %s\n", interpreterProxy->stSizeOf(array), string);

	// oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), strlen(buffer));
	// sqLocGetCountryInto(firstIndexableField(oop));
	char *str = (char *)interpreterProxy->firstIndexableField(oop);

	// memcpy(str, buffer, strlen(buffer));
	if (!(interpreterProxy->failed()))
	{
		// printf("pushed\n");
		interpreterProxy->popthenPush(2, oop);
	}
	// printf("Completed\n");
	return null;
}

*/

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
