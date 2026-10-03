# common.sh - helpers of the node test lanes of the WebAssembly build
#
# Sourced by wasm-smoke.sh and bench.sh, and by any lane (tests/wasm/lanes)
# that wants its helpers, once SRCDIR is set.  It gives the environment of
# `make wasm-check' (see run-lanes.sh) its defaults, makes its paths
# absolute, and defines
#
#   skip REASON            a lane that cannot run here: prints the reason, exits 77
#   limit SECONDS CMD ...  CMD (a command, not a function), stopped after SECONDS
#   copy_image FROM DIR    a copy of the image in directory FROM as DIR/Pharo.image
#                          and DIR/Pharo.changes, with a link to its .sources
#   fresh NAME [FROM]      starts check NAME in a directory of its own,
#                          TEST_DIR/<lane>-NAME, on a fresh copy of the image in
#                          FROM (WASM_DIR/image/stock); the directory of the
#                          check before is removed, unless that check failed
#                          (which keeps it, less an unchanged image)
#   measure FILE ARG ...   pharo.js in this node with ARG ... as its command
#                          line; appends '<wall ms> <CPU ms>' to FILE at its exit
#   finish WHAT            prints the summary of WHAT, exits 1 after any failure
#
# and the checks, which run in the current directory and leave the output of
# their command in smoke.out and smoke.err:
#
#   run NAME CMD ...                    runs CMD, setting status
#   expect NAME EXPECTED CMD ...        stdout must be EXPECTED, status 0
#   expect_quiet NAME EXPECTED CMD ...  stdout must be exactly EXPECTED and a
#                                       newline, stderr empty, status 0
#   expect_match NAME REGEX CMD ...     a line of stdout must match REGEX, status 0
#   expect_range NAME LOW HIGH CMD ...  stdout must be an integer in LOW..HIGH
#   expect_time NAME LOW HIGH CMD ...   the same for a time, which is measured
#                                       again when over HIGH (see again)
#   expect_status NAME STATUS CMD ...   the exit status must be STATUS
#   expect_error NAME REGEX CMD ...     must fail with REGEX on stderr
#   ok NAME [NOTE], fail NAME REASON, skip_check NAME REASON
#   again NAME WHAT                     true when a check whose time was over
#                                       its limit (WHAT) may run once more
#   least TIME                          keeps the best time of a check in
#                                       best_time
#
# Any engine-level failure on stderr (engine_errors) fails a check, and so
# does a command that limit stopped (status 124).
#
# A check that measures a time (a wall or a CPU time) runs again when that
# time is over its limit, up to WASM_CHECK_TRIES (3) runs in all, and the
# best run counts: a loaded machine, running other make wasm-check runs or
# a build, slows a run down but never speeds one up.  Any other failure
# fails at once.

