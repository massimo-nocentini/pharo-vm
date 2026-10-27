#!/bin/sh
# run-image-tests.sh - the LargeIntegers plugin of a VM, driven from a Pharo 12 image
#
#   sh run-image-tests.sh [--record] <vm-dir> <image> [reference-vm-dir]
#
# <vm-dir> holds pharo and its plugins (build/vm of a build); <image> is a
# Pharo 12 .image, with its .changes and .sources next to it.  Every VM run
# works on a fresh copy of the image in a directory of its own, and runs one
# script made of st/lib.st and a driver (see README.md):
#
#   repros     st/repro.st, one VM each: li01 li05 li06 li07 arity; the
#              output must be that in expected/repros.txt (the plugin before
#              the kernels crashes on li01 and answers garbage in li06, li07
#              and arity);
#   diff       st/diff.st, 86,417 calls; the 71 shapes that made the old
#              plugin write past an object or read uninitialised memory run
#              too and must answer right (UB-LI0x OK); the output, with those
#              lines put back as the SKIP-LI0x lines the old plugin printed,
#              must match expected/image-diff.sha256 (digests recorded from
#              the plugin before the kernels);
#   kernels    st/diff-kernels.st, 6,297 multiplies and divisions and 1,596
#              Montgomery products over the kernel thresholds, against
#              expected/image-kernels.sha256;
#   sunit      st/imagetests.st: the image's integer test classes.
#
# With [reference-vm-dir], diff and kernels also run on that VM (without the
# UB shapes) and the two outputs must be identical line for line; the
# reference's digests are compared with the expected ones for information.
# --record (needs the reference) writes the expected digests from the
# reference's output instead.
#
# Environment: LI_TIMEOUT (seconds a VM run may take, 300 by default; a run
# takes a few seconds), LI_WORK_DIR (where the runs go, kept; default a
# temporary directory, removed unless a step fails).  About 10 s in all, a
# little more with a reference.
# Exit status 0 when every step passed, 1 when one failed, 2 on a usage error.

record=
if test "$1" = --record; then record=1; shift; fi
if test $# -lt 2 || test $# -gt 3; then
    echo "usage: $0 [--record] <vm-dir> <image> [reference-vm-dir]" >&2
    exit 2
fi
TESTS=$(cd "$(dirname "$0")" && pwd) || exit 2
ST=$TESTS/st
EXPECTED=$TESTS/expected

. "$TESTS/vm.sh"
VM_DIR=$(cd "$1" 2>/dev/null && pwd) && VM=$(vmexe "$VM_DIR") || {
    echo "run-image-tests: no pharo VM in '$1' (the <vm-dir> argument)" >&2; exit 2; }
REF_DIR= REF=
if test $# = 3; then
    REF_DIR=$(cd "$3" 2>/dev/null && pwd) && REF=$(vmexe "$REF_DIR") || {
        echo "run-image-tests: no pharo VM in '$3' (the [reference-vm-dir] argument)" >&2; exit 2; }
elif test -n "$record"; then
    echo "run-image-tests: --record needs a reference-vm-dir" >&2
    exit 2
fi
case $2 in
*.image) ;;
*) echo "run-image-tests: '$2' is not a .image file (the <image> argument)" >&2; exit 2 ;;
esac
if ! test -f "$2"; then
    echo "run-image-tests: no image '$2'" >&2
    exit 2
fi
IMAGE=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
if ! command -v python3 >/dev/null 2>&1; then
    echo "run-image-tests: python3 is needed (digest.py)" >&2
    exit 2
fi
if test -n "$LI_WORK_DIR"; then
    mkdir -p "$LI_WORK_DIR" && WORK=$(cd "$LI_WORK_DIR" && pwd) || exit 2
    keep=1
else
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/li-image-tests.XXXXXX") || exit 2
    keep=
    trap 'test -n "$keep" || rm -rf "$WORK"' EXIT
    trap 'exit 130' INT TERM
fi
case ${LI_TIMEOUT:-300} in
*[!0-9]*|0) echo "run-image-tests: LI_TIMEOUT must be a positive number of seconds, not '$LI_TIMEOUT'" >&2; exit 2 ;;
esac
limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout -k 10 ${LI_TIMEOUT:-300}"; fi
echo "vm: $VM"
test -n "$REF" && echo "reference vm: $REF"
echo "image: $IMAGE"


