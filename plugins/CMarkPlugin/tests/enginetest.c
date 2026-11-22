/* enginetest.c -- tests of the CMarkPlugin's engine (src/engine) through
 * cmEngine.h alone, as tests/run-c-tests.sh builds them (lanes engine,
 * engine-gcc, engine-uchar, engine-asan, engine-wasm64): the sanitising,
 * positions and character indexes, the sample's tape against
 * fixtures/sample.tree, every code, flag and field, the extension masks and
 * the options, the depth limit, the input and block limits, the C-limit
 * sweep and the special characters after each landing, the CommonMark
 * blow-up, the renders against fixtures/sample.*, determinism, the kept
 * result, the known gfm.14 behaviour, stress inputs and a fuzz-lite.
 *
 *   enginetest FIXTURES [--slow]      run every test
 *   enginetest --dump FILE OPTIONS EXTENSIONS
 *                                     print the tape of FILE (the format of
 *                                     fixtures/sample.tree)
 *   enginetest --render FILE FORMAT OPTIONS EXTENSIONS WIDTH
 *                                     print a render of FILE
 *   enginetest --special              print the tape of `www:a~b~*c*' with
 *                                     extensions 0 (fixtures/special-chars.tree)
 */

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "cmEngine.h"

#ifdef __EMSCRIPTEN__
# define WASM 1
#else
# define WASM 0
#endif

#define FOOTNOTES (1 << 13)
#define SOURCEPOS (1 << 1)
#define HARDBREAKS (1 << 2)
#define NOBREAKS (1 << 4)
#define SMART (1 << 10)
#define DOUBLE_TILDE (1 << 14)
#define UNSAFE (1 << 17)

static int failures, checks;
static const char *fixtures = ".";

static void
check(int ok, const char *format, ...)
{
	va_list args;

	checks++;
	if (ok) {
		return;
	}
	failures++;
	printf("FAILED: ");
	va_start(args, format);
	vprintf(format, args);
	va_end(args);
	printf("\n");
}

/*** Text buffers ***/

typedef struct {
	char *s;
	size_t n, cap;
} Buf;

static void
bput(Buf *b, const void *data, size_t n)
{
	if (b->n + n + 1 > b->cap) {
		b->cap = (b->n + n + 1) * 2;
		b->s = realloc(b->s, b->cap);
		if (!b->s) {
			printf("out of memory\n");
			exit(2);
		}
	}
	memcpy(b->s + b->n, data, n);
	b->n += n;
	b->s[b->n] = 0;
}

static void
bprintf(Buf *b, const char *format, ...)
{
	char tmp[512];
	va_list args;
	int n;

	va_start(args, format);
	n = vsnprintf(tmp, sizeof tmp, format, args);
	va_end(args);
	bput(b, tmp, (size_t)n);
}

static void
brepeat(Buf *b, const char *s, size_t times)
{
	size_t n = strlen(s), i;

	for (i = 0; i < times; i++) {
		bput(b, s, n);
	}
}

static void
bfree(Buf *b)
{
	free(b->s);
	memset(b, 0, sizeof *b);
}

static char *
readFile(const char *name, size_t *size)
{
	char path[4096];
	FILE *f;
	char *data;
	size_t n = 0, cap = 4096, r;

	snprintf(path, sizeof path, "%s", name);
	if (!(f = fopen(path, "rb"))) {
		snprintf(path, sizeof path, "%s/%s", fixtures, name);
		f = fopen(path, "rb");
	}
	if (!f) {
		printf("cannot read %s\n", name);
		exit(2);
	}
	data = malloc(cap);
	while ((r = fread(data + n, 1, cap - n, f)) > 0) {
		n += r;
		if (n == cap) {
			cap *= 2;
			data = realloc(data, cap);
		}
	}
	fclose(f);
	data[n] = 0;
	*size = n;
	return data;
}


/*** Calls ***/

static cm_source
bytes(const void *s, size_t n)
{
	cm_source src;

	src.kind = CM_SRC_UTF8;
	src.units = s;
	src.count = n;
	return src;
}

#define LIT(s) bytes(s, sizeof(s) - 1)

static cm_source
latin1(const void *s, size_t n)
{
	cm_source src = bytes(s, n);

	src.kind = CM_SRC_LATIN1;
	return src;
}

static cm_source
utf32(const uint32_t *s, size_t n)
{
	cm_source src;

	src.kind = CM_SRC_UTF32;
	src.units = s;
	src.count = n;
	return src;
}

static int
parse(const cm_source *src, int options, int extensions, uint32_t maxDepth, int64_t cBudget,
      cm_tree *tree, uint64_t *code)
{
	int status = cm_parse(src, options, extensions, maxDepth, cBudget, tree, code);

	check(cm_track_live_blocks() == 0, "a parse left %zu blocks", cm_track_live_blocks());
	return status;
}

static int
parseString(const char *s, int options, int extensions, cm_tree *tree)
{
	cm_source src = bytes(s, strlen(s));
	uint64_t code;

	return parse(&src, options, extensions, 0, -1, tree, &code);
}

static uint32_t
word(const cm_tree *t, size_t record, int w)
{
	return t->tape[(record - 1) * CM_REC_WORDS + (size_t)w];
}

static void
escaped(Buf *b, const uint8_t *s, size_t n)
{
	size_t i;

	bput(b, "\"", 1);
	for (i = 0; i < n; i++) {
		uint8_t c = s[i];
		if (c == '\n') {
			bput(b, "\\n", 2);
		} else if (c == '\r') {
			bput(b, "\\r", 2);
		} else if (c == '\t') {
			bput(b, "\\t", 2);
		} else if (c == '"' || c == '\\') {
			bput(b, "\\", 1);
			bput(b, &c, 1);
		} else if (c < 0x20 || c == 0x7f) {
			bprintf(b, "\\x%02x", c);
		} else {
			bput(b, &c, 1);
		}
	}
	bput(b, "\"", 1);
}

/* The tape as fixtures/sample.tree has it: one line per record,
   code parent flags sl:sc-el:ec si-ei int1 int2 int3 "str1" "str2" */
static void
dumpTree(Buf *b, const cm_tree *t)
{
	size_t i;

	for (i = 1; i <= t->nodes; i++) {
		bprintf(b, "0x%04x %u 0x%03x %u:%u-%u:%u %u-%u %u %u %u ", word(t, i, CM_W_CODE), word(t, i, CM_W_PARENT),
			word(t, i, CM_W_FLAGS), word(t, i, CM_W_SL), word(t, i, CM_W_SC), word(t, i, CM_W_EL),
			word(t, i, CM_W_EC), word(t, i, CM_W_SI), word(t, i, CM_W_EI), word(t, i, CM_W_INT1),
			word(t, i, CM_W_INT2), word(t, i, CM_W_INT3));
		escaped(b, t->strings + word(t, i, CM_W_S1OFF), word(t, i, CM_W_S1LEN));
		bput(b, " ", 1);
		escaped(b, t->strings + word(t, i, CM_W_S2OFF), word(t, i, CM_W_S2LEN));
		bput(b, "\n", 1);
	}
}

/* The bytes of T holding code points si .. ei (1-based, inclusive) */
static void
textSpan(Buf *b, const cm_tree *t, uint32_t si, uint32_t ei)
{
	size_t i, k = 0, start = t->t.textBytes, stop = t->t.textBytes;

	for (i = 0; i < t->t.textBytes; i++) {
		if ((t->t.text[i] & 0xC0) != 0x80) {
			k++;
			if (k == si) {
				start = i;
			}
			if (k == (size_t)ei + 1) {
				stop = i;
				break;
			}
		}
	}
	if (si == 0 || ei + 1 == si || start > stop) {
		return;
	}
	bput(b, t->t.text + start, stop - start);
}

/* code [T of si..ei] per record ("-" without a position) */
static void
dumpSpans(Buf *b, const cm_tree *t)
{
	size_t i;

	for (i = 1; i <= t->nodes; i++) {
		bprintf(b, "0x%04x", word(t, i, CM_W_CODE));
		if (word(t, i, CM_W_FLAGS) & CM_F_HAS_POS) {
			Buf s = {0};
			bprintf(b, "%s [", word(t, i, CM_W_FLAGS) & CM_F_CLAMPED ? " clamped" : "");
			textSpan(&s, t, word(t, i, CM_W_SI), word(t, i, CM_W_EI));
			if (s.s) {
				escaped(b, (const uint8_t *)s.s, s.n);
			}
			bput(b, "]", 1);
			bfree(&s);
		} else {
			bput(b, " -", 2);
		}
		bput(b, "\n", 1);
	}
}

