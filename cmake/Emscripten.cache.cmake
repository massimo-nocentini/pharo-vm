# Initial cache of the Emscripten (WebAssembly) build
#
#     emcmake cmake -C cmake/Emscripten.cache.cmake -S . -B <dir>
#           -DGENERATED_SOURCE_DIR=<dir holding generated/64>
#     cmake --build <dir>
#
# `make wasm' (GNUmakefile) does the same in build-wasm/cmake, and generates
# the sources first; see docs/WebAssembly.md.  The settings below are read
# before cmake/Emscripten.cmake is included, so they cannot be made there:
# the flavour and the generated sources select the interpreter, config.h
# records PHARO_VM_IN_WORKER_THREAD, and the Emscripten toolchain file makes
# pointers 8 bytes wide only when CMAKE_C_FLAGS has -m64.  Other settings
# given with -D before -C are seen here, and kept.

# A StackVM from sources generated elsewhere (the JIT needs executable memory)
set(FLAVOUR "StackVM" CACHE STRING "The kind of VM to generate. Possible values: StackVM, CoInterpreter" FORCE)
set(GENERATE_SOURCES OFF CACHE BOOL "If it generates the C sources" FORCE)

# One thread, and no libffi yet
set(PHARO_VM_IN_WORKER_THREAD OFF CACHE BOOL "Run the VM in a thread different that the main" FORCE)
set(FEATURE_FFI OFF CACHE BOOL "Enable FFI" FORCE)
set(FEATURE_THREADED_FFI OFF CACHE BOOL "Enable Threaded (running in another thread) FFI" FORCE)
set(FEATURE_JIT_SIMD OFF CACHE BOOL "Use SIMD support in JIT compilation when available" FORCE)

# UUIDs come from SocketPlugin; there is no OpenSSL, nor libraries for the FFI
set(FEATURE_PLUGIN_UUID OFF CACHE BOOL "Build UUID plugin" FORCE)
set(FEATURE_PLUGIN_SSL OFF CACHE BOOL "Build SqueakSSL plugin" FORCE)
set(FEATURE_LIB_SDL2 OFF CACHE BOOL "Build SDL2 support" FORCE)
set(FEATURE_LIB_CAIRO OFF CACHE BOOL "Build Cairo support" FORCE)
set(FEATURE_LIB_FREETYPE2 OFF CACHE BOOL "Build freetype2 support" FORCE)
set(FEATURE_LIB_GIT2 OFF CACHE BOOL "Build LibGit2 support" FORCE)
set(BUILD_BUNDLE OFF CACHE BOOL "Builds a bundle with all dependencies" FORCE)
set(BUILD_WITH_GRAPHVIZ OFF CACHE BOOL "Generate dependency graphs" FORCE)

set(CMAKE_BUILD_TYPE "Release" CACHE STRING "Choose the type of build.")

# Attribute 1003 (Smalltalk vm architectureName)
set(EMSCRIPTEN_SYSTEM_PROCESSOR "wasm64" CACHE STRING "CMAKE_SYSTEM_PROCESSOR of the Emscripten toolchain" FORCE)

# Every object is compiled for 64-bit pointers once, and serves both links:
# memory64 for node, and -sMEMORY64=2 (32-bit memory) for the web.  (The
# flags of setjmp/longjmp, WASM_SJLJ, are added by cmake/Emscripten.cmake.)
set(CMAKE_C_FLAGS "-m64" CACHE STRING "Flags used by the C compiler" FORCE)
