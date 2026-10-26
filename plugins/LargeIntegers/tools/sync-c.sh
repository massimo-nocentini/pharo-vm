#!/bin/sh
# sync-c.sh - keep src/common/LargeIntegers.c in sync with
# smalltalksrc/VMMaker/LargeIntegersPlugin.class.st
#
#   sync-c.sh [--check | --digests] [--keep] VMMAKER_IMAGE [REPO_ROOT]
#
# Translates LargeIntegersPlugin from the .st of REPO_ROOT (default: the
# repository this script is in) with Pharo Slang, in a temporary copy of
# VMMAKER_IMAGE, converts the output to the conventions of the checked-in C
# (convert.py), and writes REPO_ROOT/plugins/LargeIntegers/src/common/
# LargeIntegers.c, or with --check only compares it.
#
#   VMMAKER_IMAGE  the image cmake/vmmaker.cmake builds, e.g.
#                  <build>/build/vmmaker/image/VMMaker.image; its .changes
#                  and the .sources next to it are copied too, the image is
#                  never written
#   PHARO_VM       the VM that runs it: the pharo executable, or a
#                  directory holding one; default: the VM that cmake
#                  downloads next to the image (<build>/build/vmmaker/vm)
#   PYTHON         default python3
#   TMPDIR         where the temporary directory goes (default /tmp)
#   --check        do not write: exit 1 when the checked-in C differs from
#                  the conversion, with the first 100 lines of the diff
#   --digests      do not write: print the digests of the C that convert.py
#                  keeps from the checked-in file (VERBATIM, VARIABLES)
#   --keep         keep the temporary directory (generator log, raw Slang
#                  output regen/LargeIntegers.c, converted out.c), without
#                  the copy of the image
#
# Exits 0 when the C agrees (--check) or was written, 1 on drift (--check),
# or when C that convert.py keeps from the checked-in file no longer matches
# its pinned digests, 2 on a usage or environment error, 3 when the
# translation or the conversion fails.

set -u

usage() {
    echo "usage: sync-c.sh [--check | --digests] [--keep] VMMAKER_IMAGE [REPO_ROOT]" >&2
    exit 2
}
fail() {
    echo "sync-c: $*" >&2
    exit 2
}

check=0 keep=0 digests=0
while test $# -gt 0; do
    case $1 in
    --check) check=1 ;;
    --digests) digests=1 ;;
    --keep) keep=1 ;;
    -h|--help) sed -n '2,33p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "sync-c: unknown option $1" >&2; usage ;;
    *) break ;;
    esac
    shift
done
test $# -ge 1 && test $# -le 2 || usage
test $check = 1 && test $digests = 1 && usage

tools=$(cd "$(dirname "$0")" && pwd)
image=$1
case $image in
*.image) ;;
*) fail "$image is not a .image file" ;;
esac
test -f "$image" || fail "no VMMaker image at $image (build it with the cmake target vmmaker, see cmake/vmmaker.cmake)"
imagedir=$(cd "$(dirname "$image")" && pwd)
imagebase=$(basename "$image" .image)
if test $# = 2; then
    repo=$(cd "$2" 2>/dev/null && pwd) || fail "no directory $2"
else
    repo=$(cd "$tools/../../.." && pwd)
fi
st=$repo/smalltalksrc
target=$repo/plugins/LargeIntegers/src/common/LargeIntegers.c
test -f "$st/VMMaker/LargeIntegersPlugin.class.st" || fail "$repo has no smalltalksrc/VMMaker/LargeIntegersPlugin.class.st"
test -f "$target" || fail "no $target"

vm=${PHARO_VM:-}
if test -z "$vm"; then
    for c in "$imagedir/../vm/pharo" "$imagedir/../vm/Contents/MacOS/Pharo"; do
        if test -x "$c"; then vm=$c; break; fi
    done
    test -n "$vm" || fail "set PHARO_VM to a Pharo VM (the pharo executable, or its directory): none next to the image in $imagedir/../vm"
fi
test -d "$vm" && vm=$vm/pharo
test -x "$vm" && test -f "$vm" || fail "PHARO_VM: no executable $vm"
vmdir=$(cd "$(dirname "$vm")" && pwd)
vm=$vmdir/$(basename "$vm")
python=${PYTHON:-python3}
"$python" -c 'import sys; sys.exit(sys.version_info < (3, 6))' 2>/dev/null || fail "$python is not a Python 3.6 or later"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/lisync.XXXXXX") || fail "cannot create a temporary directory"
if test $keep = 1; then
    # the image copy is ~130 MB: keep only the log and the C
    trap 'rm -rf "$tmp/$imagebase.image" "$tmp/$imagebase.changes" "$tmp"/*.sources "$tmp/pharo-local"
          echo "sync-c: kept $tmp" >&2' EXIT
else
    trap 'rm -rf "$tmp"' EXIT
fi
trap 'exit 130' INT TERM

cp "$image" "$tmp/$imagebase.image" || fail "cannot copy $image"
test -f "$imagedir/$imagebase.changes" && cp "$imagedir/$imagebase.changes" "$tmp/"
for s in "$imagedir"/*.sources; do
    test -f "$s" && cp "$s" "$tmp/"
done

# the timeout guards against an image that waits for input
tmo=
command -v timeout >/dev/null 2>&1 && tmo="timeout --kill-after=10 900"
echo "sync-c: translating LargeIntegersPlugin from $st" >&2
(cd "$tmp" &&
    LISYNC_SRC=$st LISYNC_OUT=$tmp/gen \
    LD_LIBRARY_PATH=$vmdir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH} \
    $tmo "$vm" --headless "$tmp/$imagebase.image" --no-default-preferences \
        st --quit "$tools/regen.st" > "$tmp/regen.log" 2>&1 < /dev/null)
status=$?
regen=$tmp/gen/plugins/src/LargeIntegers/LargeIntegers.c
if test $status != 0 || ! test -f "$regen"; then
    grep -A 3 'LISYNC-ERROR' "$tmp/regen.log" >&2 || tail -n 40 "$tmp/regen.log" >&2
    echo "sync-c: the translation failed (VM exit status $status); --keep keeps its log" >&2
    exit 3
fi
grep 'LISYNC-WARNING' "$tmp/regen.log" >&2
mkdir -p "$tmp/regen" && cp "$regen" "$tmp/regen/LargeIntegers.c"

if test $digests = 1; then
    "$python" -I "$tools/convert.py" --digests "$tmp/regen/LargeIntegers.c" "$target" || exit 3
    exit 0
fi
"$python" -I "$tools/convert.py" "$tmp/regen/LargeIntegers.c" "$target" "$tmp/out.c"
status=$?
case $status in
0) ;;
1) exit 1 ;;
*) exit 3 ;;
esac

if cmp -s "$tmp/out.c" "$target"; then
    echo "sync-c: AGREE: $target is the conversion of the class"
    exit 0
fi
if test $check = 1; then
    diff -u "$target" "$tmp/out.c" | head -n 100
    echo "sync-c: DRIFT: $target differs from the conversion of the class (run without --check to write it)"
    exit 1
fi
cp "$tmp/out.c" "$target" || fail "cannot write $target"
echo "sync-c: wrote $target"
exit 0
