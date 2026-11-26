#!/usr/bin/env bash
# regen.sh [--src SMALLTALKSRC_DIR | --rev GITREV] [--keep] [--strict] <PluginClassName> <outdir>
#   Regenerates ONE plugin's C with Slang, from the WORKING TREE smalltalksrc
#   (default), from another smalltalksrc dir (--src), or from a git revision
#   of this repo (--rev HEAD, --rev pharo-12, ...; exported with git archive,
#   the working tree is not touched).
#   Uses a throw-away COPY of build-wasm/vmmaker/image/VMMaker.image, run by the
#   baseline VM.  Writes <outdir>/<moduleName>.c (plus any other file Slang wrote
#   for that plugin, e.g. a .h) and prints its path on the last line:
#       REGEN-OUTPUT: <outdir>/<moduleName>.c
#   Exit status 0 on success; non-zero on load/translation error (messages
#   start with REGEN-LOAD-ERROR / REGEN-TRANSLATE-ERROR).
#   --strict: abort on Slang's 'Undefined local or argument X type declaration'
#   (by default the declaration is dropped, logged as REGEN-LENIENT; needed
#   for BitBltSimulation, whose alphaBlendScaled:with: types an inst var).
#   <PluginClassName> may be a comma list (A,B,C): the image is loaded once and
#   each module goes to <outdir>/<moduleName>/<moduleName>.c.
#   Examples:  regen.sh LargeIntegersPlugin /tmp/x   -> /tmp/x/LargeIntegers.c
#              regen.sh BitBltSimulation /tmp/x      -> /tmp/x/BitBltPlugin.c
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
VM="$BASEVM"

src="$REPO/smalltalksrc"; rev=""; keep=0; lenient=true; args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --src) shift; src="$(realpath "$1")" ;;
    --rev) shift; rev="$1" ;;
    --keep) keep=1 ;;
    --strict) lenient=false ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) args+=("$1") ;;
  esac
  shift
done
[ ${#args[@]} = 2 ] || { echo "usage: regen.sh [--src DIR|--rev REV] [--keep] <PluginClassName> <outdir>" >&2; exit 2; }
plugin="${args[0]}"; outdir="${args[1]}"
mkdir -p "$outdir"; outdir="$(realpath "$outdir")"

mkdir -p "$WORK/runs"
D="$(mktemp -d "$WORK/runs/regen.XXXXXX")"
cleanup() { if [ $keep = 1 ]; then echo "[regen] kept $D" >&2; else rm -rf "$D"; fi; }
trap cleanup EXIT

if [ -n "$rev" ]; then
  mkdir -p "$D/tree"
  git -C "$REPO" archive "$rev" smalltalksrc | tar -x -C "$D/tree" || { echo "REGEN-ERROR: git archive $rev failed" >&2; exit 3; }
  src="$D/tree/smalltalksrc"
fi
[ -d "$src/VMMaker" ] || { echo "REGEN-ERROR: $src is not a smalltalksrc dir" >&2; exit 2; }

start=$(date +%s)
cp "$VMI/VMMaker.image" "$VMI/VMMaker.changes" "$VMI"/Pharo*.sources "$D/"
mkdir -p "$D/gen"; outs=()
plist="#($(echo "$plugin" | tr ',' ' '))"
sed -e "s|@SRC@|$src|g" -e "s|@PLUGIN@|$plist|g" -e "s|@OUT@|$D/gen|g" -e "s|@LENIENT@|$lenient|g" \
  "$TOOLS/regen-template.st" > "$D/regen.st"

( cd "$D" && LD_LIBRARY_PATH="$VM" timeout --kill-after=5 600 \
    "$VM/pharo" --headless "$D/VMMaker.image" --no-default-preferences st --quit "$D/regen.st" \
    >"$D/log.txt" 2>&1 </dev/null )
rc=$?
if [ $rc != 0 ]; then
  echo "[regen] FAILED (exit $rc); log tail:" >&2; tail -60 "$D/log.txt" >&2
  exit $rc
fi
# Show our own messages and anything that looks like a problem; hide the
# 'xxx localised to yyy' / 'inlined' chatter Slang prints.
grep -E 'REGEN|Error|error|Exception|not understood|DNU|failed' "$D/log.txt" | grep -vE '^REGEN(: |-LENIENT)' | head -60 >&2
grep -E '^REGEN: |^REGEN-LENIENT|external plugin' "$D/log.txt" >&2
n=0
for gdir in "$D"/gen/plugins/src/*/; do
  [ -d "$gdir" ] || continue
  module="$(basename "$gdir")"
  if [ "$(echo "$plugin" | tr ',' '\n' | wc -l)" -gt 1 ]; then dest="$outdir/$module"; else dest="$outdir"; fi
  mkdir -p "$dest"; cp -a "$gdir". "$dest/"
  echo "[regen] module $module: $(ls "$gdir" | tr '\n' ' ')" >&2
  outs+=("$dest/$module.c"); n=$((n+1))
done
[ $n -gt 0 ] || { echo "REGEN-ERROR: no output produced" >&2; tail -40 "$D/log.txt" >&2; exit 4; }
echo "[regen] $plugin  src=$( [ -n "$rev" ] && echo "git:$rev" || echo "$src")  $(( $(date +%s) - start ))s" >&2
for o in "${outs[@]}"; do echo "REGEN-OUTPUT: $o"; done
