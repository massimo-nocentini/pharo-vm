# Image of the Pharo world in the browser
#
# The browser world needs the OSWindow-Web package in the image, and bitmap
# fonts, since the web VM has no FFI and therefore no FreeType.  A native
# Pharo VM prepares that image at build time: it runs
# packaging/emscripten/st/prepare-web-image.st on a copy of the stock Pharo 12
# image.  That takes about a second, and keeps the world image independent of
# the web VM being built.
#
# Emscripten.cmake includes this file after download.cmake and before
# stage.cmake, with
#
#     WASM_HOST_PHARO          - the native Pharo VM.  Without it the world
#                                image is not prepared, and web/ gets the
#                                stock image
#     WASM_STOCK_IMAGE_DIR     - the unpacked stock image: one .image, its
#                                .changes and the .sources (download.cmake)
#     WASM_STOCK_IMAGE_DEPENDS - the files and targets that produce the stock
#                                image (download.cmake)
#     WASM_WEB_IMAGE_DIR       - where the world image goes
#                                (default ${WASM_STAGE_DIR}/image/web)
#     WASM_WORLD               - OFF leaves the world out
#
# and it defines the target
#
#     wasm-web-image           - ${WASM_WEB_IMAGE_DIR}/Pharo12-web.image,
#                                Pharo12-web.changes and OSWindow-Web.st (the
#                                package filed out for web-bootstrap.st), next
#                                to a copy of the stock .sources
#
# which wasm-web-stage depends on when it exists.  The files of the package
# are listed at configure time, and their directory is a dependency of both
# the configuration and the image: a class that comes or goes configures
# again and prepares the image again.
#
# At build time the same file runs as a script, because the name of the stock
# image is known only once it is unpacked:
#
#     cmake -DWASM_HOST_PHARO=<vm> -DWASM_STOCK_IMAGE_DIR=<dir>
#           -DWASM_WEB_IMAGE_DIR=<dir> -DWORK_DIR=<dir> -P webimage.cmake

get_filename_component(PHARO_WASM_ST_DIR "${CMAKE_CURRENT_LIST_DIR}/../../packaging/emscripten/st" ABSOLUTE)

if(CMAKE_SCRIPT_MODE_FILE)
    # An empty WORK_DIR would make the REMOVE_RECURSE below clear the
    # current directory with older CMake versions.
    foreach(var WASM_HOST_PHARO WASM_STOCK_IMAGE_DIR WASM_WEB_IMAGE_DIR WORK_DIR)
        if(NOT ${var})
            message(FATAL_ERROR "webimage.cmake: ${var} is not set")
        endif()
    endforeach()
    file(GLOB stockImage "${WASM_STOCK_IMAGE_DIR}/*.image")
    list(LENGTH stockImage count)
    if(NOT count EQUAL 1)
        message(FATAL_ERROR "webimage.cmake: expected one .image in ${WASM_STOCK_IMAGE_DIR}, found ${count}")
    endif()
    string(REGEX REPLACE "\\.image$" ".changes" stockChanges "${stockImage}")
    file(GLOB stockSources "${WASM_STOCK_IMAGE_DIR}/*.sources")
    if(NOT EXISTS "${stockChanges}" OR NOT stockSources)
        message(FATAL_ERROR "webimage.cmake: ${WASM_STOCK_IMAGE_DIR} lacks the .changes or the .sources of ${stockImage}")
    endif()
    get_filename_component(imageName "${stockImage}" NAME)
    set(outputs
        "${WASM_WEB_IMAGE_DIR}/Pharo12-web.image"
        "${WASM_WEB_IMAGE_DIR}/Pharo12-web.changes"
        "${WASM_WEB_IMAGE_DIR}/OSWindow-Web.st")

    # Work on a copy: the stock image also goes to web/ when there is no
    # world, and the script saves the image it runs in.
    file(REMOVE_RECURSE "${WORK_DIR}")
    file(COPY "${stockImage}" "${stockChanges}" ${stockSources} DESTINATION "${WORK_DIR}")
    file(REMOVE ${outputs})

    execute_process(
        COMMAND "${WASM_HOST_PHARO}" --headless "${WORK_DIR}/${imageName}" --no-default-preferences
                --save --quit "${PHARO_WASM_ST_DIR}/prepare-web-image.st"
                "${PHARO_WASM_ST_DIR}" "${WASM_WEB_IMAGE_DIR}"
        WORKING_DIRECTORY "${WORK_DIR}"
        TIMEOUT 600
        RESULT_VARIABLE status)
    if(NOT status EQUAL 0)
        message(FATAL_ERROR "webimage.cmake: prepare-web-image.st failed (${status}), see ${WORK_DIR}")
    endif()
    foreach(output IN LISTS outputs)
        if(NOT EXISTS "${output}")
            message(FATAL_ERROR "webimage.cmake: prepare-web-image.st did not write ${output}, see ${WORK_DIR}")
        endif()
    endforeach()
    # With its .sources the new image runs natively too.  pharo-local holds
    # the Epicea log of the preparation.
    file(COPY ${stockSources} DESTINATION "${WASM_WEB_IMAGE_DIR}")
    file(REMOVE_RECURSE "${WORK_DIR}" "${WASM_WEB_IMAGE_DIR}/pharo-local")
    return()
