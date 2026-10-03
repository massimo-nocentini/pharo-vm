#!/bin/sh
# 82-osweb-windows - the windows of OSWebDriver, natively
#
# tests/wasm/st/osweb-windows.st drives the image side of the browser world on
# a copy of the prepared image (build-wasm/image/web), with a native Pharo
# VM (HOST_PHARO), as lane 80 does with osweb-native.st: a second OSWindow
# without an event handler leaves the world shown, and the window of the
# Emergency Debugger takes the canvas and the input, then gives them back.
# Skipped without HOST_PHARO or the prepared image.
#
# Environment (from make wasm-check): WASM_DIR, HOST_PHARO, SRCDIR, TEST_DIR.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
. "$SRCDIR/tests/wasm/lib/common.sh"

web_image=$WASM_DIR/image/web/Pharo12-web.image
if test -z "$HOST_PHARO" || ! test -x "$HOST_PHARO"; then
    skip "no HOST_PHARO, a native Pharo VM"
fi
if ! test -f "$web_image"; then
    skip "no $web_image (WASM_WORLD=OFF, or no host Pharo to prepare it)"
fi

dir=$TEST_DIR/82-osweb-windows
rm -rf "$dir"
mkdir -p "$dir"
cp "$web_image" "$dir/Pharo.image"
cp "${web_image%.image}.changes" "$dir/Pharo.changes"
for f in "$WASM_DIR"/image/web/*.sources; do
    if test -f "$f"; then ln -s "$f" "$dir/"; fi
done
script=$SRCDIR/tests/wasm/st/osweb-windows.st
echo "82-osweb-windows: osweb-windows.st, with $HOST_PHARO"
status=0
(cd "$dir" &&
    SDL_VIDEODRIVER=dummy && export SDL_VIDEODRIVER &&
    limit 300 "$HOST_PHARO" --headless Pharo.image --no-default-preferences \
        --save --quit "$script" force &&
    limit 300 "$HOST_PHARO" --headless Pharo.image --no-default-preferences \
        --interactive "$script") >"$dir/stdout.log" 2>"$dir/stderr.log" || status=$?
if test "$status" -eq 0 && grep -q '^osweb-windows: 0 failed$' "$dir/stdout.log"; then
    grep -a '^ok\|^not ok\|^osweb-windows' "$dir/stdout.log"
    rm -f "$dir/Pharo.image" "$dir/Pharo.changes"
    exit 0
fi
cat "$dir/stdout.log"
tail -40 "$dir/stderr.log" >&2
if test "$status" -eq 124; then
    echo "FAIL 82-osweb-windows: osweb-windows.st timed out (kept $dir)"
else
    echo "FAIL 82-osweb-windows: osweb-windows.st (kept $dir)"
fi
exit 1
