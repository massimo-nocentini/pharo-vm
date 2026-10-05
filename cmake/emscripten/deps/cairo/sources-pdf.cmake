# The sources that cairo 1.18.4 adds to those of sources.cmake with zlib,
# relative to the archive's src/ directory: the PDF, PostScript and script
# surfaces of its meson build with zlib (cmake/emscripten/deps/cairo/README
# says how it ran), which cmake/emscripten/deps/cairo.cmake compiles with
# config-pdf.h and cairo-features-pdf.h under WASM_CAIRO_PDF.  (That build
# also makes the library of cairo's script interpreter, which no image
# uses, and which is not built.)
set(PHARO_WASM_CAIRO_PDF_SOURCES
    cairo-pdf-interchange.c
    cairo-pdf-surface.c
    cairo-ps-surface.c
    cairo-script-surface.c)
