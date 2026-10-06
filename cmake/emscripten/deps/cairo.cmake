# cairo of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file when cairo is on
# (PHARO_WASM_HAS_CAIRO, cmake/emscripten/deps/options.cmake), after
# FreeType, and the registry of the FFI links its target, pharo_cairo, into
# the VMs (cmake/emscripten/ffiLibraries.cmake), where the image's
# CairoLibrary (libcairo.so.2, so cairo) finds the functions Athens binds.
# The target's include directories are those of cairo's public headers, and,
# through the libraries it links PUBLIC, those of FreeType (which cairo-ft.h
# includes), pixman and libpng.
#
# This file takes the pinned release archive (cmake/emscripten/deps/
# fetch.cmake) and builds cairo as the static library pharo_cairo, with the
# flags of the deps directory and those of cairo's own build, but no meson,
# on pharo_pixman, pharo_png (and so pharo_zlib) and pharo_freetype, the
# FreeType of the image's fonts: one FreeType in the VM.  What meson made
# once (cmake/emscripten/deps/cairo/README says how) is committed next to
# this file, in cairo/, in two configurations:
#
#     config.h, cairo-features.h, sources.cmake
#         the image, recording, observer, SVG and mime surfaces, the
#         FreeType and user fonts, and PNG; no fontconfig (the image makes
#         its font faces from the FT_Face of its own fonts), no zlib
#     config-pdf.h, cairo-features-pdf.h, and sources-pdf.cmake too
#         with zlib (WASM_CAIRO_PDF): the PDF, PostScript and script
#         surfaces as well (cairo_pdf_surface_create, which the image binds
#         for AthensCairoPDFSurface)
#
# Both are meson's but for two lines, which leave out cairo's renderers of
# COLR v1 and OT-SVG colour glyphs: the VM's FreeType has neither.  The one
# chosen is copied into the build tree as config.h, which only cairo's
# sources include, and cairo-features.h, which cairo.h includes.
# cairo is built with its mutexes on pthread_mutex_* (CAIRO_HAS_PTHREAD):
# without -pthread they are Emscripten's single-thread stubs, and no
# -pthread reaches these objects (it would make the module's memory shared).
#
# The versions of the archive, and of the FreeType headers that cairo sees,
# must be those the configuration was made for, or the build stops: config.h
# says which FreeType API there is (HAVE_FT_COLR_V1, HAVE_FT_SVG_DOCUMENT,
# HAVE_FT_LOAD_NO_SVG), and another cairo or FreeType needs it made again.
#
# cairo is available under the LGPL 2.1 or the MPL 1.1, whose texts COPYING
# names: the notices give all three files (THIRD-PARTY-NOTICES.txt,
# cmake/emscripten/stage.cmake).

set(WASM_CAIRO_VERSION "1.18.4")
# The FreeType the configuration was made for
set(WASM_CAIRO_FREETYPE_VERSION "2.14.3")

if(NOT TARGET pharo_freetype)
    message(FATAL_ERROR "cmake/emscripten/deps/cairo.cmake: no pharo_freetype, which cairo needs (cmake/emscripten/deps/freetype.cmake)")
endif()
include(${CMAKE_CURRENT_LIST_DIR}/libpng.cmake)
include(${CMAKE_CURRENT_LIST_DIR}/pixman.cmake)

pharo_wasm_dep_fetch(cairo
    VERSION ${WASM_CAIRO_VERSION}
    URL "https://www.cairographics.org/releases/cairo-${WASM_CAIRO_VERSION}.tar.xz"
    SHA256 445ed8208a6e4823de1226a74ca319d3600e83f6369f99b14265006599c32ccb
    FILE cairo-${WASM_CAIRO_VERSION}.tar.xz
    LICENSES COPYING COPYING-LGPL-2.1 COPYING-MPL-1.1)
set(source "${cairo_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/cairo")

set(version "")
foreach(part MAJOR MINOR MICRO)
    file(STRINGS "${source}/src/cairo-version.h" line REGEX "^#define CAIRO_VERSION_${part} ")
    string(REGEX REPLACE "^#define CAIRO_VERSION_${part} +" "" line "${line}")
    list(APPEND version "${line}")
endforeach()
string(REPLACE ";" "." version "${version}")
if(NOT version STREQUAL WASM_CAIRO_VERSION)
    message(FATAL_ERROR "${source} is not cairo ${WASM_CAIRO_VERSION} (its cairo-version.h says ${version}), "
        "for which cmake/emscripten/deps/cairo/ was made: see cmake/emscripten/deps/cairo/README")
endif()

# The configuration, and its sources
include(${CMAKE_CURRENT_LIST_DIR}/cairo/sources.cmake)
set(files ${PHARO_WASM_CAIRO_SOURCES})
if(PHARO_WASM_HAS_CAIRO_PDF)
    include(${CMAKE_CURRENT_LIST_DIR}/cairo/sources-pdf.cmake)
    list(APPEND files ${PHARO_WASM_CAIRO_PDF_SOURCES})
    set(variant "-pdf")
else()
    set(variant "")
endif()
configure_file("${CMAKE_CURRENT_LIST_DIR}/cairo/config${variant}.h" "${dir}/config/config.h" COPYONLY)
configure_file("${CMAKE_CURRENT_LIST_DIR}/cairo/cairo-features${variant}.h" "${dir}/include/cairo-features.h" COPYONLY)
set(sources "")
foreach(file IN LISTS files)
    if(NOT EXISTS "${source}/src/${file}")
        message(FATAL_ERROR "${source} has no src/${file}: it is not cairo ${WASM_CAIRO_VERSION}")
    endif()
    list(APPEND sources "${source}/src/${file}")
endforeach()

# A source that stops the build unless the FreeType headers cairo sees are
# those of WASM_CAIRO_FREETYPE_VERSION
string(REPLACE "." ";" ft "${WASM_CAIRO_FREETYPE_VERSION}")
list(GET ft 0 ftMajor)
list(GET ft 1 ftMinor)
list(GET ft 2 ftPatch)
set(check "${dir}/pharo-cairo-freetype.c")
file(CONFIGURE OUTPUT "${check}" @ONLY CONTENT
"/* Generated by cmake/emscripten/deps/cairo.cmake.  Do not edit. */
#include <ft2build.h>
#include FT_FREETYPE_H

#if FREETYPE_MAJOR != @ftMajor@ || FREETYPE_MINOR != @ftMinor@ || FREETYPE_PATCH != @ftPatch@
#error \"cairo's configuration (cmake/emscripten/deps/cairo/config.h) is that of FreeType @WASM_CAIRO_FREETYPE_VERSION@, not of the FreeType of these headers: see cmake/emscripten/deps/cairo/README\"
#endif

typedef int pharoWasmCairoFreeTypeVersion;
")

add_library(pharo_cairo STATIC ${sources} "${check}")
target_include_directories(pharo_cairo
    PRIVATE "${dir}/config"
    PUBLIC "${dir}/include" "${source}/src")
target_compile_definitions(pharo_cairo PRIVATE
    CAIRO_COMPILATION _REENTRANT _GNU_SOURCE _FILE_OFFSET_BITS=64)
# (the options of cairo's meson.build)
target_compile_options(pharo_cairo PRIVATE
    -std=gnu11 -fno-strict-aliasing -fno-common -fvisibility=hidden)
target_link_libraries(pharo_cairo PUBLIC pharo_pixman pharo_png pharo_freetype)
pharo_wasm_dep_flags(pharo_cairo -O2)
