#!/bin/sh
# shellcheck disable=SC2016,SC2018,SC2019,SC2086  # $ORIGIN is literal; names are ASCII; word splitting of lists is wanted
# The cairo stack of the native VMs (docs/CairoStack.md): zlib, libpng,
# FreeType, expat, fontconfig, pixman and cairo (core), GLib, FriBidi,
# HarfBuzz and Pango (feature pango), libjpeg-turbo and Poppler (feature
# poppler), built from the pinned archives of cmake/cairo-stack/pins.txt, each
# with its own upstream build system (meson for FreeType, fontconfig, pixman,
# cairo, GLib, FriBidi, HarfBuzz and Pango; CMake for zlib, libpng, expat,
# PCRE2, libjpeg-turbo and Poppler; autotools for libffi), into one prefix.
#
#   build.sh --platform linux|darwin|mingw --prefix <dir>
#            [--work <dir>] [--deps <dir>] [--downloads <dir>] [--arch <arch>]
#            [--features <list>] [--only freetype] [--pins <file>]
#            [--glibc-max <x.y>] [--jobs <n>]
#
#   --platform  linux or darwin build here; mingw runs fetch-msys2.sh, which
#               unpacks MSYS2's pinned packages instead (decision U2), then
#               builds the source pins marked mingw (Poppler) against them.
#   --prefix    the stack: include/, lib/, share/pharo-cairo-stack/{manifest.txt,
#               CAIRO-STACK.txt, licenses/<name>/, meson-summaries/<name>.txt,
#               cmake-summaries/poppler.txt}; bin/ holds GLib's tools (glib-mkenums
#               for Poppler's build), never bundled.  It is emptied first, if
#               it is a stack prefix (.pharo-cairo-stack).
#   --work      sources, build trees, the fontconfig and GLib staging, and the
#               private static prefix <work>/static (libffi, PCRE2 and
#               libjpeg-turbo: headers and archives that only GLib and Poppler
#               link, never in <prefix>) (default <prefix>.work)
#   --deps      a directory of archives (WASM_DEPS_DIR), looked into first, then
#               $PHARO_CAIRO_STACK_DOWNLOADS, then --downloads; anything else
#               is downloaded (HTTPS only) from the URL, or the mirror, into
#               --downloads.  Every SHA256 is checked before any extraction.
#   --arch      darwin: the one architecture (x86_64 or arm64), the host's
#   --features  a comma list of pango and poppler (default pango,poppler;
#               "none" or empty: none).  The map (docs/CairoStack.md):
#                 pango   = libffi pcre2 proxy-libintl glib fribidi harfbuzz pango
#                 poppler = libffi pcre2 proxy-libintl glib libjpeg-turbo poppler,
#                           and on darwin expat and fontconfig (core on Linux)
#               proxy-libintl is a darwin pin, poppler the only mingw one.
#   --only freetype   zlib, libpng and FreeType only (FEATURE_LIB_FREETYPE2
#               alone); no feature
#   --glibc-max linux: then runs check.sh --prefix with --glibc-max
#
# Environment: CC, CXX (default clang, clang++ when present), MACOSX_DEPLOYMENT_TARGET
# (darwin, default 11.0), PHARO_CAIRO_STACK_GCC_INSTALL_DIR (linux, below),
# PHARO_CAIRO_STACK_<NAME>_ARGS (extra meson, CMake or configure arguments for
# one library, NAME in ZLIB LIBPNG FREETYPE EXPAT FONTCONFIG PIXMAN CAIRO
# LIBFFI PCRE2 PROXY_LIBINTL GLIB FRIBIDI HARFBUZZ PANGO LIBJPEG_TURBO POPPLER,
# recorded in the manifest).
# Tools: meson >= 1.11, ninja, pkg-config (or pkgconf), cmake, python3, and
# gperf on Linux (scripts/cairo-stack/requirements.txt has meson, ninja and
# pkgconf); with a feature, a C++ compiler; with poppler, cmake >= 3.28, and
# NASM >= 2.13 on Linux x86_64 (libjpeg-turbo's SIMD), gperf on macOS.
# Linux with a feature: the libstdc++ of GCC 13 (GLIBCXX <= 3.4.32): with clang,
# --gcc-install-dir=<dir of g++-13's libgcc> (or $PHARO_CAIRO_STACK_GCC_INSTALL_DIR)
# on every compile and link; another compiler must be GCC 13.
# mingw with poppler, in the MSYS2 shell: cmake >= 3.28, ninja, pkgconf,
# python3 and the environment's clang/clang++ (pacboy).
#
# POSIX sh: dash, bash, macOS sh.

set -eu

script_version=2
here=$(cd "$(dirname "$0")" && pwd)
pins=$here/../../cmake/cairo-stack/pins.txt
platform=
prefix=
work=
deps=
downloads=
arch=
only=
features=pango,poppler
features_given=no
glibc_max=
jobs=

die() { echo "build.sh: $*" >&2; exit 1; }
note() { echo "build.sh: $*" >&2; }
usage() { sed -n '2,/^# POSIX sh/p' "$0" | sed 's/^# \{0,1\}//'; exit "${1:-0}"; }

while [ $# -gt 0 ]; do
    case $1 in
        --platform) platform=$2; shift 2 ;;
        --prefix) prefix=$2; shift 2 ;;
        --work) work=$2; shift 2 ;;
        --deps) deps=$2; shift 2 ;;
        --downloads) downloads=$2; shift 2 ;;
        --arch) arch=$2; shift 2 ;;
        --features) features=$2; features_given=yes; shift 2 ;;
        --only) only=$2; shift 2 ;;
        --pins) pins=$2; shift 2 ;;
        --glibc-max) glibc_max=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        -h|--help) usage 0 ;;
        *) echo "build.sh: unknown argument $1" >&2; usage 1 >&2 ;;
    esac
done

[ -n "$prefix" ] || die "no --prefix"
case $platform in linux|darwin|mingw) ;; *) die "--platform linux, darwin or mingw" ;; esac
case $only in ''|freetype) ;; *) die "--only freetype, or nothing" ;; esac
[ -f "$pins" ] || die "no $pins"

# The features, in the fixed order "pango poppler" (feature_list) and as the
# comma list fetch-msys2.sh takes (feature_csv)
for f in $(echo "$features" | tr ',' ' '); do
    case $f in pango|poppler|none) ;; *) die "--features $features: a comma list of pango and poppler (or none)" ;; esac
done
feature_list=
for f in pango poppler; do
    case ",$features," in *",$f,"*) feature_list="$feature_list $f" ;; esac
done
feature_list=${feature_list# }
if [ "$only" = freetype ]; then
    [ "$features_given" = no ] || [ -z "$feature_list" ] || die "--only freetype builds no feature: not with --features $features"
    feature_list=
fi
feature_csv=$(echo "$feature_list" | tr ' ' ',')
has_feature() { case " $feature_list " in *" $1 "*) return 0 ;; esac; return 1; }

