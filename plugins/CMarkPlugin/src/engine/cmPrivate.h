/* cmPrivate.h -- what the three files of the engine share and nothing else
 * sees: the call context and the tracking allocator (cmTrack.c), the
 * sanitised text and its position tables (cmText.c).  cmEngine.c is the
 * only file of the engine that includes cmark headers (rule R2), so this
 * header holds no cmark type: the tracker's cmark_mem travels as a void *.
 *
 * Every function here is CM_API (hidden visibility): visible to the other
 * files of the engine, never exported from libCMarkPlugin.so.
 */

#ifndef CM_PRIVATE_H
#define CM_PRIVATE_H

#include "cmEngine.h"

#ifdef CM_TESTING
# include <assert.h>
# define CM_ASSERT(x) assert(x)
#else
# define CM_ASSERT(x) ((void)0)
#endif

/*** The call context and the tracker (cmTrack.c) ***/

/* One primitive call of the engine.  Only cmEngine.c fills it; the tracker
 * reads budget and keeps the counts and the landing's code. */
typedef struct cm_call {
	uint64_t budget;	/* the C bytes the call may hold at once */
	uint64_t kept;		/* the bytes of the kept result, charged to the call */
	int limit;		/* after a landing: CM_CLIM_BUDGET, _BLOCK or _MALLOC */
	void *parser;		/* the cmark_parser, once cmark_parser_new_with_mem returned */
	void *job;		/* cmEngine.c's own state of the call */
} cm_call;

/* cm_guarded answers this after a landing (a longjmp of the tracker) */
#define CM_LANDED (-1)

/* Starts and ends a call: the tracker is active (and charges every block to
 * ctx->budget) only between the two.  A call during a call is a bug. */
CM_API void cm_track_begin(cm_call *ctx);
CM_API void cm_track_end(void);
/* Runs work(ctx) behind the call's only setjmp, and answers what it answers,
 * or CM_LANDED when a request over a C limit longjmped back (ctx->limit says
 * which).  Then every tracked block is still allocated: the caller restores
 * cmark's static tables first (cmEngine.c), then sweeps. */
CM_API int cm_guarded(int (*work)(cm_call *), cm_call *ctx);
/* The cmark_mem (a struct of cmark-gfm.h) whose blocks the tracker keeps */
CM_API void *cm_track_allocator(void);
/* Leaves the call with ctx->limit = code (only during cm_guarded's work) */
CM_API void cm_track_fail(int code);
/* Lowers the cap of one block of cmark (0 restores CM_MAX_BLOCK_BYTES) */
CM_API void cm_track_set_max_block(size_t maxBlock);
/* Moves the kept result's charge (the kept result was freed or taken) */
CM_API void cm_track_set_kept(uint64_t keptBytes);

/* The engine's own blocks (T, the position tables, the tape, the strings,
 * the parent stack): plain malloc, charged, in a list of their own.
 * Failing requests longjmp like cmark's, so they too may only be made
 * during cm_guarded's work. */
CM_API void *cm_own_alloc(size_t size);
CM_API void *cm_own_realloc(void *block, size_t size);
CM_API void cm_own_free(void *block);
/* Moves a block of cmark (the output of a renderer) to the engine's list */
CM_API void cm_track_detach(void *block);
/* Takes a block of the engine's list out of the call: the result's owner
 * frees it later with cm_block_free, outside any call */
CM_API void cm_own_keep(void *block);
CM_API void cm_block_free(void *block);
/* The bytes a block takes, its header included */
CM_API size_t cm_block_bytes(const void *block);
/* Frees every block of cmark, then every block of the engine's list, and
 * answers how many blocks of cmark there were */
CM_API size_t cm_track_sweep(void);
CM_API size_t cm_own_sweep(void);
/* The blocks of both lists */
CM_API size_t cm_track_blocks(void);


/*** The sanitised text and its positions (cmText.c) ***/

/* The exact size of T for src, whether T differs from the bytes of a
 * ByteArray source, and the code points of T */
CM_API size_t cm_text_measure(const cm_source *src, int *differs, size_t *chars);
/* Writes T (of the size cm_text_measure answered) into out */
CM_API void cm_text_fill(const cm_source *src, uint8_t *out);

/* The lines of T and its character index (1.8): lineStart(64j + 1) and
 * cont(64j) every 64 lines and 64 bytes, and a few cursors (the last lines
 * resolved, with their start and end), so that the start of a block's
 * children after the block's end (further down) costs no rescan */
enum { CM_CURSORS = 4 };
typedef struct {
	size_t line;		/* 0: unused */
	size_t start;		/* lineStart(line) */
	size_t end;		/* lineStart(line + 1), or |T| */
	size_t used;		/* the lookup that last used it */
} cm_cursor;
typedef struct {
	const uint8_t *t;
	size_t n;
	size_t lines;		/* line starts, the phantom line L+1 included */
	uint32_t *lineMarks;	/* lineStart(64j + 1), 0-based byte offsets */
	uint32_t *contMarks;	/* cont(64j): continuation bytes in T[0, 64j) */
	cm_cursor cursors[CM_CURSORS];
	size_t lookups;
} cm_lines;

/* Builds the tables (cm_own_alloc) */
CM_API void cm_lines_init(cm_lines *l, const uint8_t *t, size_t n);
/* The character indexes of cmark's (sl:sc-el:ec) of a node whose sl and sc
 * are at least 1, into *si and *ei; answers 1 when a position was clamped */
CM_API int cm_lines_span(cm_lines *l, int sl, int sc, int el, int ec, uint32_t *si, uint32_t *ei);

#endif /* CM_PRIVATE_H */
