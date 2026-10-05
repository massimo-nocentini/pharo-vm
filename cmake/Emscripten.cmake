# Emscripten (WebAssembly)
#
# Configure with the initial cache of this platform, which selects the
# StackVM, 64-bit pointers and no threads:
#
#     emcmake cmake -C cmake/Emscripten.cache.cmake -S . -B <dir>
#           -DGENERATED_SOURCE_DIR=<dir holding generated/64>
#
# `make wasm' (GNUmakefile) generates the sources and does this in
# build-wasm/cmake; see docs/WebAssembly.md.  The sources must be generated
# from this tree's smalltalksrc, which has the Emscripten memory map and the
# heartbeat poll of the synchronous heartbeat.
#
# The VM runs in slices on the thread of its host, node or a Web Worker of a
# browser, and returns to it between slices (src/emscripten/emscriptenMain.c).
# There is no dlopen: the VM core and every plugin are static libraries, the
# plugins built in and listed in pharoBuiltinPlugins.h, and so are the
# libraries the FFI calls (cmake/emscripten/deps), which it finds in a
# registry (cmake/emscripten/ffiLibraries.cmake).  WebAssembly checks
# the type of every indirect call, so the named primitives are called through
# trampolines (cmake/emscripten/primitiveTables.cmake).  Two executables are
# linked from the same objects:
#
#     pharo      - build/vm/pharo.js and pharo.wasm: the command line VM for
#                  node, on the host file system (memory64)
#     pharo-web  - build/vm/pharo-web.js and pharo-web.wasm: the factory
#                  createPharoVM, for a Web Worker (and node), with a 32-bit
#                  memory by default (WASM_WEB_MEMORY64)
#
# and staged in WASM_STAGE_DIR: node/, and the static site web/
# (cmake/emscripten/stage.cmake).

set(WASM_SJLJ "wasm" CACHE STRING "setjmp/longjmp of the VM: wasm (WebAssembly exception handling), wasm-legacy (its legacy instructions) or emscripten (JavaScript)")
set(WASM_WEB_MEMORY64 "2" CACHE STRING "Memory of pharo-web: 2 (32-bit memory, 64-bit pointers) or 1 (memory64)")
set(WASM_STACK_SIZE "8MB" CACHE STRING "Size of the C stack")
set(WASM_INITIAL_MEMORY "32MB" CACHE STRING "Initial size of the memory; it must hold the stack and the data")
set(WASM_MAXIMUM_MEMORY "4GB" CACHE STRING "Maximum size of the memory")
set(WASM_OLD_SPACE_BASE "0x20000000" CACHE STRING "Address of old space, a power of two above new space (PHARO_WASM_OLD_SPACE_BASE overrides it)")
set(WASM_SLICE_MS "20" CACHE STRING "Length of a slice of the VM in milliseconds (PHARO_WASM_SLICE_MS overrides it)")
option(WASM_WORLD "Prepare the image of the Pharo world for web/ (needs WASM_HOST_PHARO)" ON)
set(WASM_IMAGE_ZIP "" CACHE FILEPATH "The Pharo 12 image zip, instead of downloading it")
set(WASM_HOST_PHARO "" CACHE FILEPATH "A native Pharo VM, which prepares the image of the world")
set(WASM_STAGE_DIR "${CMAKE_CURRENT_BINARY_DIR}" CACHE PATH "Where node/ and web/ are staged")

if(NOT NODE_JS_EXECUTABLE)
    find_program(NODE_JS_EXECUTABLE NAMES node nodejs)
endif()
if(NOT NODE_JS_EXECUTABLE)
    message(FATAL_ERROR "node is needed to stage web/: set NODE_JS_EXECUTABLE")
endif()

# What the initial cache sets, and what comes of it
if(NOT FLAVOUR STREQUAL "StackVM" OR NOT SIZEOF_VOID_P EQUAL 8 OR FEATURE_THREADED_FFI OR PHARO_VM_IN_WORKER_THREAD)
    message(FATAL_ERROR "The Emscripten VM is a 64-bit StackVM without threads (and without the threaded FFI): "
        "configure it with -C ${CMAKE_CURRENT_SOURCE_DIR}/cmake/Emscripten.cache.cmake")
