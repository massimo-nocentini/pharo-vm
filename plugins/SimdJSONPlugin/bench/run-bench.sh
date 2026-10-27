#!/bin/sh
# run-bench.sh - the SimdJSON benchmark on the native and WebAssembly VMs
#
# usage: sh plugins/SimdJSONPlugin/bench/run-bench.sh [--cog DIR] [--stack DIR]
#            [--wasm DIR] [--node NODE] --image IMAGE --corpus DIR --out DIR
#            [--repeat N] [--vms LIST] [--cpu N|none] [--files LIST] [--series LIST]
#
#   --cog DIR     a CMake build tree of the CoInterpreter with the plugin
#                 (DIR/build/vm/pharo): the VM native-cog
#   --stack DIR   the same for the StackVM: native-stack
#   --wasm DIR    a make wasm build directory (WASM_BUILDDIR): wasm-node, its
#                 node/pharo, and wasm-web-node, its web/pharo-web.js driven
#                 by web-bench.js in node
#   --node NODE   the node of the WebAssembly VMs (default: node)
#   --image IMAGE a Pharo 15 image with NeoJSON loaded, with its .changes and
#                 one .sources next to it
#   --corpus DIR  the JSON files (fetch-corpus.sh)
#   --out DIR     where the results go: LABEL.runN.tsv, the output of SJBench,
#                 and LABEL.runN.err for each VM and run, summary.md
#   --repeat N    runs of the whole matrix, one after the other (default 2)
#   --vms LIST    comma-separated labels to run, of those the options give
#                 (default: all of them)
#   --cpu N       the CPU that taskset pins every VM to (default 2); none
#                 runs them unpinned
#   --files, --series  SJ_FILES and SJ_SERIES (see SJBench.st)
#
# The image is prepared once, natively (prepare-image.st, with the Cog VM
# if given, else the Stack VM), into DIR/image/sjbench.image; each VM then
# runs a fresh copy of it with run-bench.st, N=10 and 2 warm-up runs natively,
# N=5 and 1 in WebAssembly (SJ_N and SJ_WARMUP override both), the VMs
# sequentially.  The INFO lines get the CPU model and its frequency governor.
# summarize.py then writes DIR/summary.md and checks the gates.
# Exits with 1 when a VM failed (an ERROR line of SJBench included) or a gate
# failed.
#
# Environment: SJ_N, SJ_WARMUP, SJ_BUDGET, SJ_MIN_SAMPLE, SJ_CHECK (see
# SJBench.st).

bench=$(cd "$(dirname "$0")" && pwd)
plugin=$(dirname "$bench")
cog= stack= wasm= node=node image= corpus= out= repeat=2 vms= cpu=2

usage() {
    sed -n '3,6p' "$0" | sed 's/^# //' >&2
    exit 2
}
while test $# -gt 0; do
    case $1 in
	--cog) cog=$2 ;;
	--stack) stack=$2 ;;
	--wasm) wasm=$2 ;;
	--node) node=$2 ;;
	--image) image=$2 ;;
	--corpus) corpus=$2 ;;
	--out) out=$2 ;;
	--repeat) repeat=$2 ;;
	--vms) vms=$2 ;;
	--cpu) cpu=$2 ;;
	--files) SJ_FILES=$2; export SJ_FILES ;;
	--series) SJ_SERIES=$2; export SJ_SERIES ;;
	*) usage ;;
    esac
    test $# -ge 2 || usage
    shift 2
done
test -n "$image" && test -n "$corpus" && test -n "$out" || usage

die() { echo "run-bench: $*" >&2; exit 1; }
absolute() { (cd "$1" && pwd); }