endif()

if(DEFINED WASM_WORLD AND NOT WASM_WORLD)
    return()
endif()
if(NOT WASM_HOST_PHARO)
    message(STATUS "WASM_HOST_PHARO is not set: the world image is not prepared, and web/ gets the stock image")
    return()
endif()
if(NOT EXISTS "${WASM_HOST_PHARO}")
    message(WARNING "WASM_HOST_PHARO (${WASM_HOST_PHARO}) does not exist: the world image is not prepared, and web/ gets the stock image")
    return()
endif()

if(NOT WASM_WEB_IMAGE_DIR)
    set(WASM_WEB_IMAGE_DIR "${WASM_STAGE_DIR}/image/web")
endif()

set(PHARO_WASM_WEB_IMAGE_OUTPUTS
    "${WASM_WEB_IMAGE_DIR}/Pharo12-web.image"
    "${WASM_WEB_IMAGE_DIR}/Pharo12-web.changes"
    "${WASM_WEB_IMAGE_DIR}/OSWindow-Web.st")

file(GLOB PHARO_WASM_WEB_PACKAGE_FILES "${PHARO_WASM_ST_DIR}/OSWindow-Web/*.st")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS "${PHARO_WASM_ST_DIR}/OSWindow-Web")
set(PHARO_WASM_WEB_IMAGE_DEPENDS "")
set(PHARO_WASM_WEB_IMAGE_TARGETS "")
foreach(item IN LISTS WASM_STOCK_IMAGE_DEPENDS)
    if(TARGET "${item}")
        list(APPEND PHARO_WASM_WEB_IMAGE_TARGETS "${item}")
    else()
        list(APPEND PHARO_WASM_WEB_IMAGE_DEPENDS "${item}")
    endif()
endforeach()

add_custom_command(
    OUTPUT ${PHARO_WASM_WEB_IMAGE_OUTPUTS}
    COMMAND ${CMAKE_COMMAND} "-DWASM_HOST_PHARO=${WASM_HOST_PHARO}"
            "-DWASM_STOCK_IMAGE_DIR=${WASM_STOCK_IMAGE_DIR}"
            "-DWASM_WEB_IMAGE_DIR=${WASM_WEB_IMAGE_DIR}"
            "-DWORK_DIR=${CMAKE_BINARY_DIR}/wasm/webimage"
            -P "${CMAKE_CURRENT_LIST_FILE}"
    DEPENDS ${PHARO_WASM_WEB_PACKAGE_FILES} "${PHARO_WASM_ST_DIR}/OSWindow-Web"
            "${PHARO_WASM_ST_DIR}/prepare-web-image.st"
            "${CMAKE_CURRENT_LIST_FILE}" ${PHARO_WASM_WEB_IMAGE_DEPENDS}
    COMMENT "Preparing the image of the Pharo world with ${WASM_HOST_PHARO}"
    VERBATIM)

add_custom_target(wasm-web-image ALL DEPENDS ${PHARO_WASM_WEB_IMAGE_OUTPUTS})
if(PHARO_WASM_WEB_IMAGE_TARGETS)
    add_dependencies(wasm-web-image ${PHARO_WASM_WEB_IMAGE_TARGETS})
endif()
