#!/bin/sh
# 65-cairo - cairo in the WebAssembly VM, in node: Athens and Roassal
#
# build-wasm/node/pharo on fresh copies of the prepared image of the world
# (image/web) runs tests/wasm/st/athens-parity.st, which draws five scenes
# with cairo through the image's own bindings (CairoLibrary, the library
# cairo of the registry, found as the placeholder node/libcairo.so.2):
# Athens shapes, text (cairo_ft, over the FreeType of the VM), a repeated
# gradient, a Roassal canvas and a Mondrian, and a PNG that cairo writes:
#
#   parity    every line of the golden file of the image's version,
#             tests/wasm/golden/athens-p<major>.txt, must be a line of what it
#             prints: TFFIBackend, the extension of OSWindow-Web that copies
#             the pixels of a surface into a Form (there is no SurfacePlugin),
#             cairo 1.18.4, and the SHA256 of the bits of each scene.  The
#             golden files are the reference; with HOST_PHARO, the same script
#             on the same image, natively, is compared with them as an advice
#             only (a note, never a failure): a native VM ships its own cairo;
#   adapted   the same run is made with PHARO_WASM_FFI_TRACE=1, which has the
#             width adapter of the FFI (src/emscripten/ffiAdapt.c) name each
#             function whose declaration it adapted, once, on stderr: every
#             name must be in tests/wasm/golden/adapted-65.txt, and those
#             marked required there must be named (cairo_pattern_set_extend,
#             which the repeated gradient calls, declared with a ulong for
#             C's int enum); a run that names none fails, since the trace
#             would then say nothing;
#   adapt off the negative control of the adapter: with PHARO_WASM_FFI_ADAPT=0
#             the repeated gradient alone must fail as a primitive, with
#             exactly one line of the guard of the callouts on stderr, a
#             mismatch, and the trace must name nothing;
#   stock     the negative control of the extension: on the stock image,
#             which does not have it, Athens shapes must fail with 'Unable to
#             register surface with SurfacePlugin', without a failed callout;
#   hidden    the negative control of the registry: with cairo hidden from
#             the VM (PHARO_WASM_FFI_HIDE=cairo), Athens shapes must fail with
#             a SymbolNotFoundError, without a crash or a failed callout.
#
# Every run must exit with 0, print no line of the guard of the callouts
# ('FFI callout failed, its declaration does not match ...' or 'FFI callout
# trapped in ...') but those the control expects, nothing else on stderr but
# the lines of the trace, and write no PharoDebug.log.  Skipped when the
# build has no cairo (WASM_CAIRO=OFF, or no FreeType, or WASM_FFI=OFF): its
# node VM's registry (cmake/wasm/ffi/ffiRegistry-node.c) has no library
# cairo.
#
# Environment (from make wasm-check): NODE, WASM_DIR, HOST_PHARO, SRCDIR,
# TEST_DIR, and WASM_CHECK_TIMEOUT (see lib/common.sh).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/65-cairo}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=cairo

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 65-cairo: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF), so no cairo"
fi
registry=$WASM_DIR/cmake/wasm/ffi/ffiRegistry-node.c
if ! test -f "$registry"; then
    echo "FAIL 65-cairo: no $registry, the registry of the node VM (make wasm)"
    exit 1
fi
if ! grep -q '^extern const PharoFFILibrary pharoFFILibrary_cairo;$' "$registry"; then
    skip "no cairo: the registry of the node VM has no library cairo (WASM_CAIRO=OFF, or no FreeType)"
fi
echo "65-cairo: $WASM_DIR/node/pharo"

script=$SRCDIR/tests/wasm/st/athens-parity.st
golden_dir=$SRCDIR/tests/wasm/golden
adapted_list=$golden_dir/adapted-65.txt

