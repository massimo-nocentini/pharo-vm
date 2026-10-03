#!/bin/sh
# 50-prim-audit - the builtin primitive tables of the linked node CLI
#
# prim-audit.mjs lists the rows the named-primitive lookup dispatches through
# (vm_list_builtin_primitives) and what the lookup of sqNamedPrims.c answers
# for each of them (vm_list_builtin_lookups), and checks them in
# build-wasm/node/pharo.wasm: a named-primitive lookup must answer the void
# trampolines' table, every primitive (a row with an accessor depth in the
# sources of build-wasm/cmake/wasm/prims, which every trampolined row must
# have) must be called through a () -> () function, every function pointer
# must be at least TABLE_BASE (1024), no function may be wrapped by
# -sEMULATE_FUNCTION_POINTER_CASTS, and the builtin modules must be the
# expected ones.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
WASM_DIR=${WASM_DIR:-$SRCDIR/build-wasm}

echo "50-prim-audit: $WASM_DIR/node/pharo.wasm"
exec "${NODE:-node}" "$SRCDIR/tests/wasm/prim-audit.mjs" --srcdir "$SRCDIR" --prims "$WASM_DIR/cmake/wasm/prims" \
    "$WASM_DIR/node/pharo.js"
