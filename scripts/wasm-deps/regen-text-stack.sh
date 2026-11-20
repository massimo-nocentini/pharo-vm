#!/bin/sh
# regen-text-stack.sh - make again what cmake/emscripten/deps builds Pango
# and Poppler from (WASM_PANGO, WASM_POPPLER; docs/WebAssembly.md)
#
#   sh scripts/wasm-deps/regen-text-stack.sh <work dir> <archives dir>
#
# The Emscripten VM builds GLib, FriBidi, HarfBuzz, expat, fontconfig, Pango,
# libjpeg-turbo and Poppler with CMake alone, from their pinned archives and
# the patches of cmake/emscripten/deps/<library>/patches: `make wasm' needs
# no meson, ninja, pkg-config, gperf nor glib-mkenums.  What their own
# builds generate (config headers, enumeration types, gperf hashes...) and
# the sources and flags of each library are committed instead, in
# cmake/emscripten/deps/<library>/sources.cmake and gen/.  This script makes
# them: it builds the stack for Emscripten (64-bit objects, WebAssembly
# setjmp/longjmp, static) with each library's own meson or CMake, in
# <work dir>, from the archives of <archives dir> (each checked against
# its SHA256), and harvests each build with scripts/wasm-deps/harvest.py.
# It also writes cairo's configurations with fontconfig
# (cmake/emscripten/deps/cairo/config-fc.h, cairo-features-fc.h and their
# -pdf-fc twins), which Pango needs.  Run it again after a bump of one of
# these libraries, or of a patch, and review the diff.
#
# Needs: emsdk 6.0.10 (emcc, em++, emar on the PATH: source emsdk_env.sh),
# meson >= 1.4 and ninja (pip install meson ninja), pkg-config (pkgconf),
# gperf (fontconfig), python3, cmake >= 3.28 and patch.  The patches are
# applied with patch -p1, in the order of their names.  It was run with
# meson 1.12.1, ninja 1.13.2 and pkgconf 2.5 from pip, and gperf 3.1.

set -eu

