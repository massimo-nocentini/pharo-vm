#!/bin/sh
# run-bench.sh - the SimdUTF benchmark on the native and WebAssembly VMs
#
# usage: sh plugins/SimdUTFPlugin/bench/run-bench.sh [--cog DIR] [--stack DIR]
#            [--wasm DIR] [--node NODE] --image IMAGE --out DIR [--repeat N]
#            [--vms LIST] [--cpu N|none]
#
#   --cog DIR     the CoInterpreter VM with the SimdUTFPlugin and libSimdUTFFFI:
#                 the directory of its pharo (build/vm of a CMake build tree),
#                 or the build tree itself: the VM native-cog
#   --stack DIR   the same for the StackVM: native-stack
#   --wasm DIR    a make wasm build directory (WASM_BUILDDIR): wasm-node, its
#                 node/pharo, and wasm-web-node, its web/pharo-web.js driven
#                 by web-bench.js in node
#   --node NODE   the node of the WebAssembly VMs (default: node)
#   --image IMAGE a Pharo 12 to 15 image, with its .changes and one .sources
#                 next to it, or the zip of one (files.pharo.org)
#   --out DIR     where the results go: LABEL.runN.tsv, the output of SUBench,
#                 and LABEL.runN.err for each VM and run, summary.md
#   --repeat N    runs of the whole matrix, one after the other (default 2)
#   --vms LIST    comma-separated labels to run, of those the options give
#                 (default: all of them)
#   --cpu N       the CPU that taskset pins every VM to (default 2); none
#                 runs them unpinned
#
# The image is prepared once, natively (prepare-image.st, with the Cog VM if
# given, else the Stack VM), into DIR/image/subench.image: the groups Core and
# FFI of SimdUTF from the submodule simdutf.st, and SUBench.st.  Each VM then
# runs a fresh copy of it with run-bench.st, N=10 and 2 warm-up calls natively,
# N=5 and 1 in WebAssembly (SU_N and SU_WARMUP override both), the VMs
# sequentially.  The INFO lines get the CPU model and its frequency governor.
# summarize.py then writes DIR/summary.md and checks the gates.
# Exits with 1 when a VM failed (an ERROR or CHECK DIFFERENT line of SUBench
# included) or a gate failed.
#
# Environment: SU_OPS, SU_DATA, SU_SIZES, SU_BACKENDS, SU_N, SU_WARMUP,
# SU_BUDGET, SU_MIN_SAMPLE (see SUBench.st); SU_PLUGIN_DIR, the directory of
# the plugin whose tests/load-simdutf.st and simdutf.st/ the image is
# prepared from (default: the parent of this directory).

bench=$(cd "$(dirname "$0")" && pwd)
plugin=${SU_PLUGIN_DIR:-$(dirname "$bench")}
cog= stack= wasm= node=node image= out= repeat=2 vms= cpu=2

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
	--out) out=$2 ;;
	--repeat) repeat=$2 ;;
	--vms) vms=$2 ;;
	--cpu) cpu=$2 ;;
	*) usage ;;
    esac
    test $# -ge 2 || usage
    shift 2
done
test -n "$image" && test -n "$out" || usage

die() { echo "run-bench: $*" >&2; exit 1; }
absolute() { (cd "$1" && pwd); }

test -d "$plugin/simdutf.st/src/BaselineOfSimdUTF" ||
    die "no $plugin/simdutf.st/src: git submodule update --init plugins/SimdUTFPlugin/simdutf.st"
