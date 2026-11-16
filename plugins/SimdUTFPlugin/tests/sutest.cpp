/* sutest.cpp -- C-level tests of the SimdUTFPlugin engine and of libSimdUTFFFI
 *
 * Links suEngine.cpp, suFFI.c and simdutf 9.4.0 (src/simdutf.cpp, with the
 * six features of cmake/simdutf.cmake).  The references are written here,
 * independently of simdutf: UTF-8 after RFC 3629 and Table 3-7 of the
 * Unicode Standard (with simdutf's names for the kinds of error), UTF-16,
 * and the base64 encoder of Zinc with its line rule
 * (ZnBase64Encoder>>encode:to:).  simdutf itself is the oracle only of base64
 * decoding (base64_to_binary_safe with a large buffer) and of the positions
 * of UTF-8 errors (validate_utf8_with_errors).  The checks:
 *
 *   U1  encode: Latin-1 and UTF-32 sources to UTF-8, UTF-16LE and UTF-16BE,
 *       with and without a BOM, lengths 0-300 and longer SIMD edges, 100,000
 *       random texts of five families (ASCII, Latin, BMP, astral, mixed),
 *       sub-ranges with absolute positions; UTF-32 words that are not
 *       scalar values (0xD800, 0xDFFF, 0x110000, 0x3FFFFFFF, 0xFFFFFFFF)
 *       first, in the middle and last
 *   U2  decode: UTF-8 and UTF-16 to the narrowest String, to WideString and
 *       to Latin-1; the >= 0xC4 rule for every byte value valid UTF-8 can
 *       hold at every alignment, and the SWAR expression on all 256 values;
 *       200,000 random texts; BOM skipping; crafted and mutated invalid
 *       input against the reference and validate_utf8_with_errors; 1001 at
 *       stop; 1002 at the first character above U+00FF; validity first
 *   U3  measure: ENCODED and DECODED are what U1 and U2 made; BASE64 is the
 *       decoded size of valid input and the portable rule on any input,
 *       bytes >= 0x80 included
 *   U4  validate: UTF-8, UTF-16LE/BE and UTF-32, and ASCII of bytes and of
 *       words, against the references
 *   U5  base64 encode: options 0-3 x line lengths 0, 4, 8, 64, 76 x line
 *       ends LF, CR, CRLF x lengths 0-300 and random ones, against Zinc's
 *       rule; su_base64_encoded_size is what is written; odd offsets
 *   U6  base64 decode: options standard, URL, either x loose, strict against
 *       base64_to_binary_safe; white space anywhere; codes 7, 8, 9 and their
 *       positions; the allocation is the portable measure; TOO_LARGE before
 *       code 7; 1,000,000 random strings never give a size other than the
 *       measure nor OUTPUT_BUFFER_TOO_SMALL
 *   U7  sinks: refused allocations, one allocation at most and none on
 *       invalid UTF input, rooms one unit short and exact, empty results,
 *       requests outside the contract
 *   U8  the SIMDUTF_FORCE_IMPLEMENTATION guard (--expect-kernel, given by
 *       run-c-tests.sh, which runs one process per value), then a small
 *       corpus of U1 and U2
 *   U9  libSimdUTFFFI: every su_ffi_* answer is the status word of the
 *       engine's outcome; the failures in the primitive's order; NULL
 *       addresses; su_ffi_base64_encoded_size at its limits; the names
 *
 * With --slow (SU_SLOW=1 in run-c-tests.sh), round trips of 256 MiB inputs
 * per family.
 *
 * Usage: sutest [--only U1,U2,...] [--slow]
 *        sutest --only U8 --expect-kernel NAME|best --expect-forced TEXT
 *        sutest --kernels    (prints "best NAME" and "supported NAME...")
 * Prints one line per failed check and a summary; exits 1 when a check
 * failed.
 */

#include <algorithm>
#include <chrono>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <random>
#include <string>
#include <vector>

#include "simdutf.h"
#include "suEngine.h"
#include "suFFI.h"

static int checks, failures;
static const char *currentTest = "";

static void
fail(int line, const char *format, ...)
{
	va_list args;

	failures++;
	if (failures > 200)
		return;
	printf("FAIL %s sutest.cpp:%d: ", currentTest, line);
	va_start(args, format);
	vprintf(format, args);
	va_end(args);
	printf("\n");
}

#define CHECK(condition) (checks++, (condition) ? true : (fail(__LINE__, "%s", #condition), false))
#define CHECKF(condition, ...) (checks++, (condition) ? true : (fail(__LINE__, __VA_ARGS__), false))

static int64_t
nowNanos()
{
	return std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count();
}

/*** Random ***/

static std::mt19937_64 rng(0x5375746573740001ull);

static uint64_t
below(uint64_t n)
{
	return n ? rng() % n : 0;
}

static bool
chance(unsigned percent)
{
	return below(100) < percent;
}

static char32_t
randomChar(int family)
{
	switch (family) {
	case 0:
		return char32_t(below(0x80));
	case 1:
		return char32_t(chance(50) ? below(0x80) : 0x80 + below(0x80));
	case 2: {
		char32_t c;
		do {
			c = char32_t(below(0x10000));
		} while (c >= 0xD800 && c <= 0xDFFF);
		return c;
	}
	case 3:
		return char32_t(0x10000 + below(0x100000));
	default:
		return randomChar(int(below(4)));
	}
}

static std::u32string
randomText(int family, size_t n)
{
	std::u32string s;

	s.reserve(n);
	for (size_t i = 0; i < n; i++)
		s += randomChar(family);
	return s;
}

static std::string
randomBytes(size_t n)
{
	std::string s(n, '\0');

	for (size_t i = 0; i < n; i++)
		s[i] = char(rng());
	return s;
}

/* mostly short, sometimes a few hundred, rarely a few thousand */
static size_t
randomLength()
{
	uint64_t k = below(100);

	return k < 70 ? below(65) : k < 95 ? below(301) : below(3001);
}

static const size_t edges[] = { 0, 1, 2, 3, 4, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 129,
	255, 256, 257, 511, 512, 513, 1023, 1024, 1025, 4095, 4096, 4097 };

/*** References, written again here ***/

static size_t
utf8Length(char32_t c)
{
	return c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
}

static size_t
utf16Length(char32_t c)
{
	return c < 0x10000 ? 2 : 4;
}

static std::string
utf8Of(const std::u32string &cps)
{
	std::string out;

	for (char32_t c : cps) {
		if (c < 0x80) {
			out += char(c);
		} else if (c < 0x800) {
			out += char(0xC0 | c >> 6);
			out += char(0x80 | (c & 0x3F));
		} else if (c < 0x10000) {
			out += char(0xE0 | c >> 12);
			out += char(0x80 | (c >> 6 & 0x3F));
			out += char(0x80 | (c & 0x3F));
		} else {
			out += char(0xF0 | c >> 18);
			out += char(0x80 | (c >> 12 & 0x3F));
			out += char(0x80 | (c >> 6 & 0x3F));
			out += char(0x80 | (c & 0x3F));
		}
	}
	return out;
}

static void
putUnit(std::string &out, unsigned u, bool le)
{
	if (le) {
		out += char(u & 0xFF);
		out += char(u >> 8);
	} else {
		out += char(u >> 8);
		out += char(u & 0xFF);
	}
}

static std::string
utf16Of(const std::u32string &cps, bool le)
{
	std::string out;

	for (char32_t c : cps) {
		if (c < 0x10000) {
			putUnit(out, c, le);
		} else {
			putUnit(out, 0xD800 + ((c - 0x10000) >> 10), le);
			putUnit(out, 0xDC00 + ((c - 0x10000) & 0x3FF), le);
		}
	}
	return out;
}