endif()
if(NOT WASM_WEB_MEMORY64 MATCHES "^[12]$")
    message(FATAL_ERROR "WASM_WEB_MEMORY64 must be 1 or 2, not '${WASM_WEB_MEMORY64}'")
endif()

# setjmp/longjmp, the same for the objects and the links: wasm uses the
# exception handling of WebAssembly (try_table and exnref), wasm-legacy its
# legacy instructions (try, delegate and rethrow, which Firefox reports as
# deprecated, for engines without exnref) and emscripten calls out to
# JavaScript.  The flags go in this directory's CMAKE_C_FLAGS, which reach the
# links too, so that a later `cmake -DWASM_SJLJ=<mode> <dir>' changes both.
if(WASM_SJLJ STREQUAL "wasm")
    set(PHARO_WASM_SJLJ_FLAGS -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0)
elseif(WASM_SJLJ STREQUAL "wasm-legacy")
    set(PHARO_WASM_SJLJ_FLAGS -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=1)
elseif(WASM_SJLJ STREQUAL "emscripten")
    set(PHARO_WASM_SJLJ_FLAGS -sSUPPORT_LONGJMP=emscripten)
else()
    message(FATAL_ERROR "WASM_SJLJ must be wasm, wasm-legacy or emscripten, not '${WASM_SJLJ}'")
endif()
string(REPLACE ";" " " PHARO_WASM_SJLJ_C_FLAGS "${PHARO_WASM_SJLJ_FLAGS}")
string(APPEND CMAKE_C_FLAGS " ${PHARO_WASM_SJLJ_C_FLAGS}")

# CMakeLists.txt adds -g for every build type: keep the DWARF out of the
# release objects, and so out of the modules.
if(NOT CMAKE_BUILD_TYPE MATCHES "Debug")
    string(REGEX REPLACE "(^| )-g( |$)" " " CMAKE_C_FLAGS "${CMAKE_C_FLAGS}")
    string(REGEX REPLACE "(^| )-g( |$)" " " CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS}")
endif()

#
# The libraries: their settings (WASM_FFI, WASM_DEPS_DIR...) and what comes
# of them (PHARO_WASM_HAS_FFI...), then their directory, which builds them
# from their pinned archives, with flags of its own (libffi for
# cmake/importLibFFI.cmake)
#
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps/options.cmake)
add_subdirectory(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/deps)

function(add_platform_headers)
target_include_directories(${VM_LIBRARY_NAME}
PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/emscripten
    ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/unix
    ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/common
)
endfunction() #add_platform_headers

set(EXTRACTED_SOURCES
#Platform sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/aio.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/debugUnix.c

#Virtual Memory functions
    ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/memoryEmscripten.c

# Support sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/fileDialogUnix.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/emscriptenSupport.c
)

#
# The FFI: the same-thread runner of src/ffi (CMakeLists.txt adds its
# sources with FEATURE_FFI), on upstream libffi
#
# The support primitives of src/ffi have no module, and natively are found
# by a global dlsym: genSupportTable.cmake lists them in vmsupport_exports.
# The libraries of the FFI are linked in, and src/externalPrimitives.c looks
# their symbols up in their registry (include/pharovm/emscripten/
# ffiRegistry.h, made by cmake/emscripten/ffiRegistry.cmake from
# ffiLibraries.cmake).  src/emscripten/ffiAdapt.c adapts the callouts whose
# declaration differs from the function in width, by the signatures of the
# registry.
if(FEATURE_FFI)
    set(PHARO_WASM_FFI_PRIMITIVE_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/functionDefinitionPrimitives.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/primitiveCalls.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/primitiveUtils.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/typesPrimitives.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/sameThread/sameThread.c
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/callbacks/callbackPrimitives.c)
    set(PHARO_WASM_SUPPORT_TABLE "${CMAKE_BINARY_DIR}/wasm/prims/vmsupport_exports.c")
    add_custom_command(
        OUTPUT "${PHARO_WASM_SUPPORT_TABLE}"
        COMMAND ${CMAKE_COMMAND} "-DOUTPUT=${PHARO_WASM_SUPPORT_TABLE}"
                "-DSOURCES=${PHARO_WASM_FFI_PRIMITIVE_SOURCES}"
                -P "${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/genSupportTable.cmake"
        COMMAND ${CMAKE_COMMAND} -E touch_nocreate "${PHARO_WASM_SUPPORT_TABLE}"
        DEPENDS ${PHARO_WASM_FFI_PRIMITIVE_SOURCES} "${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/genSupportTable.cmake"
        COMMENT "Generating the table of the FFI support primitives"
        VERBATIM)
    list(APPEND EXTRACTED_SOURCES "${PHARO_WASM_SUPPORT_TABLE}"
        ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/ffiAdapt.c)
