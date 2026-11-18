#!/bin/sh
# run-st-tests.sh - the Smalltalk tests of the CairoPlugin on a VM
#
#   sh run-st-tests.sh <vm-dir> <image.zip | X.image>
#
# <vm-dir> holds the VM, pharo, and its plugins (build/vm of a native build,
# or node/ of make wasm); <image> is a Pharo 12 to 15 image, zipped or not.
# The script works on a copy of the image in a directory of its own
# (CP_WORK_DIR, or a new one in TMPDIR), and:
#
#   surface  on a WebAssembly VM (<vm-dir> holds pharo.wasm), web-surface.st
#            gives a stock image the AthensCairoSurface methods of
#            OSWindow-Web (no SurfacePlugin there);
#   load     load-cairo.st loads the groups of CP_GROUPS ('Cairo CairoTests')
#            of BaselineOfVMPluginsPrimitives with Metacello, from the
#            submodule plugins/UtilsPlugin/vm-plugins-primitives.st (git
#            submodule update --init), or from CP_ST_REPO, with its checks;
#            the banner 'CairoPlugin: loaded, plugin <info>, installed
#            <groups>' must be printed;
#   plugin   the image must find the plugin, and loading must have installed
#            every group, when CP_EXPECT_PLUGIN is yes (by default when
#            <vm-dir> holds libCairoPlugin.so, .dylib or CairoPlugin.dll);
#            it must find none, and nothing may be installed, when it is no;
#   test     CairoPluginTestRunner runs the package
#            VM-Plugins-Primitives-Cairo-Tests (criteria C1 to C10 of
#            docs/CairoPlugin.md) and prints a line a test, with the reason
#            of each skip; no failure and no error may happen, and no test
#            may skip itself when the plugin is expected (a skipped test is
#            a pass in a JUnit file: the runner writes its own,
#            cairo-tests.xml, with the skips; the tests of a VM without the
#            plugin are 'not applicable' on a VM with it, not skips);
#   session  criterion C8 across image starts: with the plugin, the image is
#            saved with every group installed and started again on the same
#            VM (bound, installed, verify clean, the pixels of stock Athens);
#            without it, the groups are forced in, the image saved and
#            started again (uninstalled at the start, still wanted, one
#            libcairo loaded);
#   vm0      with CP_VM0, a VM without the plugin in another directory than
#            <vm-dir> (CP_EXPECT_PLUGIN yes): the saved image is started on
#            it (groups uninstalled and still wanted, exactly one libcairo
#            loaded, VM0's, and Athens draws), saved, and started again on
#            <vm-dir> (installed again, verify clean).
#
# The status is 0 when every step passed.  Metacello makes UUIDs: <vm-dir>
# needs the UUIDPlugin.  The tests draw their scenes at the scale
# CP_SCENE_SCALE (0.05 by default) on surfaces of CP_SCENE_SIZE pixels (1000).
#
# Environment: CP_GROUPS, CP_EXPECT_PLUGIN (yes, no or any), CP_ST_REPO,
# CP_WORK_DIR, CP_VM0, CP_TIMEOUT (seconds a step may take, 3600 by default),
# CP_SCENE_SCALE, CP_SCENE_SIZE.

if test $# -ne 2; then
    echo "usage: $0 <vm-dir> <image.zip | X.image>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
PLUGIN=$(cd "$(dirname "$0")/.." && pwd)
REPO=$(cd "$PLUGIN/../.." && pwd)
load_groups=${CP_GROUPS:-'Cairo CairoTests'}
expect=${CP_EXPECT_PLUGIN:-}
wasm=no
test -f "$VM_DIR/pharo.wasm" && wasm=yes
if test -z "$expect"; then
    expect=no
    for f in libCairoPlugin.so libCairoPlugin.dylib CairoPlugin.dll libCairoPlugin.dll; do
	if test -f "$VM_DIR/$f"; then expect=yes; fi
    done
    # a WebAssembly VM links its plugins in: whether it has the CairoPlugin
    # depends on its build (FEATURE_PLUGIN_CAIRO)
    test $wasm = yes && expect=any
fi
case $expect in
yes|no|any) ;;
*) echo "usage: CP_EXPECT_PLUGIN is yes, no or any, not '$expect'" >&2; exit 2 ;;
esac
VM0_DIR=
if test -n "$CP_VM0"; then
    VM0_DIR=$(cd "$CP_VM0" && pwd) || exit 2
    if test "$VM0_DIR" = "$VM_DIR"; then
	echo "usage: CP_VM0 must be a VM in another directory than $VM_DIR" >&2
	exit 2
    fi
