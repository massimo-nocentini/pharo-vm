#!/bin/sh
# shellcheck disable=SC2319,SC2018,SC2019,SC2012,SC2086,SC2016  # "[ ... ]; check $?" is the idiom; DLL names are ASCII; word splitting of lists is wanted
# Assertions on a cairo stack (docs/CairoStack.md, "What check.sh asserts"):
# on a prefix that build.sh or fetch-msys2.sh made, or on a VM bundle that
# carries it (the build tree's VM directory, or an unzipped release zip).
#
#   check.sh (--prefix <dir> | --bundle <dir>) --platform linux|darwin|mingw
#            [--arch <arch>] [--glibc-max <x.y>] [--minos <x.y>] [--env ucrt64|clangarm64]
#            [--pins <file>] [--symbols <file>] [--no-probe]
#
#   --prefix   <dir>/lib (Linux, macOS) or <dir>/bin (Windows), and the
#              prefix's share/pharo-cairo-stack (manifest, licences)
#   --bundle   Linux: <dir>/lib if it has libcairo.so.2, else <dir>; macOS:
#              <dir>/Contents/MacOS/Plugins (notices in Contents/Resources) if
#              <dir> is an app, else <dir>; Windows: <dir>.  The notices
#              (LICENSE.*, THIRD-PARTY-NOTICES.cairo-stack.txt, CAIRO-STACK.txt)
#              are looked for next to the libraries, then in <dir>.
#   --glibc-max  Linux: fail when a stack library needs a newer GLIBC_ symbol
#              version (the highest one is always reported)
#   --minos    macOS: fail when a library's minimum macOS is newer
#   --no-probe do not compile and run probe.c (Linux and macOS: toy text)
#
# It prints "ok <what>" or "FAIL <what>" per assertion, and exits 1 on any FAIL.
# Tools: readelf, nm, objdump (Linux); otool, nm, lipo, codesign (macOS);
# objdump or llvm-objdump (Windows); a C compiler for the probe.
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
minos=
env=
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
        --minos) minos=$2; shift 2 ;;
        --env) env=$2; shift 2 ;;
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

pin_version() {
    awk -v n="$1" '$1 == n && $1 !~ /^#/ { print $2; exit }' "$pins"
}
cairo_pin=$(pin_version cairo)
[ -n "$cairo_pin" ] || die "no cairo line in $pins"
# The libraries of this platform, from pins.txt
platform_libs=$(awk -v p="$platform" '
    /^#/ || NF < 7 || $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
    { n = split($6, ps, ","); ok = ($6 == "all"); for (i = 1; i <= n; i++) if (ps[i] == p) ok = 1; if (ok) print $1 }' "$pins")

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# version_le <a> <b>: a <= b, for dotted numbers
version_le() {
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -n 1)" = "$1" ]
}

# The cairo functions the images bind (cmake/emscripten/ffi/symbols/cairo.txt):
# every name, but those of the plain "# unavailable:" sections; the Quartz
# section's name only on macOS.  ("unavailable unless" sections are wasm
# options that the native stacks always have.)
required_symbols() {
    [ -f "$symbols" ] || return 0
    awk -v p="$platform" '
        /^#[ \t]*unavailable[ \t]+unless/ { skip = 0; next }
        /^#[ \t]*unavailable/ { skip = !(p == "darwin" && $0 ~ /Quartz/); next }
        /^#/ || NF == 0 { next }
        !skip { print $1 }' "$symbols"
}

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
}

check_prefix_share() {
    share=$dir/share/pharo-cairo-stack
    test -s "$share/manifest.txt"; check $? "manifest $share/manifest.txt"
    if [ -s "$share/manifest.txt" ]; then
        m_pins=$(awk '$1 == "pins-sha256" { print $2 }' "$share/manifest.txt")
        [ "$m_pins" = "$(sha256 "$pins")" ]; check $? "manifest pins-sha256 is that of $pins (else the prefix is stale)"
        m_platform=$(awk '$1 == "platform" { print $2 }' "$share/manifest.txt")
        [ "$m_platform" = "$platform" ]; check $? "manifest platform $m_platform"
    fi
    test -s "$share/CAIRO-STACK.txt"; check $? "index $share/CAIRO-STACK.txt"
}

