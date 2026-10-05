# The registry of the libraries of the FFI (Emscripten)
#
# There is no dlopen: a library that the image calls through the FFI is
# linked into the VM, and listed in the registry of
# include/pharovm/emscripten/ffiRegistry.h, where src/externalPrimitives.c
# looks its symbols up.  cmake/emscripten/ffiLibraries.cmake, which includes
# this file, lists the libraries.
#
# This support file defines the following functions
#
#     pharo_wasm_ffi_library(<name> [ALIASES <name>...]
#                            [HEADERS <header>...] [SOURCES <file.c>...]
#                            [ARCHIVES <lib.a>...] [EXCLUDE <regex>]
#                            [SYMBOLS_FILE <file>] [NODE_ONLY]
#                            [LINK <target or library>...] [CFLAGS <flag>...]
#                            [FILES <file name>...] [ON_LOAD <function>])
#         Adds the library <name>, the name the image gives reduced as
#         src/externalPrimitives.c does (libc.so.6 is c, libSDL2-2.0.so.0 is
#         SDL2), or one of ALIASES.  Its table, ${CMAKE_BINARY_DIR}/wasm/ffi/
#         ffi_<name>.c, is generated at build time by genFFILibrary.cmake,
#         which prints 'FFI library <name>: <n> symbols':
#
#         - the names of SYMBOLS_FILE that it does not exempt: every one must
#           be declared by HEADERS, or the build stops (and the link stops
#           when the library does not define it);
#         - with ARCHIVES, also every global symbol they define that EXCLUDE
#           does not match and HEADERS declare (the others are left out);
#         - with SOURCES, every global symbol of their objects.
#
#         The table includes HEADERS, the library's public headers, so that
#         each address is taken with the real prototype; a library without
#         headers that declare its functions (the FFI test library) gives its
#         SOURCES instead, which the table then includes and compiles.  LINK
#         is linked into the VMs with the table: the library's targets
#         (defined before), whose include directories and compile
#         definitions reach the table, or its archives.  CFLAGS go to the
#         compilation of the table (and of the SOURCES, and of the probe of
#         the headers), a source property: call pharo_wasm_ffi_library() in
#         the directory of pharo_wasm_ffi_registry().  FILES are the file
#         names the image looks for before it loads the library (the global
#         property PHARO_WASM_FFI_FILES collects them, for the staging).
#         ON_LOAD names a C function, void <function>(void), defined in the
#         VM, which src/externalPrimitives.c calls the first time the library
#         is loaded.  A NODE_ONLY library is in the node VM pharo only.
#
#         SYMBOLS_FILE, cmake/emscripten/ffi/symbols/<name>.txt, holds a
#         name per line; '#' starts a comment line.  A line
#
#             # unavailable: <reason>
#             # unavailable unless <VARIABLE>: <reason>
#
#         starts a section, up to the next such line, of names that the table
#         leaves out (always, or when the CMake variable <VARIABLE> is off).
#         The image binds them, but the library has none of them here (_popen
#         is the Windows name of popen): lane 59 does not report them.  The
#         names before the first section, and those of an 'unless' section
#         whose variable is on, are checked.
#
#     pharo_wasm_ffi_registry()
#         Called once, after the libraries: writes the two registries, the
#         pharoFFILibraries of ffiRegistry-node.c (every library) and of
#         ffiRegistry-web.c (all but the NODE_ONLY ones), and makes the
#         static libraries
#
#             pharo_ffi_tables         - the objects of every table
#             pharo_ffi_registry_node  - the registry of the node VM, pharo
#             pharo_ffi_registry_web   - the registry of the web VM, pharo-web
#
#         Each registry links pharo_ffi_tables, and the link takes from it
#         only the tables its registry names: a NODE_ONLY library is not in
#         pharo-web.wasm.
#
# Global properties: PHARO_WASM_FFI_LIBRARIES (the names),
# PHARO_WASM_FFI_FILES (see FILES).

set(PHARO_WASM_FFI_LIBRARY_GENERATOR "${CMAKE_CURRENT_LIST_DIR}/genFFILibrary.cmake")
# Where ffiRegistry.h is, as pharovm/emscripten/ffiRegistry.h
get_filename_component(PHARO_WASM_FFI_INCLUDE_DIR "${CMAKE_CURRENT_LIST_DIR}/../../include" ABSOLUTE)

