#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the SimdJSONPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image> <standalone | neo>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 15 image, zipped as
# build-wasm/images/p15/pharo15.zip or not.  The script works on a copy of
# the image in a directory of its own (SJ_WORK_DIR, or a new one in TMPDIR),
# and:
#
#   lint   every .st file it files in is ASCII, and has one method a
#          methodsFor: section (Pharo 15 reads only the first one);
#   load   neo: NeoJSON first, with load-neojson.st (NEOJSON_DIR names the
#          clone); then SimdJSON.st, whose banner must name the mode, and
#          SimdJSON-Tests.st; neo: SimdJSON-NeoTests.st, which generates the
#          SJ* classes from NeoJSON's tests;
#   test   the package SimdJSON-Tests (SimdJSONSlowTests are skipped unless
#          SJ_SLOW is set); neo: SimdJSON-NeoTests, then the report of the
#          SJ* classes (SimdJSONNeoTestGenerator report): what passed, and
#          how many values NeoJSON read again.
#
# A JUnit file of each package is left in the directory.  The status is 0
# when every step passed.
#
# Environment: NEOJSON_DIR (neo), SJ_CORPUS (the jsonexamples directory of
# simdjson-data, for the corpus and jsonchecker tests of SimdJSON-NeoTests),
# SJ_SLOW, SJ_NUM_TOKENS (100000 random number tokens by default),
# SJ_WORK_DIR, SJ_TIMEOUT (seconds a step may take, 3600 by default).

if test $# -ne 3; then
    echo "usage: $0 <vm-dir> <image.zip | X.image> <standalone | neo>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
MODE=$3
ST=$(cd "$(dirname "$0")/../st" && pwd)

case $MODE in
standalone) FILES="SimdJSON.st SimdJSON-Tests.st" ;;
neo)        FILES="load-neojson.st SimdJSON.st SimdJSON-Tests.st SimdJSON-NeoTests.st" ;;
*)          echo "the mode is standalone or neo, not '$MODE'" >&2; exit 2 ;;
esac
if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if test "$MODE" = neo && ! test -d "$NEOJSON_DIR/repository"; then
    echo "FAIL: NEOJSON_DIR must name a clone of NeoJSON (with its repository directory)" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# lint
nonascii=$(printf '[\200-\377]')
for file in $FILES; do
    if LC_ALL=C grep -n "$nonascii" "$ST/$file" >/dev/null; then
	fail "lint: $file has characters that are not ASCII:"
	LC_ALL=C grep -n "$nonascii" "$ST/$file" | head -5
    fi
    # the first chunk of a section must end the section ('! !'): a chunk that
    # ends with a single '!' is a second method, which Pharo 15 loses
    # (kept in a variable: the script writes nothing in the current directory)
    lint=$(awk -v file="$file" '
	/^!.*methodsFor:.*!$/ { open = NR; sections++; next }
	open && /!$/ {
	    match($0, /!+$/)
	    if (RLENGTH % 2 == 0) next          # !! is a ! in a string
	    if ($0 ~ /! !$/) methods++
	    else print file ": the section of line " open " has a second method at line " NR
	    open = 0
	}
	END {
	    if (sections != methods) print file ": " sections " methodsFor: sections, " methods " methods"
	}' "$ST/$file")
    if test -n "$lint"; then
	fail "lint:"
	printf '%s\n' "$lint"
    fi
done
test $failed = 0 || exit 1
echo "lint: $FILES"

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

# load: one file a run, saved
for file in $FILES; do
    pharo st --save --quit "$ST/$file" >"$WORK/$file.out" 2>&1
    status=$?
    sed 's/^/    /' "$WORK/$file.out"
    if test $status -ne 0 || test -f "$WORK/PharoDebug.log"; then
	fail "load: $file (exit status $status, see $WORK)"
	exit 1
    fi
    echo "load: $file"
done
if ! grep "SimdJSON: $MODE mode" "$WORK/SimdJSON.st.out" >/dev/null; then
    fail "load: SimdJSON.st did not file in in $MODE mode"
    exit 1
fi

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
if test "$MODE" = neo; then
    run_package SimdJSON-NeoTests
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
    echo "PASS $MODE: $VM_DIR/pharo ($WORK)"
fi
exit $failed
