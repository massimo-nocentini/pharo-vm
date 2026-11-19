# The cairo stack of the native VMs (docs/CairoStack.md): cairo and FreeType,
# which the image loads by name (CairoLibrary, FT2FFILibrary), and the
# libraries they need, at the versions pinned in cmake/cairo-stack/pins.txt;
# with its features (CMakeLists.txt, both ON by default) Pango and PangoCairo
# (FEATURE_LIB_PANGO) and Poppler's poppler-glib (FEATURE_LIB_POPPLER, GPL:
# OFF makes a VM without GPL code), with GLib, which the image loads by name
# too (libpangocairo.st, libpoppler.st).  Without FEATURE_LIB_CAIRO both are
# off.  The feature map (which pins each feature needs) is the same here, in
# scripts/cairo-stack/build.sh and in check.sh.
#
# Included by cmake/Linux.cmake, cmake/Darwin.cmake and, for MinGW (MSYS2),
# cmake/Windows.cmake, at the top level of the configure, before
# cmake/plugins.cmake (whose CairoPlugin block reads
# PHARO_CAIRO_STACK_INCLUDE_DIRS).  FreeBSD, OpenBSD, MSVC and Cygwin never
# include it: their bundles have no cairo and no FreeType, and the image
# loads the system's.
#
# The cache entry (CMakeLists.txt):
#   PHARO_CAIRO_STACK_DIR  an installed stack prefix to bundle instead of
#                          building one (scripts/cairo-stack/build.sh --prefix)
# and, optionally, -DPHARO_CAIRO_STACK_GLIBC_MAX=<x.y> (build.sh --glibc-max).
# (PHARO_CAIRO_STACK, whose prebuilt mode bundled the old libraries of
# files.pharo.org, was removed; CMakeLists.txt refuses it.)
#
# With BUILD_BUNDLE on and FEATURE_LIB_CAIRO or FEATURE_LIB_FREETYPE2 on:
#   - without PHARO_CAIRO_STACK_DIR, the guard (pharo_cairo_stack_guard) checks
#     that the stack can be built here, and ExternalProject pharo_cairo_stack
#     runs ${CMAKE_BINARY_DIR}/cairo-stack/build-stack.sh, which runs
#     scripts/cairo-stack/build.sh (Linux, macOS) or fetch-msys2.sh (MinGW,
#     through build.sh --platform mingw) into ${CMAKE_BINARY_DIR}/cairo-stack/prefix,
#     with the archives of WASM_DEPS_DIR when there, else downloaded into
#     ${CMAKE_BINARY_DIR}/downloads;
#   - with PHARO_CAIRO_STACK_DIR, its manifest (format 2) must match pins.txt
#     (SHA256), the platform, the architecture and the macOS deployment
#     target, so that a stale stack (a CI cache) cannot slip in, and carry at
#     least the features of this build (a stack with more is fine: the bundle
#     takes only this build's);
#   - the target pharo_cairo_stack_bundle (ALL; the VM library depends on it)
#     runs this file as a script (cmake -P, PHARO_CAIRO_STACK_ACTION=bundle) at
#     build time: it copies the libraries of docs/CairoStack.md "What the
#     bundles contain" into LIBRARY_OUTPUT_DIRECTORY, symlinks as symlinks
#     (file(COPY)), and writes LICENSE.<name>, THIRD-PARTY-NOTICES.cairo-stack.txt
#     (packaging/cairo-stack/THIRD-PARTY-NOTICES.head.txt, its "@IF <cond>@"
#     blocks kept or dropped by the bundled features and the platform) and
#     CAIRO-STACK.txt (with a "Bundled features:" line, which check.sh reads)
#     next to them (macOS: Contents/Resources, and with Poppler
#     Contents/Resources/fontconfig/fonts.conf, packaging/cairo-stack/fonts.conf);
#   - it sets, for the rest of the configure: PHARO_CAIRO_STACK_ACTIVE (ON),
#     PHARO_CAIRO_STACK_FEATURES (pango, poppler or both: the features ON),
#     PHARO_CAIRO_VERSION, PHARO_FREETYPE_VERSION, and the CACHE INTERNAL
#     PHARO_CAIRO_STACK_PREFIX and PHARO_CAIRO_STACK_INCLUDE_DIRS
#     (<prefix>/include;<prefix>/include/cairo;<prefix>/include/freetype2),
#     which plugins/CairoPlugin/tests/run-c-tests.sh reads from CMakeCache.txt.
#     Both are empty when the stack is not used.
# Otherwise (BUILD_BUNDLE off, or both features off) the stack is off: the
# bundle has no cairo and no FreeType, and a STATUS line says so.
#
# The functions pharo_cairo_stack_platform, pharo_cairo_stack_guard,
# pharo_cairo_stack_read_pins and pharo_cairo_stack_check_manifest have no side
# effect on the build and work in script mode: including this file from a
# cmake -P script defines them and does nothing else.

include_guard(GLOBAL)
if(CMAKE_SCRIPT_MODE_FILE AND NOT CMAKE_MINIMUM_REQUIRED_VERSION)
    cmake_minimum_required(VERSION 3.13)    # policies of script mode (IN_LIST)
endif()

