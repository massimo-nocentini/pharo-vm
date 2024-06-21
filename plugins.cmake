include(plugins.macros.cmake)

#
# FilePlugin
#
message(STATUS "Adding plugin: FilePlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/osx
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/unix
    )
    
    file(GLOB FilePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/src/unix/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/vm/src/unix/sqUnixCharConv.c        
        ${CMAKE_CURRENT_SOURCE_DIR}/src/fileUtils.c
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/unix
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/vm/include/unix
    )
    
    file(GLOB FilePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/src/unix/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/fileUtils.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/vm/src/unix/sqUnixCharConv.c
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/include/win
    )
    
    file(GLOB FilePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/FilePlugin/src/win/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/fileUtilsWin.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/vm/src/win/sqWin32Directory.c        
    )    
endif()

addLibraryWithRPATH(FilePlugin
    ${FilePlugin_SOURCES}
    ${PHARO_CURRENT_GENERATED}/plugins/src/FilePlugin/FilePlugin.c)

if(OSX)
    target_link_libraries(FilePlugin PRIVATE "-framework CoreFoundation")
endif()

if(WIN)
    target_compile_definitions(FilePlugin PRIVATE "-DWIN32_FILE_SUPPORT")
endif()


#
# FileAttributesPlugin
#
add_vm_plugin(FileAttributesPlugin)
target_link_libraries(FileAttributesPlugin PRIVATE FilePlugin)

#
# UUIDPlugin
#

