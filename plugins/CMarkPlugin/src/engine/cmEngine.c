/* cmEngine.c -- the engine of the CMarkPlugin: cmark-gfm 0.29.0.gfm.14's
 * registration, the parse and its walk into the tape, the renders, and the
 * kept result (docs/CMark.md, "Tree format" and "Primitive contract").  The
 * only file of the plugin that includes cmark headers (rule R2); no VM
 * header (rule R3).
 *
 * A call (cm_parse, cm_render) first measures the sanitised UTF-8 text T of
 * its source (cmText.c; TOO_LARGE before any allocation), then runs behind
 * cm_guarded (cmTrack.c): it builds T, answers the kept result when its key
 * (the primitive, T, and the number arguments but the budgets) matches, and
 * otherwise frees it and parses T with the tracking allocator, in one
 * cmark_parser_feed, with the extensions of the mask attached.  A parse
 * walks the tree in pre-order with cmark_node_first_child/next/parent (an
 * explicit stack of record indexes, never recursion) into the tape, 16
 * uint32 words per node, and the strings; a render renders it and detaches
 * the output from the tracker.  Then cmark_parser_free, cmark_node_free and
 * a sweep that must find no block of cmark.
 *
 * When a request goes over a C limit, the tracker longjmps to cm_guarded,
 * and the landing here, in this order: resets cmark's static tables of
 * special characters (process_inlines sets them for the extensions' `~',
 * `:' and `w' around the inline phase, blocks.c, inlines.c; left set, every
 * later parse of the process would split its text there), frees every block
 * of cmark without calling cmark, frees the engine's buffers, and answers
 * CM_C_LIMIT.
 *
 * Registration (cm_init) is once per process, with cmark's own allocator,
 * and is never undone: after cmark_release_plugins the extensions could
 * never be found again.
 */

#include <stdlib.h>
#include <string.h>

#include "cmark-gfm.h"
#include "cmark-gfm-extension_api.h"
#include "cmark-gfm-core-extensions.h"
#include "table.h"
#include "strikethrough.h"

#include "cmPrivate.h"

enum { EXTENSIONS = 5 };

static const char *const extensionNames[EXTENSIONS] = {
	"table", "strikethrough", "autolink", "tagfilter", "tasklist"
};
enum { EXT_TASKLIST = 4 };

static cmark_syntax_extension *extensions[EXTENSIONS];
static int initialised;
static int dropped;			/* cm_test_drop_extension's bits */
static cm_stats stats;
static size_t maxInput = CM_MAX_INPUT_BYTES;
static uint64_t keepCapOverride;
static int extraNodes;
static uint8_t emptyText[1];		/* T of an empty source: never NULL */

uint64_t
cm_default_c_budget(void)
{
#ifdef __EMSCRIPTEN__
	return UINT64_C(128) << 20;
#else
	return UINT64_C(4) << 30;
#endif
}

uint64_t
cm_keep_cap(void)
{
	if (keepCapOverride) {
		return keepCapOverride;
	}
#ifdef __EMSCRIPTEN__
	return UINT64_C(8) << 20;
#else
	return UINT64_C(4) << 30;
#endif
}


/*** Registration ***/

static int
found(int i)
{
	return extensions[i] != NULL && !(dropped & (1 << i));
}

int
cm_init(void)
{
	int i;

	if (!initialised) {
		cmark_gfm_core_extensions_ensure_registered();
		for (i = 0; i < EXTENSIONS; i++) {
			extensions[i] = cmark_find_syntax_extension(extensionNames[i]);
		}
		/* the node types of the extensions are globals that registration
		   sets; a type still 0 could not be told from no node */
		if (!CMARK_NODE_TABLE || !CMARK_NODE_TABLE_ROW || !CMARK_NODE_TABLE_CELL) {
			extensions[0] = NULL;
		}
		if (!CMARK_NODE_STRIKETHROUGH) {
			extensions[1] = NULL;
		}
		initialised = 1;
	}
	return cm_extensions_present(CM_EXT_ALL) ? 0 : -1;
}

