# simdcsv for the SimdCSVPlugin: Geoff Langdale's simdcsv, commit a9494a8 (the
# repository has no tags), referenced by the URL of its archive and that
# archive's SHA256, and the engine of the plugin as the static library
# pharo_sc_engine.
#
# The part of src/main.cpp of simdcsv a9494a8 (Apache-2.0) from 'struct
# simd_input' to flatten_bits is included byte for byte (SHA256 checked). On
# x86_64 __AVX2__ is defined for the include; on arm64 its vmull_p64 is
# replaced by a macro (a shift prefix-xor, no PMULL) and neonmovemask_bulk,
# which upstream does not define, is ours. The loop, the tail, the checks, the
# other kernels, the builders and the errors are ours. WebAssembly builds
# contain no simdcsv code.
#
# So on Emscripten no archive is taken, nothing is cut and there is no notice.
# Natively the archive is taken as simdjson's is (cmake/simdjson.cmake): from
# WASM_DEPS_DIR, then <build>/downloads, otherwise downloaded;
# -DSIMDCSV_SOURCE_DIR=<tree> uses a source tree instead (an extraction of the
# same archive, or a clone at that commit; its LICENSE is checked too).  The
# slice is cut out of it at configure time, its SHA256 checked, into
# <build>/deps/simdcsv-slice/simdcsv_slice.inc, which only
# src/engine/scKernels.inc includes.
#
# The engine is C++17 without the C++ library (no C++ header, no exception, no
# RTTI, no static constructor): the plugin takes its archive by file
# (cmake/plugins.cmake) and stays linked as C.
#   -O3 -g0  as cmake/simdjson.cmake: target options come after the -O2 and
#         the -g of the VM's top CMakeLists.txt, so they win
#   no -m flag: the kernels of scKernels.inc choose their instructions with
#         target pragmas, and the CPU is asked at run time
#   -fno-strict-aliasing  the units of a source are read as bytes and as words
#   -ffp-contract=off  a decimal is converted by one exact multiplication or
#         division
#   CXX_EXTENSIONS ON  the slice and the kernels use GNU C++
#   Emscripten: -m64 and the setjmp/longjmp flags, which
#         cmake/Emscripten.cache.cmake and cmake/Emscripten.cmake give to C
#         only
#   PIC natively (the plugin is a shared library); the archive goes to deps/,
#         not build/vm/, which cmake/Linux.cmake installs whole
#   hidden visibility: libSimdCSVPlugin exports only its primitives and its
#         module functions

include_guard(GLOBAL)

set(PHARO_SIMDCSV_VERSION a9494a8)

