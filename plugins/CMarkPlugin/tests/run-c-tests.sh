#!/bin/sh
# run-c-tests.sh - C-level tests of the CMarkPlugin: its engine (src/engine,
# with cmark-gfm 0.29.0.gfm.14), its glue (src/common/CMarkPlugin.c), and
# upstream's own spec tests against the same objects
#
# The script compiles cmark-gfm's 34 sources itself (never upstream's CMake),
# as cmake/cmark.cmake does: -DNDEBUG -DCMARK_GFM_STATIC_DEFINE
# -DCMARK_NO_SHORT_NAMES, C99, with cmark-config/private (our config.h) and
# cmark-config/public on their include path; the engine, the glue and the
# tests see only cmark-config/public.  Every lane defines -DCM_TESTING (the
# test hooks of cmEngine.h); the VM build never does.  One object directory
# per lane under --build.  The lanes:
#
#   engine         tests/enginetest.c, the engine, cmark; clang -O2 -std=c99
#                  -Wall -Wextra -Wno-unused-parameter -pedantic -Werror, and
#                  -Wshorten-64-to-32 for the engine (the LLP64 guard)
#   engine-gcc     the same with gcc (without -Wshorten-64-to-32)
#   engine-uchar   the same with clang -funsigned-char (AArch64's char)
#   engine-asan    clang -O1 -g -fsanitize=address,undefined
#                  -fno-sanitize-recover=all, ASAN_OPTIONS=detect_leaks=1
#   engine-wasm64  emcc -O2 -m64 -sMEMORY64=1, run under the SDK's node,
#                  twice: with -sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0
#                  and with -sSUPPORT_LONGJMP=emscripten (the VM's WASM_SJLJ
#                  modes); needs --emsdk
#   glue           tests/gluetest.c and tests/fakevm.c with the glue, the
#                  engine and cmark, clang -O1 -g ASan and UBSan, once as the
#                  glue of a shared library and once as a builtin plugin's
#                  (-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN); first the glue
#                  must compile with -Wall -Werror in both modes against the
#                  VM's headers, and cmake/emscripten/genPrimitiveTable.cmake
#                  must read its exports table; needs --vm-build
#   upstream       (only with --only upstream) upstream's src/main.c built
#                  against the lane's objects (clang -O2), and the runs of
#                  upstream's test/CMakeLists.txt of spec_tests.py and
#                  roundtrip_tests.py, with python3 -I on copies of the four
#                  scripts: all pass but example 25 of extensions.txt in its
#                  two runs (the gfm.14 footnote label regression)
#
# and checks these static rules (R), on which every lane depends:
#
#   R1  the glue calls only the proxy functions of the contract
#       (instantiateClassindexableSize, integerObjectOf,
#       storePointerofObjectwithValue, firstIndexableField, methodReturnValue,
#       primitiveFailFor, stackValue, methodArgumentCount, fetchClassOf,
#       isWords, isIntegerObject, integerValueOf, slotSizeOf, byteSizeOf,
#       classArray, classBitmap, classByteArray, classString, nilObject,
#       trueObject, falseObject, and majorVersion and minorVersion in
#       setInterpreter), never floatObjectOf, signed64BitIntegerFor,
#       positive64BitIntegerFor, stringForCString, methodReturnString, clone,
#       pushRemappableOop, nor the C heap
#   R2  cmark's headers (cmark-gfm*.h, table.h, strikethrough.h) are included
#       by src/engine/cmEngine.c only, and cmark-gfm.h by src/engine/cmTrack.c
#   R3  no VM header in src/engine nor include/common
#   R5  the primitives the glue exports, the rows of its builtin table and
#       its *AccessorDepth variables are the same four, all of depth 0
#   R7  src/common holds exactly one .c file, CMarkPlugin.c
#   R8  no cmark_release_plugins, cmark_arena, cmark_parse_document,
#       cmark_parse_file, cmark_markdown_to_html,
#       cmark_get_default_mem_allocator, printf, fprintf, puts, _setjmp nor
#       ` long ' in the plugin's sources (tests excepted)
#   R9  setjmp is called exactly once, in cm_guarded (src/engine/cmTrack.c),
#       and no file of the glue or the engine includes cmark-config/private
#       or a "config.h"
#   V   the --cmark tree is the pin: src/cmark-gfm_version.h.in, the
#       versions 29 and 14 of its CMakeLists.txt, and the SHA256 of the 34
#       sources, the headers, the CLI and the test files in
#       tests/fixtures/cmark-sources.sha256
#
# Options:
#   --cmark DIR    a cmark-gfm 0.29.0.gfm.14 tree; default: the one the
#                  --vm-build tree fetched (deps/cmark-gfm/<top dir>);
#                  required otherwise, except for --only R
#   --emsdk DIR    the Emscripten SDK; default: $EMSDK; without it the lane
#                  engine-wasm64 is skipped
#   --node PATH    node for engine-wasm64; default: the newest node of the SDK
#   --vm-build DIR a configured native VM tree, for the lane glue (its
#                  build/include/pharovm)
#   --gen DIR      the GENERATED_SOURCE_DIR of that tree, for the lane glue;
#                  default: read from DIR/CMakeCache.txt
#   --build DIR    where to build (kept, and reused by the next run);
#                  default: a temporary directory, removed at the end
#   --only LIST    comma-separated lanes (above) and R, e.g. engine,glue,R;
#                  default: R and every lane that can run here but upstream
#                  (glue with --vm-build, engine-wasm64 with an SDK)
#
# Nothing is written outside the build directory.  Exits 1 when a lane or
# a rule fails, 2 on a usage error.