SRCDIR=$(cd "${SRCDIR:?SRCDIR must name the source tree}" && pwd)
WASM_DIR=${WASM_DIR:-$SRCDIR/build-wasm}
case $WASM_DIR in /*) ;; *) WASM_DIR=$(pwd)/$WASM_DIR ;; esac
if test -d "$WASM_DIR"; then WASM_DIR=$(cd "$WASM_DIR" && pwd); fi
TEST_DIR=${TEST_DIR:-$WASM_DIR/tests-run}
mkdir -p "$TEST_DIR" && TEST_DIR=$(cd "$TEST_DIR" && pwd) || exit 1
NODE=${NODE:-node}
HOST_PHARO=${HOST_PHARO-}
# (the checks run in directories of their own: a relative path is made
# absolute, a bare command name is left to the PATH)
case $NODE in /*) ;; */*) NODE=$(pwd)/$NODE ;; esac
case $HOST_PHARO in ''|/*) ;; *) HOST_PHARO=$(pwd)/$HOST_PHARO ;; esac
export SRCDIR WASM_DIR TEST_DIR NODE HOST_PHARO

# seconds one VM run may take
timeout=${WASM_CHECK_TIMEOUT:-120}
# runs a check that measures a time may take (at least one)
tries=${WASM_CHECK_TRIES:-3}
case $tries in ''|*[!0-9]*|0) tries=1 ;; esac

# (node-pump.js prints the stack of what _vm_resume threw: a trap, abort(),
# or an error of the JavaScript runtime, and exits 1, as exitFailure does.)
engine_errors='RuntimeError|RangeError|Aborted|unreachable|signature_mismatch|unsupported syscall|TypeError|ReferenceError'

skip() {
    echo "skip: $1"
    exit 77
}

# (--foreground keeps the command in the process group of the terminal, so
# that ^C stops it too.  Only the command itself is then timed out, not its
# children: that is node, which the launcher execs.  A command that runs the
# VM in a child must time it out itself, as wall in bench.sh does.)
if timeout --foreground -k 5 10 true 2>/dev/null; then
    limit() {
	limit_seconds=$1; shift
	timeout --foreground -k 5 "$limit_seconds" "$@"
    }
elif command -v timeout >/dev/null 2>&1; then
    limit() {
	limit_seconds=$1; shift
	timeout "$limit_seconds" "$@"
    }
else
    limit() { shift; "$@"; }
fi

is_number() {
    case $1 in
	''|*[!0-9]*) return 1 ;;
	*) return 0 ;;
    esac
}

copy_image() {
    source_image=
    for f in "$1"/*.image; do
	if test -f "$f"; then source_image=$f; fi
    done
    if test -z "$source_image"; then
	echo "no image in $1" >&2
	return 1
    fi
    cp "$source_image" "$2/Pharo.image" && cp "${source_image%.image}.changes" "$2/Pharo.changes" || return 1
    for f in "$1"/*.sources; do
	if test -f "$f"; then ln -s "$f" "$2/${f##*/}" || return 1; fi
    done
}

measure() {
    measure_file=$1; shift
    limit "$timeout" "$NODE" -e '
const t0 = process.hrtime.bigint();
const [file, vm, ...args] = process.argv.slice(1);
process.on("exit", () => {
  const cpu = process.cpuUsage();
  const wall = Number(process.hrtime.bigint() - t0) / 1e6;
  require("fs").appendFileSync(file, `${Math.round(wall)} ${Math.round((cpu.user + cpu.system) / 1000)}\n`);
});
process.argv = [process.execPath, vm, ...args];
require(vm);' "$measure_file" "$WASM_DIR/node/pharo.js" "$@"
}

lane=default
checks=0
failures=0
skipped=0
check_dir=
check_failed=0
try=1
best_time=

# (A failed check keeps its directory, less a copy of the image that is still
# the image it was copied from, and with its crash dump and stderr cut to
# their first megabyte: a VM that dies deep in a recursion prints every
# frame, hundreds of megabytes.)
retire() {
    if test -n "$check_dir"; then
	cd "$TEST_DIR" || exit 1
	if test "$check_failed" -eq 0; then
	    rm -rf "$check_dir"
	else
	    if test -n "$source_image" && cmp -s "$source_image" "$check_dir/Pharo.image"; then
		rm -f "$check_dir/Pharo.image"
	    fi
	    for f in "$check_dir/crash.dmp" "$check_dir/smoke.err"; do
		if test -f "$f" && test "$(wc -c <"$f")" -gt 1048576; then
		    dd if="$f" of="$f.head" bs=1024 count=1024 2>/dev/null && mv "$f.head" "$f"
		fi
	    done
	fi
    fi
    check_dir=
    check_failed=0
    try=1
    best_time=
}

fresh() {
    retire
    check_dir=$TEST_DIR/$lane-$1
    rm -rf "$check_dir"
    mkdir -p "$check_dir" && cd "$check_dir" || exit 1
    copy_image "${2:-$WASM_DIR/image/stock}" "$check_dir"
}

finish() {
    retire
    rm -f smoke.out smoke.err
    echo
    note=
    if test "$skipped" -ne 0; then note=" ($skipped skipped)"; fi
    if test "$failures" -ne 0; then
	echo "$1: $failures of $checks checks FAILED$note"
	exit 1
    fi
    echo "$1: all $checks checks passed$note"
}

ok() {
    echo "ok   [$lane] $1${2:+ ($2)}"
}

fail() {
    failures=$((failures + 1))
    check_failed=1
    echo "FAIL [$lane] $1: $2"
    for f in smoke.out smoke.err; do
	if test -s $f; then echo "  --- $f:"; head -20 $f | cut -c 1-300 | sed 's/^/  | /'; fi
    done
    if test -n "$check_dir"; then echo "  (kept $check_dir)"; fi
}

