#!/bin/sh
# 72-open-image - the Open of the pages (open-image.js), in node
#
# tests/wasm/open-image.test.mjs checks packaging/emscripten/web/open-image.js,
# which the Console and the world page run to open an image of the user's
# own: how it reads zips (deflated and stored entries, data descriptors,
# UTF-8 names, directories and the files that the Finder adds, zip64), how
# it pairs the .image with its .changes and .sources, what it refuses and
# why (not a zip or not a whole one, damaged or encrypted entries, other
# methods, a split zip, no image or several, a 32-bit image or none), and
# the drops of files on a page.  It zips and opens the stock image of the
# build too (WASM_DIR/image/stock), and so the zips of WASM_DIR/downloads
# and those that OPEN_ZIPS names (paths separated by colons, such as Pharo
# downloads of files.pharo.org).  It needs no VM, and is stopped after
# WASM_CHECK_TIMEOUT (120) seconds.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR; and
# WASM_CHECK_TIMEOUT and OPEN_ZIPS.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
# (the defaults of WASM_DIR, TEST_DIR, which it exports, and NODE, and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"

status=0
limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/open-image.test.mjs" || status=$?
if test "$status" -eq 124; then
    echo "FAIL 72-open-image: open-image.test.mjs timed out after $timeout s"
fi
if test "$status" -ne 0; then exit 1; fi
