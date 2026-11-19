#!/bin/sh
# shellcheck disable=SC2319,SC2018,SC2019,SC2012,SC2086,SC2016  # "[ ... ]; check $?" is the idiom; DLL names are ASCII; word splitting of lists is wanted
# Assertions on a cairo stack (docs/CairoStack.md, "What check.sh asserts"):
# on a prefix that build.sh or fetch-msys2.sh made, or on a VM bundle that
# carries it (the build tree's VM directory, or an unzipped release zip).
#
#   check.sh (--prefix <dir> | --bundle <dir>) --platform linux|darwin|mingw
#            [--arch <arch>] [--glibc-max <x.y>] [--glibcxx-max <x.y.z>]
#            [--minos <x.y>] [--env ucrt64|clangarm64] [--features <list>]
#            [--pins <file>] [--symbols <file>] [--no-probe]
#
#   --prefix   <dir>/lib (Linux, macOS) or <dir>/bin (Windows), and the
#              prefix's share/pharo-cairo-stack (manifest, licences)
#   --bundle   Linux: <dir>/lib if it has libcairo.so.2, else <dir>; macOS:
#              <dir>/Contents/MacOS/Plugins (notices in Contents/Resources) if
#              <dir> is an app, else <dir>; Windows: <dir>.  The notices
#              (LICENSE.*, THIRD-PARTY-NOTICES.cairo-stack.txt, CAIRO-STACK.txt)
#              are looked for next to the libraries, then in <dir>.
#   --features the features to check besides cairo and FreeType, a comma list
#              of pango and poppler (none or empty: none); default: the
#              prefix manifest's "features" line, or the bundle's "Bundled
#              features:" line in CAIRO-STACK.txt.  The libraries of a feature
#              that is neither checked nor declared must be absent.
#   --glibc-max  Linux: fail when a stack library needs a newer GLIBC_ symbol
#              version (the highest one is always reported)
#   --glibcxx-max  Linux: the same for GLIBCXX_ (default 3.4.32 when poppler
#              is checked: libstdc++ of GCC 13)
#   --minos    macOS: fail when a library's minimum macOS is newer
#   --symbols  cairo.txt, the list of cairo functions the images bind; the
#              lists of the other libraries (glib.txt, gobject.txt,
#              harfbuzz.txt, pango.txt, pangocairo.txt, poppler-glib.txt) are
#              read from the same directory
#   --no-probe do not compile and run probe.c (Linux and macOS: toy text) and
#              probe-text.c (Pango, Poppler)
#
# It prints "ok <what>" or "FAIL <what>" per assertion, and exits 1 on any FAIL.
# Tools: readelf, nm, objdump (Linux); otool, nm, lipo, codesign (macOS);
# objdump or llvm-objdump (Windows); a C compiler for the probes.
#
# POSIX sh.

set -u

here=$(cd "$(dirname "$0")" && pwd)
pins=$here/../../cmake/cairo-stack/pins.txt
symbols=$here/../../cmake/emscripten/ffi/symbols/cairo.txt
mode=
dir=
platform=
arch=
glibc_max=
glibcxx_max=
minos=
env=
features_opt=
features_given=no
probe=yes
failures=0

die() { echo "check.sh: $*" >&2; exit 2; }
usage() { sed -n '2,/^# POSIX sh/p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }
ok() { echo "ok $*"; }
fail() { echo "FAIL $*"; failures=$((failures + 1)); }
check() { if [ "$1" = 0 ]; then shift; ok "$@"; else shift; fail "$@"; fi; }

while [ $# -gt 0 ]; do
    case $1 in
        --prefix) mode=prefix; dir=$2; shift 2 ;;
        --bundle) mode=bundle; dir=$2; shift 2 ;;
        --platform) platform=$2; shift 2 ;;
        --arch) arch=$2; shift 2 ;;
        --glibc-max) glibc_max=$2; shift 2 ;;
        --glibcxx-max) glibcxx_max=$2; shift 2 ;;
        --minos) minos=$2; shift 2 ;;
        --env) env=$2; shift 2 ;;
        --features) features_opt=$2; features_given=yes; shift 2 ;;
        --pins) pins=$2; shift 2 ;;
        --symbols) symbols=$2; shift 2 ;;
        --no-probe) probe=no; shift ;;
        -h|--help) usage 0 ;;
        *) echo "check.sh: unknown argument $1" >&2; usage 2 >&2 ;;
    esac
done
[ -n "$mode" ] || die "--prefix or --bundle"
[ -d "$dir" ] || die "no directory $dir"
case $platform in linux|darwin|mingw) ;; *) die "--platform linux, darwin or mingw" ;; esac
[ -f "$pins" ] || die "no $pins"
dir=$(cd "$dir" && pwd)
symdir=$(dirname "$symbols")

pin_version() {
    awk -v n="$1" '$1 == n && $1 !~ /^#/ { print $2; exit }' "$pins"
}
cairo_pin=$(pin_version cairo)
[ -n "$cairo_pin" ] || die "no cairo line in $pins"

# ---------------------------------------------------------------- features
# norm_features <list>: "pango poppler", in this order, from a comma or space
# list (none, - or empty: none)
norm_features() {
    nf_list=$(echo "$1" | tr ',' ' ')
    for nf_f in $nf_list; do
        case $nf_f in pango|poppler|none|-) ;; *) die "features: $nf_f is not pango or poppler" ;; esac
    done
    nf_out=
    for nf_f in pango poppler; do
        case " $nf_list " in *" $nf_f "*) nf_out="$nf_out $nf_f" ;; esac
    done
    echo "${nf_out# }"
}
# What the prefix or the bundle says it carries
declared=
declared_from=
only=-
if [ "$mode" = prefix ]; then
    m=$dir/share/pharo-cairo-stack/manifest.txt
    if [ -f "$m" ] && grep -q '^features ' "$m"; then
        declared=$(norm_features "$(sed -n 's/^features //p' "$m" | head -n 1)")
        declared_from="the manifest"
    fi
    [ -f "$m" ] && only=$(awk '$1 == "only" { print $2; exit }' "$m")
    [ -n "$only" ] || only=-
else
    for c in "$dir/lib/CAIRO-STACK.txt" "$dir/Contents/Resources/CAIRO-STACK.txt" "$dir/CAIRO-STACK.txt"; do
        [ -f "$c" ] || continue
        if grep -q '^Bundled features:' "$c"; then
            declared=$(norm_features "$(sed -n 's/^Bundled features://p' "$c" | head -n 1 | tr ' ' '\n' | grep -x -e pango -e poppler | tr '\n' ' ')")
            declared_from="CAIRO-STACK.txt"
        fi
        break
    done
fi
if [ "$features_given" = yes ]; then
    features=$(norm_features "$features_opt")
else
    features=$declared
fi
has_feature() { case " $features " in *" $1 "*) return 0 ;; esac; return 1; }
declares_feature() { case " $declared " in *" $1 "*) return 0 ;; esac; return 1; }
# A feature neither checked nor declared: its libraries must be absent
unbundled() { ! has_feature "$1" && ! declares_feature "$1"; }
src="no features line: cairo and FreeType only"
[ -z "$declared_from" ] || src="$declared_from says ${declared:-none}"
[ "$only" = - ] || src="$src; only $only"
echo "check.sh: features: ${features:-none} ($src)"
if [ -n "$declared_from" ]; then
    bad=
    for f in $features; do declares_feature "$f" || bad="$bad $f"; done
    [ -z "$bad" ]; check $? "features ${features:-none} are bundled by the stack (${declared_from}: ${declared:-none})${bad:+; missing:$bad}"
fi
[ -n "$glibcxx_max" ] || ! has_feature poppler || glibcxx_max=3.4.32

