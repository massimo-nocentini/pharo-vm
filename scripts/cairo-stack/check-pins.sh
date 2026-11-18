#!/bin/sh
# shellcheck disable=SC2015,SC2013,SC2018,SC2019  # ok/err never fail; names are ASCII
# Checks cmake/cairo-stack/pins.txt (docs/CairoStack.md, "The pins"):
#
#   1. its lines are well formed (SHA256s, HTTPS URLs, platforms, the msys2
#      section: both environments, each with cairo and freetype, a source
#      package for every package, a package for every msys2-dll line, and
#      MSYS2's cairo is the pinned cairo);
#   2. one version per library across the VMs: cairo, pixman, FreeType,
#      libpng and zlib have the version and SHA256 of the wasm pins
#      (WASM_<NAME>_VERSION and the SHA256 of pharo_wasm_dep_fetch in
#      cmake/emscripten/deps/<name>.cmake), and WASM_CAIRO_FREETYPE_VERSION is
#      the FreeType pin.  A "native-ahead <name> <until YYYY-MM-DD> <reason>"
#      line turns a difference into a warning until that date, and fails after;
#   3. every URL answers (HTTPS HEAD; the mirror when the URL does not): MSYS2
#      removes superseded packages after a while.  --no-probe skips this.
#
#   check-pins.sh [--repo <pharo-vm checkout>] [--pins <file>] [--no-probe] [--today YYYY-MM-DD]
#
# Exits 1 on any error.  POSIX sh.

set -u

here=$(cd "$(dirname "$0")" && pwd)
repo=$here/../..
pins=
probe=yes
today=$(date -u +%Y-%m-%d)

while [ $# -gt 0 ]; do
    case $1 in
        --repo) repo=$2; shift 2 ;;
        --pins) pins=$2; shift 2 ;;
        --no-probe) probe=no; shift ;;
        --today) today=$2; shift 2 ;;
        -h|--help) sed -n '2,/^# Exits/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "check-pins.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
[ -n "$pins" ] || pins=$repo/cmake/cairo-stack/pins.txt
[ -f "$pins" ] || { echo "check-pins.sh: no $pins" >&2; exit 2; }
deps=$repo/cmake/emscripten/deps

errors=0
err() { echo "ERROR $*"; errors=$((errors + 1)); }
warn() { echo "WARNING $*"; }
ok() { echo "ok $*"; }

