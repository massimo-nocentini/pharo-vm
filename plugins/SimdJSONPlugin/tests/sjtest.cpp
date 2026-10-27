/* sjtest.cpp -- C-level tests of the SimdJSONPlugin engine
 *
 * Links sjEngine.cpp (compiled with -DSJ_TEST_HOOKS), sjText.c and the
 * simdjson amalgamation with a fake builder in place of SimdJSONPlugin.c:
 * its objects are C++ nodes, it follows the builder rules of the glue
 * (number texts and their fixups, keys and the key set, Dictionary
 * insertion with the last duplicate winning), it computes Spur sizes with
 * its own table rather than with sj_spur_*, and it can fail its k-th
 * allocation.  The checks:
 *
 *   T1  the value-end scanner against parse_many, on UTF-8, Latin-1 and
 *       UTF-32 sources: crafted cases and random streams of values
 *   T2  the number-token enumerator and the exponent gate against
 *       On-Demand raw_json_token() and the structural-index replay, on the
 *       number kinds and the corpus; a broken token list gives 1002
 *   T3  the number policy: integers, uint64, big integers, floats and
 *       exponent tokens with and without EXACT
 *   T4  transcoding of Latin-1 and UTF-32 sources, invalid code points,
 *       unit indexes of next0
 *   T5  the key set, symbol keys, Dictionary building and duplicate keys
 *   T6  the measure equals the bytes built, and stays under sj_bound, on
 *       the corpus and on the worst shapes, in every build mode
 *   T7  idempotence: failed builds and budget refusals reuse the parse
 *   T8  edges: BOM, blanks, depth, {} and [], white space after a value,
 *       messages; with SJ_SLOW=1, an array of 16,777,216 elements
 *   T10 (--report) the cost of parse, gate, enumeration, measure and walk
 *   T11 the texts of the writer: strings and their escapes (Latin-1,
 *       code points, asciiOnly), integers of any size against a decimal
 *       conversion of their own, and doubles: Float>>printOn:'s layout,
 *       its rounding of ties, and the round trip of random bit patterns
 *
 * Usage: sjtest [--corpus DIR] [--only T1,T2,...] [--slow] [--report]
 * DIR is the jsonexamples directory (default: $SJ_CORPUS); without it the
 * corpus checks are skipped.  Prints one line per failed check and a
 * summary; exits 1 when a check failed.
 */

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <random>
#include <string>
#include <unordered_map>
#include <vector>

#include "simdjson.h"
#include "sjBridge.h"
#include "sjText.h"

extern "C" int sj_test_number_fault;

using namespace simdjson;

static int checks, failures;
static const char *currentTest = "";

static void
fail(int line, const char *format, ...)
{
	va_list args;

	failures++;
	if (failures > 200)
		return;
	printf("FAIL %s sjtest.cpp:%d: ", currentTest, line);
	va_start(args, format);
	vprintf(format, args);
	va_end(args);
	printf("\n");
}

#define CHECK(condition) (checks++, (condition) ? true : (fail(__LINE__, "%s", #condition), false))
#define CHECKF(condition, ...) (checks++, (condition) ? true : (fail(__LINE__, __VA_ARGS__), false))

/*** UTF-8, written again here ***/

static std::u32string
decodeUtf8(const char *s, size_t n)
{
	std::u32string out;
	size_t i = 0;

	while (i < n) {
		unsigned char c = (unsigned char)s[i];
		char32_t cp;
		int k;
		if (c < 0x80) { cp = c; k = 1; }
		else if (c < 0xE0) { cp = c & 0x1F; k = 2; }
		else if (c < 0xF0) { cp = c & 0x0F; k = 3; }
		else { cp = c & 0x07; k = 4; }
		for (int j = 1; j < k && i + j < n; j++)
			cp = (cp << 6) | ((unsigned char)s[i + j] & 0x3F);
		out += cp;
		i += k;
	}
	return out;
}

static std::string
encodeUtf8(const std::u32string &cps)
{
	std::string out;

	for (char32_t cp : cps) {
		if (cp < 0x80) out += char(cp);
		else if (cp < 0x800) { out += char(0xC0 | (cp >> 6)); out += char(0x80 | (cp & 0x3F)); }
		else if (cp < 0x10000) { out += char(0xE0 | (cp >> 12)); out += char(0x80 | ((cp >> 6) & 0x3F)); out += char(0x80 | (cp & 0x3F)); }
		else { out += char(0xF0 | (cp >> 18)); out += char(0x80 | ((cp >> 12) & 0x3F)); out += char(0x80 | ((cp >> 6) & 0x3F)); out += char(0x80 | (cp & 0x3F)); }
	}
	return out;
}

/*** the fake builder ***/

/* Spur 64-bit sizes: a header of 8 bytes, a second one of 8 from 255
   slots on, and 8-byte slots, at least one */
static uint64_t
spurSize(uint64_t slots)
{
	if (slots == 0)
		slots = 1;
	return (slots < 255 ? 8 : 16) + slots * 8;
}
static uint64_t byteStringSize(uint64_t n) { return spurSize((n + 7) / 8); }
static uint64_t wideStringSize(uint64_t n) { return spurSize((4 * n + 7) / 8); }

static bool
smallFloat(double d)	/* SmallFloat64: zero, or 2^-127 < |d| < 2^129 */
{
	double a = std::fabs(d);
	return a == 0 || (a > std::ldexp(1.0, -127) && a < std::ldexp(1.0, 129));
}

static size_t
primeCapacity(size_t npairs)	/* smallest prime >= max(5, (npairs+1)*4/3) */
{
	size_t n = std::max<size_t>(5, (npairs + 1) * 4 / 3);
	for (;; n++) {
		bool prime = n > 1;
		for (size_t d = 2; d * d <= n && prime; d++)
			prime = n % d != 0;
		if (prime)
			return n;
	}
}

const int64_t MaxSmall = (int64_t(1) << 60) - 1, MinSmall = -(int64_t(1) << 60);

struct Node {
	enum Kind { NIL, TRUE_, FALSE_, SMALLINT, LARGEINT, SMALLFLOAT, BOXEDFLOAT, TEXT, BYTESTRING, WIDESTRING,
		ARRAY, FLATMAP, DICT, ASSOC } kind;
	int64_t i = 0;
	uint64_t u = 0;
	bool isUnsigned = false;
	double d = 0;
	std::string bytes;		/* TEXT; BYTESTRING as Latin-1 */
	std::u32string wide;
	std::vector<Node *> slots;	/* ARRAY, FLATMAP (k v k v), DICT's array, ASSOC (key value) */
	size_t tally = 0;
	explicit Node(Kind k) : kind(k) {}
};

struct Heap {
	std::vector<std::unique_ptr<Node>> nodes;
	std::unordered_map<int64_t, Node *> smallInts;
	Node nil{Node::NIL}, yes{Node::TRUE_}, no{Node::FALSE_};
	unsigned flags = 0;
	uint64_t bytes = 0, allocations = 0, failAt = 0;
	Node *lastText = nullptr;
	std::vector<std::pair<Node *, size_t>> fixups;	/* container, 1-based slot */
	sj_keyset *keys = nullptr;
	std::vector<Node *> keyStrings;

	~Heap() { sj_keyset_free(keys); }

	/* a heap object of size bytes, or null when it is the one that fails */
	Node *
	allocate(Node::Kind kind, uint64_t size)
	{
		if (failAt && ++allocations == failAt)
			return nullptr;
		if (!failAt)
			allocations++;
		bytes += size;
		nodes.emplace_back(new Node(kind));
		return nodes.back().get();
	}

	Node *
	immediate(Node::Kind kind)
	{
		nodes.emplace_back(new Node(kind));
		return nodes.back().get();
	}

	Node *
	smallInt(int64_t v)
	{
		auto found = smallInts.find(v);
		if (found != smallInts.end())
			return found->second;
		Node *n = immediate(Node::SMALLINT);
		n->i = v;
		smallInts[v] = n;
		return n;
	}

	Node *
	string(const char *s, size_t n)
	{
		std::u32string cps = decodeUtf8(s, n);
		char32_t max = 0;
		for (char32_t c : cps)
			max = std::max(max, c);
		Node *node = max > 0xFF ? allocate(Node::WIDESTRING, wideStringSize(cps.size()))
			: allocate(Node::BYTESTRING, byteStringSize(cps.size()));
		if (!node)
			return nullptr;
		if (max > 0xFF)
			node->wide = cps;
		else
			for (char32_t c : cps)
				node->bytes += char(c);
		return node;
	}

	void
	stored(Node *container, size_t slot1, Node *v)
	{
		if (v == lastText) {
			fixups.emplace_back(container, slot1);
			lastText = nullptr;
		}
	}
};

static Heap *H;
static sj_oop oopOf(Node *n) { return (sj_oop)(intptr_t)n; }
static Node *nodeOf(sj_oop oop) { return (Node *)(intptr_t)oop; }

static uint64_t
keyHashOf(Node *k)
{
	uint64_t h = 1469598103934665603ULL;
	if (k->kind == Node::BYTESTRING)
		for (unsigned char c : k->bytes) h = (h ^ c) * 1099511628211ULL;
	else
		for (char32_t c : k->wide) h = (h ^ c) * 1099511628211ULL;
	return h;
}

static bool
sameKey(Node *a, Node *b)
{
	return a->kind == b->kind && a->bytes == b->bytes && a->wide == b->wide;
}

static sj_oop
fInt64(int64_t v)
{
	if (v >= MinSmall && v <= MaxSmall)
		return oopOf(H->smallInt(v));
	Node *n = H->allocate(Node::LARGEINT, 16);
	if (!n) return 0;
	n->i = v;
	return oopOf(n);
}

static sj_oop
fUint64(uint64_t v)
{
	if (v <= uint64_t(MaxSmall))
		return oopOf(H->smallInt(int64_t(v)));
	Node *n = H->allocate(Node::LARGEINT, 16);
	if (!n) return 0;
	n->u = v;
	n->isUnsigned = true;
	return oopOf(n);
}

static sj_oop
fFloat64(double v)
{
	Node *n = smallFloat(v) ? H->immediate(Node::SMALLFLOAT) : H->allocate(Node::BOXEDFLOAT, 16);
	if (!n) return 0;
	n->d = v;
	return oopOf(n);
}

static sj_oop
fNumberText(const char *s, size_t n)
{
	Node *node = H->allocate(Node::TEXT, byteStringSize(n));
	if (!node) return 0;
	node->bytes.assign(s, n);
	H->lastText = node;
	sj_note_number_text();
	return oopOf(node);
}

static sj_oop
fString(const char *s, size_t n)
{
	return oopOf(H->string(s, n));
}

static sj_oop
fKey(const char *s, size_t n)
{
	int isNew;
	int64_t index;

	if (!(H->flags & SJ_F_SYMKEYS))
		return oopOf(H->string(s, n));
	if ((index = sj_keyset_index(H->keys, s, n, &isNew)) < 0)
		return 0;
	if (isNew) {
		Node *k = H->string(s, n);
		if (!k) return 0;
		H->keyStrings.push_back(k);
	}
	return oopOf(H->smallInt(index));
}

