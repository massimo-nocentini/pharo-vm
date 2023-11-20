
static char __buildInfo[] = "LuaPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <lua.h>
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

#include "LuaPlugin.h"

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
	"LuaPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"LuaPlugin VMMaker.oscog-eem.2495 (e)"
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
primitive_luaL_newstate(void)
{
	lua_State *L = luaL_newstate();

	sqInt externalAddress = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classExternalAddress(), sizeof(void *));

	*((void **)(interpreterProxy->firstIndexableField(externalAddress))) = L;

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, externalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushstring(void)
{
	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	char *str = interpreterProxy->cStringOrNullFor(interpreterProxy->stackValue(0));

	lua_pushstring(L, str);

	free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushinteger(void)
{
	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));

	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	lua_pushinteger(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tostring(void)
{
	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));

	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	size_t l;
	const char *str = lua_tolstring(L, sq_value, &l);

	sqInt oop = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), l);

	strncpy(interpreterProxy->firstIndexableField(oop), str, l);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, oop);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pop(void)
{
	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));

	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	lua_pop(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushvalue(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	lua_pushvalue(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pcall(void)
{
	sqInt n, r, f;

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(3)));
	n = interpreterProxy->stackIntegerValue(2);
	r = interpreterProxy->stackIntegerValue(1);
	f = interpreterProxy->stackIntegerValue(0);

	int code = lua_pcall(L, n, r, f);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(5);
		interpreterProxy->pushInteger(code);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushboolean(void)
{
	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	sqInt b = interpreterProxy->booleanValueOf(interpreterProxy->stackValue(0));

	lua_pushboolean(L, b);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_compare(void)
{
	sqInt a, b, op;

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(3)));
	a = interpreterProxy->stackIntegerValue(2);
	b = interpreterProxy->stackIntegerValue(1);
	op = interpreterProxy->stackIntegerValue(0);

	int cmp = lua_compare(L, a, b, op);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(5);
		interpreterProxy->pushInteger(cmp);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_loadstring(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	char *str = interpreterProxy->cStringOrNullFor(interpreterProxy->stackValue(0));

	int ret = luaL_loadstring(L, str);

	free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(ret);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_dostring(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	char *str = interpreterProxy->cStringOrNullFor(interpreterProxy->stackValue(0));

	int ret = luaL_dostring(L, str);

	free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(ret);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_copy(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(2)));
	sqInt fromidx = interpreterProxy->stackIntegerValue(1);
	sqInt toidx = interpreterProxy->stackIntegerValue(0);

	lua_copy(L, fromidx, toidx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_getfield(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(2)));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	char *k = interpreterProxy->cStringOrNullFor(interpreterProxy->stackValue(0));

	int type = lua_getfield(L, idx, k);

	free(k);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4);
		interpreterProxy->pushInteger(type);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_geti(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(2)));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	sqInt i = interpreterProxy->stackIntegerValue(0);

	int type = lua_geti(L, idx, i);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4);
		interpreterProxy->pushInteger(type);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_getglobal(void)
{

	lua_State *L = (lua_State *)(readAddress(interpreterProxy->stackValue(1)));
	char *name = interpreterProxy->cStringOrNullFor(interpreterProxy->stackValue(0));

	int type = lua_getglobal(L, name);

	free(name);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(type);
	}

	return null;
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
