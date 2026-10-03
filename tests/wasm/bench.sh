#!/bin/sh
# bench.sh - performance and regression guard of the WebAssembly (node) build
# of the Pharo VM
#
# usage: sh tests/wasm/bench.sh
#
# Times eval '3+4' three times and runs a benchmark script once with
# build-wasm/node/pharo, each on a fresh copy of the stock image, then prints
# the measures next to the reference numbers of the design, measured on the
# machine the port was made on:
#   critic    the return-to-host prototype of the design review (-O2,
#             NDEBUG, TABLE_BASE=1024, -sMEMORY64=1, no function pointer cast
#             emulation)
#   e2e -m64  the end-to-end build of the design, with -m64
#   native    the native StackVM (-O2 -DNDEBUG)
#
# These fail the bench, the limits being overridable:
#   - 300k-deep block recursion takes WASM_BENCH_RECURSION_MS (1000) or
#     more: the C asserts are on (WASM_DEBUG=1, about 16 s), or the
#     interpreter is otherwise slowed down;
#   - eval '3+4' takes WASM_BENCH_EVAL_MS (1500) or more of wall time, at
#     best;
#   - the benchmark script, more than 2 s of computing, runs in fewer than
#     WASM_BENCH_SLICES (10) slices: the VM no longer returns to its host,
#     whose page would freeze.
# The first two are measured again while over their limit, up to
# WASM_CHECK_TRIES (3) times in all (three more evals each time, or a
# recursion alone), and the best time counts: a loaded machine slows a
# run down, never speeds one up.
# These only warn, unless WASM_BENCH_STRICT=1:
#   - tinyBenchmarks below WASM_BENCH_BYTECODES (100000000) bytecodes/s or
#     WASM_BENCH_SENDS (6000000) sends/s: an unoptimised build, or the
#     interpreter left running in Liftoff code (node enters a function again
#     to run its optimised code, which a VM that never returns misses).
# A last check, wall-limit, checks that a command timed out by wall (see
# below) is stopped with its children.
#
# Environment: WASM_DIR, TEST_DIR, NODE, SRCDIR, WASM_CHECK_TIMEOUT and
# WASM_CHECK_TRIES, as for wasm-smoke.sh.

SRCDIR=${SRCDIR:-$(dirname "$0")/../..}
. "$SRCDIR/tests/wasm/lib/common.sh"

recursion_limit=${WASM_BENCH_RECURSION_MS:-1000}
eval_limit=${WASM_BENCH_EVAL_MS:-1500}
min_slices=${WASM_BENCH_SLICES:-10}
min_bytecodes=${WASM_BENCH_BYTECODES:-100000000}
min_sends=${WASM_BENCH_SENDS:-6000000}
strict=${WASM_BENCH_STRICT:-0}
lane=bench

if ! test -x "$WASM_DIR/node/pharo"; then
    echo "bench: no $WASM_DIR/node/pharo (make wasm builds it)"
    exit 1
fi

# wall FILE CMD ...: runs CMD, appending its wall time in milliseconds to
# FILE, and stops it after $wall_limit ($timeout) seconds, with status 124,
# as limit does.  (limit would stop the node that times CMD, not CMD, its
# child: so that node times CMD out itself.  CMD leads a process group of
# its own, which is stopped whole: after a timeout, and when that node is
# interrupted or terminated.)
wall_limit=$timeout
wall() {
    wall_file=$1; shift
    "$NODE" -e '
const { spawn } = require("child_process");
const { signals } = require("os").constants;
const [file, seconds, cmd, ...args] = process.argv.slice(1);
const t0 = process.hrtime.bigint();
const child = spawn(cmd, args, { stdio: "inherit", detached: true });
const stop = signal => { try { process.kill(-child.pid, signal); } catch (e) {} };
let timedOut = false;
const timer = setTimeout(() => {
  timedOut = true;
  stop("SIGTERM");
  setTimeout(() => stop("SIGKILL"), 5000).unref();
}, Number(seconds) * 1000);
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"])
  process.on(signal, () => { stop("SIGKILL"); process.exit(128 + signals[signal]); });
child.on("error", e => { clearTimeout(timer); console.error(`wall: ${cmd}: ${e.message}`); process.exitCode = 127; });
child.on("exit", (code, signal) => {
  clearTimeout(timer);
  if (timedOut) {
    stop("SIGKILL");
    process.exitCode = 124;
    return;
  }
  require("fs").appendFileSync(file, `${Math.round(Number(process.hrtime.bigint() - t0) / 1e6)}\n`);
  process.exitCode = code === null ? 128 + signals[signal] : code;
});' "$wall_file" "$wall_limit" "$@"
}

