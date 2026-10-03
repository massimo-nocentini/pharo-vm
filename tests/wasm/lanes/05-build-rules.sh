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
#   - an edit of cmake/Emscripten.cache.cmake configures the wasm build again;
#   - wasm-check-browser fails without Playwright, and runs world.spec.mjs
#     only for a build with the image of the world;
#   - a class of OSWindow-Web that comes, goes or changes prepares the image
#     of the world again (and one that goes breaks nothing), and nothing
#     else does.
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
# --save --quit prepare-web-image.st ST-DIR WEB-IMAGE-DIR)
cat >"$T/bin/host-pharo" <<'EOF'
#!/bin/sh
mkdir -p "$8"
cat "$7"/OSWindow-Web/*.st >"$8/OSWindow-Web.st"
: >"$8/Pharo12-web.image"
: >"$8/Pharo12-web.changes"
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

# ---- wasm-check-browser (on a web/ of its own; wasm is taken as built)

mkdir -p "$W/web"
echo '{ "world": false }' >"$W/web/manifest.json"
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
step="wasm-check-browser, no image of the world"
PLAYWRIGHT_MODULE=$T/playwright wmake -o wasm wasm-check-browser
grep -q "^page.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: page.spec.mjs did not run"; }
if grep -q "^world.spec.mjs" "$T/make.out" || ! grep -q "^skip world.spec.mjs" "$T/make.out"; then
    cat "$T/make.out"
    fail "$step: world.spec.mjs ran, or no skip was reported"
fi
step="wasm-check-browser, with the image of the world"
echo '{ "world": true }' >"$W/web/manifest.json"
PLAYWRIGHT_MODULE=$T/playwright wmake -o wasm wasm-check-browser
grep -q "^world.spec.mjs $W/web\$" "$T/make.out" || { cat "$T/make.out"; fail "$step: world.spec.mjs did not run"; }
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
"$cmake" -S "$wi" -B "$wb" >"$T/configure.out" 2>&1 || "$cmake" -H"$wi" -B"$wb" >"$T/configure.out" 2>&1 ||
    { cat "$T/configure.out"; fail "$step: configure failed"; }
wbuild
calls "prepare"
grep -q OSWebB "$package" || fail "$step: no OSWebB in $package"
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

echo "05-build-rules: all checks passed"
