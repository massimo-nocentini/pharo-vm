#!/usr/bin/env bash
# run-diff.sh [--eval] <script.st | 'expr'> [timeoutSecs]
#   Runs the same script on the BASELINE VM and on the NEW VM (each on its own
#   fresh image copy, via run-st.sh) and prints both status lines plus a unified
#   diff of their outputs (stdout+stderr; crash dumps included; VM/run dir
#   paths, the vm= label and timings are normalized; addresses are not).
#   Exit status: 0 if outputs and exit codes are identical, 1 otherwise.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
mkdir -p "$WORK/runs"
W="$(mktemp -d "$WORK/runs/rundiff.XXXXXX")"; trap 'rm -rf "$W"' EXIT
"$TOOLS/run-st.sh" --baseline "$@" > "$W/base" 2>&1
"$TOOLS/run-st.sh" "$@" > "$W/new" 2>&1
grep '^\[run-st\] vm=' "$W/base" "$W/new" | sed "s|$W/||"
# drop timing from the status line so it does not show up as a difference
# and normalize the VM label and the VM directory paths (crash dumps print them)
sed -E -i -e 's/ time=[0-9.]+s$//' -e 's/^\[run-st\] vm=(baseline|new) /[run-st] vm=X /' \
  -e "s|$BASEVM|<VMDIR>|g" \
  -e "s|$NEWVM|<VMDIR>|g" \
  -e "s|$WORK/runs/run\.[A-Za-z0-9]+|<RUNDIR>|g" "$W/base" "$W/new"
if diff -u --label baseline --label new "$W/base" "$W/new"; then
  echo "[run-diff] IDENTICAL output and exit status"; exit 0
else
  echo "[run-diff] DIFFERENT"; exit 1
fi
