# The stock Pharo 12 image of the Emscripten build
#
# Emscripten.cmake includes this file, with WASM_STAGE_DIR and, optionally,
# WASM_IMAGE_ZIP (a local copy of the zip, for building offline).  It
# defines the target
#
#     wasm-stock-image  - the pinned image zip, downloaded into
#                         ${WASM_STAGE_DIR}/downloads (or WASM_IMAGE_ZIP),
#                         unpacked into ${WASM_STAGE_DIR}/image/stock: its
#                         image and .changes as Pharo.image and Pharo.changes,
#                         whatever their names in the zip, and its .sources
#
# and sets WASM_STOCK_IMAGE_DIR for stage.cmake.  The target runs on every
# build, and does something only when the zip, or image/stock, changed:
# image/stock.stamp records the SHA256 and the path of the zip unpacked.
# The zip is checked against its SHA256 (any zip given as WASM_IMAGE_ZIP is
# accepted), and unpacked again when it differs from that one or a file of
# image/stock is missing.
#
# At build time the same file runs as a script:
#
#     cmake -DURL=<url> -DSHA256=<hash> [-DZIP=<zip>] -DDOWNLOAD_DIR=<dir>
#           -DIMAGE_DIR=<dir> -DSTAMP=<file> -P download.cmake

# Gives the one .image of dir and its .changes the names Pharo.image and
# Pharo.changes
function(pharo_wasm_name_stock_image dir)
    file(GLOB image "${dir}/*.image")
    list(LENGTH image count)
    if(count EQUAL 1 AND NOT image STREQUAL "${dir}/Pharo.image")
        string(REGEX REPLACE "\\.image$" ".changes" changes "${image}")
        if(EXISTS "${changes}")
            file(RENAME "${image}" "${dir}/Pharo.image")
            file(RENAME "${changes}" "${dir}/Pharo.changes")
        endif()
    endif()
endfunction()

if(CMAKE_SCRIPT_MODE_FILE)
    foreach(var URL SHA256 DOWNLOAD_DIR IMAGE_DIR STAMP)
        if(NOT ${var})
            message(FATAL_ERROR "download.cmake: ${var} is not set")
        endif()
    endforeach()
    if(ZIP)
        if(NOT EXISTS "${ZIP}")
            message(FATAL_ERROR "download.cmake: ${ZIP} does not exist")
        endif()
        set(given TRUE)
    else()
        get_filename_component(name "${URL}" NAME)
        set(ZIP "${DOWNLOAD_DIR}/${name}")
        set(given FALSE)
    endif()

    # What is unpacked, and from which zip: "<SHA256> <zip>"
    file(GLOB sources "${IMAGE_DIR}/*.sources")
    set(unpacked "")
    if(EXISTS "${IMAGE_DIR}/Pharo.image" AND EXISTS "${IMAGE_DIR}/Pharo.changes" AND sources
       AND EXISTS "${STAMP}")
        file(READ "${STAMP}" record)
        string(REGEX MATCH "^[0-9a-f]+" unpacked "${record}")
        # Nothing changed since: the zip is the one unpacked, and no newer
        # (a downloaded zip may be gone, a given one may not)
        if(record STREQUAL "${unpacked} ${ZIP}\n"
           AND (given OR unpacked STREQUAL SHA256)
           AND (NOT EXISTS "${ZIP}" OR NOT "${ZIP}" IS_NEWER_THAN "${STAMP}"))
            return()
        endif()
    endif()

    set(hash "")
    if(given)
        file(SHA256 "${ZIP}" hash)
        if(NOT hash STREQUAL SHA256)
            message(STATUS "download.cmake: ${ZIP} is not the pinned image (SHA256 ${hash})")
        endif()
    else()
        if(EXISTS "${ZIP}")
            file(SHA256 "${ZIP}" hash)
            if(NOT hash STREQUAL SHA256)
                message(STATUS "download.cmake: ${ZIP} does not match its SHA256, downloading it again")
                file(REMOVE "${ZIP}")
            endif()
        elseif(unpacked STREQUAL SHA256)
            set(hash "${SHA256}")       # unpacked already, and the zip removed since
        endif()
        if(NOT EXISTS "${ZIP}" AND NOT hash STREQUAL SHA256)
            file(MAKE_DIRECTORY "${DOWNLOAD_DIR}")
            message(STATUS "Downloading ${URL}")
            file(DOWNLOAD "${URL}" "${ZIP}.part" EXPECTED_HASH SHA256=${SHA256} STATUS status)
            list(GET status 0 code)
            if(NOT code EQUAL 0)
                file(REMOVE "${ZIP}.part")
                list(GET status 1 reason)
                message(FATAL_ERROR "download.cmake: cannot download ${URL}: ${reason}; "
                    "to build offline, give a copy of it with WASM_IMAGE_ZIP")
            endif()
            file(RENAME "${ZIP}.part" "${ZIP}")
            set(hash "${SHA256}")
        endif()
    endif()

    set(record "${hash} ${ZIP}\n")
    if(unpacked STREQUAL hash)
        file(WRITE "${STAMP}" "${record}")
        return()
    endif()

    message(STATUS "Unpacking ${ZIP} into ${IMAGE_DIR}")
    set(unpack "${IMAGE_DIR}.unpack")
    file(REMOVE_RECURSE "${unpack}")
    file(MAKE_DIRECTORY "${unpack}")
    execute_process(
        COMMAND ${CMAKE_COMMAND} -E tar xf "${ZIP}"
        WORKING_DIRECTORY "${unpack}"
        RESULT_VARIABLE status)
    if(NOT status EQUAL 0)
        message(FATAL_ERROR "download.cmake: cannot unpack ${ZIP}")
    endif()
    file(GLOB image "${unpack}/*.image")
    list(LENGTH image count)
    if(NOT count EQUAL 1)
        message(FATAL_ERROR "download.cmake: expected one .image in ${ZIP}, found ${count}")
    endif()
    string(REGEX REPLACE "\\.image$" ".changes" changes "${image}")
    file(GLOB sources "${unpack}/*.sources")
    if(NOT EXISTS "${changes}" OR NOT sources)
        message(FATAL_ERROR "download.cmake: ${ZIP} lacks the .changes or the .sources of its image")
    endif()
    pharo_wasm_name_stock_image("${unpack}")
    # The files carry the dates of the zip: make them newer than whatever
    # was made from an earlier image.
    file(GLOB files "${unpack}/*")
    execute_process(COMMAND ${CMAKE_COMMAND} -E touch ${files})

    file(REMOVE_RECURSE "${IMAGE_DIR}")
    file(RENAME "${unpack}" "${IMAGE_DIR}")
    file(WRITE "${STAMP}" "${record}")
    return()