abspath() { case $1 in /*) echo "$1" ;; *) echo "$(pwd)/$1" ;; esac; }
prefix=$(abspath "$prefix")
[ -n "$work" ] || work=$prefix.work
work=$(abspath "$work")
[ -n "$downloads" ] || downloads=$work/downloads
downloads=$(abspath "$downloads")
[ -z "$deps" ] || deps=$(abspath "$deps")
pins=$(abspath "$pins")
static=$work/static

host_arch=$(uname -m)
case $platform in
    darwin)
        [ -n "$arch" ] || arch=$host_arch
        [ "$arch" = "$host_arch" ] || die "--arch $arch is not this Mac's ($host_arch): the stack is built natively, one architecture"
        : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"
        export MACOSX_DEPLOYMENT_TARGET ;;
    linux)
        [ -z "$arch" ] || [ "$arch" = "$host_arch" ] || die "--arch $arch is not this machine's ($host_arch)"
        arch=$host_arch ;;
    mingw) ;;   # fetch-msys2.sh takes --arch (or MSYSTEM)
esac

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# version_ge <a> <b>: a >= b, for dotted numbers
version_ge() {
    [ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -n 1)" = "$2" ]
}

# ---------------------------------------------------------------- the pins
# The feature map (docs/CairoStack.md; the same in cmake/cairoStack.cmake):
# wanted(name) holds when the name is core or one of its features is enabled
feature_awk='
    function feats(n) {
        if (n == "libffi" || n == "pcre2" || n == "proxy-libintl" || n == "glib") return "pango poppler"
        if (n == "fribidi" || n == "harfbuzz" || n == "pango") return "pango"
        if (n == "libjpeg-turbo" || n == "poppler") return "poppler"
        if ((n == "expat" || n == "fontconfig") && platform == "darwin") return "poppler"
        return "core"
    }
    function wanted(n,   m, fs, i) {
        m = split(feats(n), fs, " ")
        for (i = 1; i <= m; i++) if (fs[i] == "core" || index(" " features " ", " " fs[i] " ")) return 1
        return 0
    }
    function on_platform(p,   n, ps, i, ok) {
        n = split(p, ps, ","); ok = (p == "all")
        for (i = 1; i <= n; i++) if (ps[i] == platform) ok = 1
        return ok
    }'
# "<name> <version> <sha256> <file> <url> <platforms> <licences...>" of this platform and these features
selected() {
    awk -v platform="$platform" -v only="$only" -v features="$feature_list" "$feature_awk"'
        /^[ \t]*#/ || NF == 0 { next }
        $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
        NF < 7 { print "build.sh: bad line in pins.txt: " $0 > "/dev/stderr"; exit 1 }
        {
            ok = on_platform($6) && wanted($1)
            if (only == "freetype" && $1 != "zlib" && $1 != "libpng" && $1 != "freetype") ok = 0
            if (ok) print
        }' "$pins"
}
mirror_of() { awk -v n="$1" '$1 == "mirror" && $2 == n { print $3 }' "$pins"; }
selected > /dev/null || die "bad $pins"
if [ "$platform" != mingw ]; then
    [ -n "$(selected)" ] || die "$pins has nothing for $platform"
fi
is_selected() { selected | awk -v n="$1" '$1 == n { f = 1 } END { exit !f }'; }

# ---------------------------------------------------------------- tools
if [ -z "${CC:-}" ]; then
    if command -v clang >/dev/null 2>&1; then CC=clang; else CC=cc; fi
fi
if [ -z "${CXX:-}" ]; then
    if command -v clang++ >/dev/null 2>&1; then CXX=clang++; else CXX=c++; fi
fi
export CC CXX
is_clang() { "$1" --version 2>/dev/null | head -n 1 | grep -q -i clang; }
python=$(command -v python3 2>/dev/null || command -v python 2>/dev/null || true)
needs_cxx=no
if is_selected harfbuzz || is_selected poppler; then needs_cxx=yes; fi

if [ "$platform" = mingw ]; then
    if is_selected poppler; then
        for t in cmake:cmake ninja:ninja pkgconf:pkgconf "$CC:clang" "$CXX:clang"; do
            command -v "${t%:*}" >/dev/null 2>&1 || die "${t%:*} is not on PATH: poppler is built here (pacboy -S ${t##*:}:p)"
        done
        [ -n "$python" ] || die "python3 is not on PATH: poppler's build runs glib-mkenums (pacboy -S python:p)"
        PKG_CONFIG=$(command -v pkgconf)
    fi
else
    PKG_CONFIG=${PKG_CONFIG:-}
    if [ -z "$PKG_CONFIG" ]; then
        for t in pkg-config pkgconf; do
            if command -v $t >/dev/null 2>&1; then PKG_CONFIG=$(command -v $t); break; fi
        done
    fi
    for t in meson ninja cmake "$CC"; do
        command -v "$t" >/dev/null 2>&1 || die "$t is not on PATH (docs/CairoStack.md, \"Tools\")"
    done
    [ -n "$PKG_CONFIG" ] || die "pkg-config is not on PATH (docs/CairoStack.md, \"Tools\")"
    meson_version=$(meson --version)
    case $meson_version in
        0.*|1.[0-9]|1.[0-9].*|1.10|1.10.*) die "meson $meson_version: fontconfig 2.18 needs meson >= 1.11 (pip install -r scripts/cairo-stack/requirements.txt)" ;;
    esac
    [ -n "$python" ] || die "python3 is not on PATH (meson and GLib's build scripts)"
    if [ "$needs_cxx" = yes ]; then
        command -v "$CXX" >/dev/null 2>&1 || die "$CXX is not on PATH: HarfBuzz and Poppler are C++ (set CXX)"
    fi
    if is_selected fontconfig; then
        command -v gperf >/dev/null 2>&1 || die "gperf is not on PATH: fontconfig needs it ($([ "$platform" = linux ] && echo 'apt install gperf' || echo "Xcode's command-line tools have /usr/bin/gperf, else brew install gperf"))"
    fi
fi
if is_selected poppler; then
    cmake_version=$(cmake --version | awk 'NR == 1 { print $3 }')
    version_ge "$cmake_version" 3.28 || die "cmake $cmake_version: Poppler 26 needs cmake >= 3.28"
fi
if [ "$platform" = linux ] && [ "$arch" = x86_64 ] && is_selected libjpeg-turbo; then
    command -v nasm >/dev/null 2>&1 || die "nasm is not on PATH: libjpeg-turbo's SIMD needs NASM >= 2.13 (apt install nasm)"
fi
export PKG_CONFIG

# Linux, a C++ library: the libstdc++ of GCC 13, whose GLIBCXX_3.4.32 is the
# ceiling of the bundle (clang otherwise picks the newest GCC, e.g. 14's 3.4.33)
gcc_dir=
gcc_c=
gcc_cxx=
if [ "$platform" = linux ] && [ "$needs_cxx" = yes ]; then
    if is_clang "$CXX"; then
        if [ -z "${PHARO_CAIRO_STACK_GCC_INSTALL_DIR:-}" ]; then
            command -v g++-13 >/dev/null 2>&1 || die "g++-13 not found: apt install g++-13 (libstdc++ 13 keeps GLIBCXX <= 3.4.32)"
        fi
        gcc_dir=${PHARO_CAIRO_STACK_GCC_INSTALL_DIR:-$(dirname "$(g++-13 -print-libgcc-file-name)")}
        [ -d "$gcc_dir" ] || die "no GCC installation directory $gcc_dir"
        gcc_cxx=--gcc-install-dir=$gcc_dir
        if is_clang "$CC"; then gcc_c=$gcc_cxx; fi
    else
        cxx_major=$("$CXX" -dumpversion | cut -d. -f1)
        [ "$cxx_major" = 13 ] || die "$CXX is GCC $cxx_major: the C++ libraries need GCC 13's libstdc++ (GLIBCXX <= 3.4.32): use g++-13, or clang with g++-13 installed"
        gcc_dir=$(dirname "$("$CXX" -print-libgcc-file-name)")
    fi
fi

download() {
    rm -f "$2.part"
    if curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --connect-timeout 30 -o "$2.part" "$1"; then
        mv "$2.part" "$2"
    else
        rm -f "$2.part"; return 1
    fi
}

# fetch <name> <file> <sha256> <url>: prints the path of a checked archive
fetch() {
    for f_dir in "$deps" "${PHARO_CAIRO_STACK_DOWNLOADS:-}" "$downloads"; do
        [ -n "$f_dir" ] && [ -f "$f_dir/$2" ] || continue
        f_got=$(sha256 "$f_dir/$2")
        [ "$f_got" = "$3" ] || die "$f_dir/$2 has SHA256 $f_got, pins.txt says $3: stopping before any extraction"
        echo "$f_dir/$2"
        return 0
    done
    command -v curl >/dev/null 2>&1 || die "$2 is not in --deps, \$PHARO_CAIRO_STACK_DOWNLOADS or --downloads, and there is no curl"
    mkdir -p "$downloads"
    for f_from in "$4" $(mirror_of "$1"); do
        note "downloading $f_from"
        if download "$f_from" "$downloads/$2"; then
            f_got=$(sha256 "$downloads/$2")
            if [ "$f_got" = "$3" ]; then echo "$downloads/$2"; return 0; fi
            rm -f "$downloads/$2"
            die "$f_from has SHA256 $f_got, pins.txt says $3: stopping before any extraction"
        fi
    done
    die "cannot download $2 (offline? put it in --deps)"
}

# 1. every archive, checked, before anything else is touched
mkdir -p "$work"
: > "$work/archives.txt"
selected | while read -r name version sha file url _rest; do
    path=$(fetch "$name" "$file" "$sha" "$url") || exit 1
    echo "$name $path" >> "$work/archives.txt"
done
[ "$(wc -l < "$work/archives.txt")" -eq "$(selected | wc -l)" ] || die "not every archive was found"

args_of() {
    a_var=PHARO_CAIRO_STACK_$(echo "$1" | tr 'a-z-' 'A-Z_')_ARGS
    eval "echo \"\${$a_var:-}\""
}
all_names="zlib libpng freetype expat fontconfig pixman cairo libffi pcre2 proxy-libintl glib fribidi harfbuzz pango libjpeg-turbo poppler"
tool_line() { printf '%s %s\n' "$1" "$(eval "$2" 2>/dev/null | head -n 1 | tr -s ' \t' '  ')"; }
# the configure summary of Poppler's CMake ("Building Poppler with support for:" ...)
cmake_summary() {
    mkdir -p "$share/cmake-summaries"
    awk '/^Building Poppler with support for:/ { on = 1 } /^-- Configuring done/ { on = 0 } on' "$2" > "$share/cmake-summaries/$1.txt"
    [ -s "$share/cmake-summaries/$1.txt" ] || die "$1: no configure summary in $2"
}

# ================================================================ Windows
# MSYS2's packages (fetch-msys2.sh), then the source pins marked mingw
# (Poppler: MSYS2's own needs curl, NSS, lcms2, OpenJPEG...) built with the
# environment's clang, CMake and Ninja against them.  Native Windows programs
# (cmake, ninja, pkgconf, clang, python) get "cygpath -m" paths (C:/msys64/...).
if [ "$platform" = mingw ]; then
    sh "$here/fetch-msys2.sh" --prefix "$prefix" ${arch:+--arch "$arch"} --work "$work" \
        ${deps:+--deps "$deps"} --downloads "$downloads" --pins "$pins" --features "${feature_csv:-none}"
    share=$prefix/share/pharo-cairo-stack
    if [ ! -s "$work/archives.txt" ]; then
        has_feature poppler && die "--features poppler, but $pins has no source line for mingw (poppler)"
        exit 0
    fi
    win() { if command -v cygpath >/dev/null 2>&1; then cygpath -m "$1"; else echo "$1"; fi; }
    winexe() { if [ -f "$1.exe" ]; then win "$1.exe"; else win "$1"; fi; }    # a program, for CreateProcess
    rm -rf "$work/src" "$work/build" "$work/stage"
    mkdir -p "$work/src" "$work/build" "$work/stage"
    : > "$work/built.txt"
    while read -r name path; do
        set -- $(selected | awk -v n="$name" '$1 == n')
        version=$2 sha=$3 url=$5
        shift 6
        [ "$name" = poppler ] || die "pins.txt marks $name mingw: build.sh builds only poppler on Windows"
        note "$name $version: extracting $(basename "$path")"
        tar -xf "$path" -C "$work/src"
        src=$work/src/$name-$version
        [ -d "$src" ] || die "$path has no $name-$version/ directory"
        for l in "$@"; do
            [ -f "$src/$l" ] || die "$name: no licence file $l in the archive"
            mkdir -p "$share/licenses/$name/$(dirname "$l")"
            cp "$src/$l" "$share/licenses/$name/$l"
        done
        b_dir=$work/build/$name
        stage=$work/stage/$name
        PKG_CONFIG_LIBDIR=$(win "$prefix/lib/pkgconfig"); export PKG_CONFIG_LIBDIR
        unset PKG_CONFIG_PATH || true
        note "$name: cmake"
        # shellcheck disable=SC2046
        cmake -S "$(win "$src")" -B "$(win "$b_dir")" -G Ninja -DCMAKE_BUILD_TYPE=Release \
            -DCMAKE_INSTALL_PREFIX="$(win "$stage")" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_INSTALL_INCLUDEDIR=include \
            -DCMAKE_PREFIX_PATH="$(win "$prefix")" -DCMAKE_FIND_USE_CMAKE_SYSTEM_PATH=OFF -DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF \
            -DCMAKE_C_COMPILER="$CC" -DCMAKE_CXX_COMPILER="$CXX" \
            -DCMAKE_C_FLAGS="-ffile-prefix-map=$(win "$work")=/cairo-stack-build" \
            -DCMAKE_CXX_FLAGS="-ffile-prefix-map=$(win "$work")=/cairo-stack-build" \
            -DPKG_CONFIG_EXECUTABLE="$(winexe "$PKG_CONFIG")" \
            -DENABLE_GLIB=ON -DENABLE_CPP=OFF -DENABLE_QT5=OFF -DENABLE_QT6=OFF -DENABLE_UTILS=OFF \
            -DENABLE_GOBJECT_INTROSPECTION=OFF -DENABLE_GTK_DOC=OFF -DBUILD_GTK_TESTS=OFF -DBUILD_MANUAL_TESTS=OFF \
            -DBUILD_CPP_TESTS=OFF -DBUILD_QT5_TESTS=OFF -DBUILD_QT6_TESTS=OFF -DENABLE_BOOST=OFF \
            -DENABLE_LIBOPENJPEG=OFF -DENABLE_LCMS=OFF -DENABLE_LIBTIFF=OFF -DENABLE_LIBCURL=OFF -DENABLE_NSS3=OFF \
            -DENABLE_GPGME=OFF -DENABLE_BROTLI=OFF -DENABLE_HARFBUZZ=OFF -DENABLE_LIBJPEG=ON \
            -DENABLE_UNSTABLE_API_ABI_HEADERS=OFF -DRUN_GPERF_IF_PRESENT=OFF \
            -DCMAKE_DISABLE_FIND_PACKAGE_Boost=ON -DCMAKE_DISABLE_FIND_PACKAGE_GTK=ON \
            -DFONT_CONFIGURATION=win32 -DENABLE_RELOCATABLE=ON \
            -DJPEG_LIBRARY="$(win "$prefix/lib/libjpeg.dll.a")" -DJPEG_INCLUDE_DIR="$(win "$prefix/include")" \
            -DGLIB2_MKENUMS="$(win "$prefix/bin/glib-mkenums-script.py")" -DGLIB2_MKENUMS_PYTHON="$(winexe "$python")" \
            $(args_of "$name") > "$work/build/$name.setup.log" 2>&1 \
            || { tail -n 40 "$work/build/$name.setup.log" >&2; die "$name: cmake configure failed ($work/build/$name.setup.log)"; }
        cmake_summary "$name" "$work/build/$name.setup.log"
        note "$name: build"
        cmake --build "$(win "$b_dir")" ${jobs:+-j "$jobs"} > "$work/build/$name.build.log" 2>&1 \
            || { tail -n 40 "$work/build/$name.build.log" >&2; die "$name: build failed ($work/build/$name.build.log)"; }
        cmake --install "$(win "$b_dir")" > "$work/build/$name.install.log" 2>&1 \
            || { tail -n 40 "$work/build/$name.install.log" >&2; die "$name: install failed"; }
        # the DLLs (names from the staged bin/, not assumed), import libraries, headers
        core_dll=$(cd "$stage/bin" && ls libpoppler-[0-9]*.dll 2>/dev/null || true)
        glib_dll=$(cd "$stage/bin" && ls libpoppler-glib-[0-9]*.dll 2>/dev/null || true)
        [ "$(echo "$core_dll" | wc -w)" = 1 ] && [ "$(echo "$glib_dll" | wc -w)" = 1 ] \
            || die "$name: expected one libpoppler-<N>.dll and one libpoppler-glib-<M>.dll in $stage/bin, found: $core_dll $glib_dll"
        for d in $core_dll $glib_dll; do
            [ ! -e "$prefix/bin/$d" ] || die "$d is already in $prefix/bin (an MSYS2 package of the closure?)"
            cp -p "$stage/bin/$d" "$prefix/bin/"
            echo "$d built $name $(sha256 "$prefix/bin/$d") $name" >> "$work/built.txt"
        done
        cp -p "$stage"/lib/*.dll.a "$prefix/lib/"
        mkdir -p "$prefix/include/poppler"
        cp -R "$stage/include/poppler/." "$prefix/include/poppler/"
        echo "$name $version $sha $url" >> "$work/built-pins.txt.$$"
    done < "$work/archives.txt"
    cat "$work/built.txt" >> "$share/dlls.txt"

    # The manifest: fetch-msys2.sh's, its features line rewritten to what the
    # prefix now holds, plus the source pins built here and the tools
    {
        sed '/^features /d' "$share/manifest.txt"
        echo "features ${feature_list:--}"
        echo "built-by build.sh $script_version"
        while read -r name version sha url; do echo "lib $name $version $sha"; done < "$work/built-pins.txt.$$"
        tool_line "tool cc" "\"$CC\" --version"
        tool_line "tool c++" "\"$CXX\" --version"
        tool_line "tool cmake" "cmake --version"
        tool_line "tool ninja" "ninja --version"
        tool_line "tool pkg-config" "\"$PKG_CONFIG\" --version"
        tool_line "tool python3" "\"$python\" --version"
        a=$(args_of poppler)
        [ -z "$a" ] || echo "args POPPLER $a"
    } > "$share/manifest.txt.new"
    mv "$share/manifest.txt.new" "$share/manifest.txt"
    {
        echo
        echo "Built from source by scripts/cairo-stack/build.sh --platform mingw, with the"
        echo "toolchain of the MSYS2 environment, against the packages above (no patch):"
        while read -r name version sha url; do
            awk -v n="$name" '/^# (signed-by|unsigned)/ { prov = $0; sub(/^# /, "", prov); next }
                /^#/ { next } $1 == n { printf "    %s %s\n        sha256 %s\n        %s\n        %s\n", $1, $2, $3, $5, prov; exit }
                { prov = "" }' "$pins"
        done < "$work/built-pins.txt.$$"
        echo "    options: core and glib only (FONT_CONFIGURATION=win32, libjpeg on; no Qt, C++ wrapper,"
        echo "    utilities, curl, NSS, GPGME, lcms2, OpenJPEG, libtiff, Brotli, Boost, HarfBuzz)"
        echo "    compilers: $("$CC" --version 2>/dev/null | head -n 1); $("$CXX" --version 2>/dev/null | head -n 1)"
        echo
        echo "DLLs built here (name, kind, package, SHA256, features):"
        sed 's/^/    dll /' "$work/built.txt"
    } >> "$share/CAIRO-STACK.txt"
    rm -f "$work/built-pins.txt.$$"
    note "done: $(awk '{ printf "%s%s", sep, $1; sep = ", " }' "$work/built.txt") built into $prefix"
    exit 0
fi

# ================================================================ Linux, macOS
# 2. a fresh prefix
# (a prefix that holds only empty directories counts as fresh: Ninja makes
# the directories of an ExternalProject's byproducts before running it)
if [ -d "$prefix" ] && [ -n "$(find "$prefix" ! -type d 2>/dev/null | head -n 1)" ] && [ ! -f "$prefix/.pharo-cairo-stack" ]; then
    die "$prefix is not empty and is not a cairo stack prefix (no .pharo-cairo-stack): not touching it"
fi
rm -rf "$prefix"
mkdir -p "$prefix/lib/pkgconfig" "$prefix/include" "$prefix/share/pharo-cairo-stack/licenses" "$prefix/share/pharo-cairo-stack/meson-summaries"
: > "$prefix/.pharo-cairo-stack"
rm -rf "$work/src" "$work/build" "$work/stage" "$static"
mkdir -p "$work/src" "$work/build"
touch "$work/.started"
share=$prefix/share/pharo-cairo-stack

# ---------------------------------------------------------------- options
# Only the prefix and the private static prefix: pkg-config sees nothing else,
# and no subproject is built (GLib's .pc files name libffi and libpcre2-8 in
# Requires.private, which live in <work>/static)
PKG_CONFIG_LIBDIR=$prefix/lib/pkgconfig:$static/lib/pkgconfig
export PKG_CONFIG_LIBDIR
unset PKG_CONFIG_PATH || true
unset PKG_CONFIG_SYSROOT_DIR || true

case $platform in
    linux)
        # DT_RPATH $ORIGIN (searched before LD_LIBRARY_PATH); the private static
        # archives (also compiled with hidden visibility) not re-exported
        exclude='-Wl,--exclude-libs,libz.a:libexpat.a:libffi.a:libpcre2-8.a:libjpeg.a'
        link_args="-Wl,--disable-new-dtags,-rpath,\$ORIGIN $exclude"
        cmake_link_args="-Wl,--disable-new-dtags $exclude"   # + CMAKE_INSTALL_RPATH
        rpath_flag='-Wl,-rpath,\$ORIGIN'    # Poppler (CMAKE_SKIP_RPATH): Ninja's shell sees \$
        rpath='$ORIGIN'
        c_args=
        ;;
    darwin)
        link_args="-arch $arch -Wl,-rpath,@loader_path"
        cmake_link_args=   # CMAKE_OSX_ARCHITECTURES and CMAKE_INSTALL_RPATH
        rpath_flag=-Wl,-rpath,@loader_path
        rpath=@loader_path
        c_args="-arch $arch"
        ;;
esac
# No build path in the objects (__FILE__, G_STRLOC): <work> reads /cairo-stack-build
prefix_map=-ffile-prefix-map=$work=/cairo-stack-build

# meson's array syntax for a list of words: ['a', 'b']
meson_array() {
    m_out=
    for m_w in $1; do m_out="$m_out${m_out:+, }'$m_w'"; done
    echo "[$m_out]"
}

wrap_mode=nofallback
meson_common() {
    echo "--prefix=$prefix --libdir=lib --includedir=include --buildtype=release -Db_ndebug=false --wrap-mode=$wrap_mode -Ddefault_library=shared"
}

# summary <name> <setup log>: the summary meson prints at the end of setup
summary() {
    awk '/^Build targets in project/ { on = 1; next } /^Found ninja/ { on = 0 } on' "$2" > "$share/meson-summaries/$1.txt"
    [ -s "$share/meson-summaries/$1.txt" ] || cp "$2" "$share/meson-summaries/$1.txt"
}

build_meson() {
    b_name=$1 b_src=$2; shift 2
    b_dir=$work/build/$b_name
    note "$b_name: meson setup"
    # shellcheck disable=SC2046
    meson setup "$b_dir" "$b_src" $(meson_common) \
        -Dc_args="$(meson_array "$c_args $prefix_map $gcc_c")" -Dcpp_args="$(meson_array "$c_args $prefix_map $gcc_cxx")" \
        -Dc_link_args="$(meson_array "$link_args $gcc_c")" -Dcpp_link_args="$(meson_array "$link_args $gcc_cxx")" \
        "$@" $(args_of "$b_name") > "$work/build/$b_name.setup.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.setup.log" >&2; die "$b_name: meson setup failed ($work/build/$b_name.setup.log)"; }
    summary "$b_name" "$work/build/$b_name.setup.log"
    note "$b_name: build"
    meson compile -C "$b_dir" ${jobs:+-j "$jobs"} > "$work/build/$b_name.build.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.build.log" >&2; die "$b_name: build failed ($work/build/$b_name.build.log)"; }
}

install_meson() {
    i_name=$1; shift
    meson install -C "$work/build/$i_name" --no-rebuild "$@" > "$work/build/$i_name.install.log" 2>&1 \
        || { tail -n 40 "$work/build/$i_name.install.log" >&2; die "$i_name: install failed"; }
}

# build_cmake <name> <src> <-D...>: configure, build and install into
# $cmake_prefix (default <prefix>; <work>/static for the private archives),
# with $cmake_link extra shared-library link flags; both reset afterwards
cmake_prefix=
cmake_link=
build_cmake() {
    b_name=$1 b_src=$2; shift 2
    b_dir=$work/build/$b_name
    set -- -S "$b_src" -B "$b_dir" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="${cmake_prefix:-$prefix}" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_INSTALL_INCLUDEDIR=include \
        -DCMAKE_PREFIX_PATH="$prefix" -DCMAKE_FIND_USE_CMAKE_SYSTEM_PATH=OFF -DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        -DCMAKE_C_COMPILER="$CC" -DCMAKE_CXX_COMPILER="$CXX" \
        -DCMAKE_C_FLAGS="$prefix_map${gcc_c:+ $gcc_c}" -DCMAKE_CXX_FLAGS="$prefix_map${gcc_cxx:+ $gcc_cxx}" \
        -DCMAKE_SHARED_LINKER_FLAGS="$cmake_link_args${gcc_c:+ $gcc_c}${cmake_link:+ $cmake_link}" \
        -DCMAKE_INSTALL_RPATH="$rpath" -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
        "$@"
    if [ "$platform" = darwin ]; then
        set -- "$@" -DCMAKE_OSX_ARCHITECTURES="$arch" -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET" \
            -DCMAKE_INSTALL_NAME_DIR=@rpath -DCMAKE_MACOSX_RPATH=ON \
            "-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local" "-DCMAKE_SYSTEM_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local"
    fi
    cmake_prefix=
    cmake_link=
    note "$b_name: cmake"
    # shellcheck disable=SC2046
    cmake "$@" $(args_of "$b_name") > "$work/build/$b_name.setup.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.setup.log" >&2; die "$b_name: cmake configure failed ($work/build/$b_name.setup.log)"; }
    note "$b_name: build"
    cmake --build "$b_dir" ${jobs:+-j "$jobs"} > "$work/build/$b_name.build.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.build.log" >&2; die "$b_name: build failed ($work/build/$b_name.build.log)"; }
    cmake --install "$b_dir" > "$work/build/$b_name.install.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.install.log" >&2; die "$b_name: install failed"; }
}

# build_autotools <name> <src> <configure arguments...>: an out-of-tree
# configure, make and make install (libffi has no meson or CMake build)
ncpu=$(getconf _NPROCESSORS_ONLN 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 2)
build_autotools() {
    b_name=$1 b_src=$2; shift 2
    b_dir=$work/build/$b_name
    rm -rf "$b_dir"; mkdir -p "$b_dir"
    note "$b_name: configure"
    # shellcheck disable=SC2046
    (cd "$b_dir" && "$b_src/configure" "$@" $(args_of "$b_name")) > "$work/build/$b_name.setup.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.setup.log" >&2; die "$b_name: configure failed ($work/build/$b_name.setup.log)"; }
    note "$b_name: build"
    make -C "$b_dir" -j"${jobs:-$ncpu}" > "$work/build/$b_name.build.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.build.log" >&2; die "$b_name: build failed ($work/build/$b_name.build.log)"; }
    make -C "$b_dir" install > "$work/build/$b_name.install.log" 2>&1 \
        || { tail -n 40 "$work/build/$b_name.install.log" >&2; die "$b_name: install failed"; }
}

# staged_install <name> <dir>...: meson install into <work>/stage/<name>, then
# only the named directories of the prefix are copied (what an install writes
# outside the prefix, or into share/ and libexec/, stays in the stage)
staged_install() {
    s_name=$1; shift
    s_stage=$work/stage/$s_name
    rm -rf "$s_stage"
    install_meson "$s_name" --destdir "$s_stage"
    [ -d "$s_stage$prefix/lib" ] || die "$s_name: nothing staged in $s_stage$prefix/lib"
    for s_d in "$@"; do
        [ -d "$s_stage$prefix/$s_d" ] || continue
        mkdir -p "$prefix/$s_d"
        cp -R "$s_stage$prefix/$s_d/." "$prefix/$s_d/"
    done
}

# Nothing may appear outside <prefix> and <work>: fontconfig's own install
# would write /etc/fonts and /var/cache/fontconfig (hence its staging)
fc_dirs="/etc/fonts /var/cache/fontconfig /usr/share/fontconfig /usr/share/xml/fontconfig"
check_outside() {
    o_new=$(find $fc_dirs -newer "$work/.started" 2>/dev/null | head -n 5)
    [ -z "$o_new" ] || die "$1 wrote outside the prefix: $o_new"
    for o_d in etc var share/fontconfig share/xml; do
        [ ! -e "$prefix/$o_d" ] || die "$1 installed $prefix/$o_d"
    done
}

# ---------------------------------------------------------------- the build
while read -r name path; do
    line=$(selected | awk -v n="$name" '$1 == n')
    set -- $line
    version=$2
    shift 6
    licences=$*
    note "$name $version: extracting $(basename "$path")"
    tar -xf "$path" -C "$work/src"
    src=$work/src/$name-$version
    [ -d "$src" ] || die "$path has no $name-$version/ directory"
    for l in $licences; do
        [ -f "$src/$l" ] || die "$name: no licence file $l in the archive"
        mkdir -p "$share/licenses/$name/$(dirname "$l")"
        cp "$src/$l" "$share/licenses/$name/$l"
    done
    case $name in
        zlib)
            build_cmake zlib "$src" -DZLIB_BUILD_SHARED=OFF -DZLIB_BUILD_STATIC=ON -DZLIB_BUILD_TESTING=OFF \
                -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_C_VISIBILITY_PRESET=hidden
            [ -f "$prefix/lib/libz.a" ] || die "zlib: no lib/libz.a"
            [ -f "$prefix/lib/pkgconfig/zlib.pc" ] || die "zlib: no zlib.pc"
            ;;
        libpng)
            build_cmake libpng "$src" -DPNG_SHARED=ON -DPNG_STATIC=OFF -DPNG_TESTS=OFF -DPNG_TOOLS=OFF -DPNG_FRAMEWORK=OFF \
                -DZLIB_ROOT="$prefix" -DZLIB_USE_STATIC_LIBS=ON -DZLIB_LIBRARY="$prefix/lib/libz.a" -DZLIB_INCLUDE_DIR="$prefix/include"
            ;;
        freetype)
            set -- -Dharfbuzz=disabled -Dbrotli=disabled -Dbzip2=disabled -Dpng=enabled -Dzlib=system -Dtests=disabled
            [ "$platform" = darwin ] && set -- "$@" -Dhvf=disabled
            build_meson freetype "$src" "$@"
            install_meson freetype
            ;;
        expat)
            build_cmake expat "$src" -DEXPAT_SHARED_LIBS=OFF -DEXPAT_BUILD_TOOLS=OFF -DEXPAT_BUILD_EXAMPLES=OFF \
                -DEXPAT_BUILD_TESTS=OFF -DEXPAT_BUILD_DOCS=OFF -DEXPAT_BUILD_FUZZERS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
                -DCMAKE_C_VISIBILITY_PRESET=hidden
            ;;
        fontconfig)
            set -- -Ddoc=disabled -Dtests=disabled -Dtools=disabled -Dcache-build=disabled \
                -Dnls=disabled -Diconv=disabled -Dfontations=disabled -Dxml-backend=expat \
                -Dbaseconfig-dir=/etc/fonts -Dconfig-dir=/etc/fonts/conf.d \
                -Dtemplate-dir=/usr/share/fontconfig/conf.avail -Dxml-dir=/usr/share/xml/fontconfig \
                -Dcache-dir=/var/cache/fontconfig
            case $platform in
                linux) set -- "$@" -Ddefault-fonts-dirs=/usr/share/fonts,/usr/local/share/fonts ;;
                # macOS (Poppler's font lookup): the system's font directories
                # compiled in; the bundle's fonts.conf is what LibPoppler loads
                darwin) set -- "$@" -Ddefault-fonts-dirs=/System/Library/Fonts,/System/Library/Fonts/Supplemental,/Library/Fonts,~/Library/Fonts \
                    -Dadditional-fonts-dirs=no ;;
            esac
            build_meson fontconfig "$src" "$@"
            staged_install fontconfig lib include
            ;;
        pixman)
            build_meson pixman "$src" -Dtests=disabled -Ddemos=disabled -Dgtk=disabled -Dlibpng=disabled -Dopenmp=disabled
            install_meson pixman
            ;;
        cairo)
            set -- -Dtests=disabled -Dgtk2-utils=disabled -Dglib=disabled -Dspectre=disabled -Dlzo=disabled \
                -Dsymbol-lookup=disabled -Dxlib=disabled -Dxcb=disabled -Dxlib-xcb=disabled \
                -Dfreetype=enabled -Dpng=enabled -Dzlib=enabled
            case $platform in
                linux) set -- "$@" -Dfontconfig=enabled -Dquartz=disabled -Ddwrite=disabled ;;
                darwin) set -- "$@" -Dfontconfig=disabled -Dquartz=enabled -Ddwrite=disabled ;;
            esac
            build_meson cairo "$src" "$@"
            install_meson cairo
            ;;
        libffi)
            # static PIC, hidden, inside libgobject only (the VM has its own libffi)
            ffi_cc=$CC
            [ "$platform" = darwin ] && ffi_cc="$CC -arch $arch -mmacosx-version-min=$MACOSX_DEPLOYMENT_TARGET"
            build_autotools libffi "$src" --prefix="$static" --libdir="$static/lib" --includedir="$static/include" \
                --disable-shared --enable-static --with-pic --disable-docs --disable-multi-os-directory \
                CC="$ffi_cc" CFLAGS="-O3 -fvisibility=hidden $prefix_map${gcc_c:+ $gcc_c}"
            [ -f "$static/lib/libffi.a" ] || die "libffi: no $static/lib/libffi.a"
            ;;
        pcre2)
            # static PIC, hidden (HAVE_VISIBILITY=OFF: PCRE2_EXPORT empty), inside libglib only
            cmake_prefix=$static
            build_cmake pcre2 "$src" -DBUILD_SHARED_LIBS=OFF -DBUILD_STATIC_LIBS=ON -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
                -DCMAKE_C_VISIBILITY_PRESET=hidden -DHAVE_VISIBILITY=OFF \
                -DPCRE2_BUILD_PCRE2_8=ON -DPCRE2_BUILD_PCRE2_16=OFF -DPCRE2_BUILD_PCRE2_32=OFF -DPCRE2_SUPPORT_JIT=OFF \
                -DPCRE2_SUPPORT_UNICODE=ON -DPCRE2_BUILD_PCRE2GREP=OFF -DPCRE2_BUILD_TESTS=OFF -DPCRE2_SUPPORT_LIBBZ2=OFF \
                -DPCRE2_SUPPORT_LIBZ=OFF -DPCRE2_SUPPORT_LIBEDIT=OFF -DPCRE2_SUPPORT_LIBREADLINE=OFF
            [ -f "$static/lib/libpcre2-8.a" ] || die "pcre2: no $static/lib/libpcre2-8.a"
            ;;
        proxy-libintl)
            # macOS: not built on its own; GLib's intl fallback (below) takes the archive
            ;;
        glib)
            set -- -Dtests=false -Dinstalled_tests=false -Dnls=disabled -Dlibmount=disabled -Dselinux=disabled \
                -Dxattr=false -Dman-pages=disabled -Ddocumentation=false -Ddtrace=disabled -Dsystemtap=disabled \
                -Dsysprof=disabled -Dintrospection=disabled -Dlibelf=disabled -Dglib_debug=disabled \
                -Dgio_module_dir=/dev/null/gio-modules -Dsysconfdir=/etc -Dlocalstatedir=/var
            if [ "$platform" = darwin ]; then
                # libintl: the pinned proxy-libintl through GLib's own wrap (meson
                # checks its source_hash), never Homebrew's libintl
                pli=$(awk '$1 == "proxy-libintl" { print $2 }' "$work/archives.txt")
                [ -n "$pli" ] || die "glib on darwin needs the proxy-libintl pin"
                mkdir -p "$src/subprojects/packagecache"
                cp "$pli" "$src/subprojects/packagecache/$(basename "$pli")"
                wrap_mode=nodownload
                set -- "$@" --force-fallback-for=intl -Dproxy-libintl:default_library=static
            fi
            build_meson glib "$src" "$@"
            wrap_mode=nofallback
            # staged: the gio module directory /dev/null/gio-modules (nothing can
            # be put there) cannot be made; share/ and libexec/ are not wanted
            staged_install glib lib include bin
            ;;
        fribidi)
            build_meson fribidi "$src" -Ddocs=false -Dbin=false -Dtests=false
            install_meson fribidi
            ;;
        harfbuzz)
            set -- -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dcairo=disabled -Dchafa=disabled \
                -Dpng=disabled -Dzlib=disabled -Dicu=disabled -Dgraphite2=disabled -Draster=disabled -Dvector=disabled \
                -Dgpu=disabled -Dsubset=disabled -Dtests=disabled -Dintrospection=disabled -Ddocs=disabled \
                -Dutilities=disabled -Dbenchmark=disabled -Ddirectwrite=disabled -Dgdi=disabled
            case $platform in
                linux) set -- "$@" -Dcoretext=disabled ;;
                darwin) set -- "$@" -Dcoretext=enabled ;;    # Pango's CoreText backend calls hb_coretext_font_create
            esac
            build_meson harfbuzz "$src" "$@"
            install_meson harfbuzz
            ;;
        pango)
            set -- -Dcairo=enabled -Dlibthai=disabled -Dxft=disabled -Dintrospection=disabled -Ddocumentation=false \
                -Dman-pages=false -Dbuild-testsuite=false -Dbuild-examples=false -Dsysprof=disabled -Dsysconfdir=/etc
            case $platform in
                linux) set -- "$@" -Dfontconfig=enabled -Dfreetype=enabled ;;
                # CoreText only: macOS cairo has no FontConfig, so no pangoft2
                darwin) set -- "$@" -Dfontconfig=disabled -Dfreetype=disabled ;;
            esac
            build_meson pango "$src" "$@"
            install_meson pango
            ;;
        libjpeg-turbo)
            # static PIC, hidden, inside libpoppler only (the VM's JPEG plugin has its own IJG jpeg_*)
            simd=ON
            [ "$platform-$arch" = darwin-x86_64 ] && simd=OFF     # no NASM on Intel Macs without Homebrew
            cmake_prefix=$static
            build_cmake libjpeg-turbo "$src" -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DCMAKE_POSITION_INDEPENDENT_CODE=ON \
                -DCMAKE_C_VISIBILITY_PRESET=hidden -DWITH_TURBOJPEG=OFF -DWITH_TOOLS=OFF -DWITH_TESTS=OFF \
                -DCMAKE_INSTALL_DEFAULT_LIBDIR=lib -DWITH_SIMD=$simd -DREQUIRE_SIMD=$simd
            [ -f "$static/lib/libjpeg.a" ] || die "libjpeg-turbo: no $static/lib/libjpeg.a"
            ;;
        poppler)
            set -- -DENABLE_GLIB=ON -DENABLE_CPP=OFF -DENABLE_QT5=OFF -DENABLE_QT6=OFF -DENABLE_UTILS=OFF \
                -DENABLE_GOBJECT_INTROSPECTION=OFF -DENABLE_GTK_DOC=OFF -DBUILD_GTK_TESTS=OFF -DBUILD_MANUAL_TESTS=OFF \
                -DBUILD_CPP_TESTS=OFF -DBUILD_QT5_TESTS=OFF -DBUILD_QT6_TESTS=OFF -DENABLE_BOOST=OFF \
                -DENABLE_LIBOPENJPEG=OFF -DENABLE_LCMS=OFF -DENABLE_LIBTIFF=OFF -DENABLE_LIBCURL=OFF -DENABLE_NSS3=OFF \
                -DENABLE_GPGME=OFF -DENABLE_BROTLI=OFF -DENABLE_HARFBUZZ=OFF -DENABLE_LIBJPEG=ON \
                -DENABLE_UNSTABLE_API_ABI_HEADERS=OFF -DRUN_GPERF_IF_PRESENT=OFF \
                -DCMAKE_DISABLE_FIND_PACKAGE_Boost=ON -DCMAKE_DISABLE_FIND_PACKAGE_GTK=ON \
                -DGLIB2_MKENUMS="$prefix/bin/glib-mkenums" -DGLIB2_MKENUMS_PYTHON="$python" \
                -DJPEG_LIBRARY="$static/lib/libjpeg.a" -DJPEG_INCLUDE_DIR="$static/include" \
                -DFONT_CONFIGURATION=fontconfig -DPOPPLER_DATADIR=/usr/share/poppler
            # Poppler's CMakeLists.txt sets CMAKE_INSTALL_RPATH_USE_LINK_PATH (a
            # build path in the RPATH): no CMake rpath, the linker flag instead
            set -- "$@" -DCMAKE_SKIP_RPATH=ON
            cmake_link=$rpath_flag
            build_cmake poppler "$src" "$@"
            cmake_summary poppler "$work/build/poppler.setup.log"
            ;;
        *) die "pins.txt names $name, which build.sh does not know how to build" ;;
    esac
    check_outside "$name"
done < "$work/archives.txt"

# Nothing of the system's: no X11, no other zlib, no foreign prefix in the .pc files
if grep -l -E '/opt/homebrew|/usr/local/(lib|include)|/ucrt64|/clangarm64' "$prefix"/lib/pkgconfig/*.pc 2>/dev/null; then
    die "a .pc file of the prefix names a foreign prefix"
fi

# ---------------------------------------------------------------- macOS install names
# meson's install rewrites the install name (LC_ID_DYLIB) of its libraries,
# and their references to each other, to absolute paths in <prefix>/lib,
# which exist only on the build machine (a CMake build with CMAKE_SKIP_RPATH,
# Poppler, may leave a bare file name instead): the bundle needs
# @rpath/<name>, found through their LC_RPATH @loader_path.  Changing load
# commands invalidates a signature, and arm64 loads only signed code, so
# every library is then signed ad hoc again (x86_64 ones too, which the
# linker does not sign, so that codesign -v holds on both).
if [ "$platform" = darwin ]; then
    for d_f in "$prefix"/lib/*.dylib; do
        { [ -f "$d_f" ] && [ ! -L "$d_f" ]; } || continue
        d_id=$(otool -D "$d_f" | sed -n 2p)
        case $d_id in
            @*) ;;
            *) install_name_tool -id "@rpath/${d_id##*/}" "$d_f" || die "install_name_tool -id on $d_f" ;;
        esac
        for d_dep in $(otool -L "$d_f" | sed -n '2,$p' | awk '{ print $1 }'); do
            case $d_dep in
                /usr/lib/*|/System/*|@*) ;;
                *) [ -e "$prefix/lib/${d_dep##*/}" ] || die "$d_f loads $d_dep, which is not in the stack"
                    install_name_tool -change "$d_dep" "@rpath/${d_dep##*/}" "$d_f" || die "install_name_tool -change on $d_f" ;;
            esac
        done
        codesign --force --sign - "$d_f" >/dev/null 2>&1 || die "codesign --sign - $d_f"
    done