const char *
cm_version(void)
{
	return cmark_version_string();
}

int
cm_extension_count(void)
{
	int i, n = 0;

	for (i = 0; i < EXTENSIONS; i++) {
		n += found(i);
	}
	return n;
}

const char *
cm_extension_name(int index)
{
	int i;

	for (i = 0; i < EXTENSIONS; i++) {
		if (found(i) && index-- == 0) {
			return extensionNames[i];
		}
	}
	return NULL;
}

int
cm_extensions_present(int mask)
{
	int i;

	for (i = 0; i < EXTENSIONS; i++) {
		if ((mask & (1 << i)) && !found(i)) {
			return 0;
		}
	}
	return (mask & ~CM_EXT_ALL) == 0;
}

const cm_stats *
cm_get_stats(void)
{
	return &stats;
}


/*** The kept result ***/

enum { KEPT_NONE, KEPT_TREE, KEPT_OUTPUT };

static struct {
	int kind;
	int options, extensions, format, width;
	uint32_t maxDepth;
	cm_tree tree;
	cm_output output;
	size_t bytes;
} kept;

static void
freeText(cm_text *t)
{
	if (t->text != emptyText) {
		cm_block_free(t->text);
	}
	memset(t, 0, sizeof *t);
}

void
cm_tree_free(cm_tree *t)
{
	cm_block_free(t->tape);
	cm_block_free(t->strings);
	freeText(&t->t);
	memset(t, 0, sizeof *t);
}

void
cm_output_free(cm_output *o)
{
	cm_block_free(o->bytes);
	freeText(&o->t);
	memset(o, 0, sizeof *o);
}

static size_t
textBytes(const cm_text *t)
{
	return t->text == emptyText ? 0 : cm_block_bytes(t->text);
}

size_t
cm_release_kept(void)
{
	size_t bytes = kept.bytes;

	if (kept.kind == KEPT_TREE) {
		cm_tree_free(&kept.tree);
	} else if (kept.kind == KEPT_OUTPUT) {
		cm_output_free(&kept.output);
	}
	kept.kind = KEPT_NONE;
	kept.bytes = 0;
	return bytes;
}

size_t
cm_kept_bytes(void)
{
	return kept.bytes;
}

void
cm_keep_tree(int options, int extensions, uint32_t maxDepth, cm_tree *t)
{
	size_t bytes;

	if (!t->tape) {
		return;
	}
	bytes = cm_block_bytes(t->tape) + cm_block_bytes(t->strings) + textBytes(&t->t);
	cm_release_kept();
	if (bytes > cm_keep_cap()) {
		cm_tree_free(t);
		return;
	}
	kept.kind = KEPT_TREE;
	kept.options = options;
	kept.extensions = extensions;
	kept.maxDepth = maxDepth;
	kept.tree = *t;
	kept.tree.reused = 0;
	kept.bytes = bytes;
	memset(t, 0, sizeof *t);
}

void
cm_keep_output(int format, int options, int extensions, int width, cm_output *o)
{
	size_t bytes;

	if (!o->bytes) {
		return;
	}
	bytes = cm_block_bytes(o->bytes) + textBytes(&o->t);
	cm_release_kept();
	if (bytes > cm_keep_cap()) {
		cm_output_free(o);
		return;
	}
	kept.kind = KEPT_OUTPUT;
	kept.format = format;
	kept.options = options;
	kept.extensions = extensions;
	kept.width = width;
	kept.output = *o;
	kept.output.reused = 0;
	kept.bytes = bytes;
	memset(o, 0, sizeof *o);
}


/*** A call ***/

typedef struct {
	uint32_t record;	/* the record index of the node at this depth */
	uint32_t children;	/* its children so far */
} Frame;

