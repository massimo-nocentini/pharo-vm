# libpng of the Emscripten VM
#
# cmake/emscripten/deps/cairo.cmake includes this file, with cairo
# (PHARO_WASM_HAS_CAIRO, cmake/emscripten/deps/options.cmake): cairo reads
# and writes PNG files with it (cairo_image_surface_create_from_png,
# cairo_surface_write_to_png, which the image binds).  The target,
# pharo_png, is not in the registry of the FFI: no image binds libpng.
#
# This file takes the pinned release archive (cmake/emscripten/deps/
# fetch.cmake), the one of libpng's own download site, which Emscripten's
# port of libpng mirrors byte for byte, and builds libpng as the static
# library pharo_png on pharo_zlib (cmake/emscripten/deps/zlib.cmake), with
# the flags of the deps directory: no CMake nor configure of libpng.  Its
# configuration, pnglibconf.h, is the archive's scripts/pnglibconf.h.prebuilt
# (libpng's default options), copied where pharo_png and its users find it;
# Emscripten's port has the same definitions.  libpng reports its errors with
# setjmp and longjmp, compiled in the mode of the VM (the deps directory's
# flags).  The sources of its optimisations for ARM, Intel, MIPS, PowerPC,
# LoongArch and RISC-V are left out, as libpng's CMake leaves them out for
# other processors: pngpriv.h turns them off on WebAssembly.

set(WASM_LIBPNG_VERSION "1.6.58")

include(${CMAKE_CURRENT_LIST_DIR}/zlib.cmake)

pharo_wasm_dep_fetch(libpng
    VERSION ${WASM_LIBPNG_VERSION}
    URL "https://download.sourceforge.net/libpng/libpng-${WASM_LIBPNG_VERSION}.tar.gz"
    SHA256 8c9b05b675ca7301a458df2c2e46f26e1d41ff36b8863f8c33530bc58c2e6225
    FILE libpng-${WASM_LIBPNG_VERSION}.tar.gz
    LICENSES LICENSE)
set(source "${libpng_SOURCE_DIR}")
set(dir "${CMAKE_CURRENT_BINARY_DIR}/libpng")

# The library (the sources of libpng's own CMake, without pngtest.c)
set(PHARO_WASM_LIBPNG_SOURCES "")
foreach(file
        png.c pngerror.c pngget.c pngmem.c pngpread.c pngread.c pngrio.c
        pngrtran.c pngrutil.c pngset.c pngtrans.c pngwio.c pngwrite.c
        pngwtran.c pngwutil.c)
    if(NOT EXISTS "${source}/${file}")
        message(FATAL_ERROR "${source} has no ${file}: it is not libpng ${WASM_LIBPNG_VERSION}")
    endif()
    list(APPEND PHARO_WASM_LIBPNG_SOURCES "${source}/${file}")
endforeach()
if(NOT EXISTS "${source}/scripts/pnglibconf.h.prebuilt")
    message(FATAL_ERROR "${source} has no scripts/pnglibconf.h.prebuilt: it is not libpng ${WASM_LIBPNG_VERSION}")
endif()
configure_file("${source}/scripts/pnglibconf.h.prebuilt" "${dir}/include/pnglibconf.h" COPYONLY)

add_library(pharo_png STATIC ${PHARO_WASM_LIBPNG_SOURCES})
target_include_directories(pharo_png PUBLIC "${dir}/include" "${source}")
target_link_libraries(pharo_png PUBLIC pharo_zlib)
pharo_wasm_dep_flags(pharo_png -O2)