skip_check() {
    skipped=$((skipped + 1))
    echo "skip [$lane] $1: $2"
}

# again NAME WHAT: for when run number $try of check NAME measured WHAT, a
# time over its limit.  While fewer than $tries runs were made, it says so,
# moves on to the next run and answers true, for the check to run again;
# otherwise it answers false.  (fresh starts a check at its first run, and
# run counts a check at its first run only.)
again() {
    if test "$try" -lt "$tries"; then
	try=$((try + 1))
	echo "slow [$lane] $1: $2; run $try of $tries"
	return 0
    fi
    return 1
}

# least TIME: best_time becomes TIME, when less than it or unset
least() {
    if test -z "$best_time" || test "$1" -lt "$best_time"; then best_time=$1; fi
}

# tried and best_of: what is said of a check that took more than one run,
# when it passes and when it fails (with the best of its times)
tried() {
    if test "$try" -gt 1; then echo ", after $try runs"; fi
}
best_of() {
    if test "$try" -gt 1; then echo " (the best of $try runs)"; fi
}

run() {
    name=$1; shift
    if test "$try" -eq 1; then checks=$((checks + 1)); fi
    if "$@" </dev/null >smoke.out 2>smoke.err; then status=0; else status=$?; fi
    if grep -Eq "$engine_errors" smoke.err; then
	fail "$name" "engine error on stderr"
	return 1
    fi
    if test "$status" -eq 124; then
	fail "$name" "timed out"
	return 1
    fi
    return 0
}

expect() {
    name=$1; expected=$2; shift 2
    run "$name" "$@" || return 0
    out=$(cat smoke.out)
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif test "x$out" != "x$expected"; then
	fail "$name" "expected '$expected', got '$out'"
    else
	ok "$name"
    fi
}

expect_quiet() {
    name=$1; expected=$2; shift 2
    run "$name" "$@" || return 0
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif ! printf '%s\n' "$expected" | cmp -s - smoke.out; then
	fail "$name" "stdout is not exactly '$expected' and a newline"
    elif test -s smoke.err; then
	fail "$name" "stderr is not empty"
    else
	ok "$name"
    fi
}

expect_match() {
    name=$1; regex=$2; shift 2
    run "$name" "$@" || return 0
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif ! grep -Eq "$regex" smoke.out; then
	fail "$name" "stdout does not match '$regex'"
    else
	ok "$name" "$(head -n 1 smoke.out)"
    fi
}

expect_range() {
    name=$1; low=$2; high=$3; shift 3
    run "$name" "$@" || return 0
    out=$(cat smoke.out)
    if test "$status" -ne 0; then
	fail "$name" "exit status $status"
    elif ! is_number "$out" || test "$out" -lt "$low" || test "$out" -gt "$high"; then
	fail "$name" "expected $low..$high, got '$out'"
    else
	ok "$name" "$out"
    fi
}

expect_time() {
    name=$1; low=$2; high=$3; shift 3
    while run "$name" "$@"; do
	out=$(cat smoke.out)
	if test "$status" -ne 0; then
	    fail "$name" "exit status $status"
	elif ! is_number "$out" || test "$out" -lt "$low"; then
	    fail "$name" "expected $low..$high, got '$out'"
	elif test "$out" -gt "$high"; then
	    least "$out"
	    again "$name" "$out, more than $high" && continue
	    fail "$name" "expected $low..$high, got $best_time$(best_of)"
	else
	    ok "$name" "$out$(tried)"
	fi
	return 0
    done
}

expect_status() {
    name=$1; expected=$2; shift 2
    run "$name" "$@" || return 0
    if test "$expected" = nonzero && test "$status" -ne 0; then
	ok "$name"
    elif test "$expected" = "$status"; then
	ok "$name"
    else
	fail "$name" "exit status $status, expected $expected"
    fi
}

expect_error() {
    name=$1; regex=$2; shift 2
    run "$name" "$@" || return 0
    if test "$status" -eq 0; then
	fail "$name" "unexpected success"
    elif ! grep -Eq "$regex" smoke.err; then
	fail "$name" "stderr does not match '$regex'"
    else
	ok "$name"
    fi
}
