/* scstub.c -- a byte-loop stand-in for the engine (src/engine/scEngine.cpp),
 * for gluetest.c until the engine exists.  It follows scEngine.h and is
 * neither fast nor part of the plugin. */

#include <string.h>
#include "scEngine.h"

void sc_init(void) { }
const char *sc_kernel(void) { return "stub"; }
const char *sc_forced_note(void) { return ""; }
int sc_kernel_count(void) { return 1; }
const char *sc_kernel_name(int i) { (void)i; return "stub"; }

static uint32_t
unit(const sc_input *in, uint64_t i)
{
	return in->wide ? ((const uint32_t *)in->units)[i] : ((const uint8_t *)in->units)[i];
}

static int
lineEnd(const sc_dialect *d, uint32_t u)
{
	return u == '\n' || (u == '\r' && !(d->flags & SC_F_LF_ONLY));
}

static int
fieldEnd(const sc_dialect *d, uint32_t u)
{
	return u == d->separator || lineEnd(d, u);
}

static int
isQuote(const sc_dialect *d, uint32_t u)
{
	return !(d->flags & SC_F_NO_QUOTE) && u == d->quote;
}

/* Past the line end at e */
static uint64_t
after(const sc_input *in, const sc_dialect *d, uint64_t e)
{
	if (unit(in, e) == '\r' && !(d->flags & SC_F_LF_ONLY) && e + 1 < in->to && unit(in, e + 1) == '\n') {
		return e + 2;
	}
	return e + 1;
}

static void
scan(const sc_input *in, const sc_dialect *d, uint64_t base, uint32_t *dest, uint64_t capacity, sc_scan *s)
{
	uint64_t pos = in->from, i, n, open;
	int final = d->flags & SC_F_FINAL, state, done, wait;
	enum { START, UNQUOTED, QUOTED, AFTER };

	memset(s, 0, sizeof *s);
	s->next = in->from;
	if (in->to - base > SC_MAX_SPAN) {
		s->status = SC_TOO_LARGE;
		return;
	}
	for (;;) {
		/* a line end in the last position may be half of a CRLF */
#define WAIT(e) (unit(in, e) == '\r' && e + 1 == in->to && !final)
		if ((d->flags & SC_F_SKIP_BLANK) && pos < in->to && lineEnd(d, unit(in, pos))) {
			if (WAIT(pos)) {
				break;
			}
			pos = s->next = after(in, d, pos);
			continue;
		}
		if (pos == in->to) {
			break;
		}
		n = 0;
		open = 0;
		done = wait = 0;
		state = START;
		for (i = pos; i < in->to && !done; i++) {
			uint32_t u = unit(in, i);

			if (state == QUOTED) {
				if (isQuote(d, u)) {
					state = AFTER;
				}
			} else if (fieldEnd(d, u)) {
				if (s->stored + n == capacity) {
					s->status = SC_MORE;
					return;
				}
				if (dest) {
					dest[s->stored + n] = (uint32_t)(i - base);
				}
				n++;
				state = START;
				if (u != d->separator || lineEnd(d, u)) {
					done = 1;
					wait = WAIT(i);
					pos = after(in, d, i);
				}
			} else if (isQuote(d, u) && (state == START || state == AFTER)) {
				if (state == START) {
					open = i;
				}
				state = QUOTED;
			} else {
				if ((d->flags & SC_F_STRICT) && (state == AFTER || (state == UNQUOTED && isQuote(d, u)))) {
					s->status = SC_ERROR;
					s->code = state == AFTER ? SC_E_TEXT_AFTER_QUOTE : SC_E_UNEXPECTED_QUOTE;
					s->position = i;
					return;
				}
				state = UNQUOTED;
			}
		}
		if (!done) {
			if (!final) {
				break;
			}
			if (state == QUOTED && (d->flags & SC_F_STRICT)) {
				s->status = SC_ERROR;
				s->code = SC_E_UNTERMINATED_QUOTE;
				s->position = open;
				return;
			}
			if (s->stored + n == capacity) {
				s->status = SC_MORE;
				return;
			}
			if (dest) {
				dest[s->stored + n] = (uint32_t)(in->to - base);
			}
			n++;
			pos = in->to;
		}
		if (wait) {
			break;
		}
		s->stored += n;
		s->records++;
		s->next = pos;
		if (s->records == 1 || n < s->min_fields) {
			s->min_fields = n;
		}
		if (n > s->max_fields) {
			s->max_fields = n;
		}
	}
}

static void
lines(const sc_input *in, const sc_dialect *d, sc_scan *s)
{
	uint64_t i;

	if (d->flags & SC_F_LINES) {
		for (i = in->from; i < s->next; i++) {
			if (lineEnd(d, unit(in, i))) {
				s->lines++;
				i = after(in, d, i) - 1;
			}
		}
	}
}