static bool
isUtf16(unsigned encoding)
{
	return encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

static std::string
encodingOf(const std::u32string &cps, unsigned encoding)
{
	return encoding == SU_ENC_UTF8 ? utf8Of(cps) : utf16Of(cps, encoding == SU_ENC_UTF16LE);
}

static std::string
bomOf(unsigned encoding)
{
	return encoding == SU_ENC_UTF8 ? "\xEF\xBB\xBF" : encoding == SU_ENC_UTF16LE ? "\xFF\xFE" : "\xFE\xFF";
}

/* the bytes of the first i characters */
static size_t
offsetOf(const std::u32string &cps, size_t i, unsigned encoding)
{
	size_t n = 0;

	for (size_t k = 0; k < i; k++)
		n += encoding == SU_ENC_UTF8 ? utf8Length(cps[k]) : utf16Length(cps[k]);
	return n;
}

static char32_t
maxOf(const std::u32string &s)
{
	char32_t m = 0;

	for (char32_t c : s)
		m = std::max(m, c);
	return m;
}

static size_t
firstAbove(const std::u32string &s, char32_t limit)
{
	for (size_t i = 0; i < s.size(); i++)
		if (s[i] > limit)
			return i;
	return s.size();
}

static std::string
latin1Of(const std::u32string &cps)
{
	std::string out;

	for (char32_t c : cps)
		out += char(c);
	return out;
}

/* Table 3-7: the length of the valid prefix of b, and in *code the kind of
   the ill-formed sequence found there (0 when b is valid), as simdutf names
   them: a continuation byte where a character starts is TOO_LONG, a byte
   F8-FF HEADER_BITS, a missing continuation byte TOO_SHORT, then OVERLONG,
   TOO_LARGE, SURROGATE by the value */
static size_t
utf8ValidPrefix(const std::string &s, int *code)
{
	const unsigned char *b = reinterpret_cast<const unsigned char *>(s.data());
	size_t n = s.size(), i = 0;

	while (i < n) {
		unsigned c = b[i];
		size_t k;
		uint32_t cp, least;
		if (c < 0x80) {
			i++;
			continue;
		}
		if (c < 0xC0) {
			*code = SU_E_TOO_LONG;
			return i;
		} else if (c < 0xE0) {
			k = 2, cp = c & 0x1F, least = 0x80;
		} else if (c < 0xF0) {
			k = 3, cp = c & 0x0F, least = 0x800;
		} else if (c < 0xF8) {
			k = 4, cp = c & 0x07, least = 0x10000;
		} else {
			*code = SU_E_HEADER_BITS;
			return i;
		}
		if (i + k > n) {
			*code = SU_E_TOO_SHORT;
			return i;
		}
		for (size_t j = 1; j < k; j++) {
			if ((b[i + j] & 0xC0) != 0x80) {
				*code = SU_E_TOO_SHORT;
				return i;
			}
			cp = cp << 6 | (b[i + j] & 0x3F);
		}
		if (cp < least) {
			*code = SU_E_OVERLONG;
			return i;
		}
		if (cp > 0x10FFFF) {
			*code = SU_E_TOO_LARGE;
			return i;
		}
		if (cp >= 0xD800 && cp <= 0xDFFF) {
			*code = SU_E_SURROGATE;
			return i;
		}
		i += k;
	}
	*code = 0;
	return n;
}

static std::u32string
utf8Decode(const std::string &s)
{
	std::u32string out;
	const unsigned char *b = reinterpret_cast<const unsigned char *>(s.data());

	for (size_t i = 0; i < s.size();) {
		unsigned c = b[i];
		size_t k = c < 0x80 ? 1 : c < 0xE0 ? 2 : c < 0xF0 ? 3 : 4;
		char32_t cp = k == 1 ? c : k == 2 ? c & 0x1F : k == 3 ? c & 0x0F : c & 0x07;
		for (size_t j = 1; j < k; j++)
			cp = cp << 6 | (b[i + j] & 0x3F);
		out += cp;
		i += k;
	}
	return out;
}

static unsigned
unitAt(const std::string &s, size_t i, bool le)
{
	unsigned a = (unsigned char)s[i], b = (unsigned char)s[i + 1];

	return le ? a | b << 8 : a << 8 | b;
}

/* the byte offset of the first unit of s (even size) that is not part of a
   valid pair or a non-surrogate, and in *code SURROGATE, or 0 when valid */
static size_t
utf16ValidPrefix(const std::string &s, bool le, int *code)
{
	size_t i = 0;

	while (i < s.size()) {
		unsigned u = unitAt(s, i, le);
		if (u >= 0xD800 && u <= 0xDBFF && i + 2 < s.size()) {
			unsigned v = unitAt(s, i + 2, le);
			if (v >= 0xDC00 && v <= 0xDFFF) {
				i += 4;
				continue;
			}
		}
		if (u >= 0xD800 && u <= 0xDFFF) {
			*code = SU_E_SURROGATE;
			return i;
		}
		i += 2;
	}
	*code = 0;
	return i;
}

static std::u32string
utf16Decode(const std::string &s, bool le)
{
	std::u32string out;

	for (size_t i = 0; i < s.size(); i += 2) {
		unsigned u = unitAt(s, i, le);
		if (u >= 0xD800 && u <= 0xDBFF) {
			out += char32_t(0x10000 + ((u - 0xD800) << 10) + (unitAt(s, i + 2, le) - 0xDC00));
			i += 2;
		} else {
			out += char32_t(u);
		}
	}
	return out;
}

/* ZnBase64Encoder>>encode:to: -- 4 is counted for every group, padded or
   not, and the line end is written when the count reaches the line length,
   after the last group too */
static std::string
zincBase64(const std::string &b, unsigned options, size_t lineLength, unsigned lineEnd)
{
	static const char standard[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	static const char url[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
	const char *alphabet = options & 1 ? url : standard;
	bool padded = options == 0 || options == 3;
	const char *eol = lineEnd == 0 ? "\n" : lineEnd == 1 ? "\r" : "\r\n";
	std::string out;
	size_t count = 0;

	for (size_t i = 0; i < b.size(); i += 3) {
		size_t k = std::min<size_t>(3, b.size() - i);
		uint32_t v = (unsigned char)b[i] << 16;
		if (k > 1)
			v |= (unsigned char)b[i + 1] << 8;
		if (k > 2)
			v |= (unsigned char)b[i + 2];
		out += alphabet[v >> 18 & 63];
		out += alphabet[v >> 12 & 63];
		if (k > 1)
			out += alphabet[v >> 6 & 63];
		else if (padded)
			out += '=';
		if (k > 2)
			out += alphabet[v & 63];
		else if (padded)
			out += '=';
		if (lineLength && (count += 4) == lineLength) {
			out += eol;
			count = 0;
		}
	}
	return out;
}

static bool
isBase64Space(unsigned char c)
{
	return c == ' ' || c == '\t' || c == '\n' || c == '\f' || c == '\r';
}

/* The portable measure of the contract (primitiveMeasure, SU_OP_BASE64) */
static size_t
portableMeasure(const std::string &s)
{
	size_t count = 0, padding = 0;

	for (unsigned char c : s)
		if (c > 0x20 && c < 0x80)
			count++;
	for (size_t i = s.size(); i-- > 0 && padding < 2;) {
		unsigned char c = s[i];
		if (c == '=')
			padding++;
		else if (c > 0x20 && c < 0x80)
			break;
	}
	return (count - padding) * 3 / 4;
}

static size_t
lastNonSpace(const std::string &s)
{
	size_t n = s.size();

	while (n > 0 && isBase64Space(s[n - 1]))
		n--;
	return n ? n - 1 : 0;
}

/*** Objects: the memory of a source or a destination, 8-byte aligned as in
     the VM ***/

struct Obj {
	std::vector<uint64_t> mem;
	size_t units = 0;
	int words = 0;
	void *base() { return mem.data(); }
	char *bytes() { return reinterpret_cast<char *>(mem.data()); }
};

static Obj
byteObj(const std::string &b)
{
	Obj o;

	o.mem.assign(b.size() / 8 + 1, 0);
	if (!b.empty())
		memcpy(o.mem.data(), b.data(), b.size());
	o.units = b.size();
	return o;
}

static Obj
wordObj(const std::u32string &w)
{
	Obj o;

	o.mem.assign(w.size() / 2 + 1, 0);
	if (!w.empty())
		memcpy(o.mem.data(), w.data(), w.size() * 4);
	o.units = w.size();
	o.words = 1;
	return o;
}

static su_input
inputOf(Obj &o, size_t s, size_t e)
{
	su_input in;

	in.base = o.base();
	in.units = o.units;
	in.words = o.words;
	in.start0 = s;
	in.stop0 = e;
	return in;
}

/*** Engine calls in allocate mode and in into mode ***/

struct Result {
	int rc = -99;
	su_outcome out{};
	int calls = 0;		/* allocations asked */
	int askedKind = -1;
	size_t askedUnits = 0;
	std::string bytes;	/* the result, of kind BYTES or LATIN1 */
	std::u32string chars;	/* the result, of kind UTF32 */
};

typedef std::function<int(const su_sink *, su_outcome *)> Call;

struct Allocator {
	Result *r;
	bool refuse;
	std::vector<uint64_t> mem;
};

static const uint64_t Sentinel = 0xA5A5A5A5A5A5A5A5ull;

static void *
allocateFor(void *ctx, int kind, size_t units)
{
	Allocator *a = static_cast<Allocator *>(ctx);

	a->r->calls++;
	a->r->askedKind = kind;
	a->r->askedUnits = units;
	if (a->refuse)
		return nullptr;
	a->mem.assign(units * (kind == SU_KIND_UTF32 ? 4 : 1) / 8 + 2, Sentinel);
	return a->mem.data();
}

static bool
untouched(const char *p, size_t from, size_t to)
{
	for (size_t i = from; i < to; i++)
		if ((unsigned char)p[i] != 0xA5)
			return false;
	return true;
}

/* Allocate mode: the result is what the one allocation holds, and nothing
   is written past it */
static Result
allocating(const Call &call, bool refuse = false)
{
	Result r;
	Allocator a{&r, refuse, {}};
	su_sink sink{nullptr, 0, 0, &a, allocateFor};

	r.rc = call(&sink, &r.out);
	CHECKF(r.calls <= 1, "%d allocations", r.calls);
	if (r.rc == SU_RC_DONE && r.out.status == SU_OK) {
		CHECKF(r.calls == 1 && r.askedUnits == r.out.units && r.askedKind == r.out.kind,
			"%d allocations of %zu units of kind %d, made %zu of kind %d", r.calls, r.askedUnits, r.askedKind,
			r.out.units, r.out.kind);
		size_t bytes = r.out.units * (r.out.kind == SU_KIND_UTF32 ? 4 : 1);
		const char *p = reinterpret_cast<const char *>(a.mem.data());
		if (r.out.kind == SU_KIND_UTF32)
			r.chars.assign(reinterpret_cast<const char32_t *>(p), r.out.units);
		else
			r.bytes.assign(p, bytes);
		CHECKF(untouched(p, bytes, a.mem.size() * 8), "written past the %zu bytes of the result", bytes);
	}
	return r;
}

/* Into mode: a destination of capacity units of kind, at byte offset at of
   a buffer of sentinels; nothing outside what the call reports written may
   change (except, when mayWrite, the room of a base64 decode that failed) */
static Result
intoSink(const Call &call, int kind, size_t capacity, size_t at, bool mayWrite = false)
{
	Result r;
	size_t unit = kind == SU_KIND_UTF32 ? 4 : 1;
	std::vector<uint64_t> mem((at + capacity * unit + 16) / 8 + 1, Sentinel);
	char *buf = reinterpret_cast<char *>(mem.data());
	su_sink sink{buf + at, capacity, kind, nullptr, nullptr};
	size_t written = 0;

	r.rc = call(&sink, &r.out);
	if (r.rc == SU_RC_DONE && r.out.status == SU_OK) {
		CHECKF(r.out.units <= capacity, "%zu units written in a room of %zu", r.out.units, capacity);
		written = std::min(r.out.units, capacity) * unit;
		if (kind == SU_KIND_UTF32)
			r.chars.assign(reinterpret_cast<const char32_t *>(buf + at), written / 4);
		else
			r.bytes.assign(buf + at, written);
	} else if (mayWrite && r.rc == SU_RC_DONE && r.out.status == SU_ERROR) {
		written = capacity * unit;
	}
	CHECKF(untouched(buf, 0, at) && untouched(buf, at + written, mem.size() * 8),
		"the destination changed outside the result (rc %d, status %d)", r.rc, r.out.status);
	return r;
}

static bool
isOk(const Result &r)
{
	return r.rc == SU_RC_DONE && r.out.status == SU_OK;
}

static bool
isError(const Result &r, int code, size_t position0)
{
	return r.rc == SU_RC_DONE && r.out.status == SU_ERROR && r.out.code == code && r.out.position0 == position0;
}

static bool
isNoRoom(const Result &r, size_t units)
{
	return r.rc == SU_RC_NO_ROOM && r.out.units == units;
}

static std::string
show(const Result &r)
{
	char text[160];

	snprintf(text, sizeof text, "rc %d status %d code %d at %zu units %zu kind %d", r.rc, r.out.status, r.out.code,
		r.out.position0, r.out.units, r.out.kind);
	return text;
}

/* the answer of a primitive for an engine outcome: docs/SimdUTF.md, "The
   status word" */
static int64_t
statusWord(int status, int code, uint64_t payload)
{
	return -int64_t(status + 4 * uint64_t(code) + 4096 * payload);
}

static int64_t
primitiveAnswer(const Result &r, bool packed = false)
{
	switch (r.rc) {
	case SU_RC_DONE:
		if (r.out.status == SU_ERROR)
			return statusWord(SU_ERROR, r.out.code, r.out.position0 + 1);
		return packed ? int64_t(r.out.units) * 2 + (r.out.kind == SU_KIND_UTF32) : int64_t(r.out.units);
	case SU_RC_NO_ROOM:
		return statusWord(SU_TOO_LARGE, 0, r.out.units);
	case SU_RC_BAD_INDEX:
		return SU_FFI_BAD_INDEX;
	default:
		return SU_FFI_BAD_ARGUMENT;
	}
}

/* A destination of the library: dstUnits units of unit bytes, sentinels;
   answers the bytes from at0 that a call wrote (written units) */
struct Dst {
	std::vector<uint64_t> mem;
	size_t unit;
	Dst(size_t units, size_t unitBytes) : mem((units * unitBytes + 16) / 8 + 1, Sentinel), unit(unitBytes) {}
	void *base() { return mem.data(); }
	const char *at(size_t i) { return reinterpret_cast<const char *>(mem.data()) + i * unit; }
};

/*** U1, U3, U9: encoding ***/

static const unsigned targets[] = { SU_ENC_UTF8, SU_ENC_UTF16LE, SU_ENC_UTF16BE };

/* text from [pre, pre + n) of a source; full: into mode and the library too */
static void
encodeText(const std::u32string &text, bool full)
{
	size_t pre = chance(30) ? below(9) : 0, post = chance(30) ? below(9) : 0;
	int family = maxOf(text) <= 0xFF ? 1 : 4;
	std::u32string all = randomText(family, pre) + text + randomText(family, post);
	std::vector<Obj> sources;

	sources.push_back(wordObj(all));
	if (maxOf(all) <= 0xFF)
		sources.push_back(byteObj(latin1Of(all)));
	for (Obj &src : sources) {
		su_input in = inputOf(src, pre, pre + text.size());
		for (unsigned target : targets) {
			for (unsigned bom : { 0u, unsigned(SU_F_BOM) }) {
				unsigned flags = target | bom;
				std::string expected = (bom ? bomOf(target) : "") + encodingOf(text, target);
				Call call = [&](const su_sink *s, su_outcome *o) { return su_encode(&in, flags, s, o); };
				Result r = allocating(call);
				CHECKF(isOk(r) && r.out.kind == SU_KIND_BYTES && r.bytes == expected,
					"encode of %zu characters (words %d) flags %u: %s, %zu bytes expected", text.size(),
					src.words, flags, show(r).c_str(), expected.size());
				su_outcome m;
				CHECKF(su_measure(&in, SU_OP_ENCODED | flags, &m) == SU_RC_DONE && m.status == SU_OK
					&& m.units == expected.size(), "measure ENCODED flags %u: %zu, not %zu", flags, m.units,
					expected.size());
				if (!full)
					continue;
				size_t at = isUtf16(target) ? 2 * below(4) : below(8);
				Result x = intoSink(call, SU_KIND_BYTES, expected.size(), at);
				CHECKF(isOk(x) && x.bytes == expected, "encode into an exact room at %zu: %s", at, show(x).c_str());
				if (!expected.empty()) {
					Result y = intoSink(call, SU_KIND_BYTES, expected.size() - 1, at);
					CHECKF(isNoRoom(y, expected.size()), "encode into a room one byte short: %s", show(y).c_str());
				}
				Dst d(at + expected.size(), 1);
				int64_t w = su_ffi_encode(src.base(), src.words, src.units, pre, pre + text.size(), flags,
					d.base(), at + expected.size(), at);
				CHECKF(w == int64_t(expected.size()) && std::string(d.at(at), expected.size()) == expected,
					"su_ffi_encode flags %u: %lld", flags, (long long)w);
				if (!expected.empty()) {
					w = su_ffi_encode(src.base(), src.words, src.units, pre, pre + text.size(), flags,
						d.base(), at + expected.size() - 1, at);
					CHECKF(w == statusWord(SU_TOO_LARGE, 0, expected.size()), "su_ffi_encode short: %lld",
						(long long)w);
				}
			}
		}
	}
}

/* words that are not scalar values: 5 or 6 at the word, no allocation */
static void
encodeErrors()
{
	const char32_t bad[] = { 0xD800, 0xDFFF, 0x110000, 0x3FFFFFFF, 0xFFFFFFFF };

	for (char32_t b : bad) {
		int code = b <= 0xDFFF ? SU_E_SURROGATE : SU_E_TOO_LARGE;
		for (size_t n : { 1, 2, 5, 16, 17, 64, 100 }) {
			for (size_t where = 0; where < 3; where++) {
				size_t i = where == 0 ? 0 : where == 1 ? n / 2 : n - 1;
				size_t pre = below(4);
				std::u32string text = randomText(4, n);
				text[i] = b;
				std::u32string all = randomText(0, pre) + text + U"zz";
				Obj src = wordObj(all);
				su_input in = inputOf(src, pre, pre + n);
				for (unsigned target : targets) {
					unsigned flags = target | (chance(50) ? SU_F_BOM : 0);
					Call call = [&](const su_sink *s, su_outcome *o) { return su_encode(&in, flags, s, o); };
					Result r = allocating(call);
					CHECKF(isError(r, code, pre + i) && r.calls == 0, "word %x at %zu of %zu: %s, %d allocations",
						unsigned(b), pre + i, n, show(r).c_str(), r.calls);
					Result x = intoSink(call, SU_KIND_BYTES, 4 * n + 8, 2);
					CHECKF(isError(x, code, pre + i), "into: %s", show(x).c_str());
					Result tiny = intoSink(call, SU_KIND_BYTES, 0, 0);
					CHECKF(isError(tiny, code, pre + i), "validity before the room: %s", show(tiny).c_str());
					su_outcome m;
					CHECK(su_measure(&in, SU_OP_ENCODED | flags, &m) == SU_RC_DONE && m.status == SU_ERROR
						&& m.code == code && m.position0 == pre + i);
					Dst d(4 * n + 8, 1);
					CHECK(su_ffi_encode(src.base(), 1, src.units, pre, pre + n, flags, d.base(), 4 * n + 8, 0)
						== statusWord(SU_ERROR, code, pre + i + 1));
					/* outside the range it is not read */
					su_input before = inputOf(src, pre, pre + i);
					Result ok = allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&before, flags, s, o); });
					CHECKF(isOk(ok), "the range before the bad word: %s", show(ok).c_str());
				}
			}
		}
	}
}

