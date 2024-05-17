
static char __buildInfo[] = "TreeSitterPlugin VMMaker.oscog-eem.2495 uuid: fcbf4c90-4c50-4ff3-8690-0edfded4f9c4 " __DATE__;

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <tree_sitter/api.h>

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

#include "TreeSitterPlugin.h"

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
	"TreeSitterPlugin VMMaker.oscog-eem.2495 (i)"
#else
	"TreeSitterPlugin VMMaker.oscog-eem.2495 (e)"
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

TSLanguage *tree_sitter_c();

EXPORT(sqInt)
primitive_tree_sitter_c(void)
{

	TSLanguage *lang = tree_sitter_c();

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, lang);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_parser_new(void)
{

	TSParser *parser = ts_parser_new();

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, parser);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_parser_delete(void)
{

	TSParser *parser = readAddress(interpreterProxy->stackValue(0));

	ts_parser_delete(parser);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_parser_set_language(void)
{
	TSParser *parser = readAddress(interpreterProxy->stackValue(1));
	TSLanguage *language = readAddress(interpreterProxy->stackValue(0));

	bool assigned = ts_parser_set_language(parser, language);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(3);
		interpreterProxy->pushBool(assigned);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_parser_parse_string(void)
{

	TSParser *parser = readAddress(interpreterProxy->stackValue(1));
	char *source_code = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(0));
	sqInt length = interpreterProxy->stSizeOf(interpreterProxy->stackValue(0));

	TSTree *tree = ts_parser_parse_string(
		parser,
		NULL,
		source_code,
		length);

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, tree);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(3, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_tree_delete(void)
{

	TSTree *tree = readAddress(interpreterProxy->stackValue(0));

	ts_tree_delete(tree);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1); // just leave the receiver on the stack
	}

	return null;
}

sqInt walk(sqInt class, sqInt cumulatedArray, const char *src, const TSLanguage *lang, TSNode node, int node_pos, const char *field_name)
{
	int fieldIndex = 0;

	sqInt oop = interpreterProxy->instantiateClassindexableSize(class, 0);

	const char *type = ts_node_type(node);
	TSSymbol symbol_id = ts_node_symbol(node);
	TSPoint start_point = ts_node_start_point(node);
	TSPoint end_point = ts_node_end_point(node);
	const char *symbol_name = ts_language_symbol_name(lang, symbol_id);
	int start = interpreterProxy->integerValueOf(interpreterProxy->stObjectat(cumulatedArray, start_point.row + 1)) + start_point.column;
	int end = interpreterProxy->integerValueOf(interpreterProxy->stObjectat(cumulatedArray, end_point.row + 1)) + end_point.column;
	int n = end - start;

	sqInt oopContentString = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classString(), n);
	memcpy(interpreterProxy->firstIndexableField(oopContentString), src + start, n);

	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, checked_stringForCString(field_name));
	interpreterProxy->storeIntegerofObjectwithValue(fieldIndex++, oop, node_pos);
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, checked_stringForCString(type));
	interpreterProxy->storeIntegerofObjectwithValue(fieldIndex++, oop, symbol_id);
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, checked_stringForCString(symbol_name));
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, interpreterProxy->makePointwithxValueyValue(start + 1, end));
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, oopContentString);
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, interpreterProxy->makePointwithxValueyValue(start_point.column + 1, start_point.row + 1));
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, interpreterProxy->makePointwithxValueyValue(end_point.column, end_point.row + 1));
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, ts_node_is_null(node) ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, ts_node_is_named(node) ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, ts_node_is_missing(node) ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, ts_node_is_extra(node) ? interpreterProxy->trueObject() : interpreterProxy->falseObject());
	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, ts_node_has_error(node) ? interpreterProxy->trueObject() : interpreterProxy->falseObject());

	uint32_t c = ts_node_child_count(node);

	sqInt oopChildren = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), c);

	TSNode child;
	for (int i = 0; i < c; i++)
	{
		child = ts_node_child(node, i);

		interpreterProxy->stObjectatput(oopChildren,
										i + 1,
										walk(class, cumulatedArray, src, lang, child, i + 1, ts_node_field_name_for_child(node, i)));
	}

	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, oopChildren);

	return oop;
}

EXPORT(sqInt)
primitive_ts_ast(void)
{

	TSTree *tree = readAddress(interpreterProxy->stackValue(3));
	char *src = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(2));
	sqInt cumulatedSizes = interpreterProxy->stackValue(1);
	sqInt class = interpreterProxy->stackValue(0);

	TSNode root_node = ts_tree_root_node(tree);
	const TSLanguage *lang = ts_tree_language(tree);

	sqInt w = walk(class, cumulatedSizes, src, lang, root_node, 1, NULL);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(5, w);
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