typedef struct {
	int render;
	const cm_source *src;
	int options, extensions, format, width;
	uint32_t maxDepth;
	/* T */
	size_t textBytes, chars;
	int differs;
	uint8_t *text;
	/* the parse */
	uint32_t *tape;
	size_t nodes, capacity;
	uint8_t *strings;
	size_t stringBytes, stringCapacity;
	uint32_t depth;
	cm_lines lines;
	/* the render */
	uint8_t *output;
	size_t outputSize;
	int ascii;
	/* a kept result taken */
	int reused;
	cm_tree keptTree;
	cm_output keptOutput;
	uint64_t code;
} Job;

static int
sameText(const cm_text *t, const Job *job)
{
	return t->textBytes == job->textBytes
	    && (job->textBytes == 0 || memcmp(t->text, job->text, job->textBytes) == 0);
}

/* Builds T; answers whether the kept result is this call's (then taken) */
static int
takeKept(cm_call *ctx, Job *job)
{
	if (job->textBytes == 0) {
		job->text = emptyText;
	} else {
		job->text = cm_own_alloc(job->textBytes);
		cm_text_fill(job->src, job->text);
	}
	if (!job->render && kept.kind == KEPT_TREE && kept.options == job->options
	 && kept.extensions == job->extensions && kept.maxDepth == job->maxDepth
	 && sameText(&kept.tree.t, job)) {
		job->keptTree = kept.tree;
		job->reused = 1;
	} else if (job->render && kept.kind == KEPT_OUTPUT && kept.format == job->format
	 && kept.options == job->options && kept.extensions == job->extensions
	 && kept.width == job->width && sameText(&kept.output.t, job)) {
		job->keptOutput = kept.output;
		job->reused = 1;
	}
	if (job->reused) {
		memset(&kept.tree, 0, sizeof kept.tree);
		memset(&kept.output, 0, sizeof kept.output);
		kept.kind = KEPT_NONE;
		kept.bytes = 0;
		stats.reused++;
	} else {
		cm_release_kept();
	}
	cm_track_set_kept(0);
	(void)ctx;
	return job->reused;
}

static cmark_node *
parseText(cm_call *ctx, Job *job, int options)
{
	cmark_parser *parser;
	cmark_node *root;
	int i;

	parser = cmark_parser_new_with_mem(options, cm_track_allocator());
	ctx->parser = parser;
	for (i = 0; i < EXTENSIONS; i++) {
		if (job->extensions & (1 << i)) {
			cmark_parser_attach_syntax_extension(parser, extensions[i]);
		}
	}
	cmark_parser_feed(parser, (const char *)job->text, job->textBytes);
	root = cmark_parser_finish(parser);
	return root;
}

static void
freeParser(cm_call *ctx)
{
	cmark_parser_free(ctx->parser);
	ctx->parser = NULL;
}


/*** The tape ***/

static uint32_t
codeOf(cmark_node_type type)
{
	unsigned t = (unsigned)type;

	if (type == CMARK_NODE_TABLE) {
		return CM_CODE_TABLE;
	}
	if (type == CMARK_NODE_TABLE_ROW) {
		return CM_CODE_TABLE_ROW;
	}
	if (type == CMARK_NODE_TABLE_CELL) {
		return CM_CODE_TABLE_CELL;
	}
	if (type == CMARK_NODE_STRIKETHROUGH) {
		return CM_CODE_STRIKETHROUGH;
	}
	if ((t & CMARK_NODE_TYPE_MASK) == CMARK_NODE_TYPE_INLINE) {
		return t >= CMARK_NODE_TEXT && t <= CMARK_NODE_FOOTNOTE_REFERENCE ? t : CM_CODE_UNKNOWN_INLINE;
	}
	return t >= CMARK_NODE_DOCUMENT && t <= CMARK_NODE_FOOTNOTE_DEFINITION ? t : CM_CODE_UNKNOWN_BLOCK;
}