set -e

CMARK_VERSION=0.29.0.gfm.14

SRCDIR=$(cd "$(dirname "$0")/.." && pwd)
cmark= emsdk=${EMSDK:-} node= vmbuild= gen= build= only=
while test $# -gt 0; do
    case $1 in
    --cmark) cmark=$2; shift ;;
    --emsdk) emsdk=$2; shift ;;
    --node) node=$2; shift ;;
    --vm-build) vmbuild=$2; shift ;;
    --gen) gen=$2; shift ;;
    --build) build=$2; shift ;;
    --only) only=$2; shift ;;
    *) echo "run-c-tests: unknown option $1" >&2; exit 2 ;;
    esac
    shift
done

all_lanes="engine engine-gcc engine-uchar engine-asan engine-wasm64 glue upstream"
for name in $(echo "$only" | tr ',' ' '); do
    case " R $all_lanes " in
    *" $name "*) ;;
    *) echo "run-c-tests: unknown lane $name (R $all_lanes)" >&2; exit 2 ;;
    esac
done

selected() {
    test -z "$only" || case ",$only," in *",$1,"*) true ;; *) false ;; esac
}

failed=0

# Prints the lines of the files that are code: comments (/* */ and //) and
# preprocessor lines removed, as FILE:LINE: TEXT
code_lines() {
    awk '{ line = $0
           while (1) {
               if (incomment) { e = index(line, "*/"); if (!e) { line = ""; break }
                                line = substr(line, e + 2); incomment = 0 }
               s = index(line, "/*"); d = index(line, "//")
               if (d && (!s || d < s)) { line = substr(line, 1, d - 1); break }
               if (!s) break
               rest = substr(line, s + 2); line = substr(line, 1, s - 1); incomment = 1
               e = index(rest, "*/"); if (e) { line = line " " substr(rest, e + 2); incomment = 0 } else break
           }
           if (line ~ /^[ \t]*#/) next
           print FILENAME ":" FNR ": " line }' "$@"
}

# Strips the string literals of code_lines' output
no_strings() {
    sed 's/"\([^"\\]\|\\.\)*"/""/g'
}