static sj_oop
fArray(size_t n)
{
	Node *a = H->allocate(Node::ARRAY, spurSize(n));
	if (!a) return 0;
	a->slots.assign(n, &H->nil);
	return oopOf(a);
}

static int
fArrayPut(sj_oop array, size_t i0, sj_oop v)
{
	Node *a = nodeOf(array);
	a->slots.at(i0) = nodeOf(v);
	H->stored(a, i0 + 1, nodeOf(v));
	return 0;
}

static sj_oop
fObject(size_t npairs)
{
	if (H->flags & SJ_F_DICT) {
		Node *array = H->allocate(Node::ARRAY, spurSize(primeCapacity(npairs)));
		if (!array) return 0;
		Node *dict = H->allocate(Node::DICT, spurSize(2));
		if (!dict) return 0;
		array->slots.assign(primeCapacity(npairs), &H->nil);
		dict->slots.push_back(array);
		return oopOf(dict);
	}
	Node *map = H->allocate(Node::FLATMAP, spurSize(2 * npairs));
	if (!map) return 0;
	map->slots.assign(2 * npairs, &H->nil);
	return oopOf(map);
}

static int
fObjectPut(sj_oop object, size_t pair0, sj_oop key, sj_oop v)
{
	Node *o = nodeOf(object), *k = nodeOf(key), *value = nodeOf(v);

	if (o->kind == Node::FLATMAP) {
		o->slots.at(2 * pair0) = k;
		o->slots.at(2 * pair0 + 1) = value;
		H->stored(o, 2 * pair0 + 2, value);
		return 0;
	}
	std::vector<Node *> &array = o->slots[0]->slots;
	size_t i = keyHashOf(k) % array.size();
	while (array[i] != &H->nil && !sameKey(array[i]->slots[0], k))
		i = (i + 1) % array.size();
	Node *assoc = array[i];
	if (assoc == &H->nil) {
		if (!(assoc = H->allocate(Node::ASSOC, spurSize(2))))
			return -1;
		assoc->slots = {k, value};
		array[i] = assoc;
		o->tally++;
	} else {
		auto &f = H->fixups;
		f.erase(std::remove(f.begin(), f.end(), std::make_pair(assoc, size_t(2))), f.end());
		assoc->slots[1] = value;
	}
	H->stored(assoc, 2, value);
	return 0;
}

static sj_builder fakeBuilder;

/* A builder that allocates nothing, for timing the walk alone */
static sj_oop nInt64(int64_t) { return 1; }
static sj_oop nUint64(uint64_t) { return 1; }
static sj_oop nFloat64(double) { return 1; }
static sj_oop nBytes(const char *, size_t) { return 1; }
static sj_oop nSize(size_t) { return 1; }
static int nArrayPut(sj_oop, size_t, sj_oop) { return 0; }
static int nObjectPut(sj_oop, size_t, sj_oop, sj_oop) { return 0; }
static const sj_builder nullBuilder = { 1, 1, 1, nInt64, nUint64, nFloat64, nBytes, nBytes, nBytes,
	nSize, nArrayPut, nSize, nObjectPut };

/*** reading through the engine ***/

struct Source {
	std::string bytes;		/* UTF-8 or Latin-1 */
	std::vector<uint32_t> words;	/* UTF-32 */
	int encoding;

	static Source utf8(const std::string &s) { return Source{s, {}, SJ_ENC_UTF8}; }
	static Source latin1(const std::string &s) { return Source{s, {}, SJ_ENC_LATIN1}; }
	static Source
	utf32(const std::u32string &s)
	{
		Source src{"", {}, SJ_ENC_UTF32};
		src.words.assign(s.begin(), s.end());
		return src;
	}
	const void *base() const { return encoding == SJ_ENC_UTF32 ? (const void *)words.data() : (const void *)bytes.data(); }
	size_t units() const { return encoding == SJ_ENC_UTF32 ? words.size() : bytes.size(); }
};

struct Result {
	int rc;
	sj_outcome out;
	Node *root;
	uint64_t bytes;		/* every object the glue would allocate, the result Array included */
	uint64_t allocations;
	std::vector<std::pair<Node *, size_t>> fixups;
	std::vector<Node *> keyStrings;
	unsigned flags;
	std::unique_ptr<Heap> heap;
};

static Result
readWith(const Source &src, size_t start0, size_t stop0, unsigned flags, int64_t budget = -1, uint64_t failAt = 0,
	int64_t maxUnits = -1)
{
	Result r;
	r.heap.reset(new Heap());
	H = r.heap.get();
	H->flags = flags;
	H->failAt = failAt;
	if (flags & SJ_F_SYMKEYS)
		H->keys = sj_keyset_new(16);
	Node *result = H->immediate(Node::ARRAY);	/* the glue allocates it first */
	H->bytes = spurSize(6);
	fakeBuilder = { oopOf(&H->nil), oopOf(&H->yes), oopOf(&H->no), fInt64, fUint64, fFloat64, fNumberText,
		fString, fKey, fArray, fArrayPut, fObject, fObjectPut };
	sj_request rq = { { src.base(), src.units(), src.encoding, start0, stop0 }, flags, budget, maxUnits };
	r.rc = sj_read(&rq, &fakeBuilder, &r.out);
	r.root = nullptr;
	r.flags = flags;
	r.allocations = failAt ? 0 : H->allocations;
	if (r.rc == SJ_RC_DONE && r.out.status == SJ_OK) {
		r.root = nodeOf(r.out.value);
		if (H->lastText) {	/* the root itself */
			H->fixups.emplace_back(result, 2);
			H->lastText = nullptr;
		}
		if (!H->fixups.empty())
			H->bytes += spurSize(2 * H->fixups.size());
		if (flags & SJ_F_SYMKEYS)
			H->bytes += spurSize(H->keyStrings.size());
		r.fixups = H->fixups;
		r.keyStrings = H->keyStrings;
	}
	r.bytes = H->bytes;
	return r;
}

static Result
readAll(const Source &src, unsigned flags, int64_t budget = -1, uint64_t failAt = 0)
{
	return readWith(src, 0, src.units(), flags, budget, failAt);
}

static Result
readText(const std::string &json, unsigned flags = 0, int64_t budget = -1)
{
	return readAll(Source::utf8(json), flags, budget);
}

static bool
ok(const Result &r)
{
	return r.rc == SJ_RC_DONE && r.out.status == SJ_OK;
}

static sj_info
info()
{
	sj_info i;
	sj_get_info(&i);
	return i;
}

enum { CALLS, PARSES, CACHE_HITS, BUILDS, BUILD_FAILURES, OVER_BUDGET, MEASURES, NUMBER_TEXTS,
	GATE_HITS, ENUMERATIONS, PARSE_NANOS, BUILD_NANOS };

/*** canonical forms, to compare values ***/

static void
canonString(const std::u32string &cps, std::string &out)
{
	char32_t max = 0;
	for (char32_t c : cps)
		max = std::max(max, c);
	out += max > 0xFF ? 'w' : 's';
	out += std::to_string(cps.size());
	out += ':';
	out += encodeUtf8(cps);
}

static void
canonDouble(double d, std::string &out)
{
	char b[64];
	snprintf(b, sizeof b, "d%a", d);
	out += b;
}

static void canonNode(Node *n, const Result &r, bool asDict, std::string &out);

static std::string
canonKey(Node *k, const Result &r)
{
	std::string s;
	if (k->kind == Node::SMALLINT && (r.flags & SJ_F_SYMKEYS))
		k = r.keyStrings.at(size_t(k->i - 1));
	canonNode(k, r, false, s);
	return s;
}

static void
canonPairs(std::vector<std::pair<std::string, std::string>> &pairs, bool asDict, std::string &out)
{
	if (asDict) {
		std::vector<std::pair<std::string, std::string>> last;
		for (auto &p : pairs) {
			auto found = std::find_if(last.begin(), last.end(), [&](auto &q) { return q.first == p.first; });
			if (found != last.end()) found->second = p.second; else last.push_back(p);
		}
		std::sort(last.begin(), last.end());
		pairs = last;
	}
	out += asDict ? "D{" : "{";
	for (size_t i = 0; i < pairs.size(); i++) {
		if (i) out += ',';
		out += pairs[i].first + ':' + pairs[i].second;
	}
	out += '}';
}

static void
canonNode(Node *n, const Result &r, bool asDict, std::string &out)
{
	switch (n->kind) {
	case Node::NIL: out += 'n'; break;
	case Node::TRUE_: out += 'T'; break;
	case Node::FALSE_: out += 'F'; break;
	case Node::SMALLINT: out += 'i' + std::to_string(n->i); break;
	case Node::LARGEINT: out += 'i' + (n->isUnsigned ? std::to_string(n->u) : std::to_string(n->i)); break;
	case Node::SMALLFLOAT: case Node::BOXEDFLOAT: canonDouble(n->d, out); break;
	case Node::TEXT: out += 't' + n->bytes; break;
	case Node::BYTESTRING: {
		std::u32string cps;
		for (unsigned char c : n->bytes) cps += char32_t(c);
		canonString(cps, out);
		break;
	}
	case Node::WIDESTRING: canonString(n->wide, out); break;
	case Node::ARRAY:
		out += '[';
		for (size_t i = 0; i < n->slots.size(); i++) {
			if (i) out += ',';
			canonNode(n->slots[i], r, asDict, out);
		}
		out += ']';
		break;
	case Node::FLATMAP: {
		std::vector<std::pair<std::string, std::string>> pairs;
		for (size_t i = 0; i < n->slots.size(); i += 2) {
			std::string v;
			canonNode(n->slots[i + 1], r, asDict, v);
			pairs.emplace_back(canonKey(n->slots[i], r), v);
		}
		canonPairs(pairs, asDict, out);
		break;
	}
	case Node::DICT: {
		std::vector<std::pair<std::string, std::string>> pairs;
		size_t count = 0;
		for (Node *assoc : n->slots[0]->slots)
			if (assoc->kind == Node::ASSOC) {
				std::string v;
				canonNode(assoc->slots[1], r, true, v);
				pairs.emplace_back(canonKey(assoc->slots[0], r), v);
				count++;
			}
		if (count != n->tally) out += "!tally";
		canonPairs(pairs, true, out);
		break;
	}
	case Node::ASSOC: out += "!assoc"; break;
	}
}

static std::string
canon(const Result &r, bool asDict = false)
{
	std::string out;
	if (!r.root) return "!none";
	canonNode(r.root, r, asDict, out);
	return out;
}

