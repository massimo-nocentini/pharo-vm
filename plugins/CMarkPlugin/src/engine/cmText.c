/* cmText.c -- the sanitised UTF-8 text T that the CMarkPlugin's engine feeds
 * to cmark-gfm, and the tables that turn cmark's positions (1-based lines,
 * 1-based byte columns) into 1-based character indexes of T
 * (docs/CMark.md, "Tree format": UTF-8, sanitising, positions).
 *
 * T is valid UTF-8 without NUL:
 *   ByteString (Latin-1)  bytes below 0x80 are copied, the others become 2
 *                         bytes, NUL becomes U+FFFD;
 *   WideString (UTF-32)   each code point is encoded; NUL, the surrogates and
 *                         values above U+10FFFF become U+FFFD;
 *   ByteArray (UTF-8)     valid shortest-form sequences are copied; each
 *                         byte that does not start one becomes one U+FFFD,
 *                         and so does NUL.
 * So character k of a String source is code point k of T.
 *
 * Lines are cut as cmark cuts them (blocks.c, S_parser_feed): a line ends at
 * CR LF, a lone CR or a lone LF; line L+1, the phantom line of the blocks
 * that end at the end of the text, starts at |T|.  The tables are sparse:
 * lineStart(64j + 1) and cont(64j) (the continuation bytes before byte 64j)
 * every 64 lines and 64 bytes, plus a few cursors on the last lines
 * resolved, so that the C memory stays far below one byte per input byte.
 * No cmark header here.
 */

#include <string.h>

#include "cmPrivate.h"

/* The length of the valid, shortest-form UTF-8 sequence of a code point
 * other than NUL at s (avail bytes there), or 0 */
static size_t
sequenceAt(const uint8_t *s, size_t avail)
{
	uint8_t c = s[0];

	if (c < 0x80) {
		return c ? 1 : 0;
	}
	if (c < 0xC2) {
		return 0;
	}
	if (c < 0xE0) {
		return avail >= 2 && (s[1] & 0xC0) == 0x80 ? 2 : 0;
	}
	if (c < 0xF0) {
		if (avail < 3 || (s[1] & 0xC0) != 0x80 || (s[2] & 0xC0) != 0x80
		 || (c == 0xE0 && s[1] < 0xA0) || (c == 0xED && s[1] >= 0xA0)) {
			return 0;
		}
		return 3;
	}
	if (c < 0xF5) {
		if (avail < 4 || (s[1] & 0xC0) != 0x80 || (s[2] & 0xC0) != 0x80 || (s[3] & 0xC0) != 0x80
		 || (c == 0xF0 && s[1] < 0x90) || (c == 0xF4 && s[1] >= 0x90)) {
			return 0;
		}
		return 4;
	}
	return 0;
}

/* The UTF-8 bytes of a code point of a WideString (U+FFFD when invalid) */
static size_t
encodedSize(uint32_t c)
{
	if (c == 0 || (c >= 0xD800 && c <= 0xDFFF) || c > 0x10FFFF) {
		return 3;
	}
	return c < 0x80 ? 1 : c < 0x800 ? 2 : c < 0x10000 ? 3 : 4;
}

static uint8_t *
encode(uint8_t *out, uint32_t c)
{
	if (c == 0 || (c >= 0xD800 && c <= 0xDFFF) || c > 0x10FFFF) {
		c = 0xFFFD;
	}
	if (c < 0x80) {
		*out++ = (uint8_t)c;
	} else if (c < 0x800) {
		*out++ = (uint8_t)(0xC0 | (c >> 6));
		*out++ = (uint8_t)(0x80 | (c & 0x3F));
	} else if (c < 0x10000) {
		*out++ = (uint8_t)(0xE0 | (c >> 12));
		*out++ = (uint8_t)(0x80 | ((c >> 6) & 0x3F));
		*out++ = (uint8_t)(0x80 | (c & 0x3F));
	} else {
		*out++ = (uint8_t)(0xF0 | (c >> 18));
		*out++ = (uint8_t)(0x80 | ((c >> 12) & 0x3F));
		*out++ = (uint8_t)(0x80 | ((c >> 6) & 0x3F));
		*out++ = (uint8_t)(0x80 | (c & 0x3F));
	}
	return out;
}

