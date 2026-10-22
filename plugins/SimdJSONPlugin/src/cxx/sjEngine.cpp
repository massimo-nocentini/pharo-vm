/* sjEngine.cpp -- the C++17 engine of the SimdJSONPlugin (see sjBridge.h)
 *
 * sj_read parses one JSON value of a source with simdjson's DOM and walks
 * it, calling the builder of SimdJSONPlugin.c for every Smalltalk object.
 * It includes no VM header.  Only simdjson's public API is used, and no
 * std:: container, string or throwing allocation: the plugin is compiled
 * with -fno-exceptions, so a failed allocation must be an answer.
 *
 * The state kept between calls is the parser, the input buffer, the token
 * list, the counters and one cache entry.  The cache makes the primitive
 * idempotent: when a build fails for lack of object memory, the VM calls
 * the primitive again up to 5 times (with garbage collections in between,
 * which can move the source), and the image may grow memory and call it
 * again; a call on the same bytes reuses the parse instead of parsing
 * again.  Any parse invalidates the cache first, and a successful build
 * drops it.
 */

#include <chrono>
#include <cstdlib>
#include <cstring>
#include <new>

#include "simdjson.h"
#include "sjBridge.h"
#include "sjText.h"

#ifdef SJ_TEST_HOOKS
/* Test harness only: 1 counts one number token more than there are, 2
   moves the first exponent token to the next number (tests/sjtest.cpp). */
extern "C" { int sj_test_number_fault; }
#endif

