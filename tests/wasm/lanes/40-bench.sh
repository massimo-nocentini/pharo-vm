#!/bin/sh
# 40-bench - the performance and regression guard (bench.sh)
#
# Prints the measures of the node CLI next to reference numbers.  It fails
# when 300k-deep recursion takes 1 s or more (the C asserts are on, as in a
# WASM_DEBUG=1 build), when eval '3+4' takes 1.5 s or more, or when more
# than 2 s of computing run in fewer than 10 slices; slow tinyBenchmarks
# rates only warn, unless WASM_BENCH_STRICT=1.  A time over its limit is
# measured again, up to WASM_CHECK_TRIES (3) times.  (It is no guard
# against emulated function pointer casts: a relink of the node CLI with
# -sEMULATE_FUNCTION_POINTER_CASTS passed it, 300k-deep recursion taking
# 173 ms.  Lane 50 catches them.)
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR; and
# the WASM_BENCH_* limits, WASM_CHECK_TIMEOUT and WASM_CHECK_TRIES of
# bench.sh.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/40-bench}
export TEST_DIR

echo "40-bench: ${WASM_DIR:-$SRCDIR/build-wasm}/node/pharo"
exec sh "$SRCDIR/tests/wasm/bench.sh"