size_t
cm_text_measure(const cm_source *src, int *differs, size_t *chars)
{
	size_t size = 0, i, k;

	*differs = 0;
	*chars = 0;
	switch (src->kind) {
	case CM_SRC_LATIN1: {
		const uint8_t *s = src->units;
		for (i = 0; i < src->count; i++) {
			size += s[i] == 0 ? 3 : s[i] < 0x80 ? 1 : 2;
		}
		*chars = src->count;
		break;
	}
	case CM_SRC_UTF32: {
		const uint32_t *s = src->units;
		for (i = 0; i < src->count; i++) {
			size += encodedSize(s[i]);
		}
		*chars = src->count;
		break;
	}
	default: {
		const uint8_t *s = src->units;
		for (i = 0; i < src->count; i += k) {
			k = sequenceAt(s + i, src->count - i);
			if (k) {
				size += k;
			} else {
				size += 3;
				*differs = 1;
				k = 1;
			}
			(*chars)++;
		}
		break;
	}
	}
	return size;
}

void
cm_text_fill(const cm_source *src, uint8_t *out)
{
	size_t i, k;

	switch (src->kind) {
	case CM_SRC_LATIN1: {
		const uint8_t *s = src->units;
		for (i = 0; i < src->count; i++) {
			out = s[i] < 0x80 && s[i] ? (*out = s[i], out + 1) : encode(out, s[i]);
		}
		break;
	}
	case CM_SRC_UTF32: {
		const uint32_t *s = src->units;
		for (i = 0; i < src->count; i++) {
			out = encode(out, s[i]);
		}
		break;
	}
	default: {
		const uint8_t *s = src->units;
		for (i = 0; i < src->count; i += k) {
			k = sequenceAt(s + i, src->count - i);
			if (k) {
				memcpy(out, s + i, k);
				out += k;
			} else {
				out = encode(out, 0xFFFD);
				k = 1;
			}
		}
		break;
	}
	}
}


/*** Lines and character indexes ***/

/* The start of the line after the one that contains (or starts at) pos:
 * after its CR LF, CR or LF, or |T| when it has no terminator */
static size_t
nextLine(const cm_lines *l, size_t pos)
{
	const uint8_t *t = l->t;
	size_t n = l->n;

	while (pos < n) {
		uint8_t c = t[pos++];
		if (c == '\n') {
			return pos;
		}
		if (c == '\r') {
			return pos < n && t[pos] == '\n' ? pos + 1 : pos;
		}
	}
	return n;
}

void
cm_lines_init(cm_lines *l, const uint8_t *t, size_t n)
{
	size_t marks = 16, line = 1, pos = 0, i, j, cont = 0;

	memset(l, 0, sizeof *l);
	l->t = t;
	l->n = n;
	l->contMarks = cm_own_alloc((n / 64 + 1) * sizeof(uint32_t));
	for (j = 0, i = 0; j <= n / 64; j++) {
		size_t stop = j * 64;
		for (; i < stop; i++) {
			cont += (t[i] & 0xC0) == 0x80;
		}
		l->contMarks[j] = (uint32_t)cont;
	}
	l->lineMarks = cm_own_alloc(marks * sizeof(uint32_t));
	l->lineMarks[0] = 0;
	/* every line start: 0, the end of each terminator, and |T| */
	while (pos < n) {
		pos = nextLine(l, pos);
		line++;
		if ((line - 1) % 64 == 0) {
			if ((line - 1) / 64 >= marks) {
				marks *= 2;
				l->lineMarks = cm_own_realloc(l->lineMarks, marks * sizeof(uint32_t));
			}
			l->lineMarks[(line - 1) / 64] = (uint32_t)pos;
		}
	}
	/* a text that ends with a terminator ends with the start of an empty
	   line, the phantom one; an empty text has the one line start 0 */
	l->lines = line;
}

