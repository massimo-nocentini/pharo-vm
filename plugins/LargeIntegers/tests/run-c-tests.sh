#!/bin/sh
# run-c-tests.sh - C-level tests of the LargeIntegers plugin; no VM, image or build
#
#   sh run-c-tests.sh [options] [scale]
#
# Compiles src/common/LargeIntegers.c and LargeIntegersKernels.c with the
# headers in tests/c/include standing in for the generated config.h and
# interp.h, and runs (see README.md):
#
#   kernels  c/kernels_test.c: liDigitMultiply and liDigitDiv digit for digit
#            against frozen copies of the plugin's original 32-bit loops and
#            independent oracles, in six builds: the host's limbs (64-bit on
#            x86_64 and aarch64), forced 32-bit limbs, and both with emulated
#            big-endian digits, under ASan+UBSan; both limb sizes at -O2;
#   oracle   the -O2 builds print 3000 products and quotients, which
#            c/oracle.py checks with Python's integers;
#   calls    c/plugin_harness.c drives the plugin through a mock VM: about
#            85,000 calls whose answers must match expected/mock-calls.sha256,
#            recorded from the plugin as it was before the kernels;
#   ub       the shapes that made that plugin write past an object or read
#            outside one: right answers, no ASan report;
#   oom      every allocation of 28 calls failed in turn: PrimErrNoMemory,
#            stack untouched, no crash;
#   arity    every primitive at every wrong argument count: PrimErrBadNumArgs
#            without touching the caller's frame.
#
# scale (default 1) multiplies the random case counts of the kernel tests:
# about 30 s with 1 on one core per lane (the lanes run in parallel), about
# 3 minutes with 10.
#
# Options:
#   --reference-c FILE  another LargeIntegers.c, e.g. the one before the
#                  kernels: git show <commit>^:plugins/LargeIntegers/src/common/LargeIntegers.c
#                  'calls' then also compares the two call by call and prints
#                  every mismatch, and ub, oom and arity run on the reference
#                  too, for information (the plugin before the kernels fails
#                  them: that is what they were written to catch)
#   --record       with --reference-c: write expected/mock-calls.sha256 from
#                  the reference's answers, instead of checking
#   --build DIR    where to build (kept); default: a temporary directory,
#                  removed at the end
# Environment:
#   CC             the C compiler (default: cc); ASan+UBSan are used when it
#                  can build and run a program with them
#   LI_TIMEOUT     seconds each test program may run (default 300 x scale),
#                  when timeout(1) is there; a run that takes longer fails
#
# Exit status 1 when a check fails, 2 on a usage or build error.

TESTS=$(cd "$(dirname "$0")" && pwd) || exit 2
PLUGIN=$(cd "$TESTS/.." && pwd)
ROOT=$(cd "$PLUGIN/../.." && pwd)
reference= record= build= scale=1
while test $# -gt 0; do
    case $1 in
    --reference-c|--build)
        if test $# -lt 2 || test -z "$2"; then
            echo "run-c-tests: $1 needs a value" >&2
            exit 2
        fi
        case $1 in --build) build=$2 ;; *) reference=$2 ;; esac
        shift ;;
    --record) record=1 ;;
    -h|--help) sed -n '2,48p' "$0"; exit 0 ;;
    -*) echo "run-c-tests: unknown option $1" >&2; exit 2 ;;
    *) scale=$1 ;;
    esac
    shift
done
case $scale in
''|*[!0-9]*|0) echo "run-c-tests: the scale must be a positive integer, not '$scale'" >&2; exit 2 ;;
esac
if test -n "$reference"; then
    if ! test -f "$reference"; then
        echo "run-c-tests: no file $reference (--reference-c)" >&2
        exit 2
    fi
    reference=$(cd "$(dirname "$reference")" && pwd)/$(basename "$reference")
elif test -n "$record"; then
    echo "run-c-tests: --record needs --reference-c FILE" >&2
    exit 2
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "run-c-tests: python3 is needed (oracle.py, digest.py)" >&2
    exit 2
fi
if test -n "$build"; then
    mkdir -p "$build" && build=$(cd "$build" && pwd) || exit 2
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/li-c-tests.XXXXXX") || exit 2
    trap 'rm -rf "$build"' EXIT
    trap 'exit 130' INT TERM
fi
CC=${CC:-cc}
tmo=${LI_TIMEOUT:-$((300 * scale))}
case $tmo in
''|*[!0-9]*|0) echo "run-c-tests: LI_TIMEOUT must be a positive number of seconds, not '$tmo'" >&2; exit 2 ;;
esac
# $limit prefixes every run of a test program: a kernel that loops fails instead of hanging
limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout -k 10 $tmo"; fi