/* Appends len bytes to the strings: their offset, and whether all are ASCII */
static uint32_t
addString(Job *job, const char *s, size_t len, int *ascii)
{
	size_t offset = job->stringBytes, i;

	*ascii = 1;
	if (len == 0) {
		return 0;
	}
	if (len > UINT32_MAX - job->stringBytes) {
		/* the offsets of the tape are 32-bit */
		cm_track_fail(CM_CLIM_BUDGET);
	}
	if (job->stringBytes + len > job->stringCapacity) {
		size_t capacity = job->stringCapacity ? job->stringCapacity : 256;
		while (capacity < job->stringBytes + len) {
			capacity *= 2;
		}
		job->strings = cm_own_realloc(job->strings, capacity);
		job->stringCapacity = capacity;
	}
	memcpy(job->strings + offset, s, len);
	for (i = 0; i < len; i++) {
		if ((unsigned char)s[i] >= 0x80) {
			*ascii = 0;
			break;
		}
	}
	job->stringBytes += len;
	return (uint32_t)offset;
}

/* Fills str1 (which 0) or str2 (which 1) of rec with s (NULL: absent) */
static void
setString(Job *job, uint32_t *rec, int which, const char *s)
{
	size_t len = s ? strlen(s) : 0;
	int ascii;
	uint32_t offset = addString(job, s, len, &ascii);

	rec[which ? CM_W_S2OFF : CM_W_S1OFF] = offset;
	rec[which ? CM_W_S2LEN : CM_W_S1LEN] = (uint32_t)len;
	if (ascii) {
		rec[CM_W_FLAGS] |= which ? CM_F_STR2_ASCII : CM_F_STR1_ASCII;
	}
}

static uint32_t
nonNegative(int v)
{
	return v > 0 ? (uint32_t)v : 0;
}

/* The decimal ordinal of a footnote reference's literal */
static uint32_t
ordinalOf(const char *s)
{
	uint32_t n = 0;

	while (s && *s >= '0' && *s <= '9' && n < 100000000) {
		n = n * 10 + (uint32_t)(*s++ - '0');
	}
	return n;
}

/* Appends the record of node (whose parent has record index parent and
 * which is child number sibling of it) and answers its record index */
