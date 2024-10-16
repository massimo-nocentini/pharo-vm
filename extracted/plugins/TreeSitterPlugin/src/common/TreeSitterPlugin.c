
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
TSLanguage *tree_sitter_json();
TSLanguage *tree_sitter_javascript();
TSLanguage *tree_sitter_python();

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
primitive_tree_sitter_json(void)
{

	TSLanguage *lang = tree_sitter_json();

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, lang);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_tree_sitter_javascript(void)
{

	TSLanguage *lang = tree_sitter_javascript();

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, lang);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(1, anExternalAddress);
	}

	return null;
}

EXPORT(sqInt)
primitive_tree_sitter_python(void)
{

	TSLanguage *lang = tree_sitter_python();

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

	TSParser *parser = readAddress(interpreterProxy->stackValue(2));
	sqInt sourceString = interpreterProxy->stackValue(1);
	sqInt treeClass = interpreterProxy->stackValue(0);

	char *source_code = interpreterProxy->firstIndexableField(sourceString);
	sqInt length = interpreterProxy->stSizeOf(sourceString);

	TSTree *tree = ts_parser_parse_string_encoding(
		parser,
		NULL,
		source_code,
		length,
		TSInputEncodingUTF8);

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, tree);

	sqInt oop = interpreterProxy->instantiateClassindexableSize(treeClass, 0);
	interpreterProxy->storePointerofObjectwithValue(0, oop, sourceString);
	interpreterProxy->storePointerofObjectwithValue(1, oop, anExternalAddress);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, oop);
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

sqInt walk(sqInt class, const char *src, const TSLanguage *lang, TSNode node, int node_pos, const char *field_name)
{
	int fieldIndex = 0;

	sqInt oop = interpreterProxy->instantiateClassindexableSize(class, 0);

	const char *type = ts_node_type(node);
	TSSymbol symbol_id = ts_node_symbol(node);
	TSPoint start_point = ts_node_start_point(node);
	TSPoint end_point = ts_node_end_point(node);
	const char *symbol_name = ts_language_symbol_name(lang, symbol_id);
	// char *node_string = ts_node_string(node);
	int start = ts_node_start_byte(node);
	int end = ts_node_end_byte(node);
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
	// interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, checked_stringForCString(node_string));

	// free(node_string);

	uint32_t c = ts_node_child_count(node);

	sqInt oopChildren = interpreterProxy->instantiateClassindexableSize(interpreterProxy->classArray(), c);

	for (int i = 0; i < c; i++)
	{
		TSNode child = ts_node_child(node, i);

		interpreterProxy->stObjectatput(oopChildren,
										i + 1,
										walk(class, src, lang, child, i + 1, ts_node_field_name_for_child(node, i)));
	}

	interpreterProxy->storePointerofObjectwithValue(fieldIndex++, oop, oopChildren);

	return oop;
}

EXPORT(sqInt)
primitive_ts_ast(void)
{

	TSTree *tree = readAddress(interpreterProxy->stackValue(2));
	char *src = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(1));
	sqInt class = interpreterProxy->stackValue(0);

	TSNode root_node = ts_tree_root_node(tree);
	const TSLanguage *lang = ts_tree_language(tree);

	sqInt w = walk(class, src, lang, root_node, 1, NULL);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, w);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_query_new(void)
{
	sqInt oopLanguage = interpreterProxy->stackValue(3); // the receiver.
	TSLanguage *language = readAddress(interpreterProxy->stackValue(2));
	sqInt oopQueryString = interpreterProxy->stackValue(1);
	const char *query_source = interpreterProxy->firstIndexableField(oopQueryString);
	sqInt length = interpreterProxy->stSizeOf(oopQueryString);
	sqInt queryClass = interpreterProxy->stackValue(0);

	uint32_t error_offset;
	TSQueryError error_type;

	TSQuery *query = ts_query_new(
		language,
		query_source,
		length,
		&error_offset,
		&error_type);

	sqInt anExternalAddress = newExternalAddress();

	writeAddress(anExternalAddress, query);

	sqInt oopQuery = interpreterProxy->instantiateClassindexableSize(queryClass, 0);
	interpreterProxy->storeIntegerofObjectwithValue(0, oopQuery, error_offset + 1); // because Pharo uses 1-based indexes.
	interpreterProxy->storeIntegerofObjectwithValue(1, oopQuery, error_type);
	interpreterProxy->storePointerofObjectwithValue(2, oopQuery, oopLanguage);
	interpreterProxy->storePointerofObjectwithValue(3, oopQuery, oopQueryString);
	interpreterProxy->storePointerofObjectwithValue(4, oopQuery, anExternalAddress);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->popthenPush(4, oopQuery);
	}

	return null;
}

