# zlib of the Emscripten VM
#
# The files of the libraries that need zlib include this file: libpng
# (cmake/emscripten/deps/libpng.cmake), and so cairo, and cairo's PDF,
# PostScript and script surfaces (WASM_CAIRO_PDF).  It defines its target
# once, for whoever includes it first.  The target, pharo_zlib, is not in the
# registry of the FFI: no image binds zlib, which the libraries call
# directly.
#
# This file takes the pinned release archive (cmake/emscripten/deps/
# fetch.cmake), the one zlib.net and the release of GitHub give (signed by
# Mark Adler), and builds zlib as the static library pharo_zlib, with the
# flags of the deps directory: no CMake nor configure of zlib.  What their
# checks find that zconf.h uses, unistd.h and stdarg.h, is given here as
# zconf.h's own results (Z_HAVE_UNISTD_H, Z_HAVE_STDARG_H), to whoever
# includes its headers too, so that zlib and its users see the same zconf.h
# (gzread.c needs unistd.h's read).  Emscripten's port of zlib, from the
# archive of the tag v1.3.2, writes them into a zconf.h of its own.  (off_t
# is 64 bits wide on wasm64: no _LARGEFILE64_SOURCE.)

if(TARGET pharo_zlib)
    return()
endif()

set(WASM_ZLIB_VERSION "1.3.2")

pharo_wasm_dep_fetch(zlib
    VERSION ${WASM_ZLIB_VERSION}
    URL "https://github.com/madler/zlib/releases/download/v${WASM_ZLIB_VERSION}/zlib-${WASM_ZLIB_VERSION}.tar.gz"
    SHA256 bb329a0a2cd0274d05519d61c667c062e06990d72e125ee2dfa8de64f0119d16
    FILE zlib-${WASM_ZLIB_VERSION}.tar.gz
    LICENSES LICENSE)

# The library (the sources of zlib's own Makefile, without the examples)
set(PHARO_WASM_ZLIB_SOURCES "")
foreach(file
        adler32.c compress.c crc32.c deflate.c gzclose.c gzlib.c gzread.c
        gzwrite.c infback.c inffast.c inflate.c inftrees.c trees.c uncompr.c
        zutil.c)
    if(NOT EXISTS "${zlib_SOURCE_DIR}/${file}")
        message(FATAL_ERROR "${zlib_SOURCE_DIR} has no ${file}: it is not zlib ${WASM_ZLIB_VERSION}")
    endif()
    list(APPEND PHARO_WASM_ZLIB_SOURCES "${zlib_SOURCE_DIR}/${file}")
endforeach()

add_library(pharo_zlib STATIC ${PHARO_WASM_ZLIB_SOURCES})
target_include_directories(pharo_zlib PUBLIC "${zlib_SOURCE_DIR}")
target_compile_definitions(pharo_zlib PUBLIC Z_HAVE_UNISTD_H Z_HAVE_STDARG_H)
pharo_wasm_dep_flags(pharo_zlib -O2)
