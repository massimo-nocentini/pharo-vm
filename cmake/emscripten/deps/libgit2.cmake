# libgit2 of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file with libgit2
# (PHARO_WASM_HAS_LIBGIT2, WASM_LIBGIT2 of cmake/emscripten/deps/
# options.cmake), and the registry of the FFI links its target, pharo_git2,
# into the VMs (cmake/emscripten/ffiLibraries.cmake), where the image's
# LGitLibrary (libgit2.so.1.4.4, so git2) finds the functions Iceberg binds.
# The target's include directory is that of libgit2's public headers.
#
# The version is 1.4.4, the one the native VMs of Linux ship to Pharo 12
# and Pharo 15 (cmake/importLibGit2.cmake): the structures Pharo 12's LGit
# allocates (fetch, clone, remote callbacks, push options, blame hunks) have
# the sizes of 1.4.4, and are too small for those of later versions
# (git_fetch_options is 208 bytes in 1.4.4 and 216 in 1.9), which
# git_*_init_options would write past.
#
# This file takes the pinned archive of the tag v1.4.4 (cmake/emscripten/
# deps/fetch.cmake), the one GitHub makes (the release has no other), and
# builds libgit2 as the static library pharo_git2, with the flags of the deps
# directory, at -Os, and the definitions of libgit2's own CMake, but without
# running it: its configure spends half a minute on checks under emcc.  What
# it made once is committed next to this file, in libgit2/:
#
#     git2/sys/features.h  the configuration, which libgit2's sources include
#                          (src/common.h): no threads, SSH, HTTPS, NTLM,
#                          GSSAPI nor iconv, SHA1DC, the C library's regcomp
#     sources.cmake        the sources of that build, and how it ran
#
# without libgit2's own copy of zlib: libgit2 is built on pharo_zlib
# (cmake/emscripten/deps/zlib.cmake), which libpng and cairo use too, so that
# the VM links one zlib.  The version of the archive must be the one they
# were made for, or configure stops.
#
# libgit2 has its http:// and git:// transports on sockets, which call
# getaddrinfo: src/emscripten/gitSupport.c makes it fail (it would stop the
# runtime of wasm64), and gives the http and https transports their requests
# over XHR instead (WASM_LIBGIT2_HTTP).
#
# Its licence is the GPL 2 with the linking exception of COPYING, which also
# holds the licences of what libgit2 bundles (SHA1DC, wildmatch...), and the
# text of the LGPL 2.1 (for deps/winhttp, not built here).  Two of the
# bundled libraries that the VM has are not in COPYING: http-parser
# (deps/http-parser/COPYING), and LibXDiff (src/xdiff), which is under the
# LGPL 2.1 or later; the notice of its sources is written into
# ${CMAKE_CURRENT_BINARY_DIR}/libgit2/xdiff-NOTICE.txt and added to the
# notices of libgit2 (THIRD-PARTY-NOTICES.txt, cmake/emscripten/stage.cmake).

set(WASM_LIBGIT2_VERSION "1.4.4")

include(${CMAKE_CURRENT_LIST_DIR}/zlib.cmake)

pharo_wasm_dep_fetch(libgit2
    VERSION ${WASM_LIBGIT2_VERSION}
    URL "https://github.com/libgit2/libgit2/archive/refs/tags/v${WASM_LIBGIT2_VERSION}.tar.gz"
    SHA256 e9923e9916a32f54c661d55d79c28fa304cb23617639e68bff9f94d3e18f2d4b
    FILE libgit2-${WASM_LIBGIT2_VERSION}.tar.gz
    LICENSES COPYING deps/http-parser/COPYING)
set(source "${libgit2_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/libgit2")

file(STRINGS "${source}/include/git2/version.h" version REGEX "^#define LIBGIT2_VERSION " LIMIT_COUNT 1)
if(NOT version MATCHES "\"${WASM_LIBGIT2_VERSION}\"")
    message(FATAL_ERROR "${source} is not libgit2 ${WASM_LIBGIT2_VERSION} (its include/git2/version.h has '${version}'), "
        "for which cmake/emscripten/deps/libgit2/ was made: see cmake/emscripten/deps/libgit2/sources.cmake")
endif()

include(${CMAKE_CURRENT_LIST_DIR}/libgit2/sources.cmake)
set(sources "")
foreach(file IN LISTS PHARO_WASM_LIBGIT2_SOURCES)
    if(NOT EXISTS "${source}/${file}")
        message(FATAL_ERROR "${source} has no ${file}: it is not libgit2 ${WASM_LIBGIT2_VERSION}")
    endif()
    list(APPEND sources "${source}/${file}")
endforeach()
# (C90, as libgit2's CMake has it, but for the http-parser it bundles)
set(c90 ${sources})
list(FILTER c90 EXCLUDE REGEX "/deps/http-parser/")
set_source_files_properties(${c90} PROPERTIES COMPILE_OPTIONS -std=c90)

# The notice of LibXDiff, from the comment that starts src/xdiff/xdiff.h
file(READ "${source}/src/xdiff/xdiff.h" xdiff)
string(FIND "${xdiff}" "*/" end)
if(NOT xdiff MATCHES "^/\\*" OR end LESS 0 OR NOT xdiff MATCHES "GNU Lesser General Public")
    message(FATAL_ERROR "${source}/src/xdiff/xdiff.h does not start with the LGPL notice of LibXDiff")
endif()
math(EXPR end "${end} + 2")
string(SUBSTRING "${xdiff}" 0 ${end} xdiff)
file(CONFIGURE OUTPUT "${dir}/xdiff-NOTICE.txt" @ONLY CONTENT
"libgit2 includes LibXDiff (its src/xdiff), which is licensed under the GNU
Lesser General Public License version 2.1 or later rather than the GPL of
libgit2: the text of the LGPL 2.1 is in COPYING above.  The notice
of its sources (src/xdiff/xdiff.h):

@xdiff@
")
# (added to the notice of libgit2 that pharo_wasm_dep_fetch recorded)
get_property(notices GLOBAL PROPERTY PHARO_WASM_NOTICES)
set(found OFF)
set(items "")
foreach(item IN LISTS notices)
    if(item MATCHES "^libgit2\\|")
        string(APPEND item "|${dir}/xdiff-NOTICE.txt")
        set(found ON)
    endif()
    list(APPEND items "${item}")
endforeach()
if(NOT found)
    message(FATAL_ERROR "cmake/emscripten/deps/libgit2.cmake: pharo_wasm_dep_fetch recorded no notice of libgit2")
endif()
set_property(GLOBAL PROPERTY PHARO_WASM_NOTICES "${items}")

add_library(pharo_git2 STATIC ${sources})
target_include_directories(pharo_git2
    PRIVATE "${CMAKE_CURRENT_LIST_DIR}/libgit2" "${source}/src" "${source}/deps/http-parser"
    PUBLIC "${source}/include")
# (the definitions of libgit2's CMake; NDEBUG in its builds but Debug)
target_compile_definitions(pharo_git2 PRIVATE
    _GNU_SOURCE _FILE_OFFSET_BITS=64
    SHA1DC_NO_STANDARD_INCLUDES=1
    "SHA1DC_CUSTOM_INCLUDE_SHA1_C=\"common.h\""
    "SHA1DC_CUSTOM_INCLUDE_UBC_CHECK_C=\"common.h\""
    $<$<NOT:$<CONFIG:Debug>>:NDEBUG>)
target_link_libraries(pharo_git2 PRIVATE pharo_zlib)
pharo_wasm_dep_flags(pharo_git2 -Os)