function(pharo_wasm_ffi_library name)
    cmake_parse_arguments(ARG "NODE_ONLY" "SYMBOLS_FILE;EXCLUDE;ON_LOAD"
        "ALIASES;HEADERS;SOURCES;ARCHIVES;LINK;CFLAGS;FILES" ${ARGN})
    if(ARG_UNPARSED_ARGUMENTS)
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): unexpected arguments ${ARG_UNPARSED_ARGUMENTS}")
    endif()
    if(NOT name MATCHES "^[A-Za-z_][A-Za-z0-9_]*$")
        message(FATAL_ERROR "pharo_wasm_ffi_library: '${name}' is not a C identifier")
    endif()
    get_property(names GLOBAL PROPERTY PHARO_WASM_FFI_LIBRARIES)
    if(name IN_LIST names)
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): the library is already in the registry")
    endif()
    if(NOT ARG_HEADERS AND NOT ARG_SOURCES)
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): no HEADERS nor SOURCES, which declare its symbols")
    endif()
    if(ARG_HEADERS AND ARG_SOURCES)
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): HEADERS and SOURCES both: the table includes either")
    endif()
    if(ARG_ON_LOAD AND NOT ARG_ON_LOAD MATCHES "^[A-Za-z_][A-Za-z0-9_]*$")
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): ON_LOAD '${ARG_ON_LOAD}' is not a C identifier")
    endif()
    set(symbolsFile "")
    set(enabled "")
    if(ARG_SYMBOLS_FILE)
        get_filename_component(symbolsFile "${ARG_SYMBOLS_FILE}" ABSOLUTE)
        if(NOT EXISTS "${symbolsFile}")
            message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): no SYMBOLS_FILE ${symbolsFile}")
        endif()
        # The variables of its 'unless' sections are read here, at configure
        # time: an edit of the file configures again
        set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${symbolsFile}")
        file(READ "${symbolsFile}" text)
        string(REGEX MATCHALL "(^|\n)#[ \t]*unavailable[ \t]+unless[ \t]+[A-Za-z_][A-Za-z0-9_]*" sections "${text}")
        foreach(section IN LISTS sections)
            string(REGEX REPLACE ".*unless[ \t]+" "" variable "${section}")
            if(${variable})
                list(APPEND enabled ${variable})
            endif()
        endforeach()
        list(REMOVE_DUPLICATES enabled)
    elseif(NOT ARG_ARCHIVES AND NOT ARG_SOURCES)
        message(FATAL_ERROR "pharo_wasm_ffi_library(${name}): no SYMBOLS_FILE, ARCHIVES nor SOURCES: no symbols")
    endif()
    set(sources "")
    foreach(source IN LISTS ARG_SOURCES)
        get_filename_component(source "${source}" ABSOLUTE)
        list(APPEND sources "${source}")
    endforeach()

    # The probe of the headers, and the objects of SOURCES, are compiled as
    # the table is: with this directory's CMAKE_C_FLAGS (-m64, setjmp), the
    # include directories and definitions of the LINK targets, and CFLAGS
    separate_arguments(cflags UNIX_COMMAND "${CMAKE_C_FLAGS}")
    list(APPEND cflags "-I${PHARO_WASM_FFI_INCLUDE_DIR}")
    set(depends "")
    foreach(link IN LISTS ARG_LINK)
        if(TARGET ${link})
            set(dirs "$<TARGET_PROPERTY:${link},INTERFACE_INCLUDE_DIRECTORIES>")
            set(defs "$<TARGET_PROPERTY:${link},INTERFACE_COMPILE_DEFINITIONS>")
            list(APPEND cflags "$<$<BOOL:${dirs}>:-I$<JOIN:${dirs},|-I>>")
            list(APPEND cflags "$<$<BOOL:${defs}>:-D$<JOIN:${defs},|-D>>")
            list(APPEND depends ${link})
        elseif(EXISTS "${link}")
            list(APPEND depends "${link}")
        endif()
    endforeach()
    list(APPEND cflags ${ARG_CFLAGS})
    # Lists go to the script with '|' between their items
    string(REPLACE ";" "|" cflags "${cflags}")
    foreach(list ALIASES HEADERS ARCHIVES)
        string(REPLACE ";" "|" arg_${list} "${ARG_${list}}")
    endforeach()
    string(REPLACE ";" "|" arg_SOURCES "${sources}")
    string(REPLACE ";" "|" arg_ENABLED "${enabled}")

    set(output "${CMAKE_BINARY_DIR}/wasm/ffi/ffi_${name}.c")
    add_custom_command(
        OUTPUT "${output}"
        COMMAND ${CMAKE_COMMAND} "-DNAME=${name}" "-DOUTPUT=${output}"
                "-DCC=${CMAKE_C_COMPILER}" "-DNM=${CMAKE_NM}" "-DCFLAGS=${cflags}"
                "-DWORK=${CMAKE_BINARY_DIR}/wasm/ffi/work-${name}"
                "-DALIASES=${arg_ALIASES}" "-DHEADERS=${arg_HEADERS}"
                "-DSOURCES=${arg_SOURCES}" "-DARCHIVES=${arg_ARCHIVES}"
                "-DEXCLUDE=${ARG_EXCLUDE}" "-DSYMBOLS_FILE=${symbolsFile}"
                "-DENABLED=${arg_ENABLED}" "-DON_LOAD=${ARG_ON_LOAD}"
                -P "${PHARO_WASM_FFI_LIBRARY_GENERATOR}"
        COMMAND ${CMAKE_COMMAND} -E touch_nocreate "${output}"
        DEPENDS ${symbolsFile} ${sources} ${ARG_ARCHIVES} ${depends} "${PHARO_WASM_FFI_LIBRARY_GENERATOR}"
        COMMENT "Generating the FFI symbol table of ${name}"
        VERBATIM)
    if(ARG_CFLAGS)
        set_source_files_properties("${output}" PROPERTIES COMPILE_OPTIONS "${ARG_CFLAGS}")
    endif()

    set_property(GLOBAL APPEND PROPERTY PHARO_WASM_FFI_LIBRARIES ${name})
    set_property(GLOBAL PROPERTY PHARO_WASM_FFI_TABLE_${name} "${output}")
    set_property(GLOBAL PROPERTY PHARO_WASM_FFI_NODE_ONLY_${name} ${ARG_NODE_ONLY})
    set_property(GLOBAL APPEND PROPERTY PHARO_WASM_FFI_LINK ${ARG_LINK})
    set_property(GLOBAL APPEND PROPERTY PHARO_WASM_FFI_FILES ${ARG_FILES})