/* The tape's invariants (2.4 item 16) */
static int
invariants(const char *name, const cm_tree *t)
{
	size_t i;
	int ok = 1;

	if (t->nodes < 1 || word(t, 1, CM_W_CODE) != 0x8001 || word(t, 1, CM_W_PARENT) != 0) {
		check(0, "%s: record 1 is not the document", name);
		return 0;
	}
	for (i = 1; i <= t->nodes; i++) {
		uint32_t parent = word(t, i, CM_W_PARENT), si = word(t, i, CM_W_SI), ei = word(t, i, CM_W_EI);
		if (i > 1 && (parent < 1 || parent >= i)) {
			ok = 0;
		}
		if ((uint64_t)word(t, i, CM_W_S1OFF) + word(t, i, CM_W_S1LEN) > t->stringBytes
		 || (uint64_t)word(t, i, CM_W_S2OFF) + word(t, i, CM_W_S2LEN) > t->stringBytes) {
			ok = 0;
		}
		if ((word(t, i, CM_W_FLAGS) & CM_F_HAS_POS) && ((uint64_t)si > (uint64_t)ei + 1 || ei > t->sourceChars)) {
			ok = 0;
		}
		if (!(word(t, i, CM_W_FLAGS) & CM_F_HAS_POS)) {
			int w;
			for (w = CM_W_SL; w <= CM_W_EI; w++) {
				ok = ok && word(t, i, w) == 0;
			}
		}
		if (!ok) {
			check(0, "%s: record %zu breaks an invariant", name, i);
			return 0;
		}
	}
	return 1;
}

static int
sameText(const char *name, const char *got, size_t gotSize, const char *want, size_t wantSize)
{
	size_t i, line = 1;

	if (gotSize == wantSize && memcmp(got, want, gotSize) == 0) {
		return 1;
	}
	for (i = 0; i < gotSize && i < wantSize && got[i] == want[i]; i++) {
		line += got[i] == '\n';
	}
	check(0, "%s: differs at line %zu (byte %zu; %zu bytes, %zu expected)", name, line, i, gotSize, wantSize);
	return 0;
}

static int
sameAsFixture(const char *name, const Buf *b, const char *fixture)
{
	size_t size;
	char *want = readFile(fixture, &size);
	int same = sameText(name, b->s ? b->s : "", b->n, want, size);

	free(want);
	return same;
}

static const char *
formatName(int format)
{
	static const char *names[] = { "?", "html", "xml", "commonmark", "plaintext", "latex", "man" };

	return names[format >= 1 && format <= 6 ? format : 0];
}


/*** 1. Sanitising ***/

typedef struct {
	const char *in;
	size_t inSize;
	const char *t;
	size_t chars;
} TextCase;

static void
checkText(const char *name, const cm_source *src, const char *want, size_t wantSize, size_t chars, int differs)
{
	cm_tree t;
	uint64_t code;
	int status = parse(src, 0, 0, 0, -1, &t, &code);

	check(status == CM_OK, "%s: status %d", name, status);
	if (status != CM_OK) {
		return;
	}
	check(t.t.textBytes == wantSize && memcmp(t.t.text, want, wantSize) == 0, "%s: T", name);
	check(t.t.text != NULL, "%s: T is NULL", name);
	check(t.sourceChars == chars, "%s: sourceChars %zu, not %zu", name, t.sourceChars, chars);
	check(t.t.differs == differs, "%s: differs %d", name, t.t.differs);
	cm_tree_free(&t);
}

#define R "\xEF\xBF\xBD"

static void
testSanitising(void)
{
	static const TextCase utf8[] = {
		{ "a", 1, "a", 1 },
		{ "\xC3\xA9", 2, "\xC3\xA9", 1 },
		{ "\xE2\x82\xAC", 3, "\xE2\x82\xAC", 1 },
		{ "\xF0\x9F\x98\x80", 4, "\xF0\x9F\x98\x80", 1 },
		{ "\xC0\xAF", 2, R R, 2 },
		{ "\xC1\xBF", 2, R R, 2 },
		{ "\xE0\x80\xAF", 3, R R R, 3 },
		{ "\xF0\x80\x80\xAF", 4, R R R R, 4 },
		{ "\xED\xA0\x80", 3, R R R, 3 },
		{ "\xED\x9F\xBF", 3, "\xED\x9F\xBF", 1 },
		{ "\xF4\x90\x80\x80", 4, R R R R, 4 },
		{ "\xF4\x8F\xBF\xBF", 4, "\xF4\x8F\xBF\xBF", 1 },
		{ "a\xE2\x82", 3, "a" R R, 3 },
		{ "\xE2\x82" "a", 3, R R "a", 3 },
		{ "\xF0\x9F\x98", 3, R R R, 3 },
		{ "\x80", 1, R, 1 },
		{ "a\0b", 3, "a" R "b", 3 },
		{ "\xFF\xFE", 2, R R, 2 },
		{ "\xF5\x80\x80\x80", 4, R R R R, 4 },
	};
	static const uint32_t wide[] = { 0x41, 0, 0xD800, 0xDFFF, 0x110000, 0xFFFFFFFF, 0xE9, 0x20AC, 0x1F600, 0x10FFFF };
	size_t i;
	char name[64];
	cm_source src;

	for (i = 0; i < sizeof utf8 / sizeof utf8[0]; i++) {
		int differs = utf8[i].inSize != strlen(utf8[i].t) || memcmp(utf8[i].in, utf8[i].t, utf8[i].inSize) != 0;
		snprintf(name, sizeof name, "utf-8 case %zu", i + 1);
		src = bytes(utf8[i].in, utf8[i].inSize);
		checkText(name, &src, utf8[i].t, strlen(utf8[i].t), utf8[i].chars, differs);
	}
	src = latin1("a\0\x80\xFF", 4);
	checkText("latin-1", &src, "a" R "\xC2\x80\xC3\xBF", 8, 4, 0);
	src = latin1("plain ascii", 11);
	checkText("latin-1 ascii", &src, "plain ascii", 11, 11, 0);
	src = utf32(wide, sizeof wide / sizeof wide[0]);
	checkText("utf-32", &src, "A" R R R R R "\xC3\xA9\xE2\x82\xAC\xF0\x9F\x98\x80\xF4\x8F\xBF\xBF",
		1 + 5 * 3 + 2 + 3 + 4 + 4, 10, 0);
	src = bytes("", 0);
	checkText("empty", &src, "", 0, 0, 0);
	printf("1. sanitising: ok\n");
}


/*** 2. Positions and character indexes ***/

typedef struct {
	const char *name;
	cm_source src;
	int extensions;
	const char *spans;	/* dumpSpans */
} SpanCase;

static void
checkSpans(const SpanCase *c)
{
	cm_tree t;
	uint64_t code;
	Buf b = {0};
	int status = parse(&c->src, FOOTNOTES, c->extensions, 0, -1, &t, &code);

	check(status == CM_OK, "%s: status %d", c->name, status);
	if (status != CM_OK) {
		return;
	}
	invariants(c->name, &t);
	dumpSpans(&b, &t);
	if (!sameText(c->name, b.s, b.n, c->spans, strlen(c->spans))) {
		printf("%s:\n%s", c->name, b.s);
	}
	bfree(&b);
	cm_tree_free(&t);
}

/* The index of the first record of code, from record from */
static size_t
findCode(const cm_tree *t, uint32_t code, size_t from)
{
	size_t i;

	for (i = from; i <= t->nodes; i++) {
		if (word(t, i, CM_W_CODE) == code) {
			return i;
		}
	}
	return 0;
}

/* The smallest C budget with which parse answers CM_OK */
static uint64_t
needOf(const cm_source *src, int options, int extensions, int render, int format)
{
	uint64_t lo = 1, hi = 1, code;
	cm_tree t;
	cm_output o;
	int status;

	for (;;) {
		status = render ? cm_render(src, format, options, extensions, 0, (int64_t)hi, &o, &code)
			: cm_parse(src, options, extensions, 0, (int64_t)hi, &t, &code);
		if (status == CM_OK) {
			break;
		}
		lo = hi + 1;
		hi *= 2;
	}
	render ? cm_output_free(&o) : cm_tree_free(&t);
	while (lo < hi) {
		uint64_t mid = lo + (hi - lo) / 2;
		status = render ? cm_render(src, format, options, extensions, 0, (int64_t)mid, &o, &code)
			: cm_parse(src, options, extensions, 0, (int64_t)mid, &t, &code);
		if (status == CM_OK) {
			hi = mid;
			render ? cm_output_free(&o) : cm_tree_free(&t);
		} else {
			lo = mid + 1;
		}
	}
	return hi;
}

