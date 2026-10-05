# The settings of the libraries of the Emscripten VM
#
# cmake/Emscripten.cmake includes this file before it adds the directory of
# the libraries (cmake/emscripten/deps/CMakeLists.txt).  It declares the
# settings, as cache variables (`make wasm' gives them with -D, and records
# them in build-wasm/config.make), and computes what comes of them as normal
# variables of the top directory, which the deps directory, the registry of
# the FFI (cmake/emscripten/ffiLibraries.cmake) and the staging read:
#
#     PHARO_WASM_HAS_FFI   the FFI: libffi, the same-thread runner of src/ffi
#                          and the registry of the libraries it calls
#
# Every library is an FFI library, so none is built without the FFI.  One
# STATUS line gives the result.

# The FFI.  The initial cache (cmake/Emscripten.cache.cmake) declares
# WASM_FFI and sets FEATURE_FFI from it, since CMakeLists.txt reads
# FEATURE_FFI; FEATURE_FFI is kept in step here too, for a `cmake
# -DWASM_FFI=...' of a configured tree without the initial cache.
option(WASM_FFI "Build the FFI (libffi, the same-thread runner and the library registry)" ON)
if((WASM_FFI AND NOT FEATURE_FFI) OR (FEATURE_FFI AND NOT WASM_FFI))
    set(FEATURE_FFI ${WASM_FFI} CACHE BOOL "Enable FFI" FORCE)
endif()

# Where the pinned archives of the libraries are, for an offline build, by
# their file names (cmake/emscripten/deps/fetch.cmake).  Without it they are
# downloaded into ${WASM_STAGE_DIR}/downloads.
set(WASM_DEPS_DIR "" CACHE PATH "A directory holding the pinned archives of the libraries, instead of downloading them")

# Developer options of the FFI (CMake only, not settings of make wasm): the
# whole C library in the registry rather than the functions the images call
# (about +100 KB gzipped), and the library of the FFI tests in pharo-web too
# (it is always in the node VM, for lane 58).  ffiLibraries.cmake reads them.
option(WASM_FFI_LIBC_ALL "List every function of the C library for the FFI, not only those the images call" OFF)
option(WASM_FFI_TEST_LIBRARY "Link the FFI test library (libTestLibrary.so) into pharo-web too" OFF)

if(WASM_FFI)
    set(PHARO_WASM_HAS_FFI ON)
    message(STATUS "wasm deps: ffi ON")
else()
    set(PHARO_WASM_HAS_FFI OFF)
    message(STATUS "wasm deps: ffi OFF (WASM_FFI=OFF): no library is built")
endif()