namespace {

using namespace simdjson;

#ifdef __EMSCRIPTEN__
const size_t MiB = 1024 * 1024;
#endif
const int64_t MinSmallInteger = -(int64_t(1) << 60);
const int64_t MaxSmallInteger = (int64_t(1) << 60) - 1;
const size_t SaturatedSize = 0xFFFFFF;	/* dom::array::size() and dom::object::size() stop there */

enum { CALLS, PARSES, CACHE_HITS, BUILDS, BUILD_FAILURES, OVER_BUDGET, MEASURES,
	NUMBER_TEXTS, GATE_HITS, ENUMERATIONS, PARSE_NANOS, BUILD_NANOS, NUM_STATS };
static_assert(NUM_STATS == SJ_NUM_STATS, "the counters of sj_info");

/* WHOLE: [p, e) is the whole rest of the input but its trailing white
   space (FIRST mode); SLICE: [p, e) is the value found by sj_value_end
   (NEXT mode, or FIRST mode when the whole rest failed or is over the
   cap).  The unit after a value is always found from e. */
enum Kind { WHOLE, SLICE };

struct Cache {
	bool valid;
	size_t p, e, length;		/* units [p, e) of the source, length bytes in buffer */
	Kind kind;
	dom::element root;
	bool tokensReady, enumerated;	/* tokens computed (at the first DOUBLE); by the enumerator (else none) */
	int64_t totalNumbers;		/* all number tokens, when enumerated */
	size_t tokenCount;
	bool measured;
	unsigned measureFlags;
	int64_t need;
};

dom::parser *parser;
char *buffer;			/* the UTF-8 text of the cache entry, then SIMDJSON_PADDING spaces */
size_t bufferSize;
sj_token *tokenList;
size_t tokenCap;
Cache cache;
int64_t stats[NUM_STATS];
char implementationName[32];

int64_t
nanos()
{
	return std::chrono::duration_cast<std::chrono::nanoseconds>(
		std::chrono::steady_clock::now().time_since_epoch()).count();
}

int
widthOf(const sj_input &in)
{
	return in.encoding == SJ_ENC_UTF32 ? 4 : 1;
}

size_t
elementCount(dom::array a)
{
	size_t n = a.size();

	if (n == SaturatedSize) {
		n = 0;
		for (dom::element child : a) { (void)child; n++; }
	}
	return n;
}

size_t
pairCount(dom::object o)
{
	size_t n = o.size();

	if (n == SaturatedSize) {
		n = 0;
		for (dom::key_value_pair pair : o) { (void)pair; n++; }
	}
	return n;
}

/* Transcode units [p, e) of the source into the buffer, followed by the
   padding simdjson reads past the end.  False when malloc failed. */
bool
fillBuffer(const sj_input &in, size_t p, size_t e, size_t *length)
{
	size_t n = sj_utf8_length(in.base, in.encoding, p, e);

	if (bufferSize < n + SIMDJSON_PADDING) {
		char *grown = static_cast<char *>(std::malloc(n + SIMDJSON_PADDING));
		if (!grown)
			return false;
		std::free(buffer);
		buffer = grown;
		bufferSize = n + SIMDJSON_PADDING;
	}
	sj_transcode(in.base, in.encoding, p, e, buffer);
	std::memset(buffer + n, ' ', SIMDJSON_PADDING);
	*length = n;
	return true;
}

/* Whether units [p, e) of the source transcode to the cached text.
   Latin-1 and UTF-32 sources are transcoded by chunks on the stack. */
bool
sameText(const sj_input &in, size_t p, size_t e)
{
	char chunk[4096];
	size_t done = 0, i, to, n;

	if (in.encoding == SJ_ENC_UTF8)
		return e - p == cache.length && std::memcmp(static_cast<const char *>(in.base) + p, buffer, cache.length) == 0;
	for (i = p; i < e; i = to) {
		to = e - i > 1024 ? i + 1024 : e;
		n = sj_utf8_length(in.base, in.encoding, i, to);
		if (n > cache.length - done)
			return false;
		sj_transcode(in.base, in.encoding, i, to, chunk);
		if (std::memcmp(chunk, buffer + done, n) != 0)
			return false;
		done += n;
	}
	return done == cache.length;
}

error_code
parseBuffer(size_t length, dom::element *root)
{
	if (!parser) {
		parser = new (std::nothrow) dom::parser();
		if (!parser)
			return MEMALLOC;
		parser->number_as_string(true);
	}
#ifdef __EMSCRIPTEN__
	/* malloc memory is never given back on wasm: grow by 4 MiB steps */
	if (parser->capacity() < length) {
		error_code error = parser->allocate((length + 4 * MiB - 1) / (4 * MiB) * (4 * MiB));
		if (error)
			return error;
	}
#endif
	int64_t start = nanos();
	error_code error = parser->parse(buffer, length, false).get(*root);
	stats[PARSE_NANOS] += nanos() - start;
	stats[PARSES]++;
	return error;
}

bool
delimitsNumber(char c)
{
	return c == ',' || c == ']' || c == '}' || c == ':' || c == '[' || c == ' ' || c == '\t' || c == '\n' || c == '\r';
}

/* After the gate, a closer look at its letters: an exponent token also
   has a JSON delimiter (or the end of the text) before its mantissa and
   after its exponent, which hashes and URLs in strings seldom have. */
bool
exponentLikely(const char *text, size_t n)
{
	for (const char *letters = "eE"; *letters; letters++) {
		const char *end = text + n, *at = text, *q;
		while ((q = static_cast<const char *>(std::memchr(at, *letters, size_t(end - at))))) {
			const char *a = q, *z = q + 1;
			at = q + 1;
			while (a > text && ((a[-1] >= '0' && a[-1] <= '9') || a[-1] == '.'))
				a--;
			if (a == q || a[0] == '.')
				continue;
			if (a > text && a[-1] == '-')
				a--;
			if (z < end && (*z == '+' || *z == '-'))
				z++;
			if (z == end || *z < '0' || *z > '9')
				continue;
			while (z < end && *z >= '0' && *z <= '9')
				z++;
			if ((a == text || delimitsNumber(a[-1])) && (z == end || delimitsNumber(*z)))
				return true;
		}
	}
	return false;
}

/* Number tokens of the cache entry, when EXACT asks for them: none
   unless the exponent gate finds a possible exponent token.  Only an
   exponent token becomes number text, and it is always a DOUBLE, so they
   are computed at the first DOUBLE of a walk: a document without one
   never pays the gate. */
bool
prepareTokens()
{
	if (cache.tokensReady)
		return true;
	cache.enumerated = false;
	cache.tokenCount = 0;
	if (sj_exponent_gate(buffer, cache.length)) {
		stats[GATE_HITS]++;
		if (exponentLikely(buffer, cache.length)) {
			stats[ENUMERATIONS]++;
			int64_t total = sj_number_tokens(buffer, cache.length, &tokenList, &cache.tokenCount, &tokenCap);
			if (total < 0)
				return false;
			cache.totalNumbers = total;
			cache.enumerated = true;
#ifdef SJ_TEST_HOOKS
			if (sj_test_number_fault == 1)
				cache.totalNumbers++;
			if (sj_test_number_fault == 2 && cache.tokenCount)
				tokenList[0].ordinal++;
#endif
		}
	}
	cache.tokensReady = true;
	return true;
}

/* Walks the DOM in pre-order, counting the number elements, and tells
   which of them are exponent tokens that must become number text. */
struct Numbers {
	bool lazy;		/* EXACT, and the tokens are not computed yet */
	const sj_token *tokens;
	size_t count, next;
	int64_t ordinal;
	bool mismatch;
	bool noMemory;		/* computing the tokens failed */