if(NOT OPENBSD)
    message(STATUS "Adding plugin: UUIDPlugin")

    file(GLOB UUIDPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/plugins/UUIDPlugin/common/*.c
    )

    addLibraryWithRPATH(UUIDPlugin ${UUIDPlugin_SOURCES})
    if(WIN)
        target_link_libraries(UUIDPlugin PRIVATE "-lole32")
    elseif(UNIX AND NOT OSX)
       #find_path(LIB_UUID_INCLUDE_DIR uuid.h PATH_SUFFIXES uuid)
        find_library(LIB_UUID_LIBRARY uuid)
        message(STATUS "Using uuid library:" ${LIB_UUID_LIBRARY})
        target_link_libraries(UUIDPlugin PRIVATE ${LIB_UUID_LIBRARY})
    endif()
endif()

#
# Socket Plugin
#
if (${FEATURE_NETWORK})
    add_vm_plugin(SocketPlugin)
  if(WIN)
    target_link_libraries(SocketPlugin PRIVATE "-lWs2_32")
  endif()
endif()

#
# Surface Plugin
#

add_vm_plugin(SurfacePlugin 
	${PHARO_CURRENT_GENERATED}/plugins/src/SurfacePlugin/SurfacePlugin.c)

#
# FloatArray Plugin
#

add_vm_plugin(FloatArrayPlugin 
	${PHARO_CURRENT_GENERATED}/plugins/src/FloatArrayPlugin/FloatArrayPlugin.c)

#
# LargeIntegers Plugin
#

add_vm_plugin(LargeIntegers)

#
# JPEGReaderPlugin
#

add_vm_plugin(JPEGReaderPlugin)

#
# JPEGReadWriter2Plugin
#

add_vm_plugin(JPEGReadWriter2Plugin)


#
# MiscPrimitivePlugin
#

add_vm_plugin(MiscPrimitivePlugin)

#
# BitBltPlugin
#

include_directories(
    ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/BitBltPlugin/include/common
)

set(BitBltPlugin_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/BitBltPlugin/src/common/BitBltPlugin.c   
)

addLibraryWithRPATH(BitBltPlugin ${BitBltPlugin_SOURCES})

#
# B2DPlugin
#

add_vm_plugin(B2DPlugin)

#
# LocalePlugin
#

message(STATUS "Adding plugin: LocalePlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/osx
    )
    
    file(GLOB LocalePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/unix
    )
    
    file(GLOB LocalePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/include/win
    )
    
    file(GLOB LocalePlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LocalePlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(LocalePlugin ${LocalePlugin_SOURCES})

if(OSX)
	target_link_libraries(LocalePlugin PRIVATE "-framework CoreFoundation")
endif()

#
# DateTimeFormatterPlugin
#

message(STATUS "Adding plugin: DateTimeFormatterPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/osx
        /usr/local/include
    )
    
    file(GLOB DateTimeFormatterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/unix
    )
    
    file(GLOB DateTimeFormatterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/include/win
        ${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/include
    )
    
    file(GLOB DateTimeFormatterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/DateTimeFormatterPlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(DateTimeFormatterPlugin ${DateTimeFormatterPlugin_SOURCES})

if(OSX)
	target_link_libraries(DateTimeFormatterPlugin PRIVATE "-L/usr/local/lib -ldatetimeformatter")
elseif(UNIX)
    target_link_libraries(DateTimeFormatterPlugin PRIVATE "-ldatetimeformatter")
else()
    target_link_libraries(DateTimeFormatterPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/lib -ldatetimeformatter")
endif()


#
# LuaPlugin
#

message(STATUS "Adding plugin: LuaPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/osx
        /usr/local/include
    )
    
    file(GLOB LuaPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/unix
    )
    
    file(GLOB LuaPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/include/win
        ${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/include
    )
    
    file(GLOB LuaPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/LuaPlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(LuaPlugin ${LuaPlugin_SOURCES})

if(OSX)
	target_link_libraries(LuaPlugin PRIVATE "-L/usr/local/lib -llua")
elseif(UNIX)
    target_link_libraries(LuaPlugin PRIVATE "-llua")
else()
    target_link_libraries(LuaPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/lib -llua54")
endif()

#
# UtilsPlugin
#

message(STATUS "Adding plugin: UtilsPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/osx
        /usr/local/include
    )
    
    file(GLOB UtilsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/unix
    )
    
    file(GLOB UtilsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/include/win
        ${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/include
    )
    
    file(GLOB UtilsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/UtilsPlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(UtilsPlugin ${UtilsPlugin_SOURCES})

if(OSX)
	target_link_libraries(UtilsPlugin PRIVATE "-L/usr/local/lib -ltimsort")
elseif(UNIX)
    target_link_libraries(UtilsPlugin PRIVATE "-ltimsort")
else()
    target_link_libraries(UtilsPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/lib -ltimsort")
endif()


#
# CairoGraphicsPlugin
#

message(STATUS "Adding plugin: CairoGraphicsPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/osx
        /usr/local/include/pango-1.0
        /usr/local/lib/glib-2.0/include
        /usr/local/include/glib-2.0
        /usr/local/include/harfbuzz
        /usr/local/include/cairo
        /usr/local/include/gdk-pixbuf-2.0
        /usr/local/include/gtk-4.0
    )
    
    file(GLOB CairoGraphicsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/unix
        /usr/include/pango-1.0
        /usr/include/harfbuzz
        /usr/include/libmount
        /usr/include/blkid
        /usr/include/fribidi
        /usr/include/cairo
        /usr/include/glib-2.0
        /usr/lib/x86_64-linux-gnu/glib-2.0/include
        /usr/include/pixman-1
        /usr/include/uuid
        /usr/include/freetype2
        /usr/include/libpng16
        /usr/include/gdk-pixbuf-2.0
        /usr/include/gtk-4.0
    )
    
    file(GLOB CairoGraphicsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/include/win
        ${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/include
        D:/msys64/ucrt64/include/pango-1.0
        D:/msys64/ucrt64/include/glib-2.0
        D:/msys64/ucrt64/lib/glib-2.0/include
        D:/msys64/ucrt64/include/cairo
        D:/msys64/ucrt64/include/harfbuzz
        D:/msys64/ucrt64/include/gdk-pixbuf-2.0
        D:/msys64/ucrt64/include/gtk-4.0
    )
    
    file(GLOB CairoGraphicsPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/CairoGraphicsPlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(CairoGraphicsPlugin ${CairoGraphicsPlugin_SOURCES})

if(OSX)
	target_link_libraries(CairoGraphicsPlugin PRIVATE "-L/usr/local/lib -lpango-1.0 -lpangocairo-1.0 -lcairo -lglib-2.0 -lgobject-2.0 -lgdk_pixbuf-2.0.0 -lgtk-4.1")
elseif(UNIX)
    target_link_libraries(CairoGraphicsPlugin PRIVATE "-lpangocairo-1.0 -lcairo -lgdk_pixbuf-2.0 -lgtk-4")
else()
    target_link_libraries(CairoGraphicsPlugin PRIVATE "-LD:/msys64/ucrt64/bin -lpango-1.0-0 -lpangocairo-1.0-0 -lglib-2.0-0 -lcairo-2 -lgobject-2.0-0 -lgdk_pixbuf-2.0-0 -lgtk-4-1")
endif()


#
# TreeSitterPlugin
#

message(STATUS "Adding plugin: TreeSitterPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/osx
    )
    
    file(GLOB TreeSitterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/unix
    )
    
    file(GLOB TreeSitterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/unix/*.c   
    )    
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/include/win
        D:/msys64/ucrt64/include/
    )
    
    file(GLOB TreeSitterPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/TreeSitterPlugin/src/win/*.c   
    )    
endif()

addLibraryWithRPATH(TreeSitterPlugin ${TreeSitterPlugin_SOURCES})

if(OSX)
	target_link_libraries(TreeSitterPlugin PRIVATE "-L/usr/local/lib -ltree-sitter -ltree-sitter-c -ltree-sitter-json -ltree-sitter-javascript -ltree-sitter-python")
elseif(UNIX)
    target_link_libraries(TreeSitterPlugin PRIVATE "-ltree-sitter -ltree-sitter-c -ltree-sitter-json -ltree-sitter-javascript -ltree-sitter-python")
else()
    target_link_libraries(TreeSitterPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/../current-dependencies/lib -ltree-sitter -ltree-sitter-c -ltree-sitter-json -ltree-sitter-javascript -ltree-sitter-python")
endif()


#
# WolframPlugin
#

message(STATUS "Adding plugin: WolframPlugin")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/osx
    )
    
    file(GLOB WolframPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/osx/*.c   
    )
elseif(UNIX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/unix
    )
    
    file(GLOB WolframPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/unix/*.c   
    )
else()
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/include/win
        D:/msys64/ucrt64/include/
    )
    
    file(GLOB WolframPlugin_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/src/win/*.c   
    )
endif()

addLibraryWithRPATH(WolframPlugin ${WolframPlugin_SOURCES})

if(OSX)
    target_link_libraries(WolframPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/lib -lWSTPi4 -lstdc++")
elseif(UNIX)
    target_link_libraries(WolframPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/lib -lWSTP64i4")
else()
    target_link_libraries(WolframPlugin PRIVATE "-L${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/WolframPlugin/lib -lwstp64i4")
endif()


#
# SqueakSSL
#

message(STATUS "Adding plugin: SqueakSSL")

if(OSX)
    include_directories(
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/common
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/osx
    )

    file(GLOB SqueakSSL_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/common/*.c
        ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/osx/*.c
    )
else()
    if(WIN)
        include_directories(
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/common
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/win
        )

        file(GLOB SqueakSSL_SOURCES
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/common/*.c
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/win/*.c
        )
    else()
        include_directories(
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/common
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/include/unix
        )

        file(GLOB SqueakSSL_SOURCES
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/common/*.c
            ${CMAKE_CURRENT_SOURCE_DIR}/extracted/plugins/SqueakSSL/src/unix/*.c
        )
    endif()
endif()

addLibraryWithRPATH(SqueakSSL ${SqueakSSL_SOURCES})

if(OSX)
    target_link_libraries(SqueakSSL PRIVATE "-framework CoreFoundation")
    target_link_libraries(SqueakSSL PRIVATE "-framework Security")
elseif(WIN)
    target_link_libraries(SqueakSSL PRIVATE Crypt32 Secur32)
else()
    find_package(OpenSSL REQUIRED)
    target_link_libraries(SqueakSSL PRIVATE OpenSSL::SSL OpenSSL::Crypto)
endif()


#
# DSAPrims
#
add_vm_plugin(DSAPrims)

#
# UnixOSProcessPlugin
#

if(NOT WIN)
    add_vm_plugin(UnixOSProcessPlugin)
    target_link_libraries(UnixOSProcessPlugin PRIVATE FilePlugin)
endif()