endif()

set(PHARO_WASM_IMAGE_URL "https://files.pharo.org/image/120/Pharo12.0-SNAPSHOT.build.1599.sha.7d5f14cb47.arch.64bit.zip")
set(PHARO_WASM_IMAGE_SHA256 "1fc69710c5febf04e84dd5eff4c10239ac10ad2a2405be93ab049266499eb091")

if(NOT WASM_STAGE_DIR)
    set(WASM_STAGE_DIR "${CMAKE_BINARY_DIR}")
endif()
set(WASM_STOCK_IMAGE_DIR "${WASM_STAGE_DIR}/image/stock")
set(PHARO_WASM_STOCK_IMAGE_STAMP "${WASM_STAGE_DIR}/image/stock.stamp")

if(WASM_IMAGE_ZIP)
    get_filename_component(WASM_IMAGE_ZIP "${WASM_IMAGE_ZIP}" ABSOLUTE)
    if(NOT EXISTS "${WASM_IMAGE_ZIP}")
        message(FATAL_ERROR "WASM_IMAGE_ZIP (${WASM_IMAGE_ZIP}) does not exist")
    endif()
endif()

add_custom_target(wasm-stock-image ALL
    COMMAND ${CMAKE_COMMAND} "-DURL=${PHARO_WASM_IMAGE_URL}" "-DSHA256=${PHARO_WASM_IMAGE_SHA256}"
            "-DZIP=${WASM_IMAGE_ZIP}" "-DDOWNLOAD_DIR=${WASM_STAGE_DIR}/downloads"
            "-DIMAGE_DIR=${WASM_STOCK_IMAGE_DIR}" "-DSTAMP=${PHARO_WASM_STOCK_IMAGE_STAMP}"
            -P "${CMAKE_CURRENT_LIST_FILE}"
    BYPRODUCTS "${WASM_STOCK_IMAGE_DIR}/Pharo.image" "${WASM_STOCK_IMAGE_DIR}/Pharo.changes"
               "${PHARO_WASM_STOCK_IMAGE_STAMP}"
    VERBATIM)
