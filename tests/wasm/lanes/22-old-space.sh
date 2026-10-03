#!/bin/sh
# 22-old-space - old space stays in its window, and perm space is refused
#
# build-wasm/node/pharo, the VM for node, on fresh copies of the stock
# image (src/emscripten/memoryEmscripten.c, VMMemoryMap):
#
#   window  with PHARO_WASM_OLD_SPACE_BASE=0x10000000, old space is
#           [256 MB, 512 MB): oldspace-window.st fills it with 10 MB
#           ByteArrays, which must end in an OutOfMemory with old space no
#           larger than the window (an object past it would be neither young
#           nor old for the write barrier), then stores young Strings into
#           an Array at the top of old space and scavenges: every slot must
#           keep its String;
#   regrow  with the window of 512 MB, oldspace-regrow.st allocates
#           ByteArrays of 50, 100 and 150 MB, each one garbage collected
#           before the next: the segment of each must go where the freed one
#           was, at the top, or the last one does not fit;
#   perm    permspace.st: the primitives that move objects to perm space
#           (90, 91 and 93), which WebAssembly memory cannot reach, must
#           fail with PrimErrNoMemory instead of aborting the VM.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/22-old-space}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=old-space
ST=$SRCDIR/tests/wasm/st

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 22-old-space: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
echo "22-old-space: $WASM_DIR/node/pharo"

pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
P="pharo_run --headless Pharo.image --no-default-preferences"

window() ( PHARO_WASM_OLD_SPACE_BASE=0x10000000; export PHARO_WASM_OLD_SPACE_BASE; $P st --quit "$ST/oldspace-window.st" )
regrow() ( PHARO_WASM_OLD_SPACE_BASE=0x20000000; export PHARO_WASM_OLD_SPACE_BASE; $P st --quit "$ST/oldspace-regrow.st" )
perm() { $P st --quit "$ST/permspace.st"; }

fresh window
if run window window; then
    set -- $(sed -n 1p smoke.out)
    if test "$status" -ne 0; then
	fail window "exit status $status"
    elif test "x$1" != xOutOfMemory || ! is_number "$2" || test "$2" -gt 268435456; then
	fail window "expected 'OutOfMemory <at most 268435456>', got '$(sed -n 1p smoke.out)'"
    elif test "x$(sed -n 2p smoke.out)" != "xbad slots: 0"; then
	fail window "expected 'bad slots: 0', got '$(sed -n 2p smoke.out)'"
    else
	ok window "old space of $2 bytes"
    fi
fi

fresh regrow
expect regrow '#(true true true)' regrow

fresh perm
expect perm "#(#'insufficient object memory' #'insufficient object memory' #'insufficient object memory' false)" perm

finish 22-old-space
