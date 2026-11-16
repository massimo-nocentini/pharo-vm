#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the SimdUTFPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 12 to 15 image, zipped or not.
# The script works on a copy of the image in a directory of its own
# (SU_WORK_DIR, or a new one in TMPDIR), and:
#
#   load   load-simdutf.st loads the Smalltalk code with Metacello from the
#          submodule plugins/SimdUTFPlugin/simdutf.st (git submodule update
#          --init): the groups Core, Tests, FFI and FFITests of
#          BaselineOfSimdUTF, with its checks (protocol 1, no undeclared
#          variable, no method on a Zn* class, simd prefixes); the banner
#          'SimdUTF: loaded, protocol 1, backend #<b>, ...' must be printed,
#          with the backend that SU_EXPECT_BACKEND names (chosen at the
#          load, before SU_FFI_LIBRARY is given; by default
#          plugin when <vm-dir> holds the plugin, libSimdUTFPlugin.so,
#          .dylib or SimdUTFPlugin.dll, else any): a plugin that does not
#          load would else only make its tests skip themselves, which the
#          runner counts as passes;
#   test   the packages SimdUTF-Tests (every suite runs on the plugin, when
#          the VM has it, and in pure Smalltalk; SimdUTFSlowTests are skipped
#          unless SU_SLOW is set) and SimdUTF-FFI-Tests (the library
#          libSimdUTFFFI through the FFI, skipped when it is not found).
#
# A JUnit file of each package is left in the directory.  The status is 0
# when every step passed.
#
# The plugin is never hidden by deleting it (a VM of a build tree finds it
# through its absolute RUNPATH): the tests hide it with test hooks.
# Metacello makes UUIDs: <vm-dir> needs the UUIDPlugin.
#
# Environment: SU_GROUPS (the groups to load, 'Core Tests FFI FFITests' by
# default; SimdUTF-FFI-Tests runs when FFITests is one of them),
# SU_EXPECT_BACKEND (plugin, ffi, smalltalk or any; see load), SU_SLOW,
# SU_WORK_DIR, SU_TIMEOUT (seconds a step may take, 3600 by default),
# SU_FFI_LIBRARY (the libSimdUTFFFI to use, given to SimdUTFLibrary
# libraryPath: before the tests).
#
# On a <vm-dir> without the plugin (a build with FEATURE_PLUGIN_SIMDUTF=OFF)
# and SU_FFI_LIBRARY naming the libSimdUTFFFI of another build, the suites
# run on the library and SimdUTFLibraryTests>>testNotFoundTakesTheLibrary
# runs too: it is skipped wherever the plugin is.

if test $# -ne 2; then
    echo "usage: $0 <vm-dir> <image.zip | X.image>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
PLUGIN=$(cd "$(dirname "$0")/.." && pwd)
load_groups=${SU_GROUPS:-'Core Tests FFI FFITests'}
expect=${SU_EXPECT_BACKEND:-}
if test -z "$expect"; then
    expect=any
    for f in libSimdUTFPlugin.so libSimdUTFPlugin.dylib SimdUTFPlugin.dll; do
	if test -f "$VM_DIR/$f"; then expect=plugin; fi
    done
fi
case $expect in
plugin|ffi|smalltalk|any) ;;
*) echo "usage: SU_EXPECT_BACKEND is plugin, ffi, smalltalk or any, not '$expect'" >&2; exit 2 ;;
esac

