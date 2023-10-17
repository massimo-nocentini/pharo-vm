
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

/* Do not include the entire sq.h file but just those parts needed. */
#include "pharovm/pharo.h"
// #include "sqConfig.h"			/* Configuration options */
// #include "sqVirtualMachine.h"	/*  The virtual machine proxy definition */
// #include "sqPlatformSpecific.h" /* Platform specific definitions */



#define true 1
#define false 0
#define null 0 /* using 'null' because nil is predefined in Think C */
#ifdef SQUEAK_BUILTIN_PLUGIN
#undef EXPORT
#define EXPORT(returnType) static returnType
#endif

#include "LuaPlugin.h"
// #include "sqMemoryAccess.h"

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
	sqInt sq_value;

	sq_value = interpreterProxy->stackValue(1);
	lua_State *L = (lua_State *)(readAddress(sq_value));

	sq_value = interpreterProxy->stackValue(0);
	char *str = (char *)(interpreterProxy->firstIndexableField(sq_value));

	lua_pushstring(L, str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushinteger(void)
{
	sqInt sq_value;

	sq_value = interpreterProxy->stackValue(1);
	lua_State *L = (lua_State *)(readAddress(sq_value));

	sq_value = interpreterProxy->stackIntegerValue(0);

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
	sqInt sq_value;

	sq_value = interpreterProxy->stackValue(1);
	lua_State *L = (lua_State *)(readAddress(sq_value));

	sq_value = interpreterProxy->stackIntegerValue(0);

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
