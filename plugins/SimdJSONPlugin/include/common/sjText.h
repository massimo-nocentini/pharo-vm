/* sjText.h -- the pure C99 helpers of the SimdJSONPlugin (sjText.c): the
 * value-end scanner, transcoding of Smalltalk sources to UTF-8 and of
 * UTF-8 strings back to Smalltalk characters, the exponent gate and the
 * number-token enumerator, Spur object sizes, and the key set.
 *
 * No VM header and no simdjson header: the glue (C), the engine (C++) and
 * the test harness all use these functions.  JSON white space is the
 * space, tab, line feed, carriage return and form feed (NeoJSON's
 * isSeparator).
 */

#ifndef SJ_TEXT_H
#define SJ_TEXT_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

/* width: 1 for byte sources (UTF-8 or Latin-1), 4 for UTF-32 sources */
size_t   sj_skip_ws(const void *src, int width, size_t from, size_t stop);   /* first non-ws in [from,stop), else stop */
size_t   sj_trim_ws(const void *src, int width, size_t from, size_t stop);   /* one past the last non-ws in [from,stop), >= from */
size_t   sj_value_end(const void *src, int width, size_t from, size_t stop); /* requires from < stop */
int      sj_is_bom(const void *src, int encoding, size_t at, size_t stop);   /* EF BB BF (UTF-8) or U+FEFF (UTF-32) */
size_t   sj_utf8_length(const void *src, int encoding, size_t from, size_t to);
void     sj_transcode(const void *src, int encoding, size_t from, size_t to, char *dst); /* writes sj_utf8_length bytes */
void     sj_utf8_scan(const char *s, size_t n, size_t *chars, uint32_t *maxcp); /* input validated by simdjson */
void     sj_utf8_to_latin1(const char *s, size_t n, uint8_t *dst);
void     sj_utf8_to_utf32(const char *s, size_t n, uint32_t *dst);
int      sj_exponent_gate(const char *json, size_t n);  /* 1 iff some byte e/E has a digit before it and a digit, '+' or '-' after it */
typedef struct { uint32_t ordinal, offset, length; } sj_token;
int64_t  sj_number_tokens(const char *json, size_t n, sj_token **list, size_t *count, size_t *cap);
         /* valid JSON only: answers the count of ALL number tokens in document order and collects
            (ordinal, offset, length) of those containing e/E into *list (malloc/realloc); -1 on C OOM */
int      sj_is_small_float(double d);                   /* SmallFloat64 range, as the VM's isSmallFloatValue */
uint64_t sj_spur_pointers(uint64_t slots);              /* 8 B header, +8 B from 255 slots, at least 1 slot */
uint64_t sj_spur_bytes(uint64_t nbytes);                /* byte-indexable object of nbytes */
uint64_t sj_spur_words32(uint64_t nwords);              /* 32-bit-word object (WideString) */
size_t   sj_dict_capacity(size_t npairs);               /* smallest prime >= max(5, (npairs+1)*4/3) */
uint64_t sj_bound(uint64_t utf8Bytes);                  /* 32 * utf8Bytes + 4096 */
typedef struct sj_keyset sj_keyset;                     /* open addressing on UTF-8 bytes, malloc only */
sj_keyset *sj_keyset_new(size_t hint);                  /* NULL on C OOM */
void       sj_keyset_free(sj_keyset *k);
int64_t    sj_keyset_index(sj_keyset *k, const char *utf8, size_t n, int *isNew);  /* 1-based; -1 on C OOM */

#ifdef __cplusplus
}
#endif
#endif