# millions N: N in millions, with one decimal
millions() {
    echo "$(($1 / 1000000)).$(($1 / 100000 % 10))M"
}

# best FILE: the least of the numbers in FILE, one a line
best() {
    sort -n "$1" 2>/dev/null | head -n 1
}

fresh eval
evals() {
    for i in 1 2 3; do
	wall eval.ms "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences eval '3+4' || return
    done
}
expect eval '7
7
7' evals
eval_ms=$(best eval.ms)
# (three more while the best is over the limit, unless eval failed)
while test "$check_failed" -eq 0 && is_number "$eval_ms" && test "$eval_ms" -ge "$eval_limit" &&
    again eval-wall "eval '3+4' took $eval_ms ms at best"; do
    run eval evals || break
    if test "$status" -ne 0 || test "x$(cat smoke.out)" != 'x7
7
7'; then
	fail eval "run $try: exit status $status, stdout '$(cat smoke.out)'"
	break
    fi
    eval_ms=$(best eval.ms)
done
eval_runs=$(grep -c . eval.ms 2>/dev/null)

fresh script
cat >bench.st <<'EOF'
| line tiny arrays block |
line := [ :key :value |
	Stdio stdout nextPutAll: key; nextPut: Character space; nextPutAll: value asString; lf; flush ].
tiny := 1 tinyBenchmarks substrings: ' ;'.
line value: 'bytecodes' value: tiny first.
line value: 'sends' value: (tiny at: 3).
line value: 'benchfib' value: [ 26 benchFib ] timeToRun asMilliSeconds.
line value: 'inject' value: [
	20 timesRepeat: [ (1 to: 100000) inject: 0 into: [ :sum :each | sum + each ] ] ] timeToRun asMilliSeconds.
line value: 'gcstress' value: [
	arrays := OrderedCollection new.
	1 to: 3000000 do: [ :i | arrays add: (Array new: 3) ].
	Smalltalk garbageCollect ] timeToRun asMilliSeconds.
arrays := nil.
Smalltalk garbageCollect.
block := nil.
block := [ :n | n = 0 ifTrue: [ 0 ] ifFalse: [ 1 + (block value: n - 1) ] ].
line value: 'recursion300k' value: [ block value: 300000 ] timeToRun asMilliSeconds.
line value: 'recursion1m' value: [ block value: 1000000 ] timeToRun asMilliSeconds.
EOF
script() (
    PHARO_WASM_STATS=1; export PHARO_WASM_STATS
    limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences st --quit bench.st
)
measures='bytecodes sends benchfib inject gcstress recursion300k recursion1m'
# (run itself fails the check on an engine error or a timeout; what was
# measured until then is still shown and guarded.)
stopped=no
run script script || stopped=yes
for key in $measures; do
    value=$(sed -n "s/^$key \\([0-9][0-9]*\\)\$/\\1/p" smoke.out)
    eval "$key=\$value"
done
slices=$(sed -n 's/.*slices=\([0-9]*\).*/\1/p' smoke.err | tail -n 1)
missing=
for key in $measures slices; do
    eval "value=\$$key"
    is_number "$value" || missing="$missing $key"
done
if test $stopped = yes; then
    :
elif test "$status" -ne 0; then
    fail script "exit status $status"
elif test -n "$missing"; then
    fail script "no measure of$missing"
else
    ok script
fi

# The recursion alone again, on a fresh image, while over the limit
recursion_runs=1
if test $stopped = no && is_number "$recursion300k" && test "$recursion300k" -ge "$recursion_limit"; then
    fresh recursion
    while again recursion "300k-deep recursion took $recursion300k ms at best"; do
	run recursion limit "$timeout" "$WASM_DIR/node/pharo" --headless Pharo.image --no-default-preferences eval \
	    '| block | block := [ :n | n = 0 ifTrue: [ 0 ] ifFalse: [ 1 + (block value: n - 1) ] ].
	    [ block value: 300000 ] timeToRun asMilliSeconds' || break
	value=$(cat smoke.out)
	if test "$status" -ne 0 || ! is_number "$value"; then
	    fail recursion "the recursion alone: exit status $status, '$value'"
	    break
	fi
	recursion_runs=$try
	if test "$value" -lt "$recursion300k"; then recursion300k=$value; fi
	test "$recursion300k" -ge "$recursion_limit" || break
    done
