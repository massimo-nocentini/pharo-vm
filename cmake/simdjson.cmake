# simdjson for the SimdJSONPlugin: release v5.0.5, referenced by the URL of
# its tag archive and that archive's SHA256, compiled from the amalgamation
# singleheader/simdjson.cpp as the static library pharo_simdjson.
#
# The archive is taken from WASM_DEPS_DIR (offline builds, native ones too),
# then <build>/downloads, otherwise downloaded (cmake/emscripten/deps/fetch.cmake,
# which also records the licences for the THIRD-PARTY-NOTICES.txt of the
# Emscripten VM).  -DSIMDJSON_SOURCE_DIR=<tree> uses a source tree instead.
#
# simdjson's own CMakeLists.txt is never run (it needs CMake 3.14 and brings
# options, Threads and install rules of its own): the flags are ours.
#   -O3   simdjson's level; target options come after the -O2 that the VM's
#         top CMakeLists.txt adds with add_compile_options(${OPTIMIZATION_LEVEL}),
#         so -O3 wins
#   -g0   no debug information; the VM's top CMakeLists.txt adds -g to
#         CMAKE_C_FLAGS and CMAKE_CXX_FLAGS in every build type (and its
#         Windows COMMON_FLAGS -ggdb2), and the last -g option wins
#   -fno-exceptions + SIMDJSON_EXCEPTIONS=0: error codes only; without them
#         the wasm link fails on __cxa_throw
#   Emscripten: -m64 and the setjmp/longjmp flags, which cmake/Emscripten.cache.cmake
#         and cmake/Emscripten.cmake give to C only
#   Linux: -D_GLIBCXX_IOSTREAM=1 skips the <iostream> of libstdc++, which
#         simdjson includes and never uses (<ostream> is enough): from GCC 13
#         on, its static initializer is the symbol ios_base_library_init of
#         GLIBCXX_3.4.32, and the plugin would not load on systems whose
#         libstdc++ is older (Ubuntu 22.04, Debian 12, RHEL 9), where the rest
#         of the VM runs.  It needs then GLIBCXX_3.4.21 (GCC 5) at most.
#         libc++ ignores the macro.
#   Windows: simdjson turns its threads on when _MT is defined, which the
#         VM's COMMON_FLAGS do and the mingw-w64 _mingw.h does again whenever
#         it is missing; include/win/sjNoThreads.h, included first, reads
#         _mingw.h and then undefines _MT for good
#   PIC natively (the plugin is a shared library); the archive goes to deps/,
#         not build/vm/, which cmake/Linux.cmake installs whole
#   hidden visibility for C++ only: the C entry points keep theirs

set(PHARO_SIMDJSON_VERSION 5.0.5)
set(PHARO_SIMDJSON_SHA256 50bb17769e1054ff1bfcd5dbbfcf33544c77f6ebdb31a318436ea0e47f78f571)
set(PHARO_SIMDJSON_URL "https://github.com/simdjson/simdjson/archive/refs/tags/v${PHARO_SIMDJSON_VERSION}.tar.gz")
set(SIMDJSON_SOURCE_DIR "" CACHE PATH
    "A simdjson ${PHARO_SIMDJSON_VERSION} source tree to use instead of the pinned archive")

if(SIMDJSON_SOURCE_DIR)
    get_filename_component(simdjson_SOURCE_DIR "${SIMDJSON_SOURCE_DIR}" ABSOLUTE)
    if(EMSCRIPTEN)    # what pharo_wasm_dep_fetch would have recorded
        set_property(GLOBAL APPEND PROPERTY PHARO_WASM_NOTICES
            "simdjson|${PHARO_SIMDJSON_VERSION}|${simdjson_SOURCE_DIR}|local tree|${simdjson_SOURCE_DIR}/LICENSE|${simdjson_SOURCE_DIR}/LICENSE-MIT")
    endif()
else()
    if(NOT COMMAND pharo_wasm_dep_fetch)       # Emscripten has it already (cmake/emscripten/deps)
        include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/fetch.cmake)
    endif()
    pharo_wasm_dep_fetch(simdjson
        VERSION ${PHARO_SIMDJSON_VERSION}
        URL "${PHARO_SIMDJSON_URL}"
        SHA256 ${PHARO_SIMDJSON_SHA256}
        FILE simdjson-${PHARO_SIMDJSON_VERSION}.tar.gz
        LICENSES LICENSE LICENSE-MIT)
endif()
if(NOT EXISTS "${simdjson_SOURCE_DIR}/singleheader/simdjson.cpp")
    message(FATAL_ERROR "SimdJSONPlugin: ${simdjson_SOURCE_DIR} has no singleheader/simdjson.cpp")
endif()
file(STRINGS "${simdjson_SOURCE_DIR}/singleheader/simdjson.h" pharo_simdjson_version
     REGEX "^#define SIMDJSON_VERSION " LIMIT_COUNT 1)
if(NOT pharo_simdjson_version MATCHES "\"${PHARO_SIMDJSON_VERSION}\"")
    message(FATAL_ERROR "SimdJSONPlugin: ${simdjson_SOURCE_DIR} is not simdjson ${PHARO_SIMDJSON_VERSION} (${pharo_simdjson_version})")
endif()

set(PHARO_SIMDJSON_CXX_OPTIONS -O3 -g0 -fno-exceptions)
if(EMSCRIPTEN)
    list(APPEND PHARO_SIMDJSON_CXX_OPTIONS -m64 ${PHARO_WASM_SJLJ_FLAGS})
endif()
if(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    list(APPEND PHARO_SIMDJSON_CXX_OPTIONS -D_GLIBCXX_IOSTREAM=1)
endif()
if(WIN)
    list(APPEND PHARO_SIMDJSON_CXX_OPTIONS
        "-include${CMAKE_CURRENT_SOURCE_DIR}/plugins/SimdJSONPlugin/include/win/sjNoThreads.h")
endif()

add_library(pharo_simdjson STATIC ${simdjson_SOURCE_DIR}/singleheader/simdjson.cpp)
target_include_directories(pharo_simdjson PUBLIC ${simdjson_SOURCE_DIR}/singleheader)
target_compile_definitions(pharo_simdjson PUBLIC SIMDJSON_EXCEPTIONS=0)
target_compile_options(pharo_simdjson PRIVATE ${PHARO_SIMDJSON_CXX_OPTIONS})
set_target_properties(pharo_simdjson PROPERTIES
    ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/deps
    CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
    CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
if(NOT EMSCRIPTEN)
    set_target_properties(pharo_simdjson PROPERTIES POSITION_INDEPENDENT_CODE ON)
    # Apache-2.0 or MIT, at the user's choice: both texts go with the plugin.
    # Linux and Windows package build/vm/ whole (lib/ of the Linux zip); the
    # plugins of macOS are in Contents/MacOS/Plugins, which holds only code,
    # so the texts go to Contents/Resources of the bundle (cmake/sign.cmake)
    if(OSX)
        set(PHARO_SIMDJSON_LICENSE_DIR "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(PHARO_SIMDJSON_LICENSE_DIR "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    configure_file(${simdjson_SOURCE_DIR}/LICENSE     ${PHARO_SIMDJSON_LICENSE_DIR}/LICENSE.simdjson     COPYONLY)
    configure_file(${simdjson_SOURCE_DIR}/LICENSE-MIT ${PHARO_SIMDJSON_LICENSE_DIR}/LICENSE.simdjson-MIT COPYONLY)
endif()
