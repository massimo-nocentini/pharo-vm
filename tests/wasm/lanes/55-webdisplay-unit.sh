#!/bin/sh
# 55-webdisplay-unit - the frame code and the primitives of WebDisplayPlugin
#
# tests/wasm/webdisplay-unit.c includes the plain C part of
# src/emscripten/plugins/WebDisplayPlugin.c and checks it: Form pixels to
# opaque RGBA, clipping, the merge of the dirty rectangles past 64, a new
# extent marking the frame all dirty, 1-bit cursors to RGBA, the event ring
# and the clipboard store.  It is built for wasm64, as the VM is, and run in
# node; it needs no VM build.
#
# Then, when build-wasm/node/pharo and the stock image are there, the node
# VM runs the primitives on bad arguments (the script written below): each
# must fail with the error code its comment gives, PrimErrBadArgument for
# an argument of the wrong class or out of range, and the good ones must
# succeed.  The node VM has no Module.webDisplay: the EM_JS glue of the
# primitives that reach it runs, finds no display and calls nothing on it.
#
# Each of the two runs, in node, is stopped after WASM_CHECK_TIMEOUT (120)
# seconds, which fails the lane.
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR; and
# WASM_CHECK_TIMEOUT.  Compiles with emcc -m64 (from PATH or EMSDK), or with
# WASM_CC when set; an exported CC or CFLAGS, usually for the native
# compiler, is ignored.  Exits 77 when no emcc is found.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
engine_errors='RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall'

if test -n "$WASM_CC"; then
    cc=$WASM_CC
elif command -v emcc >/dev/null 2>&1; then
    cc="emcc -m64"
elif test -n "$EMSDK" && test -x "$EMSDK/upstream/emscripten/emcc"; then
    cc="$EMSDK/upstream/emscripten/emcc -m64"
else
    echo "skip: no emcc on PATH or in EMSDK (or set WASM_CC)"
    exit 77
fi
# (run by hand without a TEST_DIR, in a temporary one)
if test -z "$TEST_DIR"; then
    TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/webdisplay-unit.XXXXXX")
    trap 'rm -rf "$TEST_DIR"' EXIT
fi
# (the defaults of WASM_DIR and NODE, timeout and limit)
. "$SRCDIR/tests/wasm/lib/common.sh"
out=$TEST_DIR/55-webdisplay-unit
rm -rf "$out"
mkdir -p "$out"

# timed_out WHAT: the message of a run that limit stopped (status 124)
timed_out() {
    if test "$status" -eq 124; then
        echo "FAIL 55-webdisplay-unit: $1 timed out after $timeout s"
        exit 1
    fi
}

echo "55-webdisplay-unit: $SRCDIR/src/emscripten/plugins/WebDisplayPlugin.c"
if ! $cc -O2 -Wall -Werror=implicit-function-declaration -sENVIRONMENT=node \
    -o "$out/webdisplay-unit.js" "$SRCDIR/tests/wasm/webdisplay-unit.c" \
    >"$out/build.log" 2>&1; then
    cat "$out/build.log"
    echo "FAIL 55-webdisplay-unit: webdisplay-unit.c does not build"
    exit 1
fi

status=0
limit "$timeout" "$NODE" "$out/webdisplay-unit.js" </dev/null 2>"$out/stderr.log" || status=$?
cat "$out/stderr.log" >&2
timed_out webdisplay-unit.js
if grep -Eq "$engine_errors" "$out/stderr.log"; then
    echo "FAIL 55-webdisplay-unit: the engine failed"
    exit 1
fi
if test "$status" -ne 0; then
    echo "FAIL 55-webdisplay-unit: exit status $status"
    exit 1
fi

# ---- the primitives' arguments, in the node VM

