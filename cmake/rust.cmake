# Rust support: the platform layer, and plugins.
#
# The VM's interpreter, GC and JIT are generated from Slang and stay in C. Two
# separate things around them can be Rust, and they are independently switchable
# because they carry different risk:
#
#   USE_RUST_PLATFORM  replaces hand-written C in src/ with pharo-platform,
#                      linked into ${VM_LIBRARY_NAME}. Symbol-for-symbol; see
#                      rust/tools/abi-check.sh.
#
#   USE_RUST_PLUGINS   builds plugins from rust/plugins/ as cdylibs and drops
#                      the C plugins they replace. A plugin is a separate shared
#                      library the VM loads by name, so this cannot affect the
#                      VM core at all.
#
# Turning both OFF must always yield the original all-C build. That is what
# makes the migration's differential testing possible, so please keep it working.
#
# Used in two steps, because the C source list is fixed before the VM library
# target exists while the cargo environment can only be derived from that target:
#
#   include(cmake/rust.cmake)      -- defines the RUST_REPLACED_* lists
#   ... create ${VM_LIBRARY_NAME}, set its includes and definitions ...
#   configure_rust_platform()      -- imports the crates and wires them in
#
# Defines:
#
#     RUST_REPLACED_C_SOURCES  src/*.c files now provided by Rust
#     RUST_REPLACED_PLUGINS    C plugins now provided by Rust
#     configure_rust_platform() deferred wiring; a no-op when both are off

# src/ files whose symbols pharo-platform now provides.
# Add to this list in the same commit that adds the Rust module.
set(RUST_REPLACED_C_SOURCES
    ${CMAKE_CURRENT_SOURCE_DIR}/src/errorCode.c        # rust/pharo-platform/src/error_code.rs
    ${CMAKE_CURRENT_SOURCE_DIR}/src/parameters/parameterVector.c
                                                       # rust/pharo-platform/src/parameter_vector.rs
)

# Ported for Unix only. Each of these has a _WIN32 branch built on APIs whose
# behaviour differs enough that porting them without a Windows machine to test
# on would be guesswork, so Windows keeps compiling the C:
#
#   pathUtilities.c    GetCurrentDirectoryW, FindFirstFileW
#   imageAccess.c      _wfopen, _wstat
#   externalPrimitives.c
#                      LoadLibraryW, GetProcAddress, and a fallback that looks
#                      symbols up in PharoVMCore.dll by name
#   stringUtilities.c  MultiByteToWideChar, WideCharToMultiByte -- these are
#                      `vm_string_convert_utf8_to_utf16` and its inverse, which
#                      string_utilities.rs does not provide and which
#                      src/win/fileDialogWin32.c calls unconditionally. Dropping
#                      the C file on Windows therefore fails the link.
if(NOT WIN32)
    list(APPEND RUST_REPLACED_C_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/pathUtilities.c # rust/pharo-platform/src/path_utilities.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/imageAccess.c   # rust/pharo-platform/src/image_access.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/externalPrimitives.c
                                                        # rust/pharo-platform/src/external_primitives.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/stringUtilities.c
                                                        # rust/pharo-platform/src/string_utilities.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/semaphores/pharoSemaphore.c
                                                        # rust/pharo-platform/src/pharo_semaphore.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/unix/memoryUnix.c
                                                        # rust/pharo-platform/src/memory_unix.rs
    )
endif()

# Unix only, and only when the threaded-FFI worker is built at all:
# CMakeLists.txt compiles these two under FEATURE_FFI AND FEATURE_THREADED_FFI.
# pharo-platform mirrors the same condition with a `feature_threaded_ffi` cfg
# derived from PHAROVM_COMPILE_DEFS (see rust/pharo-platform/build.rs), so the
# Rust modules vanish exactly when the C files do.
#
# This is the first slice of the FFI wave. The sigsetjmp trampolines in
# src/ffi/sameThread/ stay C permanently -- longjmp must never cross a Rust
# frame -- but the worker thread and its task descriptors never touch them.
if(NOT WIN32 AND FEATURE_FFI AND FEATURE_THREADED_FFI)
    list(APPEND RUST_REPLACED_C_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/worker/workerTask.c
                                                        # rust/pharo-platform/src/worker_task.rs
        ${CMAKE_CURRENT_SOURCE_DIR}/src/ffi/worker/worker.c
                                                        # rust/pharo-platform/src/worker.rs
    )
