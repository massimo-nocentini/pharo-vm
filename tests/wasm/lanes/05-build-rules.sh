#!/bin/sh
# 05-build-rules - the rules of the GNUmakefile and of webimage.cmake
#
# Builds nothing of the VM.  The GNUmakefile and cmake/emscripten/webimage.cmake
# of SRCDIR are copied into small trees of TEST_DIR, where stand-ins log what
# cmake, emcmake, the Pharo VMs and the specs are asked to do, and the lane
# checks that
#
#   - a goal the GNUmakefile does not define goes to the Makefile of an
#     in-source build, a file of it (src/client.o) too, and that two goals
#     run one after the other under -j;
#   - the guard of WASM_BUILDDIR holds for a goal given as .DEFAULT_GOAL,
#     and for an empty WASM_BUILDDIR;
#   - a .st file of smalltalksrc that comes, goes or changes refreshes the
#     VMMaker image and generates the sources again, and nothing else does;
#   - an edit of cmake/Emscripten.cache.cmake configures the wasm build again,
#     and so does a change of WASM_FFI or WASM_DEPS_DIR, which config.make
#     records (a WASM_DEPS_DIR that does not exist is refused);
#   - wasm-check-browser fails without Playwright, and runs world.spec.mjs
#     only for a build with the image of the world, and ffi.spec.mjs only for
#     one with the FFI;
#   - a class of OSWindow-Web that comes, goes or changes prepares the image
#     of the world again (and one that goes breaks nothing), and nothing
#     else does; the preparation runs the command line that both Pharo 12
#     and Pharo 15 take, and leaves no image of an earlier build, nor the
#     .sources of another stock image, next to the world image;
#   - pharo_wasm_dep_fetch (cmake/emscripten/deps/fetch.cmake) takes an
#     archive from WASM_DEPS_DIR, or downloads it once (from a file:// URL
#     here), unpacks it once, records its notice, and stops at an archive of
#     another SHA256 with a message that names the file and both hashes, or
#     at a missing licence file;
#   - the patch of cmake/emscripten/deps/libffi.cmake writes both widened
#     returns into its copy of src/wasm/ffi.c, and refuses an ffi.c that has
#     the text it replaces zero times or twice.
#
# Environment (from make wasm-check): NODE, SRCDIR, TEST_DIR.  Needs GNU
# make and cmake (from PATH, or MAKE and CMAKE); exits 77 without them.

set -e

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
NODE=${NODE:-node}
# (run by hand without a TEST_DIR, in a temporary one)
if test -z "$TEST_DIR"; then
    TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/build-rules.XXXXXX")
    trap 'rm -rf "$TEST_DIR"' EXIT
fi
make=${MAKE:-make}
cmake=${CMAKE:-cmake}
if ! "$make" --version 2>/dev/null | grep -q '^GNU Make'; then
    echo "skip: no GNU make ($make)"
    exit 77
fi
if ! "$cmake" --version >/dev/null 2>&1; then
    echo "skip: no cmake ($cmake)"
    exit 77
fi
cmake=$(command -v "$cmake")

# The settings of the make running wasm-check, and its own, must not reach
# the makes below.
for v in $(env | sed -n 's/^\(WASM_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
unset MAKEFLAGS MFLAGS MAKELEVEL MAKEOVERRIDES GNUMAKEFLAGS EMSDK EMCMAKE PLAYWRIGHT_MODULE NODE_PATH

T=$TEST_DIR/05-build-rules
rm -rf "$T"
mkdir -p "$T/bin" "$T/home"
log=$T/calls.log
RULES_LOG=$log
export RULES_LOG

fail() {
    echo "FAIL 05-build-rules: $1"
    if test -s "$log"; then echo "(calls:)"; sed 's/^/  /' "$log"; fi
    exit 1
}

# calls WANT...: the calls logged since the last check are WANT, one line
# each (none for no WANT); empties the log
calls() {
    want=$T/want.log
    : >"$want"
    for w in "$@"; do echo "$w" >>"$want"; done
    if ! cmp -s "$want" "$log"; then
        echo "want:"; sed 's/^/  /' "$want"
        fail "$step: other calls than these"
    fi
    : >"$log"
}

# ---- stand-ins

cat >"$T/bin/cmake" <<'EOF'
#!/bin/sh
# cmake: logs a configure (-B DIR) or a build (--build DIR --target T)
case $1 in
--build)
    dir=$2 target=all
    while test $# -gt 0; do
        case $1 in --target) target=$2; shift ;; esac
        shift
    done
    echo "build $(basename "$dir") $target" >>"$RULES_LOG" ;;
