# SDL2 of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file with SDL2
# (PHARO_WASM_HAS_SDL2, WASM_SDL2 of cmake/emscripten/deps/options.cmake),
# and the registry of the FFI links its target, pharo_sdl2, into the VMs
# (cmake/emscripten/ffiLibraries.cmake), where the image's SDL2 class
# (libSDL2-2.0.so.0, so SDL2) finds the functions OSWindow-SDL2 binds: a
# stock image, which the web page sdl.html runs in its worker, then draws
# its world through OSSDL2Driver, as natively.  The target's include
# directory is that of SDL's public headers, with the configuration below.
#
# The version is 2.32.10, the one of Emscripten 6.0.10's own port of SDL2
# (upstream/emscripten/tools/ports/sdl2.py), and the one the native VMs of
# macOS and of Windows on x86-64 bundle (cmake/importSDL2.cmake).  The
# archive is the release of github.com/libsdl-org/SDL (signed by Sam
# Lantinga), whose sources are those of the archive of the tag
# release-2.32.10 that the port takes; the release adds REVISION.txt and
# .git-hash.
#
# This file takes the pinned archive (cmake/emscripten/deps/fetch.cmake),
# and builds SDL as the static library pharo_sdl2, with the flags of the
# deps directory, at -Os, without SDL's CMake (whose configure spends
# minutes on checks under emcc), from the sources and with the
# configuration committed next to this file, in sdl2/:
#
#     SDL_config_emscripten.h  SDL's own configuration for Emscripten, with
#                              the video drivers of Emscripten and the dummy
#                              one, and dummy haptic, joystick and audio
#                              drivers; no OpenGL ES 2 nor EGL, and no iconv
#                              (SDL's own SDL_iconv instead of musl's)
#     sources.cmake            the sources of Emscripten's port, and the
#                              dummy joystick driver
#
# SDL's headers include SDL_config_emscripten.h with quotes, which finds the
# one next to them before any include directory: the archive's include/ is
# therefore copied into ${CMAKE_CURRENT_BINARY_DIR}/sdl2/include, with the
# committed configuration in place of the archive's, and SDL and the table
# of the registry are compiled with that copy.  (A few sources also reach
# the archive's SDL_touch.h through "../../include/", which then includes
# nothing more: its own headers are included already.)  The version of the
# archive must be the one they were made for, or configure stops.
#
# The image needs no more than the video, the events, the timer and the
# clipboard of SDL (OSSDL2Driver initialises the other subsystems, and
# opens joysticks only when one is added): in a worker there is neither an
# AudioContext nor gamepads, nor a DOM, which the web page gives SDL's
# Emscripten driver a shim of (packaging/emscripten/web/sdl-shim.js).
#
# Its licence is the zlib licence of LICENSE.txt; the YUV conversions of
# src/video/yuv2rgb, which the VM has (yuv_rgb_std.c), are under the BSD
# licence of src/video/yuv2rgb/LICENSE (THIRD-PARTY-NOTICES.txt,
# cmake/emscripten/stage.cmake).

set(WASM_SDL2_VERSION "2.32.10")

pharo_wasm_dep_fetch(SDL2
    VERSION ${WASM_SDL2_VERSION}
    URL "https://github.com/libsdl-org/SDL/releases/download/release-${WASM_SDL2_VERSION}/SDL2-${WASM_SDL2_VERSION}.tar.gz"
    SHA256 5f5993c530f084535c65a6879e9b26ad441169b3e25d789d83287040a9ca5165
    FILE SDL2-${WASM_SDL2_VERSION}.tar.gz
    LICENSES LICENSE.txt src/video/yuv2rgb/LICENSE)
set(source "${SDL2_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/sdl2")

set(version "")
foreach(part MAJOR_VERSION MINOR_VERSION PATCHLEVEL)
    file(STRINGS "${source}/include/SDL_version.h" line REGEX "^#define SDL_${part}[ \t]" LIMIT_COUNT 1)
    string(REGEX REPLACE "^#define SDL_${part}[ \t]+([0-9]+).*" "\\1" line "${line}")
    list(APPEND version "${line}")
endforeach()
string(REPLACE ";" "." version "${version}")
if(NOT version STREQUAL WASM_SDL2_VERSION)
    message(FATAL_ERROR "${source} is not SDL ${WASM_SDL2_VERSION} (its include/SDL_version.h says '${version}'), "
        "for which cmake/emscripten/deps/sdl2/ was made: see cmake/emscripten/deps/sdl2/SDL_config_emscripten.h")
endif()

include(${CMAKE_CURRENT_LIST_DIR}/sdl2/sources.cmake)
set(sources "")
foreach(file IN LISTS PHARO_WASM_SDL2_SOURCES)
    if(NOT EXISTS "${source}/src/${file}")
        message(FATAL_ERROR "${source} has no src/${file}: it is not SDL ${WASM_SDL2_VERSION}")
    endif()
    list(APPEND sources "${source}/src/${file}")
endforeach()

# The headers, with the committed configuration.  (configure_file copies a
# header again when it changes, and configure runs again when the committed
# configuration does.)
file(GLOB headers RELATIVE "${source}/include" "${source}/include/*.h")
if(NOT "SDL_config_emscripten.h" IN_LIST headers)
    message(FATAL_ERROR "${source}/include has no SDL_config_emscripten.h: it is not SDL ${WASM_SDL2_VERSION}")
endif()
foreach(header IN LISTS headers)
    if(header STREQUAL "SDL_config_emscripten.h")
        configure_file("${CMAKE_CURRENT_LIST_DIR}/sdl2/SDL_config_emscripten.h" "${dir}/include/${header}" COPYONLY)
    else()
        configure_file("${source}/include/${header}" "${dir}/include/${header}" COPYONLY)
    endif()
endforeach()

add_library(pharo_sdl2 STATIC ${sources})
target_include_directories(pharo_sdl2 PUBLIC "${dir}/include")
pharo_wasm_dep_flags(pharo_sdl2 -Os)
