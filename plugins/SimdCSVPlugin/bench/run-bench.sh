#!/bin/sh
# run-bench.sh - the SimdCSV benchmark on the native and WebAssembly VMs
#
# usage: sh plugins/SimdCSVPlugin/bench/run-bench.sh [--cog DIR] [--stack DIR]
#            [--wasm DIR] [--node NODE] [--simdcsv PATH] --image IMAGE --out DIR
#            [--repeat N] [--vms LIST] [--cpu N|none]
#
#   --cog DIR     the CoInterpreter VM with the SimdCSVPlugin: the directory of
#                 its pharo (build/vm of a CMake build tree), or the build tree
#                 itself: the VM native-cog
#   --stack DIR   the same for the StackVM: native-stack
#   --wasm DIR    a make wasm build directory (WASM_BUILDDIR): wasm-node, its
#                 node/pharo
#   --node NODE   the node of the WebAssembly VM (default: node)
#   --simdcsv PATH  the archive simdcsv-a9494a8.tar.gz or its tree: scbench.cpp
#                 is built and run too, against upstream's find_indexes: engine
#   --image IMAGE a Pharo 12 to 15 image, with its .changes and one .sources
#                 next to it, or the zip of one (files.pharo.org)
#   --out DIR     where the results go: LABEL.runN.tsv, the output of SCBench
#                 (engine.runN.tsv: of scbench), and LABEL.runN.err for each VM
#                 and run, summary.md
#   --repeat N    runs of the whole matrix, one after the other (default 2)
#   --vms LIST    comma-separated labels to run, of those the options give
#                 (default: all of them)
#   --cpu N       the CPU that taskset pins every VM to (default 2); none
#                 runs them unpinned
#
# The image is prepared once, natively (prepare-image.st, with the Cog VM if
# given, else the Stack VM), into DIR/image/scbench.image: the groups Core and
# NeoTests of SimdCSV from the submodule simdcsv.st (NeoTests has Metacello
# download NeoCSV, which nothing but this image holds), and SCBench.st; the
# data are written into DIR/data for scbench.  Each VM then runs a fresh copy
# of it with run-bench.st, 5 samples a series, the VMs sequentially.  The INFO
# lines get the CPU model and its frequency governor.  summarize.py then
# writes DIR/summary.md and checks the gates.
# Exits with 1 when a VM failed (an ERROR or CHECK DIFFERENT line of SCBench
# included) or a gate is missed.
#
# Environment: SC_OPS, SC_DATA, SC_SIZES, SC_BACKENDS, SC_N, SC_WARMUP,
# SC_BUDGET, SC_MIN_SAMPLE, SC_SMALLTALK_MAX, SC_LARGE (see SCBench.st); SC_MB,
# the size of scbench's texts (default 64); SIMDCSV_ST_DIR, the repository of
# the Smalltalk code (default: simdcsv.st in the plugin's directory); CXX
# (default clang++).

bench=$(cd "$(dirname "$0")" && pwd)
plugin=$(dirname "$bench")
cog= stack= wasm= node=node simdcsv= image= out= repeat=2 vms= cpu=2

usage() {
    sed -n '4,6p' "$0" | sed 's/^# //' >&2
    exit 2
}
while test $# -gt 0; do
    case $1 in
	--cog) cog=$2 ;;
	--stack) stack=$2 ;;
	--wasm) wasm=$2 ;;
	--node) node=$2 ;;
	--simdcsv) simdcsv=$2 ;;
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

stdir=${SIMDCSV_ST_DIR:-$plugin/simdcsv.st}
test -d "$stdir/src/BaselineOfSimdCSV" ||
    die "no $stdir/src/BaselineOfSimdCSV: git submodule update --init plugins/SimdCSVPlugin/simdcsv.st"
stdir=$(absolute "$stdir")
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
test -z "$simdcsv" || all="$all engine"
test -z "$cog" || all="$all native-cog"
test -z "$stack" || all="$all native-stack"
test -z "$wasm" || all="$all wasm-node"
if test -n "$vms"; then
    for v in $(echo "$vms" | tr ',' ' '); do
	case " $all " in *" $v "*) ;; *) die "--vms: $v is not one of:$all" ;; esac
    done
    all=$(echo "$vms" | tr ',' ' ')
fi
test -n "$all" || die "nothing to run: give --cog, --stack, --wasm or --simdcsv"

# The environment of the INFO lines
SC_CPU=$(sed -n 's/^model name[[:space:]]*: //p' /proc/cpuinfo 2>/dev/null | sed -n 1p)
SC_CPU=${SC_CPU:-n/a}
pin=
if test "x$cpu" = xnone; then
    SC_TASKSET=none SC_GOVERNOR=n/a
elif command -v taskset >/dev/null 2>&1; then
    pin="taskset -c $cpu"
    SC_TASKSET="taskset -c $cpu"
    SC_GOVERNOR=$(cat /sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_governor 2>/dev/null || echo n/a)
else
    echo "run-bench: warning: no taskset, the VMs run unpinned" >&2
    SC_TASKSET=none SC_GOVERNOR=n/a
fi
export SC_CPU SC_GOVERNOR SC_TASKSET