static void
testEncode()
{
	for (int family = 0; family < 5; family++) {
		for (size_t n = 0; n <= 300; n++)
			encodeText(randomText(family, n), true);
		for (size_t n : edges)
			encodeText(randomText(family, n), true);
	}
	for (int i = 0; i < 100000; i++)
		encodeText(randomText(i % 5, randomLength()), i % 5 == 0);
	encodeErrors();
}

/*** U2, U3, U9: decoding ***/

static bool
sameChars(const Result &r, const std::u32string &chars)
{
	return r.out.kind == SU_KIND_UTF32 ? r.chars == chars : r.bytes == latin1Of(chars);
}

/* the encoding of text as [pre, pre + n) of a source; every decode, the
   DECODED measure; full: into mode and the library too */
static void
decodeText(const std::u32string &text, unsigned encoding, bool full)
{
	std::string pre = encodingOf(randomText(0, chance(30) ? below(5) : 0), encoding);
	std::string post = encodingOf(randomText(0, chance(30) ? below(5) : 0), encoding);
	std::string body = encodingOf(text, encoding);
	Obj src = byteObj(pre + body + post);
	su_input in = inputOf(src, pre.size(), pre.size() + body.size());

	for (unsigned bom : { 0u, unsigned(SU_F_BOM) }) {
		std::u32string chars = text;
		size_t skipped = 0;
		if (bom && !text.empty() && text[0] == 0xFEFF) {
			chars.erase(0, 1);
			skipped = encoding == SU_ENC_UTF8 ? 3 : 2;
		}
		size_t k = firstAbove(chars, 0xFF);
		bool wide = k < chars.size();
		size_t firstWide = pre.size() + skipped + offsetOf(chars, k, encoding);
		for (unsigned force : { 0u, unsigned(SU_F_WIDE), unsigned(SU_F_LATIN1) }) {
			unsigned flags = encoding | bom | force;
			Call call = [&](const su_sink *s, su_outcome *o) { return su_decode(&in, flags, s, o); };
			Result r = allocating(call);
			int kind = force == SU_F_WIDE ? SU_KIND_UTF32 : force == SU_F_LATIN1 ? SU_KIND_LATIN1
				: wide ? SU_KIND_UTF32 : SU_KIND_LATIN1;
			if (kind == SU_KIND_LATIN1 && wide) {
				CHECKF(isError(r, SU_E_NOT_LATIN1, firstWide) && r.calls == 0, "decode flags %u of %zu bytes: %s, 1002 at %zu",
					flags, body.size(), show(r).c_str(), firstWide);
			} else {
				CHECKF(isOk(r) && r.out.kind == kind && sameChars(r, chars), "decode flags %u of %zu bytes: %s",
					flags, body.size(), show(r).c_str());
			}
		}
		su_outcome m;
		CHECKF(su_measure(&in, SU_OP_DECODED | encoding | bom, &m) == SU_RC_DONE && m.status == SU_OK
			&& m.units == chars.size() && m.kind == (wide ? SU_KIND_UTF32 : SU_KIND_LATIN1),
			"measure DECODED: %zu kind %d", m.units, m.kind);
		if (!full)
			continue;
		unsigned flags = encoding | bom;
		Call call = [&](const su_sink *s, su_outcome *o) { return su_decode(&in, flags, s, o); };
		size_t at = below(8);
		Result x = intoSink(call, SU_KIND_LATIN1, chars.size(), at);
		if (wide)
			CHECKF(isError(x, SU_E_NOT_LATIN1, firstWide), "into a byte room: %s", show(x).c_str());
		else
			CHECKF(isOk(x) && x.out.kind == SU_KIND_LATIN1 && x.bytes == latin1Of(chars), "into a byte room: %s",
				show(x).c_str());
		Result y = intoSink(call, SU_KIND_UTF32, chars.size(), 4 * below(3));
		CHECKF(isOk(y) && y.out.kind == SU_KIND_UTF32 && y.chars == chars, "into a word room: %s", show(y).c_str());
		if (!chars.empty()) {
			Result z = intoSink(call, SU_KIND_UTF32, chars.size() - 1, 0);
			CHECKF(isNoRoom(z, chars.size()), "into a word room one short: %s", show(z).c_str());
			if (!wide) {
				z = intoSink(call, SU_KIND_LATIN1, chars.size() - 1, at);
				CHECKF(isNoRoom(z, chars.size()), "into a byte room one short: %s", show(z).c_str());
			}
		}
		for (int64_t dstWords = 0; dstWords < 2; dstWords++) {
			size_t at0 = below(3);
			Dst d(at0 + chars.size(), dstWords ? 4 : 1);
			int64_t w = su_ffi_decode(src.base(), src.units, in.start0, in.stop0, flags, d.base(), dstWords,
				at0 + chars.size(), at0);
			if (!dstWords && wide) {
				CHECKF(w == statusWord(SU_ERROR, SU_E_NOT_LATIN1, firstWide + 1), "su_ffi_decode: %lld", (long long)w);
			} else {
				bool same = dstWords ? std::u32string(reinterpret_cast<const char32_t *>(d.at(at0)), chars.size()) == chars
					: std::string(d.at(at0), chars.size()) == latin1Of(chars);
				CHECKF(w == int64_t(chars.size()) && same, "su_ffi_decode words %lld: %lld", (long long)dstWords, (long long)w);
			}
		}
		CHECK(su_ffi_measure(src.base(), 0, src.units, in.start0, in.stop0, SU_OP_DECODED | encoding | bom)
			== int64_t(chars.size()) * 2 + wide);
	}
}

/* Invalid input: the reference's verdict, validate_utf8_with_errors' position
   for UTF-8, no allocation, the same answer from decode, the DECODED measure,
   validate and the library */
