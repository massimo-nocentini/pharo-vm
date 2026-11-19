#!/bin/sh
# shellcheck disable=SC2016,SC2018,SC2019,SC2086  # $ORIGIN is literal; names are ASCII; word splitting of lists is wanted
# The cairo stack of the native VMs (docs/CairoStack.md): zlib, libpng,
# FreeType, expat, fontconfig, pixman and cairo, built from the pinned archives
# of cmake/cairo-stack/pins.txt, each with its own upstream build system
# (meson for FreeType, fontconfig, pixman and cairo; CMake for zlib, libpng and
# expat), into one prefix.
#
#   build.sh --platform linux|darwin|mingw --prefix <dir>
#            [--work <dir>] [--deps <dir>] [--downloads <dir>] [--arch <arch>]
#            [--only freetype] [--pins <file>] [--glibc-max <x.y>] [--jobs <n>]
#
#   --platform  linux or darwin build here; mingw runs fetch-msys2.sh, which
#               unpacks MSYS2's pinned packages instead (decision U2).
#   --prefix    the stack: include/, lib/, share/pharo-cairo-stack/{manifest.txt,
#               CAIRO-STACK.txt, licenses/<name>/, meson-summaries/<name>.txt}.
#               It is emptied first, if it is a stack prefix (.pharo-cairo-stack).
#   --work      sources, build trees, the fontconfig staging (default <prefix>.work)
#   --deps      a directory of archives (WASM_DEPS_DIR), looked into first, then
#               $PHARO_CAIRO_STACK_DOWNLOADS, then --downloads; anything else
#               is downloaded (HTTPS only) from the URL, or the mirror, into
#               --downloads.  Every SHA256 is checked before any extraction.
#   --arch      darwin: the one architecture (x86_64 or arm64), the host's
#   --only freetype   zlib, libpng and FreeType only (FEATURE_LIB_FREETYPE2 alone)
#   --glibc-max linux: then runs check.sh --prefix with --glibc-max
#
# Environment: CC, CXX (default clang, clang++ when present), MACOSX_DEPLOYMENT_TARGET
# (darwin, default 11.0), PHARO_CAIRO_STACK_<NAME>_ARGS (extra meson or CMake
# arguments for one library, NAME in ZLIB LIBPNG FREETYPE EXPAT FONTCONFIG
# PIXMAN CAIRO, recorded in the manifest).
# Tools: meson >= 1.11, ninja, pkg-config (or pkgconf), cmake, and gperf on
# Linux (scripts/cairo-stack/requirements.txt has meson, ninja and pkgconf).
#
# POSIX sh: dash, bash, macOS sh.

set -eu

script_version=1
here=$(cd "$(dirname "$0")" && pwd)
pins=$here/../../cmake/cairo-stack/pins.txt
platform=
prefix=
work=
deps=
downloads=
arch=
only=
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
        --only) only=$2; shift 2 ;;
        --pins) pins=$2; shift 2 ;;
        --glibc-max) glibc_max=$2; shift 2 ;;
        --jobs) jobs=$2; shift 2 ;;
        -h|--help) usage 0 ;;
        *) echo "build.sh: unknown argument $1" >&2; usage 1 >&2 ;;
    esac
done

[ -n "$prefix" ] || die "no --prefix"
case $platform in
    linux|darwin) ;;
    mingw)
        exec sh "$here/fetch-msys2.sh" --prefix "$prefix" ${arch:+--arch "$arch"} ${work:+--work "$work"} \
            ${deps:+--deps "$deps"} ${downloads:+--downloads "$downloads"} --pins "$pins" ;;
    *) die "--platform linux, darwin or mingw" ;;
