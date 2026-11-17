#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the TimSortPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 12 to 15 image, zipped or not.
# The script works on a copy of the image in a directory of its own
# (TS_WORK_DIR, or a new one in TMPDIR), and:
#
#   load     load-timsort.st loads the Smalltalk code with Metacello from the
#            submodule plugins/TimSortPlugin/timsort.st (git submodule update
#            --init): the groups of TS_GROUPS of BaselineOfTimSort, with its
#            checks (protocol 1, no undeclared variable, no method on a Zn*
#            class, timSort prefixes); the banner 'TimSort: loaded, protocol
#            1, backend #<b>, blockSortBackend #<b>, resumableBackend
#            #<b> or nil, ...' must be printed;
#   library  with TS_FFI_LIBRARY, TimSortLibrary libraryPath: names that
#            libTimSortFFI, and the image is saved again;
#   backend  the backend (the natural order, buffers, keys), the default
#            backend of the sorts by a sortBlock and the first backend with a
#            resumable C sort (TimSort sort:with:backend:) must be those that
#            TS_EXPECT_BACKEND, TS_EXPECT_BLOCK_BACKEND and
#            TS_EXPECT_RESUMABLE name, as the image chose them after the
#            load, or after the library step when there is one (by default:
#            plugin when <vm-dir> holds the plugin, libTimSortPlugin.so,
#            .dylib or TimSortPlugin.dll, else ffi when the library is
#            expected, else any, for the first and the third, none for the
#            third when the first is smalltalk; smalltalk for the second, on
#            every VM: the port is faster than the resumable C sort, which
#            the tests run by naming it); and, when TimSort-FFI is
#            loaded, the library must be available when TS_EXPECT_LIBRARY is
#            yes (by default when <vm-dir> holds libTimSortFFI.so, .dylib,
#            TimSortFFI.dll or libTimSortFFI.dll, or TS_FFI_LIBRARY is set):
#            a plugin or a library that does not load would else only make
#            their tests skip themselves, which the runner counts as passes;
#   test     the packages TimSort-Tests (every suite runs on the plugin, when
#            the VM has it, and on the port TimSortSmalltalk),
#            TimSort-Smalltalk-Tests (the port itself) and TimSort-FFI-Tests
#            (the suites again on the library libTimSortFFI through the FFI,
#            with the plugin hidden, and the library's own tests), each when
#            its group is loaded.
#
# A JUnit file of each package is left in the directory.  The status is 0
# when every step passed.
#
# The plugin is never hidden by deleting it (a VM of a build tree finds it
# through its absolute RUNPATH): the tests hide it with test hooks.
# Metacello makes UUIDs: <vm-dir> needs the UUIDPlugin.
#
# Environment: TS_GROUPS (the groups to load, 'Core Tests FFI FFITests
# SmalltalkTests' by default), TS_EXPECT_BACKEND and TS_EXPECT_BLOCK_BACKEND
# (plugin, ffi, smalltalk or any; the second is smalltalk by default),
# TS_EXPECT_RESUMABLE (plugin, ffi, none or any), TS_EXPECT_LIBRARY (yes, no or any), TS_WORK_DIR, TS_TIMEOUT (seconds a step
# may take, 3600 by default), TS_FFI_LIBRARY (the libTimSortFFI to use).
#
# The three configurations of docs/TimSort.md (Tests):
#   plugin           sh run-st-tests.sh build/build/vm <image>
#   library only     TS_FFI_LIBRARY=<build>/build/vm/libTimSortFFI.so \
#                    sh run-st-tests.sh <vm-dir without the plugin> <image>
#   Smalltalk only   TS_GROUPS='Core Tests SmalltalkTests' TS_EXPECT_BACKEND=smalltalk \
#                    sh run-st-tests.sh <vm-dir without the plugin> <image>
# A <vm-dir without the plugin> is that of a build with
# FEATURE_PLUGIN_TIMSORT=OFF, or of a stock VM.

