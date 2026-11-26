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
# headers declare them (freetype.h and ftoutln.h, and tttables.h for
# Alexandrie's FT_Get_Sfnt_Table), with the bitmap, multiple-master and
# synthesis APIs of FreeType next to them.  Alexandrie's AeFreeTypeLibrary
# looks for the file libfreetype.so.6 before it loads the library
# (FFIUnix64LibraryFinder): the staging writes it, empty.
if(PHARO_WASM_HAS_FREETYPE)
    pharo_wasm_ffi_library(freetype
        HEADERS
            ft2build.h freetype/freetype.h freetype/ftoutln.h
            freetype/ftbitmap.h freetype/ftmm.h freetype/ftsynth.h
            freetype/tttables.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/freetype.txt
        LINK pharo_freetype
        FILES libfreetype.so.6)
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

# Pango (PHARO_WASM_HAS_PANGO, cmake/emscripten/deps/pango.cmake): the
# libraries of libpangocairo.st, GLibLibrary (libglib-2.0.so.0, so glib),
# GObjectLibrary (gobject), PangoLibrary (pango), PangoCairoLibrary
# (pangocairo), PangoFontconfigLibrary (libfontconfig.so.1, fontconfig) and
# PangoFt2Library (pangoft2), with the functions of their lists, and
# harfbuzz, which no class loads by name, for the instance check of the
# bindings (below).  Their placeholders are the sonames the bindings look
# for first.  fontconfig and pangoft2 serve PangoImageFonts, which adds the
# fonts of the image to fontconfig (FcConfigAppFontAddFile) and makes Pango
# forget its cached fonts (pango_fc_font_map_config_changed).
#
# DEPENDS: a library looks up what its table lacks in the libraries it
# depends on, as dlsym does on Linux, where the bindings' GLibInstanceCheck
# looks glib_check_version, g_free and g_object_unref (and hb_version_atleast
# and cairo_create) up through libpango, libpangocairo and libpoppler-glib to
# check that they bind the GLib, GObject, HarfBuzz and cairo of GLibLibrary,
# GObjectLibrary and CairoLibrary: here they do, the VM has one of each.
# pharoWasmTextInit (src/emscripten/textSupport.c) gives fontconfig the
# fonts of the VM before Pango, Poppler or fontconfig itself first runs.
if(PHARO_WASM_HAS_PANGO)
    pharo_wasm_ffi_library(glib
        HEADERS glib.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/glib.txt
        LINK pharo_glib
        FILES libglib-2.0.so.0)
    pharo_wasm_ffi_library(gobject
        HEADERS glib-object.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/gobject.txt
        LINK pharo_gobject
        FILES libgobject-2.0.so.0
        DEPENDS glib)
    # (hb-ft.h and hb-ot.h, and the file libharfbuzz.so.0, for Alexandrie's
    # AeHarfbuzzLibrary, which looks for it before it loads the library)
    pharo_wasm_ffi_library(harfbuzz
        HEADERS hb.h hb-ft.h hb-ot.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/harfbuzz.txt
        LINK pharo_harfbuzz
        FILES libharfbuzz.so.0)
    pharo_wasm_ffi_library(pango
        HEADERS pango/pango.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/pango.txt
        LINK pharo_pango
        FILES libpango-1.0.so.0
        ON_LOAD pharoWasmTextInit
        DEPENDS gobject glib harfbuzz)
    pharo_wasm_ffi_library(pangocairo
        HEADERS pango/pangocairo.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/pangocairo.txt
        LINK pharo_pango
        FILES libpangocairo-1.0.so.0
        ON_LOAD pharoWasmTextInit
        DEPENDS pango gobject glib harfbuzz cairo)
    pharo_wasm_ffi_library(fontconfig
        HEADERS fontconfig/fontconfig.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/fontconfig.txt
        LINK pharo_fontconfig
        FILES libfontconfig.so.1
        ON_LOAD pharoWasmTextInit)
    pharo_wasm_ffi_library(pangoft2
        HEADERS pango/pangofc-fontmap.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/pangoft2.txt
        LINK pharo_pango
        FILES libpangoft2-1.0.so.0
        ON_LOAD pharoWasmTextInit
        DEPENDS pango gobject glib harfbuzz fontconfig)
endif()

# Poppler (PHARO_WASM_HAS_POPPLER, cmake/emscripten/deps/poppler.cmake):
# PopplerLibrary of libpoppler.st, libpoppler-glib.so.8, which the VM
# reduces to poppler-glib (an alias: the name of a table is a C name).
if(PHARO_WASM_HAS_POPPLER)
    pharo_wasm_ffi_library(poppler_glib
        ALIASES poppler-glib
        HEADERS poppler.h
        SYMBOLS_FILE ${CMAKE_CURRENT_LIST_DIR}/ffi/symbols/poppler-glib.txt
        LINK pharo_poppler_glib
        FILES libpoppler-glib.so.8
        ON_LOAD pharoWasmTextInit
        DEPENDS gobject glib cairo)
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