/* The same canonical form, from a DOM element (no exponent tokens) */
static void
canonElement(dom::element e, bool asDict, std::string &out)
{
	switch (e.type()) {
	case dom::element_type::NULL_VALUE: out += 'n'; break;
	case dom::element_type::BOOL: out += e.get_bool().value_unsafe() ? 'T' : 'F'; break;
	case dom::element_type::INT64: out += 'i' + std::to_string(e.get_int64().value_unsafe()); break;
	case dom::element_type::UINT64: out += 'i' + std::to_string(e.get_uint64().value_unsafe()); break;
	case dom::element_type::DOUBLE: canonDouble(e.get_double().value_unsafe(), out); break;
	case dom::element_type::BIGINT: out += 't' + std::string(e.get_bigint().value_unsafe()); break;
	case dom::element_type::STRING: {
		std::string_view s = e.get_string().value_unsafe();
		canonString(decodeUtf8(s.data(), s.size()), out);
		break;
	}
	case dom::element_type::ARRAY: {
		dom::array a = e.get_array().value_unsafe();
		bool first = true;
		out += '[';
		for (dom::element c : a) {
			if (!first) out += ',';
			first = false;
			canonElement(c, asDict, out);
		}
		out += ']';
		break;
	}
	case dom::element_type::OBJECT: {
		dom::object o = e.get_object().value_unsafe();
		std::vector<std::pair<std::string, std::string>> pairs;
		for (dom::key_value_pair kv : o) {
			std::string k, v;
			canonString(decodeUtf8(kv.key.data(), kv.key.size()), k);
			canonElement(kv.value, asDict, v);
			pairs.emplace_back(k, v);
		}
		canonPairs(pairs, asDict, out);
		break;
	}
	}
}

static std::string
canonOf(dom::element e, bool asDict = false)
{
	std::string out;
	canonElement(e, asDict, out);
	return out;
}

/*** corpus ***/

static std::string corpusDir;

static bool
readFile(const std::string &path, std::string &out)
{
	FILE *f = fopen(path.c_str(), "rb");
	if (!f) return false;
	char chunk[65536];
	size_t n;
	out.clear();
	while ((n = fread(chunk, 1, sizeof chunk, f)) > 0)
		out.append(chunk, n);
	fclose(f);
	return true;
}

static const char *corpusFiles[] = {
	"apache_builds.json", "canada.json", "citm_catalog.json", "github_events.json",
	"google_maps_api_compact_response.json", "google_maps_api_response.json", "gsoc-2018.json",
	"instruments.json", "marine_ik.json", "mesh.json", "mesh.pretty.json", "numbers.json", "random.json",
	"repeat.json", "semanticscholar-corpus.json", "tree-pretty.json", "twitter_api_compact_response.json",
	"twitter_api_response.json", "twitterescaped.json", "twitter.json", "twitter_timeline.json",
	"update-center.json", "small/adversarial.json", "small/demo.json", "small/flatadversarial.json",
	"small/jsoniter_scala/che-1.geo.json", "small/smalldemo.json", "small/truenull.json",
	"small/twitter_timeline.json", "small/jsoniter_scala/twitter_api_response.json" };

struct CorpusFile { std::string name, json; };

static std::vector<CorpusFile>
corpus()
{
	std::vector<CorpusFile> files;
	if (corpusDir.empty()) return files;
	for (const char *name : corpusFiles) {
		CorpusFile f{name, ""};
		if (readFile(corpusDir + "/" + name, f.json))
			files.push_back(f);
	}
	return files;
}

/*** T1: the value-end scanner ***/

static std::mt19937 rng(20261021);
static int rnd(int n) { return int(rng() % unsigned(n)); }

static std::u32string
fromAscii(const std::string &s)
{
	return std::u32string(s.begin(), s.end());
}

static std::u32string
randomWhite()
{
	static const char *w[] = {"", " ", "\n", "  \n ", "\t", "\r\n"};
	if (rnd(5) == 0) return std::u32string(rnd(70), U' ');
	return fromAscii(w[rnd(6)]);
}

static std::u32string
randomValue(int depth, bool latin1)
{
	switch (rnd(depth > 2 ? 6 : 8)) {
	case 0: {
		std::string d = std::to_string(rnd(2000000) - 1000000);
		if (rnd(3) == 0) d += std::string(rnd(40), '7') + ".5";
		return fromAscii(d);
	}
	case 1: {
		static const char *forms[] = {"1.25e", "-3E+", "7e-", "0.5E", "-12e0"};
		return fromAscii(forms[rnd(5)] + std::to_string(rnd(40)));
	}
	case 2: {
		static const char32_t *pieces[] = {U"a", U"b", U" ", U"{", U"}", U"[", U"]", U",", U":", U"\\\"",
			U"\\\\", U"\\n", U"\u00e9", U"\u00ff", U"\u20ac", U"\U0001D11E", U"\\u00e9", U"5e3", U"1E+2", U"-"};
		std::u32string s = U"\"";
		int n = rnd(12);
		for (int i = 0; i < n; i++) {
			std::u32string p = pieces[rnd(20)];
			if (latin1 && p[0] > 0xFF) p = U"\u00e9";
			s += p;
		}
		return s + U"\"";
	}
	case 3: return U"true";
	case 4: return U"null";
	case 5: return U"false";
	case 6: {
		std::u32string s = U"[" + randomWhite();
		int n = rnd(4);
		for (int i = 0; i < n; i++) {
			if (i) s += U"," + randomWhite();
			s += randomValue(depth + 1, latin1);
		}
		return s + randomWhite() + U"]";
	}
	default: {
		std::u32string s = U"{" + randomWhite();
		int n = rnd(4);
		for (int i = 0; i < n; i++) {
			if (i) s += U",";
			s += fromAscii("\"k" + std::to_string(i) + "\"") + randomWhite() + U":" + randomValue(depth + 1, latin1);
		}
		return s + U"}";
	}
	}
}

static Source
sourceOf(const std::u32string &cps, int encoding)
{
	if (encoding == SJ_ENC_UTF32)
		return Source::utf32(cps);
	if (encoding == SJ_ENC_LATIN1) {
		std::string s;
		for (char32_t c : cps) s += char(c);
		return Source::latin1(s);
	}
	return Source::utf8(encodeUtf8(cps));
}

static void
testScannerCases()
{
	struct { const char *json; size_t end; } cases[] = {
		{"\"\\\\\" 1", 4}, {"\"a\\\"b\" x", 6}, {"[\"]\", \"[\", {\"}\":1}] 2", 19}, {"[1, 2", 5},
		{"\"open", 5}, {"123abc ", 6}, {"[}", 2}, {"1\f2", 1}, {"true,false", 4}, {"-1.5e+3]", 7},
		{"{\"a\":[1,{\"b\":\"]]\"}]}x", 20}, {"[[[]]]]", 6}, {"\"\\\"", 3}, {"1\r2", 1}, {"nul", 3},
		{"x:1", 1}, {"]", 1}, {"\"a\"\"b\"", 3}, {"[\"\\\\\"]1", 6}};
	for (auto &c : cases) {
		size_t n = strlen(c.json);
		CHECKF(sj_value_end(c.json, 1, 0, n) == c.end, "sj_value_end('%s') = %zu, not %zu", c.json,
			sj_value_end(c.json, 1, 0, n), c.end);
		std::u32string cps = fromAscii(c.json);
		std::vector<uint32_t> w(cps.begin(), cps.end());
		CHECKF(sj_value_end(w.data(), 4, 0, n) == c.end, "UTF-32 sj_value_end('%s')", c.json);
	}
	const char *white = " \t\n\r\f";
	CHECK(sj_skip_ws(white, 1, 0, 5) == 5);
	CHECK(sj_trim_ws(white, 1, 0, 5) == 0);
	CHECK(sj_skip_ws("  x ", 1, 0, 4) == 2);
	CHECK(sj_trim_ws(" x \f", 1, 0, 4) == 2);
	CHECK(sj_trim_ws("x", 1, 1, 1) == 1);
	CHECK(sj_value_end("\xC2\xA0", 1, 0, 2) == 2);	/* NBSP is not white space */
}

static void
testScannerFuzz()
{
	dom::parser reference, minifier;
	long streams = 0, values = 0;

	for (int round = 0; round < 3000; round++) {
		bool latin1 = round % 2;
		std::u32string cps = randomWhite();
		int n = 1 + rnd(8);
		for (int i = 0; i < n; i++)
			cps += randomValue(0, latin1) + (rnd(2) ? U"\n" : U" ") + randomWhite();
		std::string utf8 = encodeUtf8(cps);
		/* unit index of every byte offset at a code point boundary */
		std::vector<size_t> unitOf(utf8.size() + 1, SIZE_MAX);
		for (size_t u = 0, b = 0; u <= cps.size(); u++) {
			unitOf[b] = u;
			if (u < cps.size()) b += encodeUtf8(cps.substr(u, 1)).size();
		}
		padded_string padded(utf8);
		dom::document_stream docs;
		if (!CHECK(!reference.parse_many(padded, utf8.size() + 64).get(docs))) continue;
		std::vector<size_t> ends;
		std::vector<std::string> canons;
		for (auto it = docs.begin(); it != docs.end(); ++it) {
			dom::element e;
			if (!CHECK(!(*it).get(e))) break;
			ends.push_back(it.current_index() + it.source().size());
			canons.push_back(canonOf(e));
		}
		if (!CHECKF(int(ends.size()) == n, "reference found %zu documents, not %d", ends.size(), n)) continue;
		streams++;
		size_t start = 0;
		for (int k = 0; k < n; k++) {
			size_t p = sj_skip_ws(utf8.data(), 1, start, utf8.size());
			size_t next = sj_skip_ws(utf8.data(), 1, ends[k], utf8.size());
			CHECKF(sj_value_end(utf8.data(), 1, p, utf8.size()) == ends[k], "value end %zu, not %zu in '%s'",
				sj_value_end(utf8.data(), 1, p, utf8.size()), ends[k], utf8.c_str());
			for (int encoding : {SJ_ENC_UTF8, SJ_ENC_LATIN1, SJ_ENC_UTF32}) {
				if (encoding == SJ_ENC_LATIN1 && !latin1) continue;
				Source src = sourceOf(cps, encoding);
				size_t s0 = encoding == SJ_ENC_UTF8 ? start : unitOf[start];
				size_t n0 = encoding == SJ_ENC_UTF8 ? next : unitOf[next];
				if (encoding != SJ_ENC_UTF8) {
					int width = encoding == SJ_ENC_UTF32 ? 4 : 1;
					size_t up = sj_skip_ws(src.base(), width, s0, src.units());
					CHECKF(sj_value_end(src.base(), width, up, src.units()) == unitOf[ends[k]],
						"encoding %d: value end in '%s'", encoding, utf8.c_str());
				}
				for (unsigned mode : {unsigned(SJ_F_NEXT), 0u}) {
					Result r = readWith(src, s0, src.units(), mode);
					values++;
					if (!CHECKF(ok(r), "encoding %d mode %u: no value at %zu in '%s'", encoding, mode, s0, utf8.c_str()))
						continue;
					CHECKF(canon(r) == canons[k], "encoding %d mode %u: %s, not %s", encoding, mode,
						canon(r).c_str(), canons[k].c_str());
					CHECKF(r.out.next0 == n0, "encoding %d mode %u: next0 %zu, not %zu in '%s'", encoding, mode,
						r.out.next0, n0, utf8.c_str());
				}
			}
			start = ends[k];
		}
	}
	printf("T1 fuzz: %ld streams, %ld reads\n", streams, values);
}

