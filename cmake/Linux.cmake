option(READ_ONLY_CODE_ZONE "Makes Cogit's code zone never writeable and executable at the same time" OFF)

# Every ELF file of the VM (lib/pharo, libPharoVMCore.so, the plugins, the
# FFI libraries) finds the libraries of the bundle in its own directory, and
# nowhere else: the rpath is exactly $ORIGIN.  It used to be ".", the current
# directory (a libPharoVMCore.so or libffi.so planted there was loaded:
# CWE-426), plus the absolute path of the build tree, which the bundle copies
# (build/vm).  The build tree gets the rpath of the bundle (CMake escapes
# $ORIGIN for the generator); CMAKE_BUILD_RPATH_USE_ORIGIN keeps any build
# rpath relative should a target leave that setting.
# DT_RPATH (--disable-new-dtags), as the cairo stack's libraries have, rather
# than DT_RUNPATH: the libraries the image loads from the VM's directory and
# that have no rpath of their own (the files.pharo.org libgit2, libssh2 and
# OpenSSL) find their dependencies through the DT_RPATH of the VM core and
# of lib/pharo, and the bundle's libraries come before any LD_LIBRARY_PATH,
# as when the launcher put the VM's directory first in it (it no longer
# does: packaging/linux/launch.sh.in).
# -Wl flags are link-time flags; keep them out of CMAKE_C_FLAGS so they aren't
# passed to every compile step (clang: -Wunused-command-line-argument).
set(CMAKE_SKIP_BUILD_RPATH OFF)
set(CMAKE_BUILD_WITH_INSTALL_RPATH ON)
set(CMAKE_INSTALL_RPATH "$ORIGIN")
set(CMAKE_BUILD_RPATH_USE_ORIGIN ON)
set(CMAKE_EXE_LINKER_FLAGS "${CMAKE_EXE_LINKER_FLAGS} -Wl,--disable-new-dtags")
set(CMAKE_SHARED_LINKER_FLAGS "${CMAKE_SHARED_LINKER_FLAGS} -Wl,--disable-new-dtags")
set(CMAKE_MODULE_LINKER_FLAGS "${CMAKE_MODULE_LINKER_FLAGS} -Wl,--disable-new-dtags")
set(PHARO_BIN_LOCATION "default" CACHE STRING "The default location of the PHARO bin, used by the launch.sh.in")

if(${PHARO_BIN_LOCATION} STREQUAL "default")
	set(PHARO_BIN_IN_ROOT "`/usr/bin/dirname \"\$0\"`/lib")
	set(PHARO_BIN_IN_BIN "`/usr/bin/dirname \"\$0\"`/../lib")
else()
	set(PHARO_BIN_IN_ROOT ${PHARO_BIN_LOCATION})
	set(PHARO_BIN_IN_BIN ${PHARO_BIN_LOCATION})
endif()

function(add_platform_headers)
target_include_directories(${VM_LIBRARY_NAME}
PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/unix
    ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/common
)
endfunction() #add_platform_headers

set(EXTRACTED_SOURCES
#Platform sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/aio.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/debugUnix.c

#Virtual Memory functions
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/memoryUnix.c

# Support sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/fileDialogUnix.c
)

set(VM_FRONTEND_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/unixMain.c)


# cairo and FreeType: the cairo stack (cmake/cairoStack.cmake), set up here,
# before cmake/plugins.cmake, whose CairoPlugin compiles with its headers
include(cmake/cairoStack.cmake)

macro(add_third_party_dependencies_per_platform)
	if(${FEATURE_LIB_GIT2})
        include(cmake/importLibGit2.cmake)
    endif()

    # cairo and FreeType: the pinned stack, when it is on (cmake/cairoStack.cmake)
    pharo_cairo_stack_attach()

    if(${FEATURE_LIB_SDL2})
        include(cmake/importSDL2.cmake)
    endif()
endmacro()


macro(configure_installables INSTALL_COMPONENT)
    set(CMAKE_INSTALL_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/build/dist")
    
    #Configure the launch scripts and set execution permissions explicitly
    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/packaging/linux/launch.sh.in
        ${CMAKE_CURRENT_BINARY_DIR}/tmp/${VM_EXECUTABLE_NAME} @ONLY)
    file(
        COPY ${CMAKE_CURRENT_BINARY_DIR}/tmp/${VM_EXECUTABLE_NAME}
        DESTINATION ${CMAKE_CURRENT_BINARY_DIR}/build/packaging/linux/
        FILE_PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE
    )

    configure_file(${CMAKE_CURRENT_SOURCE_DIR}/packaging/linux/bin/launch.sh.in
        ${CMAKE_CURRENT_BINARY_DIR}/tmp/bin/${VM_EXECUTABLE_NAME} @ONLY)
    file(
        COPY ${CMAKE_CURRENT_BINARY_DIR}/tmp/bin/${VM_EXECUTABLE_NAME}
        DESTINATION ${CMAKE_CURRENT_BINARY_DIR}/build/packaging/linux/bin/
        FILE_PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE
    )

    install(
      DIRECTORY "${CMAKE_BINARY_DIR}/build/packaging/linux/"
      DESTINATION "."
      USE_SOURCE_PERMISSIONS
      COMPONENT ${INSTALL_COMPONENT})
    install(
      DIRECTORY "${CMAKE_BINARY_DIR}/build/vm/"
      DESTINATION "lib"
      USE_SOURCE_PERMISSIONS
      COMPONENT ${INSTALL_COMPONENT})
	install(
		DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/build/libffi/install/lib/"
		DESTINATION "lib"
		USE_SOURCE_PERMISSIONS
		COMPONENT ${INSTALL_COMPONENT}
		FILES_MATCHING PATTERN ${DYLIB_EXT})


	install(
	    DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/include/unix/"
	    DESTINATION include/pharovm
	    COMPONENT include
	    FILES_MATCHING PATTERN *.h)
endmacro()

macro(add_required_libs_per_platform)
  if(READ_ONLY_CODE_ZONE)
    target_compile_definitions(${VM_LIBRARY_NAME} PRIVATE READ_ONLY_CODE_ZONE=1)
  endif()
  target_link_libraries(${VM_LIBRARY_NAME} dl)
  target_link_libraries(${VM_LIBRARY_NAME} m)
  target_link_libraries(${VM_LIBRARY_NAME} pthread)
endmacro()
