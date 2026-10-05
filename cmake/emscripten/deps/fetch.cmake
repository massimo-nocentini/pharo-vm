# The pinned archives of the libraries of the Emscripten VM
#
# cmake/emscripten/deps/CMakeLists.txt includes this file.  It defines
#
#     pharo_wasm_dep_fetch(<name> VERSION <version> URL <url> SHA256 <hash>
#                          FILE <file> [LICENSES <path>...])
#
# which finds the archive <file>, the pinned release <version> of the
# library <name>, and unpacks it at configure time into
# ${CMAKE_BINARY_DIR}/deps/<name>.  It sets <name>_SOURCE_DIR, in the scope
# of its caller, to the directory of the sources: the one directory the
# archive holds at its top, or deps/<name> when it holds several entries.
#
# The archive is looked for in WASM_DEPS_DIR (a directory of archives for an
# offline build), then in ${WASM_STAGE_DIR}/downloads (where an earlier
# configure downloaded it, which make wasm-clean keeps), and otherwise
# downloaded from <url> there, through <file>.part.  FILE is explicit since
# the names of the archives GitHub makes (v1.2.3.tar.gz) say nothing of the
# library.  Whatever its origin, the archive must have the SHA256 <hash>:
# configure stops when it has another, with a message that names the file,
# both hashes and WASM_DEPS_DIR.  A downloaded archive with another hash is
# downloaded again once (it may be the one of an earlier pin).
#
# The archive is unpacked again only when the one unpacked had another hash
# (deps/<name>.stamp records it), or deps/<name> is gone.
#
# LICENSES are the files of the licence, relative to <name>_SOURCE_DIR.
# Each must exist.  The library is recorded in the global property
# PHARO_WASM_NOTICES, one item per library,
#
#     <name>|<version>|<url>|<hash>|<licence>|<licence>...
#
# with the absolute paths of its licence files, from which
# cmake/emscripten/stage.cmake writes THIRD-PARTY-NOTICES.txt.

