#!/bin/sh
# 78-fast-interp - what makes the fast interpreter, and what stops it
#
#   guards   tests/wasm/fast-interp-guards.py: the generated interpreter
#            (GEN) becomes the fast one and compiles, with 64-bit and with
#            32-bit addresses; and with one change of what the fast
#            interpreter repeats or assumes (each constant of checks.inc,
#            each bytecode of handlers.inc, the write barrier, the frames,
#            the primitives, the caches, the code that is rewritten) the
#            build stops, with the message of its guard;
#   narrow   tests/wasm/narrow.test.mjs, the unit tests of
#            packaging/emscripten/tools/wasm-narrow.mjs;
#   links    the links of the build (WASM_DIR/cmake) are what its settings
#            say: the memories of WASM_NODE_MEMORY64 and WASM_WEB_MEMORY64
#            (in the modules too), and the passes and wasm-narrow.mjs of
#            WASM_TUNE=NARROW only for a 32-bit memory;
#   off      with WASM_FAST_INTERP=OFF, WASM_TUNE=OFF and
#            WASM_NODE_MEMORY64=1, a tree configured here (without the
#            libraries) compiles the generated interp.c, unchanged (but
#            for the trampolines of genPrimitiveTable.cmake, which includes
#            it), and nothing of the fast interpreter; the node VM is linked
#            with memory64, and neither VM is tuned.  And a bad value of a
#            setting, a python3 that is none, or an interpreter that the
#            fast one does not fit, stops;
#   python   the python3 that makes the fast interpreter is EMSDK_PYTHON, or
#            the one of the PATH, as it is at each configure, or the one of
#            PHARO_WASM_PYTHON3.
#
# Environment (from make wasm-check): NODE, WASM_DIR, GEN, SRCDIR, TEST_DIR.
# Compiles with emcc (from PATH or EMSDK, or WASM_CC); guards needs python3
# (PYTHON, EMSDK_PYTHON or python3), and off the emcmake and the cmake of
# WASM_DIR/config.make.  Exits 77 without GEN.

SRCDIR=$(cd "${SRCDIR:-$(dirname "$0")/../../..}" && pwd)
TEST_DIR=${TEST_DIR:+$TEST_DIR/78-fast-interp}
# (the defaults of WASM_DIR, TEST_DIR and NODE, and the checks)
. "$SRCDIR/tests/wasm/lib/common.sh"
lane=fast-interp

if test -z "$GEN" || ! test -f "$GEN/vm/src/interp.c"; then
    skip "GEN does not name the generated sources"
fi
GEN=$(cd "$GEN" && pwd)
python=${PYTHON:-${EMSDK_PYTHON:-python3}}
if test -n "$WASM_CC"; then
    cc=$WASM_CC
elif command -v emcc >/dev/null 2>&1; then
    cc="emcc -m64"
elif test -n "$EMSDK" && test -x "$EMSDK/upstream/emscripten/emcc"; then
    cc="$EMSDK/upstream/emscripten/emcc -m64"
else
    cc=
fi
setting() { sed -n "s/^$1:[A-Z]*=//p" "$WASM_DIR/cmake/CMakeCache.txt" 2>/dev/null; }
echo "78-fast-interp: $GEN/vm/src/interp.c, WASM_FAST_INTERP=$(setting WASM_FAST_INTERP) WASM_TUNE=$(setting WASM_TUNE) WASM_NODE_MEMORY64=$(setting WASM_NODE_MEMORY64) WASM_WEB_MEMORY64=$(setting WASM_WEB_MEMORY64)"
cd "$TEST_DIR" || exit 1

# ---- guards
if ! "$python" -I -c 'import sys; sys.exit(sys.version_info < (3, 6))' 2>/dev/null; then
    skip_check guards "no python3 ($python)"
elif test -z "$cc"; then
    skip_check guards "no emcc on PATH or in EMSDK (or set WASM_CC)"