fi

# The table, with '?' for what could not be measured
row() {
    printf '%-30s %11s %9s %9s %9s%s\n' "$1" "$2" "$3" "$4" "$5" "${6:+  $6}"
}
show() {
    if is_number "$1"; then echo "$1$2"; else echo '?'; fi
}
showm() {
    if is_number "$1"; then millions "$1"; else echo '?'; fi
}
recursion_label='300k-deep block recursion'
if test "$recursion_runs" -gt 1; then recursion_label="300k-deep recursion, best of $recursion_runs"; fi
echo
row measure 'this build' critic 'e2e -m64' native guard
row "eval '3+4' wall, best of ${eval_runs:-0}" "$(show "$eval_ms" ' ms')" '350 ms' - '80 ms' "< $eval_limit ms"
row "$recursion_label" "$(show "$recursion300k" ' ms')" - '225 ms' - "< $recursion_limit ms"
row '1M-deep block recursion' "$(show "$recursion1m" ' ms')" '987 ms' - '766 ms'
row 'tinyBenchmarks bytecodes/s' "$(showm "$bytecodes")" 167.0M 154.4M 941.0M ">= $(millions "$min_bytecodes") (warning)"
row 'tinyBenchmarks sends/s' "$(showm "$sends")" 10.1M 9.5M 48.0M ">= $(millions "$min_sends") (warning)"
row '26 benchFib' "$(show "$benchfib" ' ms')" '39 ms' - '8 ms'
row '20 x 100000 inject:into:' "$(show "$inject" ' ms')" '535 ms' - '120 ms'
row 'GC stress, 3M arrays' "$(show "$gcstress" ' ms')" '2560 ms' - '1460 ms'
row 'slices of the benchmark run' "$(show "$slices")" - - - ">= $min_slices"
echo
retire

# guard NAME CONDITION REASON: a hard guard
guard() {
    checks=$((checks + 1))
    if eval "$2"; then ok "$1"; else fail "$1" "$3"; fi
}
# soft NAME CONDITION REASON: a warning, a failure with WASM_BENCH_STRICT=1
soft() {
    checks=$((checks + 1))
    if eval "$2"; then
	ok "$1"
    elif test "$strict" = 1; then
	fail "$1" "$3 (WASM_BENCH_STRICT=1)"
    else
	echo "warn [$lane] $1: $3"
    fi
}

guard eval-wall 'is_number "$eval_ms" && test "$eval_ms" -lt "$eval_limit"' \
    "eval '3+4' took ${eval_ms:-?} ms at best of ${eval_runs:-0}, $eval_limit or more"
guard recursion 'is_number "$recursion300k" && test "$recursion300k" -lt "$recursion_limit"' \
    "300k-deep recursion took ${recursion300k:-?} ms at best of $recursion_runs, $recursion_limit or more"
guard slices 'is_number "$slices" && test "$slices" -ge "$min_slices"' \
    "the benchmark run took ${slices:-?} slices, fewer than $min_slices"
soft bytecodes 'is_number "$bytecodes" && test "$bytecodes" -ge "$min_bytecodes"' \
    "${bytecodes:-?} bytecodes/s, fewer than $min_bytecodes"
soft sends 'is_number "$sends" && test "$sends" -ge "$min_sends"' \
    "${sends:-?} sends/s, fewer than $min_sends"

# wall-limit: wall stops a command still running after its limit (1 s
# here) with status 124, and the children of that command with it
fresh wall-limit
checks=$((checks + 1))
status=0
(wall_limit=1; wall wall.ms sh -c 'sleep 60 & echo $! >sleeper; wait') </dev/null >smoke.out 2>smoke.err || status=$?
sleeper=$(cat sleeper 2>/dev/null)
# (a child already stopped may be a zombie for a moment)
alive() {
    kill -0 "$1" 2>/dev/null && case $(ps -o stat= -p "$1" 2>/dev/null) in Z*) false ;; *) true ;; esac
}
for i in 1 2 3 4 5; do
    is_number "$sleeper" && alive "$sleeper" || break
    sleep 1
done
if test "$status" -ne 124; then
    fail wall-limit "exit status $status, expected 124"
elif ! is_number "$sleeper"; then
    fail wall-limit "the command did not start its child"
elif alive "$sleeper"; then
    kill -9 "$sleeper" 2>/dev/null
    fail wall-limit "the child $sleeper of the command outlived it"
else
    ok wall-limit
fi

finish bench