if test $# -ne 2; then
    echo "usage: $0 <vm-dir> <image.zip | X.image>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
PLUGIN=$(cd "$(dirname "$0")/.." && pwd)
load_groups=${TS_GROUPS:-'Core Tests FFI FFITests SmalltalkTests'}
has_plugin=no
for f in libTimSortPlugin.so libTimSortPlugin.dylib TimSortPlugin.dll; do
    if test -f "$VM_DIR/$f"; then has_plugin=yes; fi
done
library=${TS_EXPECT_LIBRARY:-}
if test -z "$library"; then
    library=any
    if test -n "$TS_FFI_LIBRARY"; then library=yes; fi
    for f in libTimSortFFI.so libTimSortFFI.dylib TimSortFFI.dll libTimSortFFI.dll; do
	if test -f "$VM_DIR/$f"; then library=yes; fi
    done
fi
expect=${TS_EXPECT_BACKEND:-}
if test -z "$expect"; then
    if test $has_plugin = yes; then expect=plugin
    elif test $library = yes; then expect=ffi
    else expect=any
    fi
fi
expect_block=${TS_EXPECT_BLOCK_BACKEND:-smalltalk}
for value in "$expect" "$expect_block"; do
    case $value in
    plugin|ffi|smalltalk|any) ;;
    *) echo "usage: TS_EXPECT_BACKEND and TS_EXPECT_BLOCK_BACKEND are plugin, ffi, smalltalk or any, not '$value'" >&2; exit 2 ;;
    esac
done
expect_resumable=${TS_EXPECT_RESUMABLE:-}
if test -z "$expect_resumable"; then
    case $expect in
    smalltalk) expect_resumable=none ;;
    *) expect_resumable=$expect ;;
    esac
fi
case $expect_resumable in
plugin|ffi|none|any) ;;
*) echo "usage: TS_EXPECT_RESUMABLE is plugin, ffi, none or any, not '$expect_resumable'" >&2; exit 2 ;;
esac
case $library in
yes|no|any) ;;
*) echo "usage: TS_EXPECT_LIBRARY is yes, no or any, not '$library'" >&2; exit 2 ;;
esac

