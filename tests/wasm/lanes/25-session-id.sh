#!/bin/sh
# 25-session-id - a restored snapshot gets a VM session ID of its own
#
# tests/wasm/session-id.mjs saves an image in one session of the web VM
# (build-wasm/web/pharo-web.js, through vm-driver.js) and boots the
# snapshot in a second one, under a clock that puts both at the point
# where the VM sets its session ID in the same second.  With the old
# formula of StackInterpreter>>initializeGlobalSessionID (Unix seconds plus
# ioMSecs, the milliseconds since the start) the two sessions get the same
# ID, and the second one traps when it closes the first one's FILE*s of the
# source files.  The second session must boot, have another ID, refuse the
# first one's file handle and read method sources.  The first session quits
# right after writing a line it does not flush, which must still arrive
# (the C streams are flushed on exit).  An engine-level failure in what a
# VM printed or in what the emscripten runtime and the driver said fails a
# check of session-id.mjs, which copies the latter to its stderr; one there
# fails the lane too.  The harness is stopped after 300 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR and NODE, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

for f in "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
        echo "FAIL 25-session-id: no $f (make wasm)"
        exit 1
    fi
done

err=$TEST_DIR/25-session-id.err
status=0
limit 300 "$NODE" "$SRCDIR/tests/wasm/session-id.mjs" \
    "$WASM_DIR/web" "$WASM_DIR/image/stock" 2>"$err" || status=$?
if test "$status" -eq 124; then
    echo "FAIL 25-session-id: timed out after 300 s"
fi
if grep -E 'RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall' "$err" >/dev/null; then
    echo "FAIL 25-session-id: engine error on stderr"
    status=1
fi
cat "$err" >&2
exit $status