static uint32_t
emit(Job *job, cmark_node *node, uint32_t parent, uint32_t sibling)
{
	cmark_node_type type = cmark_node_get_type(node);
	uint32_t *rec, code = codeOf(type);
	int sl, sc, el, ec;

	if (job->nodes >= UINT32_MAX / CM_REC_WORDS) {
		cm_track_fail(CM_CLIM_BUDGET);
	}
	if (job->nodes == job->capacity) {
		job->capacity = job->capacity ? 2 * job->capacity : 64;
		job->tape = cm_own_realloc(job->tape, job->capacity * CM_REC_WORDS * sizeof(uint32_t));
	}
	rec = job->tape + job->nodes * CM_REC_WORDS;
	memset(rec, 0, CM_REC_WORDS * sizeof(uint32_t));
	job->nodes++;
	rec[CM_W_CODE] = code;
	rec[CM_W_PARENT] = parent;
	if (cmark_node_get_syntax_extension(node)) {
		rec[CM_W_FLAGS] |= CM_F_EXTENSION;
	}
	sl = cmark_node_get_start_line(node);
	sc = cmark_node_get_start_column(node);
	el = cmark_node_get_end_line(node);
	ec = cmark_node_get_end_column(node);
	if (sl >= 1 && sc >= 1) {
		rec[CM_W_FLAGS] |= CM_F_HAS_POS;
		rec[CM_W_SL] = (uint32_t)sl;
		rec[CM_W_SC] = (uint32_t)sc;
		rec[CM_W_EL] = nonNegative(el);
		rec[CM_W_EC] = nonNegative(ec);
		if (cm_lines_span(&job->lines, sl, sc, el, ec, &rec[CM_W_SI], &rec[CM_W_EI])) {
			rec[CM_W_FLAGS] |= CM_F_CLAMPED;
		}
	}
	switch (code) {
	case CMARK_NODE_LIST:
		rec[CM_W_INT1] = cmark_node_get_list_type(node) == CMARK_ORDERED_LIST ? 2 : 1;
		rec[CM_W_INT2] = nonNegative((int)cmark_node_get_list_delim(node));
		rec[CM_W_INT3] = rec[CM_W_INT1] == 2 ? nonNegative(cmark_node_get_list_start(node)) : 0;
		if (cmark_node_get_list_tight(node)) {
			rec[CM_W_FLAGS] |= CM_F_TIGHT;
		}
		break;
	case CMARK_NODE_ITEM:
		rec[CM_W_INT1] = nonNegative(cmark_node_get_item_index(node));
		if (extensions[EXT_TASKLIST] && cmark_node_get_syntax_extension(node) == extensions[EXT_TASKLIST]) {
			rec[CM_W_FLAGS] |= CM_F_TASK;
			if (cmark_gfm_extensions_get_tasklist_item_checked(node)) {
				rec[CM_W_FLAGS] |= CM_F_CHECKED;
			}
		}
		break;
	case CMARK_NODE_CODE_BLOCK: {
		int length = 0, offset = 0;
		char character = 0;
		if (cmark_node_get_fenced(node, &length, &offset, &character)) {
			rec[CM_W_FLAGS] |= CM_F_FENCED;
			rec[CM_W_INT1] = nonNegative(length);
			rec[CM_W_INT2] = (unsigned char)character;
			rec[CM_W_INT3] = nonNegative(offset);
		}
		setString(job, rec, 0, cmark_node_get_literal(node));
		setString(job, rec, 1, cmark_node_get_fence_info(node));
		break;
	}
	case CMARK_NODE_HTML_BLOCK:
	case CMARK_NODE_TEXT:
	case CMARK_NODE_CODE:
	case CMARK_NODE_HTML_INLINE:
		setString(job, rec, 0, cmark_node_get_literal(node));
		break;
	case CMARK_NODE_CUSTOM_BLOCK:
	case CMARK_NODE_CUSTOM_INLINE:
		setString(job, rec, 0, cmark_node_get_on_enter(node));
		setString(job, rec, 1, cmark_node_get_on_exit(node));
		break;
	case CMARK_NODE_HEADING:
		rec[CM_W_INT1] = nonNegative(cmark_node_get_heading_level(node));
		break;
	case CMARK_NODE_FOOTNOTE_DEFINITION:
		/* the ordinal: numbered after the walk (the definitions come last) */
		setString(job, rec, 0, cmark_node_get_literal(node));
		break;
	case CMARK_NODE_LINK:
	case CMARK_NODE_IMAGE:
		setString(job, rec, 0, cmark_node_get_url(node));
		setString(job, rec, 1, cmark_node_get_title(node));
		break;
	case CMARK_NODE_FOOTNOTE_REFERENCE: {
		const char *literal = cmark_node_get_literal(node);
		rec[CM_W_INT1] = ordinalOf(literal);
		setString(job, rec, 0, literal);
		break;
	}
	case CM_CODE_TABLE: {
		uint16_t columns = cmark_gfm_extensions_get_table_columns(node), i;
		uint8_t *alignments = cmark_gfm_extensions_get_table_alignments(node);
		char *s = cm_own_alloc((size_t)columns + 1);
		for (i = 0; i < columns; i++) {
			uint8_t a = alignments ? alignments[i] : 0;
			s[i] = a == 'l' || a == 'c' || a == 'r' ? (char)a : '-';
		}
		s[columns] = 0;
		rec[CM_W_INT1] = columns;
		setString(job, rec, 0, s);
		cm_own_free(s);
		break;
	}
	case CM_CODE_TABLE_ROW:
		if (cmark_gfm_extensions_get_table_row_is_header(node)) {
			rec[CM_W_FLAGS] |= CM_F_HEADER;
		}
		break;
	case CM_CODE_TABLE_CELL:
		rec[CM_W_INT1] = sibling;
		break;
	case CM_CODE_UNKNOWN_BLOCK:
	case CM_CODE_UNKNOWN_INLINE:
		rec[CM_W_INT1] = (uint32_t)type;
		setString(job, rec, 0, cmark_node_get_type_string(node));
		break;
	default:
		break;
	}
	return (uint32_t)job->nodes;
}

