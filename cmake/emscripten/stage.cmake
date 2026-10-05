# Staging of the Emscripten build
#
# Emscripten.cmake includes this file last, once the executables, the stock
# image (download.cmake) and the optional image of the world (webimage.cmake)
# are defined.  It defines the targets
#
#     wasm-node-stage  - ${WASM_STAGE_DIR}/node: pharo.js, pharo.wasm, the
#                        launcher pharo (packaging/emscripten/node/launch.sh.in)
#                        and THIRD-PARTY-NOTICES.txt
#     wasm-web-stage   - ${WASM_STAGE_DIR}/web, the static site, written by
#                        packaging/emscripten/tools/stage.mjs: the files of
#                        packaging/emscripten/web with @BUILD@ replaced by the
#                        build id, pharo-web.js and pharo-web.wasm,
#                        THIRD-PARTY-NOTICES.txt, the image (gzipped) in
#                        image/, the st files in st/ and manifest.json.  It
#                        always runs, and rewrites only what changed.
#
# The manifest says, besides, whether the VM has the FFI (FEATURE_FFI), which
# fonts the preparation of an image for the world sets up (freetype when the
# VM has FreeType, PHARO_WASM_HAS_FREETYPE of deps/options.cmake, bitmap
# otherwise: what webimage.cmake prepares, and web-bootstrap.st in the page),
# and the version of the package OSWindow-Web, which stage.mjs reads from
# packaging/emscripten/st/OSWindow-Web/OSWebDriver.class.st.
#
# web/ gets the image of the world when webimage.cmake defines its target
# wasm-web-image, and the stock image otherwise.
#
# THIRD-PARTY-NOTICES.txt is assembled here, at configure time, into
# ${CMAKE_CURRENT_BINARY_DIR}/wasm, and written only when its text changes.  It
# holds packaging/emscripten/THIRD-PARTY-NOTICES.head.txt, then each library
# that pharo_wasm_dep_fetch (cmake/emscripten/deps/fetch.cmake) fetched for the
# VM, from the global property PHARO_WASM_NOTICES: its version, archive and
# SHA256, and the text of its licence files.  It ends with the licences of the
# runtime that Emscripten links into every program: Emscripten's LICENSE,
# musl's COPYRIGHT and compiler-rt's LICENSE.TXT, from the emsdk.

