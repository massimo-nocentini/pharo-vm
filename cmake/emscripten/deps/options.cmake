# The settings of the libraries of the Emscripten VM
#
# cmake/Emscripten.cmake includes this file before it adds the directory of
# the libraries (cmake/emscripten/deps/CMakeLists.txt).  It declares the
# settings, as cache variables (`make wasm' gives them with -D, and records
# them in build-wasm/config.make), and computes what comes of them as normal
# variables of the top directory, which the deps directory, the registry of
# the FFI (cmake/emscripten/ffiLibraries.cmake) and the staging read:
#
#     PHARO_WASM_HAS_FFI       the FFI: libffi, the same-thread runner of
#                              src/ffi and the registry of the libraries it
#                              calls
#     PHARO_WASM_HAS_FREETYPE  FreeType, for the fonts of the image: the
#                              world image is prepared with them
#                              (cmake/emscripten/webimage.cmake), and the
#                              manifest says so (stage.cmake)
#     PHARO_WASM_HAS_CAIRO     cairo, for Athens (Roassal, the Spec
#                              presenters drawn with Athens), with pixman,
#                              libpng and zlib, on that FreeType
#     PHARO_WASM_HAS_CAIRO_PDF the PDF, PostScript and script surfaces of
#                              cairo
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

# The libraries.  FreeType: the fonts of the image (Source Sans Pro and
# Source Code Pro) in the world, rather than bitmap fonts.
option(WASM_FREETYPE "Build FreeType, for the fonts of the image (needs WASM_FFI)" ON)
# cairo: AUTO builds it when FreeType is built (cairo draws its text with
# the one FreeType of the VM, which the image's fonts use too), ON stops
# configure without FreeType, OFF leaves it out.  The PDF, PostScript and
# script surfaces (the image binds cairo_pdf_surface_create, for
# AthensCairoPDFSurface) cost about 70 KB of gzipped wasm (170 KB raw), and
# are off by default: cairo's configuration with them is another one
# (cmake/emscripten/deps/cairo/README).
set(WASM_CAIRO AUTO CACHE STRING "Build cairo, for Athens and Roassal: AUTO (with FreeType), ON or OFF")
set_property(CACHE WASM_CAIRO PROPERTY STRINGS AUTO ON OFF)
option(WASM_CAIRO_PDF "Build the PDF, PostScript and script surfaces of cairo (with cairo)" OFF)

# WASM_CAIRO is AUTO or a boolean (ON, OFF, 1, 0, YES, NO...)
string(TOUPPER "${WASM_CAIRO}" PHARO_WASM_CAIRO_SETTING)
if(PHARO_WASM_CAIRO_SETTING STREQUAL "AUTO")
    # (decided below, from FreeType)
elseif(PHARO_WASM_CAIRO_SETTING MATCHES "^(ON|YES|Y|TRUE|1)$")
    set(PHARO_WASM_CAIRO_SETTING ON)
elseif(PHARO_WASM_CAIRO_SETTING MATCHES "^(OFF|NO|N|FALSE|0)$")
    set(PHARO_WASM_CAIRO_SETTING OFF)
else()
    message(FATAL_ERROR "WASM_CAIRO is AUTO, ON or OFF, not '${WASM_CAIRO}'")
endif()

if(WASM_FFI)
    set(PHARO_WASM_HAS_FFI ON)
    if(WASM_FREETYPE)
        set(PHARO_WASM_HAS_FREETYPE ON)
    else()
        set(PHARO_WASM_HAS_FREETYPE OFF)
    endif()
    if(PHARO_WASM_CAIRO_SETTING STREQUAL "AUTO")
        set(PHARO_WASM_HAS_CAIRO ${PHARO_WASM_HAS_FREETYPE})
        if(PHARO_WASM_HAS_CAIRO)
            set(PHARO_WASM_CAIRO_STATUS "cairo ON (auto)")
        else()
            set(PHARO_WASM_CAIRO_STATUS "cairo OFF (auto: no freetype)")
        endif()
    elseif(PHARO_WASM_CAIRO_SETTING STREQUAL "ON")
        if(NOT PHARO_WASM_HAS_FREETYPE)
            message(FATAL_ERROR "WASM_CAIRO=ON needs FreeType, which WASM_FREETYPE=OFF leaves out: "
                "cairo draws its text with the FreeType of the VM.  Give WASM_FREETYPE=ON, or "
                "WASM_CAIRO=AUTO (cairo then follows FreeType) or OFF")
        endif()
        set(PHARO_WASM_HAS_CAIRO ON)
        set(PHARO_WASM_CAIRO_STATUS "cairo ON")
    else()
        set(PHARO_WASM_HAS_CAIRO OFF)
        set(PHARO_WASM_CAIRO_STATUS "cairo OFF")
    endif()
    if(PHARO_WASM_HAS_CAIRO AND WASM_CAIRO_PDF)
        set(PHARO_WASM_HAS_CAIRO_PDF ON)
        string(APPEND PHARO_WASM_CAIRO_STATUS ", cairo pdf ON")
    else()
        set(PHARO_WASM_HAS_CAIRO_PDF OFF)
        if(WASM_CAIRO_PDF)
            string(APPEND PHARO_WASM_CAIRO_STATUS ", cairo pdf OFF (no cairo)")
        endif()
    endif()
    message(STATUS "wasm deps: ffi ON, freetype ${PHARO_WASM_HAS_FREETYPE}, ${PHARO_WASM_CAIRO_STATUS}")
else()
    set(PHARO_WASM_HAS_FFI OFF)
    set(PHARO_WASM_HAS_FREETYPE OFF)
    set(PHARO_WASM_HAS_CAIRO OFF)
    set(PHARO_WASM_HAS_CAIRO_PDF OFF)
    message(STATUS "wasm deps: ffi OFF (WASM_FFI=OFF): no library is built, freetype OFF, cairo OFF")
endif()