static void
decodeInvalid(const std::string &body, unsigned encoding, size_t pre)
{
	std::string prefix(pre, 'a');
	Obj src = byteObj(prefix + body + "zz");
	su_input in = inputOf(src, pre, pre + body.size());
	int code = 0;
	size_t at;

	if (encoding == SU_ENC_UTF8) {
		at = utf8ValidPrefix(body, &code);
		if (code) {
			simdutf::result direct = simdutf::validate_utf8_with_errors(body.data(), body.size());
			CHECKF(int(direct.error) == code && direct.count == at, "simdutf says %d at %zu, the reference %d at %zu",
				int(direct.error), direct.count, code, at);
		}
	} else if (body.size() & 1) {
		code = SU_E_ODD_UTF16_LENGTH;
		at = body.size() - 1;
	} else {
		at = utf16ValidPrefix(body, encoding == SU_ENC_UTF16LE, &code);
	}
	if (!code) {
		std::u32string chars = encoding == SU_ENC_UTF8 ? utf8Decode(body) : utf16Decode(body, encoding == SU_ENC_UTF16LE);
		Result r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&in, encoding | SU_F_WIDE, s, o); });
		CHECKF(isOk(r) && r.chars == chars, "valid input: %s", show(r).c_str());
		return;
	}
	for (unsigned force : { 0u, unsigned(SU_F_WIDE), unsigned(SU_F_LATIN1) }) {
		unsigned flags = encoding | force;
		Call call = [&](const su_sink *s, su_outcome *o) { return su_decode(&in, flags, s, o); };
		Result r = allocating(call);
		CHECKF(isError(r, code, pre + at) && r.calls == 0, "decode flags %u of %zu bytes: %s, expected %d at %zu", flags,
			body.size(), show(r).c_str(), code, pre + at);
		Result x = intoSink(call, force == SU_F_WIDE ? SU_KIND_UTF32 : SU_KIND_LATIN1, body.size() + 1, 0);
		CHECKF(isError(x, code, pre + at), "into: %s", show(x).c_str());
		x = intoSink(call, force == SU_F_WIDE ? SU_KIND_UTF32 : SU_KIND_LATIN1, 0, 0);
		CHECKF(isError(x, code, pre + at), "validity before the room: %s", show(x).c_str());
	}
	su_outcome m;
	CHECK(su_measure(&in, SU_OP_DECODED | encoding, &m) == SU_RC_DONE && m.status == SU_ERROR && m.code == code
		&& m.position0 == pre + at);
	CHECK(su_validate(&in, SU_OP_VALID | encoding, &m) == SU_RC_DONE && m.status == SU_ERROR && m.code == code
		&& m.position0 == pre + at);
	Dst d(body.size() + 1, 4);
	CHECK(su_ffi_decode(src.base(), src.units, pre, pre + body.size(), encoding, d.base(), 1, body.size() + 1, 0)
		== statusWord(SU_ERROR, code, pre + at + 1));
	CHECK(su_ffi_validate(src.base(), 0, src.units, pre, pre + body.size(), encoding)
		== statusWord(SU_ERROR, code, pre + at + 1));
}

/* the high bit of byte b survives iff b >= 0xC4 (suEngine.cpp's scan) */
static void
swarRule()
{
	for (unsigned b = 0; b < 256; b++) {
		for (unsigned k = 0; k < 8; k++) {
			uint64_t w = 0x4141414141414141ull & ~(0xFFull << 8 * k);
			w |= uint64_t(b) << 8 * k;
			bool hit = (((w & 0x7F7F7F7F7F7F7F7Full) + 0x3C3C3C3C3C3C3C3Cull) & w & 0x8080808080808080ull) != 0;
			CHECKF(hit == (b >= 0xC4), "the SWAR rule on %02x in byte %u", b, k);
		}
	}
}

/* every lead byte C2-F4 and every continuation byte 80-BF, at every
   alignment, decide the kind and the position of 1002 */
static void
leadBytes()
{
	std::vector<char32_t> cps;
	for (char32_t c = 0x80; c < 0x800; c++)		/* C2-DF, and 80-BF after them */
		cps.push_back(c);
	for (char32_t lead = 0; lead < 16; lead++)	/* E0-EF */
		for (char32_t low = 0; low < 64; low++)
			cps.push_back(lead == 0xD ? 0xD000 | (low & 0x1F) << 6 | low : std::max<char32_t>(0x800, lead << 12 | 0x800 | low));
	for (char32_t lead = 0; lead < 5; lead++)	/* F0-F4 */
		for (char32_t low = 0; low < 64; low++)
			cps.push_back(std::max<char32_t>(0x10000, lead << 18 | (lead == 4 ? 0 : low << 12) | low << 6 | low));
	for (char32_t c : cps) {
		for (size_t a = 0; a < 16; a++) {
			std::u32string text = randomText(0, a) + c + randomText(0, below(10));
			decodeText(text, SU_ENC_UTF8, false);
		}
	}
}

static void
decodeErrors()
{
	struct Bad {
		const char *bytes;
		int code;
		size_t at;
	} bad[] = {
		{ "\xF8", SU_E_HEADER_BITS, 0 }, { "\xFF", SU_E_HEADER_BITS, 0 }, { "\x80", SU_E_TOO_LONG, 0 },
		{ "\xBF", SU_E_TOO_LONG, 0 }, { "\xC3\xA9\xA9", SU_E_TOO_LONG, 2 }, { "\xE2\x82\xAC\x80", SU_E_TOO_LONG, 3 },
		{ "\xC0\x80", SU_E_OVERLONG, 0 }, { "\xC1\xBF", SU_E_OVERLONG, 0 }, { "\xE0\x80\x80", SU_E_OVERLONG, 0 },
		{ "\xE0\x9F\xBF", SU_E_OVERLONG, 0 }, { "\xF0\x8F\xBF\xBF", SU_E_OVERLONG, 0 },
		{ "\xF4\x90\x80\x80", SU_E_TOO_LARGE, 0 }, { "\xF5\x80\x80\x80", SU_E_TOO_LARGE, 0 },
		{ "\xF7\xBF\xBF\xBF", SU_E_TOO_LARGE, 0 }, { "\xED\xA0\x80", SU_E_SURROGATE, 0 },
		{ "\xED\xBF\xBF", SU_E_SURROGATE, 0 }, { "\xC3", SU_E_TOO_SHORT, 0 }, { "\xE2", SU_E_TOO_SHORT, 0 },
		{ "\xE2\x82", SU_E_TOO_SHORT, 0 }, { "\xF0", SU_E_TOO_SHORT, 0 }, { "\xF0\x9F", SU_E_TOO_SHORT, 0 },
		{ "\xF0\x9F\x98", SU_E_TOO_SHORT, 0 }, { "\xC3\x41", SU_E_TOO_SHORT, 0 }, { "\xE2\x41\xAC", SU_E_TOO_SHORT, 0 },
		{ "\xF0\x9F\x41\x80", SU_E_TOO_SHORT, 0 },
	};
	for (const Bad &b : bad) {
		for (size_t p = 0; p <= 40; p++) {
			for (int suffix = 0; suffix < 2; suffix++) {
				/* a prefix of p bytes of valid text, ASCII or not */
				std::string prefix = chance(50) ? std::string(p, 'x') : "";
				while (prefix.size() < p) {
					std::string c = utf8Of(std::u32string(1, randomChar(4)));
					prefix += c.size() <= p - prefix.size() ? c : std::string(p - prefix.size(), 'y');
				}
				std::string body = prefix + b.bytes + (suffix ? "AB" : "");
				int code;
				size_t at = utf8ValidPrefix(body, &code);
				CHECKF(code == b.code && at == p + b.at, "the reference on case %02x: %d at %zu",
					(unsigned char)b.bytes[0], code, at);
				decodeInvalid(body, SU_ENC_UTF8, below(5));
			}
		}
	}
	/* mutated valid text */
	for (int i = 0; i < 20000; i++) {
		std::string body = utf8Of(randomText(i % 5, randomLength() % 200 + 1));
		for (int k = int(below(3)) + 1; k > 0; k--) {
			size_t j = below(body.size());
			body[j] = char(chance(50) ? rng() : 0x80 + below(0x80));
		}
		decodeInvalid(body, SU_ENC_UTF8, below(5));
	}
	/* UTF-16: lone and swapped surrogates, odd lengths */
	for (unsigned encoding : { unsigned(SU_ENC_UTF16LE), unsigned(SU_ENC_UTF16BE) }) {
		bool le = encoding == SU_ENC_UTF16LE;
		for (int i = 0; i < 20000; i++) {
			std::u32string text = randomText(i % 5, randomLength() % 100 + 1);
			std::string body = utf16Of(text, le);
			size_t units = body.size() / 2;
			int kind = int(below(5));
			if (kind == 0) {
				body.pop_back();		/* odd */
			} else if (kind == 1) {
				size_t j = below(units);
				body[2 * j + (le ? 1 : 0)] = char(0xD8 + below(8));	/* a surrogate, high or low */
			} else if (kind == 2) {
				std::string h;
				putUnit(h, unsigned(0xD800 + below(0x400)), le);
				body += h;			/* a high surrogate last */
			} else if (kind == 3) {
				std::string l;
				putUnit(l, unsigned(0xDC00 + below(0x400)), le);
				body.insert(2 * below(units + 1), l);	/* a low surrogate alone, or after a high one */
			} else {
				std::string h;
				putUnit(h, unsigned(0xD800 + below(0x400)), le);
				putUnit(h, 'A', le);
				body.insert(2 * below(units + 1), h);	/* a high surrogate before a non-surrogate */
			}
			decodeInvalid(body, encoding, 2 * below(3));
		}
	}
	/* validity before 1002 */
	std::string body = utf8Of(U"\u00e9\u20ac") + "\xC0\x80";
	Obj src = byteObj(body);
	su_input in = inputOf(src, 0, body.size());
	Result r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&in, SU_ENC_UTF8 | SU_F_LATIN1, s, o); });
	CHECKF(isError(r, SU_E_OVERLONG, 5), "validity before 1002: %s", show(r).c_str());
	body = utf16Of(U"\u20ac", true) + std::string("\x00\xDC", 2);
	src = byteObj(body);
	in = inputOf(src, 0, body.size());
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&in, SU_ENC_UTF16LE | SU_F_LATIN1, s, o); });
	CHECKF(isError(r, SU_E_SURROGATE, 2), "validity before 1002 (UTF-16): %s", show(r).c_str());
	/* odd UTF-16 is 1001 at stop, before validity */
	body = std::string("\x00\xD8\x41", 3);
	src = byteObj("ab" + body);
	in = inputOf(src, 2, 5);
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&in, SU_ENC_UTF16LE, s, o); });
	CHECKF(isError(r, SU_E_ODD_UTF16_LENGTH, 4), "odd UTF-16: %s", show(r).c_str());
}

static void
testDecode()
{
	swarRule();
	for (unsigned encoding : targets) {
		for (int family = 0; family < 5; family++) {
			for (size_t n = 0; n <= 300; n++)
				decodeText(randomText(family, n), encoding, true);
			for (size_t n : edges)
				decodeText(randomText(family, n), encoding, true);
		}
		/* BOMs: one leading U+FEFF is skipped when asked, the others kept */
		for (int i = 0; i < 300; i++) {
			std::u32string text = U"\uFEFF" + randomText(i % 5, below(40));
			if (chance(30))
				text = U"\uFEFF" + text;
			if (chance(30))
				text.insert(below(text.size() + 1), 1, U'\uFEFF');
			decodeText(text, encoding, true);
		}
	}
	for (int i = 0; i < 200000; i++)
		decodeText(randomText(i % 5, randomLength()), targets[i % 3], i % 5 == 0);
	leadBytes();
	decodeErrors();
}

/*** U3: the base64 measure of any bytes ***/

static void
testMeasure()
{
	static const char pool[] = "ABCDQRSTwxyz0189+/-_=== \t\n\r\f!\x01\x7F\x80\xC3\xFF";
	for (int i = 0; i < 200000; i++) {
		size_t n = below(60);
		std::string s;
		for (size_t k = 0; k < n; k++)
			s += chance(5) ? char(rng()) : pool[below(sizeof pool - 1)];
		if (chance(30))		/* bytes >= 0x80 after the padding */
			s += std::string("==") + char(0x80 + below(0x80)) + (chance(50) ? " " : "");
		size_t pre = below(4);
		Obj src = byteObj(std::string(pre, '=') + s);
		su_input in = inputOf(src, pre, pre + s.size());
		su_outcome m;
		CHECKF(su_measure(&in, SU_OP_BASE64, &m) == SU_RC_DONE && m.status == SU_OK && m.units == portableMeasure(s),
			"measure BASE64 of %zu bytes: %zu, not %zu", s.size(), m.units, portableMeasure(s));
		CHECK(su_ffi_measure(src.base(), 0, src.units, pre, pre + s.size(), SU_OP_BASE64) == int64_t(portableMeasure(s)));
	}
}

