#!/usr/bin/env bash
# install-generated.sh <Plugin> [regenerated.c]
#   <Plugin> = FilePlugin | NewFilePlugin | SurfacePlugin | FloatArrayPlugin
#   (module name == class name for these four build-time-generated plugins).
#   Without [regenerated.c], regenerates <Plugin> from the WORKING TREE
#   smalltalksrc first (regen.sh).  Then shows the diff and copies the file to
#   build-backport-gen/generated/64/plugins/src/<Plugin>/<Plugin>.c, which the
#   build-backport CMake target <Plugin> compiles.  Follow with:
#       build.sh <Plugin>
#   Note: build-backport-gen/generated used to be a symlink to the user's
#   build/generated; it is now a private copy (the script refuses to write
#   through a symlink).
set -u
. "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/env.sh"
p="${1:-}"
case "$p" in
  FilePlugin|NewFilePlugin|SurfacePlugin|FloatArrayPlugin) ;;
  *) echo "usage: install-generated.sh FilePlugin|NewFilePlugin|SurfacePlugin|FloatArrayPlugin [regenerated.c]" >&2; exit 2 ;;
esac
[ -L "$GEN" ] && { echo "REFUSING: $GEN is a symlink (would write into another tree)" >&2; exit 3; }
dest="$GEN/64/plugins/src/$p/$p.c"
[ -f "$dest" ] || { echo "no such file: $dest" >&2; exit 3; }

if [ $# -ge 2 ]; then
  new="$(realpath "$2")"
else
  mkdir -p "$WORK/runs"
  tmp="$(mktemp -d "$WORK/runs/install.XXXXXX")"; trap 'rm -rf "$tmp"' EXIT
  "$TOOLS/regen.sh" "$p" "$tmp" >/dev/null || { echo "regen failed" >&2; exit 1; }
  new="$tmp/$p.c"
fi
[ -f "$new" ] || { echo "no such file: $new" >&2; exit 2; }
if cmp -s "$new" "$dest"; then echo "[install-generated] $p: unchanged"; exit 0; fi
diff -u --label "installed/$p.c" --label "new/$p.c" "$dest" "$new"
# Under the build lock so a concurrent build never compiles a half-copied file.
flock "$WORK/build.lock" cp "$new" "$dest"
echo "[install-generated] $p installed -> $dest ; now run: $TOOLS/build.sh $p"
