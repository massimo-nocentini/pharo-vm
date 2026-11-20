# The libraries whose sources and flags scripts/wasm-deps/harvest.py wrote
#
# GLib, FriBidi, HarfBuzz, expat, fontconfig, Pango, libjpeg-turbo and
# Poppler are built from a sources.cmake of their directory in
# cmake/emscripten/deps, which scripts/wasm-deps/regen-text-stack.sh makes
# from their own meson or CMake builds for Emscripten, and from the files
# those builds generated (in gen/ next to it).  This file defines
#
#     pharo_wasm_harvested_library(<target> <VARIABLE> <optimisation>
#                                  [<public include directory>...])
#
# which makes the static library <target> of the PHARO_WASM_<VARIABLE>_*
# lists of the sources.cmake included last (with ${src} and ${gen} set):
# its sources, and for each language its definitions, include directories
# and options, as the library's own build compiles them, but for the flags
# of the deps directory (-m64, the setjmp/longjmp flags, <optimisation>, -w).
# Its include directories are PRIVATE (the build's config.h would reach the
# libraries that link it): the public ones, where the headers it installs
# are, follow the optimisation, for those libraries and the table of the
# registry of the FFI.

function(pharo_wasm_harvested_library target var optimisation)
    if(NOT PHARO_WASM_${var}_SOURCES)
        message(FATAL_ERROR "pharo_wasm_harvested_library(${target}): no PHARO_WASM_${var}_SOURCES")
    endif()
    foreach(file IN LISTS PHARO_WASM_${var}_SOURCES)
        if(NOT EXISTS "${file}")
            message(FATAL_ERROR "pharo_wasm_harvested_library(${target}): no ${file}: the archive is not the "
                "version its sources.cmake was made for (scripts/wasm-deps/regen-text-stack.sh)")
        endif()
    endforeach()
    add_library(${target} STATIC ${PHARO_WASM_${var}_SOURCES})
    set(languages "")
    foreach(lang C CXX)
        if(DEFINED PHARO_WASM_${var}_${lang}_DEFINES)
            list(APPEND languages ${lang})
        endif()
    endforeach()
    list(LENGTH languages count)
    foreach(lang IN LISTS languages)
        set(defines ${PHARO_WASM_${var}_${lang}_DEFINES})
        set(includes ${PHARO_WASM_${var}_${lang}_INCLUDES})
        set(options ${PHARO_WASM_${var}_${lang}_OPTIONS})
        if(count GREATER 1)
            # (each item for the objects of its language only)
            foreach(list defines includes options)
                foreach(item IN LISTS ${list})
                    if(item MATCHES "[>,;]")
                        message(FATAL_ERROR "pharo_wasm_harvested_library(${target}): '${item}' cannot be "
                            "given to the objects of one language")
                    endif()
                endforeach()
                list(TRANSFORM ${list} PREPEND "$<$<COMPILE_LANGUAGE:${lang}>:")
                list(TRANSFORM ${list} APPEND ">")
            endforeach()
        endif()
        target_compile_definitions(${target} PRIVATE ${defines})
        target_include_directories(${target} PRIVATE ${includes})
        target_compile_options(${target} PRIVATE ${options})
    endforeach()
    if(ARGN)
        target_include_directories(${target} PUBLIC ${ARGN})
    endif()
    pharo_wasm_dep_flags(${target} ${optimisation})
endfunction()