esac
case $only in ''|freetype) ;; *) die "--only freetype, or nothing" ;; esac
[ -f "$pins" ] || die "no $pins"
abspath() { case $1 in /*) echo "$1" ;; *) echo "$(pwd)/$1" ;; esac; }
prefix=$(abspath "$prefix")
[ -n "$work" ] || work=$prefix.work
work=$(abspath "$work")
[ -n "$downloads" ] || downloads=$work/downloads
downloads=$(abspath "$downloads")
[ -z "$deps" ] || deps=$(abspath "$deps")
pins=$(abspath "$pins")

host_arch=$(uname -m)
if [ "$platform" = darwin ]; then
    [ -n "$arch" ] || arch=$host_arch
    [ "$arch" = "$host_arch" ] || die "--arch $arch is not this Mac's ($host_arch): the stack is built natively, one architecture"
    : "${MACOSX_DEPLOYMENT_TARGET:=11.0}"
    export MACOSX_DEPLOYMENT_TARGET
else
    [ -z "$arch" ] || [ "$arch" = "$host_arch" ] || die "--arch $arch is not this machine's ($host_arch)"
    arch=$host_arch
fi

# ---------------------------------------------------------------- tools
if [ -z "${CC:-}" ]; then
    if command -v clang >/dev/null 2>&1; then CC=clang; else CC=cc; fi
fi
if [ -z "${CXX:-}" ]; then
    if command -v clang++ >/dev/null 2>&1; then CXX=clang++; else CXX=c++; fi
fi
export CC CXX
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
export PKG_CONFIG
meson_version=$(meson --version)
case $meson_version in
    0.*|1.[0-9]|1.[0-9].*|1.10|1.10.*) die "meson $meson_version: fontconfig 2.18 needs meson >= 1.11 (pip install -r scripts/cairo-stack/requirements.txt)" ;;
esac
if [ "$platform" = linux ] && [ -z "$only" ]; then
    command -v gperf >/dev/null 2>&1 || die "gperf is not on PATH: fontconfig needs it (apt install gperf)"
fi

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# ---------------------------------------------------------------- the pins
# "<name> <version> <sha256> <file> <url> <platforms> <licences...>" of this platform
selected() {
    awk -v platform="$platform" -v only="$only" '
        /^[ \t]*#/ || NF == 0 { next }
        $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
        NF < 7 { print "build.sh: bad line in pins.txt: " $0 > "/dev/stderr"; exit 1 }
        {
            n = split($6, ps, ",")
            ok = ($6 == "all")
            for (i = 1; i <= n; i++) if (ps[i] == platform) ok = 1
            if (only == "freetype" && $1 != "zlib" && $1 != "libpng" && $1 != "freetype") ok = 0
            if (ok) print
        }' "$pins"
}
mirror_of() { awk -v n="$1" '$1 == "mirror" && $2 == n { print $3 }' "$pins"; }
selected > /dev/null || die "bad $pins"
[ -n "$(selected)" ] || die "$pins has nothing for $platform"

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

# 2. a fresh prefix
# (a prefix that holds only empty directories counts as fresh: Ninja makes
# the directories of an ExternalProject's byproducts before running it)
if [ -d "$prefix" ] && [ -n "$(find "$prefix" ! -type d 2>/dev/null | head -n 1)" ] && [ ! -f "$prefix/.pharo-cairo-stack" ]; then
    die "$prefix is not empty and is not a cairo stack prefix (no .pharo-cairo-stack): not touching it"
fi
rm -rf "$prefix"
mkdir -p "$prefix/lib/pkgconfig" "$prefix/include" "$prefix/share/pharo-cairo-stack/licenses" "$prefix/share/pharo-cairo-stack/meson-summaries"
: > "$prefix/.pharo-cairo-stack"
rm -rf "$work/src" "$work/build" "$work/stage"
mkdir -p "$work/src" "$work/build"
touch "$work/.started"
share=$prefix/share/pharo-cairo-stack

# ---------------------------------------------------------------- options
# Only the prefix: pkg-config sees nothing else, and no subproject is built
PKG_CONFIG_LIBDIR=$prefix/lib/pkgconfig
export PKG_CONFIG_LIBDIR
unset PKG_CONFIG_PATH || true
unset PKG_CONFIG_SYSROOT_DIR || true

case $platform in
    linux)
        # DT_RPATH $ORIGIN (searched before LD_LIBRARY_PATH), zlib and expat not re-exported
        link_args='-Wl,--disable-new-dtags,-rpath,$ORIGIN -Wl,--exclude-libs,libz.a:libexpat.a'
        cmake_link_args='-Wl,--disable-new-dtags -Wl,--exclude-libs,libz.a:libexpat.a'   # + CMAKE_INSTALL_RPATH
        rpath='$ORIGIN'
        c_args=
        ;;
    darwin)
        link_args="-arch $arch -Wl,-rpath,@loader_path"
        cmake_link_args=   # CMAKE_OSX_ARCHITECTURES and CMAKE_INSTALL_RPATH
        rpath=@loader_path
        c_args="-arch $arch"
        ;;
esac

# meson's array syntax for a list of words: ['a', 'b']
meson_array() {
    m_out=
    for m_w in $1; do m_out="$m_out${m_out:+, }'$m_w'"; done
    echo "[$m_out]"
}

args_of() {
    a_var=PHARO_CAIRO_STACK_$(echo "$1" | tr 'a-z' 'A-Z')_ARGS
    eval "echo \"\${$a_var:-}\""
}

meson_common() {
    echo "--prefix=$prefix --libdir=lib --includedir=include --buildtype=release -Db_ndebug=false --wrap-mode=nofallback -Ddefault_library=shared"
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
        -Dc_args="$(meson_array "$c_args")" -Dcpp_args="$(meson_array "$c_args")" \
        -Dc_link_args="$(meson_array "$link_args")" -Dcpp_link_args="$(meson_array "$link_args")" \
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

build_cmake() {
    b_name=$1 b_src=$2; shift 2
    b_dir=$work/build/$b_name
    set -- -S "$b_src" -B "$b_dir" -G Ninja \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_INSTALL_INCLUDEDIR=include \
        -DCMAKE_PREFIX_PATH="$prefix" -DCMAKE_FIND_USE_CMAKE_SYSTEM_PATH=OFF -DCMAKE_FIND_USE_PACKAGE_REGISTRY=OFF \
        -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
        -DCMAKE_C_COMPILER="$CC" \
        -DCMAKE_SHARED_LINKER_FLAGS="$cmake_link_args" \
        -DCMAKE_INSTALL_RPATH="$rpath" -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON \
        "$@"
    if [ "$platform" = darwin ]; then
        set -- "$@" -DCMAKE_OSX_ARCHITECTURES="$arch" -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOSX_DEPLOYMENT_TARGET" \
            -DCMAKE_INSTALL_NAME_DIR=@rpath -DCMAKE_MACOSX_RPATH=ON \
            "-DCMAKE_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local" "-DCMAKE_SYSTEM_IGNORE_PREFIX_PATH=/opt/homebrew;/usr/local"
    fi
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
                -DCMAKE_POSITION_INDEPENDENT_CODE=ON
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
                -DEXPAT_BUILD_TESTS=OFF -DEXPAT_BUILD_DOCS=OFF -DEXPAT_BUILD_FUZZERS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON
            ;;
        fontconfig)
            build_meson fontconfig "$src" -Ddoc=disabled -Dtests=disabled -Dtools=disabled -Dcache-build=disabled \
                -Dnls=disabled -Diconv=disabled -Dfontations=disabled -Dxml-backend=expat \
                -Dbaseconfig-dir=/etc/fonts -Dconfig-dir=/etc/fonts/conf.d \
                -Dtemplate-dir=/usr/share/fontconfig/conf.avail -Dxml-dir=/usr/share/xml/fontconfig \
                -Dcache-dir=/var/cache/fontconfig -Ddefault-fonts-dirs=/usr/share/fonts,/usr/local/share/fonts
            stage=$work/stage/fontconfig
            rm -rf "$stage"
            install_meson fontconfig --destdir "$stage"
            [ -d "$stage$prefix/lib" ] || die "fontconfig: nothing staged in $stage$prefix/lib"
            cp -R "$stage$prefix/lib/." "$prefix/lib/"
            cp -R "$stage$prefix/include/." "$prefix/include/"
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
        *) die "pins.txt names $name, which build.sh does not know how to build" ;;
    esac
    check_outside "$name"
done < "$work/archives.txt"

# Nothing of the system's: no X11, no other zlib, no foreign prefix in the .pc files
if grep -l -E '/opt/homebrew|/usr/local/(lib|include)|/ucrt64|/clangarm64' "$prefix"/lib/pkgconfig/*.pc 2>/dev/null; then
    die "a .pc file of the prefix names a foreign prefix"
fi

# ---------------------------------------------------------------- manifest, index
version_of() { selected | awk -v n="$1" '$1 == n { print $2 }'; }
tool_line() { printf '%s %s\n' "$1" "$(eval "$2" 2>/dev/null | head -n 1 | tr -s ' \t' '  ')"; }
{
    echo "# pharo-cairo-stack manifest (docs/CairoStack.md)"
    echo "format 1"
    echo "pins-sha256 $(sha256 "$pins")"
    echo "platform $platform"
    echo "arch $arch"
    if [ "$platform" = darwin ]; then echo "deployment-target $MACOSX_DEPLOYMENT_TARGET"; else echo "deployment-target -"; fi
    echo "script build.sh $script_version"
    echo "only ${only:--}"
    for n in zlib libpng freetype expat fontconfig pixman cairo; do
        a=$(args_of "$n")
        [ -z "$a" ] || echo "args $(echo "$n" | tr 'a-z' 'A-Z') $a"
    done
    tool_line "tool cc" "\"$CC\" --version"
    tool_line "tool meson" "meson --version"
    tool_line "tool ninja" "ninja --version"
    tool_line "tool pkg-config" "\"$PKG_CONFIG\" --version"
    tool_line "tool cmake" "cmake --version"
    if [ "$platform" = linux ]; then
        [ -z "$only" ] && tool_line "tool gperf" "gperf --version"
        tool_line "tool libc" "ldd --version"
    else
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
    echo
    echo "Libraries (name, version, SHA256, URL, how the archive was authenticated):"
    awk -v platform="$platform" -v only="$only" '
        /^# (signed-by|unsigned)/ { prov = $0; sub(/^# /, "", prov); next }
        /^#/ || NF == 0 || $1 == "mirror" || $1 == "native-ahead" || $1 == "accepted-behind" || $1 ~ /^msys2/ { next }
        {
            n = split($6, ps, ","); ok = ($6 == "all")
            for (i = 1; i <= n; i++) if (ps[i] == platform) ok = 1
            if (only == "freetype" && $1 != "zlib" && $1 != "libpng" && $1 != "freetype") ok = 0
            if (ok) printf "    %s %s\n        sha256 %s\n        %s\n        %s\n", $1, $2, $3, $5, prov
            prov = ""
        }' "$pins"
    echo
    echo "Options (scripts/cairo-stack/build.sh, docs/CairoStack.md \"Build options\"):"
    echo "    all: --buildtype=release -Db_ndebug=false (assertions on) --wrap-mode=nofallback,"
    echo "    shared libraries but zlib and expat (static, linked into their users);"
    echo "    link flags: $link_args"
    for n in zlib libpng freetype expat fontconfig pixman cairo; do
        a=$(args_of "$n")
        [ -z "$a" ] || echo "    PHARO_CAIRO_STACK_$(echo "$n" | tr 'a-z' 'A-Z')_ARGS: $a"
    done
    echo
    echo "Toolchain:"
    grep '^tool ' "$share/manifest.txt" | sed 's/^tool /    /'
    [ "$platform" != darwin ] || echo "    deployment target $MACOSX_DEPLOYMENT_TARGET"
} > "$share/CAIRO-STACK.txt"

note "done: $(selected | awk '{ printf "%s%s %s", sep, $1, $2; sep = ", " }') in $prefix"

if [ -n "$glibc_max" ]; then
    sh "$here/check.sh" --prefix "$prefix" --platform "$platform" --glibc-max "$glibc_max" --pins "$pins"
fi
