
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
primitive_WSClose(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(0)));

	WSClose(link);

	return null;
}

EXPORT(sqInt)
primitive_WSActivate(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->stackValue(0));

	int r = WSActivate(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSFlush(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->stackValue(0));

	int r = WSFlush(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSEndPacket(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->stackValue(0));

	int r = WSEndPacket(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSNewPacket(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(0)));

	int r = WSNewPacket(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSNextPacket(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(0)));

	int r = WSNextPacket(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSLinkName(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->stackValue(0));

	const char *name = WSLinkName(link);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, checked_stringForCString(name));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutRealNumberAsUTF8String(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(1)));
	sqInt oop = interpreterProxy->stackValue(0);
	int nbytes = interpreterProxy->stSizeOf(oop);

	int r = WSPutRealNumberAsUTF8String(link, interpreterProxy->firstIndexableField(oop), nbytes);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutReal(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(1)));
	double d = interpreterProxy->stackFloatValue(0);

	int r = WSPutReal(link, d);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutInteger32(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(1)));
	int d = interpreterProxy->stackIntegerValue(0);

	int r = WSPutInteger32(link, d);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutByteString(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(1)));
	sqInt oop = interpreterProxy->stackValue(0);
	int nbytes = interpreterProxy->stSizeOf(oop);

	int r = WSPutByteString(link, interpreterProxy->firstIndexableField(oop), nbytes);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutByteSymbol(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(1)));
	sqInt oop = interpreterProxy->stackValue(0);
	int nbytes = interpreterProxy->stSizeOf(oop);

	int r = WSPutByteSymbol(link, interpreterProxy->firstIndexableField(oop), nbytes);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(2, interpreterProxy->integerObjectOf(r));
	}

	return null;
}

EXPORT(sqInt)
primitive_WSPutUTF8Function(void)
{
	WSLINK link = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, interpreterProxy->stackValue(2)));
	sqInt s = interpreterProxy->stackValue(1);
	int argc = interpreterProxy->stackIntegerValue(0);

	int nbytes = interpreterProxy->stSizeOf(s);

	int r = WSPutUTF8Function(link, interpreterProxy->firstIndexableField(s), nbytes, argc);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->integerObjectOf(r));
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