if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if ! test -d "$PLUGIN/simdutf.st/src/BaselineOfSimdUTF"; then
    echo "FAIL: no $PLUGIN/simdutf.st/src: git submodule update --init plugins/SimdUTFPlugin/simdutf.st" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${SU_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/su-st-tests.XXXXXX")} || exit 1
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
cp "$image" "$WORK/SimdUTF.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/SimdUTF.changes"; else : >"$WORK/SimdUTF.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip" "$WORK/PharoDebug.log"
echo "image: $image, copied to $WORK/SimdUTF.image"
echo "vm: $VM_DIR/pharo"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${SU_TIMEOUT:-3600}"; fi
# LD_LIBRARY_PATH: the libraries next to the VM load the libraries they need
# from there (libgit2 needs libssh2 and libssl, which Iceberg calls at startup)
pharo() {
    (cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$VM_DIR/pharo" --headless SimdUTF.image --no-default-preferences "$@")
}

# load: the groups, saved
(SU_PLUGIN_DIR=$PLUGIN SU_GROUPS=$load_groups; export SU_PLUGIN_DIR SU_GROUPS
 pharo st --save --quit "$PLUGIN/tests/load-simdutf.st") >"$WORK/load.out" 2>&1
status=$?
grep -v '^MetacelloNotification' "$WORK/load.out" | sed 's/^/    /'
if test $status -ne 0; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
if ! grep '^SimdUTF: loaded, protocol 1, backend #' "$WORK/load.out" >/dev/null; then
    fail "load: no banner 'SimdUTF: loaded, protocol 1, backend #...'"
    exit 1
fi
if test "$expect" != any && ! grep "^SimdUTF: loaded, protocol 1, backend #$expect[,]" "$WORK/load.out" >/dev/null; then
    fail "load: the backend is not #$expect (SU_EXPECT_BACKEND=$expect$(test -n "$SU_EXPECT_BACKEND" || echo ", since $VM_DIR has the plugin"))"
    exit 1
fi
echo "load: $load_groups ($(git -C "$PLUGIN/simdutf.st" log -1 --format=%h 2>/dev/null || echo 'simdutf.st'))"

# the library of SU_FFI_LIBRARY, saved in the image for the test runs
if test -n "$SU_FFI_LIBRARY"; then
    printf '%s\n' "(Smalltalk globals at: #SimdUTFLibrary) libraryPath: '$SU_FFI_LIBRARY'." \
	"Stdio stdout nextPutAll: 'SimdUTF: library ', (Smalltalk globals at: #SimdUTFLibrary) libraryPath printString; lf; flush." \
	'!' >"$WORK/library.st"
    pharo st --save --quit "$WORK/library.st" >"$WORK/library.out" 2>&1 || fail "library: $SU_FFI_LIBRARY (see $WORK/library.out)"
    grep '^SimdUTF: library' "$WORK/library.out" | sed 's/^/    /'
fi

# test: one package a run, then the summary of its JUnit file
run_package() {
    package=$1
    pharo test --junit-xml-output --fail-on-failure --fail-on-error "$package" >"$WORK/$package.out" 2>&1
    status=$?
    sed 's/^/    /' "$WORK/$package.out" | grep -v '^    \[' | tail -40
    xml=$WORK/$package-Test.xml
    if ! test -f "$xml"; then
	fail "test: $package (exit status $status, no $xml)"
	return
    fi
    # The runner writes the counts into <testsuite> when the run ends: a run
    # that died on the way (an error the runner did not catch) leaves them
    # out, and no </testsuite>; then the elements are counted.
    awk '
	/<testsuite / {
	    if (match($0, /failures="[0-9]*"/)) {
		failures = substr($0, RSTART + 10, RLENGTH - 11)
		match($0, /errors="[0-9]*"/); errors = substr($0, RSTART + 8, RLENGTH - 9)
		match($0, /tests="[0-9]*"/); tests = substr($0, RSTART + 7, RLENGTH - 8)
	    }
	}
	/<testcase / {
	    cases++
	    match($0, /classname="[^"]*"/); class = substr($0, RSTART + 11, RLENGTH - 12)
	    sub(/.*\./, "", class)
	    match($0, / name="[^"]*"/); name = substr($0, RSTART + 7, RLENGTH - 8)
	}
	/<(failure|error) / {
	    if (/<failure/) failed++; else erred++
	    match($0, /message="[^"]*"/)
	    print "    " (/<failure/ ? "failure" : "error") ": " class ">>" name " " substr($0, RSTART + 9, RLENGTH - 10)
	}
	/<\/testsuite>/ { complete = 1 }
	END {
	    if (failures == "") { tests = cases + 0; failures = failed + 0; errors = erred + 0 }
	    print tests " " failures " " errors " " (complete ? "complete" : "incomplete") > "/dev/stderr"
	}' "$xml" 2>"$WORK/$package.counts"
    set -- $(cat "$WORK/$package.counts")
    if test "$4" = complete; then
	echo "test: $package: $1 run, $2 failures, $3 errors"
    else
	echo "test: $package: $1 run, $2 failures, $3 errors before the run died (no </testsuite> in $xml)"
    fi
    if test $status -ne 0 || test "$4" != complete || test "${1:-0}" -eq 0 || test "$2" -ne 0 || test "$3" -ne 0; then
	fail "test: $package (exit status $status)"
    fi
}

run_package SimdUTF-Tests
case " $load_groups " in
*' FFITests '*|*' default '*) run_package SimdUTF-FFI-Tests ;;
esac

if test $failed = 0; then
    echo "PASS: $VM_DIR/pharo ($WORK)"
fi
exit $failed