die() { echo "regen-text-stack: $*" >&2; exit 1; }
[ $# -eq 2 ] || die "usage: $0 <work dir> <archives dir>"
repo=$(cd "$(dirname "$0")/../.." && pwd)
deps=$repo/cmake/emscripten/deps
pins=$repo/cmake/cairo-stack/pins.txt
mkdir -p "$1"
work=$(cd "$1" && pwd)
dl=$(cd "$2" && pwd)
for tool in emcc em++ emar meson ninja pkg-config gperf python3 cmake patch; do
    command -v "$tool" >/dev/null 2>&1 || die "no $tool on the PATH"
done
emcc --version | head -1 | grep -q ' 6\.0\.10 ' || echo "regen-text-stack: warning: not emcc 6.0.10" >&2

stage=$work/stage          # DESTDIR of the installs, prefix /usr
prefix=/usr
sysroot_prefix=$stage$prefix
tools=$work/tools
logs=$work/logs
rm -rf "$work/src" "$work/build" "$stage" "$tools" "$logs"
mkdir -p "$work/src" "$work/build" "$sysroot_prefix/lib/pkgconfig" "$sysroot_prefix/include" "$tools" "$logs"

flags="-m64 -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0"

# The archive of <name> (by pins.txt, or the given line), checked, unpacked
# into $work/src/<name>, patched
pin() { awk -v n="$1" '$1 == n && NF >= 6 { print $2, $3, $4; exit }' "$pins"; }
unpack() {
    name=$1
    set -- $(pin "$name")
    [ $# -eq 3 ] || die "no pin of $name in $pins"
    file=$3 sha=$2
    [ -f "$dl/$file" ] || die "no $dl/$file"
    echo "$sha  $dl/$file" | sha256sum -c --quiet - || die "$dl/$file is not the pinned $name"
    mkdir -p "$work/src/$name.unpack"
    tar xf "$dl/$file" -C "$work/src/$name.unpack"
    mv "$work/src/$name.unpack"/* "$work/src/$name"
    rmdir "$work/src/$name.unpack"
    if [ -d "$deps/$name/patches" ]; then
        for p in "$deps/$name/patches"/*.patch; do
            [ -f "$p" ] || continue
            patch -s -p1 -d "$work/src/$name" -i "$p" || die "$p does not apply"
        done
    fi
}

# emcc and em++ without -pthread, which meson's threads dependency adds on
# Emscripten: it would make the module's memory shared.  The VM has no
# threads, and pthread_* are Emscripten's single-thread stubs.
for c in emcc em++; do
    cat > "$tools/$c-nopthread" <<EOF
#!/bin/sh
for a; do shift; [ "\$a" = -pthread ] || set -- "\$@" "\$a"; done
exec $c "\$@"
EOF
    chmod +x "$tools/$c-nopthread"
done
cat > "$tools/wasm64.cross" <<EOF
[binaries]
c = '$tools/emcc-nopthread'
cpp = '$tools/em++-nopthread'
ar = 'emar'
ranlib = 'emranlib'
strip = 'emstrip'
pkg-config = '$(command -v pkg-config)'
[built-in options]
c_args = ['-m64', '-sSUPPORT_LONGJMP=wasm', '-sWASM_LEGACY_EXCEPTIONS=0']
c_link_args = ['-m64', '-sSUPPORT_LONGJMP=wasm', '-sWASM_LEGACY_EXCEPTIONS=0']
cpp_args = ['-m64', '-sSUPPORT_LONGJMP=wasm', '-sWASM_LEGACY_EXCEPTIONS=0']
cpp_link_args = ['-m64', '-sSUPPORT_LONGJMP=wasm', '-sWASM_LEGACY_EXCEPTIONS=0']
[properties]
needs_exe_wrapper = true
pkg_config_libdir = ['$sysroot_prefix/lib/pkgconfig']
sys_root = '$stage'
[host_machine]
system = 'emscripten'
cpu_family = 'wasm64'
cpu = 'wasm64'
endian = 'little'
EOF
export PKG_CONFIG_LIBDIR="$sysroot_prefix/lib/pkgconfig" PKG_CONFIG_SYSROOT_DIR="$stage"
export PATH="$tools:$PATH"

meson_build() {   # meson_build <name> <options...>
    name=$1; shift
    echo "== $name (meson)"
    meson setup "$work/build/$name" "$work/src/$name" --cross-file "$tools/wasm64.cross" \
        --prefix=$prefix --libdir=lib --default-library=static --buildtype=release -Doptimization=2 \
        -Db_ndebug=false --wrap-mode=nodownload "$@" > "$logs/$name.setup.log" 2>&1 \
        || { tail -30 "$logs/$name.setup.log"; die "$name: meson setup failed"; }
    ninja -C "$work/build/$name" > "$logs/$name.build.log" 2>&1 \
        || { tail -30 "$logs/$name.build.log"; die "$name: build failed"; }
    DESTDIR="$stage" meson install -C "$work/build/$name" --tags devel > "$logs/$name.install.log" 2>&1 \
        || { tail -30 "$logs/$name.install.log"; die "$name: install failed"; }
}
cmake_build() {   # cmake_build <name> <options...>
    name=$1; shift
    echo "== $name (cmake)"
    emcmake cmake -S "$work/src/$name" -B "$work/build/$name" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_C_FLAGS="$flags" -DCMAKE_CXX_FLAGS="$flags" -DCMAKE_EXPORT_COMPILE_COMMANDS=ON \
        -DCMAKE_INSTALL_PREFIX=$prefix -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_PREFIX_PATH="$sysroot_prefix" \
        -DCMAKE_FIND_ROOT_PATH="$sysroot_prefix" -DBUILD_SHARED_LIBS=OFF "$@" > "$logs/$name.setup.log" 2>&1 \
        || { tail -30 "$logs/$name.setup.log"; die "$name: cmake configure failed"; }
    cmake --build "$work/build/$name" > "$logs/$name.build.log" 2>&1 \
        || { tail -30 "$logs/$name.build.log"; die "$name: build failed"; }
    DESTDIR="$stage" cmake --install "$work/build/$name" > "$logs/$name.install.log" 2>&1 \
        || { tail -30 "$logs/$name.install.log"; die "$name: install failed"; }
}
pc() {   # pc <name> <version> <libs> <cflags> [Requires]
    printf 'prefix=%s\nName: %s\nDescription: %s\nVersion: %s\nRequires.private: %s\nLibs: -L${prefix}/lib %s\nCflags: %s\n' \
        "$prefix" "$1" "$1" "$2" "${5:-}" "$3" "$4" > "$sysroot_prefix/lib/pkgconfig/$1.pc"
}
harvest() { python3 -I "$repo/scripts/wasm-deps/harvest.py" --redact "$work" "$@"; }

# What the VM builds by hand: zlib, libffi (3.8.0, the VM's) and libpng, as
# cmake/emscripten/deps builds them, for the libraries to find
unpack zlib
mkdir -p "$work/build/zlib"
for s in adler32 compress crc32 deflate gzclose gzlib gzread gzwrite infback inffast inflate inftrees trees uncompr zutil; do
    emcc $flags -O2 -w -DZ_HAVE_UNISTD_H -DZ_HAVE_STDARG_H -c "$work/src/zlib/$s.c" -o "$work/build/zlib/$s.o"
done
emar rcs "$sysroot_prefix/lib/libz.a" "$work/build/zlib"/*.o
cp "$work/src/zlib/zlib.h" "$work/src/zlib/zconf.h" "$sysroot_prefix/include/"
pc zlib "$(pin zlib | cut -d' ' -f1)" -lz "-I\${prefix}/include"

ffi_version=$(sed -n 's/^set(WASM_LIBFFI_VERSION "\(.*\)")$/\1/p' "$deps/libffi.cmake")
ffi_sha=$(sed -n 's/^ *SHA256 \([0-9a-f]\{64\}\)$/\1/p' "$deps/libffi.cmake")
echo "$ffi_sha  $dl/libffi-$ffi_version.tar.gz" | sha256sum -c --quiet - || die "no pinned $dl/libffi-$ffi_version.tar.gz"
mkdir -p "$work/src/libffi" "$work/build/libffi/include"
tar xf "$dl/libffi-$ffi_version.tar.gz" -C "$work/src/libffi" --strip-components=1
f=$work/src/libffi b=$work/build/libffi
sed -e "s/@VERSION@/$ffi_version/;s/@TARGET@/wasm64/;s/@HAVE_LONG_DOUBLE@/1/;s/@FFI_VERSION_STRING@/$ffi_version/;s/@FFI_VERSION_NUMBER@/30800/;s/@FFI_EXEC_TRAMPOLINE_TABLE@/0/" \
    "$f/include/ffi.h.in" > "$b/include/ffi.h"
cp "$f/src/wasm/ffitarget.h" "$b/include/"
printf '#define FFI_NO_RAW_API 1\n#define HAVE_ALLOCA_H 1\n#define HAVE_MEMCPY 1\n#define HAVE_STDINT_H 1\n#define HAVE_STDLIB_H 1\n#define HAVE_STRING_H 1\n#define STDC_HEADERS 1\n#define SIZEOF_DOUBLE 8\n#define SIZEOF_LONG_DOUBLE 16\n#define SIZEOF_SIZE_T 8\n#define HAVE_LONG_DOUBLE 1\n#define FFI_HIDDEN __attribute__ ((visibility ("hidden")))\n' > "$b/include/fficonfig.h"
for s in prep_cif types closures tramp wasm/ffi; do
    emcc $flags -O2 -w -I"$b/include" -I"$f/include" -I"$f/src" -c "$f/src/$s.c" -o "$b/$(basename $s).o"
done
emar rcs "$sysroot_prefix/lib/libffi.a" "$b"/*.o
cp "$b/include/ffi.h" "$b/include/ffitarget.h" "$sysroot_prefix/include/"
pc libffi "$ffi_version" -lffi "-I\${prefix}/include"

unpack libpng
mkdir -p "$work/build/libpng"
cp "$work/src/libpng/scripts/pnglibconf.h.prebuilt" "$work/build/libpng/pnglibconf.h"
for s in png pngerror pngget pngmem pngpread pngread pngrio pngrtran pngrutil pngset pngtrans pngwio pngwrite pngwtran pngwutil; do
    emcc $flags -O2 -w -I"$work/build/libpng" -I"$sysroot_prefix/include" -c "$work/src/libpng/$s.c" -o "$work/build/libpng/$s.o"
done
emar rcs "$sysroot_prefix/lib/libpng16.a" "$work/build/libpng"/*.o
mkdir -p "$sysroot_prefix/include/libpng16"
cp "$work/src/libpng/png.h" "$work/src/libpng/pngconf.h" "$work/build/libpng/pnglibconf.h" "$sysroot_prefix/include/libpng16/"
pc libpng "$(pin libpng | cut -d' ' -f1)" -lpng16 "-I\${prefix}/include/libpng16" zlib
cp "$sysroot_prefix/lib/pkgconfig/libpng.pc" "$sysroot_prefix/lib/pkgconfig/libpng16.pc"

# FreeType with its defaults (the VM's is leaner: the same API), pixman
unpack freetype
meson_build freetype -Dzlib=disabled -Dbzip2=disabled -Dpng=disabled -Dharfbuzz=disabled -Dbrotli=disabled -Dtests=disabled
unpack pixman
meson_build pixman -Dloongson-mmi=disabled -Dmmx=disabled -Dsse2=disabled -Dssse3=disabled -Dvmx=disabled \
    -Darm-simd=disabled -Dneon=disabled -Da64-neon=disabled -Drvv=disabled -Dmips-dspr2=disabled \
    -Dgnu-inline-asm=disabled -Dopenmp=disabled -Dtimers=false -Dgtk=disabled -Dlibpng=disabled \
    -Dtests=disabled -Ddemos=disabled

# expat and fontconfig
unpack expat
cmake_build expat -DEXPAT_SHARED_LIBS=OFF -DEXPAT_BUILD_TOOLS=OFF -DEXPAT_BUILD_EXAMPLES=OFF \
    -DEXPAT_BUILD_TESTS=OFF -DEXPAT_BUILD_DOCS=OFF -DEXPAT_BUILD_FUZZERS=OFF -DEXPAT_BUILD_PKGCONFIG=ON
harvest --src "$work/src/expat" --build "$work/build/expat" --out "$deps/expat" --name "expat $(pin expat | cut -d' ' -f1)" \
    --target expat=EXPAT
unpack fontconfig
meson_build fontconfig -Ddoc=disabled -Dtests=disabled -Dtools=disabled -Dcache-build=disabled -Dnls=disabled \
    -Diconv=disabled -Dfontations=disabled -Dxml-backend=expat -Dbaseconfig-dir=/etc/fonts \
    -Dconfig-dir=/etc/fonts/conf.d -Dtemplate-dir=/usr/share/fontconfig/conf.avail \
    -Dxml-dir=/usr/share/xml/fontconfig -Dcache-dir=/var/cache/fontconfig \
    -Ddefault-fonts-dirs=/usr/share/fonts -Dadditional-fonts-dirs=no
harvest --src "$work/src/fontconfig" --build "$work/build/fontconfig" --out "$deps/fontconfig" \
    --name "fontconfig $(pin fontconfig | cut -d' ' -f1)" \
    --target libfontconfig.a=FONTCONFIG --target src/libpatternlib_internal.a=FONTCONFIG_PATTERN

# cairo with fontconfig: its configurations for the VM (the sources are
# those of cmake/emscripten/deps/cairo/sources*.cmake, checked below)
unpack cairo
for variant in fc pdf-fc; do
    case $variant in fc) zlib=disabled ;; pdf-fc) zlib=enabled ;; esac
    rm -rf "$work/build/cairo"
    meson_build cairo -Ddwrite=disabled -Dfontconfig=enabled -Dfreetype=enabled -Dpng=enabled -Dquartz=disabled \
        -Dtee=disabled -Dxcb=disabled -Dxlib=disabled -Dtests=disabled -Dlzo=disabled -Dglib=disabled \
        -Dspectre=disabled -Dsymbol-lookup=disabled -Dzlib=$zlib
    # cairo's renderers of COLR v1 and OT-SVG glyphs stay out, as in
    # config.h (cmake/emscripten/deps/cairo/README)
    sed -e 's|^#define HAVE_FT_COLR_V1 1$|/* #define HAVE_FT_COLR_V1 1  (off in Pharo: cmake/emscripten/deps/cairo/README) */|' \
        -e 's|^#define HAVE_FT_SVG_DOCUMENT 1$|/* #define HAVE_FT_SVG_DOCUMENT 1  (off in Pharo: cmake/emscripten/deps/cairo/README) */|' \
        "$work/build/cairo/config.h" > "$deps/cairo/config-$variant.h"
    cp "$work/build/cairo/src/cairo-features.h" "$deps/cairo/cairo-features-$variant.h"
    python3 -I -c 'import json, os, sys
for e in json.load(open(sys.argv[1])):
    f = os.path.normpath(os.path.join(e["directory"], e["file"]))
    if os.path.basename(os.path.dirname(f)) == "src": print(os.path.basename(f))' \
        "$work/build/cairo/compile_commands.json" | sort > "$logs/cairo-$variant.sources"
done

# FriBidi, HarfBuzz
unpack fribidi
meson_build fribidi -Ddocs=false -Dbin=false -Dtests=false
harvest --src "$work/src/fribidi" --build "$work/build/fribidi" --out "$deps/fribidi" \
    --name "FriBidi $(pin fribidi | cut -d' ' -f1)" --target lib/libfribidi.a=FRIBIDI
unpack harfbuzz
meson_build harfbuzz -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dcairo=disabled -Dchafa=disabled \
    -Dpng=disabled -Dzlib=disabled -Dicu=disabled -Dgraphite2=disabled -Draster=disabled -Dvector=disabled \
    -Dgpu=disabled -Dsubset=disabled -Dtests=disabled -Dintrospection=disabled -Ddocs=disabled \
    -Dutilities=disabled -Dbenchmark=disabled -Ddirectwrite=disabled -Dgdi=disabled -Dcoretext=disabled
harvest --src "$work/src/harfbuzz" --build "$work/build/harfbuzz" --out "$deps/harfbuzz" \
    --name "HarfBuzz $(pin harfbuzz | cut -d' ' -f1)" --target src/libharfbuzz.a=HARFBUZZ

# GLib: libglib, libgobject, libgmodule and libgio (Pango's font maps are
# GListModels, poppler-glib binds GFile and GInputStream)
unpack glib
meson_build glib --force-fallback-for=gvdb -Dintrospection=disabled -Dselinux=disabled -Dxattr=false \
    -Dlibmount=disabled -Dsysprof=disabled -Dnls=disabled -Dglib_debug=disabled -Dtests=false \
    -Dinstalled_tests=false -Dman-pages=disabled -Ddocumentation=false -Ddtrace=disabled -Dsystemtap=disabled \
    -Dlibelf=disabled -Dgio_module_dir=/dev/null/gio-modules -Dsysconfdir=/etc -Dlocalstatedir=/var
harvest --src "$work/src/glib" --build "$work/build/glib" --out "$deps/glib" \
    --name "GLib $(pin glib | cut -d' ' -f1)" \
    --target glib/libglib-2.0.a=GLIB --target glib/libcharset/libcharset.a=GLIB_CHARSET \
    --target gobject/libgobject-2.0.a=GOBJECT --target gmodule/libgmodule-2.0.a=GMODULE \
    --target gio/libgio-2.0.a=GIO
# glib-mkenums (a python script of GLib), for Pango and poppler-glib, where
# glib-2.0.pc says it is (the tools are not installed: 0001-wasm-vips.patch)
mkdir -p "$sysroot_prefix/bin"
sed -e "s|@PYTHON@|$(command -v python3)|" -e "s|@VERSION@|$(pin glib | cut -d' ' -f1)|g" \
    -e "s|@GLIB_VERSION@|$(pin glib | cut -d' ' -f1)|g" "$work/src/glib/gobject/glib-mkenums.in" > "$sysroot_prefix/bin/glib-mkenums"
chmod +x "$sysroot_prefix/bin/glib-mkenums"

# Pango, with fontconfig and FreeType (pangoft2), and cairo
unpack pango
meson_build pango -Dcairo=enabled -Dlibthai=disabled -Dxft=disabled -Dintrospection=disabled \
    -Ddocumentation=false -Dman-pages=false -Dbuild-testsuite=false -Dbuild-examples=false -Dsysprof=disabled \
    -Dsysconfdir=/etc -Dfontconfig=enabled -Dfreetype=enabled
harvest --src "$work/src/pango" --build "$work/build/pango" --out "$deps/pango" \
    --name "Pango $(pin pango | cut -d' ' -f1)" \
    --target pango/libpango-1.0.a=PANGO --target pango/libpangoft2-1.0.a=PANGOFT2 \
    --target pango/libpangocairo-1.0.a=PANGOCAIRO

# libjpeg-turbo (without SIMD: there is none for WebAssembly) and Poppler,
# with the features of the native VMs (scripts/cairo-stack/build.sh)
unpack libjpeg-turbo
cmake_build libjpeg-turbo -DENABLE_SHARED=OFF -DENABLE_STATIC=ON -DWITH_TURBOJPEG=OFF -DWITH_TOOLS=OFF \
    -DWITH_TESTS=OFF -DWITH_SIMD=OFF -DCMAKE_INSTALL_DEFAULT_LIBDIR=lib
harvest --src "$work/src/libjpeg-turbo" --build "$work/build/libjpeg-turbo" --out "$deps/libjpeg-turbo" \
    --name "libjpeg-turbo $(pin libjpeg-turbo | cut -d' ' -f1)" --target jpeg-static=JPEG
# its symbols, renamed for the VM (cmake/emscripten/deps/poppler.cmake)
{
    printf '%s\n' '/* The global symbols of libjpeg-turbo, renamed for the Emscripten VM:' \
        ' * its JPEGReadWriter2Plugin links a libjpeg of its own (IJG 6b), whose' \
        ' * functions have the same names and other structures.  Force-included' \
        ' * (-include) in libjpeg-turbo and in Poppler, which calls it, by' \
        ' * cmake/emscripten/deps/poppler.cmake.  Generated by' \
        ' * scripts/wasm-deps/regen-text-stack.sh (the defined global symbols of' \
        ' * its libjpeg.a).  Do not edit. */' '#ifndef PHARO_JPEG_PREFIX_H' '#define PHARO_JPEG_PREFIX_H'
    "$(dirname "$(command -v emcc)")/../bin/llvm-nm" --defined-only -g "$sysroot_prefix/lib/libjpeg.a" \
        | awk 'NF == 3 { print $3 }' | sort -u | awk '{ print "#define " $1 " pharo_turbo_" $1 }'
    printf '%s\n' '#endif'
} > "$deps/libjpeg-turbo/pharo-prefix.h"
unpack poppler
cmake_build poppler -DPKG_CONFIG_EXECUTABLE="$(command -v pkg-config)" -DENABLE_GLIB=ON -DENABLE_CPP=OFF \
    -DENABLE_QT5=OFF -DENABLE_QT6=OFF -DENABLE_UTILS=OFF -DENABLE_GOBJECT_INTROSPECTION=OFF -DENABLE_GTK_DOC=OFF \
    -DBUILD_GTK_TESTS=OFF -DBUILD_MANUAL_TESTS=OFF -DBUILD_CPP_TESTS=OFF -DBUILD_QT5_TESTS=OFF \
    -DBUILD_QT6_TESTS=OFF -DENABLE_BOOST=OFF -DENABLE_LIBOPENJPEG=OFF -DENABLE_LCMS=OFF -DENABLE_LIBTIFF=OFF \
    -DENABLE_LIBCURL=OFF -DENABLE_NSS3=OFF -DENABLE_GPGME=OFF -DENABLE_BROTLI=OFF -DENABLE_HARFBUZZ=OFF \
    -DENABLE_LIBJPEG=ON -DENABLE_UNSTABLE_API_ABI_HEADERS=OFF -DRUN_GPERF_IF_PRESENT=OFF \
    -DCMAKE_DISABLE_FIND_PACKAGE_Boost=ON -DCMAKE_DISABLE_FIND_PACKAGE_GTK=ON \
    -DGLIB2_MKENUMS="$sysroot_prefix/bin/glib-mkenums" -DGLIB2_MKENUMS_PYTHON="$(command -v python3)" \
    -DJPEG_LIBRARY="$sysroot_prefix/lib/libjpeg.a" -DJPEG_INCLUDE_DIR="$sysroot_prefix/include" \
    -DFONT_CONFIGURATION=fontconfig -DPOPPLER_DATADIR=/usr/share/poppler
harvest --src "$work/src/poppler" --build "$work/build/poppler" --out "$deps/poppler" \
    --name "Poppler $(pin poppler | cut -d' ' -f1)" --target poppler=POPPLER --target poppler-glib=POPPLER_GLIB

echo "== done: the source lists of cairo with fontconfig, compared with cmake/emscripten/deps/cairo:"
for variant in fc pdf-fc; do
    echo "  $variant: $(wc -l < "$logs/cairo-$variant.sources") sources (in $logs/cairo-$variant.sources)"
done
