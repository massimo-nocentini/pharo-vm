#!/bin/sh
# 80-world-harness - the keymap of the world page and the Pharo world, in node
#
# keymap.test.mjs checks packaging/emscripten/web/keymap.js, which turns the
# page's keyboard events into the SDL keycodes, scancodes, modifiers and
# text of the world image.  world-harness.mjs then boots the world image of
# build-wasm/web (manifest.json world: true) with build-wasm/web/pharo-web.js,
# packaging/emscripten/web/vm-driver.js and the memory framebuffer of
# display-worker.js, and plays the world page: the first frame, the menus,
# a Playground with typing and print it, the clipboard, a resize, a burst of
# events, Stop during a busy UI process and a save request, with the probe
# of tests/wasm/st/world-probe.st saying what the world shows.  It prints a
# table of timings; the median keystroke-to-present must be under 100 ms.
# An engine-level failure on its stderr, where it also writes what the
# emscripten runtime said, fails the lane too.  It then runs again on each
# other world image of the catalog (manifest.images, the extra images of
# WASM_EXTRA_IMAGES with world: true), with WORLD_IMAGE_ID, under the same
# checks.  Last, with a host Pharo,
# tests/wasm/st/osweb-native.st drives the image side natively, on a copy
# of the prepared image (build-wasm/image/web), whose primitives then fail
# and fall back.  Without a world image (WASM_WORLD=OFF, or no host Pharo
# to prepare it) only the keymap is checked, and the lane skips.
# keymap.test.mjs is stopped after WASM_CHECK_TIMEOUT (120) seconds, each
# run of the harness and each native run after 600.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR; and WASM_CHECK_TIMEOUT.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR, NODE and HOST_PHARO, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

echo "80-world-harness: keymap.test.mjs"
status=0
limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/keymap.test.mjs" || status=$?
if test "$status" -eq 124; then
    echo "FAIL 80-world-harness: keymap.test.mjs timed out after $timeout s"
fi
if test "$status" -ne 0; then exit 1; fi

manifest=$WASM_DIR/web/manifest.json
for f in pharo-web.js pharo-web.wasm manifest.json; do
    if ! test -f "$WASM_DIR/web/$f"; then
        echo "FAIL 80-world-harness: no $WASM_DIR/web/$f (make wasm)"
        exit 1
    fi
done
if ! grep -q '"world": *true' "$manifest"; then
    echo "skip: $manifest has no world image (WASM_WORLD=OFF, or no host Pharo to prepare it)"
    exit 77
fi

# harness ERR [ID]: world-harness.mjs on the default image, or on the image ID
# of the catalog, its stderr into ERR; sets status to 1 when it fails
harness() {
    harness_status=0
    limit 600 env WORLD_IMAGE_ID="${2:-}" "$NODE" "$SRCDIR/tests/wasm/world-harness.mjs" "$WASM_DIR/web" \
        2>"$1" || harness_status=$?
    if test "$harness_status" -eq 124; then
        echo "FAIL 80-world-harness: world-harness.mjs${2:+ ($2)} timed out after 600 s"
    fi
    if test "$harness_status" -ne 0; then status=1; fi
    if grep -E 'RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall' "$1" >/dev/null; then
        echo "FAIL 80-world-harness: engine error on stderr${2:+ ($2)}"
        status=1
    fi
    cat "$1" >&2
}

echo "80-world-harness: world-harness.mjs"
status=0
harness "$TEST_DIR/80-world-harness.err"

# The other world images of the catalog
ids=$("$NODE" -e '
const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"));
for (const i of m.images || []) if (!i.default && i.world) console.log(i.id);' "$manifest")
for id in $ids; do
    echo "80-world-harness: world-harness.mjs, image $id"
    harness "$TEST_DIR/80-world-harness-$id.err" "$id"
done

# The image side natively.  The script forces OSWebDriver and saves the image
# it runs in, so it gets a copy; the first run makes the copy boot through
# OSWebDriver (no SDL), the second one drives the world.  The stack of the
# interrupted processes goes to stderr, kept in the log.  (Pharo 15 takes a .st
# file through the st command only; Pharo 12 takes it so too.)
web_image=$WASM_DIR/image/web/Pharo-web.image
if test -z "$HOST_PHARO" || ! test -x "$HOST_PHARO"; then
    echo "80-world-harness: no HOST_PHARO: osweb-native.st is not run"
elif ! test -f "$web_image"; then
    echo "80-world-harness: no $web_image: osweb-native.st is not run"
else
    echo "80-world-harness: osweb-native.st, with $HOST_PHARO"
    native=$TEST_DIR/80-osweb-native
    rm -rf "$native"
    mkdir -p "$native"
    cp "$web_image" "$native/Pharo.image"
    cp "${web_image%.image}.changes" "$native/Pharo.changes"
    for f in "$WASM_DIR"/image/web/*.sources; do
        if test -f "$f"; then ln -s "$f" "$native/"; fi
    done
    script=$SRCDIR/tests/wasm/st/osweb-native.st
    native_status=0
    (cd "$native" &&
        SDL_VIDEODRIVER=dummy && export SDL_VIDEODRIVER &&
        limit 600 "$HOST_PHARO" --headless Pharo.image --no-default-preferences \
            st --save --quit "$script" &&
        limit 600 "$HOST_PHARO" --headless Pharo.image --no-default-preferences \
            --interactive st "$script") >"$native/stdout.log" 2>"$native/stderr.log" || native_status=$?
    if test "$native_status" -eq 0 && grep -q '^osweb-native: 0 failed$' "$native/stdout.log"; then
        grep -a '^ok\|^not ok\|^osweb-native' "$native/stdout.log"
        rm -f "$native/Pharo.image" "$native/Pharo.changes"
    else
        cat "$native/stdout.log"
        tail -40 "$native/stderr.log" >&2
        if test "$native_status" -eq 124; then
            echo "FAIL 80-world-harness: osweb-native.st timed out after 600 s (kept $native)"
        else
            echo "FAIL 80-world-harness: osweb-native.st (kept $native)"
        fi
        status=1
    fi
fi
exit $status
