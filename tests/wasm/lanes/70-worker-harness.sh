#!/bin/sh
# 70-worker-harness - the Web Worker of the pages, in node
#
# worker-harness.js runs build-wasm/web/vm-worker.js, as staged, in
# worker_threads Workers with a 1 MB stack, behind a shim of the worker
# globals, and plays the page: the protocol and its order (progress, ready,
# state), input before ready, Stop, the fs operations, output credit, exit
# and crash, the download of the manifest with and without gzip, the
# persistence of the image (save, restore without a fetch, the .changes,
# a failing store, upload, reset, two workers sharing the slot, a database
# that cannot be opened) and the M2 display extension point.  An
# engine-level failure on its stderr fails the lane too.  The harness is
# stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR, which it exports, and NODE, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

for f in vm-worker.js vm-driver.js vm-storage.js pharo-web.js pharo-web.wasm manifest.json; do
    if ! test -f "$WASM_DIR/web/$f"; then
        echo "FAIL 70-worker-harness: no $WASM_DIR/web/$f (make wasm)"
        exit 1
    fi
done

err=$TEST_DIR/70-worker-harness.err
status=0
limit 600 "$NODE" "$SRCDIR/tests/wasm/worker-harness.js" "$WASM_DIR/web" 2>"$err" || status=$?
if test "$status" -eq 124; then
    echo "FAIL 70-worker-harness: timed out after 600 s"
fi
if test "$status" -ne 0; then status=1; fi
if grep -E 'RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall' "$err" >/dev/null; then
    echo "FAIL 70-worker-harness: engine error on stderr"
    status=1
fi
cat "$err" >&2
exit $status
