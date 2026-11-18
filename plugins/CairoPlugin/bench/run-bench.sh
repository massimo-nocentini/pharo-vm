#!/bin/sh
# run-bench.sh - the benchmark of the CairoPlugin: Athens drawing through the
# FFI against the plugin, in one image and one VM (docs/CairoPlugin.md)
#
#   sh run-bench.sh <vm-dir> <image.zip | X.image> [--wasm | --wasm-web] [--out DIR]
#
# <vm-dir> is the directory of the VM: build/vm of a native build (pharo,
# libCairoPlugin.so, libcairo.so.2), or, with --wasm and --wasm-web, the node
# directory of a WebAssembly build (make wasm), whose ../web holds pharo-web
# and ../image/web the image of the world.  <image> is a Pharo 12 to 15
# image, zipped or not.  The script works on a copy of the image in a
# directory of its own (CP_WORK_DIR, or DIR/work-<vm>-<pid>):
#
#   image    natively the image given; on WebAssembly the image of the world
#            (../image/web, which has OSWindow-Web) when the build has one of
#            the same Pharo version, else the image given with
#            tests/web-surface.st filed in (the methods of OSWindow-Web that
#            make a surface's Form without the SurfacePlugin, which
#            h-morph-frame needs);
#   load     tests/load-cairo.st loads the groups Cairo, CairoTests and
#            CairoBenchmarks of BaselineOfVMPluginsPrimitives (from
#            CP_ST_REPO, or the submodule plugins/UtilsPlugin/
#            vm-plugins-primitives.st) and saves the image;
#   run      bench/run-bench.st runs CairoPluginBenchmark in the image,
#            started again (CairoPlugin startUp: binds the plugin): natively
#            and with --wasm by <vm-dir>/pharo (vm native-cog, wasm-node);
#            with --wasm-web in pharo-web, the module of the pages, through
#            bench/web-bench.mjs (vm wasm-web: the MICRO set and b-rects by
#            default).
#
# The lines of the run (BENCH, CALLS, MICRO, SKIP, ERROR and COND, with the
# conditions: host, CPU, load average before and after, commits) go to
# DIR/bench-<pharo>-<vm>.txt (DIR: --out, default ./bench-results), which
# bench/summarize.py turns into the Markdown report.  The status is 0 when the
# run finished (BENCH-DONE) with no scene whose pixels differ between the
# modes (PIXELS-DIFFER), none whose calls of a cairo function differ
# (CALLS-DIFFER), no plugin mode that fell back to the FFI (FALLBACK) and no
# error; else 1.
#
# Environment: CP_BENCH_SCENES, CP_BENCH_SIZES, CP_BENCH_REPS, CP_BENCH_SCALE,
# CP_BENCH_ROASSAL_SCALE, CP_BENCH_MODES, CP_BENCH_MICRO_CALLS (see the class
# comment of CairoPluginBenchmark; on WebAssembly the defaults are 5
# repetitions, scale 0.1 for the scenes a to e, 1 for the Roassal ones, and
# 100000 calls per micro-benchmark), CP_ST_REPO, CP_WORK_DIR, CP_BENCH_STOCK
# (set: the image given even when the build has image/web), CP_BENCH_TIMEOUT
# (seconds, 14400 by default), CP_BENCH_VM (the name of the VM in the lines
# and the file, native-cog or wasm-node by default: e.g. native-cog-vm0 for a
# VM without the plugin), NODE (the node of --wasm-web, and of the node VM).

usage() {
    echo "usage: $0 <vm-dir> <image.zip | X.image> [--wasm | --wasm-web] [--out DIR]" >&2
    exit 2
}
test $# -ge 2 || usage
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE=$2
shift 2
kind=native
OUT=$(pwd)/bench-results
while test $# -gt 0; do
    case $1 in
    --wasm) kind=wasm ;;
    --wasm-web) kind=wasm-web ;;
    --out) test $# -ge 2 || usage; OUT=$2; shift ;;
    *) usage ;;
    esac
    shift
done
mkdir -p "$OUT" || exit 1
OUT=$(cd "$OUT" && pwd)
BENCH=$(cd "$(dirname "$0")" && pwd)
PLUGIN=$(cd "$BENCH/.." && pwd)
REPO=$(cd "$PLUGIN/../.." && pwd)
case $kind in
native) vm=${CP_BENCH_VM:-native-cog} ;;
wasm) vm=${CP_BENCH_VM:-wasm-node} ;;
wasm-web) vm=wasm-web ;;
esac
if ! test -x "$VM_DIR/pharo"; then
    echo "FAIL: no $VM_DIR/pharo" >&2
    exit 1