fi

# ---------------------------------------------------------------- manifest, index
sel_names=$(selected | awk '{ print $1 }' | tr '\n' ' ')
{
    echo "# pharo-cairo-stack manifest (docs/CairoStack.md)"
    echo "format 2"
    echo "pins-sha256 $(sha256 "$pins")"
    echo "platform $platform"
    echo "arch $arch"
    if [ "$platform" = darwin ]; then echo "deployment-target $MACOSX_DEPLOYMENT_TARGET"; else echo "deployment-target -"; fi
    echo "script build.sh $script_version"
    echo "only ${only:--}"
    echo "features ${feature_list:--}"
    for n in $all_names; do
        a=$(args_of "$n")
        [ -z "$a" ] || echo "args $(echo "$n" | tr 'a-z-' 'A-Z_') $a"
    done
    tool_line "tool cc" "\"$CC\" --version"
    [ "$needs_cxx" = no ] || tool_line "tool c++" "\"$CXX\" --version"
    if [ -n "$gcc_dir" ]; then
        if command -v g++-13 >/dev/null 2>&1; then gv=$(g++-13 --version | head -n 1); else gv=$("$CXX" --version | head -n 1); fi
        echo "tool gcc-install-dir $gcc_dir ($gv)"
    fi
    tool_line "tool meson" "meson --version"
    tool_line "tool ninja" "ninja --version"
    tool_line "tool pkg-config" "\"$PKG_CONFIG\" --version"
    tool_line "tool cmake" "cmake --version"
    tool_line "tool python3" "\"$python\" --version"
    if [ "$platform" = linux ]; then
        [ -n "$only" ] || tool_line "tool gperf" "gperf --version"
        if [ "$arch" = x86_64 ] && is_selected libjpeg-turbo; then tool_line "tool nasm" "nasm -v"; fi
        tool_line "tool libc" "ldd --version"
    else
        is_selected fontconfig && tool_line "tool gperf" "gperf --version"
        tool_line "tool xcode" "xcodebuild -version | tr '\n' ' '"
        tool_line "tool sdk" "xcrun --show-sdk-version"
    fi
    selected | while read -r name version sha _rest; do
        echo "lib $name $version $sha"
    done
} > "$share/manifest.txt"