plugin=$(absolute "$plugin")
test -f "$image" || die "no image $image"
mkdir -p "$out" || exit 1
out=$(absolute "$out")
case $image in
*.zip)
    rm -rf "$out/image-zip"
    mkdir -p "$out/image-zip" && unzip -q -o "$image" -d "$out/image-zip" || die "cannot unzip $image"
    image=$(ls "$out"/image-zip/*.image 2>/dev/null | head -1)
    test -n "$image" || die "no .image in the zip"
    ;;
esac
imagedir=$(absolute "$(dirname "$image")")
image=$imagedir/$(basename "$image")
changes=${image%.image}.changes
sources=$(ls "$imagedir"/*.sources 2>/dev/null)
test -f "$changes" || die "no $changes next to the image"
test -n "$sources" && test "$(echo "$sources" | wc -l)" -eq 1 || die "expected one .sources in $imagedir"

# A VM: the directory of its pharo, given or under build/vm of a build tree
vmdir() {
    if test -x "$1/pharo"; then absolute "$1"
    elif test -x "$1/build/vm/pharo"; then absolute "$1/build/vm"
    else die "no pharo in $1 or $1/build/vm"
    fi
}
test -z "$cog" || cog=$(vmdir "$cog") || exit 1
test -z "$stack" || stack=$(vmdir "$stack") || exit 1
if test -n "$wasm"; then
    test -d "$wasm" || die "no build directory $wasm"
    wasm=$(absolute "$wasm")
fi

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
SU_CPU=$(sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | sed -n 1p)
SU_CPU=${SU_CPU:-n/a}
pin=
if test "x$cpu" = xnone; then
    SU_TASKSET=none SU_GOVERNOR=n/a
elif command -v taskset >/dev/null 2>&1; then
    pin="taskset -c $cpu"
    SU_TASKSET="taskset -c $cpu"
    SU_GOVERNOR=$(cat /sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_governor 2>/dev/null || echo n/a)
else
    echo "run-bench: warning: no taskset, the VMs run unpinned" >&2
    SU_TASKSET=none SU_GOVERNOR=n/a
fi
export SU_CPU SU_GOVERNOR SU_TASKSET

# Prepare the image, natively
native=${cog:-$stack}
test -n "$native" || die "preparing the image needs a native VM: give --cog or --stack"
rm -rf "$out/image"
mkdir -p "$out/image"
cp "$image" "$out/image/subench.image"
cp "$changes" "$out/image/subench.changes"
cp "$sources" "$out/image/"
echo "run-bench: preparing $out/image/subench.image from $image"
if ! (cd "$out/image" && env SU_PLUGIN_DIR="$plugin" SU_BENCH_DIR="$bench" SU_GROUPS='Core FFI' \
	LD_LIBRARY_PATH="$native" "$native/pharo" --headless subench.image --no-default-preferences \
	st --save --quit "$bench/prepare-image.st") > "$out/prepare.log" 2>&1 \
   || ! grep -q '^prepare-image: SimdUTF and SUBench loaded' "$out/prepare.log"; then
    cat "$out/prepare.log" >&2
    die "preparing the image failed (log: $out/prepare.log)"
fi
sed -n 's/^prepare-image: //p; s/^\(SimdUTF: loaded.*\)/\1/p' "$out/prepare.log"

# One VM, one run: $1 label, $2 run
bench_vm() {
    dir=$out/run-$1
    rm -rf "$dir"
    mkdir -p "$dir"
    cp "$out/image/subench.image" "$out/image/subench.changes" "$out/image/"*.sources "$dir/"
    case $1 in
	native-cog|native-stack)
	    test "$1" = native-cog && vm=$cog || vm=$stack
	    (cd "$dir" && env SU_LABEL=$1 SU_RUN=$2 SU_N=${SU_N:-10} SU_WARMUP=${SU_WARMUP:-2} \
		LD_LIBRARY_PATH="$vm" $pin "$vm/pharo" --headless subench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
	wasm-node)
	    (cd "$dir" && env SU_LABEL=$1 SU_RUN=$2 SU_N=${SU_N:-5} SU_WARMUP=${SU_WARMUP:-1} \
		NODE="$node" $pin "$wasm/node/pharo" --headless subench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
	wasm-web-node)
	    env SU_LABEL=$1 SU_RUN=$2 SU_N=${SU_N:-5} SU_WARMUP=${SU_WARMUP:-1} \
		$pin "$node" "$bench/web-bench.js" "$wasm/web" "$dir" "$bench/run-bench.st" ;;
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
