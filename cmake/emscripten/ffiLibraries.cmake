# The libraries of the FFI of the Emscripten VM
#
# cmake/Emscripten.cmake includes this file when FEATURE_FFI is on, after
# the libraries of cmake/emscripten/deps.  Each library is linked in, and its
# symbols listed in the registry (cmake/emscripten/ffiRegistry.cmake), under
# the name the image gives, reduced: LibC asks for libc.so.6, so "c".  The
# names each library lists are in cmake/emscripten/ffi/symbols/<name>.txt:
# what Pharo 12 and Pharo 15 bind, which packaging/emscripten/tools/
# ffi-symbols.st lists, with some more.  The registries are the static
# libraries pharo_ffi_registry_node, linked into pharo, and
# pharo_ffi_registry_web, linked into pharo-web.

include(${CMAKE_CURRENT_LIST_DIR}/ffiRegistry.cmake)

# The C library.  Its aliases are the other names the image loads it by:
# libm, libdl (DynamicLoader), libpthread, librt, and LibC (the module
# #LibC of UnixPlatform>>getErrorMessage:, the call of strerror).  The
# headers are those of what the images call, and of the functions listed
# beside them (the C library, the allocator, the math library: libc.a,
# libdlmalloc.a and Emscripten's JavaScript).  dlopen and dlsym are in the
# table: Emscripten defines them, and they fail at run time without a
# MAIN_MODULE, which UnixDynamicLoader then reports.
#
# WASM_FFI_LIBC_ALL (a developer option of cmake/emscripten/deps/
# options.cmake) lists every function of the C library that its headers
# declare (llvm-nm of libc.a, libdlmalloc.a and libstubs.a), at about
# +100 KB of gzipped wasm, for trying out callouts the list lacks; setjmp and
# longjmp, the internal names and Emscripten's own are left out.
set(PHARO_WASM_LIBC_ARCHIVES "")
if(WASM_FFI_LIBC_ALL)
    set(PHARO_WASM_LIBC_DIR "${EMSCRIPTEN_SYSROOT}/lib/wasm64-emscripten")
    foreach(archive libc.a libdlmalloc.a libstubs.a)
        if(NOT EXISTS "${PHARO_WASM_LIBC_DIR}/${archive}")
            message(FATAL_ERROR "No ${PHARO_WASM_LIBC_DIR}/${archive}: build it with Emscripten's embuilder, or set WASM_FFI_LIBC_ALL=OFF")
        endif()
        list(APPEND PHARO_WASM_LIBC_ARCHIVES "${PHARO_WASM_LIBC_DIR}/${archive}")
    endforeach()
    set(PHARO_WASM_LIBC_ARCHIVES
        ARCHIVES ${PHARO_WASM_LIBC_ARCHIVES}
        EXCLUDE "^(_.*|setjmp|longjmp|sigsetjmp|siglongjmp|emscripten_.*)$")
endif()
pharo_wasm_ffi_library(c
    ALIASES m dl pthread rt LibC
    CFLAGS -D_GNU_SOURCE
    HEADERS
        stdlib.h string.h strings.h stdio.h unistd.h fcntl.h errno.h
        time.h sys/time.h sys/stat.h sys/types.h dirent.h math.h ctype.h
        wctype.h wchar.h locale.h signal.h inttypes.h sys/utsname.h pwd.h
        grp.h poll.h sys/select.h termios.h sys/ioctl.h sys/mman.h
        sys/resource.h sys/wait.h glob.h fnmatch.h libgen.h dlfcn.h
    SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/c.txt
    ${PHARO_WASM_LIBC_ARCHIVES})

# FreeType (FT2FFILibrary, libfreetype.so.6), pharo_freetype of
# cmake/emscripten/deps/freetype.cmake, with PHARO_WASM_HAS_FREETYPE
# (options.cmake).  The table lists the functions the images bind; the
# headers declare them (freetype.h and ftoutln.h), with the bitmap,
# multiple-master and synthesis APIs of FreeType next to them.
if(PHARO_WASM_HAS_FREETYPE)
    pharo_wasm_ffi_library(freetype
        HEADERS
            ft2build.h freetype/freetype.h freetype/ftoutln.h
            freetype/ftbitmap.h freetype/ftmm.h freetype/ftsynth.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/freetype.txt
        LINK pharo_freetype)
