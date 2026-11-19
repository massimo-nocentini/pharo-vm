# The cairo stack of the native VMs (docs/CairoStack.md): cairo and FreeType,
# which the image loads by name (CairoLibrary, FT2FFILibrary), and the
# libraries they need, at the versions pinned in cmake/cairo-stack/pins.txt.
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
#   - with PHARO_CAIRO_STACK_DIR, its manifest must match pins.txt (SHA256),
#     the platform, the architecture and the macOS deployment target, so that a
#     stale stack (a CI cache) cannot slip in;
#   - the target pharo_cairo_stack_bundle (ALL; the VM library depends on it)
#     runs this file as a script (cmake -P, PHARO_CAIRO_STACK_ACTION=bundle) at
#     build time: it copies the libraries of docs/CairoStack.md "What the
#     bundles contain" into LIBRARY_OUTPUT_DIRECTORY, symlinks as symlinks
#     (file(COPY)), and writes LICENSE.<name>, THIRD-PARTY-NOTICES.cairo-stack.txt
#     (packaging/cairo-stack/THIRD-PARTY-NOTICES.head.txt) and CAIRO-STACK.txt
#     next to them (macOS: Contents/Resources);
#   - it sets, for the rest of the configure: PHARO_CAIRO_STACK_ACTIVE (ON),
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
set(PHARO_CAIRO_STACK_THIS_FILE "${CMAKE_CURRENT_LIST_FILE}")
include("${CMAKE_CURRENT_LIST_DIR}/thirdPartyNotice.cmake")

# The libraries of each platform, by pins.txt name: "<name>|<glob>..." relative
# to <prefix>/lib ("?<glob>": may match nothing).  Only these are bundled (not libcairo-script-interpreter, not
# the static zlib and expat, not libpng.so).
set(PHARO_CAIRO_STACK_LINUX_FILES
    "cairo|libcairo.so|libcairo.so.2|libcairo.so.2.*"
    "freetype|libfreetype.so|libfreetype.so.6|libfreetype.so.6.*"
    "pixman|libpixman-1.so|libpixman-1.so.0|libpixman-1.so.0.*"
    "libpng|libpng16.so|libpng16.so.16|libpng16.so.16.*"
    "fontconfig|libfontconfig.so|libfontconfig.so.1|libfontconfig.so.1.*")
set(PHARO_CAIRO_STACK_DARWIN_FILES
    "cairo|libcairo.dylib|libcairo.2.dylib"
    "freetype|libfreetype.dylib|libfreetype.6.dylib|?libfreetype.6.*.dylib"
    "pixman|libpixman-1.dylib|libpixman-1.0.dylib|?libpixman-1.0.*.dylib"
    "libpng|libpng16.dylib|libpng16.16.dylib|?libpng16.16.*.dylib")
# Libraries that only cairo needs (dropped with --only freetype)
set(PHARO_CAIRO_STACK_CAIRO_ONLY cairo pixman fontconfig expat)

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