# The pins of this platform and the checked features (the feature map of
# build.sh and cmake/cairoStack.cmake)
platform_libs=$(awk -v p="$platform" -v features="$features" -v only="$only" '
    function feats(n) {
        if (n == "libffi" || n == "pcre2" || n == "proxy-libintl" || n == "glib") return "pango poppler"
        if (n == "fribidi" || n == "harfbuzz" || n == "pango") return "pango"
        if (n == "libjpeg-turbo" || n == "poppler") return "poppler"
        if ((n == "expat" || n == "fontconfig") && p == "darwin") return "poppler"
        return "core"
    }
    function wanted(n,   m, fs, i) {
        if (only == "freetype") return n == "zlib" || n == "libpng" || n == "freetype"
        m = split(feats(n), fs, " ")
        for (i = 1; i <= m; i++) if (fs[i] == "core" || index(" " features " ", " " fs[i] " ")) return 1
        return 0
    }
    /^#/ || NF < 7 || $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
    { n = split($6, ps, ","); ok = ($6 == "all"); for (i = 1; i <= n; i++) if (ps[i] == p) ok = 1; if (ok && wanted($1)) print $1 }' "$pins")
in_platform_libs() { echo "$platform_libs" | grep -qx "$1"; }

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# version_le <a> <b>: a <= b, for dotted numbers
version_le() {
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -n 1)" = "$1" ]
}

# The functions the images bind, from a list of cmake/emscripten/ffi/symbols/
# (default cairo.txt): every name, but those of the plain "# unavailable:"
# sections; the Quartz section's name only on macOS.  ("unavailable unless"
# sections are wasm options that the native stacks always have.)
required_symbols() {
    rs_file=${1:-$symbols}
    [ -f "$rs_file" ] || return 0
    awk -v p="$platform" '
        /^#[ \t]*unavailable[ \t]+unless/ { skip = 0; next }
        /^#[ \t]*unavailable/ { skip = !(p == "darwin" && $0 ~ /Quartz/); next }
        /^#/ || NF == 0 { next }
        !skip { print $1 }' "$rs_file"
}
# The cairo functions of LibGLib-Cairo, LibPangoCairo and the Poppler test
# fixtures (cairo.txt feeds the wasm registry and stays as it is)
extra_cairo_symbols="cairo_save cairo_restore cairo_set_matrix cairo_new_path cairo_move_to cairo_set_source_rgba
cairo_set_source_surface cairo_paint cairo_font_options_create cairo_font_options_destroy
cairo_font_options_set_hint_metrics cairo_font_options_set_hint_style cairo_pdf_surface_create
cairo_pdf_surface_set_size cairo_pdf_surface_set_page_label cairo_pdf_surface_set_metadata
cairo_surface_set_mime_data cairo_surface_finish cairo_surface_destroy cairo_show_page
cairo_select_font_face cairo_set_font_size cairo_show_text"

# check_exports <library label> <file of its exported names> <list name>...:
# the library exports every name of the lists (cairo: also the extra names)
check_exports() {
    ce_label=$1 ce_exports=$2; shift 2
    sort -u "$ce_exports" > "$ce_exports.sorted"
    for ce_list in "$@"; do
        if [ "$ce_list" = extra-cairo ]; then
            ce_names=$(echo $extra_cairo_symbols | tr ' ' '\n')
            ce_what="the extra cairo functions of the Pango and Poppler bindings"
        else
            [ -f "$symdir/$ce_list" ] || { fail "$ce_label: no symbol list $symdir/$ce_list"; continue; }
            ce_names=$(required_symbols "$symdir/$ce_list")
            ce_what="the functions of $ce_list"
        fi
        ce_total=$(echo "$ce_names" | grep -c .)
        ce_missing=$(echo "$ce_names" | sort -u | comm -23 - "$ce_exports.sorted" | tr '\n' ' ')
        [ -z "$ce_missing" ]; check $? "$ce_label exports $ce_what ($ce_total)${ce_missing:+; missing: $ce_missing}"
    done
    rm -f "$ce_exports.sorted"
}

# The libraries that a stack never carries (Linux, Windows deny-list): the
# network, crypto, colour and image stacks of a full Poppler, Qt, and shared
# copies of what is static and private here
denied_linux() {
    case $1 in
        libnss3.*|libsmime3.*|libnspr4.*|libplc4.*|libcurl.*|libgpgme.*|libgpgmepp.*|liblcms2.*|libopenjp2.*|\
        libtiff.*|libbrotli*|libQt*|libjpeg.so*|libffi.so*|libpcre2*) return 0 ;;
    esac
    return 1
}
denied_dll() {
    case $(echo "$1" | tr 'A-Z' 'a-z') in
        nss3.dll|libnss3*|smime3.dll|libsmime3*|nspr4.dll|libnspr4*|plc4.dll|libplc4*|libcurl*|libgpgme*|\
        liblcms2*|libopenjp2*|libtiff*|libbrotli*|qt5*|qt6*|libqt*) return 0 ;;
    esac
    return 1
}
# The private static copies: no symbol of theirs in a dynamic symbol table
private_symbol_re='^(ffi_|pcre2_|jpeg_|jinit_|jcopy_|inflate|deflate|crc32|adler32|zlibVersion$|zlibCompileFlags$|XML_)'

notices_dir() {
    for n_d in "$@"; do
        if [ -f "$n_d/CAIRO-STACK.txt" ]; then echo "$n_d"; return 0; fi
    done
    echo "$1"
}

check_notices() {
    nd=$1; shift
    for n in "$@"; do
        test -s "$nd/LICENSE.$n"; check $? "notice LICENSE.$n"
    done
    test -s "$nd/THIRD-PARTY-NOTICES.cairo-stack.txt"; check $? "notice THIRD-PARTY-NOTICES.cairo-stack.txt"
    test -s "$nd/CAIRO-STACK.txt"; check $? "notice CAIRO-STACK.txt"
    if has_feature poppler; then
        # Poppler is distributed under the GPL version 3, with a written offer
        n=$nd/THIRD-PARTY-NOTICES.cairo-stack.txt
        grep -q "GNU General Public License" "$n" 2>/dev/null && grep -q "version 3" "$n" && grep -q "three years" "$n"
        check $? "THIRD-PARTY-NOTICES.cairo-stack.txt: Poppler under the GNU General Public License version 3, the offer valid three years"
    fi
}

check_prefix_share() {
    share=$dir/share/pharo-cairo-stack
    test -s "$share/manifest.txt"; check $? "manifest $share/manifest.txt"
    if [ -s "$share/manifest.txt" ]; then
        m_pins=$(awk '$1 == "pins-sha256" { print $2 }' "$share/manifest.txt")
        [ "$m_pins" = "$(sha256 "$pins")" ]; check $? "manifest pins-sha256 is that of $pins (else the prefix is stale)"
        m_platform=$(awk '$1 == "platform" { print $2 }' "$share/manifest.txt")
        [ "$m_platform" = "$platform" ]; check $? "manifest platform $m_platform"
        m_format=$(awk '$1 == "format" { print $2 }' "$share/manifest.txt")
        [ "$m_format" = 2 ] && grep -q '^features ' "$share/manifest.txt"; check $? "manifest format $m_format (2) with a features line"
    fi
    test -s "$share/CAIRO-STACK.txt"; check $? "index $share/CAIRO-STACK.txt"
}

# check_absent <label> <dir> "<globs>": none of the globs matches in <dir>
# (one argument, expanded in <dir> only)
check_absent() {
    ca_label=$1 ca_dir=$2 ca_globs=$3
    ca_left=$(cd "$ca_dir" && for g in $ca_globs; do ls -d $g 2>/dev/null; done | tr '\n' ' ')
    [ -z "$ca_left" ]; check $? "$ca_label${ca_left:+: $ca_left}"
}

# The libraries of the features that are not there (Linux and macOS): globs
unbundled_globs() {
    if unbundled pango; then echo "libpango* libfribidi* libharfbuzz*"; fi
    if unbundled poppler; then echo "libpoppler*"; [ "$platform" = darwin ] && echo "libfontconfig*"; fi
    if unbundled pango && unbundled poppler; then echo "libglib* libgobject* libgio* libgmodule*"; fi
}

cc_for_probe() {
    cfp=${CC:-}
    if [ -z "$cfp" ]; then for t in clang cc gcc; do command -v $t >/dev/null 2>&1 && { cfp=$t; break; }; done; fi
    echo "$cfp"
}