/*** U4: validation ***/

static void
validateCase(Obj &src, size_t s, size_t e, unsigned flags, int code, size_t position0)
{
	su_input in = inputOf(src, s, e);
	su_outcome out;
	int rc = su_validate(&in, flags, &out);

	if (code) {
		CHECKF(rc == SU_RC_DONE && out.status == SU_ERROR && out.code == code && out.position0 == position0,
			"validate flags %u words %d: rc %d status %d code %d at %zu, expected %d at %zu", flags, src.words, rc, out.status,
			out.code, out.position0, code, position0);
		CHECK(su_ffi_validate(src.base(), src.words, src.units, s, e, flags) == statusWord(SU_ERROR, code, position0 + 1));
	} else {
		CHECKF(rc == SU_RC_DONE && out.status == SU_OK && out.units == 0, "validate flags %u words %d: rc %d status %d code %d",
			flags, src.words, rc, out.status, out.code);
		CHECK(su_ffi_validate(src.base(), src.words, src.units, s, e, flags) == 0);
	}
}

static void
testValidate()
{
	for (int i = 0; i < 50000; i++) {
		std::u32string text = randomText(i % 5, randomLength() % 400);
		size_t pre = below(4);
		/* UTF-8, sometimes broken */
		std::string body = utf8Of(text);
		if (!body.empty() && chance(50))
			body[below(body.size())] = char(rng());
		Obj src = byteObj(std::string(pre, 'a') + body);
		int code;
		size_t at = utf8ValidPrefix(body, &code);
		validateCase(src, pre, pre + body.size(), SU_OP_VALID | SU_ENC_UTF8, code, pre + at);
		/* ASCII of bytes */
		size_t k = 0;
		while (k < body.size() && (unsigned char)body[k] < 0x80)
			k++;
		validateCase(src, pre, pre + body.size(), SU_OP_ASCII, k < body.size() ? SU_E_TOO_LARGE : 0, pre + k);
		/* UTF-16, sometimes broken, sometimes odd */
		for (unsigned encoding : { unsigned(SU_ENC_UTF16LE), unsigned(SU_ENC_UTF16BE) }) {
			bool le = encoding == SU_ENC_UTF16LE;
			std::string u = utf16Of(text, le);
			if (!u.empty() && chance(30))
				u[below(u.size() / 2) * 2 + (le ? 1 : 0)] = char(0xD8 + below(8));
			if (chance(10))
				u += 'x';
			Obj s16 = byteObj(std::string(2 * pre, 'a') + u);
			if (u.size() & 1) {
				validateCase(s16, 2 * pre, 2 * pre + u.size(), encoding, SU_E_ODD_UTF16_LENGTH, 2 * pre + u.size() - 1);
			} else {
				at = utf16ValidPrefix(u, le, &code);
				validateCase(s16, 2 * pre, 2 * pre + u.size(), encoding, code, 2 * pre + at);
			}
		}
		/* UTF-32 words, sometimes not scalar values; ASCII of words */
		std::u32string w = randomText(0, pre) + text;
		if (!text.empty() && chance(50)) {
			static const char32_t bad[] = { 0xD800, 0xDBFF, 0xDC00, 0xDFFF, 0x110000, 0x3FFFFFFF, 0x80000000, 0xFFFFFFFF };
			w[pre + below(text.size())] = bad[below(8)];
		}
		Obj s32 = wordObj(w);
		size_t j = pre;
		while (j < w.size() && w[j] <= 0x10FFFF && !(w[j] >= 0xD800 && w[j] <= 0xDFFF))
			j++;
		validateCase(s32, pre, w.size(), SU_OP_VALID | SU_ENC_UTF32,
			j == w.size() ? 0 : w[j] > 0x10FFFF ? SU_E_TOO_LARGE : SU_E_SURROGATE, j);
		j = pre;
		while (j < w.size() && w[j] <= 0x7F)
			j++;
		validateCase(s32, pre, w.size(), SU_OP_ASCII, j == w.size() ? 0 : SU_E_TOO_LARGE, j);
	}
}

/*** U5, U6: base64 ***/

/* decode text with options and mode as [pre, pre + n) of a source: the
   oracle is simdutf's base64_to_binary_safe with room for anything */
static void
base64Case(const std::string &text, unsigned options, unsigned mode, bool full)
{
	size_t pre = chance(30) ? below(5) : 0;
	Obj src = byteObj(randomBytes(pre) + text);
	su_input in = inputOf(src, pre, pre + text.size());
	const char *p = src.bytes() + pre;
	std::vector<char> room(text.size() + 8);
	size_t outlen = room.size();
	simdutf::result oracle = simdutf::base64_to_binary_safe(p, text.size(), room.data(), outlen,
		simdutf::base64_options(options), simdutf::last_chunk_handling_options(mode), false);
	size_t measure = portableMeasure(text);
	Call call = [&](const su_sink *s, su_outcome *o) { return su_base64_decode(&in, options, mode, s, o); };
	Result r = allocating(call);

	CHECKF(r.calls == 1 && r.askedUnits == measure && r.askedKind == SU_KIND_BYTES,
		"%d allocations of %zu bytes, the measure is %zu", r.calls, r.askedUnits, measure);
	int code = 0;
	size_t at = 0;
	switch (oracle.error) {
	case simdutf::SUCCESS:
		CHECKF(outlen == measure, "the measure %zu is not the %zu bytes decoded", measure, outlen);
		CHECKF(isOk(r) && r.bytes == std::string(room.data(), outlen), "base64 decode options %u mode %u of %zu: %s",
			options, mode, text.size(), show(r).c_str());
		break;
	case simdutf::INVALID_BASE64_CHARACTER:
		code = SU_E_INVALID_BASE64_CHARACTER;
		at = pre + oracle.count;
		break;
	case simdutf::BASE64_INPUT_REMAINDER:
	case simdutf::BASE64_EXTRA_BITS:
		code = oracle.error;
		at = pre + lastNonSpace(text);
		break;
	default:
		CHECKF(false, "the oracle answers %d", int(oracle.error));
	}
	if (code)
		CHECKF(isError(r, code, at), "base64 decode options %u mode %u of %zu bytes: %s, expected %d at %zu", options, mode,
			text.size(), show(r).c_str(), code, at);
	CHECKF(!(r.rc == SU_RC_DONE && r.out.status == SU_ERROR && r.out.code == SU_E_OTHER),
		"OTHER: the size was not the measure, or the room too small");
	if (!full)
		return;
	Result x = intoSink(call, SU_KIND_BYTES, measure, below(8), true);
	CHECKF(x.rc == r.rc && x.out.status == r.out.status && x.out.code == r.out.code && x.out.position0 == r.out.position0
		&& x.out.units == r.out.units && x.bytes == r.bytes, "into an exact room: %s, not %s", show(x).c_str(), show(r).c_str());
	x = intoSink(call, SU_KIND_BYTES, measure + 3, 1, true);
	CHECKF(x.rc == r.rc && x.out.code == r.out.code && x.bytes == r.bytes, "into a larger room: %s", show(x).c_str());
	if (measure > 0) {
		x = intoSink(call, SU_KIND_BYTES, measure - 1, 3);
		CHECKF(isNoRoom(x, measure), "TOO_LARGE before decoding: %s", show(x).c_str());
	}
	Dst d(measure + 2, 1);
	int64_t w = su_ffi_base64_decode(src.base(), src.units, pre, pre + text.size(), options, mode, d.base(), measure + 2, 2);
	CHECKF(w == primitiveAnswer(r) && (!isOk(r) || std::string(d.at(2), measure) == r.bytes), "su_ffi_base64_decode: %lld",
		(long long)w);
	if (measure > 0)
		CHECK(su_ffi_base64_decode(src.base(), src.units, pre, pre + text.size(), options, mode, d.base(), measure, 1)
			== statusWord(SU_TOO_LARGE, 0, measure));
}

static const size_t lineLengths[] = { 0, 4, 8, 64, 76 };

static void
encodeBase64(const std::string &bytes, unsigned options, size_t lineLength, unsigned lineEnd, bool full)
{
	size_t pre = chance(30) ? below(5) : 0;
	Obj src = byteObj(randomBytes(pre) + bytes + randomBytes(below(3)));
	su_input in = inputOf(src, pre, pre + bytes.size());
	std::string expected = zincBase64(bytes, options, lineLength, lineEnd);
	Call call = [&](const su_sink *s, su_outcome *o) { return su_base64_encode(&in, options, lineLength, lineEnd, s, o); };
	Result r = allocating(call);

	CHECKF(isOk(r) && r.out.kind == SU_KIND_LATIN1 && r.bytes == expected,
		"base64 encode of %zu bytes options %u lines %zu end %u: %s", bytes.size(), options, lineLength, lineEnd,
		show(r).c_str());
	CHECKF(su_base64_encoded_size(bytes.size(), options, lineLength, lineEnd) == expected.size(),
		"su_base64_encoded_size %zu, not %zu", su_base64_encoded_size(bytes.size(), options, lineLength, lineEnd),
		expected.size());
	CHECK(su_ffi_base64_encoded_size(int64_t(bytes.size()), options, lineLength, lineEnd) == int64_t(expected.size()));
	/* the round trip, line ends being white space */
	bool padded = options == 0 || options == 3;
	unsigned decodeOptions = chance(50) ? unsigned(SU_B64_ANY) : options & 1;
	Obj back = byteObj(expected);
	su_input bin = inputOf(back, 0, expected.size());
	Result d = allocating([&](const su_sink *s, su_outcome *o) {
		return su_base64_decode(&bin, decodeOptions, padded ? SU_CHUNK_STRICT : SU_CHUNK_LOOSE, s, o); });
	CHECKF(isOk(d) && d.bytes == bytes, "the round trip of %zu bytes options %u: %s", bytes.size(), options, show(d).c_str());
	su_outcome m;
	CHECK(su_measure(&bin, SU_OP_BASE64, &m) == SU_RC_DONE && m.units == bytes.size());
	if (!full)
		return;
	size_t at = 2 * below(4) + 1;	/* odd */
	Result x = intoSink(call, chance(50) ? SU_KIND_BYTES : SU_KIND_LATIN1, expected.size(), at);
	CHECKF(isOk(x) && x.bytes == expected, "into at %zu: %s", at, show(x).c_str());
	if (!expected.empty()) {
		x = intoSink(call, SU_KIND_LATIN1, expected.size() - 1, at);
		CHECKF(isNoRoom(x, expected.size()), "into a room one short: %s", show(x).c_str());
	}
	Dst dst(at + expected.size(), 1);
	int64_t w = su_ffi_base64_encode(src.base(), src.units, pre, pre + bytes.size(), options, lineLength, lineEnd,
		dst.base(), at + expected.size(), at);
	CHECKF(w == int64_t(expected.size()) && std::string(dst.at(at), expected.size()) == expected,
		"su_ffi_base64_encode: %lld", (long long)w);
	if (!expected.empty())
		CHECK(su_ffi_base64_encode(src.base(), src.units, pre, pre + bytes.size(), options, lineLength, lineEnd,
			dst.base(), at + expected.size() - 1, at) == statusWord(SU_TOO_LARGE, 0, expected.size()));
}