endfunction()

# Writes <file>, the registry of the libraries <names>, when its text changes
function(pharo_wasm_write_ffi_registry file names)
    set(declarations "")
    set(rows "")
    foreach(name IN LISTS names)
        string(APPEND declarations "extern const PharoFFILibrary pharoFFILibrary_${name};\n")
        string(APPEND rows "\t&pharoFFILibrary_${name},\n")
    endforeach()
    set(text "/* Generated by cmake/emscripten/ffiRegistry.cmake.  Do not edit. */
#include \"pharovm/emscripten/ffiRegistry.h\"

${declarations}
const PharoFFILibrary *const pharoFFILibraries[] = {
${rows}\tNULL
};
")
    set(old "")
    if(EXISTS "${file}")
        file(READ "${file}" old)
    endif()
    if(NOT old STREQUAL text)
        file(WRITE "${file}" "${text}")
    endif()
endfunction()

function(pharo_wasm_ffi_registry)
    if(TARGET pharo_ffi_tables)
        message(FATAL_ERROR "pharo_wasm_ffi_registry: called twice")
    endif()
    get_property(names GLOBAL PROPERTY PHARO_WASM_FFI_LIBRARIES)
    get_property(link GLOBAL PROPERTY PHARO_WASM_FFI_LINK)
    set(tables "")
    set(webNames "")
    set(listed "")
    foreach(name IN LISTS names)
        get_property(table GLOBAL PROPERTY PHARO_WASM_FFI_TABLE_${name})
        get_property(nodeOnly GLOBAL PROPERTY PHARO_WASM_FFI_NODE_ONLY_${name})
        list(APPEND tables "${table}")
        if(nodeOnly)
            list(APPEND listed "${name} (node only)")
        else()
            list(APPEND webNames ${name})
            list(APPEND listed "${name}")
        endif()
    endforeach()
    set(dir "${CMAKE_BINARY_DIR}/wasm/ffi")
    pharo_wasm_write_ffi_registry("${dir}/ffiRegistry-node.c" "${names}")
    pharo_wasm_write_ffi_registry("${dir}/ffiRegistry-web.c" "${webNames}")

    # The tables are generated code: their headers' warnings are not ours
    add_library(pharo_ffi_tables STATIC ${tables})
    target_include_directories(pharo_ffi_tables PRIVATE ${PHARO_WASM_FFI_INCLUDE_DIR})
    target_compile_options(pharo_ffi_tables PRIVATE -w)
    target_link_libraries(pharo_ffi_tables PUBLIC ${link})
    foreach(vm node web)
        add_library(pharo_ffi_registry_${vm} STATIC "${dir}/ffiRegistry-${vm}.c")
        target_include_directories(pharo_ffi_registry_${vm} PRIVATE ${PHARO_WASM_FFI_INCLUDE_DIR})
        target_link_libraries(pharo_ffi_registry_${vm} PUBLIC pharo_ffi_tables)
    endforeach()
    string(REPLACE ";" ", " listed "${listed}")
    message(STATUS "FFI libraries: ${listed}")
endfunction()