case $(uname -s) in
Darwin) platform=osx ;;
*) platform=unix ;;
esac
INC="-I$TESTS/c/include -I$PLUGIN/include/common -I$PLUGIN/src/common -I$ROOT/include
     -I$ROOT/include/pharovm -I$ROOT/include/pharovm/common -I$ROOT/include/pharovm/$platform"
SRC=$PLUGIN/src/common

# ASan+UBSan when the compiler has them; UBSan reports are fatal
san="-fsanitize=address,undefined -fno-sanitize-recover=undefined -fno-omit-frame-pointer"
printf 'int main(void) { return 0; }\n' > "$build/san.c"
if $CC $san -o "$build/san" "$build/san.c" >/dev/null 2>&1 && "$build/san"; then
    SAN="$san -g -O1"
    echo "compiler: $CC, with ASan+UBSan"
else
    SAN="-g -O1"
    echo "compiler: $CC, WITHOUT ASan+UBSan (the compiler cannot build with them)"
fi
export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=0}
export UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}

failed=0
fail() { echo "== $1: FAILED"; failed=1; }
# timedout STATUS: true when STATUS is that of a run killed by $limit, which it reports
timedout() {
    test -n "$limit" && { test "$1" = 124 || test "$1" = 137; } || return 1
    echo "   timed out after $tmo s (LI_TIMEOUT)"
}

# --- builds (in parallel; one log each)
compile() {   # name command...
    name=$1; shift
    if ! "$@" > "$build/$name.build.log" 2>&1; then
        echo "run-c-tests: building $name failed:" >&2
        tail -n 30 "$build/$name.build.log" >&2
        return 1
    fi
}
pids=
kt="$TESTS/c/kernels_test.c"
compile kernels_test $CC $SAN $INC -o "$build/kernels_test" "$kt" & pids="$pids $!"
compile kernels_test32 $CC $SAN $INC -DLI_LIMB_BITS=32 -o "$build/kernels_test32" "$kt" & pids="$pids $!"
compile kernels_test_be $CC $SAN $INC -DFAKE_BIGENDIAN -o "$build/kernels_test_be" "$kt" & pids="$pids $!"
compile kernels_test_be32 $CC $SAN $INC -DFAKE_BIGENDIAN -DLI_LIMB_BITS=32 -o "$build/kernels_test_be32" "$kt" & pids="$pids $!"
compile kernels_fast $CC -O2 $INC -o "$build/kernels_fast" "$kt" & pids="$pids $!"
compile kernels_fast32 $CC -O2 $INC -DLI_LIMB_BITS=32 -o "$build/kernels_fast32" "$kt" & pids="$pids $!"
# the plugin as the VM loads it: a shared object; -w: its warnings are the build's business
compile plugin $CC $SAN $INC -w -fPIC -shared -o "$build/plugin.so" "$SRC/LargeIntegers.c" "$SRC/LargeIntegersKernels.c" & pids="$pids $!"
compile plugin_harness $CC $SAN $INC -o "$build/plugin_harness" "$TESTS/c/plugin_harness.c" -ldl & pids="$pids $!"
if test -n "$reference"; then
    # with the kernels when it calls them (the include path has their header)
    refsrc=$reference
    grep -q liDigitMultiply "$reference" && refsrc="$reference $SRC/LargeIntegersKernels.c"
    compile reference $CC $SAN $INC -w -fPIC -shared -o "$build/reference.so" $refsrc & pids="$pids $!"
fi
built=1
for pid in $pids; do
    wait $pid || built=0
done
test $built = 1 || exit 2

# --- kernels: the four sanitized builds in parallel, then the -O2 ones
pids=
for k in kernels_test kernels_test32 kernels_test_be kernels_test_be32 kernels_fast kernels_fast32; do
    $limit "$build/$k" "$scale" > "$build/$k.log" 2>&1 &
    pids="$pids $!:$k"
