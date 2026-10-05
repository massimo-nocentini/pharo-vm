#!/bin/sh
# wasm-smoke.sh - smoke tests for the WebAssembly (node) build of the Pharo VM
#
# usage: sh tests/wasm/wasm-smoke.sh [LANE ...]
#
#   default  every check, S1-S22, with build-wasm/node/pharo (memory64)
#   liftoff  the checks that stress the engine, S2, S8 and S9, with Liftoff
#            code only (--liftoff-only) and a 900 KB stack
#            (NODE_OPTIONS_WASM), and S2t, which checks that V8 compiled no
#            function with TurboFan; S8 allows 1400 ms, not 1000, as Liftoff
#            code is slower
#   worker   the same checks in a worker thread of 1 MB of stack, the stack of
#            a browser worker (in-worker.js), with Liftoff code only
#
# With no LANE, all three.  Lane 20-smoke runs default, 30-engines the other
# two.  Nothing is written outside TEST_DIR.
#
# (--liftoff --no-wasm-tier-up, the flags of the plan, do not keep V8 in
# Liftoff code: its dynamic tiering still compiles the hot functions again
# with TurboFan, which --trace-wasm-compilation-times shows on node 25, and
# then the VM runs as fast as with the default engine.  --liftoff-only
# does, as --no-wasm-dynamic-tiering added to them would.)
#
# Environment (from make wasm-check, see run-lanes.sh):
#   WASM_DIR    the build directory: node/pharo, image/stock and, for S22,
#               the prepared image of the world in image/web
#   TEST_DIR    a scratch directory; every check runs in a directory of its
#               own there, on a fresh copy of the image, which is removed
#               when the check passes
#   HOST_PHARO  a native Pharo VM, which must load the image S14 saved (S14b
#               is skipped without it)
#   NODE, SRCDIR
#   WASM_CHECK_TIMEOUT  the seconds one VM run may take (default 120)
#   WASM_CHECK_TRIES    the runs a check that measures a time may take
#                       (default 3): S6, S6b and S8 run again while their
#                       time is over its limit (see lib/common.sh)
#
# Every check compares stdout and/or the exit status.  Any engine-level
# failure on stderr (RuntimeError, RangeError, Aborted, unreachable,
# signature_mismatch, unsupported syscall, or a TypeError or ReferenceError
# of the JavaScript runtime) fails the check.

SRCDIR=${SRCDIR:-$(dirname "$0")/../..}
. "$SRCDIR/tests/wasm/lib/common.sh"

lanes=${*:-default liftoff worker}
ST=$SRCDIR/tests/wasm/st

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "wasm-smoke: no $WASM_DIR/node/pharo (make wasm builds it)"
    exit 1
fi

# The VM of the lane, pharo_run, defined below, on the image of the check
P_ARGS="--headless Pharo.image --no-default-preferences"
P="pharo_run $P_ARGS"

# The checks that stress the engine (the interpreter's frame, deep
# recursion that spills the stack pages, and a growing heap); the lanes
# liftoff and worker rerun them.

check_s2() {
    fresh S2
    expect_quiet S2 7 $P eval '3+4'
}