static void
testPositions(void)
{
	static const uint32_t wide[] = { 0xE9, ' ', '*', 0x1F600, '*', ' ', 'x', '\n' };
	SpanCase cases[] = {
		{ "ascii", LIT("# Hi *there*\n\npara `x`\n"), 31,
		  "0x8001 [\"# Hi *there*\\n\\npara `x`\"]\n"
		  "0x8009 [\"# Hi *there*\"]\n"
		  "0xc001 [\"Hi \"]\n"
		  "0xc007 [\"*there*\"]\n"
		  "0xc001 [\"there\"]\n"
		  "0x8008 [\"para `x`\"]\n"
		  "0xc001 [\"para \"]\n"
		  "0xc004 [\"x\"]\n" },
		{ "2-byte", LIT("\xC3\xA9 *\xC3\xBC* \xC3\xA7\n"), 31,
		  "0x8001 [\"\xC3\xA9 *\xC3\xBC* \xC3\xA7\"]\n"
		  "0x8008 [\"\xC3\xA9 *\xC3\xBC* \xC3\xA7\"]\n"
		  "0xc001 [\"\xC3\xA9 \"]\n"
		  "0xc007 [\"*\xC3\xBC*\"]\n"
		  "0xc001 [\"\xC3\xBC\"]\n"
		  "0xc001 [\" \xC3\xA7\"]\n" },
		{ "3- and 4-byte", LIT("\xE2\x82\xAC **\xF0\x9F\x98\x80x** \xF0\x9F\x98\x80\n"), 31,
		  "0x8001 [\"\xE2\x82\xAC **\xF0\x9F\x98\x80x** \xF0\x9F\x98\x80\"]\n"
		  "0x8008 [\"\xE2\x82\xAC **\xF0\x9F\x98\x80x** \xF0\x9F\x98\x80\"]\n"
		  "0xc001 [\"\xE2\x82\xAC \"]\n"
		  "0xc008 [\"**\xF0\x9F\x98\x80x**\"]\n"
		  "0xc001 [\"\xF0\x9F\x98\x80x\"]\n"
		  "0xc001 [\" \xF0\x9F\x98\x80\"]\n" },
		{ "crlf", LIT("a\r\nb *c*\r\n\r\n- d\r\n"), 31,
		  "0x8001 [\"a\\r\\nb *c*\\r\\n\\r\\n- d\"]\n"
		  "0x8008 [\"a\\r\\nb *c*\"]\n"
		  "0xc001 [\"a\"]\n"
		  "0xc002 -\n"
		  "0xc001 [\"b \"]\n"
		  "0xc007 [\"*c*\"]\n"
		  "0xc001 [\"c\"]\n"
		  "0x8003 [\"- d\"]\n"
		  "0x8004 [\"- d\"]\n"
		  "0x8008 [\"d\"]\n"
		  "0xc001 [\"d\"]\n" },
		{ "lone cr", LIT("a\rb *c*\r\r# h\r"), 31,
		  "0x8001 [\"a\\rb *c*\\r\\r# h\"]\n"
		  "0x8008 [\"a\\rb *c*\"]\n"
		  "0xc001 [\"a\"]\n"
		  "0xc002 -\n"
		  "0xc001 [\"b \"]\n"
		  "0xc007 [\"*c*\"]\n"
		  "0xc001 [\"c\"]\n"
		  "0x8009 [\"# h\"]\n"
		  "0xc001 [\"h\"]\n" },
		{ "bom", LIT("\xEF\xBB\xBF# Title *x*\n"), 31,
		  "0x8001 [\"\xEF\xBB\xBF# Title *x*\"]\n"
		  "0x8009 [\"# Title *x*\"]\n"
		  "0xc001 [\"Title \"]\n"
		  "0xc007 [\"*x*\"]\n"
		  "0xc001 [\"x\"]\n" },
		{ "tabs", LIT("\tcode\n\n-\tx\n"), 31,
		  "0x8001 [\"\\tcode\\n\\n-\\tx\"]\n"
		  "0x8005 [\"code\\n\"]\n"
		  "0x8003 [\"-\\tx\"]\n"
		  "0x8004 [\"-\\tx\"]\n"
		  "0x8008 [\"x\"]\n"
		  "0xc001 [\"x\"]\n" },
		{ "ec 0", LIT("- a\n- b\n\nc\n"), 31,
		  "0x8001 [\"- a\\n- b\\n\\nc\"]\n"
		  "0x8003 [\"- a\\n- b\\n\"]\n"
		  "0x8004 [\"- a\"]\n"
		  "0x8008 [\"a\"]\n"
		  "0xc001 [\"a\"]\n"
		  "0x8004 [\"- b\\n\"]\n"
		  "0x8008 [\"b\"]\n"
		  "0xc001 [\"b\"]\n"
		  "0x8008 [\"c\"]\n"
		  "0xc001 [\"c\"]\n" },
		/* a TEXT that ends an unterminated last line ends one column past
		   it (cmark counts a newline there): clamped to the last byte */
		{ "eof", LIT("x ---```"), 31,
		  "0x8001 [\"x ---```\"]\n"
		  "0x8008 [\"x ---```\"]\n"
		  "0xc001 clamped [\"x ---```\"]\n" },
		{ "empty", LIT(""), 31,
		  "0x8001 clamped []\n" },
		{ "angle autolink (known bug 3)", LIT("abcdefghijklmnopqrstuvwxyz\n<https://x.y> d\n"), 31,
		  "0x8001 [\"abcdefghijklmnopqrstuvwxyz\\n<https://x.y> d\"]\n"
		  "0x8008 [\"abcdefghijklmnopqrstuvwxyz\\n<https://x.y> d\"]\n"
		  "0xc001 [\"abcdefghijklmnopqrstuvwxyz\"]\n"
		  "0xc002 -\n"
		  "0xc009 clamped [\"\\n\"]\n"
		  "0xc001 [\"https://x.y\"]\n"
		  "0xc001 [\" d\"]\n" },
		/* an unresolved reference is a TEXT without a position, merged
		   into the TEXT before it if any, which then ends at 0 */
		{ "unresolved footnote", LIT("[^nope]*x*\n\na [^gone]\n"), 31,
		  "0x8001 [\"[^nope]*x*\\n\\na [^gone]\"]\n"
		  "0x8008 [\"[^nope]*x*\"]\n"
		  "0xc001 -\n"
		  "0xc007 [\"*x*\"]\n"
		  "0xc001 [\"x\"]\n"
		  "0x8008 [\"a [^gone]\"]\n"
		  "0xc001 []\n" },
		{ "latin-1 string", latin1("\xE9 *x*\n", 6), 31,
		  "0x8001 [\"\xC3\xA9 *x*\"]\n"
		  "0x8008 [\"\xC3\xA9 *x*\"]\n"
		  "0xc001 [\"\xC3\xA9 \"]\n"
		  "0xc007 [\"*x*\"]\n"
		  "0xc001 [\"x\"]\n" },
		{ "wide string", utf32(wide, 8), 31,
		  "0x8001 [\"\xC3\xA9 *\xF0\x9F\x98\x80* x\"]\n"
		  "0x8008 [\"\xC3\xA9 *\xF0\x9F\x98\x80* x\"]\n"
		  "0xc001 [\"\xC3\xA9 \"]\n"
		  "0xc007 [\"*\xF0\x9F\x98\x80*\"]\n"
		  "0xc001 [\"\xF0\x9F\x98\x80\"]\n"
		  "0xc001 [\" x\"]\n" },
	};
	size_t i;
	Buf b = {0};
	cm_tree t;
	cm_source src;
	uint64_t code, need;
	size_t lineSize = WASM ? 64 * 1024 : 1024 * 1024, emph;

	for (i = 0; i < sizeof cases / sizeof cases[0]; i++) {
		checkSpans(&cases[i]);
	}

	/* 200,000 empty lines: the sparse line table */
	brepeat(&b, "\n", 200000);
	bput(&b, "*x* y\n", 6);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 31, 0, -1, &t, &code) == CM_OK, "empty lines: status");
	emph = findCode(&t, 0xc007, 1);
	check(emph && word(&t, emph, CM_W_SL) == 200001 && word(&t, emph, CM_W_SI) == 200001
	      && word(&t, emph, CM_W_EI) == 200003, "empty lines: the emphasis at %u-%u",
	      emph ? word(&t, emph, CM_W_SI) : 0, emph ? word(&t, emph, CM_W_EI) : 0);
	check(word(&t, 1, CM_W_EI) == 200005, "empty lines: the document ends at %u", word(&t, 1, CM_W_EI));
	invariants("empty lines", &t);
	cm_tree_free(&t);
	need = needOf(&src, 0, 31, 0, 0);
	check(need < 2 * (uint64_t)b.n + 256 * 1024, "empty lines: %llu bytes of C memory for %zu bytes of input",
		(unsigned long long)need, b.n);
	bfree(&b);

	/* 70 lines of 1 MB (64 KB on wasm), inlines on the last one */
	for (i = 0; i < 70; i++) {
		size_t k;
		for (k = 0; k + 1 < lineSize; k += 8) {
			bput(&b, "\xC3\xA9xxxxx ", 8);
		}
		bput(&b, "\n", 1);
	}
	bput(&b, "end *em* `co` [l](u)\n", 21);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 31, 0, -1, &t, &code) == CM_OK, "long lines: status");
	if (t.tape) {
		Buf s = {0};
		size_t c = findCode(&t, 0xc004, 1), l = findCode(&t, 0xc009, 1);
		emph = findCode(&t, 0xc007, 1);
		check(emph && word(&t, emph, CM_W_SL) == 71, "long lines: the emphasis is on line 71");
		if (emph) {
			textSpan(&s, &t, word(&t, emph, CM_W_SI), word(&t, emph, CM_W_EI));
			check(s.s && strcmp(s.s, "*em*") == 0, "long lines: the emphasis is %s", s.s ? s.s : "");
			bfree(&s);
		}
		if (c) {
			textSpan(&s, &t, word(&t, c, CM_W_SI), word(&t, c, CM_W_EI));
			check(s.s && strcmp(s.s, "co") == 0, "long lines: the code is %s", s.s ? s.s : "");
			bfree(&s);
		}
		if (l) {
			textSpan(&s, &t, word(&t, l, CM_W_SI), word(&t, l, CM_W_EI));
			check(s.s && strcmp(s.s, "[l](u)") == 0, "long lines: the link is %s", s.s ? s.s : "");
			bfree(&s);
		}
		check(c && l, "long lines: the code and the link");
		invariants("long lines", &t);
		cm_tree_free(&t);
	}
	bfree(&b);
	printf("2. positions and indexes: ok\n");
}


