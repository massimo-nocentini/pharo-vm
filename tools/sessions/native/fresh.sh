#!/bin/sh
# fresh.sh IMAGE DIR [LOADER.st...] - makes DIR a fresh copy of an image
# (dev.image, dev.changes and the .sources), then runs each loader on it with
# run.sh and saves it, the output of each in DIR/<loader>.log with a last line
# 'EXIT <status>'; stops at the first loader that fails.
#
#   IMAGE      a Pharo zip (Pharo15.0-SNAPSHOT...zip, as files.pharo.org
#              serves them) or a directory holding one .image and its files
#   LOADER.st  e.g. load-roassal.st then load-ce.st (ROASSAL_DIR, LAYOUTS_DIR,
#              CE_DIR), or load-tonel.st (TONEL_DIR, TONEL_PACKAGES)
#   PHARO_VM   the VM (run.sh)
# DIR is removed first.  The loaders' environment variables pass through.
HERE=$(cd "$(dirname "$0")" && pwd)
test $# -ge 2 || { sed -n '2,13p' "$0"; exit 2; }
src=$1; dir=$2; shift 2
case $src in /*) ;; *) src=$(pwd)/$src ;; esac
rm -rf "$dir" && mkdir -p "$dir" && cd "$dir" || exit 2
if test -d "$src"; then from=$src; else unzip -q "$src" -d unzipped || exit 2; from=$(dirname "$(ls unzipped/*.image unzipped/*/*.image 2>/dev/null | head -n 1)"); fi
image=$(ls "$from"/*.image | head -n 1)
test -f "$image" || { echo "fresh.sh: no image in $src" >&2; exit 2; }
cp "$image" dev.image && cp "${image%.image}.changes" dev.changes || exit 2
for s in "$from"/*.sources; do test -f "$s" && cp "$s" .; done
rm -rf unzipped
for loader in "$@"; do
  case $loader in /*) ;; */*) loader=$OLDPWD/$loader ;; *) test -f "$loader" || loader=$HERE/$loader ;; esac
  log=$(basename "$loader" .st).log
  SAVE=1 "$HERE/run.sh" dev.image "$loader" "${FRESH_TIMEOUT:-3000}" > "$log" 2>&1
  rc=$?; echo "EXIT $rc" >> "$log"
  grep -v '^MetacelloNotification\|^Fetched\|^Loading\|^Loaded\|^\.\.\.\|^Project:\|^Resolving' "$log" | tail -n 20
  test $rc = 0 || exit $rc
done
echo "fresh.sh: $dir/dev.image"
