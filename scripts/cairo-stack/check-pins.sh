#!/bin/sh
# shellcheck disable=SC2015,SC2013,SC2018,SC2019  # ok/err never fail; names are ASCII
# Checks cmake/cairo-stack/pins.txt (docs/CairoStack.md, "The pins"):
#
#   1. its lines are well formed (SHA256s, HTTPS URLs ending with the file
#      name, or a GitHub tag archive of the version with a mirror line;
#      platforms, mingw only for poppler; the msys2 section: the msys2-root
#      lines, both environments, each with cairo and freetype, a source
#      package for every package, a package for every msys2-dll line, whose
#      features are core, pango or poppler, and MSYS2's cairo is the pinned
#      cairo);
#   1b. the Windows closure keeps its promises: a msys2-root line for core
#      (cairo, FreeType, libffi-8.dll), pango and poppler, each root a
#      msys2-dll of both environments reached by its own feature; no MSYS2
#      package of poppler, curl, nss, nspr, openssl, lcms2, openjpeg2,
#      libtiff or gpgme (poppler is built from source, without them); the
#      libffi package's only DLL is libffi-8.dll (the VM's FFI is soname 8);
#      a warning when MSYS2's pango, glib2, fribidi, harfbuzz, libjpeg-turbo
#      or libffi is not the version of the source pin;
#   2. one version per library across the VMs: cairo, pixman, FreeType,
#      libpng and zlib have the version and SHA256 of the wasm pins
#      (WASM_<NAME>_VERSION and the SHA256 of pharo_wasm_dep_fetch in
#      cmake/emscripten/deps/<name>.cmake), and WASM_CAIRO_FREETYPE_VERSION is
#      the FreeType pin.  A "native-ahead <name> <until YYYY-MM-DD> <reason>"
#      line turns a difference into a warning until that date, and fails after.
#      The wasm VM builds the libraries of Pango and Poppler from the pins
#      themselves, with the sources harvested from a build of each version
#      (cmake/emscripten/deps/<name>/sources.cmake names it);
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
    # a GitHub tag archive (.../archive/refs/tags/[v]<version><ext>) of the
    # file <name>-<version><ext>: its name is the tag, not the file
    function tag_archive(url, file, name, version,   pre, ext, base) {
        pre = name "-" version
        if (substr(file, 1, length(pre)) != pre) return 0
        ext = substr(file, length(pre) + 1)
        if (ext !~ /^\.(tar\.gz|tar\.xz|tar\.bz2|zip)$/) return 0
        if (url !~ /^https:\/\/github\.com\/[^\/]+\/[^\/]+\/archive\/refs\/tags\/[^\/]+$/) return 0
        base = url; sub(/.*\//, "", base)
        return base == (version ext) || base == ("v" version ext)
    }
    /^#/ { prev = (prev ~ /^# (signed-by|unsigned)/ && $0 !~ /^# (signed-by|unsigned)/) ? prev : $0; next }
    NF == 0 { prev = ""; next }
    $1 == "mirror" { if (NF != 3 || !https($3)) bad("mirror <name> <https url>"); mirrored[$2] = 1; next }
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
    $1 == "msys2-root" {
        if (NF < 3 || ($2 != "core" && $2 != "pango" && $2 != "poppler")) bad("msys2-root core|pango|poppler <dll>...")
        for (i = 3; i <= NF; i++) if ($i !~ /\.dll$/) bad("msys2-root: " $i " is not a DLL")
        next
    }
    $1 == "msys2-dll" {
        if ((NF != 4 && NF != 5) || $3 !~ /\.dll$/) bad("msys2-dll <env> <dll> <package> [<features>]")
        else if (NF == 5 && $5 !~ /^(core|pango|poppler)(,(core|pango|poppler))*$/) bad("msys2-dll features (a comma list of core, pango, poppler)")
        next
    }
    {
        if (NF < 7) { bad("<name> <version> <sha256> <file> <url> <platforms> <licences...>"); next }
        if ($2 !~ /^[0-9]+(\.[0-9]+)+$/) bad("version")
        if (!sha($3)) bad("SHA256")
        if (!https($5)) bad("URL (https only)")
        if (substr($5, length($5) - length($4) + 1) != $4) {
            if (!tag_archive($5, $4, $1, $2)) bad("the URL does not end with the file name (nor is it a GitHub tag archive of the version)")
            else { tagged[$1] = NR; tagline[$1] = $0 }   # GitHub regenerates those: a mirror is required
        }
        if ($6 !~ /^(all|(linux|darwin|mingw)(,(linux|darwin|mingw))*)$/) bad("platforms (all, or a list of linux, darwin, mingw)")
        else if ($6 ~ /mingw/ && $1 != "poppler") bad("only poppler is built from source on Windows (platform mingw)")
        if (seen[$1]++) bad("two lines for " $1)
        if (prev !~ /^# signed-by [0-9A-F]+ https:/ && prev !~ /^# unsigned: /) bad("no \"# signed-by <fingerprint> <url>\" or \"# unsigned: <how>\" comment before it")
        prev = ""
    }
    END {
        for (n in tagged) if (!(n in mirrored))
            printf "ERROR pins.txt:%d: a GitHub tag archive needs a mirror line (GitHub may regenerate it): %s\n", tagged[n], tagline[n]
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

# ---------------------------------------------------------------- 1b. the Windows closure
# The msys2-root lines: one or more per feature; core holds cairo, FreeType and
# libffi-8.dll (the VM's FFI, whatever the features); each root is a DLL of
# both environments, and its features column names the root's feature.
for f in core pango poppler; do
    r=$(awk -v f="$f" '$1 == "msys2-root" && $2 == f { for (i = 3; i <= NF; i++) printf "%s%s", (s++ ? " " : ""), $i }' "$pins")
    [ -n "$r" ] && ok "msys2-root $f: $r" || err "no msys2-root $f line (fetch-msys2.sh --closure walks from it)"
done
for d in libcairo-2.dll libfreetype-6.dll libffi-8.dll; do
    awk -v d="$d" '$1 == "msys2-root" && $2 == "core" { for (i = 3; i <= NF; i++) if ($i == d) f = 1 } END { exit !f }' "$pins" \
        || err "msys2-root core does not name $d"
done
for e in ucrt64 clangarm64; do
    bad=$(awk -v e="$e" '
        $1 == "msys2-root" { for (i = 3; i <= NF; i++) root[$i] = root[$i] " " $2; next }
        $1 == "msys2-dll" && $2 == e { feat[$3] = (NF == 5 ? $5 : "core"); next }
        END {
            for (d in root) {
                if (!(d in feat)) { print "the root " d " has no msys2-dll line"; continue }
                n = split(root[d], fs, " ")
                for (i = 1; i <= n; i++) if (("," feat[d] ",") !~ ("," fs[i] ",")) print "the root " d " of " fs[i] " is marked " feat[d]
            }
        }' "$pins")
    [ -z "$bad" ] && ok "msys2 $e: every root is in the closure of its feature" || err "msys2 $e: $bad"
done

# Poppler is built from source on Windows (it needs none of these); MSYS2's
# poppler would bring curl, nss, nspr, lcms2, openjpeg2, libtiff (and more).
denied=$(awk '$1 == "msys2" && $4 ~ /-(poppler|curl|nss|nspr|openssl|lcms2|openjpeg2|libtiff|gpgme)$/ { printf "%s%s", (s++ ? " " : ""), $2 ":" $4 }' "$pins")
[ -z "$denied" ] && ok "msys2: no poppler, curl, nss, nspr, openssl, lcms2, openjpeg2, libtiff or gpgme package" \
    || err "msys2: denied packages pinned (poppler is built from source without them): $denied"

# The VM's FFI on Windows is libffi-8.dll, from the pinned libffi package (D4):
# a libffi with another soname would break the VM itself.
for e in ucrt64 clangarm64; do
    p=$(awk -v e="$e" '$1 == "msys2" && $2 == e && $4 ~ /-libffi$/ { print $4 }' "$pins")
    if [ -z "$p" ]; then err "msys2 $e: no libffi package (the VM's libffi-8.dll)"; continue; fi
    dlls=$(awk -v e="$e" -v p="$p" '$1 == "msys2-dll" && $2 == e && $4 == p { printf "%s%s", (s++ ? " " : ""), $3 }' "$pins")
    owner=$(awk -v e="$e" '$1 == "msys2-dll" && $2 == e && $3 == "libffi-8.dll" { print $4 }' "$pins")
    if [ "$dlls" = libffi-8.dll ] && [ "$owner" = "$p" ]; then
        ok "msys2 $e: $p provides libffi-8.dll"
    else
        err "msys2 $e: $p must provide exactly libffi-8.dll, the VM's FFI (its DLLs: ${dlls:-none}; libffi-8.dll's package: ${owner:-none})"
    fi
done

# The MSYS2 builds of what Linux and macOS build from source: a different
# upstream version is allowed (MSYS2 moves on its own) but reported, like
# FreeType above.
for e in ucrt64 clangarm64; do
    for n in pango glib2 fribidi harfbuzz libjpeg-turbo libffi; do
        v=$(awk -v e="$e" -v n="$n" '$1 == "msys2" && $2 == e && $4 ~ ("-" n "$") { print $5; exit }' "$pins")
        [ -n "$v" ] || continue
        native=$n; [ "$n" = glib2 ] && native=glib
        nv=$(awk -v n="$native" '$1 == n && NF >= 7 { print $2; exit }' "$pins")
        [ -n "$nv" ] || continue
        if [ "${v%-*}" = "$nv" ]; then ok "msys2 $e: $n $v (native $nv)"
        else warn "msys2 $e $n ${v%-*} vs native $nv (MSYS2's version; recorded in CAIRO-STACK.txt)"; fi
    done
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
# The libraries of the pango and poppler features.  The wasm VM builds
# expat, fontconfig, GLib, FriBidi, HarfBuzz, Pango, libjpeg-turbo and
# Poppler (WASM_PANGO, WASM_POPPLER) from these very lines
# (cmake/emscripten/deps/pango.cmake reads pins.txt), with the sources that
# scripts/wasm-deps/regen-text-stack.sh harvested from a build of that
# version: the first line of cmake/emscripten/deps/<name>/sources.cmake must
# name it.  libffi (the wasm VM's is the uFFI one, a separate pin), PCRE2
# (no GRegex there) and proxy-libintl are native only.
for name in expat fontconfig glib fribidi harfbuzz pango libjpeg-turbo poppler; do
    v=$(awk -v n="$name" '$1 == n && NF >= 7 { print $2; exit }' "$pins")
    f=$deps/$name/sources.cmake
    if [ -z "$v" ]; then
        err "no source line for $name (the feature map of build.sh builds it)"
    elif ! [ -f "$f" ]; then
        err "no cmake/emscripten/deps/$name/sources.cmake (scripts/wasm-deps/regen-text-stack.sh)"
    else
        hv=$(sed -n '1s/^# The sources of [^ ]* \([0-9][0-9.]*\) for the Emscripten VM.*/\1/p' "$f")
        drift "$name" "version (deps/$name/sources.cmake)" "$v" "$hv"
    fi
done
for name in libffi pcre2 proxy-libintl; do
    v=$(awk -v n="$name" '$1 == n && NF >= 7 { print $2; exit }' "$pins")
    [ -n "$v" ] && ok "$name $v: native only" || err "no source line for $name (the feature map of build.sh builds it)"
done
cfv=$(sed -n 's/^set(WASM_CAIRO_FREETYPE_VERSION "\([^"]*\)")/\1/p' "$deps/cairo.cmake" | head -n 1)
drift freetype "WASM_CAIRO_FREETYPE_VERSION (cairo's wasm configuration)" "$ft_pin" "$cfv"

# ---------------------------------------------------------------- 3. the URLs
if [ "$probe" = yes ]; then
    # --retry-all-errors: a release download of GitHub once failed in 6 s,
    # an error --retry alone does not retry (only timeouts, 408, 429, 5xx)
    head_ok() { curl -fsIL --proto '=https' --tlsv1.2 --retry 3 --retry-all-errors --retry-delay 5 \
        --connect-timeout 20 --max-time 60 -o /dev/null "$1" 2>/dev/null; }
    msys2_mirror=$(awk '$1 == "msys2-mirror" { print $2; exit }' "$pins")
    awk '
        /^#/ || NF == 0 { next }
        $1 == "msys2" { print "msys2", $7; print "msys2", $9; next }
        $1 == "mirror" || $1 == "msys2-mirror" || $1 == "msys2-dll" || $1 == "msys2-root" || $1 == "native-ahead" || $1 == "accepted-behind" { next }
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