/*** T2: number tokens ***/

static const char *numberKinds[] = {
	"0", "-0", "123", "-123", "01", "00", "-01", "00123", "-0123",
	"9223372036854775807", "9223372036854775808", "18446744073709551615", "18446744073709551616",
	"-9223372036854775808", "-9223372036854775809", "12345678901234567890123456789",
	"1.5", "0.1", "-0.0", "1.", ".5", "3.141592653589793238462643383279",
	"1e2", "1E2", "1e+2", "1e-2", "100e-2", "1.0e-2", "1.5e2", "2.5E-3", "123e6", "123e-6",
	"1e308", "1.0e308", "1e309", "1.0e309", "1e400", "-1e400", "1e-400", "1.0e-400", "4.9e-324",
	"1e1023", "1e1024", "1e-1022", "1e-1074", "123456789012345678901234567890.5", "123456789012345678901234567890e2",
	"1e", "1e+", "+1", "-", "NaN", "Infinity", "0x10", "123abc"};

struct Token { uint32_t ordinal, offset, length; };

static bool
operator==(const Token &a, const Token &b)
{
	return a.ordinal == b.ordinal && a.offset == b.offset && a.length == b.length;
}

static size_t
trimmedLength(const char *p, size_t n)
{
	while (n && (p[n - 1] == ' ' || p[n - 1] == '\n' || p[n - 1] == '\r' || p[n - 1] == '\t'))
		n--;
	return n;
}

static bool
hasExponent(std::string_view token)
{
	return token.find_first_of("eE") != std::string_view::npos;
}

/* On-Demand: every number's raw token */
static error_code
onDemandTokens(ondemand::value v, const char *base, std::vector<Token> &tokens, uint32_t &ordinal)
{
	ondemand::json_type t;
	error_code err = v.type().get(t);
	if (err) return err;
	switch (t) {
	case ondemand::json_type::array: {
		ondemand::array a;
		if ((err = v.get_array().get(a))) return err;
		for (auto x : a) {
			ondemand::value xv;
			if ((err = x.get(xv)) || (err = onDemandTokens(xv, base, tokens, ordinal))) return err;
		}
		return SUCCESS;
	}
	case ondemand::json_type::object: {
		ondemand::object o;
		if ((err = v.get_object().get(o))) return err;
		for (auto f : o) {
			ondemand::value fv;
			if ((err = f.value().get(fv)) || (err = onDemandTokens(fv, base, tokens, ordinal))) return err;
		}
		return SUCCESS;
	}
	case ondemand::json_type::number: {
		std::string_view token = v.raw_json_token();
		size_t n = trimmedLength(token.data(), token.size());
		if (hasExponent(token.substr(0, n)))
			tokens.push_back({ordinal, uint32_t(token.data() - base), uint32_t(n)});
		ordinal++;
		return SUCCESS;
	}
	default:
		return SUCCESS;
	}
}

static bool
onDemandAll(const padded_string &json, std::vector<Token> &tokens, uint32_t &total)
{
	ondemand::parser parser;
	ondemand::document doc;
	total = 0;
	if (parser.iterate(json).get(doc)) return false;
	bool scalar;
	if (doc.is_scalar().get(scalar)) return false;
	if (scalar) {
		ondemand::json_type t;
		std::string_view token;
		if (doc.type().get(t) || doc.raw_json_token().get(token)) return false;
		if (t == ondemand::json_type::number) {
			size_t n = trimmedLength(token.data(), token.size());
			if (hasExponent(token.substr(0, n)))
				tokens.push_back({0, uint32_t(token.data() - json.data()), uint32_t(n)});
			total = 1;
		}
		return true;
	}
	ondemand::value v;
	if (doc.get_value().get(v)) return false;
	return !onDemandTokens(v, json.data(), tokens, total);
}

/* A replay of simdjson's structural indexes, the other way to find the
   number tokens (a private simdjson member: tests and T10 only) */
struct Cursor { const char *buf; const uint32_t *idx; uint32_t j; };

static uint32_t
take(Cursor &c)
{
	char ch;
	while ((ch = c.buf[c.idx[c.j]]) == ',' || ch == ':')
		c.j++;
	return c.idx[c.j++];
}

static void
replay(dom::element e, Cursor &c, std::vector<Token> &tokens, uint32_t &ordinal)
{
	switch (e.type()) {
	case dom::element_type::ARRAY: {
		take(c);
		dom::array a = e.get_array().value_unsafe();
		for (dom::element x : a) replay(x, c, tokens, ordinal);
		take(c);
		break;
	}
	case dom::element_type::OBJECT: {
		take(c);
		dom::object o = e.get_object().value_unsafe();
		for (dom::key_value_pair kv : o) { take(c); replay(kv.value, c, tokens, ordinal); }
		take(c);
		break;
	}
	case dom::element_type::INT64: case dom::element_type::UINT64:
	case dom::element_type::DOUBLE: case dom::element_type::BIGINT: {
		uint32_t s = take(c), end = c.idx[c.j];
		size_t n = trimmedLength(c.buf + s, end - s);
		if (hasExponent(std::string_view(c.buf + s, n)))
			tokens.push_back({ordinal, s, uint32_t(n)});
		ordinal++;
		break;
	}
	default:
		take(c);
	}
}

static void
checkTokens(const std::string &json, const std::string &name, bool *gate = nullptr)
{
	padded_string padded(json);
	dom::parser dp;
	dp.number_as_string(true);
	dom::element root;
	if (dp.parse(padded).get(root)) return;	/* the enumerator takes valid JSON only */
	std::vector<Token> fromOd, fromReplay, fromEngine;
	uint32_t odTotal = 0, replayTotal = 0;
	CHECKF(onDemandAll(padded, fromOd, odTotal), "%s: On-Demand failed", name.c_str());
	Cursor c{padded.data(), dp.implementation->structural_indexes.get(), 0};
	replay(root, c, fromReplay, replayTotal);
	sj_token *list = nullptr;
	size_t count = 0, cap = 0;
	int64_t total = sj_number_tokens(padded.data(), padded.size(), &list, &count, &cap);
	for (size_t i = 0; i < count; i++)
		fromEngine.push_back({list[i].ordinal, list[i].offset, list[i].length});
	free(list);
	CHECKF(total == odTotal && total == replayTotal, "%s: %lld numbers, On-Demand %u, replay %u", name.c_str(),
		(long long)total, odTotal, replayTotal);
	CHECKF(fromEngine == fromOd, "%s: %zu exponent tokens, On-Demand %zu", name.c_str(), fromEngine.size(), fromOd.size());
	CHECKF(fromEngine == fromReplay, "%s: %zu exponent tokens, replay %zu", name.c_str(), fromEngine.size(), fromReplay.size());
	int g = sj_exponent_gate(padded.data(), padded.size());
	CHECKF(g || fromEngine.empty(), "%s: the gate misses an exponent token", name.c_str());
	if (gate) *gate = g;
	/* the engine's closer look after the gate misses none either */
	sj_release(0);
	sj_info before = info();
	Result r = readText(json, SJ_F_EXACT);
	int64_t enumerations = info().stats[ENUMERATIONS] - before.stats[ENUMERATIONS];
	CHECKF(ok(r) && (fromEngine.empty() || enumerations == 1), "%s: %lld enumerations for %zu exponent tokens",
		name.c_str(), (long long)enumerations, fromEngine.size());
	if (enumerations == 1)
		CHECKF(info().stats[NUMBER_TEXTS] - before.stats[NUMBER_TEXTS] >= int64_t(fromEngine.size()), "%s: number texts", name.c_str());
}

static void
testNumberTokens()
{
	int kinds = 0;
	for (const char *n : numberKinds) {
		std::string k(n);
		dom::parser p;
		dom::element e;
		p.number_as_string(true);
		if (p.parse(padded_string(k)).get(e)) continue;
		kinds++;
		checkTokens(k, k);
		checkTokens("[" + k + "]", "[" + k + "]");
		checkTokens("{\"k,:\" : " + k + " , \"a\":[ " + k + " ,{\"x\" :" + k + "}],\"e\":\"1e5\",\"a\":" + k + "}", "object " + k);
	}
	CHECKF(kinds == 34, "%d of the number kinds parse, not 34", kinds);
	checkTokens("{ \"a,:b\" : [ 1e2 , -0 , {\"x\":1.5e-3,\"y\" :[ 12345678901234567890123 ] }, \"s:,\", true, null, 7 ] ,\n\t\"a\":1E+2, \"a\" : 3 }",
		"structural characters in strings, duplicate keys");
	checkTokens("[\"\\\\\", 1e1, \"\\\"1e2\", 2E-2, {\"\\\\\\\"\":3e3}]", "escaped strings");
	for (int round = 0; round < 3000; round++) {
		std::u32string doc = U"[";
		int n = 1 + rnd(12);
		for (int i = 0; i < n; i++)
			doc += (i ? U"," : U"") + randomWhite() + randomValue(0, false);
		std::string json = encodeUtf8(doc + U"]");
		checkTokens(json, json);
	}
	for (auto &f : corpus()) {
		bool gate = false;
		checkTokens(f.json, f.name, &gate);
		if (f.name == "canada.json" || f.name == "citm_catalog.json")
			CHECKF(!gate, "%s: the gate fires", f.name.c_str());
		if (f.name == "twitter.json" || f.name == "github_events.json" || f.name == "gsoc-2018.json"
		 || f.name == "citm_catalog.json") {
			/* the tokens are computed at the first DOUBLE: twitter has one, the
			   others none, so they never run the gate */
			int64_t gateHits = f.name == "twitter.json" ? 1 : 0;
			sj_release(0);
			sj_info before = info();
			readText(f.json, SJ_F_EXACT);
			CHECKF(info().stats[GATE_HITS] - before.stats[GATE_HITS] == gateHits
				&& info().stats[ENUMERATIONS] == before.stats[ENUMERATIONS],
				"%s: %lld gate hits, %lld enumerations", f.name.c_str(),
				(long long)(info().stats[GATE_HITS] - before.stats[GATE_HITS]),
				(long long)(info().stats[ENUMERATIONS] - before.stats[ENUMERATIONS]));
		}
	}

	/* a token list that disagrees with the document gives 1002 */
	for (int fault : {1, 2}) {
		for (int64_t budget : {int64_t(-1), int64_t(0)}) {
			sj_release(0);
			sj_test_number_fault = fault;
			Result r = readText("[1e2, 5, 2.5]", SJ_F_EXACT, budget);
			sj_test_number_fault = 0;
			CHECKF(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR && r.out.code == SJ_ERR_NUMBERS,
				"fault %d budget %lld: status %d code %d", fault, (long long)budget, r.out.status, r.out.code);
		}
	}
	sj_release(0);
	Result r = readText("[1e2, 5, 2.5]", SJ_F_EXACT);
	CHECKF(ok(r) && canon(r) == "[t1e2,i5,d0x1.4p+1]", "after the faults: %s", canon(r).c_str());
}