else
    mkdir -p guards/include
    # (the widths of the C types are all that matters of config.h)
    sed -e 's|^#cmakedefine \([A-Za-z0-9_]*\).*|/* #undef \1 */|' -e 's|@SIZEOF_INT@|__SIZEOF_INT__|' \
	-e 's|@SIZEOF_LONG@|__SIZEOF_LONG__|' -e 's|@SIZEOF_LONG_LONG@|__SIZEOF_LONG_LONG__|' \
	-e 's|@SIZEOF_VOID_P@|__SIZEOF_POINTER__|' -e 's|@ALWAYS_INTERACTIVE@|0|' -e 's|@[A-Za-z_]*@|fast-interp|g' \
	"$SRCDIR/include/pharovm/config.h.in" >guards/include/config.h
    i=$SRCDIR/include
    guards() {
	limit 900 "$python" -I "$SRCDIR/tests/wasm/fast-interp-guards.py" "$SRCDIR" "$GEN" "$TEST_DIR/guards/work" \
	    $cc -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -DFEATURE_FFI=0 -DLSB_FIRST=1 -DSOURCE_PATH_SIZE=50 -DUSE_INLINE_MEMORY_ACCESSORS=1 \
	    "-I$TEST_DIR/guards/include" "-I$i" "-I$i/pharovm" "-I$i/pharovm/common" "-I$GEN/vm/include" "-I$i/semaphores" \
	    "-I$i/pharovm/emscripten" "-I$i/pharovm/unix"
    }
    if run guards guards; then
	if test "$status" -ne 0; then
	    fail guards "exit status $status"
	    grep '^FAIL' smoke.out | cut -c 1-300
	else
	    sed 's/^ok  /    /' smoke.out
	    ok guards
	fi
    fi
    rm -rf guards
fi

# ---- narrow
if run narrow limit "$timeout" "$NODE" "$SRCDIR/tests/wasm/narrow.test.mjs"; then
    if test "$status" -ne 0; then fail narrow "exit status $status"; else ok narrow "$(sed 's/^ok *//' smoke.out)"; fi
fi

# ---- links and off
# linked TREE TARGET: the link command of TARGET, and what runs after it
linked() {
    for f in "$1/CMakeFiles/$2.dir/link.txt" "$1/CMakeFiles/$2.dir/build.make"; do
	if test -f "$f"; then cat "$f"; fi
    done
    for f in "$1"/build*.ninja; do
	if test -f "$f"; then
	    awk -v rule="^C(XX)?_EXECUTABLE_LINKER__$2(_[A-Za-z]*)?\$" '
		/^build / { found = 0; for (i = 2; i <= NF; i++) if ($i ~ rule) found = 1 }
		/^build / || /^  / { if (found) print; next }
		{ found = 0 }' "$f"
	fi
    done
}
# links_are TREE NAME TARGET MEMORY64 TUNED: the links of TARGET are those
links_are() {
    linked "$1" "$3" >links.txt
    case $4 in
	2) wanted='-sMEMORY64=2' ;;
	*) wanted='-m64' ;;
    esac
    if ! test -s links.txt; then
	fail "$2" "no link command of $3 in $1"
    elif test "$4" != 2 && grep -q -e '-sMEMORY64=2' links.txt; then
	fail "$2" "$3 is linked with -sMEMORY64=2, and not with memory64"
    elif ! grep -q -e "$wanted" links.txt; then
	fail "$2" "$3 is linked without $wanted"
    elif test "$5" = yes && ! { grep -q -e '--low-memory-unused' links.txt && grep -q 'wasm-narrow\.mjs.* .*\.wasm' links.txt; }; then
	fail "$2" "$3 is linked without the passes of WASM_TUNE=NARROW, or is not narrowed"
    elif test "$5" = no && grep -q -e '--low-memory-unused' -e 'wasm-narrow\.mjs.* .*\.wasm' links.txt; then
	fail "$2" "$3 is tuned"
    else
	return 0
    fi
    return 1
}
# memory_of MODULE: 32 or 64, the index type of its memory
memory_of() {
    "$NODE" -e '
const b = require("fs").readFileSync(process.argv[1]); let p = 8;
const u = () => { let n = 0, k = 1, x; do { x = b[p++]; n += (x & 127) * k; k *= 128; } while (x & 128); return n; };
while (p < b.length) { const id = b[p++], n = u(), q = p; if (id === 5) { u(); console.log(b[p] & 4 ? 64 : 32); } p = q + n; }' "$1"
}
checks=$((checks + 1))
name=links
if ! test -f "$WASM_DIR/cmake/CMakeCache.txt"; then
    fail links "no wasm CMake tree in $WASM_DIR/cmake"