static void
testBase64Encode()
{
	int i = 0;
	for (unsigned options = 0; options < 4; options++) {
		for (size_t lineLength : lineLengths) {
			for (unsigned lineEnd = 0; lineEnd < 3; lineEnd++) {
				for (size_t n = 0; n <= 300; n++)
					encodeBase64(randomBytes(n), options, lineLength, lineEnd, i++ % 7 == 0);
				for (int k = 0; k < 20; k++)
					encodeBase64(randomBytes(below(5000)), options, lineLength, lineEnd, true);
			}
		}
	}
	for (size_t n = 0; n < 2000; n++) {
		for (unsigned options = 0; options < 4; options++) {
			CHECK(su_base64_encoded_size(n, options, 0, 0)
				== simdutf::base64_length_from_binary(n, simdutf::base64_options(options)));
		}
	}
	/* lines of the largest length */
	size_t longest = SU_MAX_LINE_LENGTH - 4;
	CHECK(su_base64_encoded_size(3 * (longest / 4), 0, longest, SU_EOL_CRLF) == longest + 2);
	CHECK(su_base64_encoded_size(3 * (longest / 4) - 1, 0, longest, SU_EOL_CRLF) == longest + 2);
	CHECK(su_base64_encoded_size(3 * (longest / 4) - 1, 2, longest, SU_EOL_CRLF) == longest - 1 + 2);
	CHECK(su_base64_encoded_size(3 * (longest / 4) - 3, 0, longest, SU_EOL_CRLF) == longest - 4);
}

/* white space anywhere, inside the groups too */
static std::string
sprinkle(const std::string &s)
{
	static const char space[] = " \t\n\f\r";
	std::string out;

	for (char c : s) {
		while (chance(10))
			out += space[below(5)];
		out += c;
	}
	while (chance(10))
		out += space[below(5)];
	return out;
}

static std::string
mutate(const std::string &s)
{
	static const char pool[] = "=!-_+/A\x80\xFF\x01\x7F ";
	std::string out = s;

	switch (below(4)) {
	case 0:
		if (!out.empty())
			out[below(out.size())] = pool[below(sizeof pool - 1)];
		break;
	case 1:
		if (!out.empty())
			out.erase(below(out.size()), 1);
		break;
	case 2:
		out += '=';
		break;
	default:
		out.insert(below(out.size() + 1), 1, pool[below(sizeof pool - 1)]);
	}
	return out;
}

static void
testBase64Decode()
{
	static const unsigned decodeOptions[] = { SU_B64_STANDARD, SU_B64_URL, SU_B64_ANY };
	for (int i = 0; i < 30000; i++) {
		std::string bytes = randomBytes(i % 10 == 0 ? below(3000) : below(100));
		unsigned encodeOptions = unsigned(below(4));
		std::string text = zincBase64(bytes, encodeOptions, chance(30) ? lineLengths[below(5)] : 0, unsigned(below(3)));
		if (chance(50))
			text = sprinkle(text);
		if (chance(40))
			text = mutate(text);
		base64Case(text, decodeOptions[i % 3], unsigned(i / 3 % 2), i % 3 == 0);
	}
	/* the codes and their positions */
	struct {
		const char *text;
		unsigned options, mode;
		int code;
		size_t at;
	} cases[] = {
		{ "QUJD", 0, 1, 0, 0 }, { "QUJD!", 0, 0, 7, 4 }, { "QQ==QQ==", 0, 0, 7, SIZE_MAX }, { "Q", 0, 0, 8, 0 },
		{ "QUJDQ \n", 0, 0, 8, 4 }, { "QR==", 0, 1, 9, 3 }, { "QR==  ", 0, 1, 9, 3 }, { "QQ", 0, 1, 8, 1 },
		{ "QQ", 0, 0, 0, 0 }, { "QU JD", 0, 1, 0, 0 }, { "-_-_", 0, 0, 7, 0 }, { "-_-_", 1, 0, 0, 0 },
		{ "+/+/", 1, 0, 7, 0 }, { "-/+_", 8, 0, 0, 0 }, { "QUJD\x80", 0, 0, 7, 4 },
	};
	for (auto &c : cases) {
		std::string text = c.text;
		Obj src = byteObj("==" + text);
		su_input in = inputOf(src, 2, 2 + text.size());
		Result r = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&in, c.options, c.mode, s, o); });
		if (c.code && c.at == SIZE_MAX)
			CHECKF(r.rc == SU_RC_DONE && r.out.status == SU_ERROR && r.out.code == c.code, "'%s': %s", c.text, show(r).c_str());
		else if (c.code)
			CHECKF(isError(r, c.code, 2 + c.at), "'%s' options %u mode %u: %s", c.text, c.options, c.mode, show(r).c_str());
		else
			CHECKF(isOk(r), "'%s' options %u mode %u: %s", c.text, c.options, c.mode, show(r).c_str());
		base64Case(text, c.options, c.mode, true);
	}
	/* non-ASCII input in into mode: TOO_LARGE or 7, as the measure says */
	for (int i = 0; i < 2000; i++) {
		std::string text = zincBase64(randomBytes(below(40)), 0, 0, 0);
		text.insert(below(text.size() + 1), 1, char(0x80 + below(0x80)));
		Obj src = byteObj(text);
		su_input in = inputOf(src, 0, text.size());
		size_t measure = portableMeasure(text), room = below(measure + 2);
		Result x = intoSink([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&in, 0, 0, s, o); },
			SU_KIND_BYTES, room, 0, true);
		if (measure > room)
			CHECKF(isNoRoom(x, measure), "%zu > %zu: %s", measure, room, show(x).c_str());
		else
			CHECKF(x.rc == SU_RC_DONE && x.out.status == SU_ERROR && x.out.code == 7, "%s", show(x).c_str());
	}
	/* 1,000,000 random strings: the size is always the measure */
	static const char pool[] = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
		"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/-_====  \t\n\r\f";
	for (int i = 0; i < 1000000; i++) {
		size_t n = below(41);
		std::string text;
		for (size_t k = 0; k < n; k++)
			text += chance(4) ? char(rng()) : pool[below(sizeof pool - 1)];
		base64Case(text, decodeOptions[i % 3], unsigned(i / 3 % 2), false);
	}
}

/*** U7: sinks, and requests outside the contract ***/