/*** T3: number policy ***/

static void
testNumberPolicy()
{
	struct { const char *json; unsigned flags; const char *canon; Node::Kind kind; } cases[] = {
		{"-0", 0, "i0", Node::SMALLINT},
		{"1152921504606846975", 0, "i1152921504606846975", Node::SMALLINT},
		{"1152921504606846976", 0, "i1152921504606846976", Node::LARGEINT},
		{"-1152921504606846976", 0, "i-1152921504606846976", Node::SMALLINT},
		{"-1152921504606846977", 0, "i-1152921504606846977", Node::LARGEINT},
		{"9223372036854775807", 0, "i9223372036854775807", Node::LARGEINT},
		{"9223372036854775808", 0, "i9223372036854775808", Node::LARGEINT},
		{"18446744073709551615", 0, "i18446744073709551615", Node::LARGEINT},
		{"12345678901234567890123456789", 0, "t12345678901234567890123456789", Node::TEXT},
		{"-12345678901234567890123456789", SJ_F_EXACT, "t-12345678901234567890123456789", Node::TEXT},
		{"1e2", SJ_F_EXACT, "t1e2", Node::TEXT},
		{"1E+2", SJ_F_EXACT, "t1E+2", Node::TEXT},
		{"1e-2", SJ_F_EXACT, "t1e-2", Node::TEXT},
		{"1.5e2", SJ_F_EXACT, "t1.5e2", Node::TEXT},
		{"1e2", 0, "d0x1.9p+6", Node::SMALLFLOAT},
		{"1E+2", 0, "d0x1.9p+6", Node::SMALLFLOAT},
		{"1e-2", 0, "d0x1.47ae147ae147bp-7", Node::SMALLFLOAT},
		{"1.5e2", 0, "d0x1.2cp+7", Node::SMALLFLOAT},
		{"1e300", 0, "d0x1.7e43c8800759cp+996", Node::BOXEDFLOAT},
		{"1.5", SJ_F_EXACT, "d0x1.8p+0", Node::SMALLFLOAT},
		{"0.1", SJ_F_EXACT, "d0x1.999999999999ap-4", Node::SMALLFLOAT}};
	for (auto &c : cases) {
		Result r = readText(c.json, c.flags);
		if (!CHECKF(ok(r), "'%s': no value", c.json)) continue;
		CHECKF(canon(r) == c.canon && r.root->kind == c.kind, "'%s' flags %u: %s kind %d", c.json, c.flags,
			canon(r).c_str(), r.root->kind);
		if (c.kind == Node::TEXT)
			CHECKF(r.fixups.size() == 1 && r.fixups[0].second == 2, "'%s': root fixup", c.json);
	}
	Result r = readText("-0.0");
	CHECK(ok(r) && r.root->kind == Node::SMALLFLOAT && std::signbit(r.root->d) && r.root->d == 0);
	r = readText("[1e2, 2, 3.5e1, [4E0]]", SJ_F_EXACT);
	CHECKF(ok(r) && canon(r) == "[t1e2,i2,t3.5e1,[t4E0]]", "%s", canon(r).c_str());
	CHECK(r.fixups.size() == 3 && r.fixups[0].second == 1 && r.fixups[1].second == 3 && r.fixups[2].second == 1);
	r = readText("{\"a\":1e2,\"b\":[2e2]}", SJ_F_EXACT);
	CHECK(ok(r) && r.fixups.size() == 2 && r.fixups[0].second == 2 && r.fixups[1].second == 1);
}

/*** T4: transcoding ***/

static void
testTranscoding()
{
	std::string latin1 = "\"";
	for (int c = 0x20; c <= 0xFF; c++)
		if (c != '"' && c != '\\') latin1 += char(c);
	latin1 += "\"";
	Result r = readAll(Source::latin1(latin1), 0);
	CHECK(ok(r) && r.root->kind == Node::BYTESTRING && r.root->bytes == latin1.substr(1, latin1.size() - 2));
	for (int c : {0x00, 0x09, 0x0A, 0x1F}) {
		std::string bad = std::string("\"a") + char(c) + "\"";
		r = readAll(Source::latin1(bad), 0);
		CHECKF(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR, "raw %02x in a string is not an error", c);
	}
	r = readAll(Source::latin1("[\"\\u00e9\\u0000\",\"\xe9\"]"), 0);
	CHECKF(ok(r) && canon(r) == "[s2:\xc3\xa9" + std::string(1, '\0') + ",s1:\xc3\xa9]", "%s", canon(r).c_str());

	r = readAll(Source::utf32(U"[\"\u20ac\U0001D11E\u00e9\", \"a\", {\"\u20ac\":\"\U0001D11E\"}]"), 0);
	CHECKF(ok(r) && r.root->slots[0]->kind == Node::WIDESTRING && r.root->slots[0]->wide == U"\u20ac\U0001D11E\u00e9"
		&& r.root->slots[1]->kind == Node::BYTESTRING, "%s", canon(r).c_str());
	for (char32_t bad : {char32_t(0xD800), char32_t(0xDFFF), char32_t(0x110000), char32_t(0x3FFFFFFF)}) {
		std::u32string s = U"\"a";
		s += bad;
		s += U"\"";
		r = readAll(Source::utf32(s), 0);
		CHECKF(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR, "code point %x is not an error", unsigned(bad));
	}
	r = readText("\"\xff\"");
	CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR && r.out.code == UTF8_ERROR);

	/* next0 counts units */
	r = readWith(Source::latin1("\"\xe9\" [2]"), 0, 7, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 4);
	r = readWith(Source::latin1("\"\xe9\" [2]"), 4, 7, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 7 && canon(r) == "[i2]");
	r = readWith(Source::utf32(U"\"\u20ac\" [2]"), 0, 7, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 4 && canon(r) == "w1:\xe2\x82\xac");
	r = readWith(Source::utf32(U"\"\u20ac\" [2]"), 0, 7, 0);
	CHECK(ok(r) && r.out.next0 == 4);

	/* the helpers, against the encoder of this file */
	for (int round = 0; round < 2000; round++) {
		std::u32string cps;
		bool latin = round % 2;
		int n = rnd(40);
		for (int i = 0; i < n; i++) {
			char32_t c;
			switch (latin ? rnd(2) : rnd(4)) {
			case 0: c = char32_t(rnd(0x80)); break;
			case 1: c = char32_t(0x80 + rnd(0x80)); break;
			case 2: c = char32_t(0x100 + rnd(0xD800 - 0x100)); break;
			default: c = char32_t(0x10000 + rnd(0x100000)); break;
			}
			cps += c;
		}
		std::string utf8 = encodeUtf8(cps);
		Source src = sourceOf(cps, latin ? SJ_ENC_LATIN1 : SJ_ENC_UTF32);
		size_t length = sj_utf8_length(src.base(), src.encoding, 0, src.units());
		std::string out(length, '\0');
		sj_transcode(src.base(), src.encoding, 0, src.units(), &out[0]);
		CHECK(out == utf8);
		size_t chars;
		uint32_t maxcp;
		sj_utf8_scan(utf8.data(), utf8.size(), &chars, &maxcp);
		char32_t max = 0;
		for (char32_t c : cps) max = std::max(max, c);
		CHECK(chars == cps.size() && maxcp == max);
		std::vector<uint32_t> wide(chars);
		sj_utf8_to_utf32(utf8.data(), utf8.size(), wide.data());
		CHECK(std::u32string(wide.begin(), wide.end()) == cps);
		if (latin) {
			std::string bytes(chars, '\0');
			sj_utf8_to_latin1(utf8.data(), utf8.size(), (uint8_t *)&bytes[0]);
			CHECK(bytes == src.bytes);
		}
	}
	CHECK(sj_is_bom("\xEF\xBB\xBF", SJ_ENC_UTF8, 0, 3) && !sj_is_bom("\xEF\xBB\xBF", SJ_ENC_UTF8, 0, 2));
	CHECK(!sj_is_bom("\xEF\xBB\xBF", SJ_ENC_LATIN1, 0, 3));
}

/*** T5: keys and Dictionaries ***/

static void
testKeys()
{
	sj_keyset *k = sj_keyset_new(0);
	std::unordered_map<std::string, int64_t> expected;
	int isNew;
	bool same = true;
	for (int i = 0; i < 200000 && same; i++) {
		std::string key = i % 1000 == 0 ? std::string("a\0b", 3) + std::to_string(i) : "k" + std::to_string(i % 150000);
		if (i == 7) key = "";
		int64_t index = sj_keyset_index(k, key.data(), key.size(), &isNew);
		auto found = expected.find(key);
		if (found == expected.end()) {
			same = isNew && index == int64_t(expected.size()) + 1;
			expected[key] = index;
		} else
			same = !isNew && index == found->second;
	}
	CHECK(same);
	sj_keyset_free(k);

	Result r = readText("{\"a\":1,\"b\":{\"a\":2,\"c\":[{\"b\":3}]},\"\xc3\xa9\":4,\"\xe2\x82\xac\":5}", SJ_F_SYMKEYS);
	CHECK(ok(r) && r.keyStrings.size() == 5);
	if (r.keyStrings.size() == 5) {
		CHECK(r.keyStrings[0]->bytes == "a" && r.keyStrings[1]->bytes == "b" && r.keyStrings[2]->bytes == "c");
		CHECK(r.keyStrings[3]->kind == Node::BYTESTRING && r.keyStrings[3]->bytes == "\xe9");
		CHECK(r.keyStrings[4]->kind == Node::WIDESTRING && r.keyStrings[4]->wide == U"\u20ac");
		CHECK(r.root->slots[0]->i == 1 && r.root->slots[2]->i == 2 && r.root->slots[3]->slots[0]->i == 1);
	}
	CHECKF(canon(r) == canon(readText("{\"a\":1,\"b\":{\"a\":2,\"c\":[{\"b\":3}]},\"\xc3\xa9\":4,\"\xe2\x82\xac\":5}")),
		"symbol keys: %s", canon(r).c_str());
	r = readText("[1]", SJ_F_SYMKEYS);
	CHECK(ok(r) && r.keyStrings.empty());

	r = readText("{\"a\":1,\"b\":2,\"a\":3}", SJ_F_DICT);
	CHECKF(ok(r) && canon(r) == "D{s1:a:i3,s1:b:i2}" && r.root->tally == 2, "%s", canon(r).c_str());
	r = readText("{\"a\":1e5,\"a\":\"1e5\"}", SJ_F_DICT | SJ_F_EXACT);
	CHECKF(ok(r) && r.fixups.empty() && canon(r) == "D{s1:a:s3:1e5}", "%s", canon(r).c_str());
	r = readText("{\"a\":\"x\",\"a\":1e5}", SJ_F_DICT | SJ_F_EXACT);
	CHECK(ok(r) && r.fixups.size() == 1 && r.fixups[0].first->kind == Node::ASSOC && r.fixups[0].second == 2);
	r = readText("{\"\xc3\xa9\":1,\"\xe2\x82\xac\":2,\"\":{}}", SJ_F_DICT);
	CHECKF(ok(r) && canon(r) == "D{s0::D{},s1:\xc3\xa9:i1,w1:\xe2\x82\xac:i2}", "%s", canon(r).c_str());
	r = readText("{}", SJ_F_DICT);
	CHECK(ok(r) && r.root->kind == Node::DICT && r.root->slots[0]->slots.size() == 5);

	/* Dictionaries equal the flat maps they replace */
	std::string many = "{";
	for (int i = 0; i < 300; i++)
		many += (i ? ",\"k" : "\"k") + std::to_string(i % 200) + "\":" + std::to_string(i);
	many += "}";
	std::vector<std::pair<std::string, std::string>> docs = {{"300 keys", many}};
	for (auto &f : corpus())
		if (f.name == "twitter.json" || f.name == "citm_catalog.json" || f.name == "github_events.json")
			docs.emplace_back(f.name, f.json);
	for (auto &d : docs) {
		Result tree = readText(d.second, SJ_F_EXACT), dict = readText(d.second, SJ_F_DICT | SJ_F_EXACT);
		CHECKF(ok(tree) && ok(dict) && canon(tree, true) == canon(dict), "%s: Dictionary differs", d.first.c_str());
	}
}