fi

if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
if test -z "$CP_ST_REPO" && ! test -d "$REPO/plugins/UtilsPlugin/vm-plugins-primitives.st/src/BaselineOfVMPluginsPrimitives"; then
    echo "FAIL: no $REPO/plugins/UtilsPlugin/vm-plugins-primitives.st/src: git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st" >&2
    exit 1
fi

failed=0
fail() { echo "FAIL $*"; failed=1; }

# a copy of the image
WORK=${CP_WORK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/cp-st-tests.XXXXXX")} || exit 1
mkdir -p "$WORK" || exit 1
WORK=$(cd "$WORK" && pwd)
case $IMAGE in
*.zip)
    rm -rf "$WORK/zip"
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
rm -f "$WORK"/Cairo.image "$WORK"/Cairo.changes "$WORK"/PharoDebug.log "$WORK"/cairo-tests.xml
cp "$image" "$WORK/Cairo.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/Cairo.changes"; else : >"$WORK/Cairo.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip"
echo "image: $image, copied to $WORK/Cairo.image"
echo "vm: $VM_DIR/pharo (plugin expected: $expect)"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${CP_TIMEOUT:-3600}"; fi
# pharo_on <vm-dir> <args>: the image of $WORK on a VM; LD_LIBRARY_PATH: the
# libraries next to the VM load the libraries they need from there
pharo_on() {
    vm=$1
    shift
    (cd "$WORK" && LD_LIBRARY_PATH="$vm${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	$limit "$vm/pharo" --headless Cairo.image --no-default-preferences "$@")
}
# show <file>: the output of a step, CR as LF, indented
show() {
    tr '\r' '\n' <"$1" | grep -v '^Metacello' | grep -v '^ *$' | sed 's/^/    /'
}

# surface (WebAssembly only): a stock image has no OSWindow-Web, whose
# AthensCairoSurface methods need no SurfacePlugin: web-surface.st adds them
# (it changes nothing in an image that has them, image/web)
if test $wasm = yes; then
    pharo_on "$VM_DIR" st --save --quit "$PLUGIN/tests/web-surface.st" >"$WORK/surface.out" 2>&1
    status=$?
    show "$WORK/surface.out"
    if test $status -ne 0 || ! tr '\r' '\n' <"$WORK/surface.out" | grep '^web-surface: ' >/dev/null; then
	fail "surface: web-surface.st (exit status $status, see $WORK/surface.out)"
	exit 1
    fi
fi

# load: the groups, saved
(CP_PLUGIN_DIR=$REPO/plugins/UtilsPlugin CP_GROUPS=$load_groups; export CP_PLUGIN_DIR CP_GROUPS
 pharo_on "$VM_DIR" st --save --quit "$PLUGIN/tests/load-cairo.st") >"$WORK/load.out" 2>&1
status=$?
show "$WORK/load.out"
if test $status -ne 0; then
    fail "load: $load_groups (exit status $status, see $WORK)"
    exit 1
fi
banner=$(tr '\r' '\n' <"$WORK/load.out" | grep '^CairoPlugin: loaded, plugin ')
if test -z "$banner"; then
    fail "load: no banner 'CairoPlugin: loaded, plugin ...'"
    exit 1
fi
if test -n "$CP_ST_REPO"; then
    source=$CP_ST_REPO
else
    source=$(git -C "$REPO/plugins/UtilsPlugin/vm-plugins-primitives.st" log -1 --format=%h 2>/dev/null || echo 'vm-plugins-primitives.st')
fi
echo "load: $load_groups ($source)"

# plugin: the image's view, after the load
case $expect in
yes)
    if echo "$banner" | grep 'plugin nil,' >/dev/null; then
	fail "plugin: the image finds no CairoPlugin (CP_EXPECT_PLUGIN=yes$(test -n "$CP_EXPECT_PLUGIN" || echo ", the default for $VM_DIR"))"
	exit 1
    fi
    if ! echo "$banner" | grep 'installed #(#basic #shape #text)$' >/dev/null; then
	fail "plugin: loading did not install every group: $banner"
	exit 1
    fi ;;
