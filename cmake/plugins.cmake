include(cmake/plugins.macros.cmake)

add_vm_plugin(FilePlugin TRUE TRUE)
if(OSX)
    target_link_libraries(FilePlugin PRIVATE "-framework CoreFoundation")
endif()
if(WIN)
    target_compile_definitions(FilePlugin PRIVATE "-DWIN32_FILE_SUPPORT")
endif()

add_vm_plugin(NewFilePlugin TRUE TRUE)


add_vm_plugin(FileAttributesPlugin FALSE TRUE)
target_link_libraries(FileAttributesPlugin PRIVATE FilePlugin)


# UUIDPlugin

if(FEATURE_PLUGIN_UUID AND NOT OPENBSD)
    message(STATUS "Adding plugin: UUIDPlugin")

    file(GLOB UUIDPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/plugins/UUIDPlugin/common/*.c
    )

    addLibraryWithRPATH(UUIDPlugin ${UUIDPlugin_SOURCES})
    if(WIN)
        target_link_libraries(UUIDPlugin PRIVATE "-lole32")
    elseif(CMAKE_SYSTEM_NAME STREQUAL "FreeBSD" OR EMSCRIPTEN)
        # FreeBSD provides uuidgen(2) in libc, and Emscripten uuid_generate in
        # its JavaScript library: no separate libuuid is needed.
    elseif(UNIX AND NOT OSX)
       #find_path(LIB_UUID_INCLUDE_DIR uuid.h PATH_SUFFIXES uuid)
        find_library(LIB_UUID_LIBRARY uuid)
        message(STATUS "Using uuid library:" ${LIB_UUID_LIBRARY})
        target_link_libraries(UUIDPlugin PRIVATE ${LIB_UUID_LIBRARY})
    endif()
endif()

# Socket Plugin
if (${FEATURE_NETWORK})
    add_vm_plugin(SocketPlugin FALSE FALSE)
  if(WIN)
    target_link_libraries(SocketPlugin PRIVATE "-lws2_32")
  elseif(NOT EMSCRIPTEN)
    # The resolver's lookup threads (ASYNC_RESOLVER in SocketPluginImpl.c):
    # link pthreads explicitly rather than through the VM, which a glibc
    # before 2.34 does not export them from
    find_package(Threads REQUIRED)
    target_link_libraries(SocketPlugin PRIVATE Threads::Threads)
  endif()
endif()

# The Emscripten VM has no SurfacePlugin: what blits through its manual
# surfaces is OSWindow-SDL2, and the Emscripten VM has no SDL2 (its world
# goes through OSWebDriver, and Athens draws through the AthensCairoSurface
# extension of OSWindow-Web, which needs no SurfacePlugin); the Athens of a
# stock image fails cleanly without it, as lane 65 checks.  (Its
# sqManualSurface.c, which calls the surface functions of SurfacePlugin.c
# directly, would not do as a builtin plugin of Emscripten, where those are
# static and WebAssembly checks the type of an indirect call)
if(NOT EMSCRIPTEN)
    add_vm_plugin(SurfacePlugin TRUE FALSE)
endif()
add_vm_plugin(FloatArrayPlugin TRUE FALSE)
add_vm_plugin(LargeIntegers FALSE FALSE)
add_vm_plugin(JPEGReaderPlugin FALSE FALSE)
add_vm_plugin(JPEGReadWriter2Plugin FALSE FALSE)
add_vm_plugin(MiscPrimitivePlugin FALSE FALSE)
add_vm_plugin(DSAPrims FALSE FALSE)
add_vm_plugin(BitBltPlugin FALSE FALSE)
add_vm_plugin(B2DPlugin FALSE FALSE)

add_vm_plugin(LocalePlugin FALSE TRUE)
if(OSX)
	target_link_libraries(LocalePlugin PRIVATE "-framework CoreFoundation")
endif()

if(FEATURE_PLUGIN_SSL)
    add_vm_plugin(SqueakSSL FALSE FALSE)
    if(OSX)
        target_link_libraries(SqueakSSL PRIVATE "-framework CoreFoundation")
        target_link_libraries(SqueakSSL PRIVATE "-framework Security")
    elseif(WIN)
        target_link_libraries(SqueakSSL PRIVATE crypt32 secur32)
    else()
        find_package(OpenSSL REQUIRED)
        target_link_libraries(SqueakSSL PRIVATE OpenSSL::SSL OpenSSL::Crypto)
        # The VM builds on an ubuntu with openssl 1.0, thus the ssl plugin links to it.
        # Ship ssl 1.0 with the VM, so the ssl plugin loads
        # (files.pharo.org has no openssl 1.0.2q for Linux on ARM)
        if(BUILD_BUNDLE AND NOT CMAKE_SYSTEM_PROCESSOR MATCHES "aarch64|armv7l")
            add_third_party_dependency("openssl-1.0.2q")
        endif()
    endif()
endif()

# SimdJSONPlugin: JSON reading with simdjson 5.0.5 (C++17), the plugin of
# the package SimdJSON (the submodule plugins/SimdJSONPlugin/simdjson.st).
# Declared here, with the plugin, rather than next to FEATURE_PLUGIN_UUID in
# CMakeLists.txt.  ON everywhere; verified on Linux and Emscripten only:
# macOS (a .dylib with the libc++ of the system) and Windows (MSYS2, whose
# C++ runtime DLLs the install step of cmake/Windows.cmake bundles) have
# never built it
option(FEATURE_PLUGIN_SIMDJSON "Build the SimdJSONPlugin (simdjson JSON reader, C++17)" ON)
if(FEATURE_PLUGIN_SIMDJSON)
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/simdjson.cmake)
    add_vm_plugin(SimdJSONPlugin FALSE FALSE)            # globs src/common/*.c; a builtin STATIC lib on Emscripten
    target_sources(SimdJSONPlugin PRIVATE
        ${CMAKE_CURRENT_SOURCE_DIR}/plugins/SimdJSONPlugin/src/cxx/sjEngine.cpp)
    target_link_libraries(SimdJSONPlugin PRIVATE pharo_simdjson)
    # primitiveInfo answers this version (SimdJSONPlugin.c has no simdjson.h)
    target_compile_definitions(SimdJSONPlugin PRIVATE SJ_SIMDJSON_VERSION="${PHARO_SIMDJSON_VERSION}")
    target_compile_options(SimdJSONPlugin PRIVATE -O3 -g0)
    foreach(option IN LISTS PHARO_SIMDJSON_CXX_OPTIONS)
        target_compile_options(SimdJSONPlugin PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${option}>)
    endforeach()
    set_target_properties(SimdJSONPlugin PROPERTIES
        CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
        CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)

    # libSimdJSONFFI: the same engine for the FFI of the image (the package
    # SimdJSON-FFI), on a VM without the plugin: not a plugin, it has no VM
    # header, and only its sj_ffi_* functions are exported (sjFFI.h).
    # Natively only: the web VMs have the plugin built in
    if(NOT EMSCRIPTEN)
        option(FEATURE_SIMDJSON_FFI "Build libSimdJSONFFI, simdjson for the FFI of the package SimdJSON-FFI" ON)
    endif()
    if(FEATURE_SIMDJSON_FFI AND NOT EMSCRIPTEN)
        set(sj_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/SimdJSONPlugin)
        addIndependentLibraryWithRPATH(SimdJSONFFI
            ${sj_dir}/src/ffi/sjFFI.c
            ${sj_dir}/src/common/sjText.c
            ${sj_dir}/src/cxx/sjEngine.cpp)
        add_dependencies(${VM_EXECUTABLE_NAME} SimdJSONFFI)
        target_include_directories(SimdJSONFFI PRIVATE ${sj_dir}/include/common ${sj_dir}/include/ffi)
        target_link_libraries(SimdJSONFFI PRIVATE pharo_simdjson)
        target_compile_definitions(SimdJSONFFI PRIVATE SJ_SIMDJSON_VERSION="${PHARO_SIMDJSON_VERSION}")
        target_compile_options(SimdJSONFFI PRIVATE -O3 -g0)
        foreach(option IN LISTS PHARO_SIMDJSON_CXX_OPTIONS)
            target_compile_options(SimdJSONFFI PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${option}>)
        endforeach()
        set_target_properties(SimdJSONFFI PROPERTIES
            CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
            C_VISIBILITY_PRESET hidden CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
    endif()
endif()

# SimdUTFPlugin: transcoding between UTF-8, UTF-16LE/BE, Latin-1 and UTF-32,
# validation and base64 with simdutf 9.4.0 (C++17), the plugin of the package
# SimdUTF (the submodule plugins/SimdUTFPlugin/simdutf.st); docs/SimdUTF.md.
# ON on every 64-bit target, OFF on 32-bit ones (Linux armv7l,
# cmake/vmmaker.cmake), where turning it on is refused: the status word that
# its primitives answer needs the 61-bit SmallIntegers of a 64-bit VM.
# simdutf has never run upstream on Windows arm64 (MinGW clang) nor on macOS
# x86_64: the CI steps of those jobs are its first test there
if(SIZEOF_VOID_P EQUAL 8)
    set(pharo_simdutf_default ON)
else()
    set(pharo_simdutf_default OFF)
endif()
option(FEATURE_PLUGIN_SIMDUTF "Build the SimdUTFPlugin (simdutf transcoding and base64, C++17)" ${pharo_simdutf_default})
if(FEATURE_PLUGIN_SIMDUTF AND NOT SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "FEATURE_PLUGIN_SIMDUTF needs a 64-bit VM (void* is ${SIZEOF_VOID_P} bytes)")
endif()
if(FEATURE_PLUGIN_SIMDUTF)
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/simdutf.cmake)
    set(su_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/SimdUTFPlugin)
    add_vm_plugin(SimdUTFPlugin FALSE FALSE)            # globs src/common/*.c; a builtin STATIC lib on Emscripten
    target_sources(SimdUTFPlugin PRIVATE ${su_dir}/src/cxx/suEngine.cpp)
    target_link_libraries(SimdUTFPlugin PRIVATE pharo_simdutf)
    # (no version definition: the engine answers SIMDUTF_VERSION of simdutf.h)
    target_compile_options(SimdUTFPlugin PRIVATE -O3 -g0)
    foreach(option IN LISTS PHARO_SIMDUTF_CXX_OPTIONS)
        target_compile_options(SimdUTFPlugin PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${option}>)
    endforeach()
    # (C keeps the default visibility: the VM looks up the entry points)
    set_target_properties(SimdUTFPlugin PROPERTIES
        CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
        CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)

    # libSimdUTFFFI: the same engine for the FFI of the image (the package
    # SimdUTF-FFI) on a VM without the plugin: not a plugin, it has no VM
    # header, and only its su_ffi_* functions are exported (suFFI.h).
    # Natively only: the web VMs have the plugin built in
    if(NOT EMSCRIPTEN)
        option(FEATURE_SIMDUTF_FFI "Build libSimdUTFFFI, simdutf for the FFI of the package SimdUTF-FFI" ON)
    endif()
    if(FEATURE_SIMDUTF_FFI AND NOT EMSCRIPTEN)
        addIndependentLibraryWithRPATH(SimdUTFFFI
            ${su_dir}/src/ffi/suFFI.c
            ${su_dir}/src/cxx/suEngine.cpp)
        add_dependencies(${VM_EXECUTABLE_NAME} SimdUTFFFI)
        target_include_directories(SimdUTFFFI PRIVATE ${su_dir}/include/common ${su_dir}/include/ffi)
        target_link_libraries(SimdUTFFFI PRIVATE pharo_simdutf)
        target_compile_options(SimdUTFFFI PRIVATE -O3 -g0)
        foreach(option IN LISTS PHARO_SIMDUTF_CXX_OPTIONS)
            target_compile_options(SimdUTFFFI PRIVATE $<$<COMPILE_LANGUAGE:CXX>:${option}>)
        endforeach()
        set_target_properties(SimdUTFFFI PROPERTIES
            CXX_STANDARD 17 CXX_STANDARD_REQUIRED ON CXX_EXTENSIONS OFF
            C_VISIBILITY_PRESET hidden CXX_VISIBILITY_PRESET hidden VISIBILITY_INLINES_HIDDEN ON)
    endif()
endif()

# TimSortPlugin: stable sorts of Arrays of numbers or strings, of numeric
# buffers and of permutations by keys with timsort.c (C, the sort of
# CPython's lists), and the resumable sort by any sortBlock, whose
# comparisons the image answers, on a C stack of its own; the plugin of the
# package TimSort (the submodule plugins/TimSortPlugin/timsort.st);
# docs/TimSort.md.  ON on every 64-bit target, OFF on 32-bit ones, where
# turning it on is refused: the requests of the resumable sort are 61-bit
# SmallIntegers.  The resumable sort (src/coro) is compiled natively only:
# WebAssembly cannot switch C stacks, and its primitives report it
# unsupported there.  No HOST_PLUGIN_FLAGS entry in the GNUmakefile: the host
# tree of make wasm builds no plugin (only the VMMaker VM and image and the
# generated sources), but it configures this one, and so fetches the archive
# of timsort.c (WASM_DEPS_DIR, else GitHub) at its configure
if(SIZEOF_VOID_P EQUAL 8)
    set(pharo_timsort_default ON)
else()
    set(pharo_timsort_default OFF)
endif()
option(FEATURE_PLUGIN_TIMSORT "Build the TimSortPlugin (timsort.c stable sorts, C)" ${pharo_timsort_default})
if(FEATURE_PLUGIN_TIMSORT AND NOT SIZEOF_VOID_P EQUAL 8)
    message(FATAL_ERROR "FEATURE_PLUGIN_TIMSORT needs a 64-bit VM (void* is ${SIZEOF_VOID_P} bytes)")
endif()
if(FEATURE_PLUGIN_TIMSORT)
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/timsort.cmake)
    set(ts_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/TimSortPlugin)
    # the engine (the one file that includes timsort.h) and, natively, the
    # resumable sort: every .c of src/coro (CONFIGURE_DEPENDS: a new one
    # configures again), each compiling only for its own targets
    set(ts_sources ${ts_dir}/src/engine/tsEngine.c)
    if(NOT EMSCRIPTEN)
        file(GLOB ts_coro_sources CONFIGURE_DEPENDS ${ts_dir}/src/coro/*.c)
        list(APPEND ts_sources ${ts_coro_sources})
    endif()
    add_vm_plugin(TimSortPlugin FALSE FALSE)            # globs src/common/*.c; a builtin STATIC lib on Emscripten
    target_sources(TimSortPlugin PRIVATE ${ts_sources})
    target_link_libraries(TimSortPlugin PRIVATE pharo_timsort)
    # primitiveInfo answers 'timsort.c <this>' (the glue has no timsort.h)
    target_compile_definitions(TimSortPlugin PRIVATE TS_TIMSORT_VERSION="${PHARO_TIMSORT_VERSION}")
    target_compile_options(TimSortPlugin PRIVATE -O3 -g0)
    # (C keeps the default visibility: the VM looks up the entry points)

    # libTimSortFFI: the same engine and resumable sort for the FFI of the
    # image (the package TimSort-FFI) on a VM without the plugin: not a
    # plugin, it has no VM header, and only its ts_ffi_* functions are
    # exported (tsFFI.h).  Natively only: the web VMs have the plugin built in
    if(NOT EMSCRIPTEN)
        option(FEATURE_TIMSORT_FFI "Build libTimSortFFI, timsort.c for the FFI of the package TimSort-FFI" ON)
    endif()
    if(FEATURE_TIMSORT_FFI AND NOT EMSCRIPTEN)
        addIndependentLibraryWithRPATH(TimSortFFI ${ts_dir}/src/ffi/tsFFI.c ${ts_sources})
        add_dependencies(${VM_EXECUTABLE_NAME} TimSortFFI)
        target_include_directories(TimSortFFI PRIVATE ${ts_dir}/include/common ${ts_dir}/include/ffi)
        target_link_libraries(TimSortFFI PRIVATE pharo_timsort)
        target_compile_definitions(TimSortFFI PRIVATE TS_TIMSORT_VERSION="${PHARO_TIMSORT_VERSION}")
        target_compile_options(TimSortFFI PRIVATE -O3 -g0)
        set_target_properties(TimSortFFI PROPERTIES C_VISIBILITY_PRESET hidden)
    endif()
endif()

# UtilsPlugin: the fused multiply-add of Floats, the splitting of byte
# strings (strtok_r), the points of Bezier curves (de Casteljau) and Knuth's
# lagged Fibonacci generator of fractions (his rng-double.c, unchanged in
# src/knuth), ported from the UtilsPlugin of the pharo-10 branch; the plugin
# of the package VM-Plugins-Primitives (the submodule
# plugins/UtilsPlugin/vm-plugins-primitives.st); docs/UtilsPlugin.md.  ON
# everywhere: C99 and no dependency.  -ffp-contract=off: the engine rounds
# as written, so that the Smalltalk twins of the image answer the same bits
option(FEATURE_PLUGIN_UTILS "Build the UtilsPlugin (fma, strtok_r, de Casteljau, Knuth's generator)" ON)
if(FEATURE_PLUGIN_UTILS)
    set(up_dir ${CMAKE_CURRENT_SOURCE_DIR}/plugins/UtilsPlugin)
    add_vm_plugin(UtilsPlugin FALSE FALSE)               # globs src/common/*.c; a builtin STATIC lib on Emscripten
    target_sources(UtilsPlugin PRIVATE ${up_dir}/src/engine/upEngine.c)
    target_compile_options(UtilsPlugin PRIVATE -O2 -ffp-contract=off)
    if(UNIX AND NOT EMSCRIPTEN)
        target_link_libraries(UtilsPlugin PRIVATE m)    # fma
    endif()
endif()

# UnixOSProcessPlugin (it needs fork(), which Emscripten has not)
if(NOT WIN AND NOT EMSCRIPTEN)
    add_vm_plugin(UnixOSProcessPlugin FALSE FALSE)
    target_link_libraries(UnixOSProcessPlugin PRIVATE FilePlugin)
    target_link_libraries(UnixOSProcessPlugin PRIVATE SocketPlugin)
endif()