done
for p in $pids; do
    k=${p#*:}
    wait ${p%%:*}
    st=$?
    if test $st = 0; then
        echo "== $k: ok, $(grep -m1 '^LI_LIMB_BITS' "$build/$k.log" | sed 's/ LI_KARATSUBA[^ ]*//; s/ LI_MUL[^ ]*//; s/ LI_SQR[^ ]*//g; s/ LI_DIV[^ ]*//; s/ LI_STACK[^ ]*//'), $(grep '^totals:' "$build/$k.log" | sed 's/^totals: //')"
    else
        grep -E 'FAIL|ERROR|runtime error' "$build/$k.log" | head -n 20
        tail -n 3 "$build/$k.log"
        timedout $st
        fail "$k"
    fi
done

# --- oracle: Python's integers
for k in kernels_fast kernels_fast32; do
    $limit "$build/$k" dump 3000 > "$build/$k.dump"
    st=$?
    if test $st = 0 && python3 -I "$TESTS/c/oracle.py" < "$build/$k.dump" > "$build/$k.oracle"; then
        echo "== oracle $k: ok, $(cat "$build/$k.oracle")"
    else
        timedout $st || { test -f "$build/$k.oracle" && cat "$build/$k.oracle"; }
        fail "oracle $k"
    fi
done

# --- the plugin through the mock VM
harness() { $limit "$build/plugin_harness" "$@"; }
# donecalls FILE: the call counts of its '# done ...' lines, 'done 84773 calls + 320 same-object calls'
donecalls() { echo "done $(sed -n 's/^# done //p' "$1" | sed -e ':a' -e 'N;s/\n/ + /;ta')"; }
expected=$TESTS/expected/mock-calls.sha256
if test -n "$record"; then
    if harness calls "$build/reference.so" > "$build/reference.calls"; then
        {
            echo "# python3 -I digest.py < (the output of plugin_harness calls), recorded with"
            echo "# run-c-tests.sh --record --reference-c from the LargeIntegers.c before the kernels."
            python3 -I "$TESTS/digest.py" < "$build/reference.calls"
        } > "$expected"
        echo "== calls: recorded $(grep -vc '^#' "$expected") digests of $(donecalls "$build/reference.calls") into $expected"
    else
        fail "calls (the reference crashed or failed, nothing recorded)"
    fi
else
    harness calls "$build/plugin.so" > "$build/plugin.calls" 2> "$build/plugin.calls.err"
    st=$?
    if test $st = 0 &&
       python3 -I "$TESTS/digest.py" --compare "$expected" < "$build/plugin.calls" > "$build/calls.cmp"; then
        echo "== calls: ok, $(donecalls "$build/plugin.calls"), $(cat "$build/calls.cmp")"
    else
        head -n 20 "$build/plugin.calls.err"
        timedout $st
        test -f "$build/calls.cmp" && head -n 20 "$build/calls.cmp"
        fail calls
    fi
fi
if test -n "$reference"; then
    harness diff "$build/reference.so" "$build/plugin.so" > "$build/diff.log" 2>&1
    st=$?
    if test $st = 0; then
        echo "== diff with the reference: ok, $(tail -n 1 "$build/diff.log")"
    else
        head -n 40 "$build/diff.log"
        timedout $st
        fail "diff with the reference"
    fi
fi
harness ub "$build/plugin.so" > "$build/ub.log" 2>&1
st=$?
if test $st = 0; then
    echo "== ub: ok, $(tail -n 1 "$build/ub.log")"
else
    head -n 40 "$build/ub.log"
    timedout $st
    fail ub
fi
harness oom "$build/plugin.so" > "$build/oom.log" 2>&1
st=$?
if test $st = 0; then
    echo "== oom: ok, $(grep -c ' k=' "$build/oom.log") injections, $(tail -n 1 "$build/oom.log" | sed 's/^oom plugin: //')"
else
    grep -E 'CRASH|STACK|REMAP|FAIL [^9]|=> [^F]' "$build/oom.log" | grep -v no-inject | head -n 20
    tail -n 1 "$build/oom.log"
    timedout $st
    fail oom
fi
harness arity "$build/plugin.so" > "$build/arity.log" 2>&1
st=$?
if test $st = 0; then
    echo "== arity: ok, $(grep -c '=> ' "$build/arity.log") calls, $(tail -n 1 "$build/arity.log" | sed 's/^arity plugin: //')"
else
    grep -v '=> FAIL 5$' "$build/arity.log" | head -n 20
    timedout $st
    fail arity
fi

# --- the reference against the same checks, for information only
if test -n "$reference"; then
    echo "-- the reference on the checks above (information only; the plugin before the kernels fails them):"
    for u in li01 li05 li06; do
        harness ub "$build/reference.so" $u > "$build/ref-ub-$u.log" 2>&1
        st=$?
        echo "   ub $u: exit status $st, $(grep -m1 -oE 'AddressSanitizer: [a-z-]+|runtime error: .*|^(ok|FAIL) .*' "$build/ref-ub-$u.log" | head -n 1)"
    done
    harness oom "$build/reference.so" > "$build/ref-oom.log" 2>/dev/null
    echo "   oom: $(grep -c CRASH "$build/ref-oom.log") injections crash, $(tail -n 1 "$build/ref-oom.log" | sed 's/^oom plugin: //')"
    harness arity "$build/reference.so" > "$build/ref-arity.log" 2>&1
    echo "   arity: $(tail -n 1 "$build/ref-arity.log" | sed 's/^arity plugin: //')"
fi

if test $failed = 0; then
    echo "run-c-tests: all checks passed (scale $scale)"
else
    echo "run-c-tests: FAILED (logs in $build)"
    trap - EXIT
fi
exit $failed
