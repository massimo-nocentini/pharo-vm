# libffi of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file when the FFI is
# on, and cmake/importLibFFI.cmake links its target, pharo_libffi, into the
# VM.  The Pharo fork of libffi (v3.3-cmake) has no WebAssembly port; upstream
# libffi has one since 3.4.5 (src/wasm32), and for wasm64 since 3.5.2
# (src/wasm).  This file takes the pinned release archive
# (cmake/emscripten/deps/fetch.cmake) and builds its five sources as the
# static library pharo_libffi, with the flags of the deps directory: no
# autotools, ffi.h is made from include/ffi.h.in and fficonfig.h is written
# here.  Every object is compiled for 64-bit pointers once and serves both
# links, the memory64 node VM and the -sMEMORY64=2 web VM: the JavaScript half
# of src/wasm/ffi.c is the same for both, and asks the table for its index
# type at run time.
#
# libffi 3.8.0 needs $stackAlloc, $stackSave and $stackRestore in the
# JavaScript library: src/emscripten/emscriptenSupport.c adds them with
# EM_JS_DEPS (later libffi adds them itself).  Its closures call _malloc and
# _free from JavaScript, and grow the table, so the links also need
# -sALLOW_TABLE_GROWTH and those exports (cmake/Emscripten.cmake).
#
# The return of a 64-bit integer.  ffi_call is JavaScript here: it calls the
# function through the table, and stores what comes back, which is a BigInt
# for a function that returns an i64.  A function that returns an i32, which
# the image declares as returning 64 bits (FreeType's FT_Error is an int, the
# image's a long), hands back a Number, which libffi stores into a
# BigUint64Array, and that throws.  On x86-64 the declaration works: the
# callee writes eax, and the caller reads rax.  So src/wasm/ffi.c is
# compiled from a copy in which the SINT64 return sign-extends a Number and
# the UINT64 one zero-extends it (as a callee writing eax leaves rax).  The
# text the copy replaces must occur exactly once in the archive's ffi.c, or
# configure stops: another libffi needs this file looked at again.
#
# The same file applies the patch alone, for lane 05:
#
#     cmake -DFFI_C=<src/wasm/ffi.c> -DOUTPUT=<copy> -P libffi.cmake