glue_allowed="instantiateClassindexableSize integerObjectOf storePointerofObjectwithValue firstIndexableField
 methodReturnValue primitiveFailFor stackValue methodArgumentCount fetchClassOf isWords isIntegerObject
 integerValueOf slotSizeOf byteSizeOf classArray classBitmap classByteArray classString nilObject trueObject
 falseObject majorVersion minorVersion"

rules() {
    glue=$SRCDIR/src/common/CMarkPlugin.c
    engines=$(ls "$SRCDIR"/src/engine/*.c)
    sources="$glue $engines $(ls "$SRCDIR"/src/engine/*.h "$SRCDIR"/include/common/*.h)"
    bad=0
    # R1: the proxy functions of the contract only, no C heap
    used=$(code_lines "$glue" | no_strings | grep -o 'interpreterProxy->[A-Za-z0-9_]*' | sed 's/.*->//' | sort -u)
    for f in $used; do
        case " $(echo $glue_allowed) " in
        *" $f "*) ;;
        *) echo "R1: the glue calls interpreterProxy->$f"; bad=1 ;;
        esac
    done
    if code_lines "$glue" | no_strings | grep -E '(^|[^A-Za-z0-9_])(floatObjectOf|signed64BitIntegerFor|positive64BitIntegerFor|stringForCString|methodReturnString|clone|pushRemappableOop|malloc|calloc|realloc|free)[ \t]*\('; then
        echo "R1: the glue calls a forbidden function"
        bad=1
    fi
    # R2: cmark's headers in cmEngine.c only (cmark-gfm.h in cmTrack.c too)
    for f in $sources "$SRCDIR"/tests/*.c "$SRCDIR"/tests/*.h; do
        hits=$(grep -n '#include *[<"]\(cmark-gfm[A-Za-z0-9_-]*\|table\|strikethrough\)\.h[>"]' "$f" || true)
        case $(basename "$f") in
        cmEngine.c) continue ;;
        cmTrack.c) hits=$(echo "$hits" | grep -v '#include "cmark-gfm\.h"' || true) ;;
        esac
        if test -n "$hits"; then
            echo "R2: $f includes a cmark header: $hits"
            bad=1
        fi
    done
    if ! grep -q '#include "cmark-gfm-core-extensions\.h"' "$SRCDIR/src/engine/cmEngine.c"; then
        echo "R2: cmEngine.c does not include the cmark headers"
        bad=1
    fi
    # R3: no VM header in the engine
    for f in $engines "$SRCDIR"/src/engine/*.h "$SRCDIR"/include/common/*.h; do
        if grep -n '#include *[<"]\(sq[A-Za-z]*\|virtualMachine\|interp\|pharovm/[A-Za-z]*\|vmCallback\|cointerp\|pharo\)\.h[>"]' "$f"; then
            echo "R3: $f includes a VM header"
            bad=1
        fi
    done
    # R5: the EXPORT(sqInt) primitives (declared and defined), the rows of
    # the builtin table (name and function alike, with the depth) and the
    # *AccessorDepth variables (with the depth) are the same set
    want="primitiveInfo:0 primitiveParse:0 primitiveRelease:0 primitiveRender:0"
    want=$(echo $want | tr ' ' '\n' | sort)
    names=$(echo "$want" | sed 's/:.*//')
    declared=$(sed -n 's/^EXPORT(sqInt) \(primitive[A-Za-z0-9]*\)(void);$/\1/p' "$glue" | sort)
    defined=$(awk '/^EXPORT\(sqInt\)$/ { getline; if (match($0, /^primitive[A-Za-z0-9]*\(void\)$/)) { sub(/\(.*/, ""); print } }' "$glue" | sort)
    rows=$(sed -n 's/^	{(void\*)_m, "\(primitive[A-Za-z0-9]*\)\\000\\00\([0-7]\)", (void\*)\(primitive[A-Za-z0-9]*\)},$/\1:\2 \3/p' "$glue" \
        | awk '{ split($1, p, ":") } p[1] == $2 { print $1 } p[1] != $2 { print "mismatch:" $1 "/" $2 }' | sort)
    depths=$(sed -n 's/^EXPORT(signed char) \(primitive[A-Za-z0-9]*\)AccessorDepth = \([0-9]\);$/\1:\2/p' "$glue" | sort)
    for set in declared defined; do
        eval "value=\$$set"
        if test "$value" != "$names"; then
            echo "R5: the $set primitives are:" $value
            bad=1
        fi
    done
    for set in rows depths; do
        eval "value=\$$set"
        if test "$value" != "$want"; then
            echo "R5: the $set are:" $value
            bad=1
        fi
    done
    # R7: add_vm_plugin compiles every .c of src/common
    commons=$(cd "$SRCDIR/src/common" && ls *.c 2>/dev/null) || true
    if test "$commons" != CMarkPlugin.c; then
        echo "R7: src/common holds:" $commons
        bad=1
    fi
    # R8: no cmark entry point that must not be used, no output, no long
    if code_lines $sources | no_strings | grep -E '(cmark_release_plugins|cmark_arena|cmark_parse_document|cmark_parse_file|cmark_markdown_to_html|cmark_get_default_mem_allocator|_setjmp|(^|[^A-Za-z0-9_])(printf|fprintf|puts)[ \t]*\()'; then
        echo "R8: a forbidden call"
        bad=1
    fi
    if code_lines $sources | no_strings | grep -E '(^|[^A-Za-z0-9_])long([^A-Za-z0-9_]|$)'; then
        echo "R8: long (32-bit on Windows): use the sized types"
        bad=1
    fi
    # R9: one setjmp, in cm_guarded; no private config.h
    calls=$(code_lines $sources | no_strings | grep -E '(^|[^A-Za-z0-9_])setjmp[ \t]*\(' || true)
    where=$(code_lines "$SRCDIR/src/engine/cmTrack.c" | awk '
        { sub(/^[^:]*:[0-9]+: /, "") }
        /^[A-Za-z_][A-Za-z0-9_]*\(/ && !/;[ \t]*$/ { fn = $0; sub(/\(.*/, "", fn) }
        /^}/ { fn = "" }
        /(^|[^A-Za-z0-9_])setjmp[ \t]*\(/ { print fn }')
    if test "$(echo "$calls" | grep -c .)" != 1 || test "$where" != cm_guarded; then
        echo "R9: setjmp is called in:" $calls
        bad=1
    fi
    if grep -n 'cmark-config/private\|#include *"config\.h"' $sources; then
        echo "R9: the glue or the engine includes cmark's config.h"
        bad=1
    fi
    # V: the pin
    if test -n "$cmark"; then
        if ! test -f "$cmark/src/cmark-gfm_version.h.in" \
            || ! grep -q '^set(PROJECT_VERSION_MINOR 29)$' "$cmark/CMakeLists.txt" \
            || ! grep -q '^set(PROJECT_VERSION_GFM 14)$' "$cmark/CMakeLists.txt"; then
            echo "V: $cmark is not cmark-gfm $CMARK_VERSION"
            bad=1
        elif ! (cd "$cmark" && sha256sum -c --quiet "$SRCDIR/tests/fixtures/cmark-sources.sha256"); then
            echo "V: $cmark differs from the pinned sources"
            bad=1
        fi
    fi
    if test $bad = 0; then
        echo "R1 R2 R3 R5 R7 R8 R9${cmark:+ V} ok"
    else
        echo "R FAILED"
        failed=1
    fi
}

# the cmark-gfm tree that the VM tree fetched, when --cmark is not given
if test -z "$cmark" && test -n "$vmbuild"; then
    for d in "$vmbuild"/deps/cmark-gfm/*/; do
        if test -f "$d/src/cmark-gfm_version.h.in"; then
            cmark=$d
        fi
    done
fi
if test -n "$cmark"; then
    cmark=$(cd "$cmark" && pwd)
    if ! test -f "$cmark/src/blocks.c" || ! test -f "$cmark/extensions/table.c"; then
        echo "run-c-tests: $cmark is not a cmark-gfm source tree (no src/blocks.c)" >&2
        exit 2
    fi
fi
rules_failed=0
if selected R; then
    rules
    rules_failed=$failed
fi

# the lanes to run
cc=${CC:-clang}
if test -n "$emsdk" && test -z "$node"; then
    node=$(ls -d "$emsdk"/node/*/bin/node 2>/dev/null | sort -V | tail -n 1)
fi
lanes=
for lane in $all_lanes; do
    if test -n "$only"; then
        selected $lane || continue
    else
        case $lane in
        engine-wasm64) test -n "$emsdk" || { echo "== engine-wasm64 skipped: no --emsdk"; continue; } ;;
        glue) test -n "$vmbuild" || { echo "== glue skipped: no --vm-build"; continue; } ;;
        upstream) continue ;;
        esac
    fi
    lanes="$lanes $lane"
