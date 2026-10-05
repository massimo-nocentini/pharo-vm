/*
 * The configuration of libgit2 1.4.4 for the Emscripten VM, which the
 * sources include as "git2/sys/features.h" (cmake/emscripten/deps/
 * libgit2.cmake): what libgit2's CMake wrote from its src/features.h.in, with
 * the options of cmake/emscripten/deps/libgit2/sources.cmake.  No threads,
 * SSH, HTTPS, NTLM, GSSAPI nor iconv; SHA-1 with collision detection
 * (SHA1DC), the regcomp of the C library, and getentropy for random numbers.
 */
#ifndef INCLUDE_features_h__
#define INCLUDE_features_h__

/* #undef GIT_DEBUG_POOL */
/* #undef GIT_DEBUG_STRICT_ALLOC */
/* #undef GIT_DEBUG_STRICT_OPEN */

/* #undef GIT_THREADS */
/* #undef GIT_WIN32_LEAKCHECK */

#define GIT_ARCH_64 1
/* #undef GIT_ARCH_32 */

/* #undef GIT_USE_ICONV */
#define GIT_USE_NSEC 1
#define GIT_USE_STAT_MTIM 1
/* #undef GIT_USE_STAT_MTIMESPEC */
/* #undef GIT_USE_STAT_MTIME_NSEC */
#define GIT_USE_FUTIMENS 1

/* #undef GIT_REGEX_REGCOMP_L */
#define GIT_REGEX_REGCOMP
/* #undef GIT_REGEX_PCRE */
/* #undef GIT_REGEX_PCRE2 */
/* #undef GIT_REGEX_BUILTIN */

/* #undef GIT_QSORT_R_BSD */
#define GIT_QSORT_R_GNU
/* #undef GIT_QSORT_S */

/* #undef GIT_SSH */
/* #undef GIT_SSH_MEMORY_CREDENTIALS */

/* #undef GIT_NTLM */
/* #undef GIT_GSSAPI */
/* #undef GIT_GSSFRAMEWORK */

/* #undef GIT_WINHTTP */
/* #undef GIT_HTTPS */
/* #undef GIT_OPENSSL */
/* #undef GIT_OPENSSL_DYNAMIC */
/* #undef GIT_SECURE_TRANSPORT */
/* #undef GIT_MBEDTLS */

#define GIT_SHA1_COLLISIONDETECT 1
/* #undef GIT_SHA1_WIN32 */
/* #undef GIT_SHA1_COMMON_CRYPTO */
/* #undef GIT_SHA1_OPENSSL */
/* #undef GIT_SHA1_MBEDTLS */

#define GIT_RAND_GETENTROPY 1

#endif