*)
    dir=
    while test $# -gt 0; do
        case $1 in -B) dir=$2; shift ;; esac
        shift
    done
    mkdir -p "$dir"
    echo "configure $(basename "$dir")" >>"$RULES_LOG" ;;
esac
EOF
cat >"$T/bin/emcmake" <<'EOF'
#!/bin/sh
echo "emcmake" >>"$RULES_LOG"
exec "$@"
EOF
cat >"$T/bin/vmmaker-vm" <<'EOF'
#!/bin/sh
echo "refresh" >>"$RULES_LOG"
EOF
# (the host Pharo of webimage.cmake: --headless IMAGE --no-default-preferences
# st --save --quit prepare-web-image.st, the command line of both Pharo 12 and
# Pharo 15, with the directories in PHARO_WEB_ST_DIR and PHARO_WEB_IMAGE_DIR,
# where the stock .sources must be already; it logs anything else)
cat >"$T/bin/host-pharo" <<'EOF'
#!/bin/sh
case $#:$1:$2:$3:$4:$5:$6:$7 in
7:--headless:*.image:--no-default-preferences:st:--save:--quit:*/prepare-web-image.st) ;;
*)
    echo "prepare with $*" >>"$RULES_LOG"
    exit 1 ;;
esac
if test -z "$PHARO_WEB_ST_DIR" || test -z "$PHARO_WEB_IMAGE_DIR"; then
    echo "prepare without PHARO_WEB_ST_DIR or PHARO_WEB_IMAGE_DIR" >>"$RULES_LOG"
    exit 1
fi
if ! test -f "$PHARO_WEB_IMAGE_DIR/Pharo.sources"; then
    echo "prepare before the stock .sources is in PHARO_WEB_IMAGE_DIR" >>"$RULES_LOG"
    exit 1
