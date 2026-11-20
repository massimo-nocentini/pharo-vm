# Poppler for the Emscripten VM (WASM_POPPLER)
#
# cmake/emscripten/deps/CMakeLists.txt includes this file when Poppler is on
# (PHARO_WASM_HAS_POPPLER, cmake/emscripten/deps/options.cmake), after
# pango.cmake, whose GLib, fontconfig and helpers (pharo_wasm_text_dep) it
# uses, as cairo with its PDF surface.  It builds, from the pinned archives
# of the native VMs' cairo stack (cmake/cairo-stack/pins.txt), the static
# libraries
#
#     pharo_jpeg          libjpeg-turbo 3.2.1, without SIMD (there is none
#                         for WebAssembly), its symbols renamed pharo_turbo_*
#                         (libjpeg-turbo/pharo-prefix.h, force-included in it
#                         and in Poppler): the JPEGReadWriter2Plugin links
#                         the IJG libjpeg 6b of its sources, with the same
#                         names
#     pharo_poppler_core  Poppler 26.11.0 (C++23), with the features of the
#                         native VMs (scripts/cairo-stack/build.sh): its
#                         cairo output device, fontconfig for the fonts a
#                         PDF does not embed, libjpeg-turbo for DCT images;
#                         no NSS, GPGME, libcurl, OpenJPEG, lcms2, libtiff,
#                         brotli nor Boost
#     pharo_poppler_glib  poppler-glib, its GLib interface
#
# from the sources.cmake and gen/ of libjpeg-turbo/ and poppler/ here
# (scripts/wasm-deps/regen-text-stack.sh), with -O2.  The registry of the FFI
# lists the functions LibPoppler calls (poppler-glib.txt).  Poppler is under
# the GPL: docs/PangoPoppler.md, "Licence".
#
# Poppler's C++ code is compiled without exceptions, as Emscripten does by
# default (-fignore-exceptions): a throw would abort.  Poppler reports its
# errors by return values; libc++ throws only on a failed allocation.

if(NOT TARGET pharo_pango OR NOT PHARO_WASM_HAS_CAIRO_PDF)
    message(FATAL_ERROR "cmake/emscripten/deps/poppler.cmake: no pharo_pango, or no PDF surface of cairo, which Poppler needs")
endif()

pharo_wasm_text_dep(libjpeg-turbo)
pharo_wasm_harvested_library(pharo_jpeg JPEG -O2 "${src}/src" "${gen}")
# (its symbols renamed: the JPEGReadWriter2Plugin links another libjpeg)
target_compile_options(pharo_jpeg PUBLIC "SHELL:-include ${CMAKE_CURRENT_LIST_DIR}/libjpeg-turbo/pharo-prefix.h")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${CMAKE_CURRENT_LIST_DIR}/libjpeg-turbo/pharo-prefix.h")

pharo_wasm_text_dep(poppler)
pharo_wasm_harvested_library(pharo_poppler_core POPPLER -O2)
target_link_libraries(pharo_poppler_core PUBLIC pharo_jpeg pharo_fontconfig pharo_cairo pharo_gio pharo_zlib)
pharo_wasm_harvested_library(pharo_poppler_glib POPPLER_GLIB -O2 "${src}/glib" "${gen}/glib")
target_link_libraries(pharo_poppler_glib PUBLIC pharo_poppler_core pharo_gio pharo_cairo)
