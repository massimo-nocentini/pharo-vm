# The stock images of the Emscripten build: the default one (Pharo 12 unless
# WASM_IMAGE_ZIP gives another), and the extra images of the site
#
# Emscripten.cmake includes this file, with WASM_STAGE_DIR and, optionally,
# WASM_IMAGE_ZIP (a local copy of the zip, for building offline, or another
# image) and WASM_EXTRA_IMAGES (the extra images, a list of <id>=<zip>,
# which the GNUmakefile makes from its space-separated WASM_EXTRA_IMAGES,
# each id checked and each zip absolute).  It defines the targets
#
#     wasm-stock-image       - the pinned image zip, downloaded into
#                              ${WASM_STAGE_DIR}/downloads (or WASM_IMAGE_ZIP),
#                              unpacked into ${WASM_STAGE_DIR}/image/stock:
#                              its image and .changes as Pharo.image and
#                              Pharo.changes, whatever their names in the zip,
#                              and its .sources
#     wasm-stock-image-<id>  - for each <id>=<zip> of WASM_EXTRA_IMAGES, the
#                              same for that zip, which is never downloaded,
#                              into ${WASM_STAGE_DIR}/image/stock-<id>
#
# with the function pharo_wasm_stock_image(target dir stamp url sha zip),
# and sets WASM_STOCK_IMAGE_DIR and WASM_STOCK_IMAGE_DEPENDS (the target and
# the image) for webimage.cmake and stage.cmake, WASM_EXTRA_IMAGE_IDS (the
# ids of the extra images, in their order) and, for each id,
# WASM_EXTRA_STOCK_DIR_<id> and WASM_EXTRA_STOCK_DEPENDS_<id>.  The fixed
# names keep the paths a configuration records valid when a zip changes.
# The targets run on every build, and do something only when the zip, or the
# directory, changed: <dir>.stamp (image/stock.stamp, image/stock-<id>.stamp)
# records the SHA256 and the path of the zip unpacked, and stage.mjs reads
# the build of the image from the name of that zip
# (Pharo15.0-SNAPSHOT.build.50.sha.b85960fcc6.arch.64bit.zip is build 50).
# The pinned zip is checked against its SHA256 (any zip given as
# WASM_IMAGE_ZIP is accepted, and so is any extra zip, whose SHA256 CI checks
# when it downloads it), and unpacked again when it differs from that one or
# a file of the directory is missing.
#
# At build time the same file runs as a script:
#
#     cmake [-DURL=<url> -DSHA256=<hash>] [-DZIP=<zip>] -DDOWNLOAD_DIR=<dir>
#           -DIMAGE_DIR=<dir> -DSTAMP=<file> -P download.cmake
#
# where URL and SHA256 are needed only without ZIP.

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
    foreach(var DOWNLOAD_DIR IMAGE_DIR STAMP)
        if(NOT ${var})
            message(FATAL_ERROR "download.cmake: ${var} is not set")
        endif()
    endforeach()
    if(NOT ZIP)
        foreach(var URL SHA256)
            if(NOT ${var})
                message(FATAL_ERROR "download.cmake: ${var} is not set, and no ZIP is given")
            endif()
        endforeach()
    endif()
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
        # (an extra image has no pinned one)
        if(SHA256 AND NOT hash STREQUAL SHA256)
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
# (CMAKE_CURRENT_LIST_FILE, in a function, is the file that calls it)
set(PHARO_WASM_DOWNLOAD_SCRIPT "${CMAKE_CURRENT_LIST_FILE}")

# pharo_wasm_stock_image(<target> <dir> <stamp> <url> <sha> <zip>): the target
# that unpacks the zip <zip>, or else the one of <url> checked against <sha>,
# into <dir>, recording it in <stamp>
function(pharo_wasm_stock_image target dir stamp url sha zip)
    add_custom_target(${target} ALL
        COMMAND ${CMAKE_COMMAND} "-DURL=${url}" "-DSHA256=${sha}"
                "-DZIP=${zip}" "-DDOWNLOAD_DIR=${WASM_STAGE_DIR}/downloads"
                "-DIMAGE_DIR=${dir}" "-DSTAMP=${stamp}"
                -P "${PHARO_WASM_DOWNLOAD_SCRIPT}"
        BYPRODUCTS "${dir}/Pharo.image" "${dir}/Pharo.changes" "${stamp}"
        VERBATIM)
endfunction()

set(WASM_STOCK_IMAGE_DIR "${WASM_STAGE_DIR}/image/stock")
set(PHARO_WASM_STOCK_IMAGE_STAMP "${WASM_STAGE_DIR}/image/stock.stamp")

