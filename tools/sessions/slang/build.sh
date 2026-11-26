#!/usr/bin/env bash
# build.sh [--fast] [--full-log] <cmake-target>...
#   Builds targets of the CMake build $SLANG_TOOLS_BUILD (env.sh) under a lock, so
#   that concurrent builds of several shells or agents are serialized.
#   --fast      : build <target>/fast (Makefiles generator): skips rebuilding the
#                 target's dependencies (e.g. PharoVMCore), so someone else's
#                 half-finished src/ edit cannot break your plugin build.
#                 Use only when PharoVMCore is already up to date for you.
#   --full-log  : print the whole build log instead of the filtered summary.
# Exit status = the build's exit status.  Full log is kept in $WORK/runs/build-logs.
# Paths and env vars: env.sh.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
LOCK="$WORK/build.lock"
LOGDIR="$WORK/runs/build-logs"
mkdir -p "$LOGDIR"

fast=0; full=0; targets=()
for a in "$@"; do
  case "$a" in
    --fast) fast=1 ;;
    --full-log) full=1 ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) targets+=("$a") ;;
  esac
done
[ ${#targets[@]} -gt 0 ] || { echo "usage: build.sh [--fast] [--full-log] <target>..." >&2; exit 2; }

log="$(mktemp "$LOGDIR/build.$(date +%H%M%S).XXXX.log")"
start=$(date +%s)
echo "[build.sh] waiting for lock ..." >&2
if [ $fast = 1 ]; then
  # /fast targets exist only in the Makefiles generator; run make directly.
  ft=(); for t in "${targets[@]}"; do ft+=("$t/fast"); done
  flock "$LOCK" make -C "$BUILD" -j "$JOBS" "${ft[@]}" >"$log" 2>&1
else
  flock "$LOCK" cmake --build "$BUILD" --target "${targets[@]}" -j "$JOBS" >"$log" 2>&1
fi
rc=$?
end=$(date +%s)

if [ $full = 1 ]; then
  cat "$log"
else
  echo "=== diagnostics (error/warning lines with 3 lines of context) ==="
  # Only translation units recompiled in this run emit diagnostics, i.e. the
  # files you (or another agent) changed since the last build.
  grep -E -A3 --group-separator=-- ': (error|fatal error|warning):|undefined reference|ld(\.lld)?: error' "$log" | head -200
  echo "=== failed make steps ==="
  grep -E 'make(\[[0-9]+\])?: \*\*\*' "$log" | head -10
  echo "=== tail ==="
  tail -n 12 "$log"
fi
echo "[build.sh] targets: ${targets[*]}  fast=$fast  exit=$rc  time=$((end-start))s  log=$log"
exit $rc