fi
mkdir -p "$PHARO_WEB_IMAGE_DIR"
cat "$PHARO_WEB_ST_DIR"/OSWindow-Web/*.st >"$PHARO_WEB_IMAGE_DIR/OSWindow-Web.st"
: >"$PHARO_WEB_IMAGE_DIR/Pharo-web.image"
: >"$PHARO_WEB_IMAGE_DIR/Pharo-web.changes"
echo "prepare" >>"$RULES_LOG"
EOF
chmod +x "$T/bin/"*

# A source tree: the GNUmakefile, smalltalksrc, the initial cache and the
# specs
src=$T/src
mkdir -p "$src/smalltalksrc/VMMaker" "$src/scripts" "$src/cmake" "$src/tests/wasm/lib"
cp "$SRCDIR/GNUmakefile" "$src/"
echo '"A"' >"$src/smalltalksrc/VMMaker/A.class.st"
echo '"B"' >"$src/smalltalksrc/VMMaker/B.class.st"
: >"$src/scripts/refreshVMMaker.st"
echo 'set(FLAVOUR "StackVM" CACHE STRING "" FORCE)' >"$src/cmake/Emscripten.cache.cmake"
echo 'console.log("page.spec.mjs " + process.argv[2])' >"$src/tests/wasm/page.spec.mjs"
echo 'console.log("world.spec.mjs " + process.argv[2])' >"$src/tests/wasm/world.spec.mjs"
echo 'console.log("ffi.spec.mjs " + process.argv[2])' >"$src/tests/wasm/ffi.spec.mjs"
mkdir -p "$T/vmmaker"
: >"$T/vmmaker/VMMaker.image"

W=$T/bw
knobs="WASM_BUILDDIR=$W CMAKE=$T/bin/cmake EMCMAKE=$T/bin/emcmake NODE=$NODE
  WASM_VMMAKER_IMAGE=$T/vmmaker/VMMaker.image WASM_VMMAKER_VM=$T/bin/vmmaker-vm"

# wmake GOAL...: the GNUmakefile of the tree on GOAL..., with the knobs
wmake() {
    HOME=$T/home "$make" -s --no-print-directory -f "$src/GNUmakefile" -C "$src" $knobs "$@" \
        >"$T/make.out" 2>&1 || { cat "$T/make.out"; fail "$step: make $* failed"; }
}

# ---- the Makefile of an in-source build

step="in-source goals"
insrc=$T/insrc
mkdir -p "$insrc/src"
echo 'int x;' >"$insrc/src/client.c"
cat >"$insrc/Makefile" <<'EOF'
.NOTPARALLEL:
.SUFFIXES:
all install:
	@echo "start $@" >>"$$RULES_LOG"; sleep 1; echo "end $@" >>"$$RULES_LOG"
src/client.o:
	@echo "forwarded $@" >>"$$RULES_LOG"
EOF
(cd "$insrc" && "$make" -s -j4 -f "$src/GNUmakefile" all install) || fail "$step: make -j4 all install failed"
calls "start all" "end all" "start install" "end install"
(cd "$insrc" && "$make" -s -f "$src/GNUmakefile" src/client.o) || fail "$step: make src/client.o failed"
calls "forwarded src/client.o"
if test -e "$insrc/src/client.o"; then fail "$step: a built-in rule made src/client.o"; fi

# ---- the guard of WASM_BUILDDIR

step="guard"
if HOME=$T/home "$make" -s -f "$src/GNUmakefile" -C "$src" .DEFAULT_GOAL=wasm-clean \
    "WASM_BUILDDIR=$src" >"$T/make.out" 2>&1; then
    fail "$step: wasm-clean as .DEFAULT_GOAL ran on the source tree"
fi
grep -q 'is the source tree' "$T/make.out" || { cat "$T/make.out"; fail "$step: no refusal of the source tree"; }
test -f "$src/cmake/Emscripten.cache.cmake" || fail "$step: the source tree lost cmake/"
if HOME=$T/home "$make" -s -f "$src/GNUmakefile" -C "$src" wasm-clean WASM_BUILDDIR= >"$T/make.out" 2>&1; then
    fail "$step: wasm-clean ran with an empty WASM_BUILDDIR"
fi
grep -q 'WASM_BUILDDIR is empty' "$T/make.out" || { cat "$T/make.out"; fail "$step: no refusal of an empty WASM_BUILDDIR"; }

# ---- the generation of the sources

step="first generation"
wmake "$W/host/.generated"
# (the copied VMMaker image is refreshed)
calls "configure host" "build host vmmaker_vm" "build host vmmaker" "refresh" "build host generate-sources"
step="nothing changed"
wmake "$W/host/.generated"
calls
step="a .st file goes"
rm "$src/smalltalksrc/VMMaker/B.class.st"
wmake "$W/host/.generated"
calls "build host vmmaker_vm" "build host vmmaker" "refresh" "build host generate-sources"
step="nothing changed after a .st file went"
wmake "$W/host/.generated"
calls
step="a .st file comes"
echo '"C"' >"$src/smalltalksrc/VMMaker/C.class.st"
wmake "$W/host/.generated"
calls "build host vmmaker_vm" "build host vmmaker" "refresh" "build host generate-sources"
step="a .st file changes"
echo '"A, edited"' >"$src/smalltalksrc/VMMaker/A.class.st"
wmake "$W/host/.generated"
calls "build host vmmaker_vm" "build host vmmaker" "refresh" "build host generate-sources"

# ---- the configuration of the wasm build

step="first configuration"
wmake "$W/cmake/.configured"
calls "emcmake" "configure cmake"
step="nothing changed in the configuration"
wmake "$W/cmake/.configured"
calls
step="the initial cache changes"
echo 'set(GENERATE_SOURCES OFF CACHE BOOL "" FORCE)' >>"$src/cmake/Emscripten.cache.cmake"
wmake "$W/cmake/.configured"
calls "emcmake" "configure cmake"
step="a setting changes"
wmake "$W/cmake/.configured" WASM_SLICE_MS=10
calls "emcmake" "configure cmake"
step="the FFI is turned off"
wmake "$W/cmake/.configured" WASM_FFI=OFF
calls "emcmake" "configure cmake"
grep -qx 'WASM_FFI = OFF' "$W/config.make" || { cat "$W/config.make"; fail "$step: config.make does not record WASM_FFI = OFF"; }
step="the FFI is on again, by default"
wmake "$W/cmake/.configured"
calls "emcmake" "configure cmake"
grep -qx 'WASM_FFI = ON' "$W/config.make" || { cat "$W/config.make"; fail "$step: config.make does not record WASM_FFI = ON"; }
step="a WASM_DEPS_DIR that does not exist"
if HOME=$T/home "$make" -s --no-print-directory -f "$src/GNUmakefile" -C "$src" $knobs \
    "WASM_DEPS_DIR=$T/no-archives" "$W/cmake/.configured" >"$T/make.out" 2>&1; then
    cat "$T/make.out"
    fail "$step: make accepted it"
fi
grep -q "WASM_DEPS_DIR: $T/no-archives does not exist" "$T/make.out" || { cat "$T/make.out"; fail "$step: no refusal"; }
calls
step="a WASM_DEPS_DIR"
mkdir -p "$T/archives"
wmake "$W/cmake/.configured" "WASM_DEPS_DIR=$T/archives"
calls "emcmake" "configure cmake"
grep -qx "WASM_DEPS_DIR = $T/archives" "$W/config.make" || { cat "$W/config.make"; fail "$step: config.make does not record it"; }
step="nothing changed with a WASM_DEPS_DIR"
wmake "$W/cmake/.configured" "WASM_DEPS_DIR=$T/archives"
calls

# ---- wasm-check-browser (on a web/ of its own; wasm is taken as built)

mkdir -p "$W/web"
echo '{ "world": false, "ffi": false }' >"$W/web/manifest.json"
step="wasm-check-browser without Playwright"
if (cd "$src/tests/wasm/lib" && HOME=$T/home "$NODE" -e 'require.resolve("playwright")') >/dev/null 2>&1; then
    echo "05-build-rules: node finds a playwright package here: the goal without one is not checked"
elif HOME=$T/home "$make" -s -f "$src/GNUmakefile" -C "$src" $knobs -o wasm wasm-check-browser \
    >"$T/make.out" 2>&1; then
    cat "$T/make.out"
    fail "$step: it succeeded"
elif ! grep -q 'need Playwright' "$T/make.out" || grep -q 'spec.mjs /' "$T/make.out"; then
    cat "$T/make.out"
    fail "$step: no message, or a spec ran"
fi
step="wasm-check-browser, no image of the world, no FFI"
PLAYWRIGHT_MODULE=$T/playwright wmake -o wasm wasm-check-browser
grep -q "^page.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: page.spec.mjs did not run"; }
if grep -q "^world.spec.mjs" "$T/make.out" || ! grep -q "^skip world.spec.mjs" "$T/make.out"; then
    cat "$T/make.out"
    fail "$step: world.spec.mjs ran, or no skip was reported"
fi
if grep -q "^ffi.spec.mjs" "$T/make.out" || ! grep -q "^skip ffi.spec.mjs" "$T/make.out"; then
    cat "$T/make.out"
    fail "$step: ffi.spec.mjs ran, or no skip was reported"
fi
step="wasm-check-browser, with the image of the world and the FFI"
printf '{\n  "world": true,\n  "ffi": true\n}\n' >"$W/web/manifest.json"
PLAYWRIGHT_MODULE=$T/playwright wmake -o wasm wasm-check-browser
grep -q "^world.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: world.spec.mjs did not run"; }
grep -q "^ffi.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: ffi.spec.mjs did not run"; }
if grep -q "^skip " "$T/make.out"; then cat "$T/make.out"; fail "$step: a spec was skipped"; fi
step="wasm-check-browser, playwright found by node"
mkdir -p "$src/tests/node_modules/playwright"
echo 'module.exports = {}' >"$src/tests/node_modules/playwright/index.js"
wmake -o wasm wasm-check-browser
grep -q "^page.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: page.spec.mjs did not run"; }
: >"$log"

# ---- webimage.cmake

wi=$T/webimage
st=$wi/packaging/emscripten/st
mkdir -p "$wi/cmake/emscripten" "$st/OSWindow-Web" "$wi/stock"
cp "$SRCDIR/cmake/emscripten/webimage.cmake" "$wi/cmake/emscripten/"
: >"$st/prepare-web-image.st"
echo 'Class { #name : #OSWebA }' >"$st/OSWindow-Web/OSWebA.class.st"
echo 'Class { #name : #OSWebB }' >"$st/OSWindow-Web/OSWebB.class.st"
: >"$wi/stock/Pharo.image"
: >"$wi/stock/Pharo.changes"
: >"$wi/stock/Pharo.sources"
cat >"$wi/CMakeLists.txt" <<EOF
cmake_minimum_required(VERSION 3.10)
project(webimage NONE)
set(WASM_HOST_PHARO "$T/bin/host-pharo")
set(WASM_STAGE_DIR "\${CMAKE_CURRENT_BINARY_DIR}")
set(WASM_STOCK_IMAGE_DIR "\${CMAKE_CURRENT_SOURCE_DIR}/stock")
set(WASM_STOCK_IMAGE_DEPENDS "\${WASM_STOCK_IMAGE_DIR}/Pharo.image")
include(cmake/emscripten/webimage.cmake)
EOF
wb=$T/webimage-build
package=$wb/image/web/OSWindow-Web.st

# wbuild: builds the tree of webimage.cmake
wbuild() {
    "$cmake" --build "$wb" >"$T/build.out" 2>&1 || { cat "$T/build.out"; fail "$step: the build failed"; }
}

step="webimage.cmake, first build"
# (over the image of an earlier build, under its former name, and the .sources
# of another stock image, which stage.mjs would take for a second image and
# .sources)
mkdir -p "$wb/image/web"
for f in Pharo12-web.image Pharo12-web.changes Other.sources; do : >"$wb/image/web/$f"; done
"$cmake" -S "$wi" -B "$wb" >"$T/configure.out" 2>&1 || "$cmake" -H"$wi" -B"$wb" >"$T/configure.out" 2>&1 ||
    { cat "$T/configure.out"; fail "$step: configure failed"; }
wbuild
calls "prepare"
grep -q OSWebB "$package" || fail "$step: no OSWebB in $package"
for f in Pharo12-web.image Pharo12-web.changes Other.sources; do
    if test -e "$wb/image/web/$f"; then fail "$step: $f is still in $wb/image/web"; fi
done
for f in Pharo-web.image Pharo-web.changes Pharo.sources; do
    test -f "$wb/image/web/$f" || fail "$step: no $f in $wb/image/web"
done
step="webimage.cmake, nothing changed"
wbuild
calls
step="webimage.cmake, a class comes"
echo 'Class { #name : #OSWebC }' >"$st/OSWindow-Web/OSWebC.class.st"
wbuild
calls "prepare"
grep -q OSWebC "$package" || fail "$step: no OSWebC in $package"
step="webimage.cmake, the new class changes"
echo '"OSWebC, edited"' >>"$st/OSWindow-Web/OSWebC.class.st"
wbuild
calls "prepare"
grep -q 'OSWebC, edited' "$package" || fail "$step: the edit is not in $package"
step="webimage.cmake, a class goes"
rm "$st/OSWindow-Web/OSWebB.class.st"
wbuild
calls "prepare"
if grep -q OSWebB "$package"; then fail "$step: OSWebB is still in $package"; fi
step="webimage.cmake, nothing changed after a class went"
wbuild
calls

# ---- pharo_wasm_dep_fetch (fetch.cmake, in script mode)

fe=$T/fetch
mkdir -p "$fe/lib-1.0" "$fe/archives" "$fe/run"
echo 'int lib;' >"$fe/lib-1.0/lib.c"
echo 'The licence of lib' >"$fe/lib-1.0/COPYING"
(cd "$fe" && "$cmake" -E tar czf lib-1.0.tar.gz lib-1.0) || fail "fetch: cannot make the archive"
sha=$("$cmake" -E sha256sum "$fe/lib-1.0.tar.gz" | sed 's/ .*//')
other=0000000000000000000000000000000000000000000000000000000000000000
# (the script: SHA256, LICENSES and WASM_DEPS_DIR from the environment)
cat >"$fe/fetch-lib.cmake" <<EOF
set(WASM_STAGE_DIR "$fe/stage")
set(WASM_DEPS_DIR "\$ENV{DEPS}")
include("$SRCDIR/cmake/emscripten/deps/fetch.cmake")
pharo_wasm_dep_fetch(lib VERSION 1.0 URL "file://$fe/lib-1.0.tar.gz"
    SHA256 "\$ENV{SHA}" FILE lib-1.0.tar.gz LICENSES \$ENV{LICENSES})
