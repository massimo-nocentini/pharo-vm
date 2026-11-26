#!/usr/bin/env bash
# run-st.sh [--baseline] [--keep] [--vm DIR] [--eval] <script.st | 'expr'> [timeoutSecs]
#   Runs a Smalltalk script on a FRESH copy of the Pharo 12 image, headless.
#   default VM : build-backport/build/vm   (the VM with our changes)
#   --baseline : $SLANG_TOOLS_WORK/baseline-vm   (snapshot of the unmodified C build)
#   --vm DIR   : any other VM directory (must contain ./pharo)
#   --eval     : the argument is a Smalltalk expression (uses `eval`, which
#                prints the expression's printString on stdout), not a file
#   --keep     : keep the temp run dir (path printed) for inspection
#   timeoutSecs: default 120; on timeout the VM is killed (status 124/137).
# Output: the VM's stdout, then stderr, then one final status line:
#   [run-st] vm=<new|baseline> exit=<n> status=<OK|ERROR|SEGFAULT|CRASH(sig)|TIMEOUT> time=<s>
# Exit status of run-st.sh = the VM's exit status.
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
IMAGE=Pharo12.0-SNAPSHOT-64bit-7d5f14cb47
SOURCES=Pharo12.0-64bit-7d5f14c.sources

vm="$NEWVM"; label=new; keep=0; ev=0; args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --baseline) vm="$BASEVM"; label=baseline ;;
    --vm) shift; vm="$(realpath "$1")"; label="$vm" ;;
    --keep) keep=1 ;;
    --eval) ev=1 ;;
    -h|--help) sed -n '2,14p' "$0"; exit 0 ;;
    *) args+=("$1") ;;
  esac
  shift
done
[ ${#args[@]} -ge 1 ] || { echo "usage: run-st.sh [--baseline] [--keep] [--eval] <script.st|expr> [timeoutSecs]" >&2; exit 2; }
what="${args[0]}"; tmo="${args[1]:-120}"
if [ $ev = 0 ]; then
  [ -f "$what" ] || { echo "run-st: no such script: $what" >&2; exit 2; }
  what="$(realpath "$what")"
fi

mkdir -p "$WORK/runs"
D="$(mktemp -d "$WORK/runs/run.XXXXXX")"
trap '[ $keep = 1 ] || rm -rf "$D"' EXIT   # also cleans up when killed by SIGPIPE/INT
cp "$IMGS/$IMAGE.image" "$IMGS/$IMAGE.changes" "$IMGS/$SOURCES" "$D/"
if [ $ev = 0 ]; then cp "$what" "$D/script.st"; fi

start=$(date +%s.%N)
# LD_LIBRARY_PATH is REQUIRED: the pharo binary's RUNPATH is ".:<build-backport/build/vm>",
# so without it the baseline would silently load the NEW libPharoVMCore.so/plugins,
# and libgit2 would fail to find libssl/libssh2 (startup error -> exit 1).
# timeout --kill-after: never hang, even if the VM ignores SIGTERM.
if [ $ev = 0 ]; then
  ( cd "$D" && LD_LIBRARY_PATH="$vm" timeout --kill-after=5 "$tmo" \
      "$vm/pharo" --headless "$D/$IMAGE.image" st --quit "$D/script.st" \
      >"$D/stdout.txt" 2>"$D/stderr.txt" </dev/null )
else
  ( cd "$D" && LD_LIBRARY_PATH="$vm" timeout --kill-after=5 "$tmo" \
      "$vm/pharo" --headless "$D/$IMAGE.image" eval "$what" \
      >"$D/stdout.txt" 2>"$D/stderr.txt" </dev/null )
fi
rc=$?
end=$(date +%s.%N)

cat "$D/stdout.txt"
[ -s "$D/stdout.txt" ] && [ -n "$(tail -c1 "$D/stdout.txt")" ] && echo
if [ -s "$D/stderr.txt" ]; then echo "----- stderr -----"; cat "$D/stderr.txt"; fi

status=OK
if [ $rc = 124 ] || [ $rc = 137 ]; then status=TIMEOUT; fi
# The VM traps SIGSEGV itself, prints "Segmentation fault <date>" + a C/Smalltalk
# stack dump on stdout, writes crash.dmp, then aborts.
if grep -qE '^(Segmentation fault|Bus error|Illegal instruction|Floating point exception|Abort)' "$D/stdout.txt" "$D/stderr.txt" 2>/dev/null \
   || [ -f "$D/crash.dmp" ]; then
  status=SEGFAULT
  grep -hE '^(Segmentation fault|Bus error|Illegal instruction|Floating point exception|Abort)' "$D/stdout.txt" "$D/stderr.txt" | head -1 | sed 's/^/[run-st] crash: /'
elif [ $rc -gt 128 ] && [ $rc != 137 ]; then
  status="CRASH(signal $((rc-128)))"
elif [ $status = OK ] && [ $rc != 0 ]; then
  status=ERROR
fi
if [ -f "$D/PharoDebug.log" ] && [ $status != OK ]; then
  echo "----- PharoDebug.log (head) -----"; head -40 "$D/PharoDebug.log"
fi
[ $keep = 1 ] && echo "[run-st] kept run dir: $D"
printf '[run-st] vm=%s exit=%d status=%s time=%.1fs\n' "$label" "$rc" "$status" "$(echo "$end - $start" | bc)"
exit $rc