/*** T6: measure ***/

static std::string
repeatTo(const std::string &open, const std::string &item, const std::string &close, size_t size)
{
	std::string s = open;
	s.reserve(size + item.size() + close.size());
	while (s.size() < size) {
		if (s.size() > open.size()) s += ',';
		s += item;
	}
	return s + close;
}

static double worstRatio;
static std::string worstShape;

static void
checkMeasure(const std::string &name, const std::string &json, int encoding = SJ_ENC_UTF8)
{
	static const unsigned modes[] = {0, SJ_F_EXACT, SJ_F_DICT, SJ_F_DICT | SJ_F_EXACT, SJ_F_SYMKEYS, SJ_F_SYMKEYS | SJ_F_EXACT};
	Source src = encoding == SJ_ENC_UTF8 ? Source::utf8(json) : sourceOf(decodeUtf8(json.data(), json.size()), encoding);
	uint64_t bytes = sj_trim_ws(json.data(), 1, sj_skip_ws(json.data(), 1, 0, json.size()), json.size())
		- sj_skip_ws(json.data(), 1, 0, json.size());
	for (unsigned mode : modes) {
		Result measured = readAll(src, mode, 0);
		if (!CHECKF(measured.rc == SJ_RC_DONE && measured.out.status == SJ_OVER_BUDGET, "%s mode %u: status %d code %d",
				name.c_str(), mode, measured.out.status, measured.out.code))
			continue;
		CHECKF(measured.allocations == 0, "%s mode %u: %llu objects allocated over budget", name.c_str(), mode,
			(unsigned long long)measured.allocations);
		Result built = readAll(src, mode, -1);
		if (!CHECKF(ok(built), "%s mode %u: no value", name.c_str(), mode)) continue;
		CHECKF(uint64_t(measured.out.need) == built.bytes, "%s mode %u: need %lld, built %llu", name.c_str(), mode,
			(long long)measured.out.need, (unsigned long long)built.bytes);
		if (bytes >= 4096 && double(measured.out.need) / double(bytes) > worstRatio) {
			worstRatio = double(measured.out.need) / double(bytes);
			worstShape = name + " mode " + std::to_string(mode);
		}
		CHECKF(uint64_t(measured.out.need) <= sj_bound(bytes), "%s mode %u: need %lld over the bound %llu (%.2f B/byte)",
			name.c_str(), mode, (long long)measured.out.need, (unsigned long long)sj_bound(bytes),
			double(measured.out.need) / double(bytes));
	}
}

static void
testMeasure()
{
	const size_t MB = 1000000;
	std::string deep;
	for (int i = 0; i < 999; i++) deep += "{\"\":";
	deep += "{}";
	deep += std::string(999, '}');
	std::string keys300 = "{";
	for (int i = 0; i < 300; i++) keys300 += (i ? ",\"k" : "\"k") + std::to_string(i) + "\":0";
	keys300 += "}";
	std::string dup40 = "{";
	for (int i = 0; i < 40; i++) dup40 += (i ? ",\"k" : "\"k") + std::to_string(i % 23) + "\":" + (i % 3 ? "1e1" : "\"x\"");
	dup40 += "}";
	std::string chain;
	for (int i = 0; i < 1000; i++) chain += "{\"a\":";
	chain += "1" + std::string(1000, '}');
	struct { const char *name; std::string json; } shapes[] = {
		{"deep", repeatTo("[", deep, "]", MB)},
		{"[{},...]", repeatTo("[", "{}", "]", MB)},
		{"[{\"a\":{}},...]", repeatTo("[", "{\"a\":{}}", "]", MB)},
		{"[{\"a\":1e1},...]", repeatTo("[", "{\"a\":1e1}", "]", MB)},
		{"[1e1,...]", repeatTo("[", "1e1", "]", MB)},
		{"[\"\",...]", repeatTo("[", "\"\"", "]", MB)},
		{"[[],...]", repeatTo("[", "[]", "]", MB)},
		{"[0,...]", repeatTo("[", "0", "]", MB)},
		{"euro", "[\"\xe2\x82\xac" + std::string(MB, 'a') + "\"]"},
		{"300 keys", repeatTo("[", keys300, "]", MB)},
		{"40 keys, duplicates", repeatTo("[", dup40, "]", MB / 10)},
		{"{\"a\":{\"a\":...", repeatTo("[", chain, "]", MB / 4)},
		{"boxed", "[1.5e300, -1e-300, 9223372036854775807, 18446744073709551615, -1152921504606846977]"},
		{"big integers", "[123456789012345678901234567890, -1e5, {\"\xe2\x82\xac\":12345678901234567890123456789}]"},
		{"root text", "1e5"}, {"root", "17"}, {"empty", "{}"}};
	for (auto &s : shapes)
		checkMeasure(s.name, s.json);
	checkMeasure("Latin-1 source", "[\"\xc3\xa9t\xc3\xa9\", {\"\xc3\xa9\":1e1}]", SJ_ENC_LATIN1);
	checkMeasure("UTF-32 source", "[\"\xe2\x82\xac\", {\"\xf0\x9d\x84\x9e\":1e1}]", SJ_ENC_UTF32);
	for (auto &f : corpus())
		checkMeasure(f.name, f.json);
	printf("T6 worst need: %.1f bytes per input byte (%s), bound 32\n", worstRatio, worstShape.c_str());
}

/*** T7: idempotence and the cache ***/

static void
testIdempotence()
{
	std::vector<std::pair<std::string, std::string>> docs = {
		{"crafted", "{\"a\":[1,2.5,1e2,\"x\",{\"b\":[true,null,12345678901234567890123]}],\"c\":\"\xe2\x82\xac\"}"}};
	for (auto &f : corpus())
		if (f.name == "twitter.json" || f.name == "citm_catalog.json") docs.emplace_back(f.name, f.json);
	for (auto &d : docs)
		for (unsigned flags : {unsigned(SJ_F_EXACT), unsigned(SJ_F_EXACT | SJ_F_DICT), unsigned(SJ_F_SYMKEYS)}) {
			Source src = Source::utf8(d.second);
			Result reference = readAll(src, flags);
			if (!CHECK(ok(reference))) continue;
			std::string expected = canon(reference);
			uint64_t all = reference.allocations;
			for (uint64_t k : {uint64_t(1), all / 2, all}) {
				sj_release(0);
				sj_info before = info();
				Result failed = readAll(src, flags, -1, k);
				CHECKF(failed.rc == SJ_RC_BUILD_FAILED, "%s: allocation %llu of %llu failed, rc %d", d.first.c_str(),
					(unsigned long long)k, (unsigned long long)all, failed.rc);
				Result again = readAll(src, flags);
				sj_info after = info();
				CHECKF(ok(again) && canon(again) == expected, "%s flags %u: different value after failing allocation %llu",
					d.first.c_str(), flags, (unsigned long long)k);
				CHECKF(after.stats[PARSES] - before.stats[PARSES] == 1 && after.stats[CACHE_HITS] - before.stats[CACHE_HITS] == 1
					&& after.stats[BUILD_FAILURES] - before.stats[BUILD_FAILURES] == 1,
					"%s: parses +%lld, hits +%lld", d.first.c_str(), (long long)(after.stats[PARSES] - before.stats[PARSES]),
					(long long)(after.stats[CACHE_HITS] - before.stats[CACHE_HITS]));
			}
		}

	/* the VM's 5 calls, then a call after success */
	Source src = Source::utf8("[1, 2.5e3, \"x\"]");
	sj_release(0);
	sj_info before = info();
	for (int i = 0; i < 4; i++)
		CHECK(readAll(src, SJ_F_EXACT, -1, 1).rc == SJ_RC_BUILD_FAILED);
	Result r = readAll(src, SJ_F_EXACT);
	sj_info after = info();
	CHECK(ok(r) && canon(r) == "[i1,t2.5e3,s1:x]");
	CHECK(after.stats[PARSES] - before.stats[PARSES] == 1 && after.stats[CACHE_HITS] - before.stats[CACHE_HITS] == 4);
	CHECK(after.stats[ENUMERATIONS] - before.stats[ENUMERATIONS] == 1);
	r = readAll(src, SJ_F_EXACT);
	CHECK(ok(r) && info().stats[PARSES] - after.stats[PARSES] == 1);	/* a success drops the cache */

	/* FIRST: the retry finds the value slice before parsing the whole rest */
	src = Source::utf8("[1,2] [3]");
	sj_release(0);
	before = info();
	r = readAll(src, 0, -1, 1);
	after = info();
	CHECK(r.rc == SJ_RC_BUILD_FAILED && after.stats[PARSES] - before.stats[PARSES] == 2);
	r = readAll(src, 0);
	CHECK(ok(r) && canon(r) == "[i1,i2]" && r.out.next0 == 6 && info().stats[PARSES] == after.stats[PARSES]);
	/* a WHOLE entry serves NEXT */
	src = Source::utf8(" [1,2]  ");
	sj_release(0);
	CHECK(readAll(src, 0, -1, 1).rc == SJ_RC_BUILD_FAILED);
	after = info();
	r = readAll(src, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 8 && info().stats[PARSES] == after.stats[PARSES]);
	/* another source with the same text hits the cache too */
	CHECK(readAll(Source::latin1("[\"\xe9\"]"), 0, -1, 1).rc == SJ_RC_BUILD_FAILED);
	after = info();
	r = readAll(Source::utf32(U"[\"\u00e9\"]"), 0);
	CHECK(ok(r) && info().stats[PARSES] == after.stats[PARSES]);
	CHECK(readAll(Source::latin1("[\"\xe9\"]"), 0, -1, 1).rc == SJ_RC_BUILD_FAILED);
	after = info();
	r = readAll(Source::utf32(U"[\"\u00ea\"]"), 0);
	CHECK(ok(r) && info().stats[PARSES] == after.stats[PARSES] + 1);
	/* a WHOLE entry left by a refused budget serves a NEXT read of a longer
	   text: the next value starts after the slice, not at the end */
	src = Source::utf8("[1] [2]");
	sj_release(0);
	r = readWith(src, 0, 3, 0, 0);
	CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_OVER_BUDGET);
	after = info();
	r = readWith(src, 0, 7, SJ_F_NEXT);
	CHECKF(ok(r) && canon(r) == "[i1]" && r.out.next0 == 4 && info().stats[CACHE_HITS] == after.stats[CACHE_HITS] + 1,
		"NEXT after a WHOLE entry: next0 %zu", r.out.next0);

	/* the cap: a whole rest over it gives way to the value slice, a value
	   over it is refused before any parse */
	src = Source::utf8("[1,2] [3,4] [5,6]");
	sj_release(0);
	before = info();
	r = readWith(src, 0, 17, 0, -1, 0, 5);
	CHECKF(ok(r) && canon(r) == "[i1,i2]" && r.out.next0 == 6, "cap 5, FIRST: next0 %zu", r.out.next0);
	r = readWith(src, 6, 17, SJ_F_NEXT, -1, 0, 5);
	CHECK(ok(r) && canon(r) == "[i3,i4]" && r.out.next0 == 12);
	r = readWith(src, 0, 5, 0, -1, 0, 5);
	CHECK(ok(r) && canon(r) == "[i1,i2]" && r.out.next0 == 5 && info().stats[PARSES] - before.stats[PARSES] == 3);
	before = info();
	for (unsigned flags : {0u, unsigned(SJ_F_NEXT)}) {
		r = readWith(src, 0, 17, flags, -1, 0, 4);
		CHECKF(r.rc == SJ_RC_DONE && r.out.status == SJ_TOO_LARGE && r.out.need == 5 && r.allocations == 0,
			"cap 4, flags %u: status %d, need %lld", flags, r.out.status, (long long)r.out.need);
	}
	CHECK(info().stats[PARSES] == before.stats[PARSES]);
	r = readWith(Source::utf32(U" [\"\u20ac\"] "), 0, 7, 0, -1, 0, 5);
	CHECK(ok(r) && r.out.next0 == 7);
	r = readWith(Source::utf32(U" [\"\u20ac\"] "), 0, 7, 0, -1, 0, 4);
	CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_TOO_LARGE && r.out.need == 5);

	/* budgets */
	for (auto &d : docs) {
		Source s = Source::utf8(d.second);
		sj_release(0);
		before = info();
		Result refused = readAll(s, SJ_F_EXACT, 0);
		if (!CHECK(refused.rc == SJ_RC_DONE && refused.out.status == SJ_OVER_BUDGET)) continue;
		int64_t need = refused.out.need;
		r = readAll(s, SJ_F_EXACT, need - 1);
		CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_OVER_BUDGET && r.out.need == need && r.allocations == 0);
		r = readAll(s, SJ_F_EXACT, need);
		after = info();
		CHECKF(ok(r) && r.bytes == uint64_t(need), "%s: built %llu, need %lld", d.first.c_str(),
			(unsigned long long)r.bytes, (long long)need);
		CHECK(after.stats[PARSES] - before.stats[PARSES] == 1 && after.stats[MEASURES] - before.stats[MEASURES] == 1);
		CHECK(after.stats[OVER_BUDGET] - before.stats[OVER_BUDGET] == 2);
		r = readAll(s, SJ_F_EXACT, int64_t(sj_bound(d.second.size())));	/* no measure under the bound */
		CHECK(ok(r) && info().stats[MEASURES] == after.stats[MEASURES]);
	}
}