# ---------------------------------------------------------------- 1. form
form=$(awk '
    function bad(why) { printf "ERROR pins.txt:%d: %s: %s\n", NR, why, $0 }
    function sha(s) { return s ~ /^[0-9a-f]+$/ && length(s) == 64 }
    function https(s) { return s ~ /^https:\/\/[^ ]+$/ }
    /^#/ { prev = (prev ~ /^# (signed-by|unsigned)/ && $0 !~ /^# (signed-by|unsigned)/) ? prev : $0; next }
    NF == 0 { prev = ""; next }
    $1 == "mirror" { if (NF != 3 || !https($3)) bad("mirror <name> <https url>"); next }
    $1 == "msys2-mirror" { if (NF != 2 || !https($2)) bad("msys2-mirror <https base url>"); next }
    $1 == "native-ahead" { if (NF < 4 || $3 !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/) bad("native-ahead <name> <YYYY-MM-DD> <reason>"); next }
    $1 == "accepted-behind" { if (NF < 5 || $4 !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$/) bad("accepted-behind <name> <version> <YYYY-MM-DD> <reason>"); next }
    $1 == "msys2" {
        if (NF != 9) { bad("msys2 line: 9 fields"); next }
        if ($2 != "ucrt64" && $2 != "clangarm64") bad("msys2 environment " $2)
        if ($3 != "root" && $3 != "lib" && $3 != "runtime") bad("msys2 kind " $3)
        if (!sha($6)) bad("package SHA256")
        f = $7; sub(/.*\//, "", f)
        if (!https($7) || f != ($4 "-" $5 "-any.pkg.tar.zst")) bad("package URL (https, <package>-<version>-any.pkg.tar.zst)")
        if (!sha($8)) bad("source package SHA256 (fetch-msys2.sh --closure --sources)")
        if (!https($9) || $9 !~ /\.src\.tar\.zst$/) bad("source package URL")
        next
    }
    $1 == "msys2-dll" { if (NF != 4 || $3 !~ /\.dll$/) bad("msys2-dll <env> <dll> <package>"); next }
    {
        if (NF < 7) { bad("<name> <version> <sha256> <file> <url> <platforms> <licences...>"); next }
        if ($2 !~ /^[0-9]+(\.[0-9]+)+$/) bad("version")
        if (!sha($3)) bad("SHA256")
        if (!https($5)) bad("URL (https only)")
        if (substr($5, length($5) - length($4) + 1) != $4) bad("the URL does not end with the file name")
        if ($6 !~ /^(all|(linux|darwin)(,(linux|darwin))*)$/) bad("platforms (all, or a list of linux, darwin)")
        if (seen[$1]++) bad("two lines for " $1)
        if (prev !~ /^# signed-by [0-9A-F]+ https:/ && prev !~ /^# unsigned: /) bad("no \"# signed-by <fingerprint> <url>\" or \"# unsigned: <how>\" comment before it")
        prev = ""
    }' "$pins")
if [ -n "$form" ]; then
    echo "$form"
    errors=$((errors + $(echo "$form" | wc -l)))
else
    ok "pins.txt is well formed"
fi

for e in ucrt64 clangarm64; do
    for r in cairo freetype; do
        n=$(awk -v e="$e" -v r="$r" '$1 == "msys2" && $2 == e && $3 == "root" && $4 ~ ("-" r "$")' "$pins" | wc -l | tr -d ' ')
        [ "$n" = 1 ] && ok "msys2 $e: $r pinned" || err "msys2 $e: $n root lines for $r"
    done
    for d in libcairo-2.dll libfreetype-6.dll; do
        awk -v e="$e" -v d="$d" '$1 == "msys2-dll" && $2 == e && $3 == d { f = 1 } END { exit !f }' "$pins" \
            && ok "msys2 $e: $d in the closure" || err "msys2 $e: no msys2-dll line for $d"
    done
    missing=$(awk -v e="$e" '$1 == "msys2" && $2 == e { p[$4] = 1 } $1 == "msys2-dll" && $2 == e && !($4 in p) { print $4 }' "$pins" | sort -u | tr '\n' ' ')
    [ -z "$missing" ] && ok "msys2 $e: every DLL's package is pinned" || err "msys2 $e: msys2-dll lines name unpinned packages: $missing"
    unused=$(awk -v e="$e" '$1 == "msys2-dll" && $2 == e { d[$4] = 1 } $1 == "msys2" && $2 == e { p[$4] = 1 } END { for (x in p) if (!(x in d)) print x }' "$pins" | tr '\n' ' ')
    [ -z "$unused" ] && ok "msys2 $e: every pinned package owns a DLL of the closure" || err "msys2 $e: pinned packages without a DLL: $unused"
done
cairo_pin=$(awk '$1 == "cairo" { print $2; exit }' "$pins")
ft_pin=$(awk '$1 == "freetype" { print $2; exit }' "$pins")
for e in ucrt64 clangarm64; do
    v=$(awk -v e="$e" '$1 == "msys2" && $2 == e && $4 ~ /-cairo$/ { print $5 }' "$pins")
    case $v in "$cairo_pin"-*) ok "msys2 $e: cairo $v is the pinned $cairo_pin (+ MSYS2's patches)" ;; *) err "msys2 $e: cairo $v is not the pinned $cairo_pin" ;; esac
    v=$(awk -v e="$e" '$1 == "msys2" && $2 == e && $4 ~ /-freetype$/ { print $5 }' "$pins")
    case $v in "$ft_pin"-*) ok "msys2 $e: freetype $v" ;; *) warn "msys2 $e: freetype $v, not the pinned $ft_pin (MSYS2's version; recorded in CAIRO-STACK.txt)" ;; esac
done

