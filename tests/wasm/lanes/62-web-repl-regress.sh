#!/bin/sh
# 62-web-repl-regress - regression checks of the REPL of the Console page, in node
#
# web-repl-regress.js runs build-wasm/web/pharo-web.js through
# packaging/emscripten/web/vm-driver.js, on copies of the stock image with
# the REPL of packaging/emscripten/st/web-repl.st, as lane 60 does: a Stop
# of an evaluation that did not start yet, a Warning that nothing handles,
# and where a syntax error is.  An engine-level failure on its stderr fails
# the lane too.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
WASM_DIR=${WASM_DIR:-$SRCDIR/build-wasm}
TEST_DIR=${TEST_DIR:-$WASM_DIR/tests-run}
NODE=${NODE:-node}

for f in "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
        echo "FAIL 62-web-repl-regress: no $f (make wasm)"
        exit 1
    fi
done
mkdir -p "$TEST_DIR"
if command -v timeout >/dev/null 2>&1; then limit="timeout 600"; else limit=; fi

err=$TEST_DIR/62-web-repl-regress.err
status=0
$limit "$NODE" "$SRCDIR/tests/wasm/web-repl-regress.js" "$WASM_DIR/web" "$WASM_DIR/image/stock" 2>"$err" || status=1
if grep -E 'RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall' "$err" >/dev/null; then
    echo "FAIL 62-web-repl-regress: engine error on stderr"
    status=1
fi
cat "$err" >&2
exit $status