EXPORT(sqInt)
primitive_ts_query_delete(void)
{

	TSQuery *query = readAddress(interpreterProxy->stackValue(0));

	ts_query_delete(query);

	if (!(interpreterProxy->failed()))
	{
		interpreterProxy->pop(1);
	}

	return null;
}

typedef struct TS_link_s
{
	struct TS_link_s *next;
	sqInt oop;
	uint32_t size;
} TS_link_t;

TS_link_t *push_oop_on_link(sqInt oop, TS_link_t *link)
{
	TS_link_t *new_link = (TS_link_t *)malloc(sizeof(TS_link_t));
	new_link->next = link;
	new_link->oop = oop;
	new_link->size = link == NULL ? 0 : link->size + 1;
	return new_link;
}

void free_link(TS_link_t *link)
{
	if (link->size > 0)
		free_link(link->next);

	free(link);
}

EXPORT(sqInt)
primitive_ts_query_matches(void)
{
	TSQuery *query = readAddress(interpreterProxy->stackValue(3));
	TSTree *tree = readAddress(interpreterProxy->stackValue(2));
	char *sourceCode = interpreterProxy->firstIndexableField(interpreterProxy->stackValue(1));
	sqInt class = interpreterProxy->stackValue(0);

	TSQueryCursor *cursor = ts_query_cursor_new();

	ts_query_cursor_exec(cursor, query, ts_tree_root_node(tree));

	TSQueryMatch match;
	TSQueryCapture capture;
	TSNode captured_node;
	TSPoint point;

	uint32_t length, start_byte, end_byte, row, column, discreteTime = 1;

	TS_link_t *link = push_oop_on_link(interpreterProxy->nilObject(), NULL);

	while (ts_query_cursor_next_match(cursor, &match))
	{
		// const TSQueryPredicateStep *qps = ts_query_predicates_for_pattern(query, match.pattern_index, &length);

		// for (uint32_t i = 0; i < length; i++)
		// {
		// 	TSQueryPredicateStep step = qps[i];
		// 	switch (step.type)
		// 	{
		// 	case TSQueryPredicateStepTypeString:
		// 	{
		// 		uint32_t length;
		// 		const char *str = ts_query_string_value_for_id(query, step.value_id, &length);
		// 		printf("Step %u: String (\"%.*s\")\n", i, length, str);
		// 	}
		// 	break;

		// 	case TSQueryPredicateStepTypeCapture:
		// 		printf("Step %u: Capture (%u)\n", i, step.value_id);

		// 		break;

		// 	default:
		// 		printf("Step %u: Unknown type (%u)\n", i, step.value_id);
		// 		break;
		// 	}
		// }

		for (uint16_t i = 0; i < match.capture_count; i++)
		{
			sqInt oop = interpreterProxy->instantiateClassindexableSize(class, 0);
			link = push_oop_on_link(oop, link);

			capture = match.captures[i];

			const char *capture_name = ts_query_capture_name_for_id(
				query,
				capture.index,
				&length);

			sqInt oopContentString = interpreterProxy->instantiateClassindexableSize(
				interpreterProxy->classString(), length);
			memcpy(interpreterProxy->firstIndexableField(oopContentString), capture_name, length);

			interpreterProxy->storePointerofObjectwithValue(0, oop, oopContentString);

			captured_node = capture.node;

			point = ts_node_start_point(captured_node);
			row = point.row + 1;
			column = point.column + 1;

			interpreterProxy->storePointerofObjectwithValue(
				1, oop, interpreterProxy->makePointwithxValueyValue(column, row));

			point = ts_node_end_point(captured_node);
			row = point.row + 1;
			column = point.column;

			interpreterProxy->storePointerofObjectwithValue(
				2, oop, interpreterProxy->makePointwithxValueyValue(column, row));

			start_byte = ts_node_start_byte(captured_node);
			end_byte = ts_node_end_byte(captured_node);

			interpreterProxy->storePointerofObjectwithValue(
				3, oop, interpreterProxy->makePointwithxValueyValue(start_byte + 1, end_byte));

			interpreterProxy->storeIntegerofObjectwithValue(
				4, oop, discreteTime++);

			length = end_byte - start_byte;

			oopContentString = interpreterProxy->instantiateClassindexableSize(
				interpreterProxy->classString(), length);

			memcpy(interpreterProxy->firstIndexableField(oopContentString), sourceCode + start_byte, length);

			interpreterProxy->storePointerofObjectwithValue(5, oop, oopContentString);
		}
	}

	ts_query_cursor_delete(cursor);

	sqInt oop = interpreterProxy->instantiateClassindexableSize(
		interpreterProxy->classArray(), link->size);

	for (TS_link_t *l = link; l->size > 0; l = l->next)
	{
		interpreterProxy->stObjectatput(oop, l->size, l->oop);
	}

	free_link(link);

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