get_property(notices GLOBAL PROPERTY PHARO_WASM_NOTICES)
message("source: \${lib_SOURCE_DIR}")
message("notices: \${notices}")
EOF
# fetch WANT-STATUS DEPS SHA LICENSES: runs the script in fe/run, into fe/fetch.out
fetch() {
    if (cd "$fe/run" && DEPS=$2 SHA=$3 LICENSES=$4 "$cmake" -P "$fe/fetch-lib.cmake") >"$fe/fetch.out" 2>&1; then
        status=ok
    else
        status=failed
    fi
    test $status = "$1" || { cat "$fe/fetch.out"; fail "$step: the fetch $status"; }
}
# said TEXT: the message of the fetch says TEXT (CMake folds the lines of
# its errors)
said() {
    tr '\n' ' ' <"$fe/fetch.out" | sed 's/   */ /g' | grep -qF "$1"
}
notice="notices: lib|1.0|file://$fe/lib-1.0.tar.gz|$sha|$fe/run/deps/lib/lib-1.0/COPYING"

step="fetch, from WASM_DEPS_DIR"
cp "$fe/lib-1.0.tar.gz" "$fe/archives/"
fetch ok "$fe/archives" "$sha" COPYING
grep -q "^-- Unpacking $fe/archives/lib-1.0.tar.gz" "$fe/fetch.out" || { cat "$fe/fetch.out"; fail "$step: not unpacked from WASM_DEPS_DIR"; }
if grep -q "Downloading" "$fe/fetch.out" || test -e "$fe/stage/downloads/lib-1.0.tar.gz"; then fail "$step: it downloaded"; fi
grep -qx "source: $fe/run/deps/lib/lib-1.0" "$fe/fetch.out" || { cat "$fe/fetch.out"; fail "$step: not the source directory"; }
grep -qxF "$notice" "$fe/fetch.out" || { cat "$fe/fetch.out"; fail "$step: not the notice"; }
test -f "$fe/run/deps/lib/lib-1.0/lib.c" || fail "$step: lib.c is not unpacked"
step="fetch, from WASM_DEPS_DIR again"
fetch ok "$fe/archives" "$sha" COPYING
if grep -q "Unpacking" "$fe/fetch.out"; then cat "$fe/fetch.out"; fail "$step: unpacked again"; fi
step="fetch, another SHA256 in WASM_DEPS_DIR"
fetch failed "$fe/archives" "$other" COPYING
for w in "$fe/archives/lib-1.0.tar.gz is not lib 1.0" "$sha" "$other" "WASM_DEPS_DIR"; do
    said "$w" || { cat "$fe/fetch.out"; fail "$step: the message does not name $w"; }
