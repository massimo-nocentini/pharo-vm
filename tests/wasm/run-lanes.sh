#!/bin/sh
# run-lanes.sh - the node test lanes of the WebAssembly build (make wasm-check)
#
# usage: sh tests/wasm/run-lanes.sh [LANE ...]
#
# Empties TEST_DIR, then runs every tests/wasm/lanes/*.sh in sorted order,
# with sh, from TEST_DIR and with the environment below exported, and prints
# how many lanes passed, failed and were skipped, and which checks of the
# lanes that ran were skipped (their 'skip [...]' lines, as lib/common.sh
# prints them).  A lane exits 0 when it passes, and 77 when it cannot run
# here, after printing 'skip: <reason>'; any other status is a failure.
# Exits 1 when a lane failed.
#
# LANE ... runs only the lanes whose names start with one of them (20,
# 20-smoke); a lane joins by adding a file to tests/wasm/lanes.
#
# Environment (what make wasm-check passes; the defaults suit build-wasm):
#   NODE        node (default: node)
#   WASM_DIR    the build directory, holding node/, web/ and image/
#               (default: build-wasm in the source tree)
#   GEN         the generated sources of the VM, a directory holding vm/src
#               (default: WASM_DIR/host/generated/64)
#   HOST_PHARO  a native Pharo VM, for what is compared with one; may be empty
#   SRCDIR      the source tree (default: the tree holding this script)
#   TEST_DIR    the scratch directory, emptied first; lanes write nothing
#               anywhere else (default: WASM_DIR/tests-run)
# Relative paths are made absolute (the lanes run in TEST_DIR).
#
# The output of each lane is also kept in TEST_DIR/<lane>.log.
#
# TEST_DIR is emptied only when it is a scratch directory: not /, HOME,
# SRCDIR or WASM_DIR, or a directory above one of them; not in SRCDIR,
# unless in WASM_DIR; holding no source tree; and either empty, made by an
# earlier run (which leaves .run-lanes in it), or WASM_DIR/tests-run, the
# one of make wasm-check; otherwise the runner exits 2.  One run at a time
# uses a TEST_DIR: a run started meanwhile, such as a second make
# wasm-check of the same build, waits for the first one to end.

# canonical PATH: PATH made absolute, with its symbolic links, . and ..
# resolved as far as it exists; fails when . or .. follow a part that does
# not exist
canonical() {
    canonical_dir=$1
    canonical_rest=
    case $canonical_dir in /*) ;; *) canonical_dir=$(pwd)/$canonical_dir ;; esac
    while test "$canonical_dir" != / && case $canonical_dir in */) true ;; *) false ;; esac; do
	canonical_dir=${canonical_dir%/}
    done
    until test -d "$canonical_dir"; do
	case ${canonical_dir##*/} in .|..) return 1 ;; esac
	canonical_rest=/${canonical_dir##*/}$canonical_rest
	canonical_dir=$(dirname "$canonical_dir")
    done
    canonical_dir=$(cd "$canonical_dir" && pwd -P) || return 1
    canonical_dir=${canonical_dir%/}$canonical_rest
    echo "${canonical_dir:-/}"
}

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../..}" && pwd -P)
WASM_DIR=${WASM_DIR:-$SRCDIR/build-wasm}
WASM_DIR=$(canonical "$WASM_DIR") || {
    echo "run-lanes: WASM_DIR has . or .. below a directory that does not exist"
    exit 2
}
if test -z "${GEN+set}" && test -d "$WASM_DIR/host/generated/64"; then
    GEN=$WASM_DIR/host/generated/64
fi
GEN=${GEN-}
HOST_PHARO=${HOST_PHARO-}
NODE=${NODE:-node}
# (a bare command name is left to the PATH)
case $GEN in ''|/*) ;; *) GEN=$(pwd)/$GEN ;; esac
case $HOST_PHARO in ''|/*) ;; *) HOST_PHARO=$(pwd)/$HOST_PHARO ;; esac
case $NODE in /*) ;; */*) NODE=$(pwd)/$NODE ;; esac

refuse() {
    echo "run-lanes: TEST_DIR ($TEST_DIR) must be a scratch directory: $1"
    exit 2
}