# S8: 300k-deep recursion in less than s8_limit ms, set by the lane: 1000,
# or 1400 for Liftoff code, which takes about 1.4 times as long as
# TurboFan's (382 against 280 ms on node 25), for the same margin
check_s8() {
    fresh S8
    while run S8 $P eval '| b | b := [:n | n = 0 ifTrue: [0] ifFalse: [1 + (b value: n - 1)]].
	{[b value: 300000] timeToRun asMilliSeconds. b value: 1000000}'; do
	out=$(cat smoke.out)
	ms=${out#'#('}
	ms=${ms%' 1000000)'}
	if test "$status" -ne 0; then
	    fail S8 "exit status $status"
	elif test "x$out" != "x#($ms 1000000)" || ! is_number "$ms"; then
	    fail S8 "expected '#(<ms> 1000000)', got '$out'"
	elif test "$ms" -ge "$s8_limit"; then
	    least "$ms"
	    again S8 "300k-deep recursion took $ms ms" && continue
	    fail S8 "300k-deep recursion took $best_time ms$(best_of), $s8_limit or more"
	else
	    ok S8 "300k deep in $ms ms$(tried)"
	fi
	break
    done
}

# S2t, for the lanes liftoff and worker: eval '3+4' again, with V8 printing
# on stdout a line for every function it compiles, which must say Liftoff,
# never TurboFan.  (node_trace adds the V8 flag to the lane's pharo_run.)
# A run that prints no such line at all fails too, when this node's trace
# names the tier: the lane's V8 flags then never reached node (a launcher
# that drops NODE_OPTIONS_WASM), and S2, S8 and S9 ran on the default
# engine.  Only a node whose trace names no tier skips S2t.
check_s2t() {
    fresh S2t
    if ! "$NODE" --v8-options 2>/dev/null | grep -q -e '--trace-wasm-compilation-times'; then
	skip_check S2t "this node has no --trace-wasm-compilation-times"
	return 0
    fi
    node_trace=--trace-wasm-compilation-times
    if run S2t $P eval '3+4'; then
	liftoff=$(grep -c 'using Liftoff' smoke.out)
	turbofan=$(grep -c 'using TurboFan' smoke.out)
	if test "$status" -ne 0; then
	    fail S2t "exit status $status"
	elif ! grep -qx 7 smoke.out; then
	    fail S2t "no line '7' on stdout"
	elif test "$turbofan" -ne 0; then
	    fail S2t "V8 compiled $turbofan functions with TurboFan"
	elif test "$liftoff" -eq 0 && trace_names_tier; then
	    fail S2t "V8 traced no function 'using Liftoff', though this node's trace names the tier: the lane's V8 flags did not reach node"
	elif test "$liftoff" -eq 0; then
	    checks=$((checks - 1))
	    skip_check S2t "this node's --trace-wasm-compilation-times names no tier ('using Liftoff')"
	else
	    ok S2t "$liftoff functions compiled, all with Liftoff"
	fi
    fi
    node_trace=
}

# trace_names_tier: whether this node's --trace-wasm-compilation-times says
# 'using Liftoff' for the one function of a module of its own (which
# returns 7), compiled with Liftoff only, as in the lanes of S2t
trace_names_tier() {
    limit "$timeout" "$NODE" --liftoff-only --trace-wasm-compilation-times -e '
new WebAssembly.Instance(new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0,
  1, 5, 1, 96, 0, 1, 127, 3, 2, 1, 0, 7, 5, 1, 1, 102, 0, 0, 10, 6, 1, 4, 0, 65, 7, 11]))).exports.f();' \
	</dev/null 2>/dev/null | grep -q 'using Liftoff'
}

# gc_check NAME MIN_SLICES CMD ...: gcstress.st must keep its 3000000
# arrays in an old space of more than 200 MB (whose exact size differs from
# run to run), in at least MIN_SLICES slices when PHARO_WASM_STATS counts
# them.  When old_space_at is set, CMD also left in placement.out the log of
# a VM at level 4, which must say that it allocated old space there.
gc_check() {
    name=$1; min_slices=$2; shift 2
    slices=
    rm -f placement.out
    run "$name" "$@" || return 0
    out=$(cat smoke.out)
    slices=$(sed -n 's/.*slices=\([0-9]*\).*/\1/p' smoke.err | tail -n 1)
    set -- $out
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test "x$1" != x3000000 || ! is_number "$2" || test "$2" -le 200000000; then
	fail "$name" "expected '3000000 <more than 200000000>', got '$out'"
    elif test -n "$slices" && test "$slices" -lt "$min_slices"; then
	fail "$name" "expected at least $min_slices slices, got $slices"
    elif test -n "$old_space_at" &&
	! grep -q "Allocated [0-9]* bytes at $old_space_at for " placement.out 2>/dev/null; then
	# (the first segment of old space is the first allocation that is not
	# where it was asked for: it is at the base, asked for at the address
	# of the memory map)
	at=$(sed -n 's/.*Allocated [0-9]* bytes at \([^ ]*\) for \([^ ]*\) .*/\1 \2/p' placement.out 2>/dev/null |
	    awk '$1 != $2 { print $1; exit }')
	fail "$name" "old space is not at $old_space_at, but at '${at:-nowhere the log at level 4 says}'"
    else
	ok "$name" "$out${slices:+, $slices slices}${old_space_at:+, old space at $old_space_at}"
    fi
}

check_s9() {
    fresh S9
    s9() ( PHARO_WASM_STATS=1; export PHARO_WASM_STATS; $P st --quit "$ST/gcstress.st" )
    gc_check S9 1 s9
    s9_slices=${slices:-0}
}

engine_checks() {
    check_s2
    check_s2t
    check_s8
    check_s9
}

default_checks() {
    fresh S1
    expect_match S1 '^Pharo v?[0-9]+\.[0-9]+' pharo_run --version

    check_s2

    # (OSPlatform current and OSEnvironment current, not Smalltalk os, which
    # Pharo 15 deprecates with a notification on stdout)
    # The FFI backend: TFFIBackend when the build has the FFI (FEATURE_FFI in
    # the CMake cache of the build, WASM_FFI), NullFFIBackend otherwise
    backend=NullFFIBackend
    if grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
	backend=TFFIBackend
    fi
    fresh S3
    expect S3 "#(#Unix64Platform 'wasm64' 8 'unix' #$backend)" $P eval \
	'{OSPlatform current class name. Smalltalk vm architectureName. Smalltalk vm wordSize.
	  Smalltalk vm getSystemAttribute: 1001. FFIBackend current class name}'

    fresh S4
    s4() {
	echo 'written by the shell' >from-host.txt &&
	$P st --quit "$ST/fileio.st" &&
	echo "on the host: $(cat fileio/round-trip.txt)"
    }
    expect S4 'from the host: written by the shell
