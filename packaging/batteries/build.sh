#!/bin/sh
# build.sh - build the batteries image: a stock Pharo 15 image with the
# projects of load.st, checked by smoke.st, zipped (docs/Batteries.md)
#
#   sh packaging/batteries/build.sh <vm-dir> <Pharo15 image zip> <out.zip>
#
# <vm-dir> holds the VM that loads the projects: pharo (Linux; its
# directory and lib/ go first on LD_LIBRARY_PATH, as for a VM of a build
# tree), Pharo.app (macOS; or <vm-dir> is the .app itself) or
# PharoConsole.exe (Windows, from an MSYS2 or Git Bash shell).  The image
# zip is a Pharo 15 zip of files.pharo.org.  In a work directory
# (BATTERIES_WORK, or a new one in TMPDIR) the script copies the image as
# <name>.image and <name>.changes, with its .sources and pharo.version, then
#
#   1. runs load.st on it (st --save --quit): the projects are loaded and the
#      image saved, or the script stops with exit status 1;
#   2. starts the image saved and runs smoke.st on it (st --no-source: the
#      .changes are kept as they are);
#   3. zips <name>.image, <name>.changes, the .sources and pharo.version, at
#      the top of the zip as in the zips of files.pharo.org, into <out.zip>,
#      and prints the sizes of the files, raw and gzipped (the site of the
#      WebAssembly VM serves them gzipped).
#
# Environment: BATTERIES_NAME (the name of the image, Pharo15-batteries by
# default), BATTERIES_WORK, BATTERIES_TIMEOUT (seconds for each run, 1800 by
# default), BATTERIES_SMOKE (basic, the default, or native: smoke.st), and
# the BATTERIES_<KEY> and BATTERIES_<KEY>_GROUPS of load.st, which this
# script passes on: for a local checkout, tonel://<checkout>/src, whose git
# commit it prints.
#
# The status is 0 when load.st printed 'Batteries loaded ... ok', smoke.st
# 'Batteries smoke ... ok' and the zip was written, else 1 (2 for a usage
# error).
#
# Another image is built the same way from the load.st and smoke.st of
# another directory, BATTERIES_SCRIPTS (this one by default), whose lines
# begin with BATTERIES_MARK instead of Batteries, and named after
# BATTERIES_DEFAULT_NAME when BATTERIES_NAME is not set: so
# packaging/bloc/build.sh builds the Bloc image (docs/Bloc.md).  load.st
# finds its directory in BATTERIES_HOME.

set -u

if test $# -ne 3; then
    echo "usage: $0 <vm-dir> <Pharo15 image zip> <out.zip>" >&2
    exit 2