# ---------------------------------------------------------------- 2. the wasm pins
# before <date> <date>: the first is earlier (YYYY-MM-DD)
before() { [ "$(echo "$1" | tr -d -)" -lt "$(echo "$2" | tr -d -)" ]; }
ahead_until() { awk -v n="$1" '$1 == "native-ahead" && $2 == n { print $3; exit }' "$pins"; }
for a in $(awk '$1 == "native-ahead" { print $2 "=" $3 }' "$pins"); do
    if before "${a#*=}" "$today"; then err "native-ahead ${a%%=*} expired on ${a#*=}"; fi
done

# drift <name> <what> <native> <wasm>
drift() {
    if [ "$3" = "$4" ]; then ok "$1 $2: $3 (native = wasm)"; return; fi
    until=$(ahead_until "$1")
    if [ -n "$until" ] && ! before "$until" "$today"; then
        warn "$1 $2: native $3, wasm $4 (native-ahead until $until)"
    else
        err "$1 $2: native $3, wasm $4 (cmake/emscripten/deps/$1.cmake; or a native-ahead line)"
    fi
}
for name in cairo pixman freetype libpng zlib; do
    f=$deps/$name.cmake
    [ -f "$f" ] || { err "no $f"; continue; }
    var=WASM_$(echo "$name" | tr 'a-z' 'A-Z')_VERSION
    wv=$(sed -n "s/^set($var \"\\([^\"]*\\)\")/\\1/p" "$f" | head -n 1)
    ws=$(awk -v n="$name" '$0 ~ ("^pharo_wasm_dep_fetch\\(" n "([ \t]|$)") { on = 1 } on && $1 == "SHA256" { print $2; exit } on && /\)[ \t]*$/ { on = 0 }' "$f")
    pv=$(awk -v n="$name" '$1 == n { print $2; exit }' "$pins")
    ps=$(awk -v n="$name" '$1 == n { print $3; exit }' "$pins")
    [ -n "$wv" ] || err "$f: no set($var ...)"
    [ -n "$ws" ] || err "$f: no SHA256 in pharo_wasm_dep_fetch($name"
    drift "$name" version "$pv" "$wv"
    drift "$name" SHA256 "$ps" "$ws"
done
cfv=$(sed -n 's/^set(WASM_CAIRO_FREETYPE_VERSION "\([^"]*\)")/\1/p' "$deps/cairo.cmake" | head -n 1)
drift freetype "WASM_CAIRO_FREETYPE_VERSION (cairo's wasm configuration)" "$ft_pin" "$cfv"

# ---------------------------------------------------------------- 3. the URLs
if [ "$probe" = yes ]; then
    head_ok() { curl -fsIL --proto '=https' --tlsv1.2 --retry 2 --connect-timeout 20 --max-time 60 -o /dev/null "$1" 2>/dev/null; }
    msys2_mirror=$(awk '$1 == "msys2-mirror" { print $2; exit }' "$pins")
    awk '
        /^#/ || NF == 0 { next }
        $1 == "msys2" { print "msys2", $7; print "msys2", $9; next }
        $1 == "mirror" || $1 == "msys2-mirror" || $1 == "msys2-dll" || $1 == "native-ahead" || $1 == "accepted-behind" { next }
        { print $1, $5 }' "$pins" | sort -u > "${TMPDIR:-/tmp}/check-pins.$$"
    probed=0
    while read -r name url; do
        probed=$((probed + 1))
        if head_ok "$url"; then continue; fi
        if [ "$name" = msys2 ]; then m=${msys2_mirror:+$msys2_mirror/${url##*/}}
        else m=$(awk -v n="$name" '$1 == "mirror" && $2 == n { print $3; exit }' "$pins"); fi
        if [ -n "$m" ] && head_ok "$m"; then
            warn "$url does not answer; its mirror $m does"
        else
            err "$url does not answer${m:+, nor its mirror $m} (docs/CairoStack.md: mirror it, or bump)"
        fi
    done < "${TMPDIR:-/tmp}/check-pins.$$"
    rm -f "${TMPDIR:-/tmp}/check-pins.$$"
    ok "probed $probed URLs"
fi

if [ "$errors" -gt 0 ]; then
    echo "check-pins.sh: $errors error(s)"
    exit 1
fi
echo "check-pins.sh: all good"
