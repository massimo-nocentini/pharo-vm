#!/bin/sh
# The security watch of the cairo stack (decision U9, docs/CairoStack.md,
# "Security bumps"), run by hand (it was the weekly GitHub workflow
# cairo-stack-watch.yml):
#
#   watch.sh [--report <file>] [--no-msys2]
#
#   1. check-latest.sh compares each pin of cmake/cairo-stack/pins.txt with
#      its upstream's latest release, and the msys2 section with MSYS2's
#      repositories (--no-msys2 skips them): it fails when one is BEHIND,
#      GONE or UNKNOWN.  A release that fixes a CVE reachable from the images
#      is bumped within 14 days (docs/CairoStack.md);
#   2. check-pins.sh fails when a pinned file no longer downloads (MSYS2
#      removes superseded packages after a while: mirror them), when a
#      native-ahead exception expired, or when the native and wasm pins
#      drifted apart.
#
# Both always run; the exit status is 1 when either failed.  --report also
# writes check-latest.sh's output to <file>.  Needs curl, and zstd and tar
# for MSYS2's databases.  GITHUB_TOKEN, when set, lifts the rate limit of the
# GitHub API (60 requests an hour without it), e.g.
#
#   GITHUB_TOKEN=$(gh auth token) sh scripts/cairo-stack/watch.sh
#
# Runs from any directory: the paths are those of this checkout.

set -u

report=
latest_args=
while [ $# -gt 0 ]; do
  case $1 in
    --report) [ $# -ge 2 ] || { echo "watch.sh: --report needs a file" >&2; exit 2; }
              report=$2; shift 2 ;;
    --no-msys2) latest_args=--no-msys2; shift ;;
    -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "watch.sh: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
done

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root" || exit 2

for tool in curl tar; do
  command -v "$tool" > /dev/null || { echo "watch.sh: $tool is missing" >&2; exit 2; }
done
if [ -z "$latest_args" ] && ! command -v zstd > /dev/null; then
  echo "watch.sh: zstd is missing (it reads MSYS2's databases; or pass --no-msys2)" >&2
  exit 2
fi
[ -n "${GITHUB_TOKEN:-}" ] || echo "watch.sh: GITHUB_TOKEN is not set: the GitHub API allows 60 requests an hour" >&2

tmp=$(mktemp) || exit 2
trap 'rm -f "$tmp"' EXIT INT TERM

echo "== The pins of cmake/cairo-stack/pins.txt against upstream (check-latest.sh)"
latest=0
# shellcheck disable=SC2086  # latest_args is empty or one word
sh "$here/check-latest.sh" $latest_args > "$tmp" 2>&1 || latest=$?
cat "$tmp"
[ -z "$report" ] || cp "$tmp" "$report"

echo
echo "== Every pinned file still downloads (check-pins.sh)"
pins=0
sh "$here/check-pins.sh" || pins=$?

echo
if [ "$latest" -eq 0 ] && [ "$pins" -eq 0 ]; then
  echo "watch.sh: ok"
  exit 0
fi
echo "watch.sh: FAILED (check-latest.sh exit $latest, check-pins.sh exit $pins)"
exit 1
