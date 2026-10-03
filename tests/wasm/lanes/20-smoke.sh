#!/bin/sh
# 20-smoke - the node CLI against the smoke checks S1-S21 (wasm-smoke.sh)
#
# build-wasm/node/pharo, the VM for node (memory64), on fresh copies of the
# stock image: exact stdout of eval and st, the platform it reports, files,
# environment, Delays and preemption, deep recursion, a growing heap, image
# save and reload in wasm and natively, exit statuses, time zones, the
# builtin plugins, the slices of the return-to-host driver and the
# placement of old space.  S14b, the reload on a native VM, needs
# HOST_PHARO; without it, it is skipped, with the reason.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR; and WASM_CHECK_TIMEOUT and WASM_CHECK_TRIES (see wasm-smoke.sh).

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/20-smoke}
export TEST_DIR

echo "20-smoke: ${WASM_DIR:-$SRCDIR/build-wasm}/node/pharo"
exec sh "$SRCDIR/tests/wasm/wasm-smoke.sh" default
