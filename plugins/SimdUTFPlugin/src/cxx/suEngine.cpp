/* suEngine.cpp -- the C++17 engine of the SimdUTFPlugin and of libSimdUTFFFI
 * (suEngine.h has the interface, docs/SimdUTF.md the contract).
 *
 * It is the only file that includes simdutf.h, and it includes no VM header.
 * It never allocates: no new, no malloc, no std::vector or std::string, no
 * throw (it is compiled with -fno-exceptions).  Every result goes into a sink
 * of the caller: the room of a destination, or one allocation at the exact
 * size, made after every validation that can fail.  Base64 decoding validates
 * while it decodes, so there the allocation comes first; its size is exact
 * for every input that decodes.
 *
 * Each function checks its request again, although the glue and the library
 * check it first, in the order of the primitives: flags and options with the
 * kind of the source (SU_RC_BAD_ARGUMENT), the range and the UTF-16 alignment
 * of the source (SU_RC_BAD_INDEX), then the sink.  A request outside the
 * contract is refused, never converted wrongly.
 *
 * Positions in an outcome are absolute: units of the source object, not of
 * the range.
 */

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string_view>

#include "simdutf.h"
#include "suEngine.h"

/* The codes of an outcome are simdutf's, plus 1001 and 1002 */
static_assert(int(simdutf::SUCCESS) == 0, "simdutf::error_code");
static_assert(int(simdutf::HEADER_BITS) == SU_E_HEADER_BITS, "simdutf::error_code");
static_assert(int(simdutf::TOO_SHORT) == SU_E_TOO_SHORT, "simdutf::error_code");
static_assert(int(simdutf::TOO_LONG) == SU_E_TOO_LONG, "simdutf::error_code");
static_assert(int(simdutf::OVERLONG) == SU_E_OVERLONG, "simdutf::error_code");
static_assert(int(simdutf::TOO_LARGE) == SU_E_TOO_LARGE, "simdutf::error_code");
static_assert(int(simdutf::SURROGATE) == SU_E_SURROGATE, "simdutf::error_code");
static_assert(int(simdutf::INVALID_BASE64_CHARACTER) == SU_E_INVALID_BASE64_CHARACTER, "simdutf::error_code");
static_assert(int(simdutf::BASE64_INPUT_REMAINDER) == SU_E_BASE64_INPUT_REMAINDER, "simdutf::error_code");
static_assert(int(simdutf::BASE64_EXTRA_BITS) == SU_E_BASE64_EXTRA_BITS, "simdutf::error_code");
static_assert(int(simdutf::OUTPUT_BUFFER_TOO_SMALL) == 10, "simdutf::error_code");
static_assert(int(simdutf::OTHER) == SU_E_OTHER, "simdutf::error_code");
/* The base64 options are simdutf's */
static_assert(int(simdutf::base64_default) == SU_B64_STANDARD && int(simdutf::base64_url) == SU_B64_URL
	&& int(simdutf::base64_default_no_padding) == SU_B64_STANDARD_NO_PADDING
	&& int(simdutf::base64_url_with_padding) == SU_B64_URL_PADDED
	&& int(simdutf::base64_default_or_url) == SU_B64_ANY, "simdutf::base64_options");
static_assert(int(simdutf::loose) == SU_CHUNK_LOOSE && int(simdutf::strict) == SU_CHUNK_STRICT,
	"simdutf::last_chunk_handling_options");
static_assert(sizeof(char16_t) == 2 && sizeof(char32_t) == 4, "UTF-16 and UTF-32 units");