# pharo_wasm_libffi_widen(<ffi.c> <copy>): writes <copy>, src/wasm/ffi.c
# with the returns of 64-bit integers widened, when its contents change
function(pharo_wasm_libffi_widen ffi_c output)
    file(READ "${ffi_c}" text)
    set(anchor "  case FFI_TYPE_UINT64:\n  case FFI_TYPE_SINT64:\n    DEREF_U64(rvalue, 0) = result;\n")
    string(FIND "${text}" "${anchor}" first)
    string(FIND "${text}" "${anchor}" last REVERSE)
    if(first EQUAL -1)
        message(FATAL_ERROR "${ffi_c} has no return of the 64-bit integers to widen "
            "(cmake/emscripten/deps/libffi.cmake): it is not the ffi.c of libffi ${WASM_LIBFFI_VERSION}")
    endif()
    if(NOT first EQUAL last)
        message(FATAL_ERROR "${ffi_c} has the return of the 64-bit integers more than once "
            "(cmake/emscripten/deps/libffi.cmake): it is not the ffi.c of libffi ${WASM_LIBFFI_VERSION}")
    endif()
    set(widened "  /* Pharo (cmake/emscripten/deps/libffi.cmake): a callee that returns an
     i32 where the cif says 64 bits hands back a Number.  Widen it as an
     x86-64 caller sees it: sign-extended, or zero-extended for UINT64. */
  case FFI_TYPE_UINT64:
    DEREF_U64(rvalue, 0) = typeof result === 'bigint' ? result : BigInt(result >>> 0);
    break;
  case FFI_TYPE_SINT64:
    DEREF_U64(rvalue, 0) = typeof result === 'bigint' ? result : BigInt(result | 0);
")
    string(REPLACE "${anchor}" "${widened}" text "${text}")
    file(WRITE "${output}.tmp" "${text}")
    configure_file("${output}.tmp" "${output}" COPYONLY)
    file(REMOVE "${output}.tmp")
endfunction()

set(WASM_LIBFFI_VERSION "3.8.0")
set(WASM_LIBFFI_VERSION_NUMBER "30800")

if(CMAKE_SCRIPT_MODE_FILE)
    if(NOT FFI_C OR NOT OUTPUT)
        message(FATAL_ERROR "libffi.cmake: FFI_C and OUTPUT must be set")
    endif()
    pharo_wasm_libffi_widen("${FFI_C}" "${OUTPUT}")
    return()
endif()

pharo_wasm_dep_fetch(libffi
    VERSION ${WASM_LIBFFI_VERSION}
    URL "https://github.com/libffi/libffi/releases/download/v${WASM_LIBFFI_VERSION}/libffi-${WASM_LIBFFI_VERSION}.tar.gz"
    SHA256 7da3e2d9a171eb0a038f592ecad3ff2bb2550f3496d87b3b29ad0cf4430c0db4
    FILE libffi-${WASM_LIBFFI_VERSION}.tar.gz
    LICENSES LICENSE)
set(source "${libffi_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/libffi")
foreach(file include/ffi.h.in src/wasm/ffitarget.h src/wasm/ffi.c)
    if(NOT EXISTS "${source}/${file}")
        message(FATAL_ERROR "${source} has no ${file}: it is not libffi ${WASM_LIBFFI_VERSION}")
    endif()
endforeach()

# ffi.h, as configure makes it for --host=wasm64-unknown-linux
file(READ "${source}/include/ffi.h.in" header)
string(REPLACE "@VERSION@" "${WASM_LIBFFI_VERSION}" header "${header}")
string(REPLACE "@TARGET@" "wasm64" header "${header}")
string(REPLACE "@HAVE_LONG_DOUBLE@" "1" header "${header}")
string(REPLACE "@FFI_VERSION_STRING@" "${WASM_LIBFFI_VERSION}" header "${header}")
string(REPLACE "@FFI_VERSION_NUMBER@" "${WASM_LIBFFI_VERSION_NUMBER}" header "${header}")
string(REPLACE "@FFI_EXEC_TRAMPOLINE_TABLE@" "0" header "${header}")
if(header MATCHES "@[A-Z_]+@")
    message(FATAL_ERROR "${source}/include/ffi.h.in has a placeholder this file does not know: ${CMAKE_MATCH_0}")
endif()
file(WRITE "${dir}/include/ffi.h.tmp" "${header}")
configure_file("${dir}/include/ffi.h.tmp" "${dir}/include/ffi.h" COPYONLY)
file(REMOVE "${dir}/include/ffi.h.tmp")
configure_file("${source}/src/wasm/ffitarget.h" "${dir}/include/ffitarget.h" COPYONLY)
file(WRITE "${dir}/include/fficonfig.h.tmp"
"/* Written by cmake/emscripten/deps/libffi.cmake */
#define FFI_NO_RAW_API 1
#define HAVE_ALLOCA_H 1
#define HAVE_HIDDEN_VISIBILITY_ATTRIBUTE 1
#define HAVE_INTTYPES_H 1
#define HAVE_LONG_DOUBLE 1
#define HAVE_MEMCPY 1
#define HAVE_STDINT_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRING_H 1
#define STDC_HEADERS 1
#define SIZEOF_DOUBLE 8
#define SIZEOF_LONG_DOUBLE 16
#define SIZEOF_SIZE_T 8
#ifdef LIBFFI_ASM
#define FFI_HIDDEN(name) .hidden name
#else
#define FFI_HIDDEN __attribute__ ((visibility (\"hidden\")))
#endif
")
configure_file("${dir}/include/fficonfig.h.tmp" "${dir}/include/fficonfig.h" COPYONLY)
file(REMOVE "${dir}/include/fficonfig.h.tmp")

# src/wasm/ffi.c, widened.  (Configure runs again when this file changes,
# and so writes the copy again.)
pharo_wasm_libffi_widen("${source}/src/wasm/ffi.c" "${dir}/src/wasm/ffi.c")

add_library(pharo_libffi STATIC
    ${source}/src/prep_cif.c
    ${source}/src/types.c
    ${source}/src/closures.c
    ${source}/src/tramp.c
    ${dir}/src/wasm/ffi.c)
target_include_directories(pharo_libffi
    PUBLIC "${dir}/include"
    PRIVATE "${source}/include" "${source}/src")
pharo_wasm_dep_flags(pharo_libffi -O2)