	/* The token of the number element of type t, or null for a value.
	   Ordinals are absolute, so tokens computed late still match; one the
	   enumerator put before the first DOUBLE leaves next short of count. */
	const sj_token *
	step(dom::element_type t)
	{
		const sj_token *token = nullptr;

		if (lazy && t == dom::element_type::DOUBLE) {
			lazy = false;
			if (prepareTokens()) {
				tokens = tokenList;
				count = cache.tokenCount;
			} else
				noMemory = true;
		}
		if (next < count && tokens[next].ordinal == ordinal) {
			if (t == dom::element_type::DOUBLE)
				token = &tokens[next++];
			else
				mismatch = true;
		}
		ordinal++;
		return token;
	}

	bool
	finished(int64_t total, bool enumerated)
	{
		return !mismatch && (!enumerated || (ordinal == total && next == count));
	}
};

/* The exact bytes of the objects a build allocates (see sjText.h for the
   Spur sizes); the builder's rules are those of SimdJSONPlugin.c. */
struct Measure {
	unsigned flags;
	Numbers numbers;
	uint64_t fixups;	/* number texts that end up in the fixups Array */
	sj_keyset *keys;	/* SJ_F_SYMKEYS: the distinct keys of the document */
	uint64_t distinctKeys, keyBytes;
	bool noMemory;

	static uint64_t
	stringBytes(const char *s, size_t n)
	{
		size_t chars;
		uint32_t maxcp;

		sj_utf8_scan(s, n, &chars, &maxcp);
		return maxcp > 0xFF ? sj_spur_words32(chars) : sj_spur_bytes(chars);
	}

	uint64_t
	keyCost(std::string_view key)
	{
		int isNew;

		if (!(flags & SJ_F_SYMKEYS))
			return stringBytes(key.data(), key.size());
		if (sj_keyset_index(keys, key.data(), key.size(), &isNew) < 0) {
			noMemory = true;
			return 0;
		}
		if (isNew) {
			distinctKeys++;
			keyBytes += stringBytes(key.data(), key.size());
		}
		return 0;
	}