no)
    if ! echo "$banner" | grep 'plugin nil, installed #()$' >/dev/null; then
	fail "plugin: the image finds a CairoPlugin, or installed groups without it (CP_EXPECT_PLUGIN=no): $banner"
	exit 1
    fi ;;
esac
echo "plugin: $(echo "$banner" | sed 's/^CairoPlugin: loaded, plugin //')"

case " $load_groups " in
*' CairoTests '*) ;;
*)
    echo "test: skipped (CP_GROUPS has no CairoTests)"
    test $failed = 0 && echo "PASS: $VM_DIR/pharo ($WORK)"
    exit $failed ;;
esac

# test: the package, through CairoPluginTestRunner
cat >"$WORK/run-tests.st" <<'EOF'
"run-tests.st (written by run-st-tests.sh): the tests of the CairoPlugin, not saved"
(Smalltalk globals at: #CairoPluginTestRunner) runOn: Stdio stdout junit: 'cairo-tests.xml'.
Smalltalk snapshot: false andQuit: true.
EOF
pharo_on "$VM_DIR" st "$WORK/run-tests.st" >"$WORK/test.out" 2>&1
status=$?
tr '\r' '\n' <"$WORK/test.out" | grep -E '^(SKIP|FAIL|ERROR|N/A)' | sed 's/^/    /'
summary=$(tr '\r' '\n' <"$WORK/test.out" | grep '^CairoPluginTests: ')
if test -z "$summary"; then
    tr '\r' '\n' <"$WORK/test.out" | tail -30 | sed 's/^/    /'
    fail "test: the run died (exit status $status, no summary, see $WORK/test.out)"
else
    echo "test: $(echo "$summary" | sed 's/^CairoPluginTests: //')"
    set -- $(echo "$summary" | sed 's/^CairoPluginTests: \([0-9]*\) run, \([0-9]*\) passed, \([0-9]*\) failures, \([0-9]*\) errors, \([0-9]*\) skipped, \([0-9]*\) not applicable.*/\1 \2 \3 \4 \5 \6/')
    if test "${1:-0}" -eq 0 || test "$3" -ne 0 || test "$4" -ne 0; then
	fail "test: $3 failures, $4 errors (see $WORK/test.out, $WORK/cairo-tests.xml)"
    fi
    if test "$expect" = yes && test "$5" -ne 0; then
	fail "test: $5 tests skipped themselves although the plugin is expected"
    fi
fi

# session: C8 across image starts
cat >"$WORK/session-save.st" <<'EOF'
"session-save.st (written by run-st-tests.sh): every group installed (forced without the plugin), drawn, saved"
| out surface |
out := Stdio stdout.
CairoPlugin isAvailable
	ifTrue: [ CairoPlugin install ]
	ifFalse: [ CairoPlugin forceInstall: CairoPlugin groups ].
surface := AthensCairoSurface extent: 20 @ 20.
surface drawDuring: [ :c |
	c setPaint: Color red; drawShape: (0 @ 0 corner: 5 @ 5).
	c setStrokePaint: Color blue.
	c drawShape: (c createPath: [ :b | b moveTo: 1 @ 1; lineTo: 8 @ 8 ]).
	c setFont: (LogicalFont familyName: 'Source Sans Pro' pointSize: 10).
	c setPaint: Color black.
	c drawString: 'abc' ].
out nextPutAll: 'session-save: bound '; nextPutAll: CairoPlugin isBound printString;
	nextPutAll: ' installed '; nextPutAll: CairoPlugin installedGroups printString;
	nextPutAll: ' wanted '; nextPutAll: CairoPlugin wantedGroups printString; lf; flush.
Smalltalk snapshot: true andQuit: true.
EOF
cat >"$WORK/session-check.st" <<'EOF'
"session-check.st (written by run-st-tests.sh): the state after a start, the libcairo loaded, the pixels of two scenes against stock Athens; saved when CP_SESSION_SAVE is set"
| out scenes installed parity cairos verify |
out := Stdio stdout.
installed := CairoPlugin installedGroups.
verify := CairoPlugin verify.
out nextPutAll: 'session: available '; nextPutAll: CairoPlugin isAvailable printString;
	nextPutAll: ' bound '; nextPutAll: CairoPlugin isBound printString;
	nextPutAll: ' installed '; nextPutAll: installed printString;
	nextPutAll: ' wanted '; nextPutAll: CairoPlugin wantedGroups printString;
	nextPutAll: ' verify '; nextPutAll: verify printString; lf.
scenes := CairoPluginScenes scale: 0.02.
parity := #( 'b-rects' 'e-text' 'e-text-wide' 'c-bezier' ) allSatisfy: [ :name |
	| here stock |
	here := CairoPluginTestSupport bytesOf: (scenes runScene: name).
	CairoPluginInstaller uninstall: CairoPlugin groups.
	stock := CairoPluginTestSupport bytesOf: (scenes runScene: name).
	installed isEmpty ifFalse: [ CairoPluginInstaller install: installed force: CairoPlugin isBound not ].
	here = stock ].
