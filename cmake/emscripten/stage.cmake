# Staging of the Emscripten build
#
# Emscripten.cmake includes this file last, once the executables and the
# stock image (download.cmake) are defined.  It defines the targets
#
#     wasm-node-stage  - ${WASM_STAGE_DIR}/node: pharo.js, pharo.wasm and
#                        the launcher pharo (packaging/emscripten/node/launch.sh.in)
#     wasm-web-stage   - ${WASM_STAGE_DIR}/web, the static site, written by
#                        packaging/emscripten/tools/stage.mjs: the files of
#                        packaging/emscripten/web with @BUILD@ replaced by the
#                        build id, pharo-web.js and pharo-web.wasm, the image
#                        (gzipped) in image/, the st files in st/ and
#                        manifest.json.  It always runs, and rewrites only what
#                        changed.

set(PHARO_WASM_NODE_DIR "${WASM_STAGE_DIR}/node")
set(PHARO_WASM_WEB_DIR "${WASM_STAGE_DIR}/web")
set(PHARO_WASM_ST_SOURCE_DIR "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/st")

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
    COMMAND ${CMAKE_COMMAND} -E make_directory "${PHARO_WASM_NODE_DIR}"
    COMMAND ${CMAKE_COMMAND} -E copy "$<TARGET_FILE:${VM_EXECUTABLE_NAME}>"
            "$<TARGET_FILE_DIR:${VM_EXECUTABLE_NAME}>/${VM_EXECUTABLE_NAME}.wasm"
            "${PHARO_WASM_LAUNCHER}" "${PHARO_WASM_NODE_DIR}"
    DEPENDS ${VM_EXECUTABLE_NAME} "${PHARO_WASM_LAUNCHER}"
    COMMENT "Staging ${PHARO_WASM_NODE_DIR}"
    VERBATIM)
add_custom_target(wasm-node-stage ALL
    DEPENDS "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.js"
            "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}.wasm"
            "${PHARO_WASM_NODE_DIR}/${VM_EXECUTABLE_NAME}")

#
# web/
#
set(PHARO_WASM_STAGE_ARGUMENTS
    --out "${PHARO_WASM_WEB_DIR}"
    --web "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/web"
    --module "$<TARGET_FILE:pharo-web>"
    --memory64 ${WASM_WEB_MEMORY64}
    --git "${PharoVM_VERSION_GIT_SHA}"
    --stock-image "${WASM_STOCK_IMAGE_DIR}"
    --st "${PHARO_WASM_ST_SOURCE_DIR}/web-repl.st")

add_custom_target(wasm-web-stage ALL
    COMMAND "${NODE_JS_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/packaging/emscripten/tools/stage.mjs"
            ${PHARO_WASM_STAGE_ARGUMENTS}
    COMMENT "Staging ${PHARO_WASM_WEB_DIR}"
    VERBATIM)
add_dependencies(wasm-web-stage pharo-web wasm-stock-image)