sqInt rec(WSLINK lp, sqInt oopLink, sqInt exprClass, sqInt exprSymbolClass, sqInt exprIntegerClass, sqInt exprRealClass)
{
	char *error_msg = NULL;
	const char *string;
	const char *theNumber;
	int bytes;
	int characters;
	int n;
	int rawType;
	wslong32 i;
	double r;

	sqInt oop, args;

	switch (WSGetNext(lp))
	{
	case WSTKFUNC:

		if (!WSGetUTF8Function(lp, &string, &bytes, &n))
		{
			error_msg = "unable to read the function from lp";
			return interpreterProxy->primitiveFail();
		}

		oop = interpreterProxy->instantiateClassindexableSize(exprClass, 0);

		interpreterProxy->storePointerofObjectwithValue(0, oop, checked_stringForCString(string));
		WSReleaseUTF8Symbol(lp, string, bytes);

		args = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), n);

		for (int j = 1; j <= n; j++)
		{
			interpreterProxy->stObjectatput(args, j, rec(lp, oopLink, exprClass, exprSymbolClass, exprIntegerClass, exprRealClass));
		}

		interpreterProxy->storePointerofObjectwithValue(1, oop, args);
		interpreterProxy->storePointerofObjectwithValue(2, oop, oopLink);
		interpreterProxy->storePointerofObjectwithValue(3, oop, interpreterProxy->falseObject());

		break;

	case WSTKSTR:

		if (!WSGetUTF8String(lp, &string, &bytes, &characters))
		{
			error_msg = "unable to read the UTF-8 string from lp";
			return interpreterProxy->primitiveFail();
		}

		oop = checked_stringForCString(string);

		WSReleaseUTF8String(lp, string, bytes);

		break;

	case WSTKSYM:

		if (!WSGetUTF8Symbol(lp, &string, &bytes, &characters))
		{
			error_msg = "unable to read the UTF-8 symbol from lp";
			return interpreterProxy->primitiveFail();
		}

		oop = interpreterProxy->instantiateClassindexableSize(exprSymbolClass, 0);

		interpreterProxy->storePointerofObjectwithValue(0, oop, checked_stringForCString("Symbol"));

		args = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 1);
		interpreterProxy->stObjectatput(args, 1, checked_stringForCString(string));

		interpreterProxy->storePointerofObjectwithValue(1, oop, args);
		interpreterProxy->storePointerofObjectwithValue(2, oop, oopLink);
		interpreterProxy->storePointerofObjectwithValue(3, oop, interpreterProxy->falseObject());

		WSReleaseUTF8Symbol(lp, string, bytes);

		break;

	case WSTKINT:

		rawType = WSGetRawType(lp);

		if (rawType == WSTK_WSSHORT || rawType == WSTK_WSINT || rawType == WSTK_WSSIZE_T || rawType == WSTK_WSLONG || rawType == WSTK_WSINT64)
		{
			if (!WSGetInteger32(lp, &i))
			{
				error_msg = "unable to read the long from lp";
				return interpreterProxy->primitiveFail();
			}

			oop = interpreterProxy->integerObjectOf(i);
		}
		else
		{

			WSGetNumberAsString(lp, &theNumber);

			oop = interpreterProxy->instantiateClassindexableSize(exprIntegerClass, 0);

			interpreterProxy->storePointerofObjectwithValue(0, oop, checked_stringForCString("Integer"));

			args = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 1);
			interpreterProxy->stObjectatput(args, 1, checked_stringForCString(theNumber));

			interpreterProxy->storePointerofObjectwithValue(1, oop, args);
			interpreterProxy->storePointerofObjectwithValue(2, oop, oopLink);
			interpreterProxy->storePointerofObjectwithValue(3, oop, interpreterProxy->falseObject());

			WSReleaseString(lp, theNumber);
		}

		break;

	case WSTKREAL:

		rawType = WSGetRawType(lp);

		if (rawType == WSTK_WSFLOAT || rawType == WSTK_WSDOUBLE)
		{
			if (!WSGetReal64(lp, &r))
			{
				error_msg = "unable to read the real from lp";
				return interpreterProxy->primitiveFail();
			}

			oop = interpreterProxy->floatObjectOf(r);
		}
		else
		{

			WSGetNumberAsString(lp, &theNumber);

			oop = interpreterProxy->instantiateClassindexableSize(exprRealClass, 0);

			interpreterProxy->storePointerofObjectwithValue(0, oop, checked_stringForCString("Real"));

			args = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), 1);
			interpreterProxy->stObjectatput(args, 1, checked_stringForCString(theNumber));
		
			interpreterProxy->storePointerofObjectwithValue(1, oop, args);
			interpreterProxy->storePointerofObjectwithValue(2, oop, oopLink);
			interpreterProxy->storePointerofObjectwithValue(3, oop, interpreterProxy->falseObject());

			WSReleaseString(lp, theNumber);
		}

		break;

	default:
		error_msg = "Unhandled value of type id %d.", WSGetType(lp);
		return interpreterProxy->primitiveFail();
	}

	return oop;
}

EXPORT(sqInt)
primitive_read_from_link(void)
{
	sqInt oopLink = interpreterProxy->stackValue(4);
	sqInt exprClass = interpreterProxy->stackValue(3);
	sqInt exprSymbolClass = interpreterProxy->stackValue(2);
	sqInt exprIntegerClass = interpreterProxy->stackValue(1);
	sqInt exprRealClass = interpreterProxy->stackValue(0);

	WSLINK lp = (WSLINK)readAddress(interpreterProxy->fetchPointerofObject(0, oopLink));

	int pkt, err;
	int code, param;

	const unsigned char *string;
	int bytes;
	int characters;

	char *error_msg = NULL;

	while ((pkt = WSNextPacket(lp), pkt) && pkt != RETURNPKT)
	{
		switch (pkt)
		{
		case MESSAGEPKT:
			if (!WSGetMessage(lp, &code, &param))
			{
				error_msg = "unable to read the message code from lp";
			}
			printf("Got message code %d with param %d\n", code, param);
			break;
		case TEXTPKT:

			if (!WSGetUTF8String(lp, &string, &bytes, &characters))
			{
				error_msg = "unable to read the UTF-8 string from lp";
			}

			printf("Got the text: %s\n", string);

			WSReleaseUTF8String(lp, string, bytes);
			break;
		default:
			printf("Got packet of type %d.\n", pkt);
			break;
		}

		WSNewPacket(lp);

		if (err = WSError(lp), err)
		{
			error_msg = err;
			break;
		}
	}

	sqInt oop = error_msg != NULL ? oop = interpreterProxy->stringForCString(error_msg) : rec(lp, oopLink, exprClass, exprSymbolClass, exprIntegerClass, exprRealClass);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(5, oop);
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
