#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the SimdJSONPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image> [compat]
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 15 image, zipped as
# build-wasm/images/p15/pharo15.zip or not.  The script works on a copy of
# the image in a directory of its own (SJ_WORK_DIR, or a new one in TMPDIR),
# and:
#
#   load   load-simdjson.st loads the Smalltalk code with Metacello from the
#          submodule plugins/SimdJSONPlugin/simdjson.st (git submodule update
#          --init): the groups Core, Tests, FFI and FFITests of
#          BaselineOfSimdJSON, and
#          SimdJSONReader must be under SimdJSONMapper; compat: all of them,
#          NeoTests too, with NeoJSON and its tests from the clone that
#          NEOJSON_DIR names (only to compare with), and the SJ* classes that
#          SimdJSON-NeoTests generates from NeoJSON's tests;
#   test   the packages SimdJSON-Tests (SimdJSONSlowTests are skipped unless
#          SJ_SLOW is set) and SimdJSON-FFI-Tests, which reads through
#          libSimdJSONFFI with the plugin hidden (skipped when <vm-dir> has
#          no libSimdJSONFFI); compat: SimdJSON-NeoTests and the package of the
#          SJ* classes, SimdJSON-NeoTests-Generated, then the report of the
#          SJ* classes (SimdJSONNeoTestGenerator report): what passed, and
#          which tests NeoJSON passes only by its leniency.
#
# A JUnit file of each package is left in the directory.  The status is 0
# when every step passed.
#
# Metacello makes UUIDs: <vm-dir> needs the UUIDPlugin.
#
# Environment: NEOJSON_DIR (compat), SJ_CORPUS (the jsonexamples directory of
# simdjson-data, for the corpus and jsonchecker tests of SimdJSON-NeoTests),
# SJ_SLOW, SJ_NUM_TOKENS (100000 random number tokens by default),
# SJ_WORK_DIR, SJ_TIMEOUT (seconds a step may take, 3600 by default).

if test $# -lt 2 || test $# -gt 3; then
    echo "usage: $0 <vm-dir> <image.zip | X.image> [compat]" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
COMPAT=${3:-}
PLUGIN=$(cd "$(dirname "$0")/.." && pwd)

case $COMPAT in
'')     load_groups='Core Tests FFI FFITests' ;;
compat) load_groups='default' ;;
*)      echo "the third argument is compat or nothing, not '$COMPAT'" >&2; exit 2 ;;
esac
if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if ! test -d "$PLUGIN/simdjson.st/src/BaselineOfSimdJSON"; then
    echo "FAIL: no $PLUGIN/simdjson.st/src: git submodule update --init plugins/SimdJSONPlugin/simdjson.st" >&2
    exit 1