run_probe() {
    # run_probe <path of libcairo>
    [ "$probe" = yes ] || { echo "skip probe (--no-probe)"; return 0; }
    cc=$(cc_for_probe)
    [ -n "$cc" ] || { fail "probe: no C compiler (CC)"; return 0; }
    t=$(mktemp -d "${TMPDIR:-/tmp}/cairo-check.XXXXXX")
    libs=; [ "$platform" = linux ] && libs=-ldl
    if ! $cc -std=c99 -D_POSIX_C_SOURCE=200809L -D_DARWIN_C_SOURCE -o "$t/probe" "$here/probe.c" $libs 2> "$t/cc.txt"; then
        cat "$t/cc.txt"; fail "probe: compile"; rm -rf "$t"; return 0
    fi
    # The system's configuration (FONTCONFIG_FILE unset), cold then warm caches
    (unset FONTCONFIG_FILE FONTCONFIG_PATH; XDG_CACHE_HOME=$t/cache; export XDG_CACHE_HOME
     exec "$t/probe" "$1") > "$t/cold.txt" 2> "$t/cold.err"
    rc=$?
    (unset FONTCONFIG_FILE FONTCONFIG_PATH; XDG_CACHE_HOME=$t/cache; export XDG_CACHE_HOME
     exec "$t/probe" "$1") > "$t/warm.txt" 2> "$t/warm.err"
    sed 's/^/    /' "$t/cold.txt" "$t/cold.err"
    v=$(awk '$1 == "probe" && $2 == "cairo" { print $3 }' "$t/cold.txt")
    [ "$v" = "$cairo_pin" ]; check $? "cairo_version_string() $v = the pin $cairo_pin"
    [ "$rc" = 0 ] && grep -q '^probe toy ok .* ink [1-9]' "$t/cold.txt"; check $? "toy text draws ink: $(sed -n 's/^probe toy //p' "$t/cold.txt")"
    ! grep -q -i 'fontconfig' "$t/cold.err" "$t/warm.err"; check $? "no fontconfig message on stderr (e.g. Cannot load default config file)"
    echo "    first cairo_show_text: cold $(sed -n 's/.*first-call-ms //p' "$t/cold.txt") ms, warm $(sed -n 's/.*first-call-ms //p' "$t/warm.txt") ms"
    if [ "$platform" = linux ]; then
        # An empty configuration: no fonts, cairo's built-in "twin" font
        printf '<?xml version="1.0"?>\n<fontconfig></fontconfig>\n' > "$t/empty.conf"
        FONTCONFIG_FILE=$t/empty.conf XDG_CACHE_HOME="$t/cache2" "$t/probe" "$1" > "$t/empty.txt" 2> "$t/empty.err"
        rc=$?
        sed 's/^/    /' "$t/empty.txt" "$t/empty.err"
        [ "$rc" = 0 ] && grep -q '^probe toy ok family twin type user ink [1-9]' "$t/empty.txt"; check $? "empty fontconfig configuration: toy text falls back to twin"
    fi
    rm -rf "$t"
}

run_probe_text() {
    # run_probe_text <dir of the libraries> [<fonts.conf>]: probe-text.c on
    # the checked features
    [ -n "$features" ] || return 0
    [ "$probe" = yes ] || { echo "skip probe-text (--no-probe)"; return 0; }
    cc=$(cc_for_probe)
    [ -n "$cc" ] || { fail "probe-text: no C compiler (CC)"; return 0; }
    t=$(mktemp -d "${TMPDIR:-/tmp}/cairo-check.XXXXXX")
    if [ "$platform" = linux ]; then defs=-D_GNU_SOURCE libs=-ldl; else defs=-D_DARWIN_C_SOURCE libs=; fi
    if ! $cc -std=c99 $defs -o "$t/probe-text" "$here/probe-text.c" $libs 2> "$t/cc.txt"; then
        cat "$t/cc.txt"; fail "probe-text: compile"; rm -rf "$t"; return 0
    fi
    # The system's configuration, or (macOS) the bundled fonts.conf as LibPoppler loads it
    (unset FONTCONFIG_FILE FONTCONFIG_PATH
     if [ -n "${2:-}" ]; then FONTCONFIG_FILE=$2; export FONTCONFIG_FILE; fi
     XDG_CACHE_HOME=$t/cache; export XDG_CACHE_HOME
     exec "$t/probe-text" "$1" "$t" $features) > "$t/out.txt" 2> "$t/err.txt"
    rc=$?
    sed 's/^/    /' "$t/out.txt" "$t/err.txt"
    [ "$rc" = 0 ]; check $? "probe-text exits 0 ($rc)"
    [ ! -s "$t/err.txt" ]; check $? "probe-text: nothing on stderr (no Poppler, Pango or fontconfig message)"
    if has_feature pango; then
        v=$(pin_version pango)
        grep -q "^pango $v font-map [^ ]* family .* ink [1-9]" "$t/out.txt"
        check $? "Pango $v (the pin) lays out and draws text: $(sed -n 's/^pango //p' "$t/out.txt")"
    fi
    if has_feature poppler; then
        v=$(pin_version poppler)
        grep -q "^poppler $v pages 2 size 300x200 text \"Hello Poppler.*\" ink [1-9][0-9]* jpeg " "$t/out.txt"
        check $? "Poppler $v (the pin) opens cairo's 2-page PDF from bytes, extracts and renders page 1"
        rgb=$(sed -n 's/^poppler .* jpeg \([0-9]*\),\([0-9]*\),\([0-9]*\)$/\1 \2 \3/p' "$t/out.txt")
        read -r r g b _ <<EOF2
$rgb 0 255 255
EOF2
        [ "$r" -gt 150 ] && [ "$g" -lt 60 ] && [ "$b" -lt 60 ]; check $? "Poppler decodes the JPEG of page 2 (DCT): centre $r,$g,$b is red"
        grep -q '^base14 text "Base14 Pharo.*" ink [1-9]' "$t/out.txt"
        check $? "Poppler extracts and renders the base-14 PDF (Helvetica not embedded, substituted through fontconfig)"
    fi
    grep -qx 'instances ok' "$t/out.txt"; check $? "one instance each of GLib, GObject, cairo, FreeType, HarfBuzz, Pango, fontconfig, from the stack"
    rm -rf "$t"
}