get_filename_component(PHARO_CAIRO_STACK_SOURCE_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
set(PHARO_CAIRO_STACK_PINS_FILE "${PHARO_CAIRO_STACK_SOURCE_ROOT}/cmake/cairo-stack/pins.txt")
set(PHARO_CAIRO_STACK_SCRIPTS_DIR "${PHARO_CAIRO_STACK_SOURCE_ROOT}/scripts/cairo-stack")
set(PHARO_CAIRO_STACK_NOTICE_HEAD "${PHARO_CAIRO_STACK_SOURCE_ROOT}/packaging/cairo-stack/THIRD-PARTY-NOTICES.head.txt")
set(PHARO_CAIRO_STACK_FONTS_CONF "${PHARO_CAIRO_STACK_SOURCE_ROOT}/packaging/cairo-stack/fonts.conf")
set(PHARO_CAIRO_STACK_THIS_FILE "${CMAKE_CURRENT_LIST_FILE}")
include("${CMAKE_CURRENT_LIST_DIR}/thirdPartyNotice.cmake")

# The libraries of each platform, by pins.txt name:
# "<name>[@<feature>[,<feature>]]|<glob>..." relative to <prefix>/lib
# ("?<glob>": may match nothing; "@<features>": bundled only when one of these
# features is on, otherwise always).  Only these are bundled (not
# libcairo-script-interpreter, libgthread, libgirepository, bin/, the static
# zlib, expat, libffi, PCRE2 and libjpeg-turbo, not libpng.so).
set(PHARO_CAIRO_STACK_LINUX_FILES
    "cairo|libcairo.so|libcairo.so.2|libcairo.so.2.*"
    "freetype|libfreetype.so|libfreetype.so.6|libfreetype.so.6.*"
    "pixman|libpixman-1.so|libpixman-1.so.0|libpixman-1.so.0.*"
    "libpng|libpng16.so|libpng16.so.16|libpng16.so.16.*"
    "fontconfig|libfontconfig.so|libfontconfig.so.1|libfontconfig.so.1.*"
    "glib@pango,poppler|libglib-2.0.so|libglib-2.0.so.0|libglib-2.0.so.0.*|libgobject-2.0.so|libgobject-2.0.so.0|libgobject-2.0.so.0.*|libgio-2.0.so.0|libgio-2.0.so.0.*|libgmodule-2.0.so.0|libgmodule-2.0.so.0.*"
    "fribidi@pango|libfribidi.so.0|libfribidi.so.0.*"
    "harfbuzz@pango|libharfbuzz.so.0|libharfbuzz.so.0.*"
    "pango@pango|libpango-1.0.so|libpango-1.0.so.0|libpango-1.0.so.0.*|libpangocairo-1.0.so|libpangocairo-1.0.so.0|libpangocairo-1.0.so.0.*|libpangoft2-1.0.so.0|libpangoft2-1.0.so.0.*"
    # libpoppler's soname changes with every release (166 for 26.11): the
    # image loads only poppler-glib
    "poppler@poppler|libpoppler.so.*|libpoppler-glib.so|libpoppler-glib.so.8|libpoppler-glib.so.8.*")
set(PHARO_CAIRO_STACK_DARWIN_FILES
    "cairo|libcairo.dylib|libcairo.2.dylib"
    "freetype|libfreetype.dylib|libfreetype.6.dylib|?libfreetype.6.*.dylib"
    "pixman|libpixman-1.dylib|libpixman-1.0.dylib|?libpixman-1.0.*.dylib"
    "libpng|libpng16.dylib|libpng16.16.dylib|?libpng16.16.*.dylib"
    "glib@pango,poppler|?libglib-2.0.dylib|libglib-2.0.0.dylib|?libgobject-2.0.dylib|libgobject-2.0.0.dylib|libgio-2.0.0.dylib|libgmodule-2.0.0.dylib"
    "fribidi@pango|libfribidi.0.dylib"
    "harfbuzz@pango|libharfbuzz.0.dylib"
    "pango@pango|?libpango-1.0.dylib|libpango-1.0.0.dylib|?libpangocairo-1.0.dylib|libpangocairo-1.0.0.dylib"
    # Poppler's own fontconfig (macOS: D5 of the design, docs/CairoStack.md)
    "fontconfig@poppler|?libfontconfig.dylib|libfontconfig.1.dylib"
    "poppler@poppler|libpoppler.*.dylib|libpoppler-glib.8.dylib|?libpoppler-glib.8.*.dylib|?libpoppler-glib.dylib")
# Libraries that only cairo needs (dropped with --only freetype)
set(PHARO_CAIRO_STACK_CAIRO_ONLY cairo pixman fontconfig expat)
# The features (FEATURE_LIB_PANGO, FEATURE_LIB_POPPLER), in their fixed order
set(PHARO_CAIRO_STACK_ALL_FEATURES pango poppler)

# ---------------------------------------------------------------- functions

# pharo_cairo_stack_platform(<out> SYSTEM <CMAKE_SYSTEM_NAME> [MINGW <bool>] [CYGWIN <bool>])
# linux, darwin or mingw: where the configure can build the stack; "" elsewhere
function(pharo_cairo_stack_platform out)
    cmake_parse_arguments(A "" "SYSTEM;MINGW;CYGWIN" "" ${ARGN})
    set(p "")
    if(A_MINGW AND NOT A_CYGWIN)
        set(p mingw)
    elseif(A_SYSTEM STREQUAL "Linux")
        set(p linux)
    elseif(A_SYSTEM STREQUAL "Darwin")
        set(p darwin)
    endif()
    set(${out} "${p}" PARENT_SCOPE)
endfunction()

# pharo_cairo_stack_arch(<out> PLATFORM <p> PROCESSOR <CMAKE_SYSTEM_PROCESSOR>
#                        [OSX_ARCHITECTURES <list>])
# The architecture as the manifest names it: uname -m on Linux (x86_64,
# aarch64), x86_64 or arm64 on macOS and Windows.
function(pharo_cairo_stack_arch out)
    cmake_parse_arguments(A "" "PLATFORM;PROCESSOR" "OSX_ARCHITECTURES" ${ARGN})
    set(a "${A_PROCESSOR}")
    if(A_PLATFORM STREQUAL "darwin")
        list(LENGTH A_OSX_ARCHITECTURES n)
        if(n EQUAL 1)
            set(a "${A_OSX_ARCHITECTURES}")
        endif()
    elseif(A_PLATFORM STREQUAL "mingw")
        string(TOLOWER "${a}" a)
        if(a MATCHES "^(amd64|x86_64|x64)$")
            set(a x86_64)
        elseif(a MATCHES "^(arm64|aarch64)$")
            set(a arm64)
        endif()
    endif()
    set(${out} "${a}" PARENT_SCOPE)
endfunction()

# pharo_cairo_stack_pin_features(<out> <pins.txt name> <platform>)
# The feature map: the features whose libraries the pin is (a list), or core
# (cairo's and FreeType's, always there).  The same map as build.sh's and
# check.sh's feats().
function(pharo_cairo_stack_pin_features out name platform)
    if(name MATCHES "^(libffi|pcre2|proxy-libintl|glib)$")
        set(f pango poppler)
    elseif(name MATCHES "^(fribidi|harfbuzz|pango)$")
        set(f pango)
    elseif(name MATCHES "^(libjpeg-turbo|poppler)$")
        set(f poppler)
    elseif(name MATCHES "^(expat|fontconfig)$" AND platform STREQUAL "darwin")
        set(f poppler)
    else()
        set(f core)
    endif()
    set(${out} "${f}" PARENT_SCOPE)
endfunction()

# pharo_cairo_stack_wanted(<out> "<features of an item>" "<features on>")
# TRUE when the item is core or one of its features is on
function(pharo_cairo_stack_wanted out item_features features)
    set(w FALSE)
    foreach(f IN LISTS item_features)
        if(f STREQUAL "core" OR f IN_LIST features)
            set(w TRUE)
        endif()
    endforeach()
    set(${out} ${w} PARENT_SCOPE)
endfunction()

# pharo_cairo_stack_guard(<out error> <out tool dirs>
#     SYSTEM <CMAKE_SYSTEM_NAME> [MINGW <bool>] [CYGWIN <bool>]
#     [CROSSCOMPILING <bool>] [OSX_ARCHITECTURES <list>]
#     HOST_PROCESSOR <CMAKE_HOST_SYSTEM_PROCESSOR> [PROCESSOR <CMAKE_SYSTEM_PROCESSOR>]
#     [POINTER_SIZE <bytes>] [FEATURES <pango and/or poppler>]
#     [CXX_COMPILER <path>] [CXX_COMPILER_ID <id>] [CXX_COMPILER_VERSION <v>])
# Whether the configure can build the stack here: <out error> is "" when it can,
# otherwise the reason.  The tools are looked for on PATH (the environment's,
# as build.sh will see it): sh, meson >= 1.11, ninja, pkg-config or pkgconf,
# cmake and, on Linux, gperf; on MinGW (MSYS2's binary packages) sh, curl,
# zstd and tar.  With a feature (Linux, macOS) also python3 (or python),
# cmake >= 3.28 and the C++ compiler (HarfBuzz, Poppler); Linux: GCC 13's
# libstdc++ (g++-13 for clang, unless PHARO_CAIRO_STACK_GCC_INSTALL_DIR is
# set; a GCC must be 13), and with Poppler on x86_64 nasm (libjpeg-turbo's
# SIMD); macOS with Poppler: gperf (its fontconfig).  MinGW with Poppler
# (built from source there): cmake >= 3.28, ninja, pkgconf, python3 and the
# C++ compiler.  <out tool dirs> lists the directories they were found in,
# for the PATH of build-stack.sh.
function(pharo_cairo_stack_guard outError outDirs)
    cmake_parse_arguments(A "" "SYSTEM;MINGW;CYGWIN;CROSSCOMPILING;HOST_PROCESSOR;PROCESSOR;POINTER_SIZE;CXX_COMPILER;CXX_COMPILER_ID;CXX_COMPILER_VERSION"
        "OSX_ARCHITECTURES;FEATURES" ${ARGN})
    set(${outError} "" PARENT_SCOPE)
    set(${outDirs} "" PARENT_SCOPE)
    pharo_cairo_stack_platform(platform SYSTEM "${A_SYSTEM}" MINGW "${A_MINGW}" CYGWIN "${A_CYGWIN}")
    if(platform STREQUAL "")
        set(${outError} "the stack is built on Linux, macOS and MinGW (MSYS2) only, not on ${A_SYSTEM}" PARENT_SCOPE)
        return()
    endif()
    if(A_CROSSCOMPILING)
        set(${outError} "cross-compiling: the stack is built natively, for the machine that builds it" PARENT_SCOPE)
        return()
    endif()
    if(platform STREQUAL "darwin")
        list(LENGTH A_OSX_ARCHITECTURES n)
        if(n GREATER 1)
            set(${outError} "CMAKE_OSX_ARCHITECTURES is '${A_OSX_ARCHITECTURES}': the stack is built for one architecture, this Mac's (${A_HOST_PROCESSOR})" PARENT_SCOPE)
            return()
        endif()
        if(n EQUAL 1 AND NOT A_OSX_ARCHITECTURES STREQUAL A_HOST_PROCESSOR)
            set(${outError} "CMAKE_OSX_ARCHITECTURES is ${A_OSX_ARCHITECTURES}, not this Mac's architecture (${A_HOST_PROCESSOR}): the stack is built natively" PARENT_SCOPE)
            return()
        endif()
    elseif(platform STREQUAL "linux")
        if(A_PROCESSOR AND NOT A_PROCESSOR STREQUAL A_HOST_PROCESSOR)
            set(${outError} "the target processor ${A_PROCESSOR} is not this machine's (${A_HOST_PROCESSOR}): the stack is built natively" PARENT_SCOPE)
            return()
        endif()
        if(A_POINTER_SIZE EQUAL 4 AND A_HOST_PROCESSOR MATCHES "^(x86_64|amd64|aarch64|arm64|ppc64le|riscv64|s390x)$")
            set(${outError} "a 32-bit VM on a 64-bit ${A_HOST_PROCESSOR} host: the stack would be built for the host's 64-bit ABI" PARENT_SCOPE)
            return()
        endif()
    endif()

    set(any_feature OFF)
    if(A_FEATURES)
        set(any_feature ON)
    endif()
    set(poppler OFF)
    if("poppler" IN_LIST A_FEATURES)
        set(poppler ON)
    endif()
    # what the features need: a C++ compiler, python3 and cmake >= 3.28
    set(need_cxx OFF)
    set(gxx13 OFF)
    if(platform STREQUAL "mingw")
        set(tools "sh" "curl" "zstd" "tar")
        if(poppler)
            list(APPEND tools "cmake" "ninja" "pkgconf" "python3|python")
            set(need_cxx ON)
        endif()
    else()
        set(tools "sh" "meson" "ninja" "pkg-config|pkgconf" "cmake")
        if(platform STREQUAL "linux")
            list(APPEND tools "gperf")
        endif()
        if(any_feature)
            list(APPEND tools "python3|python")
            set(need_cxx ON)
            if(platform STREQUAL "linux" AND A_CXX_COMPILER_ID MATCHES "Clang"
                    AND "$ENV{PHARO_CAIRO_STACK_GCC_INSTALL_DIR}" STREQUAL "")
                list(APPEND tools "g++-13")
                set(gxx13 ON)
            endif()
        endif()
        if(poppler AND platform STREQUAL "linux" AND A_HOST_PROCESSOR MATCHES "^(x86_64|amd64|AMD64)$")
            list(APPEND tools "nasm")
        endif()
        if(poppler AND platform STREQUAL "darwin")
            list(APPEND tools "gperf")
        endif()
    endif()
    set(need_cmake_328 OFF)
    if(need_cxx)
        set(need_cmake_328 ON)
        if(NOT A_CXX_COMPILER OR (IS_ABSOLUTE "${A_CXX_COMPILER}" AND NOT EXISTS "${A_CXX_COMPILER}"))
            set(${outError} "no C++ compiler (CMAKE_CXX_COMPILER '${A_CXX_COMPILER}'): HarfBuzz and Poppler are C++ (FEATURE_LIB_PANGO, FEATURE_LIB_POPPLER)" PARENT_SCOPE)
            return()
        endif()
        if(platform STREQUAL "linux" AND A_CXX_COMPILER_ID STREQUAL "GNU" AND NOT A_CXX_COMPILER_VERSION MATCHES "^13([.]|$)")
            set(${outError} "the C++ compiler is GCC ${A_CXX_COMPILER_VERSION} (${A_CXX_COMPILER}): the stack's C++ libraries need GCC 13's libstdc++ (GLIBCXX <= 3.4.32): configure with g++-13, or with clang and g++-13 installed (apt install g++-13)" PARENT_SCOPE)
            return()
        endif()
        if(platform STREQUAL "linux" AND NOT "$ENV{PHARO_CAIRO_STACK_GCC_INSTALL_DIR}" STREQUAL ""
                AND NOT IS_DIRECTORY "$ENV{PHARO_CAIRO_STACK_GCC_INSTALL_DIR}")
            set(${outError} "PHARO_CAIRO_STACK_GCC_INSTALL_DIR=$ENV{PHARO_CAIRO_STACK_GCC_INSTALL_DIR} is not a directory (GCC 13's, for libstdc++ 13)" PARENT_SCOPE)
            return()
        endif()
    endif()
    set(missing "")
    set(dirs "")
    foreach(t IN LISTS tools)
        string(REPLACE "|" ";" names "${t}")
        unset(_pcs_tool CACHE)
        unset(_pcs_tool)
        find_program(_pcs_tool NAMES ${names} NO_CMAKE_PATH NO_CMAKE_ENVIRONMENT_PATH NO_CMAKE_SYSTEM_PATH NO_CMAKE_FIND_ROOT_PATH)
        if(NOT _pcs_tool AND t STREQUAL "cmake" AND CMAKE_COMMAND)
            set(_pcs_tool "${CMAKE_COMMAND}")    # the cmake running this configure
        endif()
        if(NOT _pcs_tool)
            list(APPEND missing "${t}")
        else()
            get_filename_component(d "${_pcs_tool}" DIRECTORY)
            list(APPEND dirs "${d}")
            if(t STREQUAL "meson")
                execute_process(COMMAND "${_pcs_tool}" --version OUTPUT_VARIABLE v
                    RESULT_VARIABLE rc OUTPUT_STRIP_TRAILING_WHITESPACE ERROR_QUIET)
                if(NOT rc EQUAL 0 OR v VERSION_LESS 1.11)
                    set(where "${_pcs_tool}")
                    unset(_pcs_tool CACHE)
                    set(${outError} "meson ${v} (${where}) is too old: fontconfig needs meson >= 1.11 (pip install --require-hashes -r scripts/cairo-stack/requirements.txt, docs/CairoStack.md \"Tools\")" PARENT_SCOPE)
                    return()
                endif()
            endif()
            if(t STREQUAL "cmake" AND need_cmake_328)
                execute_process(COMMAND "${_pcs_tool}" --version OUTPUT_VARIABLE v
                    RESULT_VARIABLE rc ERROR_QUIET)
                string(REGEX MATCH "version ([0-9][0-9.]*)" vm "${v}")
                set(v "${CMAKE_MATCH_1}")
                if(NOT rc EQUAL 0 OR v STREQUAL "" OR v VERSION_LESS 3.28)
                    set(where "${_pcs_tool}")
                    unset(_pcs_tool CACHE)
                    set(${outError} "cmake ${v} (${where}) is too old for FEATURE_LIB_PANGO/FEATURE_LIB_POPPLER: Poppler 26 needs cmake >= 3.28 (docs/CairoStack.md \"Tools\")" PARENT_SCOPE)
                    return()
                endif()
            endif()
        endif()
        unset(_pcs_tool CACHE)
    endforeach()
    if(missing)
        string(REPLACE ";" ", " m "${missing}")
        string(REPLACE "|" " or " m "${m}")
        if(platform STREQUAL "mingw")
            set(how "they come with MSYS2's shell (pacman -S curl zstd tar)")
            if(poppler)
                string(APPEND how "; Poppler is built from source here: pacboy -S cmake:p ninja:p pkgconf:p python:p clang:p")
            endif()
        elseif(platform STREQUAL "linux")
            set(how "meson, ninja and pkgconf: pip install --require-hashes -r scripts/cairo-stack/requirements.txt into a venv on PATH; gperf: apt install gperf (docs/CairoStack.md \"Tools\")")
            if(any_feature)
                string(APPEND how "; python3: apt install python3")
            endif()
            if(gxx13)
                string(APPEND how "; g++-13: apt install g++-13 (libstdc++ 13 keeps GLIBCXX <= 3.4.32), or set PHARO_CAIRO_STACK_GCC_INSTALL_DIR")
            endif()
            if("nasm" IN_LIST tools)
                string(APPEND how "; nasm: apt install nasm (libjpeg-turbo's SIMD, for Poppler)")
            endif()
        else()
            set(how "meson, ninja and pkgconf: pip install --require-hashes -r scripts/cairo-stack/requirements.txt into a venv on PATH (docs/CairoStack.md \"Tools\")")
            if(poppler)
                string(APPEND how "; gperf (Poppler's fontconfig): Xcode's command-line tools have /usr/bin/gperf, else brew install gperf")
            endif()
        endif()
        set(${outError} "not on PATH: ${m}; ${how}" PARENT_SCOPE)
        return()
    endif()
    # in the order of PATH, so that build.sh finds the same tools as here
    if(CMAKE_HOST_WIN32)
        set(path "$ENV{PATH}")
    else()
        string(REPLACE ":" ";" path "$ENV{PATH}")
    endif()
    set(ordered "")
    foreach(d IN LISTS path)
        file(TO_CMAKE_PATH "${d}" d)
        if(d IN_LIST dirs AND NOT d IN_LIST ordered)
            list(APPEND ordered "${d}")
        endif()
    endforeach()
    foreach(d IN LISTS dirs)
        if(NOT d IN_LIST ordered)
            list(APPEND ordered "${d}")
        endif()
    endforeach()
    set(${outDirs} "${ordered}" PARENT_SCOPE)
endfunction()

# pharo_cairo_stack_read_pins(<pins file> <var prefix>)
# Sets, in the caller's scope:
#   <p>_SOURCES                           the source libraries, in build order
#   <p>_<name>_VERSION, _SHA256, _PLATFORMS (list), _LICENCES (list)
#                                         (_PLATFORMS may hold mingw: an archive
#                                         built on Windows over the MSYS2 prefix;
#                                         Linux and macOS build only theirs)
#   <p>_MSYS2_<env>_PACKAGES              the msys2 packages of <env>
#   <p>_MSYS2_<env>_<package>_VERSION, _KIND
#   <p>_MSYS2_<env>_DLLS                  "<dll>|<package>" per msys2-dll line
#   <p>_MSYS2_DLL_<env>_<dll>_FEATURES    its 5th field as a list (core, pango,
#                                         poppler); core when the line has 4
# (msys2-root lines are fetch-msys2.sh --closure's input: skipped.)
function(pharo_cairo_stack_read_pins pins p)
    if(NOT EXISTS "${pins}")
        message(FATAL_ERROR "cairoStack.cmake: no ${pins}")
    endif()
    file(STRINGS "${pins}" lines)
    set(sources "")
    foreach(line IN LISTS lines)
        if(line MATCHES "^[ \t]*(#|$)")
            continue()
        endif()
        string(STRIP "${line}" line)
        string(REGEX REPLACE "[ \t]+" ";" f "${line}")
        list(GET f 0 kind)
        list(LENGTH f n)
        if(kind STREQUAL "mirror" OR kind STREQUAL "native-ahead" OR kind STREQUAL "accepted-behind" OR kind STREQUAL "msys2-mirror"
                OR kind STREQUAL "msys2-root")
            continue()
        elseif(kind STREQUAL "msys2")
            list(GET f 1 env)
            list(GET f 2 k)
            list(GET f 3 pkg)
            list(GET f 4 ver)
            set(${p}_MSYS2_${env}_PACKAGES ${${p}_MSYS2_${env}_PACKAGES} "${pkg}")
            set(${p}_MSYS2_${env}_${pkg}_VERSION "${ver}" PARENT_SCOPE)
            set(${p}_MSYS2_${env}_${pkg}_KIND "${k}" PARENT_SCOPE)
        elseif(kind STREQUAL "msys2-dll")
            if(NOT (n EQUAL 4 OR n EQUAL 5))
                message(FATAL_ERROR "cairoStack.cmake: ${pins}: 'msys2-dll <env> <dll> <package> [<features>]': '${line}'")
            endif()
            list(GET f 1 env)
            list(GET f 2 dll)
            list(GET f 3 pkg)
            set(feats "core")
            if(n EQUAL 5)
                list(GET f 4 feats)
                string(REPLACE "," ";" feats "${feats}")
            endif()
            set(${p}_MSYS2_${env}_DLLS ${${p}_MSYS2_${env}_DLLS} "${dll}|${pkg}")
            set(${p}_MSYS2_DLL_${env}_${dll}_FEATURES "${feats}" PARENT_SCOPE)
        elseif(n GREATER_EQUAL 6)
            list(GET f 1 ver)
            list(GET f 2 sha)
            list(GET f 5 plats)
            list(SUBLIST f 6 -1 lic)
            string(REPLACE "," ";" plats "${plats}")
            list(APPEND sources "${kind}")
            set(${p}_${kind}_VERSION "${ver}" PARENT_SCOPE)
            set(${p}_${kind}_SHA256 "${sha}" PARENT_SCOPE)
            set(${p}_${kind}_PLATFORMS "${plats}" PARENT_SCOPE)
            set(${p}_${kind}_LICENCES "${lic}" PARENT_SCOPE)
        else()
            message(FATAL_ERROR "cairoStack.cmake: ${pins}: cannot read the line '${line}'")
        endif()
    endforeach()
    set(${p}_SOURCES "${sources}" PARENT_SCOPE)
    foreach(env ucrt64 clangarm64)
        set(${p}_MSYS2_${env}_PACKAGES "${${p}_MSYS2_${env}_PACKAGES}" PARENT_SCOPE)
        set(${p}_MSYS2_${env}_DLLS "${${p}_MSYS2_${env}_DLLS}" PARENT_SCOPE)
    endforeach()
endfunction()

# pharo_cairo_stack_check_manifest(<out errors> MANIFEST <file> PINS <file>
#     PLATFORM <p> ARCH <a> DEPLOYMENT_TARGET <t or -> [NEED_CAIRO <bool>]
#     [FEATURES <pango and/or poppler>])
# <out errors>: "" when the stack of the manifest is the one this build
# needs, otherwise every mismatch, "; "-separated.  The manifest is format 2;
# its "features" line must hold every one of FEATURES (more is fine).  The
# tool lines are not compared (informational).
function(pharo_cairo_stack_check_manifest out)
    cmake_parse_arguments(A "" "MANIFEST;PINS;PLATFORM;ARCH;DEPLOYMENT_TARGET;NEED_CAIRO" "FEATURES" ${ARGN})
    if(NOT EXISTS "${A_MANIFEST}")
        set(${out} "no manifest ${A_MANIFEST} (not a prefix of scripts/cairo-stack/build.sh)" PARENT_SCOPE)
        return()
    endif()
    file(STRINGS "${A_MANIFEST}" lines)
    foreach(k format pins-sha256 platform arch deployment-target only)
        set(m_${k} "")
    endforeach()
    set(m_features "")
    set(has_features OFF)
    foreach(line IN LISTS lines)
        if(line MATCHES "^(format|pins-sha256|platform|arch|deployment-target|only)[ \t]+([^ \t]+)")
            set(m_${CMAKE_MATCH_1} "${CMAKE_MATCH_2}")
        elseif(line MATCHES "^features[ \t]+(.*)$")
            set(has_features ON)
            string(STRIP "${CMAKE_MATCH_1}" fl)
            string(REGEX REPLACE "[ \t,]+" ";" m_features "${fl}")
            list(REMOVE_ITEM m_features "-")
        endif()
    endforeach()
    file(SHA256 "${A_PINS}" pins_sha)
    set(errors "")
    if(NOT m_format STREQUAL "2")
        if(m_format STREQUAL "1")
            list(APPEND errors "format 1, not 2: a stack of an older scripts/cairo-stack/build.sh or fetch-msys2.sh, from before the Pango and Poppler features (no features line)")
        else()
            list(APPEND errors "format '${m_format}', not 2")
        endif()
    elseif(NOT has_features)
        list(APPEND errors "no features line")
    else()
        set(missing "")
        foreach(f IN LISTS A_FEATURES)
            if(NOT f IN_LIST m_features)
                list(APPEND missing "${f}")
            endif()
        endforeach()
        if(missing)
            string(REPLACE ";" " " missing "${missing}")
            if(m_features)
                string(REPLACE ";" " " have "${m_features}")
            else()
                set(have "none")
            endif()
            list(APPEND errors "built without the features ${missing} (its features: ${have}), which FEATURE_LIB_PANGO/FEATURE_LIB_POPPLER ask for")
        endif()
    endif()
    if(NOT m_pins-sha256 STREQUAL pins_sha)
        list(APPEND errors "made from another pins.txt (pins-sha256 ${m_pins-sha256}, ${A_PINS} has ${pins_sha})")
    endif()
    if(NOT m_platform STREQUAL A_PLATFORM)
        list(APPEND errors "platform '${m_platform}', this build is ${A_PLATFORM}")
    endif()
    if(NOT m_arch STREQUAL A_ARCH)
        list(APPEND errors "arch '${m_arch}', this build is ${A_ARCH}")
    endif()
    if(NOT m_deployment-target STREQUAL A_DEPLOYMENT_TARGET)
        list(APPEND errors "deployment-target '${m_deployment-target}', this build's is ${A_DEPLOYMENT_TARGET}")
    endif()
    if(A_NEED_CAIRO AND m_only STREQUAL "freetype")
        list(APPEND errors "built with --only freetype, and FEATURE_LIB_CAIRO is on")
    endif()
    string(REPLACE ";" "; " errors "${errors}")
    set(${out} "${errors}" PARENT_SCOPE)
endfunction()

# _pharo_cairo_stack_sh_quote(<out> <string>): '<string>' for sh
function(_pharo_cairo_stack_sh_quote out s)
    string(REPLACE "'" "'\\''" s "${s}")
    set(${out} "'${s}'" PARENT_SCOPE)
endfunction()

# _pharo_cairo_stack_write_if_different(<path> <content>)
function(_pharo_cairo_stack_write_if_different path content)
    if(EXISTS "${path}")
        file(READ "${path}" old)
        if(old STREQUAL content)
            return()
        endif()
    endif()
    file(WRITE "${path}" "${content}")
endfunction()

# _pharo_cairo_stack_licence_text(<out> <name> <version> <files...>)
# The concatenation of <files> (absolute paths, shown relative to their
# licences/<name> directory), each after a line "==> <name> <version>: <file> <==".
# A C source (FreeType's src/base/fthash.c) contributes its MIT notice only.
function(_pharo_cairo_stack_licence_text out name version base)
    set(text "")
    foreach(f IN LISTS ARGN)
        file(RELATIVE_PATH rel "${base}" "${f}")
        if(f MATCHES "\\.[ch]$")
            pharo_third_party_notice_text(body "${name}" "${f}" MIT START "Copyright" END "*/")
        else()
            file(READ "${f}" body)
        endif()
        if(NOT body MATCHES "\n$")
            string(APPEND body "\n")
        endif()
        string(APPEND text "==> ${name} ${version}: ${rel} <==\n${body}\n")
    endforeach()
    set(${out} "${text}" PARENT_SCOPE)
endfunction()

# ---------------------------------------------------------------- the bundle (cmake -P)

# _pharo_cairo_stack_notice_blocks(<out> <text> <tokens that hold>...)
# The "@IF <condition>@" ... "@ENDIF@" blocks of a notice text (each marker on
# a line of its own, blocks not nested): kept without their markers when the
# condition holds, dropped otherwise.  A condition is a space-separated list
# of tokens that must all hold; "!<token>" holds when <token> does not.
function(_pharo_cairo_stack_notice_blocks out text)
    set(holds ${ARGN})
    set(result "")
    while(TRUE)
        string(FIND "${text}" "@IF " i)
        if(i EQUAL -1)
            string(APPEND result "${text}")
            break()
        endif()
        string(SUBSTRING "${text}" 0 ${i} before)
        string(APPEND result "${before}")
        string(SUBSTRING "${text}" ${i} -1 text)
        string(FIND "${text}" "\n" nl)
        string(SUBSTRING "${text}" 0 ${nl} head)
        if(nl EQUAL -1 OR NOT head MATCHES "^@IF ([^@]+)@$" OR NOT (before STREQUAL "" OR before MATCHES "\n$"))
            message(FATAL_ERROR "cairoStack.cmake: a bad @IF line in the notice: '${head}'")
        endif()
        string(REPLACE " " ";" condition "${CMAKE_MATCH_1}")
        math(EXPR nl "${nl} + 1")
        string(SUBSTRING "${text}" ${nl} -1 text)
        string(FIND "${text}" "@ENDIF@\n" e)
        if(e EQUAL -1)
            message(FATAL_ERROR "cairoStack.cmake: no @ENDIF@ line after '${head}' in the notice")
        endif()
        string(SUBSTRING "${text}" 0 ${e} body)
        math(EXPR e "${e} + 8")
        string(SUBSTRING "${text}" ${e} -1 text)
        string(FIND "${body}" "@IF " nested)
        if(NOT nested EQUAL -1 OR NOT (body STREQUAL "" OR body MATCHES "\n$"))
            message(FATAL_ERROR "cairoStack.cmake: a nested @IF, or @ENDIF@ not on a line of its own, after '${head}' in the notice")
        endif()
        set(keep TRUE)
        foreach(t IN LISTS condition)
            if(t MATCHES "^!(.+)$")
                if(CMAKE_MATCH_1 IN_LIST holds)
                    set(keep FALSE)
                endif()
            elseif(NOT t IN_LIST holds)
                set(keep FALSE)
            endif()
        endforeach()
        if(keep)
            string(APPEND result "${body}")
        endif()
    endwhile()
    set(${out} "${result}" PARENT_SCOPE)
endfunction()

# Variables: PCS_PREFIX PCS_PINS PCS_HEAD PCS_FONTS_CONF PCS_PLATFORM PCS_ARCH
# PCS_DEPLOYMENT_TARGET PCS_ENV (mingw) PCS_MSYS2_PKG_ARCH (mingw) PCS_CAIRO
# (ON/OFF) PCS_FEATURES (a comma list of pango and poppler, or none)
# PCS_SOURCE_OFFER_URL PCS_LIB_DIR PCS_NOTICE_DIR PCS_WORK
function(_pharo_cairo_stack_bundle)
    set(share "${PCS_PREFIX}/share/pharo-cairo-stack")
    string(REPLACE "," ";" features "${PCS_FEATURES}")
    list(REMOVE_ITEM features none -)
    pharo_cairo_stack_check_manifest(errors MANIFEST "${share}/manifest.txt" PINS "${PCS_PINS}"
        PLATFORM "${PCS_PLATFORM}" ARCH "${PCS_ARCH}" DEPLOYMENT_TARGET "${PCS_DEPLOYMENT_TARGET}" NEED_CAIRO "${PCS_CAIRO}"
        FEATURES ${features})
    if(errors)
        message(FATAL_ERROR "cairoStack.cmake: the stack in ${PCS_PREFIX} is not this build's: ${errors}")
    endif()
    pharo_cairo_stack_read_pins("${PCS_PINS}" pin)
    file(MAKE_DIRECTORY "${PCS_LIB_DIR}" "${PCS_NOTICE_DIR}" "${PCS_WORK}")

    # 1. the libraries of cairo, FreeType and the features on
    set(names "")       # whose LICENSE.<name> to write: "<name>|<package or pin>|<msys2, built or source>"
    set(versions "")
    if(PCS_PLATFORM STREQUAL "mingw")
        set(dlls_txt "${share}/dlls.txt")
        if(NOT EXISTS "${dlls_txt}")
            message(FATAL_ERROR "cairoStack.cmake: no ${dlls_txt} (a prefix of scripts/cairo-stack/fetch-msys2.sh has one)")
        endif()
        file(STRINGS "${dlls_txt}" lines)
        set(files "")
        set(runtime_pkgs "")
        foreach(line IN LISTS lines)
            string(STRIP "${line}" line)
            if(line STREQUAL "" OR line MATCHES "^#")
                continue()
            endif()
            string(REGEX REPLACE "[ \t]+" ";" f "${line}")
            list(LENGTH f n)
            list(GET f 0 dll)
            list(GET f 1 kind)
            list(GET f 2 pkg)
            set(feats core)
            if(n GREATER_EQUAL 5)
                list(GET f 4 feats)
                string(REPLACE "," ";" feats "${feats}")
            endif()
            string(REGEX REPLACE "^mingw-w64-[a-z0-9]+-[a-z0-9_]+-" "" short "${pkg}")
            if(kind STREQUAL "runtime")
                # the C/C++ runtime: the toolchain's, which the install step
                # bundles (configure_installables, cmake/Windows.cmake)
                list(APPEND runtime_pkgs "${short}|${pkg}")
                continue()
            endif()
            pharo_cairo_stack_wanted(wanted "${feats}" "${features}")
            if(NOT wanted OR (dll STREQUAL "libcairo-2.dll" AND NOT PCS_CAIRO))
                continue()
            endif()
            list(APPEND files "${PCS_PREFIX}/bin/${dll}")
            if(kind STREQUAL "built")
                # built from the source pin named in the package field (Poppler)
                if(NOT DEFINED pin_${pkg}_VERSION)
                    message(FATAL_ERROR "cairoStack.cmake: ${dlls_txt}: ${dll} is built from '${pkg}', which is no source pin of ${PCS_PINS}")
                endif()
                list(APPEND names "${pkg}|${pkg}|built")
            else()
                list(APPEND names "${short}|${pkg}|msys2")
            endif()
        endforeach()
        list(REMOVE_DUPLICATES names)
        list(REMOVE_DUPLICATES runtime_pkgs)
    else()
        if(PCS_PLATFORM STREQUAL "linux")
            set(table ${PHARO_CAIRO_STACK_LINUX_FILES})
        else()
            set(table ${PHARO_CAIRO_STACK_DARWIN_FILES})
        endif()
        set(files "")
        foreach(entry IN LISTS table)
            string(REPLACE "|" ";" entry "${entry}")
            list(GET entry 0 name)
            list(REMOVE_AT entry 0)
            set(feats core)
            if(name MATCHES "^([^@]+)@(.+)$")
                set(name "${CMAKE_MATCH_1}")
                string(REPLACE "," ";" feats "${CMAKE_MATCH_2}")
            endif()
            pharo_cairo_stack_wanted(wanted "${feats}" "${features}")
            if(NOT wanted OR (NOT PCS_CAIRO AND name IN_LIST PHARO_CAIRO_STACK_CAIRO_ONLY))
                continue()
            endif()
            foreach(g IN LISTS entry)
                set(optional FALSE)
                if(g MATCHES "^\\?(.*)")
                    set(g "${CMAKE_MATCH_1}")
                    set(optional TRUE)
                endif()
                file(GLOB found LIST_DIRECTORIES false "${PCS_PREFIX}/lib/${g}")
                if(NOT found AND NOT optional)
                    message(FATAL_ERROR "cairoStack.cmake: ${PCS_PREFIX}/lib has no ${g} (${name})")
                endif()
                list(APPEND files ${found})
            endforeach()
        endforeach()
        # every pin of the platform and the features, the static ones too
        # (zlib, expat, libffi, PCRE2, libjpeg-turbo, proxy-libintl): licences
        foreach(name IN LISTS pin_SOURCES)
            if(NOT PCS_PLATFORM IN_LIST pin_${name}_PLATFORMS AND NOT "all" IN_LIST pin_${name}_PLATFORMS)
                continue()
            endif()
            pharo_cairo_stack_pin_features(feats "${name}" "${PCS_PLATFORM}")
            pharo_cairo_stack_wanted(wanted "${feats}" "${features}")
            if(NOT wanted OR (NOT PCS_CAIRO AND name IN_LIST PHARO_CAIRO_STACK_CAIRO_ONLY))
                continue()
            endif()
            list(APPEND names "${name}|${name}|source")
        endforeach()
    endif()
    list(REMOVE_DUPLICATES files)
    # file(COPY) copies a symlink as a symlink (cmake -E copy would follow it,
    # and on macOS a copied libfreetype.dylib would be a second FreeType)
    file(COPY ${files} DESTINATION "${PCS_LIB_DIR}")

    # 2. LICENSE.<name>
    set(written "")
    foreach(item IN LISTS names)
        string(REPLACE "|" ";" item "${item}")
        list(GET item 0 short)
        list(GET item 1 full)
        list(GET item 2 from)
        set(dir "${share}/licenses/${short}")
        if(from STREQUAL "msys2")
            set(version "${pin_MSYS2_${PCS_ENV}_${full}_VERSION}")
            file(GLOB_RECURSE lic LIST_DIRECTORIES false "${dir}/*")
            list(SORT lic)
        else()
            set(version "${pin_${short}_VERSION}")
            set(lic "")
            foreach(l IN LISTS pin_${short}_LICENCES)
                list(APPEND lic "${dir}/${l}")
            endforeach()
        endif()
        if(NOT lic)
            message(FATAL_ERROR "cairoStack.cmake: no licence files of ${short} in ${dir}")
        endif()
        foreach(l IN LISTS lic)
            if(NOT EXISTS "${l}")
                message(FATAL_ERROR "cairoStack.cmake: ${l} is missing (the licence files of pins.txt)")
            endif()
        endforeach()
        _pharo_cairo_stack_licence_text(text "${short}" "${version}" "${dir}" ${lic})
        if(short STREQUAL "poppler")
            # the licence this distribution elects (THIRD-PARTY-NOTICES)
            set(text "Distributed in this VM under the GNU General Public License version 3 (see THIRD-PARTY-NOTICES)\n\n${text}")
        endif()
        _pharo_cairo_stack_write_if_different("${PCS_WORK}/LICENSE.${short}" "${text}")
        list(APPEND written "LICENSE.${short}")
        list(APPEND versions "${short} ${version}")
    endforeach()
    if(PCS_PLATFORM STREQUAL "mingw")
        # The C/C++ runtime that the install step bundles: libstdc++/libgcc
        # (GCC Runtime Library Exception), libc++ (Apache-2.0 with LLVM
        # exceptions), winpthreads (MIT and BSD)
        set(text "")
        foreach(item IN LISTS runtime_pkgs)
            string(REPLACE "|" ";" item "${item}")
            list(GET item 0 short)
            list(GET item 1 full)
            set(dir "${share}/licenses/${short}")
            file(GLOB_RECURSE lic LIST_DIRECTORIES false "${dir}/*")
            list(SORT lic)
            if(NOT lic)
                message(FATAL_ERROR "cairoStack.cmake: no licence files of ${short} in ${dir}")
            endif()
            _pharo_cairo_stack_licence_text(t "${short}" "${pin_MSYS2_${PCS_ENV}_${full}_VERSION}" "${dir}" ${lic})
            string(APPEND text "${t}")
        endforeach()
        if(text)
            set(text "The C/C++ runtime of MinGW-w64 next to the VM (the toolchain's build of\nthese packages, which the VM's own C++ code also needs):\n\n${text}")
            _pharo_cairo_stack_write_if_different("${PCS_WORK}/LICENSE.mingw-runtime" "${text}")
            list(APPEND written "LICENSE.mingw-runtime")
        endif()
    endif()

    # 3. THIRD-PARTY-NOTICES.cairo-stack.txt and CAIRO-STACK.txt
    set(POPPLER_VERSION "${pin_poppler_VERSION}")
    if(PCS_PLATFORM STREQUAL "mingw")
        set(_p "mingw-w64-${PCS_MSYS2_PKG_ARCH}")
        set(CAIRO_VERSION "${pin_MSYS2_${PCS_ENV}_${_p}-cairo_VERSION}")
        set(FREETYPE_VERSION "${pin_MSYS2_${PCS_ENV}_${_p}-freetype_VERSION}")
        set(GLIB_VERSION "${pin_MSYS2_${PCS_ENV}_${_p}-glib2_VERSION}")
        set(PANGO_VERSION "${pin_MSYS2_${PCS_ENV}_${_p}-pango_VERSION}")
        set(ORIGIN "They are MSYS2's ${PCS_ENV} binary packages, pinned by URL and SHA256 in
cmake/cairo-stack/pins.txt of the VM's sources: cairo with MSYS2's two
patches (a DWrite/Direct2D glyph patch and cairo merge request 673), and
the libraries those packages need.")
        set(SOURCES "\nFor Windows the sources are MSYS2's source packages of the packages
above (the upstream archives, MSYS2's patches and build recipes, named
with their SHA256 in pins.txt), attached to the release as well.")
        if("poppler" IN_LIST features)
            # Poppler is no MSYS2 package here: build.sh --platform mingw built it
            file(STRINGS "${share}/manifest.txt" cxx REGEX "^tool c\\+\\+ ")
            string(REGEX REPLACE "^tool c\\+\\+ " "" cxx "${cxx}")
            if(cxx STREQUAL "")
                set(cxx "the C++ compiler of the MSYS2 environment")
            endif()
            string(APPEND ORIGIN "  Poppler ${POPPLER_VERSION} (libpoppler
and libpoppler-glib) is not MSYS2's: scripts/cairo-stack/build.sh built it,
unpatched, from the upstream archive poppler-${POPPLER_VERSION}.tar.xz pinned by SHA256
in pins.txt, with the compilers of the MSYS2 environment (${cxx}),
against the packages above.")
            string(APPEND SOURCES "  Poppler's source is its upstream archive
poppler-${POPPLER_VERSION}.tar.xz, in the same tar.")
        endif()
    else()
        set(CAIRO_VERSION "${pin_cairo_VERSION}")
        set(FREETYPE_VERSION "${pin_freetype_VERSION}")
        set(GLIB_VERSION "${pin_glib_VERSION}")
        set(PANGO_VERSION "${pin_pango_VERSION}")
        set(ORIGIN "They are built from the upstream release archives pinned by SHA256 in
cmake/cairo-stack/pins.txt of the VM's sources, unpatched, by
scripts/cairo-stack/build.sh.")
        set(SOURCES "")
    endif()
    if(PCS_CAIRO)
        set(CONTENTS "cairo ${CAIRO_VERSION} and FreeType ${FREETYPE_VERSION}")
        set(bundled "cairo freetype")
    else()
        set(CONTENTS "FreeType ${FREETYPE_VERSION} (FEATURE_LIB_CAIRO is off: no cairo)")
        set(bundled "freetype")
    endif()
    foreach(f IN LISTS PHARO_CAIRO_STACK_ALL_FEATURES)
        if(f IN_LIST features)
            string(APPEND bundled " ${f}")
        endif()
    endforeach()
    if(PCS_SOURCE_OFFER_URL)
        set(SOURCE_OFFER_URL "${PCS_SOURCE_OFFER_URL}")
    else()
        set(SOURCE_OFFER_URL "the address from which you obtained this build")
    endif()
    string(REPLACE ";" "\n    " LICENSE_FILES "${written}")
    set(LICENSE_FILES "    ${LICENSE_FILES}")
    string(REPLACE ";" "\n    " LIBRARY_VERSIONS "${versions}")
    set(LIBRARY_VERSIONS "    ${LIBRARY_VERSIONS}")
    # the blocks of the head that apply: the features, glib (either), the platform
    set(holds ${features} "${PCS_PLATFORM}")
    if(features)
        list(APPEND holds glib)
    endif()
    file(READ "${PCS_HEAD}" head)
    _pharo_cairo_stack_notice_blocks(head "${head}" ${holds})
    string(CONFIGURE "${head}" notice @ONLY)
    _pharo_cairo_stack_write_if_different("${PCS_WORK}/THIRD-PARTY-NOTICES.cairo-stack.txt" "${notice}")
    # The prefix's index, with the features of this bundle after its first
    # paragraph (check.sh --bundle reads them), without the source pins
    # ("    <name> <version>" and its three lines) and the dll lines (Windows)
    # of a feature it leaves out
    file(READ "${share}/CAIRO-STACK.txt" index)
    if(PCS_PLATFORM STREQUAL "mingw" AND NOT "poppler" IN_LIST features)
        # the last section, which build.sh --platform mingw appends: Poppler's
        string(FIND "${index}" "\nBuilt from source by scripts/cairo-stack/build.sh --platform mingw" p)
        if(NOT p EQUAL -1)
            math(EXPR p "${p} + 1")
            string(SUBSTRING "${index}" 0 ${p} index)
        endif()
    endif()
    string(REGEX MATCHALL "    [A-Za-z0-9_.+-]+ [^ \n]+\n        sha256 [^\n]*\n        [^\n]*\n        [^\n]*\n" pin_blocks "${index}")
    foreach(b IN LISTS pin_blocks)
        string(REGEX MATCH "^    ([^ ]+) " _m "${b}")
        pharo_cairo_stack_pin_features(feats "${CMAKE_MATCH_1}" "${PCS_PLATFORM}")
        pharo_cairo_stack_wanted(wanted "${feats}" "${features}")
        if(NOT wanted)
            string(REPLACE "${b}" "" index "${index}")
        endif()
    endforeach()
    string(REGEX MATCHALL "    dll [^\n]*\n" dll_lines "${index}")
    foreach(l IN LISTS dll_lines)
        string(STRIP "${l}" s)
        string(REGEX REPLACE "[ \t]+" ";" f "${s}")
        list(LENGTH f n)
        if(n GREATER_EQUAL 6)
            list(GET f 5 feats)
            string(REPLACE "," ";" feats "${feats}")
            pharo_cairo_stack_wanted(wanted "${feats}" "${features}")
            if(NOT wanted)
                string(REPLACE "${l}" "" index "${index}")
            endif()
        endif()
    endforeach()
    # the prefix's own line names the prefix's features
    if(features)
        string(REPLACE ";" " " fl "${features}")
    else()
        set(fl "none")
    endif()
    string(REGEX REPLACE "\nFeatures besides cairo and FreeType: [^\n]*\n" "\nFeatures besides cairo and FreeType: ${fl}.\n" index "${index}")
    string(FIND "${index}" "\n\n" p)
    if(p EQUAL -1)
        set(index "${index}\nBundled features: ${bundled}\n")
    else()
        math(EXPR p "${p} + 2")
        string(SUBSTRING "${index}" 0 ${p} first)
        string(SUBSTRING "${index}" ${p} -1 rest)
        set(index "${first}Bundled features: ${bundled}\n\n${rest}")
    endif()
    _pharo_cairo_stack_write_if_different("${PCS_WORK}/CAIRO-STACK.txt" "${index}")
    list(APPEND written THIRD-PARTY-NOTICES.cairo-stack.txt CAIRO-STACK.txt)
    foreach(n IN LISTS written)
        execute_process(COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${PCS_WORK}/${n}" "${PCS_NOTICE_DIR}/${n}"
            RESULT_VARIABLE rc)
        if(NOT rc EQUAL 0)
            message(FATAL_ERROR "cairoStack.cmake: cannot copy ${n} to ${PCS_NOTICE_DIR}")
        endif()
    endforeach()
    # macOS: Poppler's fontconfig reads this configuration, which LibPoppler
    # loads from the bundle (docs/CairoStack.md)
    if(PCS_PLATFORM STREQUAL "darwin" AND "poppler" IN_LIST features)
        file(MAKE_DIRECTORY "${PCS_NOTICE_DIR}/fontconfig")
        execute_process(COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${PCS_FONTS_CONF}" "${PCS_NOTICE_DIR}/fontconfig/fonts.conf"
            RESULT_VARIABLE rc)
        if(NOT rc EQUAL 0)
            message(FATAL_ERROR "cairoStack.cmake: cannot copy ${PCS_FONTS_CONF} to ${PCS_NOTICE_DIR}/fontconfig")
        endif()
    endif()

    # Left in an older tree by the old files.pharo.org cairo and FreeType,
    # which earlier versions of this build bundled: not this stack's (HarfBuzz
    # and, on macOS, fontconfig are the stack's own with Pango, Poppler)
    set(stale_globs "")
    if(PCS_PLATFORM STREQUAL "linux")
        set(stale_globs libbz2* libexpat* libcairo.so.2.1170* libcairo.so.2.1160*)
        if(NOT "pango" IN_LIST features)
            list(APPEND stale_globs libharfbuzz*)
        endif()
    elseif(PCS_PLATFORM STREQUAL "darwin")
        set(stale_globs libbz2* libz.*)
        if(NOT "pango" IN_LIST features)
            list(APPEND stale_globs libharfbuzz*)
        endif()
        if(NOT "poppler" IN_LIST features)
            list(APPEND stale_globs libfontconfig*)
        endif()
    endif()
    set(stale "")
    foreach(g IN LISTS stale_globs)
        file(GLOB s LIST_DIRECTORIES false "${PCS_LIB_DIR}/${g}")
        list(APPEND stale ${s})
    endforeach()
    if(stale)
        message(WARNING "cairoStack.cmake: ${PCS_LIB_DIR} still has libraries that are not the cairo stack's "
            "(the old files.pharo.org cairo and FreeType of an earlier configure of this tree), "
            "which the bundle must not ship: ${stale}.  Build in a fresh tree.")
    endif()
    # Left by an earlier configure of this tree with a feature that is now off
    # (a VM built with -DFEATURE_LIB_POPPLER=OFF must not carry Poppler)
    set(off_globs "")
    if(PCS_PLATFORM STREQUAL "linux" OR PCS_PLATFORM STREQUAL "darwin")
        if(NOT "pango" IN_LIST features)
            list(APPEND off_globs libpango* libfribidi*)
        endif()
        if(NOT "poppler" IN_LIST features)
            list(APPEND off_globs libpoppler*)
        endif()
        if(NOT features)
            list(APPEND off_globs libglib-2.0* libgobject-2.0* libgio-2.0* libgmodule-2.0*)
        endif()
    endif()
    set(leftover "")
    foreach(g IN LISTS off_globs)
        file(GLOB s LIST_DIRECTORIES false "${PCS_LIB_DIR}/${g}")
        list(APPEND leftover ${s})
    endforeach()
    if(leftover)
        message(WARNING "cairoStack.cmake: ${PCS_LIB_DIR} still has libraries of a feature that is off "
            "(features: ${bundled}), from an earlier configure of this tree, which the bundle must not ship: "
            "${leftover}.  Build in a fresh tree.")
    endif()
    list(LENGTH files nf)
    message(STATUS "cairo stack: ${nf} files from ${PCS_PREFIX} into ${PCS_LIB_DIR} (features: ${bundled})")
endfunction()

if(CMAKE_SCRIPT_MODE_FILE)
    if(PHARO_CAIRO_STACK_ACTION STREQUAL "bundle")
        _pharo_cairo_stack_bundle()
    endif()
    return()
endif()

# ---------------------------------------------------------------- configure

# pharo_cairo_stack_attach(): make the VM library depend on the bundle (from
# add_third_party_dependencies_per_platform, once the target exists)
macro(pharo_cairo_stack_attach)
    if(PHARO_CAIRO_STACK_ACTIVE)
        add_dependencies(${VM_LIBRARY_NAME} pharo_cairo_stack_bundle)
    endif()
endmacro()

# pharo_cairo_stack_install_mingw(<component>): after the install step's
# runtime-dependency copies (cmake/Windows.cmake), install the stack's DLLs
# again from the prefix, so that a DLL of the same name from another MSYS2
# package of the build machine (zlib1.dll, libiconv-2.dll...) cannot replace
# the pinned one.  The runtime ones (kind runtime) stay the toolchain's; a DLL
# of a feature that is off (dlls.txt's 5th field: core, pango, poppler) is
# left out; the built ones (Poppler) are installed like the others.
function(pharo_cairo_stack_install_mingw component)
    if(NOT PHARO_CAIRO_STACK_ACTIVE OR NOT PHARO_CAIRO_STACK_PLATFORM STREQUAL "mingw")
        return()
    endif()
    if(FEATURE_LIB_CAIRO)
        set(skip "")
    else()
        set(skip "libcairo-2.dll")
    endif()
    install(CODE "
        set(_pcs_features \"${PHARO_CAIRO_STACK_FEATURES}\")
        file(STRINGS \"${PHARO_CAIRO_STACK_PREFIX}/share/pharo-cairo-stack/dlls.txt\" _pcs_lines)
        foreach(_pcs_line IN LISTS _pcs_lines)
            string(REGEX REPLACE \"[ \\t]+\" \";\" _pcs_f \"\${_pcs_line}\")
            list(LENGTH _pcs_f _pcs_n)
            list(GET _pcs_f 0 _pcs_dll)
            list(GET _pcs_f 1 _pcs_kind)
            if(_pcs_kind STREQUAL \"runtime\" OR _pcs_dll STREQUAL \"${skip}\")
                continue()
            endif()
            set(_pcs_wanted TRUE)
            if(_pcs_n GREATER_EQUAL 5)
                list(GET _pcs_f 4 _pcs_feats)
                string(REPLACE \",\" \";\" _pcs_feats \"\${_pcs_feats}\")
                set(_pcs_wanted FALSE)
                foreach(_pcs_x IN LISTS _pcs_feats)
                    if(_pcs_x STREQUAL \"core\" OR _pcs_x IN_LIST _pcs_features)
                        set(_pcs_wanted TRUE)
                    endif()
                endforeach()
            endif()
            if(NOT _pcs_wanted)
                continue()
            endif()
            message(STATUS \"Bundling the cairo stack's \${_pcs_dll}\")
            file(INSTALL \"${PHARO_CAIRO_STACK_PREFIX}/bin/\${_pcs_dll}\" DESTINATION \"\$ENV{DESTDIR}\${CMAKE_INSTALL_PREFIX}\")
        endforeach()
    " COMPONENT ${component})
endfunction()

set(PHARO_CAIRO_STACK_ACTIVE OFF)
set(PHARO_CAIRO_STACK_FEATURES "")
set(PHARO_CAIRO_STACK_PREFIX "" CACHE INTERNAL "The cairo stack prefix of this build (cmake/cairoStack.cmake)")
set(PHARO_CAIRO_STACK_INCLUDE_DIRS "" CACHE INTERNAL "The include directories of the cairo stack (cmake/cairoStack.cmake)")

# Pango and Poppler draw with cairo: without it both are off (for the rest of
# the configure too)
if(NOT FEATURE_LIB_CAIRO AND (FEATURE_LIB_PANGO OR FEATURE_LIB_POPPLER))
    message(STATUS "cairo stack: FEATURE_LIB_PANGO and FEATURE_LIB_POPPLER off (FEATURE_LIB_CAIRO is off)")
    set(FEATURE_LIB_PANGO OFF)
    set(FEATURE_LIB_POPPLER OFF)
endif()

if(BUILD_BUNDLE AND (FEATURE_LIB_CAIRO OR FEATURE_LIB_FREETYPE2))
    if(FEATURE_LIB_CAIRO AND NOT FEATURE_LIB_FREETYPE2)
        message(FATAL_ERROR "FEATURE_LIB_CAIRO=ON with FEATURE_LIB_FREETYPE2=OFF: the cairo stack bundles "
            "FreeType, since cairo-ft needs the image's FreeType (one FreeType for both, docs/CairoStack.md). "
            "Turn FEATURE_LIB_FREETYPE2 on, or FEATURE_LIB_CAIRO off.")
    endif()
    set(PHARO_CAIRO_STACK_ACTIVE ON)
    if(FEATURE_LIB_PANGO)
        list(APPEND PHARO_CAIRO_STACK_FEATURES pango)
    endif()
    if(FEATURE_LIB_POPPLER)
        list(APPEND PHARO_CAIRO_STACK_FEATURES poppler)
    endif()
else()
    # Nothing else bundles cairo or FreeType: the image loads the system's
    if(NOT BUILD_BUNDLE)
        set(_pcs_why "BUILD_BUNDLE=OFF")
    else()
        set(_pcs_why "FEATURE_LIB_CAIRO=OFF and FEATURE_LIB_FREETYPE2=OFF")
    endif()
    if(PHARO_CAIRO_STACK_DIR)
        message(WARNING "PHARO_CAIRO_STACK_DIR (${PHARO_CAIRO_STACK_DIR}) is ignored: ${_pcs_why}")
    endif()
    message(STATUS "cairo stack: off (${_pcs_why}): the VM bundles no cairo and no FreeType, and the image loads the system's")
endif()

if(PHARO_CAIRO_STACK_ACTIVE)
    string(CONCAT _pcs_fix_msg "Configure with -DPHARO_CAIRO_STACK_DIR=<prefix> (a stack that scripts/cairo-stack/build.sh built "
        "elsewhere, docs/CairoStack.md), or with -DFEATURE_LIB_PANGO=OFF -DFEATURE_LIB_POPPLER=OFF "
        "(a VM without Pango and Poppler, which need the C++ compiler, python3, cmake >= 3.28 and more), "
        "or with -DFEATURE_LIB_CAIRO=OFF -DFEATURE_LIB_FREETYPE2=OFF (a VM without cairo and FreeType).")
    pharo_cairo_stack_platform(PHARO_CAIRO_STACK_PLATFORM SYSTEM "${CMAKE_SYSTEM_NAME}" MINGW "${MINGW}" CYGWIN "${CYGWIN}")
    if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "")
        message(FATAL_ERROR "cmake/cairoStack.cmake: no cairo stack for ${CMAKE_SYSTEM_NAME}. ${_pcs_fix_msg}")
    endif()
    pharo_cairo_stack_arch(PHARO_CAIRO_STACK_ARCH PLATFORM "${PHARO_CAIRO_STACK_PLATFORM}"
        PROCESSOR "${CMAKE_SYSTEM_PROCESSOR}" OSX_ARCHITECTURES ${CMAKE_OSX_ARCHITECTURES})
    if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "darwin")
        if(NOT CMAKE_OSX_ARCHITECTURES)
            set(PHARO_CAIRO_STACK_ARCH "${CMAKE_HOST_SYSTEM_PROCESSOR}")
        endif()
        set(PHARO_CAIRO_STACK_DEPLOYMENT_TARGET "${CMAKE_OSX_DEPLOYMENT_TARGET}")
        if(NOT PHARO_CAIRO_STACK_DEPLOYMENT_TARGET)
            set(PHARO_CAIRO_STACK_DEPLOYMENT_TARGET "11.0")
        endif()
    else()
        set(PHARO_CAIRO_STACK_DEPLOYMENT_TARGET "-")
    endif()
    set(PHARO_CAIRO_STACK_ENV "")
    set(PHARO_CAIRO_STACK_MSYS2_PKG_ARCH "")
    if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "mingw")
        if(PHARO_CAIRO_STACK_ARCH STREQUAL "arm64")
            set(PHARO_CAIRO_STACK_ENV clangarm64)
            set(PHARO_CAIRO_STACK_MSYS2_PKG_ARCH clang-aarch64)
        else()
            set(PHARO_CAIRO_STACK_ENV ucrt64)
            set(PHARO_CAIRO_STACK_MSYS2_PKG_ARCH ucrt-x86_64)
        endif()
    endif()

    # The versions (CI reads pins.txt; the CairoPlugin checks CAIRO_VERSION)
    pharo_cairo_stack_read_pins("${PHARO_CAIRO_STACK_PINS_FILE}" _pcs_pin)
    if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "mingw")
        set(_pcs_p "mingw-w64-${PHARO_CAIRO_STACK_MSYS2_PKG_ARCH}")
        string(REGEX REPLACE "-[0-9]+$" "" PHARO_CAIRO_VERSION "${_pcs_pin_MSYS2_${PHARO_CAIRO_STACK_ENV}_${_pcs_p}-cairo_VERSION}")
        string(REGEX REPLACE "-[0-9]+$" "" PHARO_FREETYPE_VERSION "${_pcs_pin_MSYS2_${PHARO_CAIRO_STACK_ENV}_${_pcs_p}-freetype_VERSION}")
        if(NOT _pcs_pin_MSYS2_${PHARO_CAIRO_STACK_ENV}_DLLS)
            message(FATAL_ERROR "cmake/cairoStack.cmake: ${PHARO_CAIRO_STACK_PINS_FILE} has no msys2-dll line for ${PHARO_CAIRO_STACK_ENV}")
        endif()
    else()
        set(PHARO_CAIRO_VERSION "${_pcs_pin_cairo_VERSION}")
        set(PHARO_FREETYPE_VERSION "${_pcs_pin_freetype_VERSION}")
    endif()
    if(NOT PHARO_CAIRO_VERSION OR NOT PHARO_FREETYPE_VERSION)
        message(FATAL_ERROR "cmake/cairoStack.cmake: no cairo or freetype version in ${PHARO_CAIRO_STACK_PINS_FILE}")
    endif()
    if(PHARO_CAIRO_STACK_FEATURES)
        string(REPLACE ";" " " _pcs_features_text "${PHARO_CAIRO_STACK_FEATURES}")
        string(REPLACE ";" "," _pcs_features_csv "${PHARO_CAIRO_STACK_FEATURES}")
    else()
        set(_pcs_features_text "none")
        set(_pcs_features_csv "none")
    endif()
    message(STATUS "cairo stack: features ${_pcs_features_text}")

    if(PHARO_CAIRO_STACK_DIR)
        # A stack built beforehand (CI, or a developer's reuse): validate its manifest
        get_filename_component(_pcs_prefix "${PHARO_CAIRO_STACK_DIR}" ABSOLUTE)
        pharo_cairo_stack_check_manifest(_pcs_errors
            MANIFEST "${_pcs_prefix}/share/pharo-cairo-stack/manifest.txt" PINS "${PHARO_CAIRO_STACK_PINS_FILE}"
            PLATFORM "${PHARO_CAIRO_STACK_PLATFORM}" ARCH "${PHARO_CAIRO_STACK_ARCH}"
            DEPLOYMENT_TARGET "${PHARO_CAIRO_STACK_DEPLOYMENT_TARGET}" NEED_CAIRO "${FEATURE_LIB_CAIRO}"
            FEATURES ${PHARO_CAIRO_STACK_FEATURES})
        if(_pcs_errors)
            message(FATAL_ERROR "PHARO_CAIRO_STACK_DIR=${PHARO_CAIRO_STACK_DIR} is not the stack this build needs: "
                "${_pcs_errors}.  Rebuild it (scripts/cairo-stack/build.sh, docs/CairoStack.md), or leave "
                "PHARO_CAIRO_STACK_DIR empty and the build makes its own.")
        endif()
        foreach(_pcs_h cairo/cairo.h freetype2/ft2build.h)
            if(NOT EXISTS "${_pcs_prefix}/include/${_pcs_h}" AND (FEATURE_LIB_CAIRO OR NOT _pcs_h MATCHES "^cairo/"))
                message(FATAL_ERROR "PHARO_CAIRO_STACK_DIR=${PHARO_CAIRO_STACK_DIR} has no include/${_pcs_h}")
            endif()
        endforeach()
        message(STATUS "cairo stack: ${_pcs_prefix} (PHARO_CAIRO_STACK_DIR, manifest checked): cairo ${PHARO_CAIRO_VERSION}, FreeType ${PHARO_FREETYPE_VERSION}")
        add_custom_target(pharo_cairo_stack)
    else()
        pharo_cairo_stack_guard(_pcs_error _pcs_tool_dirs
            SYSTEM "${CMAKE_SYSTEM_NAME}" MINGW "${MINGW}" CYGWIN "${CYGWIN}"
            CROSSCOMPILING "${CMAKE_CROSSCOMPILING}" OSX_ARCHITECTURES ${CMAKE_OSX_ARCHITECTURES}
            HOST_PROCESSOR "${CMAKE_HOST_SYSTEM_PROCESSOR}" PROCESSOR "${CMAKE_SYSTEM_PROCESSOR}"
            POINTER_SIZE "${CMAKE_SIZEOF_VOID_P}" FEATURES ${PHARO_CAIRO_STACK_FEATURES}
            CXX_COMPILER "${CMAKE_CXX_COMPILER}" CXX_COMPILER_ID "${CMAKE_CXX_COMPILER_ID}"
            CXX_COMPILER_VERSION "${CMAKE_CXX_COMPILER_VERSION}")
        if(_pcs_error)
            message(FATAL_ERROR "cmake/cairoStack.cmake: the cairo stack (cairo ${PHARO_CAIRO_VERSION} and FreeType) "
                "cannot be built by this configure: ${_pcs_error}. ${_pcs_fix_msg}")
        endif()
        set(_pcs_prefix "${CMAKE_BINARY_DIR}/cairo-stack/prefix")
        set(_pcs_work "${CMAKE_BINARY_DIR}/cairo-stack/work")
        set(_pcs_downloads "${CMAKE_BINARY_DIR}/downloads")
        set(_pcs_script "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/build.sh")

        # MSYS2's sh takes /c/... paths: build.sh and fetch-msys2.sh make
        # relative paths absolute by prefixing $(pwd), which C:/... is not
        set(_pcs_cc "${CMAKE_C_COMPILER}")
        set(_pcs_cxx "${CMAKE_CXX_COMPILER}")
        set(_pcs_paths _pcs_prefix _pcs_work _pcs_downloads _pcs_script _pcs_cc _pcs_cxx)
        if(DEFINED WASM_DEPS_DIR AND NOT WASM_DEPS_DIR STREQUAL "")
            get_filename_component(_pcs_deps "${WASM_DEPS_DIR}" ABSOLUTE)
            list(APPEND _pcs_paths _pcs_deps)
        else()
            set(_pcs_deps "")
        endif()
        set(_pcs_sh_dirs ${_pcs_tool_dirs})
        get_filename_component(_pcs_cmake_dir "${CMAKE_COMMAND}" DIRECTORY)
        if(NOT _pcs_cmake_dir IN_LIST _pcs_sh_dirs)
            list(APPEND _pcs_sh_dirs "${_pcs_cmake_dir}")     # last: only if cmake is not on PATH
        endif()
        if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "mingw")
            find_program(PHARO_CAIRO_STACK_CYGPATH cygpath)
            if(PHARO_CAIRO_STACK_CYGPATH)
                foreach(_pcs_v IN LISTS _pcs_paths)
                    if(${_pcs_v})
                        execute_process(COMMAND "${PHARO_CAIRO_STACK_CYGPATH}" -u "${${_pcs_v}}"
                            OUTPUT_VARIABLE _pcs_u OUTPUT_STRIP_TRAILING_WHITESPACE RESULT_VARIABLE _pcs_rc)
                        if(_pcs_rc EQUAL 0 AND _pcs_u)
                            set(${_pcs_v}_sh "${_pcs_u}")
                        else()
                            set(${_pcs_v}_sh "${${_pcs_v}}")
                        endif()
                    endif()
                endforeach()
                set(_pcs_u_dirs "")
                foreach(_pcs_d IN LISTS _pcs_sh_dirs)
                    execute_process(COMMAND "${PHARO_CAIRO_STACK_CYGPATH}" -u "${_pcs_d}"
                        OUTPUT_VARIABLE _pcs_u OUTPUT_STRIP_TRAILING_WHITESPACE)
                    list(APPEND _pcs_u_dirs "${_pcs_u}")
                endforeach()
                set(_pcs_sh_dirs ${_pcs_u_dirs})
            endif()
        endif()
        foreach(_pcs_v IN LISTS _pcs_paths)
            if(NOT DEFINED ${_pcs_v}_sh)
                set(${_pcs_v}_sh "${${_pcs_v}}")
            endif()
        endforeach()

        # ${CMAKE_BINARY_DIR}/cairo-stack/build-stack.sh: the exact command,
        # to rerun by hand (sh build-stack.sh)
        string(REPLACE ";" ":" _pcs_path "${_pcs_sh_dirs}")
        _pharo_cairo_stack_sh_quote(_q_path "${_pcs_path}")
        _pharo_cairo_stack_sh_quote(_q_script "${_pcs_script_sh}")
        _pharo_cairo_stack_sh_quote(_q_prefix "${_pcs_prefix_sh}")
        _pharo_cairo_stack_sh_quote(_q_work "${_pcs_work_sh}")
        _pharo_cairo_stack_sh_quote(_q_dl "${_pcs_downloads_sh}")
        set(_pcs_args "--platform ${PHARO_CAIRO_STACK_PLATFORM} --prefix ${_q_prefix} --work ${_q_work} --downloads ${_q_dl} --arch ${PHARO_CAIRO_STACK_ARCH}")
        if(_pcs_deps)
            _pharo_cairo_stack_sh_quote(_q_deps "${_pcs_deps_sh}")
            string(APPEND _pcs_args " --deps ${_q_deps}")
        endif()
        if(NOT FEATURE_LIB_CAIRO)
            string(APPEND _pcs_args " --only freetype")
        else()
            string(APPEND _pcs_args " --features ${_pcs_features_csv}")
        endif()
        if(PHARO_CAIRO_STACK_GLIBC_MAX AND PHARO_CAIRO_STACK_PLATFORM STREQUAL "linux")
            string(APPEND _pcs_args " --glibc-max ${PHARO_CAIRO_STACK_GLIBC_MAX}")
        endif()
        # the configure's compilers (MinGW: Poppler, built from source there)
        set(_pcs_env "")
        if(_pcs_cc_sh AND _pcs_cxx_sh)
            _pharo_cairo_stack_sh_quote(_q_cc "${_pcs_cc_sh}")
            _pharo_cairo_stack_sh_quote(_q_cxx "${_pcs_cxx_sh}")
            string(APPEND _pcs_env "CC=${_q_cc}; export CC\nCXX=${_q_cxx}; export CXX\n")
        endif()
        if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "darwin")
            string(APPEND _pcs_env "MACOSX_DEPLOYMENT_TARGET=${PHARO_CAIRO_STACK_DEPLOYMENT_TARGET}; export MACOSX_DEPLOYMENT_TARGET\n")
        endif()
        set(_pcs_build_stack "${CMAKE_BINARY_DIR}/cairo-stack/build-stack.sh")
        _pharo_cairo_stack_write_if_different("${_pcs_build_stack}" "#!/bin/sh
# Written by cmake/cairoStack.cmake at configure time: builds this tree's cairo
# stack (docs/CairoStack.md).  ExternalProject pharo_cairo_stack runs it; it
# can be rerun by hand.
set -e
# the stack's ninja is not a sub-make: no jobserver of an outer make
unset MAKEFLAGS MFLAGS MAKELEVEL
PATH=${_q_path}:\"\$PATH\"; export PATH
${_pcs_env}exec sh ${_q_script} ${_pcs_args} \"\$@\"
")

        include(ExternalProject)
        if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "linux")
            set(_pcs_byproducts "${_pcs_prefix}/lib/libfreetype.so.6" "${_pcs_prefix}/lib/libpng16.so.16")
            if(FEATURE_LIB_CAIRO)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libcairo.so.2" "${_pcs_prefix}/lib/libpixman-1.so.0" "${_pcs_prefix}/lib/libfontconfig.so.1")
            endif()
            if(PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libglib-2.0.so.0" "${_pcs_prefix}/lib/libgobject-2.0.so.0")
            endif()
            if("pango" IN_LIST PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libpango-1.0.so.0" "${_pcs_prefix}/lib/libpangocairo-1.0.so.0")
            endif()
            if("poppler" IN_LIST PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libpoppler-glib.so.8")
            endif()
        elseif(PHARO_CAIRO_STACK_PLATFORM STREQUAL "darwin")
            set(_pcs_byproducts "${_pcs_prefix}/lib/libfreetype.6.dylib" "${_pcs_prefix}/lib/libpng16.16.dylib")
            if(FEATURE_LIB_CAIRO)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libcairo.2.dylib" "${_pcs_prefix}/lib/libpixman-1.0.dylib")
            endif()
            if(PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libglib-2.0.0.dylib" "${_pcs_prefix}/lib/libgobject-2.0.0.dylib")
            endif()
            if("pango" IN_LIST PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libpango-1.0.0.dylib" "${_pcs_prefix}/lib/libpangocairo-1.0.0.dylib")
            endif()
            if("poppler" IN_LIST PHARO_CAIRO_STACK_FEATURES)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libpoppler-glib.8.dylib")
            endif()
        else()
            set(_pcs_byproducts "${_pcs_prefix}/bin/libfreetype-6.dll" "${_pcs_prefix}/bin/libcairo-2.dll"
                "${_pcs_prefix}/share/pharo-cairo-stack/dlls.txt")
        endif()
        list(APPEND _pcs_byproducts "${_pcs_prefix}/share/pharo-cairo-stack/manifest.txt"
            "${_pcs_prefix}/include/freetype2/ft2build.h")
        if(FEATURE_LIB_CAIRO)
            list(APPEND _pcs_byproducts "${_pcs_prefix}/include/cairo/cairo.h" "${_pcs_prefix}/include/cairo/cairo-ft.h"
                "${_pcs_prefix}/include/cairo/cairo-features.h" "${_pcs_prefix}/include/cairo/cairo-version.h")
        endif()
        find_program(PHARO_CAIRO_STACK_SH NAMES sh bash)
        if(NOT PHARO_CAIRO_STACK_SH)
            message(FATAL_ERROR "cmake/cairoStack.cmake: no sh. ${_pcs_fix_msg}")
        endif()
        ExternalProject_Add(pharo_cairo_stack
            PREFIX "${CMAKE_BINARY_DIR}/cairo-stack/ep"
            SOURCE_DIR "${PHARO_CAIRO_STACK_SCRIPTS_DIR}"
            DOWNLOAD_COMMAND ""
            UPDATE_COMMAND ""
            PATCH_COMMAND ""
            CONFIGURE_COMMAND ""
            BUILD_COMMAND "${PHARO_CAIRO_STACK_SH}" "${_pcs_build_stack}"
            INSTALL_COMMAND ""
            TEST_COMMAND ""
            BUILD_BYPRODUCTS ${_pcs_byproducts}
            USES_TERMINAL_BUILD TRUE)
        # rebuilt when the pins, the scripts or the command change
        file(GLOB _pcs_inputs "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/*.sh" "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/probe.c"
            "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/probe-text.c")
        ExternalProject_Add_StepDependencies(pharo_cairo_stack build
            "${PHARO_CAIRO_STACK_PINS_FILE}" "${_pcs_build_stack}" ${_pcs_inputs})
        message(STATUS "cairo stack: built from ${PHARO_CAIRO_STACK_PINS_FILE} into ${_pcs_prefix} "
            "(cairo ${PHARO_CAIRO_VERSION}, FreeType ${PHARO_FREETYPE_VERSION}; ${_pcs_build_stack})")
    endif()

    set(PHARO_CAIRO_STACK_PREFIX "${_pcs_prefix}" CACHE INTERNAL "The cairo stack prefix of this build (cmake/cairoStack.cmake)")
    set(PHARO_CAIRO_STACK_INCLUDE_DIRS "${_pcs_prefix}/include;${_pcs_prefix}/include/cairo;${_pcs_prefix}/include/freetype2"
        CACHE INTERNAL "The include directories of the cairo stack (cmake/cairoStack.cmake)")

    # The bundle, at build time (the prefix that the configure builds exists only then)
    if(PHARO_CAIRO_STACK_PLATFORM STREQUAL "darwin")
        set(_pcs_notice_dir "${EXECUTABLE_OUTPUT_PATH}/${VM_EXECUTABLE_NAME}.app/Contents/Resources")
    else()
        set(_pcs_notice_dir "${LIBRARY_OUTPUT_DIRECTORY}")
    endif()
    if(FEATURE_LIB_CAIRO)
        set(_pcs_cairo ON)
    else()
        set(_pcs_cairo OFF)
    endif()
    # Where the GPL's written offer of the notice asks to be sent: the issues
    # of the repository that CI builds, else the notice's own wording
    set(_pcs_offer "")
    if(NOT "$ENV{GITHUB_SERVER_URL}" STREQUAL "" AND NOT "$ENV{GITHUB_REPOSITORY}" STREQUAL "")
        set(_pcs_offer "$ENV{GITHUB_SERVER_URL}/$ENV{GITHUB_REPOSITORY}/issues")
    endif()
    add_custom_target(pharo_cairo_stack_bundle ALL
        COMMAND "${CMAKE_COMMAND}"
            -DPHARO_CAIRO_STACK_ACTION=bundle
            "-DPCS_PREFIX=${_pcs_prefix}"
            "-DPCS_PINS=${PHARO_CAIRO_STACK_PINS_FILE}"
            "-DPCS_HEAD=${PHARO_CAIRO_STACK_NOTICE_HEAD}"
            "-DPCS_PLATFORM=${PHARO_CAIRO_STACK_PLATFORM}"
            "-DPCS_ARCH=${PHARO_CAIRO_STACK_ARCH}"
            "-DPCS_DEPLOYMENT_TARGET=${PHARO_CAIRO_STACK_DEPLOYMENT_TARGET}"
            "-DPCS_ENV=${PHARO_CAIRO_STACK_ENV}"
            "-DPCS_MSYS2_PKG_ARCH=${PHARO_CAIRO_STACK_MSYS2_PKG_ARCH}"
            "-DPCS_CAIRO=${_pcs_cairo}"
            "-DPCS_FEATURES=${_pcs_features_csv}"
            "-DPCS_FONTS_CONF=${PHARO_CAIRO_STACK_FONTS_CONF}"
            "-DPCS_SOURCE_OFFER_URL=${_pcs_offer}"
            "-DPCS_LIB_DIR=${LIBRARY_OUTPUT_DIRECTORY}"
            "-DPCS_NOTICE_DIR=${_pcs_notice_dir}"
            "-DPCS_WORK=${CMAKE_BINARY_DIR}/cairo-stack/notices"
            -P "${PHARO_CAIRO_STACK_THIS_FILE}"
        COMMENT "Bundling the cairo stack (cmake/cairoStack.cmake)"
        VERBATIM)
    add_dependencies(pharo_cairo_stack_bundle pharo_cairo_stack)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${PHARO_CAIRO_STACK_PINS_FILE}")
endif()