run_probe() {
    # run_probe <path of libcairo>
    [ "$probe" = yes ] || { echo "skip probe (--no-probe)"; return 0; }
    cc=${CC:-}
    if [ -z "$cc" ]; then for t in clang cc gcc; do command -v $t >/dev/null 2>&1 && { cc=$t; break; }; done; fi
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

    # name: real file (or glob), links
    set -- "cairo $cairo_real libcairo.so.2 libcairo.so" \
           "freetype libfreetype.so.6.* libfreetype.so.6 libfreetype.so" \
           "pixman libpixman-1.so.$pix libpixman-1.so.0 libpixman-1.so" \
           "libpng libpng16.so.16.$png_minor.0 libpng16.so.16 libpng16.so" \
           "fontconfig libfontconfig.so.1.* libfontconfig.so.1 libfontconfig.so"
    reals=
    for entry in "$@"; do
        set -- $entry
        name=$1 real=$2; shift 2
        echo "$platform_libs" | grep -qx "$name" || continue
        found=
        for f in $lib/$real; do [ -f "$f" ] && [ ! -L "$f" ] && found=$f; done
        [ -n "$found" ]; check $? "$name: $(basename "${found:-$real}") is a file"
        [ -n "$found" ] && reals="$reals $found"
        for l in "$@"; do
            test -L "$lib/$l" && [ -e "$lib/$l" ]; check $? "$name: $l is a symlink to it"
        done
    done
    soname_of() { readelf -d "$1" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p'; }
    stack_sonames=
    for f in $reals; do stack_sonames="$stack_sonames $(soname_of "$f")"; done

    max_glibc=0; max_lib=
    for f in $reals; do
        b=$(basename "$f")
        needed=$(readelf -d "$f" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | tr '\n' ' ')
        bad=
        for n in $needed; do
            case " $stack_sonames " in *" $n "*) continue ;; esac
            case $n in libc.so.6|libm.so.6|libpthread.so.0|libdl.so.2|librt.so.1|ld-linux*.so.*) continue ;; esac
            bad="$bad $n"
        done
        [ -z "$bad" ]; check $? "$b NEEDED within the stack and glibc: $needed${bad:+ (not allowed:$bad)}"
        rpath=$(readelf -d "$f" | sed -n 's/.*(RPATH).*\[\(.*\)\]/\1/p')
        runpath=$(readelf -d "$f" | sed -n 's/.*(RUNPATH).*\[\(.*\)\]/\1/p')
        [ "$rpath" = '$ORIGIN' ] && [ -z "$runpath" ]; check $? "$b RPATH '$rpath' = \$ORIGIN, no RUNPATH${runpath:+ ($runpath)}"
        g=$(objdump -T "$f" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
        if [ -n "$g" ] && ! version_le "$g" "$max_glibc"; then max_glibc=$g; max_lib=$b; fi
    done
    cairo_f=$lib/$cairo_real
    fc_f=$(ls $lib/libfontconfig.so.1.* 2>/dev/null | head -n 1)
    for f in "$cairo_f" $fc_f; do
        [ -f "$f" ] || continue
        b=$(basename "$f")
        readelf -d "$f" | grep -q 'NEEDED.*\[libfreetype\.so\.6\]'; check $? "$b NEEDS libfreetype.so.6 (one FreeType with the image)"
        n=$(nm -D --defined-only "$f" | awk '$3 ~ /^FT_/' | wc -l | tr -d ' ')
        [ "$n" = 0 ]; check $? "$b defines no FT_ symbol ($n)"
        n=$(nm -D --defined-only "$f" | awk '$3 ~ /^(deflate|inflate|crc32|adler32|compress|uncompress|zlibVersion|XML_)/' | wc -l | tr -d ' ')
        [ "$n" = 0 ]; check $? "$b exports no zlib or expat symbol ($n)"
    done
    # The highest glibc symbol version (decision U1: Ubuntu 24.04's 2.39 at most)
    echo "check.sh: highest GLIBC_ version needed: $max_glibc ($max_lib)"
    echo "glibc-max $max_glibc"
    if [ -n "$glibc_max" ]; then
        version_le "$max_glibc" "$glibc_max"; check $? "GLIBC_$max_glibc <= GLIBC_$glibc_max"
    fi
    if [ -f "$cairo_f" ]; then
        nm -D --defined-only "$cairo_f" | awk '{ print $3 }' | sort -u > "${TMPDIR:-/tmp}/cairo-exports.$$"
        missing=$(required_symbols | sort -u | comm -23 - "${TMPDIR:-/tmp}/cairo-exports.$$" | tr '\n' ' ')
        total=$(required_symbols | wc -l | tr -d ' ')
        [ -z "$missing" ]; check $? "libcairo exports the $total functions the images bind${missing:+; missing: $missing}"
        rm -f "${TMPDIR:-/tmp}/cairo-exports.$$"
    fi
    # Nothing that nothing loads, nothing of a licence we do not want
    left=$(cd "$lib" && ls -d libharfbuzz* libbz2* libexpat* libX11* libXext* liblzo2* libbfd* libglib* libcairo-script-interpreter* 2>/dev/null | tr '\n' ' ')
    if [ "$mode" = bundle ]; then
        [ -z "$left" ]; check $? "no dropped library in the bundle${left:+: $left}"
        nd=$(notices_dir "$lib" "$dir")
        check_notices "$nd" $platform_libs
    else
        for d in etc var share/fontconfig share/xml; do
            [ ! -e "$dir/$d" ]; check $? "no $d in the prefix (fontconfig's configuration stays the system's)"
        done
        for n in $platform_libs; do
            [ -n "$(ls -A "$dir/share/pharo-cairo-stack/licenses/$n" 2>/dev/null)" ]; check $? "licences of $n"
        done
        for n in cairo freetype; do
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
        check_prefix_share
    fi
    [ -f "$cairo_f" ] && run_probe "$cairo_f"
}