out nextPutAll: 'session: pixels of stock Athens '; nextPutAll: parity printString;
	nextPutAll: ' installed after '; nextPutAll: CairoPlugin installedGroups printString; lf.
"after drawing: the libcairo of the FFI and the plugin's are one"
cairos := Smalltalk vm listLoadedModules select: [ :m | m includesSubstring: 'libcairo' ].
out nextPutAll: 'session: libcairo loaded '; nextPutAll: cairos size printString; nextPutAll: ' '; nextPutAll: cairos asArray printString; lf.
out nextPutAll: 'session: libraryName '; nextPutAll: CairoLibrary uniqueInstance libraryName; lf; flush.
(OSEnvironment current at: 'CP_SESSION_SAVE' ifAbsent: [ nil ]) isNil
	ifTrue: [ Smalltalk snapshot: false andQuit: true ]
	ifFalse: [ Smalltalk snapshot: true andQuit: true ].
EOF
# session_check <label> <vm-dir> <save: yes/no> <expected line regexps...>
session_check() {
    label=$1; vm=$2; save=$3
    shift 3
    if test "$save" = yes; then
	(CP_SESSION_SAVE=1; export CP_SESSION_SAVE; pharo_on "$vm" st "$WORK/session-check.st") >"$WORK/$label.out" 2>&1
    else
	(unset CP_SESSION_SAVE; pharo_on "$vm" st "$WORK/session-check.st") >"$WORK/$label.out" 2>&1
    fi
    tr '\r' '\n' <"$WORK/$label.out" | grep '^session: ' | sed 's/^/    /'
    for pattern in "$@"; do
	if ! tr '\r' '\n' <"$WORK/$label.out" | grep -E "$pattern" >/dev/null; then
	    fail "$label: no line matching '$pattern' (see $WORK/$label.out)"
	fi
    done
}
all='#\(#basic #shape #text\)'
# one libcairo loaded, natively (the WebAssembly VMs link cairo in)
one_cairo='^session: libcairo loaded 1 '
test $wasm = yes && one_cairo='^session: libcairo loaded '
pharo_on "$VM_DIR" st "$WORK/session-save.st" >"$WORK/session-save.out" 2>&1
tr '\r' '\n' <"$WORK/session-save.out" | grep '^session-save: ' | sed 's/^/    /'
if echo "$banner" | grep 'plugin nil,' >/dev/null; then
    session_check session "$VM_DIR" no \
	"^session: available false bound false installed #\(\) wanted $all verify #\(\)" \
	"$one_cairo" \
	'^session: pixels of stock Athens true installed after #\(\)'
    test -n "$VM0_DIR" && echo "vm0: skipped (the VM has no plugin)"
else
    session_check session "$VM_DIR" no \
	"^session: available true bound true installed $all wanted $all verify #\(\)" \
	"$one_cairo" \
	"^session: pixels of stock Athens true installed after $all"
    if test -n "$VM0_DIR"; then
	session_check vm0 "$VM0_DIR" yes \
	    "^session: available false bound false installed #\(\) wanted $all" \
	    "^session: libcairo loaded 1 #\('$VM0_DIR/" \
	    '^session: pixels of stock Athens true installed after #\(\)'
	session_check vm0-back "$VM_DIR" no \
	    "^session: available true bound true installed $all wanted $all verify #\(\)" \
	    "^session: libcairo loaded 1 #\('$VM_DIR/" \
	    "^session: pixels of stock Athens true installed after $all"
    fi
fi
test -z "$VM0_DIR" || test -f "$WORK/vm0.out" || echo "vm0: not run"

if test $failed = 0; then
    echo "PASS: $VM_DIR/pharo ($WORK)"
fi
exit $failed
