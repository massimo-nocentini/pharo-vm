set(WIN 1)

set(VM_EXECUTABLE_CONSOLE_NAME "${VM_EXECUTABLE_NAME}Console")
set(VM_VERSION_FILEVERSION "${APPNAME}VM-${PharoVM_VERSION_STRING_FULL}")

set(Win32ResourcesFolder "${CMAKE_CURRENT_SOURCE_DIR}/resources/windows")

if(${CYGWIN})
  # transform the path into a windows path with unix backslashes C:/bla/blu
  # this is the path required to send as argument to libraries outside of the control of cygwin (like pharo itself)
  execute_process(
  	COMMAND cygpath ${Win32ResourcesFolder} --mixed
  	OUTPUT_VARIABLE Win32ResourcesFolder_OUT
  	OUTPUT_STRIP_TRAILING_WHITESPACE)
else()
  set(Win32ResourcesFolder_OUT ${Win32ResourcesFolder})
endif()

if(NOT Win32VMExecutableIcon)
    set(Win32VMExecutableIcon "${Win32ResourcesFolder_OUT}/Pharo.ico")
endif()

include_directories(PUBLIC ${CMAKE_CURRENT_SOURCE_DIR}/resources/windows)

set(Win32Resource "${CMAKE_CURRENT_BINARY_DIR}/${VM_EXECUTABLE_NAME}.rc")
set(Win32ConsoleResource "${CMAKE_CURRENT_BINARY_DIR}/${VM_EXECUTABLE_CONSOLE_NAME}.rc")
set(Win32DLLResource "${CMAKE_CURRENT_BINARY_DIR}/${VM_EXECUTABLE_NAME}DLL.rc")
set(Win32Manifest "${CMAKE_CURRENT_BINARY_DIR}/${VM_EXECUTABLE_NAME}.exe.manifest")
set(Win32ConsoleManifest "${CMAKE_CURRENT_BINARY_DIR}/${VM_EXECUTABLE_CONSOLE_NAME}.exe.manifest")

function(add_platform_headers)
    target_include_directories(${VM_LIBRARY_NAME}
    PUBLIC
        ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/win
        ${CMAKE_CURRENT_SOURCE_DIR}/include/pharovm/common
    )
endfunction()

set(EXTRACTED_SOURCES
#Platform sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/sqWin32SpurAlloc.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/aioWin.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/winDebug.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/winDebugMenu.c
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/winDebugWindow.c

# Support sources
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/fileDialogWin32.c

# Resource with DLL version info.
    ${Win32DLLResource}
)

set(VM_FRONTEND_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/win32Main.c
    ${Win32Resource})

set(VM_CONSOLE_FRONTEND_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/win/win32Main.c
    ${Win32ConsoleResource})

set(VM_FRONTEND_APPLICATION_TYPE WIN32)

configure_file("${Win32ResourcesFolder}/${VM_EXECUTABLE_NAME}.rc.in" "${Win32Resource}" @ONLY IMMEDIATE)
configure_file("${Win32ResourcesFolder}/${VM_EXECUTABLE_NAME}DLL.rc.in" "${Win32DLLResource}" @ONLY IMMEDIATE)
configure_file("${Win32ResourcesFolder}/${VM_EXECUTABLE_NAME}.exe.manifest.in" "${Win32Manifest}" @ONLY IMMEDIATE)
configure_file("${Win32ResourcesFolder}/${VM_EXECUTABLE_CONSOLE_NAME}.rc.in" "${Win32ConsoleResource}" @ONLY IMMEDIATE)
configure_file("${Win32ResourcesFolder}/${VM_EXECUTABLE_CONSOLE_NAME}.exe.manifest.in" "${Win32ConsoleManifest}" @ONLY IMMEDIATE)