namespace {

/* "", the honoured name, or "refused <value>": 63 bytes at most (su_info.forced), so of a longer value the first 55 */
char forcedNote[64];
bool ready;

/* The SIMDUTF_FORCE_IMPLEMENTATION guard: the variable is honoured only when
 * it names a kernel that is compiled in, that the CPU supports and that is
 * not "unsupported"; otherwise the best kernel is kept and the refusal is
 * reported.  The choice is assigned before any first use of simdutf, so
 * simdutf's own getenv (in the first-use proxy) never runs, and no kernel
 * code runs while choosing: a kernel the CPU lacks cannot raise SIGILL. */
bool
choose()
{
	const auto &list = simdutf::get_available_implementations();
	const simdutf::implementation *chosen = list.detect_best_supported();	/* cpuid only, no kernel code */
	const char *forced = std::getenv("SIMDUTF_FORCE_IMPLEMENTATION");
	if (forced && *forced) {
		const simdutf::implementation *f = list[forced];
		if (f && f->name() != "unsupported" && f->supported_by_runtime_system()) {
			chosen = f;
			std::snprintf(forcedNote, sizeof forcedNote, "%s", forced);
		} else {
			std::snprintf(forcedNote, sizeof forcedNote, "refused %s", forced);
		}
	}
	simdutf::get_active_implementation() = chosen;	/* before any first use: set_best() never runs */
	ready = chosen->name() != "unsupported";
	return true;
}

/* A string_view into a fixed buffer, cut to fit and always terminated */
void
copyName(char *to, size_t size, std::string_view name)
{
	size_t n = name.size() < size - 1 ? name.size() : size - 1;
	std::memcpy(to, name.data(), n);
	to[n] = 0;
}

/*** Answers ***/

int
answerError(su_outcome *out, int code, size_t position0)
{
	out->status = SU_ERROR;
	out->code = code;
	out->position0 = position0;
	return SU_RC_DONE;
}

int
answerOk(su_outcome *out, int kind, size_t units)
{
	out->status = SU_OK;
	out->kind = kind;
	out->units = units;
	return SU_RC_DONE;
}

/* simdutf's code 10 (OUTPUT_BUFFER_TOO_SMALL) cannot happen, since rooms are
   compared before converting: it and anything unknown are OTHER */
int
codeOf(simdutf::error_code e)
{
	return e >= simdutf::HEADER_BITS && e <= simdutf::BASE64_EXTRA_BITS ? int(e) : SU_E_OTHER;
}

/*** Requests ***/

bool
isAligned(const void *p, size_t to)
{
	return (reinterpret_cast<uintptr_t>(p) & (to - 1)) == 0;
}

/* A source that cannot be read: NULL with units, an unknown unit, words not
   on a 4-byte boundary */
bool
badInput(const su_input *in)
{
	return !in || (!in->base && in->units > 0) || (in->words != 0 && in->words != 1)
		|| (in->words && !isAligned(in->base, 4));
}

bool
badRange(const su_input *in)
{
	return in->start0 > in->stop0 || in->stop0 > in->units;
}

bool
badSink(const su_sink *sink)
{
	return !sink || (!sink->base && !sink->allocate);
}

/* An into-sink of a byte object (a ByteArray or a ByteString) */
bool
byteSink(const su_sink *sink)
{
	return sink->kind == SU_KIND_BYTES || sink->kind == SU_KIND_LATIN1;
}

/* The encodings of transcoding: UTF-8, UTF-16LE, UTF-16BE */
bool
isUnicode(unsigned encoding)
{
	return encoding == SU_ENC_UTF8 || encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

bool
isUtf16(unsigned encoding)
{
	return encoding == SU_ENC_UTF16LE || encoding == SU_ENC_UTF16BE;
}

/* Where a result of units units of kind goes: the room of the sink, or its
   one allocation.  NULL when it does not fit or the allocation failed: the
   request is then in out->units and out->kind. */
void *
place(const su_sink *sink, int kind, size_t units, su_outcome *out)
{
	void *at;

	if (sink->base) {
		at = units <= sink->capacity ? sink->base : nullptr;
	} else {
		at = sink->allocate(sink->ctx, kind, units);
	}
	if (!at) {
		out->units = units;
		out->kind = kind;
	}
	return at;
}

/*** Scans that simdutf does not have ***/

uint64_t
load64(const char *p)
{
	uint64_t w;
	std::memcpy(&w, p, 8);
	return w;
}

/* The offset of the first byte >= 0xC4 in p[0, n), or n.  In valid UTF-8 the
   characters above U+00FF are exactly those whose first byte is >= 0xC4.
   Per 8-byte word: the high bit of byte b survives iff b >= 0x80 and
   (b & 0x7F) + 0x3C >= 0x80, i.e. b >= 0xC4; the sums never carry into the
   next byte (0x7F + 0x3C < 0x100). */
size_t
firstAboveLatin1(const char *p, size_t n)
{
	size_t i = 0;

	for (; i + 8 <= n; i += 8) {
		uint64_t w = load64(p + i);
		if (((w & 0x7F7F7F7F7F7F7F7Full) + 0x3C3C3C3C3C3C3C3Cull) & w & 0x8080808080808080ull) {
			break;
		}
	}
	for (; i < n; i++) {
		if ((unsigned char)p[i] >= 0xC4) {
			return i;
		}
	}
	return n;
}

/* The byte offset of the first UTF-16 unit above 0xFF in p[0, n) (n even),
   or n: the first unit whose high byte (the second byte of LE, the first of
   BE) is not 0.  The mask is made from bytes, so it holds on any host. */
size_t
firstAboveLatin1Unit(const char *p, size_t n, bool le)
{
	static const unsigned char leHigh[8] = { 0, 0xFF, 0, 0xFF, 0, 0xFF, 0, 0xFF };
	static const unsigned char beHigh[8] = { 0xFF, 0, 0xFF, 0, 0xFF, 0, 0xFF, 0 };
	uint64_t mask;
	size_t i = 0, high = le ? 1 : 0;

	std::memcpy(&mask, le ? leHigh : beHigh, 8);
	for (; i + 8 <= n; i += 8) {
		if (load64(p + i) & mask) {
			break;
		}
	}
	for (; i < n; i += 2) {
		if (p[i + high]) {
			return i;
		}
	}
	return n;
}

/* ASCII white space of base64: space, tab, LF, FF, CR */
bool
isBase64Space(unsigned char c)
{
	return c == ' ' || c == '\t' || c == '\n' || c == '\f' || c == '\r';
}

/* The portable base64 measure of p[0, n): count = bytes in 0x21..0x7F;
   padding = up to two '=' found scanning back from the end, past the bytes
   outside 0x21..0x7F, before any other byte in 0x21..0x7F;
   ((count - padding) * 3) / 4.  It is exact for every input that decodes.
   On ASCII input it is simdutf's binary_length_from_base64 (bytes > ' ',
   the same set); on other input simdutf's answer depends on the kernel
   (the scalar code compares a char, which is unsigned on aarch64), so the
   rule is applied here, with the bytes >= 0x80 never counted. */
size_t
base64Measure(const char *p, size_t n)
{
	if (n == 0) {
		return 0;
	}
	if (simdutf::validate_ascii_with_errors(p, n).error == simdutf::SUCCESS) {
		return simdutf::binary_length_from_base64(p, n);
	}
	size_t count = 0, padding = 0, i = n;
	for (size_t k = 0; k < n; k++) {
		unsigned char c = p[k];
		count += c >= 0x21 && c <= 0x7F;
	}
	while (i > 0 && padding < 2) {
		unsigned char c = p[--i];
		if (c == '=') {
			padding++;
		} else if (c >= 0x21 && c <= 0x7F) {
			break;
		}
	}
	return (count - padding) * 3 / 4;
}

/* The 0-based offset of the last byte of p[0, n) that is not base64 white
   space (0 when there is none) */
size_t
lastNonSpace(const char *p, size_t n)
{
	while (n > 0 && isBase64Space(p[n - 1])) {
		n--;
	}
	return n > 0 ? n - 1 : 0;
}

/* A misplaced or extra '=': simdutf's code for it depends on the room it is
   given.  With room for any input (its canonical answer, the contract's) it
   is 7 at the first '=' of the run; with the exact room the scalar code
   that decodes the end of the input answers 8, at that '=' or at the white
   space before it.  So after an 8 at c: when the first byte from c on that
   is not white space is a '=', and after that run of '=' and white space
   another byte follows or there are more than two '=', the answer is 7 at
   that '=' (*at, an offset in p).  Measured equal to the canonical answer
   on every kernel of this host (tests/sutest.cpp, U6, checks it on each). */
bool
misplacedPadding(const char *p, size_t n, size_t c, size_t *at)
{
	size_t q = c, k, signs = 0;

	while (q < n && isBase64Space(p[q])) {
		q++;
	}
	if (q == n || p[q] != '=') {
		return false;
	}
	for (k = q; k < n && (p[k] == '=' || isBase64Space(p[k])); k++) {
		signs += p[k] == '=';
	}
	*at = q;
	return k < n || signs > 2;
}

/*** Encoding: a String (Latin-1 bytes or UTF-32 words) to UTF-8 or UTF-16 ***/

/* The bytes of the encoding of in's range as target, BOM included; a
   character that is not a Unicode scalar value is an error in *out */
bool
encodedSize(const su_input *in, unsigned flags, su_outcome *out, size_t *size)
{
	unsigned target = flags & SU_F_ENCODING;
	size_t n = in->stop0 - in->start0, bom = 0, body = 0;

	if (flags & SU_F_BOM) {
		bom = target == SU_ENC_UTF8 ? 3 : 2;
	}
	if (n == 0) {
		body = 0;
	} else if (!in->words) {
		const char *p = static_cast<const char *>(in->base) + in->start0;
		body = target == SU_ENC_UTF8 ? simdutf::utf8_length_from_latin1(p, n) : 2 * n;
	} else {
		const char32_t *p = static_cast<const char32_t *>(in->base) + in->start0;
		simdutf::result r = simdutf::validate_utf32_with_errors(p, n);
		if (r.error != simdutf::SUCCESS) {
			answerError(out, codeOf(r.error), in->start0 + r.count);
			return false;
		}
		body = target == SU_ENC_UTF8 ? simdutf::utf8_length_from_utf32(p, n) : 2 * simdutf::utf16_length_from_utf32(p, n);
	}
	*size = bom + body;
	return true;
}

/* Writes the encoding of a valid range: size bytes from dst */
void
writeEncoded(const su_input *in, unsigned flags, char *dst, size_t size)
{
	unsigned target = flags & SU_F_ENCODING;
	size_t n = in->stop0 - in->start0;

	if (flags & SU_F_BOM) {
		if (target == SU_ENC_UTF8) {
			std::memcpy(dst, "\xEF\xBB\xBF", 3);
			dst += 3;
			size -= 3;
		} else {
			std::memcpy(dst, target == SU_ENC_UTF16LE ? "\xFF\xFE" : "\xFE\xFF", 2);
			dst += 2;
			size -= 2;
		}
	}
	if (n == 0) {
		return;
	}
	char16_t *dst16 = reinterpret_cast<char16_t *>(dst);
	if (!in->words) {
		const char *p = static_cast<const char *>(in->base) + in->start0;
		if (target == SU_ENC_UTF8) {
			if (size == n) {
				std::memcpy(dst, p, n);		/* ASCII */
			} else {
				(void)simdutf::convert_latin1_to_utf8(p, n, dst);
			}
		} else if (target == SU_ENC_UTF16LE) {
			(void)simdutf::convert_latin1_to_utf16le(p, n, dst16);
		} else {
			(void)simdutf::convert_latin1_to_utf16be(p, n, dst16);
		}
	} else {
		const char32_t *p = static_cast<const char32_t *>(in->base) + in->start0;
		if (target == SU_ENC_UTF8) {
			(void)simdutf::convert_valid_utf32_to_utf8(p, n, dst);
		} else if (target == SU_ENC_UTF16LE) {
			(void)simdutf::convert_valid_utf32_to_utf16le(p, n, dst16);
		} else {
			(void)simdutf::convert_valid_utf32_to_utf16be(p, n, dst16);
		}
	}
}

/*** Decoding: UTF-8 or UTF-16 bytes to Latin-1 or UTF-32 ***/

/* A validated text: [s, s + n) of the source, after the BOM that was skipped */
struct Text {
	unsigned encoding;
	const char *p;		/* the first byte of the text */
	size_t s, n;		/* absolute offset and bytes */
	size_t chars;
	bool ascii;		/* UTF-8 only: every byte < 0x80 */
	bool wide;		/* a character above U+00FF ... */
	size_t firstWide;	/* ... whose first byte is at this absolute offset */
};

/* Validates in's range as encoding after one leading BOM when bom; an error
   goes into *out.  The odd length of UTF-16 is checked first, then the
   validity, as the contract orders them. */
bool
scanText(const su_input *in, unsigned encoding, bool bom, Text *t, su_outcome *out)
{
	const char *base = static_cast<const char *>(in->base);
	size_t s = in->start0, e = in->stop0;

	*t = Text();
	t->encoding = encoding;
	if (encoding == SU_ENC_UTF8) {
		if (bom && e - s >= 3 && std::memcmp(base + s, "\xEF\xBB\xBF", 3) == 0) {
			s += 3;
		}
		t->s = s;
		t->n = e - s;
		t->p = base + s;
		if (t->n == 0) {
			t->ascii = true;
			return true;
		}
		simdutf::utf8_result r = simdutf::validate_utf8_with_counts(t->p, t->n);
		if (r.error != simdutf::SUCCESS) {
			/* the position of the counting validator may be that of a
			   block; the one of validate_utf8_with_errors is the first
			   byte of the faulty sequence */
			simdutf::result w = simdutf::validate_utf8_with_errors(t->p, t->n);
			if (w.error == simdutf::SUCCESS) {
				answerError(out, SU_E_OTHER, s + r.input_count);
			} else {
				answerError(out, codeOf(w.error), s + w.count);
			}
			return false;
		}
		t->chars = t->n - r.continuation_count;
		t->ascii = t->chars == t->n;
		if (!t->ascii) {
			size_t at = firstAboveLatin1(t->p, t->n);
			t->wide = at < t->n;
			t->firstWide = s + at;
		}
		return true;
	}
	bool le = encoding == SU_ENC_UTF16LE;
	if ((e - s) & 1) {
		answerError(out, SU_E_ODD_UTF16_LENGTH, e - 1);
		return false;
	}
	if (bom && e - s >= 2
		&& std::memcmp(base + s, le ? "\xFF\xFE" : "\xFE\xFF", 2) == 0) {
		s += 2;
	}
	t->s = s;
	t->n = e - s;
	t->p = base + s;
	if (t->n == 0) {
		return true;
	}
	const char16_t *p16 = reinterpret_cast<const char16_t *>(t->p);
	size_t m = t->n / 2;
	simdutf::result r = le ? simdutf::validate_utf16le_with_errors(p16, m)
		: simdutf::validate_utf16be_with_errors(p16, m);
	if (r.error != simdutf::SUCCESS) {
		answerError(out, codeOf(r.error), s + 2 * r.count);
		return false;
	}
	size_t at = firstAboveLatin1Unit(t->p, t->n, le);
	t->wide = at < t->n;
	t->firstWide = s + at;
	t->chars = !t->wide ? m : le ? simdutf::utf32_length_from_utf16le(p16, m)
		: simdutf::utf32_length_from_utf16be(p16, m);
	return true;
}

/* Writes the characters of a validated text as kind (Latin-1 bytes or UTF-32
   words; Latin-1 only when no character is above U+00FF) */
void
writeDecoded(const Text *t, int kind, void *dst)
{
	if (t->chars == 0) {
		return;
	}
	char *dst8 = static_cast<char *>(dst);
	char32_t *dst32 = static_cast<char32_t *>(dst);
	if (t->encoding == SU_ENC_UTF8) {
		if (kind == SU_KIND_UTF32) {
			(void)simdutf::convert_valid_utf8_to_utf32(t->p, t->n, dst32);
		} else if (t->ascii) {
			std::memcpy(dst8, t->p, t->n);
		} else {
			(void)simdutf::convert_valid_utf8_to_latin1(t->p, t->n, dst8);
		}
		return;
	}
	const char16_t *p16 = reinterpret_cast<const char16_t *>(t->p);
	size_t m = t->n / 2;
	if (t->encoding == SU_ENC_UTF16LE) {
		if (kind == SU_KIND_UTF32) {
			(void)simdutf::convert_valid_utf16le_to_utf32(p16, m, dst32);
		} else {
			(void)simdutf::convert_valid_utf16le_to_latin1(p16, m, dst8);
		}
	} else {
		if (kind == SU_KIND_UTF32) {
			(void)simdutf::convert_valid_utf16be_to_utf32(p16, m, dst32);
		} else {
			(void)simdutf::convert_valid_utf16be_to_latin1(p16, m, dst8);
		}
	}
}

/* A UTF-16 source must start on a unit: an even start0 (an index), from a
   base on a 2-byte boundary (an object) */
int
checkUtf16Source(const su_input *in)
{
	if (!isAligned(in->base, 2)) {
		return SU_RC_BAD_ARGUMENT;
	}
	return in->start0 & 1 ? SU_RC_BAD_INDEX : SU_RC_DONE;
}

} /* namespace */

extern "C" int
su_init(void)
{
	static const bool once = choose();	/* a magic static: thread-safe, run once */
	(void)once;
	return ready;
}

extern "C" void
su_get_info(su_info *info)
{
	su_init();
	std::memset(info, 0, sizeof *info);
	info->ready = ready;
#ifdef __EMSCRIPTEN__
	info->emscripten = 1;
#endif
	copyName(info->simdutf, sizeof info->simdutf, SIMDUTF_VERSION);
	copyName(info->implementation, sizeof info->implementation,
		simdutf::get_active_implementation()->name());
	copyName(info->forced, sizeof info->forced, forcedNote);
	for (const simdutf::implementation *each : simdutf::get_available_implementations()) {
		if (info->count == (int)(sizeof info->implementations / sizeof info->implementations[0])) {
			break;
		}
		if (each->name() != "unsupported" && each->supported_by_runtime_system()) {
			copyName(info->implementations[info->count], sizeof info->implementations[0], each->name());
			info->count++;
		}
	}
}

/* A String to UTF-8, UTF-16LE or UTF-16BE (flags: the target, SU_F_BOM).
   A word source is validated first: 5 (above U+10FFFF) or 6 (a surrogate)
   at the word. */
extern "C" int
su_encode(const su_input *in, unsigned flags, const su_sink *sink, su_outcome *out)
{
	su_init();
	if (!out || badInput(in) || badSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	unsigned target = flags & SU_F_ENCODING;
	if ((flags & ~(unsigned)(SU_F_ENCODING | SU_F_BOM)) || !isUnicode(target)) {
		return SU_RC_BAD_ARGUMENT;
	}
	if (badRange(in)) {
		return SU_RC_BAD_INDEX;
	}
	if (sink->base) {
		if (!byteSink(sink)) {
			return SU_RC_BAD_ARGUMENT;
		}
		if (isUtf16(target) && !isAligned(sink->base, 2)) {
			return SU_RC_BAD_INDEX;
		}
	}
	size_t size;
	if (!encodedSize(in, flags, out, &size)) {
		return SU_RC_DONE;
	}
	char *dst = static_cast<char *>(place(sink, SU_KIND_BYTES, size, out));
	if (!dst) {
		return SU_RC_NO_ROOM;
	}
	writeEncoded(in, flags, dst, size);
	return answerOk(out, SU_KIND_BYTES, size);
}

/* UTF-8, UTF-16LE or UTF-16BE bytes to a String: the narrowest kind, unless
   SU_F_WIDE or SU_F_LATIN1 force one, or an into-sink takes one.  Latin-1
   asked of a text with a character above U+00FF is 1002 at that character;
   validity errors come first. */
extern "C" int
su_decode(const su_input *in, unsigned flags, const su_sink *sink, su_outcome *out)
{
	su_init();
	if (!out || badInput(in) || badSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	unsigned encoding = flags & SU_F_ENCODING;
	if (in->words || (flags & ~(unsigned)(SU_F_ENCODING | SU_F_BOM | SU_F_WIDE | SU_F_LATIN1))
		|| !isUnicode(encoding) || ((flags & SU_F_WIDE) && (flags & SU_F_LATIN1))) {
		return SU_RC_BAD_ARGUMENT;
	}
	if (badRange(in)) {
		return SU_RC_BAD_INDEX;
	}
	if (isUtf16(encoding)) {
		int rc = checkUtf16Source(in);
		if (rc != SU_RC_DONE) {
			return rc;
		}
	}
	if (sink->base) {
		if (sink->kind == SU_KIND_UTF32) {
			if ((flags & SU_F_LATIN1) || !isAligned(sink->base, 4)) {
				return SU_RC_BAD_ARGUMENT;
			}
		} else if (!byteSink(sink) || (flags & SU_F_WIDE)) {
			return SU_RC_BAD_ARGUMENT;
		}
	}
	Text t;
	if (!scanText(in, encoding, flags & SU_F_BOM, &t, out)) {
		return SU_RC_DONE;
	}
	int kind = t.wide ? SU_KIND_UTF32 : SU_KIND_LATIN1;
	if (sink->base) {
		kind = sink->kind == SU_KIND_UTF32 ? SU_KIND_UTF32 : SU_KIND_LATIN1;
	} else if (flags & SU_F_WIDE) {
		kind = SU_KIND_UTF32;
	} else if (flags & SU_F_LATIN1) {
		kind = SU_KIND_LATIN1;
	}
	if (kind == SU_KIND_LATIN1 && t.wide) {
		return answerError(out, SU_E_NOT_LATIN1, t.firstWide);
	}
	void *dst = place(sink, kind, t.chars, out);
	if (!dst) {
		return SU_RC_NO_ROOM;
	}
	writeDecoded(&t, kind, dst);
	return answerOk(out, kind, t.chars);
}

/* ENCODED: the bytes su_encode writes; DECODED: the characters su_decode
   makes, and in out->kind the narrowest kind; BASE64: the portable measure
   of the bytes base64 decoding makes (no validation) */
extern "C" int
su_measure(const su_input *in, unsigned flags, su_outcome *out)
{
	su_init();
	if (!out || badInput(in)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	unsigned encoding = flags & SU_F_ENCODING;
	switch (flags & SU_F_OP) {
	case SU_OP_ENCODED: {
		if ((flags & ~(unsigned)(SU_F_ENCODING | SU_F_BOM)) || !isUnicode(encoding)) {
			return SU_RC_BAD_ARGUMENT;
		}
		if (badRange(in)) {
			return SU_RC_BAD_INDEX;
		}
		size_t size;
		if (!encodedSize(in, flags, out, &size)) {
			return SU_RC_DONE;
		}
		return answerOk(out, SU_KIND_BYTES, size);
	}
	case SU_OP_DECODED: {
		if (in->words || (flags & ~(unsigned)(SU_F_OP | SU_F_ENCODING | SU_F_BOM)) || !isUnicode(encoding)) {
			return SU_RC_BAD_ARGUMENT;
		}
		if (badRange(in)) {
			return SU_RC_BAD_INDEX;
		}
		if (isUtf16(encoding)) {
			int rc = checkUtf16Source(in);
			if (rc != SU_RC_DONE) {
				return rc;
			}
		}
		Text t;
		if (!scanText(in, encoding, flags & SU_F_BOM, &t, out)) {
			return SU_RC_DONE;
		}
		return answerOk(out, t.wide ? SU_KIND_UTF32 : SU_KIND_LATIN1, t.chars);
	}
	case SU_OP_BASE64:
		if (in->words || flags != SU_OP_BASE64) {
			return SU_RC_BAD_ARGUMENT;
		}
		if (badRange(in)) {
			return SU_RC_BAD_INDEX;
		}
		return answerOk(out, SU_KIND_BYTES,
			base64Measure(static_cast<const char *>(in->base) + in->start0, in->stop0 - in->start0));
	default:
		return SU_RC_BAD_ARGUMENT;
	}
}

/* VALID: UTF-8 or UTF-16 bytes, or UTF-32 words, are valid; ASCII: no unit
   above 0x7F (code 5 at the first one).  out->units is 0 when valid. */
extern "C" int
su_validate(const su_input *in, unsigned flags, su_outcome *out)
{
	su_init();
	if (!out || badInput(in)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	const char *base = static_cast<const char *>(in->base);
	size_t s = in->start0, n = in->stop0 - in->start0;
	if (flags == SU_OP_ASCII) {
		if (badRange(in)) {
			return SU_RC_BAD_INDEX;
		}
		if (n == 0) {
			return answerOk(out, SU_KIND_BYTES, 0);
		}
		if (!in->words) {
			simdutf::result r = simdutf::validate_ascii_with_errors(base + s, n);
			if (r.error != simdutf::SUCCESS) {
				return answerError(out, SU_E_TOO_LARGE, s + r.count);
			}
		} else {
			/* simdutf has no ASCII check of UTF-32 */
			const uint32_t *p = reinterpret_cast<const uint32_t *>(in->base) + s;
			for (size_t i = 0; i < n; i++) {
				if (p[i] > 0x7F) {
					return answerError(out, SU_E_TOO_LARGE, s + i);
				}
			}
		}
		return answerOk(out, SU_KIND_BYTES, 0);
	}
	unsigned encoding = flags;	/* SU_OP_VALID: flags are the encoding alone */
	if (in->words ? encoding != SU_ENC_UTF32 : !isUnicode(encoding)) {
		return SU_RC_BAD_ARGUMENT;
	}
	if (badRange(in)) {
		return SU_RC_BAD_INDEX;
	}
	if (isUtf16(encoding)) {
		int rc = checkUtf16Source(in);
		if (rc != SU_RC_DONE) {
			return rc;
		}
		if (n & 1) {
			return answerError(out, SU_E_ODD_UTF16_LENGTH, in->stop0 - 1);
		}
	}
	if (n == 0) {
		return answerOk(out, SU_KIND_BYTES, 0);
	}
	simdutf::result r;
	size_t unit = 1;
	if (encoding == SU_ENC_UTF8) {
		r = simdutf::validate_utf8_with_errors(base + s, n);
	} else if (encoding == SU_ENC_UTF32) {
		r = simdutf::validate_utf32_with_errors(reinterpret_cast<const char32_t *>(in->base) + s, n);
	} else {
		const char16_t *p16 = reinterpret_cast<const char16_t *>(base + s);
		r = encoding == SU_ENC_UTF16LE ? simdutf::validate_utf16le_with_errors(p16, n / 2)
			: simdutf::validate_utf16be_with_errors(p16, n / 2);
		unit = 2;
	}
	if (r.error != simdutf::SUCCESS) {
		return answerError(out, codeOf(r.error), s + unit * r.count);
	}
	return answerOk(out, SU_KIND_BYTES, 0);
}

/* The characters of base64 for n bytes: L = simdutf's length (padded or
   not), plus a line end after each group of 4 characters that completes a
   line, the last one included, counting the last group as 4 even when it
   is not padded (ZnBase64Encoder's rule).  SIZE_MAX for bad options or when
   the result does not fit in a size_t (below SIZE_MAX). */
extern "C" size_t
su_base64_encoded_size(size_t n, unsigned options, size_t lineLength, unsigned lineEnd)
{
	if (options > SU_B64_URL_PADDED || lineEnd > SU_EOL_CRLF
		|| (lineLength != 0 && (lineLength % 4 != 0 || lineLength >= SU_MAX_LINE_LENGTH))) {
		return SIZE_MAX;
	}
	size_t groups = n / 3 + (n % 3 != 0);
	if (groups > SIZE_MAX / 4) {
		return SIZE_MAX;
	}
	bool padded = options == SU_B64_STANDARD || options == SU_B64_URL_PADDED;
	size_t length = padded || n % 3 == 0 ? groups * 4 : n / 3 * 4 + n % 3 + 1;
	size_t lineEnds = lineLength == 0 ? 0 : groups / (lineLength / 4);
	size_t extra = lineEnds * (lineEnd == SU_EOL_CRLF ? 2 : 1);	/* <= 2 * groups: no overflow */
	if (length > SIZE_MAX - 1 - extra) {
		return SIZE_MAX;
	}
	return length + extra;
}

/* Bytes to base64 characters (a ByteString).  simdutf encodes the whole
   range once, at the end of the result; the lines are then moved down to
   their places, a line end after each.  Line k moves from E + k*ll to
   k*(ll + eol), never past where line k + 1 still is, since
   (k + 1)*(ll + eol) <= E + (k + 1)*ll for k + 1 <= lineEnds (E =
   lineEnds*eol).  The rest after the last line end is already in place. */
extern "C" int
su_base64_encode(const su_input *in, unsigned options, size_t lineLength, unsigned lineEnd,
		const su_sink *sink, su_outcome *out)
{
	su_init();
	if (!out || badInput(in) || badSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	if (in->words || options > SU_B64_URL_PADDED || lineEnd > SU_EOL_CRLF
		|| (lineLength != 0 && (lineLength % 4 != 0 || lineLength >= SU_MAX_LINE_LENGTH))) {
		return SU_RC_BAD_ARGUMENT;
	}
	if (badRange(in)) {
		return SU_RC_BAD_INDEX;
	}
	if (sink->base && !byteSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	size_t n = in->stop0 - in->start0;
	size_t size = su_base64_encoded_size(n, options, lineLength, lineEnd);
	if (size == SIZE_MAX) {
		return SU_RC_BAD_ARGUMENT;	/* cannot happen: n is the size of an object */
	}
	char *dst = static_cast<char *>(place(sink, SU_KIND_LATIN1, size, out));
	if (!dst) {
		return SU_RC_NO_ROOM;
	}
	if (n > 0) {
		size_t groups = n / 3 + (n % 3 != 0);
		size_t lineEnds = lineLength == 0 ? 0 : groups / (lineLength / 4);
		size_t eol = lineEnd == SU_EOL_CRLF ? 2 : 1;
		size_t ends = lineEnds * eol;
		size_t length = simdutf::binary_to_base64(static_cast<const char *>(in->base) + in->start0, n,
			dst + ends, static_cast<simdutf::base64_options>(options));
		for (size_t k = 0; k < lineEnds; k++) {
			size_t from = ends + k * lineLength, to = k * (lineLength + eol);
			size_t line = length - k * lineLength < lineLength ? length - k * lineLength : lineLength;
			std::memmove(dst + to, dst + from, line);
			to += line;
			if (lineEnd == SU_EOL_LF) {
				dst[to] = '\n';
			} else if (lineEnd == SU_EOL_CR) {
				dst[to] = '\r';
			} else {
				dst[to] = '\r';
				dst[to + 1] = '\n';
			}
		}
	}
	return answerOk(out, SU_KIND_LATIN1, size);
}

/* Base64 characters to bytes (a ByteArray).  ASCII white space is skipped
   anywhere.  The size is the portable measure, compared with the room first
   (or allocated) before decoding; an error leaves the bytes written so far
   in the result.  7 is at the bad character (a misplaced '=' included); 8
   and 9 (the end of the input) at the last byte that is not white space. */
extern "C" int
su_base64_decode(const su_input *in, unsigned options, unsigned lastChunk,
		const su_sink *sink, su_outcome *out)
{
	su_init();
	if (!out || badInput(in) || badSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	*out = su_outcome();
	if (in->words || (options != SU_B64_STANDARD && options != SU_B64_URL && options != SU_B64_ANY)
		|| (lastChunk != SU_CHUNK_LOOSE && lastChunk != SU_CHUNK_STRICT)) {
		return SU_RC_BAD_ARGUMENT;
	}
	if (badRange(in)) {
		return SU_RC_BAD_INDEX;
	}
	if (sink->base && !byteSink(sink)) {
		return SU_RC_BAD_ARGUMENT;
	}
	size_t s = in->start0, n = in->stop0 - in->start0;
	const char *p = n ? static_cast<const char *>(in->base) + s : "";
	size_t length = base64Measure(p, n);
	char *dst = static_cast<char *>(place(sink, SU_KIND_BYTES, length, out));
	if (!dst) {
		return SU_RC_NO_ROOM;
	}
	size_t written = length;
	simdutf::result r = simdutf::base64_to_binary_safe(p, n, dst, written,
		static_cast<simdutf::base64_options>(options),
		static_cast<simdutf::last_chunk_handling_options>(lastChunk), false);
	switch (r.error) {
	case simdutf::SUCCESS:
		if (written != length) {
			return answerError(out, SU_E_OTHER, s);		/* never: the measure is exact */
		}
		return answerOk(out, SU_KIND_BYTES, length);
	case simdutf::INVALID_BASE64_CHARACTER:
		return answerError(out, SU_E_INVALID_BASE64_CHARACTER, s + r.count);
	case simdutf::BASE64_INPUT_REMAINDER: {
		size_t at;
		if (misplacedPadding(p, n, r.count, &at)) {
			return answerError(out, SU_E_INVALID_BASE64_CHARACTER, s + at);
		}
		return answerError(out, SU_E_BASE64_INPUT_REMAINDER, s + lastNonSpace(p, n));
	}
	case simdutf::BASE64_EXTRA_BITS:
		return answerError(out, SU_E_BASE64_EXTRA_BITS, s + lastNonSpace(p, n));
	default:
		return answerError(out, SU_E_OTHER, s);		/* never: OUTPUT_BUFFER_TOO_SMALL */
	}
}
