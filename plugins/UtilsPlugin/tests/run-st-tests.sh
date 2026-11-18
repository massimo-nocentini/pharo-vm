#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the UtilsPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 12 to 15 image, zipped or not.
# The script works on a copy of the image in a directory of its own
# (UP_WORK_DIR, or a new one in TMPDIR), and:
#
#   load     load-utils.st loads the Smalltalk code with Metacello from the
#            submodule plugins/UtilsPlugin/vm-plugins-primitives.st (git
#            submodule update --init): the groups of UP_GROUPS of
#            BaselineOfVMPluginsPrimitives, with its checks; the banner
#            'VMPluginsPrimitives: loaded, plugin <info>' must be printed;
#   plugin   the image must find the plugin when UP_EXPECT_PLUGIN is yes (by
#            default when <vm-dir> holds libUtilsPlugin.so, .dylib or
#            UtilsPlugin.dll), and must not when it is no: the tests of the
#            #plugin backend would else skip themselves, which the runner
#            counts as passes;
#   test     the package VM-Plugins-Primitives-Tests: every test of the
#            features on the plugin (when the VM has it) and on the Smalltalk
#            twins, the differential tests and those of the primitives'
#            contract (with the plugin).
#
# A JUnit file is left in the directory.  The status is 0 when every step
# passed.  The plugin is never hidden by deleting it (a VM of a build tree
# finds it through its absolute RUNPATH): the tests hide it with
# UtilsPlugin pluginHidden:.  Metacello makes UUIDs: <vm-dir> needs the
# UUIDPlugin.
#
# Environment: UP_GROUPS (the groups to load, 'Core Tests' by default),
# UP_EXPECT_PLUGIN (yes, no or any), UP_WORK_DIR, UP_TIMEOUT (seconds a step
# may take, 3600 by default).

if test $# -ne 2; then
    echo "usage: $0 <vm-dir> <image.zip | X.image>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
PLUGIN=$(cd "$(dirname "$0")/.." && pwd)
load_groups=${UP_GROUPS:-'Core Tests'}
expect=${UP_EXPECT_PLUGIN:-}
if test -z "$expect"; then
    expect=any
    for f in libUtilsPlugin.so libUtilsPlugin.dylib UtilsPlugin.dll; do
	if test -f "$VM_DIR/$f"; then expect=yes; fi
    done
fi
case $expect in
yes|no|any) ;;
*) echo "usage: UP_EXPECT_PLUGIN is yes, no or any, not '$expect'" >&2; exit 2 ;;
esac

if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if ! test -d "$PLUGIN/vm-plugins-primitives.st/src/BaselineOfVMPluginsPrimitives"; then
    echo "FAIL: no $PLUGIN/vm-plugins-primitives.st/src: git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${UP_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/up-st-tests.XXXXXX")} || exit 1
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
cp "$image" "$WORK/Utils.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/Utils.changes"; else : >"$WORK/Utils.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip" "$WORK/PharoDebug.log"
echo "image: $image, copied to $WORK/Utils.image"
echo "vm: $VM_DIR/pharo"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${UP_TIMEOUT:-3600}"; fi
# LD_LIBRARY_PATH: the libraries next to the VM load the libraries they need
# from there (libgit2 needs libssh2 and libssl, which Iceberg calls at startup)
pharo() {
    (cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$VM_DIR/pharo" --headless Utils.image --no-default-preferences "$@")
}

# load: the groups, saved
(UP_PLUGIN_DIR=$PLUGIN UP_GROUPS=$load_groups; export UP_PLUGIN_DIR UP_GROUPS
 pharo st --save --quit "$PLUGIN/tests/load-utils.st") >"$WORK/load.out" 2>&1
status=$?
tr '\r' '\n' <"$WORK/load.out" | grep -v '^MetacelloNotification' | grep -v '^ *$' | sed 's/^/    /'
if test $status -ne 0; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
banner=$(tr '\r' '\n' <"$WORK/load.out" | grep '^VMPluginsPrimitives: loaded, plugin ')
if test -z "$banner"; then
    fail "load: no banner 'VMPluginsPrimitives: loaded, plugin ...'"
    exit 1
fi
echo "load: $load_groups ($(git -C "$PLUGIN/vm-plugins-primitives.st" log -1 --format=%h 2>/dev/null || echo 'vm-plugins-primitives.st'))"

# plugin: the image's view, after the load
case $expect in
yes)
    if echo "$banner" | grep 'plugin nil$' >/dev/null; then
	fail "plugin: the image finds no UtilsPlugin (UP_EXPECT_PLUGIN=yes$(test -n "$UP_EXPECT_PLUGIN" || echo ", the default for $VM_DIR"))"
	exit 1
    fi ;;
no)
    if ! echo "$banner" | grep 'plugin nil$' >/dev/null; then
	fail "plugin: the image finds a UtilsPlugin (UP_EXPECT_PLUGIN=no)"
	exit 1
    fi ;;
esac
echo "plugin: $(echo "$banner" | sed 's/^VMPluginsPrimitives: loaded, plugin //')"

# test: the package, then the summary of its JUnit file
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

case " $load_groups " in
*' Tests '*|*' default '*) run_package VM-Plugins-Primitives-Tests ;;
esac

if test $failed = 0; then
    echo "PASS: $VM_DIR/pharo ($WORK)"
fi
exit $failed