/* The bounds of line (1 .. l->lines) */
static void
lineBounds(cm_lines *l, size_t line, size_t *start, size_t *end)
{
	size_t j = (line - 1) / 64, from = j * 64 + 1, pos = l->lineMarks[j], k;
	cm_cursor *best = NULL, *slot;

	l->lookups++;
	for (k = 0; k < CM_CURSORS; k++) {
		cm_cursor *c = &l->cursors[k];
		if (c->line && c->line <= line && c->line >= from && (!best || c->line > best->line)) {
			best = c;
		}
	}
	if (best && best->line == line) {
		best->used = l->lookups;
		*start = best->start;
		*end = best->end;
		return;
	}
	if (best) {
		best->used = l->lookups;
		from = best->line + 1;
		pos = best->end;
	}
	for (; from < line; from++) {
		pos = nextLine(l, pos);
	}
	/* a new cursor, in the least recently used slot (never the base) */
	slot = &l->cursors[0];
	for (k = 1; k < CM_CURSORS; k++) {
		if (l->cursors[k].used < slot->used) {
			slot = &l->cursors[k];
		}
	}
	slot->line = line;
	slot->start = pos;
	slot->end = nextLine(l, pos);
	slot->used = l->lookups;
	*start = slot->start;
	*end = slot->end;
}

/* cont(k) for 0 <= k <= |T| */
static size_t
contBefore(const cm_lines *l, size_t k)
{
	size_t c = l->contMarks[k / 64], i;

	for (i = k / 64 * 64; i < k; i++) {
		c += (l->t[i] & 0xC0) == 0x80;
	}
	return c;
}

/* The character index of the character holding byte k - 1 (k = b + 1, so
 * 0 for none) */
static uint32_t
charIndex(const cm_lines *l, size_t k)
{
	return (uint32_t)(k - contBefore(l, k));
}

/* b + 1 for the byte b of cmark's (line, column >= 1), clamped */
static size_t
bytePos(cm_lines *l, int line, int column, int *clamped)
{
	size_t start, end, off;

	if ((size_t)line > l->lines) {
		*clamped = 1;
		return l->n;
	}
	lineBounds(l, (size_t)line, &start, &end);
	off = start + (size_t)column - 1;
	if (off >= end) {
		*clamped = 1;
		/* the last byte of the line, or of T for the phantom line */
		return end > start ? end : l->n;
	}
	return off + 1;
}

int
cm_lines_span(cm_lines *l, int sl, int sc, int el, int ec, uint32_t *si, uint32_t *ei)
{
	int clamped = 0;
	size_t k;

	CM_ASSERT(sl >= 1 && sc >= 1);
	if (l->n == 0) {
		/* no byte to point at (the document of an empty source, 1:1-0:0):
		   the empty span at the start */
		*si = 1;
		*ei = 0;
		return 1;
	}
	*si = charIndex(l, bytePos(l, sl, sc, &clamped));
	if (el >= 1 && ec >= 1) {
		k = bytePos(l, el, ec, &clamped);
	} else if (el >= 1) {
		/* the end of the line before el: its terminator */
		if ((size_t)el > l->lines) {
			clamped = 1;
			k = l->n;
		} else {
			size_t start, end;
			lineBounds(l, (size_t)el, &start, &end);
			k = start;
		}
	} else {
		clamped = 1;
		k = 0;
	}
	*ei = charIndex(l, k);
	if ((uint64_t)*ei + 1 < *si) {
		clamped = 1;
		*ei = *si - 1;
	}
	return clamped;
}
