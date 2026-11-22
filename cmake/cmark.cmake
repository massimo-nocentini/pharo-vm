# cmark-gfm for the CMarkPlugin: GitHub's fork of the CommonMark reference
# parser, release 0.29.0.gfm.14, referenced by the URL of its tag archive and
# that archive's SHA256, compiled from the 27 files of its library (src/,
# without the command line program main.c) and the 7 files of its GFM
# extensions (extensions/: table, strikethrough, autolink, tagfilter,
# tasklist, their scanners and their registration) as the static library
# pharo_cmark.  The archive's COPYING is the notice: cmark-gfm is
# BSD-2-Clause, with houdini, buffer and the utf8proc tables under MIT, all
# in that one file (docs/CMark.md, Licences).
#
# The archive is taken as simdjson's is (cmake/simdjson.cmake): from
# WASM_DEPS_DIR, then <build>/downloads, otherwise downloaded.
# -DCMARK_SOURCE_DIR=<tree> uses a source tree instead (an extraction of the
# same archive, or a clone of the tag).  No patch is applied: the known bugs
# of gfm.14 are documented and pinned by the C tests (docs/CMark.md,
# Limitations).
#
# cmark-gfm's own CMakeLists.txt is never run: the flags are ours.
#   -O3 -g0  as cmake/simdjson.cmake: target options come after the -O2 and
#         the -g of the VM's top CMakeLists.txt, so they win
#   C_STANDARD 99, C_EXTENSIONS OFF  upstream's -std=c99 (src/CMakeLists.txt),
#         as target properties, so that CMake writes the one -std; no
#         -pedantic -Werror here: the C tests of the plugin build cmark-gfm
#         with them (plugins/CMarkPlugin/tests/run-c-tests.sh)
#   -DNDEBUG  upstream's Release does the same (29 assert()s); never
#         CMARK_DEBUG_NODES, which aborts on a failed tree check
#   CMARK_GFM_STATIC_DEFINE, CMARK_NO_SHORT_NAMES, PUBLIC: what every file
#         that includes cmark-gfm.h (the engine of the plugin) must see the
#         same way; the second hides the NODE_* short macros
#   the headers CMake would generate: plugins/CMarkPlugin/cmark-config,
#         written by hand.  private/config.h is PRIVATE: the VM's headers
#         include "config.h" and mean the VM's own (sqConfig.h, pharo.h), so
#         the glue must never see this one, whatever the include order.
#         public/ (cmark-gfm_export.h with empty macros, cmark-gfm_version.h)
#         is PUBLIC with src/ and extensions/, because the engine includes
#         cmark-gfm.h, cmark-gfm-core-extensions.h, table.h and
#         strikethrough.h; the extensions include src/'s private headers
#         with <>, so src/ is a -I directory
#   C only, and on Emscripten CMAKE_C_FLAGS give -m64 and the setjmp/longjmp
#         flags already (cmake/Emscripten.cache.cmake, cmake/Emscripten.cmake):
#         the engine longjmps out of cmark-gfm's frames on its C limits
#   PIC natively (the plugin is a shared library); the archive goes to deps/,
#         not build/vm/, which cmake/Linux.cmake installs whole
#   hidden visibility: the plugin keeps a private copy of cmark-gfm, which the
#         VM's dlopen with RTLD_GLOBAL|RTLD_DEEPBIND (src/externalPrimitives.c)
#         then cannot mix up; libCMarkPlugin exports only its primitives and
#         its module functions.  On Emscripten every builtin plugin shares
#         one module: cmark-gfm's globals without the cmark_ prefix
#         (houdini_*, _scan_*, _ext_scan_at, create_*_extension,
#         normalize_map_label) must not clash with another archive's
#         (docs/CMark.md; lane 75 checks it)

include_guard(GLOBAL)

set(PHARO_CMARK_VERSION 0.29.0.gfm.14)
set(PHARO_CMARK_SHA256 1c1acf50c5e9939e170b6e017ec06fcf94abeb383670b819d091a285b7ba11fe)
set(PHARO_CMARK_URL "https://github.com/github/cmark-gfm/archive/refs/tags/${PHARO_CMARK_VERSION}.tar.gz")
set(CMARK_SOURCE_DIR "" CACHE PATH
    "A cmark-gfm ${PHARO_CMARK_VERSION} source tree to use instead of the pinned archive")

if(CMARK_SOURCE_DIR)
    get_filename_component(cmark-gfm_SOURCE_DIR "${CMARK_SOURCE_DIR}" ABSOLUTE)
    if(NOT EXISTS "${cmark-gfm_SOURCE_DIR}/COPYING")
        message(FATAL_ERROR "CMarkPlugin: ${cmark-gfm_SOURCE_DIR} has no COPYING")
    endif()
    if(EMSCRIPTEN)    # what pharo_wasm_dep_fetch would have recorded
        set_property(GLOBAL APPEND PROPERTY PHARO_WASM_NOTICES
            "cmark-gfm|${PHARO_CMARK_VERSION}|${cmark-gfm_SOURCE_DIR}|local tree|${cmark-gfm_SOURCE_DIR}/COPYING")
    endif()
