#!/bin/sh
# bench.sh - the LargeIntegers benchmark in a VM, or two VMs side by side
#
#   sh bench.sh <vm-dir> <image> [other-vm-dir]
#
# Runs st/bench.st (after st/lib.st) on a fresh copy of the Pharo 12 image:
# ns per direct primitive call of multiply and divide from 64 to 262,144 bits,
# the small operands where the kernels choose between the 32-bit loops and the
# limb code, a few other small calls, and image expressions (see README.md).
# With [other-vm-dir] the two VMs alternate, run by run, and the table shows
# the median of each and other/first.
#
# Environment: LI_BENCH_RUNS (runs per VM, 3 by default; the table shows
# medians), LI_TIMEOUT (seconds a run may take, 1800 by default),
# LI_WORK_DIR (where the runs go, kept; default a temporary directory).
# A run takes 20 to 30 s.  Pin the VMs to idle cores (numactl, taskset) for
# numbers within a few percent; on a busy host rows above about 1 us vary by
# 10% or more.

if test $# -lt 2 || test $# -gt 3; then
    echo "usage: $0 <vm-dir> <image> [other-vm-dir]" >&2
    exit 2
fi
TESTS=$(cd "$(dirname "$0")" && pwd) || exit 2
. "$TESTS/vm.sh"
dirs= exes=
for d in "$1" ${3:+"$3"}; do
    dir=$(cd "$d" 2>/dev/null && pwd) && exe=$(vmexe "$dir") || {
        echo "bench: no pharo VM in '$d'" >&2; exit 2; }
    dirs="$dirs $dir" exes="$exes $exe"
done
case $2 in
*.image) test -f "$2" || { echo "bench: no image '$2'" >&2; exit 2; } ;;
*) echo "bench: '$2' is not a .image file (the <image> argument)" >&2; exit 2 ;;
esac
IMAGE=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
runs=${LI_BENCH_RUNS:-3}
case $runs in
''|*[!0-9]*|0) echo "bench: LI_BENCH_RUNS must be a positive integer" >&2; exit 2 ;;
esac
if ! command -v python3 >/dev/null 2>&1; then
    echo "bench: python3 is needed for the table" >&2
    exit 2
fi
if test -n "$LI_WORK_DIR"; then
    mkdir -p "$LI_WORK_DIR" && WORK=$(cd "$LI_WORK_DIR" && pwd) || exit 2
else
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/li-bench.XXXXXX") || exit 2
    trap 'rm -rf "$WORK"' EXIT
    trap 'exit 130' INT TERM
fi
limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout -k 10 ${LI_TIMEOUT:-1800}"; fi

i=1
while test $i -le "$runs"; do
    v=1
    for dir in $dirs; do
        exe=$(echo $exes | cut -d' ' -f$v)
        run "vm$v-$i" "$exe" "$dir" "LIH new benchKernels; benchSmall; benchImage." "$TESTS/st/lib.st" "$TESTS/st/bench.st"
        echo "run $i, vm $v ($dir): $outcome, ${secs}s" >&2
        if test "$outcome" != OK; then
            tail -n 5 "$WORK/vm$v-$i/out" "$WORK/vm$v-$i/err" >&2
            exit 1
        fi
        v=$((v + 1))
    done
    i=$((i + 1))
done

python3 -I - "$WORK" "$runs" $dirs <<'EOF'
import re, statistics, sys
work, runs, dirs = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
order, rows = [], [{} for _ in dirs]
for v in range(len(dirs)):
    for i in range(1, runs + 1):
        for line in open('%s/vm%d-%d/out' % (work, v + 1, i)):
            m = re.match(r'^(.*) (ns|ms): ([0-9.]+)$', line.strip())
            if m:
                key = '%s (%s)' % (m.group(1), m.group(2))
                if key not in rows[v] and v == 0:
                    order.append(key)
                rows[v].setdefault(key, []).append(float(m.group(3)))
for v, d in enumerate(dirs):
    print('vm%d: %s' % (v + 1, d))
print('median of %d run%s' % (runs, '' if runs == 1 else 's'))
head = '%-52s' % 'row' + ''.join('%12s' % ('vm%d' % (v + 1)) for v in range(len(dirs)))
print(head + ('   vm2/vm1' if len(dirs) == 2 else ''))
for k in order:
    med = [statistics.median(r[k]) if k in r else float('nan') for r in rows]
    fmt = lambda m: '%12.2f' % m if k.endswith('(ms)') else '%12.1f' % m if m < 10000 else '%12.0f' % m
    line = '%-52s' % k + ''.join(fmt(m) for m in med)
    if len(dirs) == 2:
        line += '%10.2f' % (med[1] / med[0]) if med[0] else '       nan'
    print(line)
EOF
