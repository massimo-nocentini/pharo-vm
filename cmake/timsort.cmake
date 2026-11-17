# timsort.c for the TimSortPlugin and libTimSortFFI: the C port of the sort
# of CPython's lists (listsort of Objects/listobject.c) at commit fd70381 of
# github.com/massimo-nocentini/timsort.c, referenced by the URL of that
# commit's archive and the archive's SHA256, compiled from src/timsort.c as
# the static library pharo_timsort.
#
# The archive is taken as simdjson's is (cmake/simdjson.cmake): from
# WASM_DEPS_DIR, then <build>/downloads, otherwise downloaded.
# -DTIMSORT_SOURCE_DIR=<tree> uses a source tree instead (a clone of
# timsort.c at that commit).
#
# timsort.c has no licence file: it is a derivative work of CPython, under
# the PSF License Agreement, whose text, with the copyright line of CPython
# and the summary of the changes that its clause 3 asks for, is the
# committed plugins/TimSortPlugin/LICENSE.timsort.  The archive is fetched
# without LICENSES and that file is added to the notice of timsort that
# pharo_wasm_dep_fetch recorded, as cmake/emscripten/deps/libgit2.cmake adds
# its xdiff notice (cmake/emscripten/stage.cmake wants a licence file in
# every item of PHARO_WASM_NOTICES).
#
# timsort.c's own Makefile is never run: the flags are ours.
#   -O3 -g0  as cmake/simdjson.cmake: target options come after the -O2 and
#         the -g of the VM's top CMakeLists.txt, so they win
#   -DNDEBUG  timsort.c's assertions are for its own tests (its Makefile
#         builds with -DNDEBUG as well)
#   C only: timsort.c is not C++ (a goto crosses initializations); on
#         Emscripten, CMAKE_C_FLAGS give -m64 and the setjmp/longjmp flags
#         already (cmake/Emscripten.cache.cmake, cmake/Emscripten.cmake)
#   list_sort_impl=pharo_timsort_list_sort_impl, PUBLIC: the one global
#         function of timsort.c has a generic name; every file that includes
#         timsort.h (only src/engine/tsEngine.c) sees the same one
#   PIC natively (the plugin and the library are shared libraries); the
#         archive goes to deps/, not build/vm/, which cmake/Linux.cmake
#         installs whole
#   hidden visibility: each consumer keeps a private copy of timsort.c, which
#         the VM's dlopen with RTLD_GLOBAL|RTLD_DEEPBIND
#         (src/externalPrimitives.c) then cannot mix up

include_guard(GLOBAL)

set(PHARO_TIMSORT_COMMIT fd70381039da47507984b9c1306914828871afbf)
set(PHARO_TIMSORT_VERSION fd70381)		# what primitiveInfo answers: 'timsort.c fd70381'
set(PHARO_TIMSORT_SHA256 3e1eb62a452bf53c6c1427f4f0bb9e126c35b9051424eee20be43090d9850016)
set(PHARO_TIMSORT_URL "https://github.com/massimo-nocentini/timsort.c/archive/${PHARO_TIMSORT_COMMIT}.tar.gz")
set(PHARO_TIMSORT_NOTICE "${CMAKE_CURRENT_SOURCE_DIR}/plugins/TimSortPlugin/LICENSE.timsort")
set(TIMSORT_SOURCE_DIR "" CACHE PATH
    "A timsort.c ${PHARO_TIMSORT_VERSION} source tree to use instead of the pinned archive")

if(NOT EXISTS "${PHARO_TIMSORT_NOTICE}")
    message(FATAL_ERROR "TimSortPlugin: no ${PHARO_TIMSORT_NOTICE}")
endif()
if(TIMSORT_SOURCE_DIR)
    get_filename_component(timsort_SOURCE_DIR "${TIMSORT_SOURCE_DIR}" ABSOLUTE)
    if(EMSCRIPTEN)    # what pharo_wasm_dep_fetch would have recorded, and the notice
        set_property(GLOBAL APPEND PROPERTY PHARO_WASM_NOTICES
            "timsort|${PHARO_TIMSORT_VERSION}|${timsort_SOURCE_DIR}|local tree|${PHARO_TIMSORT_NOTICE}")
    endif()