	/* *text: the value is a number text, stored right after its creation */
	uint64_t
	value(dom::element e, bool *text)
	{
		*text = false;
		switch (e.type()) {
		case dom::element_type::INT64: {
			int64_t v = e.get_int64().value_unsafe();
			numbers.step(dom::element_type::INT64);
			return v < MinSmallInteger || v > MaxSmallInteger ? 16 : 0;
		}
		case dom::element_type::UINT64:
			numbers.step(dom::element_type::UINT64);
			return e.get_uint64().value_unsafe() > uint64_t(MaxSmallInteger) ? 16 : 0;
		case dom::element_type::DOUBLE: {
			const sj_token *token = numbers.step(dom::element_type::DOUBLE);
			if (token) {
				*text = true;
				return sj_spur_bytes(token->length);
			}
			return sj_is_small_float(e.get_double().value_unsafe()) ? 0 : 16;
		}
		case dom::element_type::BIGINT:
			numbers.step(dom::element_type::BIGINT);
			*text = true;
			return sj_spur_bytes(e.get_bigint().value_unsafe().size());
		case dom::element_type::STRING: {
			std::string_view s = e.get_string().value_unsafe();
			return stringBytes(s.data(), s.size());
		}
		case dom::element_type::ARRAY: {
			dom::array a = e.get_array().value_unsafe();	/* named: iterating a temporary dangles */
			bool childText;
			uint64_t bytes = sj_spur_pointers(elementCount(a));
			for (dom::element child : a) {
				bytes += value(child, &childText);
				fixups += childText;
			}
			return bytes;
		}
		case dom::element_type::OBJECT: {
			dom::object o = e.get_object().value_unsafe();
			return flags & SJ_F_DICT ? dictionary(o) : flatMap(o);
		}
		default:	/* null, true, false */
			return 0;
		}
	}

	uint64_t
	flatMap(dom::object o)
	{
		uint64_t bytes = sj_spur_pointers(2 * pairCount(o));
		bool text;

		for (dom::key_value_pair pair : o) {
			bytes += keyCost(pair.key);
			bytes += value(pair.value, &text);
			fixups += text;
		}
		return bytes;
	}

	/* A Dictionary, its array, a String per key occurrence and an
	   Association per distinct key.  A duplicate key replaces the value of
	   the first one (last wins), and its fixup entry goes with it. */
	uint64_t
	dictionary(dom::object o)
	{
		size_t n = pairCount(o), i, j, distinct = 0;
		uint64_t bytes = sj_spur_pointers(2) + sj_spur_pointers(sj_dict_capacity(n));
		bool text;

		if (n <= 32) {	/* each key against the ones after it */
			uint32_t texts = 0;
			i = 0;
			for (dom::key_value_pair pair : o) {
				bytes += stringBytes(pair.key.data(), pair.key.size());
				bytes += value(pair.value, &text);
				texts |= uint32_t(text) << i++;
			}
			i = 0;
			for (dom::object::iterator it = o.begin(); it != o.end(); ++it, i++) {
				dom::object::iterator later = it;
				for (++later; later != o.end() && later.key() != it.key(); ++later)
					;
				if (later == o.end()) {
					distinct++;
					fixups += (texts >> i) & 1;
				}
			}
			return bytes + distinct * sj_spur_pointers(2);
		}
		/* larger objects: the index of each key, and the last pair of each index */
		sj_keyset *seen = sj_keyset_new(n);
		uint32_t *index = static_cast<uint32_t *>(std::malloc(n * sizeof(uint32_t)));
		uint32_t *last = static_cast<uint32_t *>(std::malloc((n + 1) * sizeof(uint32_t)));
		uint8_t *texts = static_cast<uint8_t *>(std::malloc(n));
		if (seen && index && last && texts) {
			int isNew;
			int64_t k;
			i = 0;
			for (dom::key_value_pair pair : o) {
				if ((k = sj_keyset_index(seen, pair.key.data(), pair.key.size(), &isNew)) < 0) {
					noMemory = true;
					break;
				}
				index[i] = uint32_t(k);
				last[k] = uint32_t(i);
				distinct += isNew;
				bytes += stringBytes(pair.key.data(), pair.key.size());
				bytes += value(pair.value, &text);
				texts[i++] = text;
			}
			for (j = 0; j < i; j++)
				fixups += texts[j] && last[index[j]] == j;
		} else
			noMemory = true;
		sj_keyset_free(seen);
		std::free(index);
		std::free(last);
		std::free(texts);
		return bytes + distinct * sj_spur_pointers(2);
	}
};

/* Builds the objects through the builder, in pre-order; every scalar is
   stored right after its creation, which is how the builder recognizes
   the number text it answered last (the root's stays pending). */
struct Build {
	const sj_builder *b;
	const char *text;
	Numbers numbers;
	bool failed;		/* a builder call failed */