/*** 3. The sample ***/

static void
testSample(void)
{
	size_t size, i;
	char *md = readFile("sample.md", &size);
	cm_source src = bytes(md, size);
	cm_tree t;
	uint64_t code;
	Buf b = {0};
	int cells = 0;

	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "sample: status");
	invariants("sample", &t);
	dumpTree(&b, &t);
	sameAsFixture("sample.tree", &b, "sample.tree");
	/* the | f | row: three autocompleted cells, no position */
	for (i = 1; i <= t.nodes; i++) {
		if (word(&t, i, CM_W_CODE) == CM_CODE_TABLE_CELL && !(word(&t, i, CM_W_FLAGS) & CM_F_HAS_POS)) {
			int w, zero = 1;
			for (w = CM_W_SL; w <= CM_W_EI; w++) {
				zero = zero && word(&t, i, w) == 0;
			}
			check(zero, "sample: an autocompleted cell has a position word");
			cells++;
		}
	}
	check(cells == 3, "sample: %d autocompleted cells", cells);
	check(t.depth == 7, "sample: depth %u", t.depth);
	cm_tree_free(&t);
	bfree(&b);
	free(md);
	printf("3. sample tree: ok\n");
}


/*** 4. Every code, flag and field ***/

static void
testCoverage(void)
{
	/* the fields that the table of 1.4 gives each code, all of which must
	   be non-zero somewhere (CUSTOM_INLINE's str2 is empty on purpose) */
	static const struct { uint32_t code; int words[6]; } fields[] = {
		{ 0x8003, { CM_W_INT1, CM_W_INT2, CM_W_INT3, -1 } },
		{ 0x8004, { CM_W_INT1, -1 } },
		{ 0x8005, { CM_W_INT1, CM_W_INT2, CM_W_INT3, CM_W_S1LEN, CM_W_S2LEN, -1 } },
		{ 0x8006, { CM_W_S1LEN, -1 } },
		{ 0x8007, { CM_W_S1LEN, CM_W_S2LEN, -1 } },
		{ 0x8009, { CM_W_INT1, -1 } },
		{ 0x800b, { CM_W_INT1, CM_W_S1LEN, -1 } },
		{ 0x8100, { CM_W_INT1, CM_W_S1LEN, -1 } },
		{ 0x8102, { CM_W_INT1, -1 } },
		{ 0x80ff, { CM_W_INT1, CM_W_S1LEN, -1 } },
		{ 0xc001, { CM_W_S1LEN, -1 } },
		{ 0xc004, { CM_W_S1LEN, -1 } },
		{ 0xc005, { CM_W_S1LEN, -1 } },
		{ 0xc006, { CM_W_S1LEN, -1 } },
		{ 0xc009, { CM_W_S1LEN, CM_W_S2LEN, -1 } },
		{ 0xc00a, { CM_W_S1LEN, CM_W_S2LEN, -1 } },
		{ 0xc00b, { CM_W_INT1, CM_W_INT2, CM_W_S1LEN, -1 } },
		{ 0xc0ff, { CM_W_INT1, CM_W_S1LEN, -1 } },
	};
	static const uint32_t codes[] = {
		0x8001, 0x8002, 0x8003, 0x8004, 0x8005, 0x8006, 0x8007, 0x8008, 0x8009, 0x800a, 0x800b,
		0x8100, 0x8101, 0x8102, 0x80ff,
		0xc001, 0xc002, 0xc003, 0xc004, 0xc005, 0xc006, 0xc007, 0xc008, 0xc009, 0xc00a, 0xc00b,
		0xc100, 0xc0ff
	};
	/* a fence with an offset and a non-ASCII info string, a non-ASCII link */
	static const char extra[] = "\n  ~~~~ info \xC3\xA9\n  fenced\n  ~~~~\n\n[t\xC3\xA9](/u \"\xC3\xA9\")\n";
	size_t size, i, j;
	char *md = readFile("sample.md", &size);
	Buf b = {0};
	cm_source src;
	cm_tree t;
	uint64_t code;
	uint32_t flags = 0;
	int nonAsciiString = 0, emptyString = 0;
	size_t r, y, x, refY = 0, refX = 0;

	bput(&b, md, size);
	bput(&b, extra, sizeof extra - 1);
	src = bytes(b.s, b.n);
	cm_test_extra_nodes(1);
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "coverage: status");
	cm_test_extra_nodes(0);
	invariants("coverage", &t);
	for (i = 0; i < sizeof codes / sizeof codes[0]; i++) {
		check(findCode(&t, codes[i], 1) != 0, "coverage: no record of code 0x%04x", codes[i]);
	}
	for (i = 0; i < sizeof fields / sizeof fields[0]; i++) {
		for (j = 0; fields[i].words[j] >= 0; j++) {
			int seen = 0;
			for (r = 1; r <= t.nodes; r++) {
				seen = seen || (word(&t, r, CM_W_CODE) == fields[i].code && word(&t, r, fields[i].words[j]) != 0);
			}
			check(seen, "coverage: word %d of code 0x%04x is never set", fields[i].words[j] + 1, fields[i].code);
		}
	}
	for (r = 1; r <= t.nodes; r++) {
		uint32_t f = word(&t, r, CM_W_FLAGS);
		flags |= f;
		if (word(&t, r, CM_W_S1LEN) && !(f & CM_F_STR1_ASCII)) {
			nonAsciiString = 1;
		}
		if (word(&t, r, CM_W_CODE) == 0xc006 && word(&t, r, CM_W_S2LEN) == 0 && (f & CM_F_STR2_ASCII)) {
			emptyString = 1;
		}
		if (word(&t, r, CM_W_CODE) == 0x8005 && (f & CM_F_FENCED)) {
			check(word(&t, r, CM_W_INT2) == 96 || word(&t, r, CM_W_INT2) == 126, "coverage: fence character");
		}
		if (word(&t, r, CM_W_CODE) == 0x8005 && !(f & CM_F_FENCED)) {
			check(word(&t, r, CM_W_INT1) == 0 && word(&t, r, CM_W_INT2) == 0 && word(&t, r, CM_W_INT3) == 0,
				"coverage: an indented code block has fence words");
		}
	}
	check(flags == 0x3FF, "coverage: the flags seen are 0x%03x", flags);
	check(nonAsciiString, "coverage: no non-ASCII string");
	check(emptyString, "coverage: no empty string");
	cm_tree_free(&t);
	bfree(&b);
	free(md);

	/* footnotes: an unreferenced definition (dropped), the moved order;
	   an unresolved reference becomes a TEXT without a position
	   (blocks.c, process_footnotes), which cmark_consolidate_text_nodes
	   then merges into a TEXT just before it, if any (that one keeps its
	   start, and takes the end 0 of the unresolved one when it is last) */
	check(parseString("A[^y] and [^x] and *e*[^nope]*f*.\n\n[^x]: X\n\n[^z]: Z\n\n[^y]: Y\n", FOOTNOTES, 0, &t) == CM_OK,
		"footnotes: status");
	invariants("footnotes", &t);
	r = findCode(&t, 0xc00b, 1);
	refY = r;
	refX = r ? findCode(&t, 0xc00b, r + 1) : 0;
	y = findCode(&t, 0x800b, 1);
	x = y ? findCode(&t, 0x800b, y + 1) : 0;
	check(y && x && !findCode(&t, 0x800b, x + 1), "footnotes: two definitions");
	if (y && x && refY && refX) {
		check(t.strings[word(&t, y, CM_W_S1OFF)] == 'y' && t.strings[word(&t, x, CM_W_S1OFF)] == 'x',
			"footnotes: y comes first");
		check(word(&t, y, CM_W_INT1) == 1 && word(&t, x, CM_W_INT1) == 2, "footnotes: the ordinals");
		check(word(&t, refY, CM_W_INT1) == 1 && word(&t, refY, CM_W_INT2) == y, "footnotes: y's reference");
		check(word(&t, refX, CM_W_INT1) == 2 && word(&t, refX, CM_W_INT2) == x, "footnotes: x's reference");
		check(word(&t, y, CM_W_PARENT) == 1 && word(&t, x, CM_W_PARENT) == 1, "footnotes: definitions in the document");
		check(word(&t, y, CM_W_SI) > word(&t, x, CM_W_SI), "footnotes: the positions are not monotonic");
	}
	for (r = 1, j = 0; r <= t.nodes; r++) {
		if (word(&t, r, CM_W_CODE) == 0xc001 && word(&t, r, CM_W_S1LEN) == 7
		 && memcmp(t.strings + word(&t, r, CM_W_S1OFF), "[^nope]", 7) == 0) {
			check(!(word(&t, r, CM_W_FLAGS) & CM_F_HAS_POS), "footnotes: the unresolved reference has a position");
			j++;
		}
	}
	check(j == 1, "footnotes: the unresolved reference as TEXT");
	cm_tree_free(&t);
	printf("4. codes, flags and fields: ok\n");
}