done
step="fetch, a missing licence file"
fetch failed "$fe/archives" "$sha" "COPYING;LICENSE"
said "has no licence file LICENSE" || { cat "$fe/fetch.out"; fail "$step: no message"; }
step="fetch, downloaded"
fetch ok "" "$sha" COPYING
grep -q "^-- Downloading file://$fe/lib-1.0.tar.gz" "$fe/fetch.out" || { cat "$fe/fetch.out"; fail "$step: not downloaded"; }
test -f "$fe/stage/downloads/lib-1.0.tar.gz" || fail "$step: not in the downloads"
if test -e "$fe/stage/downloads/lib-1.0.tar.gz.part"; then fail "$step: the .part is left"; fi
step="fetch, downloaded already"
fetch ok "$fe/no-archives" "$sha" COPYING
if grep -q "Downloading" "$fe/fetch.out"; then cat "$fe/fetch.out"; fail "$step: downloaded again"; fi
step="fetch, a download of another SHA256"
rm -rf "$fe/stage/downloads"
fetch failed "" "$other" COPYING
for w in "is not lib 1.0" "$sha" "$other" "WASM_DEPS_DIR"; do
    said "$w" || { cat "$fe/fetch.out"; fail "$step: the message does not name $w"; }
done
if test -e "$fe/stage/downloads/lib-1.0.tar.gz" || test -e "$fe/stage/downloads/lib-1.0.tar.gz.part"; then
    fail "$step: the download is kept"