	sj_oop
	fail()
	{
		failed = true;
		return 0;
	}

	sj_oop
	value(dom::element e)
	{
		switch (e.type()) {
		case dom::element_type::NULL_VALUE:
			return b->nilOop;
		case dom::element_type::BOOL:
			return e.get_bool().value_unsafe() ? b->trueOop : b->falseOop;
		case dom::element_type::INT64:
			numbers.step(dom::element_type::INT64);
			return checked(b->int64(e.get_int64().value_unsafe()));
		case dom::element_type::UINT64:
			numbers.step(dom::element_type::UINT64);
			return checked(b->uint64(e.get_uint64().value_unsafe()));
		case dom::element_type::DOUBLE: {
			const sj_token *token = numbers.step(dom::element_type::DOUBLE);
			if (numbers.noMemory)
				return fail();
			if (token)
				return checked(b->number_text(text + token->offset, token->length));
			return checked(b->float64(e.get_double().value_unsafe()));
		}
		case dom::element_type::BIGINT: {
			std::string_view digits = e.get_bigint().value_unsafe();
			numbers.step(dom::element_type::BIGINT);
			return checked(b->number_text(digits.data(), digits.size()));
		}
		case dom::element_type::STRING: {
			std::string_view s = e.get_string().value_unsafe();
			return checked(b->string(s.data(), s.size()));
		}
		case dom::element_type::ARRAY: {
			dom::array a = e.get_array().value_unsafe();
			size_t i = 0;
			sj_oop array = b->array(elementCount(a));
			if (!array)
				return fail();
			for (dom::element child : a) {
				sj_oop v = value(child);
				if (!v)
					return 0;
				if (b->array_put(array, i++, v) < 0)
					return fail();
			}
			return array;
		}
		case dom::element_type::OBJECT: {
			dom::object o = e.get_object().value_unsafe();
			size_t i = 0;
			sj_oop object = b->object(pairCount(o));
			if (!object)
				return fail();
			for (dom::key_value_pair pair : o) {
				sj_oop k = b->key(pair.key.data(), pair.key.size());
				if (!k)
					return fail();
				sj_oop v = value(pair.value);
				if (!v)
					return 0;
				if (b->object_put(object, i++, k, v) < 0)
					return fail();
			}
			return object;
		}
		}
		return fail();
	}

	sj_oop
	checked(sj_oop oop)
	{
		return oop ? oop : fail();
	}
};

Numbers
numbersFor(unsigned flags)
{
	bool exact = (flags & SJ_F_EXACT) != 0;
	Numbers numbers = { exact && !cache.tokensReady, tokenList,
		exact && cache.tokensReady ? cache.tokenCount : 0, 0, 0, false, false };
	return numbers;
}

/* SJ_OK, or SJ_ERR_NUMBERS, or SJ_RC_NO_C_MEMORY; *need is meaningful
   with SJ_OK. */
int
measure(unsigned flags, int64_t *need)
{
	unsigned key = flags & (SJ_F_EXACT | SJ_F_DICT | SJ_F_SYMKEYS);

	if (cache.measured && cache.measureFlags == key) {
		*need = cache.need;
		return SJ_OK;
	}
	stats[MEASURES]++;
	Measure m = { flags, numbersFor(flags), 0, nullptr, 0, 0, false };
	if ((flags & SJ_F_SYMKEYS) && !(m.keys = sj_keyset_new(64)))
		return SJ_RC_NO_C_MEMORY;
	bool text;
	uint64_t bytes = sj_spur_pointers(6) + m.value(cache.root, &text);
	m.fixups += text;
	if (m.fixups)
		bytes += sj_spur_pointers(2 * m.fixups);
	if (flags & SJ_F_SYMKEYS) {	/* the keys Array and its Strings */
		bytes += sj_spur_pointers(m.distinctKeys) + m.keyBytes;
		sj_keyset_free(m.keys);
	}
	if (m.noMemory || m.numbers.noMemory)
		return SJ_RC_NO_C_MEMORY;
	if (!m.numbers.finished(cache.totalNumbers, (flags & SJ_F_EXACT) && cache.enumerated))
		return SJ_ERR_NUMBERS;
	cache.measured = true;
	cache.measureFlags = key;
	cache.need = int64_t(bytes);
	*need = cache.need;
	return SJ_OK;
}

void
dropParser()
{
	delete parser;
	parser = nullptr;
	std::free(buffer);
	buffer = nullptr;
	bufferSize = 0;
}

int
answerError(sj_outcome *out, int code)
{
	out->status = SJ_ERROR;
	out->code = code;
	return SJ_RC_DONE;
}

} // namespace

