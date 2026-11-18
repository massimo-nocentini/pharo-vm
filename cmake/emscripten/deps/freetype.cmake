# FreeType of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file when FreeType is
# on (PHARO_WASM_HAS_FREETYPE, cmake/emscripten/deps/options.cmake), and the
# registry of the FFI links its target, pharo_freetype, into the VMs
# (cmake/emscripten/ffiLibraries.cmake), where the image's FT2FFILibrary
# (libfreetype.so.6, so freetype) finds the functions it binds.  The target's
# include directories are those of FreeType's public headers, for the table
# of the registry and for any other library built on FreeType.
#
# This file takes the pinned release archive (cmake/emscripten/deps/
# fetch.cmake) and builds FreeType as the static library pharo_freetype,
# with the flags of the deps directory: no CMake of FreeType, no zlib, png,
# bzip2, brotli nor harfbuzz.  Its sources are the amalgamated ones of the
# modules of cmake/emscripten/deps/freetype/ftmodule.h, which FreeType reads
# in place of its own list (FT_CONFIG_MODULES_H), and of base/: ftbase.c
# and the files of the optional API.  It is the library the images use:
# TrueType outlines, through the autofitter and the rasterisers.
#
# The archive is the release of savannah.gnu.org (signed by Werner
# Lemberg, whose key is cmake/cairo-stack/keys/freetype.asc), the one that
# the native VMs build too (cmake/cairo-stack/pins.txt, which
# scripts/cairo-stack/check-pins.sh compares with this file).  Emscripten's
# own port of FreeType is not used: it is older (VER-2-14-3 in emsdk 6.0.10).
#
# FreeType is available under the FreeType License (FTL) or the GPL 2; the
# VM takes it under the FTL, docs/FTL.TXT, which asks for a credit line in
# the documentation.  Besides, src/base/fthash.c, which ftbase.c includes,
# carries an MIT notice of its own (it comes from the BDF driver, which is
# not built here, nor is the PCF one).  Both go into THIRD-PARTY-NOTICES.txt
# (cmake/emscripten/stage.cmake) after FTL.TXT, from
# ${CMAKE_CURRENT_BINARY_DIR}/freetype/freetype-NOTICE.txt, which this file
# writes (with cmake/thirdPartyNotice.cmake's MIT check).
#
# The options are those of the archive's include/freetype/config/
# ftoption.h, but for the ones below, which this file comments out in a
# copy of it, freetype/config/ftoption.h of the include directories, ahead
# of the archive's.  Each must be defined exactly once in the archive's
# copy, or configure stops: another FreeType needs this file looked at
# again.
#
#     FT_CONFIG_OPTION_USE_ZLIB, FT_CONFIG_OPTION_USE_LZW
#         compressed fonts (WOFF, gzip and compress files): no zlib here
#     FT_CONFIG_OPTION_ADOBE_GLYPH_LIST
#         the 62 KB table that makes a Unicode charmap from the glyph names
#         of a font that has none (OpenType fonts have one)
#     FT_CONFIG_OPTION_MAC_FONTS, FT_CONFIG_OPTION_INCREMENTAL
#         Mac fonts (dfont, resource forks), and glyphs that a client such
#         as Ghostscript supplies through callbacks
#     FT_CONFIG_OPTION_SVG, TT_CONFIG_OPTION_COLOR_LAYERS,
#     TT_CONFIG_OPTION_EMBEDDED_BITMAPS, TT_CONFIG_OPTION_BDF
#         colour fonts (OT-SVG, COLR/CPAL), the bitmaps embedded in
#         TrueType and OpenType fonts, and the X11 'BDF ' table of .otb ones
#     TT_CONFIG_OPTION_GX_VAR_SUPPORT
#         variable fonts (FT_Get_MM_Var answers an error)
#     AF_CONFIG_OPTION_CJK, AF_CONFIG_OPTION_INDIC
#         the autofitter's CJK and Indic scripts
#
# The bytecode interpreter stays (TT_CONFIG_OPTION_BYTECODE_INTERPRETER): the
# full hinting, and the image's mono hinting, then give the glyphs of the
# native VMs.  The image's default, light hinting through the autofitter,
# gives them either way.
#
# FreeType needs more than the 64 KB of stack of Emscripten's default (the
# autofitter); the VM has WASM_STACK_SIZE (8 MB).  The rasterisers use
# setjmp, compiled in the mode of the VM (the deps directory's flags).
#
# The same file writes the copy of ftoption.h alone, for checking:
#
#     cmake -DFTOPTION_H=<ftoption.h> -DOUTPUT=<copy> -P freetype.cmake

