/* cmEngine.h -- the interface between the glue of the CMarkPlugin
 * (src/common/CMarkPlugin.c, the only file that includes VM headers) and its
 * engine (src/engine/cmEngine.c, cmText.c and cmTrack.c, which wrap
 * cmark-gfm 0.29.0.gfm.14).  It holds no VM type and no cmark type.
 *
 * docs/CMark.md, sections "Tree format" and "Primitive contract", is the
 * contract this interface serves: the statuses, the C limits, the record
 * layout of the tape (16 unsigned 32-bit words per node, in pre-order), the
 * sanitised UTF-8 text T that every result carries, and the one kept result.
 *
 * The engine never builds an object: the glue validates the arguments,
 * calls cm_parse or cm_render, checks the budget of object memory, allocates
 * and copies, then frees the result (cm_tree_free, cm_output_free) or hands
 * it to the kept result (cm_keep_tree, cm_keep_output) when the answer is
 * OVER_BUDGET or an allocation failed.  Everything the engine allocates is
 * plain C memory under the call's C budget; nothing of cmark outlives a call.
 *
 * cm_stats.overBudget is not counted by the engine (it never sees the budget
 * of object memory): the glue counts the OVER_BUDGET answers itself.
 */

#ifndef CM_ENGINE_H
#define CM_ENGINE_H

#include <stddef.h>
#include <stdint.h>
#if defined(__GNUC__) || defined(__clang__)
# define CM_API __attribute__((visibility("hidden")))   /* never exported from libCMarkPlugin.so */
#else
# define CM_API
#endif
enum { CM_PROTOCOL = 1 };
enum { CM_OK = 0, CM_OVER_BUDGET = 1, CM_TOO_LARGE = 2, CM_TOO_DEEP = 3, CM_C_LIMIT = 4 };
enum { CM_CLIM_BUDGET = 1, CM_CLIM_BLOCK = 2, CM_CLIM_MALLOC = 3 };
enum { CM_SRC_UTF8 = 0, CM_SRC_LATIN1 = 1, CM_SRC_UTF32 = 2 };
enum { CM_FMT_HTML = 1, CM_FMT_XML, CM_FMT_COMMONMARK, CM_FMT_PLAINTEXT, CM_FMT_LATEX, CM_FMT_MAN };
#define CM_OPT_ALLOWED 0x3FF1E
#define CM_EXT_ALL 31
enum { CM_REC_WORDS = 16 };
/* record word indexes, 0-based in C: */
enum { CM_W_CODE, CM_W_PARENT, CM_W_FLAGS, CM_W_SL, CM_W_SC, CM_W_EL, CM_W_EC, CM_W_SI, CM_W_EI,
       CM_W_INT1, CM_W_INT2, CM_W_INT3, CM_W_S1OFF, CM_W_S1LEN, CM_W_S2OFF, CM_W_S2LEN };
enum { CM_F_STR1_ASCII = 1, CM_F_STR2_ASCII = 2, CM_F_HAS_POS = 4, CM_F_TIGHT = 8, CM_F_FENCED = 16,
       CM_F_HEADER = 32, CM_F_TASK = 64, CM_F_CHECKED = 128, CM_F_EXTENSION = 256, CM_F_CLAMPED = 512 };
enum { CM_CODE_TABLE = 0x8100, CM_CODE_TABLE_ROW = 0x8101, CM_CODE_TABLE_CELL = 0x8102,
       CM_CODE_UNKNOWN_BLOCK = 0x80ff, CM_CODE_STRIKETHROUGH = 0xc100, CM_CODE_UNKNOWN_INLINE = 0xc0ff };
#define CM_MAX_INPUT_BYTES ((size_t)256 << 20)
#define CM_MAX_BLOCK_BYTES ((size_t)256 << 20)
CM_API uint64_t cm_default_c_budget(void);   /* UINT64_C(4) << 30 native, UINT64_C(128) << 20 __EMSCRIPTEN__ */
CM_API uint64_t cm_keep_cap(void);           /* UINT64_C(4) << 30 native, UINT64_C(8) << 20 __EMSCRIPTEN__ */

typedef struct { int kind; const void *units; size_t count; } cm_source;  /* count: bytes or 32-bit words */
/* T (the sanitised UTF-8, 1.8) travels with every result: it is the kept result's key and the `sanitised` slot. */
typedef struct { uint8_t *text; size_t textBytes; int differs; } cm_text;  /* differs: T != the source's bytes */
typedef struct { uint32_t *tape; size_t nodes; uint8_t *strings; size_t stringBytes;
                 uint32_t depth; size_t sourceChars; cm_text t; int reused; } cm_tree;
typedef struct { uint8_t *bytes; size_t size; int ascii; cm_text t; int reused; } cm_output;
typedef struct { uint64_t parses, renders, reused, overBudget, tooLarge, tooDeep, cLimit; } cm_stats;

CM_API int  cm_init(void);                     /* 0 ok, -1 an extension is missing; idempotent */
CM_API const char *cm_version(void);           /* "0.29.0.gfm.14" (cmark_version_string()) */
CM_API int  cm_extension_count(void);          /* the found ones */
CM_API const char *cm_extension_name(int i);   /* in bit order */
CM_API int  cm_extensions_present(int mask);   /* 1 when every set bit's extension was found */
/* Parse. First computes T; when the kept tree's key (T, options, extensions, maxDepth) matches, moves it into
   *out (out->reused = 1) without parsing; otherwise frees the kept result and parses. On CM_OK fills *out
   (engine-owned until cm_tree_free or cm_keep_tree). On CM_TOO_DEEP, out->depth and out->sourceChars are set.
   *code as in 1.1. */
CM_API int  cm_parse(const cm_source *src, int options, int extensions, uint32_t maxDepth,
                     int64_t cBudget, cm_tree *out, uint64_t *code);
CM_API int  cm_render(const cm_source *src, int format, int options, int extensions, int width,
                      int64_t cBudget, cm_output *out, uint64_t *code);   /* the same reuse rule */
CM_API void cm_tree_free(cm_tree *t);
CM_API void cm_output_free(cm_output *o);     /* frees the detached output block (1.9) and T */
/* The kept result (1.7): keep takes ownership; it frees instead when the result is over cm_keep_cap() or the
   bookkeeping cannot be allocated. */
CM_API void cm_keep_tree(int options, int extensions, uint32_t maxDepth, cm_tree *t);
CM_API void cm_keep_output(int format, int options, int extensions, int width, cm_output *o);
CM_API size_t cm_release_kept(void);           /* bytes freed */
CM_API size_t cm_kept_bytes(void);
CM_API const cm_stats *cm_get_stats(void);
#ifdef CM_TESTING                              /* only the enginetest and glue lanes define it */
CM_API void cm_test_limits(size_t maxInput, size_t maxBlock);   /* lower the limits; 0 restores */
CM_API void cm_test_drop_extension(int bit);   /* pretend cm_init did not find it; -1 restores */
CM_API size_t cm_track_live_blocks(void);      /* 0 outside a call */
/* Test-only additions (SPEC DEVIATION, docs: DEVIATIONS.md WP-A-1): enginetest needs a lower keep cap (2.4
   item 13) and nodes the parser never makes (2.4 item 4: CUSTOM_BLOCK, CUSTOM_INLINE, an unknown block type
   and an unknown inline type). */
CM_API void cm_test_keep_cap(uint64_t cap);    /* lower cm_keep_cap(); 0 restores */
CM_API void cm_test_extra_nodes(int on);       /* 1: cm_parse appends those four nodes to the document */
#endif

#endif /* CM_ENGINE_H */