# The lines of the guard of the callouts, of the trace of the adapter, and
# what the runtime of a debug build says of itself, as in lane 59: vm()
# keeps the guard's in guard.err and the trace's in trace.err, out of the
# stderr the checks see (the message of what threw may name an error of the
# JavaScript runtime, see engine_errors in lib/common.sh), and leaves the
# runtime's out; the rest of stderr goes on.  A line of the trace is
# 'FFI callout adapted: <function> ...', the name of the function first.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
trace='^FFI callout adapted: '
runtime='^(Heap resize call from [0-9]+ to [0-9]+ took .* Success: true|program exited \(with status: [0-9]+\), but keepRuntimeAlive\(\) is set)'
vm() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -E "$trace" vm.err >trace.err
    grep -Ev "$guard|$trace|$runtime" vm.err >&2
    return $vm_status
}
# (each run has the trace on: its lines are the evidence of the adapted
# check, and a control must show that it says nothing)
parity() ( PHARO_WASM_FFI_TRACE=1; export PHARO_WASM_FFI_TRACE; vm st --quit "$script" )
adapt_off() (
    PHARO_WASM_FFI_TRACE=1 PHARO_WASM_FFI_ADAPT=0 WASM_ATHENS_SCENES='gradient repeat'
    export PHARO_WASM_FFI_TRACE PHARO_WASM_FFI_ADAPT WASM_ATHENS_SCENES
    vm st --quit "$script"
)
shapes() ( WASM_ATHENS_SCENES=athens-shapes; export WASM_ATHENS_SCENES; vm st --quit "$script" )
hidden() ( PHARO_WASM_FFI_HIDE=cairo; export PHARO_WASM_FFI_HIDE; shapes )

# value KEY: the value of the line 'KEY: ...' of smoke.out
value() {
    sed -n "s/^$1: //p" smoke.out | head -n 1
}
# adapted: the functions that trace.err names, sorted, one a line
adapted() {
    sed -n 's/^FFI callout adapted: \([A-Za-z_][A-Za-z0-9_]*\).*/\1/p' trace.err | sort -u
}

# exited NAME: fails NAME and answers false after a run that did not exit
# with 0, printed anything on stderr but the lines of the guard and of the
# trace, or wrote PharoDebug.log
exited() {
    if test "$status" -ne 0; then
	fail "$1" "exit status $status"
    elif test -s smoke.err; then
	fail "$1" "stderr is not empty: $(head -n 1 smoke.err | cut -c 1-200)"
    elif test -s PharoDebug.log; then
	fail "$1" "it wrote PharoDebug.log: $(grep -m 1 . PharoDebug.log | cut -c 1-200)"
    else
	return 0
    fi
    return 1
}
# quiet NAME: exited NAME, and no line of the guard
quiet() {
    exited "$1" || return 1
    if test -s guard.err; then
	fail "$1" "$(grep -c '' guard.err) failed callouts: $(head -n 1 guard.err)"
	return 1
    fi
    return 0
}

# golden NAME: smoke.out against the golden file of its major version;
# answers false, with the lines that differ in differ.txt, when one is not
# there
golden() {
    major=$(value major)
    golden=$golden_dir/athens-p$major.txt
    if ! test -f "$golden"; then
	fail "$1" "no golden file $golden for the Pharo of the image (major: '$major')"
	return 1
    fi
    : >differ.txt
    grep -v '^#' "$golden" | grep -v '^[ 	]*$' | while IFS= read -r line; do
	if ! grep -qxF "$line" smoke.out; then
	    key=${line%%: *}
	    echo "$key: expected '${line#*: }', got '$(value "$key" | cut -c 1-200)'" >>differ.txt
	fi
    done
    if test -s differ.txt; then
	fail "$1" "differs from ${golden#$SRCDIR/}: $(tr '\n' ';' <differ.txt)"
	return 1
    fi
    return 0
}

# allowed NAME: the functions of trace.err against adapted-65.txt, whose
# lines are '<function>' or '<function> required'
allowed() {
    if ! test -f "$adapted_list"; then
	fail "$1" "no $adapted_list"
	return 1
    fi
    names=$(adapted)
    known=$(grep -v '^#' "$adapted_list" | awk 'NF { print $1 }')
    required=$(grep -v '^#' "$adapted_list" | awk '$2 == "required" { print $1 }')
    unknown=
    for name in $names; do
	echo "$known" | grep -qxF "$name" || unknown="$unknown $name"
    done
    absent=
    for name in $required; do
	echo "$names" | grep -qxF "$name" || absent="$absent $name"
    done
    if test -z "$names"; then
	fail "$1" "the trace (PHARO_WASM_FFI_TRACE=1) named no adapted function: it says nothing, or the adapter is off"
    elif test -n "$unknown"; then
	fail "$1" "adapted functions that ${adapted_list#$SRCDIR/} does not list:$unknown (all: $(echo $names))"
    elif test -n "$absent"; then
	fail "$1" "required adapted functions the trace did not name:$absent (it named $(echo $names))"
    else
	ok "$1" "$(echo "$names" | grep -c .) functions: $(echo $names)"
    fi
}