/*** 5. Extension masks, 6. options ***/

static int
hasCode(const char *s, int options, int extensions, uint32_t code)
{
	cm_tree t;
	int found;

	if (parseString(s, options, extensions, &t) != CM_OK) {
		check(0, "parse of %s failed", s);
		return 0;
	}
	found = findCode(&t, code, 1) != 0;
	cm_tree_free(&t);
	return found;
}

static int
hasFlag(const char *s, int extensions, uint32_t flag)
{
	cm_tree t;
	size_t i;
	int found = 0;

	parseString(s, 0, extensions, &t);
	for (i = 1; i <= t.nodes; i++) {
		found = found || (word(&t, i, CM_W_FLAGS) & flag);
	}
	cm_tree_free(&t);
	return found;
}

static char *
render(const char *s, int format, int options, int extensions, int width, int *ascii)
{
	cm_source src = bytes(s, strlen(s));
	cm_output o;
	uint64_t code;
	char *copy;
	int status = cm_render(&src, format, options, extensions, width, -1, &o, &code);

	check(status == CM_OK, "render of %s: status %d", s, status);
	check(cm_track_live_blocks() == 0, "a render left blocks");
	if (status != CM_OK) {
		return calloc(1, 1);
	}
	check(code == o.size, "render: the code is not the size");
	copy = malloc(o.size + 1);
	memcpy(copy, o.bytes, o.size);
	copy[o.size] = 0;
	if (ascii) {
		*ascii = o.ascii;
	}
	cm_output_free(&o);
	return copy;
}

static void
checkRender(const char *s, int format, int options, int extensions, int width, const char *want)
{
	char *got = render(s, format, options, extensions, width, NULL);

	check(strcmp(got, want) == 0, "render %s of %s: %s", formatName(format), s, got);
	free(got);
}

static void
testExtensionsAndOptions(void)
{
	const char *table = "| a |\n|---|\n| b |\n";
	cm_tree t;
	char *s;
	int i;

	check(!hasCode(table, 0, 0, CM_CODE_TABLE) && hasCode(table, 0, 0, 0x8008), "mask 0: the table is a paragraph");
	for (i = 0; i < 5; i++) {
		int bit = 1 << i;
		check(hasCode(table, 0, bit, CM_CODE_TABLE) == (bit == 1), "mask %d: table", bit);
		check(hasCode("~~s~~\n", 0, bit, CM_CODE_STRIKETHROUGH) == (bit == 2), "mask %d: strikethrough", bit);
		check(hasCode("see www.pharo.org\n", 0, bit, 0xc009) == (bit == 4), "mask %d: autolink", bit);
		check(hasFlag("- [x] t\n", bit, CM_F_TASK) == (bit == 16), "mask %d: tasklist", bit);
	}
	check(hasCode(table, 0, 31, CM_CODE_TABLE) && hasCode("~~s~~\n", 0, 31, CM_CODE_STRIKETHROUGH)
	      && hasCode("see www.pharo.org\n", 0, 31, 0xc009) && hasFlag("- [x] t\n", 31, CM_F_TASK), "mask 31");
	/* tagfilter changes nothing in the tree */
	check(parseString("<script>x</script>\n", 0, 8, &t) == CM_OK && findCode(&t, 0x8006, 1), "tagfilter: tree");
	cm_tree_free(&t);

	/* SMART: curly quotes in a TEXT literal */
	check(parseString("\"q\" -- it's...\n", SMART, 0, &t) == CM_OK, "smart: status");
	check(t.stringBytes > 0 && strstr((const char *)t.strings, "\xE2\x80\x9C") != NULL, "smart: no curly quote");
	cm_tree_free(&t);
	check(parseString("\"q\"\n", 0, 0, &t) == CM_OK && t.stringBytes == 3 && memcmp(t.strings, "\"q\"", 3) == 0,
		"no smart: straight quotes");
	cm_tree_free(&t);
	/* FOOTNOTES on and off */
	check(hasCode("a[^1]\n\n[^1]: b\n", FOOTNOTES, 0, 0xc00b) && hasCode("a[^1]\n\n[^1]: b\n", FOOTNOTES, 0, 0x800b),
		"footnotes on");
	check(!hasCode("a[^1]\n\n[^1]: b\n", 0, 0, 0xc00b) && !hasCode("a[^1]\n\n[^1]: b\n", 0, 0, 0x800b),
		"footnotes off");
	/* STRIKETHROUGH_DOUBLE_TILDE: ~x~ stays text */
	check(hasCode("~x~\n", 0, 2, CM_CODE_STRIKETHROUGH) && !hasCode("~x~\n", DOUBLE_TILDE, 2, CM_CODE_STRIKETHROUGH)
	      && hasCode("~~x~~\n", DOUBLE_TILDE, 2, CM_CODE_STRIKETHROUGH), "double tilde");
	/* HARDBREAKS, NOBREAKS, UNSAFE, SOURCEPOS of a render */
	checkRender("a\nb\n", CM_FMT_HTML, 0, 0, 0, "<p>a\nb</p>\n");
	checkRender("a\nb\n", CM_FMT_HTML, HARDBREAKS, 0, 0, "<p>a<br />\nb</p>\n");
	checkRender("a\nb\n", CM_FMT_HTML, NOBREAKS, 0, 0, "<p>a b</p>\n");
	checkRender("<b>x</b>\n\n<div>y</div>\n", CM_FMT_HTML, 0, 0, 0,
		"<p><!-- raw HTML omitted -->x<!-- raw HTML omitted --></p>\n<!-- raw HTML omitted -->\n");
	checkRender("<b>x</b>\n\n<div>y</div>\n", CM_FMT_HTML, UNSAFE, 0, 0, "<p><b>x</b></p>\n<div>y</div>\n");
	checkRender("a\n", CM_FMT_HTML, SOURCEPOS, 0, 0, "<p data-sourcepos=\"1:1-1:1\">a</p>\n");
	/* the parse always has SOURCEPOS: right positions after a multi-line
	   code span (U/cmark.md 3.5) */
	check(parseString("a `b\nc` *d*\n", 0, 0, &t) == CM_OK, "multi-line span: status");
	i = (int)findCode(&t, 0xc007, 1);
	check(i && word(&t, (size_t)i, CM_W_SL) == 2 && word(&t, (size_t)i, CM_W_SC) == 4, "multi-line span: emphasis at %u:%u",
		i ? word(&t, (size_t)i, CM_W_SL) : 0, i ? word(&t, (size_t)i, CM_W_SC) : 0);
	cm_tree_free(&t);
	s = render("x\n", CM_FMT_HTML, 0, 0, 0, NULL);
	free(s);
	printf("5. extension masks: ok\n6. options: ok\n");
}


/*** 7. Depth, 8. limits ***/