fi
WEB_DIR=
if test $kind != native; then
    if ! test -f "$VM_DIR/pharo.wasm"; then
	echo "FAIL: $VM_DIR is not the node directory of a WebAssembly build (no pharo.wasm)" >&2
	exit 1
    fi
    if test $kind = wasm-web; then
	WEB_DIR=$(cd "$VM_DIR/../web" 2>/dev/null && pwd)
	if test -z "$WEB_DIR" || ! test -f "$WEB_DIR/pharo-web.js"; then
	    echo "FAIL: no pharo-web.js in $VM_DIR/../web" >&2
	    exit 1
	fi
    fi
    # the defaults of the WebAssembly runs (the interpreter is ~30 times
    # slower than Cog, the node clock has 1 ms resolution: every timed run
    # should take 50 ms or more)
    : "${CP_BENCH_REPS:=5}" "${CP_BENCH_SCALE:=0.1}" "${CP_BENCH_ROASSAL_SCALE:=1}" "${CP_BENCH_MICRO_CALLS:=100000}"
    export CP_BENCH_REPS CP_BENCH_SCALE CP_BENCH_ROASSAL_SCALE CP_BENCH_MICRO_CALLS
    if test $kind = wasm-web; then
	: "${CP_BENCH_SCENES:=b-rects,micro}"
	export CP_BENCH_SCENES
    fi
fi
if test -z "$CP_ST_REPO" && ! test -d "$REPO/plugins/UtilsPlugin/vm-plugins-primitives.st/src/BaselineOfVMPluginsPrimitives"; then
    echo "FAIL: no $REPO/plugins/UtilsPlugin/vm-plugins-primitives.st/src: git submodule update --init plugins/UtilsPlugin/vm-plugins-primitives.st" >&2
    exit 1
fi

