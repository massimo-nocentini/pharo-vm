# simdutf for the SimdUTFPlugin and libSimdUTFFFI: release v9.4.0,
# referenced by the URL of its tag archive and that archive's SHA256 (the
# singleheader.zip of the release has no licence file, which
# pharo_wasm_dep_fetch requires), compiled from the unity file
# src/simdutf.cpp, with src/ PRIVATE and include/ PUBLIC, as the static
# library pharo_simdutf.
#
# The archive is taken as simdjson's is (cmake/simdjson.cmake): from
# WASM_DEPS_DIR, then <build>/downloads, otherwise downloaded.
# -DSIMDUTF_SOURCE_DIR=<tree> uses a source tree instead.
#
# simdutf's own CMakeLists.txt is never run: the flags are ours.
#   -O3 -g0  as cmake/simdjson.cmake: target options come after the -O2 and
#         the -g of the VM's top CMakeLists.txt, so they win
#   -fno-exceptions  simdutf never throws in C++17 (only its C++23 API can)
#   Emscripten: -m64 and the setjmp/longjmp flags, which
#         cmake/Emscripten.cache.cmake and cmake/Emscripten.cmake give to C
#         only
#   SIMDUTF_FEATURE_*: six of the seven features, all but
#         DETECT_ENCODING, which nothing uses: without it simdutf has no C
#         API (src/simdutf_c.cpp needs all seven) and about 55 KB less code
#         on x86_64.  PUBLIC: they change the virtual table of
#         simdutf::implementation, so every file that includes simdutf.h must
#         see the same ones
#   no -D_GLIBCXX_IOSTREAM and no sjNoThreads.h (cmake/simdjson.cmake):
#         simdutf includes no <iostream>, and reads no macro that _MT sets
#   PIC natively (the plugin and the library are shared libraries); the
#         archive goes to deps/, not build/vm/, which cmake/Linux.cmake
#         installs whole
#   hidden visibility: each consumer keeps a private copy of simdutf, which
#         the VM's dlopen with RTLD_GLOBAL|RTLD_DEEPBIND
#         (src/externalPrimitives.c) then cannot mix up

include_guard(GLOBAL)
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/thirdPartyNotice.cmake)

set(PHARO_SIMDUTF_VERSION 9.4.0)
set(PHARO_SIMDUTF_SHA256 361555059c963a1e5d78f3c7d42f35a19722b0333e6385994ff0fe7d80a5786a)
set(PHARO_SIMDUTF_URL "https://github.com/simdutf/simdutf/archive/refs/tags/v${PHARO_SIMDUTF_VERSION}.tar.gz")
set(SIMDUTF_SOURCE_DIR "" CACHE PATH
    "A simdutf ${PHARO_SIMDUTF_VERSION} source tree to use instead of the pinned archive")

if(SIMDUTF_SOURCE_DIR)
    get_filename_component(simdutf_SOURCE_DIR "${SIMDUTF_SOURCE_DIR}" ABSOLUTE)
    if(EMSCRIPTEN)    # what pharo_wasm_dep_fetch would have recorded
        set_property(GLOBAL APPEND PROPERTY PHARO_WASM_NOTICES
            "simdutf|${PHARO_SIMDUTF_VERSION}|${simdutf_SOURCE_DIR}|local tree|${simdutf_SOURCE_DIR}/LICENSE-APACHE|${simdutf_SOURCE_DIR}/LICENSE-MIT")
    endif()
else()
    if(NOT COMMAND pharo_wasm_dep_fetch)       # Emscripten has it already (cmake/emscripten/deps)
        include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/fetch.cmake)
    endif()
    pharo_wasm_dep_fetch(simdutf
        VERSION ${PHARO_SIMDUTF_VERSION}
        URL "${PHARO_SIMDUTF_URL}"
        SHA256 ${PHARO_SIMDUTF_SHA256}
        FILE simdutf-${PHARO_SIMDUTF_VERSION}.tar.gz
        LICENSES LICENSE-APACHE LICENSE-MIT)
endif()
if(NOT EXISTS "${simdutf_SOURCE_DIR}/src/simdutf.cpp")
    message(FATAL_ERROR "SimdUTFPlugin: ${simdutf_SOURCE_DIR} has no src/simdutf.cpp")
endif()
file(STRINGS "${simdutf_SOURCE_DIR}/include/simdutf/simdutf_version.h" pharo_simdutf_version
     REGEX "^#define SIMDUTF_VERSION " LIMIT_COUNT 1)
if(NOT pharo_simdutf_version MATCHES "\"${PHARO_SIMDUTF_VERSION}\"")
    message(FATAL_ERROR "SimdUTFPlugin: ${simdutf_SOURCE_DIR} is not simdutf ${PHARO_SIMDUTF_VERSION} (${pharo_simdutf_version})")
endif()
# The BSD-3-Clause notice of PyTorch, at the top of
# include/simdutf/internal/isadetection.h
pharo_third_party_notice(simdutf "${simdutf_SOURCE_DIR}/include/simdutf/internal/isadetection.h"
    LICENSE.simdutf-isadetection PHARO_SIMDUTF_NOTICE)

set(PHARO_SIMDUTF_CXX_OPTIONS -O3 -g0 -fno-exceptions)
if(EMSCRIPTEN)
    list(APPEND PHARO_SIMDUTF_CXX_OPTIONS -m64 ${PHARO_WASM_SJLJ_FLAGS})
endif()

add_library(pharo_simdutf STATIC ${simdutf_SOURCE_DIR}/src/simdutf.cpp)
target_include_directories(pharo_simdutf
    PRIVATE ${simdutf_SOURCE_DIR}/src
    PUBLIC ${simdutf_SOURCE_DIR}/include)
target_compile_definitions(pharo_simdutf PUBLIC
    SIMDUTF_FEATURE_UTF8=1 SIMDUTF_FEATURE_UTF16=1 SIMDUTF_FEATURE_UTF32=1
    SIMDUTF_FEATURE_ASCII=1 SIMDUTF_FEATURE_LATIN1=1 SIMDUTF_FEATURE_BASE64=1
    SIMDUTF_FEATURE_DETECT_ENCODING=0)
target_compile_options(pharo_simdutf PRIVATE ${PHARO_SIMDUTF_CXX_OPTIONS})
set_target_properties(pharo_simdutf PROPERTIES
    ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/deps
    CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
    CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
if(NOT EMSCRIPTEN)
    set_target_properties(pharo_simdutf PROPERTIES POSITION_INDEPENDENT_CODE ON)
    # Apache-2.0 or MIT, at the user's choice, and the BSD-3-Clause notice of
    # PyTorch: next to the plugin on Linux and Windows (build/vm/, packaged
    # whole), in Contents/Resources of the bundle on macOS, as simdjson's
    if(OSX)
        set(PHARO_SIMDUTF_LICENSE_DIR "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(PHARO_SIMDUTF_LICENSE_DIR "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    configure_file(${simdutf_SOURCE_DIR}/LICENSE-APACHE ${PHARO_SIMDUTF_LICENSE_DIR}/LICENSE.simdutf              COPYONLY)
    configure_file(${simdutf_SOURCE_DIR}/LICENSE-MIT    ${PHARO_SIMDUTF_LICENSE_DIR}/LICENSE.simdutf-MIT          COPYONLY)
    configure_file(${PHARO_SIMDUTF_NOTICE}              ${PHARO_SIMDUTF_LICENSE_DIR}/LICENSE.simdutf-isadetection COPYONLY)
endif()