static void
testDepth(void)
{
	Buf b = {0};
	cm_source src;
	cm_tree t;
	uint64_t code;
	size_t deep = WASM ? 200000 : 1000000;
	clock_t start;

	brepeat(&b, ">", 1000);
	bput(&b, " x\n", 3);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_OK && t.depth == 1003 && t.nodes == 1003,
		"1000 quotes: depth %u", t.depth);
	invariants("1000 quotes", &t);
	cm_tree_free(&t);
	check(parse(&src, 0, 0, 1003, -1, &t, &code) == CM_OK, "1000 quotes, maxDepth 1003");
	cm_tree_free(&t);
	check(parse(&src, 0, 0, 1001, -1, &t, &code) == CM_TOO_DEEP && code == 1002 && t.depth == 1002 && !t.tape,
		"1000 quotes, maxDepth 1001: code %llu", (unsigned long long)code);
	check(parse(&src, 0, 0, 500, -1, &t, &code) == CM_TOO_DEEP && code == 501 && t.depth == 501 && t.sourceChars == b.n,
		"1000 quotes, maxDepth 500: code %llu", (unsigned long long)code);
	check(parse(&src, 0, 0, 1, -1, &t, &code) == CM_TOO_DEEP && code == 2, "maxDepth 1");
	bfree(&b);

	brepeat(&b, ">", deep);
	bput(&b, " x\n", 3);
	src = bytes(b.s, b.n);
	start = clock();
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_OK && t.depth == deep + 3, "%zu quotes: status, depth %u",
		deep, t.depth);
	invariants("deep quotes", &t);
	cm_tree_free(&t);
	printf("7. depth: ok (%zu nested quotes in %.2f s)\n", deep, (double)(clock() - start) / CLOCKS_PER_SEC);
	bfree(&b);
}

static void
testLimits(void)
{
	char l[501];
	Buf b = {0};
	cm_source src;
	cm_tree t;
	uint64_t code;

	cm_test_limits(1000, 0);
	brepeat(&b, "a", 1000);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_OK, "1000 bytes: status");
	cm_tree_free(&t);
	bput(&b, "b", 1);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_TOO_LARGE && code == 1001 && !t.tape, "1001 bytes: code %llu",
		(unsigned long long)code);
	/* the size is T's: 500 characters of Latin-1 above 0x7F are 1000 bytes */
	memset(l, 0xE9, sizeof l);
	src = latin1(l, 500);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_OK, "500 Latin-1 characters");
	cm_tree_free(&t);
	src = latin1(l, 501);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_TOO_LARGE && code == 1002, "501 Latin-1 characters");
	check(cm_render(&src, CM_FMT_HTML, 0, 0, 0, -1, &(cm_output){0}, &code) == CM_TOO_LARGE, "render: too large");
	cm_test_limits(0, 0);
	bfree(&b);

	cm_test_limits(0, 4096);
	brepeat(&b, "word ", 2000);
	src = bytes(b.s, b.n);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_C_LIMIT && code == CM_CLIM_BLOCK, "block cap: status, code %llu",
		(unsigned long long)code);
	check(cm_track_live_blocks() == 0, "block cap: blocks left");
	cm_test_limits(0, 0);
	check(parse(&src, 0, 0, 0, -1, &t, &code) == CM_OK, "block cap restored");
	cm_tree_free(&t);
	/* a budget of 0 */
	check(parse(&src, 0, 0, 0, 0, &t, &code) == CM_C_LIMIT && code == CM_CLIM_BUDGET, "budget 0");
	bfree(&b);
	printf("8. limits: ok\n");
}


/*** 9. The C-limit sweep and the special characters ***/

static char *specialGolden;
static size_t specialGoldenSize;

static void
checkSpecialCharacters(const char *after)
{
	cm_tree t;
	Buf b = {0};

	if (parseString("www:a~b~*c*", 0, 0, &t) != CM_OK) {
		check(0, "special characters: status");
		return;
	}
	dumpTree(&b, &t);
	if (!sameText("special characters", b.s, b.n, specialGolden, specialGoldenSize)) {
		printf("after %s:\n%s", after, b.s);
	}
	cm_tree_free(&t);
	bfree(&b);
}

static int sweepCalls, sweepLandings;

static void
sweepOne(const cm_source *src, int render, int format, uint64_t budget)
{
	cm_tree t;
	cm_output o;
	uint64_t code;
	int status = render ? cm_render(src, format, FOOTNOTES, 31, 0, (int64_t)budget, &o, &code)
		: cm_parse(src, FOOTNOTES, 31, 0, (int64_t)budget, &t, &code);

	sweepCalls++;
	check(status == CM_OK || status == CM_C_LIMIT, "sweep: status %d", status);
	check(cm_track_live_blocks() == 0, "sweep: %zu blocks left", cm_track_live_blocks());
	if (status == CM_OK) {
		render ? cm_output_free(&o) : cm_tree_free(&t);
	} else {
		char what[64];
		sweepLandings++;
		check(code >= 1 && code <= 3, "sweep: code %llu", (unsigned long long)code);
		snprintf(what, sizeof what, "%s with a budget of %llu", render ? formatName(format) : "parse",
			(unsigned long long)budget);
		checkSpecialCharacters(what);
	}
}

static void
testSweep(void)
{
	size_t size;
	char *md = readFile("sample.md", &size);
	cm_source src = bytes(md, size);
	int pass;

	specialGolden = readFile("special-chars.tree", &specialGoldenSize);
	checkSpecialCharacters("nothing");
	for (pass = 0; pass <= 6; pass++) {
		uint64_t need = needOf(&src, FOOTNOTES, 31, pass > 0, pass), budget, j;
		for (budget = 1; budget < need; budget *= 2) {
			sweepOne(&src, pass > 0, pass, budget);
		}
		for (j = 32; j <= 66; j++) {
			sweepOne(&src, pass > 0, pass, need * j / 64);
		}
		sweepOne(&src, pass > 0, pass, need - 1);
		sweepOne(&src, pass > 0, pass, need);
	}
	free(md);
	printf("9. C-limit sweep: ok (%d calls, %d landings)\n", sweepCalls, sweepLandings);
}


/*** 10. The CommonMark blow-up ***/

static void
testBlowUp(void)
{
	Buf b = {0};
	cm_source src;
	cm_output o;
	uint64_t code;
	int status;

	brepeat(&b, ">", 3000);
	bput(&b, " x\n", 3);
	brepeat(&b, "ab\n", 400000);
	src = bytes(b.s, b.n);
	status = cm_render(&src, CM_FMT_COMMONMARK, 0, 0, 0, (int64_t)64 << 20, &o, &code);
	check(status == CM_C_LIMIT && (code == CM_CLIM_BUDGET || code == CM_CLIM_BLOCK),
		"blow-up, 64 MiB: status %d, code %llu", status, (unsigned long long)code);
	check(cm_track_live_blocks() == 0, "blow-up: blocks left");
	if (!WASM) {
		status = cm_render(&src, CM_FMT_COMMONMARK, 0, 0, 0, -1, &o, &code);
		check(status == CM_C_LIMIT && code == CM_CLIM_BLOCK, "blow-up, default budget: status %d, code %llu",
			status, (unsigned long long)code);
	}
	/* the tree of 400,000 lines takes more than the 128 MiB of the wasm
	   default budget: there the HTML is of 100,000 lines */
	if (WASM) {
		bfree(&b);
		brepeat(&b, ">", 3000);
		bput(&b, " x\n", 3);
		brepeat(&b, "ab\n", 100000);
		src = bytes(b.s, b.n);
	}
	status = cm_render(&src, CM_FMT_HTML, 0, 0, 0, -1, &o, &code);
	check(status == CM_OK, "blow-up as html: status %d", status);
	if (status == CM_OK) {
		cm_output_free(&o);
	}
	status = cm_render(&src, CM_FMT_COMMONMARK, 0, 0, 0, (int64_t)64 << 20, &o, &code);
	check(status == CM_C_LIMIT, "blow-up (%zu bytes), 64 MiB: status %d", b.n, status);
	checkSpecialCharacters("the blow-up");
	bfree(&b);
	printf("10. commonmark blow-up: ok\n");
}


/*** 11. Renders ***/

static void
testRenders(void)
{
	static const char *const files[] = { NULL, "sample.html", "sample.xml", "sample.out.md", "sample.txt",
		"sample.tex", "sample.man" };
	size_t size;
	char *md = readFile("sample.md", &size);
	int format, ascii = -1;
	char *s;

	for (format = CM_FMT_HTML; format <= CM_FMT_MAN; format++) {
		Buf b = {0};
		s = render(md, format, FOOTNOTES, 31, 0, NULL);
		bput(&b, s, strlen(s));
		sameAsFixture(files[format], &b, files[format]);
		free(s);
		bfree(&b);
	}
	checkRender("<script>x</script>\n\n<b>y</b>\n", CM_FMT_HTML, UNSAFE, 8, 0,
		"&lt;script>x&lt;/script>\n<p><b>y</b></p>\n");
	checkRender("<script>x</script>\n\n<b>y</b>\n", CM_FMT_HTML, UNSAFE, 0, 0,
		"<script>x</script>\n<p><b>y</b></p>\n");
	s = render("one two three four five six seven eight nine ten eleven\n", CM_FMT_PLAINTEXT, 0, 0, 20, NULL);
	{
		char *line = s, *nl;
		int lines = 0;
		while ((nl = strchr(line, '\n'))) {
			check(nl - line <= 20, "width 20: a line of %d", (int)(nl - line));
			lines++;
			line = nl + 1;
		}
		check(lines >= 3, "width 20: %d lines", lines);
	}
	free(s);
	free(render("a\n", CM_FMT_HTML, 0, 0, 0, &ascii));
	check(ascii == 1, "ascii: a");
	free(render("\xC3\xA9\n", CM_FMT_HTML, 0, 0, 0, &ascii));
	check(ascii == 0, "ascii: \xC3\xA9");
	free(render("", CM_FMT_HTML, 0, 0, 0, &ascii));
	check(ascii == 1, "ascii: the empty output");
	free(md);
	printf("11. renders: ok\n");
}


