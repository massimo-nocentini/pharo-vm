# The libraries of the Emscripten VM, built from their pinned archives
#
# cmake/Emscripten.cmake adds this directory once cmake/emscripten/deps/
# options.cmake computed the settings (PHARO_WASM_HAS_FFI...).  Each
# library is built here as a static library, which the VM links, and which
# its FFI finds through the registry (cmake/emscripten/ffiLibraries.cmake,
# in the top directory: the targets of this one are global).
#
#     libffi.cmake    pharo_libffi, upstream libffi (with the FFI)
#     freetype.cmake  pharo_freetype, FreeType for the image's fonts
#                     (PHARO_WASM_HAS_FREETYPE)
#     cairo.cmake     pharo_cairo, cairo for Athens (PHARO_WASM_HAS_CAIRO),
#                     on FreeType and on what it includes:
#       pixman.cmake    pharo_pixman
#       libpng.cmake    pharo_png
#       zlib.cmake      pharo_zlib (for libpng, and the PDF surface)
#
# The libraries are compiled with flags of their own, not with those of the
# VM: a directory starts with what its parent set, and CMakeLists.txt and
# cmake/Emscripten.cmake set warnings (-Wall
# -Werror=implicit-function-declaration), -g, the optimisation (-O0 for a
# Debug build), definitions (LSB_FIRST, SOURCE_PATH_SIZE...) and the
# setjmp/longjmp flags, for the VM.  This directory starts again from the
# compiler flags of the initial cache (-m64, cmake/Emscripten.cache.cmake)
# and the setjmp/longjmp flags, which every object of a link must share, and
# no compile options, definitions or flags of the build type.  Each library
# then gets, with pharo_wasm_dep_flags(<target> <optimisation>), its
# optimisation, whatever the build type, -g in a Debug build only, and -w:
# their warnings are not the VM's business.

set_directory_properties(PROPERTIES COMPILE_OPTIONS "" COMPILE_DEFINITIONS "")
set(CMAKE_C_FLAGS "-m64 ${PHARO_WASM_SJLJ_C_FLAGS}")
foreach(config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set(CMAKE_C_FLAGS_${config} "")
endforeach()

# pharo_wasm_dep_flags(<target> <optimisation>): the flags of the library
# <target>, built with <optimisation> (-O2, -Os)
function(pharo_wasm_dep_flags target optimisation)
    if(CMAKE_BUILD_TYPE MATCHES "Debug")
        target_compile_options(${target} PRIVATE ${optimisation} -g -w)
    else()
        target_compile_options(${target} PRIVATE ${optimisation} -w)
    endif()
endfunction()

include(${CMAKE_CURRENT_LIST_DIR}/fetch.cmake)

if(PHARO_WASM_HAS_FFI)
    include(${CMAKE_CURRENT_LIST_DIR}/libffi.cmake)
endif()
if(PHARO_WASM_HAS_FREETYPE)
    include(${CMAKE_CURRENT_LIST_DIR}/freetype.cmake)
endif()
if(PHARO_WASM_HAS_CAIRO)
    include(${CMAKE_CURRENT_LIST_DIR}/cairo.cmake)
endif()
