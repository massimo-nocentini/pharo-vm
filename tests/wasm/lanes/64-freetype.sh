#!/bin/sh
# 64-freetype - FreeType in the WebAssembly VM, in node
#
# build-wasm/node/pharo on fresh copies of the stock image and of the
# prepared image of the world (image/web, when the build has one) runs
# tests/wasm/st/ft-parity.st, which draws a line of text with the default
# font of the image through the image's own FreeType bindings (FT2Library,
# the library freetype of the registry), and prints what it found:
#
#   parity    every line of the golden file of the image's version,
#             tests/wasm/golden/ft-p<major>.txt, must be a line of what it
#             prints: TFFIBackend, a FreeTypeFont of Source Sans Pro 10,
#             FreeType 2.14.3, glyphs for U+03BB and U+2192, 688 pixels
#             that are not white, and the SHA256 of the bits of the Form.
#             The golden files are the reference; with HOST_PHARO, the same
#             script on the stock image, natively, is compared with them as
#             an advice only (a note, never a failure): the VM of another
#             platform ships another FreeType (cmake/importFreetype2.cmake);
#   hidden    the negative control, on the same image: with FreeType hidden
#             from the VM (PHARO_WASM_FFI_HIDE=freetype), the image must
#             fall back by itself (it has no startUp: hook for it) to a font
#             that is not a FreeTypeFont, and draw the text with it, without
#             a crash;
#   startup   a note of what FreeType costs a boot of the stock image (an
#             eval of 3 + 4, the best of 3 runs, with FreeType and with it
#             hidden), never a failure: lane 40 checks the time of a boot.
#
# Every run must print no line of the guard of the callouts ('FFI callout
# failed, its declaration does not match ...' or 'FFI callout trapped in
# ...'), nothing else on stderr, and write no PharoDebug.log.  Skipped when
# the build has no FreeType (WASM_FREETYPE=OFF, or WASM_FFI=OFF): its node
# VM's registry (cmake/wasm/ffi/ffiRegistry-node.c) has no library freetype.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, and WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/64-freetype}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=freetype

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 64-freetype: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF), so no FreeType"
fi
registry=$WASM_DIR/cmake/wasm/ffi/ffiRegistry-node.c
if ! test -f "$registry"; then
    echo "FAIL 64-freetype: no $registry, the registry of the node VM (make wasm)"
    exit 1
fi
if ! grep -q '^extern const PharoFFILibrary pharoFFILibrary_freetype;$' "$registry"; then
    skip "no FreeType: the registry of the node VM has no library freetype (WASM_FREETYPE=OFF)"
fi
echo "64-freetype: $WASM_DIR/node/pharo"

script=$SRCDIR/tests/wasm/st/ft-parity.st
golden_dir=$SRCDIR/tests/wasm/golden

# The lines of the guard of the callouts, and what the runtime of a debug
# build says of itself, as in lane 59: vm() keeps the guard's in guard.err,
# out of the stderr the checks see (the message of what threw may name an
# error of the JavaScript runtime, see engine_errors in lib/common.sh), and
# leaves the runtime's out; the rest of stderr goes on.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
runtime='^(Heap resize call from [0-9]+ to [0-9]+ took .* Success: true|program exited \(with status: [0-9]+\), but keepRuntimeAlive\(\) is set)'
vm() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -Ev "$guard|$runtime" vm.err >&2
    return $vm_status
}
parity() ( vm st --quit "$script" )
hidden() ( PHARO_WASM_FFI_HIDE=freetype; export PHARO_WASM_FFI_HIDE; vm st --quit "$script" )

# value KEY: the value of the line 'KEY: ...' of smoke.out
value() {
    sed -n "s/^$1: //p" smoke.out | head -n 1
}

# quiet NAME: fails NAME and answers false after a run that did not exit
# with 0, printed a line of the guard or anything else on stderr, or wrote
# PharoDebug.log
quiet() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -s guard.err; then
	fail "$1" "$(grep -c '' guard.err) failed callouts: $(head -n 1 guard.err)"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty: $(head -n 1 smoke.err | cut -c 1-200)"
    elif test -s PharoDebug.log; then
	fail "$1" "it wrote PharoDebug.log: $(grep -m 1 . PharoDebug.log | cut -c 1-200)"
    else
	return 0
    fi
    return 1
}

# golden NAME: smoke.out against the golden file of its major version;
# answers false, with the lines that differ in differ.txt, when one is not
# there
golden() {
    major=$(value major)
    golden=$golden_dir/ft-p$major.txt
    if ! test -f "$golden"; then
	fail "$1" "no golden file $golden for the Pharo of the image (major: '$major')"
	return 1
    fi
    : >differ.txt
    grep -v '^#' "$golden" | grep -v '^[ 	]*$' | while IFS= read -r line; do
	if ! grep -qxF "$line" smoke.out; then
	    key=${line%%: *}
	    echo "$key: expected '${line#*: }', got '$(value "$key")'" >>differ.txt
	fi
    done
    if test -s differ.txt; then
	fail "$1" "differs from ${golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt)"
	return 1
    fi
    return 0
}

