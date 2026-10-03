#!/bin/sh
# 60-vm-harness - the web VM and the REPL of the Console page, in node
#
# vm-harness.js runs build-wasm/web/pharo-web.js (the MEMORY64=2 module of
# the pages) through packaging/emscripten/web/vm-driver.js, with copies of the
# stock image in MEMFS and the REPL of packaging/emscripten/st/web-repl.st on
# the driver's stdin device: prompts, results and errors (also of other
# processes), Stop, sleeping, stdin reads, output backpressure, the save
# notification and a boot of the saved image, exits and crashes, and host
# callbacks that throw.  It runs once with node's default engine, then once
# with Liftoff only and a 900 KB stack: --liftoff-only, as --liftoff
# --no-wasm-tier-up still lets V8's dynamic tiering move hot functions to
# TurboFan.  An engine-level failure on its stderr fails the lane too.
# Each run is stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR and NODE, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

for f in "$WASM_DIR/web/pharo-web.js" "$WASM_DIR/web/pharo-web.wasm"; do
    if ! test -f "$f"; then
        echo "FAIL 60-vm-harness: no $f (make wasm)"
        exit 1
    fi
done

failures=0
for engine in default liftoff; do
    if test $engine = liftoff; then flags="--liftoff-only --stack-size=900"; else flags=; fi
    echo "60-vm-harness: $engine engine${flags:+ ($flags)}"
    err=$TEST_DIR/60-vm-harness-$engine.err
    status=0
    limit 600 "$NODE" $flags "$SRCDIR/tests/wasm/vm-harness.js" \
        "$WASM_DIR/web" "$WASM_DIR/image/stock" 2>"$err" || status=$?
    if test "$status" -eq 124; then
        echo "FAIL 60-vm-harness: timed out after 600 s ($engine)"
    fi
    if test "$status" -ne 0; then
        failures=$((failures + 1))
    fi
    if grep -E 'RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall' "$err" >/dev/null; then
        echo "FAIL 60-vm-harness: engine error on stderr ($engine)"
        failures=$((failures + 1))
    fi
    cat "$err" >&2
done
test $failures = 0