# (what of TEST_DIR does not exist yet is made once TEST_DIR is accepted)
TEST_DIR=${TEST_DIR:-$WASM_DIR/tests-run}
dir=$(canonical "$TEST_DIR") || refuse "it has . or .. below a directory that does not exist"
TEST_DIR=$dir
test "$TEST_DIR" != / || refuse "it is /"
real_home=
if test -n "$HOME" && test -d "$HOME"; then real_home=$(cd "$HOME" && pwd -P); fi
for kept in "$SRCDIR" "$WASM_DIR" "$real_home"; do
    test -n "$kept" || continue
    case $kept/ in
	"$TEST_DIR"/*) refuse "it holds $kept" ;;
    esac
done
case $TEST_DIR/ in
    "$WASM_DIR"/*) ;;
    "$SRCDIR"/*) refuse "it is in the source tree $SRCDIR, but not in WASM_DIR ($WASM_DIR)" ;;
esac
if test -d "$TEST_DIR/.git" || test -f "$TEST_DIR/.git" || test -d "$TEST_DIR/tests/wasm/lanes"; then
    refuse "it holds a source tree"
fi
if test -d "$TEST_DIR" && ! test -f "$TEST_DIR/.run-lanes" && test "$TEST_DIR" != "$WASM_DIR/tests-run" &&
    test -n "$(ls -A "$TEST_DIR")"; then
    refuse "it is not empty, and no earlier run made it"
fi
mkdir -p "$TEST_DIR" || exit 1

# One run at a time: the lock is a symbolic link, made atomically, to the
# PID of its run, and left behind only by a run that was killed.  Anything
# else of that name is refused (ln would make the link in a directory of
# that name, and every run would hold the lock), and so is a TEST_DIR where
# the link cannot be made (read-only or full) after a few tries.
lock=$TEST_DIR/.run-lanes.lock
waited=no
lock_tries=0
while :; do
    if test -e "$lock" && ! test -L "$lock"; then
	echo "run-lanes: $lock is not the lock of a run (a symbolic link to its PID): remove it"
	exit 2
    fi
    if ln_error=$(ln -s "$$" "$lock" 2>&1); then break; fi
    holder=$(ls -l "$lock" 2>/dev/null | sed -n 's/.* -> //p')
    if test -n "$holder" && kill -0 "$holder" 2>/dev/null; then
	if test $waited = no; then
	    echo "run-lanes: waiting for the run $holder, which uses $TEST_DIR (if it is gone, remove $lock)"
	    waited=yes
	fi
	sleep 2
    elif test -n "$holder"; then
	# (a lock whose run is gone, unless another run just took its place)
	if test "$(ls -l "$lock" 2>/dev/null | sed -n 's/.* -> //p')" = "$holder" &&
	   ! rm_error=$(rm -f "$lock" 2>&1); then
	    echo "run-lanes: cannot remove the stale lock $lock: $rm_error"
	    exit 2
	fi
    else
	# (no lock, yet ln failed: unless the lock just went, it cannot be made)
	lock_tries=$((lock_tries + 1))
	if test "$lock_tries" -ge 5; then
	    echo "run-lanes: cannot make the lock $lock: ${ln_error:-ln failed}"
	    exit 2
	fi
	sleep 1
    fi
done
trap 'rm -f "$lock"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

for f in "$TEST_DIR"/* "$TEST_DIR"/.[!.]* "$TEST_DIR"/..?*; do
    if test "$f" != "$lock" && { test -e "$f" || test -L "$f"; }; then
	rm -rf "$f" || exit 1
    fi
done
echo "made by tests/wasm/run-lanes.sh, which empties it on every run" >"$TEST_DIR/.run-lanes" || exit 1
export NODE WASM_DIR GEN HOST_PHARO SRCDIR TEST_DIR

passed=0
failed=0
skipped=0
failures=
skips=
check_skips=

for lane in "$SRCDIR"/tests/wasm/lanes/*.sh; do
    test -f "$lane" || continue
    name=$(basename "$lane" .sh)
    if test $# -ne 0; then
	selected=no
	for prefix in "$@"; do
	    case $name in "$prefix"*) selected=yes ;; esac
	done
	test $selected = yes || continue
    fi
    echo "=== $name"
    log=$TEST_DIR/$name.log
    start=$(date +%s)
    { (cd "$TEST_DIR" && sh "$lane" </dev/null); echo $? >"$log.status"; } 2>&1 | tee "$log"
    status=$(cat "$log.status")
    rm -f "$log.status"
    seconds=$(($(date +%s) - start))
    lane_skips=$(awk -v lane="$name" 'sub(/^skip \[/, "[") { print "  " lane ": " $0 }' "$log")
    if test -n "$lane_skips"; then check_skips="$check_skips
$lane_skips"; fi
    case $status in
	0)
	    passed=$((passed + 1))
	    echo "--- $name: passed (${seconds} s)" ;;
	77)
	    skipped=$((skipped + 1))
	    reason=$(sed -n 's/^skip: //p' "$log" | tail -n 1)
	    skips="$skips
  $name: ${reason:-no reason given}"
	    echo "--- $name: skipped (${reason:-no reason given})" ;;
	*)
	    failed=$((failed + 1))
	    failures="$failures $name"
	    echo "--- $name: FAILED with status $status (${seconds} s; log in $log)" ;;
    esac
    echo
done

echo "run-lanes: $passed passed, $failed failed, $skipped skipped"
if test -n "$skips"; then echo "skipped:$skips"; fi
if test -n "$check_skips"; then echo "skipped checks:$check_skips"; fi
if test "$failed" -ne 0; then
    echo "failed:$failures"
    exit 1
fi
if test $((passed + skipped)) -eq 0; then
    echo "run-lanes: no lane matches: $*"
    exit 1
fi