extern "C" int
sj_read(const sj_request *rq, const sj_builder *b, sj_outcome *out)
{
	const sj_input &in = rq->in;
	int width = widthOf(in);
	size_t p, e, trimEnd = 0, valueEnd = 0, length;
	Kind kind;
	bool hit;

	stats[CALLS]++;
	out->status = SJ_OK;
	out->code = 0;
	out->need = 0;
	out->value = 0;
	out->next0 = in.start0;
	if (in.start0 > in.stop0 || in.stop0 > in.units)
		return answerError(out, simdjson::INDEX_OUT_OF_BOUNDS);

	/* 1-3: the value starts at p; a candidate end */
	p = sj_skip_ws(in.base, width, in.start0, in.stop0);
	if (p == in.stop0)
		return answerError(out, simdjson::EMPTY);
	if (sj_is_bom(in.base, in.encoding, p, in.stop0))	/* NeoJSON rejects it, simdjson would skip it */
		return answerError(out, SJ_ERR_BOM);
	if (rq->flags & SJ_F_NEXT) {
		e = valueEnd = sj_value_end(in.base, width, p, in.stop0);
		kind = SLICE;
	} else {
		e = trimEnd = sj_trim_ws(in.base, width, p, in.stop0);
		kind = WHOLE;
	}
	/* the cap applies to what simdjson is given: a whole rest over it
	   gives way to the value slice, and a value over it is refused */
	if (rq->maxUnits >= 0 && e - p > uint64_t(rq->maxUnits)) {
		if (kind == WHOLE) {
			e = valueEnd = sj_value_end(in.base, width, p, in.stop0);
			kind = SLICE;
		}
		if (e - p > uint64_t(rq->maxUnits)) {
			out->status = SJ_TOO_LARGE;
			out->need = int64_t(e - p);
			return SJ_RC_DONE;
		}
	}

	/* 4: the cache, before any parse */
	hit = cache.valid && cache.p == p;
	if (hit) {
		if (cache.kind == WHOLE && kind == WHOLE)
			hit = cache.e == trimEnd;
		else {
			if (!valueEnd)
				valueEnd = sj_value_end(in.base, width, p, in.stop0);
			hit = cache.e == valueEnd;
		}
		hit = hit && sameText(in, p, cache.e);
	}
	if (hit)
		stats[CACHE_HITS]++;
	else {
		/* 5: parse; FIRST mode falls back to the value slice */
		cache.valid = false;
		if (!fillBuffer(in, p, e, &length))
			return SJ_RC_NO_C_MEMORY;
		simdjson::error_code status = parseBuffer(length, &cache.root);
		if (status && kind == WHOLE) {
			if (!valueEnd)
				valueEnd = sj_value_end(in.base, width, p, in.stop0);
			if (valueEnd < e) {
				/* a prefix of the same text: only the padding moves */
				e = valueEnd;
				kind = SLICE;
				length = sj_utf8_length(in.base, in.encoding, p, e);
				std::memset(buffer + length, ' ', SIMDJSON_PADDING);
				status = parseBuffer(length, &cache.root);
			}
		}
		if (status == simdjson::MEMALLOC)
			return SJ_RC_NO_C_MEMORY;
		if (status)
			return answerError(out, status);
		/* 6 */
		cache.valid = true;
		cache.p = p;
		cache.e = e;
		cache.kind = kind;
		cache.length = length;
		cache.tokensReady = false;
		cache.enumerated = false;
		cache.tokenCount = 0;
		cache.measured = false;
	}

	/* 7: the number tokens are computed by the walks, at their first DOUBLE */

	/* 8: budget */
	if (rq->budget >= 0 && sj_bound(cache.length) > uint64_t(rq->budget)) {
		int64_t need;
		int status = measure(rq->flags, &need);
		if (status == SJ_RC_NO_C_MEMORY)
			return SJ_RC_NO_C_MEMORY;
		if (status == SJ_ERR_NUMBERS) {
			cache.valid = false;
			return answerError(out, SJ_ERR_NUMBERS);
		}
		if (need > rq->budget) {
			stats[OVER_BUDGET]++;
			out->status = SJ_OVER_BUDGET;
			out->need = need;
			return SJ_RC_DONE;
		}
	}

	/* 9: build */
	stats[BUILDS]++;
	Build build = { b, buffer, numbersFor(rq->flags), false };
	int64_t start = nanos();
	sj_oop root = build.value(cache.root);
	stats[BUILD_NANOS] += nanos() - start;
	if (build.numbers.noMemory)
		return SJ_RC_NO_C_MEMORY;
	if (build.failed) {
		stats[BUILD_FAILURES]++;
		return SJ_RC_BUILD_FAILED;
	}
	if (!build.numbers.finished(cache.totalNumbers, (rq->flags & SJ_F_EXACT) && cache.enumerated)) {
		cache.valid = false;
		return answerError(out, SJ_ERR_NUMBERS);
	}

	/* 10: success */
	cache.valid = false;
	out->value = root;
	out->next0 = sj_skip_ws(in.base, width, cache.e, in.stop0);	/* in.stop0 after a WHOLE entry */
#ifdef __EMSCRIPTEN__
	if (parser->capacity() > 16 * MiB)	/* give the DOM's memory back to dlmalloc */
		dropParser();
#endif
	return SJ_RC_DONE;
}

