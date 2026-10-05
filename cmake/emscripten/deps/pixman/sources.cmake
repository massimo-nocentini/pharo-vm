# The sources of pixman 0.44.2 for the Emscripten VM, relative to the
# archive's pixman/ directory: those of its meson build without SIMD
# (cmake/emscripten/deps/cairo/README says how it ran), which
# cmake/emscripten/deps/pixman.cmake compiles.  pixman-arm.c, pixman-mips.c,
# pixman-ppc.c, pixman-riscv.c and pixman-x86.c add the SIMD implementations
# of their processors that pixman-config.h turns on (USE_SSE2, USE_ARM_NEON
# and the like): none.
set(PHARO_WASM_PIXMAN_SOURCES
    pixman-access-accessors.c
    pixman-access.c
    pixman-arm.c
    pixman-bits-image.c
    pixman-combine-float.c
    pixman-combine32.c
    pixman-conical-gradient.c
    pixman-edge-accessors.c
    pixman-edge.c
    pixman-fast-path.c
    pixman-filter.c
    pixman-general.c
    pixman-glyph.c
    pixman-gradient-walker.c
    pixman-image.c
    pixman-implementation.c
    pixman-linear-gradient.c
    pixman-matrix.c
    pixman-mips.c
    pixman-noop.c
    pixman-ppc.c
    pixman-radial-gradient.c
    pixman-region16.c
    pixman-region32.c
    pixman-riscv.c
    pixman-solid-fill.c
    pixman-timer.c
    pixman-trap.c
    pixman-utils.c
    pixman-x86.c
    pixman.c)