done
if test -z "$lanes"; then
    test $failed = 0 && echo "run-c-tests: all lanes passed" || echo "run-c-tests: FAILED"
    exit $failed
fi
if test $rules_failed != 0; then
    echo "run-c-tests: FAILED (the rules)"
    exit 1
fi
if test -z "$cmark"; then
    echo "run-c-tests: --cmark DIR is required (a cmark-gfm $CMARK_VERSION tree)" >&2
    exit 2
fi
if test -n "$build"; then
    mkdir -p "$build"
    build=$(cd "$build" && pwd)
else
    build=$(mktemp -d "${TMPDIR:-/tmp}/cmtest.XXXXXX")
    trap 'rm -rf "$build"' EXIT
fi
em=
if test -n "$emsdk"; then
    em=$emsdk/upstream/emscripten
fi

cmark_sources=
for f in cmark node iterator blocks inlines scanners utf8 buffer references footnotes map render man xml html \
         commonmark plaintext latex houdini_href_e houdini_html_e houdini_html_u cmark_ctype arena linked_list \
         syntax_extension registry plugin; do
    cmark_sources="$cmark_sources $cmark/src/$f.c"
done
for f in core-extensions table strikethrough autolink tagfilter ext_scanners tasklist; do
    cmark_sources="$cmark_sources $cmark/extensions/$f.c"