stock_parity=
for image in stock web; do
    from=$WASM_DIR/image/$image
    if ! ls "$from"/*.image >/dev/null 2>&1; then
	skip_check "parity $image" "no image in $from (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
	continue
    fi

    fresh "parity-$image" "$from"
    run "parity $image" parity &&
	quiet "parity $image" && golden "parity $image" &&
	ok "parity $image" "$(value 'realFont class') $(value version), nonwhite $(value nonwhite), bits $(value bits | cut -c 1-12)..., golden ft-p$(value major).txt; first draw $(value 'draw ms') ms"
    if test "$image" = stock && test "$check_failed" -eq 0; then
	stock_parity=$(grep -E '^(nonwhite|bits): ' smoke.out)
	stock_major=$(value major)
    fi

    fresh "hidden-$image" "$from"
    run "hidden $image" hidden && quiet "hidden $image" && {
	class=$(value 'realFont class')
	nonwhite=$(value nonwhite)
	if test -z "$class" || test "$class" = FreeTypeFont; then
	    fail "hidden $image" "with FreeType hidden, the font that drew is '$class', not a fallback"
	elif ! is_number "$nonwhite" || test "$nonwhite" -eq 0; then
	    fail "hidden $image" "with FreeType hidden, nothing drawn (nonwhite '$nonwhite')"
	elif test "x$(value errors)" != xnone; then
	    fail "hidden $image" "with FreeType hidden, the drawing raised $(value errors)"
	else
	    ok "hidden $image" "$class, nonwhite $nonwhite; version: $(value version | cut -c 1-80)"
	fi
    }
done

# The advice of a native VM: the same script on the stock image
if test -z "$HOST_PHARO" || ! test -x "$HOST_PHARO"; then
    echo "note [$lane] no HOST_PHARO: ft-parity.st is not compared with a native VM"
elif test -z "$stock_parity"; then
    echo "note [$lane] no parity of the stock image: ft-parity.st is not compared with a native VM"
else
    retire
    native=$TEST_DIR/$lane-native
    rm -rf "$native"
    mkdir -p "$native" && copy_image "$WASM_DIR/image/stock" "$native" && (
	cd "$native" &&
	    limit "$timeout" "$HOST_PHARO" --headless Pharo.image --no-default-preferences st --quit "$script"
    ) >"$native/native.out" 2>"$native/native.err"
    native_status=$?
    host=$(grep -E '^(nonwhite|bits): ' "$native/native.out")
    host_version=$(sed -n 's/^version: //p' "$native/native.out")
    if test "$native_status" -ne 0 || test -z "$host"; then
	echo "note [$lane] HOST_PHARO: ft-parity.st did not run natively (status $native_status; kept $native)"
    elif test "x$host" = "x$stock_parity"; then
	echo "note [$lane] HOST_PHARO, FreeType $host_version: the same nonwhite and bits as the golden ft-p$stock_major.txt"
	rm -rf "$native"
    else
	echo "note [$lane] HOST_PHARO, FreeType $host_version: other pixels than the golden ft-p$stock_major.txt:" \
	    "$(echo "$host" | tr '\n' ' ')(kept $native)"
    fi
fi

# What FreeType costs a boot: the best of 3 boots of the stock image each
best_boot() {
    best=
    for i in 1 2 3; do
	rm -f boot.ms
	measure boot.ms --headless Pharo.image --no-default-preferences eval '3 + 4' >boot.out 2>/dev/null </dev/null
	ms=$(cut -d ' ' -f 1 boot.ms 2>/dev/null)
	if ! is_number "$ms" || test "x$(cat boot.out)" != x7; then
	    best=
	    return 1
	fi
	if test -z "$best" || test "$ms" -lt "$best"; then best=$ms; fi
    done
}
fresh startup
if best_boot; then
    with=$best
    if PHARO_WASM_FFI_HIDE=freetype && export PHARO_WASM_FFI_HIDE && best_boot; then
	echo "note [$lane] a boot of the stock image (eval 3 + 4, the best of 3): $with ms with FreeType, $best ms with it hidden"
    else
	echo "note [$lane] a boot of the stock image with FreeType hidden did not answer 7: no measure"
    fi
    unset PHARO_WASM_FFI_HIDE
else
    echo "note [$lane] a boot of the stock image did not answer 7: no measure"
fi

finish 64-freetype