round trip: written by Pharo
directory: true
listing: true
deleted: true
on the host: written by Pharo' s4

    # a value of HOME of its own, so that it cannot be the default
    fresh S5
    s5() ( HOME=$check_dir/home; export HOME; $P eval "OSEnvironment current at: 'HOME'" )
    expect S5 "'$check_dir/home'" s5

    fresh S6
    expect_time S6 190 400 $P eval '[(Delay forMilliseconds: 200) wait] timeToRun asMilliSeconds'
    # The CPU a 1 s Delay costs, in percent of that second: the CPU time of a
    # run with it less that of a run without it, which takes away the start
    # (pharo.js compiled on node's threads while the image loads).  (Not
    # over the difference of their wall times, which a loaded machine makes
    # anything, even negative.)
    fresh S6b
    s6b() {
	rm -f idle.time delay.time
	measure idle.time $P_ARGS eval 1 && measure delay.time $P_ARGS eval '(Delay forSeconds: 1) wait. 1'
    }
    while run S6b s6b; do
	idle_wall= idle_cpu= delay_wall= delay_cpu=
	if test -f idle.time && test -f delay.time; then
	    read idle_wall idle_cpu <idle.time
	    read delay_wall delay_cpu <delay.time
	fi
	if test "$status" -ne 0; then
	    fail S6b "exit status $status"
	elif test "x$(cat smoke.out)" != "x1
1"; then
	    fail S6b "expected '1' twice, got '$(cat smoke.out)'"
	elif ! is_number "$idle_cpu" || ! is_number "$delay_wall" || ! is_number "$delay_cpu"; then
	    fail S6b "no wall and CPU times measured"
	elif test "$delay_wall" -lt 1000; then
	    fail S6b "the run with a 1 s Delay took $delay_wall ms"
	else
	    percent=$(( (delay_cpu - idle_cpu) / 10 ))
	    if test "$percent" -lt 0; then percent=0; fi
	    if test "$percent" -ge 50; then
		least "$percent"
		again S6b "a 1 s Delay took ${percent}% CPU" && continue
		fail S6b "a 1 s Delay took ${best_time}% CPU$(best_of), 50% or more"
	    else
		ok S6b "${percent}% CPU$(tried)"
	    fi
	fi
	break
    done

    fresh S7
    expect S7 true limit 10 "$WASM_DIR/node/pharo" $P_ARGS st --quit "$ST/preempt.st"

    check_s8
    check_s9

    fresh S10
    expect S10 '#(2568 158)' $P eval '{1000 factorial printString size. 100 factorial printString size}'

    fresh S11
    expect S11 36 $P eval 'UUID new printString size'

    fresh S12
    expect S12 '#caught' $P eval "[NetNameResolver addressForName: 'localhost'] on: Error do: [:e | #caught]"

    fresh S13
    expect_quiet S13 'Hello from Pharo on wasm64