done
engine_sources="$SRCDIR/src/engine/cmEngine.c $SRCDIR/src/engine/cmText.c $SRCDIR/src/engine/cmTrack.c"
std="-std=c99"
strict="-Wall -Wextra -Wno-unused-parameter -pedantic -Werror"
cmdefs="-DNDEBUG -DCMARK_GFM_STATIC_DEFINE -DCMARK_NO_SHORT_NAMES"
cminc="-I$SRCDIR/cmark-config/private -I$SRCDIR/cmark-config/public -I$cmark/src -I$cmark/extensions"
# the engine sees cmark's public headers, never cmark-config/private
enginc="-I$SRCDIR/include/common -I$SRCDIR/cmark-config/public -I$cmark/src -I$cmark/extensions"
engdefs="-DCM_TESTING -DCMARK_GFM_STATIC_DEFINE -DCMARK_NO_SHORT_NAMES"
asan="-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer"

# compile DIR CC "FLAGS" SOURCE...: each source into DIR/<name>.o, in
# parallel; the objects in $objects
compile() {
    cdir=$1 ccompiler=$2 cflags=$3
    shift 3
    objects=
    pids=
    mkdir -p "$cdir"
    for source in "$@"; do
        object=$cdir/$(basename "$source" .c).o
        $ccompiler $cflags -c "$source" -o "$object" &
        pids="$pids $!"
        objects="$objects $object"
    done
    status=0
    for pid in $pids; do
        wait $pid || status=1
    done
    return $status
}