results=
failed=0
pass() { echo "PASS $1${2:+: $2}"; results="$results
PASS $1"; }
fail() { echo "FAIL $1${2:+: $2}"; results="$results
FAIL $1"; failed=1; }
showrun() {   # the end of a failed run's output
    echo "   ($outcome after ${secs}s; output in $WORK/$1)"
    tail -n 5 "$WORK/$1/out" | sed 's/^/   | /'
    head -n 15 "$WORK/$1/err" | sed 's/^/   ! /'
}

# --- repros: each in a VM of its own
for r in li01 li05 li06 li07 arity; do
    run "repro-$r" "$VM" "$VM_DIR" "LIH new $r." "$ST/lib.st" "$ST/repro.st"
    awk -v r="== $r" '/^== / { on = ($0 == r); next } on' "$EXPECTED/repros.txt" > "$WORK/repro-$r/expected"
    if test "$outcome" != OK; then
        fail "repro $r" "$outcome"
        showrun "repro-$r"
    elif ! diff "$WORK/repro-$r/expected" "$WORK/repro-$r/out" > "$WORK/repro-$r/diff"; then
        fail "repro $r" "the output differs from expected/repros.txt"
        head -n 20 "$WORK/repro-$r/diff" | sed 's/^/   /'
    else
        pass "repro $r" "$(wc -l < "$WORK/repro-$r/out" | tr -d ' ') lines as expected (${secs}s)"
    fi
done

# --- differential drivers
# undo NAME: the output of the tested VM with each UB-LI0x OK line put back as the
# SKIP-LI0x line of a run without the UB shapes
undo() {
    sed 's/ => UB-LI0\([16]\) OK .*$/ => SKIP-LI0\1/' "$WORK/$1/out" > "$WORK/$1/out.skip"
}
differential() {   # NAME DRIVER SELECTOR DIGESTS
    name=$1 driver=$2 selector=$3 digests=$EXPECTED/$4
    if test -n "$REF"; then
        run "$name-reference" "$REF" "$REF_DIR" "LIH new $selector." "$ST/lib.st" "$ST/$driver"
        refoutcome=$outcome
        if test "$refoutcome" != OK; then
            fail "$name reference" "$refoutcome"
            showrun "$name-reference"
        elif test -n "$record"; then
            {
                echo "# python3 -I digest.py < (the output of st/$driver), recorded with"
                echo "# run-image-tests.sh --record from the reference VM: the plugin before the kernels."
                python3 -I "$TESTS/digest.py" < "$WORK/$name-reference/out"
            } > "$digests"
            echo "     recorded $(grep -vc '^#' "$digests") digests in $digests"
        elif python3 -I "$TESTS/digest.py" --compare "$digests" < "$WORK/$name-reference/out" > "$WORK/$name-reference/digests"; then
            echo "     the reference matches $4 too"
        else
            echo "     note: the reference does NOT match $4:"
            head -n 10 "$WORK/$name-reference/digests" | sed 's/^/     /'
        fi
    fi
    run "$name" "$VM" "$VM_DIR" "LIH new runUB: true; $selector." "$ST/lib.st" "$ST/$driver"
    if test "$outcome" != OK; then
        fail "$name" "$outcome"
        showrun "$name"
        return
    fi
    undo "$name"
    ub=$(grep -c ' => UB-LI0[16] OK ' "$WORK/$name/out")
    if grep -q 'MISMATCH' "$WORK/$name/out"; then
        fail "$name" "wrong answers to the UB shapes"
        grep 'MISMATCH' "$WORK/$name/out" | head -n 10 | sed 's/^/   /'
        return
    fi
    calls=$(sed -n 's/^# done \([0-9]*\) calls.*$/\1/p' "$WORK/$name/out" | tail -n 1)
    if ! python3 -I "$TESTS/digest.py" --compare "$digests" < "$WORK/$name/out.skip" > "$WORK/$name/digests"; then
        fail "$name" "the output differs from $4"
        head -n 20 "$WORK/$name/digests" | sed 's/^/   /'
        return
    fi
    if test -n "$REF" && test "$refoutcome" = OK && ! cmp -s "$WORK/$name-reference/out" "$WORK/$name/out.skip"; then
        fail "$name" "the output differs from the reference's"
        diff "$WORK/$name-reference/out" "$WORK/$name/out.skip" | head -n 20 | sed 's/^/   /'
        return
    fi
    test "$ub" = 0 && ub= || ub=", $ub UB shapes answered right"
    pass "$name" "${calls:-?} calls$ub, digests as expected${REF:+, identical to the reference} (${secs}s)"
}
differential diff diff.st runDiff image-diff.sha256
differential kernels diff-kernels.st runKernels image-kernels.sha256

# --- the image's test classes
run sunit "$VM" "$VM_DIR" "" "$ST/imagetests.st"
if test "$outcome" != OK; then
    fail sunit "$outcome"
    showrun sunit
else
    sed 's/^/   /' "$WORK/sunit/out"
    classes=$(grep -c ' ran, ' "$WORK/sunit/out")
    if test "$classes" = 0 || grep -q '^missing ' "$WORK/sunit/out" ||
       grep -q '^   bad: ' "$WORK/sunit/out" ||
       grep ' ran, ' "$WORK/sunit/out" | grep -vq ' 0 failures, 0 errors'; then
        fail sunit "failures or errors"
    else
        pass sunit "$classes classes, no failure or error (${secs}s)"
    fi
fi

echo "---"
echo "$results" | sed '/^$/d'
if test $failed = 0; then
    echo "run-image-tests: all passed"
else
    keep=1
    echo "run-image-tests: FAILED (runs kept in $WORK)"
fi
exit $failed