endif()

set(VM_FRONTEND_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/emscriptenMain.c)

#
# The interpreter, and its primitive trampolines
#
include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/primitiveTables.cmake)

list(LENGTH VMSOURCEFILES PHARO_WASM_VM_SOURCE_COUNT)
if(NOT PHARO_WASM_VM_SOURCE_COUNT EQUAL 1 OR NOT EXISTS "${VMSOURCEFILES}")
    message(FATAL_ERROR "No generated StackVM in ${PHARO_CURRENT_GENERATED} (${VMSOURCEFILES}): "
        "set GENERATED_SOURCE_DIR to the directory holding generated/64")
endif()
pharo_wasm_check_markers(${VMSOURCEFILES})
pharo_wasm_primitive_table(PHARO_WASM_VM_PRIMITIVES SOURCE ${VMSOURCEFILES} TABLE vm_exports MARKERS)
set(VMSOURCEFILES ${PHARO_WASM_VM_PRIMITIVES})
# The callouts of the interpreter go through emscriptenFFICall()
# (emscriptenSupport.c), which turns what ffi_call throws in JavaScript (a
# declaration that does not match the function) into a primitive failure.
if(FEATURE_FFI)
    set_property(SOURCE ${PHARO_WASM_VM_PRIMITIVES} APPEND PROPERTY COMPILE_DEFINITIONS "ffi_call=emscriptenFFICall")
endif()

#
# Libraries and builtin plugins
#
# Every plugin of cmake/plugins.cmake (which leaves out UnixOSProcessPlugin
# and SurfacePlugin here) is compiled as builtin, and its <NAME>.c is
# replaced by the translation unit that includes it and adds its trampolines.
function(pharo_wasm_add_builtin_plugin NAME)
    set(sources ${ARGN})
    set(main "")
    foreach(source IN LISTS sources)
        get_filename_component(base "${source}" NAME_WE)
        if(base STREQUAL NAME)
            set(main "${source}")
        endif()
    endforeach()
    if(NOT main)
        message(FATAL_ERROR "The sources of the plugin ${NAME} have no ${NAME}.c, whose exports table would be built in")
    endif()
    pharo_wasm_primitive_table(wrapper SOURCE "${main}" TABLE ${NAME}_exports)
    list(REMOVE_ITEM sources "${main}")

    add_library(${NAME} STATIC ${wrapper} ${sources})
    target_compile_definitions(${NAME} PRIVATE SQUEAK_BUILTIN_PLUGIN)
    target_link_libraries(${NAME} PRIVATE ${VM_LIBRARY_NAME})
    set_property(GLOBAL APPEND PROPERTY PHARO_WASM_BUILTIN_PLUGINS ${NAME})
endfunction()

# Include a library in the project, linking it to the main library
macro(addLibraryWithRPATH NAME)
    if("${NAME}" STREQUAL "${VM_LIBRARY_NAME}")
        add_library(${NAME} STATIC ${ARGN})
    else()
        pharo_wasm_add_builtin_plugin(${NAME} ${ARGN})
    endif()
endmacro()