{
    echo "The cairo stack of this VM ($platform $arch), built by scripts/cairo-stack/build.sh"
    echo "from the archives pinned in cmake/cairo-stack/pins.txt of the VM's sources"
    echo "(docs/CairoStack.md).  No patch is applied: these are the upstream releases."
    echo "Features besides cairo and FreeType: ${feature_list:-none}."
    echo
    echo "Libraries (name, version, SHA256, URL, how the archive was authenticated):"
    awk -v sel=" $sel_names" '
        /^# (signed-by|unsigned)/ { prov = $0; sub(/^# /, "", prov); next }
        /^#/ || NF == 0 || $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
        {
            if (index(sel, " " $1 " ")) printf "    %s %s\n        sha256 %s\n        %s\n        %s\n", $1, $2, $3, $5, prov
            prov = ""
        }' "$pins"
    echo
    echo "Options (scripts/cairo-stack/build.sh, docs/CairoStack.md \"Build options\"):"
    echo "    all: --buildtype=release -Db_ndebug=false (assertions on) --wrap-mode=nofallback,"
    echo "    shared libraries but zlib, expat, libffi, PCRE2 and libjpeg-turbo (static, PIC,"
    echo "    hidden visibility, linked into their users only); $prefix_map;"
    echo "    link flags: $link_args"
    [ -z "$gcc_c$gcc_cxx" ] || echo "    C++: libstdc++ of $gcc_dir ($gcc_cxx)"
    for n in $all_names; do
        a=$(args_of "$n")
        [ -z "$a" ] || echo "    PHARO_CAIRO_STACK_$(echo "$n" | tr 'a-z-' 'A-Z_')_ARGS: $a"
    done
    echo
    echo "Toolchain:"
    grep '^tool ' "$share/manifest.txt" | sed 's/^tool /    /'
    [ "$platform" != darwin ] || echo "    deployment target $MACOSX_DEPLOYMENT_TARGET"
} > "$share/CAIRO-STACK.txt"

note "done: $(selected | awk '{ printf "%s%s %s", sep, $1, $2; sep = ", " }') in $prefix"

if [ -n "$glibc_max" ]; then
    sh "$here/check.sh" --prefix "$prefix" --platform "$platform" --glibc-max "$glibc_max" --pins "$pins"
elif [ "$platform" = darwin ]; then
    # the install names, signatures and minos of the stack, before CMake bundles it
    sh "$here/check.sh" --prefix "$prefix" --platform darwin --arch "$arch" --minos "$MACOSX_DEPLOYMENT_TARGET" --pins "$pins"
fi
