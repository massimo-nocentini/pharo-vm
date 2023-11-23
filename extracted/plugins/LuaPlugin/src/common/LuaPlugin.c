
static char __buildInfo[] = "LuaPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

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

lua_State *lua_StateFor(sqInt oop)
{

	return (lua_State *)(readAddress(oop));
}

char *checked_cStringOrNullFor(sqInt oop, int *should_free)
{
	char *str = (char *)interpreterProxy->firstIndexableField(oop);
	sqInt size = interpreterProxy->stSizeOf(oop);

	*should_free = str[size] != '\0';

	return *should_free ? interpreterProxy->cStringOrNullFor(oop) : str;
}

EXPORT(sqInt)
primitive_luaL_newstate(void)
{
	lua_State *L = luaL_newstate();

	sqInt externalAddress = newExternalAddress();

	writeAddress(externalAddress, L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, externalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushstring(void)
{
	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int should_free;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	lua_pushstring(L, str);

	if (should_free)
		free(str);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushinteger(void)
{
	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));

	lua_Integer sq_value = interpreterProxy->signed64BitValueOf(interpreterProxy->stackValue(0));

	lua_pushinteger(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tostring(void)
{
	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));

	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	const char *str = lua_tostring(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->stringForCString(str));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pop(void)
{
	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));

	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	lua_pop(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushvalue(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt sq_value = interpreterProxy->stackIntegerValue(0);

	lua_pushvalue(L, sq_value);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pcall(void)
{
	sqInt n, r, f;

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(3));
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
	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt b = interpreterProxy->stackIntegerValue(0);

	lua_pushboolean(L, b);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_compare(void)
{
	sqInt a, b, op;

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(3));
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int should_free;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	int ret = luaL_loadstring(L, str);

	if (should_free)
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int should_free;
	char *str = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	int ret = luaL_dostring(L, str);

	if (should_free)
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	int should_free;
	char *k = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	int type = lua_getfield(L, idx, k);

	if (should_free)
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
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

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int should_free;
	char *name = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	int type = lua_getglobal(L, name);

	if (should_free)
		free(name);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(type);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_gettable(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int type = lua_gettable(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(type);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_isinteger(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int is = lua_isinteger(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(is);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_len(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	lua_len(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // leave the receiver on the stack.
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_next(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int is = lua_next(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(is);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushcclosure(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	lua_CFunction fn = (lua_CFunction)readAddress(interpreterProxy->stackValue(1));
	sqInt n = interpreterProxy->stackIntegerValue(0);

	lua_pushcclosure(L, fn, n);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushcfunction(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	lua_CFunction fn = (lua_CFunction)readAddress(interpreterProxy->stackValue(0));

	lua_pushcfunction(L, fn);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushnumber(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	lua_Number n = interpreterProxy->stackFloatValue(0);

	lua_pushnumber(L, n);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_register(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	int should_free;
	char *name = checked_cStringOrNullFor(interpreterProxy->stackValue(1), &should_free);

	lua_CFunction fn = (lua_CFunction)readAddress(interpreterProxy->stackValue(0));

	lua_register(L, name, fn);

	if (should_free)
		free(name);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_setfield(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	int should_free;
	char *k = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	lua_setfield(L, idx, k);

	if (should_free)
		free(k);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_seti(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	lua_Integer i = interpreterProxy->signed64BitValueOf(interpreterProxy->stackValue(0));

	lua_seti(L, idx, i);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_setglobal(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int should_free;
	char *name = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &should_free);

	lua_setglobal(L, name);

	if (should_free)
		free(name);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tointegerx(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);

	int *isnum = (int *)interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));

	lua_Integer value = lua_tointegerx(L, idx, isnum);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, interpreterProxy->signed64BitIntegerFor(value));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tointeger(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	lua_Integer value = lua_tointeger(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->signed64BitIntegerFor(value));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_toboolean(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int value = lua_toboolean(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(value);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tolstring(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	size_t *l = (size_t *)interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));

	const char *str = lua_tolstring(L, idx, l);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, interpreterProxy->stringForCString(str));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tonumber(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	lua_Number value = lua_tonumber(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->floatObjectOf(value));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_tonumberx(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);

	int *isnum = (int *)interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));

	lua_Number value = lua_tonumberx(L, idx, isnum);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, interpreterProxy->floatObjectOf(value));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_type(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int type = lua_type(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(type);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_typename(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	const char *name = lua_typename(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->stringForCString(name));
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_openlibs(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	luaL_openlibs(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_close(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	lua_close(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_typename(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	const char *name = luaL_typename(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, interpreterProxy->stringForCString(name));
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_absindex(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int j = lua_absindex(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(j);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_loadfilex(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	int freename;
	char *name = checked_cStringOrNullFor(interpreterProxy->stackValue(1), &freename);

	int freemode;
	char *mode = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &freemode);

	int retcode = luaL_loadfilex(L, name, mode);

	if (freename)
		free(name);

	if (freemode)
		free(mode);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4);
		interpreterProxy->pushInteger(retcode);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_loadfile(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	int freename;
	char *name = checked_cStringOrNullFor(interpreterProxy->stackValue(0), &freename);

	int retcode = luaL_loadfile(L, name);

	if (freename)
		free(name);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(retcode);
	}

	return null;
}

EXPORT(sqInt)
primitive_luaL_requiref(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(3));
	int freename;
	char *modname = checked_cStringOrNullFor(interpreterProxy->stackValue(2), &freename);
	lua_CFunction f = readAddress(interpreterProxy->stackValue(1));
	int glb = interpreterProxy->stackIntegerValue(0);

	luaL_requiref(L, modname, f, glb);

	if (freename)
		free(modname);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(4); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_createtable(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt narr = interpreterProxy->stackIntegerValue(1);
	sqInt nrec = interpreterProxy->stackIntegerValue(0);

	lua_createtable(L, narr, nrec);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_error(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	int useless = lua_error(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
		interpreterProxy->pushInteger(useless);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_gettop(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	int top = lua_gettop(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2);
		interpreterProxy->pushInteger(top);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_isnil(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	int is = lua_isnil(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushInteger(is);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_newtable(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	lua_newtable(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushnil(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(0));

	lua_pushnil(L);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_remove(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	lua_remove(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_rotate(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(2));
	sqInt idx = interpreterProxy->stackIntegerValue(1);
	sqInt n = interpreterProxy->stackIntegerValue(0);

	lua_rotate(L, idx, n);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_settop(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	lua_settop(L, idx);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_topointer(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	void *ptr = lua_topointer(L, idx);

	sqInt externalAddress = newExternalAddress();

	writeAddress(externalAddress, ptr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, externalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_touserdata(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	sqInt idx = interpreterProxy->stackIntegerValue(0);

	void *ptr = lua_touserdata(L, idx);

	sqInt externalAddress = newExternalAddress();

	writeAddress(externalAddress, ptr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, externalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pushlightuserdata(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(1));
	void *ptr = readAddress(interpreterProxy->stackValue(0));

	lua_pushlightuserdata(L, ptr);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(2); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_lua_pcallk(void)
{

	lua_State *L = lua_StateFor(interpreterProxy->stackValue(5));
	sqInt nargs = interpreterProxy->stackIntegerValue(4);
	sqInt nresults = interpreterProxy->stackIntegerValue(3);
	sqInt msgh = interpreterProxy->stackIntegerValue(2);
	lua_KContext ctx = interpreterProxy->signedMachineIntegerValueOf(1);
	sqInt k = readAddress(interpreterProxy->stackValue(0));

	int retcode = lua_pcallk(L, nargs, nresults, msgh, ctx, k);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(7);
		interpreterProxy->pushInteger(retcode);
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