/* The ordinals of the definitions, and the definitions of the references
 * (cmark moved the referenced definitions to the end of the document in
 * the order of their first reference: the k-th definition has ordinal k) */
static void
linkFootnotes(Job *job)
{
	size_t i, definitions = 0;
	uint32_t *records;

	for (i = 0; i < job->nodes; i++) {
		definitions += job->tape[i * CM_REC_WORDS + CM_W_CODE] == CMARK_NODE_FOOTNOTE_DEFINITION;
	}
	if (!definitions) {
		return;
	}
	records = cm_own_alloc(definitions * sizeof(uint32_t));
	definitions = 0;
	for (i = 0; i < job->nodes; i++) {
		uint32_t *rec = job->tape + i * CM_REC_WORDS;
		if (rec[CM_W_CODE] == CMARK_NODE_FOOTNOTE_DEFINITION) {
			records[definitions++] = (uint32_t)(i + 1);
			rec[CM_W_INT1] = (uint32_t)definitions;
		}
	}
	for (i = 0; i < job->nodes; i++) {
		uint32_t *rec = job->tape + i * CM_REC_WORDS;
		if (rec[CM_W_CODE] == CMARK_NODE_FOOTNOTE_REFERENCE && rec[CM_W_INT1] >= 1 && rec[CM_W_INT1] <= definitions) {
			rec[CM_W_INT2] = records[rec[CM_W_INT1] - 1];
		}
	}
	cm_own_free(records);
}

/* The walk: pre-order, an explicit stack, the depth limit */
static int
walk(Job *job, cmark_node *root)
{
	Frame *stack;
	size_t capacity = 64, depth = 1;
	cmark_node *node = root, *child;

	cm_lines_init(&job->lines, job->text, job->textBytes);
	stack = cm_own_alloc(capacity * sizeof *stack);
	job->depth = 1;
	for (;;) {
		uint32_t parent = 0, sibling = 1;
		if (depth > 1) {
			parent = stack[depth - 2].record;
			sibling = ++stack[depth - 2].children;
		}
		stack[depth - 1].record = emit(job, node, parent, sibling);
		stack[depth - 1].children = 0;
		if (depth > job->depth) {
			job->depth = (uint32_t)depth;
		}
		child = cmark_node_first_child(node);
		if (child) {
			if (job->maxDepth && depth + 1 > job->maxDepth) {
				job->depth = job->maxDepth + 1;
				job->code = (uint64_t)job->maxDepth + 1;
				return CM_TOO_DEEP;
			}
			if (depth == capacity) {
				capacity *= 2;
				stack = cm_own_realloc(stack, capacity * sizeof *stack);
			}
			depth++;
			node = child;
			continue;
		}
		while (node != root && !cmark_node_next(node)) {
			node = cmark_node_parent(node);
			depth--;
		}
		if (node == root) {
			break;
		}
		node = cmark_node_next(node);
	}
	linkFootnotes(job);
	return CM_OK;
}

/* cm_test_extra_nodes: a custom block holding a paragraph with a custom
 * inline, and nodes of an unregistered block and inline type */