if(WASM_IMAGE_ZIP)
    get_filename_component(WASM_IMAGE_ZIP "${WASM_IMAGE_ZIP}" ABSOLUTE)
    if(NOT EXISTS "${WASM_IMAGE_ZIP}")
        message(FATAL_ERROR "WASM_IMAGE_ZIP (${WASM_IMAGE_ZIP}) does not exist")
    endif()
endif()

pharo_wasm_stock_image(wasm-stock-image "${WASM_STOCK_IMAGE_DIR}" "${PHARO_WASM_STOCK_IMAGE_STAMP}"
    "${PHARO_WASM_IMAGE_URL}" "${PHARO_WASM_IMAGE_SHA256}" "${WASM_IMAGE_ZIP}")
set(WASM_STOCK_IMAGE_DEPENDS wasm-stock-image "${WASM_STOCK_IMAGE_DIR}/Pharo.image")

# The extra images: <id>=<zip>, as the GNUmakefile checked them (checked
# again here, for a configuration made without it)
set(WASM_EXTRA_IMAGE_IDS "")
foreach(PHARO_WASM_EXTRA IN LISTS WASM_EXTRA_IMAGES)
    set(PHARO_WASM_EXTRA_ID "")
    if(PHARO_WASM_EXTRA MATCHES "^([a-z0-9][a-z0-9-]*)=([^=]+)$")
        set(PHARO_WASM_EXTRA_ID "${CMAKE_MATCH_1}")
        set(PHARO_WASM_EXTRA_ZIP "${CMAKE_MATCH_2}")
        string(LENGTH "${PHARO_WASM_EXTRA_ID}" PHARO_WASM_EXTRA_LENGTH)
        if(PHARO_WASM_EXTRA_LENGTH GREATER 16)
            set(PHARO_WASM_EXTRA_ID "")
        endif()
    endif()
    # (compared with "", since an id such as 0, n or off is false to if())
    if(PHARO_WASM_EXTRA_ID STREQUAL "")
        message(FATAL_ERROR "WASM_EXTRA_IMAGES: '${PHARO_WASM_EXTRA}' is not <id>=<zip>, with an id of 1 to 16 "
            "lower-case letters, digits and dashes (not first)")
    endif()
    if(PHARO_WASM_EXTRA_ID STREQUAL "sources")
        message(FATAL_ERROR "WASM_EXTRA_IMAGES: the id sources is the directory of the WASM_EXTRA_SOURCES files "
            "(web/image/sources/): name the image otherwise")
    endif()
    get_filename_component(PHARO_WASM_EXTRA_ZIP "${PHARO_WASM_EXTRA_ZIP}" ABSOLUTE)
    if(PHARO_WASM_EXTRA_ID IN_LIST WASM_EXTRA_IMAGE_IDS)
        message(FATAL_ERROR "WASM_EXTRA_IMAGES: the id ${PHARO_WASM_EXTRA_ID} is given twice")
    endif()
    if(NOT EXISTS "${PHARO_WASM_EXTRA_ZIP}")
        message(FATAL_ERROR "WASM_EXTRA_IMAGES: ${PHARO_WASM_EXTRA_ZIP} (${PHARO_WASM_EXTRA_ID}) does not exist")
    endif()
    list(APPEND WASM_EXTRA_IMAGE_IDS "${PHARO_WASM_EXTRA_ID}")
    set(WASM_EXTRA_STOCK_DIR_${PHARO_WASM_EXTRA_ID} "${WASM_STAGE_DIR}/image/stock-${PHARO_WASM_EXTRA_ID}")
    pharo_wasm_stock_image(wasm-stock-image-${PHARO_WASM_EXTRA_ID}
        "${WASM_EXTRA_STOCK_DIR_${PHARO_WASM_EXTRA_ID}}"
        "${WASM_STAGE_DIR}/image/stock-${PHARO_WASM_EXTRA_ID}.stamp" "" "" "${PHARO_WASM_EXTRA_ZIP}")
    set(WASM_EXTRA_STOCK_DEPENDS_${PHARO_WASM_EXTRA_ID} wasm-stock-image-${PHARO_WASM_EXTRA_ID}
        "${WASM_EXTRA_STOCK_DIR_${PHARO_WASM_EXTRA_ID}}/Pharo.image")
    message(STATUS "Extra image ${PHARO_WASM_EXTRA_ID}: ${PHARO_WASM_EXTRA_ZIP}")
endforeach()