image=
for f in "$WASM_DIR"/image/stock/*.image; do
    if test -f "$f"; then image=$f; fi
done
if ! test -x "$WASM_DIR/node/pharo" || test -z "$image"; then
    echo "55-webdisplay-unit: no $WASM_DIR/node/pharo or stock image (make wasm): the primitives are not checked"
    exit 0
fi
echo "55-webdisplay-unit: the primitives' arguments, with $WASM_DIR/node/pharo"
run=$out/args
mkdir -p "$run"
cp "$image" "$run/Pharo.image"
cp "${image%.image}.changes" "$run/Pharo.changes"
for f in "$WASM_DIR"/image/stock/*.sources; do
    if test -f "$f"; then ln -s "$f" "$run/"; fi
done
cat >"$run/args.st" <<'EOF'
"Each primitive of WebDisplayPlugin on good and bad arguments.  A failing
 primitive answers {ec}: nil for the generic failure, else the name of its
 error code.  Prints a line per check, and the count."
| out cls checks failures check bits ro |
out := Stdio stdout.
checks := failures := 0.
cls := Object newAnonymousSubclass.
#( 'blit: b w: w h: h d: d l: l t: t r: r b: bb' #primitiveWebBlit
   'next: a' #primitiveWebNextEvent
   'open: e title: t' #primitiveWebOpenCanvas
   'title: t' #primitiveWebSetTitle
   'clip: t' #primitiveWebSetClipboardText
   'clip' #primitiveWebClipboardText
   'extent' #primitiveWebCanvasExtent
   'pixelExtent' #primitiveWebCanvasPixelExtent
   'cursor: b mask: m extent: e offset: o' #primitiveWebSetCursor
   'sema: i' #primitiveWebSetInputSemaphore
   'avail' #primitiveWebDisplayIsAvailable ) pairsDo: [ :pattern :name |
	cls class compile: pattern , ' <primitive: #' , name , ' module: #WebDisplayPlugin error: ec> ^ { ec }' ].
check := [ :label :want :block |
	| got |
	got := [ block value ] on: Error do: [ :e | 'error ' , e messageText ].
	got := got == cls
		ifTrue: [ 'self' ]
		ifFalse: [ got isArray ifTrue: [ 'fails ' , got first printString ] ifFalse: [ got printString ] ].
	checks := checks + 1.
	got = want
		ifTrue: [ out nextPutAll: 'ok - ' , label ]
		ifFalse: [
			failures := failures + 1.
			out nextPutAll: 'not ok - ' , label , ': got ' , got , ', want ' , want ].
	out lf; flush ].
bits := Bitmap new: 4 * 3.
check value: 'blit' value: 'self' value: [ cls blit: bits w: 4 h: 3 d: 32 l: 0 t: 0 r: 4 b: 3 ].
check value: 'blit, an empty rectangle' value: 'self' value: [ cls blit: bits w: 4 h: 3 d: 32 l: 3 t: 0 r: 1 b: 3 ].
check value: 'blit, depth 16' value: 'fails #''bad argument''' value: [ cls blit: bits w: 4 h: 3 d: 16 l: 0 t: 0 r: 4 b: 3 ].
check value: 'blit, too few bits' value: 'fails #''bad argument''' value: [ cls blit: bits w: 4 h: 4 d: 32 l: 0 t: 0 r: 4 b: 4 ].
check value: 'blit, a Float left' value: 'fails #''bad argument''' value: [ cls blit: bits w: 4 h: 3 d: 32 l: 0.5 t: 0 r: 4 b: 3 ].
check value: 'blit, a nil width' value: 'fails #''bad argument''' value: [ cls blit: bits w: nil h: 3 d: 32 l: 0 t: 0 r: 4 b: 3 ].
check value: 'blit, a LargeInteger bottom' value: 'fails #''bad argument''' value: [ cls blit: bits w: 4 h: 3 d: 32 l: 0 t: 0 r: 4 b: SmallInteger maxVal + 1 ].
check value: 'blit, bytes' value: 'fails #''bad argument''' value: [ cls blit: (ByteArray new: 48) w: 4 h: 3 d: 32 l: 0 t: 0 r: 4 b: 3 ].
check value: 'next event, none' value: 'false' value: [ cls next: (IntegerArray new: 8) ].
check value: 'next event, 7 elements' value: 'fails #''bad argument''' value: [ cls next: (IntegerArray new: 7) ].
ro := IntegerArray new: 8.
ro beReadOnlyObject.
check value: 'next event, read-only' value: 'fails #''no modification''' value: [ cls next: ro ].
check value: 'open' value: 'self' value: [ cls open: 10 @ 20 title: #[ 104 105 ] ].
check value: 'open, a Float x' value: 'fails #''bad argument''' value: [ cls open: 10.5 @ 20 title: #[ 104 ] ].
check value: 'open, a WideString title' value: 'fails #''bad argument''' value: [ cls open: 10 @ 20 title: (String with: (Character value: 300)) ].
check value: 'open, nil' value: 'fails #''bad argument''' value: [ cls open: nil title: #[  ] ].
check value: 'title' value: 'self' value: [ cls title: #[ 97 ] ].
check value: 'title, an integer' value: 'fails #''bad argument''' value: [ cls title: 3 ].
check value: 'clipboard text, none yet' value: 'nil' value: [ cls clip ].
check value: 'set clipboard text' value: 'self' value: [ cls clip: #[ 104 195 169 ] ].
check value: 'clipboard text, the copy' value: '#[104 195 169]' value: [ cls clip ].
check value: 'clipboard text, not consumed' value: '#[104 195 169]' value: [ cls clip ].
check value: 'set clipboard text, nil' value: 'fails #''bad argument''' value: [ cls clip: nil ].
check value: 'canvas extent, not reported' value: 'nil' value: [ cls extent ].
check value: 'canvas pixel extent, not reported' value: 'nil' value: [ cls pixelExtent ].
check value: 'cursor' value: 'self' value: [ cls cursor: (Bitmap new: 16) mask: (Bitmap new: 16) extent: 16 @ 16 offset: -3 @ -4 ].
check value: 'cursor, 257 wide' value: 'fails #''bad argument''' value: [ cls cursor: (Bitmap new: 9 * 257) mask: (Bitmap new: 9 * 257) extent: 257 @ 257 offset: 0 @ 0 ].
check value: 'cursor, a short mask' value: 'fails #''bad argument''' value: [ cls cursor: (Bitmap new: 16) mask: (Bitmap new: 15) extent: 16 @ 16 offset: 0 @ 0 ].
check value: 'input semaphore 0' value: 'self' value: [ cls sema: 0 ].
check value: 'input semaphore 2^31 - 1' value: 'self' value: [ cls sema: 16r7FFFFFFF ].
check value: 'input semaphore 2^31' value: 'fails #''bad argument''' value: [ cls sema: 16r80000000 ].
check value: 'input semaphore 2^40' value: 'fails #''bad argument''' value: [ cls sema: 1 << 40 ].
check value: 'input semaphore -1' value: 'fails #''bad argument''' value: [ cls sema: -1 ].
check value: 'input semaphore nil' value: 'fails #''bad argument''' value: [ cls sema: nil ].
check value: 'input semaphore 0 again' value: 'self' value: [ cls sema: 0 ].
check value: 'no display in node' value: 'false' value: [ cls avail ].
out nextPutAll: 'webdisplay-args: ' , checks printString , ' checks, ' , failures printString , ' failed'; lf; flush.
EOF

status=0
(cd "$run" && limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences \
    st --quit args.st </dev/null >"$run/stdout.log" 2>"$run/stderr.log") || status=$?
cat "$run/stdout.log"
cat "$run/stderr.log" >&2
timed_out "the node VM on the primitives' checks"
if grep -Eq "$engine_errors" "$run/stderr.log"; then
    echo "FAIL 55-webdisplay-unit: the engine failed in the primitives' checks"
    exit 1
fi
if test "$status" -ne 0 || grep -q '^not ok' "$run/stdout.log" ||
    ! grep -Eq '^webdisplay-args: [0-9]+ checks, 0 failed$' "$run/stdout.log"; then
    echo "FAIL 55-webdisplay-unit: the primitives' checks (exit status $status)"
    exit 1
fi
rm -f "$run/Pharo.image" "$run/Pharo.changes"