static void
addExtraNodes(cmark_node *root)
{
	static cmark_node_type unknownBlock, unknownInline;
	cmark_mem *mem = cm_track_allocator();
	cmark_node *custom, *paragraph, *inlineNode;

	if (!unknownBlock) {
		unknownBlock = cmark_syntax_extension_add_node(0);
		unknownInline = cmark_syntax_extension_add_node(1);
	}
	custom = cmark_node_new_with_mem(CMARK_NODE_CUSTOM_BLOCK, mem);
	cmark_node_set_on_enter(custom, "<cb>");
	cmark_node_set_on_exit(custom, "</cb>");
	paragraph = cmark_node_new_with_mem(CMARK_NODE_PARAGRAPH, mem);
	inlineNode = cmark_node_new_with_mem(CMARK_NODE_CUSTOM_INLINE, mem);
	cmark_node_set_on_enter(inlineNode, "<ci>");
	cmark_node_set_on_exit(inlineNode, "");
	cmark_node_append_child(paragraph, inlineNode);
	cmark_node_append_child(custom, paragraph);
	cmark_node_append_child(root, custom);
	cmark_node_append_child(root, cmark_node_new_with_mem(unknownBlock, mem));
	paragraph = cmark_node_new_with_mem(CMARK_NODE_PARAGRAPH, mem);
	cmark_node_append_child(paragraph, cmark_node_new_with_mem(unknownInline, mem));
	cmark_node_append_child(root, paragraph);
}

static int
parseWork(cm_call *ctx)
{
	Job *job = ctx->job;
	cmark_node *root;
	int status;

	if (takeKept(ctx, job)) {
		return CM_OK;
	}
	stats.parses++;
	root = parseText(ctx, job, job->options | CMARK_OPT_SOURCEPOS | CMARK_OPT_VALIDATE_UTF8);
	freeParser(ctx);
	if (extraNodes) {
		addExtraNodes(root);
	}
	status = walk(job, root);
	cmark_node_free(root);
	return status;
}

static int
renderWork(cm_call *ctx)
{
	Job *job = ctx->job;
	cmark_node *root;
	char *output = NULL;
	size_t i;

	if (takeKept(ctx, job)) {
		return CM_OK;
	}
	stats.renders++;
	root = parseText(ctx, job, job->options | CMARK_OPT_VALIDATE_UTF8);
	switch (job->format) {
	case CM_FMT_HTML:
		output = cmark_render_html(root, job->options, cmark_parser_get_syntax_extensions(ctx->parser));
		break;
	case CM_FMT_XML:
		output = cmark_render_xml(root, job->options);
		break;
	case CM_FMT_COMMONMARK:
		output = cmark_render_commonmark(root, job->options, job->width);
		break;
	case CM_FMT_PLAINTEXT:
		output = cmark_render_plaintext(root, job->options, job->width);
		break;
	case CM_FMT_LATEX:
		output = cmark_render_latex(root, job->options, job->width);
		break;
	default:
		output = cmark_render_man(root, job->options, job->width);
		break;
	}
	/* the renderers answer a block of the root's allocator (html.c,
	   buffer.c cmark_strbuf_detach): detached, it outlives the tree */
	cm_track_detach(output);
	freeParser(ctx);
	cmark_node_free(root);
	job->output = (uint8_t *)output;
	job->outputSize = strlen(output);
	job->ascii = 1;
	for (i = 0; i < job->outputSize; i++) {
		if (job->output[i] >= 0x80) {
			job->ascii = 0;
			break;
		}
	}
	return CM_OK;
}

