# pharo_third_party_notice(<name> <source> <file name> <out var> [START <text>])
#
# Extracts a licence notice that a library carries inside one of its source
# files, rather than in a file of its own: the /* ... */ comment of <source>
# from its first byte (or from the first occurrence of <text>) to the end of
# the first following line "*/".  simdutf and simdjson both carry so the
# BSD-3-Clause notice of PyTorch, whose CPU detection they copied
# (isadetection.h), and that licence asks for the notice in binary
# distributions.
#
# The notice is written verbatim to ${CMAKE_BINARY_DIR}/notices/<file name>,
# never under ${CMAKE_BINARY_DIR}/deps/<name>, which pharo_wasm_dep_fetch
# owns: it takes the one entry of that directory as the source tree
# (cmake/emscripten/deps/fetch.cmake), and a second entry would make the next
# configure lose it.  The file is written through <file name>.tmp, so that
# an unchanged notice keeps its time stamp.
#
# Configure stops when there is no such comment, or when it does not hold
# "Redistributions in binary form must reproduce" (a version of the library
# that moved or dropped the notice).  The text is read with file(READ), not
# file(STRINGS), whose answer is a list: the ';' of the notice must survive.
#
# On Emscripten, "|<file>" is appended to the item of the global property
# PHARO_WASM_NOTICES whose first field is <name> (which the library's
# pharo_wasm_dep_fetch, or its SOURCE_DIR branch, recorded before), so that
# cmake/emscripten/stage.cmake writes the notice into THIRD-PARTY-NOTICES.txt
# with the library's licence files.
#
# <out var> is set, in the caller's scope, to the path of the notice.

include_guard(GLOBAL)

function(pharo_third_party_notice name source fileName outVar)
    cmake_parse_arguments(PARSE_ARGV 4 N "" "START" "")
    if(N_UNPARSED_ARGUMENTS)
        message(FATAL_ERROR "pharo_third_party_notice(${name}): unexpected arguments ${N_UNPARSED_ARGUMENTS}")
    endif()
    if(NOT EXISTS "${source}")
        message(FATAL_ERROR "pharo_third_party_notice(${name}): ${source} does not exist")
    endif()
    file(READ "${source}" text)
    if(DEFINED N_START)
        string(FIND "${text}" "${N_START}" at)
        if(at EQUAL -1)
            message(FATAL_ERROR "pharo_third_party_notice(${name}): ${source} has no notice that begins with '${N_START}'")
        endif()
        string(SUBSTRING "${text}" ${at} -1 text)
    endif()
    string(FIND "${text}" "\n*/" end)
    if(end EQUAL -1)
        message(FATAL_ERROR "pharo_third_party_notice(${name}): the notice of ${source} has no closing line '*/'")
    endif()
    math(EXPR end "${end} + 3")
    string(SUBSTRING "${text}" 0 ${end} notice)
    string(APPEND notice "\n")
    string(FIND "${notice}" "Redistributions in binary form must reproduce" at)
    if(at EQUAL -1)
        message(FATAL_ERROR "pharo_third_party_notice(${name}): the comment taken from ${source} is not the "
            "licence notice it should be (no 'Redistributions in binary form must reproduce'): "
            "did a new version of ${name} move it?")
    endif()

    set(out "${CMAKE_BINARY_DIR}/notices/${fileName}")
    # (the .tmp stays: configure_file makes its input a dependency of the
    # configure, and a missing one would configure again at every build)
    file(WRITE "${out}.tmp" "${notice}")
    configure_file("${out}.tmp" "${out}" COPYONLY)
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${source}")

    if(EMSCRIPTEN)
        get_property(items GLOBAL PROPERTY PHARO_WASM_NOTICES)
        set(updated "")
        set(found FALSE)
        foreach(item IN LISTS items)
            string(FIND "${item}" "${name}|" at)
            if(at EQUAL 0)
                string(APPEND item "|${out}")
                set(found TRUE)
            endif()
            list(APPEND updated "${item}")
        endforeach()
        if(NOT found)
            message(FATAL_ERROR "pharo_third_party_notice(${name}): PHARO_WASM_NOTICES has no item for ${name}")
        endif()
        set_property(GLOBAL PROPERTY PHARO_WASM_NOTICES "${updated}")
    endif()
    set(${outVar} "${out}" PARENT_SCOPE)
endfunction()
