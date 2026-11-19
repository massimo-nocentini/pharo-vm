# Emscripten builds upstream libffi, which has a WebAssembly port, from its
# pinned archive in the directory of the libraries
# (cmake/emscripten/deps/libffi.cmake): no system libffi, nor binaries
if(EMSCRIPTEN)
  target_link_libraries(${VM_LIBRARY_NAME} pharo_libffi)
  return()
endif()

function(find_system_ffi)
  message(STATUS "Looking for FFI in the system")
  find_package(FFI)
  if(FFI_FOUND)
	target_link_libraries(${VM_LIBRARY_NAME} FFI::lib)
  else()
    message(STATUS "FFI not found.")
  endif()
  set(FFI_FOUND ${FFI_FOUND} PARENT_SCOPE)
endfunction()

function(build_ffi)
	message(STATUS "Building FFI")
	include(cmake/DownloadProject.cmake)

	# The pharo-project fork (libffi 3.3 with a CMake build), pinned by commit
	# and SHA256: the head of its branch v3.3-cmake on 2026-11-19, which the
	# build used to clone by name, unchecked
	download_project(PROJ   libffi
		URL                 https://github.com/pharo-project/libffi/archive/2b832583491b9f468caf04cbfaf54d3d33c0e04a.tar.gz
		URL_HASH            SHA256=d0c60bc6e46a7c8fce689dbddd106b3c1d030ceaeefc16943eda425135234eaf
		DOWNLOAD_NAME       libffi-2b832583491b9f468caf04cbfaf54d3d33c0e04a.tar.gz
        ${UPDATE_DISCONNECTED_IF_AVAILABLE}
	)

	add_subdirectory(${libffi_SOURCE_DIR} ${libffi_BINARY_DIR} EXCLUDE_FROM_ALL)

	if(CYGWIN)
		set_target_properties(objlib PROPERTIES POSITION_INDEPENDENT_CODE OFF)
	endif()

	set_target_properties(ffi_shared PROPERTIES LIBRARY_OUTPUT_DIRECTORY "${LIBRARY_OUTPUT_DIRECTORY}")

	# libffi cmakelists does not correctly export the library includes
	# so we have to make it ourselves...
	include_directories("${libffi_BINARY_DIR}/include")

	add_library(libFFI ALIAS ffi_shared)
    
  add_custom_target(libffi_copy
    COMMAND ${CMAKE_COMMAND} -E copy "$<TARGET_FILE:ffi_shared>" "${LIBRARY_OUTPUT_DIRECTORY}"
		COMMENT "Copying libffi binaries to '${LIBRARY_OUTPUT_DIRECTORY}'" VERBATIM
  )
  
  add_dependencies(libffi_copy libFFI)
  
	target_link_libraries(${VM_LIBRARY_NAME} ffi_shared)
  add_dependencies(${VM_LIBRARY_NAME} libffi_copy)
endfunction()

if(PHARO_DEPENDENCIES_PREFER_DOWNLOAD_BINARIES AND NOT(OSX AND ${CMAKE_SYSTEM_PROCESSOR} MATCHES "arm64"))
  #Download FFI binaries directly
  build_FFI()
else()
  #Look for FFI in the system, then build or download if possible
  find_system_FFI()
  if(NOT FFI_FOUND)
    build_FFI()
  endif()
endif()