static void
testSinks()
{
	std::u32string wide = U"a\u00e9\u20ac\U0001F600";
	Obj words = wordObj(wide);
	su_input win = inputOf(words, 0, wide.size());
	std::string utf8 = utf8Of(wide);
	Obj bytes = byteObj(utf8);
	su_input bin = inputOf(bytes, 0, utf8.size());
	std::string plain = "hello";
	Obj plainObj = byteObj(plain);
	su_input pin = inputOf(plainObj, 0, plain.size());
	std::string b64 = "aGVsbG8=";
	Obj b64Obj = byteObj(b64);
	su_input b64in = inputOf(b64Obj, 0, b64.size());

	/* a refused allocation: NO_ROOM with the request */
	Result r = allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&win, SU_ENC_UTF8, s, o); }, true);
	CHECKF(isNoRoom(r, utf8.size()) && r.out.kind == SU_KIND_BYTES && r.calls == 1, "%s", show(r).c_str());
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&bin, SU_ENC_UTF8, s, o); }, true);
	CHECKF(isNoRoom(r, 4) && r.out.kind == SU_KIND_UTF32 && r.calls == 1, "%s", show(r).c_str());
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&pin, SU_ENC_UTF8, s, o); }, true);
	CHECKF(isNoRoom(r, 5) && r.out.kind == SU_KIND_LATIN1 && r.calls == 1, "%s", show(r).c_str());
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_encode(&pin, 0, 4, 2, s, o); }, true);
	CHECKF(isNoRoom(r, 12) && r.out.kind == SU_KIND_LATIN1 && r.calls == 1, "%s", show(r).c_str());
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&b64in, 0, 1, s, o); }, true);
	CHECKF(isNoRoom(r, 5) && r.out.kind == SU_KIND_BYTES && r.calls == 1, "%s", show(r).c_str());
	/* empty results are allocated, at 0 units */
	su_input empty = inputOf(plainObj, 2, 2);
	for (unsigned target : targets) {
		r = allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&empty, target, s, o); });
		CHECK(isOk(r) && r.out.units == 0 && r.calls == 1);
		r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&empty, target, s, o); });
		CHECK(isOk(r) && r.out.units == 0 && r.out.kind == SU_KIND_LATIN1 && r.calls == 1);
	}
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_encode(&empty, 0, 76, 2, s, o); });
	CHECK(isOk(r) && r.out.units == 0 && r.calls == 1);
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&empty, 0, 1, s, o); });
	CHECK(isOk(r) && r.out.units == 0 && r.calls == 1);
	Obj bomOnly = byteObj("\xEF\xBB\xBF");
	su_input bomIn = inputOf(bomOnly, 0, 3);
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&bomIn, SU_ENC_UTF8 | SU_F_BOM, s, o); });
	CHECK(isOk(r) && r.out.units == 0);
	r = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&bomIn, SU_ENC_UTF8, s, o); });
	CHECK(isOk(r) && r.chars == U"\uFEFF");

	/* a sink with neither base nor allocate, and no sink */
	su_sink none{nullptr, 0, 0, nullptr, nullptr};
	su_outcome out;
	CHECK(su_encode(&win, SU_ENC_UTF8, &none, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_decode(&bin, SU_ENC_UTF8, &none, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_base64_encode(&pin, 0, 0, 0, &none, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_base64_decode(&b64in, 0, 0, &none, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_encode(&win, SU_ENC_UTF8, nullptr, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_measure(nullptr, SU_OP_BASE64, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_validate(&bin, SU_OP_VALID, nullptr) == SU_RC_BAD_ARGUMENT);
	su_input nullBase = win;
	nullBase.base = nullptr;
	CHECK(su_measure(&nullBase, SU_OP_ENCODED, &out) == SU_RC_BAD_ARGUMENT);
	nullBase.units = nullBase.start0 = nullBase.stop0 = 0;
	CHECK(su_measure(&nullBase, SU_OP_ENCODED, &out) == SU_RC_DONE && out.units == 0);
	su_input twoWords = win;
	twoWords.words = 2;
	CHECK(su_measure(&twoWords, SU_OP_ENCODED, &out) == SU_RC_BAD_ARGUMENT);

	/* requests outside the contract, and their order: flags first, then the range, then the sink */
	auto encodeRc = [&](su_input in, unsigned flags) {
		return allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&in, flags, s, o); }).rc; };
	auto decodeRc = [&](su_input in, unsigned flags) {
		return allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&in, flags, s, o); }).rc; };
	su_input badRange = bin;
	badRange.stop0 = bin.units + 1;
	su_input crossed = bin;
	crossed.start0 = 3;
	crossed.stop0 = 2;
	for (unsigned flags : { 1u, 2u, 5u, 6u, 7u, 16u, 32u, 64u, 3u | 16u })
		CHECKF(encodeRc(win, flags) == SU_RC_BAD_ARGUMENT && encodeRc(badRange, flags) == SU_RC_BAD_ARGUMENT,
			"encode flags %u", flags);
	CHECK(encodeRc(badRange, 0) == SU_RC_BAD_INDEX && encodeRc(crossed, 0) == SU_RC_BAD_INDEX);
	CHECK(decodeRc(win, SU_ENC_UTF8) == SU_RC_BAD_ARGUMENT);			/* a word source */
	for (unsigned flags : { 1u, 2u, 5u, 64u, 128u, unsigned(SU_F_WIDE | SU_F_LATIN1) })
		CHECKF(decodeRc(bin, flags) == SU_RC_BAD_ARGUMENT && decodeRc(badRange, flags) == SU_RC_BAD_ARGUMENT,
			"decode flags %u", flags);
	CHECK(decodeRc(badRange, SU_ENC_UTF8) == SU_RC_BAD_INDEX);
	su_input oddStart = bin;
	oddStart.start0 = 1;
	CHECK(decodeRc(oddStart, SU_ENC_UTF16LE) == SU_RC_BAD_INDEX && decodeRc(oddStart, SU_ENC_UTF8) == SU_RC_DONE);
	/* into-sinks: the kinds of the destination */
	alignas(8) char room[64];
	auto intoRc = [&](int (*f)(const su_input *, unsigned, const su_sink *, su_outcome *), su_input in, unsigned flags,
			int kind, size_t at) {
		su_sink s{room + at, 8, kind, nullptr, nullptr};
		return f(&in, flags, &s, &out);
	};
	CHECK(intoRc(su_decode, bin, SU_ENC_UTF8 | SU_F_WIDE, SU_KIND_LATIN1, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(intoRc(su_decode, bin, SU_ENC_UTF8 | SU_F_WIDE, SU_KIND_BYTES, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(intoRc(su_decode, bin, SU_ENC_UTF8 | SU_F_LATIN1, SU_KIND_UTF32, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(intoRc(su_decode, bin, SU_ENC_UTF8, SU_KIND_UTF32, 2) == SU_RC_BAD_ARGUMENT);	/* words off their boundary */
	CHECK(intoRc(su_decode, bin, SU_ENC_UTF8, 7, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(intoRc(su_decode, badRange, SU_ENC_UTF8, 7, 0) == SU_RC_BAD_INDEX);		/* the range before the sink */
	CHECK(intoRc(su_encode, win, SU_ENC_UTF8, SU_KIND_UTF32, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(intoRc(su_encode, win, SU_ENC_UTF16LE, SU_KIND_BYTES, 1) == SU_RC_BAD_INDEX);	/* UTF-16 at an odd byte */
	CHECK(intoRc(su_encode, win, SU_ENC_UTF8, SU_KIND_BYTES, 1) == SU_RC_NO_ROOM);
	/* measure */
	for (unsigned flags : { 1u, 2u, 16u, 32u, 64u | 16u, 64u | 32u, 64u | 1u, 129u, 128u | 8u, 192u })
		CHECKF(su_measure(&bin, flags, &out) == SU_RC_BAD_ARGUMENT && su_measure(&badRange, flags, &out) == SU_RC_BAD_ARGUMENT,
			"measure flags %u", flags);
	CHECK(su_measure(&win, SU_OP_DECODED, &out) == SU_RC_BAD_ARGUMENT && su_measure(&win, SU_OP_BASE64, &out) == SU_RC_BAD_ARGUMENT);
	CHECK(su_measure(&win, SU_OP_ENCODED, &out) == SU_RC_DONE);
	CHECK(su_measure(&badRange, SU_OP_BASE64, &out) == SU_RC_BAD_INDEX);
	CHECK(su_measure(&oddStart, SU_OP_DECODED | SU_ENC_UTF16BE, &out) == SU_RC_BAD_INDEX);
	/* validate */
	for (unsigned flags : { 1u, 2u, 8u, 5u, 65u, 72u, 128u, 192u })
		CHECKF(su_validate(&bin, flags, &out) == SU_RC_BAD_ARGUMENT && su_validate(&badRange, flags, &out) == SU_RC_BAD_ARGUMENT,
			"validate flags %u", flags);
	for (unsigned flags : { 0u, 1u, 3u, 4u })
		CHECKF(su_validate(&win, flags, &out) == SU_RC_BAD_ARGUMENT, "validate words flags %u", flags);
	CHECK(su_validate(&win, SU_ENC_UTF32, &out) == SU_RC_DONE && su_validate(&win, SU_OP_ASCII, &out) == SU_RC_DONE);
	CHECK(su_validate(&oddStart, SU_ENC_UTF16LE, &out) == SU_RC_BAD_INDEX && su_validate(&badRange, SU_OP_ASCII, &out) == SU_RC_BAD_INDEX);
	/* base64 */
	auto encode64Rc = [&](su_input in, unsigned options, size_t lineLength, unsigned lineEnd) {
		return allocating([&](const su_sink *s, su_outcome *o) { return su_base64_encode(&in, options, lineLength, lineEnd, s, o); }).rc; };
	CHECK(encode64Rc(win, 0, 0, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(encode64Rc(pin, 4, 0, 0) == SU_RC_BAD_ARGUMENT && encode64Rc(pin, 8, 0, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(encode64Rc(pin, 0, 6, 0) == SU_RC_BAD_ARGUMENT && encode64Rc(pin, 0, 2, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(encode64Rc(pin, 0, SU_MAX_LINE_LENGTH, 0) == SU_RC_BAD_ARGUMENT);
	CHECK(encode64Rc(pin, 0, SU_MAX_LINE_LENGTH - 4, 0) == SU_RC_DONE);
	CHECK(encode64Rc(pin, 0, 0, 3) == SU_RC_BAD_ARGUMENT && encode64Rc(badRange, 0, 0, 3) == SU_RC_BAD_ARGUMENT);
	CHECK(encode64Rc(badRange, 0, 0, 0) == SU_RC_BAD_INDEX);
	auto decode64Rc = [&](su_input in, unsigned options, unsigned mode) {
		return allocating([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&in, options, mode, s, o); }).rc; };
	CHECK(decode64Rc(win, 0, 0) == SU_RC_BAD_ARGUMENT);
	for (unsigned options : { 2u, 3u, 4u, 5u, 9u, 12u })
		CHECKF(decode64Rc(b64in, options, 0) == SU_RC_BAD_ARGUMENT, "base64 decode options %u", options);
	CHECK(decode64Rc(b64in, 0, 2) == SU_RC_BAD_ARGUMENT && decode64Rc(b64in, 0, 3) == SU_RC_BAD_ARGUMENT);
	CHECK(decode64Rc(badRange, 0, 2) == SU_RC_BAD_ARGUMENT && decode64Rc(badRange, 0, 0) == SU_RC_BAD_INDEX);
	CHECK(su_base64_encoded_size(SIZE_MAX, 0, 0, 0) == SIZE_MAX);
	CHECK(su_base64_encoded_size(SIZE_MAX / 4 * 3, 0, 0, 0) == SIZE_MAX / 4 * 4);
	CHECK(su_base64_encoded_size(SIZE_MAX / 4 * 3 + 1, 0, 0, 0) == SIZE_MAX);
	CHECK(su_base64_encoded_size(SIZE_MAX / 4 * 3, 0, 76, 0) == SIZE_MAX);
	CHECK(su_base64_encoded_size(3, 4, 0, 0) == SIZE_MAX && su_base64_encoded_size(3, 0, 5, 0) == SIZE_MAX);
	CHECK(su_base64_encoded_size(3, 0, 4, 3) == SIZE_MAX);
}

/*** U9: the library ***/

static void
testFFI()
{
	su_info info;
	su_get_info(&info);
	CHECK(su_ffi_protocol() == SU_FFI_PROTOCOL);
	CHECK(!strcmp(su_ffi_simdutf_version(), "9.4.0") && !strcmp(info.simdutf, "9.4.0"));
	CHECK(!strcmp(su_ffi_implementation(), info.implementation) && !strcmp(su_ffi_forced(), info.forced));
	CHECK(std::string(su_ffi_implementation()) == std::string(simdutf::get_active_implementation()->name()));
	CHECK(su_ffi_simdutf_version() == su_ffi_simdutf_version());	/* the same storage on every call */

	std::u32string text = U"h\u00e9llo \u20ac\U0001F600";
	Obj words = wordObj(text);
	std::string latin = "h\xe9llo";
	Obj bytes = byteObj(latin);
	std::string utf8 = utf8Of(text);
	Obj u8 = byteObj(utf8);
	Dst d(64, 4);
	void *dst = d.base();

	/* NULL addresses: before anything else */
	CHECK(su_ffi_encode(nullptr, 0, 1, 0, 1, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_encode(nullptr, 0, 0, 0, 0, 0, dst, 64, 0) == 0);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, 0, nullptr, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 99, 99, nullptr, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(nullptr, 3, 0, 3, 0, dst, 0, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, 0, nullptr, 0, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_measure(nullptr, 0, 1, 0, 1, SU_OP_BASE64) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_measure(nullptr, 0, 0, 0, 0, SU_OP_BASE64) == 0);
	CHECK(su_ffi_validate(nullptr, 0, 1, 0, 1, SU_OP_ASCII) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(nullptr, 1, 0, 1, 0, 0, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, 0, 0, nullptr, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(nullptr, 1, 0, 1, 0, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, 0, nullptr, 0, 0) == SU_FFI_BAD_ARGUMENT);
	/* the order of the primitive: the source unit, the flags, the range, the destination */
	CHECK(su_ffi_encode(bytes.base(), 2, 6, 0, 99, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 99, 1, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 99, 0, bytes.base(), 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, -1, 0, 0, 0, dst, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, -1, 6, 0, dst, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 4, 3, 0, dst, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, 0, bytes.base(), 64, 70) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, 0, dst, 64, 65) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, 0, dst, -1, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, 0, dst, 64, -1) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, SU_ENC_UTF16LE, dst, 64, 1) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, SU_ENC_UTF16LE, dst, 64, 2) == 12);
	CHECK(su_ffi_encode(bytes.base(), 0, 6, 0, 6, SU_ENC_UTF8, dst, 64, 1) == 7);
	CHECK(su_ffi_encode(words.base(), 1, words.units, 0, words.units, SU_ENC_UTF8 | SU_F_BOM, dst, 64, 0)
		== int64_t(utf8.size() + 3));
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8 | SU_F_WIDE | SU_F_LATIN1, dst, 2, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_LATIN1, dst, 1, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 1, u8.units, SU_ENC_UTF16LE, dst, 1, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_decode(u8.base(), u8.units, 1, 99, SU_ENC_UTF8, dst, 2, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, dst, 2, 64, 99) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8 | SU_F_WIDE, dst, 0, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8 | SU_F_LATIN1, dst, 1, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, u8.base(), 1, 64, 99) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, dst, 1, 64, 65) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, dst, 1, 64, 0) == int64_t(text.size()));
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, dst, 1, 2, 0)
		== statusWord(SU_TOO_LARGE, 0, text.size()));
	CHECK(su_ffi_decode(u8.base(), u8.units, 0, u8.units, SU_ENC_UTF8, dst, 0, 64, 0)
		== statusWord(SU_ERROR, SU_E_NOT_LATIN1, 8));
	CHECK(su_ffi_measure(words.base(), 2, words.units, 0, 1, SU_OP_ENCODED) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_measure(words.base(), 1, words.units, 0, 99, SU_OP_DECODED) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_measure(words.base(), 1, words.units, 0, 99, SU_OP_ENCODED) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 0, u8.units, SU_OP_DECODED | SU_F_WIDE) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 1, 3, SU_OP_DECODED | SU_ENC_UTF16BE) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 0, u8.units, SU_OP_DECODED) == int64_t(text.size() * 2 + 1));
	CHECK(su_ffi_measure(bytes.base(), 0, bytes.units, 0, bytes.units, SU_OP_DECODED)
		== statusWord(SU_ERROR, SU_E_TOO_SHORT, 2));	/* "h\xe9llo" is not UTF-8 */
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 0, 2, SU_OP_DECODED) == statusWord(SU_ERROR, SU_E_TOO_SHORT, 2));
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 0, 3, SU_OP_DECODED) == 2 * 2 + 0);
	CHECK(su_ffi_measure(u8.base(), 0, u8.units, 0, u8.units, 3 << 6) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_validate(words.base(), 1, words.units, 0, words.units, SU_ENC_UTF8) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_validate(u8.base(), 0, u8.units, 0, u8.units, SU_ENC_UTF32) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_validate(u8.base(), 0, u8.units, 0, u8.units, SU_ENC_LATIN1) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_validate(u8.base(), 0, u8.units, 1, u8.units, SU_ENC_UTF16LE) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_validate(u8.base(), 0, u8.units, 1, u8.units, SU_OP_ASCII) == statusWord(SU_ERROR, SU_E_TOO_LARGE, 2));
	CHECK(su_ffi_validate(words.base(), 1, words.units, 0, words.units, SU_OP_ASCII) == statusWord(SU_ERROR, SU_E_TOO_LARGE, 2));
	CHECK(su_ffi_validate(words.base(), 1, words.units, 0, words.units, SU_ENC_UTF32) == 0);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 4, 0, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, -1, 0, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, -4, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, 0, 3, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 7, 0, 0, 0, dst, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, 0, 0, bytes.base(), 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, 0, 0, dst, 7, 0) == statusWord(SU_TOO_LARGE, 0, 8));
	CHECK(su_ffi_base64_encode(bytes.base(), 6, 0, 6, 0, 4, 2, dst, 64, 3) == 12);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 2, 0, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, 2, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, -1, dst, 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 6, 5, 0, 0, dst, 64, 0) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, 0, bytes.base(), 64, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, 0, dst, 64, 65) == SU_FFI_BAD_INDEX);
	CHECK(su_ffi_base64_decode(bytes.base(), 6, 0, 6, 0, 0, dst, 64, 0) == statusWord(SU_ERROR, 7, 2));
	/* su_ffi_base64_encoded_size at its limits: the largest result is INT64_MAX - 3 */
	const int64_t most = INT64_MAX / 4 * 3;
	CHECK(su_ffi_base64_encoded_size(-1, 0, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(INT64_MAX, 0, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(most, 0, 0, 0) == INT64_MAX / 4 * 4);
	CHECK(su_ffi_base64_encoded_size(most + 1, 0, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(most + 1, 2, 0, 0) == INT64_MAX / 4 * 4 + 2);
	CHECK(su_ffi_base64_encoded_size(most, 0, 76, 2) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(5, 4, 0, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(5, 0, 3, 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(5, 0, int64_t(SU_MAX_LINE_LENGTH), 0) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(5, 0, 0, -1) == SU_FFI_BAD_ARGUMENT);
	CHECK(su_ffi_base64_encoded_size(57, 0, 76, 2) == 78);
	CHECK(su_ffi_base64_encoded_size(0, 0, 76, 2) == 0);
	/* words of a status: an error at a large position */
	std::string big(70000, 'a');
	big += "\xFF";
	Obj bigObj = byteObj(big);
	CHECK(su_ffi_validate(bigObj.base(), 0, bigObj.units, 0, bigObj.units, SU_ENC_UTF8)
		== -(2 + 4 * SU_E_HEADER_BITS + 4096 * int64_t(70001)));
}

/*** U8: the guard ***/

static bool
testGuard(const std::string &expectKernel, const std::string &expectForced)
{
	const auto &list = simdutf::get_available_implementations();
	std::string best(list.detect_best_supported()->name());
	std::string kernel = expectKernel == "best" ? best : expectKernel;
	su_info info;

	su_get_info(&info);
	printf("U8: kernel %s, forced '%s', best %s, runtime-supported:", info.implementation, info.forced, best.c_str());
	bool listed = false;
	for (int i = 0; i < info.count; i++) {
		printf(" %s", info.implementations[i]);
		listed |= kernel == info.implementations[i];
	}
	printf("\n");
	CHECKF(kernel == info.implementation, "the kernel is %s, not %s", info.implementation, kernel.c_str());
	CHECKF(kernel == std::string(simdutf::get_active_implementation()->name()), "simdutf's active kernel is %s",
		std::string(simdutf::get_active_implementation()->name()).c_str());
	CHECKF(expectForced == info.forced && expectForced == su_ffi_forced(), "forced is '%s', not '%s'", info.forced,
		expectForced.c_str());
	CHECK(listed && info.ready && su_init() && su_ffi_protocol() == SU_FFI_PROTOCOL);
	CHECK(!strcmp(su_ffi_implementation(), info.implementation));
	/* the kernel works */
	for (int i = 0; i < 2000; i++) {
		encodeText(randomText(i % 5, below(200)), i % 10 == 0);
		decodeText(randomText(i % 5, below(200)), targets[i % 3], i % 10 == 0);
	}
	return true;
}

/*** --slow: 256 MiB inputs ***/

static void
fastFill(char *p, size_t n, uint64_t seed, uint64_t asciiMask)
{
	uint64_t x = seed;

	for (size_t i = 0; i + 8 <= n; i += 8) {
		x ^= x << 13;
		x ^= x >> 7;
		x ^= x << 17;
		uint64_t w = x & asciiMask;
		memcpy(p + i, &w, 8);
	}
	for (size_t i = n & ~size_t(7); i < n; i++)
		p[i] = 'x';
}

static void
testSlow()
{
	const size_t N = size_t(256) << 20;
	int64_t start = nowNanos();
	{
		/* Latin-1: bytes with the high bit about 1 in 8 */
		Obj src;
		src.mem.assign(N / 8 + 1, 0);
		src.units = N;
		fastFill(src.bytes(), N, 0x9E3779B97F4A7C15ull, 0xFF7F7F7F7F7F7F7Full);
		su_input in = inputOf(src, 0, N);
		for (unsigned target : targets) {
			Result e = allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&in, target, s, o); });
			CHECKF(isOk(e), "slow encode %u: %s", target, show(e).c_str());
			Obj enc = byteObj(e.bytes);
			e = Result();
			su_input ein = inputOf(enc, 0, enc.units);
			su_outcome v;
			CHECK(su_validate(&ein, target, &v) == SU_RC_DONE && v.status == SU_OK);
			Result d = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&ein, target, s, o); });
			CHECKF(isOk(d) && d.out.kind == SU_KIND_LATIN1 && d.out.units == N && !memcmp(d.bytes.data(), src.bytes(), N),
				"slow Latin-1 round trip %u: %s", target, show(d).c_str());
		}
		su_outcome a;
		CHECK(su_validate(&in, SU_OP_ASCII, &a) == SU_RC_DONE && a.status == SU_ERROR);
		/* base64 of the bytes, in lines of 76 ended by CRLF */
		Result b = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_encode(&in, 0, 76, SU_EOL_CRLF, s, o); });
		CHECK(isOk(b) && b.out.units == su_base64_encoded_size(N, 0, 76, SU_EOL_CRLF));
		Obj text = byteObj(b.bytes);
		b = Result();
		su_input tin = inputOf(text, 0, text.units);
		Result d = allocating([&](const su_sink *s, su_outcome *o) { return su_base64_decode(&tin, 0, 1, s, o); });
		CHECKF(isOk(d) && d.out.units == N && !memcmp(d.bytes.data(), src.bytes(), N), "slow base64 round trip: %s",
			show(d).c_str());
	}
	{
		/* UTF-32: 64 Mi words of the mixed family */
		size_t n = N / 4;
		Obj src;
		src.mem.assign(n / 2 + 1, 0);
		src.units = n;
		src.words = 1;
		char32_t *w = reinterpret_cast<char32_t *>(src.mem.data());
		for (size_t i = 0; i < n; i++)
			w[i] = i % 4096 == 0 ? randomChar(4) : char32_t(0x20 + i % 0x5F) + (i % 7 == 0 ? 0x4E00 : 0);
		su_input in = inputOf(src, 0, n);
		for (unsigned target : targets) {
			Result e = allocating([&](const su_sink *s, su_outcome *o) { return su_encode(&in, target, s, o); });
			CHECKF(isOk(e), "slow wide encode %u: %s", target, show(e).c_str());
			Obj enc = byteObj(e.bytes);
			e = Result();
			su_input ein = inputOf(enc, 0, enc.units);
			Result d = allocating([&](const su_sink *s, su_outcome *o) { return su_decode(&ein, target, s, o); });
			CHECKF(isOk(d) && d.out.kind == SU_KIND_UTF32 && d.out.units == n && !memcmp(d.chars.data(), w, N),
				"slow wide round trip %u: %s", target, show(d).c_str());
		}
		su_outcome v;
		CHECK(su_validate(&in, SU_ENC_UTF32, &v) == SU_RC_DONE && v.status == SU_OK);
	}
	printf("slow: 256 MiB round trips in %.1f s\n", (nowNanos() - start) / 1e9);
}