endif()

# Non-Apple Unix only: platformSemaphore.c has three implementations, and the
# Apple one uses dispatch semaphores because POSIX unnamed semaphores are
# deprecated and non-functional there. platform_semaphore.rs is the sem_init
# one only.
if(NOT WIN32 AND NOT APPLE)
    list(APPEND RUST_REPLACED_C_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/semaphores/platformSemaphore.c
                                                        # rust/pharo-platform/src/platform_semaphore.rs
        # Builds its mutex with platform_semaphore, so it follows it.
        ${CMAKE_CURRENT_SOURCE_DIR}/src/threadSafeQueue/threadSafeQueue.c
                                                        # rust/pharo-platform/src/thread_safe_queue.rs
        # Likewise.
        ${CMAKE_CURRENT_SOURCE_DIR}/src/common/sqExternalSemaphores.c
                                                        # rust/pharo-platform/src/external_semaphores.rs
        # The interpreter proxy. Its ~150 declarations live in
        # include/pharovm/common/interpreterProxyFunctions.h, which both this C
        # file and pharo-vm-sys read, so neither side restates them.
        ${CMAKE_CURRENT_SOURCE_DIR}/src/common/sqVirtualMachine.c
                                                        # rust/pharo-platform/src/virtual_machine.rs
        # Apple reads defaults from a PList through parameters.m before
        # parsing, which parameters.rs does not do.
        ${CMAKE_CURRENT_SOURCE_DIR}/src/parameters/parameters.c
                                                        # rust/pharo-platform/src/parameters.rs
    )
endif()

# Not on 32-bit x86 or PowerPC: client.c opens with fldcw / mtfsfi, which set
# the x87 control word and the PowerPC FPSCR. Both expand to nothing on every
# other architecture. Getting x87 precision control wrong changes float results
# instead of crashing, and nothing here can test it, so those two keep the C.
if(NOT WIN32 AND NOT APPLE
   AND NOT CMAKE_SYSTEM_PROCESSOR MATCHES "^(i[3-6]86|x86$|ppc|powerpc)")
    list(APPEND RUST_REPLACED_C_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/client.c        # rust/pharo-platform/src/client.rs
    )
endif()

# 64-bit Unix only: sqHeapMap.c has a second implementation, selected by
# SQ_IMAGE32, with a single-level 256-entry table for a 32-bit address space.
# A 64-bit build cannot reach it, and heap_map.rs does not provide it.
if(NOT WIN32 AND ${SIZEOF_VOID_P} STREQUAL "8")
    list(APPEND RUST_REPLACED_C_SOURCES
        ${CMAKE_CURRENT_SOURCE_DIR}/src/common/sqHeapMap.c
                                                        # rust/pharo-platform/src/heap_map.rs
        # named_prims.rs treats pointerForOop as the identity, which holds
        # while sqMemoryBase is 0 -- true everywhere except a 32-bit image on
        # a 64-bit host.
        ${CMAKE_CURRENT_SOURCE_DIR}/src/common/sqNamedPrims.c
                                                        # rust/pharo-platform/src/named_prims.rs
    )
endif()

# Plugins built from rust/plugins/ instead of plugins/.
# The name is the module name, which is also the cargo lib name and therefore
# the CMake target name -- so the C plugin of the same name must be skipped, or
# the two would collide.
set(RUST_REPLACED_PLUGINS
    JPEGReadWriter2Plugin                              # rust/plugins/jpeg-plugin
)

if(NOT USE_RUST_PLATFORM)
    set(RUST_REPLACED_C_SOURCES "")
endif()
if(NOT USE_RUST_PLUGINS)
    set(RUST_REPLACED_PLUGINS "")
endif()

if(NOT USE_RUST_PLATFORM AND NOT USE_RUST_PLUGINS)
    function(configure_rust_platform)
    endfunction()
    return()
endif()

find_program(CARGO_EXECUTABLE cargo)
if(NOT CARGO_EXECUTABLE)
    message(FATAL_ERROR
        "USE_RUST_PLATFORM or USE_RUST_PLUGINS is ON but cargo was not found.\n"
        "Install a Rust toolchain (https://rustup.rs), or configure with both "
        "OFF to build the all-C VM.")
endif()
message(STATUS "Found cargo: ${CARGO_EXECUTABLE}")