fi

# ---- the return patch of libffi.cmake (in script mode)

ff=$T/libffi
mkdir -p "$ff"
anchor='  case FFI_TYPE_UINT64:
  case FFI_TYPE_SINT64:
    DEREF_U64(rvalue, 0) = result;
    break;'
printf 'switch (rtype_id) {\n%s\n  case FFI_TYPE_POINTER:\n}\n' "$anchor" >"$ff/once.c"
printf 'switch (rtype_id) {\n  case FFI_TYPE_POINTER:\n}\n' >"$ff/none.c"
printf 'switch (rtype_id) {\n%s\n%s\n}\n' "$anchor" "$anchor" >"$ff/twice.c"
# widen FFI_C: the patch of FFI_C into ff/out.c, its output into ff/widen.out
widen() {
    rm -f "$ff/out.c"
    "$cmake" -DFFI_C="$ff/$1" -DOUTPUT="$ff/out.c" -P "$SRCDIR/cmake/emscripten/deps/libffi.cmake" >"$ff/widen.out" 2>&1
}
step="libffi, the patch"
widen once.c || { cat "$ff/widen.out"; fail "$step: it failed"; }
grep -qF "DEREF_U64(rvalue, 0) = typeof result === 'bigint' ? result : BigInt(result >>> 0);" "$ff/out.c" ||
    { cat "$ff/out.c"; fail "$step: no zero-extended UINT64"; }
grep -qF "DEREF_U64(rvalue, 0) = typeof result === 'bigint' ? result : BigInt(result | 0);" "$ff/out.c" ||
    { cat "$ff/out.c"; fail "$step: no sign-extended SINT64"; }
if grep -qF "DEREF_U64(rvalue, 0) = result;" "$ff/out.c"; then cat "$ff/out.c"; fail "$step: the old return is left"; fi
test "$(grep -c 'break;' "$ff/out.c")" = 2 || { cat "$ff/out.c"; fail "$step: not two cases"; }
step="libffi, no text to replace"
if widen none.c; then fail "$step: it was accepted"; fi
grep -q "has no return of the 64-bit integers" "$ff/widen.out" || { cat "$ff/widen.out"; fail "$step: no message"; }
if test -e "$ff/out.c"; then fail "$step: it wrote the copy"; fi
step="libffi, the text twice"
if widen twice.c; then fail "$step: it was accepted"; fi
grep -q "more than once" "$ff/widen.out" || { cat "$ff/widen.out"; fail "$step: no message"; }
if test -e "$ff/out.c"; then fail "$step: it wrote the copy"; fi

echo "05-build-rules: all checks passed"