/*** main ***/

static bool
selected(const std::string &only, const char *name)
{
	if (only.empty())
		return true;
	std::string list = "," + only + ",";
	return list.find("," + std::string(name) + ",") != std::string::npos;
}

int
main(int argc, char **argv)
{
	std::string only, expectKernel, expectForced;
	bool slow = false, kernels = false, guard = false;

	/* first: the engine chooses the kernel before anything calls simdutf,
	   whose own choice would read SIMDUTF_FORCE_IMPLEMENTATION unguarded */
	su_init();
	for (int i = 1; i < argc; i++) {
		if (!strcmp(argv[i], "--only") && i + 1 < argc) only = argv[++i];
		else if (!strcmp(argv[i], "--slow")) slow = true;
		else if (!strcmp(argv[i], "--kernels")) kernels = true;
		else if (!strcmp(argv[i], "--expect-kernel") && i + 1 < argc) expectKernel = argv[++i], guard = true;
		else if (!strcmp(argv[i], "--expect-forced") && i + 1 < argc) expectForced = argv[++i];
		else {
			fprintf(stderr, "usage: sutest [--only U1,U2,...] [--slow] [--kernels] [--expect-kernel K --expect-forced F]\n");
			return 2;
		}
	}
	if (kernels) {
		su_info info;
		su_get_info(&info);
		printf("best %s\nsupported", std::string(simdutf::get_available_implementations().detect_best_supported()->name()).c_str());
		for (int i = 0; i < info.count; i++)
			printf(" %s", info.implementations[i]);
		printf("\n");
		return 0;
	}
	printf("sutest: simdutf %s, kernel %s, forced '%s'\n", su_ffi_simdutf_version(), su_ffi_implementation(), su_ffi_forced());
	if (guard) {
		currentTest = "U8";
		testGuard(expectKernel, expectForced);
		printf("U8 %s: %d checks, %d failures\n", failures ? "FAILED" : "ok", checks, failures);
		return failures != 0;
	}
	struct {
		const char *name;
		void (*run)();
	} tests[] = { { "U1", testEncode }, { "U2", testDecode }, { "U3", testMeasure }, { "U4", testValidate },
		{ "U5", testBase64Encode }, { "U6", testBase64Decode }, { "U7", testSinks }, { "U9", testFFI } };
	for (auto &t : tests) {
		if (!selected(only, t.name))
			continue;
		currentTest = t.name;
		int before = failures, checksBefore = checks;
		int64_t start = nowNanos();
		t.run();
		printf("%s %s: %d checks, %d failures, %.1f s\n", t.name, failures == before ? "ok" : "FAILED", checks - checksBefore,
			failures - before, (nowNanos() - start) / 1e9);
	}
	if (slow) {
		currentTest = "slow";
		int before = failures, checksBefore = checks;
		testSlow();
		printf("slow %s: %d checks, %d failures\n", failures == before ? "ok" : "FAILED", checks - checksBefore,
			failures - before);
	}
	printf("sutest: %d checks, %d failures\n", checks, failures);
	return failures != 0;
}
