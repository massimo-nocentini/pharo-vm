#!/usr/bin/env bash
# regen-diff.sh [--base REV] [--raw] <PluginClassName> <c-file>
#   Shows only the meaningful differences between a plugin's Slang source and
#   its C.  <c-file> is the checked-in C (plugins/<P>/src/common/<P>.c) or the
#   build-generated one (build-backport-gen/generated/64/plugins/src/<P>/<P>.c);
#   relative paths are taken from the repo root.  --base defaults to HEAD.
#
#   [1] SLANG DELTA: regen(<base> smalltalksrc) -> regen(working-tree smalltalksrc).
#       Exact, no noise: what your .st edit changes in the generated C.
#   [2] C DELTA: what changed in <c-file> since <base>
#       (tracked file: git diff <base>; build-generated file: regen(<base>) -> file).
#   [3] CHECK:
#       build-generated plugin: diff <c-file> vs regen(working tree); must be
#         empty once install-generated.sh was run (Slang output is byte-identical).
#       checked-in plugin: the old Squeak-Slang C differs from this Slang's output
#         by thousands of lines (inlining, casts, names), so instead the +/- lines
#         of [1] and [2] are normalized (normalize-c.sh rules, all whitespace
#         removed) and compared: lines listed there are in one delta only.
#         Expect some cosmetic leftovers (cast/parenthesis style, inlined copies
#         of the same edit appearing several times in [1]); read them by eye.
#   --src DIR    : use DIR instead of the working-tree smalltalksrc (testing)
#   --c-base F   : treat <c-file> as checked-in and use file F as its base version
#                  instead of git (testing / files outside the repo)
#   --raw : additionally print the full normalized diff <c-file> vs regen(worktree).
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
base=HEAD; raw=0; srcopt=(); cbase=""; args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --base) shift; base="$1" ;;
    --raw) raw=1 ;;
    --src) shift; srcopt=(--src "$1") ;;
    --c-base) shift; cbase="$(realpath "$1")" ;;
    -h|--help) sed -n '2,28p' "$0"; exit 0 ;;
    *) args+=("$1") ;;
  esac
  shift
done
[ ${#args[@]} = 2 ] || { echo "usage: regen-diff.sh [--base REV] [--raw] <PluginClassName> <c-file>" >&2; exit 2; }
plugin="${args[0]}"; cfile="${args[1]}"
case "$cfile" in /*) ;; *) cfile="$REPO/$cfile" ;; esac
[ -f "$cfile" ] || { echo "no such file: $cfile" >&2; exit 2; }
sha="$(git -C "$REPO" rev-parse --short "$base")" || exit 2

W="$(mktemp -d "$WORK/runs/regendiff.XXXXXX")"
trap 'rm -rf "$W"' EXIT
CACHE="$WORK/runs/regen-cache/$sha/$plugin"
mkdir -p "$(dirname "$CACHE")"

# Base regen is cached per (commit, plugin); flock so concurrent callers don't race.
(
  flock 9
  if ! ls "$CACHE"/*.c >/dev/null 2>&1; then
    echo "[regen-diff] regenerating $plugin from $base ($sha) ..." >&2
    rm -rf "$CACHE.tmp"
    "$TOOLS/regen.sh" --rev "$base" "$plugin" "$CACHE.tmp" >/dev/null || { rm -rf "$CACHE.tmp"; exit 1; }
    mv "$CACHE.tmp" "$CACHE"
  fi
) 9>"$CACHE.lock" || { echo "[regen-diff] regen of $base failed" >&2; exit 1; }
echo "[regen-diff] regenerating $plugin from the working tree ..." >&2
out="$("$TOOLS/regen.sh" "${srcopt[@]}" "$plugin" "$W/wt")" || { echo "[regen-diff] regen of working tree failed" >&2; exit 1; }
wt="$(echo "$out" | sed -n 's/^REGEN-OUTPUT: //p' | head -1)"
hd="$CACHE/$(basename "$wt")"
rel="${cfile#$REPO/}"

echo "=================================================================="
echo "[1] SLANG DELTA  regen($base:smalltalksrc) -> regen(working tree)   [$plugin]"
echo "=================================================================="
diff -u --label "regen-$base/$(basename "$wt")" --label "regen-worktree/$(basename "$wt")" "$hd" "$wt" > "$W/d1"
if [ -s "$W/d1" ]; then cat "$W/d1"; else echo "(none: no .st change affects $plugin's C)"; fi

tracked=0
git -C "$REPO" ls-files --error-unmatch "$rel" >/dev/null 2>&1 && tracked=1
[ -n "$cbase" ] && tracked=2
echo "=================================================================="
if [ $tracked = 2 ]; then
  echo "[2] C DELTA  $cbase -> $rel"
  echo "=================================================================="
  diff -u "$cbase" "$cfile" > "$W/d2"
elif [ $tracked = 1 ]; then
  echo "[2] C DELTA  git diff $base -- $rel"
  echo "=================================================================="
  git -C "$REPO" diff "$base" -- "$rel" > "$W/d2"
else
  echo "[2] C DELTA  regen($base) -> $rel   (build-generated, untracked)"
  echo "=================================================================="
  diff -u --label "regen-$base" --label "$rel" "$hd" "$cfile" > "$W/d2"
fi
if [ -s "$W/d2" ]; then cat "$W/d2"; else echo "(none)"; fi

echo "=================================================================="
if [ $tracked = 0 ]; then
  echo "[3] CHECK  $rel  vs  regen(working tree)  (must be empty)"
  echo "=================================================================="
  if diff -u --label "$rel" --label "regen-worktree" "$cfile" "$wt"; then
    echo "OK: identical"
  else
    echo "MISMATCH: run install-generated.sh $(basename "$(dirname "$cfile")") after regen, or fix the .st"
  fi
else
  echo "[3] CHECK  normalized +/- lines present in only one of [1] and [2]"
  echo "=================================================================="
  norm() { # $1 = diff file, $2 = + or -
    grep -E "^[$2]" "$1" | grep -vE '^(\+\+\+|---) ' | cut -c2- > "$W/n.c"
    "$TOOLS/normalize-c.sh" "$W/n.c" | tr -d ' \t' | grep -v '^$' | sort -u
  }
  for s in + -; do
    norm "$W/d1" "$s" > "$W/a"; norm "$W/d2" "$s" > "$W/b"
    [ "$s" = + ] && what=added || what=removed
    comm -23 "$W/a" "$W/b" | sed "s/^/  only in SLANG delta ($what): /"
    comm -13 "$W/a" "$W/b" | sed "s/^/  only in C delta     ($what): /"
  done > "$W/d3"
  if [ -s "$W/d3" ]; then cat "$W/d3"; else echo "OK: the normalized line sets of [1] and [2] match"; fi
fi

if [ $raw = 1 ]; then
  echo "=================================================================="
  echo "[raw] normalized diff -w  $rel  vs  regen(working tree)"
  echo "=================================================================="
  diff -u -w <("$TOOLS/normalize-c.sh" "$cfile") <("$TOOLS/normalize-c.sh" "$wt")
fi
exit 0