# Include a loose-dependency library in the project, but do not link it to the main library
macro(addIndependentLibraryWithRPATH NAME)
    add_library(${NAME} STATIC ${ARGN})
endmacro()

#
# Links
#
# Function pointers start at 1024, above the quick primitive indices that
# share the primitive function slot (up to 519).  A 'function signature
# mismatch' of wasm-ld is an error: the call would trap.
set(PHARO_WASM_LINK_FLAGS
    ${PHARO_WASM_SJLJ_FLAGS}
    -sTABLE_BASE=1024
    -sINITIAL_MEMORY=${WASM_INITIAL_MEMORY}
    -sALLOW_MEMORY_GROWTH=1
    -sMAXIMUM_MEMORY=${WASM_MAXIMUM_MEMORY}
    -sSTACK_SIZE=${WASM_STACK_SIZE}
    -Wl,--stack-first
    -Wl,--fatal-warnings
    -sFORCE_FILESYSTEM=1)
# libffi's closures grow the table, and call _malloc and _free from
# JavaScript; its $stackSave and friends need the stack exports, which the
# links do not add by themselves with SUPPORT_LONGJMP=wasm.
if(FEATURE_FFI)
    list(APPEND PHARO_WASM_LINK_FLAGS
        -sALLOW_TABLE_GROWTH=1
        -sEXPORTED_FUNCTIONS=_main,_malloc,_free,_emscripten_stack_get_current,__emscripten_stack_restore,__emscripten_stack_alloc)
else()
    list(APPEND PHARO_WASM_LINK_FLAGS -sEXPORTED_FUNCTIONS=_main)
endif()
# Debug: assertions in the JavaScript and the system libraries, and a checked
# stack.  The stack cookies sit at the end of the stack, at the bottom of
# memory with --stack-first, and the glue writes one at address 0 too: a read
# through a null pointer finds them rather than zeros.
if(CMAKE_BUILD_TYPE MATCHES "Debug")
    list(INSERT PHARO_WASM_LINK_FLAGS 0 -O0 -g -sASSERTIONS=2 -sSTACK_OVERFLOW_CHECK=2)
else()
    list(INSERT PHARO_WASM_LINK_FLAGS 0 -O2 -g0)
endif()