# ================================================================ Linux
check_linux() {
    if [ "$mode" = prefix ]; then lib=$dir/lib
    elif [ -e "$dir/lib/libcairo.so.2" ]; then lib=$dir/lib
    else lib=$dir; fi
    echo "check.sh: Linux stack in $lib"
    for t in readelf nm objdump; do command -v $t >/dev/null 2>&1 || die "needs $t"; done
    cmaj=$(echo "$cairo_pin" | cut -d. -f1); cmin=$(echo "$cairo_pin" | cut -d. -f2); cmic=$(echo "$cairo_pin" | cut -d. -f3)
    cairo_real=libcairo.so.2.$((cmaj * 10000 + cmin * 100 + cmic)).$cmic
    png_minor=$(pin_version libpng | cut -d. -f3)
    pix=$(pin_version pixman)

    # pin: real file (or glob), links (@soname: the file's SONAME)
    set -- "cairo $cairo_real libcairo.so.2 libcairo.so" \
           "freetype libfreetype.so.6.* libfreetype.so.6 libfreetype.so" \
           "pixman libpixman-1.so.$pix libpixman-1.so.0 libpixman-1.so" \
           "libpng libpng16.so.16.$png_minor.0 libpng16.so.16 libpng16.so" \
           "fontconfig libfontconfig.so.1.* libfontconfig.so.1 libfontconfig.so" \
           "glib libglib-2.0.so.0.* libglib-2.0.so.0 libglib-2.0.so" \
           "glib libgobject-2.0.so.0.* libgobject-2.0.so.0 libgobject-2.0.so" \
           "glib libgio-2.0.so.0.* libgio-2.0.so.0" \
           "glib libgmodule-2.0.so.0.* libgmodule-2.0.so.0" \
           "fribidi libfribidi.so.0.* libfribidi.so.0" \
           "harfbuzz libharfbuzz.so.0.* libharfbuzz.so.0" \
           "pango libpango-1.0.so.0.* libpango-1.0.so.0 libpango-1.0.so" \
           "pango libpangocairo-1.0.so.0.* libpangocairo-1.0.so.0 libpangocairo-1.0.so" \
           "pango libpangoft2-1.0.so.0.* libpangoft2-1.0.so.0" \
           "poppler libpoppler.so.[0-9]*.[0-9]*.[0-9]* @soname" \
           "poppler libpoppler-glib.so.8.* libpoppler-glib.so.8 libpoppler-glib.so"
    soname_of() { readelf -d "$1" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p'; }
    reals=
    for entry in "$@"; do
        set -- $entry
        name=$1 real=$2; shift 2
        in_platform_libs "$name" || continue
        found=
        for f in $lib/$real; do [ -f "$f" ] && [ ! -L "$f" ] && found=$f; done
        [ -n "$found" ]; check $? "$name: $(basename "${found:-$real}") is a file"
        [ -n "$found" ] && reals="$reals $found"
        for l in "$@"; do
            if [ "$l" = @soname ]; then
                l=$( [ -n "$found" ] && soname_of "$found")
                [ -n "$l" ] || { fail "$name: $(basename "${found:-$real}") has a SONAME"; continue; }
            fi
            test -L "$lib/$l" && [ -e "$lib/$l" ]; check $? "$name: $l is a symlink to it"
        done
    done
    stack_sonames=
    for f in $reals; do stack_sonames="$stack_sonames $(soname_of "$f")"; done
    # real_of <glob>: the checked file of that name
    real_of() { for ro_f in $reals; do case $(basename "$ro_f") in $1) echo "$ro_f"; return 0 ;; esac; done; }

    max_glibc=0; max_lib=
    max_glibcxx=0; max_cxx_lib=
    denied=
    for f in $reals; do
        b=$(basename "$f")
        needed=$(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')
        bad=
        for n in $needed; do
            denied_linux "$n" && denied="$denied $b:$n"
            case " $stack_sonames " in *" $n "*) continue ;; esac
            case $n in libc.so.6|libm.so.6|libpthread.so.0|libdl.so.2|librt.so.1|ld-linux*.so.*) continue ;; esac
            # Poppler is C++: libstdc++ of GCC 13 (D20), and its libgcc_s
            case $b:$n in libpoppler.so.*:libstdc++.so.6|libpoppler.so.*:libgcc_s.so.1|\
                          libpoppler-glib.so.*:libstdc++.so.6|libpoppler-glib.so.*:libgcc_s.so.1) continue ;; esac
            bad="$bad $n"
        done
        [ -z "$bad" ]; check $? "$b NEEDED within the stack and glibc: $needed${bad:+ (not allowed:$bad)}"
        rpath=$(readelf -d "$f" | sed -n 's/.*(RPATH).*\[\(.*\)\]/\1/p')
        runpath=$(readelf -d "$f" | sed -n 's/.*(RUNPATH).*\[\(.*\)\]/\1/p')
        [ "$rpath" = '$ORIGIN' ] && [ -z "$runpath" ]; check $? "$b RPATH '$rpath' = \$ORIGIN, no RUNPATH${runpath:+ ($runpath)}"
        g=$(objdump -T "$f" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
        if [ -n "$g" ] && ! version_le "$g" "$max_glibc"; then max_glibc=$g; max_lib=$b; fi
        g=$(objdump -T "$f" | grep -o 'GLIBCXX_[0-9][0-9.]*' | sed 's/GLIBCXX_//' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
        if [ -n "$g" ] && ! version_le "$g" "$max_glibcxx"; then max_glibcxx=$g; max_cxx_lib=$b; fi
        # The private static copies (libffi, pcre2, libjpeg-turbo, zlib, expat)
        # are hidden: none of their symbols defined or undefined
        n=$(nm -D "$f" | awk '{ sub(/@.*/, "", $NF); print $NF }' | grep -E "$private_symbol_re" | sort -u | tr '\n' ' ')
        [ -z "$n" ]; check $? "$b: no symbol of a private copy (ffi_ pcre2_ jpeg_ jinit_ jcopy_, zlib, XML_), defined or undefined${n:+: $n}"
    done
    [ -z "$denied" ]; check $? "no library NEEDS a denied one (nss, nspr, curl, gpgme, lcms2, openjp2, tiff, brotli, Qt, shared jpeg, ffi or pcre2)${denied:+:$denied}"
    cairo_f=$lib/$cairo_real
    fc_f=$(ls $lib/libfontconfig.so.1.* 2>/dev/null | head -n 1)
    for f in "$cairo_f" $fc_f; do
        [ -f "$f" ] || continue
        b=$(basename "$f")
        n=$(nm -D --defined-only "$f" | awk '$3 ~ /^(deflate|inflate|crc32|adler32|compress|uncompress|zlibVersion|XML_)/' | wc -l | tr -d ' ')
        [ "$n" = 0 ]; check $? "$b exports no zlib or expat symbol ($n)"
    done
    # One FreeType, one cairo, one HarfBuzz, one GLib with the image: the
    # libraries load the stack's, and no other library defines their symbols
    for f in "$cairo_f" $fc_f $(real_of 'libharfbuzz.so.*') $(real_of 'libpangoft2-1.0.so.*') \
             $(real_of 'libpoppler.so.*') $(real_of 'libpoppler-glib.so.*'); do
        [ -f "$f" ] || continue
        readelf -d "$f" | grep -q 'NEEDED.*\[libfreetype\.so\.6\]'; check $? "$(basename "$f") NEEDS libfreetype.so.6 (one FreeType with the image)"
    done
    for f in $(real_of 'libpangocairo-1.0.so.*') $(real_of 'libpoppler-glib.so.*'); do
        readelf -d "$f" | grep -q 'NEEDED.*\[libcairo\.so\.2\]'; check $? "$(basename "$f") NEEDS libcairo.so.2 (one cairo with the image)"
    done
    f=$(real_of 'libpango-1.0.so.*')
    if [ -n "$f" ]; then
        readelf -d "$f" | grep -q 'NEEDED.*\[libharfbuzz\.so\.0\]'; check $? "$(basename "$f") NEEDS libharfbuzz.so.0 (one HarfBuzz)"
    fi
    for p in 'FT_:libfreetype.so.*' 'cairo_:libcairo.so.*' 'hb_:libharfbuzz.so.*' \
             'g_|glib_:libglib-2.0.so.*|libgobject-2.0.so.*|libgio-2.0.so.*|libgmodule-2.0.so.*'; do
        sym=${p%%:*} owners=${p#*:}
        others=
        for f in $reals; do
            b=$(basename "$f")
            eval "case \$b in $owners) is_owner=yes ;; *) is_owner=no ;; esac"
            [ "$is_owner" = no ] || continue
            n=$(nm -D --defined-only "$f" | awk '{ print $3 }' | grep -c -E "^($sym)")
            [ "$n" = 0 ] || others="$others $b($n)"
        done
        [ -z "$others" ]; check $? "no library but $(echo "$owners" | sed 's/\.so\.\*//g; s/|/, /g') defines ${sym%%|*}*${others:+; also:$others}"
    done
    # The highest glibc symbol version (decision U1: Ubuntu 24.04's 2.39 at most)
    echo "check.sh: highest GLIBC_ version needed: $max_glibc ($max_lib)"
    echo "glibc-max $max_glibc"
    if [ -n "$glibc_max" ]; then
        version_le "$max_glibc" "$glibc_max"; check $? "GLIBC_$max_glibc <= GLIBC_$glibc_max"
    fi
    # libstdc++ (Poppler): GCC 13's, GLIBCXX_3.4.32 at most (D20)
    [ "$max_glibcxx" = 0 ] && max_glibcxx=-
    echo "check.sh: highest GLIBCXX_ version needed: $max_glibcxx${max_cxx_lib:+ ($max_cxx_lib)}"
    echo "glibcxx-max $max_glibcxx"
    if [ -n "$glibcxx_max" ]; then
        [ "$max_glibcxx" = - ] || version_le "$max_glibcxx" "$glibcxx_max"; check $? "GLIBCXX_$max_glibcxx <= GLIBCXX_$glibcxx_max"
    fi
    # The exports the images bind
    t=${TMPDIR:-/tmp}/cairo-exports.$$
    if [ -f "$cairo_f" ]; then
        nm -D --defined-only "$cairo_f" | awk '{ print $3 }' > "$t"
        check_exports libcairo "$t" "$(basename "$symbols")" extra-cairo
    fi
    for e in 'glib.txt:libglib-2.0.so.*' 'gobject.txt:libgobject-2.0.so.*' 'harfbuzz.txt:libharfbuzz.so.*' \
             'pango.txt:libpango-1.0.so.*' 'pangocairo.txt:libpangocairo-1.0.so.*' 'poppler-glib.txt:libpoppler-glib.so.*'; do
        f=$(real_of "${e#*:}")
        [ -n "$f" ] || continue
        [ "${e%%:*}" != harfbuzz.txt ] || has_feature pango || continue
        nm -D --defined-only "$f" | awk '{ print $3 }' > "$t"
        check_exports "$(basename "$f")" "$t" "${e%%:*}"
    done
    rm -f "$t"
    check_build_strings
    check_absent "no library of a feature that is not bundled" "$lib" "$(unbundled_globs)"
    if [ "$mode" = bundle ]; then
        check_absent "no dropped library in the bundle" "$lib" 'libbz2* libexpat* libX11* libXext* liblzo2* libbfd*
            libcairo-script-interpreter* libgthread* libgirepository* lib*.a'
        nd=$(notices_dir "$lib" "$dir")
        check_notices "$nd" $platform_libs
    else
        for d in etc var share/fontconfig share/xml; do
            [ ! -e "$dir/$d" ]; check $? "no $d in the prefix (fontconfig's configuration stays the system's)"
        done
        check_no_private_files
        for n in $platform_libs; do
            [ -n "$(ls -A "$dir/share/pharo-cairo-stack/licenses/$n" 2>/dev/null)" ]; check $? "licences of $n"
        done
        summaries=
        in_platform_libs cairo && summaries=cairo
        for n in $summaries freetype; do
            s=$dir/share/pharo-cairo-stack/meson-summaries/$n.txt
            test -s "$s"; check $? "meson summary of $n"
        done
        s=$dir/share/pharo-cairo-stack/meson-summaries/cairo.txt
        if [ -s "$s" ]; then
            for k in FreeType Fontconfig "PNG functions" PDF SVG PostScript; do
                grep -q "^ *$k *: YES" "$s"; check $? "cairo summary: $k YES"
            done
            for k in Xlib XCB Quartz "Win32 DWrite"; do
                grep -q "^ *$k *: NO" "$s"; check $? "cairo summary: $k NO"
            done
            grep -q "^ *lzo *: disabled" "$s" && grep -q "^ *symbol-lookup *: disabled" "$s"; check $? "cairo summary: lzo and symbol-lookup disabled (no GPL)"
        fi
        s=$dir/share/pharo-cairo-stack/meson-summaries/freetype.txt
        [ ! -s "$s" ] || { grep -q '^ *Zlib *: system' "$s"; check $? "FreeType summary: Zlib system"; }
        check_feature_summaries
        if has_feature poppler; then
            g=$(awk '$1 == "tool" && $2 == "gcc-install-dir" { print $3; exit }' "$dir/share/pharo-cairo-stack/manifest.txt" 2>/dev/null)
            case $g in */13) ok "manifest: the libstdc++ of GCC 13 ($g)" ;; *) fail "manifest: no tool gcc-install-dir .../13 (${g:-none})" ;; esac
        fi
        check_prefix_share
    fi
    [ -f "$cairo_f" ] && run_probe "$cairo_f"
    run_probe_text "$lib"
}

