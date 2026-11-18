#!/bin/sh
# shellcheck disable=SC2013  # one word per line
# The security watch of the cairo stack (decision U9, docs/CairoStack.md,
# "Security bumps"): compares cmake/cairo-stack/pins.txt with the latest
# release of each upstream, and the msys2 section with MSYS2's repositories.
#
#   check-latest.sh [--pins <file>] [--no-msys2]
#
# Sources: the GitHub releases API (zlib, expat), the GitLab API (fontconfig),
# the cairographics.org listing (cairo and pixman, stable = even minor), the
# savannah listing (FreeType), the SourceForge listing (libpng), and
# https://repo.msys2.org/mingw/<env>/<env>.db.  GITHUB_TOKEN, when set, is
# sent to the GitHub API (CI).
#
# Prints "current", "accepted", "BEHIND" or "UNKNOWN" per library ("accepted": an
# accepted-behind line of pins.txt covers exactly that newer release, until its date).  For MSYS2: "BEHIND" when
# the upstream version of a package with a DLL in the bundle (kind root or
# lib) changed, "behind" (a warning) for a rebuild of the same version or a
# runtime package, "GONE" when the package is no longer in the repository.
# (Whether the pinned files still download is check-pins.sh's probe.)  Exits 1 when anything is BEHIND,
# GONE or UNKNOWN, so the weekly run is red.  POSIX sh; needs curl, and zstd
# and tar for MSYS2.

set -u

here=$(cd "$(dirname "$0")" && pwd)
pins=$here/../../cmake/cairo-stack/pins.txt
msys2=yes
while [ $# -gt 0 ]; do
    case $1 in
        --pins) pins=$2; shift 2 ;;
        --no-msys2) msys2=no; shift ;;
        -h|--help) sed -n '2,/^# and tar for MSYS2/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "check-latest.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -f "$pins" ] || { echo "check-latest.sh: no $pins" >&2; exit 2; }

status=0
get() { curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --connect-timeout 20 --max-time 90 "$@"; }
github() {
    if [ -n "${GITHUB_TOKEN:-}" ]; then get -H "Authorization: Bearer $GITHUB_TOKEN" "https://api.github.com/repos/$1/releases/latest"
    else get "https://api.github.com/repos/$1/releases/latest"; fi
}
# newest: the highest dotted version of stdin
newest() { grep -E '^[0-9]+(\.[0-9]+)+$' | sort -u -t. -k1,1n -k2,2n -k3,3n -k4,4n | tail -n 1; }
# even_minor: keeps the stable series (cairo, pixman: odd minor = development)
even_minor() { awk -F. '$2 % 2 == 0'; }

latest() {
    case $1 in
        zlib) github madler/zlib | sed -n 's/.*"tag_name": *"v\{0,1\}\([0-9.]*\)".*/\1/p' | newest ;;
        expat) github libexpat/libexpat | sed -n 's/.*"tag_name": *"R_\([0-9_]*\)".*/\1/p' | tr _ . | newest ;;
        fontconfig) get 'https://gitlab.freedesktop.org/api/v4/projects/890/releases?per_page=20' \
                        | grep -o '"tag_name":"[0-9.]*"' | sed 's/.*:"\(.*\)"/\1/' \
                        | awk -F. '$3 < 90' | newest ;;   # x.y.9x are development snapshots
        cairo) get https://www.cairographics.org/releases/ | grep -o 'cairo-[0-9]*\.[0-9]*\.[0-9]*\.tar\.xz' \
                        | sed 's/cairo-\(.*\)\.tar\.xz/\1/' | even_minor | newest ;;
        pixman) get https://www.cairographics.org/releases/ | grep -o 'pixman-[0-9]*\.[0-9]*\.[0-9]*\.tar\.gz' \
                        | sed 's/pixman-\(.*\)\.tar\.gz/\1/' | even_minor | newest ;;
        freetype) get https://download.savannah.gnu.org/releases/freetype/ | grep -o 'freetype-[0-9]*\.[0-9]*\.[0-9]*\.tar\.xz' \
                        | sed 's/freetype-\(.*\)\.tar\.xz/\1/' | newest ;;
        libpng) get 'https://sourceforge.net/projects/libpng/rss?path=/libpng16' | grep -o '/libpng16/[0-9.]*/' \
                        | sed 's#/libpng16/\(.*\)/#\1#' | newest ;;
        *) echo "" ;;
    esac
}