# Corrosion drives cargo from CMake. Preferred over a hand-rolled
# add_custom_command because it maps CMAKE_SYSTEM_NAME/CMAKE_SYSTEM_PROCESSOR
# onto Rust target triples, which this project genuinely needs: CI cross-builds
# for Linux armv7/aarch64, Darwin arm64 and Windows.
include(FetchContent)
FetchContent_Declare(
    Corrosion
    GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
    GIT_TAG v0.5.2
)
FetchContent_MakeAvailable(Corrosion)

# Corrosion names the CMake target after the cargo *lib target*, not the
# package. Cargo lib target names cannot contain hyphens, so the package
# `pharo-platform` yields the lib target `pharo_platform`. Plugin crates set
# their lib name to the module name, so those two already agree.
set(RUST_PLATFORM_PACKAGE pharo-platform)
set(RUST_PLATFORM_TARGET pharo_platform)

function(configure_rust_platform)
    # Import every crate this build needs in one call: corrosion parses the
    # workspace manifest once, and a second call against the same manifest
    # would redefine its targets.
    set(_crates "")
    if(USE_RUST_PLATFORM)
        list(APPEND _crates ${RUST_PLATFORM_PACKAGE})
    endif()
    if(USE_RUST_PLUGINS)
        list(APPEND _crates jpeg-plugin)
    endif()

    corrosion_import_crate(
        MANIFEST_PATH ${CMAKE_SOURCE_DIR}/rust/Cargo.toml
        CRATES ${_crates}
        # Always optimise: this code sits on paths the interpreter leans on,
        # and a debug-profile build would skew any measurement taken against
        # the C build.
        PROFILE release
    )
    # Stated in the configure log so a build can be checked at a glance: the
    # profile is fixed here and does not follow CMAKE_BUILD_TYPE.
    message(STATUS "Rust crates: cargo profile 'release' (independent of CMAKE_BUILD_TYPE)")

    if(USE_RUST_PLATFORM)
        _configure_rust_platform_layer()
    endif()
    if(USE_RUST_PLUGINS)
        _configure_rust_plugins()
    endif()
endfunction()