else
    tuned=no
    if test "$(setting WASM_TUNE)" = NARROW && ! setting CMAKE_BUILD_TYPE | grep -q Debug; then tuned=yes; fi
    good=yes
    for t in pharo:WASM_NODE_MEMORY64:node/pharo pharo-web:WASM_WEB_MEMORY64:web/pharo-web; do
	target=${t%%:*} memory=$(setting "$(echo "$t" | cut -d: -f2)") module=$WASM_DIR/${t##*:}.wasm
	if test "$memory" = 2; then links_are "$WASM_DIR/cmake" links $target 2 $tuned || good=no
	else links_are "$WASM_DIR/cmake" links $target 1 no || good=no
	fi
	bits=64
	if test "$memory" = 2; then bits=32; fi
	if test $good = yes && test "$(memory_of "$module")" != $bits; then
	    good=no
	    fail links "$module has not a $bits-bit memory"
	fi
    done
    if test $good = yes; then ok links "node: WASM_NODE_MEMORY64=$(setting WASM_NODE_MEMORY64), web: WASM_WEB_MEMORY64=$(setting WASM_WEB_MEMORY64), tuned: $tuned"; fi
fi

# configure SETTING ...: the CMake tree off, of those settings (made once:
# most of the time goes to the tests of the compiler)
configure() {
    limit 600 $emcmake "$cmake" -S "$SRCDIR" -B off -DWASM_FFI=OFF -DWASM_FREETYPE=OFF -DWASM_WORLD=OFF \
	-DFEATURE_PLUGIN_SIMDJSON=OFF -DFEATURE_PLUGIN_SIMDUTF=OFF -DFEATURE_PLUGIN_TIMSORT=OFF -DFEATURE_PLUGIN_CMARK=OFF \
	"-DWASM_IMAGE_ZIP=$SRCDIR/README.md" "-DGENERATED_SOURCE_DIR=$generated" "-DNODE_JS_EXECUTABLE=$node" "$@" \
	-C "$SRCDIR/cmake/Emscripten.cache.cmake" >configure.out 2>&1 </dev/null
}
recorded() { sed -n "s/^$1 = //p" "$WASM_DIR/config.make" 2>/dev/null; }
emcmake=$(recorded EMCMAKE)
cmake=$(recorded CMAKE)
generated=${GEN%/generated/64}
node=$(command -v "$NODE" || echo "$NODE")
if ! test -x "$emcmake" || ! command -v "$cmake" >/dev/null 2>&1 || test "$generated" = "$GEN"; then
    skip_check off "no emcmake and cmake of $WASM_DIR/config.make, or no generated/64 in GEN"
else
    checks=$((checks + 1))
    name=off
    rm -rf off
    if ! configure -DWASM_FAST_INTERP=OFF -DWASM_TUNE=OFF -DWASM_NODE_MEMORY64=1; then
	fail off "the tree of WASM_FAST_INTERP=OFF is not configured: $(tail -n 5 configure.out | tr '\n' ' ' | cut -c 1-300)"
    elif ! { "$cmake" "-DSOURCE=$GEN/vm/src/interp.c" -DTABLE=vm_exports "-DOUTPUT=$TEST_DIR/off.c" -DMARKERS=ON \
	    -P "$SRCDIR/cmake/emscripten/genPrimitiveTable.cmake" >configure.out 2>&1 && grep -q "^#include \".*/vm/src/interp\.c\"" off.c; }; then
	fail off "genPrimitiveTable.cmake does not include interp.c: $(tail -n 3 configure.out | tr '\n' ' ')"
    elif grep -rqi 'fastinterp' off/CMakeFiles/PharoVMCore.dir off/build*.ninja 2>/dev/null; then
	fail off "the fast interpreter is compiled with WASM_FAST_INTERP=OFF"
    elif ! grep -rqs "DSOURCE=$GEN/vm/src/interp\.c" off/CMakeFiles/PharoVMCore.dir off/build.ninja; then
	fail off "the interpreter compiled with WASM_FAST_INTERP=OFF is not $GEN/vm/src/interp.c"
    elif links_are off off pharo 1 no && links_are off off pharo-web 2 no; then
	ok off
    fi

    # refused NAME TEXT SETTING ...: those settings stop CMake, with TEXT
    refused() {
	name=$1 text=$2; shift 2
	checks=$((checks + 1))
	if configure -DWASM_FAST_INTERP=OFF -DWASM_TUNE=OFF -DWASM_NODE_MEMORY64=1 "$@"; then
	    fail "$name" "CMake does not stop"
	elif ! tr '\n' ' ' <configure.out | tr -s ' ' | grep -q "$text"; then
	    fail "$name" "CMake does not say '$text': $(tail -n 12 configure.out | tr '\n' ' ' | cut -c 1-400)"
	else
	    ok "$name"
	fi
    }
    refused off-tune "WASM_TUNE must be NARROW or OFF, not 'ON'" -DWASM_TUNE=ON
    refused off-memory "WASM_NODE_MEMORY64 must be 1 or 2, not '3'" -DWASM_NODE_MEMORY64=3

    # a python of EMSDK_PYTHON that is gone at the next configure (an emsdk
    # that was updated), then one by its name, then none
    if "$python" -I -c 'import sys; sys.exit(sys.version_info < (3, 6))' 2>/dev/null; then
	checks=$((checks + 1))
	name=python
	real=$(command -v "$python")
	with_fast() { configure -DWASM_FAST_INTERP=ON -DWASM_TUNE=OFF -DWASM_NODE_MEMORY64=1 "$@"; }
	made_by() { grep -rqs "$1 -I .*fastInterpreter\.py" off/CMakeFiles/PharoVMCore.dir off/build.ninja; }
	# (links: emcmake sets EMSDK_PYTHON to what Python says that it is)
	for d in python-1 python-2; do
	    mkdir -p $d && ln -sf "$real" $d/python3
	done
	if ! ( EMSDK_PYTHON=$TEST_DIR/python-1/python3; export EMSDK_PYTHON; with_fast ) || ! made_by "$TEST_DIR/python-1/python3"; then
	    fail python "EMSDK_PYTHON does not make the fast interpreter: $(tail -n 5 configure.out | tr '\n' ' ' | cut -c 1-300)"
	elif rm -r python-1; ! ( EMSDK_PYTHON=$TEST_DIR/python-2/python3; export EMSDK_PYTHON; with_fast ) || ! made_by "$TEST_DIR/python-2/python3"; then
	    fail python "the python3 of the configure before is gone, and EMSDK_PYTHON not taken: $(tail -n 5 configure.out | tr '\n' ' ' | cut -c 1-300)"
	elif rm -r python-2; ! ( EMSDK_PYTHON=; unset EMSDK_PYTHON; PATH=$(dirname "$real"):$PATH; with_fast ) || made_by "$TEST_DIR/python-2/python3"; then
	    fail python "the python3 of the configure before is gone, and the one of the PATH not taken: $(tail -n 5 configure.out | tr '\n' ' ' | cut -c 1-300)"
	elif ! ( PATH=$(dirname "$real"):$PATH; with_fast "-DPHARO_WASM_PYTHON3=$(basename "$real")" ) || ! made_by "$(basename "$real")"; then
	    fail python "PHARO_WASM_PYTHON3 is not taken by its name: $(tail -n 5 configure.out | tr '\n' ' ' | cut -c 1-300)"
	else
	    ok python
	fi
    else
	skip_check python "no python3 ($python)"
    fi
    refused off-python "WASM_FAST_INTERP=ON needs python3 .* /bin/false is none" -DWASM_FAST_INTERP=ON -DPHARO_WASM_PYTHON3=/bin/false
fi
rm -rf off off.c python-1 python-2 links.txt configure.out smoke.out smoke.err
finish 78-fast-interp