# Strings of the build in the libraries (Linux): .rodata and .data.rel.ro
# (meson's dead build RPATH in .dynstr is inert).  Allowed, beginning with the
# build prefix: libglib's <prefix>/share/locale (GLIB_LOCALE_DIR, NLS off) and
# <prefix>/lib (charset.alias), libgio's <prefix>/libexec/gio-launch-desktop
# (GDesktopAppInfo), libpango's <prefix>/lib/pango (deprecated
# pango_get_lib_subdirectory); never a path of the work directory (CFLAGS
# -ffile-prefix-map=<work>=/cairo-stack-build).
check_build_strings() {
    cb_t=${TMPDIR:-/tmp}/cairo-strings.$$
    cb_prefix=
    cb_work=
    if [ "$mode" = prefix ]; then
        cb_work=$(cat "$dir"/share/pharo-cairo-stack/meson-summaries/*.txt 2>/dev/null |
            sed -n 's/.*-ffile-prefix-map=\([^=]*\)=\/cairo-stack-build.*/\1/p' | head -n 1)
    fi
    for f in $reals; do
        b=$(basename "$f")
        { readelf -p .rodata "$f"; readelf -p .data.rel.ro "$f"; } 2>/dev/null |
            LC_ALL=C sed -n 's/^ *\[ *[0-9a-f]*\]  //p' > "$cb_t.$b"
        case $b in
            libgio-2.0.so.*)
                LC_ALL=C grep -a -q -x '/dev/null/gio-modules' "$cb_t.$b"; check $? "$b: GIO module directory /dev/null/gio-modules (none is scanned)"
                # the build prefix: the one of gio-launch-desktop (a bundle,
                # or a prefix that was moved)
                cb_prefix=$(LC_ALL=C sed -n 's|^\(/.*\)/libexec/gio-launch-desktop$|\1|p' "$cb_t.$b" | head -n 1) ;;
        esac
    done
    [ -n "$cb_prefix" ] || [ "$mode" = bundle ] || cb_prefix=$dir
    [ -n "$cb_prefix" ] || echo "    no build prefix known (a bundle without GLib): the work path check only"
    [ -z "$cb_prefix" ] || echo "    build prefix $cb_prefix"
    for f in $reals; do
        b=$(basename "$f")
        case $b in
            libglib-2.0.so.*) allowed="$cb_prefix/share/locale $cb_prefix/lib" ;;
            libgio-2.0.so.*) allowed="$cb_prefix/libexec/gio-launch-desktop" ;;
            libpango-1.0.so.*) allowed="$cb_prefix/lib/pango" ;;
            *) allowed= ;;
        esac
        bad=
        if [ -n "$cb_prefix" ]; then
            bad=$(LC_ALL=C awk -v p="$cb_prefix" -v a=" $allowed " 'index($0, p) && !index(a, " " $0 " ")' "$cb_t.$b" | sort -u | tr '\n' ' ')
        fi
        # a source path outside the prefix map: <work>/src/<name>-<version>/...
        w=$(LC_ALL=C grep -a -E '^/.*/src/[A-Za-z0-9_.+-]+-[0-9]+\.[0-9]' "$cb_t.$b" | LC_ALL=C grep -a -v '^/cairo-stack-build/' | sort -u | head -n 3 | tr '\n' ' ')
        [ -z "$cb_work" ] || w="$w$(LC_ALL=C grep -a -F "$cb_work" "$cb_t.$b" | sort -u | head -n 3 | tr '\n' ' ')"
        [ -z "$bad$w" ]; check $? "$b: no build path in .rodata/.data.rel.ro but the allowed ones${bad:+; prefix: $bad}${w:+; work: $w}"
        rm -f "$cb_t.$b"
    done
}

# A prefix carries no header or archive of the private static copies (they
# stay in <work>/static; <prefix>/include feeds the CairoPlugin's include path)
check_no_private_files() {
    left=$( (find "$dir/include" \( -name ffi.h -o -name pcre2.h -o -name jpeglib.h \) 2>/dev/null
             cd "$dir/lib" 2>/dev/null && ls -d libffi.a libpcre2-8.a libjpeg.a 2>/dev/null) | tr '\n' ' ')
    [ -z "$left" ]; check $? "no header or archive of libffi, pcre2 or libjpeg-turbo in the prefix${left:+: $left}"
}