# lane_build NAME CC "OPT" "STRICT" "ENGINE EXTRA" "LINK" OUTPUT: cmark with
# OPT and STRICT, the engine with OPT, STRICT and ENGINE EXTRA,
# enginetest.c with OPT and STRICT, linked into $build/NAME/OUTPUT
lane_build() {
    name=$1 ccc=$2 opt=$3 strictness=$4 engextra=$5 link=$6 output=$7
    dir=$build/$name
    compile "$dir/cmark" "$ccc" "$std $opt $strictness $cmdefs $cminc" $cmark_sources || return 1
    all=$objects
    compile "$dir/engine" "$ccc" "$std $opt $strictness $engextra $engdefs $enginc" $engine_sources || return 1
    all="$all $objects"
    compile "$dir" "$ccc" "$std $opt $strictness $engdefs -I$SRCDIR/include/common" "$SRCDIR/tests/enginetest.c" || return 1
    $ccc $opt $link -o "$dir/$output" $all $objects
}

# lane_run NAME COMMAND...: runs a lane's test
lane_run() {
    name=$1
    shift
    echo "== $name"
    if "$@"; then
        echo "== $name: ok"
    else
        echo "== $name: FAILED"
        failed=1
    fi
}

# lane NAME [SJLJ MODE]: builds and runs it
lane() {
    name=$1
    case $1 in
    engine) set -- lane_build engine "$cc" -O2 "$strict" -Wshorten-64-to-32 "" enginetest ;;
    engine-gcc) set -- lane_build engine-gcc gcc -O2 "$strict" "" "" enginetest ;;
    engine-uchar) set -- lane_build engine-uchar "$cc" "-O2 -funsigned-char" "$strict" -Wshorten-64-to-32 "" enginetest ;;
    engine-asan) set -- lane_build engine-asan "$cc" "-O1 -g $asan" "$strict" "" "" enginetest ;;
    engine-wasm64)
        sjlj=$2
        case $sjlj in
        wasm) sjflags="-sSUPPORT_LONGJMP=wasm -sWASM_LEGACY_EXCEPTIONS=0" ;;
        *) sjflags="-sSUPPORT_LONGJMP=emscripten" ;;
        esac
        name=engine-wasm64-$sjlj
        set -- lane_build $name "$em/emcc" "-O2 -m64 -sMEMORY64=1 $sjflags" "$strict" "" \
            "-sNODERAWFS=1 -sALLOW_MEMORY_GROWTH=1 -sMAXIMUM_MEMORY=16GB -sENVIRONMENT=node" enginetest.js ;;
    esac
    log=$build/$name.log
    if ! "$@" > "$log" 2>&1; then
        cat "$log"
        echo "== $name: FAILED (build)"
        failed=1
        return 0
    fi
    case $name in
    engine-wasm64-*)
        if ! test -x "$node"; then
            echo "== $name: FAILED: no node ($node)"
            failed=1
            return 0
        fi
        lane_run "$name" "$node" "$build/$name/enginetest.js" "$SRCDIR/tests/fixtures" ;;
    *) lane_run "$name" "$build/$name/enginetest" "$SRCDIR/tests/fixtures" ;;
    esac
}