set(WASM_FREETYPE_VERSION "2.15.0")
set(WASM_FREETYPE_OPTIONS_OFF
    FT_CONFIG_OPTION_USE_ZLIB
    FT_CONFIG_OPTION_USE_LZW
    FT_CONFIG_OPTION_ADOBE_GLYPH_LIST
    FT_CONFIG_OPTION_MAC_FONTS
    FT_CONFIG_OPTION_INCREMENTAL
    FT_CONFIG_OPTION_SVG
    TT_CONFIG_OPTION_COLOR_LAYERS
    TT_CONFIG_OPTION_EMBEDDED_BITMAPS
    TT_CONFIG_OPTION_BDF
    TT_CONFIG_OPTION_GX_VAR_SUPPORT
    AF_CONFIG_OPTION_CJK
    AF_CONFIG_OPTION_INDIC)

# pharo_wasm_freetype_options(<ftoption.h> <copy>): writes <copy>,
# <ftoption.h> with the options of WASM_FREETYPE_OPTIONS_OFF commented out,
# when its contents change
function(pharo_wasm_freetype_options ftoption_h output)
    file(READ "${ftoption_h}" text)
    foreach(option IN LISTS WASM_FREETYPE_OPTIONS_OFF)
        # The line '#define <option>', alone (FreeType writes no value)
        set(anchor "\n#define ${option}\n")
        string(FIND "${text}" "${anchor}" first)
        string(FIND "${text}" "${anchor}" last REVERSE)
        if(first EQUAL -1)
            message(FATAL_ERROR "${ftoption_h} does not define ${option} "
                "(cmake/emscripten/deps/freetype.cmake): it is not the ftoption.h of FreeType ${WASM_FREETYPE_VERSION}")
        endif()
        if(NOT first EQUAL last)
            message(FATAL_ERROR "${ftoption_h} defines ${option} more than once "
                "(cmake/emscripten/deps/freetype.cmake): it is not the ftoption.h of FreeType ${WASM_FREETYPE_VERSION}")
        endif()
        string(REPLACE "${anchor}"
            "\n/* #define ${option}  (off in Pharo: cmake/emscripten/deps/freetype.cmake) */\n"
            text "${text}")
    endforeach()
    file(WRITE "${output}.tmp" "${text}")
    configure_file("${output}.tmp" "${output}" COPYONLY)
    file(REMOVE "${output}.tmp")
endfunction()

if(CMAKE_SCRIPT_MODE_FILE)
    if(NOT FTOPTION_H OR NOT OUTPUT)
        message(FATAL_ERROR "freetype.cmake: FTOPTION_H and OUTPUT must be set")
    endif()
    pharo_wasm_freetype_options("${FTOPTION_H}" "${OUTPUT}")
    return()
endif()

pharo_wasm_dep_fetch(freetype
    VERSION ${WASM_FREETYPE_VERSION}
    URL "https://download.savannah.gnu.org/releases/freetype/freetype-${WASM_FREETYPE_VERSION}.tar.xz"
    SHA256 93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5
    FILE freetype-${WASM_FREETYPE_VERSION}.tar.xz
    LICENSES docs/FTL.TXT)
set(source "${freetype_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/freetype")

# The licence election, the FTL's credit line and the MIT notice of
# src/base/fthash.c, added to the notice that pharo_wasm_dep_fetch recorded
include(${CMAKE_CURRENT_LIST_DIR}/../../thirdPartyNotice.cmake)
pharo_third_party_notice_text(fthash freetype "${source}/src/base/fthash.c"
    MIT START "Copyright" END "*/")
