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
#   large   the same objects in the largest window of a 32-bit memory,
#           [1 GB, 2 GB), with PHARO_WASM_OLD_SPACE_BASE=0x40000000, which
#           they nearly fill (skipped when WASM_MAXIMUM_MEMORY is less than
#           2 GB);
#   limit   in a VM with a 32-bit memory (WASM_NODE_MEMORY64=2), whose maximum
#           is a page under 4 GB, 0x80000000 is refused when it starts, with
#           the message that says why;
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
large() ( PHARO_WASM_OLD_SPACE_BASE=0x40000000; export PHARO_WASM_OLD_SPACE_BASE; $P st --quit "$ST/oldspace-window.st" )
limit_of_memory() ( PHARO_WASM_OLD_SPACE_BASE=0x80000000; export PHARO_WASM_OLD_SPACE_BASE; $P eval 1 )
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

# the maximum memory of the module, in pages of 64 KB (WASM_MAXIMUM_MEMORY)
pages=$("$NODE" -e '
const b = require("fs").readFileSync(process.argv[1]); let p = 8;
const u = () => { let n = 0, k = 1, x; do { x = b[p++]; n += (x & 127) * k; k *= 128; } while (x & 128); return n; };
while (p < b.length) {
  const id = b[p++], n = u(), q = p;
  if (id === 5) { u(); if (b[p++] & 1) { u(); console.log(u()); } }
  p = q + n;
}' "$WASM_DIR/node/pharo.wasm")
if is_number "$pages" && test "$pages" -lt 32768; then
    skip_check large "its window ends at 2 GB, and the maximum memory is $((pages / 16)) MB (WASM_MAXIMUM_MEMORY)"
else
    fresh large
    if run large large; then
	size=$(sed -n '1s/.* //p' smoke.out)
	if test "$status" -ne 0; then
	    fail large "exit status $status"
	elif ! is_number "$size" || test "$size" -le 900000000 || test "$size" -gt 1073741824; then
	    fail large "expected old space of more than 900000000 and at most 1073741824 bytes, got '$(sed -n 1p smoke.out)'"
	elif test "x$(sed -n 2p smoke.out)" != "xbad slots: 0"; then
	    fail large "expected 'bad slots: 0', got '$(sed -n 2p smoke.out)'"
	else
	    ok large "old space of $size bytes"
	fi
    fi
fi

if grep -q '^WASM_NODE_MEMORY64:[A-Z]*=2$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    fresh limit
    expect_error limit 'Invalid PHARO_WASM_OLD_SPACE_BASE 0x80000000: old space needs a power of two .* at most half the maximum memory' limit_of_memory
else
    skip_check limit "the node VM has memory64 (WASM_NODE_MEMORY64=1)"
fi

fresh regrow
expect regrow '#(true true true)' regrow

fresh perm
expect perm "#(#'insufficient object memory' #'insufficient object memory' #'insufficient object memory' false)" perm

finish 22-old-space
