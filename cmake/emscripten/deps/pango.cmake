# Pango, and what it needs, for the Emscripten VM (WASM_PANGO)
#
# cmake/emscripten/deps/CMakeLists.txt includes this file when Pango is on
# (PHARO_WASM_HAS_PANGO, cmake/emscripten/deps/options.cmake), after cairo,
# which it then builds with fontconfig (cairo.cmake).  It builds, from the
# pinned archives of the native VMs' cairo stack (cmake/cairo-stack/
# pins.txt: the same versions and SHA256), the static libraries
#
#     pharo_expat       expat 2.11.0, for fontconfig
#     pharo_fontconfig  fontconfig 2.18.4, the font map of Pango, cairo's
#                       toy fonts and Poppler's substitution of fonts
#     pharo_glib        GLib 2.90.1: libglib, with libcharset
#     pharo_gobject     GObject, on the VM's libffi (pharo_libffi)
#     pharo_gmodule     GModule (GIO's modules, which there are none of)
#     pharo_gio         GIO (GListModel of the font maps, GFile and streams
#                       of poppler-glib)
#     pharo_fribidi     FriBidi 1.0.18
#     pharo_harfbuzz    HarfBuzz 15.1.0, on the VM's FreeType
#     pharo_pango       Pango 1.58.2, pangoft2 (fontconfig and FreeType) and
#                       PangoCairo
#
# and the font directory of Pango, which the VMs carry (below).  Each is made
# by pharo_wasm_harvested_library (harvested.cmake) from the sources.cmake
# and gen/ of its directory here, and patched first by the patches of its
# patches/ directory: scripts/wasm-deps/regen-text-stack.sh says how they
# were made, and makes them again.  The libraries are compiled with -O2 (the
# native stack's optimisation), except GIO, which only a few calls reach,
# with -Os.
#
# The registry of the FFI (ffiLibraries.cmake) lists the functions the
# bindings call: glib, gobject, harfbuzz, pango and pangocairo.
#
# The VM has no threads: GLib runs on Emscripten's single-thread stubs of
# pthread_*, and Pango's fontconfig thread does its work synchronously
# (pango/patches).  WebAssembly traps on a call through a function pointer
# of another type, which GLib and Pango do: the patches give those calls
# functions of the right type.
#
# The fonts: Pango and Poppler find their fonts through fontconfig, which
# reads the fonts.conf of FONTCONFIG_FILE.  The VMs carry the DejaVu fonts
# (Sans and Serif, regular and bold, and Sans Mono: 2.5 MB, 1.3 MB gzipped;
# the other faces are synthesized: slanted, or emboldened, which a bold face
# of its own is not, since synthesized bold glyphs are not wider) with the
# fonts.conf of
# fonts/ here, which maps sans-serif, serif, monospace and the base-14 fonts
# of PDF to them: the web VM in its file system (--embed-file, at
# /usr/share/fonts/pharo), the node VM next to it, in node/fonts
# (cmake/emscripten/stage.cmake).  src/emscripten/textSupport.c sets
# FONTCONFIG_FILE when the image first loads Pango or Poppler, unless it is
# set.

if(NOT TARGET pharo_cairo OR NOT TARGET pharo_libffi)
    message(FATAL_ERROR "cmake/emscripten/deps/pango.cmake: no pharo_cairo or pharo_libffi, which Pango needs")
endif()
include(${CMAKE_CURRENT_LIST_DIR}/harvested.cmake)

# The pinned archive <name> of cmake/cairo-stack/pins.txt: its version, URL,
# SHA256, file and licence files, as pharo_wasm_pin_<field>
function(pharo_wasm_pin name)
    file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/../../cairo-stack/pins.txt" lines REGEX "^${name} ")
    foreach(line IN LISTS lines)
        string(REGEX REPLACE "[ \t]+" ";" fields "${line}")
        list(LENGTH fields count)
        if(count GREATER_EQUAL 6)
            list(GET fields 1 version)
            list(GET fields 2 sha)
            list(GET fields 3 file)
            list(GET fields 4 url)
            list(SUBLIST fields 6 -1 licences)
            set(pharo_wasm_pin_version "${version}" PARENT_SCOPE)
            set(pharo_wasm_pin_sha "${sha}" PARENT_SCOPE)
            set(pharo_wasm_pin_file "${file}" PARENT_SCOPE)
            set(pharo_wasm_pin_url "${url}" PARENT_SCOPE)
            set(pharo_wasm_pin_licences "${licences}" PARENT_SCOPE)
            return()
        endif()
    endforeach()
    message(FATAL_ERROR "cmake/cairo-stack/pins.txt has no archive ${name}")
endfunction()
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_LIST_DIR}/../../cairo-stack/pins.txt")

# pharo_wasm_text_dep(<name>): fetches and patches the pinned archive <name>
# (its patches/ here), includes its sources.cmake with src and gen set, in
# the caller's scope
macro(pharo_wasm_text_dep name)
    pharo_wasm_pin(${name})
    file(GLOB patches "${CMAKE_CURRENT_LIST_DIR}/${name}/patches/*.patch")
    list(SORT patches)
    pharo_wasm_dep_fetch(${name}
        VERSION ${pharo_wasm_pin_version}
        URL "${pharo_wasm_pin_url}"
        SHA256 ${pharo_wasm_pin_sha}
        FILE ${pharo_wasm_pin_file}
        LICENSES ${pharo_wasm_pin_licences}
        PATCHES ${patches})
    set(src "${${name}_SOURCE_DIR}")
    set(gen "${CMAKE_CURRENT_LIST_DIR}/${name}/gen")
    include(${CMAKE_CURRENT_LIST_DIR}/${name}/sources.cmake)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_LIST_DIR}/${name}/sources.cmake")