# The glue against a fake VM.  The headers of the VM come from the tree of
# --vm-build: its config.h (build/include/pharovm), the generated interp.h
# of its GENERATED_SOURCE_DIR (--gen, else read from its CMakeCache.txt),
# and the include directories of the repository that the VM library
# exports.
glue_lane() {
    echo "== glue"
    if test -z "$vmbuild" || ! test -f "$vmbuild/CMakeCache.txt"; then
        echo "== glue: FAILED: --vm-build DIR, a configured VM tree, is required"
        return 1
    fi
    vmbuild=$(cd "$vmbuild" && pwd)
    if test -z "$gen"; then
        gen=$(sed -n 's/^GENERATED_SOURCE_DIR:[A-Z]*=//p' "$vmbuild/CMakeCache.txt")
        test -n "$gen" || gen=$vmbuild
    fi
    repo=$(cd "$SRCDIR/../.." && pwd)
    case $(uname -s) in
    Darwin) platform=osx ;;
    *) platform=unix ;;
    esac
    vminc="-I$vmbuild/build/include/pharovm -I$gen/generated/64/vm/include
 -I$repo/include -I$repo/include/pharovm -I$repo/include/pharovm/common -I$repo/include/pharovm/$platform"
    for f in "$vmbuild/build/include/pharovm/config.h" "$gen/generated/64/vm/include/interp.h"; do
        if ! test -f "$f"; then
            echo "== glue: FAILED: no $f (--vm-build, --gen)"
            return 1
        fi
    done
    # the definitions every VM source gets (CMakeLists.txt)
    vmdefs="-DLSB_FIRST=1 -DIMMUTABILITY=1 -DCOGMTVM=0 -DDEBUGVM=0 -DASYNC_FFI_QUEUE=1 -D_FILE_OFFSET_BITS=64"
    glueinc="-I$SRCDIR/include/common"
    dir=$build/glue
    mkdir -p "$dir"
    ok=1
    for mode in "" -DSQUEAK_BUILTIN_PLUGIN; do
        if $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs $mode $glueinc $vminc "$SRCDIR/src/common/CMarkPlugin.c"; then
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: ok"
        else
            echo "-- $cc -fsyntax-only -Wall -Werror ${mode:-(shared library)}: FAILED"
            ok=0
        fi
    done
    if command -v cmake > /dev/null; then
        if cmake -DSOURCE="$SRCDIR/src/common/CMarkPlugin.c" -DTABLE=CMarkPlugin_exports \
                -DOUTPUT="$dir/CMarkPlugin_primitives.c" -P "$repo/cmake/emscripten/genPrimitiveTable.cmake" \
            && $cc -std=gnu11 -fsyntax-only -Wall -Werror $vmdefs -DSQUEAK_BUILTIN_PLUGIN $glueinc $vminc \
                -I"$SRCDIR/src/common" "$dir/CMarkPlugin_primitives.c"; then
            echo "-- genPrimitiveTable.cmake: $(grep -c '^static void w_' "$dir/CMarkPlugin_primitives.c") trampolines, ok"
        else
            echo "-- genPrimitiveTable.cmake: FAILED"
            ok=0
        fi
    else
        echo "-- genPrimitiveTable.cmake: FAILED: no cmake"
        ok=0
    fi
    flags="-O1 -g $asan"
    for mode in shared builtin; do
        modedefs=
        test $mode = builtin && modedefs="-DSQUEAK_BUILTIN_PLUGIN -DGLUE_BUILTIN"
        out=$dir/$mode
        mkdir -p "$out"
        if ! { compile "$out/cmark" "$cc" "$std $flags $cmdefs $cminc" $cmark_sources && all=$objects &&
            compile "$out/engine" "$cc" "$std $flags $strict $engdefs $enginc" $engine_sources && all="$all $objects" &&
            compile "$out" "$cc" "-std=gnu11 $flags $vmdefs $vminc $modedefs $engdefs $glueinc -I$SRCDIR/tests" \
                "$SRCDIR/src/common/CMarkPlugin.c" "$SRCDIR/tests/fakevm.c" "$SRCDIR/tests/gluetest.c" &&
            $cc $asan -o "$out/gluetest" $all $objects; } > "$out/build.log" 2>&1; then
            cat "$out/build.log"
            echo "-- gluetest ($mode): FAILED (build)"
            ok=0
            continue
        fi
        echo "-- the glue as a $mode plugin:"
        if ! "$out/gluetest"; then
            ok=0
        fi
    done
    if test $ok = 1; then
        echo "== glue: ok"
        return 0
    fi
    echo "== glue: FAILED"
    return 1
}