void
sc_measure(const sc_input *in, const sc_dialect *d, sc_scan *s)
{
	scan(in, d, in->to, NULL, UINT64_MAX, s);
	s->min_fields = s->max_fields = 0;
	lines(in, d, s);
}

void
sc_index(const sc_input *in, const sc_dialect *d, uint64_t base, uint32_t *dest, uint64_t capacity, sc_scan *s)
{
	scan(in, d, base, dest, capacity, s);
	lines(in, d, s);
}


int
sc_cursor_open(sc_cursor *c, const sc_input *in, const sc_dialect *d, const uint32_t *index, uint64_t first, uint64_t end)
{
	c->in = *in;
	c->d = *d;
	c->index = index;
	c->slot = first;
	c->end = end;
	c->pos = in->from;
	c->at_start = 1;
	if (first) {
		uint64_t e = in->from + index[first - 1];

		if (e >= in->to || !lineEnd(d, unit(in, e))) {
			return SC_E_BAD_INDEX;
		}
		c->pos = after(in, d, e);
	}
	return 0;
}

int
sc_cursor_next(sc_cursor *c, sc_field *f)
{
	uint64_t e;

	if (c->at_start) {
		if (c->d.flags & SC_F_SKIP_BLANK) {
			while (c->pos < c->in.to && lineEnd(&c->d, unit(&c->in, c->pos))) {
				c->pos++;
			}
		}
		if (c->pos >= c->in.to) {
			return SC_E_BAD_INDEX;
		}
	}
	if (c->slot >= c->end) {
		return SC_E_BAD_INDEX;
	}
	e = c->in.from + c->index[c->slot];
	if (e < c->pos || e > c->in.to) {
		return SC_E_BAD_INDEX;
	}
	f->start = c->pos;
	f->end = e;
	if (e == c->in.to) {
		f->last = 1;
		c->pos = e;
	} else if (lineEnd(&c->d, unit(&c->in, e))) {
		f->last = 1;
		c->pos = after(&c->in, &c->d, e);
	} else if (unit(&c->in, e) == c->d.separator) {
		f->last = 0;
		c->pos = e + 1;
	} else {
		return SC_E_BAD_INDEX;
	}
	c->slot++;
	c->at_start = f->last;
	return 0;
}


/* The next unit of the text at *i, unquoted; 0 at its end */
typedef struct { const sc_input *in; const sc_dialect *d; uint64_t i, end; int quoted; } Walk;

static int
nextUnit(Walk *w, uint32_t *u, int *odd)
{
	while (w->i < w->end) {
		*u = unit(w->in, w->i++);
		if (w->quoted && isQuote(w->d, *u)) {
			*odd = 1;
			if (w->i < w->end && isQuote(w->d, unit(w->in, w->i))) {
				w->i++;
				return 1;
			}
			if (w->i == w->end) {
				*odd = 0;	/* the closing quote */
			}
			w->quoted = 0;
			continue;
		}
		return 1;
	}
	return 0;
}

static Walk
walk(const sc_input *in, const sc_dialect *d, const sc_field *f)
{
	Walk w = { in, d, f->start, f->end, 0 };

	if (f->start < f->end && isQuote(d, unit(in, f->start))) {
		w.quoted = 1;
		w.i++;
	}
	return w;
}

/* The next character; 0 at the end, -1 invalid UTF-8 at *at */
static int
nextCharacter(Walk *w, uint32_t *c, uint64_t *at)
{
	uint32_t u, v, min;
	int odd = 0, more;

	if (!nextUnit(w, &u, &odd)) {
		return 0;
	}
	*at = w->i - 1;
	if (w->in->wide || (w->d->flags & SC_F_LATIN1) || u < 0x80) {
		*c = u;
		return 1;
	}
	if (u >= 0xC2 && u < 0xE0) {
		more = 1; *c = u & 0x1F; min = 0x80;
	} else if (u >= 0xE0 && u < 0xF0) {
		more = 2; *c = u & 0x0F; min = 0x800;
	} else if (u >= 0xF0 && u < 0xF5) {
		more = 3; *c = u & 0x07; min = 0x10000;
	} else {
		return -1;
	}
	while (more--) {
		if (!nextUnit(w, &v, &odd) || (v & 0xC0) != 0x80) {
			return -1;
		}
		*c = (*c << 6) | (v & 0x3F);
	}
	return *c < min || *c > 0x10FFFF || (*c >= 0xD800 && *c < 0xE000) ? -1 : 1;
}