static int
run(Job *job, int64_t cBudget, uint64_t *code)
{
	cm_call ctx;
	int status;
	size_t swept;
	uint64_t budget = cm_default_c_budget();

	job->textBytes = cm_text_measure(job->src, &job->differs, &job->chars);
	if (job->textBytes > maxInput) {
		cm_release_kept();
		stats.tooLarge++;
		*code = job->textBytes;
		return CM_TOO_LARGE;
	}
	if (cBudget >= 0 && (uint64_t)cBudget < budget) {
		budget = (uint64_t)cBudget;
	}
	memset(&ctx, 0, sizeof ctx);
	ctx.budget = budget;
	ctx.kept = kept.bytes;
	ctx.job = job;
	cm_track_begin(&ctx);
	status = cm_guarded(job->render ? renderWork : parseWork, &ctx);
	if (status == CM_LANDED) {
		/* the landing (see the top of the file) */
		if (ctx.parser) {
			cmark_manage_extensions_special_characters(ctx.parser, 0);
		}
		cm_track_sweep();
		cm_own_sweep();
		cm_track_end();
		stats.cLimit++;
		*code = (uint64_t)ctx.limit;
		return CM_C_LIMIT;
	}
	if (status == CM_OK && !job->reused) {
		cm_own_keep(job->text == emptyText ? NULL : job->text);
		cm_own_keep(job->tape);
		cm_own_keep(job->strings);
		cm_own_keep(job->output);
	}
	swept = cm_track_sweep();
	CM_ASSERT(swept == 0);	/* a block of cmark left: a leak */
	(void)swept;
	cm_own_sweep();
	cm_track_end();
	if (status == CM_TOO_DEEP) {
		stats.tooDeep++;
		*code = job->code;
	}
	return status;
}

static void
startJob(Job *job, const cm_source *src, int render, int options, int extensions)
{
	memset(job, 0, sizeof *job);
	job->src = src;
	job->render = render;
	job->options = options;
	job->extensions = extensions;
	cm_init();
	CM_ASSERT(cm_extensions_present(extensions));
	CM_ASSERT((options & ~CM_OPT_ALLOWED) == 0);
}

int
cm_parse(const cm_source *src, int options, int extensions, uint32_t maxDepth,
         int64_t cBudget, cm_tree *out, uint64_t *code)
{
	Job job;
	int status;

	CM_ASSERT(cBudget >= -1);
	memset(out, 0, sizeof *out);
	*code = 0;
	startJob(&job, src, 0, options, extensions);
	job.maxDepth = maxDepth;
	status = run(&job, cBudget, code);
	out->sourceChars = job.chars;
	out->depth = job.depth;
	if (status != CM_OK) {
		return status;
	}
	if (job.reused) {
		*out = job.keptTree;
		out->reused = 1;
	} else {
		out->tape = job.tape;
		out->nodes = job.nodes;
		out->strings = job.strings;
		out->stringBytes = job.stringBytes;
		out->depth = job.depth;
		out->sourceChars = job.chars;
		out->t.text = job.text;
		out->t.textBytes = job.textBytes;
	}
	out->t.differs = job.differs;
	return CM_OK;
}

int
cm_render(const cm_source *src, int format, int options, int extensions, int width,
          int64_t cBudget, cm_output *out, uint64_t *code)
{
	Job job;
	int status;

	CM_ASSERT(cBudget >= -1 && format >= CM_FMT_HTML && format <= CM_FMT_MAN && width >= 0);
	memset(out, 0, sizeof *out);
	*code = 0;
	startJob(&job, src, 1, options, extensions);
	job.format = format;
	job.width = width;
	status = run(&job, cBudget, code);
	if (status != CM_OK) {
		return status;
	}
	if (job.reused) {
		*out = job.keptOutput;
		out->reused = 1;
	} else {
		out->bytes = job.output;
		out->size = job.outputSize;
		out->ascii = job.ascii;
		out->t.text = job.text;
		out->t.textBytes = job.textBytes;
	}
	out->t.differs = job.differs;
	*code = out->size;
	return CM_OK;
}


/*** Tests ***/

#ifdef CM_TESTING

void
cm_test_limits(size_t maxInputBytes, size_t maxBlock)
{
	maxInput = maxInputBytes ? maxInputBytes : CM_MAX_INPUT_BYTES;
	cm_track_set_max_block(maxBlock);
}

void
cm_test_drop_extension(int bit)
{
	dropped = bit < 0 ? 0 : dropped | bit;
}

size_t
cm_track_live_blocks(void)
{
	return cm_track_blocks();
}

void
cm_test_keep_cap(uint64_t cap)
{
	keepCapOverride = cap;
}

void
cm_test_extra_nodes(int on)
{
	extraNodes = on;
}

#endif