# The configure summaries of the feature libraries (prefix; "poppler": only
# Poppler's, the one library built on Windows)
check_feature_summaries() {
    sd=$dir/share/pharo-cairo-stack
    if [ "${1:-}" != poppler ] && in_platform_libs glib; then
        s=$sd/meson-summaries/glib.txt
        test -s "$s"; check $? "meson summary of glib"
        # GLib's summary lists libmount and selinux on Linux only
        ks=sysprof; [ "$platform" = linux ] && ks="libmount selinux sysprof"
        for k in $ks; do
            grep -q "^ *$k *: false" "$s" 2>/dev/null; check $? "glib summary: $k false"
        done
    fi
    if [ "${1:-}" != poppler ] && has_feature pango; then
        s=$sd/meson-summaries/harfbuzz.txt
        test -s "$s"; check $? "meson summary of harfbuzz"
        for k in Glib ICU Graphite2 Subset "Cairo integration"; do
            grep -q "^ *$k *: NO" "$s" 2>/dev/null; check $? "harfbuzz summary: $k NO"
        done
        s=$sd/meson-summaries/pango.txt
        test -s "$s"; check $? "meson summary of pango"
        if [ "$platform" = linux ]; then
            for kv in "fontconfig enabled" "freetype enabled" "libthai disabled"; do
                grep -q "^ *${kv% *} *: ${kv#* }" "$s" 2>/dev/null; check $? "pango summary: $kv"
            done
        fi
    fi
    if has_feature poppler; then
        s=$sd/cmake-summaries/poppler.txt
        test -s "$s"; check $? "configure summary of poppler"
        fc=fontconfig; [ "$platform" = mingw ] && fc=win32
        for kv in "use nss3: no" "use gpg: no" "use curl: no" "use libopenjpeg2: no" "use lcms2: no" "use libtiff: no" \
                  "use brotli: no" "glib wrapper: yes" "cairo output: yes" "use libjpeg: yes" "font configuration: $fc"; do
            grep -q "^ *${kv%%:*}: *${kv#*: }\$" "$s" 2>/dev/null; check $? "poppler summary: $kv"
        done
    fi
}

# ================================================================ macOS
check_darwin() {
    if [ "$mode" = prefix ]; then lib=$dir/lib; nd=$dir
    elif [ -d "$dir/Contents/MacOS/Plugins" ]; then lib=$dir/Contents/MacOS/Plugins; nd=$dir/Contents/Resources
    else lib=$dir; nd=$dir; fi
    echo "check.sh: macOS stack in $lib"
    for t in otool nm; do command -v $t >/dev/null 2>&1 || die "needs $t"; done
    names=
    for n in cairo:libcairo.2.dylib freetype:libfreetype.6.dylib pixman:libpixman-1.0.dylib libpng:libpng16.16.dylib; do
        in_platform_libs "${n%%:*}" && names="$names ${n#*:}"
    done
    for l in cairo:libcairo.dylib freetype:libfreetype.dylib pixman:libpixman-1.dylib libpng:libpng16.dylib; do
        in_platform_libs "${l%%:*}" || continue
        test -L "$lib/${l#*:}" && [ -e "$lib/${l#*:}" ]; check $? "${l#*:} is a symlink (one image for dyld)"
    done
    in_platform_libs glib && names="$names libglib-2.0.0.dylib libgobject-2.0.0.dylib libgio-2.0.0.dylib libgmodule-2.0.0.dylib"
    has_feature pango && names="$names libfribidi.0.dylib libharfbuzz.0.dylib libpango-1.0.0.dylib libpangocairo-1.0.0.dylib"
    if has_feature poppler; then
        pop=
        for f in "$lib"/libpoppler.[0-9]*.dylib; do
            case $(basename "$f") in libpoppler.*.*.dylib) continue ;; esac
            [ -e "$f" ] && pop=$(basename "$f")
        done
        [ -n "$pop" ]; check $? "libpoppler.<N>.dylib exists${pop:+ ($pop)}"
        names="$names $pop libpoppler-glib.8.dylib libfontconfig.1.dylib"
    fi
    for n in $names; do
        [ -e "$lib/$n" ]; check $? "$n exists"
    done
    # @rpath/<one of these> is a dependency within the stack
    stack_re=$(echo $names | sed 's/\./\\./g; s/ /\\|/g')
    for n in $names; do
        f=$lib/$n
        [ -e "$f" ] || continue
        id=$(otool -D "$f" | sed -n 2p)
        [ "$id" = "@rpath/$n" ]; check $? "$n install name $id"
        deps=$(otool -L "$f" | sed -n '2,$p' | awk '{ print $1 }' | grep -v -x "$id")
        bad=$(echo "$deps" | grep -v -e "^@rpath/\\($stack_re\\)\$" -e '^/usr/lib/' -e '^/System/' | tr '\n' ' ')
        [ -z "$bad" ]; check $? "$n depends on the stack and the system only${bad:+ (not allowed: $bad)}"
        # GLib may use the system's libiconv; zlib and expat are static
        case $n in libcairo.*|libfreetype.*|libpixman-1.*|libpng16.*) sys_re='/usr/lib/lib(z|expat|bz2|iconv)\.' ;;
                   *) sys_re='/usr/lib/lib(z|expat|bz2)\.' ;; esac
        sysz=$(echo "$deps" | grep -E "$sys_re" | tr '\n' ' ')
        [ -z "$sysz" ]; check $? "$n links zlib statically, no system zlib, expat or bz2${sysz:+ ($sysz)}"
        otool -l "$f" | awk '/cmd LC_RPATH/ { r = 1 } r && /path / { print $2; r = 0 }' | grep -qx '@loader_path'; check $? "$n LC_RPATH @loader_path"
        m=$(otool -l "$f" | awk '/LC_BUILD_VERSION/ { b = 1 } b && $1 == "minos" { print $2; exit } /LC_VERSION_MIN_MACOSX/ { v = 1 } v && $1 == "version" { print $2; exit }')
        echo "    $n minos $m"
        if [ -n "$minos" ]; then version_le "$m" "$minos"; check $? "$n minos $m <= $minos"; fi
        if [ -n "$arch" ] && command -v lipo >/dev/null 2>&1; then
            [ "$(lipo -archs "$f")" = "$arch" ]; check $? "$n is $arch only ($(lipo -archs "$f"))"
        fi
        if command -v codesign >/dev/null 2>&1; then codesign -v "$f" 2>/dev/null; check $? "$n codesign -v"; fi
        # one FreeType; the private static copies hidden
        case $n in libfreetype.*) ;; *)
            c=$(nm -gU "$f" | awk '$3 ~ /^_FT_/' | wc -l | tr -d ' ')
            [ "$c" = 0 ]; check $? "$n defines no FT_ symbol ($c)" ;;
        esac
        p=$(nm -gU "$f" | awk '{ sub(/^_/, "", $3); print $3 }' | grep -E "$private_symbol_re" | sort -u | tr '\n' ' ')
        [ -z "$p" ]; check $? "$n: no symbol of a private copy (ffi_ pcre2_ jpeg_ jinit_ jcopy_, zlib, XML_)${p:+: $p}"
    done
    cairo_f=$lib/libcairo.2.dylib
    t=${TMPDIR:-/tmp}/cairo-exports.$$
    if [ -e "$cairo_f" ]; then
        otool -L "$cairo_f" | grep -q '@rpath/libfreetype.6.dylib'; check $? "libcairo loads @rpath/libfreetype.6.dylib"
        nm -gU "$cairo_f" | awk '{ sub(/^_/, "", $3); print $3 }' > "$t"
        check_exports "libcairo (cairo_quartz_surface_create included)" "$t" "$(basename "$symbols")" extra-cairo
    fi
    for n in libpangocairo-1.0.0.dylib libpoppler-glib.8.dylib; do
        echo " $names " | grep -q " $n " || continue
        c=$(otool -L "$lib/$n" | sed -n '2,$p' | awk '{ print $1 }' | grep '/libcairo[.-]' | tr '\n' ' ')
        [ "$c" = "@rpath/libcairo.2.dylib " ]; check $? "$n loads cairo only as @rpath/libcairo.2.dylib ($c)"
    done
    if has_feature pango; then
        nm -u "$lib/libpango-1.0.0.dylib" "$lib/libpangocairo-1.0.0.dylib" 2>/dev/null | grep -q '_hb_coretext_font_create$'
        check $? "Pango shapes CoreText fonts through HarfBuzz (imports hb_coretext_font_create)"
        check_absent "no pangoft2 (CoreText)" "$lib" 'libpangoft2*'
    fi
    for e in glib.txt:libglib-2.0.0.dylib gobject.txt:libgobject-2.0.0.dylib harfbuzz.txt:libharfbuzz.0.dylib \
             pango.txt:libpango-1.0.0.dylib pangocairo.txt:libpangocairo-1.0.0.dylib poppler-glib.txt:libpoppler-glib.8.dylib; do
        n=${e#*:}
        echo " $names " | grep -q " $n " && [ -e "$lib/$n" ] || continue
        nm -gU "$lib/$n" | awk '{ sub(/^_/, "", $3); print $3 }' > "$t"
        check_exports "$n" "$t" "${e%%:*}"
    done
    rm -f "$t"
    fonts_conf=
    if has_feature poppler; then
        # fontconfig with the macOS font directories, the bundle's fonts.conf (D5)
        # (FC_DEFAULT_FONTS, the <dir> entries of its built-in configuration:
        # "\t<dir>/System/Library/Fonts</dir>\n\t<dir>...", one C string)
        f=$lib/libfontconfig.1.dylib
        fc_dirs=$(strings "$f" 2>/dev/null | grep -o '<dir>[^<]*</dir>')
        echo "$fc_dirs" | grep -qx '<dir>/System/Library/Fonts</dir>' \
            && ! echo "$fc_dirs" | grep -q -e '<dir>/usr/local/share/fonts</dir>' -e '<dir>/usr/share/fonts</dir>'
        check $? "libfontconfig.1.dylib has the font directory /System/Library/Fonts compiled in, not /usr/share/fonts or /usr/local/share/fonts"
        if [ "$mode" = bundle ]; then
            fonts_conf=$nd/fontconfig/fonts.conf
            test -s "$fonts_conf"; check $? "fontconfig/fonts.conf in $nd (LibPoppler loads it)"
        elif [ -s "$here/../../packaging/cairo-stack/fonts.conf" ]; then
            fonts_conf=$(cd "$here/../../packaging/cairo-stack" && pwd)/fonts.conf
        fi
    fi
    check_absent "no library of a feature that is not bundled" "$lib" "$(unbundled_globs)"
    if [ "$mode" = bundle ]; then
        check_absent "no dropped library in the bundle" "$lib" 'libbz2* libz.* libexpat* libcairo-script-interpreter*
            libgthread* libgirepository* lib*.a'
        check_notices "$(notices_dir "$nd" "$dir")" $platform_libs
    else
        check_no_private_files
        check_feature_summaries
        check_prefix_share
    fi
    [ -e "$cairo_f" ] && run_probe "$cairo_f"
    run_probe_text "$lib" "$fonts_conf"
}

