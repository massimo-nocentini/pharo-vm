# pixman of the Emscripten VM
#
# cmake/emscripten/deps/cairo.cmake includes this file, with cairo
# (PHARO_WASM_HAS_CAIRO, cmake/emscripten/deps/options.cmake), which draws
# with it.  The target, pharo_pixman, is not in the registry of the FFI: no
# image binds pixman.
#
# This file takes the pinned release archive (cmake/emscripten/deps/
# fetch.cmake) and builds pixman as the static library pharo_pixman, with
# the flags of the deps directory and those of pixman's own build, but no
# meson.  What meson made once (cmake/emscripten/deps/cairo/README says how)
# is committed next to this file, in pixman/:
#
#     pixman-config.h   the configuration, which pixman's sources include
#                       (HAVE_CONFIG_H): no SIMD (pixman has none for
#                       WebAssembly), no threads (TLS is __thread, a plain
#                       global without them), no timers
#     pixman-version.h  the version of pixman.h, for pixman and whoever
#                       includes it (cairo)
#     sources.cmake     the sources of that build
#
# The version of the archive must be the one they were made for, or
# configure stops: another pixman needs them made again.

set(WASM_PIXMAN_VERSION "0.44.2")

pharo_wasm_dep_fetch(pixman
    VERSION ${WASM_PIXMAN_VERSION}
    URL "https://www.cairographics.org/releases/pixman-${WASM_PIXMAN_VERSION}.tar.gz"
    SHA256 6349061ce1a338ab6952b92194d1b0377472244208d47ff25bef86fc71973466
    FILE pixman-${WASM_PIXMAN_VERSION}.tar.gz
    LICENSES COPYING)
set(source "${pixman_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/pixman")

file(STRINGS "${source}/meson.build" version REGEX "^[ \t]*version[ \t]*:" LIMIT_COUNT 1)
if(NOT version MATCHES "'${WASM_PIXMAN_VERSION}'")
    message(FATAL_ERROR "${source} is not pixman ${WASM_PIXMAN_VERSION} (its meson.build has ${version}), "
        "for which cmake/emscripten/deps/pixman/ was made: see cmake/emscripten/deps/cairo/README")
endif()

include(${CMAKE_CURRENT_LIST_DIR}/pixman/sources.cmake)
set(sources "")
foreach(file IN LISTS PHARO_WASM_PIXMAN_SOURCES)
    if(NOT EXISTS "${source}/pixman/${file}")
        message(FATAL_ERROR "${source} has no pixman/${file}: it is not pixman ${WASM_PIXMAN_VERSION}")
    endif()
    list(APPEND sources "${source}/pixman/${file}")
endforeach()

# pixman-version.h alone where the users of pixman.h find it: not
# pixman-config.h, which is pixman's own
configure_file("${CMAKE_CURRENT_LIST_DIR}/pixman/pixman-version.h" "${dir}/include/pixman-version.h" COPYONLY)

add_library(pharo_pixman STATIC ${sources})
target_include_directories(pharo_pixman
    PRIVATE "${CMAKE_CURRENT_LIST_DIR}/pixman"
    PUBLIC "${dir}/include" "${source}/pixman")
target_compile_definitions(pharo_pixman PRIVATE HAVE_CONFIG_H _FILE_OFFSET_BITS=64)
# (the options of pixman's meson.build)
target_compile_options(pharo_pixman PRIVATE
    -std=gnu99 -fno-strict-aliasing -fvisibility=hidden -ftrapping-math)
pharo_wasm_dep_flags(pharo_pixman -O2)