echo "Source pins (Linux, macOS, wasm):"
for line in $(awk '/^#/ || NF < 7 || $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next } { print $1 "=" $2 }' "$pins"); do
    name=${line%%=*} pinned=${line#*=}
    l=$(latest "$name" 2>/dev/null)
    if [ -z "$l" ]; then
        echo "  UNKNOWN $name $pinned (no answer from upstream)"; status=1
    elif [ "$l" = "$pinned" ]; then
        echo "  current $name $pinned"
    elif [ "$(printf '%s\n%s\n' "$pinned" "$l" | newest)" = "$pinned" ]; then
        echo "  current $name $pinned (upstream's latest stable: $l)"
    else
        # an accepted-behind line: the user keeps this pin behind exactly that
        # release until a date (decision U4); any other release, or the date
        # passed, makes it BEHIND again
        acc=$(awk -v n="$name" '$1 == "accepted-behind" && $2 == n { print $3 " " $4; exit }' "$pins")
        if [ -n "$acc" ] && [ "${acc% *}" = "$l" ] && awk -v a="$(date -u +%Y-%m-%d)" -v b="${acc#* }" 'BEGIN { exit !(a < b) }'; then
            echo "  accepted $name $pinned, latest $l (accepted-behind until ${acc#* })"
        elif [ -n "$acc" ] && [ "${acc% *}" = "$l" ]; then
            echo "  BEHIND  $name $pinned, latest $l (accepted-behind expired on ${acc#* })"; status=1
        else
            echo "  BEHIND  $name $pinned, latest $l"; status=1
        fi
    fi
done

if [ "$msys2" = yes ]; then
    command -v zstd >/dev/null 2>&1 || { echo "check-latest.sh: MSYS2 needs zstd (or --no-msys2)" >&2; exit 2; }
    t=$(mktemp -d "${TMPDIR:-/tmp}/cairo-latest.XXXXXX")
    for env in $(awk '$1 == "msys2" { print $2 }' "$pins" | sort -u); do
        echo "MSYS2 $env:"
        if ! get -o "$t/$env.db" "https://repo.msys2.org/mingw/$env/$env.db"; then
            echo "  UNKNOWN $env.db not downloadable"; status=1; continue
        fi
        mkdir -p "$t/$env"
        zstd -dc "$t/$env.db" | tar -xf - -C "$t/$env"
        for d in "$t/$env"/*/; do
            awk '/^%NAME%$/ { getline; n = $0 } /^%VERSION%$/ { getline; v = $0 } /^%FILENAME%$/ { getline; f = $0 } END { print n, v, f }' "$d/desc"
        done > "$t/$env.txt"
        awk -v e="$env" '$1 == "msys2" && $2 == e { print $3, $4, $5 }' "$pins" | while read -r kind name version; do
            cur=$(awk -v n="$name" '$1 == n { print $2 }' "$t/$env.txt")
            if [ -z "$cur" ]; then
                echo "  GONE    $name $version (no longer in $env)"; echo 1 >> "$t/fail"
            elif [ "$cur" = "$version" ]; then
                echo "  current $name $version"
            elif [ "${cur%-*}" != "${version%-*}" ] && [ "$kind" != runtime ]; then
                echo "  BEHIND  $name $version, repository $cur"; echo 1 >> "$t/fail"
            else
                echo "  behind  $name $version, repository $cur (a rebuild, or the toolchain's runtime: warning)"
            fi
        done
    done
    [ ! -f "$t/fail" ] || status=1
    rm -rf "$t"
fi

if [ "$status" != 0 ]; then
    echo "check-latest.sh: some pins are behind (docs/CairoStack.md, \"Security bumps\")"
fi
exit "$status"