test -f "$image" || die "no image $image"
imagedir=$(absolute "$(dirname "$image")")
image=$imagedir/$(basename "$image")
changes=${image%.image}.changes
sources=$(ls "$imagedir"/*.sources 2>/dev/null)
test -f "$changes" || die "no $changes next to the image"
test -n "$sources" && test "$(echo "$sources" | wc -l)" -eq 1 || die "expected one .sources in $imagedir"
test -d "$corpus" || die "no corpus directory $corpus (fetch-corpus.sh)"
corpus=$(absolute "$corpus")
test -f "$corpus/twitter.json" || echo "run-bench: warning: no twitter.json in $corpus" >&2
mkdir -p "$out" || exit 1
out=$(absolute "$out")
for d in "$cog" "$stack" "$wasm"; do
    test -z "$d" || test -d "$d" || die "no build directory $d"
done
test -z "$cog" || cog=$(absolute "$cog")
test -z "$stack" || stack=$(absolute "$stack")
test -z "$wasm" || wasm=$(absolute "$wasm")

# The VMs: label and how to start it
all=
test -z "$cog" || all="$all native-cog"
test -z "$stack" || all="$all native-stack"
test -z "$wasm" || all="$all wasm-node wasm-web-node"
if test -n "$vms"; then
    for v in $(echo "$vms" | tr ',' ' '); do
	case " $all " in *" $v "*) ;; *) die "--vms: $v is not one of:$all" ;; esac
    done
    all=$(echo "$vms" | tr ',' ' ')
fi
test -n "$all" || die "no VM: give --cog, --stack or --wasm"

# The environment of the INFO lines
SJ_CPU=$(sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | sed -n 1p)
SJ_CPU=${SJ_CPU:-n/a}
pin=
if test "x$cpu" = xnone; then
    SJ_TASKSET=none SJ_GOVERNOR=n/a
elif command -v taskset >/dev/null 2>&1; then
    pin="taskset -c $cpu"
    SJ_TASKSET="taskset -c $cpu"
    SJ_GOVERNOR=$(cat /sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_governor 2>/dev/null || echo n/a)
else
    echo "run-bench: warning: no taskset, the VMs run unpinned" >&2
    SJ_TASKSET=none SJ_GOVERNOR=n/a
fi
export SJ_CPU SJ_GOVERNOR SJ_TASKSET

# Prepare the image, natively
native=${cog:-$stack}
test -n "$native" || die "preparing the image needs a native VM: give --cog or --stack"
rm -rf "$out/image"
mkdir -p "$out/image"
cp "$image" "$out/image/sjbench.image"
cp "$changes" "$out/image/sjbench.changes"
cp "$sources" "$out/image/"
echo "run-bench: preparing $out/image/sjbench.image from $image"
if ! (cd "$out/image" && env SJ_PLUGIN_DIR="$plugin" LD_LIBRARY_PATH="$native/build/vm" \
	"$native/build/vm/pharo" --headless sjbench.image --no-default-preferences \
	st --save --quit "$bench/prepare-image.st") > "$out/prepare.log" 2>&1 \
   || ! grep -q '^prepare-image: SimdJSON and SimdJSON-Bench loaded' "$out/prepare.log"; then
    cat "$out/prepare.log" >&2
    die "preparing the image failed (log: $out/prepare.log)"
fi
sed -n 's/^prepare-image: //p' "$out/prepare.log"

# One VM, one run: $1 label, $2 run
bench_vm() {
    dir=$out/run-$1
    rm -rf "$dir"
    mkdir -p "$dir"
    cp "$out/image/sjbench.image" "$out/image/sjbench.changes" "$out/image/"*.sources "$dir/"
    case $1 in
	native-cog|native-stack)
	    test "$1" = native-cog && vm=$cog || vm=$stack
	    (cd "$dir" && env SJ_LABEL=$1 SJ_RUN=$2 SJ_CORPUS="$corpus" SJ_N=${SJ_N:-10} SJ_WARMUP=${SJ_WARMUP:-2} \
		LD_LIBRARY_PATH="$vm/build/vm" $pin "$vm/build/vm/pharo" --headless sjbench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
	wasm-node)
	    (cd "$dir" && env SJ_LABEL=$1 SJ_RUN=$2 SJ_CORPUS="$corpus" SJ_N=${SJ_N:-5} SJ_WARMUP=${SJ_WARMUP:-1} \
		NODE="$node" $pin "$wasm/node/pharo" --headless sjbench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
	wasm-web-node)
	    env SJ_LABEL=$1 SJ_RUN=$2 SJ_N=${SJ_N:-5} SJ_WARMUP=${SJ_WARMUP:-1} \
		$pin "$node" "$bench/web-bench.js" "$wasm/web" "$dir" "$corpus" "$bench/run-bench.st" ;;
    esac > "$out/$1.run$2.tsv" 2> "$out/$1.run$2.err"
}

failures=0
r=1
while test $r -le "$repeat"; do
    for v in $all; do
	start=$(date +%s)
	bench_vm $v $r
	status=$?
	echo "run-bench: $v run $r: exit $status, $(($(date +%s) - start)) s, $out/$v.run$r.tsv"
	test $status -eq 0 || failures=$((failures + 1))
    done
    r=$((r + 1))
done

if command -v python3 >/dev/null 2>&1; then
    python3 -I "$bench/summarize.py" "$out"/*.run*.tsv > "$out/summary.md" || failures=$((failures + 1))
    echo "run-bench: $out/summary.md"
else
    echo "run-bench: no python3 for summarize.py" >&2
fi
test $failures -eq 0