/*** T8: edges ***/

static void
testEdges(bool slow)
{
	Result r = readText("\xEF\xBB\xBF[1]");
	CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR && r.out.code == SJ_ERR_BOM);
	r = readText(" \n\xEF\xBB\xBF" "1");
	CHECK(r.out.status == SJ_ERROR && r.out.code == SJ_ERR_BOM);
	r = readAll(Source::utf32(U"\uFEFF[1]"), 0);
	CHECK(r.out.status == SJ_ERROR && r.out.code == SJ_ERR_BOM);
	r = readWith(Source::utf8("[1] \xEF\xBB\xBF[2]"), 4, 10, SJ_F_NEXT);
	CHECK(r.out.status == SJ_ERROR && r.out.code == SJ_ERR_BOM && r.out.next0 == 4);
	r = readText(" \t\n\r\f");
	CHECK(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR && r.out.code == EMPTY);
	r = readText("");
	CHECK(r.out.status == SJ_ERROR && r.out.code == EMPTY);
	r = readText(std::string(1024, '[') + std::string(1024, ']'));
	CHECK(ok(r));
	r = readText(std::string(1025, '[') + std::string(1025, ']'));
	CHECKF(r.out.status == SJ_ERROR && r.out.code == DEPTH_ERROR, "depth 1025: status %d code %d", r.out.status, r.out.code);
	CHECK(canon(readText("{}")) == "{}" && readText("{}").root->kind == Node::FLATMAP);
	CHECK(canon(readText("[]")) == "[]" && readText("[]").root->kind == Node::ARRAY);
	for (unsigned mode : {unsigned(SJ_F_NEXT), 0u}) {
		r = readWith(Source::utf8("[1]x"), 0, 4, mode);
		CHECKF(ok(r) && r.out.next0 == 3, "[1]x mode %u: next0 %zu", mode, r.out.next0);
		r = readWith(Source::utf8("[1]\f"), 0, 4, mode);
		CHECKF(ok(r) && r.out.next0 == 4, "[1]\\f mode %u: next0 %zu", mode, r.out.next0);
		r = readWith(Source::utf8("1\f2"), 0, 3, mode);
		CHECKF(ok(r) && r.out.next0 == 2 && canon(r) == "i1", "1\\f2 mode %u", mode);
	}
	r = readWith(Source::utf8("  [1] \t"), 0, 7, 0);
	CHECK(ok(r) && r.out.next0 == 7);
	r = readWith(Source::utf8("  [1] \t"), 0, 5, 0);
	CHECK(ok(r) && r.out.next0 == 5);
	r = readText("[1] , [2]");
	CHECK(ok(r) && r.out.next0 == 4);
	r = readWith(Source::utf8("[1] , [2]"), 4, 9, SJ_F_NEXT);
	CHECK(r.out.status == SJ_ERROR && r.out.next0 == 4);
	r = readWith(Source::utf8("[1] [2]"), 0, 7, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 4);
	r = readWith(Source::utf8("[1] [2]"), 4, 7, SJ_F_NEXT);
	CHECK(ok(r) && r.out.next0 == 7 && canon(r) == "[i2]");
	for (const char *bad : {"[1,2", "[1}", "123abc", "01", "{\"a\":}", "tru", "\"\\x\""}) {
		r = readText(bad);
		CHECKF(r.rc == SJ_RC_DONE && r.out.status == SJ_ERROR && r.out.code > 0 && r.out.next0 == 0, "'%s' is not an error", bad);
	}
	CHECK(sj_error_message(SJ_ERR_BOM) && sj_error_message(SJ_ERR_NUMBERS) && sj_error_message(0));
	CHECK(sj_error_message(NUM_ERROR_CODES - 1) && !sj_error_message(NUM_ERROR_CODES) && !sj_error_message(-1)
		&& !sj_error_message(1000));
	CHECK(!strcmp(sj_error_message(DEPTH_ERROR), error_message(DEPTH_ERROR)));
	CHECK(ok(readText("[1e2]", SJ_F_EXACT)));
	sj_info i = info();
	CHECK(i.implementation[0] && i.stats[CALLS] > 0 && i.stats[PARSES] > 0 && i.stats[NUMBER_TEXTS] > 0);
	printf("T8 implementation %s, emscripten %d, capacity %lld, buffer %lld\n", i.implementation, i.emscripten,
		(long long)i.capacity, (long long)i.cachedBytes);
	sj_release(1);
	CHECK(info().capacity == 0 && info().cachedBytes == 0);
	CHECK(ok(readText("[1]")));

	if (!slow) return;
	std::string huge = "[";
	huge.reserve(2 * 16777216 + 2);
	for (size_t k = 0; k < 16777216; k++) huge += k ? ",0" : "0";
	huge += "]";
	r = readText(huge, 0);
	CHECKF(ok(r) && r.root->slots.size() == 16777216, "16M elements: %zu", r.root ? r.root->slots.size() : 0);
	Result measured = readText(huge, 0, 0);
	CHECK(measured.out.status == SJ_OVER_BUDGET && uint64_t(measured.out.need) == r.bytes);
	printf("T8 slow: 16,777,216 elements, need %lld\n", (long long)measured.out.need);
}

/*** T10: costs ***/

