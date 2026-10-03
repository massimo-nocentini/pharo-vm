#!/bin/sh
# 10-types - types of the interpreter proxy and of direct calls
#
# 1. check-types.sh on the generated sources the VM was built from: the
#    VirtualMachine slots and the prototypes of sqVirtualMachine.c, client.c
#    and sqExternalSemaphores.c must match the interpreter exactly, or an
#    indirect call traps in WebAssembly.
# 2. The links: wasm-ld only warns 'function signature mismatch' for a direct
#    call whose prototype differs from the definition, and links a stub that
#    traps.  Both link commands, as the wasm CMake tree (WASM_DIR/cmake)
#    records them, must carry -Wl,--fatal-warnings, which makes the warning a
#    link error: CMakeFiles/<target>.dir/link.txt with Unix Makefiles, the
#    target's link statement in build*.ninja with Ninja.  No tree, another
#    generator or a missing command fails the lane.  And no module may
#    contain such a stub (its name survives only in unoptimised builds that
#    keep names, such as Debug ones).
#
# Environment (from make wasm-check): GEN, WASM_DIR, SRCDIR, TEST_DIR.
# check-types.sh compiles with emcc -m64 (from PATH or EMSDK), or with
# WASM_CC when set; an exported CC or CFLAGS, usually for the native
# compiler, is ignored.
# Exits 77 when GEN is unset or no emcc is found.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
WASM_DIR=${WASM_DIR:-$SRCDIR/build-wasm}

if test -z "$GEN"; then
    echo "skip: GEN does not name the generated sources"
    exit 77
fi
if test -n "$WASM_CC"; then
    cc=$WASM_CC
elif command -v emcc >/dev/null 2>&1; then
    cc="emcc -m64"
elif test -n "$EMSDK" && test -x "$EMSDK/upstream/emscripten/emcc"; then
    cc="$EMSDK/upstream/emscripten/emcc -m64"
else
    echo "skip: no emcc on PATH or in EMSDK (or set WASM_CC)"
    exit 77
fi
test -z "$TEST_DIR" || { mkdir -p "$TEST_DIR"; TMPDIR=$TEST_DIR; export TMPDIR; }

failures=0

GEN=$GEN SRCDIR=$SRCDIR CC=$cc CFLAGS= sh "$SRCDIR/tests/wasm/check-types.sh" ||
    failures=$((failures + 1))

# The link command of target $1 as the wasm CMake tree records it
link_command() {
    case $generator in
        *Makefiles*)
            f=$(find "$WASM_DIR/cmake" -path "*/CMakeFiles/$1.dir/link.txt" 2>/dev/null | head -n 1)
            test -z "$f" || cat "$f" ;;
        *Ninja*)
            # build <outputs>: C_EXECUTABLE_LINKER__<target>[_<config>] <inputs>
            # and the indented variables (FLAGS, LINK_FLAGS, ...) after it
            for f in "$WASM_DIR"/cmake/build*.ninja; do
                test -f "$f" || continue
                awk -v rule="^C_EXECUTABLE_LINKER__$1(_[A-Za-z]*)?\$" '
                    /^build / { found = 0; for (i = 2; i <= NF; i++) if ($i ~ rule) found = 1 }
                    /^build / || /^  / { if (found) print; next }
                    { found = 0 }' "$f"
            done ;;
    esac
}

generator=
if test -f "$WASM_DIR/cmake/CMakeCache.txt"; then
    generator=$(sed -n 's/^CMAKE_GENERATOR:INTERNAL=//p' "$WASM_DIR/cmake/CMakeCache.txt")
fi
case $generator in
    '')
        echo "FAIL 10-types: no wasm CMake tree in $WASM_DIR/cmake, so the links cannot be checked"
        failures=$((failures + 1)) ;;
    *Makefiles*|*Ninja*)
        for target in pharo pharo-web; do
            link=$(link_command $target)
            if test -z "$link"; then
                echo "FAIL 10-types: no link command for $target in the $generator tree $WASM_DIR/cmake"
                failures=$((failures + 1))
            elif printf '%s\n' "$link" | grep -q -e '--fatal-warnings'; then
                echo "10-types: $target links with --fatal-warnings ($generator)"
            else
                echo "FAIL 10-types: $target links without -Wl,--fatal-warnings ($generator tree $WASM_DIR/cmake)"
                failures=$((failures + 1))
            fi
        done ;;
    *)
        echo "FAIL 10-types: cannot read the link commands of a '$generator' tree ($WASM_DIR/cmake)"
        failures=$((failures + 1)) ;;
esac

for module in "$WASM_DIR/node/pharo.wasm" "$WASM_DIR/web/pharo-web.wasm"; do
    test -f "$module" || continue
    stubs=$(grep -a -o 'signature_mismatch:[A-Za-z0-9_$]*' "$module" | sort -u)
    if test -n "$stubs"; then
        echo "FAIL 10-types: $module calls through signature mismatch stubs:" $stubs
        failures=$((failures + 1))
    else
        echo "10-types: no signature mismatch stub in $module"
    fi
done

test "$failures" -eq 0