endmacro()

# expat
pharo_wasm_text_dep(expat)
pharo_wasm_harvested_library(pharo_expat EXPAT -O2 "${src}/lib")

# fontconfig (and its pattern library, which it includes)
pharo_wasm_text_dep(fontconfig)
pharo_wasm_harvested_library(pharo_fontconfig_pattern FONTCONFIG_PATTERN -O2)
target_link_libraries(pharo_fontconfig_pattern PRIVATE pharo_freetype)
pharo_wasm_harvested_library(pharo_fontconfig FONTCONFIG -O2 "${gen}" "${src}")
target_link_libraries(pharo_fontconfig PUBLIC pharo_freetype pharo_expat pharo_fontconfig_pattern)

# GLib
pharo_wasm_text_dep(glib)
pharo_wasm_harvested_library(pharo_glib_charset GLIB_CHARSET -O2)
pharo_wasm_harvested_library(pharo_glib GLIB -O2 "${src}" "${src}/glib" "${gen}" "${gen}/glib")
target_link_libraries(pharo_glib PUBLIC pharo_glib_charset)
pharo_wasm_harvested_library(pharo_gobject GOBJECT -O2 "${src}/gobject")
target_link_libraries(pharo_gobject PUBLIC pharo_glib pharo_libffi)
pharo_wasm_harvested_library(pharo_gmodule GMODULE -O2 "${src}/gmodule" "${gen}/gmodule")
target_link_libraries(pharo_gmodule PUBLIC pharo_glib)
pharo_wasm_harvested_library(pharo_gio GIO -Os "${src}/gio")
target_link_libraries(pharo_gio PUBLIC pharo_gobject pharo_gmodule pharo_zlib)

# FriBidi, HarfBuzz
pharo_wasm_text_dep(fribidi)
pharo_wasm_harvested_library(pharo_fribidi FRIBIDI -O2 "${src}/lib" "${gen}/lib" "${gen}/gen.tab")
pharo_wasm_text_dep(harfbuzz)
pharo_wasm_harvested_library(pharo_harfbuzz HARFBUZZ -O2 "${src}/src" "${gen}/src")
target_link_libraries(pharo_harfbuzz PUBLIC pharo_freetype)

# Pango, pangoft2 and PangoCairo, in one library
pharo_wasm_text_dep(pango)
pharo_wasm_harvested_library(pharo_pango_core PANGO -O2 "${src}" "${gen}")
target_link_libraries(pharo_pango_core PUBLIC pharo_gio pharo_harfbuzz pharo_fribidi pharo_fontconfig)
pharo_wasm_harvested_library(pharo_pangoft2 PANGOFT2 -O2)
target_link_libraries(pharo_pangoft2 PUBLIC pharo_pango_core)
pharo_wasm_harvested_library(pharo_pangocairo PANGOCAIRO -O2)
target_link_libraries(pharo_pangocairo PUBLIC pharo_pangoft2 pharo_cairo)
add_library(pharo_pango INTERFACE)
target_link_libraries(pharo_pango INTERFACE pharo_pangocairo pharo_pangoft2 pharo_pango_core)

# The fonts: the DejaVu fonts and fonts/fonts.conf, in
# ${CMAKE_BINARY_DIR}/wasm/fonts, which stage.cmake and the link of the web
# VM take (PHARO_WASM_FONTS_DIR)
set(WASM_DEJAVU_VERSION "2.37")
pharo_wasm_dep_fetch(dejavu
    VERSION ${WASM_DEJAVU_VERSION}
    URL "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-${WASM_DEJAVU_VERSION}.tar.bz2"
    SHA256 fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7
    FILE dejavu-fonts-ttf-${WASM_DEJAVU_VERSION}.tar.bz2
    LICENSES LICENSE)
set(PHARO_WASM_FONTS_DIR "${CMAKE_BINARY_DIR}/wasm/fonts" PARENT_SCOPE)
set(PHARO_WASM_FONTS_DIR "${CMAKE_BINARY_DIR}/wasm/fonts")
set(PHARO_WASM_FONT_FILES DejaVuSans.ttf DejaVuSans-Bold.ttf DejaVuSerif.ttf DejaVuSerif-Bold.ttf DejaVuSansMono.ttf)
file(GLOB old "${PHARO_WASM_FONTS_DIR}/*")
foreach(file IN LISTS old)
    get_filename_component(base "${file}" NAME)
    if(NOT base IN_LIST PHARO_WASM_FONT_FILES AND NOT base STREQUAL "fonts.conf")
        file(REMOVE "${file}")
    endif()
endforeach()
foreach(font IN LISTS PHARO_WASM_FONT_FILES)
    if(NOT EXISTS "${dejavu_SOURCE_DIR}/ttf/${font}")
        message(FATAL_ERROR "${dejavu_SOURCE_DIR} has no ttf/${font}: it is not DejaVu ${WASM_DEJAVU_VERSION}")
    endif()
    configure_file("${dejavu_SOURCE_DIR}/ttf/${font}" "${PHARO_WASM_FONTS_DIR}/${font}" COPYONLY)
endforeach()
configure_file("${CMAKE_CURRENT_LIST_DIR}/fonts/fonts.conf" "${PHARO_WASM_FONTS_DIR}/fonts.conf" COPYONLY)