# Prepare the image, natively
native=${cog:-$stack}
test -n "$native" || die "preparing the image needs a native VM: give --cog or --stack"
rm -rf "$out/image" "$out/data"
mkdir -p "$out/image" "$out/data"
cp "$image" "$out/image/scbench.image"
cp "$changes" "$out/image/scbench.changes"
cp "$sources" "$out/image/"
echo "run-bench: preparing $out/image/scbench.image from $image"
if ! (cd "$out/image" && env SIMDCSV_PLUGIN_DIR="$plugin" SIMDCSV_ST_DIR="$stdir" SC_BENCH_DIR="$bench" \
	SIMDCSV_GROUPS='Core NeoTests' SC_DUMP_DIR="$out/data" \
	LD_LIBRARY_PATH="$native" "$native/pharo" --headless scbench.image --no-default-preferences \
	st --save --quit "$bench/prepare-image.st") > "$out/prepare.log" 2>&1 \
   || ! grep -q '^prepare-image: SimdCSV, NeoCSV and SCBench loaded' "$out/prepare.log"; then
    cat "$out/prepare.log" >&2
    die "preparing the image failed (log: $out/prepare.log)"
fi
sed -n 's/^prepare-image: //p; s/^\(SimdCSV: loaded.*\)/\1/p' "$out/prepare.log"

# Build scbench, with the flags of cmake/simdcsv.cmake
case " $all " in
*" engine "*)
    test -e "$simdcsv" || die "no $simdcsv"
    rm -rf "$out/scbench"
    mkdir -p "$out/scbench/inc"
    if test -f "$simdcsv"; then
	mkdir "$out/scbench/tree"
	tar -xzf "$simdcsv" -C "$out/scbench/tree" || exit 1
	simdcsv=$(ls -d "$out"/scbench/tree/*/)
    fi
    main=$simdcsv/src/main.cpp
    test -f "$main" || die "no $main"
    echo "d9b98882a52d2b018980ee9e6a07625af05be1911a706b60f0a105b2018fc383  $main" | sha256sum -c --quiet - ||
	die "$main is not simdcsv a9494a8"
    tail -c +303 "$main" | head -c 5088 > "$out/scbench/inc/simdcsv_slice.inc"
    awk '/^struct ParsedCSV/ { p = 1 } /^int main\(/ { exit } p' "$main" > "$out/scbench/inc/simdcsv_upstream.inc"
    flags="-std=gnu++17 -O3 -g0 -fno-exceptions -fno-rtti -fno-strict-aliasing -ffp-contract=off -fvisibility=hidden"
    inc="-I$plugin/include/common -I$out/scbench/inc"
    cxx=${CXX:-clang++}
    upstream=
    if test "$(uname -m)" = x86_64; then
	upstream=-DSC_UPSTREAM
	$cxx $flags -mavx2 -mbmi -mpopcnt -mpclmul $inc -c "$bench/upstream.cpp" -o "$out/scbench/upstream.o" ||
	    die "upstream.cpp does not compile"
    fi
    $cxx $flags $inc -c "$plugin/src/engine/scEngine.cpp" -o "$out/scbench/scEngine.o" &&
    $cxx $flags $upstream $inc -c "$bench/scbench.cpp" -o "$out/scbench/scbench.o" &&
    $cxx -o "$out/scbench/scbench" "$out"/scbench/*.o || die "scbench does not build"
    ;;
esac

# The engine, each kernel: the first (the best one) and upstream's without
# SIMDCSV_FORCE_IMPLEMENTATION
bench_engine() {
    printf 'INFO\tengine\trun=%s\tcpu=%s\tgovernor=%s\ttaskset=%s\tcompiler=%s\n' "$1" \
	"$SC_CPU" "$SC_GOVERNOR" "$SC_TASKSET" "$($cxx --version | sed -n 1p)"
    first=yes
    for kernel in $("$out/scbench/scbench" --kernels); do
	if test $first = yes; then
	    $pin "$out/scbench/scbench" --mb "${SC_MB:-64}" "$out"/data/*.csv || return 1
	    first=no
	else
	    SIMDCSV_FORCE_IMPLEMENTATION=$kernel $pin "$out/scbench/scbench" --mb "${SC_MB:-64}" "$out"/data/*.csv || return 1
	fi
    done
}

# One VM, one run: $1 label, $2 run
bench_vm() {
    if test "$1" = engine; then
	bench_engine $2 > "$out/$1.run$2.tsv" 2> "$out/$1.run$2.err"
	return
    fi
    dir=$out/run-$1
    rm -rf "$dir"
    mkdir -p "$dir"
    cp "$out/image/scbench.image" "$out/image/scbench.changes" "$out/image/"*.sources "$dir/"
    case $1 in
	native-cog|native-stack)
	    test "$1" = native-cog && vm=$cog || vm=$stack
	    (cd "$dir" && env SC_LABEL=$1 SC_RUN=$2 LD_LIBRARY_PATH="$vm" \
		$pin "$vm/pharo" --headless scbench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
	wasm-node)
	    (cd "$dir" && env SC_LABEL=$1 SC_RUN=$2 NODE="$node" \
		$pin "$wasm/node/pharo" --headless scbench.image \
		--no-default-preferences st --quit "$bench/run-bench.st") ;;
    esac > "$out/$1.run$2.tsv" 2> "$out/$1.run$2.err"
    status=$?
    rm -rf "$dir"
    return $status
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