void
sc_text_measure(const sc_input *in, const sc_dialect *d, const sc_field *f, sc_text *t)
{
	Walk w = walk(in, d, f);
	uint32_t c;
	int rc, wide = 0;

	memset(t, 0, sizeof *t);
	t->start = f->start;
	t->end = f->end;
	t->quoted = w.quoted;
	while ((rc = nextCharacter(&w, &c, &t->position)) > 0) {
		t->units++;
		wide |= c > 0xFF;
	}
	if (rc < 0) {
		t->code = SC_E_INVALID_UTF8;
		return;
	}
	t->position = 0;
	t->kind = t->units ? (wide ? SC_WIDE : SC_BYTES) : t->quoted ? SC_EMPTY_QUOTED : SC_EMPTY;
}

void
sc_text_copy(const sc_input *in, const sc_dialect *d, const sc_text *t, void *dest)
{
	sc_field f = { t->start, t->end, 0 };
	Walk w = walk(in, d, &f);
	uint32_t c;
	uint64_t at, i = 0;

	while (nextCharacter(&w, &c, &at) > 0) {
		if (t->kind == SC_WIDE) {
			((uint32_t *)dest)[i++] = c;
		} else {
			((uint8_t *)dest)[i++] = (uint8_t)c;
		}
	}
}

/* The text of a number, trimmed, into buffer (ASCII, NUL-terminated); 0 when
 * it cannot be one */
static int
numberText(const sc_input *in, const sc_dialect *d, const sc_field *f, char *buffer, size_t size)
{
	Walk w = walk(in, d, f);
	uint32_t u;
	size_t n = 0, i = 0;
	int odd = 0;

	while (nextUnit(&w, &u, &odd)) {
		if (u > 127 || u == 0 || n + 1 >= size) {
			return 0;
		}
		buffer[n++] = (char)u;
	}
	if (odd) {
		return 0;
	}
	while (n && (buffer[n - 1] == ' ' || buffer[n - 1] == '\t')) {
		n--;
	}
	buffer[n] = 0;
	while (buffer[i] == ' ' || buffer[i] == '\t') {
		i++;
	}
	memmove(buffer, buffer + i, n - i + 1);
	return buffer[0] != 0;
}

int
sc_to_integer(const sc_input *in, const sc_dialect *d, const sc_field *f, int64_t *value)
{
	char text[256], *p = text;
	int negative, digits = 0;
	int64_t v = 0;

	if (!numberText(in, d, f, text, sizeof text)) {
		return 0;
	}
	negative = *p == '-';
	if (*p == '-' || *p == '+') {
		p++;
	}
	if (!*p) {
		return 0;
	}
	for (; *p; p++) {
		if (*p < '0' || *p > '9' || (digits += digits || *p != '0') > 18) {
			return 0;
		}
		v = 10 * v + (*p - '0');
	}
	*value = negative ? -v : v;
	return 1;
}

int
sc_to_float(const sc_input *in, const sc_dialect *d, const sc_field *f, int comma, double *value)
{
	char text[256], *p = text;
	int negative, digits = 0, any = 0, e = 0, x = 0, xs = 1;
	uint64_t m = 0;
	double r;

	if (!numberText(in, d, f, text, sizeof text)) {
		return 0;
	}
	negative = *p == '-';
	if (*p == '-' || *p == '+') {
		p++;
	}
	for (; *p >= '0' && *p <= '9'; p++, any = 1) {
		if ((digits += digits || *p != '0') > 19) {
			return 0;
		}
		m = 10 * m + (uint64_t)(*p - '0');
	}
	if (*p == (comma ? ',' : '.')) {
		for (p++; *p >= '0' && *p <= '9'; p++, any = 1, e--) {
			if ((digits += digits || *p != '0') > 19) {
				return 0;
			}
			m = 10 * m + (uint64_t)(*p - '0');
		}
	}
	if (!any) {
		return 0;
	}
	if (*p == 'e' || *p == 'E') {
		p++;
		if (*p == '-' || *p == '+') {
			xs = *p++ == '-' ? -1 : 1;
		}
		if (!*p) {
			return 0;
		}
		for (; *p >= '0' && *p <= '9'; p++) {
			x = x > 9999 ? 99999 : 10 * x + (*p - '0');
		}
		e += xs * x;
	}
	if (*p || m > (1ULL << 53) || e < -22 || e > 22) {
		return 0;
	}
	for (r = 1; e > 0; e--) {
		r *= 10;
	}
	*value = (double)m * r;
	for (r = 1; e < 0; e++) {
		r *= 10;
	}
	*value /= r;
	if (negative) {
		*value = -*value;
	}
	return 1;
}