else()
    if(NOT COMMAND pharo_wasm_dep_fetch)       # Emscripten has it already (cmake/emscripten/deps)
        include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/fetch.cmake)
    endif()
    pharo_wasm_dep_fetch(cmark-gfm
        VERSION ${PHARO_CMARK_VERSION}
        URL "${PHARO_CMARK_URL}"
        SHA256 ${PHARO_CMARK_SHA256}
        FILE cmark-gfm-${PHARO_CMARK_VERSION}.tar.gz
        LICENSES COPYING)
endif()

# The tree must be the pin: its version template, and the version lines of
# its top CMakeLists.txt (lines 5 and 7 at the tag)
if(NOT EXISTS "${cmark-gfm_SOURCE_DIR}/src/cmark-gfm_version.h.in")
    message(FATAL_ERROR "CMarkPlugin: ${cmark-gfm_SOURCE_DIR} has no src/cmark-gfm_version.h.in")
endif()
file(STRINGS "${cmark-gfm_SOURCE_DIR}/CMakeLists.txt" pharo_cmark_marks
     REGEX "^set\\(PROJECT_VERSION_(MINOR 29|GFM 14)\\)$")
list(LENGTH pharo_cmark_marks pharo_cmark_mark_count)
if(NOT pharo_cmark_mark_count EQUAL 2)
    message(FATAL_ERROR "CMarkPlugin: ${cmark-gfm_SOURCE_DIR} is not cmark-gfm ${PHARO_CMARK_VERSION} "
        "(its CMakeLists.txt has not set(PROJECT_VERSION_MINOR 29) and set(PROJECT_VERSION_GFM 14))")
endif()

set(pharo_cmark_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/CMarkPlugin)
set(pharo_cmark_sources "")
# src/CMakeLists.txt, LIBRARY_SOURCES (scanners.re is the re2c source of
# scanners.c, which ships generated)
foreach(pharo_cmark_file IN ITEMS
        cmark.c node.c iterator.c blocks.c inlines.c scanners.c utf8.c buffer.c
        references.c footnotes.c map.c render.c man.c xml.c html.c commonmark.c
        plaintext.c latex.c houdini_href_e.c houdini_html_e.c houdini_html_u.c
        cmark_ctype.c arena.c linked_list.c syntax_extension.c registry.c plugin.c)
    list(APPEND pharo_cmark_sources ${cmark-gfm_SOURCE_DIR}/src/${pharo_cmark_file})
endforeach()
# extensions/CMakeLists.txt, LIBRARY_SOURCES
foreach(pharo_cmark_file IN ITEMS
        core-extensions.c table.c strikethrough.c autolink.c tagfilter.c
        ext_scanners.c tasklist.c)
    list(APPEND pharo_cmark_sources ${cmark-gfm_SOURCE_DIR}/extensions/${pharo_cmark_file})
endforeach()
foreach(pharo_cmark_file IN LISTS pharo_cmark_sources)
    if(NOT EXISTS "${pharo_cmark_file}")
        message(FATAL_ERROR "CMarkPlugin: no ${pharo_cmark_file}")
    endif()
endforeach()

add_library(pharo_cmark STATIC ${pharo_cmark_sources})
target_include_directories(pharo_cmark
    PRIVATE ${pharo_cmark_dir}/cmark-config/private
    PUBLIC ${pharo_cmark_dir}/cmark-config/public
           ${cmark-gfm_SOURCE_DIR}/src
           ${cmark-gfm_SOURCE_DIR}/extensions)
target_compile_definitions(pharo_cmark
    PRIVATE NDEBUG
    PUBLIC CMARK_GFM_STATIC_DEFINE CMARK_NO_SHORT_NAMES)
target_compile_options(pharo_cmark PRIVATE -O3 -g0)
set_target_properties(pharo_cmark PROPERTIES
    C_STANDARD 99 C_EXTENSIONS OFF
    ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/deps
    C_VISIBILITY_PRESET hidden)
if(NOT EMSCRIPTEN)
    set_target_properties(pharo_cmark PROPERTIES POSITION_INDEPENDENT_CODE ON)
    # BSD-2-Clause and MIT (COPYING): next to the plugin on Linux and Windows
    # (build/vm/, packaged whole), in Contents/Resources of the bundle on
    # macOS, as timsort's.  On Emscripten LICENSES COPYING puts it in
    # wasm/THIRD-PARTY-NOTICES.txt (cmake/emscripten/stage.cmake)
    if(OSX)
        set(PHARO_CMARK_LICENSE_DIR "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(PHARO_CMARK_LICENSE_DIR "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    configure_file(${cmark-gfm_SOURCE_DIR}/COPYING ${PHARO_CMARK_LICENSE_DIR}/LICENSE.cmark-gfm COPYONLY)
endif()