# Upstream's spec tests (test/CMakeLists.txt) against a CLI built from our
# objects.  The archive stays untouched: its four scripts are copied once
# into the build directory, and run with python3 -I (isolated: no
# environment, no user site, and -P, so that their own imports find each
# other only through the sys.path entry given here).
upstream_lane() {
    echo "== upstream"
    dir=$build/upstream
    py=$dir/py
    mkdir -p "$py"
    if ! { compile "$dir/cmark" "$cc" "$std -O2 $strict $cmdefs $cminc" $cmark_sources && all=$objects &&
        compile "$dir" "$cc" "$std -O2 $cmdefs $cminc" "$cmark/src/main.c" &&
        $cc -O2 -o "$dir/cmark-gfm" $all $objects; } > "$dir/build.log" 2>&1; then
        cat "$dir/build.log"
        echo "== upstream: FAILED (build)"
        return 1
    fi
    for f in spec_tests.py roundtrip_tests.py cmark.py normalize.py; do
        cp "$cmark/test/$f" "$py/$f"
    done
    cli=$dir/cmark-gfm
    exts="table strikethrough autolink tagfilter footnotes tasklist"
    ok=1
    # run NAME SCRIPT EXPECTED-FAILURES ARGS...: EXPECTED-FAILURES is the
    # list of the example numbers that may fail, "-" for none
    run() {
        rname=$1 script=$2 expected=$3
        shift 3
        log=$dir/$rname.log
        python3 -I -c 'import runpy, sys
d = sys.argv[1]; script = sys.argv[2]
sys.path.insert(0, d)
sys.argv = [d + "/" + script] + sys.argv[3:]
runpy.run_path(d + "/" + script, run_name="__main__")' "$py" "$script" "$@" > "$log" 2>&1 || true
        summary=$(grep -E '^[0-9]+ passed, [0-9]+ failed, [0-9]+ errored, [0-9]+ skipped$' "$log" | tail -n 1)
        failures=$(sed -n 's/^Example \([0-9]*\) (lines .*/\1/p' "$log" | sort -n | tr '\n' ' ' | sed 's/ $//')
        test -n "$failures" || failures=-
        if test -n "$summary" && test "$failures" = "$expected" \
            && echo "$summary" | grep -q ', 0 errored, '; then
            note=
            test "$failures" = - || note=" (example $failures fails, as expected)"
            echo "-- $rname: $summary$note"
        else
            echo "-- $rname: FAILED: ${summary:-no summary}; failing examples: $failures (expected $expected); see $log"
            ok=0
        fi
    }
    run spec spec_tests.py - --no-normalize --spec "$cmark/test/spec.txt" --program "$cli"
    run smart_punct spec_tests.py - --no-normalize --spec "$cmark/test/smart_punct.txt" --program "$cli --smart"
    run extensions spec_tests.py 25 --no-normalize --spec "$cmark/test/extensions.txt" --program "$cli" \
        --extensions "$exts"
    run roundtrip_extensions roundtrip_tests.py 25 --spec "$cmark/test/extensions.txt" --program "$cli" \
        --extensions "$exts"
    run table_prefer_style_attributes roundtrip_tests.py - \
        --spec "$cmark/test/extensions-table-prefer-style-attributes.txt" \
        --program "$cli --table-prefer-style-attributes" --extensions "$exts"
    run full_info_string roundtrip_tests.py - --spec "$cmark/test/extensions-full-info-string.txt" \
        --program "$cli --full-info-string"
    run regression spec_tests.py - --no-normalize --spec "$cmark/test/regression.txt" --program "$cli"
    if test $ok = 1; then
        echo "== upstream: ok"
        return 0
    fi
    echo "== upstream: FAILED"
    return 1
}

export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=1} UBSAN_OPTIONS=${UBSAN_OPTIONS:-print_stacktrace=1}
for name in $lanes; do
    case $name in
    glue) glue_lane || failed=1 ;;
    upstream) upstream_lane || failed=1 ;;
    engine-wasm64)
        if test -z "$em" || ! test -x "$em/emcc"; then
            echo "== engine-wasm64: FAILED: no emcc (--emsdk)"
            failed=1
        else
            lane engine-wasm64 wasm
            lane engine-wasm64 emscripten
        fi ;;
    *) lane $name ;;
    esac
done

if test $failed = 0; then
    echo "run-c-tests: all lanes passed"
else
    echo "run-c-tests: FAILED"
fi
exit $failed