set(PHARO_WASM_NODE_DIR "${WASM_STAGE_DIR}/node")
set(PHARO_WASM_WEB_DIR "${WASM_STAGE_DIR}/web")
set(PHARO_WASM_ST_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/st")
set(PHARO_WASM_NOTICES_FILE "${CMAKE_CURRENT_BINARY_DIR}/wasm/THIRD-PARTY-NOTICES.txt")

#
# THIRD-PARTY-NOTICES.txt
#
# pharo_wasm_notice_section(<variable> <title> <line>...): appends to
# <variable> a section header, the title and each line indented below it
function(pharo_wasm_notice_section VARIABLE TITLE)
    set(RULE "==============================================================================")
    set(TEXT "\n\n${RULE}\n${TITLE}\n")
    foreach(LINE IN LISTS ARGN)
        string(APPEND TEXT "  ${LINE}\n")
    endforeach()
    string(APPEND TEXT "${RULE}\n")
    set(${VARIABLE} "${${VARIABLE}}${TEXT}" PARENT_SCOPE)
endfunction()

# pharo_wasm_notice_file(<variable> <label> <file>): appends to <variable>
# the text of the licence file <file>, under the label <label>
function(pharo_wasm_notice_file VARIABLE LABEL FILE)
    if(NOT EXISTS "${FILE}")
        message(FATAL_ERROR "THIRD-PARTY-NOTICES.txt: ${FILE} (${LABEL}) does not exist")
    endif()
    file(READ "${FILE}" LICENCE)
    string(REGEX REPLACE "\n+$" "" LICENCE "${LICENCE}")
    set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${FILE}")
    set(${VARIABLE} "${${VARIABLE}}\n----- ${LABEL} -----\n\n${LICENCE}\n" PARENT_SCOPE)
endfunction()

set(PHARO_WASM_NOTICES_HEAD "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/THIRD-PARTY-NOTICES.head.txt")
file(READ "${PHARO_WASM_NOTICES_HEAD}" PHARO_WASM_NOTICES_TEXT)
string(REGEX REPLACE "\n+$" "" PHARO_WASM_NOTICES_TEXT "${PHARO_WASM_NOTICES_TEXT}")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${PHARO_WASM_NOTICES_HEAD}")

# The libraries: each item of PHARO_WASM_NOTICES is
# <name>|<version>|<url>|<sha256>|<licence>|<licence>..., with the absolute
# paths of the licence files
get_property(PHARO_WASM_NOTICE_ITEMS GLOBAL PROPERTY PHARO_WASM_NOTICES)
set(PHARO_WASM_NOTICE_NAMES "")
foreach(PHARO_WASM_NOTICE_ITEM IN LISTS PHARO_WASM_NOTICE_ITEMS)
    string(REPLACE "|" ";" PHARO_WASM_NOTICE_FIELDS "${PHARO_WASM_NOTICE_ITEM}")
    list(LENGTH PHARO_WASM_NOTICE_FIELDS PHARO_WASM_NOTICE_COUNT)
    if(PHARO_WASM_NOTICE_COUNT LESS 5)
        message(FATAL_ERROR "THIRD-PARTY-NOTICES.txt: '${PHARO_WASM_NOTICE_ITEM}' of PHARO_WASM_NOTICES "
            "is not <name>|<version>|<url>|<sha256>|<licence>...")
    endif()
    list(GET PHARO_WASM_NOTICE_FIELDS 0 PHARO_WASM_NOTICE_NAME)
    list(GET PHARO_WASM_NOTICE_FIELDS 1 PHARO_WASM_NOTICE_VERSION)
    list(GET PHARO_WASM_NOTICE_FIELDS 2 PHARO_WASM_NOTICE_URL)
    list(GET PHARO_WASM_NOTICE_FIELDS 3 PHARO_WASM_NOTICE_SHA256)
    list(REMOVE_AT PHARO_WASM_NOTICE_FIELDS 0 1 2 3)
    list(APPEND PHARO_WASM_NOTICE_NAMES "${PHARO_WASM_NOTICE_NAME}")
    pharo_wasm_notice_section(PHARO_WASM_NOTICES_TEXT
        "${PHARO_WASM_NOTICE_NAME} ${PHARO_WASM_NOTICE_VERSION}"
        "${PHARO_WASM_NOTICE_URL}"
        "SHA256 ${PHARO_WASM_NOTICE_SHA256}")
    foreach(PHARO_WASM_NOTICE_LICENCE IN LISTS PHARO_WASM_NOTICE_FIELDS)
        # (named in the archive: deps/<name>/libffi-3.8.0/LICENSE is libffi-3.8.0/LICENSE)
        file(RELATIVE_PATH PHARO_WASM_NOTICE_LABEL "${CMAKE_BINARY_DIR}/deps/${PHARO_WASM_NOTICE_NAME}"
            "${PHARO_WASM_NOTICE_LICENCE}")
        if(PHARO_WASM_NOTICE_LABEL MATCHES "^\\.\\.")
            get_filename_component(PHARO_WASM_NOTICE_LABEL "${PHARO_WASM_NOTICE_LICENCE}" NAME)
        endif()
        pharo_wasm_notice_file(PHARO_WASM_NOTICES_TEXT
            "${PHARO_WASM_NOTICE_NAME}: ${PHARO_WASM_NOTICE_LABEL}" "${PHARO_WASM_NOTICE_LICENCE}")
    endforeach()
endforeach()

# The runtime of Emscripten, from the emsdk of the toolchain
if(EMSCRIPTEN_ROOT_PATH)
    set(PHARO_WASM_EMSCRIPTEN_ROOT "${EMSCRIPTEN_ROOT_PATH}")
else()
    get_filename_component(PHARO_WASM_EMSCRIPTEN_ROOT "${CMAKE_C_COMPILER}" DIRECTORY)
endif()
pharo_wasm_notice_section(PHARO_WASM_NOTICES_TEXT
    "The runtime of Emscripten ${EMSCRIPTEN_VERSION}, https://emscripten.org/"
    "Its JavaScript support code is in pharo.js and pharo-web.js, and its C"
    "library, from musl (https://musl.libc.org/), and the compiler runtime"
    "compiler-rt of LLVM (https://compiler-rt.llvm.org/) are in pharo.wasm and"
    "pharo-web.wasm.")
pharo_wasm_notice_file(PHARO_WASM_NOTICES_TEXT "Emscripten: LICENSE"
    "${PHARO_WASM_EMSCRIPTEN_ROOT}/LICENSE")
pharo_wasm_notice_file(PHARO_WASM_NOTICES_TEXT "musl: COPYRIGHT"
    "${PHARO_WASM_EMSCRIPTEN_ROOT}/system/lib/libc/musl/COPYRIGHT")
pharo_wasm_notice_file(PHARO_WASM_NOTICES_TEXT "compiler-rt: LICENSE.TXT"
    "${PHARO_WASM_EMSCRIPTEN_ROOT}/system/lib/compiler-rt/LICENSE.TXT")

# Written only when it changes, so that node/ and web/ are staged again only
# then
set(PHARO_WASM_NOTICES_OLD "")
if(EXISTS "${PHARO_WASM_NOTICES_FILE}")
    file(READ "${PHARO_WASM_NOTICES_FILE}" PHARO_WASM_NOTICES_OLD)
endif()
if(NOT PHARO_WASM_NOTICES_OLD STREQUAL PHARO_WASM_NOTICES_TEXT)
    file(WRITE "${PHARO_WASM_NOTICES_FILE}" "${PHARO_WASM_NOTICES_TEXT}")
endif()
string(REPLACE ";" ", " PHARO_WASM_NOTICE_LIST "${PHARO_WASM_NOTICE_NAMES}")
if(NOT PHARO_WASM_NOTICE_LIST)
    set(PHARO_WASM_NOTICE_LIST "no library")
endif()
message(STATUS "THIRD-PARTY-NOTICES.txt: ${PHARO_WASM_NOTICE_LIST}, and the runtime of Emscripten")

#
# node/
#
configure_file(${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/node/launch.sh.in
    ${CMAKE_CURRENT_BINARY_DIR}/tmp/emscripten/${VM_EXECUTABLE_NAME} @ONLY)
file(
    COPY ${CMAKE_CURRENT_BINARY_DIR}/tmp/emscripten/${VM_EXECUTABLE_NAME}
    DESTINATION ${CMAKE_CURRENT_BINARY_DIR}/build/packaging/emscripten/
    FILE_PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE
)
set(PHARO_WASM_LAUNCHER ${CMAKE_CURRENT_BINARY_DIR}/build/packaging/emscripten/${VM_EXECUTABLE_NAME})

add_custom_command(
    OUTPUT "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.js"
           "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.wasm"
           "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}"
           "${PHARO_WASM_NODE_DIR}/THIRD-PARTY-NOTICES.txt"
    COMMAND ${CMAKE_COMMAND} -E make_directory "${PHARO_WASM_NODE_DIR}"
    COMMAND ${CMAKE_COMMAND} -E copy "$<TARGET_FILE:${VM_EXECUTABLE_NAME}>"
            "$<TARGET_FILE_DIR:${VM_EXECUTABLE_NAME}>/${VM_EXECUTABLE_NAME}.wasm"
            "${PHARO_WASM_LAUNCHER}" "${PHARO_WASM_NOTICES_FILE}" "${PHARO_WASM_NODE_DIR}"
    DEPENDS ${VM_EXECUTABLE_NAME} "${PHARO_WASM_LAUNCHER}" "${PHARO_WASM_NOTICES_FILE}"
    COMMENT "Staging ${PHARO_WASM_NODE_DIR}"
    VERBATIM)
add_custom_target(wasm-node-stage ALL
    DEPENDS "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.js"
            "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.wasm"
            "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}"
            "${PHARO_WASM_NODE_DIR}/THIRD-PARTY-NOTICES.txt")

#
# web/
#
if(FEATURE_FFI)
    set(PHARO_WASM_STAGE_FFI 1)
else()
    set(PHARO_WASM_STAGE_FFI 0)
endif()
if(PHARO_WASM_HAS_FREETYPE)
    set(PHARO_WASM_STAGE_FONTS freetype)
else()
    set(PHARO_WASM_STAGE_FONTS bitmap)
endif()
set(PHARO_WASM_STAGE_ARGUMENTS
    --out "${PHARO_WASM_WEB_DIR}"
    --web "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/web"
    --module "$<TARGET_FILE:pharo-web>"
    --memory64 ${WASM_WEB_MEMORY64}
    --git "${PharoVM_VERSION_GIT_SHA}"
    --ffi ${PHARO_WASM_STAGE_FFI}
    --fonts ${PHARO_WASM_STAGE_FONTS}
    --web-package "${PHARO_WASM_ST_SOURCE_DIR}/OSWindow-Web/OSWebDriver.class.st"
    --notices "${PHARO_WASM_NOTICES_FILE}"
    --stock-image "${WASM_STOCK_IMAGE_DIR}"
    --st "${PHARO_WASM_ST_SOURCE_DIR}/web-repl.st")
if(TARGET wasm-web-image)
    list(APPEND PHARO_WASM_STAGE_ARGUMENTS
        --world-image "${WASM_WEB_IMAGE_DIR}"
        --world-st "${PHARO_WASM_ST_SOURCE_DIR}/web-bootstrap.st")
endif()

add_custom_target(wasm-web-stage ALL
    COMMAND "${NODE_JS_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/tools/stage.mjs"
            ${PHARO_WASM_STAGE_ARGUMENTS}
    COMMENT "Staging ${PHARO_WASM_WEB_DIR}"
    VERBATIM)
add_dependencies(wasm-web-stage pharo-web wasm-stock-image)
if(TARGET wasm-web-image)
    add_dependencies(wasm-web-stage wasm-web-image)
endif()