if(NOT EMSCRIPTEN)
    set(PHARO_SIMDCSV_COMMIT a9494a85c83525bb42ba368d8f9bcf3acf53a8d1)
    set(PHARO_SIMDCSV_SHA256 27eb10cce2765e92244a83ad90698c545ac865ad9e8383cf87d83b6692f256c4)
    set(PHARO_SIMDCSV_MAIN_SHA256 d9b98882a52d2b018980ee9e6a07625af05be1911a706b60f0a105b2018fc383)
    set(PHARO_SIMDCSV_SLICE_SHA256 bf5511f156c66cc1c718cac01fd8b32fd5788aa1b6ba7276c0eb546b68d50874)
    set(PHARO_SIMDCSV_LICENSE_SHA256 c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4)
    set(PHARO_SIMDCSV_URL "https://github.com/geofflangdale/simdcsv/archive/${PHARO_SIMDCSV_COMMIT}.tar.gz")
    set(SIMDCSV_SOURCE_DIR "" CACHE PATH
        "A simdcsv ${PHARO_SIMDCSV_VERSION} source tree to use instead of the pinned archive")

    if(SIMDCSV_SOURCE_DIR)
        get_filename_component(simdcsv_SOURCE_DIR "${SIMDCSV_SOURCE_DIR}" ABSOLUTE)
        if(NOT EXISTS "${simdcsv_SOURCE_DIR}/LICENSE")
            message(FATAL_ERROR "SimdCSVPlugin: ${simdcsv_SOURCE_DIR} has no LICENSE")
        endif()
    else()
        if(NOT COMMAND pharo_wasm_dep_fetch)
            include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/fetch.cmake)
        endif()
        pharo_wasm_dep_fetch(simdcsv
            VERSION ${PHARO_SIMDCSV_VERSION}
            URL "${PHARO_SIMDCSV_URL}"
            SHA256 ${PHARO_SIMDCSV_SHA256}
            FILE simdcsv-${PHARO_SIMDCSV_VERSION}.tar.gz
            LICENSES LICENSE)
    endif()
    set(pharo_simdcsv_main "${simdcsv_SOURCE_DIR}/src/main.cpp")
    if(NOT EXISTS "${pharo_simdcsv_main}")
        message(FATAL_ERROR "SimdCSVPlugin: ${simdcsv_SOURCE_DIR} has no src/main.cpp")
    endif()
    # The hashes are of variables, and everything is quoted, so that semicolons
    # and brackets are kept; but CMake reads text: CRLF becomes LF, so that a
    # checkout with CRLF gives the same slice and the same LICENSE (and on
    # Windows the .inc is written with CRLF: the same tokens).
    # tests/run-c-tests.sh hashes the bytes of the files, and refuses CRLF.
    file(READ "${simdcsv_SOURCE_DIR}/LICENSE" pharo_simdcsv_text)
    string(SHA256 pharo_simdcsv_hash "${pharo_simdcsv_text}")
    if(NOT pharo_simdcsv_hash STREQUAL PHARO_SIMDCSV_LICENSE_SHA256)
        message(FATAL_ERROR "SimdCSVPlugin: ${simdcsv_SOURCE_DIR}/LICENSE is not that of simdcsv ${PHARO_SIMDCSV_VERSION}: "
            "its SHA256 is ${pharo_simdcsv_hash}, not ${PHARO_SIMDCSV_LICENSE_SHA256}")
    endif()
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${pharo_simdcsv_main}")
    file(READ "${pharo_simdcsv_main}" pharo_simdcsv_text)
    string(FIND "${pharo_simdcsv_text}" "struct simd_input {" pharo_simdcsv_begin)
    string(FIND "${pharo_simdcsv_text}" "//\n// This optimization option might be helpful" pharo_simdcsv_end)
    if(pharo_simdcsv_begin LESS 0 OR pharo_simdcsv_end LESS pharo_simdcsv_begin)
        file(SHA256 "${pharo_simdcsv_main}" pharo_simdcsv_hash)
        message(FATAL_ERROR "SimdCSVPlugin: ${pharo_simdcsv_main} is not simdcsv ${PHARO_SIMDCSV_VERSION}: "
            "it has no part from 'struct simd_input {' to '// This optimization option might be helpful' "
            "(its SHA256 is ${pharo_simdcsv_hash}, not ${PHARO_SIMDCSV_MAIN_SHA256})")
    endif()
    math(EXPR pharo_simdcsv_length "${pharo_simdcsv_end} - ${pharo_simdcsv_begin}")
    string(SUBSTRING "${pharo_simdcsv_text}" ${pharo_simdcsv_begin} ${pharo_simdcsv_length} pharo_simdcsv_slice)
    string(SHA256 pharo_simdcsv_hash "${pharo_simdcsv_slice}")
    if(NOT pharo_simdcsv_hash STREQUAL PHARO_SIMDCSV_SLICE_SHA256)
        message(FATAL_ERROR "SimdCSVPlugin: ${pharo_simdcsv_main} is not simdcsv ${PHARO_SIMDCSV_VERSION}: "
            "the SHA256 of its part from 'struct simd_input' to flatten_bits is ${pharo_simdcsv_hash}, "
            "not ${PHARO_SIMDCSV_SLICE_SHA256}")
    endif()
    set(PHARO_SIMDCSV_SLICE_DIR "${CMAKE_CURRENT_BINARY_DIR}/deps/simdcsv-slice")
    set(pharo_simdcsv_inc "${PHARO_SIMDCSV_SLICE_DIR}/simdcsv_slice.inc")
    set(pharo_simdcsv_old "")
    if(EXISTS "${pharo_simdcsv_inc}")
        file(READ "${pharo_simdcsv_inc}" pharo_simdcsv_old)
    endif()
    if(NOT pharo_simdcsv_old STREQUAL pharo_simdcsv_slice)
        file(WRITE "${pharo_simdcsv_inc}" "${pharo_simdcsv_slice}")
    endif()

    # Apache-2.0: next to the plugin on Linux and Windows (build/vm/, packaged
    # whole), in Contents/Resources of the bundle on macOS, as simdjson's
    # (simdcsv has no NOTICE file)
    if(OSX)
        set(PHARO_SIMDCSV_LICENSE_DIR "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(PHARO_SIMDCSV_LICENSE_DIR "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    configure_file(${simdcsv_SOURCE_DIR}/LICENSE ${PHARO_SIMDCSV_LICENSE_DIR}/LICENSE.simdcsv COPYONLY)
endif()

set(PHARO_SIMDCSV_CXX_OPTIONS -O3 -g0 -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off)
if(EMSCRIPTEN)
    list(APPEND PHARO_SIMDCSV_CXX_OPTIONS -m64 ${PHARO_WASM_SJLJ_FLAGS})
endif()

set(sc_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/SimdCSVPlugin)
add_library(pharo_sc_engine STATIC ${sc_dir}/src/engine/scEngine.cpp)
target_include_directories(pharo_sc_engine PRIVATE ${sc_dir}/include/common)
target_compile_definitions(pharo_sc_engine PRIVATE PHARO_SIMDCSV_VERSION="${PHARO_SIMDCSV_VERSION}")
target_compile_options(pharo_sc_engine PRIVATE ${PHARO_SIMDCSV_CXX_OPTIONS})
set_target_properties(pharo_sc_engine PROPERTIES
    ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/deps
    CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS ON
    C_VISIBILITY_PRESET hidden CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
if(NOT EMSCRIPTEN)
    target_include_directories(pharo_sc_engine PRIVATE ${PHARO_SIMDCSV_SLICE_DIR})
    set_target_properties(pharo_sc_engine PROPERTIES POSITION_INDEPENDENT_CODE ON)
endif()