# (the year of the release: the last of the copyright of freetype.h,
# "Copyright (C) 1996-2026 by")
file(STRINGS "${source}/include/freetype/freetype.h" year REGEX "Copyright \\(C\\) [0-9]+-[0-9]+ by" LIMIT_COUNT 1)
if(NOT year MATCHES "Copyright \\(C\\) [0-9]+-([0-9]+) by")
    message(FATAL_ERROR "${source}/include/freetype/freetype.h has no 'Copyright (C) <year>-<year> by': "
        "it is not FreeType ${WASM_FREETYPE_VERSION}")
endif()
set(year "${CMAKE_MATCH_1}")
file(CONFIGURE OUTPUT "${dir}/freetype-NOTICE.txt" @ONLY CONTENT
"FreeType is available under the FreeType License (FTL, docs/FTL.TXT above)
or the GNU General Public License version 2; this build uses it under the
FTL, whose credit line is:

    Portions of this software are copyright (C) @year@ The FreeType
    Project (https://freetype.org).  All rights reserved.

src/base/fthash.c, compiled into every FreeType through src/base/ftbase.c,
is under the MIT licence of its notice:

/*
 * @fthash@
")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${source}/src/base/fthash.c")
get_property(notices GLOBAL PROPERTY PHARO_WASM_NOTICES)
set(found OFF)
set(items "")
foreach(item IN LISTS notices)
    if(item MATCHES "^freetype\\|")
        string(APPEND item "|${dir}/freetype-NOTICE.txt")
        set(found ON)
    endif()
    list(APPEND items "${item}")
endforeach()
if(NOT found)
    message(FATAL_ERROR "cmake/emscripten/deps/freetype.cmake: pharo_wasm_dep_fetch recorded no notice of freetype")
endif()
set_property(GLOBAL PROPERTY PHARO_WASM_NOTICES "${items}")

# The sources: base/, then the modules of ftmodule.h
set(PHARO_WASM_FREETYPE_SOURCES "")
foreach(file
        base/ftsystem.c base/ftinit.c base/ftdebug.c base/ftbase.c
        base/ftbbox.c base/ftbitmap.c base/ftglyph.c base/ftmm.c
        base/ftsynth.c base/ftbdf.c base/fttype1.c base/ftfstype.c
        base/ftgasp.c base/ftstroke.c base/ftcid.c base/ftpatent.c
        base/ftotval.c
        autofit/autofit.c truetype/truetype.c sfnt/sfnt.c smooth/smooth.c
        raster/raster.c psnames/psnames.c cff/cff.c psaux/psaux.c
        pshinter/pshinter.c)
    if(NOT EXISTS "${source}/src/${file}")
        message(FATAL_ERROR "${source} has no src/${file}: it is not FreeType ${WASM_FREETYPE_VERSION}")
    endif()
    list(APPEND PHARO_WASM_FREETYPE_SOURCES "${source}/src/${file}")
endforeach()
foreach(file include/ft2build.h include/freetype/config/ftoption.h)
    if(NOT EXISTS "${source}/${file}")
        message(FATAL_ERROR "${source} has no ${file}: it is not FreeType ${WASM_FREETYPE_VERSION}")
    endif()
endforeach()

# ftoption.h, with the options above off.  (Configure runs again when this
# file changes, and so writes the copy again.)
pharo_wasm_freetype_options("${source}/include/freetype/config/ftoption.h"
    "${dir}/include/freetype/config/ftoption.h")

add_library(pharo_freetype STATIC ${PHARO_WASM_FREETYPE_SOURCES})
# The copy of ftoption.h comes before the archive's, for FreeType and for
# whoever includes its headers
target_include_directories(pharo_freetype
    PUBLIC "${dir}/include" "${source}/include"
    PRIVATE "${CMAKE_CURRENT_LIST_DIR}/freetype")
target_compile_definitions(pharo_freetype
    PRIVATE FT2_BUILD_LIBRARY "FT_CONFIG_MODULES_H=\"ftmodule.h\"")
pharo_wasm_dep_flags(pharo_freetype -O2)