7' $P st --quit "$ST/hello.st"

    # The image saved in wasm, reloaded in wasm, then by the native VM
    fresh S14
    s14() { $P st "$ST/snapshot.st" && $P eval 'Smalltalk at: #WasmSnapshotMarker'; }
    expect S14 "'saved by wasm64 42'" s14
    if test -n "$HOST_PHARO" && test -x "$HOST_PHARO"; then
	expect S14b "'saved by wasm64 42'" limit "$timeout" "$HOST_PHARO" $P_ARGS \
	    eval 'Smalltalk at: #WasmSnapshotMarker'
    else
	skip_check S14b "HOST_PHARO does not name a native Pharo VM"
    fi

    # (Pharo 12 has no Smalltalk quit)
    fresh S15
    expect_status S15 1 $P eval 'Smalltalk exitFailure'
    fresh S15b
    printf '%s\n' "Stdio stdout nextPutAll: 'quitting'; lf; flush." \
	'Smalltalk snapshot: false andQuit: true.' >quit.st
    expect S15b quitting $P st quit.st
    fresh S15c
    expect_status S15c 3 $P eval 'Smalltalk exit: 3'

    s16() ( TZ=$1; export TZ; $P eval 'DateAndTime now offset' )
    fresh S16
    expect S16 0:00:00:00 s16 UTC
    fresh S16b
    expect_match S16b '^0:0[12]:00:00$' s16 Europe/Rome

    # setenv is reached through the FFI only: with it, LibC's setenv, which
    # getenv then sees; without it, an error
    fresh S17
    if test $backend = TFFIBackend; then
	expect S17 "'x'" $P eval \
	    "[OSEnvironment current at: 'WASM_T' put: 'x'. OSEnvironment current at: 'WASM_T'] on: Error do: [:e | #unsupported]"
    else
	expect S17 '#unsupported' $P eval \
	    "[OSEnvironment current at: 'WASM_T' put: 'x'. #set] on: Error do: [:e | #unsupported]"
    fi

    # the 13 plugins of the tree, UUIDPlugin when the CMake cache of the build
    # turns FEATURE_PLUGIN_UUID on, and one for each src/emscripten/plugins/*.c
    # (WebHostPlugin, then WebDisplayPlugin)
    modules=13
    if grep -Eiq '^FEATURE_PLUGIN_UUID:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
	modules=$((modules + 1))
    fi
    for f in "$SRCDIR"/src/emscripten/plugins/*.c; do
	if test -f "$f"; then modules=$((modules + 1)); fi
    done
    fresh S18
    expect S18 $modules $P eval 'Smalltalk vm listBuiltinModules size'

    # 2 s of computing take 100 slices of 20 ms: the VM returns to node, and
    # is entered again, all along
    fresh S19
    s19() (
	PHARO_WASM_STATS=1; export PHARO_WASM_STATS
	$P eval '| t | t := Time millisecondClockValue. [Time millisecondClockValue - t < 2000] whileTrue. #done'
    )
    run S19 s19 && {
	slices=$(sed -n 's/.*slices=\([0-9]*\).*/\1/p' smoke.err | tail -n 1)
	if test "$status" -ne 0; then
	    fail S19 "exit status $status"
	elif test "x$(cat smoke.out)" != 'x#done'; then
	    fail S19 "expected '#done', got '$(cat smoke.out)'"
	elif ! is_number "$slices" || test "$slices" -lt 10; then
	    fail S19 "expected at least 10 slices, got '$slices'"
	else
	    ok S19 "$slices slices"
	fi
    }

    # the same with slices of 1 ms: many times the slices of S9
    fresh S20
    s20() (
	PHARO_WASM_SLICE_MS=1 PHARO_WASM_STATS=1; export PHARO_WASM_SLICE_MS PHARO_WASM_STATS
	$P st --quit "$ST/gcstress.st"
    )
    gc_check S20 $((2 * s9_slices)) s20

    # Old space at another base: where a VM logging at level 4 says it put
    # it (on its stdout, kept in placement.out), then gcstress.st in it
    s21() ( PHARO_WASM_OLD_SPACE_BASE=$1; export PHARO_WASM_OLD_SPACE_BASE; $P st --quit "$ST/gcstress.st" )
    s21_placed() (
	PHARO_WASM_OLD_SPACE_BASE=$1; export PHARO_WASM_OLD_SPACE_BASE
	pharo_run --logLevel=4 $P_ARGS eval 1 >placement.out && s21 "$1"
    )
    fresh S21
    old_space_at=0x40000000
    gc_check S21 1 s21_placed $old_space_at
    old_space_at=
    fresh S21b
    expect_error S21b 'Invalid PHARO_WASM_OLD_SPACE_BASE 0x30000000: old space needs a power of two' \
	s21 0x30000000

    # the image of the world (WP11), which boots headless as the stock one
    if test -f "$WASM_DIR/image/web/Pharo-web.image"; then
	fresh S22 "$WASM_DIR/image/web"
	expect S22 '#(7 false)' $P eval '{3 + 4. OSWebDriver isSuitable}'
    else
	skip_check S22 "no prepared image in $WASM_DIR/image/web (WASM_WORLD=OFF or no WASM_HOST_PHARO)"
    fi
}

# How each lane starts the VM (node_trace: V8 flags of S2t)
node_trace=
old_space_at=
for lane in $lanes; do
    case $lane in
	default)
	    s8_limit=1000
	    pharo_run() { limit "$timeout" "$WASM_DIR/node/pharo" "$@"; }
	    default_checks ;;
	liftoff)
	    s8_limit=1400
	    pharo_run() {
		limit "$timeout" env NODE_OPTIONS_WASM="--liftoff-only --stack-size=900${node_trace:+ $node_trace}" \
		    "$WASM_DIR/node/pharo" "$@"
	    }
	    engine_checks ;;
	worker)
	    s8_limit=1400
	    pharo_run() {
		limit "$timeout" "$NODE" --liftoff-only $node_trace "$SRCDIR/tests/wasm/in-worker.js" 1 \
		    "$WASM_DIR/node/pharo.js" "$@"
	    }
	    engine_checks ;;
	*)
	    echo "wasm-smoke: unknown lane '$lane' (default, liftoff or worker)"
	    exit 2 ;;
    esac
done

finish wasm-smoke