# The node command line VM: memory64, the host file system and environment,
# and node-pump.js running the slices from node's event loop.  The name
# section (--profiling-funcs) names the frames of a trap.
set(PHARO_WASM_NODE_LINK_FLAGS
    -m64
    -sENVIRONMENT=node
    -sNODERAWFS=1
    -sNODE_HOST_ENV=1
    -sEXIT_RUNTIME=1
    --profiling-funcs
    --pre-js ${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/node/node-pre.js
    --post-js ${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/node/node-pump.js)
set(PHARO_WASM_NODE_LINK_DEPENDS
    ${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/node/node-pre.js
    ${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/node/node-pump.js)

# The web factory: its host (packaging/emscripten/web/vm-driver.js) gives the
# devices and runs the slices.  -sMEMORY64=2 wins over the -m64 of
# CMAKE_C_FLAGS, which CMake also passes to the link.
if(WASM_WEB_MEMORY64 STREQUAL "2")
    set(PHARO_WASM_WEB_LINK_FLAGS -sMEMORY64=2)
else()
    set(PHARO_WASM_WEB_LINK_FLAGS -m64)
endif()
list(APPEND PHARO_WASM_WEB_LINK_FLAGS
    -sENVIRONMENT=web,worker,node
    -sMODULARIZE=1
    -sEXPORT_NAME=createPharoVM
    -sEXIT_RUNTIME=0
    -sEXPORTED_RUNTIME_METHODS=FS,ENV,HEAPU8,HEAP32,HEAPU32,UTF8ToString,stringToUTF8,lengthBytesUTF8)

function(pharo_wasm_link_executable TARGET)
    target_compile_definitions(${TARGET} PRIVATE PHARO_WASM_SLICE_MS=${WASM_SLICE_MS})
    string(REPLACE ";" " " flags "${PHARO_WASM_LINK_FLAGS};${ARGN}")
    set_target_properties(${TARGET} PROPERTIES LINK_FLAGS "${flags}")
endfunction()

macro(add_required_libs_per_platform)
    target_link_libraries(${VM_LIBRARY_NAME} m)
    # sqNamedPrims.h takes the builtin tables from there
    target_compile_definitions(${VM_LIBRARY_NAME} PRIVATE [[PHARO_BUILTIN_PLUGINS_HEADER="pharoBuiltinPlugins.h"]])
    set_source_files_properties(${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/memoryEmscripten.c
        PROPERTIES COMPILE_DEFINITIONS PHARO_WASM_OLD_SPACE_BASE=${WASM_OLD_SPACE_BASE})
    pharo_wasm_link_executable(${VM_EXECUTABLE_NAME} ${PHARO_WASM_NODE_LINK_FLAGS})
    set_target_properties(${VM_EXECUTABLE_NAME} PROPERTIES LINK_DEPENDS "${PHARO_WASM_NODE_LINK_DEPENDS}")
endmacro()

# Called once every plugin of cmake/plugins.cmake is defined
macro(add_third_party_dependencies_per_platform)
    # Plugins written for this platform, in the table format of Slang.  The
    # directory changes when one comes or goes, which configures again.
    file(GLOB PHARO_WASM_PLATFORM_PLUGINS ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/plugins/*.c)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${CMAKE_CURRENT_SOURCE_DIR}/src/emscripten/plugins)
    foreach(PHARO_WASM_PLUGIN_SOURCE IN LISTS PHARO_WASM_PLATFORM_PLUGINS)
        get_filename_component(PHARO_WASM_PLUGIN "${PHARO_WASM_PLUGIN_SOURCE}" NAME_WE)
        addLibraryWithRPATH(${PHARO_WASM_PLUGIN} ${PHARO_WASM_PLUGIN_SOURCE})
    endforeach()

    get_property(PHARO_WASM_BUILTIN_PLUGINS GLOBAL PROPERTY PHARO_WASM_BUILTIN_PLUGINS)
    string(REPLACE ";" " " PHARO_WASM_PLUGIN_NAMES "${PHARO_WASM_BUILTIN_PLUGINS}")
    message(STATUS "Builtin plugins: ${PHARO_WASM_PLUGIN_NAMES}")
    pharo_wasm_write_builtin_header(${CMAKE_CURRENT_BINARY_DIR}/build/include/pharovm/pharoBuiltinPlugins.h
        TABLES ${PHARO_WASM_BUILTIN_PLUGINS})
    target_link_libraries(${VM_EXECUTABLE_NAME} ${PHARO_WASM_BUILTIN_PLUGINS})

    add_executable(pharo-web ${VM_FRONTEND_SOURCES})
    target_link_libraries(pharo-web ${VM_LIBRARY_NAME} ${PHARO_WASM_BUILTIN_PLUGINS})

    # The libraries of the FFI and their registry, one per VM: the node VM
    # also has those of the tests (NODE_ONLY)
    if(FEATURE_FFI)
        include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/ffiLibraries.cmake)
        target_link_libraries(${VM_EXECUTABLE_NAME} pharo_ffi_registry_node)
        target_link_libraries(pharo-web pharo_ffi_registry_web)
    endif()
    pharo_wasm_link_executable(pharo-web ${PHARO_WASM_WEB_LINK_FLAGS})

    # The stock image, the image of the world (needs WASM_HOST_PHARO), then
    # node/ and web/
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/download.cmake)
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/webimage.cmake)
    include(${CMAKE_CURRENT_SOURCE_DIR}/cmake/emscripten/stage.cmake)
endmacro()

macro(configure_installables INSTALL_COMPONENT)
    set(CMAKE_INSTALL_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/build/dist")
    install(
      DIRECTORY "${WASM_STAGE_DIR}/node" "${WASM_STAGE_DIR}/web"
      DESTINATION "."
      USE_SOURCE_PERMISSIONS
      COMPONENT ${INSTALL_COMPONENT})
endmacro()
