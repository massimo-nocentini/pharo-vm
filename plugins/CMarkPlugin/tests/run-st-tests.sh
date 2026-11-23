#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the CMarkPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 12 to 15 image, zipped or not.
# The script works on a copy of the image in a directory of its own
# (CMARK_WORK_DIR, or a new one in TMPDIR), and:
#
#   load     load-cmark.st loads the Smalltalk code with Metacello from the
#            submodule plugins/CMarkPlugin/cmark-gfm.st (git submodule
#            update --init): the groups of CMARK_GROUPS of BaselineOfCMark,
#            with its checks (CMarkPrimitives protocol 1, no undeclared
#            variable, cmark prefixes on the extension methods); the image is
#            saved, and the banner 'CMark: loaded, protocol 1, plugin
#            CMarkPlugin 1.0 cmark-gfm 0.29.0.gfm.14 <platform>' or 'CMark:
#            loaded, no plugin' must be printed;
#   plugin   the image must find the plugin when CMARK_EXPECT_PLUGIN is yes
#            (by default when <vm-dir> holds libCMarkPlugin.so, .dylib,
#            CMarkPlugin.dll or libCMarkPlugin.dll), and must not when it is
#            no: the tests that need the plugin would else skip themselves,
#            which the runner counts as passes, and a run where everything
#            skipped would pass as green;
#   test     every test package that the groups loaded (CMark-Tests,
#            CMark-Roassal-Tests, CMark-Calypso-Tests), as load-cmark.st
#            lists them.
#
# A JUnit file of each package is left in the directory.  The status is 0
# when every step passed.  The plugin is never hidden by deleting it (a VM
# of a build tree finds it through its absolute RUNPATH): the tests hide it
# with CMarkPrimitives testHook: #pluginMissing value: true, and a run with
# no plugin takes a VM without it (CMARK_EXPECT_PLUGIN=no).  Metacello makes
# UUIDs: <vm-dir> needs the UUIDPlugin.
#
# Environment: CMARK_GROUPS (the groups to load, 'Core Tests' by default;
# 'Core Tests Roassal RoassalTests' for the renderer too), CMARK_EXPECT_PLUGIN
# (yes, no or any), CMARK_PLUGIN_DIR (the directory of the plugin, by
# default the one above this script), CMARK_WORK_DIR, CMARK_TIMEOUT (seconds
# a step may take, 3600 by default).
#
# docs/CMark.md (Tests):
#   plugin      sh run-st-tests.sh build/build/vm <image>
#   no plugin   CMARK_EXPECT_PLUGIN=no sh run-st-tests.sh <vm-dir without the plugin> <image>
#   Roassal     CMARK_GROUPS='Core Tests Roassal RoassalTests' sh run-st-tests.sh build/build/vm <image>

if test $# -ne 2; then
    echo "usage: $0 <vm-dir> <image.zip | X.image>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
if test -n "$CMARK_PLUGIN_DIR"; then
    PLUGIN=$(cd "$CMARK_PLUGIN_DIR" && pwd) || exit 2
else
    PLUGIN=$(cd "$(dirname "$0")/.." && pwd)
fi
load_groups=${CMARK_GROUPS:-'Core Tests'}
expect=${CMARK_EXPECT_PLUGIN:-}
if test -z "$expect"; then
    expect=any
    for f in libCMarkPlugin.so libCMarkPlugin.dylib CMarkPlugin.dll libCMarkPlugin.dll; do
	if test -f "$VM_DIR/$f"; then expect=yes; fi
    done
fi
case $expect in
yes|no|any) ;;
*) echo "usage: CMARK_EXPECT_PLUGIN is yes, no or any, not '$expect'" >&2; exit 2 ;;
esac

if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if ! test -d "$PLUGIN/cmark-gfm.st/src/BaselineOfCMark"; then
    echo "FAIL: no $PLUGIN/cmark-gfm.st/src/BaselineOfCMark: git submodule update --init plugins/CMarkPlugin/cmark-gfm.st" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${CMARK_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cmark-st-tests.XXXXXX")} || exit 1
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
cp "$image" "$WORK/CMark.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/CMark.changes"; else : >"$WORK/CMark.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip" "$WORK/PharoDebug.log"
echo "image: $image, copied to $WORK/CMark.image"
echo "vm: $VM_DIR/pharo"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${CMARK_TIMEOUT:-3600}"; fi
# LD_LIBRARY_PATH: the libraries next to the VM load the libraries they need
# from there (libgit2 needs libssh2 and libssl, which Iceberg calls at startup)
pharo() {
    (cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$VM_DIR/pharo" --headless CMark.image --no-default-preferences "$@")
}

# load: the groups, saved
(CMARK_PLUGIN_DIR=$PLUGIN CMARK_GROUPS=$load_groups CMARK_EXPECT_PLUGIN=$expect
 export CMARK_PLUGIN_DIR CMARK_GROUPS CMARK_EXPECT_PLUGIN
 pharo st --save --quit "$PLUGIN/tests/load-cmark.st") >"$WORK/load.out" 2>&1
status=$?
tr '\r' '\n' <"$WORK/load.out" | grep -v '^MetacelloNotification' | grep -v '^ *$' | sed 's/^/    /'
if test $status -ne 0; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
banner=$(tr '\r' '\n' <"$WORK/load.out" | grep '^CMark: loaded, ')
if test -z "$banner"; then
    fail "load: no banner 'CMark: loaded, ...'"
    exit 1
fi
echo "load: $load_groups ($(git -C "$PLUGIN/cmark-gfm.st" log -1 --format=%h 2>/dev/null || echo 'cmark-gfm.st'))"

# plugin: the image's view, after the load
case $expect in
yes)
    if ! echo "$banner" | grep '^CMark: loaded, protocol 1, plugin CMarkPlugin 1\.0 cmark-gfm 0\.29\.0\.gfm\.14 ' >/dev/null; then
	fail "plugin: the image finds no CMarkPlugin (CMARK_EXPECT_PLUGIN=yes$(test -n "$CMARK_EXPECT_PLUGIN" || echo ", the default for $VM_DIR")): $banner"
	exit 1
    fi ;;
no)
    if ! echo "$banner" | grep '^CMark: loaded, no plugin$' >/dev/null; then
	fail "plugin: the image finds a CMarkPlugin (CMARK_EXPECT_PLUGIN=no): $banner"
	exit 1
    fi ;;
esac
echo "plugin: $(echo "$banner" | sed 's/^CMark: loaded, //')"

# test: each test package, then the summary of its JUnit file
run_package() {
    package=$1
    pharo test --junit-xml-output --fail-on-failure --fail-on-error "$package" >"$WORK/$package.out" 2>&1
    status=$?
    tr '\r' '\n' <"$WORK/$package.out" | sed 's/^/    /' | grep -v '^    \[' | grep -v '^ *$' | tail -40
    xml=$WORK/$package-Test.xml
    if ! test -f "$xml"; then
	fail "test: $package (exit status $status, no $xml)"
	return
    fi
    # The runner writes the counts into <testsuite> when the run ends: a run
    # that died on the way (an error the runner did not catch) leaves them
    # out, and no </testsuite>; then the elements are counted.  (A skipped
    # test is a pass in the file: hence the banner check above)
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

packages=$(tr '\r' '\n' <"$WORK/load.out" | sed -n 's/^CMark: test packages//p' | head -1)
if test -z "$packages"; then
    echo "test: the groups $load_groups load no test package"
fi
for package in $packages; do
    run_package "$package"
done

if test $failed = 0; then
    echo "PASS: $VM_DIR/pharo ($WORK)"
fi
exit $failed