# ================================================================ Windows
is_system_dll() {
    case $(echo "$1" | tr 'A-Z' 'a-z') in
        api-ms-win-*|ext-ms-win-*|kernel32.dll|kernelbase.dll|user32.dll|gdi32.dll|msimg32.dll|ole32.dll|oleaut32.dll|\
        advapi32.dll|shell32.dll|shlwapi.dll|ws2_32.dll|dnsapi.dll|iphlpapi.dll|userenv.dll|bcrypt.dll|crypt32.dll|\
        secur32.dll|ncrypt.dll|dwrite.dll|d2d1.dll|d3d11.dll|dxgi.dll|windowscodecs.dll|usp10.dll|comdlg32.dll|\
        comctl32.dll|winmm.dll|version.dll|setupapi.dll|rpcrt4.dll|ntdll.dll|psapi.dll|dbghelp.dll|msvcrt.dll|\
        ucrtbase.dll|imm32.dll|uxtheme.dll|dwmapi.dll|winspool.drv|mpr.dll|netapi32.dll|wtsapi32.dll|\
        bcryptprimitives.dll|cfgmgr32.dll|propsys.dll|shcore.dll|mscms.dll|opengl32.dll|hid.dll) return 0 ;;
    esac
    return 1
}

# pe_exports <dll>: the names of its export table (LLVM's or GNU's objdump -p)
pe_exports() {
    "$od" -p "$1" | tr -d '\r' | awk '
        /^Export Table:/ || /\[Ordinal\/Name Pointer\] Table/ { e = 1; next }
        e && /^[ \t]*\[ *[0-9]+\] / { print $NF; next }
        e && /^[ \t]+[0-9]+[ \t]+0x[0-9a-fA-F]+[ \t]+[A-Za-z_]/ { print $3; next }
        e && NF == 0 { e = 0 }'
}

# msys2_dlls <features>: the msys2-dll names of $env whose features (5th
# field, default core) meet "core <features>"
msys2_dlls() {
    awk -v e="$env" -v f="core $1" '
        $1 == "msys2-dll" && $2 == e {
            n = split(NF >= 5 ? $5 : "core", fs, ",")
            for (i = 1; i <= n; i++) if (index(" " f " ", " " fs[i] " ")) { print $3; break }
        }' "$pins"
}