# ================================================================ macOS
check_darwin() {
    if [ "$mode" = prefix ]; then lib=$dir/lib; nd=$dir
    elif [ -d "$dir/Contents/MacOS/Plugins" ]; then lib=$dir/Contents/MacOS/Plugins; nd=$dir/Contents/Resources
    else lib=$dir; nd=$dir; fi
    echo "check.sh: macOS stack in $lib"
    for t in otool nm; do command -v $t >/dev/null 2>&1 || die "needs $t"; done
    names="libcairo.2.dylib libfreetype.6.dylib libpixman-1.0.dylib libpng16.16.dylib"
    for n in $names; do
        [ -e "$lib/$n" ]; check $? "$n exists"
    done
    for l in libcairo.dylib libfreetype.dylib libpixman-1.dylib libpng16.dylib; do
        test -L "$lib/$l" && [ -e "$lib/$l" ]; check $? "$l is a symlink (one image for dyld)"
    done
    for n in $names; do
        f=$lib/$n
        [ -e "$f" ] || continue
        id=$(otool -D "$f" | sed -n 2p)
        [ "$id" = "@rpath/$n" ]; check $? "$n install name $id"
        deps=$(otool -L "$f" | sed -n '2,$p' | awk '{ print $1 }' | grep -v -x "$id")
        bad=$(echo "$deps" | grep -v -e '^@rpath/lib\(cairo\.2\|freetype\.6\|pixman-1\.0\|png16\.16\)\.dylib$' -e '^/usr/lib/' -e '^/System/' | tr '\n' ' ')
        [ -z "$bad" ]; check $? "$n depends on the stack and the system only${bad:+ (not allowed: $bad)}"
        sysz=$(echo "$deps" | grep -E '/usr/lib/lib(z|expat|bz2|iconv)\.' | tr '\n' ' ')
        [ -z "$sysz" ]; check $? "$n links zlib statically, no system zlib, expat, bz2 or iconv${sysz:+ ($sysz)}"
        otool -l "$f" | awk '/cmd LC_RPATH/ { r = 1 } r && /path / { print $2; r = 0 }' | grep -qx '@loader_path'; check $? "$n LC_RPATH @loader_path"
        m=$(otool -l "$f" | awk '/LC_BUILD_VERSION/ { b = 1 } b && $1 == "minos" { print $2; exit } /LC_VERSION_MIN_MACOSX/ { v = 1 } v && $1 == "version" { print $2; exit }')
        echo "    $n minos $m"
        if [ -n "$minos" ]; then version_le "$m" "$minos"; check $? "$n minos $m <= $minos"; fi
        if [ -n "$arch" ] && command -v lipo >/dev/null 2>&1; then
            [ "$(lipo -archs "$f")" = "$arch" ]; check $? "$n is $arch only ($(lipo -archs "$f"))"
        fi
        if command -v codesign >/dev/null 2>&1; then codesign -v "$f" 2>/dev/null; check $? "$n codesign -v"; fi
    done
    cairo_f=$lib/libcairo.2.dylib
    if [ -e "$cairo_f" ]; then
        n=$(nm -gU "$cairo_f" | awk '$3 ~ /^_FT_/' | wc -l | tr -d ' ')
        [ "$n" = 0 ]; check $? "libcairo defines no FT_ symbol ($n)"
        otool -L "$cairo_f" | grep -q '@rpath/libfreetype.6.dylib'; check $? "libcairo loads @rpath/libfreetype.6.dylib"
        nm -gU "$cairo_f" | awk '{ sub(/^_/, "", $3); print $3 }' | sort -u > "${TMPDIR:-/tmp}/cairo-exports.$$"
        missing=$(required_symbols | sort -u | comm -23 - "${TMPDIR:-/tmp}/cairo-exports.$$" | tr '\n' ' ')
        [ -z "$missing" ]; check $? "libcairo exports the functions the images bind, cairo_quartz_surface_create included${missing:+; missing: $missing}"
        rm -f "${TMPDIR:-/tmp}/cairo-exports.$$"
    fi
    left=$(cd "$lib" && ls -d libharfbuzz* libbz2* libfontconfig* libz.* libexpat* 2>/dev/null | tr '\n' ' ')
    if [ "$mode" = bundle ]; then
        [ -z "$left" ]; check $? "no dropped library in the bundle${left:+: $left}"
        check_notices "$(notices_dir "$nd" "$dir")" $platform_libs
    else
        check_prefix_share
    fi
    [ -e "$cairo_f" ] && run_probe "$cairo_f"
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
    dlls=$(awk -v e="$env" '$1 == "msys2-dll" && $2 == e { print $3 }' "$pins")
    [ -n "$dlls" ] || die "$pins has no msys2-dll line for $env"
    od=
    for t in ${OBJDUMP:-} llvm-objdump objdump llvm-objdump-21 llvm-objdump-20 llvm-objdump-19 llvm-objdump-18; do
        command -v "$t" >/dev/null 2>&1 || continue
        if [ -f "$lib/libcairo-2.dll" ] && "$t" -p "$lib/libcairo-2.dll" 2>/dev/null | grep -q 'DLL Name:'; then od=$t; break; fi
    done
    [ -n "$od" ] || { fail "no objdump reads $lib/libcairo-2.dll (set OBJDUMP)"; return; }
    allowed=$(echo "$dlls" | tr 'A-Z' 'a-z' | tr '\n' ' ')
    for d in $dlls; do
        f=$lib/$d
        [ -f "$f" ]; check $? "$d is there (the pinned closure, pins.txt msys2-dll)"
        [ -f "$f" ] || continue
        fmt=$("$od" -f "$f" 2>/dev/null | sed -n 's/.*file format //p' | head -n 1)
        case "$want:$fmt" in
            x86-64:*x86-64*|arm64:*arm64*|arm64:*aarch64*) ok "$d is $fmt" ;;
            *) fail "$d is $fmt, not $want" ;;
        esac
        bad=
        for i in $("$od" -p "$f" | sed -n 's/^[[:space:]]*DLL Name:[[:space:]]*//p' | tr -d '\r'); do
            is_system_dll "$i" && continue
            case " $allowed " in *" $(echo "$i" | tr 'A-Z' 'a-z') "*) continue ;; esac
            bad="$bad $i"
        done
        [ -z "$bad" ]; check $? "$d imports the pinned closure and Windows only${bad:+ (not allowed:$bad)}"
    done
    cairo_f=$lib/libcairo-2.dll
    if [ -f "$cairo_f" ]; then
        pe_exports "$cairo_f" | sort -u > "${TMPDIR:-/tmp}/cairo-exports.$$"
        missing=$(required_symbols | sort -u | comm -23 - "${TMPDIR:-/tmp}/cairo-exports.$$" | tr '\n' ' ')
        [ -z "$missing" ]; check $? "libcairo-2.dll exports the functions the images bind${missing:+; missing: $missing}"
        n=$(pe_exports "$cairo_f" | grep -c '^FT_')
        [ "$n" = 0 ]; check $? "libcairo-2.dll exports no FT_ symbol ($n)"
        rm -f "${TMPDIR:-/tmp}/cairo-exports.$$"
    fi
    v=$(awk -v e="$env" '$1 == "msys2" && $2 == e && $4 ~ /-cairo$/ { print $5 }' "$pins")
    case $v in "$cairo_pin"-*) ok "MSYS2's cairo $v is cairo $cairo_pin" ;; *) fail "MSYS2's cairo $v is not cairo $cairo_pin" ;; esac
    left=$(cd "$lib" && ls -d liblzo2* libcairo-script-interpreter* libcairo-gobject* 2>/dev/null | tr '\n' ' ')
    [ -z "$left" ] || [ "$mode" = prefix ]; check $? "no DLL outside the closure that cairo's package carries${left:+: $left}"
    if [ "$mode" = bundle ]; then
        nd=$(notices_dir "$lib" "$dir")
        check_notices "$nd" cairo freetype
        # The DLLs are the pinned packages' own (not another MSYS2 package's
        # copy of the same name): CAIRO-STACK.txt has their SHA256
        if [ -f "$nd/CAIRO-STACK.txt" ]; then
            awk '$1 == "dll" && NF == 5 { print $2, $3, $5 }' "$nd/CAIRO-STACK.txt" > "${TMPDIR:-/tmp}/cairo-dlls.$$"
            [ -s "${TMPDIR:-/tmp}/cairo-dlls.$$" ]; check $? "CAIRO-STACK.txt lists the DLLs with their SHA256"
            while read -r d k h; do
                [ -f "$lib/$d" ] || continue
                if [ "$(sha256 "$lib/$d")" = "$h" ]; then ok "$d is the pinned package's"
                elif [ "$k" = runtime ]; then echo "    $d: the toolchain's, not the pinned package's (runtime: allowed)"
                else fail "$d is not the pinned package's (another copy of that name was bundled)"; fi
            done < "${TMPDIR:-/tmp}/cairo-dlls.$$"
            rm -f "${TMPDIR:-/tmp}/cairo-dlls.$$"
        fi
    else
        check_prefix_share
        test -f "$dir/include/cairo/cairo.h" && test -f "$dir/include/cairo/cairo-ft.h" && test -f "$dir/include/freetype2/ft2build.h" && test -f "$dir/include/fontconfig/fontconfig.h"; check $? "headers: cairo/cairo.h, cairo/cairo-ft.h, freetype2/ft2build.h, fontconfig/fontconfig.h"
        h=$(awk '$1 == "#define" && $2 ~ /^CAIRO_VERSION_(MAJOR|MINOR|MICRO)$/ { v[$2] = $3 }
            END { print v["CAIRO_VERSION_MAJOR"] "." v["CAIRO_VERSION_MINOR"] "." v["CAIRO_VERSION_MICRO"] }' "$dir/include/cairo/cairo-version.h" 2>/dev/null)
        [ "$h" = "$cairo_pin" ]; check $? "cairo-version.h says $h"
    fi
    echo "check.sh: toy text on Windows: ci-load.st's CairoToy line (CI)"
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