static int64_t
nowNanos()
{
	return std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

template <class F> static int64_t
best(int repeat, F f)
{
	int64_t b = INT64_MAX;
	for (int i = 0; i < repeat; i++) {
		int64_t t = nowNanos();
		f();
		b = std::min(b, nowNanos() - t);
	}
	return b;
}

/* A plain pre-order walk, the reference of the replay cost */
static uint64_t
walkAll(dom::element e)
{
	uint64_t n = 1;
	if (e.type() == dom::element_type::ARRAY) {
		dom::array a = e.get_array().value_unsafe();
		for (dom::element x : a) n += walkAll(x);
	} else if (e.type() == dom::element_type::OBJECT) {
		dom::object o = e.get_object().value_unsafe();
		for (dom::key_value_pair kv : o) n += walkAll(kv.value);
	}
	return n;
}

static void
report()
{
	const char *names[] = {"twitter.json", "canada.json", "citm_catalog.json", "marine_ik.json", "mesh.json", "numbers.json"};
	const int repeat = 21;
	printf("T10 costs in microseconds, best of %d (EXACT, tree mode, a builder that allocates nothing)\n", repeat);
	printf("  enum: sj_number_tokens, run only when the gate and the engine's closer look find an exponent;\n"
		"  replay: what the structural-index walk of plan B\n"
		"  adds to a DOM walk; share: enum / (parse + gate + enum + walk)\n");
	printf("%-18s %8s %7s %8s %8s %8s %8s %7s %6s\n", "file", "parse", "gate", "enum", "measure", "walk", "replay", "tokens", "share");
	for (const char *name : names) {
		std::string json;
		if (corpusDir.empty() || !readFile(corpusDir + "/" + name, json)) continue;
		padded_string padded(json);
		dom::parser p;
		p.number_as_string(true);
		dom::element root;
		int64_t parse = best(repeat, [&] { if (p.parse(padded).get(root)) abort(); });
		int gate = 0;
		int64_t tGate = best(repeat, [&] { gate = sj_exponent_gate(padded.data(), padded.size()); });
		sj_token *list = nullptr;
		size_t count = 0, cap = 0;
		int64_t tEnum = best(repeat, [&] { sj_number_tokens(padded.data(), padded.size(), &list, &count, &cap); });
		free(list);
		volatile uint64_t sink = 0;
		int64_t plain = best(repeat, [&] { sink = sink + walkAll(root); });
		int64_t replayed = best(repeat, [&] {
			std::vector<Token> tokens;
			uint32_t ordinal = 0;
			Cursor c{padded.data(), p.implementation->structural_indexes.get(), 0};
			replay(root, c, tokens, ordinal);
			sink = sink + ordinal;
		});
		sj_request rq = { { json.data(), json.size(), SJ_ENC_UTF8, 0, json.size() }, SJ_F_EXACT, 0, -1 };
		sj_outcome out;
		int64_t engineParse = 0, enumerated = 0;
		int64_t refused = best(repeat, [&] {
			sj_release(0);
			sj_info before = info();
			sj_read(&rq, &nullBuilder, &out);
			engineParse = info().stats[PARSE_NANOS] - before.stats[PARSE_NANOS];
			enumerated = info().stats[ENUMERATIONS] - before.stats[ENUMERATIONS];
		});
		int64_t enumCost = enumerated ? tEnum : 0;
		int64_t measure = refused - engineParse - tGate - enumCost;	/* and the closer look after the gate */
		rq.budget = -1;
		int64_t walk = INT64_MAX;
		for (int k = 0; k < repeat; k++) {
			sj_info before = info();
			sj_read(&rq, &nullBuilder, &out);
			walk = std::min(walk, info().stats[BUILD_NANOS] - before.stats[BUILD_NANOS]);
		}
		double share = 100.0 * double(enumCost) / double(parse + tGate + enumCost + walk);
		printf("%-18s %8.1f %7.1f %8.1f %8.1f %8.1f %8.1f %7zu %5.1f%%%s\n", name, parse / 1e3, tGate / 1e3, tEnum / 1e3,
			measure / 1e3, walk / 1e3, (replayed - plain) / 1e3, count, share,
			!gate ? "  gate closed" : enumerated ? "" : "  gate open, no enumeration");
	}
}

/*** T11: the texts of the writer ***/

static std::string
outText(sj_out &o)
{
	std::string text(o.p ? o.p : "", o.n);
	sj_out_free(&o);
	return text;
}

static std::string
writeLatin1(const std::string &s, int asciiOnly)
{
	sj_out o = {nullptr, 0, 0};
	CHECK(sj_out_string(&o, s.data(), SJ_ENC_LATIN1, s.size(), asciiOnly) == 0);
	return outText(o);
}

static std::string
writeWide(const std::u32string &s, int asciiOnly, int *rc = nullptr)
{
	sj_out o = {nullptr, 0, 0};
	int r = sj_out_string(&o, s.data(), SJ_ENC_UTF32, s.size(), asciiOnly);
	if (rc) *rc = r; else CHECK(r == 0);
	return outText(o);
}

static std::string
printDouble(double v)
{
	char text[SJ_DOUBLE_CHARS];
	size_t n = sj_print_double(v, text);
	CHECK(n > 0);
	return std::string(text, n);
}

/* The decimal digits of a little-endian magnitude, by doubling and adding
   in base 10: slow, and written differently from sj_out_magnitude */
static std::string
decimalOf(const std::vector<uint8_t> &le)
{
	std::vector<int> digits{0};
	for (size_t i = le.size(); i-- > 0;) {
		for (int bit = 7; bit >= 0; bit--) {
			int carry = (le[i] >> bit) & 1;
			for (int &d : digits) {
				int v = 2 * d + carry;
				d = v % 10;
				carry = v / 10;
			}
			if (carry) digits.push_back(carry);
		}
	}
	std::string text;
	for (size_t i = digits.size(); i-- > 0;) text += char('0' + digits[i]);
	return text;
}

static void
testWriter()
{
	std::string controls;
	for (int c = 0; c < 32; c++) controls += char(c);
	CHECK(writeLatin1("", 0) == "\"\"");
	CHECK(writeLatin1("a\"b\\c/d", 0) == "\"a\\\"b\\\\c/d\"");
	CHECK(writeLatin1(controls, 0) == "\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000B"
		"\\f\\r\\u000E\\u000F\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\\u0019\\u001A\\u001B"
		"\\u001C\\u001D\\u001E\\u001F\"");
	CHECK(writeLatin1("\x7f\xe9\xff", 0) == "\"\x7f\xc3\xa9\xc3\xbf\"");
	CHECK(writeLatin1("~\x7f\xe9", 1) == "\"~\\u007F\\u00E9\"");
	CHECK(writeWide(std::u32string{'a', 0x20AC, 0x1D11E}, 0) == "\"a\xe2\x82\xac\xf0\x9d\x84\x9e\"");
	CHECK(writeWide(std::u32string{0xD800, 0x10FFFF}, 0) == "\"\xed\xa0\x80\xf4\x8f\xbf\xbf\"");
	CHECK(writeWide(std::u32string{0xE9, 0x20AC, 0x1D11E, 0xD800, 0x10FFFF}, 1)
		== "\"\\u00E9\\u20AC\\uD834\\uDD1E\\uD800\\uDBFF\\uDFFF\"");
	int rc = 0;
	writeWide(std::u32string{'a', 0x110000}, 0, &rc);
	CHECK(rc == -2);
	writeWide(std::u32string{0x110000}, 1, &rc);
	CHECK(rc == -2);
	{	/* runs longer than the first buffer, and every byte value */
		std::string all, expected = "\"";
		for (int i = 0; i < 20000; i++) all += char(i % 256);
		for (unsigned char c : all) {
			char u[8];
			if (c == '"') expected += "\\\"";
			else if (c == '\\') expected += "\\\\";
			else if (c == 8) expected += "\\b";
			else if (c == 9) expected += "\\t";
			else if (c == 10) expected += "\\n";
			else if (c == 12) expected += "\\f";
			else if (c == 13) expected += "\\r";
			else if (c < 32) { snprintf(u, sizeof u, "\\u%04X", c); expected += u; }
			else if (c < 128) expected += char(c);
			else { expected += char(0xC0 | (c >> 6)); expected += char(0x80 | (c & 0x3F)); }
		}
		CHECK(writeLatin1(all, 0) == expected + "\"");
	}

	sj_out o = {nullptr, 0, 0};
	CHECK(sj_out_int64(&o, 0) == 0 && sj_out_char(&o, ' ') == 0 && sj_out_int64(&o, INT64_MIN) == 0
		&& sj_out_char(&o, ' ') == 0 && sj_out_int64(&o, INT64_MAX) == 0);
	CHECK(outText(o) == "0 -9223372036854775808 9223372036854775807");
	std::mt19937_64 random(20261027);
	for (int i = 0; i < 3000; i++) {
		std::vector<uint8_t> le(1 + random() % (i < 2000 ? 24 : 300));
		for (auto &b : le) b = uint8_t(random());
		if (i % 7 == 0) le.push_back(0);	/* not normalized */
		bool negative = i % 2;
		sj_out m = {nullptr, 0, 0};
		CHECK(sj_out_magnitude(&m, le.data(), le.size(), negative) == 0);
		std::string expected = decimalOf(le);
		if (negative && expected != "0") expected = "-" + expected;
		CHECKF(outText(m) == expected, "magnitude of %zu bytes", le.size());
	}

	struct { double v; const char *text; } spelled[] = {
		{0.0, "0.0"}, {-0.0, "-0.0"}, {1.0, "1.0"}, {100.0, "100.0"}, {0.1, "0.1"}, {123456.789, "123456.789"},
		{999999.0, "999999.0"}, {1e6, "1.0e6"}, {1234567.0, "1.234567e6"}, {123456789012345.0, "1.23456789012345e14"},
		{1e15, "1.0e15"}, {1e22, "1.0e22"}, {1e-3, "0.001"}, {1.5e-4, "0.00015"}, {1e-4, "0.0001"}, {1e-5, "1.0e-5"},
		{2.5e-7, "2.5e-7"}, {5e-324, "5.0e-324"}, {1.7976931348623157e308, "1.7976931348623157e308"},
		{-2.4902358169137062e14, "-2.4902358169137063e14"}};	/* a tie: Pharo rounds it up */
	for (auto &e : spelled)
		CHECKF(printDouble(e.v) == e.text, "%.17g spelled %s, not %s", e.v, printDouble(e.v).c_str(), e.text);
	for (int i = 0; i < 300000; i++) {
		uint64_t bits = random();
		double v;
		memcpy(&v, &bits, sizeof v);
		if (!std::isfinite(v)) continue;
		std::string text = printDouble(v);
		CHECKF(strtod(text.c_str(), nullptr) == v, "%.17g spelled %s", v, text.c_str());
		CHECKF(text.find('.') != std::string::npos && text.find('+') == std::string::npos, "%s", text.c_str());
	}
}


/*** main ***/

static bool
selected(const std::string &only, const char *test)
{
	if (only.empty()) return true;
	std::string list = "," + only + ",";
	return list.find(std::string(",") + test + ",") != std::string::npos;
}

int
main(int argc, char **argv)
{
	std::string only;
	bool slow = getenv("SJ_SLOW") && !strcmp(getenv("SJ_SLOW"), "1"), costs = false;

	if (getenv("SJ_CORPUS")) corpusDir = getenv("SJ_CORPUS");
	for (int i = 1; i < argc; i++) {
		if (!strcmp(argv[i], "--corpus") && i + 1 < argc) corpusDir = argv[++i];
		else if (!strcmp(argv[i], "--only") && i + 1 < argc) only = argv[++i];
		else if (!strcmp(argv[i], "--slow")) slow = true;
		else if (!strcmp(argv[i], "--report")) costs = true;
		else { fprintf(stderr, "usage: sjtest [--corpus DIR] [--only T1,T2,...] [--slow] [--report]\n"); return 2; }
	}
	if (costs) {
		report();
		return 0;
	}
	if (corpusDir.empty() || corpus().empty())
		printf("sjtest: no corpus (--corpus DIR or SJ_CORPUS): the corpus checks are skipped\n");
	struct { const char *name; void (*run)(); } tests[] = {
		{"T1", [] { testScannerCases(); testScannerFuzz(); }}, {"T2", testNumberTokens}, {"T3", testNumberPolicy},
		{"T4", testTranscoding}, {"T5", testKeys}, {"T6", testMeasure}, {"T7", testIdempotence}, {"T11", testWriter}};
	for (auto &t : tests) {
		if (!selected(only, t.name)) continue;
		currentTest = t.name;
		int before = failures, checksBefore = checks;
		int64_t start = nowNanos();
		t.run();
		printf("%s %s: %d checks, %d failures, %.1f s\n", t.name, failures == before ? "ok" : "FAILED", checks - checksBefore,
			failures - before, (nowNanos() - start) / 1e9);
	}
	if (selected(only, "T8")) {
		currentTest = "T8";
		int before = failures, checksBefore = checks;
		testEdges(slow);
		printf("T8 %s: %d checks, %d failures%s\n", failures == before ? "ok" : "FAILED", checks - checksBefore,
			failures - before, slow ? "" : " (SJ_SLOW=1 adds the 16M-element array)");
	}
	printf("sjtest: %d checks, %d failures\n", checks, failures);
	return failures != 0;
}
