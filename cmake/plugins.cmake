include(cmake/plugins.macros.cmake)

# Each C plugin is skipped when its Rust replacement is being built instead;
# RUST_REPLACED_PLUGINS (cmake/rust.cmake) is empty unless USE_RUST_PLUGINS is
# ON, so the default build is exactly the all-C one. Both define a CMake target
# of the plugin's name, so exactly one of them may be added.
if(NOT "FilePlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(FilePlugin TRUE TRUE)
    if(OSX)
        target_link_libraries(FilePlugin PRIVATE "-framework CoreFoundation")
    endif()
    if(WIN)
        target_compile_definitions(FilePlugin PRIVATE "-DWIN32_FILE_SUPPORT")
    endif()
endif()

if(NOT "NewFilePlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(NewFilePlugin TRUE TRUE)
endif()


# The link to FilePlugin exists for sq2uxPath/ux2sqPath; the Rust replacement
# resolves those at runtime via ioLoadFunctionFrom instead. rust.cmake replaces
# these two in the same platform group, so the C pairing stays consistent.
if(NOT "FileAttributesPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(FileAttributesPlugin FALSE TRUE)
    target_link_libraries(FileAttributesPlugin PRIVATE FilePlugin)
endif()


# UUIDPlugin

if(FEATURE_PLUGIN_UUID AND NOT OPENBSD
   AND NOT "UUIDPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    message(STATUS "Adding plugin: UUIDPlugin")

    file(GLOB UUIDPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/plugins/UUIDPlugin/common/*.c
    )

    addLibraryWithRPATH(UUIDPlugin ${UUIDPlugin_SOURCES})
    if(WIN)
        target_link_libraries(UUIDPlugin PRIVATE "-lole32")
    elseif(CMAKE_SYSTEM_NAME STREQUAL "FreeBSD")
        # FreeBSD provides uuidgen(2) in libc; no separate libuuid is needed.
    elseif(UNIX AND NOT OSX)
       #find_path(LIB_UUID_INCLUDE_DIR uuid.h PATH_SUFFIXES uuid)
        find_library(LIB_UUID_LIBRARY uuid)
        message(STATUS "Using uuid library:" ${LIB_UUID_LIBRARY})
        target_link_libraries(UUIDPlugin PRIVATE ${LIB_UUID_LIBRARY})
    endif()
endif()

# Socket Plugin
if(${FEATURE_NETWORK} AND NOT "SocketPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(SocketPlugin FALSE FALSE)
  if(WIN)
    target_link_libraries(SocketPlugin PRIVATE "-lws2_32")
  endif()
endif()

if(NOT "SurfacePlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(SurfacePlugin TRUE FALSE)
endif()
add_vm_plugin(FloatArrayPlugin TRUE FALSE)
if(NOT "LargeIntegers" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(LargeIntegers FALSE FALSE)
endif()
if(NOT "JPEGReaderPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(JPEGReaderPlugin FALSE FALSE)
endif()
# Built from rust/plugins/jpeg-plugin when USE_RUST_PLUGINS is ON. Both define
# a target of this name, so exactly one of them may be added.
if(NOT "JPEGReadWriter2Plugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(JPEGReadWriter2Plugin FALSE FALSE)
endif()
if(NOT "MiscPrimitivePlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(MiscPrimitivePlugin FALSE FALSE)
endif()
if(NOT "DSAPrims" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(DSAPrims FALSE FALSE)
endif()
if(NOT "BitBltPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(BitBltPlugin FALSE FALSE)
endif()
if(NOT "B2DPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(B2DPlugin FALSE FALSE)
endif()

if(NOT "LocalePlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(LocalePlugin FALSE TRUE)
    if(OSX)
        target_link_libraries(LocalePlugin PRIVATE "-framework CoreFoundation")
    endif()
endif()

if(FEATURE_PLUGIN_SSL AND NOT "SqueakSSL" IN_LIST RUST_REPLACED_PLUGINS)
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
        if(BUILD_BUNDLE)
            add_third_party_dependency("openssl-1.0.2q")
        endif()
    endif()
endif()

# UnixOSProcessPlugin
# The FilePlugin link is real (SQFile records, sqFileStdioHandlesInto); the
# SocketPlugin one feeds only dead code. The Rust replacement resolves its
# FilePlugin needs at runtime via ioLoadFunctionFrom, and rust.cmake replaces
# all three plugins in the same platform group, so the C pairing stays
# consistent wherever this C plugin still builds.
if(NOT WIN AND NOT "UnixOSProcessPlugin" IN_LIST RUST_REPLACED_PLUGINS)
    add_vm_plugin(UnixOSProcessPlugin FALSE FALSE)
    target_link_libraries(UnixOSProcessPlugin PRIVATE FilePlugin)
    target_link_libraries(UnixOSProcessPlugin PRIVATE SocketPlugin)
endif()
