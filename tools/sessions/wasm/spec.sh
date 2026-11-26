#!/usr/bin/env bash
# spec.sh SPEC... - runs browser specs of tests/wasm (world, webfetch.spec.mjs,
# ...: the name with or without .spec.mjs) on the site of WASM_BUILDDIR/web,
# one after the other, each with its log in LOG_DIR, and prints their
# summaries; 'spec.sh all' runs every spec of tests/wasm.
#
#   BROWSERS   chromium (default), or a list such as chromium,firefox
#   PW_ROOT    or PLAYWRIGHT_MODULE / PLAYWRIGHT_BROWSERS_PATH (env.sh)
#   WEB_DIR    the site (default: $WASM_BUILDDIR/web)
#   LOG_DIR    the logs (default: a new directory under $TMPDIR); TEST_DIR the
#              specs' own files (default: LOG_DIR)
#   WEB_FETCH_DIR  a checkout of web-fetch.st, for webfetch and websocket
# The status is the number of specs that failed.  Paths and toolchain: env.sh.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
case "${1:-}" in -h|--help|'') sed -n '2,14p' "$0"; exit 0 ;; esac
WEB_DIR="${WEB_DIR:-$WASM_BUILDDIR/web}"
LOG_DIR="${LOG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/specs.XXXXXX")}"
export BROWSERS="${BROWSERS:-chromium}" TEST_DIR="${TEST_DIR:-$LOG_DIR}"
specs=("$@")
if test "$1" = all; then
  specs=(); for f in "$WASM_SRC"/tests/wasm/*.spec.mjs; do specs+=("$(basename "$f")"); done
fi
failed=0
cd "$WASM_SRC" || exit 2
for s in "${specs[@]}"; do
  s="${s%.spec.mjs}"; log="$LOG_DIR/$s.log"; t0=$(date +%s)
  "$NODE" "tests/wasm/$s.spec.mjs" "$WEB_DIR" > "$log" 2>&1
  rc=$?; test $rc = 0 || failed=$((failed + 1))
  echo "=== $s: exit $rc, $(( $(date +%s) - t0 )) s, $log"
  grep -E '^not ok|^# [0-9]+ passed' "$log" | cut -c 1-300 | tail -n 8
done
exit $failed
