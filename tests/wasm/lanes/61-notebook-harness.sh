#!/bin/sh
# 61-notebook-harness - the notebook kernel and its client, in node
#
# notebook-lib.test.mjs checks the libraries of the Notebook tab
# (packaging/emscripten/web/notebook-st.js, notebook-lib.js and the frames
# and PNGs of nb-kernel.js), and, with HOST_PHARO, the Smalltalk scanner of
# notebook-st.js against the image's own (tests/wasm/st/nb-scan-corpus.st).
# notebook-harness.js runs the kernel of
# packaging/emscripten/st/web-notebook.st in build-wasm/web/pharo-web.js
# through packaging/emscripten/web/vm-driver.js, with its events device
# (Part K: requests, values, errors, Stop, output credit, encodings, rich
# output, background processes, snapshots, the end of stdin; Part F: the
# FFI, cairo and FreeType of the build), then the page's client,
# nb-kernel.js, against the real build-wasm/web/vm-worker.js in
# worker_threads (Part C: the queue, Stop and its watchdog, restarts,
# exits, floods, uploads, the slot of the Console).  It runs once with
# node's default engine, then once with Liftoff only and a 900 KB stack
# (--liftoff-only, as in lane 60), on the stock image, then on each image
# directory of NOTEBOOK_IMAGES (colon-separated, such as a Pharo 15 one,
# which Part C boots as the Console's slot).  An engine-level failure on
# its stderr fails the lane too.  Each run is stopped after 600 s.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR,
# HOST_PHARO; NOTEBOOK_IMAGES.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR and NODE, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

for f in pharo-web.js pharo-web.wasm vm-worker.js vm-driver.js vm-storage.js manifest.json st/web-notebook.st; do
    if ! test -f "$WASM_DIR/web/$f"; then
        echo "FAIL 61-notebook-harness: no $WASM_DIR/web/$f (make wasm)"
        exit 1
    fi
done

failures=0
errors='RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall'

images=$WASM_DIR/image/stock
old_ifs=$IFS
IFS=:
for dir in ${NOTEBOOK_IMAGES-}; do
    if test -n "$dir"; then images="$images:$dir"; fi
done
IFS=$old_ifs

n=0
IFS=:
for dir in $images; do
    IFS=$old_ifs
    n=$((n + 1))
    if ! test -d "$dir"; then
        echo "FAIL 61-notebook-harness: no image directory $dir"
        failures=$((failures + 1))
        continue
    fi
    echo "61-notebook-harness: the libraries, on $dir"
    err=$TEST_DIR/61-notebook-lib-$n.err
    status=0
    limit 600 "$NODE" "$SRCDIR/tests/wasm/notebook-lib.test.mjs" "$dir" 2>"$err" || status=$?
    if test "$status" -eq 124; then
        echo "FAIL 61-notebook-harness: notebook-lib.test.mjs timed out after 600 s"
    fi
    if test "$status" -ne 0; then
        failures=$((failures + 1))
    fi
    cat "$err" >&2
    for engine in default liftoff; do
        if test $engine = liftoff; then flags="--liftoff-only --stack-size=900"; else flags=; fi
        echo "61-notebook-harness: $engine engine${flags:+ ($flags)}, on $dir"
        err=$TEST_DIR/61-notebook-harness-$n-$engine.err
        status=0
        limit 600 "$NODE" $flags "$SRCDIR/tests/wasm/notebook-harness.js" \
            "$WASM_DIR/web" "$dir" 2>"$err" || status=$?
        if test "$status" -eq 124; then
            echo "FAIL 61-notebook-harness: timed out after 600 s ($engine, $dir)"
        fi
        if test "$status" -ne 0; then
            failures=$((failures + 1))
        fi
        if grep -E "$errors" "$err" >/dev/null; then
            echo "FAIL 61-notebook-harness: engine error on stderr ($engine, $dir)"
            failures=$((failures + 1))
        fi
        cat "$err" >&2
    done
    IFS=:
done
IFS=$old_ifs
test $failures = 0