if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if ! test -d "$PLUGIN/timsort.st/src/BaselineOfTimSort"; then
    echo "FAIL: no $PLUGIN/timsort.st/src: git submodule update --init plugins/TimSortPlugin/timsort.st" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${TS_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/ts-st-tests.XXXXXX")} || exit 1
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
cp "$image" "$WORK/TimSort.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/TimSort.changes"; else : >"$WORK/TimSort.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip" "$WORK/PharoDebug.log"
echo "image: $image, copied to $WORK/TimSort.image"
echo "vm: $VM_DIR/pharo"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${TS_TIMEOUT:-3600}"; fi
# LD_LIBRARY_PATH: the libraries next to the VM load the libraries they need
# from there (libgit2 needs libssh2 and libssl, which Iceberg calls at startup)
pharo() {
    (cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$VM_DIR/pharo" --headless TimSort.image --no-default-preferences "$@")
}

# load: the groups, saved
(TS_PLUGIN_DIR=$PLUGIN TS_GROUPS=$load_groups; export TS_PLUGIN_DIR TS_GROUPS
 pharo st --save --quit "$PLUGIN/tests/load-timsort.st") >"$WORK/load.out" 2>&1
status=$?
tr '\r' '\n' <"$WORK/load.out" | grep -v '^MetacelloNotification' | grep -v '^ *$' | sed 's/^/    /'
if test $status -ne 0; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
banner=$(tr '\r' '\n' <"$WORK/load.out" | grep '^TimSort: loaded, protocol 1, backend #')
if test -z "$banner"; then
    fail "load: no banner 'TimSort: loaded, protocol 1, backend #...'"
    exit 1
fi
echo "load: $load_groups ($(git -C "$PLUGIN/timsort.st" log -1 --format=%h 2>/dev/null || echo 'timsort.st'))"

# the library of TS_FFI_LIBRARY, saved in the image for the test runs; the
# backends are chosen again
if test -n "$TS_FFI_LIBRARY"; then
    printf '%s\n' "| library facade |" \
	"library := Smalltalk globals at: #TimSortLibrary." \
	"facade := Smalltalk globals at: #TimSort." \
	"library libraryPath: '$TS_FFI_LIBRARY'." \
	"Stdio stdout nextPutAll: 'TimSort: library ', library libraryPath printString, ', available ', library isAvailable printString;" \
	"	nextPutAll: ', backend ', facade backend printString, ', blockSortBackend ', facade blockSortBackend printString;" \
	"	nextPutAll: ', resumableBackend ', facade resumableBackend printString; lf; flush." \
	'!' >"$WORK/library.st"
    pharo st --save --quit "$WORK/library.st" >"$WORK/library.out" 2>&1 || fail "library: $TS_FFI_LIBRARY (see $WORK/library.out)"
    banner=$(tr '\r' '\n' <"$WORK/library.out" | grep '^TimSort: library ')
    echo "$banner" | sed 's/^/    /'
fi

# (the library line of load-timsort.st, or of the library step)
available=$(tr '\r' '\n' <"$WORK/load.out" | sed -n 's/^TimSort: library \(true\|false\) .*/\1/p')
if test -n "$TS_FFI_LIBRARY"; then
    available=$(echo "$banner" | sed -n 's/.*, available \(true\|false\),.*/\1/p')
fi
if test -n "$available"; then
    if test $library = yes && test "$available" != true; then
	fail "library: libTimSortFFI is not available (TS_EXPECT_LIBRARY=yes): its suites would skip themselves"
	exit 1
    fi
    if test $library = no && test "$available" != false; then
	fail "library: libTimSortFFI is available (TS_EXPECT_LIBRARY=no)"
	exit 1
    fi
fi
# backend: the choice of the image, after the load or the library step
if test "$expect" != any && ! echo "$banner" | grep ", backend #$expect," >/dev/null; then
    fail "backend: not #$expect (TS_EXPECT_BACKEND=$expect$(test -n "$TS_EXPECT_BACKEND" || echo ", the default for $VM_DIR"))"
    exit 1
fi
if test "$expect_block" != any && ! echo "$banner" | grep ", blockSortBackend #$expect_block\([,]\|$\)" >/dev/null; then
    fail "backend: the sorts by a sortBlock are not on #$expect_block by default (TS_EXPECT_BLOCK_BACKEND=$expect_block)"
    exit 1
fi
resumable="#$expect_resumable"
if test "$expect_resumable" = none; then resumable=nil; fi
if test "$expect_resumable" != any && ! echo "$banner" | grep ", resumableBackend $resumable\([,]\|$\)" >/dev/null; then
    fail "backend: the first resumable C sort is not $resumable (TS_EXPECT_RESUMABLE=$expect_resumable)"
    exit 1
fi
echo "backend: $(echo "$banner" | sed -n 's/.*\(backend #[a-z]*\), \(blockSortBackend #[a-z]*\), \(resumableBackend [#a-z]*\).*/\1, \2, \3/p')${available:+, library available $available}"

# test: one package a run, then the summary of its JUnit file
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
*' Tests '*|*' FFITests '*|*' default '*) run_package TimSort-Tests ;;
esac
case " $load_groups " in
*' SmalltalkTests '*|*' default '*) run_package TimSort-Smalltalk-Tests ;;
esac
case " $load_groups " in
*' FFITests '*|*' default '*) run_package TimSort-FFI-Tests ;;
esac

if test $failed = 0; then
    echo "PASS: $VM_DIR/pharo ($WORK)"
fi
exit $failed
