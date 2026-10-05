#!/bin/sh
# 58-ffi - the FFI: uFFI callouts and callbacks, in node
#
# build-wasm/node/pharo on a fresh copy of the stock image runs
# tests/wasm/st/ffi.st: the C library (LibC's libc.so.6, the library c of
# the registry) and the FFI test library (TestLibrary), scalars, a variadic
# function, structures by value, callbacks and reentrant ones, results
# declared wider than the function's (which the return patch of libffi
# widens, signed and unsigned), the environment, a bench of 10000 strlen
# callouts (its time is printed), and two declarations that do not match
# their function: strlen declared double, and a callback declared void
# where C calls an int (*)(int).  They must fail as primitives, with
# exactly one line each on stderr, from the guard of the callouts
# (emscriptenSupport.c): 'FFI callout failed, its declaration does not
# match ...' for strlen, whose result libffi cannot convert (a TypeError),
# and 'FFI callout trapped in ...' for the callback, whose indirect call
# from C has the wrong type (a RuntimeError); and leave the VM running.
# The declarations whose integers are only wider or narrower than the
# function's (strlen declared int, abs given an int64, bsearch given ints
# for its size_t, with a callback that makes such a callout itself) and
# sprintf declared without fixedArgumentCount: must work, through the width
# adapter of the FFI (src/emscripten/ffiAdapt.c), which the run has trace
# them (PHARO_WASM_FFI_TRACE=1): its lines on stderr, 'FFI callout adapted:
# <function> ...', must name strlen.  And sprintf given a float among its
# variadic arguments, which C cannot take, must fail as a primitive, with
# exactly one line of the adapter on stderr, 'FFI callout to a variadic
# function sprintf: declare it with fixedArgumentCount:'.
# Then a busy loop of 2 s inside a callback must end no slice: the VM
# cannot return to node while C frames of the callout are on its stack
# (emscriptenCallbackDepth), so a run with that loop takes at least 50
# slices of 20 ms fewer than one with the same loop outside a callback,
# where it takes 100 (PHARO_WASM_STATS).  Skipped when the build has no FFI
# (WASM_FFI=OFF).
#
# Environment (from make wasm-check): NODE, WASM_DIR, SRCDIR, TEST_DIR, and
# WASM_CHECK_TIMEOUT (see lib/common.sh); WASM_FFI_BENCH_MS, the limit of
# the bench in ms (default 10000, ffi.st).

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/58-ffi}
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=ffi

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "FAIL 58-ffi: no $WASM_DIR/node/pharo (make wasm)"
    exit 1
fi
if ! grep -Eiq '^FEATURE_FFI:BOOL=(ON|TRUE|YES|Y|1)$' "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; then
    skip "no FFI: the build has none (WASM_FFI=OFF)"
fi
echo "58-ffi: $WASM_DIR/node/pharo"

# The lines of the guard of the callouts.  vm() keeps them in guard.err,
# out of the stderr the checks see, as the message of what threw may name
# an error of the JavaScript runtime, which would fail any check
# (engine_errors, lib/common.sh); and those of the width adapter, its trace
# and what it says of a variadic call it cannot make, in adapter.err; the
# rest of stderr goes on.
guard='^FFI callout (failed, its declaration does not match|trapped in) '
adapter='^FFI callout (adapted: |to a variadic function )'
vm() {
    if limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences "$@" 2>vm.err
    then vm_status=0; else vm_status=$?; fi
    grep -E "$guard" vm.err >guard.err
    grep -E "$adapter" vm.err >adapter.err
    grep -Ev "$guard|$adapter" vm.err >&2
    return $vm_status
}
ffi() (
    WASM_FFI_LANE=set PHARO_WASM_FFI_TRACE=1
    export WASM_FFI_LANE PHARO_WASM_FFI_TRACE
    vm st --quit "$SRCDIR/tests/wasm/st/ffi.st"
)
# busy WHERE: ffi.st's loop of 2 s, in a callback or at the top, in slices
# of 20 ms; stderr ends with the slices
busy() (
    WASM_FFI_BUSY=$1 PHARO_WASM_STATS=1 PHARO_WASM_SLICE_MS=20
    export WASM_FFI_BUSY PHARO_WASM_STATS PHARO_WASM_SLICE_MS
    vm st --quit "$SRCDIR/tests/wasm/st/ffi.st"
)

fresh uffi
if run uffi ffi; then
    summary=$(grep '^ffi: ' smoke.out)
    bench=$(sed -n 's/^ffi bench: //p' smoke.out)
    guards=$(grep -c '' guard.err)
    mismatches=$(grep -c '^FFI callout failed, its declaration does not match' guard.err)
    traps=$(grep -c '^FFI callout trapped in ' guard.err)
    adapted=$(sed -n 's/^FFI callout adapted: \([A-Za-z_][A-Za-z0-9_]*\).*/\1/p' adapter.err | sort -u)
    variadic=$(grep '^FFI callout to a variadic function ' adapter.err)
    if test "$status" -ne 0; then
	fail uffi "exit status $status"
    elif grep -q '^FAIL ' smoke.out; then
	fail uffi "$(grep '^FAIL ' smoke.out | tr '\n' ';')"
    elif test "x$summary" != "xffi: 26 checks, 0 failed"; then
	fail uffi "expected 'ffi: 26 checks, 0 failed', got '$summary'"
    elif test "$guards" -ne 2 || test "$mismatches" -ne 1 || test "$traps" -ne 1; then
	fail uffi "expected 2 failed callouts on stderr, a mismatch and a trap, got $guards: $(tr '\n' ';' <guard.err)"
    elif ! echo "$adapted" | grep -qx strlen; then
	fail uffi "the trace of the adapter (PHARO_WASM_FFI_TRACE=1) does not name strlen: '$(echo $adapted)'"
    elif test "x$variadic" != "xFFI callout to a variadic function sprintf: declare it with fixedArgumentCount:"; then
	fail uffi "expected 1 line of the adapter for the float of sprintf, 'FFI callout to a variadic function sprintf: declare it with fixedArgumentCount:', got '$(echo "$variadic" | tr '\n' ';')'"
    elif grep -Eq 'STACK DRIFT|Assertion failed' smoke.err; then
	fail uffi "$(grep -E 'STACK DRIFT|Assertion failed' smoke.err | head -n 1)"
    else
	ok uffi "$summary; bench: $bench; adapted: $(echo $adapted)"
    fi
fi

# slices WHERE: the slices of busy WHERE, from its stderr, into $slices
slices() {
    run "busy $1" busy "$1" || return 1
    slices=$(sed -n 's/^\[pharo-wasm\] slices=\([0-9]*\) .*/\1/p' smoke.err | tail -n 1)
    if test "$status" -ne 0; then
	fail "busy $1" "exit status $status"
    elif test "x$(cat smoke.out)" != "xbusy $1: done"; then
	fail "busy $1" "expected 'busy $1: done', got '$(cat smoke.out)'"
    elif test -s guard.err; then
	fail "busy $1" "$(head -n 1 guard.err)"
    elif ! is_number "$slices"; then
	fail "busy $1" "no slices on stderr (PHARO_WASM_STATS)"
    else
	return 0
    fi
    return 1
}
fresh busy
if slices top; then
    top=$slices
    if slices callback; then
	if test $((top - slices)) -lt 50; then
	    fail "busy callback" "a loop of 2 s inside a callback took $slices slices, outside one $top: it ended slices"
	else
	    ok "busy callback" "$slices slices, against $top outside a callback"
	fi
    fi
fi

finish 58-ffi
