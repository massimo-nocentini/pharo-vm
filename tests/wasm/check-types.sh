#!/bin/sh
# check-types.sh - single translation unit type check of the interpreter proxy
#
# src/common/sqVirtualMachine.c is compiled apart from the generated
# interpreter, so no compiler ever compares its prototypes with the
# interpreter's definitions, nor the functions it stores in the
# VirtualMachine struct with the slot types of virtualMachine.h.  Natively
# a mismatch is silent undefined behaviour; in WebAssembly an indirect call
# whose type differs from the callee's traps.  This script compiles the
# interpreter and the files that declare its functions themselves
# (sqVirtualMachine.c, client.c and sqExternalSemaphores.c) as one
# translation unit, which makes every such mismatch a compile error
# ('conflicting types' or 'incompatible function pointer types').
#
# Environment:
#   GEN     the generated sources: the directory holding vm/src/gcc3x-interp.c
#           (or gcc3x-cointerp.c), e.g. build-wasm/host/generated/64; required
#   CC      a clang or emcc command; default: emcc -m64 (an exported native
#           clang works too, the LP64 types being the same)
#   CFLAGS  extra compiler flags
#   SRCDIR  the source tree; default: the tree holding this script
#
# Nothing is written outside a temporary directory.  Prints the number of
# errors, and exits 1 when there is any, or when the compiler fails
# otherwise; exits 2 when it cannot check (no GEN, a GEN that is not a
# directory, no interpreter in it, or a CC that is not clang).

set -e

if test -z "$GEN"; then
    echo "check-types: GEN must name the generated sources (the directory holding vm/src)" >&2
    exit 2
fi
if ! test -d "$GEN" || ! gen=$(cd "$GEN" 2>/dev/null && pwd); then
    echo "check-types: GEN ($GEN) is not a directory it can enter: it must name the generated sources (the directory holding vm/src)" >&2
    exit 2
fi
GEN=$gen
SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../..}" && pwd)
CC=${CC:-emcc -m64}

interp=$GEN/vm/src/gcc3x-interp.c
test -f "$interp" || interp=$GEN/vm/src/gcc3x-cointerp.c
if ! test -f "$interp"; then
    echo "check-types: no gcc3x-interp.c or gcc3x-cointerp.c in $GEN/vm/src" >&2
    exit 2
fi
# The options below are clang's (gcc rejects -Werror=incompatible-function-
# pointer-types, which would look like a type error), and so is the form of
# the diagnostics that are counted.
macros=$($CC -dM -E - </dev/null 2>/dev/null) || macros=
case $macros in
    *__clang__*) ;;
    *)
        echo "check-types: CC must be clang or emcc, not '$CC'" >&2
        exit 2 ;;
esac

tmp=${TMPDIR:-/tmp}
work=$(mktemp -d "${tmp%/}/check-types.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 1' INT TERM

# Of the configured config.h only the widths of the C types matter here, and
# the compiler itself reports those.
sed -e 's|^#cmakedefine \([A-Za-z0-9_]*\).*|/* #undef \1 */|' \
    -e 's|@SIZEOF_INT@|__SIZEOF_INT__|' \
    -e 's|@SIZEOF_LONG@|__SIZEOF_LONG__|' \
    -e 's|@SIZEOF_LONG_LONG@|__SIZEOF_LONG_LONG__|' \
    -e 's|@SIZEOF_VOID_P@|__SIZEOF_POINTER__|' \
    -e 's|@ALWAYS_INTERACTIVE@|0|' \
    -e 's|@[A-Za-z_]*@|check-types|g' \
    "$SRCDIR/include/pharovm/config.h.in" >"$work/config.h"

cat >"$work/check-types.c" <<EOF
/* The generated interpreter, and the files that declare its functions, as
   one translation unit */
#include "$interp"
#include "$SRCDIR/src/common/sqVirtualMachine.c"
#include "$SRCDIR/src/client.c"
#include "$SRCDIR/src/common/sqExternalSemaphores.c"
EOF

# The include path of the VM build; the Emscripten platform headers come
# first when the compiler targets Emscripten.
set -- -I"$SRCDIR/include" -I"$SRCDIR/include/pharovm" \
    -I"$SRCDIR/include/pharovm/common" -I"$work" -I"$GEN/vm/include"
case $macros in
    *__EMSCRIPTEN__*)
        if test -d "$SRCDIR/include/pharovm/emscripten"; then
            set -- "$@" -I"$SRCDIR/include/pharovm/emscripten"
        fi ;;
esac
set -- "$@" -I"$SRCDIR/include/pharovm/unix"

status=0
$CC -fsyntax-only -ferror-limit=0 -w -Wno-int-conversion \
    -Werror=incompatible-function-pointer-types \
    -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -D_FILE_OFFSET_BITS=64 \
    -DLSB_FIRST=1 -DUSE_INLINE_MEMORY_ACCESSORS=1 \
    "$@" $CFLAGS "$work/check-types.c" >"$work/log" 2>&1 || status=$?
# Every error with a source location, all of them reported (-ferror-limit=0).
# A fatal error, such as a missing header, or an error of the compiler driver
# is no type error: it only fails the compiler.
errors=$(grep -c ':[0-9][0-9]*:[0-9][0-9]*: error:' "$work/log" || true)

tu="$(basename "$interp") + sqVirtualMachine.c, client.c, sqExternalSemaphores.c ($CC)"
if test "$status" -ne 0 || test "$errors" -ne 0; then
    cat "$work/log" >&2
    if test "$errors" -eq 0; then
        echo "check-types: $CC failed on $tu (status $status)" >&2
    else
        echo "check-types: $errors error(s) in $tu" >&2
    fi
    exit 1
fi
echo "check-types: 0 errors in $tu"
