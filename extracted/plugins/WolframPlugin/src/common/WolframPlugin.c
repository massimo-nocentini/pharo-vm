
static char __buildInfo[] = "WolframPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

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

#include "WolframPlugin.h"
#include "wstp.h"

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
	"WolframPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"WolframPlugin VMMaker.oscog-eem.2495 (e)"
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

sqInt checked_stringForCString(const char *cString)
{
	return interpreterProxy->stringForCString(cString == NULL ? "" : cString);
}

EXPORT(sqInt)
primitive_WSInitialize(void)
{
	WSEnvironment env = WSInitialize(NULL);

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, env);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_WSDeinitialize(void)
{
	WSEnvironment env = (WSEnvironment)readAddress(interpreterProxy->stackValue(0));

	WSDeinitialize(env);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_WSOpenString(void)
{
	int errp, should_free;

	WSEnvironment env = (WSEnvironment)readAddress(interpreterProxy->stackValue(1));
	char *command_line = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	WSLINK link = WSOpenString(env, command_line, &errp);

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, link);

	if (should_free)
	{
		free(command_line);
	}

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, anExternalAddress);
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
