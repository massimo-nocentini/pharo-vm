#!/bin/sh
# 56-memory-unit - the Spur heap allocation of the WebAssembly VM
#
# tests/wasm/memory-unit.c includes src/emscripten/memoryEmscripten.c and
# drives it as Spur does (see its comment): fixed spaces, segments that grow
# again where freed ones were and come back zeroed, and old space that never
# leaves its window [base, 2 * base) nor reaches perm space; then, run with
# 'big', a first segment larger than the window.  It is built for wasm64 with
# the memory settings of the VM (growth up to 4GB), against a stub of
# pharovm/pharo.h written below, and run in node; it needs no VM build.
#
# Each run is stopped after WASM_CHECK_TIMEOUT (120) seconds, which fails the
# lane.
#
# Environment (from make wasm-check): NODE, SRCDIR, TEST_DIR; and
# WASM_CHECK_TIMEOUT.  Compiles with emcc -m64 (from PATH or EMSDK), or with
# WASM_CC when set; an exported CC or CFLAGS, usually for the native
# compiler, is ignored.  Exits 77 when no emcc is found.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
engine_errors='RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall'

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
# (run by hand without a TEST_DIR, in a temporary one)
if test -z "$TEST_DIR"; then
    TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/memory-unit.XXXXXX")
    trap 'rm -rf "$TEST_DIR"' EXIT
fi
# (the defaults of NODE, timeout and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"
out=$TEST_DIR/56-memory-unit
rm -rf "$out"
mkdir -p "$out/include/pharovm"
# (the window is the one of the build default, whatever the environment says)
unset PHARO_WASM_OLD_SPACE_BASE

# What memoryEmscripten.c takes from the VM's headers
cat >"$out/include/pharovm/pharo.h" <<'EOF'
/* pharo.h -- the stub of tests/wasm/lanes/56-memory-unit.sh */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
typedef long long sqInt;
typedef unsigned long long usqInt;
#define logError(...) (printf("logError: " __VA_ARGS__), printf("\n"))
#define logDebug(...) ((void)0)
EOF

echo "56-memory-unit: $SRCDIR/src/emscripten/memoryEmscripten.c"
if ! $cc -O1 -Wall -Werror=implicit-function-declaration -sENVIRONMENT=node \
    -sALLOW_MEMORY_GROWTH=1 -sINITIAL_MEMORY=32MB -sMAXIMUM_MEMORY=4GB \
    -I "$out/include" -o "$out/memory-unit.js" "$SRCDIR/tests/wasm/memory-unit.c" \
    >"$out/build.log" 2>&1; then
    cat "$out/build.log"
    echo "FAIL 56-memory-unit: memory-unit.c does not build"
    exit 1
fi

for mode in '' big; do
    status=0
    limit "$timeout" "$NODE" "$out/memory-unit.js" $mode </dev/null >"$out/stdout.log" 2>"$out/stderr.log" || status=$?
    grep -v '^logError: ' "$out/stdout.log" || true
    cat "$out/stderr.log" >&2
    if test "$status" -eq 124; then
        echo "FAIL 56-memory-unit: memory-unit.js $mode timed out after $timeout s"
        exit 1
    fi
    if grep -Eq "$engine_errors" "$out/stderr.log"; then
        echo "FAIL 56-memory-unit: the engine failed (memory-unit.js $mode)"
        exit 1
    fi
    if test "$status" -ne 0; then
        echo "FAIL 56-memory-unit: memory-unit.js $mode: exit status $status"
        exit 1
    fi
done