macro(add_third_party_dependencies_per_platform)

    # MSYS2 (MinGW-w64 with its own CMake): the DLLs of its packages
    if (BUILD_BUNDLE AND MINGW AND NOT CYGWIN)
        include(cmake/importMinGWDependencies.cmake)
    else()
        if (BUILD_BUNDLE AND CYGWIN)
            add_third_party_dependency("gcc-runtime-3.4")
        endif()

        if(${FEATURE_LIB_GIT2})
            include(cmake/importLibGit2.cmake)
        endif()

        if(${FEATURE_LIB_FREETYPE2})
            include(cmake/importFreetype2.cmake)
        endif()

        if(${FEATURE_LIB_CAIRO})
            include(cmake/importCairo.cmake)
        endif()

        if(${FEATURE_LIB_SDL2})
            include(cmake/importSDL2.cmake)
        endif()
    endif()
endmacro()

macro(configure_installables INSTALL_COMPONENT)
    set(CMAKE_INSTALL_PREFIX "${CMAKE_CURRENT_BINARY_DIR}/build/dist")

    install(
          DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/build/vm/"
          DESTINATION "./"
          COMPONENT ${INSTALL_COMPONENT}
          FILES_MATCHING PATTERN *.dll
          PERMISSIONS OWNER_READ OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE)

	install(
		FILES "${Win32Manifest}" "${Win32ConsoleManifest}"
		DESTINATION "./"
        COMPONENT ${INSTALL_COMPONENT})

    install(
          DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/build/vm/"
          USE_SOURCE_PERMISSIONS
          DESTINATION "./"
          USE_SOURCE_PERMISSIONS
          COMPONENT ${INSTALL_COMPONENT}
          FILES_MATCHING
            PATTERN *
            PATTERN *.dll EXCLUDE)

	if(NOT MINGW OR CYGWIN)
	install(
		DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/build/libffi/install/bin/"
		DESTINATION "./"
		COMPONENT ${INSTALL_COMPONENT}
		FILES_MATCHING PATTERN *.dll
		PERMISSIONS OWNER_READ OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE)
	else()
	    # The DLLs of the MinGW runtime and of the MSYS2 packages (libffi-8,
	    # libwinpthread-1, the dependencies of cairo, libgit2, ...) that the VM,
	    # its plugins and the bundled libraries need, found in the directories
	    # of the compiler and of those libraries
	    get_filename_component(_mingw_compiler_dir "${CMAKE_C_COMPILER}" DIRECTORY)
	    set(_mingw_dirs "${_mingw_compiler_dir}")
	    foreach(_dll IN LISTS PHARO_MINGW_DLLS FFI_LIBRARY)
	        get_filename_component(_dir "${_dll}" DIRECTORY)
	        list(APPEND _mingw_dirs "${_dir}" "${_dir}/../bin")
	    endforeach()
	    list(REMOVE_DUPLICATES _mingw_dirs)
	    set(_mingw_dirs_code "")
	    foreach(_dir IN LISTS _mingw_dirs)
	        string(APPEND _mingw_dirs_code " \"${_dir}\"")
	    endforeach()
	    # GNU objdump: before CMake 3.31 file(GET_RUNTIME_DEPENDENCIES) does not
	    # parse the output of llvm-objdump, which CMake prefers with clang
	    find_program(PHARO_PE_OBJDUMP NAMES objdump x86_64-w64-mingw32-objdump
	        HINTS "${_mingw_compiler_dir}" NO_CMAKE_FIND_ROOT_PATH)
	    if(NOT PHARO_PE_OBJDUMP)
	        set(PHARO_PE_OBJDUMP "${CMAKE_OBJDUMP}")
	    endif()
	    install(CODE "
	        if(POLICY CMP0207)
	            cmake_policy(SET CMP0207 NEW)
	        endif()
	        set(CMAKE_GET_RUNTIME_DEPENDENCIES_PLATFORM windows+pe)
	        set(CMAKE_GET_RUNTIME_DEPENDENCIES_TOOL objdump)
	        set(CMAKE_GET_RUNTIME_DEPENDENCIES_COMMAND \"${PHARO_PE_OBJDUMP}\")
	        file(GLOB _pharo_exes \"${CMAKE_CURRENT_BINARY_DIR}/build/vm/*.exe\")
	        file(GLOB _pharo_bins \"${CMAKE_CURRENT_BINARY_DIR}/build/vm/*.dll\")
	        file(GET_RUNTIME_DEPENDENCIES
	            EXECUTABLES \${_pharo_exes}
	            LIBRARIES \${_pharo_bins}
	            DIRECTORIES \"${CMAKE_CURRENT_BINARY_DIR}/build/vm\"${_mingw_dirs_code}
	            PRE_EXCLUDE_REGEXES \"^api-ms-\" \"^ext-ms-\"
	            POST_EXCLUDE_REGEXES \"[Ww][Ii][Nn][Dd][Oo][Ww][Ss][/\\\\][Ss][Yy][Ss][Tt][Ee][Mm]32\" \"/build/vm/\"
	            RESOLVED_DEPENDENCIES_VAR _deps
	            UNRESOLVED_DEPENDENCIES_VAR _unresolved
	            CONFLICTING_DEPENDENCIES_PREFIX _conflicts)
	        foreach(_dep IN LISTS _deps)
	            message(STATUS \"Bundling runtime dependency \${_dep}\")
	            file(INSTALL \"\${_dep}\" DESTINATION \"\$ENV{DESTDIR}\${CMAKE_INSTALL_PREFIX}\")
	        endforeach()
	        if(_unresolved)
	            message(WARNING \"Left to the system: \${_unresolved}\")
	        endif()
	        if(_conflicts_FILENAMES)
	            message(WARNING \"Not bundled, found in more than one directory: \${_conflicts_FILENAMES}\")
	        endif()
	    " COMPONENT ${INSTALL_COMPONENT})
	endif()

	install(
	    DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/include/win/"
	    DESTINATION include/pharovm
	    COMPONENT include
	    FILES_MATCHING PATTERN *.h)

endmacro()

macro(add_required_libs_per_platform)

    # Compile Windows Using Unicode support
    target_compile_definitions(${VM_LIBRARY_NAME}
        PRIVATE -D_UNICODE -DUNICODE)

	add_executable(${VM_EXECUTABLE_CONSOLE_NAME} ${VM_CONSOLE_FRONTEND_SOURCES})
	target_link_libraries(${VM_EXECUTABLE_CONSOLE_NAME} ${VM_LIBRARY_NAME})

	target_link_libraries(${VM_LIBRARY_NAME} winmm)
	target_link_libraries(${VM_LIBRARY_NAME} ws2_32)
	target_link_libraries(${VM_LIBRARY_NAME} dbghelp)
	target_link_libraries(${VM_LIBRARY_NAME} ole32)
	target_link_libraries(${VM_LIBRARY_NAME} comctl32)
	target_link_libraries(${VM_LIBRARY_NAME} uuid)
    # Disable Safe Structured Exception Handling
    #target_link_libraries(${VM_LIBRARY_NAME} "$<$<CXX_COMPILER_ID:MSVC>:-SAFESEH:NO>")
	
	# pthread is required by tffi and the vm itself. We should always link to it.
	if (${FEATURE_LIB_PTHREADW32})		
		find_package(PTHREADW32)
		if(PTHREADW32_FOUND)
			target_link_libraries(${VM_LIBRARY_NAME} PTHREADW32::lib)
		else()
			message(FATAL_ERROR "PTHREADW32 not found. Provide the path in PTHREADW32_DIR env.variable")
		endif()
	else()
		target_link_libraries(${VM_LIBRARY_NAME} pthread)
	endif()

	target_link_libraries(${VM_EXECUTABLE_NAME} ole32)
	target_link_libraries(${VM_EXECUTABLE_NAME} comctl32)
	target_link_libraries(${VM_EXECUTABLE_NAME} uuid)

	target_link_libraries(${VM_EXECUTABLE_CONSOLE_NAME} ole32)
	target_link_libraries(${VM_EXECUTABLE_CONSOLE_NAME} comctl32)
	target_link_libraries(${VM_EXECUTABLE_CONSOLE_NAME} uuid)


	set_target_properties(${VM_EXECUTABLE_NAME} PROPERTIES LINK_FLAGS "-mwindows")
	set_target_properties(${VM_EXECUTABLE_CONSOLE_NAME} PROPERTIES LINK_FLAGS "-mconsole")
endmacro()