web=$WASM_DIR/image/web
if ! ls "$web"/*.image >/dev/null 2>&1; then
    for name in parity adapted 'adapt off' hidden; do
	skip_check "$name" "no image in $web (WASM_WORLD=OFF, or no WASM_HOST_PHARO)"
    done
    web=
fi

web_parity=
if test -n "$web"; then
    fresh parity "$web"
    if run parity parity; then
	quiet parity && golden parity && {
	    ok parity "cairo $(value 'cairo version'), golden athens-p$(value major).txt; ms: shapes $(value 'ms athens-shapes'), text $(value 'ms athens-text'), repeat $(value 'ms gradient repeat'), roassal $(value 'ms roassal'), mondrian $(value 'ms mondrian')"
	    web_parity=$(grep -E '^(athens-shapes|athens-text|gradient repeat|roassal|mondrian|athens-png): ' smoke.out)
	    web_major=$(value major)
	}
	# (the trace of the same run)
	checks=$((checks + 1))
	allowed adapted
    fi

    fresh adapt-off "$web"
    run 'adapt off' adapt_off && exited 'adapt off' && {
	guards=$(grep -c '' guard.err)
	repeat=$(value 'gradient repeat')
	case $repeat in
	    'error PrimitiveFailed'*) ;;
	    *) repeat= ;;
	esac
	if test -z "$repeat"; then
	    fail 'adapt off' "with the adapter off, the repeated gradient did not fail as a primitive: '$(value 'gradient repeat' | cut -c 1-200)'"
	elif test "$guards" -ne 1 || ! grep -q '^FFI callout failed, its declaration does not match' guard.err; then
	    fail 'adapt off' "expected 1 failed callout on stderr, a mismatch, got $guards: $(head -n 3 guard.err | tr '\n' ';')"
	elif test -s trace.err; then
	    fail 'adapt off' "with the adapter off, the trace named $(adapted | tr '\n' ' ')"
	else
	    ok 'adapt off' "1 failed callout: $(cut -c 1-90 guard.err)"
	fi
    }

    fresh hidden "$web"
    run hidden hidden && quiet hidden && {
	shapes_line=$(value athens-shapes)
	case $shapes_line in
	    'error SymbolNotFoundError'*) ok hidden "$(echo "$shapes_line" | cut -c 1-100)" ;;
	    *) fail hidden "with cairo hidden, Athens shapes did not fail with a SymbolNotFoundError: '$(echo "$shapes_line" | cut -c 1-200)'" ;;
	esac
    }
fi

fresh stock
run stock shapes && quiet stock && {
    shapes_line=$(value athens-shapes)
    if test "x$(value surface)" = xOSWindow-Web; then
	fail stock "the stock image has the extension of OSWindow-Web"
    else
	case $shapes_line in
	    *'Unable to register surface with SurfacePlugin'*) ok stock "$shapes_line" ;;
	    *) fail stock "without the extension, Athens shapes did not fail for the SurfacePlugin: '$(echo "$shapes_line" | cut -c 1-200)'" ;;
	esac
    fi
}

# The advice of a native VM: the same script on the same image
if test -z "$HOST_PHARO" || ! test -x "$HOST_PHARO"; then
    echo "note [$lane] no HOST_PHARO: athens-parity.st is not compared with a native VM"
elif test -z "$web_parity"; then
    echo "note [$lane] no parity of the image of the world: athens-parity.st is not compared with a native VM"
else
    retire
    native=$TEST_DIR/$lane-native
    rm -rf "$native"
    mkdir -p "$native" && copy_image "$web" "$native" && (
	cd "$native" &&
	    limit "$timeout" "$HOST_PHARO" --headless Pharo.image --no-default-preferences st --quit "$script"
    ) >"$native/native.out" 2>"$native/native.err"
    native_status=$?
    host=$(grep -E '^(athens-shapes|athens-text|gradient repeat|roassal|mondrian|athens-png): ' "$native/native.out")
    host_version=$(sed -n 's/^cairo version: //p' "$native/native.out")
    if test "$native_status" -ne 0 || test -z "$host"; then
	echo "note [$lane] HOST_PHARO: athens-parity.st did not run natively (status $native_status; kept $native)"
    elif test "x$host" = "x$web_parity"; then
	echo "note [$lane] HOST_PHARO, cairo $host_version: the same scenes as the golden athens-p$web_major.txt"
	rm -rf "$native"
    else
	echo "note [$lane] HOST_PHARO, cairo $host_version: other scenes than the golden athens-p$web_major.txt:" \
	    "$(echo "$host" | grep -vxF "$web_parity" | cut -c 1-40 | tr '\n' ' ')(kept $native)"
    fi
fi

finish 65-cairo