WORK=${CP_WORK_DIR:-$OUT/work-$vm-$$}
mkdir -p "$WORK" || exit 1
WORK=$(cd "$WORK" && pwd)
rm -f "$WORK"/Bench.image "$WORK"/Bench.changes "$WORK"/*.sources "$WORK"/*.out

# the image
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
surface=no
if test $kind != native; then
    surface=yes
    web_image=$(ls "$VM_DIR"/../image/web/*.image 2>/dev/null | head -1)
    given=$(cat "$(dirname "$image")/pharo.version" 2>/dev/null)
    built=$(cat "$VM_DIR/../image/stock/pharo.version" 2>/dev/null)
    if test -z "$CP_BENCH_STOCK" && test -n "$web_image" && test -n "$given" && test "$given" = "$built"; then
	image=$web_image
	surface=no
    fi
fi
cp "$image" "$WORK/Bench.image" || exit 1
changes=${image%.image}.changes
if test -f "$changes"; then cp "$changes" "$WORK/Bench.changes"; else : >"$WORK/Bench.changes"; fi
cp "$(dirname "$image")"/*.sources "$WORK/" 2>/dev/null
rm -rf "$WORK/zip"
echo "image: $image, copied to $WORK/Bench.image"
echo "vm: $VM_DIR/pharo ($vm)"

limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${CP_BENCH_TIMEOUT:-14400}"; fi
pharo_run() {
    if test $kind = native; then
	(cd "$WORK" && LD_LIBRARY_PATH="$VM_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
	    $limit "$VM_DIR/pharo" --headless Bench.image --no-default-preferences "$@")
    else
	(cd "$WORK" && env -u LD_LIBRARY_PATH $limit "$VM_DIR/pharo" --headless Bench.image --no-default-preferences "$@")
    fi
}

if test $surface = yes; then
    pharo_run st --save --quit "$PLUGIN/tests/web-surface.st" >"$WORK/surface.out" 2>&1
    if ! tr '\r' '\n' <"$WORK/surface.out" | grep '^web-surface: ' >/dev/null; then
	echo "FAIL surface: web-surface.st (see $WORK/surface.out)"
	exit 1
    fi
    echo "surface: $(tr '\r' '\n' <"$WORK/surface.out" | grep '^web-surface: ')"
fi

(CP_PLUGIN_DIR=$REPO/plugins/UtilsPlugin CP_GROUPS='Cairo CairoTests CairoBenchmarks'
 export CP_PLUGIN_DIR CP_GROUPS
 pharo_run st --save --quit "$PLUGIN/tests/load-cairo.st") >"$WORK/load.out" 2>&1
status=$?
banner=$(tr '\r' '\n' <"$WORK/load.out" | grep '^CairoPlugin: loaded, plugin ')
if test $status -ne 0 || test -z "$banner"; then
    tr '\r' '\n' <"$WORK/load.out" | grep -v '^ *$' | tail -20 | sed 's/^/    /'
    echo "FAIL load: exit status $status (see $WORK/load.out)"
    exit 1
fi
echo "load: $banner"

# the conditions of the host (the image prints its own)
raw=$WORK/bench.out
cond() { echo "COND - $vm $1 $2"; }
commit() { git -C "$1" rev-parse --short HEAD 2>/dev/null; }
dirty() { test -n "$(git -C "$1" status --porcelain 2>/dev/null | head -1)" && echo ' (with uncommitted changes)'; }
if test -n "$CP_ST_REPO"; then st_dir=$(cd "$CP_ST_REPO/.." && pwd); else st_dir=$REPO/plugins/UtilsPlugin/vm-plugins-primitives.st; fi
{
    cond date "$(date '+%Y-%m-%d %H:%M:%S %z')"
    cond host "$(uname -srm)"
    cond cpu "$(grep -m1 '^model name' /proc/cpuinfo 2>/dev/null | sed 's/^model name[^:]*: //') x $(nproc 2>/dev/null)"
    cond load-before "$(cat /proc/loadavg 2>/dev/null)"
    cond vm-dir "$VM_DIR"
    cond pharo-vm "$(commit "$REPO")$(dirty "$REPO")"
    cond st-repo "$st_dir $(commit "$st_dir")$(dirty "$st_dir")"
    cond image-file "$image"
    test $kind = native || cond node "$("${NODE:-node}" --version 2>/dev/null)"
    cond env "CP_BENCH_SCENES=$CP_BENCH_SCENES CP_BENCH_SIZES=$CP_BENCH_SIZES CP_BENCH_REPS=$CP_BENCH_REPS CP_BENCH_SCALE=$CP_BENCH_SCALE CP_BENCH_ROASSAL_SCALE=$CP_BENCH_ROASSAL_SCALE CP_BENCH_MODES=$CP_BENCH_MODES CP_BENCH_MICRO_CALLS=$CP_BENCH_MICRO_CALLS"
} >"$WORK/cond-before.out"

started=$(date +%s)
if test $kind = wasm-web; then
    (cd "$WORK" && env -u LD_LIBRARY_PATH $limit "${NODE:-node}" "$BENCH/web-bench.mjs" "$REPO" "$WEB_DIR" "$WORK" "$BENCH/run-bench.st") >"$raw" 2>"$WORK/bench.err"
    status=$?
else
    (CP_BENCH_VM=$vm; export CP_BENCH_VM; pharo_run st "$BENCH/run-bench.st") >"$raw" 2>"$WORK/bench.err"
    status=$?
fi
seconds=$(($(date +%s) - started))
{
    cond load-after "$(cat /proc/loadavg 2>/dev/null)"
    cond seconds "$seconds"
    cond exit-status "$status"
} >"$WORK/cond-after.out"

major=$(tr '\r' '\n' <"$raw" | sed -n 's/^COND \([0-9][0-9]*\) .*/\1/p' | head -1)
test -n "$major" || major=unknown
result=$OUT/bench-$major-$vm.txt
{
    cat "$WORK/cond-before.out"
    tr '\r' '\n' <"$raw" | grep -E '^(BENCH|CALLS|MICRO|COND|SKIP|ERROR|BENCH-DONE)( |-|$)'
    cat "$WORK/cond-after.out"
} >"$result"
echo "run: $seconds s, exit status $status, lines in $result"

failed=0
if ! grep -q '^BENCH-DONE failures 0$' "$result"; then
    tr '\r' '\n' <"$raw" | tail -20 | sed 's/^/    /'
    sed 's/^/    /' "$WORK/bench.err" | tail -10
    echo "FAIL run: no 'BENCH-DONE failures 0' (see $raw)"
    failed=1
fi
if grep -E 'PIXELS-DIFFER|CALLS-DIFFER|FALLBACK|^ERROR' "$result" >/dev/null; then
    grep -E 'PIXELS-DIFFER|CALLS-DIFFER|FALLBACK|^ERROR' "$result" | sed 's/^/    /'
    echo "FAIL run: pixels or calls that differ, fallbacks or errors"
    failed=1
fi
grep '^SKIP' "$result" | sed 's/^/    /'
test $failed = 0 && echo "PASS: $result"
exit $failed