# Links pharo-platform into the VM core, and hands cargo the header environment
# bindgen needs.
function(_configure_rust_platform_layer)
    message(STATUS "Rust platform layer: ENABLED")

    #
    # pharo-vm-sys runs bindgen at build time rather than shipping checked-in
    # bindings, because sqInt's width is decided by the generated config.h (see
    # include/pharovm/common/memoryAccess.h). Guessing it would be silently
    # wrong on 32-bit targets. So bindgen must see precisely the flags the C
    # sources are compiled with.
    #
    # Read as properties rather than through generator expressions: the values
    # are plain lists at this point, and a configure-time failure is far easier
    # to diagnose than a mangled genex inside a cargo environment.
    #
    get_target_property(_include_dirs ${VM_LIBRARY_NAME} INCLUDE_DIRECTORIES)
    get_target_property(_target_defs ${VM_LIBRARY_NAME} COMPILE_DEFINITIONS)
    # LSB_FIRST and friends come from add_compile_definitions(), which is a
    # directory property and is *not* visible on the target.
    get_directory_property(_dir_defs COMPILE_DEFINITIONS)

    if(NOT _include_dirs)
        message(FATAL_ERROR
            "Could not read INCLUDE_DIRECTORIES from ${VM_LIBRARY_NAME}. "
            "configure_rust_platform() must be called after the target's "
            "include directories are configured.")
    endif()

    # The FFI wave binds types that reach <ffi.h> (wrapper.h includes
    # workerTask.h), but libffi's headers arrive at the C compiler by routes
    # the target property above does not record: as the INTERFACE include of
    # the imported FFI::lib when a system libffi is used, or as a
    # directory-scoped include_directories() in cmake/importLibFFI.cmake when
    # it is built from source. Recover both so bindgen sees what the C saw.
    if(TARGET FFI::lib)
        get_target_property(_ffi_include_dirs FFI::lib INTERFACE_INCLUDE_DIRECTORIES)
        if(_ffi_include_dirs)
            list(APPEND _include_dirs ${_ffi_include_dirs})
        endif()
    endif()
    get_directory_property(_directory_include_dirs INCLUDE_DIRECTORIES)
    if(_directory_include_dirs)
        list(APPEND _include_dirs ${_directory_include_dirs})
    endif()
    list(REMOVE_DUPLICATES _include_dirs)

    set(_defs "")
    foreach(_def IN LISTS _target_defs _dir_defs)
        if(_def)
            list(APPEND _defs "${_def}")
        endif()
    endforeach()
    if(_defs)
        list(REMOVE_DUPLICATES _defs)
    endif()

    # Use '|' rather than CMake's native ';' as the list separator: ';' would
    # need escaping to survive being placed in an environment variable, and
    # include paths never contain '|'. rust/pharo-vm-sys/build.rs splits on the
    # same character.
    string(REPLACE ";" "|" _include_dirs_env "${_include_dirs}")
    string(REPLACE ";" "|" _defs_env "${_defs}")

    corrosion_set_env_vars(${RUST_PLATFORM_TARGET}
        "PHAROVM_INCLUDE_DIRS=${_include_dirs_env}"
        "PHAROVM_COMPILE_DEFS=${_defs_env}"
    )

    # Same environment, in a form a human or a CI step can source, so that
    # `cargo test`, `cargo clippy` and editor tooling can run outside CMake:
    #
    #     source build/rust-env.sh && cd rust && cargo test
    #
    file(WRITE ${CMAKE_BINARY_DIR}/rust-env.sh
"# Generated by cmake/rust.cmake -- do not edit.
# Reproduces the header environment bindgen is given by the CMake build.
# Usage: source this file, then run cargo from the rust/ directory.
export PHAROVM_INCLUDE_DIRS='${_include_dirs_env}'
export PHAROVM_COMPILE_DEFS='${_defs_env}'
")

    # Plain signature, not PRIVATE/PUBLIC: the rest of the project links this
    # target the plain way (see cmake/importLibFFI.cmake) and CMake forbids
    # mixing the two signatures on one target.
    target_link_libraries(${VM_LIBRARY_NAME} ${RUST_PLATFORM_TARGET})

    # The generated interpreter headers must exist before bindgen can parse
    # pharo.h, which includes interp.h. When this same build is generating
    # them, that ordering has to be made explicit.
    if(GENERATE_SOURCES AND TARGET generate-sources)
        add_dependencies(cargo-prebuild_${RUST_PLATFORM_TARGET} generate-sources)
    endif()

    list(LENGTH RUST_REPLACED_C_SOURCES _replaced_count)
    message(STATUS
        "Rust platform layer: replacing ${_replaced_count} C source file(s)")
endfunction()

# Puts each Rust plugin's shared library where the VM looks for plugins.
function(_configure_rust_plugins)
    message(STATUS "Rust plugins: ENABLED")

    foreach(_plugin IN LISTS RUST_REPLACED_PLUGINS)
        # For a cdylib crate corrosion creates `<name>-shared` as the imported
        # library; plain `<name>` is an INTERFACE target and has no TARGET_FILE.
        if(NOT TARGET ${_plugin}-shared)
            message(FATAL_ERROR
                "Rust plugin '${_plugin}' is listed in RUST_REPLACED_PLUGINS "
                "but corrosion created no ${_plugin}-shared target. The crate's "
                "[lib] name must equal the module name, its crate-type must "
                "include \"cdylib\", and it must be in the CRATES list above.")
        endif()

        # Corrosion builds into the cargo target dir; the VM only looks beside
        # its executable, so copy it there like the C plugins do.
        #
        # A separate target rather than a POST_BUILD on cargo-build_<plugin>:
        # `$<TARGET_FILE:<plugin>-shared>` makes the command depend on that
        # imported target, and corrosion already makes it depend on
        # cargo-build_<plugin> -- so a POST_BUILD there would be a self-cycle.
        add_custom_target(rust-plugin-copy_${_plugin}
            COMMAND ${CMAKE_COMMAND} -E make_directory "${LIBRARY_OUTPUT_DIRECTORY}"
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                    "$<TARGET_FILE:${_plugin}-shared>" "${LIBRARY_OUTPUT_DIRECTORY}"
            COMMENT "Copying Rust plugin ${_plugin} to ${LIBRARY_OUTPUT_DIRECTORY}"
            VERBATIM)
        add_dependencies(rust-plugin-copy_${_plugin} cargo-build_${_plugin})

        # Built as part of the normal build, as add_vm_plugin arranges for the
        # C plugins.
        add_dependencies(${VM_EXECUTABLE_NAME} rust-plugin-copy_${_plugin})
        message(STATUS "Rust plugins: ${_plugin} replaces the C plugin")
    endforeach()
endfunction()
