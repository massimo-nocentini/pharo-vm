# Third-party libraries of a MinGW-w64 build (MSYS2 UCRT64): take the DLLs
# of the MSYS2 packages instead of the Pharo prebuilt binaries.  The image
# loads them through FFI, so the VM does not link them: copy each one next to
# the VM, under the names the image looks for, and let the install step add
# their transitive dependencies (see configure_installables).
#
#   FEATURE_LIB_GIT2      mingw-w64-ucrt-x86_64-libgit2-winhttp  libgit2-1.9.dll -> also libgit2.dll
#   FEATURE_LIB_FREETYPE2 mingw-w64-ucrt-x86_64-freetype         libfreetype-6.dll
#   FEATURE_LIB_CAIRO     mingw-w64-ucrt-x86_64-cairo            libcairo-2.dll
#   FEATURE_LIB_SDL2      mingw-w64-ucrt-x86_64-SDL2 (+ sdl3)    SDL2.dll (SDL3.dll)

set(PHARO_MINGW_DLLS "")

# add_mingw_dll(<var> <required> NAMES <names>... [COPY_AS <name>]): look for
# one of the names in the bin directory of the MSYS2 prefix (where the
# compiler is) and copy it to the VM directory, optionally also as <name>
get_filename_component(PHARO_MINGW_COMPILER_DIR "${CMAKE_C_COMPILER}" DIRECTORY)
function(add_mingw_dll VAR REQUIRED)
  cmake_parse_arguments(ARG "" "COPY_AS" "NAMES" ${ARGN})
  find_file(${VAR} NAMES ${ARG_NAMES} HINTS "${PHARO_MINGW_COMPILER_DIR}" PATH_SUFFIXES bin NO_CACHE)
  if(NOT ${VAR})
    if(REQUIRED)
      message(FATAL_ERROR "None of ${ARG_NAMES} found: install the MSYS2 package that provides it")
    endif()
    message(STATUS "Optional ${ARG_NAMES} not found")
    return()
  endif()
  message(STATUS "Bundling ${${VAR}}")
  add_custom_command(TARGET ${VM_LIBRARY_NAME} POST_BUILD
    COMMAND ${CMAKE_COMMAND} -E copy_if_different "${${VAR}}" "${LIBRARY_OUTPUT_DIRECTORY}"
    VERBATIM)
  if(ARG_COPY_AS)
    add_custom_command(TARGET ${VM_LIBRARY_NAME} POST_BUILD
      COMMAND ${CMAKE_COMMAND} -E copy_if_different "${${VAR}}" "${LIBRARY_OUTPUT_DIRECTORY}/${ARG_COPY_AS}"
      VERBATIM)
  endif()
  set(PHARO_MINGW_DLLS ${PHARO_MINGW_DLLS} "${${VAR}}" PARENT_SCOPE)
endfunction()

if(FEATURE_LIB_GIT2)
  # Pharo 12-15 look for libgit2-1-6.dll, libgit2-1-5.dll, libgit2-1-4-4.dll or libgit2.dll
  add_mingw_dll(PHARO_MINGW_GIT2_DLL TRUE NAMES libgit2-1.9.dll libgit2-1.8.dll libgit2.dll COPY_AS libgit2.dll)
endif()
if(FEATURE_LIB_FREETYPE2)
  add_mingw_dll(PHARO_MINGW_FREETYPE_DLL TRUE NAMES libfreetype-6.dll)
endif()
if(FEATURE_LIB_CAIRO)
  add_mingw_dll(PHARO_MINGW_CAIRO_DLL TRUE NAMES libcairo-2.dll)
endif()
if(FEATURE_LIB_SDL2)
  add_mingw_dll(PHARO_MINGW_SDL2_DLL TRUE NAMES SDL2.dll)
  add_mingw_dll(PHARO_MINGW_SDL3_DLL FALSE NAMES SDL3.dll)
endif()