else()
    if(NOT COMMAND pharo_wasm_dep_fetch)       # Emscripten has it already (cmake/emscripten/deps)
        include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/fetch.cmake)
    endif()
    pharo_wasm_dep_fetch(timsort
        VERSION ${PHARO_TIMSORT_VERSION}
        URL "${PHARO_TIMSORT_URL}"
        SHA256 ${PHARO_TIMSORT_SHA256}
        FILE timsort.c-${PHARO_TIMSORT_VERSION}.tar.gz)
    # (the archive has no licence file: the notice is added to the item that
    # pharo_wasm_dep_fetch recorded)
    get_property(pharo_timsort_notices GLOBAL PROPERTY PHARO_WASM_NOTICES)
    set(pharo_timsort_found OFF)
    set(pharo_timsort_items "")
    foreach(pharo_timsort_item IN LISTS pharo_timsort_notices)
        if(pharo_timsort_item MATCHES "^timsort\\|")
            string(APPEND pharo_timsort_item "|${PHARO_TIMSORT_NOTICE}")
            set(pharo_timsort_found ON)
        endif()
        list(APPEND pharo_timsort_items "${pharo_timsort_item}")
    endforeach()
    if(NOT pharo_timsort_found)
        message(FATAL_ERROR "cmake/timsort.cmake: pharo_wasm_dep_fetch recorded no notice of timsort")
    endif()
    set_property(GLOBAL PROPERTY PHARO_WASM_NOTICES "${pharo_timsort_items}")
endif()
if(NOT EXISTS "${timsort_SOURCE_DIR}/src/timsort.c" OR NOT EXISTS "${timsort_SOURCE_DIR}/src/timsort.h")
    message(FATAL_ERROR "TimSortPlugin: ${timsort_SOURCE_DIR} has no src/timsort.c and src/timsort.h")
endif()
# (timsort.c has no version macro: the header of the commit has the include
# guard and the ptrdiff_t size type of fd70381, which the pin needs)
file(STRINGS "${timsort_SOURCE_DIR}/src/timsort.h" pharo_timsort_marks
     REGEX "^(#ifndef TIMSORT_H|#define TIMSORT_SSIZE_T_MAX PTRDIFF_MAX|int list_sort_impl\\()")
list(LENGTH pharo_timsort_marks pharo_timsort_mark_count)
if(NOT pharo_timsort_mark_count EQUAL 3)
    message(FATAL_ERROR "TimSortPlugin: ${timsort_SOURCE_DIR}/src/timsort.h is not the one of timsort.c ${PHARO_TIMSORT_VERSION}")
endif()

add_library(pharo_timsort STATIC ${timsort_SOURCE_DIR}/src/timsort.c)
target_include_directories(pharo_timsort PUBLIC ${timsort_SOURCE_DIR}/src)
target_compile_definitions(pharo_timsort
    PRIVATE NDEBUG
    PUBLIC list_sort_impl=pharo_timsort_list_sort_impl)
target_compile_options(pharo_timsort PRIVATE -O3 -g0)
set_target_properties(pharo_timsort PROPERTIES
    ARCHIVE_OUTPUT_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}/deps
    C_VISIBILITY_PRESET hidden)
if(NOT EMSCRIPTEN)
    set_target_properties(pharo_timsort PROPERTIES POSITION_INDEPENDENT_CODE ON)
    # the PSF notice: next to the plugin on Linux and Windows (build/vm/,
    # packaged whole), in Contents/Resources of the bundle on macOS, as
    # simdutf's
    if(OSX)
        set(PHARO_TIMSORT_LICENSE_DIR "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(PHARO_TIMSORT_LICENSE_DIR "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    configure_file(${PHARO_TIMSORT_NOTICE} ${PHARO_TIMSORT_LICENSE_DIR}/LICENSE.timsort COPYONLY)
endif()