/*** 12. Determinism, 13. the kept result ***/

static void
testDeterminism(void)
{
	size_t size;
	char *md = readFile("sample.md", &size);
	cm_source src = bytes(md, size);
	cm_tree a, b;
	uint64_t code;

	check(cm_init() == 0 && cm_init() == 0, "cm_init twice");
	check(parse(&src, FOOTNOTES, 31, 0, -1, &a, &code) == CM_OK, "determinism: first");
	check(cm_init() == 0, "cm_init after a parse");
	check(parse(&src, FOOTNOTES, 31, 0, -1, &b, &code) == CM_OK, "determinism: second");
	check(a.nodes == b.nodes && memcmp(a.tape, b.tape, a.nodes * CM_REC_WORDS * 4) == 0
	      && a.stringBytes == b.stringBytes && memcmp(a.strings, b.strings, a.stringBytes) == 0,
	      "determinism: the tapes differ");
	check(cm_extension_count() == 5 && strcmp(cm_extension_name(0), "table") == 0
	      && strcmp(cm_extension_name(4), "tasklist") == 0 && cm_extension_name(5) == NULL, "extension names");
	check(strcmp(cm_version(), "0.29.0.gfm.14") == 0, "version %s", cm_version());
	check(cm_extensions_present(31) && !cm_extensions_present(32), "cm_extensions_present");
	cm_test_drop_extension(16);
	check(!cm_extensions_present(16) && cm_extensions_present(15) && cm_extension_count() == 4, "drop tasklist");
	cm_test_drop_extension(-1);
	check(cm_extensions_present(31) && cm_extension_count() == 5, "drop restored");
	cm_tree_free(&a);
	cm_tree_free(&b);
	free(md);
	printf("12. determinism: ok\n");
}

static void
testKept(void)
{
	size_t size;
	char *md = readFile("sample.md", &size);
	cm_source src = bytes(md, size);
	cm_tree t, u;
	cm_output o, p;
	uint64_t code, parses;
	size_t keptBytes;
	static const uint8_t bad[] = { 0x61, 0xFF, 0x62 };
	static const uint32_t good[] = { 0x61, 0xFFFD, 0x62 };
	cm_source badSrc = bytes(bad, 3), goodSrc = utf32(good, 3);

	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "kept: parse");
	check(cm_parse(&src, FOOTNOTES, 31, 0, -1, &u, &code) == CM_OK && !u.reused, "kept: nothing kept");
	cm_tree_free(&u);
	cm_keep_tree(FOOTNOTES, 31, 0, &t);
	check(t.tape == NULL && cm_kept_bytes() > size, "kept: %zu bytes kept", cm_kept_bytes());
	parses = cm_get_stats()->parses;
	check(parse(&src, FOOTNOTES, 31, 0, -1, &u, &code) == CM_OK && u.reused == 1, "kept: reused");
	check(cm_get_stats()->parses == parses && cm_kept_bytes() == 0, "kept: parsed again");
	cm_keep_tree(FOOTNOTES, 31, 0, &u);
	keptBytes = cm_kept_bytes();
	check(parse(&src, 0, 31, 0, -1, &t, &code) == CM_OK && !t.reused && cm_kept_bytes() == 0
	      && cm_get_stats()->parses == parses + 1, "kept: another key");
	cm_tree_free(&t);
	check(cm_release_kept() == 0, "kept: released twice");
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "kept: parse again");
	cm_keep_tree(FOOTNOTES, 31, 0, &t);
	check(cm_kept_bytes() == keptBytes && cm_release_kept() == keptBytes && cm_kept_bytes() == 0, "kept: release");
	/* another maxDepth is another key */
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "kept: parse");
	cm_keep_tree(FOOTNOTES, 31, 0, &t);
	check(parse(&src, FOOTNOTES, 31, 100, -1, &t, &code) == CM_OK && !t.reused, "kept: maxDepth is in the key");
	cm_tree_free(&t);
	/* keyed on T: a ByteArray with a bad byte and the String of its T */
	check(parse(&badSrc, 0, 0, 0, -1, &t, &code) == CM_OK && t.t.differs == 1, "kept: bad bytes");
	cm_keep_tree(0, 0, 0, &t);
	check(parse(&goodSrc, 0, 0, 0, -1, &t, &code) == CM_OK && t.reused && t.t.differs == 0, "kept: T is the key");
	cm_keep_tree(0, 0, 0, &t);
	check(parse(&badSrc, 0, 0, 0, -1, &t, &code) == CM_OK && t.reused && t.t.differs == 1,
		"kept: differs comes from the call");
	cm_tree_free(&t);
	/* over the keep cap: not kept */
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK, "kept: parse");
	cm_test_keep_cap(100);
	cm_keep_tree(FOOTNOTES, 31, 0, &t);
	check(cm_kept_bytes() == 0 && t.tape == NULL, "kept: over the keep cap");
	cm_test_keep_cap(0);
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK && !t.reused, "kept: nothing reused");
	/* a kept result counts against the next call's C budget until the call
	   frees it (a key that does not match): with a budget that T alone
	   fits, the landing comes while T is built, before the key check, so
	   the kept result is still there after it */
	cm_keep_tree(FOOTNOTES, 31, 0, &t);
	check(cm_kept_bytes() > 0, "kept: kept");
	check(parse(&src, 0, 31, 0, (int64_t)size + 64, &u, &code) == CM_C_LIMIT && code == CM_CLIM_BUDGET,
		"kept: charged to the next call");
	check(cm_kept_bytes() > 0, "kept: still kept after a landing before the key check");
	cm_release_kept();
	check(parse(&src, 0, 31, 0, (int64_t)size + 64, &u, &code) == CM_C_LIMIT && cm_kept_bytes() == 0,
		"kept: the same budget without a kept result");
	/* renders */
	check(cm_render(&src, CM_FMT_HTML, FOOTNOTES, 31, 0, -1, &o, &code) == CM_OK, "kept: render");
	check(cm_track_live_blocks() == 0 && o.bytes && o.size == code, "kept: the output is not a tracked block");
	cm_keep_output(CM_FMT_HTML, FOOTNOTES, 31, 0, &o);
	check(cm_kept_bytes() > 0 && o.bytes == NULL, "kept: output kept");
	parses = cm_get_stats()->renders;
	check(cm_render(&src, CM_FMT_HTML, FOOTNOTES, 31, 0, -1, &p, &code) == CM_OK && p.reused
	      && cm_get_stats()->renders == parses, "kept: render reused");
	cm_keep_output(CM_FMT_HTML, FOOTNOTES, 31, 0, &p);
	check(cm_render(&src, CM_FMT_HTML, FOOTNOTES, 31, 80, -1, &p, &code) == CM_OK && !p.reused, "kept: width is in the key");
	cm_keep_output(CM_FMT_HTML, FOOTNOTES, 31, 80, &p);
	check(parse(&src, FOOTNOTES, 31, 0, -1, &t, &code) == CM_OK && !t.reused && cm_kept_bytes() == 0,
		"kept: an output is not a tree");
	cm_tree_free(&t);
	check(cm_render(&src, CM_FMT_XML, FOOTNOTES, 31, 0, -1, &o, &code) == CM_OK, "kept: xml");
	cm_output_free(&o);
	free(md);
	printf("13. kept result: ok\n");
}


/*** 14. Known gfm.14 behaviour ***/

