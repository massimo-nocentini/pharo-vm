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
  endif()
endif()

# Emscripten has only builtin plugins: there sqManualSurface.c takes the
# surface functions of SurfacePlugin through ioLoadFunctionFrom.  The
# Emscripten VM has SurfacePlugin only with SDL2 (WASM_SDL2,
# cmake/emscripten/deps/options.cmake), since OSWindow-SDL2 blits through
# its manual surfaces
if(NOT EMSCRIPTEN OR PHARO_WASM_HAS_SDL2)
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
# plugins/SimdJSONPlugin/st/SimdJSON.st.  Declared here, with the plugin,
# rather than next to FEATURE_PLUGIN_UUID in CMakeLists.txt.  ON everywhere;
# verified on Linux and Emscripten only: macOS (a .dylib with the libc++ of
# the system) and Windows (MSYS2, whose C++ runtime DLLs the install step of
# cmake/Windows.cmake bundles) have never built it
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
endif()

# UnixOSProcessPlugin (it needs fork(), which Emscripten has not)
if(NOT WIN AND NOT EMSCRIPTEN)
    add_vm_plugin(UnixOSProcessPlugin FALSE FALSE)
    target_link_libraries(UnixOSProcessPlugin PRIVATE FilePlugin)
    target_link_libraries(UnixOSProcessPlugin PRIVATE SocketPlugin)
endif()