fi
VM_DIR=$(cd "$1" && pwd) || exit 2
IMAGE_ZIP=$2
OUT=$3
HERE=$(cd "${BATTERIES_SCRIPTS:-$(dirname "$0")}" && pwd) || exit 2
MARK=${BATTERIES_MARK:-Batteries}
NAME=${BATTERIES_NAME:-${BATTERIES_DEFAULT_NAME:-Pharo15-batteries}}
test -f "$IMAGE_ZIP" || { echo "FAIL: no $IMAGE_ZIP" >&2; exit 1; }
IMAGE_ZIP=$(cd "$(dirname "$IMAGE_ZIP")" && pwd)/$(basename "$IMAGE_ZIP")
case $OUT in
/*) ;;
*) OUT=$(pwd)/$OUT ;;
esac

# the VM executable
lib_path=no
if test -x "$VM_DIR/pharo" && ! test -d "$VM_DIR/pharo"; then
    vm=$VM_DIR/pharo
    lib_path=yes
elif test -x "$VM_DIR/Pharo.app/Contents/MacOS/Pharo"; then
    vm=$VM_DIR/Pharo.app/Contents/MacOS/Pharo
elif test -x "$VM_DIR/Contents/MacOS/Pharo"; then
    vm=$VM_DIR/Contents/MacOS/Pharo
elif test -f "$VM_DIR/PharoConsole.exe"; then
    vm=$VM_DIR/PharoConsole.exe
else
    echo "FAIL: no pharo, Pharo.app/Contents/MacOS/Pharo, Contents/MacOS/Pharo or PharoConsole.exe in $VM_DIR" >&2
    exit 1
fi
if test $lib_path = yes; then
    LD_LIBRARY_PATH="$VM_DIR:$VM_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export LD_LIBRARY_PATH
fi

# paths as the VM sees them (Windows: C:/... from an MSYS2 or Git Bash shell)
native() {
    if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else echo "$1"; fi
}
limit=
if command -v timeout >/dev/null 2>&1; then limit="timeout ${BATTERIES_TIMEOUT:-1800}"
elif command -v gtimeout >/dev/null 2>&1; then limit="gtimeout ${BATTERIES_TIMEOUT:-1800}"
fi

# the image, under its new name
WORK=${BATTERIES_WORK:-$(mktemp -d "${TMPDIR:-/tmp}/batteries.XXXXXX")} || exit 1
mkdir -p "$WORK" || exit 1
WORK=$(cd "$WORK" && pwd)
rm -rf "$WORK/stock" "$WORK/image"
mkdir -p "$WORK/stock" "$WORK/image" || exit 1
unzip -q "$IMAGE_ZIP" -d "$WORK/stock" || exit 1
set -- "$WORK"/stock/*.image
if test $# -ne 1 || ! test -f "$1"; then
    echo "FAIL: expected one .image at the top of $IMAGE_ZIP" >&2
    exit 1
fi
stock=${1%.image}
test -f "$stock.changes" || { echo "FAIL: no $(basename "$stock").changes in $IMAGE_ZIP" >&2; exit 1; }
cp "$stock.image" "$WORK/image/$NAME.image" &&
    cp "$stock.changes" "$WORK/image/$NAME.changes" &&
    cp "$WORK"/stock/*.sources "$WORK/image/" || exit 1
test -f "$WORK/stock/pharo.version" && cp "$WORK/stock/pharo.version" "$WORK/image/"
rm -rf "$WORK/stock"
echo "image: $(basename "$IMAGE_ZIP") as $WORK/image/$NAME.image"
echo "vm: $vm ($("$vm" --version 2>/dev/null | head -n 1))"

# the local checkouts given to load.st
for key in ROASSAL VMPLUGINSPRIMITIVES SIMDJSON SIMDUTF SIMDCSV TIMSORT LIBPANGOCAIRO LIBPOPPLER CMARK \
    CONTAINERSESSENTIALS CONTAINERSCATEGORY DELIMCC MATERIALICONS; do
    repository=
    eval "repository=\${BATTERIES_$key:-}"
    case $repository in
    tonel://*|filetree://*)
        dir=${repository#*://}
        commit=$(git -C "$dir" describe --always --dirty --abbrev=12 2>/dev/null || echo 'not a git checkout')
        echo "BATTERIES_$key: $repository ($commit)"
        ;;
    ?*)
        echo "BATTERIES_$key: $repository"
        ;;
    esac
done

# 1. the load, saved by st --save --quit
BATTERIES_HOME=$(native "$HERE")
export BATTERIES_HOME
(cd "$WORK/image" && $limit "$vm" --headless "$NAME.image" --no-default-preferences \
    st --save --quit "$(native "$HERE/load.st")") > "$WORK/load.log" 2>&1
status=$?
tr -d '\r' < "$WORK/load.log"
if test $status -ne 0 || ! tr -d '\r' < "$WORK/load.log" | grep -q "^$MARK loaded .* ok\$"; then
    echo "FAIL: load.st, exit status $status ($WORK/load.log)" >&2
    exit 1
fi

# 2. the smoke, on the image saved
(cd "$WORK/image" && $limit "$vm" --headless "$NAME.image" --no-default-preferences \
    st --no-source "$(native "$HERE/smoke.st")") > "$WORK/smoke.log" 2>&1
status=$?
tr -d '\r' < "$WORK/smoke.log"
if test $status -ne 0 || ! tr -d '\r' < "$WORK/smoke.log" | grep -q "^$MARK smoke .* ok\$"; then
    echo "FAIL: smoke.st, exit status $status ($WORK/smoke.log)" >&2
    exit 1
fi

# 3. the zip
rm -f "$OUT"
mkdir -p "$(dirname "$OUT")" || exit 1
# (zip, else the zipfile module of python3, which deflates too)
set -- "$NAME.image" "$NAME.changes"
for f in "$WORK"/image/*.sources "$WORK/image/pharo.version"; do
    if test -f "$f"; then set -- "$@" "$(basename "$f")"; fi
done
if command -v zip >/dev/null 2>&1; then
    (cd "$WORK/image" && zip -q -X "$OUT" "$@") || exit 1
else
    (cd "$WORK/image" && python3 -m zipfile -c "$OUT" "$@") || exit 1
fi
for f in "$WORK/image/$NAME.image" "$WORK/image/$NAME.changes" "$WORK"/image/*.sources; do
    echo "$(basename "$f"): $(wc -c < "$f") bytes, gzipped $(gzip -9 -c "$f" | wc -c)"
done
echo "PASS: $OUT ($(wc -c < "$OUT") bytes)"