check_mingw() {
    if [ -z "$env" ]; then
        case $arch in
            x86_64) env=ucrt64 ;;
            arm64|aarch64) env=clangarm64 ;;
            *) case ${MSYSTEM:-} in UCRT64) env=ucrt64 ;; CLANGARM64) env=clangarm64 ;; *) die "--env or --arch" ;; esac ;;
        esac
    fi
    case $env in ucrt64) want=x86-64 ;; clangarm64) want=arm64 ;; *) die "--env $env" ;; esac
    if [ "$mode" = prefix ]; then lib=$dir/bin; else lib=$dir; fi
    echo "check.sh: Windows ($env) stack in $lib"
    dlls=$(msys2_dlls "$features")
    [ -n "$dlls" ] || die "$pins has no msys2-dll line for $env"
    core_dlls=$(msys2_dlls "")
    # The DLLs the stack lists with their SHA256: dlls.txt of a prefix
    # (<dll> <kind> <package> <sha256> <features>), the dll lines of a
    # bundle's CAIRO-STACK.txt (dll <dll> <kind> <package> <sha256> <features>)
    nd=$(notices_dir "$lib" "$dir")
    listed=${TMPDIR:-/tmp}/cairo-dlls.$$
    if [ "$mode" = prefix ]; then
        awk 'NF == 5 && $1 !~ /^#/ { print $1, $2, $3, $4, $5 }' "$dir/share/pharo-cairo-stack/dlls.txt" 2>/dev/null > "$listed"
    else
        awk '$1 == "dll" && NF == 6 { print $2, $3, $4, $5, $6 }' "$nd/CAIRO-STACK.txt" 2>/dev/null > "$listed"
    fi
    # The DLLs built here from a source pin (poppler) of the checked features
    built=$(awk -v f="$features" '$2 == "built" { n = split($5, fs, ","); for (i = 1; i <= n; i++) if (index(" " f " ", " " fs[i] " ")) { print $1; break } }' "$listed")
    if has_feature poppler && awk '!/^#/ && NF >= 7 && $1 == "poppler" && $6 ~ /(^|,)mingw(,|$)/ { f = 1 } END { exit !f }' "$pins"; then
        echo "$built" | grep -q '^libpoppler-glib-[0-9]*\.dll$'; check $? "poppler built from source: libpoppler-glib-<N>.dll listed as built ($(echo $built))"
    fi
    od=
    for t in ${OBJDUMP:-} llvm-objdump objdump llvm-objdump-21 llvm-objdump-20 llvm-objdump-19 llvm-objdump-18; do
        command -v "$t" >/dev/null 2>&1 || continue
        if [ -f "$lib/libcairo-2.dll" ] && "$t" -p "$lib/libcairo-2.dll" 2>/dev/null | grep -q 'DLL Name:'; then od=$t; break; fi
    done
    [ -n "$od" ] || { fail "no objdump reads $lib/libcairo-2.dll (set OBJDUMP)"; rm -f "$listed"; return; }
    allowed=$(echo $dlls $built | tr 'A-Z' 'a-z')
    core_lc=$(echo $core_dlls | tr 'A-Z' 'a-z')
    denied=
    for d in $dlls $built; do
        f=$lib/$d
        case " $(echo $built) " in *" $d "*) what="built from source" ;; *) what="the pinned closure, pins.txt msys2-dll" ;; esac
        [ -f "$f" ]; check $? "$d is there ($what)"
        [ -f "$f" ] || continue
        fmt=$("$od" -f "$f" 2>/dev/null | sed -n 's/.*file format //p' | head -n 1)
        case "$want:$fmt" in
            x86-64:*x86-64*|arm64:*arm64*|arm64:*aarch64*) ok "$d is $fmt" ;;
            *) fail "$d is $fmt, not $want" ;;
        esac
        bad=
        for i in $("$od" -p "$f" | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p' | tr -d '\r'); do
            il=$(echo "$i" | tr 'A-Z' 'a-z')
            if denied_dll "$i"; then case " $core_lc " in *" $il "*) ;; *) denied="$denied $d:$i" ;; esac; fi
            is_system_dll "$i" && continue
            case " $allowed " in *" $il "*) continue ;; esac
            bad="$bad $i"
        done
        [ -z "$bad" ]; check $? "$d imports the pinned closure, the built DLLs and Windows only${bad:+ (not allowed:$bad)}"
    done
    # The deny-list: a full Poppler's network, crypto, colour and image stacks,
    # Qt (brotli is allowed where the core closure has it: MSYS2's FreeType).
    # Imported above; and in a bundle, not there at all (a prefix's bin/ has
    # every DLL of the extracted packages)
    for f in "$lib"/*.dll "$lib"/*.DLL; do
        [ "$mode" = bundle ] && [ -f "$f" ] || continue
        d=$(basename "$f")
        denied_dll "$d" || continue
        case " $core_lc " in *" $(echo "$d" | tr 'A-Z' 'a-z') "*) continue ;; esac
        denied="$denied $d"
    done
    [ -z "$denied" ]; check $? "no denied DLL there or imported (nss, nspr, curl, gpgme, lcms2, openjp2, tiff, brotli, Qt)${denied:+:$denied}"
    t=${TMPDIR:-/tmp}/cairo-exports.$$
    cairo_f=$lib/libcairo-2.dll
    if [ -f "$cairo_f" ]; then
        pe_exports "$cairo_f" > "$t"
        check_exports libcairo-2.dll "$t" "$(basename "$symbols")" extra-cairo
        n=$(pe_exports "$cairo_f" | grep -c '^FT_')
        [ "$n" = 0 ]; check $? "libcairo-2.dll exports no FT_ symbol ($n)"
    fi
    pg=$(echo "$built" | grep '^libpoppler-glib-[0-9]*\.dll$' | head -n 1)
    for e in glib.txt:libglib-2.0-0.dll:glib gobject.txt:libgobject-2.0-0.dll:glib harfbuzz.txt:libharfbuzz-0.dll:pango \
             pango.txt:libpango-1.0-0.dll:pango pangocairo.txt:libpangocairo-1.0-0.dll:pango "poppler-glib.txt:$pg:poppler"; do
        l=${e%%:*}; r=${e#*:}; d=${r%%:*}; need=${r#*:}
        if [ "$need" = glib ]; then [ -n "$features" ] || continue; else has_feature "$need" || continue; fi
        [ -n "$d" ] && [ -f "$lib/$d" ] || continue
        pe_exports "$lib/$d" > "$t"
        check_exports "$d" "$t" "$l"
    done
    rm -f "$t"
    v=$(awk -v e="$env" '$1 == "msys2" && $2 == e && $4 ~ /-cairo$/ { print $5 }' "$pins")
    case $v in "$cairo_pin"-*) ok "MSYS2's cairo $v is cairo $cairo_pin" ;; *) fail "MSYS2's cairo $v is not cairo $cairo_pin" ;; esac
    # The DLLs are the pinned packages' own (not another MSYS2 package's copy
    # of the same name), or the ones built here
    [ -s "$listed" ]; check $? "$([ "$mode" = prefix ] && echo dlls.txt || echo CAIRO-STACK.txt) lists the DLLs with their SHA256"
    while read -r d k p h fs; do
        [ -f "$lib/$d" ] || continue
        if [ "$(sha256 "$lib/$d")" = "$h" ]; then ok "$d is the $k DLL listed ($p)"
        elif [ "$k" = runtime ] && [ "$mode" = bundle ]; then echo "    $d: the toolchain's, not the pinned package's (runtime: allowed)"
        else fail "$d is not the listed $p's (another copy of that name)"; fi
    done < "$listed"
    # D4: the VM's FFI runtime is the pinned MSYS2 libffi, whatever the features
    awk '$1 == "libffi-8.dll" && $3 ~ /-libffi$/ { f = 1 } END { exit !f }' "$listed" && [ -f "$lib/libffi-8.dll" ] &&
        [ "$(sha256 "$lib/libffi-8.dll")" = "$(awk '$1 == "libffi-8.dll" { print $4; exit }' "$listed")" ]
    check $? "libffi-8.dll is the pinned libffi package's (the VM's FFI runtime)"
    rm -f "$listed"
    if [ "$mode" = bundle ]; then
        # what no feature bundled must be absent
        gone=
        for d in $(msys2_dlls "pango poppler"); do
            echo " $(echo $dlls) " | grep -q " $d " && continue
            fs=$(awk -v e="$env" -v d="$d" '$1 == "msys2-dll" && $2 == e && $3 == d { print (NF >= 5 ? $5 : "core"); exit }' "$pins")
            keep=no
            for x in $(echo "$fs" | tr ',' ' '); do declares_feature "$x" && keep=yes; done
            [ "$keep" = yes ] || gone="$gone $d"
        done
        unbundled poppler && gone="$gone libpoppler*"
        check_absent "no DLL of a feature that is not bundled" "$lib" "$gone"
        check_absent "no DLL outside the closure that cairo's or glib's package carries" "$lib" 'liblzo2*
            libcairo-script-interpreter* libcairo-gobject* libgthread* libgirepository*'
        lic="cairo freetype"
        has_feature pango && lic="$lic pango"
        has_feature poppler && lic="$lic poppler"
        check_notices "$nd" $lic
    else
        check_prefix_share
        test -f "$dir/include/cairo/cairo.h" && test -f "$dir/include/cairo/cairo-ft.h" && test -f "$dir/include/freetype2/ft2build.h" && test -f "$dir/include/fontconfig/fontconfig.h"; check $? "headers: cairo/cairo.h, cairo/cairo-ft.h, freetype2/ft2build.h, fontconfig/fontconfig.h"
        h=$(awk '$1 == "#define" && $2 ~ /^CAIRO_VERSION_(MAJOR|MINOR|MICRO)$/ { v[$2] = $3 }
            END { print v["CAIRO_VERSION_MAJOR"] "." v["CAIRO_VERSION_MINOR"] "." v["CAIRO_VERSION_MICRO"] }' "$dir/include/cairo/cairo-version.h" 2>/dev/null)
        [ "$h" = "$cairo_pin" ]; check $? "cairo-version.h says $h"
        if has_feature poppler && [ -n "$built" ]; then
            check_feature_summaries poppler
        fi
    fi
    echo "check.sh: toy text on Windows: ci-load.st's CairoToy line, Pango and Poppler: ci-text.st (CI)"
}

case $platform in
    linux) check_linux ;;
    darwin) check_darwin ;;
    mingw) check_mingw ;;
esac
if [ "$failures" -gt 0 ]; then
    echo "check.sh: $failures FAILED"
    exit 1
fi
echo "check.sh: all passed"