endif()

# cairo (CairoLibrary, libcairo.so.2), pharo_cairo of
# cmake/emscripten/deps/cairo.cmake, with PHARO_WASM_HAS_CAIRO
# (options.cmake).  The table lists the functions Athens binds, which
# cairo.h, cairo-ft.h and cairo-svg.h declare, and cairo-pdf.h with the PDF
# surface (WASM_CAIRO_PDF, the 'unless' section of cairo.txt).  CairoLibrary
# looks for the file libcairo.so.2 before it loads the library
# (FFIUnix64LibraryFinder): the staging writes it, empty.
if(PHARO_WASM_HAS_CAIRO)
    set(PHARO_WASM_CAIRO_HEADERS cairo.h cairo-ft.h cairo-svg.h)
    if(PHARO_WASM_HAS_CAIRO_PDF)
        list(APPEND PHARO_WASM_CAIRO_HEADERS cairo-pdf.h)
    endif()
    pharo_wasm_ffi_library(cairo
        HEADERS ${PHARO_WASM_CAIRO_HEADERS}
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/cairo.txt
        LINK pharo_cairo
        FILES libcairo.so.2)
endif()

# libgit2 (LGitLibrary, libgit2.so.1.4.4), pharo_git2 of
# cmake/emscripten/deps/libgit2.cmake, with PHARO_WASM_HAS_LIBGIT2
# (options.cmake).  The table lists the functions Iceberg binds, which git2.h
# declares (with the deprecated names of git2/deprecated.h: giterr_last), and
# git2/sys/transport.h and git2/sys/diff.h.  LGitLibrary looks for the file
# libgit2.so.1.4.4 before it loads the library: the staging writes it,
# empty.  The first load calls pharoWasmGitInit (src/emscripten/gitSupport.c),
# which initialises libgit2 once and registers its HTTP transport.
if(PHARO_WASM_HAS_LIBGIT2)
    pharo_wasm_ffi_library(git2
        HEADERS git2.h git2/sys/transport.h git2/sys/diff.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/git2.txt
        LINK pharo_git2
        FILES libgit2.so.1.4.4
        ON_LOAD pharoWasmGitInit)
endif()

# SDL2 (the SDL2 class of OSWindow-SDL2, libSDL2-2.0.so.0), pharo_sdl2 of
# cmake/emscripten/deps/sdl2.cmake, with PHARO_WASM_HAS_SDL2 (options.cmake).
# The table lists the functions OSWindow-SDL2 binds, and SDL_PushEvent, for
# the tests that give the world events of their own; SDL.h declares them,
# and SDL_syswm.h SDL_GetWindowWMInfo.  The SDL2 class looks for the file
# libSDL2-2.0.so.0 before it loads the library: the staging writes it,
# empty.
if(PHARO_WASM_HAS_SDL2)
    pharo_wasm_ffi_library(SDL2
        HEADERS SDL.h SDL_syswm.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/SDL2.txt
        LINK pharo_sdl2
        FILES libSDL2-2.0.so.0)
endif()

# The library of the FFI tests (ffiTestLibrary, libTestLibrary.so to the
# image), compiled where its table is: it has no header that declares its
# functions.  It is in the node VM, for lane 58 (tests/wasm/lanes/58-ffi.sh)
# and the TF* tests of the image, and in the web VM with
# WASM_FFI_TEST_LIBRARY (options.cmake).  callbackFromAnotherThread fails,
# since there are no threads.
set(PHARO_WASM_TEST_LIBRARY_NODE_ONLY NODE_ONLY)
if(WASM_FFI_TEST_LIBRARY)
    set(PHARO_WASM_TEST_LIBRARY_NODE_ONLY "")
endif()
file(GLOB PHARO_WASM_TEST_LIBRARY_SOURCES ${CMAKE_CURRENT_SOURCE_DIR}/ffiTestLibrary/src/*.c)
pharo_wasm_ffi_library(TestLibrary
    SOURCES ${PHARO_WASM_TEST_LIBRARY_SOURCES}
    CFLAGS -I${CMAKE_CURRENT_SOURCE_DIR}/ffiTestLibrary/includes
    ${PHARO_WASM_TEST_LIBRARY_NODE_ONLY})

pharo_wasm_ffi_registry()