static void
testKnownBugs(void)
{
	cm_tree t;
	char *s;
	size_t r;
	int found = 0;

	/* 1. gfm.14's footnote label regression (upstream's extensions.txt
	   example 25 fails at the tag): a label holding an inline is cut and
	   the reference stays text, its source text dropped.  Candidate patch
	   0001-footnote-label-from-source.patch would turn this into a
	   resolved reference. */
	check(parseString("Hi[^a<b>c].\n\n[^a<b>c]: def\n", FOOTNOTES, 31, &t) == CM_OK, "known 1: status");
	for (r = 1; r <= t.nodes; r++) {
		found = found || (word(&t, r, CM_W_CODE) == 0xc001 && word(&t, r, CM_W_S1LEN) == 7
			&& memcmp(t.strings + word(&t, r, CM_W_S1OFF), "Hi[^a].", 7) == 0);
	}
	check(found && !findCode(&t, 0xc00b, 1) && !findCode(&t, 0x800b, 1), "known 1: TEXT Hi[^a]., no footnote");
	cm_tree_free(&t);
	checkRender("Hi[^a<b>c].\n\n[^a<b>c]: def\n", CM_FMT_HTML, FOOTNOTES, 31, 0, "<p>Hi[^a].</p>\n");
	/* 2. cmark_node_get_type_string answers "<unknown>" for the footnote
	   nodes, so the XML is not well-formed.  Patch 0002 would name them. */
	s = render("a[^1]\n\n[^1]: b\n", CM_FMT_XML, FOOTNOTES, 0, 0, NULL);
	check(strstr(s, "<<unknown>") != NULL, "known 2: no <<unknown> in the XML");
	free(s);
	/* 3. the columns of <https://...> autolinks are relative to the
	   paragraph (patch 0003): clamped, see testPositions */
	printf("14. known gfm.14 behaviour: ok\n");
}


/*** 15. Stress inputs, 16. fuzz-lite ***/

typedef struct {
	const char *name;
	int ok;		/* 1: status 0 expected; 0: 0 or a C limit (wasm) */
} Stress;

static void
stressInput(Buf *b, int k)
{
	size_t i;

	switch (k) {
	case 0: brepeat(b, ">", 50000); bput(b, " deep\n", 6); break;
	case 1: for (i = 0; i < 2000; i++) { brepeat(b, "  ", i); bput(b, "- item\n", 7); } break;
	case 2: brepeat(b, "[", 100000); bput(b, "x", 1); brepeat(b, "]", 100000); bput(b, "\n", 1); break;
	case 3: brepeat(b, "*a **", 50000); bput(b, "\n", 1); break;
	case 4: for (i = 1; i < 2000; i++) { size_t j; for (j = 0; j < i; j++) bput(b, "`", 1); bput(b, "x", 1); }
		bput(b, "\n", 1); break;
	case 5: bput(b, "|", 1); brepeat(b, "h|", 2000); bput(b, "\n|", 2); brepeat(b, "-|", 2000); bput(b, "\n", 1);
		brepeat(b, "|x\n", 3000); break;
	case 6: {
		static const char s[] = "[^] [^ [^x [^x] [^\\]] [^a<b>c] [^*e*] [^`c`]\n\n[^x]: X\n[^*e*]: E\n[^`c`]: C\n";
		bput(b, s, sizeof s - 1);
		break;
	}
	case 7: {
		static const char s[] = "a\0b \xff\xfe bad utf8\n", r[] = "\0\xC3\xFF\xED\xA0\x80 *x*\r\n";
		bput(b, s, sizeof s - 1);
		for (i = 0; i < 2000; i++) {
			bput(b, r, sizeof r - 1);
		}
		break;
	}
	case 8: brepeat(b, "word ", WASM ? 400000 : 2000000); bput(b, "\n", 1); break;
	case 9: brepeat(b, "[a](", 20000); bput(b, "\n", 1); break;
	default: brepeat(b, "www.", 100000); bput(b, "\n", 1); brepeat(b, "x@", 100000); bput(b, "\n", 1); break;
	}
}

static void
testStress(void)
{
	static const char *const names[] = { "quotes", "lists", "brackets", "emph", "backticks", "table", "fnedge",
		"nul", "long", "links", "autolink" };
	int k;

	for (k = 0; k < 11; k++) {
		Buf b = {0};
		cm_source src;
		cm_tree t;
		cm_output o;
		uint64_t code;
		int status, parsed;
		clock_t start = clock();
		double seconds;

		stressInput(&b, k);
		src = bytes(b.s, b.n);
		status = parsed = parse(&src, FOOTNOTES, 31, 0, -1, &t, &code);
		check(status == CM_OK || (WASM && status == CM_C_LIMIT), "stress %s: status %d", names[k], status);
		if (status == CM_OK) {
			invariants(names[k], &t);
			cm_tree_free(&t);
		}
		status = cm_render(&src, CM_FMT_HTML, FOOTNOTES, 31, 0, -1, &o, &code);
		check(status == CM_OK || (WASM && status == CM_C_LIMIT), "stress %s: html status %d", names[k], status);
		check(cm_track_live_blocks() == 0, "stress %s: blocks left", names[k]);
		if (status == CM_OK) {
			cm_output_free(&o);
		}
		seconds = (double)(clock() - start) / CLOCKS_PER_SEC;
		check(seconds < 10, "stress %s: %.2f s", names[k], seconds);
		printf("   stress %-9s %8zu bytes, status %d and %d (html), %.2f s\n", names[k], b.n, parsed, status,
			seconds);
		bfree(&b);
	}
	printf("15. stress: ok\n");
}

static uint64_t rng = 0x9E3779B97F4A7C15u;

static uint64_t
xorshift(void)
{
	rng ^= rng << 13;
	rng ^= rng >> 7;
	rng ^= rng << 17;
	return rng;
}

static void
testFuzz(void)
{
	static const char alphabet[] = "*_`[]()!<>#-+=|~:^\\&;\"'.\n\r\t abcwx1\0\xC3\xFF";
	int n, count = 20000;
	char input[301];

	for (n = 0; n < count; n++) {
		size_t len = (size_t)(xorshift() % 301), i;
		cm_source src;
		cm_tree t;
		cm_output o;
		uint64_t code;
		char name[32];

		for (i = 0; i < len; i++) {
			input[i] = alphabet[xorshift() % (sizeof alphabet - 1)];
		}
		src = bytes(input, len);
		snprintf(name, sizeof name, "fuzz %d", n);
		check(parse(&src, n & 1 ? FOOTNOTES : 0, 31, 0, -1, &t, &code) == CM_OK, "%s: status", name);
		invariants(name, &t);
		cm_tree_free(&t);
		check(cm_render(&src, CM_FMT_HTML, n & 1 ? FOOTNOTES : 0, 31, 0, -1, &o, &code) == CM_OK, "%s: html", name);
		check(cm_track_live_blocks() == 0, "%s: blocks left", name);
		cm_output_free(&o);
	}
	printf("16. fuzz-lite: ok (%d inputs)\n", count);
}


/*** Tools ***/

static int
dumpFile(const char *file, int options, int extensions)
{
	size_t size;
	char *md = readFile(file, &size);
	cm_source src = bytes(md, size);
	cm_tree t;
	uint64_t code;
	Buf b = {0};
	int status = cm_parse(&src, options, extensions, 0, -1, &t, &code);

	if (status != CM_OK) {
		printf("status %d code %llu\n", status, (unsigned long long)code);
		return 1;
	}
	dumpTree(&b, &t);
	fwrite(b.s, 1, b.n, stdout);
	cm_tree_free(&t);
	bfree(&b);
	free(md);
	return 0;
}

int
main(int argc, char **argv)
{
	if (argc >= 5 && strcmp(argv[1], "--dump") == 0) {
		return dumpFile(argv[2], atoi(argv[3]), atoi(argv[4]));
	}
	if (argc >= 7 && strcmp(argv[1], "--render") == 0) {
		size_t size;
		char *md = readFile(argv[2], &size);
		cm_source src = bytes(md, size);
		cm_output o;
		uint64_t code;
		if (cm_render(&src, atoi(argv[3]), atoi(argv[4]), atoi(argv[5]), atoi(argv[6]), -1, &o, &code) != CM_OK) {
			return 1;
		}
		fwrite(o.bytes, 1, o.size, stdout);
		cm_output_free(&o);
		free(md);
		return 0;
	}
	if (argc >= 2 && strcmp(argv[1], "--special") == 0) {
		cm_tree t;
		Buf b = {0};
		parseString("www:a~b~*c*", 0, 0, &t);
		dumpTree(&b, &t);
		fwrite(b.s, 1, b.n, stdout);
		return 0;
	}
	if (argc >= 2) {
		fixtures = argv[1];
	}
	setvbuf(stdout, NULL, _IONBF, 0);
	check(cm_init() == 0, "cm_init");
	check(cm_default_c_budget() == (WASM ? UINT64_C(128) << 20 : UINT64_C(4) << 30), "default C budget");
	check(cm_keep_cap() == (WASM ? UINT64_C(8) << 20 : UINT64_C(4) << 30), "keep cap");
	testSanitising();
	testPositions();
	testSample();
	testCoverage();
	testExtensionsAndOptions();
	testDepth();
	testLimits();
	testSweep();
	testBlowUp();
	testRenders();
	testDeterminism();
	testKept();
	testKnownBugs();
	testStress();
	testFuzz();
	cm_release_kept();
	check(cm_track_live_blocks() == 0, "blocks left at the end");
	free(specialGolden);
	printf("enginetest: %d checks, %d failed\n", checks, failures);
	return failures ? 1 : 0;
}