function(pharo_wasm_dep_fetch name)
    cmake_parse_arguments(ARG "" "VERSION;URL;SHA256;FILE" "LICENSES" ${ARGN})
    if(ARG_UNPARSED_ARGUMENTS)
        message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): unexpected arguments ${ARG_UNPARSED_ARGUMENTS}")
    endif()
    foreach(key VERSION URL SHA256 FILE)
        if(NOT ARG_${key})
            message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): no ${key}")
        endif()
    endforeach()
    string(TOLOWER "${ARG_SHA256}" sha256)
    if(NOT sha256 MATCHES "^[0-9a-f]+$")
        message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): '${ARG_SHA256}' is not a SHA256")
    endif()
    string(LENGTH "${sha256}" length)
    if(NOT length EQUAL 64)
        message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): '${ARG_SHA256}' is not a SHA256")
    endif()
    if(ARG_FILE MATCHES "/" OR ARG_FILE MATCHES "\\\\")
        message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): FILE (${ARG_FILE}) is a file name, not a path")
    endif()
    set(what "${name} ${ARG_VERSION} (${ARG_FILE})")
    if(WASM_STAGE_DIR)
        set(downloads "${WASM_STAGE_DIR}/downloads")
    else()
        set(downloads "${CMAKE_BINARY_DIR}/downloads")
    endif()

    # The archive: from WASM_DEPS_DIR, downloaded already, or downloaded now
    set(archive "")
    if(WASM_DEPS_DIR)
        get_filename_component(dir "${WASM_DEPS_DIR}" ABSOLUTE)
        if(EXISTS "${dir}/${ARG_FILE}")
            set(archive "${dir}/${ARG_FILE}")
            file(SHA256 "${archive}" hash)
            if(NOT hash STREQUAL sha256)
                message(FATAL_ERROR "${archive} is not ${what}: its SHA256 is ${hash}, "
                    "not ${sha256}; put the pinned archive in WASM_DEPS_DIR (${dir}), or remove it from there to download it")
            endif()
        endif()
    endif()
    if(NOT archive AND EXISTS "${downloads}/${ARG_FILE}")
        set(archive "${downloads}/${ARG_FILE}")
        file(SHA256 "${archive}" hash)
        if(NOT hash STREQUAL sha256)
            message(STATUS "${archive} is not ${what} (SHA256 ${hash}): downloading it again")
            file(REMOVE "${archive}")
            set(archive "")
        endif()
    endif()
    if(NOT archive)
        set(archive "${downloads}/${ARG_FILE}")
        if(WASM_DEPS_DIR)
            set(offline "WASM_DEPS_DIR (${WASM_DEPS_DIR}) has no ${ARG_FILE}")
        else()
            set(offline "to build offline, put ${ARG_FILE} in a directory and give it as WASM_DEPS_DIR")
        endif()
        file(MAKE_DIRECTORY "${downloads}")
        message(STATUS "Downloading ${ARG_URL}")
        # (checked below rather than with EXPECTED_HASH, whose error names
        # the .part file and not what to do)
        file(DOWNLOAD "${ARG_URL}" "${archive}.part" STATUS status TLS_VERIFY ON)
        list(GET status 0 code)
        if(NOT code EQUAL 0)
            list(GET status 1 reason)
            file(REMOVE "${archive}.part")
            message(FATAL_ERROR "Cannot download ${what} from ${ARG_URL}: ${reason}; ${offline}")
        endif()
        file(SHA256 "${archive}.part" hash)
        if(NOT hash STREQUAL sha256)
            file(REMOVE "${archive}.part")
            message(FATAL_ERROR "${ARG_URL}, downloaded as ${archive}, is not ${what}: its SHA256 is ${hash}, "
                "not ${sha256}; ${offline}")
        endif()
        file(RENAME "${archive}.part" "${archive}")
    endif()

    # Unpacked, unless the archive of that hash is already
    set(root "${CMAKE_BINARY_DIR}/deps")
    set(dest "${root}/${name}")
    set(stamp "${root}/${name}.stamp")
    set(unpacked "")
    if(EXISTS "${stamp}" AND IS_DIRECTORY "${dest}")
        file(READ "${stamp}" unpacked)
    endif()
    if(NOT unpacked STREQUAL "${sha256}\n")
        message(STATUS "Unpacking ${archive} into ${dest}")
        set(unpack "${dest}.unpack")
        file(REMOVE "${stamp}")
        file(REMOVE_RECURSE "${unpack}" "${dest}")
        file(MAKE_DIRECTORY "${unpack}")
        execute_process(COMMAND ${CMAKE_COMMAND} -E tar xf "${archive}"
            WORKING_DIRECTORY "${unpack}" RESULT_VARIABLE result)
        if(NOT result EQUAL 0)
            file(REMOVE_RECURSE "${unpack}")
            message(FATAL_ERROR "Cannot unpack ${archive} into ${unpack}")
        endif()
        file(RENAME "${unpack}" "${dest}")
        file(WRITE "${stamp}" "${sha256}\n")
    endif()
    file(GLOB entries LIST_DIRECTORIES true "${dest}/*")
    list(LENGTH entries count)
    if(count EQUAL 1 AND IS_DIRECTORY "${entries}")
        set(source "${entries}")
    else()
        set(source "${dest}")
    endif()

    set(notice "${name}|${ARG_VERSION}|${ARG_URL}|${sha256}")
    foreach(licence IN LISTS ARG_LICENSES)
        if(NOT EXISTS "${source}/${licence}")
            message(FATAL_ERROR "pharo_wasm_dep_fetch(${name}): ${archive} has no licence file ${licence}")
        endif()
        string(APPEND notice "|${source}/${licence}")
    endforeach()
    set_property(GLOBAL APPEND PROPERTY PHARO_WASM_NOTICES "${notice}")
    set(${name}_SOURCE_DIR "${source}" PARENT_SCOPE)
endfunction()