# pharo_cairo_stack_guard(<out error> <out tool dirs>
#     SYSTEM <CMAKE_SYSTEM_NAME> [MINGW <bool>] [CYGWIN <bool>]
#     [CROSSCOMPILING <bool>] [OSX_ARCHITECTURES <list>]
#     HOST_PROCESSOR <CMAKE_HOST_SYSTEM_PROCESSOR> [PROCESSOR <CMAKE_SYSTEM_PROCESSOR>]
#     [POINTER_SIZE <bytes>])
# Whether the configure can build the stack here: <out error> is "" when it can,
# otherwise the reason.  The tools are looked for on PATH (the environment's,
# as build.sh will see it): sh, meson >= 1.11, ninja, pkg-config or pkgconf,
# cmake and, on Linux, gperf; on MinGW (MSYS2's binary packages) sh, curl,
# zstd and tar.  <out tool dirs> lists the directories they were found in,
# for the PATH of build-stack.sh.
function(pharo_cairo_stack_guard outError outDirs)
    cmake_parse_arguments(A "" "SYSTEM;MINGW;CYGWIN;CROSSCOMPILING;HOST_PROCESSOR;PROCESSOR;POINTER_SIZE" "OSX_ARCHITECTURES" ${ARGN})
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

    if(platform STREQUAL "mingw")
        set(tools "sh" "curl" "zstd" "tar")
    else()
        set(tools "sh" "meson" "ninja" "pkg-config|pkgconf" "cmake")
        if(platform STREQUAL "linux")
            list(APPEND tools "gperf")
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
        endif()
        unset(_pcs_tool CACHE)
    endforeach()
    if(missing)
        string(REPLACE ";" ", " m "${missing}")
        string(REPLACE "|" " or " m "${m}")
        if(platform STREQUAL "mingw")
            set(how "they come with MSYS2's shell (pacman -S curl zstd tar)")
        elseif(platform STREQUAL "linux")
            set(how "meson, ninja and pkgconf: pip install --require-hashes -r scripts/cairo-stack/requirements.txt into a venv on PATH; gperf: apt install gperf (docs/CairoStack.md \"Tools\")")
        else()
            set(how "meson, ninja and pkgconf: pip install --require-hashes -r scripts/cairo-stack/requirements.txt into a venv on PATH (docs/CairoStack.md \"Tools\")")
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
#     PLATFORM <p> ARCH <a> DEPLOYMENT_TARGET <t or -> [NEED_CAIRO <bool>])
# <out errors>: "" when the stack of the manifest is the one this build
# needs, otherwise every mismatch, "; "-separated.  The tool lines are not
# compared (informational).
function(pharo_cairo_stack_check_manifest out)
    cmake_parse_arguments(A "" "MANIFEST;PINS;PLATFORM;ARCH;DEPLOYMENT_TARGET;NEED_CAIRO" "" ${ARGN})
    if(NOT EXISTS "${A_MANIFEST}")
        set(${out} "no manifest ${A_MANIFEST} (not a prefix of scripts/cairo-stack/build.sh)" PARENT_SCOPE)
        return()
    endif()
    file(STRINGS "${A_MANIFEST}" lines)
    foreach(k format pins-sha256 platform arch deployment-target only)
        set(m_${k} "")
    endforeach()
    foreach(line IN LISTS lines)
        if(line MATCHES "^(format|pins-sha256|platform|arch|deployment-target|only)[ \t]+([^ \t]+)")
            set(m_${CMAKE_MATCH_1} "${CMAKE_MATCH_2}")
        endif()
    endforeach()
    file(SHA256 "${A_PINS}" pins_sha)
    set(errors "")
    if(NOT m_format STREQUAL "1")
        list(APPEND errors "format '${m_format}', not 1")
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
# Variables: PCS_PREFIX PCS_PINS PCS_HEAD PCS_PLATFORM PCS_ARCH
# PCS_DEPLOYMENT_TARGET PCS_ENV (mingw) PCS_CAIRO (ON/OFF) PCS_LIB_DIR
# PCS_NOTICE_DIR PCS_WORK
function(_pharo_cairo_stack_bundle)
    set(share "${PCS_PREFIX}/share/pharo-cairo-stack")
    pharo_cairo_stack_check_manifest(errors MANIFEST "${share}/manifest.txt" PINS "${PCS_PINS}"
        PLATFORM "${PCS_PLATFORM}" ARCH "${PCS_ARCH}" DEPLOYMENT_TARGET "${PCS_DEPLOYMENT_TARGET}" NEED_CAIRO "${PCS_CAIRO}")
    if(errors)
        message(FATAL_ERROR "cairoStack.cmake: the stack in ${PCS_PREFIX} is not this build's: ${errors}")
    endif()
    pharo_cairo_stack_read_pins("${PCS_PINS}" pin)
    file(MAKE_DIRECTORY "${PCS_LIB_DIR}" "${PCS_NOTICE_DIR}" "${PCS_WORK}")

    # 1. the libraries
    set(names "")       # whose LICENSE.<name> to write
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
            string(REGEX REPLACE "[ \t]+" ";" f "${line}")
            list(GET f 0 dll)
            list(GET f 1 kind)
            list(GET f 2 pkg)
            string(REGEX REPLACE "^mingw-w64-[a-z0-9]+-[a-z0-9_]+-" "" short "${pkg}")
            if(kind STREQUAL "runtime")
                # the C/C++ runtime: the toolchain's, which the install step
                # bundles (configure_installables, cmake/Windows.cmake)
                list(APPEND runtime_pkgs "${short}|${pkg}")
                continue()
            endif()
            if(dll STREQUAL "libcairo-2.dll" AND NOT PCS_CAIRO)
                continue()
            endif()
            list(APPEND files "${PCS_PREFIX}/bin/${dll}")
            list(APPEND names "${short}|${pkg}")
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
            if(NOT PCS_CAIRO AND name IN_LIST PHARO_CAIRO_STACK_CAIRO_ONLY)
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
        foreach(name IN LISTS pin_SOURCES)
            if(NOT PCS_PLATFORM IN_LIST pin_${name}_PLATFORMS AND NOT "all" IN_LIST pin_${name}_PLATFORMS)
                continue()
            endif()
            if(NOT PCS_CAIRO AND name IN_LIST PHARO_CAIRO_STACK_CAIRO_ONLY)
                continue()
            endif()
            list(APPEND names "${name}|${name}")
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
        set(dir "${share}/licenses/${short}")
        if(PCS_PLATFORM STREQUAL "mingw")
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
    if(PCS_PLATFORM STREQUAL "mingw")
        set(CAIRO_VERSION "${pin_MSYS2_${PCS_ENV}_mingw-w64-${PCS_MSYS2_PKG_ARCH}-cairo_VERSION}")
        set(FREETYPE_VERSION "${pin_MSYS2_${PCS_ENV}_mingw-w64-${PCS_MSYS2_PKG_ARCH}-freetype_VERSION}")
        set(ORIGIN "They are MSYS2's ${PCS_ENV} binary packages, pinned by URL and SHA256 in
cmake/cairo-stack/pins.txt of the VM's sources: cairo with MSYS2's two
patches (a DWrite/Direct2D glyph patch and cairo merge request 673), and
the libraries those packages need.")
        set(SOURCES "\nFor Windows the sources are MSYS2's source packages of the packages
above (the upstream archives, MSYS2's patches and build recipes, named
with their SHA256 in pins.txt), attached to the release as well.")
    else()
        set(CAIRO_VERSION "${pin_cairo_VERSION}")
        set(FREETYPE_VERSION "${pin_freetype_VERSION}")
        set(ORIGIN "They are built from the upstream release archives pinned by SHA256 in
cmake/cairo-stack/pins.txt of the VM's sources, unpatched, by
scripts/cairo-stack/build.sh.")
        set(SOURCES "")
    endif()
    if(PCS_CAIRO)
        set(CONTENTS "cairo ${CAIRO_VERSION} and FreeType ${FREETYPE_VERSION}")
    else()
        set(CONTENTS "FreeType ${FREETYPE_VERSION} (FEATURE_LIB_CAIRO is off: no cairo)")
    endif()
    string(REPLACE ";" "\n    " LICENSE_FILES "${written}")
    set(LICENSE_FILES "    ${LICENSE_FILES}")
    string(REPLACE ";" "\n    " LIBRARY_VERSIONS "${versions}")
    set(LIBRARY_VERSIONS "    ${LIBRARY_VERSIONS}")
    configure_file("${PCS_HEAD}" "${PCS_WORK}/THIRD-PARTY-NOTICES.cairo-stack.txt" @ONLY)
    configure_file("${share}/CAIRO-STACK.txt" "${PCS_WORK}/CAIRO-STACK.txt" COPYONLY)
    list(APPEND written THIRD-PARTY-NOTICES.cairo-stack.txt CAIRO-STACK.txt)
    foreach(n IN LISTS written)
        execute_process(COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${PCS_WORK}/${n}" "${PCS_NOTICE_DIR}/${n}"
            RESULT_VARIABLE rc)
        if(NOT rc EQUAL 0)
            message(FATAL_ERROR "cairoStack.cmake: cannot copy ${n} to ${PCS_NOTICE_DIR}")
        endif()
    endforeach()

    # Left in an older tree by the old files.pharo.org cairo and FreeType,
    # which earlier versions of this build bundled: not this stack's
    if(PCS_PLATFORM STREQUAL "linux")
        file(GLOB stale LIST_DIRECTORIES false "${PCS_LIB_DIR}/libharfbuzz*" "${PCS_LIB_DIR}/libbz2*"
            "${PCS_LIB_DIR}/libexpat*" "${PCS_LIB_DIR}/libcairo.so.2.1170*" "${PCS_LIB_DIR}/libcairo.so.2.1160*")
    elseif(PCS_PLATFORM STREQUAL "darwin")
        file(GLOB stale LIST_DIRECTORIES false "${PCS_LIB_DIR}/libharfbuzz*" "${PCS_LIB_DIR}/libbz2*"
            "${PCS_LIB_DIR}/libfontconfig*" "${PCS_LIB_DIR}/libz.*")
    else()
        set(stale "")
    endif()
    if(stale)
        message(WARNING "cairoStack.cmake: ${PCS_LIB_DIR} still has libraries that are not the cairo stack's "
            "(the old files.pharo.org cairo and FreeType of an earlier configure of this tree), "
            "which the bundle must not ship: ${stale}.  Build in a fresh tree.")
    endif()
    list(LENGTH files nf)
    message(STATUS "cairo stack: ${nf} files from ${PCS_PREFIX} into ${PCS_LIB_DIR}")
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
# the pinned one.  The runtime ones (kind runtime) stay the toolchain's.
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
        file(STRINGS \"${PHARO_CAIRO_STACK_PREFIX}/share/pharo-cairo-stack/dlls.txt\" _pcs_lines)
        foreach(_pcs_line IN LISTS _pcs_lines)
            string(REGEX REPLACE \"[ \\t]+\" \";\" _pcs_f \"\${_pcs_line}\")
            list(GET _pcs_f 0 _pcs_dll)
            list(GET _pcs_f 1 _pcs_kind)
            if(_pcs_kind STREQUAL \"runtime\" OR _pcs_dll STREQUAL \"${skip}\")
                continue()
            endif()
            message(STATUS \"Bundling the cairo stack's \${_pcs_dll}\")
            file(INSTALL \"${PHARO_CAIRO_STACK_PREFIX}/bin/\${_pcs_dll}\" DESTINATION \"\$ENV{DESTDIR}\${CMAKE_INSTALL_PREFIX}\")
        endforeach()
    " COMPONENT ${component})
endfunction()

set(PHARO_CAIRO_STACK_ACTIVE OFF)
set(PHARO_CAIRO_STACK_PREFIX "" CACHE INTERNAL "The cairo stack prefix of this build (cmake/cairoStack.cmake)")
set(PHARO_CAIRO_STACK_INCLUDE_DIRS "" CACHE INTERNAL "The include directories of the cairo stack (cmake/cairoStack.cmake)")

if(BUILD_BUNDLE AND (FEATURE_LIB_CAIRO OR FEATURE_LIB_FREETYPE2))
    if(FEATURE_LIB_CAIRO AND NOT FEATURE_LIB_FREETYPE2)
        message(FATAL_ERROR "FEATURE_LIB_CAIRO=ON with FEATURE_LIB_FREETYPE2=OFF: the cairo stack bundles "
            "FreeType, since cairo-ft needs the image's FreeType (one FreeType for both, docs/CairoStack.md). "
            "Turn FEATURE_LIB_FREETYPE2 on, or FEATURE_LIB_CAIRO off.")
    endif()
    set(PHARO_CAIRO_STACK_ACTIVE ON)
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
        "elsewhere, docs/CairoStack.md), or with -DFEATURE_LIB_CAIRO=OFF -DFEATURE_LIB_FREETYPE2=OFF "
        "(a VM without cairo and FreeType).")
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

    if(PHARO_CAIRO_STACK_DIR)
        # A stack built beforehand (CI, or a developer's reuse): validate its manifest
        get_filename_component(_pcs_prefix "${PHARO_CAIRO_STACK_DIR}" ABSOLUTE)
        pharo_cairo_stack_check_manifest(_pcs_errors
            MANIFEST "${_pcs_prefix}/share/pharo-cairo-stack/manifest.txt" PINS "${PHARO_CAIRO_STACK_PINS_FILE}"
            PLATFORM "${PHARO_CAIRO_STACK_PLATFORM}" ARCH "${PHARO_CAIRO_STACK_ARCH}"
            DEPLOYMENT_TARGET "${PHARO_CAIRO_STACK_DEPLOYMENT_TARGET}" NEED_CAIRO "${FEATURE_LIB_CAIRO}")
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
            POINTER_SIZE "${CMAKE_SIZEOF_VOID_P}")
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
        set(_pcs_paths _pcs_prefix _pcs_work _pcs_downloads _pcs_script)
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
        endif()
        if(PHARO_CAIRO_STACK_GLIBC_MAX AND PHARO_CAIRO_STACK_PLATFORM STREQUAL "linux")
            string(APPEND _pcs_args " --glibc-max ${PHARO_CAIRO_STACK_GLIBC_MAX}")
        endif()
        set(_pcs_env "")
        if(NOT PHARO_CAIRO_STACK_PLATFORM STREQUAL "mingw")
            _pharo_cairo_stack_sh_quote(_q_cc "${CMAKE_C_COMPILER}")
            _pharo_cairo_stack_sh_quote(_q_cxx "${CMAKE_CXX_COMPILER}")
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
        elseif(PHARO_CAIRO_STACK_PLATFORM STREQUAL "darwin")
            set(_pcs_byproducts "${_pcs_prefix}/lib/libfreetype.6.dylib" "${_pcs_prefix}/lib/libpng16.16.dylib")
            if(FEATURE_LIB_CAIRO)
                list(APPEND _pcs_byproducts "${_pcs_prefix}/lib/libcairo.2.dylib" "${_pcs_prefix}/lib/libpixman-1.0.dylib")
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
        file(GLOB _pcs_inputs "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/*.sh" "${PHARO_CAIRO_STACK_SCRIPTS_DIR}/probe.c")
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
            "-DPCS_LIB_DIR=${LIBRARY_OUTPUT_DIRECTORY}"
            "-DPCS_NOTICE_DIR=${_pcs_notice_dir}"
            "-DPCS_WORK=${CMAKE_BINARY_DIR}/cairo-stack/notices"
            -P "${PHARO_CAIRO_STACK_THIS_FILE}"
        COMMENT "Bundling the cairo stack (cmake/cairoStack.cmake)"
        VERBATIM)
    add_dependencies(pharo_cairo_stack_bundle pharo_cairo_stack)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${PHARO_CAIRO_STACK_PINS_FILE}")
endif()
