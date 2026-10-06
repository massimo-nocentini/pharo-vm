#!/bin/sh
# 70-worker-harness - the Web Worker of the pages, in node
#
# worker-harness.js runs build-wasm/web/vm-worker.js, as staged, in
# worker_threads Workers with a 1 MB stack, behind the shim of the worker
# globals of tests/wasm/lib/worker-shim.js, and plays the page: the protocol
# and its order (progress, ready, state), input before ready, Stop, the fs
# operations, output credit, exit and crash, the download of the manifest
# with and without gzip, the persistence of the image (save, restore without
# a fetch, the .changes, a failing store, upload, an upload with a .sources
# of its own, reset, two workers sharing the slot, a database that cannot be
# opened, the guarded writes of vm-storage.js), the M2 display extension
# point (init.display), and the notebook kernel (init.mode 'notebook',
# W-NB1..4: its requests, Stop, the credit of its events, and a slot it never
# stores).  On a build of the world it also checks the preparation of the
# stock image of image/stock (init.prepare, case 19) and the versions of
# OSWindow-Web (manifest.webPackage, cases 20-23, and 25-26, from version 2
# to 3, the AthensCairoSurface extension, and from 3 to 4, Iceberg's https://
# remotes), and, when manifest.libraries has some, the placeholders of the
# libraries of the FFI (24); a case whose build lacks what it needs says
# '# skip' and why.  An engine-level failure on its stderr fails the lane
# too.  The harness is stopped after 600 s.
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
