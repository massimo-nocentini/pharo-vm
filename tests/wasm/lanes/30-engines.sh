#!/bin/sh
# 30-engines - the smoke checks that stress the engine, on the worst ones
#
# S2 (eval '3+4'), S8 (deep recursion) and S9 (a growing heap) of
# wasm-smoke.sh again: in Liftoff code only, whose frames are the largest,
# and with a 900 KB stack (NODE_OPTIONS_WASM='--liftoff-only
# --stack-size=900'), then in a worker thread of 1 MB of stack
# (in-worker.js), the stack of a browser worker, in Liftoff code only too.
# In both, S2t checks that V8 compiled no function with TurboFan, and S8
# allows 1400 ms rather than 1000 for the slower Liftoff code.  (The plan's
# --liftoff --no-wasm-tier-up does not stop V8's dynamic tiering, which
# then compiles the interpreter with TurboFan: see wasm-smoke.sh.)
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR; and
# WASM_CHECK_TIMEOUT and WASM_CHECK_TRIES (see wasm-smoke.sh).

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/30-engines}
export TEST_DIR

echo "30-engines: ${WASM_DIR:-$SRCDIR/build-wasm}/node/pharo"
exec sh "$SRCDIR/tests/wasm/wasm-smoke.sh" liftoff worker