extern "C" void
sj_release(int free_parser)
{
	cache.valid = false;
	if (free_parser) {
		dropParser();
		std::free(tokenList);
		tokenList = nullptr;
		tokenCap = 0;
	}
}

extern "C" const char *
sj_error_message(int code)
{
	switch (code) {
	case SJ_ERR_BOM:
		return "byte order mark (U+FEFF) at the start of a value";
	case SJ_ERR_NUMBERS:
		return "number tokens do not match the parsed document";
	default:
		return code >= 0 && code < simdjson::NUM_ERROR_CODES
			? simdjson::error_message(simdjson::error_code(code)) : nullptr;
	}
}

extern "C" void
sj_get_info(sj_info *info)
{
	if (!implementationName[0]) {
		/* name() answers a temporary; the names are short enough for the
		   small-string buffer, so it allocates nothing */
		auto name = simdjson::get_active_implementation()->name();
		std::strncpy(implementationName, name.c_str(), sizeof(implementationName) - 1);
	}
	std::memcpy(info->implementation, implementationName, sizeof(info->implementation));
#ifdef __EMSCRIPTEN__
	info->emscripten = 1;
#else
	info->emscripten = 0;
#endif
	info->capacity = parser ? int64_t(parser->capacity()) : 0;
	info->cachedBytes = int64_t(bufferSize);
	std::memcpy(info->stats, stats, sizeof(info->stats));
}

extern "C" void
sj_note_number_text(void)
{
	stats[NUMBER_TEXTS]++;
}