fi
if test -n "$COMPAT" && ! test -d "$NEOJSON_DIR/repository"; then
    echo "FAIL: NEOJSON_DIR must name a clone of NeoJSON (with its repository directory)" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${SJ_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/sj-st-tests.XXXXXX")} || exit 1
mkdir -p "$WORK" || exit 1
WORK=$(cd "$WORK" && pwd)
case $IMAGE in
*.zip)
    mkdir -p "$WORK/zip" && unzip -q -o "$IMAGE" -d "$WORK/zip" || exit 1
    image=$(ls "$WORK"/zip/*.image | head -1)
    ;;
*.image)
    image=$IMAGE
    ;;
*)
    echo "FAIL: $IMAGE is neither a .zip nor a .image" >&2
    exit 1
    ;;
esac
if ! test -f "$image"; then
    echo "FAIL: no image in $IMAGE" >&2
    exit 1
fi
cp "$image" "$WORK/SimdJSON.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/SimdJSON.changes"; else : >"$WORK/SimdJSON.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip" "$WORK/PharoDebug.log"
echo "image: $image, copied to $WORK/SimdJSON.image"
echo "vm: $VM_DIR/pharo"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${SJ_TIMEOUT:-3600}"; fi
pharo() {
    (cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$VM_DIR/pharo" --headless SimdJSON.image --no-default-preferences "$@")
}

# load: the groups, saved
(if test -z "$COMPAT"; then unset NEOJSON_DIR; fi
 SJ_PLUGIN_DIR=$PLUGIN SJ_GROUPS=$load_groups; export SJ_PLUGIN_DIR SJ_GROUPS
 pharo st --save --quit "$PLUGIN/tests/load-simdjson.st") >"$WORK/load.out" 2>&1
status=$?
sed 's/^/    /' "$WORK/load.out"
if test $status -ne 0 || test -f "$WORK/PharoDebug.log"; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
if ! grep "SimdJSON: SimdJSONReader loaded, a subclass of SimdJSONMapper" "$WORK/load.out" >/dev/null; then
    fail "load: SimdJSONReader is not a subclass of SimdJSONMapper"
    exit 1
fi
echo "load: $load_groups ($(git -C "$PLUGIN/simdjson.st" log -1 --format=%h 2>/dev/null || echo 'simdjson.st'))"

# test: one package a run, then the summary of its JUnit file
run_package() {
    package=$1
    pharo test --junit-xml-output --fail-on-failure --fail-on-error "$package" >"$WORK/$package.out" 2>&1
    status=$?
    sed 's/^/    /' "$WORK/$package.out"
    xml=$WORK/$package-Test.xml
    if ! test -f "$xml"; then
	fail "test: $package (exit status $status, no $xml)"
	return
    fi
    awk '
	/<testsuite / {
	    match($0, /tests="[0-9]*"/); tests = substr($0, RSTART + 7, RLENGTH - 8)
	    match($0, /failures="[0-9]*"/); failures = substr($0, RSTART + 10, RLENGTH - 11)
	    match($0, /errors="[0-9]*"/); errors = substr($0, RSTART + 8, RLENGTH - 9)
	}
	/<testcase / {
	    match($0, /classname="[^"]*"/); class = substr($0, RSTART + 11, RLENGTH - 12)
	    sub(/.*\./, "", class)
	    match($0, / name="[^"]*"/); name = substr($0, RSTART + 7, RLENGTH - 8)
	}
	/<(failure|error) / {
	    match($0, /message="[^"]*"/)
	    print "    " (/<failure/ ? "failure" : "error") ": " class ">>" name " " substr($0, RSTART + 9, RLENGTH - 10)
	}
	END { print tests " " failures " " errors > "/dev/stderr" }' "$xml" 2>"$WORK/$package.counts"
    set -- $(cat "$WORK/$package.counts")
    echo "test: $package: $1 run, $2 failures, $3 errors"
    if test $status -ne 0 || test "${1:-0}" -eq 0 || test "$2" -ne 0 || test "$3" -ne 0; then
	fail "test: $package (exit status $status)"
    fi
}

run_package SimdJSON-Tests
run_package SimdJSON-FFI-Tests
if test -n "$COMPAT"; then
    run_package SimdJSON-NeoTests
    run_package SimdJSON-NeoTests-Generated
    pharo eval 'SimdJSONNeoTestGenerator report' >"$WORK/report.out" 2>&1
    sed 's/^/    /' "$WORK/report.out"
    total=$(sed -n "s/^'SJ\*: \([0-9]*\)\/\([0-9]*\) passed'$/\1 \2/p" "$WORK/report.out")
    set -- $total
    if test $# -ne 2 || test "$1" -ne "$2" || test "$2" -eq 0; then
	fail "report: SJ* classes (${1:-?}/${2:-?} passed)"
    else
	echo "report: SJ*: $1/$2 passed"
    fi
fi

if test $failed = 0; then
    echo "PASS${COMPAT:+ $COMPAT}: $VM_DIR/pharo ($WORK)"
fi
exit $failed
