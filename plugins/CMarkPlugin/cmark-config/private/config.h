/* config.h -- our replacement of cmark-gfm 0.29.0.gfm.14's src/config.h.in,
 * which upstream's CMake configures (CMake is never run here: the flags are
 * ours, cmake/cmark.cmake).  The values are the ones upstream's CMake finds
 * with gcc, clang (MinGW and MSYS2 clang included) and emcc: stdbool.h,
 * __builtin_expect and __attribute__.  The MSVC < 2015 snprintf fallback of
 * config.h.in is left out (VS 2015 and later only).
 *
 * PRIVATE to the cmark-gfm sources (target pharo_cmark): the VM's headers
 * include "config.h" and mean the VM's own (sqConfig.h, pharo.h), so neither
 * the glue nor the engine may ever have this directory on its include path
 * (rule R9 of tests/run-c-tests.sh).
 */
#ifndef CMARK_CONFIG_H
#define CMARK_CONFIG_H

#ifdef __cplusplus
extern "C" {
#endif

#define HAVE_STDBOOL_H

#ifdef HAVE_STDBOOL_H
  #include <stdbool.h>
#elif !defined(__cplusplus)
  typedef char bool;
#endif

#if defined(__GNUC__) || defined(__clang__)
  #define HAVE___BUILTIN_EXPECT
  #define HAVE___ATTRIBUTE__
#endif

#ifdef HAVE___ATTRIBUTE__
  #define CMARK_ATTRIBUTE(list) __attribute__ (list)
#else
  #define CMARK_ATTRIBUTE(list)
#endif

#ifndef CMARK_INLINE
  #if defined(_MSC_VER) && !defined(__cplusplus)
    #define CMARK_INLINE __inline
  #else
    #define CMARK_INLINE inline
  #endif
#endif

#ifdef __cplusplus
}
#endif

#endif
